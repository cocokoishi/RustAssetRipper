//! Post-export outputs ported from `ExportHandler.GetPostExporters`.
//!
//! After every collection exports, the baseline runs five post-exporters in a fixed order:
//!
//! 1. `ProjectVersionPostExporter` writes `{ProjectSettingsPath}/ProjectVersion.txt` as UTF-8
//!    without a BOM: `m_EditorVersion: {version}\n`, plus `m_StandardAssetsVersion: 0\n` when
//!    the major version is 5. The version rendering itself comes from the external primitives
//!    package, so the caller supplies the version text and the content builder adds the lines.
//! 2. `PackageManifestPostExporter` writes `{ProjectRootPath}/Packages/manifest.json` with the
//!    default Unity module dependencies, all pinned at `1.0.0`; `com.unity.modules.androidjni`
//!    is included only for versions at least 2019.2.
//! 3. `StreamingAssetsPostExporter` copies the platform streaming-assets tree below
//!    `{AssetsPath}/StreamingAssets`. It does nothing without a platform structure, when
//!    `StreamingAssetsMode` is `Ignore`, or when the input directory is missing.
//! 4. `DllPostExporter` saves every game assembly below
//!    `{AuxiliaryFilesPath}/GameAssemblies`, skipping everything when there are no assemblies.
//! 5. `PathIdMapExporter` writes `{AuxiliaryFilesPath}/path_id_map.json` with every serialized
//!    collection's `Mesh`, `Texture`, `AudioClip`, and `TextAsset` entries (name, class name,
//!    and PathID each).
//!
//! Directory roots derive from `ExportRootPath`: the project root is `ExportedProject` below
//! it, `Assets` and `ProjectSettings` sit below the project root, and `AuxiliaryFiles` sits
//! beside it. Copying, assembly saving, and JSON serialization stay outside this module; only
//! the order, destinations, skip conditions, and the two pure content builders are ported.

/// One `GetPostExporters` yield in source order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PostExporterKind {
    /// `ProjectVersionPostExporter`.
    ProjectVersion,
    /// `PackageManifestPostExporter`.
    PackageManifest,
    /// `StreamingAssetsPostExporter`.
    StreamingAssets,
    /// `DllPostExporter`.
    Dll,
    /// `PathIdMapExporter`.
    PathIdMap,
}

impl PostExporterKind {
    /// All post-exporters in `GetPostExporters` source order.
    pub const ALL: [Self; 5] = [
        Self::ProjectVersion,
        Self::PackageManifest,
        Self::StreamingAssets,
        Self::Dll,
        Self::PathIdMap,
    ];

    /// The file this post-exporter creates, relative to its destination directory.
    pub const fn file_name(self) -> &'static str {
        match self {
            Self::ProjectVersion => "ProjectVersion.txt",
            Self::PackageManifest => "manifest.json",
            Self::StreamingAssets => "StreamingAssets",
            Self::Dll => "GameAssemblies",
            Self::PathIdMap => "path_id_map.json",
        }
    }

    /// Whether the baseline skips this post-exporter without doing anything in some states.
    ///
    /// `StreamingAssets` skips without a platform structure, in `Ignore` mode, or without an
    /// input directory; `Dll` skips when there are no game assemblies. The other three always
    /// write.
    pub const fn is_conditional(self) -> bool {
        match self {
            Self::StreamingAssets | Self::Dll => true,
            Self::ProjectVersion | Self::PackageManifest | Self::PathIdMap => false,
        }
    }
}

/// The asset kinds `PathIdMapExporter` records: `IMesh`, `ITexture`, `IAudioClip`, `ITextAsset`.
pub const PATH_ID_MAP_ASSET_KINDS: [&str; 4] = ["Mesh", "Texture", "AudioClip", "TextAsset"];

/// Build the `ProjectVersion.txt` body around a caller-rendered version string.
///
/// The baseline writes `m_EditorVersion: {version}\n` and appends
/// `m_StandardAssetsVersion: 0\n` when the major version is 5.
pub fn project_version_file_contents(editor_version_text: &str, major_is_five: bool) -> String {
    let mut contents = String::with_capacity(
        "m_EditorVersion: \n".len()
            + editor_version_text.len()
            + "m_StandardAssetsVersion: 0\n".len(),
    );
    contents.push_str("m_EditorVersion: ");
    contents.push_str(editor_version_text);
    contents.push('\n');
    if major_is_five {
        contents.push_str("m_StandardAssetsVersion: 0\n");
    }
    contents
}

/// The default `Packages/manifest.json` dependencies with their pinned versions.
///
/// Every module pins `1.0.0`; `com.unity.modules.androidjni` is present only for versions at
/// least 2019.2.
pub fn default_package_dependencies(
    version_at_least_2019_2: bool,
) -> Vec<(&'static str, &'static str)> {
    let mut dependencies: Vec<(&'static str, &'static str)> = Vec::with_capacity(31);
    dependencies.push(("com.unity.modules.ai", "1.0.0"));
    if version_at_least_2019_2 {
        dependencies.push(("com.unity.modules.androidjni", "1.0.0"));
    }
    dependencies.extend([
        ("com.unity.modules.animation", "1.0.0"),
        ("com.unity.modules.assetbundle", "1.0.0"),
        ("com.unity.modules.audio", "1.0.0"),
        ("com.unity.modules.cloth", "1.0.0"),
        ("com.unity.modules.director", "1.0.0"),
        ("com.unity.modules.imageconversion", "1.0.0"),
        ("com.unity.modules.imgui", "1.0.0"),
        ("com.unity.modules.jsonserialize", "1.0.0"),
        ("com.unity.modules.particlesystem", "1.0.0"),
        ("com.unity.modules.physics", "1.0.0"),
        ("com.unity.modules.physics2d", "1.0.0"),
        ("com.unity.modules.screencapture", "1.0.0"),
        ("com.unity.modules.terrain", "1.0.0"),
        ("com.unity.modules.terrainphysics", "1.0.0"),
        ("com.unity.modules.tilemap", "1.0.0"),
        ("com.unity.modules.ui", "1.0.0"),
        ("com.unity.modules.uielements", "1.0.0"),
        ("com.unity.modules.umbra", "1.0.0"),
        ("com.unity.modules.unityanalytics", "1.0.0"),
        ("com.unity.modules.unitywebrequest", "1.0.0"),
        ("com.unity.modules.unitywebrequestassetbundle", "1.0.0"),
        ("com.unity.modules.unitywebrequestaudio", "1.0.0"),
        ("com.unity.modules.unitywebrequesttexture", "1.0.0"),
        ("com.unity.modules.unitywebrequestwww", "1.0.0"),
        ("com.unity.modules.vehicles", "1.0.0"),
        ("com.unity.modules.video", "1.0.0"),
        ("com.unity.modules.vr", "1.0.0"),
        ("com.unity.modules.wind", "1.0.0"),
        ("com.unity.modules.xr", "1.0.0"),
    ]);
    dependencies
}

#[cfg(test)]
mod tests {
    use super::{
        default_package_dependencies, project_version_file_contents, PostExporterKind,
        PATH_ID_MAP_ASSET_KINDS,
    };

    #[test]
    fn post_exporters_run_in_source_order_with_known_destinations() {
        assert_eq!(
            PostExporterKind::ALL,
            [
                PostExporterKind::ProjectVersion,
                PostExporterKind::PackageManifest,
                PostExporterKind::StreamingAssets,
                PostExporterKind::Dll,
                PostExporterKind::PathIdMap,
            ]
        );
        assert_eq!(
            PostExporterKind::ProjectVersion.file_name(),
            "ProjectVersion.txt"
        );
        assert_eq!(
            PostExporterKind::PackageManifest.file_name(),
            "manifest.json"
        );
        assert_eq!(PostExporterKind::PathIdMap.file_name(), "path_id_map.json");
        assert!(!PostExporterKind::ProjectVersion.is_conditional());
        assert!(!PostExporterKind::PackageManifest.is_conditional());
        assert!(PostExporterKind::StreamingAssets.is_conditional());
        assert!(PostExporterKind::Dll.is_conditional());
        assert!(!PostExporterKind::PathIdMap.is_conditional());
    }

    #[test]
    fn project_version_body_matches_the_source_format_strings() {
        assert_eq!(
            project_version_file_contents("2019.4.3f1", false),
            "m_EditorVersion: 2019.4.3f1\n"
        );
        assert_eq!(
            project_version_file_contents("5.6.7f1", true),
            "m_EditorVersion: 5.6.7f1\nm_StandardAssetsVersion: 0\n"
        );
    }

    #[test]
    fn default_manifest_pins_every_module_with_the_androidjni_gate() {
        let modern = default_package_dependencies(true);
        assert_eq!(modern.len(), 31);
        assert!(modern.contains(&("com.unity.modules.ai", "1.0.0")));
        assert!(modern.contains(&("com.unity.modules.androidjni", "1.0.0")));
        assert!(modern.contains(&("com.unity.modules.xr", "1.0.0")));
        assert!(modern.iter().all(|(_, version)| *version == "1.0.0"));

        let legacy = default_package_dependencies(false);
        assert_eq!(legacy.len(), 30);
        assert!(legacy
            .iter()
            .all(|(module, _)| *module != "com.unity.modules.androidjni"));
    }

    #[test]
    fn path_id_map_covers_the_four_commonly_useful_asset_kinds() {
        assert_eq!(
            PATH_ID_MAP_ASSET_KINDS,
            ["Mesh", "Texture", "AudioClip", "TextAsset"]
        );
    }
}
