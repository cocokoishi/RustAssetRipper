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
        let mut resources: Vec<(usize, &super::tree::TreeResource)> =
            bundle.resources.iter().enumerate().collect();
        resources.sort_by(|left, right| left.1.name.cmp(&right.1.name));
        inner.open("ul", &[]);
        for (position, resource) in resources {
            inner.open("li", &[]);
            tree_link(
                &mut inner,
                &resource_view_url(&path.resource(position as i32)),
                &resource.name,
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

/// The frozen `ViewPage.assetsPerPage` values, in source order.
const ASSETS_PER_PAGE: [u32; 5] = [500, 1000, 2000, 3000, 5000];

/// The `distinct().order(ordinal)` class list the class filter offers: one entry per distinct
/// class name, sorted by code unit.
///
/// `StringComparer.Ordinal` compares UTF-16 code units and the element class names are ASCII,
/// so Rust's byte-ordered `sort` yields the same sequence.
fn ordered_classes<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut classes: Vec<String> = names.map(str::to_owned).collect();
    classes.sort();
    classes.dedup();
    classes
}

/// The frozen class filter: a `Localization.Class` label over a `classFilter` select whose
/// empty option is `Localization.All`, followed by one option per distinct class name.
///
/// `ViewPage` and `Search.ViewPage` both render it only when the page holds more than one
/// distinct class, and both table rows carry `data-class` for the page script to filter on.
fn write_class_filter(html: &mut Html, state: &WebState, classes: &[String]) {
    html.element(
        "label",
        &[("for", "classFilter"), ("class", "me-2")],
        &t(state, "class"),
    );
    html.open("select", &[("id", "classFilter"), ("class", "me-2")]);
    html.element("option", &[("value", "")], &t(state, "all"));
    for class in classes {
        html.element("option", &[("value", class.as_str())], class);
    }
    html.close("select");
}

/// The frozen `ViewPage.CreatePageOption` block: the `Localization.Page` label, the
/// previous/next buttons around the `pageNumber` label, and the `assetsPerPage` select
/// (`Localization.All` plus the five page sizes).
///
/// The buttons start `disabled` and the page script enables them as the paging state changes.
/// The frozen buttons also carry a `v-else` directive; the port runs no Vue, so the directive
/// is dropped and the real `disabled` attribute it never reads is kept.
fn write_page_control(html: &mut Html, state: &WebState) {
    html.element(
        "label",
        &[("for", "pageCollection"), ("class", "me-2")],
        &t(state, "page"),
    );
    html.element(
        "button",
        &[
            ("id", "previousButton"),
            ("class", "ms-2"),
            ("type", "button"),
            ("disabled", ""),
        ],
        &t(state, "previous"),
    );
    html.element("label", &[("id", "pageNumber"), ("class", "ms-2")], "1");
    html.element(
        "button",
        &[
            ("id", "nextButton"),
            ("type", "button"),
            ("class", "ms-2"),
            ("disabled", ""),
        ],
        &t(state, "next"),
    );
    html.element("label", &[("class", "ms-2")], &t(state, "assets_per_page"));
    html.open("select", &[("id", "assetsPerPage"), ("class", "ms-2")]);
    html.element("option", &[("value", "")], &t(state, "all"));
    for count in ASSETS_PER_PAGE {
        let text = count.to_string();
        html.element("option", &[("value", text.as_str())], &text);
    }
    html.close("select");
}

/// The collection page's client script, transcribed from `ViewPage.WriteScriptReferences`.
///
/// It filters `#assetsTable` rows by `data-class` and pages them with the `assetsPerPage`
/// value, keeping the page number, the previous/next disabled state and the two selects in
/// step. The baseline registers it on every response after its framework bundles; the port
/// emits it with the table it drives, the only state where it has any effect.
const ASSETS_TABLE_SCRIPT: &str = r##"document.addEventListener('DOMContentLoaded', function() {
	const classFilter = document.getElementById('classFilter');
	if (classFilter) {
		classFilter.addEventListener('change', function() {
			const selectedClass = this.value;
			const rows = document.querySelectorAll('#assetsTable tr');

			rows.forEach(function(row) {
				const rowClass = row.getAttribute('data-class');
				if (selectedClass === '' || rowClass === selectedClass) {
					row.style.display = '';
				} else {
					row.style.display = 'none';
				}
			});

			if (pageFilter) {
				var event = new Event('change');
				pageFilter.dispatchEvent(event);
			}
		});
	}

	const pageFilter = document.getElementById('assetsPerPage');
	const previousButton = document.getElementById('previousButton');
	const nextButton = document.getElementById('nextButton');
	if (pageFilter) {
		pageFilter.addEventListener('change', function() {
			var selectedClass = "";
			var query = '#assetsTable tr';
			if (classFilter && classFilter.value != '') {
				selectedClass = classFilter.value;
				query += '[data-class="' + selectedClass + '"]';
			}

			var pageNumber = parseInt(document.getElementById('pageNumber').innerHTML);
			const rows = document.querySelectorAll(query);

			const selected = this.value;
			if (selected == '') {
				for (var i = 0; i < rows.length; ++i) {
					rows[i].style.display = '';
				}

				if (previousButton) {
					previousButton.disabled = true;
				}

				if (nextButton) {
					nextButton.disabled = true;
				}

				document.getElementById('pageNumber').innerHTML = '1';
			} else {
				const selectedCount = parseInt(this.value);

				if (rows.length < selectedCount * pageNumber) {
					pageNumber = Math.ceil(rows.length / selectedCount);
					document.getElementById('pageNumber').innerHTML = '' + pageNumber;

					if (nextButton) {
						nextButton.disabled = true;
					}

					if (pageNumber == 1) {
						if (previousButton) {
							previousButton.disabled = true;
						}
					}
				} else if (rows.length >= selectedCount * pageNumber) {
					if (nextButton) {
						nextButton.disabled = false;
					}
				}

				const start = selectedCount * (pageNumber - 1);
				for (var i = 0; i < rows.length; ++i) {
					if (i >= start && i < selectedCount * pageNumber) {
						rows[i].style.display = '';
					} else {
						rows[i].style.display = 'none';
					}
				}
			}
		});
	}

	if (previousButton) {
		previousButton.addEventListener('click', function() {
			var pageNumber = parseInt(document.getElementById('pageNumber').innerHTML);
			--pageNumber;
			document.getElementById('pageNumber').innerHTML = "" + pageNumber;

			if (pageNumber == 1) {
				this.disabled = true;
			}

			if (nextButton) {
				nextButton.disabled = false;
			}

			if (pageFilter) {
				var event = new Event('change');
				pageFilter.dispatchEvent(event);
			}
		});
	}

	if (nextButton) {
		nextButton.addEventListener('click', function() {
			var pageNumber = parseInt(document.getElementById('pageNumber').innerHTML);
			++pageNumber;
			document.getElementById('pageNumber').innerHTML = "" + pageNumber;

			if (previousButton) {
				previousButton.disabled = false;
			}

			if (pageFilter) {
				var query = '#assetsTable tr';
				if (classFilter && classFilter.value != '') {
					selectedClass = classFilter.value;
					query += '[data-class="' + selectedClass + '"]';
				}
				const rows = document.querySelectorAll(query);
				const selectedCount = parseInt(pageFilter.value);
				if (rows.length < selectedCount * pageNumber) {
					this.disabled = true;
				}

				var event = new Event('change');
				pageFilter.dispatchEvent(event);
			}
		});
	}
});"##;

/// The search page's client script, transcribed from `Search.ViewPage.WriteScriptReferences`.
///
/// The search page has no page control, so its script only filters `#resultsTable` by
/// `data-class`. Emitted with the results table for the same reason as the collection script.
const RESULTS_TABLE_SCRIPT: &str = r##"document.addEventListener('DOMContentLoaded', function() {
	const classFilter = document.getElementById('classFilter');
	if (classFilter) {
		classFilter.addEventListener('change', function() {
			const selectedClass = this.value;
			const rows = document.querySelectorAll('#resultsTable tr');

			rows.forEach(function(row) {
				const rowClass = row.getAttribute('data-class');
				if (selectedClass === '' || rowClass === selectedClass) {
					row.style.display = '';
				} else {
					row.style.display = 'none';
				}
			});
		});
	}
});"##;

/// The collection page: the collection name, its bundle, its scene link and the asset table
/// with the frozen class filter and page control.
///
/// `position` addresses the collection in the bundle at `bundle_index` and `path` is its
/// `CollectionPath`. The table keeps the frozen columns (path ID, class, name) with one linked
/// row per asset, preceded by the class filter only when more than one class is present and by
/// the page control, and followed by the page script that drives both. `scene` is the scene
/// definition's display name when the collection is a scene, so the page renders the same
/// `Localization.Scene` section and `ScenePath` link the baseline's `Collection.IsScene`
/// branch adds; it is `None` for a non-scene collection. The collection dependency section
/// still waits for its own data port. A `None` return means the tree disagrees with the
/// resolved position.
pub fn collection_page(
    state: &WebState,
    tree: &super::tree::BundleTree,
    bundle_index: usize,
    position: usize,
    path: &assetripper::web_paths::CollectionPath,
    scene: Option<&str>,
) -> Option<String> {
    use assetripper::web_paths::{asset_view_url, bundle_view_url};

    let bundle = tree.bundle(bundle_index)?;
    let collection = bundle.collections.get(position)?;
    let mut inner = Html::new();
    inner.element("h1", &[], &collection.name);

    inner.element("h2", &[], &t(state, "bundle"));
    tree_link(&mut inner, &bundle_view_url(&path.bundle), &bundle.name);

    // The baseline links a scene collection to its `ScenePath` view under a Scene heading.
    if let Some(scene_name) = scene {
        use assetripper::web_paths::{scene_view_url, ScenePath};
        inner.element("h2", &[], &t(state, "scene"));
        tree_link(
            &mut inner,
            &scene_view_url(&ScenePath::from(path.clone())),
            scene_name,
        );
    }

    if !collection.assets.is_empty() {
        inner.element("h2", &[], &t(state, "assets"));

        let classes = ordered_classes(
            collection
                .assets
                .iter()
                .map(|asset| asset.class_name.as_str()),
        );
        if classes.len() > 1 {
            write_class_filter(&mut inner, state, &classes);
        }
        write_page_control(&mut inner, state);

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

    Some(layout_with_scripts(
        state,
        &collection.name,
        inner.as_str(),
        ASSETS_TABLE_SCRIPT,
    ))
}

/// `SceneAPI.GetView`: the scene definition's name, path, GUID and collection links.
///
/// The baseline's `ViewPage` shows three rows - `Localization.Name`, `Localization.Path` and
/// `Localization.Guid` - then a `Localization.Collections` list whose entries link to each collection.
///
/// **The GUID row is a plan, not always a GUID.** The reference assigns a GUID when the definition is
/// built, generating one when the input has none; the port deliberately defers generation to export so
/// a re-export does not silently change identifiers. A scene whose input GUID is absent therefore has
/// no GUID to show, and the row says so rather than inventing one - the same honesty the export
/// pipeline keeps.
pub fn scene_page(
    state: &WebState,
    tree: &super::tree::BundleTree,
    definition: &assetripper::processing::scene_collections::WorkspaceSceneDefinition,
) -> String {
    use assetripper::processing::scene_collections::SceneDefinitionGuidPlan;

    let plan = &definition.plan;
    let mut inner = Html::new();
    inner.element("h1", &[], &plan.name);
    inner.open("table", &[("class", "table")]);
    inner.open("tbody", &[]);
    for (label, value) in [
        (t(state, "name"), plan.name.clone()),
        (t(state, "path"), plan.path.clone()),
        (
            t(state, "guid"),
            match &plan.guid {
                SceneDefinitionGuidPlan::Preserve(words) => {
                    assetripper::export::script_hashing::UnityGuid {
                        data0: words.data[0],
                        data1: words.data[1],
                        data2: words.data[2],
                        data3: words.data[3],
                    }
                    .to_unity_string()
                }
                SceneDefinitionGuidPlan::GenerateAtExport(_) => plan.guid.to_string(),
            },
        ),
    ] {
        inner.open("tr", &[]);
        inner.element("th", &[], &label);
        inner.element("td", &[], &value);
        inner.close("tr");
    }
    inner.close("tbody");
    inner.close("table");

    inner.element("h2", &[], &t(state, "collections"));
    inner.open("ul", &[]);
    for file_index in definition.collection_file_indices() {
        inner.open("li", &[]);
        match tree.collection_path_for_file(*file_index) {
            Some(path) => {
                let name = tree
                    .collection_name(*file_index)
                    .unwrap_or_else(|| file_index.to_string());
                tree_link(
                    &mut inner,
                    &assetripper::web_paths::collection_view_url(&path),
                    &name,
                );
            }
            // A definition can name a collection the snapshot does not hold; naming it is better than
            // dropping the entry, which would make the scene look like it has fewer collections.
            None => {
                inner.text(&format!("{file_index}"));
            }
        }
        inner.close("li");
    }
    inner.close("ul");

    layout(state, &plan.name, inner.as_str())
}

/// `ConfigurationFilesPage`: the named configuration texts and lists, with their actions.
///
/// The baseline renders two tabs - singletons and lists - and this page renders the same two groups
/// as sections. The interface technique differs; every action the baseline offers is here: load or
/// replace a singleton, remove a singleton, append to a list, replace a list element, remove a list
/// element.
///
/// **The file-dialog fallback is not offered.** The baseline opens a native file dialog when a form
/// carries no `Content`; the port has no dialog host, so a form submitted without content changes
/// nothing, which is what the baseline does when the dialog is cancelled or unavailable. The page
/// therefore shows the actions without pretending a picker exists.
pub fn configuration_files_page(state: &WebState) -> String {
    let mut inner = Html::new();
    inner.element("h1", &[], &t(state, "configuration_files"));

    inner.element("h2", &[], &t(state, "configuration_files_singletons"));
    if state.singletons().is_empty() {
        inner.element("p", &[], &t(state, "no_data_has_been_loaded_for_this_key"));
    }
    for key in state.singletons().keys() {
        let text = state.singletons().get(key).cloned().unwrap_or_default();
        inner.element("h3", &[], key);
        if text.is_empty() {
            inner.element("p", &[], &t(state, "no_data_has_been_loaded_for_this_key"));
        } else {
            inner.element("pre", &[("class", "bg-dark-subtle rounded-3 p-2")], &text);
        }
        // Add and Replace post to the same route, which sets the text either way.
        inner.open(
            "form",
            &[
                ("action", "/ConfigurationFiles/Singleton/Add"),
                ("method", "post"),
            ],
        );
        inner.void(
            "input",
            &[("type", "hidden"), ("name", "Key"), ("value", key)],
        );
        inner.void(
            "input",
            &[
                ("type", "submit"),
                ("class", "btn btn-primary mx-1"),
                (
                    "value",
                    &if text.is_empty() {
                        t(state, "load").into_owned()
                    } else {
                        t(state, "replace").into_owned()
                    },
                ),
            ],
        );
        inner.close("form");
        inner.open(
            "form",
            &[
                ("action", "/ConfigurationFiles/Singleton/Remove"),
                ("method", "post"),
            ],
        );
        inner.void(
            "input",
            &[("type", "hidden"), ("name", "Key"), ("value", key)],
        );
        inner.void(
            "input",
            &[
                ("type", "submit"),
                ("class", "btn btn-danger mx-1"),
                ("value", &t(state, "remove")),
            ],
        );
        inner.close("form");
    }

    inner.element("h2", &[], &t(state, "configuration_files_lists"));
    for key in state.lists().keys() {
        let values = state.lists().get(key).cloned().unwrap_or_default();
        inner.element("h3", &[], key);
        if values.is_empty() {
            inner.element("p", &[], &t(state, "no_data_has_been_loaded_for_this_key"));
        }
        for (index, value) in values.iter().enumerate() {
            inner.element("pre", &[("class", "bg-dark-subtle rounded-3 p-2")], value);
            for (action, label) in [
                ("/ConfigurationFiles/List/Replace", t(state, "replace")),
                ("/ConfigurationFiles/List/Remove", t(state, "remove")),
            ] {
                inner.open("form", &[("action", action), ("method", "post")]);
                inner.void(
                    "input",
                    &[("type", "hidden"), ("name", "Key"), ("value", key)],
                );
                inner.void(
                    "input",
                    &[
                        ("type", "hidden"),
                        ("name", "Index"),
                        ("value", &index.to_string()),
                    ],
                );
                inner.void(
                    "input",
                    &[("type", "submit"), ("class", "btn mx-1"), ("value", &label)],
                );
                inner.close("form");
            }
        }
        // The list's add form appends, which is why the baseline labels it with a plus.
        inner.open(
            "form",
            &[
                ("action", "/ConfigurationFiles/List/Add"),
                ("method", "post"),
            ],
        );
        inner.void(
            "input",
            &[("type", "hidden"), ("name", "Key"), ("value", key)],
        );
        inner.void(
            "input",
            &[
                ("type", "submit"),
                ("class", "btn btn-secondary"),
                ("value", "+"),
            ],
        );
        inner.close("form");
    }

    layout(state, &t(state, "configuration_files"), inner.as_str())
}

/// The asset page's information section: the display name with the collection link and
/// the path, class-number and class-name rows.
///
/// `bundle_index`, `position` and `row` address the resolved asset and `path` is its
/// `AssetPath`. The remaining asset tabs (audio, image, model, text, font, video, YAML,
/// JSON, hex, dependencies, development) wait for their own data ports; the page renders
/// the identity section rather than leaving the route pending. A `None` return means the
/// tree disagrees with the resolved address.
pub fn asset_page(
    state: &WebState,
    tree: &super::tree::BundleTree,
    bundle_index: usize,
    position: usize,
    row: usize,
    path: &assetripper::web_paths::AssetPath,
) -> Option<String> {
    use assetripper::web_paths::collection_view_url;

    let bundle = tree.bundle(bundle_index)?;
    let collection = bundle.collections.get(position)?;
    let asset = collection.assets.get(row)?;
    if asset.path_id != path.path_id {
        return None;
    }
    let mut inner = Html::new();
    inner.element("h1", &[], &asset.best_name);

    inner.element("h2", &[], &t(state, "asset_tab_information"));
    inner.open("table", &[("class", "table")]);
    inner.open("tbody", &[]);
    inner.open("tr", &[]);
    inner.element("th", &[], &t(state, "collection"));
    inner.open("td", &[]);
    tree_link(
        &mut inner,
        &collection_view_url(&path.collection),
        &collection.name,
    );
    inner.close("td");
    inner.close("tr");
    inner.open("tr", &[]);
    inner.element("th", &[], &t(state, "path_id"));
    inner.element("td", &[], &asset.path_id.to_string());
    inner.close("tr");
    inner.open("tr", &[]);
    inner.element("th", &[], &t(state, "class_id_type_number"));
    inner.element("td", &[], &asset.class_id.to_string());
    inner.close("tr");
    inner.open("tr", &[]);
    inner.element("th", &[], &t(state, "class_id_type_name"));
    inner.element("td", &[], &asset.class_name);
    inner.close("tr");
    inner.close("tbody");
    inner.close("table");

    if asset.has_audio {
        // The frozen audio tab renders a player plus a save button: the playback source asks for
        // WAV so any browser can play it, while the save source takes the decoder's own format.
        use assetripper::web_paths::asset_audio_url;

        inner.element("h2", &[], &t(state, "asset_tab_audio"));
        let playback_url = asset_audio_url(path, Some("wav"));
        inner.raw("<div class=\"text-center\">");
        inner.raw(&format!(
            "<audio controls=\"\" src=\"{}\" style=\"width:100%\"></audio>",
            super::html::escape(&playback_url)
        ));
        inner.raw("</div>");
        let save_url = asset_audio_url(path, None);
        // The save name uses the decoded extension when the clip decodes, else `.ogg` is a
        // reasonable placeholder that the route itself never confirms.
        let file_name = match state.read_asset_audio(asset.file_index, asset.object_index, None) {
            Ok(Some((_bytes, extension))) => format!("{}.{extension}", asset.best_name),
            _ => format!("{}.ogg", asset.best_name),
        };
        inner.raw("<div class=\"text-center\">");
        save_button(&mut inner, state, &save_url, &file_name);
        inner.raw("</div>");
    }

    if asset.has_image {
        // The frozen image tab shows the texture with a download button. Without client
        // code the port points the preview straight at the image route, so the browser
        // fetches the same encoded bytes the download button serves.
        use super::html::escape;
        use assetripper::web_paths::asset_image_url;

        inner.element("h2", &[], &t(state, "asset_tab_image"));
        let file_name = format!("{}.png", asset.best_name);
        let png_url = asset_image_url(path, Some("png"));
        inner.raw("<div class=\"text-center\">");
        inner.raw(&format!(
            "<a href=\"{}\" download=\"{}\"><img src=\"{}\" style=\"object-fit:contain; width:100%; height:100%\"></a>",
            escape(&png_url),
            escape(&file_name),
            escape(&png_url)
        ));
        inner.raw("</div>");
        inner.raw("<div class=\"text-center\">");
        save_button(&mut inner, state, &png_url, &file_name);
        inner.raw("</div>");
    }

    if asset.has_model {
        // The frozen model tab is a Babylon canvas whose `glb-data-path` attribute the page
        // script reads. The port keeps that contract: the attribute names the route, so the
        // same client script (or any consumer of the attribute) loads the GLB.
        use super::html::escape;
        use assetripper::web_paths::asset_model_url;

        inner.element("h2", &[], &t(state, "asset_tab_model"));
        let model_url = asset_model_url(path);
        inner.raw("<table width=\"100%\" height=\"100%\"><tbody><tr><td align=\"center\" valign=\"middle\">");
        inner.raw(&format!(
            "<canvas id=\"babylonRenderCanvas\" style=\"width: 100%; height: 100vh;\" glb-data-path=\"{}\"></canvas>",
            escape(&model_url)
        ));
        inner.raw("</td></tr></tbody></table>");
    }

    if asset.has_text {
        // The frozen text tab shows the asset text with a download button; without
        // client code the port inlines the text instead of fetching it from the data
        // route. Shader and MonoScript texts stay open, so only TextAsset rows qualify.
        use assetripper::export::resource_extensions::text_export_extension;
        use assetripper::web_paths::asset_text_url;

        inner.element("h2", &[], &t(state, "asset_tab_text"));
        let bytes = state
            .read_asset_text(asset.file_index, asset.object_index)
            .ok()
            .flatten()
            .filter(|script| !script.is_empty())?;
        let text = String::from_utf8_lossy(&bytes);
        inner.open("pre", &[("class", "bg-dark-subtle rounded-3 p-2")]);
        inner.text(text.as_ref());
        inner.close("pre");
        let file_name = format!(
            "{}.{}",
            asset.best_name,
            text_export_extension(None, state.settings().export.text_export_mode, &text)
        );
        inner.raw("<div class=\"text-center\">");
        save_button(&mut inner, state, &asset_text_url(path), &file_name);
        inner.raw("</div>");
    }

    if asset.has_font {
        // The frozen font tab previews the typeface with a heading styled by the font family
        // and a save button. The port keeps that shape and adds the linked preview so the
        // browser can load the served font.
        use super::html::escape;
        use assetripper::web_paths::asset_font_url;

        let Ok(Some(font)) = state.read_asset_font(asset.file_index, asset.object_index) else {
            return Some(layout(state, &asset.best_name, inner.as_str()));
        };
        let extension = font.extension;
        inner.element("h2", &[], &t(state, "asset_tab_font"));
        let font_url = asset_font_url(path);
        inner.raw("<div class=\"text-center\">");
        inner.raw(&format!(
            "<style>@font-face {{ font-family: 'asset-preview'; src: url('{}'); }}</style>",
            escape(&font_url)
        ));
        inner.raw(&format!(
            "<h1 style=\"font-family: 'asset-preview', {};\">Preview Font (0, 1, 2, 3, 4, 5, 6, 7, 8, 9)</h1>",
            escape(&asset.best_name)
        ));
        let file_name = format!("{}.{extension}", asset.best_name);
        save_button(&mut inner, state, &font_url, &file_name);
        inner.raw("</div>");
    }

    if asset.has_video {
        // The frozen video tab renders a `<video>` whose single source points at the route,
        // then a save button beneath it. The served MIME is `video/<extension>` taken from the
        // clip's original path, so the source type comes from the same read as the bytes.
        use super::html::escape;
        use assetripper::web_paths::asset_video_url;

        let Ok(Some(video)) = state.read_asset_video(asset.file_index, asset.object_index) else {
            return Some(layout(state, &asset.best_name, inner.as_str()));
        };
        inner.element("h2", &[], &t(state, "asset_tab_video"));
        let video_url = asset_video_url(path);
        inner.raw("<div class=\"text-center\">");
        inner.raw(&format!(
            "<video controls=\"\" style=\"width:100%; height:100%\"><source src=\"{}\" type=\"video/{}\"></video>",
            escape(&video_url),
            escape(&video.extension)
        ));
        let file_name = format!("{}.{}", asset.best_name, video.extension);
        save_button(&mut inner, state, &video_url, &file_name);
        inner.raw("</div>");
    }

    // The YAML tab is also enabled for every asset in the frozen page, and it names its download
    // `<best name>.asset`, so the tab and its save button use that extension rather than `.yaml`.
    if let Ok(text) = state.render_asset_yaml(asset.file_index, asset.object_index) {
        use assetripper::web_paths::asset_yaml_url;

        inner.element("h2", &[], &t(state, "yaml"));
        inner.open("pre", &[("class", "bg-dark-subtle rounded-3 p-2")]);
        inner.text(&text);
        inner.close("pre");
        let file_name = format!("{}.asset", asset.best_name);
        inner.raw("<div class=\"text-center\">");
        save_button(&mut inner, state, &asset_yaml_url(path), &file_name);
        inner.raw("</div>");
    }

    // The JSON tab is enabled for every asset in the frozen page, which points a `pre` at the
    // JSON route. Without client code the port inlines the same text the route serves, so the
    // tab shows the object and a save button rather than requiring a second fetch.
    if let Ok(text) = state.render_asset_json(asset.file_index, asset.object_index) {
        use assetripper::web_paths::asset_json_url;

        inner.element("h2", &[], &t(state, "json"));
        inner.open("pre", &[("class", "bg-dark-subtle rounded-3 p-2")]);
        inner.text(&text);
        inner.close("pre");
        let file_name = format!("{}.json", asset.best_name);
        inner.raw("<div class=\"text-center\">");
        save_button(&mut inner, state, &asset_json_url(path), &file_name);
        inner.raw("</div>");
    }

    if asset.has_binary {
        // The frozen hex tab is only a download button (its `Url` is empty when the
        // asset has no binary data, which the flag above already mirrors), so the port
        // keeps that shape with no inline content.
        use assetripper::web_paths::asset_binary_url;

        inner.element("h2", &[], &t(state, "asset_tab_hex"));
        let file_name = format!("{}.dat", asset.best_name);
        inner.raw("<div class=\"text-center\">");
        save_button(&mut inner, state, &asset_binary_url(path), &file_name);
        inner.raw("</div>");
    }

    if !asset.dependencies.is_empty() {
        inner.element("h2", &[], &t(state, "asset_tab_dependencies"));
        inner.open("table", &[("class", "table")]);
        inner.open("tbody", &[]);
        for dependency in &asset.dependencies {
            inner.open("tr", &[]);
            inner.element("th", &[], &dependency.field_path);
            inner.open("td", &[]);
            match dependency.target {
                super::tree::TreeDependencyTarget::Asset {
                    bundle,
                    position,
                    row,
                } => {
                    use assetripper::web_paths::{asset_view_url, AssetPath, CollectionPath};

                    let target = tree
                        .bundle(bundle)?
                        .collections
                        .get(position)?
                        .assets
                        .get(row)?;
                    let target_path = AssetPath {
                        collection: CollectionPath {
                            bundle: tree.bundle_path(bundle)?,
                            index: position as i32,
                        },
                        path_id: target.path_id,
                    };
                    tree_link(&mut inner, &asset_view_url(&target_path), &target.best_name);
                }
                super::tree::TreeDependencyTarget::Missing => {
                    // The frozen page writes this text unlocalized.
                    inner.text("Missing");
                }
            }
            inner.close("td");
            inner.close("tr");
        }
        inner.close("tbody");
        inner.close("table");
    }

    // The Development tab is the last in the frozen page and needs no route: it is a one-row
    // table naming the C# type the reference built for the asset. The port's closest equivalent
    // is the decoded class name the factory resolved, since that is the type the port created.
    inner.element("h2", &[], &t(state, "asset_tab_development"));
    inner.open("table", &[("class", "table")]);
    inner.open("tbody", &[]);
    inner.open("tr", &[]);
    inner.element("th", &[], &t(state, "csharp_type"));
    inner.element("td", &[], &asset.class_name);
    inner.close("tr");
    inner.close("tbody");
    inner.close("table");

    Some(layout(state, &asset.best_name, inner.as_str()))
}

/// The resource page: the fixed file name, its bundle, the byte size and a download.
///
/// `position` addresses the resource in the bundle at `bundle_index`, `path` is its
/// `ResourcePath`, and `size` is the content length the frozen page reads off the stream.
/// A `None` return means the tree disagrees with the resolved position.
pub fn resource_page(
    state: &WebState,
    tree: &super::tree::BundleTree,
    bundle_index: usize,
    position: usize,
    path: &assetripper::web_paths::ResourcePath,
    size: usize,
) -> Option<String> {
    use assetripper::io::special_names::fix_file_identifier;
    use assetripper::web_paths::{bundle_view_url, resource_data_url};

    let bundle = tree.bundle(bundle_index)?;
    let resource = bundle.resources.get(position)?;
    let name = fix_file_identifier(&resource.name);
    let mut inner = Html::new();
    inner.element("h1", &[], &name);

    inner.element("h2", &[], &t(state, "bundle"));
    tree_link(&mut inner, &bundle_view_url(&path.bundle), &bundle.name);

    inner.element("h2", &[], &t(state, "size"));
    inner.element("p", &[], &size.to_string());

    inner.raw("<div class=\"text-center\">");
    save_button(&mut inner, state, &resource_data_url(path), &name);
    inner.raw("</div>");

    Some(layout(state, &name, inner.as_str()))
}

/// The failed-file page, matching the frozen `FailedFiles.ViewPage`.
///
/// The page names the failed file, links its owning bundle when one resolves, and shows the
/// stack-trace route. The frozen page fills the `<pre>` from the route through client script;
/// without client code the port inlines the same text the route serves, so the page shows the
/// trace directly rather than a URL. The save button still points at the route and names the
/// download `error.txt`, as the frozen page does.
pub fn failed_file_page(
    state: &WebState,
    name: &str,
    path: &assetripper::web_paths::FailedFilePath,
    bundle_name: Option<&str>,
) -> String {
    use assetripper::web_paths::failed_file_stack_trace_url;

    let mut inner = Html::new();
    inner.element("h1", &[], name);

    if let Some(bundle_name) = bundle_name {
        inner.element("h2", &[], &t(state, "bundle"));
        tree_link(
            &mut inner,
            &assetripper::web_paths::bundle_view_url(&path.bundle),
            bundle_name,
        );
    }

    let url = failed_file_stack_trace_url(path);
    inner.element("h2", &[], &t(state, "stack_trace"));

    // The route serves the trace as text; the page shows the same content it would serve, so the
    // tab is readable without client code.
    let trace = state
        .failed_entry_stack_trace(path)
        .ok()
        .flatten()
        .unwrap_or_default();
    inner.open("pre", &[("class", "bg-dark-subtle rounded-3 p-2")]);
    inner.text(&trace);
    inner.close("pre");

    inner.raw("<div class=\"text-center\">");
    save_button(&mut inner, state, &url, "error.txt");
    inner.raw("</div>");

    layout(state, name, inner.as_str())
}

/// A download link, matching the frozen `SaveButton` button style.
fn save_button(html: &mut Html, state: &WebState, href: &str, file_name: &str) {
    html.open(
        "a",
        &[
            ("href", href),
            ("download", file_name),
            ("class", "btn btn-primary"),
        ],
    );
    html.text(&t(state, "save"));
    html.close("a");
}

/// The search page: the query form plus every matching asset in collection order.
///
/// `query` is the trimmed `q` value the frozen page works with. The input element carries
/// the query pre-escaped exactly like the frozen `WithValue(SearchQuery.ToHtml())` (the
/// element writer escapes attribute values again), and the table keeps the frozen columns
/// with the asset and collection links, preceded by the frozen class filter (only when the
/// results span more than one class) and followed by the search page's filter script.
pub fn search_page(state: &WebState, query: &str, matches: &[super::tree::SearchMatch]) -> String {
    use super::html::escape;
    use assetripper::web_paths::{asset_view_url, collection_view_url};

    let title = format!("{}: {query}", t(state, "search"));
    let mut inner = Html::new();
    inner.element("h1", &[], &t(state, "search"));

    inner.open(
        "form",
        &[
            ("action", "/Search/View"),
            ("method", "get"),
            ("class", "d-flex align-items-center mb-3"),
        ],
    );
    let encoded = escape(query).into_owned();
    inner.void(
        "input",
        &[
            ("type", "text"),
            ("id", "searchQuery"),
            ("name", "q"),
            ("value", &encoded),
            ("placeholder", &t(state, "search_placeholder")),
            ("class", "form-control me-2"),
            ("style", "max-width: 50%;"),
        ],
    );
    inner.element(
        "button",
        &[("type", "submit"), ("class", "btn btn-primary")],
        &t(state, "search"),
    );
    inner.close("form");

    if query.trim().is_empty() {
        inner.element("p", &[], &t(state, "enter_search_query"));
    } else if matches.is_empty() {
        inner.element("p", &[], &t(state, "no_results_found"));
    } else {
        inner.element(
            "h2",
            &[],
            &format!("{}: {}", t(state, "results"), matches.len()),
        );
        let classes = ordered_classes(matches.iter().map(|hit| hit.asset.class_name.as_str()));
        if classes.len() > 1 {
            write_class_filter(&mut inner, state, &classes);
        }
        inner.open("table", &[("class", "table")]);
        inner.open("thead", &[]);
        inner.open("tr", &[]);
        inner.element("th", &[], &t(state, "path_id"));
        inner.element("th", &[], &t(state, "class"));
        inner.element("th", &[], &t(state, "name"));
        inner.element("th", &[], &t(state, "collection"));
        inner.close("tr");
        inner.close("thead");
        inner.open("tbody", &[("id", "resultsTable")]);
        for hit in matches {
            inner.open("tr", &[("data-class", &hit.asset.class_name)]);
            inner.element("td", &[], &hit.asset.path_id.to_string());
            inner.element("td", &[], &hit.asset.class_name);
            inner.open("td", &[]);
            tree_link(
                &mut inner,
                &asset_view_url(&hit.collection_path.asset(hit.asset.path_id)),
                &hit.asset.best_name,
            );
            inner.close("td");
            inner.open("td", &[]);
            tree_link(
                &mut inner,
                &collection_view_url(&hit.collection_path),
                &hit.collection_name,
            );
            inner.close("td");
            inner.close("tr");
        }
        inner.close("tbody");
        inner.close("table");
    }

    // The baseline ships the page script on every response; the port emits it with the
    // results table it drives, so the prompt and no-results states carry none.
    let scripts = if query.trim().is_empty() || matches.is_empty() {
        ""
    } else {
        RESULTS_TABLE_SCRIPT
    };
    layout_with_scripts(state, &title, inner.as_str(), scripts)
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
    layout_with_scripts(state, title, inner, "")
}

/// As [`layout`], with the page's own client script written after the footer.
///
/// The frozen `DefaultPage.Write` writes the script references after the footer and inside the
/// body, so the port appends the page-specific script in the same place; `scripts` is empty
/// for the pages that have none.
fn layout_with_scripts(state: &WebState, title: &str, inner: &str, scripts: &str) -> String {
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
    if !scripts.is_empty() {
        html.raw("<script>\n");
        html.raw(scripts);
        html.raw("\n</script>\n");
    }
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
        premium_features, privacy, search_page, settings_edit, swagger_page, write_page_control,
        BOOLEAN_LABELS, LICENSE_TEXTS, TEXT_LABELS,
    };
    use crate::web::html::Html;
    use crate::web::localization::{Messages, MISSING_MARKER_PREFIX};
    use crate::web::settings_form::BOOLEAN_KEYS;
    use crate::web::state::WebState;
    use crate::web::tree::{BundleTree, SearchMatch, TreeAsset};
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
            ("search-prompt", search_page(state, "", &[])),
            ("search-empty", search_page(state, "zzz", &[])),
            (
                "search-results",
                search_page(
                    state,
                    "MonoBehaviour",
                    &[SearchMatch {
                        collection_path: assetripper::web_paths::CollectionPath {
                            bundle: assetripper::web_paths::BundlePath::root(),
                            index: 0,
                        },
                        collection_name: "managed.assets".to_owned(),
                        asset: TreeAsset {
                            path_id: 7,
                            class_id: 114,
                            class_name: "MonoBehaviour".to_owned(),
                            best_name: "ManagedRoot".to_owned(),
                            file_index: 0,
                            object_index: 0,
                            has_text: false,
                            has_binary: false,
                            has_image: false,
                            has_audio: false,
                            has_font: false,
                            has_video: false,
                            has_model: false,
                            dependencies: Vec::new(),
                        },
                    }],
                ),
            ),
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

    fn search_match(class_name: &str) -> SearchMatch {
        SearchMatch {
            collection_path: assetripper::web_paths::CollectionPath {
                bundle: assetripper::web_paths::BundlePath::root(),
                index: 0,
            },
            collection_name: "managed.assets".to_owned(),
            asset: TreeAsset {
                path_id: 7,
                class_id: 114,
                class_name: class_name.to_owned(),
                best_name: "ManagedRoot".to_owned(),
                file_index: 0,
                object_index: 0,
                has_text: false,
                has_binary: false,
                has_image: false,
                has_audio: false,
                has_font: false,
                has_video: false,
                has_model: false,
                dependencies: Vec::new(),
            },
        }
    }

    #[test]
    fn the_page_control_renders_the_frozen_paging_controls() {
        let state = state("en-US");
        let mut html = Html::new();
        write_page_control(&mut html, &state);
        assert_eq!(
            html.as_str(),
            concat!(
                "<label for=\"pageCollection\" class=\"me-2\">Page</label>",
                "<button id=\"previousButton\" class=\"ms-2\" type=\"button\" disabled=\"\">Previous</button>",
                "<label id=\"pageNumber\" class=\"ms-2\">1</label>",
                "<button id=\"nextButton\" type=\"button\" class=\"ms-2\" disabled=\"\">Next</button>",
                "<label class=\"ms-2\">Assets Per Page</label>",
                "<select id=\"assetsPerPage\" class=\"ms-2\">",
                "<option value=\"\">All</option>",
                "<option value=\"500\">500</option>",
                "<option value=\"1000\">1000</option>",
                "<option value=\"2000\">2000</option>",
                "<option value=\"3000\">3000</option>",
                "<option value=\"5000\">5000</option>",
                "</select>",
            )
        );
    }

    #[test]
    fn the_class_filter_appears_only_for_multiple_classes_and_lists_them_in_order() {
        let state = state("en-US");

        // One class: the frozen page renders no filter at all, though the page script still
        // mentions the control it would drive.
        let single = search_page(&state, "o", &[search_match("MonoBehaviour")]);
        assert!(!single.contains("<select id=\"classFilter\""), "{single}");
        assert!(!single.contains("<label for=\"classFilter\""), "{single}");
        assert!(single.contains("'#resultsTable tr'"), "{single}");

        let html = search_page(
            &state,
            "o",
            &[
                search_match("Texture2D"),
                search_match("MonoBehaviour"),
                search_match("MonoBehaviour"),
            ],
        );
        let filter = html
            .split("<select id=\"classFilter\" class=\"me-2\">")
            .nth(1)
            .expect("two classes render the filter");
        let options = filter.split("</select>").next().unwrap_or("");
        assert_eq!(
            options,
            concat!(
                "<option value=\"\">All</option>",
                "<option value=\"MonoBehaviour\">MonoBehaviour</option>",
                "<option value=\"Texture2D\">Texture2D</option>",
            ),
            "{html}"
        );
        assert!(
            html.contains("<label for=\"classFilter\" class=\"me-2\">Class</label>"),
            "{html}"
        );
        // The frozen search script filters the results table by `data-class`.
        assert!(html.contains("'#resultsTable tr'"), "{html}");
        assert!(html.contains("<tr data-class=\"Texture2D\">"), "{html}");
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
