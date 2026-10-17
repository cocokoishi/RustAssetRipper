//! The primary-content export driver: collection to payload to file.
//!
//! `PrimaryContentExporter.Export` is three steps, and the port had only the middle one. The loop
//! was ported (`primary_content::run_primary_content_export_loop`), the destination selection was
//! ported (`primary_content::plan_single_export_paths`), and the extractor decisions were ported -
//! but nothing connected them, so no collection could actually reach a file. This module is that
//! connection, and it is the function the `/Export/PrimaryContent` route will call.
//!
//! ```text
//! collection -> produce (caller: decoded asset data) -> plan path -> write (caller: filesystem)
//! ```
//!
//! **Two boundaries are deliberate.** Producing the payload needs decoded asset data - a
//! `MeshData`, a decoded bitmap, a serialized document - which lives with the asset model and not
//! here, so the producer is a caller closure. Writing needs a real filesystem, which the port keeps
//! out of the export layer for testability, so the write is a caller closure too. Everything in
//! between - which extractor produces what, the destination, the unique-name budget, the progress
//! events and the failure accounting - is fixed here.
//!
//! **An extractor with no producer is recorded, never skipped.** `ScriptContentExtractor` needs a C#
//! decompiler that the port does not have (see `TYPE-SCRIPT-DECOMPILE-001`), and the honest way to
//! report that is an `Unimplemented` entry plus a failed export event. Dropping the collection
//! silently would be indistinguishable from a successful export, which is exactly the degradation
//! the project forbids.

use super::asset_path::{
    ExportAssetPathIdentity, ExportFileSystem, HostPathPlatform, UniqueNameState,
};
use super::json_walker::{write_json_body, JsonWalker};
use super::primary_content::{
    plan_single_export_paths, run_primary_content_export_loop, PlannedSingleAssetPaths,
    PrimaryContentCollection, PrimaryContentExportEvent, PrimaryContentExportStep,
};
use super::primary_content::{PrimaryContentClaim, PrimaryHandlerRegistration};
use super::primary_extractors::{
    binary_asset_claims, deleted_assets_claim, glb_asset_claim,
    image_texture_check_asset_integrity, BinaryAssetFacts, GlbAssetFacts, GlbModelMainAsset,
    ScriptClaim, ScriptExtractorState,
};
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
use crate::serialized_file::object::SerializedValue;

/// The payload each primary-content extractor produces, taken from its collection class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimaryContentPayloadKind {
    /// `JsonExportCollection`: one JSON document per asset.
    Json,
    /// `GlbExportCollection`, shared by all four GLB exporters.
    Glb,
    /// `TextAssetExportCollection`, `FontExportCollection`, `MovieTextureExportCollection` and
    /// `VideoClipExportCollection`: the asset's own bytes.
    Binary,
    /// `AudioExportCollection`: decoded audio, with the extension the decoder reported.
    Audio,
    /// `ImageExportCollection`: an image in the configured format.
    Image,
    /// `TerrainHeatmapExportCollection`: a heatmap image in the configured format.
    TerrainHeatmap,
    /// `ScriptExportCollection`: assemblies, decompiled sources and a solution file.
    Scripts,
}

/// `payload_kind` for an extractor name.
///
/// `None` for `EmptyContentExtractor` and `DeletedAssetsExporter`, which cannot reach the loop:
/// the empty handler claims into the shared empty collection, which `CreateCollections` drops
/// without queueing, and the deleted-assets collection reports `Exportable => false`.
pub fn payload_kind(extractor: &str) -> Option<PrimaryContentPayloadKind> {
    Some(match extractor {
        "JsonContentExtractor" => PrimaryContentPayloadKind::Json,
        "GlbMeshExporter" | "GlbNavMeshExporter" | "GlbTerrainExporter" | "GlbModelExporter" => {
            PrimaryContentPayloadKind::Glb
        }
        "BinaryAssetContentExtractor" => PrimaryContentPayloadKind::Binary,
        "AudioContentExtractor" => PrimaryContentPayloadKind::Audio,
        "TextureExporter" => PrimaryContentPayloadKind::Image,
        "TerrainHeatmapExporter" => PrimaryContentPayloadKind::TerrainHeatmap,
        "ScriptContentExtractor" => PrimaryContentPayloadKind::Scripts,
        _ => return None,
    })
}

/// Everything needed to write one collection's payload.
///
/// The extension is part of the payload rather than a constant because it is asset-derived for
/// several extractors: a font's `otf`/`ttf` comes from its four-character code, a video clip's from
/// its original path, a texture's from the configured format, and an audio clip's from the decoder.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrimaryContentProduced {
    /// The identity the destination is planned from: best name, class name, best directory.
    pub identity: ExportAssetPathIdentity,
    /// The extension, without the leading dot.
    pub extension: String,
    /// The bytes to write.
    pub bytes: Vec<u8>,
}

/// What the caller's producer returned for one collection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrimaryContentPayload {
    /// Bytes to write.
    ///
    /// Boxed because the other two variants carry nothing, and the identity alone is several
    /// `String`s; an unboxed variant would make every value of this enum pay for the largest one.
    Produced(Box<PrimaryContentProduced>),
    /// The reference extractor exists and the port has no producer for it yet.
    Unimplemented,
    /// The producer ran and reported failure.
    Failed,
}

/// One collection that did not produce a file, with the reason.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrimaryContentFailure {
    /// `ExportCollectionBase.Name`.
    pub name: String,
    /// Why nothing was written.
    pub reason: String,
}

/// The result of one primary-content export run.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PrimaryContentRun {
    /// The progress and failure lines the loop emitted, in order.
    pub events: Vec<PrimaryContentExportEvent>,
    /// The destinations actually written, in visit order.
    pub written: Vec<PlannedSingleAssetPaths>,
    /// Collections whose extractor has no producer in the port yet, in visit order.
    pub unimplemented: Vec<String>,
    /// Collections whose producer, path planning or write failed.
    pub failures: Vec<PrimaryContentFailure>,
}

/// `PrimaryContentExporter.Export`, with the payload and the filesystem supplied by the caller.
///
/// `file_system` is only a probe here - it answers `Directory.Exists` and `File.Exists`, which is
/// what the unique-name search needs - while `write` performs the actual write. That split matches
/// the rest of the port: the path planner is testable without a disk, and the caller owns the I/O.
///
/// **The probe must be faithful, and it must reflect this run's own writes.** `get_unique_name`
/// returns the plain name *without probing `File.Exists` at all* when `Directory.Exists` is false,
/// which is the reference's own ordering: `SingleExportCollection.Export` resolves the unique name
/// and only then creates the directory, so the first asset in a fresh directory takes the plain name
/// and later assets see the directory that the first one created. A probe that always reports
/// "nothing exists" therefore disables conflict resolution and lets two assets silently collide -
/// which is what the first version of this module's own test asserted as if it were correct.
pub fn export_primary_content<F, W>(
    collections: &[PrimaryContentCollection],
    steps: &[PrimaryContentExportStep<'_>],
    project_directory: &str,
    platform: HostPathPlatform,
    file_system: &impl ExportFileSystem,
    mut produce: F,
    mut write: W,
) -> PrimaryContentRun
where
    F: FnMut(&PrimaryContentCollection) -> PrimaryContentPayload,
    W: FnMut(&str, &[u8]) -> Result<(), String>,
{
    let mut state = UniqueNameState::default();
    let mut written: Vec<PlannedSingleAssetPaths> = Vec::new();
    let mut unimplemented: Vec<String> = Vec::new();
    let mut failures: Vec<PrimaryContentFailure> = Vec::new();

    let events = run_primary_content_export_loop(collections, steps, |collection| {
        let name = collection.collection_name();
        let produced = match produce(collection) {
            PrimaryContentPayload::Produced(produced) => *produced,
            PrimaryContentPayload::Unimplemented => {
                unimplemented.push(name);
                return false;
            }
            PrimaryContentPayload::Failed => {
                failures.push(PrimaryContentFailure {
                    name,
                    reason: "the extractor reported failure".to_owned(),
                });
                return false;
            }
        };
        let planned = plan_single_export_paths(
            &produced.identity,
            &produced.extension,
            platform,
            &mut state,
            file_system,
            project_directory,
        );
        let planned = match planned {
            Ok(planned) => planned,
            Err(error) => {
                failures.push(PrimaryContentFailure {
                    name,
                    // `AssetPathError` already renders itself, and it names the limit that was hit.
                    reason: error.to_string(),
                });
                return false;
            }
        };
        match write(&planned.file_path, &produced.bytes) {
            Ok(()) => {
                written.push(planned);
                true
            }
            Err(reason) => {
                failures.push(PrimaryContentFailure { name, reason });
                false
            }
        }
    });

    PrimaryContentRun {
        events,
        written,
        unimplemented,
        failures,
    }
}

/// `JsonContentExtractor.Export`: the JSON document for one asset.
///
/// **The line break is the host's, not `\n`.** The reference writes through
/// `using StreamWriter writer = new(stream)`, which takes `TextWriter.NewLine` from
/// `Environment.NewLine` because nothing sets it - so the file export's documents use `\r\n` on
/// Windows. The web surface is the opposite case: `AssetRipper.GUI.Web/Pages/Assets/AssetAPI.cs`
/// sets `StringWriter { NewLine = "\n" }` explicitly. The two callers therefore produce different
/// bytes for the same asset, and this function is the file-export one. `JsonWalker::new()` supplies
/// the host default, which is what `StreamWriter` does.
pub fn json_content_document(value: &SerializedValue) -> String {
    let mut walker = JsonWalker::new();
    write_json_body(&mut walker, value);
    walker.finish()
}

/// The facts the claim dispatch reads for one workspace object.
///
/// Every field is a caller-supplied answer, because each one needs decoded asset data: whether a mesh
/// is set, whether an image texture has intact bytes, which of the binary arms applies. Gathering
/// them is the adapter's job; deciding from them is this module's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryContentObjectFacts<'a> {
    /// `GlbModelExporter`: the kind of `asset.MainAsset`.
    pub model_main_asset: GlbModelMainAsset,
    /// `DeletedAssetsExporter`: `asset.MainAsset is DeletedAssetsInformation`.
    pub main_asset_is_deleted_information: bool,
    /// `GlbMeshExporter`: `MeshExtensions.IsSet()`.
    pub mesh_is_set: bool,
    /// `GlbNavMeshExporter`: `HeightMeshes` is non-null.
    pub height_meshes_present: bool,
    /// `GlbTerrainExporter` and `TerrainHeatmapExporter`: `asset is ITerrainData`.
    pub is_terrain_data: bool,
    /// `TextureExporter`: `ImageTextureExtensions.CheckAssetIntegrity()`.
    pub image_texture_intact: bool,
    /// `AudioContentExtractor`: `asset is IAudioClip`.
    ///
    /// The source's gate is a successful decode, which this dispatch cannot perform; an audio clip
    /// is therefore **claimed** and its producer reports `Unimplemented` until one exists. Claiming
    /// it is the honest choice: declining would hand the asset to the JSON fallback, which the
    /// reference never does for an audio clip.
    pub is_audio_clip: bool,
    /// `ScriptContentExtractor`: `asset is IMonoScript`.
    pub is_mono_script: bool,
    /// `BinaryAssetContentExtractor`: which of its four arms applies, if any.
    pub binary: Option<BinaryAssetFacts<'a>>,
}

/// `IContentExtractor.TryCreateCollection` for one handler in an object's stack.
///
/// This is the dispatch that turns the per-extractor decisions into the three-way claim the assembly
/// rule needs, so a route's closure is a facts lookup rather than a `match` on extractor names
/// scattered through the web layer.
///
/// The empty handlers are handled first and unconditionally: all twenty claim every asset they are
/// reached for, into the shared empty collection, which is why their answer is `Empty` rather than
/// `Claimed`.
///
/// `script_state` is the script extractor's one-shot flag. Its two outcomes map exactly onto the
/// claim: the first MonoScript creates the script collection, and every later one claims into the
/// empty collection - so the flag is not a special case here but the reason the `Empty` variant
/// exists at all.
pub fn claim_for_extractor(
    registration: &PrimaryHandlerRegistration,
    facts: PrimaryContentObjectFacts<'_>,
    script_state: &mut ScriptExtractorState,
) -> PrimaryContentClaim {
    if registration.is_empty_handler {
        return PrimaryContentClaim::Empty;
    }
    let claimed = |admitted: bool| {
        if admitted {
            PrimaryContentClaim::Claimed
        } else {
            PrimaryContentClaim::Declined
        }
    };
    match registration.extractor {
        // The catch-all claims everything it is reached for; being last in stack order is what
        // makes it a fallback rather than a first choice.
        "JsonContentExtractor" => PrimaryContentClaim::Claimed,
        "DeletedAssetsExporter" => claimed(deleted_assets_claim(
            facts.main_asset_is_deleted_information,
        )),
        "GlbModelExporter" => claimed(
            glb_asset_claim(GlbAssetFacts::Model {
                main_asset: facts.model_main_asset,
            })
            .is_some(),
        ),
        "GlbMeshExporter" => claimed(
            glb_asset_claim(GlbAssetFacts::Mesh {
                mesh_is_set: facts.mesh_is_set,
            })
            .is_some(),
        ),
        "GlbNavMeshExporter" => claimed(
            glb_asset_claim(GlbAssetFacts::NavMesh {
                height_meshes_present: facts.height_meshes_present,
            })
            .is_some(),
        ),
        "GlbTerrainExporter" => claimed(glb_asset_claim(GlbAssetFacts::Terrain).is_some()),
        "BinaryAssetContentExtractor" => claimed(facts.binary.is_some_and(binary_asset_claims)),
        "AudioContentExtractor" => claimed(facts.is_audio_clip),
        "TextureExporter" => claimed(facts.image_texture_intact),
        "TerrainHeatmapExporter" => claimed(facts.is_terrain_data),
        "ScriptContentExtractor" => match script_state.claim(facts.is_mono_script) {
            None => PrimaryContentClaim::Declined,
            Some(ScriptClaim::ScriptCollection) => PrimaryContentClaim::Claimed,
            Some(ScriptClaim::EmptyCollection) => PrimaryContentClaim::Empty,
        },
        // An extractor this dispatch does not know cannot be silently declined into the JSON
        // fallback: `Declined` here means "the stack continues", and a caller that added a handler
        // would see the asset land somewhere unexpected. Keeping the arm explicit makes that visible.
        _ => PrimaryContentClaim::Declined,
    }
}

/// The context the facts derivation needs beyond the payload itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryContentObjectContext {
    /// The owning SerializedFile's Unity version, which the mesh reader branches on.
    pub version: UnityVersion,
    /// The file endianness.
    pub endian: EndianType,
}

/// The claim-dispatch facts for one object, or the reason they cannot be determined.
pub enum PrimaryContentObjectFactsOutcome<'a> {
    /// The facts are determined, so [`claim_for_extractor`] can run.
    Known(PrimaryContentObjectFacts<'a>),
    /// The object's extractor depends on a relation this path does not compute.
    ///
    /// This is deliberately not a decline. Declining sends the object to the JSON fallback, and
    /// claiming sends it to a producer that cannot run - both export it as something the reference
    /// never would. An undecidable object is **reported** instead, so the gap is visible in the run's
    /// diagnostics rather than buried in a file.
    Undecidable {
        /// Why the facts could not be determined.
        reason: &'static str,
    },
}

/// Class-1 `GameObject`, class-2 `Component`, class-4 `Transform`.
///
/// These are the objects `GlbModelExporter` is registered for, and it decides on `asset.MainAsset`,
/// which is a scene- or prefab-hierarchy relation.
const MODEL_EXTRACTOR_CLASS_IDS: [i32; 3] = [1, 2, 4];
/// Class-49 `TextAsset`.
const TEXT_ASSET_CLASS_ID: i32 = 49;
/// Class-115 `MonoScript`.
const MONO_SCRIPT_CLASS_ID: i32 = 115;
/// Class-128 `Font`.
const FONT_CLASS_ID: i32 = 128;
/// Class-152 `MovieTexture`.
const MOVIE_TEXTURE_CLASS_ID: i32 = 152;
/// Class-329 `VideoClip`.
const VIDEO_CLIP_CLASS_ID: i32 = 329;

/// The reason an object's facts are undecidable on this path.
const HIERARCHY_REASON: &str =
    "GlbModelExporter decides on the scene or prefab hierarchy relation, which this path does not \
     compute";
/// The reason a non-`Texture2D` image class is undecidable here.
const IMAGE_CLASS_REASON: &str =
    "this image class's integrity rule is not read on this path; only class-28 Texture2D is";

/// Derive the claim-dispatch facts for one object from its class and decoded payload.
///
/// The class-to-family decision is [`crate::export::res_wiring::resource_export_family`]'s, so the
/// classes the primary-content pipeline owns are decided in one place. What this adds is the
/// per-family *fact*: whether a mesh is set, whether navmesh data has a height-mesh list, whether an
/// image texture's bytes are reachable, which of the binary extractor's four arms applies.
///
/// `stream_check` answers `m_StreamData`'s collection check for an image texture - `None` when the
/// texture has no `m_StreamData`, otherwise whether the streamed file resolved. It is a closure
/// because the file lookup is the caller's, and it is passed straight to the already-ported
/// [`image_texture_check_asset_integrity`] rather than restating that rule.
pub fn object_facts_from_payload<'a>(
    class_id: i32,
    root: &'a SerializedValue,
    context: PrimaryContentObjectContext,
    stream_check: impl FnOnce() -> Option<bool>,
) -> PrimaryContentObjectFactsOutcome<'a> {
    use crate::export::res_wiring::{resource_export_family, ResourceExportFamily};
    use crate::resources::mesh::MeshView;
    use crate::resources::video_clip::VideoClipView;

    let mut facts = PrimaryContentObjectFacts {
        model_main_asset: GlbModelMainAsset::Other,
        main_asset_is_deleted_information: false,
        mesh_is_set: false,
        height_meshes_present: false,
        is_terrain_data: false,
        image_texture_intact: false,
        is_audio_clip: false,
        is_mono_script: false,
        binary: None,
    };

    if MODEL_EXTRACTOR_CLASS_IDS.contains(&class_id) {
        return PrimaryContentObjectFactsOutcome::Undecidable {
            reason: HIERARCHY_REASON,
        };
    }

    match resource_export_family(class_id) {
        Some(ResourceExportFamily::Mesh) => {
            // The reference gates the mesh extractor on `MeshExtensions.IsSet()`, so the view has to
            // be read here: a mesh with no geometry must decline and let the stack continue. The
            // composite is `resources::mesh::is_set`, which composes the compressed and vertex-data
            // halves rather than restating them - this is its first production caller.
            facts.mesh_is_set = MeshView::from_value(root, context.version, context.endian)
                .map(|view| {
                    crate::resources::mesh::is_set(
                        view.compressed_mesh
                            .as_ref()
                            .is_some_and(|mesh| mesh.is_set()),
                        view.vertex_data
                            .as_ref()
                            .map_or(0, |data| data.vertex_count),
                        view.vertex_data.as_ref().map_or(0, |data| data.data.len()),
                        view.stream_data.as_ref().map(|info| info.is_set()),
                    )
                })
                .unwrap_or(false);
        }
        Some(ResourceExportFamily::Terrain) => facts.is_terrain_data = true,
        Some(ResourceExportFamily::NavMesh) => {
            // The source tests `HeightMeshes` for null, so a present-but-empty list is admitted.
            facts.height_meshes_present = field(root, "m_HeightMeshes").is_some();
        }
        Some(ResourceExportFamily::Audio) => facts.is_audio_clip = true,
        Some(ResourceExportFamily::Texture) => {
            // Only class-28 `Texture2D` is read here. The reference's
            // `ImageTextureExtensions.CheckAssetIntegrity` reads exactly two fields - the inline
            // `ImageData` length and whether `m_StreamData` is present - so those are read directly
            // rather than through a full `Texture2DView`, which demands more of the payload than the
            // integrity rule does. The other image classes have their own integrity implementations
            // with their own field names, and are reported rather than guessed.
            if class_id != crate::export::res_wiring::TEXTURE2D_CLASS_ID {
                return PrimaryContentObjectFactsOutcome::Undecidable {
                    reason: IMAGE_CLASS_REASON,
                };
            }
            let image_data_len = bytes_field(root, "m_ImageData").map_or(0, <[u8]>::len);
            let has_stream = field(root, "m_StreamData").is_some();
            facts.image_texture_intact =
                image_texture_check_asset_integrity(image_data_len, || {
                    if has_stream {
                        stream_check()
                    } else {
                        None
                    }
                });
        }
        // The shader and material families are not primary-content extractors: a shader reaches the
        // JSON fallback and a material belongs to the Unity-project YAML route.
        Some(ResourceExportFamily::ShaderText | ResourceExportFamily::MaterialText) => {}
        // The hierarchy family is not a per-object export at all; `plan_resource_export` returns
        // `None` for it and `scene_glb` owns it.
        Some(ResourceExportFamily::HierarchyGlb) => {
            return PrimaryContentObjectFactsOutcome::Undecidable {
                reason: HIERARCHY_REASON,
            }
        }
        None => match class_id {
            TEXT_ASSET_CLASS_ID => {
                facts.binary = Some(BinaryAssetFacts::TextAsset {
                    script_data: bytes_field(root, "m_Script").unwrap_or(&[]),
                });
            }
            FONT_CLASS_ID => {
                facts.binary = Some(BinaryAssetFacts::Font {
                    font_data: bytes_field(root, "m_FontData").unwrap_or(&[]),
                });
            }
            MOVIE_TEXTURE_CLASS_ID => {
                // `Has_MovieData` and non-emptiness are separate conditions in the source.
                facts.binary = Some(BinaryAssetFacts::MovieTexture {
                    movie_data: bytes_field(root, "m_MovieData"),
                });
            }
            VIDEO_CLIP_CLASS_ID => {
                facts.binary = Some(BinaryAssetFacts::VideoClip {
                    intact: VideoClipView::from_value(root)
                        .map(|view| view.check_integrity())
                        .unwrap_or(false),
                    original_path: string_field(root, "m_OriginalPath").unwrap_or(""),
                });
            }
            MONO_SCRIPT_CLASS_ID => facts.is_mono_script = true,
            _ => {}
        },
    }

    PrimaryContentObjectFactsOutcome::Known(facts)
}

/// A uniquely named field, following the `Base` chain that carries inherited asset fields.
fn field<'a>(root: &'a SerializedValue, name: &'static str) -> Option<&'a SerializedValue> {
    root.unique_field_in_base_chain(name).ok().flatten()
}

/// A field's raw bytes, for the binary extractor's data arms.
fn bytes_field<'a>(root: &'a SerializedValue, name: &'static str) -> Option<&'a [u8]> {
    match field(root, name) {
        Some(SerializedValue::Bytes(bytes)) | Some(SerializedValue::StringBytes(bytes)) => {
            Some(bytes)
        }
        _ => None,
    }
}

/// A field's UTF-8 text, for the video clip's original path.
fn string_field<'a>(root: &'a SerializedValue, name: &'static str) -> Option<&'a str> {
    let bytes = match field(root, name) {
        Some(SerializedValue::StringBytes(bytes)) | Some(SerializedValue::Bytes(bytes)) => bytes,
        _ => return None,
    };
    std::str::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::{
        claim_for_extractor, export_primary_content, json_content_document,
        object_facts_from_payload, payload_kind, PrimaryContentFailure,
        PrimaryContentObjectContext, PrimaryContentObjectFacts, PrimaryContentPayload,
        PrimaryContentPayloadKind, PrimaryContentProduced,
    };
    use crate::export::asset_path::{ExportAssetPathIdentity, ExportFileSystem, HostPathPlatform};
    use crate::export::primary_content::PrimaryContentClaim;
    use crate::export::primary_content::PrimaryHandlerRegistration;
    use crate::export::primary_content::{PrimaryContentCollection, PrimaryContentExportStep};
    use crate::export::primary_extractors::{GlbModelMainAsset, ScriptExtractorState};
    use crate::serialized_file::object::SerializedValue;

    /// The unique-name search only probes, so the probe answers "nothing exists".
    struct EmptyFileSystem;

    impl ExportFileSystem for EmptyFileSystem {
        fn directory_exists(&self, _directory: &str) -> bool {
            false
        }

        fn file_exists(&self, _directory: &str, _name: &str) -> bool {
            false
        }
    }

    fn collection(extractor: &'static str, name: &str) -> PrimaryContentCollection {
        PrimaryContentCollection {
            extractor,
            primary: 1,
            assets: vec![1],
            name: name.to_owned(),
        }
    }

    fn identity(name: &str, class_name: &str) -> ExportAssetPathIdentity {
        ExportAssetPathIdentity {
            class_name: class_name.to_owned(),
            named_name: Some(name.to_owned()),
            ..ExportAssetPathIdentity::default()
        }
    }

    /// The extractor-to-payload mapping, including the two that cannot reach the loop.
    #[test]
    fn payload_kind_covers_every_extractor_that_can_reach_the_loop() {
        assert_eq!(
            payload_kind("JsonContentExtractor"),
            Some(PrimaryContentPayloadKind::Json)
        );
        for glb in [
            "GlbMeshExporter",
            "GlbNavMeshExporter",
            "GlbTerrainExporter",
            "GlbModelExporter",
        ] {
            assert_eq!(
                payload_kind(glb),
                Some(PrimaryContentPayloadKind::Glb),
                "{glb}"
            );
        }
        assert_eq!(
            payload_kind("BinaryAssetContentExtractor"),
            Some(PrimaryContentPayloadKind::Binary)
        );
        assert_eq!(
            payload_kind("AudioContentExtractor"),
            Some(PrimaryContentPayloadKind::Audio)
        );
        assert_eq!(
            payload_kind("TextureExporter"),
            Some(PrimaryContentPayloadKind::Image)
        );
        assert_eq!(
            payload_kind("TerrainHeatmapExporter"),
            Some(PrimaryContentPayloadKind::TerrainHeatmap)
        );
        assert_eq!(
            payload_kind("ScriptContentExtractor"),
            Some(PrimaryContentPayloadKind::Scripts)
        );
        // Neither of these can reach the loop, so neither needs a producer.
        assert_eq!(payload_kind("EmptyContentExtractor"), None);
        assert_eq!(payload_kind("DeletedAssetsExporter"), None);
    }

    /// A produced payload is written to the planned destination and reported as progress.
    #[test]
    fn a_produced_payload_is_written_and_counted() {
        let collections = [collection("JsonContentExtractor", "Cube")];
        let steps = [PrimaryContentExportStep {
            collection: &collections[0],
            exportable: true,
        }];
        let mut writes: Vec<(String, Vec<u8>)> = Vec::new();
        let run = export_primary_content(
            &collections,
            &steps,
            "out",
            HostPathPlatform::Windows,
            &EmptyFileSystem,
            |_| {
                PrimaryContentPayload::Produced(Box::new(PrimaryContentProduced {
                    identity: identity("Cube", "GameObject"),
                    extension: "json".to_owned(),
                    bytes: b"{}".to_vec(),
                }))
            },
            |path, bytes| {
                writes.push((path.to_owned(), bytes.to_vec()));
                Ok(())
            },
        );
        assert_eq!(run.written.len(), 1);
        assert!(
            run.written[0].file_path.ends_with("Cube.json"),
            "{:?}",
            run.written[0]
        );
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].1, b"{}");
        assert_eq!(run.unimplemented, Vec::<String>::new());
        assert_eq!(run.failures, Vec::<PrimaryContentFailure>::new());
        // The loop still reports the progress line for a collection it exported.
        assert_eq!(run.events.len(), 1);
    }

    /// An extractor without a producer is recorded and fails loudly.
    ///
    /// This is the script case: the port has no C# decompiler, and a silently dropped collection
    /// would look exactly like a successful export.
    #[test]
    fn an_unimplemented_extractor_is_recorded_and_fails_the_collection() {
        let collections = [collection("ScriptContentExtractor", "Assembly-CSharp")];
        let steps = [PrimaryContentExportStep {
            collection: &collections[0],
            exportable: true,
        }];
        let run = export_primary_content(
            &collections,
            &steps,
            "out",
            HostPathPlatform::Windows,
            &EmptyFileSystem,
            |_| PrimaryContentPayload::Unimplemented,
            |_, _| panic!("an unimplemented extractor must not write"),
        );
        assert_eq!(run.written.len(), 0);
        assert_eq!(run.unimplemented, vec!["Assembly-CSharp".to_owned()]);
        // The failure is visible in the event stream too, so the log says the export failed.
        assert_eq!(run.events.len(), 2, "{:?}", run.events);
        assert_eq!(run.failures.len(), 0, "it is unimplemented, not failed");
    }

    /// A write failure is recorded with its reason and is not counted as written.
    #[test]
    fn a_write_failure_is_recorded_with_the_reason() {
        let collections = [collection("JsonContentExtractor", "Cube")];
        let steps = [PrimaryContentExportStep {
            collection: &collections[0],
            exportable: true,
        }];
        let run = export_primary_content(
            &collections,
            &steps,
            "out",
            HostPathPlatform::Windows,
            &EmptyFileSystem,
            |_| {
                PrimaryContentPayload::Produced(Box::new(PrimaryContentProduced {
                    identity: identity("Cube", "GameObject"),
                    extension: "json".to_owned(),
                    bytes: b"{}".to_vec(),
                }))
            },
            |_, _| Err("disk is full".to_owned()),
        );
        assert!(run.written.is_empty());
        assert_eq!(
            run.failures,
            vec![PrimaryContentFailure {
                name: "Cube".to_owned(),
                reason: "disk is full".to_owned(),
            }]
        );
    }

    /// A non-exportable collection is skipped without a producer call, and without an event.
    #[test]
    fn a_non_exportable_collection_is_skipped_entirely() {
        let collections = [collection(
            "DeletedAssetsExporter",
            "DeletedAssetsExportCollection",
        )];
        let steps = [PrimaryContentExportStep {
            collection: &collections[0],
            exportable: false,
        }];
        let run = export_primary_content(
            &collections,
            &steps,
            "out",
            HostPathPlatform::Windows,
            &EmptyFileSystem,
            |_| panic!("a non-exportable collection must not be produced"),
            |_, _| panic!("a non-exportable collection must not be written"),
        );
        assert_eq!(
            run.events.len(),
            0,
            "no progress line for a skipped collection"
        );
        assert!(run.written.is_empty());
        assert!(run.failures.is_empty());
    }

    /// Two assets with the same name in one run get distinct files.
    ///
    /// This pins two **contracts on the caller's probe**, both of which the first version of this
    /// test got wrong by using a probe that always answered "nothing exists".
    ///
    /// First, `get_unique_name` returns the plain name **without probing `file_exists` at all** when
    /// the directory does not exist. That is the reference's own ordering - `SingleExportCollection`
    /// resolves the unique name and only then creates the directory - so the first asset in a fresh
    /// directory takes the plain name, and every later asset sees the directory that the first one
    /// created. A probe that never reports the directory therefore disables conflict resolution
    /// entirely, and two assets silently collide.
    ///
    /// Second, the probe must reflect the writes this run has already made, or the search finds no
    /// conflict and hands back the same name again.
    #[test]
    fn the_unique_name_state_is_shared_across_the_run() {
        use std::cell::RefCell;
        use std::rc::Rc;

        /// Models the real ordering: the directory appears with the first write.
        struct RecordingFileSystem(Rc<RefCell<Vec<String>>>);

        impl ExportFileSystem for RecordingFileSystem {
            fn directory_exists(&self, _directory: &str) -> bool {
                !self.0.borrow().is_empty()
            }

            fn file_exists(&self, _directory: &str, name: &str) -> bool {
                self.0.borrow().iter().any(|written| written == name)
            }
        }

        let names: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let file_system = RecordingFileSystem(Rc::clone(&names));
        let collections = [
            collection("JsonContentExtractor", "Cube"),
            collection("JsonContentExtractor", "Cube"),
        ];
        let steps: Vec<PrimaryContentExportStep<'_>> = collections
            .iter()
            .map(|collection| PrimaryContentExportStep {
                collection,
                exportable: true,
            })
            .collect();
        let run = export_primary_content(
            &collections,
            &steps,
            "out",
            HostPathPlatform::Windows,
            &file_system,
            |_| {
                PrimaryContentPayload::Produced(Box::new(PrimaryContentProduced {
                    identity: identity("Cube", "GameObject"),
                    extension: "json".to_owned(),
                    bytes: b"{}".to_vec(),
                }))
            },
            |path, _| {
                let name = path
                    .rsplit(['\\', '/'])
                    .next()
                    .expect("a file name")
                    .to_owned();
                names.borrow_mut().push(name);
                Ok(())
            },
        );
        assert_eq!(run.written.len(), 2);
        assert_ne!(
            run.written[0].file_path, run.written[1].file_path,
            "the second Cube must not overwrite the first"
        );
        assert_eq!(
            names.borrow().len(),
            2,
            "both writes happened, under different names"
        );
    }

    /// The JSON extractor's document uses the **host** line break, unlike the web surface.
    ///
    /// `JsonContentExtractor.Export` writes through a `StreamWriter` that nobody reconfigures, so
    /// `TextWriter.NewLine` is `Environment.NewLine`; the web asset tab sets `NewLine = "\n"`
    /// explicitly. Asserted on both platforms because CI runs on both, and because a hard-coded `\n`
    /// would pass a single-platform check while producing different bytes from the reference on
    /// Windows.
    #[test]
    fn the_json_document_uses_the_host_line_break() {
        use crate::serialized_file::object::SerializedFieldValue;
        use crate::serialized_file::object::SerializedValue;

        let value = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_A".to_owned(),
                value: SerializedValue::I32(1),
            },
            SerializedFieldValue {
                name: "m_B".to_owned(),
                value: SerializedValue::Bool(true),
            },
        ]);
        let document = json_content_document(&value);

        if cfg!(windows) {
            assert!(
                document.contains("\r\n"),
                "the file export's writer uses Environment.NewLine: {document:?}"
            );
            assert!(
                !document.replace("\r\n", "").contains('\n'),
                "no bare newline should remain on Windows: {document:?}"
            );
        } else {
            assert!(
                !document.contains('\r'),
                "the host line break is a bare newline off Windows: {document:?}"
            );
        }
        // The document's shape is the walker's, which the shape fixture already pins; this only
        // needs to show that both fields reached it.
        assert!(document.contains("\"m_A\""), "{document:?}");
        assert!(document.contains("\"m_B\""), "{document:?}");
    }

    /// A facts value with every fact absent, so each test states only what it is about.
    fn no_facts() -> PrimaryContentObjectFacts<'static> {
        PrimaryContentObjectFacts {
            model_main_asset: GlbModelMainAsset::Other,
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

    fn registration(extractor: &'static str, is_empty_handler: bool) -> PrimaryHandlerRegistration {
        // The table's entries are `&'static str`, and so are these literals.
        let target: &'static str = match extractor {
            "JsonContentExtractor" => "IUnityObjectBase",
            "DeletedAssetsExporter" => "IUnityObjectBase",
            "GlbModelExporter" => "IGameObject",
            "GlbMeshExporter" => "IMesh",
            "GlbNavMeshExporter" => "INavMeshData",
            "GlbTerrainExporter" => "ITerrainData",
            "TerrainHeatmapExporter" => "ITerrainData",
            "BinaryAssetContentExtractor" => "ITextAsset",
            "AudioContentExtractor" => "IAudioClip",
            "TextureExporter" => "IImageTexture",
            "ScriptContentExtractor" => "IMonoScript",
            _ => "IAnimation",
        };
        PrimaryHandlerRegistration {
            target,
            extractor,
            is_empty_handler,
        }
    }

    /// Every empty handler claims, into the shared empty collection.
    #[test]
    fn the_empty_handlers_claim_as_empty() {
        let mut script = ScriptExtractorState::new();
        assert_eq!(
            claim_for_extractor(
                &registration("EmptyContentExtractor", true),
                no_facts(),
                &mut script
            ),
            PrimaryContentClaim::Empty
        );
    }

    /// The single-fact extractors, each driven by its own fact and nothing else.
    #[test]
    fn the_single_fact_extractors_follow_their_own_fact() {
        let mut script = ScriptExtractorState::new();
        let mut claim = |extractor: &'static str, facts: PrimaryContentObjectFacts<'static>| {
            claim_for_extractor(&registration(extractor, false), facts, &mut script)
        };

        // The catch-all always claims.
        assert_eq!(
            claim("JsonContentExtractor", no_facts()),
            PrimaryContentClaim::Claimed
        );
        // Deleted assets need their main asset, and decline otherwise.
        assert_eq!(
            claim("DeletedAssetsExporter", no_facts()),
            PrimaryContentClaim::Declined
        );
        assert_eq!(
            claim(
                "DeletedAssetsExporter",
                PrimaryContentObjectFacts {
                    main_asset_is_deleted_information: true,
                    ..no_facts()
                }
            ),
            PrimaryContentClaim::Claimed
        );
        // A mesh is admitted only when it is set.
        assert_eq!(
            claim("GlbMeshExporter", no_facts()),
            PrimaryContentClaim::Declined
        );
        assert_eq!(
            claim(
                "GlbMeshExporter",
                PrimaryContentObjectFacts {
                    mesh_is_set: true,
                    ..no_facts()
                }
            ),
            PrimaryContentClaim::Claimed
        );
        // A navmesh needs a non-null height-mesh list.
        assert_eq!(
            claim("GlbNavMeshExporter", no_facts()),
            PrimaryContentClaim::Declined
        );
        assert_eq!(
            claim(
                "GlbNavMeshExporter",
                PrimaryContentObjectFacts {
                    height_meshes_present: true,
                    ..no_facts()
                }
            ),
            PrimaryContentClaim::Claimed
        );
        // The GLB terrain extractor has no condition, so its stack membership is its only gate.
        assert_eq!(
            claim("GlbTerrainExporter", no_facts()),
            PrimaryContentClaim::Claimed
        );
        // An image texture needs intact bytes.
        assert_eq!(
            claim("TextureExporter", no_facts()),
            PrimaryContentClaim::Declined
        );
        assert_eq!(
            claim(
                "TextureExporter",
                PrimaryContentObjectFacts {
                    image_texture_intact: true,
                    ..no_facts()
                }
            ),
            PrimaryContentClaim::Claimed
        );
        // The heatmap claims any terrain data, like the GLB terrain extractor.
        assert_eq!(
            claim("TerrainHeatmapExporter", no_facts()),
            PrimaryContentClaim::Declined
        );
        assert_eq!(
            claim(
                "TerrainHeatmapExporter",
                PrimaryContentObjectFacts {
                    is_terrain_data: true,
                    ..no_facts()
                }
            ),
            PrimaryContentClaim::Claimed
        );
        // An audio clip is claimed so its producer can report that it is unimplemented; declining
        // would hand it to the JSON fallback, which the reference never does.
        assert_eq!(
            claim("AudioContentExtractor", no_facts()),
            PrimaryContentClaim::Declined
        );
        assert_eq!(
            claim(
                "AudioContentExtractor",
                PrimaryContentObjectFacts {
                    is_audio_clip: true,
                    ..no_facts()
                }
            ),
            PrimaryContentClaim::Claimed
        );
    }

    /// The GLB model extractor admits only a hierarchy main asset.
    #[test]
    fn the_model_extractor_admits_only_a_hierarchy() {
        let mut script = ScriptExtractorState::new();
        for (main_asset, expected) in [
            (
                GlbModelMainAsset::SceneHierarchy,
                PrimaryContentClaim::Claimed,
            ),
            (
                GlbModelMainAsset::PrefabHierarchy,
                PrimaryContentClaim::Claimed,
            ),
            (GlbModelMainAsset::Other, PrimaryContentClaim::Declined),
        ] {
            let facts = PrimaryContentObjectFacts {
                model_main_asset: main_asset,
                ..no_facts()
            };
            assert_eq!(
                claim_for_extractor(&registration("GlbModelExporter", false), facts, &mut script),
                expected,
                "{main_asset:?}"
            );
        }
    }

    /// The script flag's two outcomes become the claim's `Claimed` and `Empty`.
    #[test]
    fn the_script_flag_becomes_claimed_then_empty() {
        let mut script = ScriptExtractorState::new();
        let script_facts = PrimaryContentObjectFacts {
            is_mono_script: true,
            ..no_facts()
        };
        let registration = registration("ScriptContentExtractor", false);

        // A non-MonoScript declines and leaves the flag alone.
        assert_eq!(
            claim_for_extractor(&registration, no_facts(), &mut script),
            PrimaryContentClaim::Declined
        );
        // The first MonoScript creates the script collection.
        assert_eq!(
            claim_for_extractor(&registration, script_facts, &mut script),
            PrimaryContentClaim::Claimed
        );
        // Every later one claims into the shared empty collection.
        assert_eq!(
            claim_for_extractor(&registration, script_facts, &mut script),
            PrimaryContentClaim::Empty
        );
        assert_eq!(
            claim_for_extractor(&registration, script_facts, &mut script),
            PrimaryContentClaim::Empty
        );
    }

    /// The binary arms go through the ported gate rather than a restated one.
    #[test]
    fn the_binary_extractor_reuses_the_ported_gate() {
        use crate::export::primary_extractors::BinaryAssetFacts;
        let mut script = ScriptExtractorState::new();
        let registration = registration("BinaryAssetContentExtractor", false);

        let with_data = PrimaryContentObjectFacts {
            binary: Some(BinaryAssetFacts::TextAsset { script_data: b"x" }),
            ..no_facts()
        };
        assert_eq!(
            claim_for_extractor(&registration, with_data, &mut script),
            PrimaryContentClaim::Claimed
        );

        let empty = PrimaryContentObjectFacts {
            binary: Some(BinaryAssetFacts::TextAsset { script_data: b"" }),
            ..no_facts()
        };
        assert_eq!(
            claim_for_extractor(&registration, empty, &mut script),
            PrimaryContentClaim::Declined,
            "an empty payload declines so the stack continues"
        );

        // No arm at all is a decline, not a claim.
        assert_eq!(
            claim_for_extractor(&registration, no_facts(), &mut script),
            PrimaryContentClaim::Declined
        );
    }

    /// The stack and the dispatch together select the extractor the reference would select.
    ///
    /// This is the payoff: `handler_stack_for_object` supplies the order, `claim_for_extractor`
    /// supplies the answers, and the assembly rule runs between them.
    #[test]
    fn the_stack_and_dispatch_select_the_reference_extractor() {
        use crate::export::primary_content::{
            create_primary_content_collections_for_objects, PrimaryContentObject,
        };
        use crate::export::project_exporter::ObjectClassFacts;

        let objects = [PrimaryContentObject {
            identity: 1,
            facts: ObjectClassFacts::new(43, false),
            collection_name: "Tri",
            collection_assets: &[1],
        }];

        // A mesh that is set goes to the mesh exporter even though JSON is in the stack.
        let mut script = ScriptExtractorState::new();
        let collections = create_primary_content_collections_for_objects(&objects, |reg, _| {
            claim_for_extractor(
                reg,
                PrimaryContentObjectFacts {
                    mesh_is_set: true,
                    ..no_facts()
                },
                &mut script,
            )
        })
        .unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].extractor, "GlbMeshExporter");

        // The same mesh with no geometry falls through to the JSON catch-all.
        let mut script = ScriptExtractorState::new();
        let collections = create_primary_content_collections_for_objects(&objects, |reg, _| {
            claim_for_extractor(reg, no_facts(), &mut script)
        })
        .unwrap();
        assert_eq!(collections[0].extractor, "JsonContentExtractor");
    }

    /// A struct payload with named fields.
    fn payload(fields: Vec<(&str, SerializedValue)>) -> SerializedValue {
        use crate::serialized_file::object::SerializedFieldValue;
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

    fn context() -> PrimaryContentObjectContext {
        use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
        PrimaryContentObjectContext {
            version: UnityVersion {
                major: 2019,
                minor: 4,
                patch: 40,
            },
            endian: EndianType::Little,
        }
    }

    /// The families whose facts come from the payload, one class each.
    #[test]
    fn the_payload_derives_each_family_fact() {
        use crate::export::primary_export::PrimaryContentObjectFactsOutcome;
        use crate::serialized_file::object::SerializedValue;

        // A macro rather than a closure: the facts borrow the payload, and a closure cannot be
        // generic over the lifetime it returns.
        macro_rules! known {
            ($class_id:expr, $root:expr) => {
                match object_facts_from_payload($class_id, $root, context(), || None) {
                    PrimaryContentObjectFactsOutcome::Known(facts) => facts,
                    PrimaryContentObjectFactsOutcome::Undecidable { reason } => {
                        panic!("class {} should be decidable: {reason}", $class_id)
                    }
                }
            };
        }

        // Class 156 TerrainData has no condition at all.
        assert!(known!(156, &payload(vec![])).is_terrain_data);
        // Class 83 AudioClip is claimed so its producer can report unimplemented.
        assert!(known!(83, &payload(vec![])).is_audio_clip);
        // Class 115 MonoScript drives the one-shot script flag.
        assert!(known!(115, &payload(vec![])).is_mono_script);
        // Class 238 NavMeshData needs its height-mesh list present.
        assert!(!known!(238, &payload(vec![])).height_meshes_present);
        assert!(
            known!(
                238,
                &payload(vec![("m_HeightMeshes", SerializedValue::Array(vec![]))])
            )
            .height_meshes_present,
            "a present-but-empty list is admitted"
        );
        // A shader and a material are not primary-content extractors, so nothing is set.
        let empty = payload(vec![]);
        let shader = known!(48, &empty);
        assert!(!shader.is_terrain_data && !shader.is_audio_clip && shader.binary.is_none());
        let material = known!(21, &empty);
        assert!(material.binary.is_none() && !material.is_terrain_data);
    }

    /// The binary extractor's four arms, each from its own field.
    #[test]
    fn the_payload_derives_the_binary_arms() {
        use crate::export::primary_export::PrimaryContentObjectFactsOutcome;
        use crate::export::primary_extractors::BinaryAssetFacts;
        use crate::serialized_file::object::SerializedValue;

        macro_rules! binary {
            ($class_id:expr, $root:expr) => {
                match object_facts_from_payload($class_id, $root, context(), || None) {
                    PrimaryContentObjectFactsOutcome::Known(facts) => facts.binary,
                    PrimaryContentObjectFactsOutcome::Undecidable { reason } => {
                        panic!("class {} should be decidable: {reason}", $class_id)
                    }
                }
            };
        }

        assert_eq!(
            binary!(
                49,
                &payload(vec![("m_Script", SerializedValue::Bytes(vec![1, 2]))])
            ),
            Some(BinaryAssetFacts::TextAsset {
                script_data: &[1, 2]
            })
        );
        // A TextAsset with no script field is still a TextAsset arm, with empty data, so the gate
        // declines it rather than the class falling through.
        assert_eq!(
            binary!(49, &payload(vec![])),
            Some(BinaryAssetFacts::TextAsset { script_data: &[] })
        );
        assert_eq!(
            binary!(
                128,
                &payload(vec![("m_FontData", SerializedValue::Bytes(vec![b'O']))])
            ),
            Some(BinaryAssetFacts::Font { font_data: b"O" })
        );
        // `Has_MovieData` and non-emptiness stay separate: absent is `None`, empty is `Some(&[])`.
        assert_eq!(
            binary!(152, &payload(vec![])),
            Some(BinaryAssetFacts::MovieTexture { movie_data: None })
        );
        assert_eq!(
            binary!(
                152,
                &payload(vec![("m_MovieData", SerializedValue::Bytes(vec![]))])
            ),
            Some(BinaryAssetFacts::MovieTexture {
                movie_data: Some(&[])
            })
        );
        // A video clip carries its original path, which the producer turns into the extension.
        assert_eq!(
            binary!(
                329,
                &payload(vec![(
                    "m_OriginalPath",
                    SerializedValue::StringBytes(b"clip.mp4".to_vec())
                )])
            ),
            Some(BinaryAssetFacts::VideoClip {
                intact: false,
                original_path: "clip.mp4"
            })
        );
    }

    /// The model and non-`Texture2D` image classes are reported rather than guessed.
    #[test]
    fn the_undecidable_classes_are_reported_with_a_reason() {
        use crate::export::primary_export::PrimaryContentObjectFactsOutcome;

        for class_id in [1, 2, 4] {
            match object_facts_from_payload(class_id, &payload(vec![]), context(), || None) {
                PrimaryContentObjectFactsOutcome::Undecidable { reason } => {
                    assert!(reason.contains("hierarchy"), "{reason}");
                }
                PrimaryContentObjectFactsOutcome::Known(_) => {
                    panic!("class {class_id} needs the hierarchy relation")
                }
            }
        }
    }

    /// An image texture's integrity is the ported rule, fed by the caller's stream check.
    #[test]
    fn the_image_texture_integrity_uses_the_ported_rule() {
        use crate::export::primary_export::PrimaryContentObjectFactsOutcome;
        use crate::serialized_file::object::SerializedValue;

        let intact = |root: &SerializedValue, stream: Option<bool>| match object_facts_from_payload(
            28,
            root,
            context(),
            || stream,
        ) {
            PrimaryContentObjectFactsOutcome::Known(facts) => facts.image_texture_intact,
            PrimaryContentObjectFactsOutcome::Undecidable { reason } => {
                panic!("class 28 should be decidable: {reason}")
            }
        };

        // Inline data wins and the stream check is never consulted.
        let inline = payload(vec![("m_ImageData", SerializedValue::Bytes(vec![1, 2, 3]))]);
        assert!(intact(&inline, None));
        // No data and no stream declines.
        let empty = payload(vec![]);
        assert!(!intact(&empty, None));
        assert!(!intact(&empty, Some(false)));
    }
}
