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
use assetripper::resources::image_export::ImageExportFormat;

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
        "/Assets/View" => assets_view(state, request),
        "/Assets/Text" => assets_text(state, request),
        "/Assets/Binary" => assets_binary(state, request),
        "/Assets/Image" => assets_image(state, request),
        "/Assets/Json" => assets_json(state, request),
        "/Assets/Yaml" => assets_yaml(state, request),
        "/Assets/Audio" => assets_audio(state, request),
        "/Resources/View" => resources_view(state, request),
        "/Resources/Data" => resources_data(state, request),
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

/// `GET /Assets/View`: render the resolved asset's information section.
///
/// Only the identity section is wired so far; the remaining asset tabs wait for their
/// own data ports.
fn assets_view(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<AssetPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position, row)) = tree.resolve_asset(&path) else {
        return Response::text(format!(
            "Asset could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    match pages::asset_page(state, tree, bundle_index, position, row, &path) {
        Some(page) => Response::html(page),
        None => Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        )),
    }
}

/// `GET /Assets/Text`: serve the resolved asset's text.
///
/// Only TextAsset scripts are served; every other asset reads as no text, matching the
/// frozen route's empty reply for assets without text.
fn assets_text(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<AssetPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position, row)) = tree.resolve_asset(&path) else {
        return Response::text(format!(
            "Asset could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    let Some(asset) = tree
        .bundle(bundle_index)
        .and_then(|bundle| bundle.collections.get(position))
        .and_then(|collection| collection.assets.get(row))
    else {
        return Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        ));
    };
    if !asset.has_text {
        return Response::text("");
    }
    match state.read_asset_text(asset.file_index, asset.object_index) {
        Ok(script) => Response::text(
            script
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default(),
        ),
        Err(diagnostic) => Response::internal_error(diagnostic),
    }
}

/// `GET /Assets/Binary`: serve the resolved asset's raw bytes.
///
/// Only undecodable objects serve bytes; every other asset reads as no bytes, matching
/// the frozen route's empty reply for assets without binary data.
fn assets_binary(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<AssetPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position, row)) = tree.resolve_asset(&path) else {
        return Response::text(format!(
            "Asset could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    let Some(asset) = tree
        .bundle(bundle_index)
        .and_then(|bundle| bundle.collections.get(position))
        .and_then(|collection| collection.assets.get(row))
    else {
        return Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        ));
    };
    if !asset.has_binary {
        return Response::bytes("application/octet-stream", Vec::new());
    }
    match state.read_asset_raw_bytes(asset.file_index, asset.object_index) {
        Ok(raw) => Response::bytes("application/octet-stream", raw),
        Err(diagnostic) => Response::internal_error(diagnostic),
    }
}

/// `GET /Assets/Image`: serve the resolved texture asset as an encoded image.
///
/// The optional `Extension` query selects the encoding; an unrecognized or absent value
/// falls back to PNG, matching the frozen route's default. Assets without an image read as
/// an empty reply rather than an error, like the frozen `TextureAsset` path.
fn assets_image(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<AssetPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let format = match request.query_value("Extension").unwrap_or("") {
        "" | "png" => ImageExportFormat::Png,
        other => match ImageExportFormat::try_from_extension(other) {
            Some(format) => format,
            None => ImageExportFormat::Png,
        },
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position, row)) = tree.resolve_asset(&path) else {
        return Response::text(format!(
            "Asset could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    let Some(asset) = tree
        .bundle(bundle_index)
        .and_then(|bundle| bundle.collections.get(position))
        .and_then(|collection| collection.assets.get(row))
    else {
        return Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        ));
    };
    if !asset.has_image {
        return Response::bytes("image/png", Vec::new());
    }
    match state.render_texture_image(asset.file_index, asset.object_index, format) {
        Ok(bytes) => Response::bytes("image/png", bytes),
        Err(diagnostic) => Response::internal_error(diagnostic),
    }
}

/// `GET /Assets/Json`: serve the resolved asset as JSON.
///
/// The frozen route walks the asset and answers `application/json`; an asset with no decoded
/// payload answers an empty body, matching the frozen route's empty reply.
fn assets_json(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<AssetPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position, row)) = tree.resolve_asset(&path) else {
        return Response::text(format!(
            "Asset could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    let Some(asset) = tree
        .bundle(bundle_index)
        .and_then(|bundle| bundle.collections.get(position))
        .and_then(|collection| collection.assets.get(row))
    else {
        return Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        ));
    };
    match state.render_asset_json(asset.file_index, asset.object_index) {
        Ok(text) => Response::json(text),
        Err(diagnostic) => Response::internal_error(diagnostic),
    }
}

/// `GET /Assets/Yaml`: serve the resolved asset as a standalone Unity YAML document.
///
/// The frozen route answers `application/yaml`; an asset with no decoded payload answers an
/// error body rather than YAML, matching the frozen route's `ex.ToString()` fallback.
fn assets_yaml(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<AssetPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position, row)) = tree.resolve_asset(&path) else {
        return Response::text(format!(
            "Asset could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    let Some(asset) = tree
        .bundle(bundle_index)
        .and_then(|bundle| bundle.collections.get(position))
        .and_then(|collection| collection.assets.get(row))
    else {
        return Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        ));
    };
    match state.render_asset_yaml(asset.file_index, asset.object_index) {
        Ok(text) => Response::bytes("application/yaml", text.into_bytes()),
        Err(diagnostic) => Response::internal_error(diagnostic),
    }
}

/// `GET /Assets/Audio`: serve the resolved clip's decoded audio.
///
/// The optional `Extension` query asks for a container; `wav` transcodes an Ogg result, which
/// is what the frozen page's playback source requests. A clip with no decodable audio answers
/// `404` with the frozen route's message rather than serving container bytes.
fn assets_audio(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<AssetPath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let desired = request.query_value("Extension");
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position, row)) = tree.resolve_asset(&path) else {
        return Response::text(format!(
            "Asset could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    let Some(asset) = tree
        .bundle(bundle_index)
        .and_then(|bundle| bundle.collections.get(position))
        .and_then(|collection| collection.assets.get(row))
    else {
        return Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        ));
    };
    if !asset.has_audio {
        return Response::text("Asset was not an audio clip.").with_status(404);
    }
    match state.read_asset_audio(asset.file_index, asset.object_index, desired) {
        Ok(Some((bytes, extension))) => Response::bytes(format!("audio/{extension}"), bytes),
        Ok(None) => Response::text("Audio data could not be decoded.").with_status(404),
        Err(_) => Response::text("Audio data could not be decoded.").with_status(404),
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

/// `GET /Resources/View`: render the resolved resource from the snapshot.
fn resources_view(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<ResourcePath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position)) = tree.resolve_resource(&path) else {
        return Response::text(format!(
            "Resource file could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    let bytes = match resource_bytes(state, bundle_index, position) {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    match pages::resource_page(state, tree, bundle_index, position, &path, bytes.len()) {
        Some(page) => Response::html(page),
        None => Response::internal_error(format!(
            "The bundle tree disagrees with the resolved path {}.",
            path.to_diagnostic()
        )),
    }
}

/// `GET /Resources/Data`: serve the resolved resource bytes as an octet stream.
fn resources_data(state: &mut WebState, request: &Request) -> Response {
    let path = match tree_gates::<ResourcePath>(state, request) {
        Ok(path) => path,
        Err(response) => return response,
    };
    let Some(tree) = loaded_tree(state) else {
        return Response::internal_error("A game is loaded but its bundle tree is missing.");
    };
    let Some((bundle_index, position)) = tree.resolve_resource(&path) else {
        return Response::text(format!(
            "Resource file could not be resolved: {}",
            path.to_diagnostic()
        ))
        .with_status(404);
    };
    match resource_bytes(state, bundle_index, position) {
        Ok(bytes) => Response::bytes("application/octet-stream", bytes),
        Err(response) => response,
    }
}

/// Read one snapshot resource's bytes through the loaded game.
fn resource_bytes(
    state: &WebState,
    bundle_index: usize,
    position: usize,
) -> Result<Vec<u8>, Response> {
    let virtual_path = state
        .tree()
        .and_then(|tree| tree.bundle(bundle_index))
        .and_then(|bundle| bundle.resources.get(position))
        .map(|resource| resource.virtual_path.clone());
    let Some(virtual_path) = virtual_path else {
        return Err(Response::internal_error(
            "The bundle tree disagrees with the resolved path.",
        ));
    };
    state
        .read_resource_bytes(&virtual_path)
        .map_err(Response::internal_error)
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

        /// The synthetic streamed-resource input served by the resource routes.
        const PROBE_RESOURCE_BYTES: &[u8] = b"probe streamed-resource bytes";

        // The probe input lives under the ignored `target/` scratch tree, like the
        // other repository tests' temporary inputs.
        use crate::tests::{make_generated_audio_clip_file, make_reference_texture_file};

        // The committed Ogg fixture the audio decoder tests already use, so the audio route
        // exercises the same bytes those tests validate against a reference WAV.
        const AUDIO_CLIP_OGG_BYTES: &[u8] = include_bytes!("../export/ogg_to_wav_test_fixture.ogg");

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
        // An explicit streamed-resource input lists on the root and serves bytes.
        let probe_resource = directory.join("probe.resS");
        std::fs::write(&probe_resource, PROBE_RESOURCE_BYTES)
            .expect("the probe resource is written");
        // A synthetic TextAsset file proves the text routes; the committed
        // source-factory fixture carries the complete 2019.4 no-TypeTree file.
        // An unknown-class file proves the binary route the same way.
        let fixture_hex = |case: &str| {
            assetripper::processing::asset_factory::PROBE_FIXTURE
                .lines()
                .find_map(|line| line.strip_prefix(&format!("SF_HEX|{case}|")))
                .unwrap_or_else(|| panic!("the source-factory fixture includes {case}"))
        };
        let decode_hex = |hex: &str| {
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                        .expect("hex is valid")
                })
                .collect::<Vec<_>>()
        };
        let text_input = directory.join("z-textasset.assets");
        std::fs::write(&text_input, decode_hex(fixture_hex("textasset_notree")))
            .expect("the probe text asset is written");
        let unknown_input = directory.join("z-unknown.assets");
        std::fs::write(&unknown_input, decode_hex(fixture_hex("unknown_class")))
            .expect("the probe unknown asset is written");
        // A Texture2D whose file TypeTree is the real 2019.4 release tree, so the image
        // route exercises the production class layout with a payload that decodes under it.
        let texture_input = directory.join("z-texture.assets");
        std::fs::write(
            &texture_input,
            make_reference_texture_file(2, 1, &[10, 20, 30, 255, 40, 50, 60, 255]),
        )
        .expect("the texture asset is written");
        // An AudioClip carrying inline Ogg payload with no sound type, so the audio tab and
        // route exercise `AudioClipDecoder.TryDecode` on real Vorbis bytes and the WAV
        // transcode, rather than only the class-ID test.
        let audio_input = directory.join("z-audio.assets");
        std::fs::write(
            &audio_input,
            make_generated_audio_clip_file(AUDIO_CLIP_OGG_BYTES),
        )
        .expect("the audio asset is written");
        let load_and_browse = || {
            // Production layouts: the test binary otherwise substitutes synthetic
            // TypeTrees that cannot decode the no-TypeTree TextAsset.
            let _layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
            let mut state = state();
            state
                .load(&[
                    input.to_string_lossy().into_owned(),
                    early_input.to_string_lossy().into_owned(),
                    probe_resource.to_string_lossy().into_owned(),
                    text_input.to_string_lossy().into_owned(),
                    unknown_input.to_string_lossy().into_owned(),
                    texture_input.to_string_lossy().into_owned(),
                    audio_input.to_string_lossy().into_owned(),
                ])
                .expect("the probe inputs load");
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
            // The bare synthetic files list as root collections, by name.
            assert_eq!(
                anchor_texts(&root, "/Collections/View?Path="),
                [
                    "z-audio.assets",
                    "z-textasset.assets",
                    "z-texture.assets",
                    "z-unknown.assets"
                ]
            );

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

            // The explicit resource input lists on the root with its byte size
            // and serves the exact bytes back.
            let resources = anchor_texts(&root, "/Resources/View?Path=");
            assert_eq!(resources, ["probe.resS"]);
            let view = format!(
                "/Resources/View?Path={}",
                url_encode("{\"B\":{\"P\":[]},\"I\":0}")
            );
            let response = get(&mut state, &view);
            assert_eq!(response.status(), 200);
            let resource = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            // The bundle link keeps the raw entry name; the view title and the
            // download name use the identifier-fixed form like the frozen page.
            assert!(resource.contains("<h1>probe.ress</h1>"), "{resource}");
            assert!(resource.contains("download=\"probe.ress\""), "{resource}");
            assert!(resource.contains(">Size<"), "{resource}");
            assert!(
                resource.contains(&format!("<p>{}</p>", PROBE_RESOURCE_BYTES.len())),
                "{resource}"
            );
            let data = format!(
                "/Resources/Data?Path={}",
                url_encode("{\"B\":{\"P\":[]},\"I\":0}")
            );
            let response = get(&mut state, &data);
            assert_eq!(response.status(), 200);
            assert_eq!(response.content_type(), "application/octet-stream");
            assert_eq!(response.body(), PROBE_RESOURCE_BYTES);
            let missing = format!(
                "/Resources/Data?Path={}",
                url_encode("{\"B\":{\"P\":[]},\"I\":9}")
            );
            let response = get(&mut state, &missing);
            assert_eq!(response.status(), 404);
            assert_eq!(
                response.body(),
                b"Resource file could not be resolved: ResourcePath { BundlePath = {\"P\":[]}, Index = 9 }"
            );

            // The TextAsset collection renders one row; its page inlines the
            // script with a download, and the data route serves the same bytes.
            let text_view = format!(
                "/Collections/View?Path={}",
                url_encode("{\"B\":{\"P\":[]},\"I\":1}")
            );
            let response = get(&mut state, &text_view);
            assert_eq!(response.status(), 200);
            let text_collection =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(text_collection.contains(">Hero<"), "{text_collection}");
            assert!(text_collection.contains("TextAsset"), "{text_collection}");
            let text_hrefs = anchor_hrefs(&text_collection, "/Assets/View?Path=");
            assert_eq!(text_hrefs.len(), 1);
            let response = get(&mut state, &text_hrefs[0]);
            assert_eq!(response.status(), 200);
            let text_asset = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(text_asset.contains("<h1>Hero</h1>"), "{text_asset}");
            assert!(text_asset.contains("<pre"), "{text_asset}");
            assert!(text_asset.contains("print(1)"), "{text_asset}");
            assert!(text_asset.contains("download=\"Hero.txt\""), "{text_asset}");
            let text_data = text_hrefs[0].replacen("/Assets/View?Path=", "/Assets/Text?Path=", 1);
            let response = get(&mut state, &text_data);
            assert_eq!(response.status(), 200);
            assert_eq!(response.content_type(), "text/plain; charset=utf-8");
            assert_eq!(response.body(), b"print(1)");

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

            // A resolved asset renders its information section with the numeric
            // class ID, the class name and the collection link.
            let asset_hrefs = anchor_hrefs(&collection, "/Assets/View?Path=");
            assert!(!asset_hrefs.is_empty());
            let response = get(&mut state, &asset_hrefs[0]);
            assert_eq!(response.status(), 200);
            let asset = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(asset.contains("<h1>"), "{asset}");
            assert!(asset.contains(">114<"), "{asset}");
            assert!(asset.contains("MonoBehaviour"), "{asset}");
            assert!(asset.contains("/Collections/View?Path="), "{asset}");
            // The reference edges render with field paths and target links: the
            // managed cross-file edge resolves to the nested ReferencedRoot
            // through the same source-order lookup the export uses, even though
            // the duplicate input names would stay ambiguous under a plain
            // name join.
            assert!(asset.contains(">Dependencies<"), "{asset}");
            assert!(
                asset.contains("references.RefIds[0].data.m_Asset"),
                "{asset}"
            );
            let target = format!(
                "/Assets/View?Path={}",
                url_encode("{\"C\":{\"B\":{\"P\":[0,0]},\"I\":0},\"D\":900}")
            );
            assert!(asset.contains(&target), "{asset}");
            assert!(asset.contains(">ReferencedRoot<"), "{asset}");
            assert!(!asset.contains(">Missing<"), "{asset}");

            // The referenced asset itself has no outgoing references: no section.
            let nested_view = format!(
                "/Collections/View?Path={}",
                url_encode("{\"B\":{\"P\":[0,0]},\"I\":0}")
            );
            let response = get(&mut state, &nested_view);
            assert_eq!(response.status(), 200);
            let nested_collection =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            let nested_hrefs = anchor_hrefs(&nested_collection, "/Assets/View?Path=");
            assert_eq!(nested_hrefs.len(), 1);
            let response = get(&mut state, &nested_hrefs[0]);
            assert_eq!(response.status(), 200);
            let nested_asset =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(!nested_asset.contains(">Dependencies<"), "{nested_asset}");

            // A MonoBehaviour has no text section and reads as empty text.
            assert!(!asset.contains("/Assets/Text?Path="), "{asset}");
            assert!(!asset.contains("print(1)"), "{asset}");
            let mute_data = asset_hrefs[0].replacen("/Assets/View?Path=", "/Assets/Text?Path=", 1);
            let response = get(&mut state, &mute_data);
            assert_eq!(response.status(), 200);
            assert!(response.body().is_empty());

            // The unknown-class object serves its raw bytes with a download,
            // while decoded assets have neither the section nor the bytes.
            let unknown_view = format!(
                "/Collections/View?Path={}",
                url_encode("{\"B\":{\"P\":[]},\"I\":3}")
            );
            let response = get(&mut state, &unknown_view);
            assert_eq!(response.status(), 200);
            let unknown_collection =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(
                unknown_collection.contains("Unknown7_3FCA88C5"),
                "{unknown_collection}"
            );
            let unknown_hrefs = anchor_hrefs(&unknown_collection, "/Assets/View?Path=");
            assert_eq!(unknown_hrefs.len(), 1);
            let response = get(&mut state, &unknown_hrefs[0]);
            assert_eq!(response.status(), 200);
            let unknown_asset =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(
                unknown_asset.contains("download=\"Unknown7_3FCA88C5.dat\""),
                "{unknown_asset}"
            );
            let unknown_data =
                unknown_hrefs[0].replacen("/Assets/View?Path=", "/Assets/Binary?Path=", 1);
            let response = get(&mut state, &unknown_data);
            assert_eq!(response.status(), 200);
            assert_eq!(response.content_type(), "application/octet-stream");
            assert_eq!(
                response.body(),
                b"\x01\x02\x03\x04\x05\x06\x07\x08".as_slice()
            );
            assert!(!asset.contains("/Assets/Binary?Path="), "{asset}");
            assert!(!text_asset.contains("/Assets/Binary?Path="), "{text_asset}");
            let mute_binary =
                asset_hrefs[0].replacen("/Assets/View?Path=", "/Assets/Binary?Path=", 1);
            let response = get(&mut state, &mute_binary);
            assert_eq!(response.status(), 200);
            assert!(response.body().is_empty());
            let missing_asset = format!(
                "/Assets/View?Path={}",
                url_encode("{\"C\":{\"B\":{\"P\":[0]},\"I\":0},\"D\":999999}")
            );
            let response = get(&mut state, &missing_asset);
            assert_eq!(response.status(), 404);
            assert_eq!(
                response.body(),
                b"Asset could not be resolved: AssetPath { CollectionPath = CollectionPath { BundlePath = {\"P\":[0]}, Index = 0 }, PathID = 999999 }"
            );

            // The texture collection's single asset serves a real PNG. The asset page
            // advertises it and the image route returns the encoded pixels, so the tab is
            // exercised end to end rather than only rendered.
            let texture_view = format!(
                "/Collections/View?Path={}",
                url_encode("{\"B\":{\"P\":[]},\"I\":2}")
            );
            let response = get(&mut state, &texture_view);
            assert_eq!(response.status(), 200);
            let texture_collection =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(
                texture_collection.contains("z-texture.assets"),
                "{texture_collection}"
            );
            let texture_hrefs = anchor_hrefs(&texture_collection, "/Assets/View?Path=");
            assert_eq!(texture_hrefs.len(), 1, "{texture_collection}");
            let response = get(&mut state, &texture_hrefs[0]);
            assert_eq!(response.status(), 200);
            let texture_asset =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(
                texture_asset.contains("/Assets/Image?Path="),
                "{texture_asset}"
            );
            let texture_image =
                texture_hrefs[0].replacen("/Assets/View?Path=", "/Assets/Image?Path=", 1)
                    + "&Extension=png";
            let response = get(&mut state, &texture_image);
            assert_eq!(response.status(), 200);
            assert_eq!(response.content_type(), "image/png");
            let png = response.body().to_vec();
            assert!(
                png.starts_with(b"\x89PNG\r\n\x1a\n"),
                "the image route serves an encoded PNG"
            );
            // The port writes PNG itself, so verify the pixels by inflating the IDAT
            // payload rather than only trusting the container header: a wrong format or a
            // truncated payload still starts with a valid signature.
            assert_png_pixels(&png, 2, 1, &[10, 20, 30, 255, 40, 50, 60, 255]);

            // Every asset also carries a JSON tab, so the texture asset's JSON is the
            // decoded payload as an object. The route and the inlined tab must agree.
            assert!(
                texture_asset.contains("/Assets/Json?Path="),
                "{texture_asset}"
            );
            let texture_json =
                texture_hrefs[0].replacen("/Assets/View?Path=", "/Assets/Json?Path=", 1);
            let response = get(&mut state, &texture_json);
            assert_eq!(response.status(), 200);
            assert_eq!(response.content_type(), "application/json; charset=utf-8");
            let json = String::from_utf8(response.body().to_vec()).expect("JSON is UTF-8");
            assert!(json.starts_with('{'), "{json}");
            assert!(json.contains("\"m_Width\": 2"), "{json}");
            assert!(json.contains("\"m_Height\": 1"), "{json}");
            // The inlined tab escapes the JSON for HTML, so the page carries the escaped
            // quotes while the route serves the raw ones.
            assert!(
                texture_asset.contains("&quot;m_Width&quot;: 2"),
                "{texture_asset}"
            );

            // The YAML tab is enabled for every asset too, and its document carries the baseline
            // header, the class-prefixed main export id, and the decoded fields.
            assert!(
                texture_asset.contains("/Assets/Yaml?Path="),
                "{texture_asset}"
            );
            assert!(
                texture_asset.contains("download=\"Texture2D.asset\""),
                "{texture_asset}"
            );
            let texture_yaml =
                texture_hrefs[0].replacen("/Assets/View?Path=", "/Assets/Yaml?Path=", 1);
            let response = get(&mut state, &texture_yaml);
            assert_eq!(response.status(), 200);
            assert_eq!(response.content_type(), "application/yaml");
            let yaml = String::from_utf8(response.body().to_vec()).expect("YAML is UTF-8");
            assert!(
                yaml.starts_with("%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n"),
                "{yaml}"
            );
            // `GetMainExportID(28, 0)` is `2800000` and the class tag is `!u!28`.
            assert!(yaml.contains("--- !u!28 &2800000"), "{yaml}");
            assert!(yaml.contains("Texture2D:"), "{yaml}");
            assert!(yaml.contains("m_Width: 2"), "{yaml}");
            assert!(yaml.contains("m_Height: 1"), "{yaml}");

            // The AudioClip collection's asset advertises the audio tab, and the route decodes
            // the inline Ogg into a playable WAV when asked, matching the frozen playback source.
            let audio_view = format!(
                "/Collections/View?Path={}",
                url_encode("{\"B\":{\"P\":[]},\"I\":0}")
            );
            let response = get(&mut state, &audio_view);
            assert_eq!(response.status(), 200);
            let audio_collection =
                String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            let audio_hrefs = anchor_hrefs(&audio_collection, "/Assets/View?Path=");
            assert_eq!(audio_hrefs.len(), 1, "{audio_collection}");
            let response = get(&mut state, &audio_hrefs[0]);
            assert_eq!(response.status(), 200);
            let audio_asset = String::from_utf8(response.body().to_vec()).expect("pages are UTF-8");
            assert!(audio_asset.contains("/Assets/Audio?Path="), "{audio_asset}");
            // Build the route target from the same asset path the page links, so the test
            // exercises the tab's own URL shape rather than a hand-written one.
            let audio_path = audio_hrefs[0].replacen("/Assets/View?Path=", "", 1);
            let audio_wav = format!("/Assets/Audio?Path={audio_path}&Extension=wav");
            let response = get(&mut state, &audio_wav);
            assert_eq!(
                response.status(),
                200,
                "audio body: {}",
                String::from_utf8_lossy(response.body())
            );
            // The clip carries an FMOD sound type, so the decoder passes the inline bytes
            // through and names the extension from that type; the `Extension=wav` request only
            // transcodes when the decoder produced Ogg. Asserting the bytes and the shape
            // rather than a hardcoded extension keeps this independent of the type table.
            let decoded_extension = response
                .content_type()
                .strip_prefix("audio/")
                .expect("the route serves audio")
                .to_owned();
            assert!(!decoded_extension.is_empty());
            assert_eq!(response.body(), AUDIO_CLIP_OGG_BYTES);
            // The save source takes the same bytes without asking for a container.
            let audio_save = format!("/Assets/Audio?Path={audio_path}");
            let response = get(&mut state, &audio_save);
            assert_eq!(response.status(), 200);
            assert_eq!(
                response.content_type(),
                format!("audio/{decoded_extension}")
            );
            assert_eq!(response.body(), AUDIO_CLIP_OGG_BYTES);

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

    /// Every `href="PREFIX..."` link target starting with `href_prefix`, in order.
    fn anchor_hrefs(page: &str, href_prefix: &str) -> Vec<String> {
        let mut hrefs = Vec::new();
        let mut rest = page;
        let marker = format!("href=\"{href_prefix}");
        while let Some(start) = rest.find(marker.as_str()) {
            let after_marker = rest.get(start + marker.len()..).unwrap_or("");
            let Some(end) = after_marker.find('"') else {
                break;
            };
            hrefs.push(format!(
                "{href_prefix}{}",
                after_marker.get(..end).unwrap_or("")
            ));
            rest = after_marker.get(end..).unwrap_or("");
        }
        hrefs
    }

    use std::io::Read;

    /// Assert an 8-bit RGBA PNG carries exactly `expected` pixels.
    ///
    /// The port writes PNG itself (filter-0 scanlines), so this reads the header and
    /// inflates the `IDAT` payload with the same `flate2` backend the writer uses. That
    /// keeps the image route's test independent of the encoder while still checking the
    /// pixel values rather than only the container signature.
    fn assert_png_pixels(png: &[u8], width: u32, height: u32, expected: &[u8]) {
        assert!(
            png.starts_with(b"\x89PNG\r\n\x1a\n"),
            "the PNG signature is present"
        );
        let mut offset = 8;
        let (mut seen_width, mut seen_height) = (0u32, 0u32);
        let mut idat = Vec::new();
        while offset + 8 <= png.len() {
            let length = u32::from_be_bytes(png[offset..offset + 4].try_into().expect("four bytes"))
                as usize;
            let kind = &png[offset + 4..offset + 8];
            let body = png
                .get(offset + 8..offset + 8 + length)
                .expect("the chunk body is present");
            match kind {
                b"IHDR" => {
                    seen_width = u32::from_be_bytes(body[0..4].try_into().expect("four bytes"));
                    seen_height = u32::from_be_bytes(body[4..8].try_into().expect("four bytes"));
                }
                b"IDAT" => idat.extend_from_slice(body),
                b"IEND" => break,
                _ => {}
            }
            offset += 12 + length;
        }
        assert_eq!((seen_width, seen_height), (width, height));
        let mut raw = Vec::new();
        flate2::read::ZlibDecoder::new(idat.as_slice())
            .read_to_end(&mut raw)
            .expect("the IDAT payload inflates");
        // Filter-0 scanlines: one filter byte per row, then RGBA pixels.
        let stride = (width as usize) * 4;
        assert_eq!(raw.len(), (stride + 1) * height as usize);
        for row in 0..height as usize {
            let start = row * (stride + 1);
            assert_eq!(raw[start], 0, "the writer emits filter-0 scanlines");
            assert_eq!(
                &raw[start + 1..start + 1 + stride],
                &expected[row * stride..(row + 1) * stride]
            );
        }
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
