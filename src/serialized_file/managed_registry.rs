//! `ManagedReferenceRegistryReader` framing ported from
//! `AssetRipper.Import/Structure/Assembly/Serializable/ManagedReferenceRegistryReader.cs`.
//!
//! Ports the format dispatch and structural guards, which need no type resolver:
//! the terminated/counted format discriminants, the terminus triple, the
//! unknown/null reference identifiers, the counted-format capacity guard, the
//! terminated-loop control decisions, per-entry framing (identifier
//! classification plus the content-presence gate), and the type-resolution
//! failure diagnostic. Entry type triples and object payloads decode through
//! the resolver-backed reader below:
//! `ManagedReferenceRegistryReader.Read` plus `ReadTerminatedObjects`,
//! `ReadCountedObjects`, `ReadManagedType`, `ReadReferencedObject`, and
//! `ReadReferencedObjectData` over caller-supplied file reference types
//! (`TypeTreeResolver.TryGetSerializableType`).
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
        frame_registry_entry, is_terminus, read_managed_registry, referenced_object_has_content,
        step_terminated_loop, ManagedRegistryEntryData, ManagedRegistryError,
        ManagedRegistryReadError, ManagedRegistryReadOptions, ReferenceIdKind, RegistryFormat,
        TerminatedLoopAction, COUNTED_FORMAT, NULL_REFERENCE_ID, TERMINATED_FORMAT,
        TERMINUS_ASSEMBLY_NAME, TERMINUS_CLASS_NAME, TERMINUS_NAMESPACE, UNKNOWN_REFERENCE_ID,
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
    fn registry_cursor_uses_file_endian_stream_offset_and_string_bounds() {
        let decode_limits = crate::serialized_file::object::ObjectDecodeLimits::default();
        let options = ManagedRegistryReadOptions {
            endian: crate::io::binary::Endian::Big,
            arrays_align: true,
            stream_offset: 1,
            registry_depth: 1,
            object_decode_limits: decode_limits,
            max_entries: 8,
            max_total_decoded_allocation_bytes: decode_limits.max_decoded_allocation_bytes,
        };
        let mut empty = Vec::new();
        empty.extend_from_slice(&COUNTED_FORMAT.to_be_bytes());
        empty.extend_from_slice(&0i32.to_be_bytes());
        empty.extend_from_slice(&[0, 0, 0]);
        let read = read_managed_registry(&empty, options, &[]).unwrap();
        assert_eq!(read.version, COUNTED_FORMAT);
        assert!(read.entries.is_empty());
        assert_eq!(read.consumed_bytes, 11);

        let mut oversized_name = Vec::new();
        oversized_name.extend_from_slice(&COUNTED_FORMAT.to_be_bytes());
        oversized_name.extend_from_slice(&1i32.to_be_bytes());
        oversized_name.extend_from_slice(&7i64.to_be_bytes());
        oversized_name.extend_from_slice(&3i32.to_be_bytes());
        oversized_name.extend_from_slice(b"Long");
        oversized_name.extend_from_slice(&0i32.to_be_bytes());
        oversized_name.extend_from_slice(&0i32.to_be_bytes());
        let bounded = ManagedRegistryReadOptions {
            stream_offset: 0,
            object_decode_limits: crate::serialized_file::object::ObjectDecodeLimits {
                max_string_bytes: 2,
                ..decode_limits
            },
            ..options
        };
        assert!(matches!(
            read_managed_registry(&oversized_name, bounded, &[]),
            Err(ManagedRegistryReadError::StringLimitExceeded {
                length: 3,
                limit: 2,
                what: "managed type class name",
            })
        ));
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

    /// Full-blob differential against the shipped `ManagedReferenceRegistryReader`
    /// (probe `Temp/opencode/mregprobe`, SDK 10.0.401): every `REGBLOB` input
    /// replays through `read_managed_registry` and every `REGREAD`/`REGENTRY`/
    /// `REGDATA` row must match. This closes entry type-triple reads and payload
    /// decoding: triples resolve through the file reference types, payloads
    /// decode against the resolver-built trees, and every failure is explicit.
    #[test]
    fn registry_blobs_reproduce_the_reference_reader_dump() {
        use crate::io::binary::Endian;
        use crate::serialized_file::metadata::{SerializedTypeReference, TypeTree, TypeTreeNode};
        use crate::serialized_file::object::SerializedValue;

        fn node(type_name: &str, name: &str, level: u8) -> TypeTreeNode {
            TypeTreeNode {
                level,
                version: 1,
                type_flags: 0,
                type_string_offset: None,
                name_string_offset: None,
                type_name: Some(type_name.to_owned()),
                name: Some(name.to_owned()),
                byte_size: 0,
                index: 0,
                meta_flags: 0,
                reference_type_hash: None,
            }
        }

        fn reference(
            class: &str,
            namespace: &str,
            assembly: &str,
            nodes: Vec<TypeTreeNode>,
        ) -> SerializedTypeReference {
            SerializedTypeReference {
                raw_type_id: 0,
                type_id_for_hash: 0,
                is_stripped_type: false,
                script_type_index: 0,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(TypeTree {
                    nodes,
                    string_buffer: Vec::new(),
                }),
                class_name: Some(class.to_owned()),
                namespace: Some(namespace.to_owned()),
                assembly_name: Some(assembly.to_owned()),
            }
        }

        fn scalar(value: &SerializedValue) -> String {
            match value {
                SerializedValue::Bool(v) => v.to_string(),
                SerializedValue::I8(v) => v.to_string(),
                SerializedValue::U8(v) => v.to_string(),
                SerializedValue::I16(v) => v.to_string(),
                SerializedValue::U16(v) => v.to_string(),
                SerializedValue::Char(v) => v.to_string(),
                SerializedValue::I32(v) => v.to_string(),
                SerializedValue::U32(v) => v.to_string(),
                SerializedValue::I64(v) => v.to_string(),
                SerializedValue::U64(v) => v.to_string(),
                SerializedValue::F32(v) => format!("{v:?}"),
                SerializedValue::F64(v) => format!("{v:?}"),
                SerializedValue::StringBytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
                SerializedValue::Bytes(bytes) => format!("{bytes:02X?}"),
                SerializedValue::Array(_) => "<array>".to_owned(),
                SerializedValue::Struct(_) => "<struct>".to_owned(),
                SerializedValue::PPtr { file_id, path_id } => {
                    format!("<pptr {file_id} {path_id}>")
                }
                SerializedValue::ManagedReferenceId { reference_id, .. } => {
                    format!("<managed-reference {reference_id}>")
                }
                SerializedValue::ManagedReferenceRegistry(_) => "<managed-registry>".to_owned(),
                SerializedValue::Raw { type_name, .. } => format!("<raw {type_name}>"),
            }
        }

        fn field<'a>(value: &'a SerializedValue, name: &str) -> &'a SerializedValue {
            let SerializedValue::Struct(fields) = value else {
                panic!("expected a struct for field {name}");
            };
            &fields
                .iter()
                .find(|field| field.name == name)
                .unwrap_or_else(|| panic!("missing field {name}"))
                .value
        }

        let references = vec![
            reference(
                "Player",
                "",
                "Assembly-CSharp",
                vec![
                    node("Player", "Base", 0),
                    node("int", "m_Health", 1),
                    node("float", "m_Speed", 1),
                ],
            ),
            reference(
                "Enemy",
                "",
                "Assembly-CSharp",
                vec![
                    node("Enemy", "Base", 0),
                    node("string", "m_Name", 1),
                    node("Array", "Array", 2),
                    node("int", "size", 3),
                    node("char", "data", 3),
                    node("int", "m_Level", 1),
                ],
            ),
        ];
        let options = ManagedRegistryReadOptions {
            endian: Endian::Little,
            arrays_align: true,
            stream_offset: 0,
            registry_depth: 1,
            object_decode_limits: crate::serialized_file::object::ObjectDecodeLimits::default(),
            max_entries: crate::serialized_file::object::ObjectDecodeLimits::default()
                .max_array_elements,
            max_total_decoded_allocation_bytes:
                crate::serialized_file::object::ObjectDecodeLimits::default()
                    .max_decoded_allocation_bytes,
        };
        let fixture = include_str!("managed_registry_fixture.txt");
        let mut blobs = std::collections::HashMap::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts[0] == "REGBLOB" {
                assert_eq!(parts.len(), 3, "{line}");
                let mut bytes = Vec::new();
                let hex = parts[2].trim();
                assert!(hex.len().is_multiple_of(2), "{line}");
                for pair in 0..hex.len() / 2 {
                    bytes.push(
                        u8::from_str_radix(&hex[pair * 2..pair * 2 + 2], 16)
                            .unwrap_or_else(|_| panic!("bad hex {line}")),
                    );
                }
                blobs.insert(parts[1].to_owned(), bytes);
            } else if parts[0] != "REGREAD" && parts[0] != "REGENTRY" && parts[0] != "REGDATA" {
                panic!("unknown fixture row {} in {line}", parts[0]);
            }
        }
        let mut cases = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "REGREAD" => {
                    let case = parts[1];
                    let blob = blobs
                        .remove(case)
                        .unwrap_or_else(|| panic!("no blob {case}"));
                    cases += 1;
                    match parts[2] {
                        "OK" => {
                            assert_eq!(parts.len(), 6, "{line}");
                            let read = read_managed_registry(&blob, options, &references)
                                .unwrap_or_else(|error| panic!("{case} failed to read: {error}"));
                            assert_eq!(read.consumed_bytes.to_string(), parts[3], "{line}");
                            assert_eq!(read.version.to_string(), parts[4], "{line}");
                            assert_eq!(read.entries.len().to_string(), parts[5], "{line}");
                            for (index, entry) in read.entries.iter().enumerate() {
                                let expected_entry = fixture
                                    .lines()
                                    .map(str::trim)
                                    .find(|row| {
                                        row.starts_with(&format!("REGENTRY|{case}|{index}|"))
                                    })
                                    .unwrap_or_else(|| panic!("no entry {case} {index}"));
                                let expected: Vec<&str> = expected_entry.split('|').collect();
                                assert_eq!(expected.len(), 7, "{expected_entry}");
                                assert_eq!(entry.reference_id.to_string(), expected[3], "{line}");
                                assert_eq!(entry.class_name, expected[4], "{line}");
                                assert_eq!(entry.namespace_name, expected[5], "{line}");
                                assert_eq!(entry.assembly_name, expected[6], "{line}");
                                let expected_data: Vec<String> = fixture
                                    .lines()
                                    .map(str::trim)
                                    .filter(|row| {
                                        row.starts_with(&format!("REGDATA|{case}|{index}|"))
                                    })
                                    .map(str::to_owned)
                                    .collect();
                                match &entry.data {
                                    ManagedRegistryEntryData::Empty => {
                                        assert_eq!(expected_data.len(), 1, "{line}");
                                        assert!(
                                            expected_data[0].ends_with(&format!(
                                                "REGDATA|{case}|{index}||EMPTY"
                                            )),
                                            "{line}",
                                        );
                                    }
                                    ManagedRegistryEntryData::Content(value) => {
                                        for row in &expected_data {
                                            let cells: Vec<&str> = row.split('|').collect();
                                            assert_eq!(cells.len(), 5, "{row}");
                                            assert_ne!(cells[3], "", "{row}");
                                            assert_eq!(
                                                scalar(field(value, cells[3])),
                                                cells[4],
                                                "{row}"
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        "THROW" => {
                            let error = read_managed_registry(&blob, options, &references)
                                .err()
                                .unwrap_or_else(|| panic!("{case} unexpectedly read clean"));
                            match case {
                                "counted-unresolvable" => {
                                    assert!(
                                        matches!(
                                            error,
                                            ManagedRegistryReadError::UnresolvableType { .. }
                                        ),
                                        "{case}: {error}",
                                    );
                                    assert!(
                                        error.to_string().contains("[NoAsm]Nope")
                                            && error.to_string().contains(
                                                "not found in the set of available type trees"
                                            ),
                                        "{case}: {error}",
                                    );
                                }
                                "bad-format" => assert_eq!(
                                    error,
                                    ManagedRegistryReadError::Framing(
                                        ManagedRegistryError::UnsupportedFormat { version: 9 }
                                    ),
                                    "{case}",
                                ),
                                // The source span slice throws ArgumentOutOfRange
                                // here; the port refuses the truncation
                                // explicitly. Both sides fail; neither succeeds.
                                "truncated-count" => assert!(
                                    matches!(
                                        error,
                                        ManagedRegistryReadError::TruncatedInput { .. }
                                    ),
                                    "{case}: {error}",
                                ),
                                "unterminated" => assert_eq!(
                                    error,
                                    ManagedRegistryReadError::Framing(
                                        ManagedRegistryError::UnterminatedRegistry {
                                            objects_read: 1
                                        }
                                    ),
                                    "{case}",
                                ),
                                other => panic!("unexpected throw case {other}"),
                            }
                        }
                        other => panic!("unexpected read status {other} in {line}"),
                    }
                }
                "REGENTRY" | "REGDATA" | "REGBLOB" => {}
                other => panic!("unknown fixture row {other} in {line}"),
            }
        }
        assert!(blobs.is_empty(), "every blob replays");
        assert_eq!(cases, 8, "all probed registry cases replay");
    }
}

/// Why a managed reference registry blob cannot be read.
#[derive(Clone, Debug, PartialEq)]
pub enum ManagedRegistryReadError {
    /// The blob framing fails (format, count, termination).
    Framing(ManagedRegistryError),
    /// The blob ends mid-value: the source answers with end-of-stream (or, when
    /// the over-long counted claim reads past the span, an argument-range throw
    /// from the span slice); here every truncation is explicit.
    TruncatedInput {
        /// The byte offset of the failed read.
        offset: usize,
        /// What was being read.
        what: &'static str,
    },
    /// A serialized string has a negative byte length.
    InvalidStringLength { length: i32, what: &'static str },
    /// A serialized string exceeds the owning object decoder's byte bound.
    StringLimitExceeded {
        length: usize,
        limit: usize,
        what: &'static str,
    },
    /// A content-bearing entry names a type the resolver cannot build, which the
    /// source answers with an invalid-data error naming the type and the reason.
    UnresolvableType {
        /// The entry index in read order.
        entry_index: usize,
        /// The assembly-stripped unique type name.
        unique_name: String,
        /// The resolver's failure reason.
        reason: String,
    },
    /// A content payload does not match its resolved tree.
    PayloadDecode {
        /// The entry index in read order.
        entry_index: usize,
        /// The payload decode failure.
        error: super::object::ObjectDecodeError,
    },
    /// The registry exceeds a caller-supplied product bound.
    EntryCountLimitExceeded { count: usize, limit: usize },
    /// Nested registries exceed the caller-supplied recursion bound.
    RegistryDepthLimitExceeded { requested: usize, limit: usize },
    /// The stream-relative start of a nested payload overflows the address space.
    StreamOffsetOverflow {
        registry_offset: usize,
        relative_offset: usize,
    },
    /// Decoding all registry payloads would exceed the owning object allocation budget.
    AllocationLimitExceeded { requested: usize, limit: usize },
    /// A bounded registry allocation could not be reserved.
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for ManagedRegistryReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Framing(error) => write!(formatter, "{error}"),
            Self::TruncatedInput { offset, what } => write!(
                formatter,
                "managed reference registry ends at offset {offset} while reading {what}"
            ),
            Self::InvalidStringLength { length, what } => write!(
                formatter,
                "managed reference registry has invalid {what} byte length {length}"
            ),
            Self::StringLimitExceeded { length, limit, what } => write!(
                formatter,
                "managed reference registry {what} has {length} bytes, exceeding limit {limit}"
            ),
            Self::UnresolvableType {
                unique_name,
                reason,
                ..
            } => write!(
                formatter,
                "Could not resolve the referenced type {unique_name}. Reason: {reason}"
            ),
            Self::PayloadDecode { error, .. } => write!(formatter, "{error}"),
            Self::EntryCountLimitExceeded { count, limit } => write!(
                formatter,
                "managed reference registry has {count} entries, exceeding limit {limit}"
            ),
            Self::RegistryDepthLimitExceeded { requested, limit } => write!(
                formatter,
                "managed reference registry depth {requested} exceeds limit {limit}"
            ),
            Self::StreamOffsetOverflow {
                registry_offset,
                relative_offset,
            } => write!(
                formatter,
                "managed reference payload offset {registry_offset} + {relative_offset} overflows the address space"
            ),
            Self::AllocationLimitExceeded { requested, limit } => write!(
                formatter,
                "managed reference registry would allocate {requested} bytes, exceeding limit {limit}"
            ),
            Self::AllocationFailed { resource, requested } => write!(
                formatter,
                "could not allocate {requested} entries for managed reference {resource}"
            ),
        }
    }
}

impl Error for ManagedRegistryReadError {}

impl From<ManagedRegistryError> for ManagedRegistryReadError {
    fn from(error: ManagedRegistryError) -> Self {
        Self::Framing(error)
    }
}

/// How a registry blob is read: the collection byte order and whether arrays
/// align (`unity_version_aligns_arrays` of the owning collection version).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedRegistryReadOptions {
    /// The collection byte order.
    pub endian: crate::io::binary::Endian,
    /// Whether arrays align (Unity 2017 and newer).
    pub arrays_align: bool,
    /// Position of this registry slice in the owning object payload, used for stream alignment.
    pub stream_offset: usize,
    /// One-based depth of this registry relative to the owning serialized object.
    pub registry_depth: usize,
    /// Allocation and nesting limits for each dynamic payload.
    pub object_decode_limits: super::object::ObjectDecodeLimits,
    /// Maximum number of registry entries.
    pub max_entries: usize,
    /// Total memory budget for decoded payloads and registry records.
    pub max_total_decoded_allocation_bytes: usize,
}

/// Resolves the serialized fields of a managed-reference object from an available assembly
/// catalog. This is the source `BaseManager` path used when a MonoBehaviour has no file
/// TypeTree; ordinary serialized `referenceTypes` remain the fallback for the file-tree path.
pub(crate) trait ManagedReferenceTypeTreeResolver {
    fn resolve_type_tree(
        &self,
        script: &crate::processing::script_identity::ScriptIdentifier,
    ) -> Result<Option<super::metadata::TypeTree>, String>;
}

impl<F> ManagedReferenceTypeTreeResolver for F
where
    F: Fn(
        &crate::processing::script_identity::ScriptIdentifier,
    ) -> Result<Option<super::metadata::TypeTree>, String>,
{
    fn resolve_type_tree(
        &self,
        script: &crate::processing::script_identity::ScriptIdentifier,
    ) -> Result<Option<super::metadata::TypeTree>, String> {
        self(script)
    }
}

/// One decoded registry entry (`ReadReferencedObject`).
#[derive(Clone, Debug, PartialEq)]
pub struct ManagedRegistryEntry {
    /// The stored identifier (counted format) or the index (terminated format).
    pub reference_id: i64,
    /// The referenced class name.
    pub class_name: String,
    /// The referenced namespace name.
    pub namespace_name: String,
    /// The referenced assembly name.
    pub assembly_name: String,
    /// The decoded content, or empty for sentinels and empty triples.
    pub data: ManagedRegistryEntryData,
}

/// A decoded registry entry payload.
#[derive(Clone, Debug, PartialEq)]
pub enum ManagedRegistryEntryData {
    /// Sentinels and empty triples carry no content.
    Empty,
    /// Content bytes decoded against the resolved tree.
    Content(super::object::SerializedValue),
}

/// A decoded managed reference registry: the version word plus entries in read
/// order and the aligned consumed byte count (`Read` aligns the array after the
/// entries, which is not necessarily true of the objects themselves).
#[derive(Clone, Debug, PartialEq)]
pub struct ManagedRegistryRead {
    /// The registry version word (1 for terminated, 2 for counted).
    pub version: i32,
    /// The entries in read order.
    pub entries: Vec<ManagedRegistryEntry>,
    /// The aligned consumed byte count.
    pub consumed_bytes: usize,
    /// Estimated owned allocation bytes, including decoded payload values and registry records.
    pub allocated_bytes: usize,
}

/// `ManagedReferenceRegistryReader.Read`: decode one registry blob against the
/// caller-supplied file reference types. `references` are the owning file's
/// serialized reference types backing `TypeTreeResolver.TryGetSerializableType`;
/// unknown script types fail explicitly like the source invalid-data error.
pub fn read_managed_registry(
    bytes: &[u8],
    options: ManagedRegistryReadOptions,
    references: &[super::metadata::SerializedTypeReference],
) -> Result<ManagedRegistryRead, ManagedRegistryReadError> {
    read_managed_registry_with_resolver(bytes, options, references, None)
}

/// Decode one managed-reference registry, allowing source-style assembly resolution before
/// falling back to serialized reference TypeTrees.
pub(crate) fn read_managed_registry_with_resolver(
    bytes: &[u8],
    options: ManagedRegistryReadOptions,
    references: &[super::metadata::SerializedTypeReference],
    resolver: Option<&dyn ManagedReferenceTypeTreeResolver>,
) -> Result<ManagedRegistryRead, ManagedRegistryReadError> {
    if options.registry_depth > options.object_decode_limits.max_managed_registry_depth {
        return Err(ManagedRegistryReadError::RegistryDepthLimitExceeded {
            requested: options.registry_depth,
            limit: options.object_decode_limits.max_managed_registry_depth,
        });
    }
    let mut cursor = Cursor::new(bytes, options.endian, options.stream_offset);
    let version = cursor.read_i32("registry version word")?;
    let format = classify_registry_format(version)?;
    let mut entries = Vec::new();
    let mut allocated_bytes = 0usize;
    match format {
        RegistryFormat::Counted => {
            let count = cursor.read_i32("registry object count")?;
            check_counted_capacity(bytes.len().saturating_sub(cursor.position()) as u64, count)?;
            let count = usize::try_from(count).map_err(|_| {
                ManagedRegistryReadError::Framing(ManagedRegistryError::NegativeObjectCount {
                    count,
                })
            })?;
            if count > options.max_entries {
                return Err(ManagedRegistryReadError::EntryCountLimitExceeded {
                    count,
                    limit: options.max_entries,
                });
            }
            let entry_bytes = count
                .checked_mul(std::mem::size_of::<ManagedRegistryEntry>())
                .ok_or(ManagedRegistryReadError::AllocationLimitExceeded {
                    requested: usize::MAX,
                    limit: options.max_total_decoded_allocation_bytes,
                })?;
            if entry_bytes > options.max_total_decoded_allocation_bytes {
                return Err(ManagedRegistryReadError::AllocationLimitExceeded {
                    requested: entry_bytes,
                    limit: options.max_total_decoded_allocation_bytes,
                });
            }
            entries.try_reserve_exact(count).map_err(|_| {
                ManagedRegistryReadError::AllocationFailed {
                    resource: "registry entries",
                    requested: count,
                }
            })?;
            allocated_bytes = entry_bytes;
            for _ in 0..count {
                let entry_index = entries.len();
                let reference_id = cursor.read_i64("registry reference identifier")?;
                let (triple, triple_bytes) = cursor.read_managed_type_triple(
                    options.object_decode_limits.max_string_bytes,
                    options
                        .max_total_decoded_allocation_bytes
                        .saturating_sub(allocated_bytes),
                )?;
                allocated_bytes = add_registry_allocation(
                    allocated_bytes,
                    triple_bytes,
                    options.max_total_decoded_allocation_bytes,
                )?;
                let entry_options = options_with_remaining_budget(options, allocated_bytes);
                let (entry, payload_bytes) = read_entry(
                    &mut cursor,
                    entry_options,
                    references,
                    resolver,
                    entry_index,
                    reference_id,
                    triple,
                )?;
                allocated_bytes = add_registry_allocation(
                    allocated_bytes,
                    payload_bytes,
                    options.max_total_decoded_allocation_bytes,
                )?;
                entries.push(entry);
            }
        }
        RegistryFormat::Terminated => loop {
            if cursor.position() >= bytes.len() {
                return Err(ManagedRegistryError::UnterminatedRegistry {
                    objects_read: entries.len(),
                }
                .into());
            }
            let (triple, triple_bytes) = cursor.read_managed_type_triple(
                options.object_decode_limits.max_string_bytes,
                options
                    .max_total_decoded_allocation_bytes
                    .saturating_sub(allocated_bytes),
            )?;
            if is_terminus(
                &triple.class_name,
                &triple.namespace_name,
                &triple.assembly_name,
            ) {
                break;
            }
            let entry_index = entries.len();
            if entry_index >= options.max_entries {
                return Err(ManagedRegistryReadError::EntryCountLimitExceeded {
                    count: entry_index.saturating_add(1),
                    limit: options.max_entries,
                });
            }
            entries
                .try_reserve(1)
                .map_err(|_| ManagedRegistryReadError::AllocationFailed {
                    resource: "registry entries",
                    requested: entry_index.saturating_add(1),
                })?;
            allocated_bytes = add_registry_allocation(
                allocated_bytes,
                std::mem::size_of::<ManagedRegistryEntry>(),
                options.max_total_decoded_allocation_bytes,
            )?;
            allocated_bytes = add_registry_allocation(
                allocated_bytes,
                triple_bytes,
                options.max_total_decoded_allocation_bytes,
            )?;
            let entry_options = options_with_remaining_budget(options, allocated_bytes);
            let (entry, payload_bytes) = read_entry(
                &mut cursor,
                entry_options,
                references,
                resolver,
                entry_index,
                entry_index as i64,
                triple,
            )?;
            allocated_bytes = add_registry_allocation(
                allocated_bytes,
                payload_bytes,
                options.max_total_decoded_allocation_bytes,
            )?;
            entries.push(entry);
        },
    }
    cursor.align(4, "registry array alignment")?;
    Ok(ManagedRegistryRead {
        version,
        entries,
        consumed_bytes: cursor.position(),
        allocated_bytes,
    })
}

fn add_registry_allocation(
    allocated: usize,
    requested: usize,
    limit: usize,
) -> Result<usize, ManagedRegistryReadError> {
    let total = allocated.checked_add(requested).ok_or(
        ManagedRegistryReadError::AllocationLimitExceeded {
            requested: usize::MAX,
            limit,
        },
    )?;
    if total > limit {
        return Err(ManagedRegistryReadError::AllocationLimitExceeded {
            requested: total,
            limit,
        });
    }
    Ok(total)
}

fn options_with_remaining_budget(
    mut options: ManagedRegistryReadOptions,
    already_allocated: usize,
) -> ManagedRegistryReadOptions {
    let remaining = options
        .max_total_decoded_allocation_bytes
        .saturating_sub(already_allocated);
    options.max_total_decoded_allocation_bytes = remaining;
    options.object_decode_limits.max_decoded_allocation_bytes = options
        .object_decode_limits
        .max_decoded_allocation_bytes
        .min(remaining);
    options
}

/// A managed-type triple: the class, namespace, and assembly names identifying
/// one referenced object type (`ReadManagedType`).
#[derive(Clone, Debug, Eq, PartialEq)]
struct ManagedTypeTriple {
    class_name: String,
    namespace_name: String,
    assembly_name: String,
}

/// `ReadReferencedObject` plus `ReadReferencedObjectData`: frame one entry and
/// decode its content when the gate allows it.
fn read_entry(
    cursor: &mut Cursor<'_>,
    options: ManagedRegistryReadOptions,
    references: &[super::metadata::SerializedTypeReference],
    resolver: Option<&dyn ManagedReferenceTypeTreeResolver>,
    entry_index: usize,
    reference_id: i64,
    triple: ManagedTypeTriple,
) -> Result<(ManagedRegistryEntry, usize), ManagedRegistryReadError> {
    use crate::io::special_names::remove_assembly_file_extension;
    use crate::processing::script_identity::{
        ScriptIdentifier, TYPE_REFERENCE_NOT_FOUND, TYPE_TREE_NODE_BUILD_FAILED,
    };

    let frame = frame_registry_entry(
        reference_id,
        triple.class_name.is_empty(),
        triple.assembly_name.is_empty(),
    );
    let mut payload_allocated_bytes = 0usize;
    let data = if !frame.has_content {
        ManagedRegistryEntryData::Empty
    } else {
        let script = ScriptIdentifier {
            assembly: remove_assembly_file_extension(&triple.assembly_name),
            namespace: triple.namespace_name.clone(),
            name: Some(triple.class_name.clone()),
        };
        let resolved_tree = resolver
            .map(|resolver| resolver.resolve_type_tree(&script))
            .transpose()
            .map_err(|reason| ManagedRegistryReadError::UnresolvableType {
                entry_index,
                unique_name: script.unique_name(),
                reason,
            })?
            .flatten();
        let payload_tree = if let Some(tree) = resolved_tree {
            tree
        } else {
            let matched =
                crate::processing::script_identity::find_type_reference_index(references, &script)
                    .ok_or_else(|| ManagedRegistryReadError::UnresolvableType {
                        entry_index,
                        unique_name: script.unique_name(),
                        reason: TYPE_REFERENCE_NOT_FOUND.to_owned(),
                    })?;
            let tree = references[matched].type_tree.as_ref().ok_or_else(|| {
                ManagedRegistryReadError::UnresolvableType {
                    entry_index,
                    unique_name: script.unique_name(),
                    reason: TYPE_TREE_NODE_BUILD_FAILED.to_owned(),
                }
            })?;
            let built = crate::processing::script_identity::build_type_nodes(&tree.nodes)
                .ok_or_else(|| ManagedRegistryReadError::UnresolvableType {
                    entry_index,
                    unique_name: script.unique_name(),
                    reason: TYPE_TREE_NODE_BUILD_FAILED.to_owned(),
                })?;
            super::metadata::TypeTree {
                nodes: flatten_built_tree(&built),
                string_buffer: Vec::new(),
            }
        };
        let payload = cursor.remaining_bytes();
        let payload_stream_offset = options.stream_offset.checked_add(cursor.position()).ok_or(
            ManagedRegistryReadError::StreamOffsetOverflow {
                registry_offset: options.stream_offset,
                relative_offset: cursor.position(),
            },
        )?;
        let mut decode_limits = options.object_decode_limits;
        decode_limits.max_decoded_allocation_bytes = decode_limits
            .max_decoded_allocation_bytes
            .min(options.max_total_decoded_allocation_bytes);
        let (value, consumed, allocated) = super::object::decode_typetree_payload_with_resolver(
            &payload_tree,
            payload,
            options.endian,
            options.arrays_align,
            references,
            super::object::ManagedPayloadDecodeContext {
                entry_index,
                stream_offset: payload_stream_offset,
                managed_registry_depth: options.registry_depth,
                managed_reference_type_resolver: resolver,
            },
            &decode_limits,
        )
        .map_err(|error| ManagedRegistryReadError::PayloadDecode { entry_index, error })?;
        cursor.advance(consumed, "registry object payload")?;
        payload_allocated_bytes = allocated;
        ManagedRegistryEntryData::Content(value)
    };
    let entry = ManagedRegistryEntry {
        reference_id,
        class_name: triple.class_name,
        namespace_name: triple.namespace_name,
        assembly_name: triple.assembly_name,
        data,
    };
    Ok((entry, payload_allocated_bytes))
}

/// Flatten a built node back into a level-nested node list in declaration order,
/// preserving versions and meta flags for the payload decoder. Root registry
/// children are skipped exactly like `FromReferencedObjectNode` skips them
/// when building a referenced-object type; registries nested below structs are
/// retained and recursively decoded.
pub(crate) fn flatten_built_tree(
    root: &crate::processing::script_identity::BuiltTypeNode,
) -> Vec<super::metadata::TypeTreeNode> {
    use crate::processing::script_identity::BuiltTypeNode;
    fn push(node: &BuiltTypeNode, level: u8, out: &mut Vec<super::metadata::TypeTreeNode>) {
        out.push(super::metadata::TypeTreeNode {
            level,
            version: node.version,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(node.type_name.clone()),
            name: Some(node.name.clone()),
            byte_size: 0,
            index: 0,
            meta_flags: node.meta_flags,
            reference_type_hash: None,
        });
        for child in &node.children {
            push(child, level + 1, out);
        }
    }
    // Node-list levels fit in a byte: file trees never nest deeper. The source
    // excludes the direct root registry marker but retains nested Registry fields.
    let mut out = Vec::new();
    push(root, 0, &mut out);
    // Drop root-registry subtrees (a level-1 marker plus its deeper nodes).
    let mut swept = Vec::with_capacity(out.len());
    let mut dropping = false;
    for node in out {
        let is_marker = node.level == 1
            && node.type_name.as_deref()
                == Some(crate::processing::script_identity::MANAGED_REGISTRY_NAME)
            && node.name.as_deref()
                == Some(crate::processing::script_identity::MANAGED_REGISTRY_FIELD_NAME);
        if is_marker {
            dropping = true;
            continue;
        }
        if dropping {
            if node.level > 1 {
                continue;
            }
            dropping = false;
        }
        swept.push(node);
    }
    swept
}

/// A checked cursor over the registry blob.
struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
    endian: crate::io::binary::Endian,
    stream_offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], endian: crate::io::binary::Endian, stream_offset: usize) -> Self {
        Self {
            bytes,
            position: 0,
            endian,
            stream_offset,
        }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn remaining_bytes(&self) -> &'a [u8] {
        &self.bytes[self.position.min(self.bytes.len())..]
    }

    fn take(
        &mut self,
        count: usize,
        what: &'static str,
    ) -> Result<&'a [u8], ManagedRegistryReadError> {
        let end =
            self.position
                .checked_add(count)
                .ok_or(ManagedRegistryReadError::TruncatedInput {
                    offset: self.position,
                    what,
                })?;
        if end > self.bytes.len() {
            return Err(ManagedRegistryReadError::TruncatedInput {
                offset: self.position,
                what,
            });
        }
        let taken = &self.bytes[self.position..end];
        self.position = end;
        Ok(taken)
    }

    fn advance(
        &mut self,
        count: usize,
        what: &'static str,
    ) -> Result<(), ManagedRegistryReadError> {
        self.take(count, what).map(|_| ())
    }

    fn read_i32(&mut self, what: &'static str) -> Result<i32, ManagedRegistryReadError> {
        let taken = self.take(4, what)?;
        let bytes = [taken[0], taken[1], taken[2], taken[3]];
        Ok(match self.endian {
            crate::io::binary::Endian::Little => i32::from_le_bytes(bytes),
            crate::io::binary::Endian::Big => i32::from_be_bytes(bytes),
        })
    }

    fn read_i64(&mut self, what: &'static str) -> Result<i64, ManagedRegistryReadError> {
        let taken = self.take(8, what)?;
        let bytes = [
            taken[0], taken[1], taken[2], taken[3], taken[4], taken[5], taken[6], taken[7],
        ];
        Ok(match self.endian {
            crate::io::binary::Endian::Little => i64::from_le_bytes(bytes),
            crate::io::binary::Endian::Big => i64::from_be_bytes(bytes),
        })
    }

    /// One length-prefixed aligned string, mirroring the structure string reads.
    fn read_aligned_string(
        &mut self,
        what: &'static str,
        max_string_bytes: usize,
        max_allocation_bytes: usize,
    ) -> Result<String, ManagedRegistryReadError> {
        let length = self.read_i32(what)?;
        let length = usize::try_from(length)
            .map_err(|_| ManagedRegistryReadError::InvalidStringLength { length, what })?;
        if length > max_string_bytes {
            return Err(ManagedRegistryReadError::StringLimitExceeded {
                length,
                limit: max_string_bytes,
                what,
            });
        }
        if length > max_allocation_bytes {
            return Err(ManagedRegistryReadError::AllocationLimitExceeded {
                requested: length,
                limit: max_allocation_bytes,
            });
        }
        let taken = self.take(length, what)?;
        let value = match std::str::from_utf8(taken) {
            Ok(text) => {
                let mut value = String::new();
                value.try_reserve_exact(text.len()).map_err(|_| {
                    ManagedRegistryReadError::AllocationFailed {
                        resource: "managed type string",
                        requested: text.len(),
                    }
                })?;
                value.push_str(text);
                value
            }
            Err(_) => {
                let maximum_lossy_bytes = length.checked_mul(3).ok_or(
                    ManagedRegistryReadError::AllocationLimitExceeded {
                        requested: usize::MAX,
                        limit: max_allocation_bytes,
                    },
                )?;
                if maximum_lossy_bytes > max_allocation_bytes {
                    return Err(ManagedRegistryReadError::AllocationLimitExceeded {
                        requested: maximum_lossy_bytes,
                        limit: max_allocation_bytes,
                    });
                }
                match String::from_utf8_lossy(taken) {
                    std::borrow::Cow::Borrowed(text) => {
                        let mut value = String::new();
                        value.try_reserve_exact(text.len()).map_err(|_| {
                            ManagedRegistryReadError::AllocationFailed {
                                resource: "managed type string",
                                requested: text.len(),
                            }
                        })?;
                        value.push_str(text);
                        value
                    }
                    std::borrow::Cow::Owned(value) => value,
                }
            }
        };
        self.align(4, what)?;
        Ok(value)
    }

    /// `ReadManagedType`: the class/namespace/assembly triple.
    fn read_managed_type_triple(
        &mut self,
        max_string_bytes: usize,
        max_allocation_bytes: usize,
    ) -> Result<(ManagedTypeTriple, usize), ManagedRegistryReadError> {
        let mut allocated_bytes = 0usize;
        let mut read = |cursor: &mut Self, what| {
            let available = max_allocation_bytes.saturating_sub(allocated_bytes);
            let value = cursor.read_aligned_string(what, max_string_bytes, available)?;
            allocated_bytes = allocated_bytes.checked_add(value.len()).ok_or(
                ManagedRegistryReadError::AllocationLimitExceeded {
                    requested: usize::MAX,
                    limit: max_allocation_bytes,
                },
            )?;
            Ok::<String, ManagedRegistryReadError>(value)
        };
        let class_name = read(self, "managed type class name")?;
        let namespace_name = read(self, "managed type namespace name")?;
        let assembly_name = read(self, "managed type assembly name")?;
        Ok((
            ManagedTypeTriple {
                class_name,
                namespace_name,
                assembly_name,
            },
            allocated_bytes,
        ))
    }

    /// The trailing array alignment (`Read` aligns after the entries).
    fn align(
        &mut self,
        boundary: usize,
        what: &'static str,
    ) -> Result<(), ManagedRegistryReadError> {
        let stream_position = self.stream_offset.checked_add(self.position).ok_or(
            ManagedRegistryReadError::TruncatedInput {
                offset: self.position,
                what,
            },
        )?;
        let remainder = stream_position % boundary;
        if remainder != 0 {
            self.advance(boundary - remainder, what)?;
        }
        Ok(())
    }
}
