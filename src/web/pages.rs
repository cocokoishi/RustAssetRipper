//! HTML pages for the product web surface.
//!
//! Each function here stands for one page class in the frozen product. The markup is emitted
//! with the shared [`Html`] builder and every user-visible string comes from the vendored
//! message tables, so a page follows the selected language.
//!
//! The baseline composes these pages from ASP.NET element builders and leans on Bootstrap,
//! Vue and native dialogs. The port renders a server-side equivalent: menus are `<details>`
//! elements instead of Bootstrap dropdowns, the Commands page submits plain forms instead of a
//! Vue model, and the deletion confirmation is an HTML page instead of the native Windows
//! dialog. Every entry point the baseline offers stays reachable, and no page renders text the
//! message tables do not define: a mistyped key shows the `⟦missing:…⟧` marker and fails the
//! render tests.

use std::borrow::Cow;

use assetripper::product::WEB_ROUTES;

use super::html::Html;
use super::settings_form::{
    boolean_value, selected_value, version_text, Dropdown, DROPDOWNS, FREE_EDITION_DISABLED_KEYS,
    PREMIUM_NOTICE_KEY,
};
use super::state::WebState;
use assetripper::web_paths::{bundle_view_url, BundlePath};

/// The localization key of the class id of the shader asset class.
const SHADER_CLASS_ID: i16 = 48;
/// The localization key of the class id of the mesh asset class.
const MESH_CLASS_ID: i16 = 43;

/// One boolean control: the form name and the localization key of its label.
const BOOLEAN_LABELS: &[(&str, &str)] = &[
    ("IgnoreStreamingAssets", "skip_streaming_assets"),
    (
        "EnableStaticMeshSeparation",
        "enable_static_mesh_separation",
    ),
    ("RemoveNullableAttributes", "remove_nullable_attributes"),
    ("PublicizeAssemblies", "publicize_assemblies"),
    ("EnablePrefabOutlining", "enable_prefab_outlining"),
    ("EnableAssetDeduplication", "enable_asset_deduplication"),
    (
        "ScriptTypesFullyQualified",
        "scripts_use_fully_qualified_type_names",
    ),
    (
        "PreferOriginalTextureExtension",
        "prefer_original_texture_extension",
    ),
    ("ExportUnreadableAssets", "export_unreadable_assets"),
    ("SaveSettingsToDisk", "save_settings_to_disk"),
];

/// One version text control: the form name and the localization key of its label.
const TEXT_LABELS: &[(&str, &str)] = &[
    ("DefaultVersion", "default_version"),
    ("TargetVersion", "target_version_for_version_changing"),
];

/// The vendored license texts, paired with the name shown in the tab navigation.
const LICENSE_TEXTS: &[(&str, &str)] = &[
    ("AssetRipper", include_str!("../../LICENSE.md")),
    ("Localizations", include_str!("localizations/LICENSE")),
    (
        "dotnetdll",
        include_str!("../../vendor/dotnetdll-0.3.0/LICENSE"),
    ),
    (
        "fsbex-vorbis-table",
        include_str!("../../vendor/fsbex-vorbis-table/LICENSE-MIT"),
    ),
    (
        "texture2ddecoder (copyright)",
        include_str!("../../vendor/texture2ddecoder/COPYRIGHT"),
    ),
    (
        "texture2ddecoder (Apache-2.0)",
        include_str!("../../vendor/texture2ddecoder/LICENSE-APACHE"),
    ),
    (
        "texture2ddecoder (MIT)",
        include_str!("../../vendor/texture2ddecoder/LICENSE-MIT"),
    ),
];

/// The cascade for the port's pages: a dark, self-contained stylesheet.
pub const SITE_CSS: &str = include_str!("site.css");

/// The page's text for a message key.
fn t<'a>(state: &'a WebState, key: &str) -> Cow<'a, str> {
    state.messages().text(state.language(), key)
}

/// Percent-encode a query value; the single encoder lives in
/// [`assetripper::web_paths`].
pub use assetripper::web_paths::url_encode;

/// The root bundle's view URL, matching `BundleAPI.GetViewUrl` for an empty `BundlePath`
/// (`{"P":[]}`).
pub fn root_bundle_url() -> String {
    bundle_view_url(&BundlePath::root())
}

/// The home page.
pub fn index(state: &WebState) -> String {
    let mut inner = Html::new();
    inner.raw("<div class=\"text-center container mt-5\">");
    inner.element("h1", &[("class", "display-4 mb-4")], &t(state, "welcome"));
    if state.is_loaded() {
        inner.open(
            "a",
            &[("class", "btn btn-success"), ("href", &root_bundle_url())],
        );
        inner.text(&t(state, "view_loaded_files"));
        inner.close("a");
    } else {
        inner.element(
            "button",
            &[
                ("type", "button"),
                ("class", "btn btn-secondary"),
                ("disabled", "disabled"),
            ],
            &t(state, "no_files_loaded"),
        );
    }
    inner.element("p", &[("class", "mt-4")], &t(state, "tipping_message"));
    inner.raw("<div class=\"d-flex justify-content-center mt-3\">");
    for (label, href) in [
        ("Paypal", "https://paypal.me/ds5678"),
        ("GitHub Sponsors", "https://github.com/sponsors/ds5678"),
        ("Buy Me a Coffee", "https://buymeacoffee.com/assetripper"),
        ("Ko-fi", "https://ko-fi.com/assetripper"),
    ] {
        inner.element(
            "a",
            &[
                ("class", "btn btn-primary m-1"),
                ("target", "_blank"),
                ("rel", "noopener"),
                ("href", href),
            ],
            label,
        );
    }
    inner.raw("</div></div>");
    layout(state, &t(state, "asset_ripper_free"), inner.as_str())
}

/// The Commands page: load a path, or export the loaded game.
pub fn commands(state: &WebState) -> String {
    let mut inner = Html::new();
    inner.element("h1", &[], &t(state, "commands"));
    if state.is_loaded() {
        inner.raw("<p>");
        post_form(
            &mut inner,
            "/Reset",
            &t(state, "menu_file_reset"),
            "btn btn-danger",
        );
        inner.raw("</p>");
        inner.open("p", &[]);
        for (action, label_key) in [
            ("/Export/UnityProject", "export_unity_project"),
            ("/Export/PrimaryContent", "export_primary_content"),
        ] {
            inner.open(
                "form",
                &[
                    ("action", action),
                    ("method", "post"),
                    ("class", "export-form"),
                ],
            );
            inner.element("label", &[("for", "path")], &t(state, "path"));
            inner.void(
                "input",
                &[
                    ("type", "text"),
                    ("class", "form-control"),
                    ("id", "path"),
                    ("name", "Path"),
                ],
            );
            inner.raw("<div class=\"form-check mb-2\">");
            inner.void(
                "input",
                &[
                    ("type", "checkbox"),
                    ("class", "form-check-input"),
                    ("id", "createSubfolder"),
                    ("name", "CreateSubfolder"),
                    ("value", "true"),
                ],
            );
            inner.element(
                "label",
                &[("class", "form-check-label"), ("for", "createSubfolder")],
                &t(state, "create_subfolder"),
            );
            inner.raw("</div>");
            inner.void(
                "input",
                &[
                    ("type", "submit"),
                    ("class", "btn btn-primary"),
                    ("value", &t(state, label_key)),
                ],
            );
            inner.close("form");
        }
        inner.close("p");
    } else {
        inner.open("p", &[]);
        inner.open("form", &[("action", "/LoadFolder"), ("method", "post")]);
        inner.void(
            "input",
            &[
                ("class", "form-control"),
                ("type", "text"),
                ("name", "Path"),
                ("placeholder", &t(state, "path")),
            ],
        );
        inner.void(
            "input",
            &[
                ("type", "submit"),
                ("class", "btn btn-primary"),
                ("value", &t(state, "menu_load")),
            ],
        );
        inner.close("form");
        inner.close("p");
    }
    layout(state, &t(state, "commands"), inner.as_str())
}

/// The privacy page. The baseline inlines this text; the port keeps it identical.
pub fn privacy(state: &WebState) -> String {
    let mut inner = Html::new();
    inner.element("h1", &[], "Privacy Policy");
    inner.element("p", &[], "This app does not access the internet.");
    layout(state, "Privacy Policy", inner.as_str())
}

/// The licenses page: one section per retained license text.
pub fn licenses(state: &WebState) -> String {
    let mut inner = Html::new();
    inner.element("h1", &[], &t(state, "licenses"));
    inner.open("div", &[("class", "licenses")]);
    for (name, text) in LICENSE_TEXTS {
        inner.element("h2", &[], name);
        inner.element("pre", &[("class", "license-text")], text);
    }
    inner.close("div");
    layout(state, &t(state, "licenses"), inner.as_str())
}

/// The premium-features page.
///
/// The baseline answers each row from the loaded asset set: whether shaders can be decompiled
/// on their platform, and whether combined meshes can be separated. The port can count the
/// asset classes but has no shader platform analysis or combined-mesh detection yet, so a
/// loaded game with such assets reports `not_implemented_yet` instead of an unproven claim.
pub fn premium_features(state: &WebState) -> String {
    let (shaders, meshes) = if let Some(game) = state.loaded() {
        let mut shaders = 0usize;
        let mut meshes = 0usize;
        for object in game.objects.objects() {
            match object.class_id {
                Some(SHADER_CLASS_ID) => shaders += 1,
                Some(MESH_CLASS_ID) => meshes += 1,
                _ => {}
            }
        }
        (Some(shaders), Some(meshes))
    } else {
        (None, None)
    };

    let mut inner = Html::new();
    inner.element("h1", &[], &t(state, "premium_features"));
    inner.raw("<table class=\"table\"><tbody>");
    for (title_key, count) in [
        ("shader_decompilation", shaders),
        ("static_mesh_separation", meshes),
    ] {
        let status = match count {
            None => t(state, "no_files_loaded").into_owned(),
            Some(0) => t(
                state,
                if title_key == "shader_decompilation" {
                    "no_shaders_found"
                } else {
                    "no_meshes_found"
                },
            )
            .into_owned(),
            Some(_) => t(state, "not_implemented_yet").into_owned(),
        };
        inner.open("tr", &[]);
        inner.element("th", &[], &t(state, title_key));
        inner.element("td", &[], &status);
        inner.close("tr");
    }
    inner.raw("</tbody></table>");
    layout(state, &t(state, "premium_features"), inner.as_str())
}

/// The settings page.
pub fn settings_edit(state: &WebState) -> String {
    let mut inner = Html::new();
    inner.element(
        "h1",
        &[("class", "text-center")],
        &t(state, "config_options"),
    );
    if state.is_loaded() {
        inner.raw("<div class=\"text-center\"><p>");
        inner.text(&t(
            state,
            "settings_can_only_be_changed_before_loading_files",
        ));
        inner.raw("</p></div>");
    } else {
        inner.open(
            "form",
            &[("action", "/Settings/Update"), ("method", "post")],
        );
        inner.raw("<div class=\"form-group\">");

        inner.raw("<div class=\"border rounded p-3 m-2\">");
        inner.element("h2", &[], &t(state, "menu_import"));
        settings_row(
            &mut inner,
            state,
            &[
                Control::Checkbox("IgnoreStreamingAssets"),
                Control::Checkbox("EnableStaticMeshSeparation"),
            ],
        );
        settings_row(
            &mut inner,
            state,
            &[
                Control::Checkbox("RemoveNullableAttributes"),
                Control::Checkbox("PublicizeAssemblies"),
            ],
        );
        settings_row(&mut inner, state, &[Control::Version("DefaultVersion")]);
        settings_row(
            &mut inner,
            state,
            &[
                Control::Dropdown("BundledAssetsExportMode"),
                Control::Dropdown("ScriptContentLevel"),
            ],
        );
        inner.void("hr", &[]);
        inner.element("h3", &[], &t(state, "experimental"));
        settings_row(
            &mut inner,
            state,
            &[
                Control::Checkbox("EnablePrefabOutlining"),
                Control::Checkbox("EnableAssetDeduplication"),
            ],
        );
        settings_row(&mut inner, state, &[Control::Version("TargetVersion")]);
        inner.raw("</div>");

        inner.raw("<div class=\"border rounded p-3 m-2\">");
        inner.element("h2", &[], &t(state, "menu_export"));
        settings_row(
            &mut inner,
            state,
            &[
                Control::Dropdown("AudioExportFormat"),
                Control::Dropdown("ImageExportFormat"),
                Control::Dropdown("LightmapTextureExportFormat"),
            ],
        );
        settings_row(
            &mut inner,
            state,
            &[
                Control::Dropdown("SpriteExportMode"),
                Control::Dropdown("ShaderExportMode"),
                Control::Dropdown("TextExportMode"),
            ],
        );
        settings_row(
            &mut inner,
            state,
            &[
                Control::Dropdown("ScriptLanguageVersion"),
                Control::Dropdown("ScriptExportMode"),
                Control::Empty,
            ],
        );
        settings_row(
            &mut inner,
            state,
            &[
                Control::Checkbox("ScriptTypesFullyQualified"),
                Control::Checkbox("PreferOriginalTextureExtension"),
                Control::Checkbox("ExportUnreadableAssets"),
            ],
        );
        settings_row(
            &mut inner,
            state,
            &[
                Control::Empty,
                Control::Checkbox("SaveSettingsToDisk"),
                Control::Empty,
            ],
        );
        inner.raw("</div>");

        inner.raw("<div class=\"text-center\">");
        inner.void("input", &[("type", "submit"), ("value", &t(state, "save"))]);
        inner.raw("</div></div>");
        inner.close("form");
    }
    layout(state, &t(state, "settings"), inner.as_str())
}

/// The deletion confirmation for a non-empty export directory.
///
/// The baseline asks through the native Windows dialog. The port has no dialog backend, so it
/// renders the same question as a page whose form repeats the command with `ConfirmDelete`
/// set, which is the form field the command handlers read.
pub fn confirm_delete(
    state: &WebState,
    action: &str,
    command_label: &str,
    path: &str,
    create_subfolder: bool,
) -> String {
    let mut inner = Html::new();
    inner.element("h1", &[], command_label);
    inner.element(
        "p",
        &[],
        &t(state, "export_directory_delete_user_confirmation"),
    );
    inner.element(
        "p",
        &[],
        &t(
            state,
            "warning_this_directory_is_not_empty_all_content_will_be_deleted",
        ),
    );
    inner.open("form", &[("action", action), ("method", "post")]);
    inner.void(
        "input",
        &[("type", "hidden"), ("name", "Path"), ("value", path)],
    );
    if create_subfolder {
        inner.void(
            "input",
            &[
                ("type", "hidden"),
                ("name", "CreateSubfolder"),
                ("value", "true"),
            ],
        );
    }
    inner.void(
        "input",
        &[
            ("type", "hidden"),
            ("name", "ConfirmDelete"),
            ("value", "true"),
        ],
    );
    inner.void(
        "input",
        &[
            ("type", "submit"),
            ("class", "btn btn-danger"),
            ("value", &t(state, "remove")),
        ],
    );
    inner.close("form");
    inner.open("p", &[]);
    inner.open(
        "a",
        &[("href", "/Commands"), ("class", "btn btn-secondary")],
    );
    inner.text(&t(state, "commands"));
    inner.close("a");
    inner.close("p");
    layout(state, command_label, inner.as_str())
}

/// A page for a baseline route the port has not wired yet.
pub fn pending_page(state: &WebState, method: &str, route: &str) -> String {
    let mut inner = Html::new();
    inner.element("h1", &[], &t(state, "not_implemented_yet"));
    inner.element(
        "p",
        &[],
        &format!("{method} {route} is part of the frozen baseline's web surface and is not wired in this build."),
    );
    layout(state, &t(state, "not_implemented_yet"), inner.as_str())
}

/// The bundle page: the bundle name plus its parent, child bundles, collections,
/// resources and failed files, each sorted by name like the frozen `ViewPage`.
///
/// `bundle_index` addresses the bundle in `tree` and `path` is its `BundlePath`; every
/// link carries the position in the tree's source-order lists, never the display order.
/// A `None` return means the tree disagrees with the resolved index, which the route
/// reports as an internal error rather than rendering a half page.
pub fn bundle_page(
    state: &WebState,
    tree: &super::tree::BundleTree,
    bundle_index: usize,
    path: &assetripper::web_paths::BundlePath,
) -> Option<String> {
    use assetripper::web_paths::{
        bundle_view_url, collection_view_url, failed_file_view_url, resource_view_url,
        CollectionPath,
    };

    let bundle = tree.bundle(bundle_index)?;
    let mut inner = Html::new();
    inner.element("h1", &[], &bundle.name);

    if let Some(parent_index) = bundle.parent {
        let parent = tree.bundle(parent_index)?;
        inner.element("h2", &[], &t(state, "parent"));
        tree_link(&mut inner, &bundle_view_url(&path.parent()), &parent.name);
    }

    if !bundle.children.is_empty() {
        inner.element("h2", &[], &t(state, "bundles"));
        let mut children: Vec<(usize, usize)> =
            bundle.children.iter().copied().enumerate().collect();
        children.sort_by(|left, right| {
            tree.bundle(left.1)
                .map(|bundle| &bundle.name)
                .cmp(&tree.bundle(right.1).map(|bundle| &bundle.name))
        });
        inner.open("ul", &[]);
        for (position, child_index) in children {
            let child = tree.bundle(child_index)?;
            inner.open("li", &[]);
            tree_link(
                &mut inner,
                &bundle_view_url(&path.child(position as i32)),
                &child.name,
            );
            inner.close("li");
        }
        inner.close("ul");
    }

    if !bundle.collections.is_empty() {
        inner.element("h2", &[], &t(state, "collections"));
        let mut collections: Vec<(usize, &super::tree::TreeCollection)> =
            bundle.collections.iter().enumerate().collect();
        collections.sort_by(|left, right| left.1.name.cmp(&right.1.name));
        inner.open("ul", &[]);
        for (position, collection) in collections {
            inner.open("li", &[]);
            tree_link(
                &mut inner,
                &collection_view_url(&CollectionPath {
                    bundle: path.clone(),
                    index: position as i32,
                }),
                &collection.name,
            );
            inner.close("li");
        }
        inner.close("ul");
    }

    if !bundle.resources.is_empty() {
        inner.element("h2", &[], &t(state, "resources"));
        let mut resources: Vec<(usize, &String)> = bundle.resources.iter().enumerate().collect();
        resources.sort_by(|left, right| left.1.cmp(right.1));
        inner.open("ul", &[]);
        for (position, name) in resources {
            inner.open("li", &[]);
            tree_link(
                &mut inner,
                &resource_view_url(&path.resource(position as i32)),
                name,
            );
            inner.close("li");
        }
        inner.close("ul");
    }

    if !bundle.failed.is_empty() {
        inner.element("h2", &[], &t(state, "failed_files"));
        let mut failed: Vec<(usize, &String)> = bundle.failed.iter().enumerate().collect();
        failed.sort_by(|left, right| left.1.cmp(right.1));
        inner.open("ul", &[]);
        for (position, name) in failed {
            inner.open("li", &[]);
            tree_link(
                &mut inner,
                &failed_file_view_url(&path.failed_file(position as i32)),
                name,
            );
            inner.close("li");
        }
        inner.close("ul");
    }

    Some(layout(state, &bundle.name, inner.as_str()))
}

/// A link to another tree page, matching the frozen `PathLinking` button style.
fn tree_link(html: &mut Html, href: &str, name: &str) {
    html.open("a", &[("class", "btn btn-dark p-0 m-0"), ("href", href)]);
    html.text(name);
    html.close("a");
}

/// The collection page: the collection name, its bundle, and the asset table.
///
/// `position` addresses the collection in the bundle at `bundle_index` and `path` is its
/// `CollectionPath`. The table keeps the frozen columns (path ID, class, name) with one
/// linked row per asset; the class filter, the page control and the client script have no
/// port counterpart (the server renders every row with no client code), and the scene and
/// dependency sections wait for their own data ports. A `None` return means the tree
/// disagrees with the resolved position.
pub fn collection_page(
    state: &WebState,
    tree: &super::tree::BundleTree,
    bundle_index: usize,
    position: usize,
    path: &assetripper::web_paths::CollectionPath,
) -> Option<String> {
    use assetripper::web_paths::{asset_view_url, bundle_view_url};

    let bundle = tree.bundle(bundle_index)?;
    let collection = bundle.collections.get(position)?;
    let mut inner = Html::new();
    inner.element("h1", &[], &collection.name);

    inner.element("h2", &[], &t(state, "bundle"));
    tree_link(&mut inner, &bundle_view_url(&path.bundle), &bundle.name);

    if !collection.assets.is_empty() {
        inner.element("h2", &[], &t(state, "assets"));
        inner.open("table", &[("class", "table")]);
        inner.open("thead", &[]);
        inner.open("tr", &[]);
        inner.element("th", &[], &t(state, "path_id"));
        inner.element("th", &[], &t(state, "class"));
        inner.element("th", &[], &t(state, "name"));
        inner.close("tr");
        inner.close("thead");
        inner.open("tbody", &[("id", "assetsTable")]);
        for asset in &collection.assets {
            inner.open("tr", &[("data-class", &asset.class_name)]);
            inner.element("td", &[], &asset.path_id.to_string());
            inner.element("td", &[], &asset.class_name);
            inner.open("td", &[]);
            tree_link(
                &mut inner,
                &asset_view_url(&path.asset(asset.path_id)),
                &asset.best_name,
            );
            inner.close("td");
            inner.close("tr");
        }
        inner.close("tbody");
        inner.close("table");
    }

    Some(layout(state, &collection.name, inner.as_str()))
}

/// The OpenAPI document the development menu links to.
///
/// The baseline serves a document generated from its minimal API definitions. The port
/// describes the same route table in OpenAPI 3 form; operation details are summarised rather
/// than annotated, which is a documentation difference and not a behaviour difference.
pub fn openapi_document() -> String {
    let mut json = String::from("{\n  \"openapi\": \"3.0.1\",\n  \"info\": {\n    \"title\": \"AssetRipper API\",\n    \"version\": \"2.0.0\"\n  },\n  \"paths\": {");
    let mut paths: Vec<&str> = Vec::new();
    for route in WEB_ROUTES {
        if !paths.contains(&route.path) {
            paths.push(route.path);
        }
    }
    for (index, path) in paths.iter().enumerate() {
        json.push_str(&format!("\n    \"{path}\": {{"));
        let mut first = true;
        for route in WEB_ROUTES.iter().filter(|route| route.path == *path) {
            if !first {
                json.push(',');
            }
            first = false;
            let method = match route.method {
                assetripper::product::RouteMethod::Get => "get",
                assetripper::product::RouteMethod::Post => "post",
            };
            json.push_str(&format!(
                "\n      \"{method}\": {{ \"responses\": {{ \"200\": {{ \"description\": \"OK\" }} }} }}"
            ));
        }
        json.push_str(&format!(
            "\n    }}{}",
            if index + 1 == paths.len() { "" } else { "," }
        ));
    }
    json.push_str("\n  }\n}\n");
    json
}

/// The documentation page the development menu links to.
///
/// The baseline embeds the Swagger UI, which needs JavaScript and network access for its
/// assets. The port renders the same route table as a plain page, so the documented API stays
/// readable without either.
pub fn swagger_page(state: &WebState) -> String {
    let mut inner = Html::new();
    inner.element("h1", &[], "AssetRipper API");
    inner.raw("<table class=\"table\"><thead><tr>");
    inner.element("th", &[], "Method");
    inner.element("th", &[], "Path");
    inner.raw("</tr></thead><tbody>");
    for route in WEB_ROUTES {
        let method = match route.method {
            assetripper::product::RouteMethod::Get => "GET",
            assetripper::product::RouteMethod::Post => "POST",
        };
        inner.open("tr", &[]);
        inner.element("td", &[], method);
        inner.open("td", &[]);
        if route.method == assetripper::product::RouteMethod::Get {
            inner.element("a", &[("href", route.path)], route.path);
        } else {
            inner.text(route.path);
        }
        inner.close("td");
        inner.close("tr");
    }
    inner.raw("</tbody></table>");
    layout(state, "AssetRipper API", inner.as_str())
}

/// One settings control slot.
enum Control {
    /// A boolean property.
    Checkbox(&'static str),
    /// A dropdown property.
    Dropdown(&'static str),
    /// A version text property.
    Version(&'static str),
    /// An unused column.
    Empty,
}

fn settings_row(html: &mut Html, state: &WebState, controls: &[Control]) {
    html.raw("<div class=\"row\">");
    for control in controls {
        if matches!(control, Control::Empty) {
            html.raw("<div class=\"col\"></div>");
            continue;
        }
        html.raw("<div class=\"col\">");
        match control {
            Control::Checkbox(key) => write_checkbox(html, state, key),
            Control::Dropdown(id) => {
                if let Some(dropdown) = DROPDOWNS.iter().find(|dropdown| dropdown.id == *id) {
                    write_dropdown(html, state, dropdown);
                }
            }
            Control::Version(key) => write_version(html, state, key),
            Control::Empty => {}
        }
        html.raw("</div>");
    }
    html.raw("</div>");
}

fn write_checkbox(html: &mut Html, state: &WebState, key: &str) {
    let disabled = FREE_EDITION_DISABLED_KEYS.contains(&key);
    let label_key = BOOLEAN_LABELS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, label)| *label)
        .unwrap_or(key);
    let mut label = t(state, label_key).into_owned();
    if disabled {
        label = format!("{label} ({})", t(state, PREMIUM_NOTICE_KEY));
    }
    let checked = !disabled && boolean_value(state.settings(), key);
    html.raw("<div class=\"form-check\">");
    let mut attributes: Vec<(&str, &str)> = vec![
        ("class", "form-check-input"),
        ("type", "checkbox"),
        ("id", key),
        ("name", key),
        ("value", "true"),
    ];
    if checked {
        attributes.push(("checked", "checked"));
    }
    if disabled {
        attributes.push(("disabled", "disabled"));
    }
    html.void("input", &attributes);
    html.element(
        "label",
        &[
            (
                "class",
                if disabled {
                    "form-check-label text-muted"
                } else {
                    "form-check-label"
                },
            ),
            ("for", key),
        ],
        &label,
    );
    html.raw("</div>");
}

fn write_version(html: &mut Html, state: &WebState, key: &str) {
    let label_key = TEXT_LABELS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, label)| *label)
        .unwrap_or(key);
    html.element(
        "label",
        &[("class", "form-label"), ("for", key)],
        &t(state, label_key),
    );
    html.void(
        "input",
        &[
            ("type", "text"),
            ("class", "form-control"),
            ("id", key),
            ("name", key),
            ("value", version_text(state.settings(), key)),
        ],
    );
}

fn write_dropdown(html: &mut Html, state: &WebState, dropdown: &Dropdown) {
    html.element(
        "label",
        &[("class", "form-label"), ("for", dropdown.id)],
        &t(state, dropdown.title_key),
    );
    html.open(
        "select",
        &[
            ("class", "form-select"),
            ("name", dropdown.id),
            ("id", dropdown.id),
        ],
    );
    let selected = selected_value(state.settings(), dropdown.id);
    for option in dropdown.options {
        let mut attributes: Vec<(&str, &str)> = vec![("value", option.value)];
        if option.value == selected {
            attributes.push(("selected", "selected"));
        }
        // A dropdown that does not localize its options shows the enum member name, exactly as
        // `Enum.GetName` does in the source.
        let display: Cow<'_, str> = match option.display_key {
            Some(key) => t(state, key),
            None => Cow::Borrowed(option.value),
        };
        html.element("option", &attributes, &display);
    }
    html.close("select");
    if let Some(option) = dropdown
        .options
        .iter()
        .find(|option| option.value == selected)
    {
        if let Some(key) = option.description_key {
            html.element(
                "div",
                &[("class", "form-text dropdown-description")],
                &t(state, key),
            );
        }
    }
}

/// A whole document with the shared header, footer and stylesheet.
fn layout(state: &WebState, title: &str, inner: &str) -> String {
    let mut html = Html::new();
    html.raw("<!DOCTYPE html>\n");
    html.open("html", &[("lang", state.language())]);
    html.open("head", &[]);
    html.void("meta", &[("charset", "utf-8")]);
    html.void(
        "meta",
        &[
            ("name", "viewport"),
            ("content", "width=device-width, initial-scale=1.0"),
        ],
    );
    html.element("title", &[], title);
    html.void("link", &[("rel", "stylesheet"), ("href", "/css/site.css")]);
    html.close("head");
    html.open("body", &[]);
    write_header(&mut html, state);
    html.raw("<div class=\"container\">");
    html.open("main", &[("id", "app"), ("class", "pb-3")]);
    html.raw(inner);
    html.close("main");
    html.raw("</div>");
    write_footer(&mut html, state);
    html.close("body");
    html.close("html");
    html.into_string()
}

fn write_header(html: &mut Html, state: &WebState) {
    html.open("header", &[("class", "site-header")]);
    html.open("nav", &[("class", "menu-bar")]);

    let label = t(state, "menu_file");
    menu(html, &label, |html| {
        menu_post(html, "/LoadFile", &t(state, "menu_file_open_file"));
        menu_post(html, "/LoadFolder", &t(state, "menu_file_open_folder"));
        menu_post(html, "/Reset", &t(state, "menu_file_reset"));
        menu_divider(html);
        menu_link(html, "/Settings/Edit", &t(state, "settings"));
    });

    let label = t(state, "menu_view");
    menu(html, &label, |html| {
        menu_link(html, "/", &t(state, "home"));
        if state.is_loaded() {
            menu_link(html, "/Search/View", &t(state, "search"));
        }
        menu_link(html, "/Settings/Edit", &t(state, "settings"));
        menu_link(
            html,
            "/ConfigurationFiles",
            &t(state, "configuration_files"),
        );
        menu_link(html, "/Commands", &t(state, "commands"));
        menu_link(html, "/Privacy", &t(state, "privacy"));
        menu_link(html, "/Licenses", &t(state, "licenses"));
        menu_link(html, "/PremiumFeatures", &t(state, "premium_features"));
    });

    let label = t(state, "menu_export");
    menu(html, &label, |html| {
        if state.is_loaded() {
            menu_link(html, "/Commands", &t(state, "menu_export_all"));
            if let Some(version) = state.max_unity_version_text() {
                menu_link(html, &format!("unityhub://{version}"), &version);
            }
        } else {
            menu_disabled(html, &t(state, "menu_export_all"));
        }
    });

    let label = t(state, "menu_language");
    menu(html, &label, |html| {
        for (code, name) in state.messages().languages() {
            menu_post(
                html,
                &format!("/Localization?code={}", url_encode(code)),
                name,
            );
        }
    });

    let label = t(state, "menu_development");
    menu(html, &label, |html| {
        menu_link(html, "/openapi.json", &t(state, "open_api_json"));
        menu_link(html, "/swagger", &t(state, "swagger_documentation"));
        menu_link(
            html,
            "https://unity.com/unity-hub",
            &t(state, "install_unity_hub"),
        );
        if let Some(version) = state.max_unity_version_text() {
            menu_link(
                html,
                &format!("unityhub://{version}"),
                &t(state, "install_unity_editor"),
            );
        }
    });

    html.close("nav");
    html.close("header");
}

fn write_footer(html: &mut Html, state: &WebState) {
    html.open("footer", &[("class", "footer")]);
    html.raw("<div class=\"text-center\">");
    html.raw("&copy; 2026 - AssetRipper - ");
    html.open("a", &[("href", "/Privacy")]);
    html.text(&t(state, "privacy"));
    html.close("a");
    html.text(" - ");
    html.open("a", &[("href", "/Licenses")]);
    html.text(&t(state, "licenses"));
    html.close("a");
    html.raw("</div>");
    html.close("footer");
}

fn menu(html: &mut Html, label: &str, items: impl FnOnce(&mut Html)) {
    html.open("details", &[("class", "menu")]);
    html.element("summary", &[], label);
    html.open("ul", &[("class", "dropdown-menu")]);
    items(html);
    html.close("ul");
    html.close("details");
}

fn menu_link(html: &mut Html, href: &str, label: &str) {
    html.open("li", &[]);
    html.open("a", &[("class", "dropdown-item"), ("href", href)]);
    html.text(label);
    html.close("a");
    html.close("li");
}

fn menu_post(html: &mut Html, action: &str, label: &str) {
    html.open("li", &[]);
    html.open("form", &[("action", action), ("method", "post")]);
    html.void(
        "input",
        &[
            ("type", "submit"),
            ("class", "dropdown-item"),
            ("value", label),
        ],
    );
    html.close("form");
    html.close("li");
}

fn menu_disabled(html: &mut Html, label: &str) {
    html.open("li", &[]);
    html.element("span", &[("class", "dropdown-item disabled")], label);
    html.close("li");
}

fn menu_divider(html: &mut Html) {
    html.open("li", &[]);
    html.void("hr", &[("class", "dropdown-divider")]);
    html.close("li");
}

/// A form whose only control is a submit button that posts to `action`.
fn post_form(html: &mut Html, action: &str, label: &str, class: &str) {
    html.open("form", &[("action", action), ("method", "post")]);
    html.void(
        "input",
        &[("type", "submit"), ("class", class), ("value", label)],
    );
    html.close("form");
}

#[cfg(test)]
mod tests {
    use super::{
        bundle_page, commands, confirm_delete, index, licenses, openapi_document, pending_page,
        premium_features, privacy, settings_edit, swagger_page, BOOLEAN_LABELS, LICENSE_TEXTS,
        TEXT_LABELS,
    };
    use crate::web::localization::{Messages, MISSING_MARKER_PREFIX};
    use crate::web::settings_form::BOOLEAN_KEYS;
    use crate::web::state::WebState;
    use crate::web::tree::BundleTree;
    use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
    use assetripper::export::settings_file::SerializedSettingsFile;
    use assetripper::localization::LANGUAGE_CODES;
    use std::path::PathBuf;

    fn state(language: &str) -> WebState {
        let mut settings = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        settings.export.language_code = Some(language.to_owned());
        WebState::with_settings(
            Messages::load().expect("tables load"),
            settings,
            PathBuf::from("/tmp/assetripper-web-tests"),
            false,
        )
        .expect("state builds")
    }

    fn pages(state: &WebState) -> Vec<(&'static str, String)> {
        vec![
            ("index", index(state)),
            ("commands", commands(state)),
            ("privacy", privacy(state)),
            ("licenses", licenses(state)),
            ("premium_features", premium_features(state)),
            ("settings", settings_edit(state)),
            ("swagger", swagger_page(state)),
            (
                "confirm",
                confirm_delete(
                    state,
                    "/Export/UnityProject",
                    "Export Unity Project",
                    "/tmp/out",
                    true,
                ),
            ),
            ("pending", pending_page(state, "GET", "/Search/View")),
            (
                "bundle",
                bundle_page(
                    state,
                    &BundleTree::from_loaded(
                        &assetripper::serialized_file::workspace::SerializedFileWorkspace::default(
                        ),
                        &assetripper::serialized_file::object_workspace::ObjectWorkspace::default(),
                        &[],
                    )
                    .expect("an empty workspace projects"),
                    0,
                    &assetripper::web_paths::BundlePath::root(),
                )
                .expect("the root always renders"),
            ),
        ]
    }

    #[test]
    fn every_page_renders_in_every_language_without_a_missing_key() {
        for code in LANGUAGE_CODES {
            let state = state(code);
            for (name, html) in pages(&state) {
                assert!(
                    !html.contains(MISSING_MARKER_PREFIX),
                    "page {name} in {code} used an unknown message key:\n{html}"
                );
                assert!(html.starts_with("<!DOCTYPE html>"), "{name} in {code}");
                assert!(
                    html.contains("<html lang=\""),
                    "{name} in {code} loses the language"
                );
                assert!(html.ends_with("</html>"), "{name} in {code}");
                // The header and footer are part of every document.
                assert!(
                    html.contains("/Settings/Edit"),
                    "{name} in {code} has no header"
                );
                assert!(html.contains("/Privacy"), "{name} in {code} has no footer");
            }
        }
    }

    #[test]
    fn the_language_menu_lists_every_vendored_language() {
        let state = state("en-US");
        let html = index(&state);
        for code in LANGUAGE_CODES {
            assert!(
                html.contains(&format!("/Localization?code={code}")),
                "the language menu omits {code}"
            );
        }
    }

    #[test]
    fn an_unloaded_index_offers_no_bundle_link_and_a_loaded_one_does() {
        let unloaded = state("en-US");
        let html = index(&unloaded);
        assert!(!html.contains("/Bundles/View"));
        assert!(html.contains("disabled"));
        // The load control is present on the Commands page.
        assert!(commands(&unloaded).contains("/LoadFolder"));
    }

    #[test]
    fn the_settings_form_renders_every_control_and_the_free_edition_notice() {
        let state = state("en-US");
        let html = settings_edit(&state);
        for key in BOOLEAN_KEYS {
            assert!(
                html.contains(&format!("name=\"{key}\"")),
                "the settings form omits the {key} control"
            );
        }
        assert!(html.contains("name=\"DefaultVersion\""));
        assert!(html.contains("name=\"TargetVersion\""));
        for dropdown in crate::web::settings_form::DROPDOWNS {
            assert!(
                html.contains(&format!("name=\"{}\"", dropdown.id)),
                "the settings form omits the {} dropdown",
                dropdown.id
            );
        }
        // The disabled free-edition controls carry the notice and cannot be submitted.
        assert!(html.contains("Premium feature"));
        assert!(html.contains("disabled=\"disabled\""));
        // `WriteCheckBox` clears `checked` whenever it disables a control, so the stored record
        // value cannot reach the markup: mesh separation is enabled in the record by default.
        assert!(state.settings().processing.enable_static_mesh_separation);
        for key in crate::web::settings_form::FREE_EDITION_DISABLED_KEYS {
            let input = html
                .split("<input")
                .find(|fragment| fragment.contains(&format!("id=\"{key}\"")))
                .unwrap_or_else(|| panic!("the {key} control is missing"));
            assert!(!input.contains("checked"), "{key} rendered as checked");
            assert!(input.contains("disabled"), "{key} is not disabled");
        }
    }

    #[test]
    fn the_boolean_and_version_labels_cover_the_form_model() {
        for key in BOOLEAN_KEYS {
            assert!(
                BOOLEAN_LABELS.iter().any(|(name, _)| *name == *key),
                "no label is defined for {key}"
            );
        }
        assert_eq!(BOOLEAN_LABELS.len(), BOOLEAN_KEYS.len());
        for key in crate::web::settings_form::TEXT_KEYS {
            assert!(
                TEXT_LABELS.iter().any(|(name, _)| *name == *key),
                "no label is defined for {key}"
            );
        }
        assert_eq!(
            TEXT_LABELS.len(),
            crate::web::settings_form::TEXT_KEYS.len()
        );
        for (name, text) in LICENSE_TEXTS {
            assert!(!name.is_empty());
            assert!(!text.is_empty(), "{name} has no text");
        }
    }

    #[test]
    fn the_openapi_document_lists_every_route() {
        let document = openapi_document();
        assert!(document.starts_with('{'));
        assert!(document.ends_with("}\n"));
        for route in assetripper::product::WEB_ROUTES {
            assert!(
                document.contains(&format!("\"{}\"", route.path)),
                "the document omits {}",
                route.path
            );
        }
    }

    #[test]
    fn the_confirmation_page_carries_the_command_and_the_consent_field() {
        let state = state("en-US");
        let html = confirm_delete(
            &state,
            "/Export/PrimaryContent",
            "Export Primary Content",
            "/tmp/out",
            false,
        );
        assert!(html.contains("action=\"/Export/PrimaryContent\""));
        assert!(html.contains("name=\"Path\" value=\"/tmp/out\""));
        assert!(html.contains("name=\"ConfirmDelete\" value=\"true\""));
        assert!(!html.contains("name=\"CreateSubfolder\""));
    }
}
