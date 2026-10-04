//! Web product surface ported from `WebApplicationLauncher`, `Arguments`, and `Commands`.
//!
//! The frozen baseline ships as one ASP.NET web application (`AssetRipper.GUI.Free` only calls
//! `WebApplicationLauncher.Launch`). Its observable surface, all recorded here without a web
//! framework, is:
//!
//! - POSIX command-line arguments with defaults: `--port` (0 selects a random unused port),
//!   `--log` (true), `--log-path` (null selects an automatic file), `--local-web-file`
//!   (repeatable static-content overrides), and `--headless` (false; skips the automatic
//!   browser launch). Unparseable arguments exit without launching.
//! - Local web file overrides map by extension: `.css` files serve below `/css/`, `.js` files
//!   below `/js/`, and anything else below `/`. Missing files print a console message.
//! - The server binds `http://127.0.0.1:{port}`, always logs to the console, writes a rotating
//!   file log (`AssetRipper_*.log`, newest five kept, stamped `yyyyMMdd_HHmmss`), and opens a
//!   browser on start unless headless.
//! - The route table below: pages, asset/collection/bundle/resource/search/scene/failed-file
//!   data endpoints, five post commands, five native dialogs, and three filesystem probes.
//! - The five post commands: `LoadFile`/`LoadFolder` take an optional form path with a native
//!   dialog fallback (redirecting to `/Commands` when neither exists) and process non-empty
//!   selections; `ExportUnityProject`/`ExportPrimaryContent` require a form path (redirecting
//!   to `/Commands` without one) and skip empty paths; both export commands append a
//!   `AssetRipper_export_yyyyMMdd_HHmmss` subfolder when `CreateSubfolder` is `true`.
//!   `Reset` always runs and every command redirects to `/` unless stated otherwise.
//!
//! Page bodies, JSON shapes, dialog backends, and request handling stay outside this module;
//! only the launch defaults, the route inventory, the command rules, and the two pure name
//! builders are ported.

/// Default `--port`: zero selects a random unused port.
pub const DEFAULT_PORT: i32 = 0;
/// Default `--log`: file logging is on.
pub const DEFAULT_LOG: bool = true;
/// Default `--headless`: a browser launches automatically.
pub const DEFAULT_HEADLESS: bool = false;
/// The loopback address the baseline binds.
pub const LOOPBACK_BIND_ADDRESS: &str = "127.0.0.1";
/// Log files kept by rotation besides the newest one.
pub const ROTATED_LOG_FILES_KEPT: usize = 5;
/// The log name pattern is `AssetRipper_{yyyyMMdd_HHmmss}.log`.
pub const LOG_FILE_PREFIX: &str = "AssetRipper_";
pub const LOG_FILE_EXTENSION: &str = ".log";
/// The export subfolder pattern is `AssetRipper_export_{yyyyMMdd_HHmmss}`.
pub const EXPORT_SUBFOLDER_PREFIX: &str = "AssetRipper_export_";

// Compile-time verification of the launch defaults against the snapshot.
const _: () = {
    assert!(DEFAULT_PORT == 0);
    assert!(DEFAULT_LOG);
    assert!(!DEFAULT_HEADLESS);
};

/// HTTP method of one baseline route.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RouteMethod {
    Get,
    Post,
}

/// One baseline web route: its method and path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct WebRoute {
    pub method: RouteMethod,
    pub path: &'static str,
}

/// Every baseline web route in `WebApplicationLauncher` registration order.
pub const WEB_ROUTES: &[WebRoute] = &[
    WebRoute {
        method: RouteMethod::Get,
        path: "/",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Commands",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Privacy",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Licenses",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/PremiumFeatures",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/ConfigurationFiles",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/ConfigurationFiles/Singleton/Add",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/ConfigurationFiles/Singleton/Remove",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/ConfigurationFiles/List/Add",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/ConfigurationFiles/List/Remove",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/ConfigurationFiles/List/Replace",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Settings/Edit",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/Settings/Update",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/View",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Image",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Audio",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Model.glb",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Font",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Video",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Json",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Yaml",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Text",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Assets/Binary",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Bundles/View",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Collections/View",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Collections/Count",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/FailedFiles/View",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/FailedFiles/StackTrace",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Resources/View",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Resources/Data",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Search/View",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Scenes/View",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/Localization",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/Export/UnityProject",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/Export/PrimaryContent",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/LoadFile",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/LoadFolder",
    },
    WebRoute {
        method: RouteMethod::Post,
        path: "/Reset",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Dialogs/SaveFile",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Dialogs/OpenFolder",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Dialogs/OpenFolders",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Dialogs/OpenFile",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/Dialogs/OpenFiles",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/IO/File/Exists",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/IO/Directory/Exists",
    },
    WebRoute {
        method: RouteMethod::Get,
        path: "/IO/Directory/Empty",
    },
];

/// The five post commands behind `/LoadFile`, `/LoadFolder`, `/Export/*`, and `/Reset`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProductCommand {
    LoadFile,
    LoadFolder,
    ExportUnityProject,
    ExportPrimaryContent,
    Reset,
}

impl ProductCommand {
    /// All five commands.
    pub const ALL: [Self; 5] = [
        Self::LoadFile,
        Self::LoadFolder,
        Self::ExportUnityProject,
        Self::ExportPrimaryContent,
        Self::Reset,
    ];

    /// The route each command is posted to.
    pub const fn route(self) -> &'static str {
        match self {
            Self::LoadFile => "/LoadFile",
            Self::LoadFolder => "/LoadFolder",
            Self::ExportUnityProject => "/Export/UnityProject",
            Self::ExportPrimaryContent => "/Export/PrimaryContent",
            Self::Reset => "/Reset",
        }
    }

    /// Whether the command redirects to `/Commands` when no usable path is available.
    ///
    /// The load commands fall back to a native dialog first and redirect only when dialogs
    /// are unsupported too; the export commands have no dialog fallback.
    pub const fn redirects_to_commands_without_path(self) -> bool {
        match self {
            Self::LoadFile
            | Self::LoadFolder
            | Self::ExportUnityProject
            | Self::ExportPrimaryContent => true,
            Self::Reset => false,
        }
    }
}

/// Map a `--local-web-file` path to its serving prefix by extension.
///
/// `.css` serves below `/css/`, `.js` below `/js/`, and anything else below `/`, matching on
/// the file name extension exactly as the baseline switch does.
pub fn local_web_file_prefix(file_name: &str) -> &'static str {
    if file_name.ends_with(".css") {
        "/css/"
    } else if file_name.ends_with(".js") {
        "/js/"
    } else {
        "/"
    }
}

/// Build the automatic log file name around a caller-rendered `yyyyMMdd_HHmmss` timestamp.
pub fn automatic_log_file_name(timestamp_text: &str) -> String {
    let mut name = String::with_capacity(
        LOG_FILE_PREFIX.len() + timestamp_text.len() + LOG_FILE_EXTENSION.len(),
    );
    name.push_str(LOG_FILE_PREFIX);
    name.push_str(timestamp_text);
    name.push_str(LOG_FILE_EXTENSION);
    name
}

/// Append the timestamped export subfolder when `CreateSubfolder` is `true`.
///
/// The timestamp text uses the baseline `yyyyMMdd_HHmmss` UTC rendering; the `false` case
/// returns the path unchanged.
pub fn maybe_append_timestamped_subfolder(
    path: &str,
    append: bool,
    timestamp_text: &str,
) -> String {
    if !append {
        return path.to_owned();
    }
    let mut joined = String::with_capacity(
        path.len() + 1 + EXPORT_SUBFOLDER_PREFIX.len() + timestamp_text.len(),
    );
    joined.push_str(path);
    joined.push('/');
    joined.push_str(EXPORT_SUBFOLDER_PREFIX);
    joined.push_str(timestamp_text);
    joined
}

/// `GameFileLoader` load state: `GameData` present or null.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LoaderState {
    Unloaded,
    Loaded,
}

/// Whether `Reset` logs: only when data was actually present.
pub fn reset_transition(state: LoaderState) -> (LoaderState, bool) {
    match state {
        LoaderState::Loaded => (LoaderState::Unloaded, true),
        LoaderState::Unloaded => (LoaderState::Unloaded, false),
    }
}

/// System directory names `IsValidExportDirectory` rejects as the final path segment.
///
/// The comparison is ordinal and case sensitive: `desktop` exports while `Desktop` does not.
pub const PROTECTED_EXPORT_DIRECTORY_NAMES: [&str; 3] = ["Desktop", "Documents", "Downloads"];

/// Mirror `GameFileLoader.IsValidExportDirectory`.
///
/// An empty path fails, as does a path whose file name is a protected system directory name.
/// The file name is the text after the last `/` or `\` separator.
pub fn is_valid_export_directory(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let file_name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    !PROTECTED_EXPORT_DIRECTORY_NAMES.contains(&file_name)
}

/// Mirror `GameFileLoader.UserConsentsToDeletion`.
///
/// Headless exports consent automatically; otherwise the dialog result applies, with a
/// dismissed dialog (`None`) declining.
pub fn deletion_consent(headless: bool, dialog_result: Option<bool>) -> bool {
    if headless {
        return true;
    }
    dialog_result.unwrap_or(false)
}

/// How an export command proceeds after the loaded and directory gates.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExportFlow {
    /// `GameData` is null: the baseline does nothing.
    AbortedNotLoaded,
    /// The export directory is empty or protected: the baseline logs an error.
    AbortedInvalidDirectory,
    /// A non-empty directory without deletion consent: the baseline logs and returns.
    AbortedDeclined,
    /// A non-empty consented directory: delete recursively, recreate, then export.
    ProceedDeleteFirst,
    /// A missing or empty directory: create it, then export.
    ProceedCreateOnly,
}

/// Mirror the gate sequence shared by `ExportUnityProject` and `ExportPrimaryContent`.
pub fn resolve_export_flow(
    is_loaded: bool,
    directory_valid: bool,
    directory_non_empty: bool,
    consent_to_delete: bool,
) -> ExportFlow {
    if !is_loaded {
        return ExportFlow::AbortedNotLoaded;
    }
    if !directory_valid {
        return ExportFlow::AbortedInvalidDirectory;
    }
    if !directory_non_empty {
        return ExportFlow::ProceedCreateOnly;
    }
    if consent_to_delete {
        ExportFlow::ProceedDeleteFirst
    } else {
        ExportFlow::AbortedDeclined
    }
}

#[cfg(test)]
mod tests {
    use super::{
        automatic_log_file_name, deletion_consent, is_valid_export_directory,
        local_web_file_prefix, maybe_append_timestamped_subfolder, reset_transition,
        resolve_export_flow, ExportFlow, LoaderState, ProductCommand, RouteMethod,
        EXPORT_SUBFOLDER_PREFIX, LOG_FILE_EXTENSION, LOG_FILE_PREFIX, LOOPBACK_BIND_ADDRESS,
        PROTECTED_EXPORT_DIRECTORY_NAMES, ROTATED_LOG_FILES_KEPT, WEB_ROUTES,
    };

    #[test]
    fn launch_defaults_match_arguments_and_launcher() {
        // Boolean and port defaults are verified at compile time above; the runtime test
        // covers the values that cannot be const-evaluated the same way.
        assert_eq!(LOOPBACK_BIND_ADDRESS, "127.0.0.1");
        assert_eq!(ROTATED_LOG_FILES_KEPT, 5);
    }

    #[test]
    fn route_table_covers_every_registered_endpoint() {
        // 34 GET routes and 12 POST routes in registration order.
        assert_eq!(WEB_ROUTES.len(), 46);
        assert_eq!(
            WEB_ROUTES
                .iter()
                .filter(|route| route.method == RouteMethod::Get)
                .count(),
            34
        );
        assert_eq!(
            WEB_ROUTES
                .iter()
                .filter(|route| route.method == RouteMethod::Post)
                .count(),
            12
        );
        assert_eq!(WEB_ROUTES.first().unwrap().path, "/");
        assert_eq!(WEB_ROUTES.last().unwrap().path, "/IO/Directory/Empty");
        // The glb suffix is part of the model route, unlike every sibling.
        assert!(WEB_ROUTES
            .iter()
            .any(|route| route.path == "/Assets/Model.glb"));
        for command in ProductCommand::ALL {
            assert!(
                WEB_ROUTES
                    .iter()
                    .any(|route| route.method == RouteMethod::Post && route.path == command.route()),
                "{command:?} must have a POST route"
            );
        }
    }

    #[test]
    fn commands_keep_their_redirect_rules() {
        for command in [
            ProductCommand::LoadFile,
            ProductCommand::LoadFolder,
            ProductCommand::ExportUnityProject,
            ProductCommand::ExportPrimaryContent,
        ] {
            assert!(command.redirects_to_commands_without_path(), "{command:?}");
        }
        assert!(!ProductCommand::Reset.redirects_to_commands_without_path());
        assert_eq!(
            ProductCommand::ExportUnityProject.route(),
            "/Export/UnityProject"
        );
        assert_eq!(ProductCommand::Reset.route(), "/Reset");
    }

    #[test]
    fn name_builders_follow_the_baseline_format_strings() {
        assert_eq!(local_web_file_prefix("site.css"), "/css/");
        assert_eq!(local_web_file_prefix("commands_page.js"), "/js/");
        assert_eq!(local_web_file_prefix("favicon.ico"), "/");
        assert_eq!(local_web_file_prefix("no-extension"), "/");
        assert_eq!(
            automatic_log_file_name("20261004_120000"),
            "AssetRipper_20261004_120000.log"
        );
        assert_eq!(LOG_FILE_PREFIX, "AssetRipper_");
        assert_eq!(LOG_FILE_EXTENSION, ".log");
        assert_eq!(
            maybe_append_timestamped_subfolder("/out", true, "20261004_120000"),
            "/out/AssetRipper_export_20261004_120000"
        );
        assert_eq!(
            maybe_append_timestamped_subfolder("/out", false, "20261004_120000"),
            "/out"
        );
        assert_eq!(EXPORT_SUBFOLDER_PREFIX, "AssetRipper_export_");
    }

    #[test]
    fn loader_reset_logs_only_when_data_was_present() {
        assert_eq!(
            reset_transition(LoaderState::Loaded),
            (LoaderState::Unloaded, true)
        );
        assert_eq!(
            reset_transition(LoaderState::Unloaded),
            (LoaderState::Unloaded, false)
        );
    }

    #[test]
    fn export_directories_reject_empty_and_protected_names_ordinally() {
        assert!(!is_valid_export_directory(""));
        assert!(!is_valid_export_directory("C:\\Users\\me\\Desktop"));
        assert!(!is_valid_export_directory("/home/me/Documents"));
        assert!(!is_valid_export_directory("Downloads"));
        // Ordinal comparison: other casings export.
        assert!(is_valid_export_directory("C:\\Users\\me\\desktop"));
        assert!(is_valid_export_directory("/home/me/DOCUMENTS"));
        assert!(is_valid_export_directory("/out/ExportedProject"));
        assert_eq!(
            PROTECTED_EXPORT_DIRECTORY_NAMES,
            ["Desktop", "Documents", "Downloads"]
        );
    }

    #[test]
    fn deletion_consent_prefers_headless_over_dialogs() {
        assert!(deletion_consent(true, None));
        assert!(deletion_consent(true, Some(false)));
        assert!(deletion_consent(false, Some(true)));
        assert!(!deletion_consent(false, Some(false)));
        assert!(!deletion_consent(false, None));
    }

    #[test]
    fn export_flow_follows_the_loaded_valid_consent_gates() {
        assert_eq!(
            resolve_export_flow(false, true, true, true),
            ExportFlow::AbortedNotLoaded
        );
        assert_eq!(
            resolve_export_flow(true, false, false, false),
            ExportFlow::AbortedInvalidDirectory
        );
        assert_eq!(
            resolve_export_flow(true, true, false, false),
            ExportFlow::ProceedCreateOnly
        );
        assert_eq!(
            resolve_export_flow(true, true, true, true),
            ExportFlow::ProceedDeleteFirst
        );
        assert_eq!(
            resolve_export_flow(true, true, true, false),
            ExportFlow::AbortedDeclined
        );
    }
}
