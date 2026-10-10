mod writer;
pub use writer::{
    default_value_for_type_tree_node, ObjectDefaultValueLimits, ObjectEncodeError,
    ObjectEncodeErrorKind, ObjectEncodeLimits,
};

include!("object_models_and_layout.rs");
include!("object_payload_decoder.rs");

#[cfg(test)]
mod tests {
    include!("object_tests_01.rs");
    include!("object_tests_02.rs");
}
