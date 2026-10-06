//! Dispatch mechanics of the exporter base classes in the frozen baseline.
//!
//! The snapshot determines the full call routing without any generated code:
//!
//! - `IAssetExporter` ships default method bodies: the callback-less single and multi
//!   overloads forward to their callback-taking twins with a null callback and report
//!   success, while the callback-taking twins throw `NotSupportedException`.
//! - `BinaryAssetExporter` overrides the single-asset 4-argument overload to throw, both
//!   multi overloads to throw, and the single callback-taking overload to run the export
//!   first and invoke the callback second (so a throwing export skips the callback).
//!   `TryCreateCollection` always succeeds with a plain `AssetExportCollection`, both
//!   `ToExportType` answers are `Meta`, and `IsValidData` accepts only non-empty buffers.
//! - `YamlExporterBase` writes one YAML document with no stream head for single assets and
//!   a head/documents/tail stream for asset sequences; the multi callback-taking overload
//!   is left on the interface default and throws "Yaml supports only single file export".
//!   Collection creation is abstract, and both type answers are `Serialized`.
//! - `DummyAssetExporter.Get` selects one of four process-wide singletons from the
//!   empty/skip and meta/serialized flags; empty exporters hand out the shared
//!   `EmptyExportCollection.Instance` while skip exporters build a fresh
//!   `SkipExportCollection`, both always succeeding.
//! - `FailExportCollection` warns and reports failure on export, matches only its own
//!   asset, resolves the main export ID for that asset and throws otherwise, always
//!   throws on GUID requests, and mints missing-reference pointers for non-local
//!   requests only. The main ID, the exporter type, and the missing-reference GUID
//!   arrive as caller facts.
//!
//! Callback invocation order is modeled as data because the callbacks themselves are
//! caller behavior; byte writing stays with the concrete exporters.

/// One of the four `IAssetExporter.Export` overload shapes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExporterCall {
    /// A single asset without a callback.
    Single,
    /// A single asset with a completion callback.
    SingleWithCallback,
    /// An asset sequence without a callback.
    Multi,
    /// An asset sequence with a completion callback.
    MultiWithCallback,
}

/// What a base-class overload does, without performing any I/O.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BaseCallOutcome {
    /// Throws a plain `NotSupportedException`.
    ThrowsNotSupported,
    /// Throws `NotSupportedException` with the recorded message.
    ThrowsMessage(&'static str),
    /// Runs the export, then invokes the callback; a throwing export skips the callback.
    RunsExportThenCallback,
    /// Writes the YAML documents in the shape's framing and reports success.
    WritesYamlDocuments,
    /// Forwards to the callback-taking twin with a null callback and reports success.
    ForwardsWithNullCallback,
}

/// The `IAssetExporter` interface defaults, shared by every exporter that does not override
/// the overload.
pub fn interface_default_call(call: ExporterCall) -> BaseCallOutcome {
    match call {
        ExporterCall::Single | ExporterCall::Multi => BaseCallOutcome::ForwardsWithNullCallback,
        ExporterCall::SingleWithCallback | ExporterCall::MultiWithCallback => {
            BaseCallOutcome::ThrowsNotSupported
        }
    }
}

/// The `BinaryAssetExporter` overrides: every shape except the single callback-taking
/// overload throws; that overload runs the export before the callback.
pub fn binary_base_call(call: ExporterCall) -> BaseCallOutcome {
    match call {
        ExporterCall::SingleWithCallback => BaseCallOutcome::RunsExportThenCallback,
        ExporterCall::Single | ExporterCall::Multi | ExporterCall::MultiWithCallback => {
            BaseCallOutcome::ThrowsNotSupported
        }
    }
}

/// `BinaryAssetExporter.TryCreateCollection` always succeeds with a plain collection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum BinaryCollectionShape {
    /// A plain `AssetExportCollection` wrapping the asset.
    AssetExportCollection,
}

/// The always-successful collection creation of the binary base.
pub const fn binary_try_create_collection() -> (BinaryCollectionShape, bool) {
    (BinaryCollectionShape::AssetExportCollection, true)
}

/// `BinaryAssetExporter.IsValidData`: non-null and non-empty.
pub fn is_valid_data(data: Option<&[u8]>) -> bool {
    data.is_some_and(|bytes| !bytes.is_empty())
}

/// How `YamlExporterBase` frames its YAML output.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum YamlExportLayout {
    /// Single assets: one document, no stream head.
    DocumentOnly,
    /// Asset sequences: head, one document per asset, tail.
    HeadDocumentsTail,
}

/// The `YamlExporterBase` overrides: single shapes write one headless document, the
/// callback-less multi shape writes a headed stream, and the multi callback-taking
/// overload stays on the interface default and throws its message.
pub fn yaml_base_call(call: ExporterCall) -> BaseCallOutcome {
    match call {
        ExporterCall::MultiWithCallback => {
            BaseCallOutcome::ThrowsMessage("Yaml supports only single file export")
        }
        _ => BaseCallOutcome::WritesYamlDocuments,
    }
}

/// The YAML framing selected for the shapes `YamlExporterBase` implements.
pub fn yaml_base_layout(call: ExporterCall) -> Option<YamlExportLayout> {
    match call {
        ExporterCall::Single | ExporterCall::SingleWithCallback => {
            Some(YamlExportLayout::DocumentOnly)
        }
        ExporterCall::Multi => Some(YamlExportLayout::HeadDocumentsTail),
        ExporterCall::MultiWithCallback => None,
    }
}

/// Which of the four `DummyAssetExporter` singletons `Get` selects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DummyExporterKind {
    /// An empty collection with `Serialized` export type.
    EmptySerialized,
    /// An empty collection with `Meta` export type.
    EmptyMeta,
    /// A skipping collection with `Serialized` export type.
    SkipSerialized,
    /// A skipping collection with `Meta` export type.
    SkipMeta,
}

/// `DummyAssetExporter.Get(isEmptyCollection, isMetaType)` as a pure four-way selection.
/// Singleton identity is an allocation detail; the observable behavior is this matrix.
pub const fn dummy_exporter_kind(
    is_empty_collection: bool,
    is_meta_type: bool,
) -> DummyExporterKind {
    if is_empty_collection {
        if is_meta_type {
            DummyExporterKind::EmptyMeta
        } else {
            DummyExporterKind::EmptySerialized
        }
    } else if is_meta_type {
        DummyExporterKind::SkipMeta
    } else {
        DummyExporterKind::SkipSerialized
    }
}

/// What `DummyAssetExporter.TryCreateCollection` hands out; always succeeds.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DummyCollectionShape {
    /// The shared `EmptyExportCollection.Instance`.
    SharedEmptySingleton,
    /// A fresh `SkipExportCollection` for the asset.
    NewSkipCollection,
}

/// Empty exporters share the singleton; skip exporters build a fresh collection.
pub const fn dummy_try_create_collection(
    is_empty_collection: bool,
) -> (DummyCollectionShape, bool) {
    if is_empty_collection {
        (DummyCollectionShape::SharedEmptySingleton, true)
    } else {
        (DummyCollectionShape::NewSkipCollection, true)
    }
}

/// `FailExportCollection.Export` always warns and reports failure.
pub const fn fail_collection_export_result() -> bool {
    false
}

/// The warning text: `$"Unable to export asset {Name}"` with the collection name.
pub fn fail_collection_warning(collection_name: &str) -> String {
    format!("Unable to export asset {collection_name}")
}

/// `FailExportCollection.Contains`: identity with the failed asset.
pub const fn fail_collection_contains(is_same_asset: bool) -> bool {
    is_same_asset
}

/// `FailExportCollection.GetExportID`: the main export ID for the failed asset
/// (resolved caller-side via `ExportIdHandler`), `ArgumentException` otherwise.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FailExportId {
    /// The failed asset: take the main export ID.
    MainExportId,
    /// Anything else: throw `ArgumentException`.
    ThrowsArgument,
}

/// Selects the `GetExportID` behavior from asset identity.
pub const fn fail_collection_export_id(is_same_asset: bool) -> FailExportId {
    if is_same_asset {
        FailExportId::MainExportId
    } else {
        FailExportId::ThrowsArgument
    }
}

/// `FailExportCollection.CreateExportPointer`: non-local requests for the failed
/// asset mint a missing-reference pointer (main ID plus the missing-reference
/// GUID plus the exporter type, all caller facts); local requests throw, and
/// foreign assets fail through the ID rule first.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FailExportPointer {
    /// Mint the missing-reference pointer.
    MissingReference,
    /// Throw `ArgumentException`.
    ThrowsArgument,
}

/// Selects the pointer behavior from locality and asset identity.
pub const fn fail_collection_pointer(is_local: bool, is_same_asset: bool) -> FailExportPointer {
    if !is_local && is_same_asset {
        FailExportPointer::MissingReference
    } else {
        FailExportPointer::ThrowsArgument
    }
}

/// `FailExportCollection.GetExportGUID` always throws `NotSupportedException`.
pub const fn fail_collection_guid() -> BaseCallOutcome {
    BaseCallOutcome::ThrowsNotSupported
}

#[cfg(test)]
mod tests {
    use super::{
        binary_base_call, binary_try_create_collection, dummy_exporter_kind,
        dummy_try_create_collection, fail_collection_contains, fail_collection_export_id,
        fail_collection_export_result, fail_collection_guid, fail_collection_pointer,
        fail_collection_warning, interface_default_call, is_valid_data, yaml_base_call,
        yaml_base_layout, BaseCallOutcome, BinaryCollectionShape, DummyCollectionShape,
        DummyExporterKind, ExporterCall, FailExportId, FailExportPointer, YamlExportLayout,
    };

    #[test]
    fn interface_defaults_forward_without_callbacks_and_throw_with_them() {
        assert_eq!(
            interface_default_call(ExporterCall::Single),
            BaseCallOutcome::ForwardsWithNullCallback
        );
        assert_eq!(
            interface_default_call(ExporterCall::Multi),
            BaseCallOutcome::ForwardsWithNullCallback
        );
        assert_eq!(
            interface_default_call(ExporterCall::SingleWithCallback),
            BaseCallOutcome::ThrowsNotSupported
        );
        assert_eq!(
            interface_default_call(ExporterCall::MultiWithCallback),
            BaseCallOutcome::ThrowsNotSupported
        );
    }

    #[test]
    fn binary_base_throws_except_for_export_then_callback() {
        assert_eq!(
            binary_base_call(ExporterCall::Single),
            BaseCallOutcome::ThrowsNotSupported
        );
        assert_eq!(
            binary_base_call(ExporterCall::Multi),
            BaseCallOutcome::ThrowsNotSupported
        );
        assert_eq!(
            binary_base_call(ExporterCall::MultiWithCallback),
            BaseCallOutcome::ThrowsNotSupported
        );
        // The one override runs the export first: a throwing export never reaches the callback.
        assert_eq!(
            binary_base_call(ExporterCall::SingleWithCallback),
            BaseCallOutcome::RunsExportThenCallback
        );
        assert_eq!(
            binary_try_create_collection(),
            (BinaryCollectionShape::AssetExportCollection, true)
        );
        assert!(is_valid_data(Some(&[1])));
        assert!(!is_valid_data(Some(&[])));
        assert!(!is_valid_data(None));
    }

    #[test]
    fn yaml_base_frames_documents_and_rejects_multi_callbacks() {
        assert_eq!(
            yaml_base_layout(ExporterCall::Single),
            Some(YamlExportLayout::DocumentOnly)
        );
        assert_eq!(
            yaml_base_layout(ExporterCall::SingleWithCallback),
            Some(YamlExportLayout::DocumentOnly)
        );
        assert_eq!(
            yaml_base_layout(ExporterCall::Multi),
            Some(YamlExportLayout::HeadDocumentsTail)
        );
        assert_eq!(yaml_base_layout(ExporterCall::MultiWithCallback), None);
        assert_eq!(
            yaml_base_call(ExporterCall::MultiWithCallback),
            BaseCallOutcome::ThrowsMessage("Yaml supports only single file export")
        );
    }

    #[test]
    fn dummy_selection_covers_the_four_singletons_and_collection_shapes() {
        assert_eq!(
            dummy_exporter_kind(true, true),
            DummyExporterKind::EmptyMeta
        );
        assert_eq!(
            dummy_exporter_kind(true, false),
            DummyExporterKind::EmptySerialized
        );
        assert_eq!(
            dummy_exporter_kind(false, true),
            DummyExporterKind::SkipMeta
        );
        assert_eq!(
            dummy_exporter_kind(false, false),
            DummyExporterKind::SkipSerialized
        );
        assert_eq!(
            dummy_try_create_collection(true),
            (DummyCollectionShape::SharedEmptySingleton, true)
        );
        assert_eq!(
            dummy_try_create_collection(false),
            (DummyCollectionShape::NewSkipCollection, true)
        );
    }

    #[test]
    fn fail_collection_warns_and_routes_by_identity_and_locality() {
        // Export always warns and reports failure.
        assert!(!fail_collection_export_result());
        assert_eq!(
            fail_collection_warning("Main Mixer"),
            "Unable to export asset Main Mixer"
        );
        // Only the failed asset is contained or ID-resolved.
        assert!(fail_collection_contains(true));
        assert!(!fail_collection_contains(false));
        assert_eq!(fail_collection_export_id(true), FailExportId::MainExportId);
        assert_eq!(
            fail_collection_export_id(false),
            FailExportId::ThrowsArgument
        );
        // Non-local requests for the failed asset mint missing references;
        // local requests and foreign assets throw.
        assert_eq!(
            fail_collection_pointer(false, true),
            FailExportPointer::MissingReference
        );
        assert_eq!(
            fail_collection_pointer(true, true),
            FailExportPointer::ThrowsArgument
        );
        assert_eq!(
            fail_collection_pointer(false, false),
            FailExportPointer::ThrowsArgument
        );
        assert_eq!(
            fail_collection_pointer(true, false),
            FailExportPointer::ThrowsArgument
        );
        // GUID requests always throw.
        assert_eq!(fail_collection_guid(), BaseCallOutcome::ThrowsNotSupported);
    }
}
