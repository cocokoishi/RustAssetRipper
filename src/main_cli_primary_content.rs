//! `/Export/PrimaryContent`: the primary-content export driver.
//!
//! The reference's `PrimaryContentExporter` walks every object, asks the handler stack for the first
//! extractor that will take it, and writes one file per collection. The pipeline pieces are in the
//! library; this module is the caller that supplies what only a loaded game can supply - each
//! object's decoded payload, its SerializedFile's version and platform, and the resolved streamed
//! resources.
//!
//! Two things here are deliberate and worth reading before changing them.
//!
//! **Objects whose facts cannot be determined are reported, not exported.** The model extractor
//! decides on the scene-or-prefab hierarchy relation, which this path does not compute, and a
//! streamed texture needs a resource lookup that is not wired here. Both are counted and printed. The
//! alternative - declining them into the JSON fallback - would write files the reference never writes
//! and call the run a success, which is exactly the silent degradation the acceptance rules forbid.
//!
//! **A collection whose extractor has no producer is counted as unimplemented, never as written.**
//! The script extractor waits on the decompiler decision, and the terrain heatmap needs the image
//! writer joined to the bitmap that already exists.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};

use assetripper::export::asset_path::{ExportFileSystem, HostPathPlatform};
use assetripper::export::primary_content::{
    create_primary_content_collections_for_objects, PrimaryContentCollection,
    PrimaryContentExportStep, PrimaryContentObject,
};
use assetripper::export::primary_export::{
    claim_for_extractor, export_primary_content, json_content_document, object_facts_from_payload,
    payload_kind, PrimaryContentObjectContext, PrimaryContentObjectFacts,
    PrimaryContentObjectFactsOutcome, PrimaryContentPayload, PrimaryContentPayloadKind,
    PrimaryContentProduced, PrimaryContentRun,
};
use assetripper::export::primary_extractors::ScriptExtractorState;
use assetripper::export::project_exporter::{
    object_class_facts, object_class_name, ObjectClassFacts,
};
use assetripper::export::res_wiring::{
    plan_binary_export, plan_resource_export, ResourceExportLimits, ResourceExportRequest,
    TEXTURE2D_CLASS_ID,
};
use assetripper::export::settings_file::SerializedSettingsFile;
use assetripper::resources::vertex_data_blob::{EndianType, UnityVersion};
use assetripper::serialized_file::object::SerializedValue;

use super::export_commands::LoadedGame;

/// What the driver needs to know about one object beyond its decoded payload.
struct ObjectEntry {
    class_id: i32,
    named_name: String,
    version: UnityVersion,
    endian: EndianType,
    platform: i32,
    platform_blob_little_endian: bool,
    /// The generated-type facts, which do not borrow the payload.
    class_facts: ObjectClassFacts,
}

/// One object the driver could not decide, with the reason.
struct UndecidableObject {
    name: String,
    reason: &'static str,
}

/// Export the loaded game's primary content to `output_root`.
///
/// `status_output` receives the run's progress lines plus the counts this port must be honest about:
/// undecidable objects, unimplemented extractors and failures.
pub(super) fn export_loaded_primary_content<W: io::Write>(
    game: &LoadedGame,
    output_root: &Path,
    delete_existing: bool,
    full_settings: &SerializedSettingsFile,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    // The three lines `GameFileLoader.ExportPrimaryContent` logs, in its order.
    writeln!(status_output, "Starting primary content export")?;
    writeln!(
        status_output,
        "Attempting to export assets to {:?}...",
        output_root
    )?;
    let objects = game.objects.objects();
    let files = game.workspace.files();

    // The adapter's parallel pieces. `names` and `members` must outlive the objects that borrow
    // them, so they are built completely before the objects are constructed.
    let mut names: Vec<String> = Vec::with_capacity(objects.len());
    let mut members: Vec<Vec<u64>> = Vec::with_capacity(objects.len());
    let mut entries: HashMap<u64, ObjectEntry> = HashMap::new();
    let mut facts_by_identity: HashMap<u64, PrimaryContentObjectFacts<'_>> = HashMap::new();
    let mut payloads: HashMap<u64, &SerializedValue> = HashMap::new();
    let mut undecidable: Vec<UndecidableObject> = Vec::new();

    for object in objects {
        let identity = object_identity(object.file_index, object.object_index);
        let class_id = object.factory.class_id;
        let file = files.get(object.file_index);
        let version = file
            .and_then(|file| file.metadata.prefix.unity_version_text.as_deref())
            .and_then(UnityVersion::parse);
        let (Some(file), Some(version)) = (file, version) else {
            // Without the owning file's version the mesh reader cannot branch, and a payload's
            // fields cannot be trusted to mean what they meant when written.
            undecidable.push(UndecidableObject {
                name: object_class_name(class_id, None),
                reason: "the owning SerializedFile has no readable Unity version",
            });
            continue;
        };
        let endian = if file.metadata.prefix.body_endian == assetripper::io::binary::Endian::Little
        {
            EndianType::Little
        } else {
            EndianType::Big
        };
        // The file's own metadata prefix is the honest available equivalent of the collection's
        // platform, which is what the existing texture path already uses; discriminant 0 matches no
        // build target and transforms nothing.
        let platform = file
            .metadata
            .prefix
            .target_platform
            .and_then(|platform| i32::try_from(platform).ok())
            .unwrap_or(0);
        let named_name = object
            .payload
            .as_ref()
            .ok()
            .and_then(|payload| object_name(&payload.root))
            .unwrap_or_default();

        let Some(payload) = object.payload.as_ref().ok() else {
            undecidable.push(UndecidableObject {
                name: best_name(&named_name, class_id, None),
                reason: "the object's payload did not decode",
            });
            continue;
        };

        // A streamed texture's integrity depends on resolving `m_StreamData` against the resource
        // files, which this path does not do yet. Reporting it is the honest alternative to guessing:
        // guessing "intact" writes pixels from an unresolved stream, and guessing "not intact" hands
        // the texture to the JSON fallback.
        if class_id == TEXTURE2D_CLASS_ID && has_field(&payload.root, "m_StreamData") {
            undecidable.push(UndecidableObject {
                name: best_name(&named_name, class_id, None),
                reason: "the texture streams its pixels and the resource lookup is not wired here",
            });
            continue;
        }

        let context = PrimaryContentObjectContext { version, endian };
        match object_facts_from_payload(class_id, &payload.root, context, || None) {
            PrimaryContentObjectFactsOutcome::Known(facts) => {
                names.push(best_name(&named_name, class_id, None));
                // A single-asset collection claims only its primary. The hierarchy collections,
                // which claim their members, are the ones this path cannot build yet.
                members.push(vec![identity]);
                facts_by_identity.insert(identity, facts);
                payloads.insert(identity, &payload.root);
                entries.insert(
                    identity,
                    ObjectEntry {
                        class_id,
                        named_name,
                        version,
                        endian,
                        platform,
                        platform_blob_little_endian: endian == EndianType::Little,
                        class_facts: object_class_facts(&object.factory.kind, class_id, true),
                    },
                );
            }
            PrimaryContentObjectFactsOutcome::Undecidable { reason } => {
                undecidable.push(UndecidableObject {
                    name: best_name(&named_name, class_id, None),
                    reason,
                });
            }
        }
    }

    if names.is_empty() {
        writeln!(
            status_output,
            "No primary content is exportable: all {} objects were undecidable.",
            undecidable.len()
        )?;
        report_undecidable(status_output, &undecidable)?;
        return Ok(());
    }

    // The objects borrow `names`, `members` and `facts_by_identity`, all complete by now.
    let content_objects: Vec<PrimaryContentObject<'_>> = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let identity = members[index][0];
            PrimaryContentObject {
                identity,
                facts: entries[&identity].class_facts,
                collection_name: name,
                collection_assets: &members[index],
            }
        })
        .collect();

    let mut script_state = ScriptExtractorState::new();
    let collections = create_primary_content_collections_for_objects(
        &content_objects,
        |registration, object| {
            let facts = facts_by_identity
                .get(&object.identity)
                .copied()
                .unwrap_or_else(empty_facts);
            claim_for_extractor(registration, facts, &mut script_state)
        },
    )?;

    let steps: Vec<PrimaryContentExportStep<'_>> = collections
        .iter()
        .map(|collection| PrimaryContentExportStep {
            collection,
            // Every collection `CreateCollections` kept is exportable: the deleted-assets
            // collection, whose `Exportable` is false, never reaches this list.
            exportable: true,
        })
        .collect();

    let project_directory = output_root.to_string_lossy().into_owned();
    let file_system = RunFileSystem::new(output_root.to_path_buf());
    let limits = ResourceExportLimits::default();
    let export_settings = &full_settings.export;

    let mut run = export_primary_content(
        &collections,
        &steps,
        &project_directory,
        HostPathPlatform::host(),
        &file_system,
        |collection| produce(collection, &entries, &payloads, export_settings, limits),
        |relative_path, bytes| {
            let path = file_system.resolve(relative_path);
            if let Some(parent) = path.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    return Err(format!("could not create {}: {error}", parent.display()));
                }
            }
            match std::fs::write(&path, bytes) {
                Ok(()) => {
                    file_system.record(relative_path);
                    Ok(())
                }
                Err(error) => Err(format!("could not write {}: {error}", path.display())),
            }
        },
    );

    report_run(status_output, &mut run, output_root)?;
    report_undecidable(status_output, &undecidable)?;
    writeln!(status_output, "Finished exporting primary content.")?;
    if delete_existing {
        // The caller deleted the directory before the run; nothing to do here, but the flag is
        // accepted so this driver matches the project export's signature and gate.
    }
    Ok(())
}

/// Produce one collection's payload from the producer map.
///
/// The map is: JSON through the document producer, the resource families through the existing
/// per-asset planner, the binary arms through the binary planner, and everything else explicitly
/// unimplemented. A planner answering `None` means "not this producer's business" - the hierarchy
/// GLB and the material text asset - which is reported as unimplemented rather than as failure.
fn produce(
    collection: &PrimaryContentCollection,
    entries: &HashMap<u64, ObjectEntry>,
    payloads: &HashMap<u64, &SerializedValue>,
    settings: &assetripper::export::settings_file::ExportSettingsFile,
    limits: ResourceExportLimits,
) -> PrimaryContentPayload {
    let Some(entry) = entries.get(&collection.primary) else {
        return PrimaryContentPayload::Failed;
    };
    let Some(root) = payloads.get(&collection.primary) else {
        return PrimaryContentPayload::Failed;
    };
    let Some(kind) = payload_kind(collection.extractor) else {
        return PrimaryContentPayload::Unimplemented;
    };

    match kind {
        PrimaryContentPayloadKind::Json => {
            let bytes = json_content_document(root).into_bytes();
            produced(collection, entry, "json", bytes)
        }
        PrimaryContentPayloadKind::Binary => {
            match plan_binary_export(&collection.collection_name(), entry.class_id, root, None) {
                Ok(Some(planned)) => planned_payload(collection, entry, &planned),
                // A declined binary arm is a gate the reference also declines; the collection would
                // not have been created for it, so this is a planner/caller disagreement.
                Ok(None) => PrimaryContentPayload::Unimplemented,
                Err(_) => PrimaryContentPayload::Failed,
            }
        }
        PrimaryContentPayloadKind::Image
        | PrimaryContentPayloadKind::Glb
        | PrimaryContentPayloadKind::Audio => {
            let request = ResourceExportRequest {
                name: &collection.collection_name(),
                class_id: entry.class_id,
                root,
                version: entry.version,
                endian: entry.endian,
                stream_content: None,
                platform: entry.platform,
                platform_blob_little_endian: entry.platform_blob_little_endian,
                image_format:
                    assetripper::resources::image_export::ImageExportFormat::from_settings_format(
                        settings.image_export_format,
                    ),
                shader_mode: settings.shader_export_mode,
                limits,
            };
            match plan_resource_export(&request) {
                Ok(Some(planned)) => planned_payload(collection, entry, &planned),
                // `None` here is the hierarchy GLB and the material text asset: neither is this
                // planner's business, and neither has a producer on this path.
                Ok(None) => PrimaryContentPayload::Unimplemented,
                Err(_) => PrimaryContentPayload::Failed,
            }
        }
        // The script producer waits on the decompiler decision, and the heatmap producer needs the
        // bitmap joined to the image writer.
        PrimaryContentPayloadKind::Scripts => PrimaryContentPayload::Unimplemented,
    }
}

/// A produced payload from a planner's file name and bytes, whose extension is the file name's.
fn planned_payload(
    collection: &PrimaryContentCollection,
    entry: &ObjectEntry,
    planned: &assetripper::export::res_wiring::PlannedResourceExport,
) -> PrimaryContentPayload {
    let extension = planned
        .file_name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_owned())
        .unwrap_or_default();
    produced(collection, entry, &extension, planned.bytes.clone())
}

/// A produced payload with an explicit extension.
///
/// The identity is the asset's best name and class name, which is what the destination planner
/// resolves `Assets/<class>/<name>.<extension>` from - so the class name is not decoration here, it
/// is the directory the reference would write into.
fn produced(
    collection: &PrimaryContentCollection,
    entry: &ObjectEntry,
    extension: &str,
    bytes: Vec<u8>,
) -> PrimaryContentPayload {
    let _ = &entry.named_name;
    PrimaryContentPayload::Produced(Box::new(PrimaryContentProduced {
        identity: assetripper::export::asset_path::ExportAssetPathIdentity {
            class_name: object_class_name(entry.class_id, None),
            named_name: Some(collection.collection_name()),
            ..Default::default()
        },
        extension: extension.to_owned(),
        bytes,
    }))
}

/// Facts for an object the driver decided against; unreachable in a well-formed run.
fn empty_facts<'a>() -> PrimaryContentObjectFacts<'a> {
    PrimaryContentObjectFacts {
        model_main_asset: assetripper::export::primary_extractors::GlbModelMainAsset::Other,
        main_asset_is_deleted_information: false,
        mesh_is_set: false,
        height_meshes_present: false,
        is_terrain_data: false,
        image_texture_intact: false,
        is_audio_clip: false,
        is_mono_script: false,
        binary: None,
    }
}

/// Print the run's counts, and the lines the reference logs.
fn report_run<W: io::Write>(
    status_output: &mut W,
    run: &mut PrimaryContentRun,
    output_root: &Path,
) -> Result<(), io::Error> {
    writeln!(
        status_output,
        "Wrote {} primary-content files to {:?}.",
        run.written.len(),
        output_root
    )?;
    if !run.unimplemented.is_empty() {
        writeln!(
            status_output,
            "{} collections were not written because this port has no producer for their extractor \
             yet (nothing was written for them):",
            run.unimplemented.len()
        )?;
        for name in run.unimplemented.iter().take(10) {
            writeln!(status_output, "  {name}")?;
        }
    }
    if !run.failures.is_empty() {
        writeln!(status_output, "{} collections failed:", run.failures.len())?;
        for failure in run.failures.iter().take(10) {
            writeln!(status_output, "  {}: {}", failure.name, failure.reason)?;
        }
    }
    Ok(())
}

/// Print the objects that could not be decided, so the gap is visible in the run.
fn report_undecidable<W: io::Write>(
    status_output: &mut W,
    undecidable: &[UndecidableObject],
) -> Result<(), io::Error> {
    if undecidable.is_empty() {
        return Ok(());
    }
    writeln!(
        status_output,
        "{} objects could not be decided and were not exported. This port reports them rather than \
         exporting them as something the reference never would:",
        undecidable.len()
    )?;
    let mut reasons: HashMap<&'static str, usize> = HashMap::new();
    for object in undecidable {
        *reasons.entry(object.reason).or_default() += 1;
    }
    let mut reasons: Vec<(&'static str, usize)> = reasons.into_iter().collect();
    reasons.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(right.0)));
    for (reason, count) in reasons {
        writeln!(status_output, "  {count} x {reason}")?;
    }
    // The names are printed for the first few so a run can be traced to objects, not just counts.
    for object in undecidable.iter().take(5) {
        writeln!(status_output, "    e.g. {}", object.name)?;
    }
    Ok(())
}

/// A stable identity for the `queued` set, from the object's position.
///
/// The reference uses reference identity. A workspace object's `(file, object)` position is stable
/// within one load and unique, which is all the queued set needs.
const fn object_identity(file_index: usize, object_index: usize) -> u64 {
    ((file_index as u64) << 32) | (object_index as u64 & 0xFFFF_FFFF)
}

/// The object's decoded `m_Name`, when it has one.
fn object_name(root: &SerializedValue) -> Option<String> {
    let value = root.unique_field_in_base_chain("m_Name").ok().flatten()?;
    match value {
        SerializedValue::StringBytes(bytes) => std::str::from_utf8(bytes).ok().map(str::to_owned),
        _ => None,
    }
}

/// Whether a field is present at all, which is what `Has_StreamData` asks.
fn has_field(root: &SerializedValue, name: &str) -> bool {
    root.struct_fields()
        .is_some_and(|fields| fields.iter().any(|field| field.name == name))
}

/// `AssetBase.GetBestName()`: the name, or the class name when the asset has none.
fn best_name(named_name: &str, class_id: i32, type_tree_root: Option<&str>) -> String {
    if named_name.is_empty() {
        object_class_name(class_id, type_tree_root)
    } else {
        named_name.to_owned()
    }
}

/// The run's own writes, on top of the real directory.
///
/// The unique-name search needs a probe that reflects **this run's** writes, not just the disk: the
/// first asset in a fresh directory takes the plain name and creates the directory, and later assets
/// must see it. A probe that only read the disk would let two assets collide.
struct RunFileSystem {
    root: PathBuf,
    written: RefCell<HashSet<String>>,
    created: RefCell<HashSet<String>>,
}

impl RunFileSystem {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            written: RefCell::new(HashSet::new()),
            created: RefCell::new(HashSet::new()),
        }
    }

    fn resolve(&self, relative_path: &str) -> PathBuf {
        self.root
            .join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    fn record(&self, relative_path: &str) {
        self.written.borrow_mut().insert(relative_path.to_owned());
        if let Some((directory, _)) = relative_path.rsplit_once('/') {
            self.created.borrow_mut().insert(directory.to_owned());
        }
    }
}

impl ExportFileSystem for RunFileSystem {
    fn directory_exists(&self, directory: &str) -> bool {
        self.created.borrow().contains(directory) || self.resolve(directory).is_dir()
    }

    fn file_exists(&self, directory: &str, name: &str) -> bool {
        let relative = if directory.is_empty() {
            name.to_owned()
        } else {
            format!("{directory}/{name}")
        };
        self.written.borrow().contains(&relative) || self.resolve(&relative).is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::{best_name, has_field, object_identity, object_name, RunFileSystem};
    use assetripper::export::asset_path::ExportFileSystem;
    use assetripper::serialized_file::object::{SerializedFieldValue, SerializedValue};

    fn payload(fields: Vec<(&str, SerializedValue)>) -> SerializedValue {
        SerializedValue::Struct(
            fields
                .into_iter()
                .map(|(name, value)| SerializedFieldValue {
                    name: name.to_owned(),
                    value,
                })
                .collect(),
        )
    }

    /// The identity packs the object's position and stays unique across files.
    #[test]
    fn the_identity_is_stable_and_unique() {
        assert_eq!(object_identity(0, 0), 0);
        assert_eq!(object_identity(0, 7), 7);
        assert_eq!(object_identity(1, 0), 1 << 32);
        assert_ne!(object_identity(3, 4), object_identity(4, 3));
    }

    /// A decoded `m_Name` is read, and an absent or non-string one is not invented.
    #[test]
    fn the_name_is_read_or_left_empty() {
        assert_eq!(
            object_name(&payload(vec![(
                "m_Name",
                SerializedValue::StringBytes(b"Body".to_vec())
            )])),
            Some("Body".to_owned())
        );
        assert_eq!(object_name(&payload(vec![])), None);
        assert_eq!(
            object_name(&payload(vec![("m_Name", SerializedValue::I32(3))])),
            None,
            "a non-string name is not coerced"
        );
        // An empty name is a real name and must not be confused with an absent one.
        assert_eq!(
            object_name(&payload(vec![(
                "m_Name",
                SerializedValue::StringBytes(Vec::new())
            )])),
            Some(String::new())
        );
    }

    /// `GetBestName` falls back to the class name only when there is no name at all.
    #[test]
    fn the_best_name_falls_back_to_the_class_name() {
        assert_eq!(best_name("Body", 43, None), "Body");
        assert_eq!(best_name("", 43, None), "Mesh");
        assert_eq!(best_name("", 28, None), "Texture2D");
    }

    /// `Has_StreamData` asks whether the field is there, not whether it is meaningful.
    #[test]
    fn the_stream_field_is_detected_by_presence() {
        assert!(has_field(
            &payload(vec![("m_StreamData", SerializedValue::Struct(vec![]))]),
            "m_StreamData"
        ));
        assert!(!has_field(&payload(vec![]), "m_StreamData"));
    }

    /// The probe reflects the run's own writes, which is what the collision rules depend on.
    #[test]
    fn the_probe_sees_the_runs_own_writes() {
        let probe = RunFileSystem::new(std::env::temp_dir().join("rust-assetripper-probe-test"));
        assert!(!probe.directory_exists("Assets"));
        assert!(!probe.file_exists("Assets", "Body.json"));

        probe.record("Assets/Body.json");
        assert!(probe.file_exists("Assets", "Body.json"));
        assert!(
            probe.directory_exists("Assets"),
            "the write created the directory"
        );
        // A different file in the same directory is still free.
        assert!(!probe.file_exists("Assets", "Other.json"));
    }
}
