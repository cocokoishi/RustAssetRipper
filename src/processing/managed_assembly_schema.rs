//! Assembly-backed MonoBehaviour field schemas.
//!
//! This is the Rust port of the `FieldSerializer` and the assembly-derived
//! `TypeTreeNodeStruct` shape used by `UnloadedStructure`. It reads metadata only;
//! assembly method bodies are never executed.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::managed_assembly::{
    ManagedAssemblyCatalog, ManagedPrimitiveType, ManagedTypeDefinition, ManagedTypeIdentity,
    ManagedTypeSignature,
};
use crate::processing::script_identity::ScriptIdentifier;
use crate::serialized_file::managed_types::{
    is_serializable_unity_struct, is_unity_engine_value_type,
};
use crate::serialized_file::metadata::{TypeTree, TypeTreeNode};

const ALIGN_BYTES: u32 = 0x0000_4000;
const ANY_CHILD_USES_ALIGN_BYTES: u32 = 0x0000_8000;
const FLOW_MAPPED_IN_YAML: u32 = 0x0020_0000;
const CHAR_PROPERTY_MASK: u32 = 0x0200_0000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedSchemaLimits {
    pub max_depth: usize,
    pub max_fields: usize,
    pub max_nodes: usize,
}

impl Default for ManagedSchemaLimits {
    fn default() -> Self {
        Self {
            max_depth: 256,
            max_fields: 1_000_000,
            max_nodes: 2_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedScriptSchema {
    pub type_tree: TypeTree,
    /// Fields excluded by the source `WillUnitySerialize` rules or recursive-cycle handling.
    /// Keeping this list makes intentional source-compatible omissions auditable.
    pub skipped_fields: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedSchemaErrorKind {
    MissingScriptIdentity,
    AssemblyOrTypeNotFound { identity: String },
    InvalidRootType { identity: String, reason: String },
    MissingBaseType { identity: String },
    UnsupportedSignature { field: String, signature: String },
    FixedBufferMetadata { field: String },
    UnsupportedArrayDepth { field: String, depth: usize },
    InheritanceCycle { identity: String },
    DepthLimitExceeded { identity: String, limit: usize },
    FieldLimitExceeded { count: usize, limit: usize },
    NodeLimitExceeded { count: usize, limit: usize },
    NodeLevelOverflow { level: usize },
    NodeIndexOverflow { index: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedSchemaError {
    pub identity: String,
    pub kind: ManagedSchemaErrorKind,
}

impl Display for ManagedSchemaError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "managed script schema {:?}: ", self.identity)?;
        match &self.kind {
            ManagedSchemaErrorKind::MissingScriptIdentity => {
                formatter.write_str("MonoScript has no class name")
            }
            ManagedSchemaErrorKind::AssemblyOrTypeNotFound { identity } => {
                write!(formatter, "assembly-backed type {identity:?} was not found")
            }
            ManagedSchemaErrorKind::InvalidRootType { identity, reason } => {
                write!(formatter, "root type {identity:?} is not usable: {reason}")
            }
            ManagedSchemaErrorKind::MissingBaseType { identity } => {
                write!(formatter, "base type {identity:?} could not be resolved")
            }
            ManagedSchemaErrorKind::UnsupportedSignature { field, signature } => {
                write!(
                    formatter,
                    "field {field:?} has unsupported signature {signature:?}"
                )
            }
            ManagedSchemaErrorKind::FixedBufferMetadata { field } => {
                write!(
                    formatter,
                    "fixed-buffer metadata for field {field:?} is malformed"
                )
            }
            ManagedSchemaErrorKind::UnsupportedArrayDepth { field, depth } => {
                write!(
                    formatter,
                    "field {field:?} has unsupported array depth {depth}"
                )
            }
            ManagedSchemaErrorKind::InheritanceCycle { identity } => {
                write!(
                    formatter,
                    "type inheritance contains a cycle at {identity:?}"
                )
            }
            ManagedSchemaErrorKind::DepthLimitExceeded { identity, limit } => {
                write!(formatter, "schema depth at {identity:?} exceeds {limit}")
            }
            ManagedSchemaErrorKind::FieldLimitExceeded { count, limit } => {
                write!(formatter, "expanded field count {count} exceeds {limit}")
            }
            ManagedSchemaErrorKind::NodeLimitExceeded { count, limit } => {
                write!(formatter, "TypeTree node count {count} exceeds {limit}")
            }
            ManagedSchemaErrorKind::NodeLevelOverflow { level } => {
                write!(
                    formatter,
                    "TypeTree node level {level} does not fit in a byte"
                )
            }
            ManagedSchemaErrorKind::NodeIndexOverflow { index } => {
                write!(
                    formatter,
                    "TypeTree node index {index} does not fit in a signed integer"
                )
            }
        }
    }
}

impl Error for ManagedSchemaError {}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SerializableType {
    name: String,
    kind: SerializableKind,
    fields: Vec<SerializableField>,
    contains_serialize_reference: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SerializableKind {
    Primitive(ManagedPrimitiveType),
    String,
    EnginePointer,
    ManagedReference { stable_ids: bool },
    Registry,
    ReferencedObject,
    ReferencedManagedType,
    ReferencedObjectData,
    Complex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SerializableField {
    name: String,
    type_: Box<SerializableType>,
    array_depth: usize,
    align: bool,
}

struct SchemaBuilder<'a> {
    catalog: &'a ManagedAssemblyCatalog,
    unity_version: &'a str,
    limits: ManagedSchemaLimits,
    stack: Vec<ManagedTypeIdentity>,
    field_count: usize,
    skipped_fields: Vec<String>,
    root_identity: String,
}

/// Build the same serializable field tree used to decode a MonoBehaviour's saved
/// `UnloadedStructure` bytes. The Unity version is the MonoScript collection version,
/// matching `MonoScriptExtensions.GetBehaviourType`.
pub fn build_mono_behaviour_schema(
    catalog: &ManagedAssemblyCatalog,
    script: &ScriptIdentifier,
    unity_version: &str,
    limits: ManagedSchemaLimits,
) -> Result<ManagedScriptSchema, ManagedSchemaError> {
    let Some(name) = script.name.as_deref() else {
        return Err(schema_error(
            script.unique_name(),
            ManagedSchemaErrorKind::MissingScriptIdentity,
        ));
    };
    let identity = ManagedTypeIdentity {
        assembly: script.assembly.clone(),
        namespace: script.namespace.clone(),
        name: name.to_owned(),
    };
    let identity_text = format_type_identity(&identity);
    let Some(definition) = catalog.find_type_by_identity(&identity) else {
        return Err(schema_error(
            identity_text.clone(),
            ManagedSchemaErrorKind::AssemblyOrTypeNotFound {
                identity: identity_text,
            },
        ));
    };
    if definition.is_abstract || definition.is_interface {
        return Err(schema_error(
            identity_text.clone(),
            ManagedSchemaErrorKind::InvalidRootType {
                identity: identity_text,
                reason: if definition.is_interface {
                    "type is an interface".to_owned()
                } else {
                    "type is abstract".to_owned()
                },
            },
        ));
    }
    if definition.generic_parameter_count != 0 {
        return Err(schema_error(
            identity_text.clone(),
            ManagedSchemaErrorKind::InvalidRootType {
                identity: identity_text,
                reason: "generic script types are not serializable".to_owned(),
            },
        ));
    }

    let mut builder = SchemaBuilder {
        catalog,
        unity_version,
        limits,
        stack: Vec::new(),
        field_count: 0,
        skipped_fields: Vec::new(),
        root_identity: format_type_identity(&identity),
    };
    let root = builder
        .build_definition(&identity, &[], 0)
        .map_err(|kind| schema_error(builder.root_identity.clone(), kind))?;
    let fields = mono_behaviour_structure_fields(&root.fields);
    let type_tree = build_tree_from_fields(&root, fields, limits, &builder.root_identity)?;
    Ok(ManagedScriptSchema {
        type_tree,
        skipped_fields: builder.skipped_fields,
    })
}

impl SchemaBuilder<'_> {
    fn build_definition(
        &mut self,
        identity: &ManagedTypeIdentity,
        generic_arguments: &[ManagedTypeSignature],
        depth: usize,
    ) -> Result<SerializableType, ManagedSchemaErrorKind> {
        let identity_text = format_type_identity(identity);
        if depth > self.limits.max_depth {
            return Err(ManagedSchemaErrorKind::DepthLimitExceeded {
                identity: identity_text,
                limit: self.limits.max_depth,
            });
        }
        if self.stack.contains(identity) {
            return Err(ManagedSchemaErrorKind::InheritanceCycle {
                identity: identity_text,
            });
        }
        let Some(definition) = self.catalog.find_type_by_identity(identity) else {
            if is_terminal_runtime_type(identity) {
                return Ok(empty_type(identity.name.clone(), SerializableKind::Complex));
            }
            return Err(ManagedSchemaErrorKind::MissingBaseType {
                identity: identity_text,
            });
        };
        if definition.generic_parameter_count != generic_arguments.len() {
            return Err(ManagedSchemaErrorKind::InvalidRootType {
                identity: identity_text,
                reason: format!(
                    "type declares {} generic parameters but received {}",
                    definition.generic_parameter_count,
                    generic_arguments.len()
                ),
            });
        }

        self.stack.push(identity.clone());
        let mut fields = Vec::new();
        let mut contains_reference = false;
        if let Some(base_signature) = definition.base_type_signature.as_ref() {
            let base_signature = substitute(base_signature, generic_arguments);
            if let Some((base_identity, base_arguments)) =
                named_identity_and_arguments(&base_signature)
            {
                if !is_terminal_runtime_type(&base_identity) {
                    let base_type =
                        self.build_definition(&base_identity, &base_arguments, depth + 1)?;
                    contains_reference |= base_type.contains_serialize_reference;
                    let inherited_count = if base_type.fields.last().is_some_and(|field| {
                        field.name == "references" && field.type_.kind == SerializableKind::Registry
                    }) {
                        base_type.fields.len().saturating_sub(1)
                    } else {
                        base_type.fields.len()
                    };
                    fields.try_reserve(inherited_count).map_err(|_| {
                        ManagedSchemaErrorKind::FieldLimitExceeded {
                            count: self.field_count.saturating_add(inherited_count),
                            limit: self.limits.max_fields,
                        }
                    })?;
                    fields.extend(base_type.fields.into_iter().take(inherited_count));
                }
            } else if !matches!(base_signature, ManagedTypeSignature::Unsupported(_)) {
                return Err(ManagedSchemaErrorKind::UnsupportedSignature {
                    field: format!("{identity_text} base"),
                    signature: debug_signature(&base_signature),
                });
            }
        }

        for field in &definition.fields {
            let field_label = format!("{identity_text}.{}", field.name);
            self.field_count = self.field_count.checked_add(1).ok_or(
                ManagedSchemaErrorKind::FieldLimitExceeded {
                    count: usize::MAX,
                    limit: self.limits.max_fields,
                },
            )?;
            if self.field_count > self.limits.max_fields {
                return Err(ManagedSchemaErrorKind::FieldLimitExceeded {
                    count: self.field_count,
                    limit: self.limits.max_fields,
                });
            }
            let serialize_reference =
                has_attribute(&field.attributes, "UnityEngine.SerializeReference");
            if field.is_static || field.is_literal || field.is_not_serialized || field.is_init_only
            {
                self.skipped_fields
                    .push(format!("{field_label}: excluded field flags"));
                continue;
            }
            if has_attribute(
                &field.attributes,
                "System.Runtime.CompilerServices.FixedBufferAttribute",
            ) {
                return Err(ManagedSchemaErrorKind::FixedBufferMetadata { field: field_label });
            }
            let unity_value_type =
                is_unity_engine_value_type(&definition.namespace, &definition.name);
            let has_visibility = field.is_public
                || unity_value_type
                || has_attribute(&field.attributes, "UnityEngine.SerializeField")
                || serialize_reference;
            if !has_visibility {
                self.skipped_fields.push(format!(
                    "{field_label}: non-public without Unity serialization attribute"
                ));
                continue;
            }
            if signature_uses_windows_assembly(&field.type_signature) {
                self.skipped_fields
                    .push(format!("{field_label}: Windows platform type"));
                continue;
            }

            let Some((field_type, array_depth)) = self.build_field_type(
                &field.type_signature,
                serialize_reference,
                &field_label,
                depth + 1,
            )?
            else {
                self.skipped_fields.push(format!(
                    "{field_label}: rejected by Unity field serialization rules"
                ));
                continue;
            };
            contains_reference |= field_type.contains_serialize_reference
                || matches!(field_type.kind, SerializableKind::ManagedReference { .. });
            if matches!(field_type.kind, SerializableKind::ManagedReference { .. }) {
                contains_reference = true;
            }
            fields
                .try_reserve(1)
                .map_err(|_| ManagedSchemaErrorKind::FieldLimitExceeded {
                    count: self.field_count.saturating_add(1),
                    limit: self.limits.max_fields,
                })?;
            fields.push(SerializableField {
                name: field.name.clone(),
                type_: Box::new(field_type),
                array_depth,
                align: true,
            });
        }

        let behaviour_or_scriptable =
            self.is_or_derives_from(identity, "UnityEngine", "MonoBehaviour")
                || self.is_or_derives_from(identity, "UnityEngine", "ScriptableObject");
        if contains_reference && behaviour_or_scriptable {
            fields.push(registry_field());
        }
        self.stack.pop();
        Ok(SerializableType {
            name: definition.name.clone(),
            kind: SerializableKind::Complex,
            fields,
            contains_serialize_reference: contains_reference,
        })
    }

    fn build_field_type(
        &mut self,
        signature: &ManagedTypeSignature,
        serialize_reference: bool,
        field_label: &str,
        depth: usize,
    ) -> Result<Option<(SerializableType, usize)>, ManagedSchemaErrorKind> {
        let signature = substitute(signature, &[]);
        if serialize_reference {
            let (element, array_depth) = managed_reference_element(&signature);
            if self.is_delegate(element) {
                return Ok(None);
            }
            if array_depth > 1 {
                return Err(ManagedSchemaErrorKind::UnsupportedArrayDepth {
                    field: field_label.to_owned(),
                    depth: array_depth,
                });
            }
            return Ok(Some((
                SerializableType {
                    name: "managedReference".to_owned(),
                    kind: SerializableKind::ManagedReference {
                        stable_ids: stable_reference_ids(self.unity_version),
                    },
                    fields: Vec::new(),
                    contains_serialize_reference: false,
                },
                array_depth,
            )));
        }

        if let ManagedTypeSignature::Vector(element) = &signature {
            if matches!(
                **element,
                ManagedTypeSignature::Vector(_) | ManagedTypeSignature::MultiDimensionalArray(_)
            ) {
                return Ok(None);
            }
            let Some(type_) = self.build_value_type(element, field_label, depth + 1)? else {
                return Ok(None);
            };
            return Ok(Some((type_, 1)));
        }
        if matches!(signature, ManagedTypeSignature::MultiDimensionalArray(_)) {
            return Ok(None);
        }
        if let ManagedTypeSignature::Generic {
            identity,
            arguments,
            ..
        } = &signature
        {
            if is_list(identity) {
                let Some(element) = arguments.first() else {
                    return Err(ManagedSchemaErrorKind::UnsupportedSignature {
                        field: field_label.to_owned(),
                        signature: debug_signature(&signature),
                    });
                };
                if arguments.len() != 1
                    || matches!(
                        element,
                        ManagedTypeSignature::Vector(_)
                            | ManagedTypeSignature::MultiDimensionalArray(_)
                    )
                {
                    return Ok(None);
                }
                let Some(type_) = self.build_value_type(element, field_label, depth + 1)? else {
                    return Ok(None);
                };
                return Ok(Some((type_, 1)));
            }
        }
        let Some(type_) = self.build_value_type(&signature, field_label, depth + 1)? else {
            return Ok(None);
        };
        Ok(Some((type_, 0)))
    }

    fn build_value_type(
        &mut self,
        signature: &ManagedTypeSignature,
        field_label: &str,
        depth: usize,
    ) -> Result<Option<SerializableType>, ManagedSchemaErrorKind> {
        if depth > self.limits.max_depth {
            return Err(ManagedSchemaErrorKind::DepthLimitExceeded {
                identity: field_label.to_owned(),
                limit: self.limits.max_depth,
            });
        }
        match signature {
            ManagedTypeSignature::Primitive(primitive) => {
                if primitive_serializable(*primitive, self.unity_version) {
                    Ok(Some(primitive_type(*primitive)))
                } else {
                    Ok(None)
                }
            }
            ManagedTypeSignature::String => Ok(Some(SerializableType {
                name: "String".to_owned(),
                kind: SerializableKind::String,
                fields: Vec::new(),
                contains_serialize_reference: false,
            })),
            ManagedTypeSignature::Object | ManagedTypeSignature::TypeParameter(_) => Ok(None),
            ManagedTypeSignature::Vector(_) | ManagedTypeSignature::MultiDimensionalArray(_) => {
                Ok(None)
            }
            ManagedTypeSignature::Unsupported(signature) => {
                Err(ManagedSchemaErrorKind::UnsupportedSignature {
                    field: field_label.to_owned(),
                    signature: signature.clone(),
                })
            }
            ManagedTypeSignature::Named {
                identity,
                is_value_type,
            } => self.build_named_type(identity, &[], *is_value_type, field_label, depth),
            ManagedTypeSignature::Generic {
                identity,
                arguments,
                is_value_type,
            } => {
                if is_list(identity) {
                    return Ok(None);
                }
                if is_dictionary(identity) {
                    return Ok(None);
                }
                self.build_named_type(identity, arguments, *is_value_type, field_label, depth)
            }
        }
    }

    fn build_named_type(
        &mut self,
        identity: &ManagedTypeIdentity,
        generic_arguments: &[ManagedTypeSignature],
        value_kind: Option<bool>,
        field_label: &str,
        depth: usize,
    ) -> Result<Option<SerializableType>, ManagedSchemaErrorKind> {
        let Some(definition) = self.catalog.find_type_by_identity(identity) else {
            return Ok(None);
        };
        if is_engine_object_identity(identity)
            || self.is_or_derives_from(identity, "UnityEngine", "Object")
        {
            return Ok(Some(SerializableType {
                name: "Object".to_owned(),
                kind: SerializableKind::EnginePointer,
                fields: Vec::new(),
                contains_serialize_reference: false,
            }));
        }
        if is_enum(definition) {
            let Some(underlying) = definition
                .fields
                .iter()
                .find(|field| field.name == "value__")
            else {
                return Err(ManagedSchemaErrorKind::InvalidRootType {
                    identity: format_type_identity(identity),
                    reason: "enum has no value__ field".to_owned(),
                });
            };
            let ManagedTypeSignature::Primitive(primitive) = underlying.type_signature else {
                return Err(ManagedSchemaErrorKind::InvalidRootType {
                    identity: format_type_identity(identity),
                    reason: "enum underlying type is not primitive".to_owned(),
                });
            };
            return Ok(primitive_serializable(primitive, self.unity_version)
                .then(|| primitive_type(primitive)));
        }

        let is_value_type = value_kind.unwrap_or_else(|| is_value_type_definition(definition));
        let engine_full_name = format!("{}.{}", identity.namespace, identity.name);
        let unity_engine_value_type =
            is_unity_engine_value_type(&identity.namespace, &definition.name);
        let serializable_unity_struct = is_serializable_unity_struct(&engine_full_name);
        let serializable_unity_class = is_serializable_unity_class(self, identity);
        if is_nonserialized_type(identity, definition) {
            return Ok(None);
        }
        if is_value_type {
            if !serializable_unity_struct
                && !unity_engine_value_type
                && (!definition.is_serializable || !version_at_least(self.unity_version, 4, 5))
            {
                return Ok(None);
            }
        } else if !serializable_unity_class
            && !definition.is_serializable
            && !self.is_or_derives_from(identity, "UnityEngine", "MonoBehaviour")
            && !self.is_or_derives_from(identity, "UnityEngine", "ScriptableObject")
        {
            return Ok(None);
        }
        if definition.is_abstract || definition.is_interface || is_compiler_generated(definition) {
            return Ok(None);
        }
        if definition.generic_parameter_count != generic_arguments.len() {
            return Ok(None);
        }
        if definition.generic_parameter_count != 0
            && !version_at_least(self.unity_version, 2020, 0)
            && !is_exposed_reference(identity)
        {
            return Ok(None);
        }
        if self.is_or_derives_from(identity, "System", "Delegate")
            || self.is_or_derives_from(identity, "System", "MulticastDelegate")
        {
            return Ok(None);
        }
        if self.stack.contains(identity) {
            // FieldSerializer caches recursive types before visiting their fields, then
            // drops the cyclic field when the cycle is discovered.
            self.skipped_fields.push(format!(
                "{field_label}: recursive type field omitted by source cycle rule"
            ));
            return Ok(None);
        }
        let mut built = self
            .build_definition(identity, generic_arguments, depth + 1)
            .map_err(|kind| match kind {
                ManagedSchemaErrorKind::MissingBaseType { .. } => {
                    ManagedSchemaErrorKind::InvalidRootType {
                        identity: format_type_identity(identity),
                        reason: format!(
                            "a serializable field type could not be fully resolved: {kind:?}"
                        ),
                    }
                }
                other => other,
            })?;
        built.name = definition.name.clone();
        Ok(Some(built))
    }

    fn is_or_derives_from(
        &self,
        identity: &ManagedTypeIdentity,
        namespace: &str,
        name: &str,
    ) -> bool {
        let mut current = identity.clone();
        for _ in 0..self.limits.max_depth {
            if current.namespace == namespace && current.name == name {
                return true;
            }
            let Some(definition) = self.catalog.find_type_by_identity(&current) else {
                return false;
            };
            let Some(signature) = definition.base_type_signature.as_ref() else {
                return false;
            };
            let Some((base_identity, _)) = named_identity_and_arguments(signature) else {
                return false;
            };
            current = base_identity;
        }
        false
    }

    fn is_delegate(&self, signature: &ManagedTypeSignature) -> bool {
        let Some((identity, _)) = named_identity_and_arguments(signature) else {
            return false;
        };
        self.is_or_derives_from(&identity, "System", "Delegate")
            || self.is_or_derives_from(&identity, "System", "MulticastDelegate")
    }
}

struct TreeBuilder {
    nodes: Vec<TypeTreeNode>,
    limits: ManagedSchemaLimits,
    root_identity: String,
}

impl TreeBuilder {
    fn append_type(
        &mut self,
        type_: &SerializableType,
        name: &str,
        align: bool,
        level: usize,
    ) -> Result<(), ManagedSchemaErrorKind> {
        let index = self.reserve_node(level)?;
        let children = self.children(type_)?;
        let any_child_align = children
            .iter()
            .any(|child| child.align || child.any_child_align);
        let meta_flags = (u32::from(align) * ALIGN_BYTES)
            | (u32::from(any_child_align) * ANY_CHILD_USES_ALIGN_BYTES)
            | (u32::from(matches!(
                type_.kind,
                SerializableKind::Primitive(ManagedPrimitiveType::Char)
            )) * CHAR_PROPERTY_MASK)
            | (u32::from(matches!(
                type_.kind,
                SerializableKind::ReferencedManagedType
            )) * FLOW_MAPPED_IN_YAML);
        let type_name = type_tree_type_name(type_);
        self.nodes[index] = type_tree_node(index, level, &type_name, name, 1, meta_flags)?;
        for child in children {
            self.append_child(child, level + 1)?;
        }
        Ok(())
    }

    fn append_child(
        &mut self,
        child: NodeSpec,
        level: usize,
    ) -> Result<(), ManagedSchemaErrorKind> {
        let index = self.reserve_node(level)?;
        let any_child_align = child
            .children
            .iter()
            .any(|node| node.align || node.any_child_align);
        let meta_flags = (u32::from(child.align) * ALIGN_BYTES)
            | (u32::from(any_child_align) * ANY_CHILD_USES_ALIGN_BYTES)
            | child.extra_meta_flags;
        self.nodes[index] = type_tree_node(
            index,
            level,
            &child.type_name,
            &child.name,
            child.version,
            meta_flags,
        )?;
        for nested in child.children {
            self.append_child(nested, level + 1)?;
        }
        Ok(())
    }

    fn reserve_node(&mut self, level: usize) -> Result<usize, ManagedSchemaErrorKind> {
        if self.nodes.len() >= self.limits.max_nodes {
            return Err(ManagedSchemaErrorKind::NodeLimitExceeded {
                count: self.nodes.len().saturating_add(1),
                limit: self.limits.max_nodes,
            });
        }
        if level > self.limits.max_depth {
            return Err(ManagedSchemaErrorKind::DepthLimitExceeded {
                identity: self.root_identity.clone(),
                limit: self.limits.max_depth,
            });
        }
        let level =
            u8::try_from(level).map_err(|_| ManagedSchemaErrorKind::NodeLevelOverflow { level })?;
        let index = self.nodes.len();
        let node_index = i32::try_from(index)
            .map_err(|_| ManagedSchemaErrorKind::NodeIndexOverflow { index })?;
        self.nodes
            .try_reserve(1)
            .map_err(|_| ManagedSchemaErrorKind::NodeLimitExceeded {
                count: index.saturating_add(1),
                limit: self.limits.max_nodes,
            })?;
        self.nodes.push(TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(String::new()),
            name: Some(String::new()),
            byte_size: -1,
            index: node_index,
            meta_flags: 0,
            reference_type_hash: None,
        });
        Ok(index)
    }

    fn children(&self, type_: &SerializableType) -> Result<Vec<NodeSpec>, ManagedSchemaErrorKind> {
        match type_.kind {
            SerializableKind::String => Ok(vec![NodeSpec::array_with_version(
                "Array",
                "Array",
                vec![
                    NodeSpec::leaf("SInt32", "size", 1),
                    NodeSpec::leaf("char", "data", 1),
                ],
                false,
                1,
            )]),
            SerializableKind::EnginePointer => Ok(vec![
                NodeSpec::leaf("SInt32", "m_FileID", 1),
                NodeSpec::leaf("SInt64", "m_PathID", 1),
            ]),
            SerializableKind::ManagedReference { stable_ids } => Ok(vec![NodeSpec::leaf(
                if stable_ids { "SInt64" } else { "SInt32" },
                if stable_ids { "rid" } else { "id" },
                1,
            )]),
            SerializableKind::Registry => Ok(registry_type()
                .fields
                .into_iter()
                .map(|field| field_node(&field))
                .collect()),
            SerializableKind::ReferencedObject => Ok(referenced_object_type()
                .fields
                .into_iter()
                .map(|field| field_node(&field))
                .collect()),
            SerializableKind::ReferencedManagedType => Ok(referenced_managed_type()
                .fields
                .into_iter()
                .map(|field| field_node(&field))
                .collect()),
            SerializableKind::ReferencedObjectData => Ok(Vec::new()),
            SerializableKind::Primitive(_) | SerializableKind::Complex => {
                let mut children = Vec::new();
                children
                    .try_reserve_exact(type_.fields.len())
                    .map_err(|_| ManagedSchemaErrorKind::NodeLimitExceeded {
                        count: type_.fields.len(),
                        limit: self.limits.max_nodes,
                    })?;
                for field in &type_.fields {
                    if field.array_depth > 1 {
                        return Err(ManagedSchemaErrorKind::UnsupportedArrayDepth {
                            field: field.name.clone(),
                            depth: field.array_depth,
                        });
                    }
                    children.push(field_node(field));
                }
                Ok(children)
            }
        }
    }
}

#[derive(Clone)]
struct NodeSpec {
    type_name: String,
    name: String,
    version: i32,
    align: bool,
    any_child_align: bool,
    extra_meta_flags: u32,
    children: Vec<Self>,
}

impl NodeSpec {
    fn leaf(type_name: &str, name: &str, version: i32) -> Self {
        Self {
            type_name: type_name.to_owned(),
            name: name.to_owned(),
            version,
            align: false,
            any_child_align: false,
            extra_meta_flags: 0,
            children: Vec::new(),
        }
    }

    fn array(type_name: &str, name: &str, children: Vec<Self>, align: bool) -> Self {
        Self::array_with_version(type_name, name, children, align, 0)
    }

    fn array_with_version(
        type_name: &str,
        name: &str,
        children: Vec<Self>,
        align: bool,
        version: i32,
    ) -> Self {
        Self {
            type_name: type_name.to_owned(),
            name: name.to_owned(),
            version,
            align,
            any_child_align: children
                .iter()
                .any(|child| child.align || child.any_child_align),
            extra_meta_flags: 0,
            children,
        }
    }
}

fn build_tree_from_fields(
    root: &SerializableType,
    fields: &[SerializableField],
    limits: ManagedSchemaLimits,
    identity: &str,
) -> Result<TypeTree, ManagedSchemaError> {
    let mut root = root.clone();
    root.fields = fields.to_vec();
    let mut tree_builder = TreeBuilder {
        nodes: Vec::new(),
        limits,
        root_identity: identity.to_owned(),
    };
    tree_builder
        .append_type(&root, "Base", false, 0)
        .map_err(|kind| schema_error(identity.to_owned(), kind))?;
    Ok(TypeTree {
        nodes: tree_builder.nodes,
        string_buffer: Vec::new(),
    })
}

fn type_tree_node(
    index: usize,
    level: usize,
    type_name: &str,
    name: &str,
    version: i32,
    meta_flags: u32,
) -> Result<TypeTreeNode, ManagedSchemaErrorKind> {
    Ok(TypeTreeNode {
        level: u8::try_from(level)
            .map_err(|_| ManagedSchemaErrorKind::NodeLevelOverflow { level })?,
        version,
        type_flags: 0,
        type_string_offset: None,
        name_string_offset: None,
        type_name: Some(type_name.to_owned()),
        name: Some(name.to_owned()),
        byte_size: -1,
        index: i32::try_from(index)
            .map_err(|_| ManagedSchemaErrorKind::NodeIndexOverflow { index })?,
        meta_flags,
        reference_type_hash: None,
    })
}

fn type_tree_type_name(type_: &SerializableType) -> String {
    match type_.kind {
        SerializableKind::Primitive(ManagedPrimitiveType::Boolean) => "bool".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::Char) => "UInt16".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::Int8) => "SInt8".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::UInt8) => "UInt8".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::Int16) => "SInt16".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::UInt16) => "UInt16".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::Int32) => "SInt32".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::UInt32) => "UInt32".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::Int64) => "SInt64".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::UInt64) => "UInt64".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::Float32) => "float".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::Float64) => "double".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::IntPtr) => "IntPtr".to_owned(),
        SerializableKind::Primitive(ManagedPrimitiveType::UIntPtr) => "UIntPtr".to_owned(),
        SerializableKind::String => "string".to_owned(),
        SerializableKind::EnginePointer => "PPtr<Object>".to_owned(),
        SerializableKind::ManagedReference { .. } => "managedReference".to_owned(),
        SerializableKind::Registry => "ManagedReferencesRegistry".to_owned(),
        SerializableKind::ReferencedObject => "ReferencedObject".to_owned(),
        SerializableKind::ReferencedManagedType => "ReferencedManagedType".to_owned(),
        SerializableKind::ReferencedObjectData => "ReferencedObjectData".to_owned(),
        SerializableKind::Complex => type_.name.clone(),
    }
}

fn field_node(field: &SerializableField) -> NodeSpec {
    if field.array_depth == 1 {
        let data = type_node_spec(&field.type_, "data", field.align);
        NodeSpec::array(
            "Array",
            &field.name,
            vec![NodeSpec::leaf("int", "size", 1), data],
            field.align,
        )
    } else {
        type_node_spec(&field.type_, &field.name, field.align)
    }
}

fn type_node_spec(type_: &SerializableType, name: &str, align: bool) -> NodeSpec {
    let children = match type_.kind {
        SerializableKind::String => vec![NodeSpec::array_with_version(
            "Array",
            "Array",
            vec![
                NodeSpec::leaf("SInt32", "size", 1),
                NodeSpec::leaf("char", "data", 1),
            ],
            false,
            1,
        )],
        SerializableKind::EnginePointer => vec![
            NodeSpec::leaf("SInt32", "m_FileID", 1),
            NodeSpec::leaf("SInt64", "m_PathID", 1),
        ],
        SerializableKind::ManagedReference { stable_ids } => vec![NodeSpec::leaf(
            if stable_ids { "SInt64" } else { "SInt32" },
            if stable_ids { "rid" } else { "id" },
            1,
        )],
        SerializableKind::Registry => registry_type().fields.iter().map(field_node).collect(),
        SerializableKind::ReferencedObject => referenced_object_type()
            .fields
            .iter()
            .map(field_node)
            .collect(),
        SerializableKind::ReferencedManagedType => referenced_managed_type()
            .fields
            .iter()
            .map(field_node)
            .collect(),
        SerializableKind::ReferencedObjectData => Vec::new(),
        SerializableKind::Primitive(_) | SerializableKind::Complex => {
            type_.fields.iter().map(field_node).collect()
        }
    };
    let extra_meta_flags = if matches!(type_.kind, SerializableKind::ReferencedManagedType) {
        FLOW_MAPPED_IN_YAML
    } else if matches!(
        type_.kind,
        SerializableKind::Primitive(ManagedPrimitiveType::Char)
    ) {
        CHAR_PROPERTY_MASK
    } else {
        0
    };
    NodeSpec {
        type_name: type_tree_type_name(type_),
        name: name.to_owned(),
        version: 1,
        align,
        any_child_align: children
            .iter()
            .any(|child| child.align || child.any_child_align),
        extra_meta_flags,
        children,
    }
}

fn registry_field() -> SerializableField {
    SerializableField {
        name: "references".to_owned(),
        type_: Box::new(registry_type()),
        array_depth: 0,
        align: false,
    }
}

fn registry_type() -> SerializableType {
    SerializableType {
        name: "ManagedReferencesRegistry".to_owned(),
        kind: SerializableKind::Registry,
        fields: vec![
            synthetic_field(
                "version",
                primitive_type(ManagedPrimitiveType::Int32),
                0,
                false,
            ),
            synthetic_field("RefIds", referenced_object_type(), 1, true),
        ],
        contains_serialize_reference: false,
    }
}

fn referenced_object_type() -> SerializableType {
    SerializableType {
        name: "ReferencedObject".to_owned(),
        kind: SerializableKind::ReferencedObject,
        fields: vec![
            synthetic_field("rid", primitive_type(ManagedPrimitiveType::Int64), 0, false),
            synthetic_field("type", referenced_managed_type(), 0, false),
            synthetic_field("data", referenced_object_data_type(), 0, false),
        ],
        contains_serialize_reference: false,
    }
}

fn referenced_managed_type() -> SerializableType {
    SerializableType {
        name: "ReferencedManagedType".to_owned(),
        kind: SerializableKind::ReferencedManagedType,
        fields: vec![
            synthetic_field("class", string_type(), 0, false),
            synthetic_field("ns", string_type(), 0, false),
            synthetic_field("asm", string_type(), 0, false),
        ],
        contains_serialize_reference: false,
    }
}

fn referenced_object_data_type() -> SerializableType {
    empty_type(
        "ReferencedObjectData".to_owned(),
        SerializableKind::ReferencedObjectData,
    )
}

fn synthetic_field(
    name: &str,
    type_: SerializableType,
    array_depth: usize,
    align: bool,
) -> SerializableField {
    SerializableField {
        name: name.to_owned(),
        type_: Box::new(type_),
        array_depth,
        align,
    }
}

fn string_type() -> SerializableType {
    SerializableType {
        name: "String".to_owned(),
        kind: SerializableKind::String,
        fields: Vec::new(),
        contains_serialize_reference: false,
    }
}

fn empty_type(name: String, kind: SerializableKind) -> SerializableType {
    SerializableType {
        name,
        kind,
        fields: Vec::new(),
        contains_serialize_reference: false,
    }
}

fn mono_behaviour_structure_fields(fields: &[SerializableField]) -> &[SerializableField] {
    let name_index = fields.iter().position(|field| field.name == "m_Name");
    let editor_identifier_index = fields
        .iter()
        .position(|field| field.name == "m_EditorClassIdentifier");
    let header_end = name_index
        .into_iter()
        .chain(editor_identifier_index)
        .max()
        .map_or(0, |index| index.saturating_add(1));
    fields.get(header_end..).unwrap_or_default()
}

fn primitive_serializable(primitive: ManagedPrimitiveType, version: &str) -> bool {
    match primitive {
        ManagedPrimitiveType::Boolean
        | ManagedPrimitiveType::UInt8
        | ManagedPrimitiveType::Int32
        | ManagedPrimitiveType::Float32
        | ManagedPrimitiveType::Float64 => true,
        ManagedPrimitiveType::Int8
        | ManagedPrimitiveType::Int16
        | ManagedPrimitiveType::UInt16
        | ManagedPrimitiveType::UInt32 => version_at_least(version, 5, 0),
        ManagedPrimitiveType::Int64 | ManagedPrimitiveType::UInt64 | ManagedPrimitiveType::Char => {
            version_at_least(version, 2017, 0)
        }
        ManagedPrimitiveType::IntPtr | ManagedPrimitiveType::UIntPtr => false,
    }
}

fn primitive_type(primitive: ManagedPrimitiveType) -> SerializableType {
    SerializableType {
        name: format!("{primitive:?}"),
        kind: SerializableKind::Primitive(primitive),
        fields: Vec::new(),
        contains_serialize_reference: false,
    }
}

fn stable_reference_ids(version: &str) -> bool {
    let Some((major, minor, patch, channel, number)) = parse_version(version) else {
        return false;
    };
    (major, minor, patch) > (2021, 2, 0)
        || ((major, minor, patch) == (2021, 2, 0)
            && (channel == 'f' || (channel == 'a' && number >= 19)))
}

fn version_at_least(version: &str, major: u32, minor: u32) -> bool {
    parse_version(version).is_some_and(|(version_major, version_minor, _, _, _)| {
        (version_major, version_minor) >= (major, minor)
    })
}

fn parse_version(version: &str) -> Option<(u32, u32, u32, char, u32)> {
    let bytes = version.as_bytes();
    let mut index = 0;
    let major = parse_u32(bytes, &mut index)?;
    if bytes.get(index) != Some(&b'.') {
        return None;
    }
    index += 1;
    let minor = parse_u32(bytes, &mut index)?;
    if bytes.get(index) != Some(&b'.') {
        return None;
    }
    index += 1;
    let patch = parse_u32(bytes, &mut index)?;
    let channel = *bytes.get(index)? as char;
    index += 1;
    let number = if index < bytes.len() {
        parse_u32(bytes, &mut index)?
    } else {
        0
    };
    Some((major, minor, patch, channel, number))
}

fn parse_u32(bytes: &[u8], index: &mut usize) -> Option<u32> {
    let start = *index;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index += 1;
    }
    if *index == start {
        return None;
    }
    std::str::from_utf8(&bytes[start..*index])
        .ok()?
        .parse()
        .ok()
}

fn substitute(
    signature: &ManagedTypeSignature,
    arguments: &[ManagedTypeSignature],
) -> ManagedTypeSignature {
    match signature {
        ManagedTypeSignature::TypeParameter(index) => arguments
            .get(*index)
            .cloned()
            .unwrap_or_else(|| signature.clone()),
        ManagedTypeSignature::Vector(element) => {
            ManagedTypeSignature::Vector(Box::new(substitute(element, arguments)))
        }
        ManagedTypeSignature::MultiDimensionalArray(element) => {
            ManagedTypeSignature::MultiDimensionalArray(Box::new(substitute(element, arguments)))
        }
        ManagedTypeSignature::Generic {
            identity,
            arguments: nested,
            is_value_type,
        } => ManagedTypeSignature::Generic {
            identity: identity.clone(),
            arguments: nested
                .iter()
                .map(|argument| substitute(argument, arguments))
                .collect(),
            is_value_type: *is_value_type,
        },
        other => other.clone(),
    }
}

fn named_identity_and_arguments(
    signature: &ManagedTypeSignature,
) -> Option<(ManagedTypeIdentity, Vec<ManagedTypeSignature>)> {
    match signature {
        ManagedTypeSignature::Named { identity, .. } => Some((identity.clone(), Vec::new())),
        ManagedTypeSignature::Generic {
            identity,
            arguments,
            ..
        } => Some((identity.clone(), arguments.clone())),
        _ => None,
    }
}

fn managed_reference_element(signature: &ManagedTypeSignature) -> (&ManagedTypeSignature, usize) {
    let mut current = signature;
    let mut depth = 0usize;
    loop {
        match current {
            ManagedTypeSignature::Vector(element) => {
                depth = depth.saturating_add(1);
                current = element;
            }
            ManagedTypeSignature::Generic {
                identity,
                arguments,
                ..
            } if is_list(identity) && arguments.len() == 1 => {
                depth = depth.saturating_add(1);
                current = &arguments[0];
            }
            _ => return (current, depth),
        }
    }
}

fn signature_uses_windows_assembly(signature: &ManagedTypeSignature) -> bool {
    match signature {
        ManagedTypeSignature::Named { identity, .. }
        | ManagedTypeSignature::Generic { identity, .. } => identity.assembly == "Windows",
        ManagedTypeSignature::Vector(element)
        | ManagedTypeSignature::MultiDimensionalArray(element) => {
            signature_uses_windows_assembly(element)
        }
        _ => false,
    }
}

fn has_attribute(attributes: &[String], suffix: &str) -> bool {
    attributes.iter().any(|attribute| attribute == suffix)
}

fn is_list(identity: &ManagedTypeIdentity) -> bool {
    identity.namespace == "System.Collections.Generic" && identity.name == "List`1"
}

fn is_dictionary(identity: &ManagedTypeIdentity) -> bool {
    identity.namespace == "System.Collections.Generic" && identity.name.starts_with("Dictionary`2")
}

fn is_exposed_reference(identity: &ManagedTypeIdentity) -> bool {
    identity.namespace == "UnityEngine" && identity.name == "ExposedReference`1"
}

fn is_engine_object_identity(identity: &ManagedTypeIdentity) -> bool {
    identity.namespace == "UnityEngine" && identity.name == "Object"
}

fn is_serializable_unity_class(
    builder: &SchemaBuilder<'_>,
    identity: &ManagedTypeIdentity,
) -> bool {
    ["AnimationCurve", "Gradient", "GUIStyle", "RectOffset"]
        .iter()
        .any(|name| builder.is_or_derives_from(identity, "UnityEngine", name))
}

fn is_nonserialized_type(
    identity: &ManagedTypeIdentity,
    definition: &ManagedTypeDefinition,
) -> bool {
    identity.namespace == "System"
        || identity.namespace.starts_with("System.")
        || identity.namespace == "UnityEngine"
            && matches!(identity.name.as_str(), "MonoBehaviour" | "ScriptableObject")
        || is_enum(definition)
}

fn is_enum(definition: &ManagedTypeDefinition) -> bool {
    definition
        .base_type_signature
        .as_ref()
        .and_then(named_identity_and_arguments)
        .is_some_and(|(identity, _)| identity.namespace == "System" && identity.name == "Enum")
}

fn is_value_type_definition(definition: &ManagedTypeDefinition) -> bool {
    definition
        .base_type_signature
        .as_ref()
        .and_then(named_identity_and_arguments)
        .is_some_and(|(identity, _)| {
            identity.namespace == "System" && matches!(identity.name.as_str(), "ValueType" | "Enum")
        })
}

fn is_compiler_generated(definition: &ManagedTypeDefinition) -> bool {
    has_attribute(
        &definition.attributes,
        "System.Runtime.CompilerServices.CompilerGeneratedAttribute",
    )
}

fn is_terminal_runtime_type(identity: &ManagedTypeIdentity) -> bool {
    identity.namespace == "System"
        && matches!(identity.name.as_str(), "Object" | "ValueType" | "Enum")
}

fn format_type_identity(identity: &ManagedTypeIdentity) -> String {
    if identity.namespace.is_empty() {
        format!("{}, {}", identity.name, identity.assembly)
    } else {
        format!(
            "{}.{}, {}",
            identity.namespace, identity.name, identity.assembly
        )
    }
}

fn debug_signature(signature: &ManagedTypeSignature) -> String {
    match signature {
        ManagedTypeSignature::Primitive(primitive) => format!("{primitive:?}"),
        ManagedTypeSignature::String => "System.String".to_owned(),
        ManagedTypeSignature::Object => "System.Object".to_owned(),
        ManagedTypeSignature::Named { identity, .. } => format_type_identity(identity),
        ManagedTypeSignature::Generic {
            identity,
            arguments,
            ..
        } => format!(
            "{}<{:?}>",
            format_type_identity(identity),
            arguments.iter().map(debug_signature).collect::<Vec<_>>()
        ),
        ManagedTypeSignature::Vector(element) => format!("{}[]", debug_signature(element)),
        ManagedTypeSignature::MultiDimensionalArray(element) => {
            format!("{}[*]", debug_signature(element))
        }
        ManagedTypeSignature::TypeParameter(index) => format!("T{index}"),
        ManagedTypeSignature::Unsupported(value) => value.clone(),
    }
}

fn schema_error(identity: String, kind: ManagedSchemaErrorKind) -> ManagedSchemaError {
    ManagedSchemaError { identity, kind }
}

#[cfg(test)]
mod tests {
    use super::{build_mono_behaviour_schema, ManagedSchemaLimits};
    use crate::processing::managed_assembly::{ManagedAssemblyCatalog, ManagedAssemblyLimits};
    use crate::processing::script_identity::ScriptIdentifier;

    const ASSEMBLY_BYTES: &[u8] = include_bytes!("managed_assembly_fixture.dll");
    const MANAGED_ASSEMBLY_FIXTURE: &str = include_str!("managed_assembly_fixture.txt");

    #[test]
    fn builds_assembly_schema_with_inherited_and_attributed_fields() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();
        let script =
            ScriptIdentifier::try_new("Assembly-CSharp", "Game", "HeroController").unwrap();
        let schema = build_mono_behaviour_schema(
            &catalog,
            &script,
            "2019.4.40f1",
            ManagedSchemaLimits::default(),
        )
        .unwrap();
        let top_level: Vec<_> = schema
            .type_tree
            .nodes
            .iter()
            .filter(|node| node.level == 1)
            .filter_map(|node| node.name.as_deref())
            .collect();
        assert_eq!(top_level, ["inherited", "health", "speed"]);
        assert!(schema
            .skipped_fields
            .iter()
            .any(|field| field.contains("ignored")));
        assert!(schema
            .skipped_fields
            .iter()
            .any(|field| field.contains("staticValue")));
        assert!(schema
            .skipped_fields
            .iter()
            .any(|field| field.contains("locked")));

        let source_nodes = MANAGED_ASSEMBLY_FIXTURE
            .lines()
            .filter_map(|line| line.strip_prefix("SCHEMA_NODE|"))
            .map(|line| {
                let parts: Vec<_> = line.split('|').collect();
                assert_eq!(parts.len(), 5, "source schema row {line:?}");
                (
                    parts[0].parse::<u8>().expect("source node level"),
                    parts[1].to_owned(),
                    parts[2].to_owned(),
                    parts[3].parse::<i32>().expect("source node version"),
                    parts[4].parse::<u32>().expect("source node meta flags"),
                )
            })
            .collect::<Vec<_>>();
        let rust_nodes = schema
            .type_tree
            .nodes
            .iter()
            .map(|node| {
                (
                    node.level,
                    node.type_name.clone().expect("schema node type"),
                    node.name.clone().expect("schema node name"),
                    node.version,
                    node.meta_flags,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rust_nodes, source_nodes,
            "Rust schema matches FieldSerializer"
        );
    }

    #[test]
    fn builds_source_schema_for_nested_and_collection_fields() {
        const ASSEMBLY_BYTES: &[u8] = include_bytes!("managed_assembly_schema_complex_fixture.dll");
        const SOURCE_FIXTURE: &str = include_str!("managed_assembly_schema_complex_fixture.txt");

        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "ComplexFixtureScripts.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();
        let script =
            ScriptIdentifier::try_new("ComplexFixtureScripts", "Game", "RichController").unwrap();
        let schema = build_mono_behaviour_schema(
            &catalog,
            &script,
            "2019.4.40f1",
            ManagedSchemaLimits::default(),
        )
        .unwrap();

        let source_nodes = SOURCE_FIXTURE
            .lines()
            .filter_map(|line| line.strip_prefix("SCHEMA_NODE|"))
            .map(|line| {
                let parts: Vec<_> = line.split('|').collect();
                assert_eq!(parts.len(), 5, "source schema row {line:?}");
                (
                    parts[0].parse::<u8>().expect("source node level"),
                    parts[1].to_owned(),
                    parts[2].to_owned(),
                    parts[3].parse::<i32>().expect("source node version"),
                    parts[4].parse::<u32>().expect("source node meta flags"),
                )
            })
            .collect::<Vec<_>>();
        let rust_nodes = schema
            .type_tree
            .nodes
            .iter()
            .map(|node| {
                (
                    node.level,
                    node.type_name.clone().expect("schema node type"),
                    node.name.clone().expect("schema node name"),
                    node.version,
                    node.meta_flags,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rust_nodes, source_nodes,
            "Rust schema matches FieldSerializer"
        );
        assert!(schema
            .skipped_fields
            .iter()
            .any(|field| field.contains("ignored")));
        assert!(schema
            .skipped_fields
            .iter()
            .any(|field| field.contains("staticValue")));
        assert!(schema
            .skipped_fields
            .iter()
            .any(|field| field.contains("locked")));
        assert!(rust_nodes.iter().any(|node| node.2 == "samples"));
        assert!(rust_nodes.iter().any(|node| node.2 == "weights"));
        assert!(rust_nodes.iter().any(|node| node.2 == "stats"));
        assert!(rust_nodes.iter().any(|node| node.1 == "PPtr<Object>"));
        assert!(rust_nodes
            .iter()
            .any(|node| node.2 == "phase" && node.1 == "UInt8"));
    }
}
