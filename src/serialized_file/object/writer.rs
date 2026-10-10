mod default_value;
mod encode;
mod error;

pub use default_value::{default_value_for_type_tree_node, ObjectDefaultValueLimits};
pub use encode::ObjectEncodeLimits;
pub use error::{ObjectEncodeError, ObjectEncodeErrorKind};

use super::ObjectDecodeError;

fn child_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else if child.is_empty() {
        parent.to_owned()
    } else {
        format!("{parent}.{child}")
    }
}

fn invalid_tree(object_index: usize, path: &str, error: ObjectDecodeError) -> ObjectEncodeError {
    ObjectEncodeError::new(
        object_index,
        path,
        ObjectEncodeErrorKind::InvalidTypeTree {
            message: error.to_string(),
        },
    )
}

#[cfg(test)]
use super::{SerializedValue, TypeTreeLayout};
#[cfg(test)]
use encode::PayloadEncoder;

#[cfg(test)]
mod tests;
