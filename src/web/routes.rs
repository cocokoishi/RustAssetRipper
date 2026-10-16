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
use assetripper::web_paths::{
    view_gate, AssetPath, BundlePath, CollectionPath, FailedFilePath, PathDocument, ResourcePath,
    ScenePath, ViewGate,
};

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
        "/Bundles/View" => bundles_view(state, request),
        "/Collections/View" => collections_view(state, request),
        "/Collections/Count" => collections_count(state, request),
        "/Assets/View" => pending_tree_view::<AssetPath>(
            state,
            request,
            "/Assets/View",
            |tree, path| tree.asset_exists(path),
            |path| format!("Asset could not be resolved: {}", path.to_diagnostic()),
        ),
        "/Resources/View" => pending_tree_view::<ResourcePath>(
            state,
            request,
            "/Resources/View",
            |tree, path| tree.resolve_resource(path).is_some(),
            |path| {
                format!(
                    "Resource file could not be resolved: {}",
                    path.to_diagnostic()
                )
            },
        ),
        "/FailedFiles/View" => pending_tree_view::<FailedFilePath>(
            state,
            request,
            "/FailedFiles/View",
            |tree, path| tree.resolve_failed(path).is_some(),
            // The frozen `FailedFileAPI` reuses the resource message verbatim.
            |path| {
                format!(
                    "Resource file could not be resolved: {}",
                    path.to_diagnostic()
                )
            },
        ),
        "/Scenes/View" => pending_tree_view::<ScenePath>(
            state,
            request,
            "/Scenes/View",
            |tree, path| tree.resolve_collection(&path.first).is_some(),
            |path| {
                format!(
                    "Scene could not be resolved: {}",
                    path.first.to_diagnostic()
                )
            },
        ),
        "/Search/View" => search_view(state, request),
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

/// Run the frozen `TryGet*FromQuery` gates shared by every tree route.
///
/// A missing or malformed `Path` and an unloaded game each answer `404` with the
/// baseline's message; the `Ready` case is left to the caller.
fn tree_gates<T: PathDocument>(state: &WebState, request: &Request) -> Result<T, Response> {
    match view_gate::<T>(request.query_value("Path"), state.is_loaded()) {
        ViewGate::MissingPath => {
            Err(Response::text("The path must be included in the request.").with_status(404))
        }
        ViewGate::InvalidPath(diagnostic) => Err(Response::text(diagnostic).with_status(404)),
        ViewGate::NotLoaded => Err(Response::text("No files loaded.").with_status(404)),
        ViewGate::Ready(path) => Ok(path),
    }
}

/// The snapshot behind a loaded game. A loaded game always carries one; a `None` here
/// would mean the state invariant broke, which is reported instead of rendered.
fn loaded_tree(state: &WebState) -> Option<&super::tree::BundleTree> {
    state.tree()
}

/// `GET /Bundles/View`: render the resolved bundle from the snapshot.
fn bundles_view(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<BundlePath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some(bundle_index) = tree.resolve_bundle(&path) else {
        return Response::text(format!("Bundle could not be resolved: {path}")).with_status(404);
    };
    match pages::bundle_page(state, tree, bundle_index, &path) {
        Some(page) => Response::html(page),
        None => Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {path}."
        )),
    }
}

/// `GET /Collections/View`: render the resolved collection's asset table.
fn collections_view(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<CollectionPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position)) = tree.resolve_collection(&path) else {
        return Response::text(format!(
            "Asset collection could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    match pages::collection_page(state, tree, bundle_index, position, &path) {
        Some(page) => Response::html(page),
        None => Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        )),
    }
}

/// `GET /Collections/Count`: the frozen `CollectionAPI.GetCount` answers the asset count
/// as JSON once the collection resolves.
fn collections_count(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<CollectionPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position)) = tree.resolve_collection(&path) else {
        return Response::text(format!(
            "Asset collection could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    match tree
        .bundle(bundle_index)
        .and_then(|bundle| bundle.collections.get(position))
        .map(|collection| collection.asset_count)
    {
        Some(count) => Response::json(count.to_string()),
        None => Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        )),
    }
}

/// `GET /Search/View`: search every asset by display and class name.
///
/// The frozen page takes no `Path`: an unloaded game answers `404`, and the trimmed `q`
/// value (empty when absent) drives the form, the matching and the title alike.
fn search_view(state: &mut WebState, request: &Request) -> Response {
    let Some(tree) = loaded_tree(state) else {
        return Response::text("No files loaded.").with_status(404);
    };
    let query = request.query_value("q").unwrap_or("").trim().to_owned();
    Response::html(pages::search_page(state, &query, &tree.search(&query)))
}

/// One `GET /X/View` page whose asset model is still open: resolve the path, report an
/// unresolvable one with the baseline's message, and otherwise keep the explicit pending
/// page instead of inventing content.
fn pending_tree_view<T: PathDocument>(
    state: &mut WebState,
    request: &Request,
    route: &'static str,
    exists: impl FnOnce(&super::tree::BundleTree, &T) -> bool,
    unresolvable: impl FnOnce(&T) -> String,
) -> Response {
    let path = match tree_gates::<T>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let resolved = loaded_tree(state)
        .map(|tree| exists(tree, &path))
        .unwrap_or(false);
    if !resolved {
        return Response::text(unresolvable(&path)).with_status(404);
    }
    pending_tree_page(state, route)
}

/// The explicit pending page for a resolved path whose view model is still open.
fn pending_tree_page(state: &WebState, route: &str) -> Response {
    Response::html(pages::pending_page(state, "GET", route))
        .with_status(501)
        .with_content_type("text/html; charset=utf-8")
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
    fn every_tree_view_follows_the_frozen_query_gates() {
        use assetripper::web_paths::{
            asset_view_url, bundle_view_url, collection_view_url, failed_file_view_url,
            resource_view_url, scene_view_url, AssetPath, BundlePath, CollectionPath,
            FailedFilePath, ResourcePath, ScenePath,
        };

        let root_collection = CollectionPath {
            bundle: BundlePath::root(),
            index: 0,
        };
        let views = [
            (
                "/Bundles/View",
                bundle_view_url(&BundlePath::root().child(1)),
            ),
            ("/Collections/View", collection_view_url(&root_collection)),
            (
                "/Assets/View",
                asset_view_url(&AssetPath {
                    collection: root_collection.clone(),
                    path_id: 7,
                }),
            ),
            (
                "/Resources/View",
                resource_view_url(&ResourcePath {
                    bundle: BundlePath::root(),
                    index: 1,
                }),
            ),
            (
                "/FailedFiles/View",
                failed_file_view_url(&FailedFilePath {
                    bundle: BundlePath::root(),
                    index: 1,
                }),
            ),
            (
                "/Scenes/View",
                scene_view_url(&ScenePath::from(root_collection)),
            ),
        ];
        for (route, valid_url) in views {
            let mut state = state();
            // No Path: the baseline reports the missing path, not an unwired route.
            let response = dispatch(&mut state, &request("GET", route));
            assert_eq!(response.status(), 404, "{route}");
            assert_eq!(
                response.body(),
                b"The path must be included in the request.",
                "{route}"
            );
            // A malformed Path is a 404 carrying the parse diagnostic.
            let target = format!("{route}?Path=%7B");
            let response = dispatch(&mut state, &request("GET", &target));
            assert_eq!(response.status(), 404, "{route}");
            assert!(!response.body().is_empty(), "{route}");
            // A well-formed Path with nothing loaded reports the empty game.
            let response = dispatch(&mut state, &request("GET", &valid_url));
            assert_eq!(response.status(), 404, "{route}");
            assert_eq!(response.body(), b"No files loaded.", "{route}");
        }
    }

    #[test]
    fn an_unwired_baseline_route_reports_not_implemented_and_an_unknown_one_404() {
        let mut state = state();
        assert_eq!(status(&mut state, "POST", "/Export/PrimaryContent"), 501);
        // The search page takes no path: without a loaded game it reports that.
        let response = dispatch(&mut state, &request("GET", "/Search/View"));
        assert_eq!(response.status(), 404);
        assert_eq!(response.body(), b"No files loaded.");
        assert_eq!(status(&mut state, "GET", "/nope"), 404);
        // The same path with the other method is a method error, not a missing route.
        assert_eq!(status(&mut state, "GET", "/Reset"), 405);
        assert_eq!(status(&mut state, "POST", "/Privacy"), 405);
    }

    /// Load the committed nested-bundle fixture through the real pipeline and browse it.
    ///
    /// The fixture is a synthetic source-runtime UnityFS bundle (3,323 bytes, two
    /// SerializedFiles); it exercises the whole `load -> snapshot -> view` chain with
    /// repository data only.
    #[test]
    fn a_loaded_bundle_renders_its_tree_pages() {
        use assetripper::web_paths::url_encode;

        // The probe input lives under the ignored `target/` scratch tree, like the
        // other repository tests' temporary inputs.
        let directory =
            std::path::PathBuf::from("target/tree-e2e").join(std::process::id().to_string());
        std::fs::create_dir_all(&directory).expect("the probe directory is created");
        let input = directory.join("workspace.bundle");
        std::fs::write(
            &input,
            include_bytes!("../export/managed_reference_project_fixture/nested_reference.bundle"),
        )
        .expect("the probe bundle is written");
        // A second copy under an earlier-sorting name proves the display sort:
        // whatever order the mount keeps, the page lists bundles by name.
        let early_input = directory.join("a-workspace.bundle");
        std::fs::write(
            &early_input,
            include_bytes!("../export/managed_reference_project_fixture/nested_reference.bundle"),
        )
        .expect("the probe bundle is copied");
        let load_and_browse = || {
            let mut state = state();
            state
                .load(&[
                    input.to_string_lossy().into_owned(),
                    early_input.to_string_lossy().into_owned(),
                ])
                .expect("the probe bundles load");
            assert!(state.is_loaded());

            let get = |state: &mut WebState, target: &str| dispatch(state, &request("GET", target));
            let page = |json: &str| format!("/Bundles/View?Path={}", url_encode(json));

            // The root names the game bundle and links both children by name,
            // even though the later-sorting input was loaded first.
            let response = get(&mut state, &page("{\"P\":[]}"));
            assert_eq!(response.status(), 200);
            let root = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(root.contains("<h1>GameBundle</h1>"), "{root}");
            assert_eq!(
                anchor_texts(&root, "/Bundles/View?Path="),
                ["a-workspace.bundle", "workspace.bundle"]
            );
            assert!(!root.contains("Collections</h2>"));

            // The outer container names its bundle after the input file, links back
            // to the root, and holds one child bundle plus one collection.
            let response = get(&mut state, &page("{\"P\":[0]}"));
            assert_eq!(response.status(), 200);
            let child = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(child.contains("<h1>workspace.bundle</h1>"), "{child}");
            assert!(child.contains(&page("{\"P\":[]}")), "{child}");
            let nested = anchor_texts(&child, "/Bundles/View?Path=");
            assert_eq!(nested, ["GameBundle", "nested.bundle"], "{child}");
            let collections = anchor_texts(&child, "/Collections/View?Path=");
            assert_eq!(collections.len(), 1, "{child}");
            assert!(collections[0].contains("managed.assets"), "{collections:?}");
            assert!(!child.contains("Resources</h2>"), "{child}");
            assert!(!child.contains("FailedFiles</h2>"), "{child}");

            // The nested container holds the second collection; both collections
            // together census every object in the fixture.
            let response = get(&mut state, &page("{\"P\":[0,0]}"));
            assert_eq!(response.status(), 200);
            let inner = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(inner.contains(&page("{\"P\":[0]}")), "{inner}");
            let inner_collections = anchor_texts(&inner, "/Collections/View?Path=");
            assert_eq!(inner_collections.len(), 1, "{inner}");

            // Counts are served as JSON like `GetCount` does.
            let mut total = 0;
            for collection_json in [
                "{\"B\":{\"P\":[0]},\"I\":0}",
                "{\"B\":{\"P\":[0,0]},\"I\":0}",
            ] {
                let count = format!("/Collections/Count?Path={}", url_encode(collection_json));
                let response = get(&mut state, &count);
                assert_eq!(response.status(), 200, "{count}");
                let body = String::from_utf8(response.body().to_vec()).expect("counts are UTF-8");
                total += body.parse::<usize>().expect("counts are JSON numbers");
            }
            assert_eq!(total, 2);

            // A resolved collection renders its asset table with links.
            let view = format!(
                "/Collections/View?Path={}",
                url_encode("{\"B\":{\"P\":[0]},\"I\":0}")
            );
            let response = get(&mut state, &view);
            assert_eq!(response.status(), 200);
            let collection = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(collection.contains("assetsTable"), "{collection}");
            assert!(collection.contains("/Assets/View?Path="), "{collection}");
            assert!(collection.contains("MonoBehaviour"), "{collection}");

            // Searching the class finds every fixture asset (both loaded copies),
            // case-insensitively.
            for query in ["MonoBehaviour", "MONOBEHAVIOUR", "mono"] {
                let search = format!("/Search/View?q={query}");
                let response = get(&mut state, &search);
                assert_eq!(response.status(), 200, "{search}");
                let body = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
                assert_eq!(body.matches("/Assets/View?Path=").count(), 4, "{search}");
                assert!(body.contains("/Collections/View?Path="), "{search}");
            }

            // Searching a display name from the table finds its row.
            let names = anchor_texts(&collection, "/Assets/View?Path=");
            assert!(!names.is_empty());
            let search = format!("/Search/View?q={}", url_encode(&names[0]));
            let response = get(&mut state, &search);
            assert_eq!(response.status(), 200, "{search}");
            let body = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(body.contains("/Assets/View?Path="), "{search}");

            // An empty query prompts, a hopeless one reports no results.
            let response = get(&mut state, "/Search/View");
            assert_eq!(response.status(), 200);
            let prompt = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(prompt.contains("name=\"q\""), "{prompt}");
            assert!(!prompt.contains("resultsTable"), "{prompt}");
            let response = get(&mut state, "/Search/View?q=zzz-no-such-asset");
            assert_eq!(response.status(), 200);
            let empty = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(!empty.contains("resultsTable"), "{empty}");
            assert!(!empty.contains("/Assets/View?Path="), "{empty}");

            // Unresolvable paths report the baseline's per-kind message.
            let response = get(&mut state, &page("{\"P\":[7]}"));
            assert_eq!(response.status(), 404);
            assert_eq!(
                response.body(),
                b"Bundle could not be resolved: {\"P\":[7]}"
            );
            let missing = format!(
                "/Collections/Count?Path={}",
                url_encode("{\"B\":{\"P\":[0]},\"I\":9}")
            );
            let response = get(&mut state, &missing);
            assert_eq!(response.status(), 404);
            assert_eq!(
                response.body(),
                b"Asset collection could not be resolved: CollectionPath { BundlePath = {\"P\":[0]}, Index = 9 }"
            );
        };
        load_and_browse();
        std::fs::remove_dir_all(&directory).ok();
    }

    /// Every `NAME</a>` anchor text pointing at `href_prefix`, in document order.
    fn anchor_texts(page: &str, href_prefix: &str) -> Vec<String> {
        let mut texts = Vec::new();
        let mut rest = page;
        while let Some(start) = rest.find(href_prefix) {
            let after_href = rest.get(start + href_prefix.len()..).unwrap_or("");
            let Some(end) = after_href.find("</a>") else {
                break;
            };
            let anchor = after_href.get(..end).unwrap_or("");
            if let Some(text_start) = anchor.rfind('>') {
                texts.push(anchor[text_start + 1..].to_owned());
            }
            let tail = after_href.get(end..).unwrap_or("");
            rest = tail.strip_prefix("</a>").unwrap_or(tail);
        }
        texts
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
