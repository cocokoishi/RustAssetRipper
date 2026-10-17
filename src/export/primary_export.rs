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
use super::primary_content::{
    plan_single_export_paths, run_primary_content_export_loop, PlannedSingleAssetPaths,
    PrimaryContentCollection, PrimaryContentExportEvent, PrimaryContentExportStep,
};

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

#[cfg(test)]
mod tests {
    use super::{
        export_primary_content, payload_kind, PrimaryContentFailure, PrimaryContentPayload,
        PrimaryContentPayloadKind, PrimaryContentProduced,
    };
    use crate::export::asset_path::{ExportAssetPathIdentity, ExportFileSystem, HostPathPlatform};
    use crate::export::primary_content::{PrimaryContentCollection, PrimaryContentExportStep};

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
}
