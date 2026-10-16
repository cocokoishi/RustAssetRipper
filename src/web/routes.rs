//! Request dispatch for the product web surface.
//!
//! The routes follow `WebApplicationLauncher`'s registrations. Every page, command and probe
//! that is wired answers exactly like the frozen product; a route that is registered in the
//! baseline but not yet wired reports `501 Not Implemented` with a page naming the route, and a
//! path the baseline never registers reports `404`. A known path reached with the wrong method
//! reports `405` with the `Allow` header, which is what routing does in the baseline.
//!
//! Two deliberate substitutions are recorded in `docs/feature-parity.md`:
//!
//! * The baseline asks for deletion consent through the native Windows dialog. The port has no
//!   dialog backend, so the export command answers with a confirmation page whose form repeats
//!   the command with `ConfirmDelete=true`. That form field is the port's dialog answer; a
//!   command without it declines, exactly as the baseline declines when no dialog backend is
//!   present.
//! * The baseline converts a thrown exception into a redirect to `/` after logging it. The port
//!   keeps that behaviour: a failed load or export logs its full diagnostic and redirects.

use std::path::Path;

use assetripper::export::export_root::is_non_empty_directory;
use assetripper::product::RouteMethod;

use super::http::{Request, Response};
use super::pages;
use super::server::utc_timestamp_text;
use super::settings_form::apply_form;
use super::state::{ExportDecision, WebState};

/// The methods every known path answers, used by `405` responses.
const ALLOWED_METHODS: &str = "GET, POST";

/// Route one request.
pub fn dispatch(state: &mut WebState, request: &Request) -> Response {
    match request.method() {
        "GET" => get(state, request),
        "POST" => post(state, request),
        _ => Response::method_not_allowed(ALLOWED_METHODS),
    }
}

fn get(state: &mut WebState, request: &Request) -> Response {
    let path = request.path();
    if let Some(body) = state.static_content(path) {
        return Response::bytes(content_type_for(path), body.to_vec());
    }
    match path {
        "/" => Response::html(pages::index(state)),
        "/Commands" => Response::html(pages::commands(state)),
        "/Privacy" => Response::html(pages::privacy(state)),
        "/Licenses" => Response::html(pages::licenses(state)),
        "/PremiumFeatures" => Response::html(pages::premium_features(state)),
        "/Settings/Edit" => Response::html(pages::settings_edit(state)),
        "/openapi.json" => Response::json(pages::openapi_document()),
        "/swagger" => Response::html(pages::swagger_page(state)),
        "/css/site.css" => Response::bytes("text/css", pages::SITE_CSS.as_bytes().to_vec()),
        // The baseline answers a dialog request from a native dialog backend. The port ships no
        // such backend, so it returns the "no selection" values the dialogs return on a platform
        // without one.
        "/Dialogs/SaveFile" | "/Dialogs/OpenFile" | "/Dialogs/OpenFolder" => Response::json("\"\""),
        "/Dialogs/OpenFiles" | "/Dialogs/OpenFolders" => Response::json("[]"),
        "/IO/File/Exists" => probe(request, |path| Path::new(path).is_file()),
        "/IO/Directory/Exists" => probe(request, |path| Path::new(path).is_dir()),
        "/IO/Directory/Empty" => probe(request, |path| !is_non_empty_directory(Path::new(path))),
        _ => fallback(state, "GET", path),
    }
}

fn post(state: &mut WebState, request: &Request) -> Response {
    match request.path() {
        "/Localization" => localization(state, request),
        "/Settings/Update" => settings_update(state, request),
        "/LoadFile" | "/LoadFolder" => load(state, request),
        "/Reset" => {
            state.reset();
            Response::redirect("/")
        }
        "/Export/UnityProject" => export(state, request),
        _ => fallback(state, "POST", request.path()),
    }
}

/// `POST /Localization`: switch language, store the code and save when the settings ask for it.
fn localization(state: &mut WebState, request: &Request) -> Response {
    if let Some(code) = request.query_value("code") {
        state.set_language(Some(code));
        let current = state.language().to_owned();
        state.settings_mut().export.language_code = Some(current);
        if let Err(error) = state.maybe_save_settings() {
            state.log_error(&error);
        }
    }
    Response::redirect("/")
}

/// `POST /Settings/Update`: apply the submitted form, then store or delete the settings file.
fn settings_update(state: &mut WebState, request: &Request) -> Response {
    let form = super::http::parse_form(request.body());
    apply_form(state.settings_mut(), &form);
    if let Err(error) = state.apply_settings_persistence() {
        state.log_error(&error);
    }
    Response::redirect("/Settings/Edit")
}

/// `POST /LoadFile` and `POST /LoadFolder`: mount and decode every submitted path.
///
/// Without a `Path` field the baseline asks a native dialog and falls back to `/Commands` when
/// none is available; the port has no dialog backend, so that fallback always applies.
fn load(state: &mut WebState, request: &Request) -> Response {
    let Some(path) = request.form_value("Path") else {
        return Response::redirect("/Commands");
    };
    if path.is_empty() {
        return Response::redirect("/");
    }
    if let Err(error) = state.load(&[path]) {
        state.log_error(&error);
    }
    Response::redirect("/")
}

/// `POST /Export/UnityProject`: apply the export gate, then export or ask for confirmation.
fn export(state: &mut WebState, request: &Request) -> Response {
    let Some(submitted) = request.form_value("Path") else {
        return Response::redirect("/Commands");
    };
    if submitted.is_empty() {
        return Response::redirect("/");
    }
    let create_subfolder = request.form_flag("CreateSubfolder");
    let confirmed = request.form_flag("ConfirmDelete");
    let path = state.resolve_export_path(
        &submitted,
        create_subfolder,
        &utc_timestamp_text(std::time::SystemTime::now()),
    );
    match state.plan_export(&path, confirmed) {
        ExportDecision::Skip => Response::redirect("/"),
        ExportDecision::InvalidDirectory => {
            state.log_error(&format!(
                "Export path '{path}' is not a valid export directory."
            ));
            Response::redirect("/")
        }
        ExportDecision::NeedsConfirmation => {
            let label = state
                .messages()
                .text(state.language(), "export_unity_project")
                .into_owned();
            Response::html(pages::confirm_delete(
                state,
                "/Export/UnityProject",
                &label,
                &path,
                create_subfolder,
            ))
        }
        ExportDecision::Proceed { delete_existing } => {
            if let Err(error) = state.export_unity_project(&path, delete_existing) {
                state.log_error(&error);
            }
            Response::redirect("/")
        }
    }
}

/// A `Path` query probe: `true`/`false` JSON when `Path` is present, otherwise `400`.
fn probe(request: &Request, test: impl FnOnce(&str) -> bool) -> Response {
    match request.query_value("Path") {
        Some(path) => Response::json(if test(path) { "true" } else { "false" }),
        None => Response::bad_request(""),
    }
}

/// The answer for a path the handlers above do not claim.
fn fallback(state: &mut WebState, method: &str, path: &str) -> Response {
    let known_path = assetripper::product::WEB_ROUTES
        .iter()
        .any(|route| route.path == path);
    if !known_path {
        return Response::not_found();
    }
    let known_method = assetripper::product::WEB_ROUTES
        .iter()
        .any(|route| route.path == path && method_name(route.method) == method);
    if !known_method {
        return Response::method_not_allowed(ALLOWED_METHODS);
    }
    Response::html(pages::pending_page(state, method, path))
        .with_status(501)
        .with_content_type("text/html; charset=utf-8")
}

fn method_name(method: RouteMethod) -> &'static str {
    match method {
        RouteMethod::Get => "GET",
        RouteMethod::Post => "POST",
    }
}

/// The content type the baseline associates with a static content path.
fn content_type_for(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("css") => "text/css",
        Some("js") => "text/javascript",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::dispatch;
    use crate::web::http::Request;
    use crate::web::localization::Messages;
    use crate::web::state::WebState;
    use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
    use assetripper::export::settings_file::SerializedSettingsFile;
    use std::path::PathBuf;

    fn state() -> WebState {
        let settings = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        WebState::with_settings(
            Messages::load().expect("tables load"),
            settings,
            PathBuf::from("/tmp/assetripper-routes-test"),
            true,
        )
        .expect("state builds")
    }

    fn request(method: &str, target: &str) -> Request {
        let mut text = format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n");
        text.push_str("Content-Length: 0\r\n\r\n");
        Request::read(&mut std::io::Cursor::new(text.into_bytes()))
            .expect("the probe request reads")
            .expect("the probe request is complete")
    }

    fn status(state: &mut WebState, method: &str, target: &str) -> u16 {
        dispatch(state, &request(method, target)).status()
    }

    #[test]
    fn the_static_pages_and_settings_page_are_served() {
        let mut state = state();
        for path in [
            "/",
            "/Commands",
            "/Privacy",
            "/Licenses",
            "/PremiumFeatures",
            "/Settings/Edit",
            "/swagger",
            "/css/site.css",
        ] {
            assert_eq!(status(&mut state, "GET", path), 200, "{path}");
        }
        assert_eq!(status(&mut state, "GET", "/openapi.json"), 200);
    }

    #[test]
    fn the_dialog_and_io_probes_answer_like_the_baseline() {
        let mut state = state();
        assert_eq!(status(&mut state, "GET", "/Dialogs/OpenFile"), 200);
        assert_eq!(status(&mut state, "GET", "/Dialogs/OpenFiles"), 200);
        // A missing Path is a bad request; an empty Path is a probe of the empty name.
        assert_eq!(status(&mut state, "GET", "/IO/File/Exists"), 400);
        assert_eq!(status(&mut state, "GET", "/IO/File/Exists?Path="), 200);
        assert_eq!(status(&mut state, "GET", "/IO/Directory/Empty?Path="), 200);
    }

    #[test]
    fn the_export_command_needs_a_path_and_a_confirmation() {
        let mut state = state();
        // Without a Path field the command falls back to the commands page.
        let response = dispatch(&mut state, &request("POST", "/Export/UnityProject"));
        assert_eq!(response.status(), 302);
        assert_eq!(response.header("Location"), Some("/Commands"));
        // An empty Path is a no-op redirect to the home page.
        let response = dispatch(
            &mut state,
            &request_with_body("/Export/UnityProject", "Path="),
        );
        assert_eq!(response.header("Location"), Some("/"));
    }

    #[test]
    fn an_unwired_baseline_route_reports_not_implemented_and_an_unknown_one_404() {
        let mut state = state();
        assert_eq!(status(&mut state, "GET", "/Bundles/View"), 501);
        assert_eq!(status(&mut state, "GET", "/Assets/View"), 501);
        assert_eq!(status(&mut state, "POST", "/Export/PrimaryContent"), 501);
        assert_eq!(status(&mut state, "GET", "/nope"), 404);
        // The same path with the other method is a method error, not a missing route.
        assert_eq!(status(&mut state, "GET", "/Reset"), 405);
        assert_eq!(status(&mut state, "POST", "/Privacy"), 405);
    }

    fn request_with_body(target: &str, body: &str) -> Request {
        let text = format!(
            "POST {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        Request::read(&mut std::io::Cursor::new(text.into_bytes()))
            .expect("the probe request reads")
            .expect("the probe request is complete")
    }
}
