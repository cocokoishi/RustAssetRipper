//! Executable Unity project export: collection selection, staged file plan, and writing.
//!
//! This is the product spine of `AssetRipper.Export.UnityProjects`: it reproduces
//! `ProjectExporter.CreateCollection` (the reverse registration stack walk), the per-family
//! destination rules of `AssetExportCollection`/`ManagerExportCollection`/
//! `UnknownExportCollection`/`UnreadableExportCollection`, and the `ExportHandler.Export`
//! write sequence (export loop, then the `ProjectVersion`/`PackageManifest` post-exporters).
//!
//! Everything is staged in memory first: `ProjectExportPlan` holds the validated relative
//! paths, the exact bytes, and the importer records, and only `execute_project_export`
//! touches the disk. A plan that cannot be expressed as a complete, collision-free project
//! is rejected before any write.

use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::OnceLock;

use super::asset_path::{
    join_path, plan_asset_export_paths, ExportAssetPathIdentity, ExportFileSystem,
    HostPathPlatform, UniqueNameLimits, UniqueNameState,
};
use super::collection_claims::{
    claim_collection, is_supported_audio_export_format, AudioClaimDecode, ClaimAssetKind,
    ClaimFacts, ClaimMainKind, ClaimOutcome, ShaderTextState,
};
use super::collection_kind::ExportCollectionKind;
use super::collection_paths::{plan_unknown_export_paths, plan_unreadable_export_paths};
use super::export_extension::{ExportClassIdentity, ExportClassMarker, EXPORT_CLASS_MARKERS};
use super::export_root::prepare_export_root;
use super::meta::UnityMetaFileOptions;
use super::post_export::{default_package_dependencies, project_version_file_contents};
use super::project_order::ProjectExportEvent;
use super::project_output::{
    export_asset_with_meta, write_new_project_file, AssetExportStep, AssetExportWithMetaPlan,
    UnityProjectOutputLimits,
};
use super::project_plan::plan_manager_export_paths;
use super::registration::{RegistrationGate, EXPORTER_REGISTRATIONS};
use super::settings::{
    AudioExportFormat, LightmapTextureExportFormat, ShaderExportMode, SpriteExportMode,
};
use super::settings_file::ExportSettingsFile;
use super::unity_yaml::{
    UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar, UnityYamlSequenceStyle,
    UnityYamlWriteLimits,
};
use crate::processing::scene_naming::SceneUnityVersion;

/// `PlayerSettings` has no `ClassIDType` member; `ManagerAssetExporter` claims it through
/// `TypeTreeObject { IsPlayerSettings: true }` instead.
pub const PLAYER_SETTINGS_CLASS_ID: i32 = 129;

/// Portable project-relative path of the `ProjectVersionPostExporter` output.
pub const PROJECT_VERSION_FILE: &str = "ProjectSettings/ProjectVersion.txt";
/// Portable project-relative path of the `PackageManifestPostExporter` output.
pub const PACKAGE_MANIFEST_FILE: &str = "Packages/manifest.json";

/// The generated class-ID to interface table, produced by
/// `tools/probes/export-collections` from the generated assembly of the frozen snapshot.
const CLASS_INTERFACES: &str = include_str!("collection_interfaces_fixture.txt");

fn class_interface_table() -> &'static HashMap<i32, Vec<&'static str>> {
    static TABLE: OnceLock<HashMap<i32, Vec<&'static str>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: HashMap<i32, Vec<&'static str>> = HashMap::new();
        for line in CLASS_INTERFACES.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((id, names)) = line.split_once('\t') else {
                continue;
            };
            let Ok(class_id) = id.trim().parse::<i32>() else {
                continue;
            };
            let interfaces: Vec<&'static str> = names
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .collect();
            table.insert(class_id, interfaces);
        }
        table
    })
}

/// The generated interfaces of one class ID; unknown classes report an empty slice.
pub fn class_interfaces(class_id: i32) -> &'static [&'static str] {
    class_interface_table()
        .get(&class_id)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

/// Whether the frozen snapshot's generated assembly contains this class ID.
pub fn is_generated_class(class_id: i32) -> bool {
    class_id == PLAYER_SETTINGS_CLASS_ID || class_interface_table().contains_key(&class_id)
}

/// The source's `ClassName` for one object: the generated class name when the class ID has
/// one, else the TypeTree root type name, else the decimal class ID.
pub fn object_class_name(class_id: i32, type_tree_root: Option<&str>) -> String {
    if let Some(name) = crate::serialized_file::object::class_id_type_name(class_id) {
        return name.to_owned();
    }
    if let Some(name) = type_tree_root.filter(|name| !name.is_empty()) {
        return name.to_owned();
    }
    class_id.to_string()
}

/// Everything the registration gates read from the effective `ExportSettings`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectExportSettings {
    pub export_unreadable_assets: bool,
    pub sprite_export_mode_yaml: bool,
    pub lightmap_texture_export_format_non_yaml: bool,
    pub shader_export_mode_yaml: bool,
    pub audio_export_format_native: bool,
    pub audio_export_format_supported: bool,
    pub version_at_least_2020_2: bool,
}

impl ProjectExportSettings {
    /// Build the gate facts from a loaded or defaulted `ExportSettings` record and the
    /// export's Unity version (`ExportHandler.Export` sets `SetProjectSettings` first).
    pub fn from_export_settings(settings: &ExportSettingsFile, version: SceneUnityVersion) -> Self {
        Self {
            export_unreadable_assets: settings.export_unreadable_assets,
            sprite_export_mode_yaml: matches!(settings.sprite_export_mode, SpriteExportMode::Yaml),
            lightmap_texture_export_format_non_yaml: !matches!(
                settings.lightmap_texture_export_format,
                LightmapTextureExportFormat::Yaml
            ),
            shader_export_mode_yaml: matches!(settings.shader_export_mode, ShaderExportMode::Yaml),
            audio_export_format_native: matches!(
                settings.audio_export_format,
                AudioExportFormat::Native
            ),
            audio_export_format_supported: is_supported_audio_export_format(
                settings.audio_export_format,
            ),
            version_at_least_2020_2: version.major > 2020
                || (version.major == 2020 && version.minor >= 2),
        }
    }
}

/// `RegistrationGate` evaluation against the effective settings.
pub const fn registration_gate_active(
    gate: RegistrationGate,
    settings: ProjectExportSettings,
) -> bool {
    match gate {
        RegistrationGate::Always => true,
        RegistrationGate::ExportUnreadableAssets => settings.export_unreadable_assets,
        RegistrationGate::NoExportUnreadableAssets => !settings.export_unreadable_assets,
        RegistrationGate::SpriteExportModeYaml => settings.sprite_export_mode_yaml,
        RegistrationGate::LightmapTextureExportFormatNonYaml => {
            settings.lightmap_texture_export_format_non_yaml
        }
        RegistrationGate::VersionAtLeast2020_2 => settings.version_at_least_2020_2,
        RegistrationGate::ShaderExportModeYaml => settings.shader_export_mode_yaml,
        RegistrationGate::ShaderExportModeNonYaml => !settings.shader_export_mode_yaml,
        RegistrationGate::AudioExportFormatNative => settings.audio_export_format_native,
        RegistrationGate::AudioExportFormatSupported => settings.audio_export_format_supported,
    }
}

/// The generated-type facts of one object that drive collection selection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectClassFacts {
    pub class_id: i32,
    /// The generated assembly declares this class ID.
    pub known: bool,
    /// The object payload failed TypeTree decoding (`UnreadableObject`).
    pub unreadable: bool,
}

impl ObjectClassFacts {
    /// Facts for a workspace object with a given decode outcome.
    pub fn new(class_id: i32, unreadable: bool) -> Self {
        Self {
            class_id,
            known: is_generated_class(class_id),
            unreadable,
        }
    }

    /// Facts for one object with class-ID knowledge overridden (used by replay tests).
    pub const fn with_known(class_id: i32, known: bool, unreadable: bool) -> Self {
        Self {
            class_id,
            known,
            unreadable,
        }
    }
}

/// `ObjectHandlerStack.CalculateAssetHandlerStack` type matching for one registration target.
pub fn class_satisfies_target(facts: ObjectClassFacts, target: &str) -> bool {
    match target {
        "IUnityObjectBase" => true,
        // `TypeTreeObject { IsPlayerSettings: true }`: only PlayerSettings reaches the manager
        // exporter through the no-generated-type fallback object.
        "TypeTreeObject" => facts.class_id == PLAYER_SETTINGS_CLASS_ID && facts.known,
        "UnknownObject" => !facts.known && !facts.unreadable,
        "UnreadableObject" => facts.unreadable,
        other => {
            class_interfaces(facts.class_id).contains(&other)
                || (facts.class_id == PLAYER_SETTINGS_CLASS_ID && other == "IGlobalGameManager")
        }
    }
}

/// The `ClaimAssetKind` of one object, in the source's interface-precedence order.
pub fn claim_asset_kind(facts: ObjectClassFacts) -> ClaimAssetKind {
    if facts.unreadable {
        return ClaimAssetKind::UnreadableObject;
    }
    if !facts.known {
        return ClaimAssetKind::UnknownObject;
    }
    if facts.class_id == PLAYER_SETTINGS_CLASS_ID {
        return ClaimAssetKind::PlayerSettingsObject;
    }
    let interfaces = class_interfaces(facts.class_id);
    let has = |name: &str| interfaces.contains(&name);
    if has("IEditorBuildSettings") {
        ClaimAssetKind::EditorBuildSettings
    } else if has("IGlobalGameManager") {
        ClaimAssetKind::GlobalGameManager
    } else if has("IMonoBehaviour") {
        ClaimAssetKind::MonoBehaviour
    } else if has("IMonoScript") {
        ClaimAssetKind::MonoScript
    } else if has("IMesh") {
        ClaimAssetKind::Mesh
    } else if has("IImageTexture") {
        ClaimAssetKind::ImageTexture
    } else if has("ISpriteAtlas") {
        ClaimAssetKind::SpriteAtlas
    } else if has("ISprite") {
        ClaimAssetKind::Sprite
    } else if has("ICubemapArray") || has("ITexture2DArray") || has("ITexture3D") {
        ClaimAssetKind::TextureArray
    } else if has("ITexture2D") {
        ClaimAssetKind::Texture2D
    } else if has("IShader") {
        // A generated Shader can also implement ITextAsset. ProjectExporter registers the
        // shader handlers after TextAssetExporter, so the shader handler wins the stack walk.
        ClaimAssetKind::Shader
    } else if has("ITextAsset") {
        ClaimAssetKind::TextAsset
    } else if has("IMovieTexture") {
        ClaimAssetKind::MovieTexture
    } else if has("IVideoClip") {
        ClaimAssetKind::VideoClip
    } else if has("IAudioClip") {
        ClaimAssetKind::AudioClip
    } else if has("IFont") {
        ClaimAssetKind::Font
    } else if has("IMaterial") {
        ClaimAssetKind::Material
    } else if has("ISceneAsset") {
        ClaimAssetKind::SceneAsset
    } else {
        ClaimAssetKind::Other
    }
}

/// The claim facts of one unprocessed workspace object with no payload-derived facts.
///
/// `MainAsset` is unset until the Scene/Prefab processors run, so `main` is `Absent`. The
/// project workflow supplies the decoded Shader script state through
/// [`claim_facts_for_shader_text`]; other specialized payload facts remain explicit defaults.
pub fn claim_facts_for(facts: ObjectClassFacts, settings: ProjectExportSettings) -> ClaimFacts {
    claim_facts_for_shader_text(facts, settings, ShaderTextState::NoScript)
}

/// The claim facts of one unprocessed object when its Shader script admission is known.
pub fn claim_facts_for_shader_text(
    facts: ObjectClassFacts,
    settings: ProjectExportSettings,
    shader_text: ShaderTextState,
) -> ClaimFacts {
    ClaimFacts {
        asset: claim_asset_kind(facts),
        main: ClaimMainKind::Absent,
        on_game_object: false,
        data_valid: false,
        export_sprites: !settings.sprite_export_mode_yaml,
        audio_decode: AudioClaimDecode::Undecodable,
        prefer_wav: false,
        shader_text,
        script_claimed_before: false,
        assembly_manager_is_set: false,
        engine_file: false,
        reserved_font_name: false,
        cache_hit: false,
    }
}

/// The winning registration of one `ProjectExporter.CreateCollection` call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CollectionSelection {
    pub registration_index: usize,
    pub exporter: &'static str,
    pub target: &'static str,
    pub outcome: ClaimOutcome,
    pub collection_name: &'static str,
}

impl CollectionSelection {
    /// The audited file-writing family of the selected baseline collection.
    pub fn kind(self) -> ExportCollectionKind {
        collection_kind(self.collection_name)
    }
}

/// Run `ProjectExporter.CreateCollection` over one object's generated-type facts.
pub fn select_collection(
    facts: ObjectClassFacts,
    settings: ProjectExportSettings,
) -> Result<CollectionSelection, ProjectExportError> {
    select_collection_with_shader_text(facts, settings, ShaderTextState::NoScript)
}

/// Run `ProjectExporter.CreateCollection` with the payload-derived Shader script state.
pub fn select_collection_with_shader_text(
    facts: ObjectClassFacts,
    settings: ProjectExportSettings,
    shader_text: ShaderTextState,
) -> Result<CollectionSelection, ProjectExportError> {
    let claim_facts = claim_facts_for_shader_text(facts, settings, shader_text);
    // `ObjectHandlerStack` pushes every matching handler in registration order and iterates the
    // resulting stack, so the last registration is tried first.
    for (registration_index, registration) in EXPORTER_REGISTRATIONS.iter().enumerate().rev() {
        if !registration_gate_active(registration.gate, settings) {
            continue;
        }
        if !class_satisfies_target(facts, registration.target) {
            continue;
        }
        let outcome = if registration.exporter == "DummyAssetExporter" {
            match registration.dummy_is_empty_collection {
                Some(true) => ClaimOutcome::EmptyCollection,
                Some(false) => ClaimOutcome::SkipCollection,
                None => ClaimOutcome::Decline,
            }
        } else {
            claim_collection(registration.exporter, &claim_facts)
        };
        if outcome == ClaimOutcome::Decline {
            continue;
        }
        return Ok(CollectionSelection {
            registration_index,
            exporter: registration.exporter,
            target: registration.target,
            outcome,
            collection_name: claim_outcome_collection_name(outcome),
        });
    }
    Err(ProjectExportError::UnhandledAsset {
        class_id: facts.class_id,
    })
}

/// The baseline type name produced by one non-declining claim outcome.
pub fn claim_outcome_collection_name(outcome: ClaimOutcome) -> &'static str {
    match outcome {
        ClaimOutcome::Collection(name) => name,
        ClaimOutcome::EmptyCollection => "EmptyExportCollection",
        ClaimOutcome::FailCollection => "FailExportCollection",
        ClaimOutcome::RedirectCollection => "RedirectExportCollection",
        ClaimOutcome::SkipCollection => "SkipExportCollection",
        ClaimOutcome::Decline => "Decline",
    }
}

/// Map a baseline collection type name onto the audited family table.
pub fn collection_kind(collection_name: &str) -> ExportCollectionKind {
    match collection_name {
        "SceneExportCollection" => ExportCollectionKind::Scene,
        "PrefabExportCollection" => ExportCollectionKind::Prefab,
        "ManagerExportCollection" | "EditorBuildSettingsExportCollection" => {
            ExportCollectionKind::Manager
        }
        "ScriptExportCollection" => ExportCollectionKind::Script,
        "EmptyScriptExportCollection" => ExportCollectionKind::EmptyScript,
        "UnknownExportCollection" => ExportCollectionKind::Unknown,
        "UnreadableExportCollection" => ExportCollectionKind::Unreadable,
        "FailExportCollection" => ExportCollectionKind::Fail,
        "SkipExportCollection" => ExportCollectionKind::Skip,
        "EmptyExportCollection" => ExportCollectionKind::Empty,
        "DeletedAssetsExportCollection" => ExportCollectionKind::Deleted,
        "RedirectExportCollection" | "SingleRedirectExportCollection" => {
            ExportCollectionKind::Redirect
        }
        // Every remaining concrete collection derives from `AssetExportCollection` and keeps
        // its file-writing flow (`ScriptableObjectExportCollection`, `ShaderExportCollection`,
        // `YamlStreamedAssetExportCollection`, the texture/audio collections, ...).
        _ => ExportCollectionKind::Asset,
    }
}

/// `ExportCollection.GetExportExtension` for one object's generated interfaces.
pub fn export_extension_for(facts: ObjectClassFacts) -> &'static str {
    let interfaces = class_interfaces(facts.class_id);
    let markers: Vec<ExportClassMarker> = EXPORT_CLASS_MARKERS
        .iter()
        .copied()
        .filter(|marker| interfaces.contains(&marker.marker_interface()))
        .collect();
    ExportClassIdentity::from_markers(facts.class_id, None, markers).export_extension()
}

/// Planning failures that must abort the export before any write.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectExportError {
    /// No registration claims the object (`CreateCollection` throws in the source).
    UnhandledAsset { class_id: i32 },
    /// Two planned files resolve to the same project-relative path.
    DuplicateProjectPath {
        relative_path: String,
        first: String,
        second: String,
    },
    /// A planned path is absolute, empty, or escapes the project root.
    InvalidProjectPath {
        relative_path: String,
        reason: &'static str,
    },
    /// The staged asset and support-file counts overflow the host address space.
    ProjectFileCountOverflow,
    /// A bounded allocation for the plan could not be reserved.
    AllocationFailed { resource: &'static str },
    /// The plan describes an object with no usable payload bytes.
    MissingRawPayload {
        file_index: usize,
        object_index: usize,
    },
}

impl Display for ProjectExportError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnhandledAsset { class_id } => {
                write!(formatter, "no exporter claims class ID {class_id}")
            }
            Self::DuplicateProjectPath {
                relative_path,
                first,
                second,
            } => write!(
                formatter,
                "project paths {first:?} and {second:?} collide at {relative_path:?}"
            ),
            Self::InvalidProjectPath {
                relative_path,
                reason,
            } => write!(
                formatter,
                "project path {relative_path:?} is invalid: {reason}"
            ),
            Self::ProjectFileCountOverflow => {
                formatter.write_str("project output file count overflowed")
            }
            Self::AllocationFailed { resource } => {
                write!(formatter, "could not allocate {resource}")
            }
            Self::MissingRawPayload {
                file_index,
                object_index,
            } => write!(
                formatter,
                "file {file_index} object {object_index} has no payload bytes for a raw collection"
            ),
        }
    }
}

impl Error for ProjectExportError {}

/// A non-fatal export notice: a collection that writes nothing, a renderer that fell back
/// to the generic asset family, or a baseline skip. Never reported as a successful export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectExportNotice {
    pub subject: String,
    pub reason: &'static str,
}

/// The importer record written beside one planned asset.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedProjectMeta {
    pub importer_class: String,
    pub importer: UnityYamlNode,
}

/// One staged project file: the portable relative path, the exact bytes, and the sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedProjectFile {
    /// The baseline collection type this file came from.
    pub collection_name: &'static str,
    /// The audited family of that collection.
    pub kind: ExportCollectionKind,
    /// Portable project-relative destination with `/` separators.
    pub relative_path: String,
    pub guid: [u8; 16],
    pub bytes: Vec<u8>,
    pub meta: Option<PlannedProjectMeta>,
    /// Collection label used by progress and failure reporting.
    pub label: String,
}

/// A project support file emitted by a collection exporter without an asset `.meta` sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedProjectSupportFile {
    pub relative_path: String,
    pub bytes: Vec<u8>,
    /// Write immediately before this zero-based staged asset-file index.
    pub before_file_index: usize,
}

/// The complete staged project: nothing is written until `execute_project_export` runs.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectExportPlan {
    pub project_version_text: String,
    pub project_version_file: String,
    pub package_manifest: String,
    pub files: Vec<PlannedProjectFile>,
    /// Collection-generated project files such as editor patches, with no importer sidecar.
    pub support_files: Vec<PlannedProjectSupportFile>,
    /// Objects whose collection writes nothing, with the baseline reason.
    pub skipped: Vec<ProjectExportNotice>,
    /// Files that used the generic asset-family renderer instead of their own.
    pub fallbacks: Vec<ProjectExportNotice>,
}

impl ProjectExportPlan {
    /// Fail-closed path validation: every destination is a portable, non-escaping,
    /// collision-free `assets`-side path, and no planned file shadows a post-export file.
    pub fn validate(&self) -> Result<(), ProjectExportError> {
        self.files
            .len()
            .checked_add(self.support_files.len())
            .ok_or(ProjectExportError::ProjectFileCountOverflow)?;
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut seen_insensitive: HashMap<String, String> = HashMap::new();
        for reserved in [PROJECT_VERSION_FILE, PACKAGE_MANIFEST_FILE] {
            seen.insert(reserved.to_owned());
            seen_insensitive.insert(reserved.to_lowercase(), reserved.to_owned());
        }
        for file in &self.files {
            validate_portable_relative_path(&file.relative_path)?;
            if !seen.insert(file.relative_path.clone()) {
                return Err(ProjectExportError::DuplicateProjectPath {
                    relative_path: file.relative_path.clone(),
                    first: file.relative_path.clone(),
                    second: file.relative_path.clone(),
                });
            }
            let folded = file.relative_path.to_lowercase();
            if let Some(first) = seen_insensitive.insert(folded, file.relative_path.clone()) {
                return Err(ProjectExportError::DuplicateProjectPath {
                    relative_path: file.relative_path.clone(),
                    first,
                    second: file.relative_path.clone(),
                });
            }
        }
        for file in &self.support_files {
            validate_portable_relative_path(&file.relative_path)?;
            if file.before_file_index > self.files.len() {
                return Err(ProjectExportError::InvalidProjectPath {
                    relative_path: file.relative_path.clone(),
                    reason: "support-file insertion index exceeds the staged asset-file count",
                });
            }
            if !seen.insert(file.relative_path.clone()) {
                return Err(ProjectExportError::DuplicateProjectPath {
                    relative_path: file.relative_path.clone(),
                    first: file.relative_path.clone(),
                    second: file.relative_path.clone(),
                });
            }
            let folded = file.relative_path.to_lowercase();
            if let Some(first) = seen_insensitive.insert(folded, file.relative_path.clone()) {
                return Err(ProjectExportError::DuplicateProjectPath {
                    relative_path: file.relative_path.clone(),
                    first,
                    second: file.relative_path.clone(),
                });
            }
        }
        if self
            .support_files
            .windows(2)
            .any(|pair| pair[0].before_file_index > pair[1].before_file_index)
        {
            return Err(ProjectExportError::InvalidProjectPath {
                relative_path: "<support-files>".to_owned(),
                reason: "support files are not ordered by their staged insertion index",
            });
        }
        Ok(())
    }
}

fn validate_portable_relative_path(value: &str) -> Result<(), ProjectExportError> {
    let invalid = |reason: &'static str| ProjectExportError::InvalidProjectPath {
        relative_path: value.to_owned(),
        reason,
    };
    if value.is_empty() {
        return Err(invalid("path is empty"));
    }
    if value.contains('\\') {
        return Err(invalid("path separators must be portable '/'"));
    }
    if value.starts_with('/') || value.contains(':') {
        return Err(invalid("path must be relative to the project root"));
    }
    for component in value.split('/') {
        if component.is_empty() {
            return Err(invalid("path has an empty component"));
        }
        if component == "." || component == ".." {
            return Err(invalid("path must not contain '.' or '..' components"));
        }
        if component
            .chars()
            .any(|character| character.is_ascii_control())
        {
            return Err(invalid("path contains a control character"));
        }
    }
    Ok(())
}

/// `ProjectExporter` progress plus the write summary of one executed plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectExportReport {
    pub file_count: usize,
    pub asset_file_count: usize,
    pub support_file_count: usize,
    pub meta_file_count: usize,
    pub total_asset_bytes: usize,
    pub events: Vec<ProjectExportEvent>,
    pub skipped: Vec<ProjectExportNotice>,
    pub fallbacks: Vec<ProjectExportNotice>,
}

/// Execute one staged plan, mirroring `ExportHandler.Export`:
/// `prepare_export_root` (the consent/target guard), the export loop's file writes, then the
/// `ProjectVersion`/`PackageManifest` post-exporters.
pub fn execute_project_export(
    output_root: &Path,
    delete_existing: bool,
    plan: &ProjectExportPlan,
    limits: UnityProjectOutputLimits,
    yaml_limits: UnityYamlWriteLimits,
) -> Result<ProjectExportReport, Box<dyn Error>> {
    plan.validate()?;
    prepare_export_root(output_root, delete_existing)?;

    // `ExportHandler.Export` runs the export loop first and the post-exporters last. Writing
    // the two post-export files up front would be observable on a failed asset write, so the
    // same order is kept: every asset, then `ProjectVersion.txt`, then `manifest.json`.
    let mut events = vec![ProjectExportEvent::ExportStarted];
    let mut asset_file_count = 0usize;
    let mut meta_file_count = 0usize;
    let mut total_asset_bytes = 0usize;
    let mut support_file_count = 0usize;
    let mut next_support_file = 0usize;

    for (index, file) in plan.files.iter().enumerate() {
        while let Some(support) = plan.support_files.get(next_support_file) {
            if support.before_file_index != index {
                break;
            }
            write_new_project_file(output_root, &support.relative_path, &support.bytes, limits)?;
            support_file_count += 1;
            next_support_file += 1;
        }
        let bytes = file.bytes.as_slice();
        match &file.meta {
            Some(meta) => {
                let step = export_asset_with_meta(
                    output_root,
                    AssetExportWithMetaPlan {
                        relative_asset_path: file.relative_path.clone(),
                        guid: file.guid,
                        importer_class: meta.importer_class.clone(),
                        importer: meta.importer.clone(),
                        meta_options: UnityMetaFileOptions::default(),
                    },
                    true,
                    |path| write_staged_asset_bytes(path, bytes),
                    limits,
                    yaml_limits,
                )?;
                match step {
                    AssetExportStep::Exported { asset_bytes, .. } => {
                        asset_file_count += 1;
                        meta_file_count += 1;
                        total_asset_bytes += asset_bytes;
                    }
                    AssetExportStep::SkippedFailedExport => {}
                }
            }
            None => {
                write_new_project_file(output_root, &file.relative_path, bytes, limits)?;
                asset_file_count += 1;
                total_asset_bytes += bytes.len();
            }
        }
        events.push(ProjectExportEvent::ProgressUpdated {
            index,
            total: plan.files.len(),
        });
    }

    while let Some(support) = plan.support_files.get(next_support_file) {
        if support.before_file_index != plan.files.len() {
            return Err(ProjectExportError::InvalidProjectPath {
                relative_path: support.relative_path.clone(),
                reason: "support files must be ordered by their staged insertion index",
            }
            .into());
        }
        write_new_project_file(output_root, &support.relative_path, &support.bytes, limits)?;
        support_file_count += 1;
        next_support_file += 1;
    }

    write_new_project_file(
        output_root,
        PROJECT_VERSION_FILE,
        plan.project_version_file.as_bytes(),
        limits,
    )?;
    write_new_project_file(
        output_root,
        PACKAGE_MANIFEST_FILE,
        plan.package_manifest.as_bytes(),
        limits,
    )?;
    events.push(ProjectExportEvent::ExportFinished);

    let mut skipped = Vec::new();
    skipped.try_reserve_exact(plan.skipped.len()).map_err(|_| {
        ProjectExportError::AllocationFailed {
            resource: "project export notices",
        }
    })?;
    skipped.extend(plan.skipped.iter().cloned());
    let mut fallbacks = Vec::new();
    fallbacks
        .try_reserve_exact(plan.fallbacks.len())
        .map_err(|_| ProjectExportError::AllocationFailed {
            resource: "project export notices",
        })?;
    fallbacks.extend(plan.fallbacks.iter().cloned());

    Ok(ProjectExportReport {
        file_count: plan.files.len() + plan.support_files.len(),
        asset_file_count,
        support_file_count,
        meta_file_count,
        total_asset_bytes,
        events,
        skipped,
        fallbacks,
    })
}

/// Write the asset file already created with add-only semantics by `export_asset_with_meta`.
fn write_staged_asset_bytes(path: &Path, bytes: &[u8]) -> io::Result<usize> {
    let mut file = OpenOptions::new().write(true).open(path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(bytes.len())
}

/// `AssetExportCollection.CreateImporter` for the `NativeFormatImporter` family.
pub fn native_format_importer(main_file_id: i64) -> Result<UnityYamlNode, Box<dyn Error>> {
    native_format_importer_for_version(
        main_file_id,
        SceneUnityVersion {
            major: 2019,
            minor: 4,
            patch: 0,
        },
        None,
    )
}

/// Version-aware base importer fields from `NativeFormatImporter.Create`.
pub fn native_format_importer_for_version(
    main_file_id: i64,
    version: SceneUnityVersion,
    asset_bundle_name: Option<&str>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    if !unity_major_minor_at_least(version, 2018, 3) {
        importer.insert_field("name", UnityYamlNode::string(""))?;
    }
    if unity_major_minor_at_least(version, 2017, 2) {
        importer.insert_field(
            "externalObjects",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
    }
    if unity_major_minor_at_least(version, 2017, 1) {
        importer.insert_field(
            "mainObjectFileID",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(main_file_id)),
        )?;
    }
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        optional_importer_string(asset_bundle_name),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

/// `ShaderExportCollection.CreateImporter`'s versioned `ShaderImporter` defaults.
pub fn shader_importer_for_version(
    version: SceneUnityVersion,
    asset_bundle_name: Option<&str>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    shader_importer_for_version_with_non_modifiable_textures(
        version,
        asset_bundle_name,
        UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow),
    )
}

/// Versioned `ShaderImporter` defaults with the source Shader's exported texture map.
pub fn shader_importer_for_version_with_non_modifiable_textures(
    version: SceneUnityVersion,
    asset_bundle_name: Option<&str>,
    non_modifiable_textures: UnityYamlNode,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    let has_shader_importer_name = !unity_major_minor_at_least(version, 2018, 3);
    let has_external_objects = unity_major_minor_at_least(version, 2017, 2);
    let has_non_modifiable_textures = unity_major_minor_at_least(version, 2018, 1);
    if has_shader_importer_name {
        importer.insert_field("name", UnityYamlNode::string(""))?;
    }
    if has_external_objects {
        importer.insert_field(
            "externalObjects",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
        )?;
    }
    importer.insert_field(
        "defaultTextures",
        UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow),
    )?;
    if has_non_modifiable_textures {
        importer.insert_field("nonModifiableTextures", non_modifiable_textures)?;
    }
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        optional_importer_string(asset_bundle_name),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

fn optional_importer_string(value: Option<&str>) -> UnityYamlNode {
    match value {
        Some(value) => UnityYamlNode::string(value),
        None => UnityYamlNode::string(""),
    }
}

const fn unity_major_minor_at_least(version: SceneUnityVersion, major: u32, minor: u32) -> bool {
    version.major > major || (version.major == major && version.minor >= minor)
}

/// The baseline default `Packages/manifest.json` body for one Unity version.
pub fn package_manifest_json(project_version: SceneUnityVersion) -> String {
    let dependencies = default_package_dependencies(
        project_version.major > 2019
            || (project_version.major == 2019 && project_version.minor >= 2),
    );
    let mut manifest = String::from("{\n  \"dependencies\": {\n");
    for (index, (name, version)) in dependencies.iter().enumerate() {
        manifest.push_str("    \"");
        manifest.push_str(name);
        manifest.push_str("\": \"");
        manifest.push_str(version);
        manifest.push('"');
        if index + 1 != dependencies.len() {
            manifest.push(',');
        }
        manifest.push('\n');
    }
    manifest.push_str("  }\n}\n");
    manifest
}

/// The `ProjectVersionPostExporter` body for one editor version.
pub fn project_version_file(
    editor_version_text: &str,
    project_version: SceneUnityVersion,
) -> String {
    project_version_file_contents(editor_version_text, project_version.major == 5)
}

/// Plans collection destinations without touching the disk, tracking the planned tree so that
/// unique-name resolution matches the baseline's `Directory.Exists`/`File.Exists` probes.
pub struct ProjectPathPlanner {
    platform: HostPathPlatform,
    state: UniqueNameState,
    file_system: PlannedFileSystem,
}

impl ProjectPathPlanner {
    /// Create a planner for one host platform with default counter bounds.
    pub fn new(platform: HostPathPlatform) -> Self {
        Self {
            platform,
            state: UniqueNameState::new(UniqueNameLimits::default()),
            file_system: PlannedFileSystem::new(platform),
        }
    }

    /// The host platform of the running export.
    pub fn host_platform() -> HostPathPlatform {
        if cfg!(windows) {
            HostPathPlatform::Windows
        } else {
            HostPathPlatform::Unix
        }
    }

    /// `AssetExportCollection.Export` prefix: fixed directory plus unique file name.
    pub fn plan_asset_path(
        &mut self,
        identity: &ExportAssetPathIdentity,
        extension: &str,
    ) -> Result<String, ProjectExportError> {
        let planned = plan_asset_export_paths(
            identity,
            extension,
            self.platform,
            &mut self.state,
            &self.file_system,
            "",
        )
        .map_err(plan_error)?;
        self.file_system
            .directories
            .insert(planned_path_key(&planned.subdirectory, self.platform));
        self.file_system
            .files
            .insert(planned_path_key(&planned.file_path, self.platform));
        self.register(&planned.relative_asset_path)
    }

    /// `ManagerExportCollection.Export`: `ProjectSettings/<Corrected>.asset`, no unique name.
    pub fn plan_manager_path(&mut self, class_name: &str) -> Result<String, ProjectExportError> {
        let planned =
            plan_manager_export_paths(class_name, self.platform, "").map_err(|error| {
                ProjectExportError::InvalidProjectPath {
                    relative_path: class_name.to_owned(),
                    reason: match error {
                        super::project_plan::FixedPathError::EmptyScenePath => "empty scene path",
                        super::project_plan::FixedPathError::AllocationFailed { .. } => {
                            "could not allocate the manager destination"
                        }
                    },
                }
            })?;
        let portable = portable_path(&planned.relative_path);
        self.file_system
            .directories
            .insert(planned_path_key(&planned.subdirectory, self.platform));
        self.file_system
            .files
            .insert(planned_path_key(&planned.file_path, self.platform));
        self.register(&portable)
    }

    /// `UnknownExportCollection.Export`: `AssetRipper/UnknownAssets/<class>/<name>.unknown`.
    pub fn plan_unknown_path(
        &mut self,
        class_name: &str,
        asset_name: &str,
    ) -> Result<String, ProjectExportError> {
        let planned = plan_unknown_export_paths(
            "",
            class_name,
            asset_name,
            self.platform,
            &mut self.state,
            &self.file_system,
        )
        .map_err(plan_error)?;
        self.file_system
            .directories
            .insert(planned_path_key(&planned.subdirectory, self.platform));
        self.file_system
            .files
            .insert(planned_path_key(&planned.file_path, self.platform));
        self.register(&planned.relative_path)
    }

    /// `UnreadableExportCollection.Export`: `AssetRipper/UnreadableAssets/<class>/<name>.unreadable`.
    pub fn plan_unreadable_path(
        &mut self,
        class_name: &str,
        asset_name: &str,
    ) -> Result<String, ProjectExportError> {
        let planned = plan_unreadable_export_paths(
            "",
            class_name,
            asset_name,
            self.platform,
            &mut self.state,
            &self.file_system,
        )
        .map_err(plan_error)?;
        self.file_system
            .directories
            .insert(planned_path_key(&planned.subdirectory, self.platform));
        self.file_system
            .files
            .insert(planned_path_key(&planned.file_path, self.platform));
        self.register(&planned.relative_path)
    }

    fn register(&self, relative_path: &str) -> Result<String, ProjectExportError> {
        let portable = portable_path(relative_path);
        validate_portable_relative_path(&portable)?;
        Ok(portable)
    }
}

fn plan_error(error: super::asset_path::AssetPathError) -> ProjectExportError {
    match error {
        super::asset_path::AssetPathError::AllocationFailed { resource } => {
            ProjectExportError::AllocationFailed { resource }
        }
        _ => ProjectExportError::InvalidProjectPath {
            relative_path: String::new(),
            reason: "the source name rules rejected the destination",
        },
    }
}

fn portable_path(value: &str) -> String {
    value.replace('\\', "/")
}

/// Canonicalize an in-memory path key using the host's path separator rules.
///
/// Baseline paths can retain `/` inside a directory even on Windows, while `Path.Join` adds `\\`
/// between the directory and file name. Windows treats both characters as separators, so the
/// planner must canonicalize them before emulating `Directory.Exists` and `File.Exists`.
fn planned_path_key(value: &str, platform: HostPathPlatform) -> String {
    match platform {
        HostPathPlatform::Windows => value.replace('/', "\\"),
        HostPathPlatform::Unix => value.to_owned(),
    }
}

#[derive(Debug)]
struct PlannedFileSystem {
    platform: HostPathPlatform,
    directories: BTreeSet<String>,
    files: BTreeSet<String>,
}

impl PlannedFileSystem {
    fn new(platform: HostPathPlatform) -> Self {
        Self {
            platform,
            directories: BTreeSet::new(),
            files: BTreeSet::new(),
        }
    }
}

impl ExportFileSystem for PlannedFileSystem {
    fn directory_exists(&self, directory: &str) -> bool {
        self.directories
            .contains(&planned_path_key(directory, self.platform))
    }

    fn file_exists(&self, directory: &str, name: &str) -> bool {
        let path = join_path(directory, name, self.platform);
        self.files.contains(&planned_path_key(&path, self.platform))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::meta::{UnityMetaFile, UnityMetaFileOptions};

    fn defaults() -> ProjectExportSettings {
        ProjectExportSettings::from_export_settings(
            &ExportSettingsFile::from(crate::export::settings::ExportDefaults::default()),
            SceneUnityVersion::parse("2019.4.0f1").unwrap(),
        )
    }

    fn facts(class_id: i32) -> ObjectClassFacts {
        ObjectClassFacts::new(class_id, false)
    }

    #[test]
    fn generated_interface_table_covers_the_registration_targets() {
        // Spot-check the classes the dispatch depends on, straight from the generated assembly.
        assert!(class_interfaces(43).contains(&"IMesh"));
        assert!(class_interfaces(48).contains(&"IShader"));
        assert!(class_interfaces(83).contains(&"IAudioClip"));
        assert!(class_interfaces(1045).contains(&"IEditorBuildSettings"));
        assert!(class_interfaces(141).contains(&"IGlobalGameManager"));
        assert!(class_interfaces(141).contains(&"IBuildSettings"));
        assert!(class_interfaces(1).contains(&"IGameObject"));
        assert!(class_interfaces(4).contains(&"IComponent"));
        assert!(class_interfaces(1001).contains(&"IPrefabInstance"));
        assert!(class_interfaces(1032).contains(&"ISceneAsset"));
        assert!(class_interfaces(213).contains(&"ISprite"));
        assert_eq!(class_interfaces(999_999), &[] as &[&str]);
        assert!(is_generated_class(43));
        assert!(is_generated_class(PLAYER_SETTINGS_CLASS_ID));
        assert!(!is_generated_class(999_999));
    }

    #[test]
    fn shader_interface_takes_precedence_over_text_asset_interface() {
        assert!(class_interfaces(48).contains(&"IShader"));
        assert!(class_interfaces(48).contains(&"ITextAsset"));
        assert_eq!(claim_asset_kind(facts(48)), ClaimAssetKind::Shader);
    }

    #[test]
    fn class_names_prefer_the_generated_name_then_the_type_tree_root() {
        assert_eq!(object_class_name(43, Some("Mesh")), "Mesh");
        assert_eq!(object_class_name(43, None), "Mesh");
        assert_eq!(
            object_class_name(129, Some("PlayerSettings")),
            "PlayerSettings"
        );
        assert_eq!(object_class_name(129, None), "129");
        assert_eq!(object_class_name(999_999, Some("Root")), "Root");
        assert_eq!(object_class_name(999_999, None), "999999");
    }

    #[test]
    fn collection_selection_matches_the_reverse_stack_walk() {
        let settings = defaults();
        // The default exporter is the catch-all for plain objects.
        let go = select_collection(facts(1), settings).unwrap();
        assert_eq!(go.exporter, "DefaultYamlExporter");
        assert_eq!(go.collection_name, "AssetExportCollection");
        assert_eq!(go.kind(), ExportCollectionKind::Asset);

        // EditorBuildSettings claims the manager family through IGlobalGameManager.
        let ebs = select_collection(facts(1045), settings).unwrap();
        assert_eq!(ebs.exporter, "ManagerAssetExporter");
        assert_eq!(ebs.collection_name, "EditorBuildSettingsExportCollection");
        assert_eq!(ebs.kind(), ExportCollectionKind::Manager);

        // A level game manager has no SceneHierarchyObject main asset yet, so the scene
        // exporter declines and the plain YAML catch-all claims it.
        let manager = select_collection(facts(104), settings).unwrap();
        assert_eq!(manager.exporter, "DefaultYamlExporter");
        assert_eq!(manager.collection_name, "AssetExportCollection");
        assert_eq!(manager.kind(), ExportCollectionKind::Asset);

        // BuildSettings is shadowed by its IBuildSettings empty-collection dummy.
        let build = select_collection(facts(141), settings).unwrap();
        assert_eq!(build.exporter, "DummyAssetExporter");
        assert_eq!(build.collection_name, "EmptyExportCollection");
        assert!(!build.kind().is_exportable());

        // AssetBundle / PreloadData / MonoManager / ResourceManager / ShaderNameRegistry dummies.
        for class_id in [142, 150, 116, 147, 94] {
            let selection = select_collection(facts(class_id), settings).unwrap();
            assert_eq!(selection.exporter, "DummyAssetExporter", "class {class_id}");
            assert_eq!(
                selection.collection_name, "EmptyExportCollection",
                "class {class_id}"
            );
        }

        // Meshes and image textures go to the streamed YAML collection.
        let mesh = select_collection(facts(43), settings).unwrap();
        assert_eq!(mesh.collection_name, "YamlStreamedAssetExportCollection");
        let texture = select_collection(facts(28), settings).unwrap();
        assert_eq!(texture.collection_name, "YamlStreamedAssetExportCollection");

        // Shaders claim the shader collection under the default Dummy mode.
        let shader = select_collection(facts(48), settings).unwrap();
        assert_eq!(shader.collection_name, "ShaderExportCollection");
        assert_eq!(shader.kind(), ExportCollectionKind::Asset);

        // Audio clips fall through the undecodable converted arm to the YAML audio exporter.
        let audio = select_collection(facts(83), settings).unwrap();
        assert_eq!(audio.collection_name, "YamlAudioExportCollection");

        // MonoScripts claim the empty-script collection without an assembly manager.
        let script = select_collection(facts(115), settings).unwrap();
        assert_eq!(script.collection_name, "EmptyScriptExportCollection");
        assert_eq!(script.kind(), ExportCollectionKind::EmptyScript);

        // Sprite atlases go to the shared empty collection; sprites to a plain asset collection.
        let atlas = select_collection(facts(687_078_895), settings).unwrap();
        assert_eq!(atlas.collection_name, "EmptyExportCollection");
        let sprite = select_collection(facts(213), settings).unwrap();
        assert_eq!(sprite.collection_name, "AssetExportCollection");
    }

    #[test]
    fn unreadable_and_unknown_objects_follow_the_unreadable_assets_gate() {
        let off = defaults();
        assert!(!off.export_unreadable_assets);
        let unknown = ObjectClassFacts::new(999_999, false);
        let selection = select_collection(unknown, off).unwrap();
        assert_eq!(selection.exporter, "DummyAssetExporter");
        assert_eq!(selection.collection_name, "SkipExportCollection");

        let mut on = off;
        on.export_unreadable_assets = true;
        let unknown = ObjectClassFacts::new(999_999, false);
        let selection = select_collection(unknown, on).unwrap();
        assert_eq!(selection.collection_name, "UnknownExportCollection");
        assert_eq!(selection.kind(), ExportCollectionKind::Unknown);

        let unreadable = ObjectClassFacts::new(43, true);
        let selection = select_collection(unreadable, on).unwrap();
        assert_eq!(selection.collection_name, "UnreadableExportCollection");
        assert_eq!(selection.kind(), ExportCollectionKind::Unreadable);
        let selection = select_collection(unreadable, off).unwrap();
        assert_eq!(selection.exporter, "DummyAssetExporter");
    }

    #[test]
    fn sprite_and_shader_mode_gates_flip_the_selection() {
        let mut settings = defaults();
        settings.sprite_export_mode_yaml = false;
        let sprite = select_collection(facts(213), settings).unwrap();
        // TextureAssetExporter needs sprite-information mains, so the sprite still lands in the
        // plain asset collection when the YAML sprite exporter is gated off.
        assert_eq!(sprite.collection_name, "AssetExportCollection");

        settings.shader_export_mode_yaml = true;
        let shader = select_collection(facts(48), settings).unwrap();
        assert_eq!(shader.exporter, "YamlShaderExporter");
        assert_eq!(shader.collection_name, "ShaderExportCollection");
    }

    #[test]
    fn shader_and_yaml_shader_importers_match_reference_project_probe() {
        let fixture = include_str!("shader_project_fixture.txt");
        let mut case = None;
        let mut compared_cases = 0;

        for row in fixture.lines() {
            if let Some(header) = row.strip_prefix("CASE|") {
                let (mode, version) = header
                    .split_once('|')
                    .expect("fixture case row has mode and Unity version");
                case = Some((mode, version, SceneUnityVersion::parse(version).unwrap()));
                continue;
            }

            let Some(meta_row) = row.strip_prefix("META|") else {
                continue;
            };
            let (path, expected_escaped) = meta_row
                .split_once('|')
                .expect("fixture meta row has a path and normalized sidecar");
            assert!(
                path.ends_with("Test_Simple.shader.meta")
                    || path.ends_with("Test_Simple.asset.meta")
            );
            let (mode, version_text, version) =
                case.expect("fixture meta row follows a case header");
            let (importer_class, importer) = match mode {
                "dummy" => (
                    "ShaderImporter",
                    shader_importer_for_version(version, None).unwrap(),
                ),
                "yaml" => (
                    "NativeFormatImporter",
                    native_format_importer_for_version(4_800_000, version, None).unwrap(),
                ),
                _ => panic!("unexpected fixture mode {mode:?}"),
            };
            let meta = UnityMetaFile::new(
                [0x7f; 16],
                importer_class,
                importer,
                UnityMetaFileOptions {
                    time_created_unix_seconds: Some(0),
                    ..Default::default()
                },
            )
            .unwrap()
            .to_yaml(UnityYamlWriteLimits::default())
            .unwrap();
            let normalized = meta
                .lines()
                .filter(|line| !line.starts_with("guid: ") && !line.starts_with("timeCreated: "))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            assert_eq!(
                normalized,
                expected_escaped.replace("\\n", "\n"),
                "source project importer mismatch for {mode} {version_text}"
            );
            compared_cases += 1;
        }

        assert_eq!(
            compared_cases, 18,
            "all source probe importer rows are checked"
        );
    }

    #[test]
    fn export_extensions_follow_the_marker_order() {
        assert_eq!(export_extension_for(facts(48)), "shader");
        assert_eq!(export_extension_for(facts(21)), "mat");
        assert_eq!(export_extension_for(facts(74)), "anim");
        assert_eq!(export_extension_for(facts(91)), "controller");
        assert_eq!(export_extension_for(facts(89)), "cubemap");
        assert_eq!(export_extension_for(facts(28)), "texture2D");
        assert_eq!(export_extension_for(facts(84)), "renderTexture");
        assert_eq!(export_extension_for(facts(1)), "asset");
        assert_eq!(export_extension_for(facts(43)), "asset");
    }

    #[test]
    fn path_planner_detects_repeated_windows_paths_across_separator_forms() {
        let identity = ExportAssetPathIdentity {
            class_name: "GameObject".to_owned(),
            named_name: Some("Avatar".to_owned()),
            ..ExportAssetPathIdentity::default()
        };
        let mut planner = ProjectPathPlanner::new(HostPathPlatform::Windows);

        assert_eq!(
            planner.plan_asset_path(&identity, "asset").unwrap(),
            "Assets/GameObject/Avatar.asset"
        );
        assert_eq!(
            planner.plan_asset_path(&identity, "asset").unwrap(),
            "Assets/GameObject/Avatar_0.asset"
        );
    }

    #[test]
    fn portable_paths_reject_escapes_and_collisions() {
        assert!(validate_portable_relative_path("Assets/GameObject/Thing.asset").is_ok());
        for bad in [
            "",
            "/absolute",
            "Assets\\Thing.asset",
            "Assets/../Thing.asset",
            "Assets/./Thing.asset",
            "C:/Thing.asset",
            "Assets//Thing.asset",
        ] {
            assert!(
                validate_portable_relative_path(bad).is_err(),
                "{bad:?} must be rejected"
            );
        }
        let plan = ProjectExportPlan {
            project_version_text: "2019.4.0f1".to_owned(),
            project_version_file: String::new(),
            package_manifest: String::new(),
            files: vec![
                planned_file("Assets/A/x.asset"),
                planned_file("Assets/a/x.asset"),
            ],
            support_files: Vec::new(),
            skipped: Vec::new(),
            fallbacks: Vec::new(),
        };
        assert!(matches!(
            plan.validate(),
            Err(ProjectExportError::DuplicateProjectPath { .. })
        ));
        let plan = ProjectExportPlan {
            files: vec![planned_file("ProjectSettings/ProjectVersion.txt")],
            ..plan
        };
        assert!(matches!(
            plan.validate(),
            Err(ProjectExportError::DuplicateProjectPath { .. })
        ));
    }

    fn planned_file(path: &str) -> PlannedProjectFile {
        PlannedProjectFile {
            collection_name: "AssetExportCollection",
            kind: ExportCollectionKind::Asset,
            relative_path: path.to_owned(),
            guid: [7; 16],
            bytes: Vec::new(),
            meta: None,
            label: path.to_owned(),
        }
    }
}
