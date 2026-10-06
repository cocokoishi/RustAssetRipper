//! `ManagedReferenceRegistryReader` framing ported from
//! `AssetRipper.Import/Structure/Assembly/Serializable/ManagedReferenceRegistryReader.cs`.
//!
//! Ports the format dispatch and structural guards, which need no type resolver:
//! the terminated/counted format discriminants, the terminus triple, the
//! unknown/null reference identifiers, the counted-format capacity guard, the
//! terminated-loop control decisions, per-entry framing (identifier
//! classification plus the content-presence gate), and the type-resolution
//! failure diagnostic. Reading entry type triples and object payloads stays open:
//! content decoding needs `ITypeResolver.TryGetSerializableType` (live assemblies
//! or TypeTrees).
//!
//! The registry has two formats. In the terminated format the objects end with a
//! terminus entry and an object's identifier is its index; in the counted format
//! the objects are preceded by a count and each object stores its own identifier.
//! Both feed the same registry layout, so the layout does not depend on the format.

use std::error::Error;
use std::fmt::{self, Display};

/// `ManagedReferenceRegistryReader.TerminatedFormat`: objects end with a terminus entry.
pub const TERMINATED_FORMAT: i32 = 1;
/// `ManagedReferenceRegistryReader.CountedFormat`: objects are preceded by a count.
pub const COUNTED_FORMAT: i32 = 2;

/// The terminus class name ending terminated-format registries.
pub const TERMINUS_CLASS_NAME: &str = "Terminus";
/// The terminus namespace ending terminated-format registries.
pub const TERMINUS_NAMESPACE: &str = "UnityEngine.DMAT";
/// The terminus assembly name ending terminated-format registries.
pub const TERMINUS_ASSEMBLY_NAME: &str = "FAKE_ASM";

/// The identifier of a reference to an object that could not be deserialized.
pub const UNKNOWN_REFERENCE_ID: i64 = -1;
/// The identifier of a null reference.
pub const NULL_REFERENCE_ID: i64 = -2;

/// Which registry layout the version word selects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RegistryFormat {
    /// Objects end with a terminus entry; identifiers are indexes.
    Terminated,
    /// Objects are preceded by a count; each object stores its own identifier.
    Counted,
}

/// What one registry reference identifier denotes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ReferenceIdKind {
    /// `UnknownReferenceId`: the object could not be deserialized; it has no content.
    Unknown,
    /// `NullReferenceId`: a null reference; it has no content.
    Null,
    /// A content-bearing object: empty class/assembly names still mean no content.
    Content,
}

/// Why a managed reference registry cannot be framed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedRegistryError {
    /// The version word is neither the terminated nor the counted format.
    UnsupportedFormat { version: i32 },
    /// The counted object count is negative, which the source answers with an
    /// allocation throw; here it is an explicit refusal.
    NegativeObjectCount { count: i32 },
    /// The counted objects do not fit the remaining bytes (`remaining < count`).
    TruncatedRegistry {
        remaining_bytes: u64,
        object_count: i32,
    },
    /// The terminated objects ended without a terminus entry, which the source
    /// answers with an end-of-stream throw carrying the objects read so far.
    UnterminatedRegistry { objects_read: usize },
}

impl Display for ManagedRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFormat { version } => write!(
                formatter,
                "managed reference registry format {version} is not supported"
            ),
            Self::NegativeObjectCount { count } => write!(
                formatter,
                "managed reference registry has a negative object count {count}"
            ),
            Self::TruncatedRegistry {
                remaining_bytes,
                object_count,
            } => write!(
                formatter,
                "the stream only has {remaining_bytes} bytes remaining, so {object_count} referenced objects cannot be read"
            ),
            Self::UnterminatedRegistry { objects_read } => write!(
                formatter,
                "the managed reference registry ended after {objects_read} objects without a terminus"
            ),
        }
    }
}

impl Error for ManagedRegistryError {}

/// Dispatches the registry version word to its layout, mirroring the
/// `format switch` that throws `NotSupportedException` on anything else.
pub const fn classify_registry_format(
    version: i32,
) -> Result<RegistryFormat, ManagedRegistryError> {
    match version {
        TERMINATED_FORMAT => Ok(RegistryFormat::Terminated),
        COUNTED_FORMAT => Ok(RegistryFormat::Counted),
        _ => Err(ManagedRegistryError::UnsupportedFormat { version }),
    }
}

/// `IsTerminus`: a managed type triple ends the terminated format only when class,
/// namespace, and assembly all match the terminus constants.
pub const fn is_terminus(class_name: &str, namespace: &str, assembly_name: &str) -> bool {
    // Byte-wise comparison, matching the ordinal source string comparison.
    const_str_eq(class_name, TERMINUS_CLASS_NAME)
        && const_str_eq(namespace, TERMINUS_NAMESPACE)
        && const_str_eq(assembly_name, TERMINUS_ASSEMBLY_NAME)
}

/// Byte-wise string equality usable in `const fn`.
const fn const_str_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// One terminated-format loop decision (`ReadTerminatedObjects` control flow
/// without the content reads, which need the type resolver).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TerminatedLoopAction {
    /// The triple is the terminus: the object list is complete.
    Finish,
    /// A content-bearing entry follows, addressed by its index like the source
    /// passes `referencedObjects.Count` as the reference identifier.
    ReadIndexedObject { reference_id: i64 },
}

/// Advances the terminated object loop over caller facts: an exhausted stream
/// is the source end-of-stream error carrying the objects read so far (checked
/// before the triple, like the source); otherwise the terminus triple ends the
/// list and any other triple opens an index-addressed entry. Callers supply the
/// already-read managed-type triple alongside the stream state. The `as` cast
/// mirrors the source `int`-to-`long` widening: object counts never approach the
/// 64-bit range.
pub const fn step_terminated_loop(
    stream_exhausted: bool,
    objects_read: usize,
    triple_class: &str,
    triple_namespace: &str,
    triple_assembly: &str,
) -> Result<TerminatedLoopAction, ManagedRegistryError> {
    if stream_exhausted {
        return Err(ManagedRegistryError::UnterminatedRegistry { objects_read });
    }
    if is_terminus(triple_class, triple_namespace, triple_assembly) {
        Ok(TerminatedLoopAction::Finish)
    } else {
        Ok(TerminatedLoopAction::ReadIndexedObject {
            reference_id: objects_read as i64,
        })
    }
}

/// Classifies one registry reference identifier: unknown and null references carry
/// no content, everything else does (subject to the empty class/assembly rule the
/// content reader applies).
pub const fn classify_reference_id(identifier: i64) -> ReferenceIdKind {
    match identifier {
        UNKNOWN_REFERENCE_ID => ReferenceIdKind::Unknown,
        NULL_REFERENCE_ID => ReferenceIdKind::Null,
        _ => ReferenceIdKind::Content,
    }
}

/// One framed registry entry: the decisions `ReadReferencedObject` makes without
/// the type resolver. Callers read the managed-type triple first (stopping on
/// `is_terminus` in the terminated format, where entries carry no stored
/// identifier), then the entry identifier in the counted format, and use
/// `has_content` to decide whether content bytes follow.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RegistryEntryFrame {
    pub reference_id: i64,
    pub kind: ReferenceIdKind,
    /// Whether content bytes follow: sentinels and empty class/assembly names
    /// carry no content (`ReadReferencedObjectData`).
    pub has_content: bool,
}

/// Frames one registry entry identifier with its content decision, giving the
/// content gate below its production caller.
pub const fn frame_registry_entry(
    reference_id: i64,
    class_name_is_empty: bool,
    assembly_name_is_empty: bool,
) -> RegistryEntryFrame {
    let kind = classify_reference_id(reference_id);
    RegistryEntryFrame {
        reference_id,
        kind,
        has_content: referenced_object_has_content(
            kind,
            class_name_is_empty,
            assembly_name_is_empty,
        ),
    }
}

/// `ReadReferencedObjectData` content gate over caller facts: null and unknown
/// references have no content, and neither does an entry whose class or assembly
/// name is empty. Everything else proceeds to type resolution.
pub const fn referenced_object_has_content(
    kind: ReferenceIdKind,
    class_name_is_empty: bool,
    assembly_name_is_empty: bool,
) -> bool {
    matches!(kind, ReferenceIdKind::Content) && !class_name_is_empty && !assembly_name_is_empty
}

/// What resolving one referenced object's serializable type decides
/// (`ReadReferencedObjectData` without the content read, which needs the resolver).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReferencedTypeOutcome {
    /// The type resolved: content bytes follow.
    Resolved,
    /// The type did not resolve: the source answers with an invalid-data error
    /// naming the type and the resolver's reason, never a silent null.
    Unresolved { message: String },
}

/// Decides whether content bytes follow a resolved type lookup. The unique name
/// arrives composed (the caller strips the assembly extension and joins the
/// triple with the script-identity kernels); an absent resolver reason reads as
/// empty, mirroring the source string interpolation.
pub fn decide_referenced_type_resolution(
    resolved: bool,
    unique_name: &str,
    failure_reason: &str,
) -> ReferencedTypeOutcome {
    if resolved {
        ReferencedTypeOutcome::Resolved
    } else {
        ReferencedTypeOutcome::Unresolved {
            message: format!(
                "Could not resolve the referenced type {unique_name}. Reason: {failure_reason}"
            ),
        }
    }
}

/// The counted-format capacity guard: the stream must hold at least one byte per
/// object (`remainingBytes < count`), mirroring the source `EndOfStreamException`.
/// A negative count is refused explicitly where the source allocation throws.
pub const fn check_counted_capacity(
    remaining_bytes: u64,
    object_count: i32,
) -> Result<(), ManagedRegistryError> {
    if object_count < 0 {
        return Err(ManagedRegistryError::NegativeObjectCount {
            count: object_count,
        });
    }
    if remaining_bytes < object_count as u64 {
        return Err(ManagedRegistryError::TruncatedRegistry {
            remaining_bytes,
            object_count,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        check_counted_capacity, classify_reference_id, classify_registry_format,
        frame_registry_entry, is_terminus, referenced_object_has_content, step_terminated_loop,
        ManagedRegistryError, ReferenceIdKind, RegistryFormat, TerminatedLoopAction,
        COUNTED_FORMAT, NULL_REFERENCE_ID, TERMINATED_FORMAT, TERMINUS_ASSEMBLY_NAME,
        TERMINUS_CLASS_NAME, TERMINUS_NAMESPACE, UNKNOWN_REFERENCE_ID,
    };

    #[test]
    fn format_words_dispatch_both_layouts() {
        assert_eq!(TERMINATED_FORMAT, 1);
        assert_eq!(COUNTED_FORMAT, 2);
        assert_eq!(classify_registry_format(1), Ok(RegistryFormat::Terminated));
        assert_eq!(classify_registry_format(2), Ok(RegistryFormat::Counted));
        assert_eq!(
            classify_registry_format(0),
            Err(ManagedRegistryError::UnsupportedFormat { version: 0 })
        );
        assert_eq!(
            classify_registry_format(3),
            Err(ManagedRegistryError::UnsupportedFormat { version: 3 })
        );
    }

    #[test]
    fn terminus_needs_all_three_names() {
        assert_eq!(TERMINUS_CLASS_NAME, "Terminus");
        assert_eq!(TERMINUS_NAMESPACE, "UnityEngine.DMAT");
        assert_eq!(TERMINUS_ASSEMBLY_NAME, "FAKE_ASM");
        assert!(is_terminus("Terminus", "UnityEngine.DMAT", "FAKE_ASM"));
        assert!(!is_terminus(
            "Terminus",
            "UnityEngine.DMAT",
            "Assembly-CSharp"
        ));
        assert!(!is_terminus("Terminus", "UnityEngine", "FAKE_ASM"));
        assert!(!is_terminus("Other", "UnityEngine.DMAT", "FAKE_ASM"));
        assert!(!is_terminus("", "", ""));
    }

    #[test]
    fn reference_identifiers_classify_sentinels() {
        assert_eq!(UNKNOWN_REFERENCE_ID, -1);
        assert_eq!(NULL_REFERENCE_ID, -2);
        assert_eq!(classify_reference_id(-1), ReferenceIdKind::Unknown);
        assert_eq!(classify_reference_id(-2), ReferenceIdKind::Null);
        assert_eq!(classify_reference_id(0), ReferenceIdKind::Content);
        assert_eq!(classify_reference_id(7), ReferenceIdKind::Content);
    }

    #[test]
    fn counted_capacity_guard_matches_the_source_rule() {
        assert!(check_counted_capacity(8, 8).is_ok());
        assert!(check_counted_capacity(100, 0).is_ok());
        assert_eq!(
            check_counted_capacity(7, 8),
            Err(ManagedRegistryError::TruncatedRegistry {
                remaining_bytes: 7,
                object_count: 8
            })
        );
        assert_eq!(
            check_counted_capacity(100, -1),
            Err(ManagedRegistryError::NegativeObjectCount { count: -1 })
        );
    }

    #[test]
    fn entry_frames_carry_the_content_decision() {
        let entry = frame_registry_entry(7, false, false);
        assert_eq!(entry.reference_id, 7);
        assert_eq!(entry.kind, ReferenceIdKind::Content);
        assert!(entry.has_content);
        // Sentinels carry no content, even with names present.
        assert!(!frame_registry_entry(-1, false, false).has_content);
        assert!(!frame_registry_entry(-2, false, false).has_content);
        // Empty class or assembly names carry no content either.
        assert!(!frame_registry_entry(7, true, false).has_content);
        assert!(!frame_registry_entry(7, false, true).has_content);
        assert!(!frame_registry_entry(7, true, true).has_content);
        // Other negative identifiers still classify as content, like the source.
        assert!(frame_registry_entry(-3, false, false).has_content);
        assert!(referenced_object_has_content(
            ReferenceIdKind::Content,
            false,
            false
        ));
        assert!(!referenced_object_has_content(
            ReferenceIdKind::Unknown,
            false,
            false
        ));
        assert!(!referenced_object_has_content(
            ReferenceIdKind::Null,
            false,
            false
        ));
    }

    #[test]
    fn unresolved_referenced_types_name_the_type_and_reason() {
        use super::{decide_referenced_type_resolution, ReferencedTypeOutcome};
        assert_eq!(
            decide_referenced_type_resolution(true, "[A]G.N", "unused"),
            ReferencedTypeOutcome::Resolved
        );
        assert_eq!(
            decide_referenced_type_resolution(false, "[A]G.N", "no such type"),
            ReferencedTypeOutcome::Unresolved {
                message: "Could not resolve the referenced type [A]G.N. Reason: no such type"
                    .to_owned(),
            }
        );
        // An absent resolver reason reads as empty, like the source interpolation.
        assert_eq!(
            decide_referenced_type_resolution(false, "[A]N", ""),
            ReferencedTypeOutcome::Unresolved {
                message: "Could not resolve the referenced type [A]N. Reason: ".to_owned(),
            }
        );
    }

    #[test]
    fn terminated_loop_steps_follow_the_source_control_flow() {
        // The terminus triple finishes the list.
        assert_eq!(
            step_terminated_loop(false, 0, "Terminus", "UnityEngine.DMAT", "FAKE_ASM"),
            Ok(TerminatedLoopAction::Finish)
        );
        // Any other triple opens an entry addressed by its index.
        assert_eq!(
            step_terminated_loop(false, 2, "Player", "", "Assembly-CSharp"),
            Ok(TerminatedLoopAction::ReadIndexedObject { reference_id: 2 })
        );
        // An exhausted stream errors with the objects read so far, even when
        // the (unread) triple would have been the terminus.
        assert_eq!(
            step_terminated_loop(true, 3, "Terminus", "UnityEngine.DMAT", "FAKE_ASM"),
            Err(ManagedRegistryError::UnterminatedRegistry { objects_read: 3 })
        );
        assert_eq!(
            step_terminated_loop(true, 0, "Player", "", "Assembly-CSharp"),
            Err(ManagedRegistryError::UnterminatedRegistry { objects_read: 0 })
        );
    }
}
