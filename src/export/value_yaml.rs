mod conversion;
mod field_rules;
mod layout;
mod managed_reference_hints;
mod model;
mod object_document;
mod paths;
mod type_rules;

pub(crate) use managed_reference_hints::add_managed_reference_pointer_target_hints;
pub use model::{
    UnityYamlDictionaryRepresentation, UnityYamlEmptyArrayKind, UnityYamlGuidSchema,
    UnityYamlHash128Schema, UnityYamlObjectConversionError, UnityYamlObjectConversionHints,
    UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode, UnityYamlPPtrRequest,
    UnityYamlPropertyNameSchema, UnityYamlPropertyNameValueKind,
    UnityYamlSceneObjectIdentifierSchema,
};
pub use object_document::serialized_object_value_to_unity_yaml_document;

#[cfg(test)]
mod tests;
