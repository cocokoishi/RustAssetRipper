mod codec;
mod decode_budget;
mod decode_error;
mod read_flags;
mod recovery;
mod type_tree;
mod value;
mod writer;

pub(crate) use codec::{decode_typetree_payload, decode_typetree_payload_with_resolver};
pub(crate) use decode_budget::ManagedPayloadDecodeContext;
pub use decode_budget::ObjectDecodeLimits;
pub use decode_error::{ObjectDecodeError, ObjectDecodeErrorKind};
pub use read_flags::{
    is_builtin_extra_name, is_engine_resource_name, serialized_file_object_read_flags,
    BUILD_TARGET_NO_TARGET, BUILTIN_EXTRA_NAME_1, BUILTIN_EXTRA_NAME_2, DEFAULT_RESOURCE_NAME_1,
    DEFAULT_RESOURCE_NAME_2, EDITOR_RESOURCE_NAME,
};
pub use recovery::{
    class_id_from_type_name, class_id_type_name, classify_type_tree_usability, raw_data_class_name,
    version_is_unsupported, TypeTreeUsability,
};
pub use value::{
    ParsedObjectPayload, SerializedFieldValue, SerializedValue, SerializedValueLookupError,
    SerializedValueMutationError,
};
pub use writer::{
    default_value_for_type_tree_node, ObjectDefaultValueLimits, ObjectEncodeError,
    ObjectEncodeErrorKind, ObjectEncodeLimits,
};

use super::metadata::unity_version_aligns_arrays;
use type_tree::{
    array_base_is_primitive, array_node_for_value, array_shape, direct_children, field_aligns,
    is_indexed_managed_reference_element, is_managed_reference_element,
    is_managed_references_registry, is_referenced_object_data, is_string_node,
    minimum_encoded_size, pptr_shape, TypeTreeLayout, CHAR_PROPERTY_MASK_FLAG,
};

#[cfg(test)]
use super::transfer_flags::TransferInstructionFlags;
#[cfg(test)]
use codec::decode_field;
#[cfg(test)]
use decode_budget::DecodeBudget;

#[cfg(test)]
#[path = "object/tests/managed_reference_policy.rs"]
mod managed_reference_policy_tests;
#[cfg(test)]
#[path = "object/tests/managed_references.rs"]
mod managed_reference_tests;
#[cfg(test)]
#[path = "object/tests/payload_codec.rs"]
mod payload_codec_tests;
#[cfg(test)]
#[path = "object/tests/payload_fixtures.rs"]
mod payload_fixture_tests;
#[cfg(test)]
#[path = "object/tests/read_flags.rs"]
mod read_flags_tests;
#[cfg(test)]
#[path = "object/test_support.rs"]
mod test_support;
#[cfg(test)]
#[path = "object/tests/type_tree_policy.rs"]
mod type_tree_policy_tests;
#[cfg(test)]
#[path = "object/tests/value_model.rs"]
mod value_model_tests;
