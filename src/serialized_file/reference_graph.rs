//! Per-SerializedFile identity indexing and cross-file PPtr resolution.

mod multi_file;
mod per_file;
mod walkers;

pub use self::multi_file::{
    MultiFileObjectIdentity, MultiFileObjectReference, MultiFilePPtrResolution,
    MultiFileReferenceGraph, MultiFileReferenceGraphBuildError, MultiFileReferenceGraphLimits,
    NamedObjectReferenceGraph, PredefinedEngineResourceKind,
};
pub(crate) use self::per_file::{
    decode_object_payload_with_generated_fallback_from_data, factory_object_outcome_error,
    DecodedReferenceObject, StreamObjectDecodeContext,
};
pub use self::per_file::{
    ObjectReferenceGraph, ReferenceGraphIssue, ReferenceGraphLimits, SerializedManagedObject,
    SerializedManagedReference, SerializedManagedReferenceResolution, SerializedObjectReference,
};
pub use self::walkers::ReferenceGraphBuildError;

/// Map a positive serialized fileID to its external dependency slot. FileID zero is local and
/// handled by callers; non-positive and out-of-table values stay invalid.
fn external_table_index(file_id: i32, external_count: usize) -> Option<usize> {
    let index = file_id.checked_sub(1)?;
    let index = usize::try_from(index).ok()?;
    (index < external_count).then_some(index)
}

#[cfg(test)]
#[path = "reference_graph/tests/support.rs"]
mod test_support;

#[cfg(test)]
#[path = "reference_graph/tests/multi_file.rs"]
mod multi_file_tests;

#[cfg(test)]
#[path = "reference_graph/tests/per_file.rs"]
mod per_file_tests;
