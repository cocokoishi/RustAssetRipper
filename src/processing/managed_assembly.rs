//! Rust-side managed assembly metadata used by the source `BaseManager`/`MonoManager` path.
//!
//! This module only reads CLR metadata. It does not execute a .NET runtime or call AssetRipper.
//! Parsed type and field facts are owned so assemblies can be indexed and queried after the
//! bounded input byte slice has been released.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use dotnetdll::prelude::{
    BaseType, MemberType, MethodReferenceParent, MethodType, ReadOptions, Resolution,
    ResolutionScope, ResolvedDebug, TypeSource, UserMethod, UserType, ValueKind,
};

use crate::io::dependency::DependencyFilePath;
use crate::io::filesystem::VirtualFileSystem;
use crate::io::special_names::fix_assembly_name;
use crate::processing::script_identity::is_mono_assembly;
use crate::processing::script_identity::ScriptIdentifier;
use crate::serialized_file::object::{ParsedObjectPayload, SerializedValue};
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedAssemblyLimits {
    pub max_file_bytes: usize,
    pub max_types: usize,
    pub max_fields: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedAssemblyCatalogLimits {
    pub max_assemblies: usize,
    pub max_total_file_bytes: u64,
    pub assembly: ManagedAssemblyLimits,
}

impl Default for ManagedAssemblyCatalogLimits {
    fn default() -> Self {
        Self {
            max_assemblies: 100_000,
            max_total_file_bytes: 4 * 1024 * 1024 * 1024,
            assembly: ManagedAssemblyLimits::default(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManagedAssemblyLoadReport {
    pub candidate_count: usize,
    pub loaded_count: usize,
    pub skipped_non_cli: Vec<String>,
    pub processed_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedScriptResolutionReport {
    pub scripts: Vec<ManagedScriptResolution>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedScriptResolution {
    pub file_index: usize,
    pub object_index: usize,
    pub path_id: i64,
    pub script: Option<ScriptIdentifier>,
    pub outcome: ManagedScriptResolutionOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedScriptResolutionOutcome {
    Resolved { is_valid: bool },
    AssemblyNotLoaded,
    TypeNotFound,
    MetadataUnavailable(String),
}

impl Default for ManagedAssemblyLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 256 * 1024 * 1024,
            max_types: 1_000_000,
            max_fields: 8_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedFieldDefinition {
    pub name: String,
    /// CLR signature rendered from the parsed metadata, retaining generic and array shape.
    pub signature: String,
    /// The same signature retained as a bounded, owned type expression for schema construction.
    pub type_signature: ManagedTypeSignature,
    pub is_public: bool,
    pub is_static: bool,
    pub is_literal: bool,
    pub is_not_serialized: bool,
    pub is_init_only: bool,
    pub attributes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedTypeDefinition {
    pub namespace: String,
    pub name: String,
    pub full_name: String,
    pub is_abstract: bool,
    pub is_interface: bool,
    pub is_serializable: bool,
    pub generic_parameter_count: usize,
    pub base_type: Option<String>,
    pub base_type_signature: Option<ManagedTypeSignature>,
    pub attributes: Vec<String>,
    /// Metadata table order, matching the source module's first-match type scan.
    pub fields: Vec<ManagedFieldDefinition>,
}

/// An assembly-qualified CLR type identity used while reconstructing Unity script fields.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ManagedTypeIdentity {
    pub assembly: String,
    pub namespace: String,
    /// Nested type names use the CLR metadata separator (`Outer+Inner`).
    pub name: String,
}

/// Owned subset of ECMA-335 member signatures that `FieldSerializer` consumes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedTypeSignature {
    Primitive(ManagedPrimitiveType),
    String,
    Object,
    Named {
        identity: ManagedTypeIdentity,
        is_value_type: Option<bool>,
    },
    Generic {
        identity: ManagedTypeIdentity,
        arguments: Vec<Self>,
        is_value_type: Option<bool>,
    },
    Vector(Box<Self>),
    MultiDimensionalArray(Box<Self>),
    TypeParameter(usize),
    Unsupported(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagedPrimitiveType {
    Boolean,
    Char,
    Int8,
    UInt8,
    Int16,
    UInt16,
    Int32,
    UInt32,
    Int64,
    UInt64,
    Float32,
    Float64,
    IntPtr,
    UIntPtr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedAssembly {
    pub file_name: String,
    pub assembly_name: String,
    /// TypeDef table order is observable when duplicate namespace/name pairs occur.
    pub types: Vec<ManagedTypeDefinition>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManagedAssemblyCatalog {
    assemblies: Vec<ManagedAssembly>,
    /// `BaseManager` registers both the path-derived name and the manifest name.
    aliases: BTreeMap<String, usize>,
}

fn managed_type_signature(
    signature: &MemberType,
    resolution: &Resolution<'_>,
    current_assembly: &str,
) -> ManagedTypeSignature {
    match signature {
        MemberType::TypeGeneric(index) => ManagedTypeSignature::TypeParameter(*index),
        MemberType::Base(base) => match &**base {
            BaseType::Boolean => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Boolean),
            BaseType::Char => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Char),
            BaseType::Int8 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Int8),
            BaseType::UInt8 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::UInt8),
            BaseType::Int16 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Int16),
            BaseType::UInt16 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::UInt16),
            BaseType::Int32 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Int32),
            BaseType::UInt32 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::UInt32),
            BaseType::Int64 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Int64),
            BaseType::UInt64 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::UInt64),
            BaseType::Float32 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Float32),
            BaseType::Float64 => ManagedTypeSignature::Primitive(ManagedPrimitiveType::Float64),
            BaseType::IntPtr => ManagedTypeSignature::Primitive(ManagedPrimitiveType::IntPtr),
            BaseType::UIntPtr => ManagedTypeSignature::Primitive(ManagedPrimitiveType::UIntPtr),
            BaseType::String => ManagedTypeSignature::String,
            BaseType::Object => ManagedTypeSignature::Object,
            BaseType::Type { value_kind, source } => {
                managed_type_source_signature(source, resolution, current_assembly)
                    .with_value_kind(*value_kind)
            }
            BaseType::Vector(_, element) => ManagedTypeSignature::Vector(Box::new(
                managed_type_signature(element, resolution, current_assembly),
            )),
            BaseType::Array(element, _) => ManagedTypeSignature::MultiDimensionalArray(Box::new(
                managed_type_signature(element, resolution, current_assembly),
            )),
            BaseType::ValuePointer(_, _) | BaseType::FunctionPointer(_) => {
                ManagedTypeSignature::Unsupported(signature.show(resolution))
            }
        },
    }
}

fn managed_type_source_signature(
    source: &TypeSource<MemberType>,
    resolution: &Resolution<'_>,
    current_assembly: &str,
) -> ManagedTypeSignature {
    match source {
        TypeSource::User(user) => managed_type_identity(user, resolution, current_assembly)
            .map(|identity| ManagedTypeSignature::Named {
                identity,
                is_value_type: None,
            })
            .unwrap_or_else(|| ManagedTypeSignature::Unsupported(source.show(resolution))),
        TypeSource::Generic { base, parameters } => {
            let Some(identity) = managed_type_identity(base, resolution, current_assembly) else {
                return ManagedTypeSignature::Unsupported(source.show(resolution));
            };
            ManagedTypeSignature::Generic {
                identity,
                arguments: parameters
                    .iter()
                    .map(|parameter| {
                        managed_type_signature(parameter, resolution, current_assembly)
                    })
                    .collect(),
                is_value_type: None,
            }
        }
    }
}

impl ManagedTypeSignature {
    fn with_value_kind(self, value_kind: Option<ValueKind>) -> Self {
        match self {
            Self::Named { identity, .. } => Self::Named {
                identity,
                is_value_type: value_kind.map(|kind| kind == ValueKind::ValueType),
            },
            Self::Generic {
                identity,
                arguments,
                ..
            } => Self::Generic {
                identity,
                arguments,
                is_value_type: value_kind.map(|kind| kind == ValueKind::ValueType),
            },
            other => other,
        }
    }
}

fn managed_type_identity(
    user_type: &UserType,
    resolution: &Resolution<'_>,
    current_assembly: &str,
) -> Option<ManagedTypeIdentity> {
    match user_type {
        UserType::Definition(index) => {
            let definition = &resolution[*index];
            Some(ManagedTypeIdentity {
                assembly: current_assembly.to_owned(),
                namespace: definition
                    .namespace
                    .as_deref()
                    .unwrap_or_default()
                    .to_owned(),
                name: definition_full_name(*index, resolution, 0)?,
            })
        }
        UserType::Reference(index) => {
            managed_type_reference_identity(*index, resolution, current_assembly, 0)
        }
    }
}

fn definition_full_name(
    type_index: dotnetdll::prelude::TypeIndex,
    resolution: &Resolution<'_>,
    depth: usize,
) -> Option<String> {
    if depth >= 64 {
        return None;
    }
    let definition = &resolution[type_index];
    match definition.encloser {
        Some(parent) => Some(format!(
            "{}+{}",
            definition_full_name(parent, resolution, depth + 1)?,
            definition.name
        )),
        None => Some(definition.name.to_string()),
    }
}

fn managed_type_reference_identity(
    type_reference: dotnetdll::prelude::TypeRefIndex,
    resolution: &Resolution<'_>,
    current_assembly: &str,
    depth: usize,
) -> Option<ManagedTypeIdentity> {
    if depth >= 64 {
        return None;
    }
    let reference = &resolution[type_reference];
    let (assembly, namespace, parent_name) = match reference.scope {
        ResolutionScope::Nested(parent) => {
            let parent =
                managed_type_reference_identity(parent, resolution, current_assembly, depth + 1)?;
            (parent.assembly, parent.namespace, Some(parent.name))
        }
        ResolutionScope::Assembly(index) => (
            resolution[index].name.to_string(),
            reference
                .namespace
                .as_deref()
                .unwrap_or_default()
                .to_owned(),
            None,
        ),
        ResolutionScope::CurrentModule | ResolutionScope::ExternalModule(_) => (
            current_assembly.to_owned(),
            reference
                .namespace
                .as_deref()
                .unwrap_or_default()
                .to_owned(),
            None,
        ),
        ResolutionScope::Exported => {
            let assembly = resolution
                .exported_types
                .iter()
                .find(|exported| {
                    exported.name == reference.name && exported.namespace == reference.namespace
                })
                .and_then(|exported| match exported.implementation {
                    dotnetdll::prelude::TypeImplementation::TypeForwarder(index) => {
                        Some(resolution[index].name.to_string())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| current_assembly.to_owned());
            (
                assembly,
                reference
                    .namespace
                    .as_deref()
                    .unwrap_or_default()
                    .to_owned(),
                None,
            )
        }
    };
    let name = match parent_name {
        Some(parent) => format!("{parent}+{}", reference.name),
        None => reference.name.to_string(),
    };
    Some(ManagedTypeIdentity {
        assembly,
        namespace,
        name,
    })
}

impl ManagedAssembly {
    pub fn parse(
        file_name: &str,
        bytes: &[u8],
        limits: ManagedAssemblyLimits,
    ) -> Result<Self, Box<ManagedAssemblyError>> {
        if bytes.len() > limits.max_file_bytes {
            return Err(managed_assembly_error(
                file_name,
                ManagedAssemblyErrorKind::FileSizeLimitExceeded {
                    file_bytes: bytes.len(),
                    limit: limits.max_file_bytes,
                },
            ));
        }

        // AssetRipper uses AsmResolver for metadata/type inspection. Method bodies are not needed
        // to identify a MonoScript type or inspect its serialized fields, so leave those bytes
        // uninterpreted and avoid the cost of decoding them here.
        let resolution = Resolution::parse(
            bytes,
            ReadOptions {
                skip_method_bodies: true,
                ..ReadOptions::default()
            },
        )
        .map_err(|error| {
            managed_assembly_error(
                file_name,
                ManagedAssemblyErrorKind::MetadataParse(error.to_string()),
            )
        })?;

        if resolution.type_definitions.len() > limits.max_types {
            return Err(managed_assembly_error(
                file_name,
                ManagedAssemblyErrorKind::TypeCountLimitExceeded {
                    count: resolution.type_definitions.len(),
                    limit: limits.max_types,
                },
            ));
        }
        let field_count = resolution
            .type_definitions
            .iter()
            .try_fold(0usize, |count, definition| {
                count.checked_add(definition.fields.len())
            })
            .ok_or_else(|| {
                managed_assembly_error(file_name, ManagedAssemblyErrorKind::FieldCountOverflow)
            })?;
        if field_count > limits.max_fields {
            return Err(managed_assembly_error(
                file_name,
                ManagedAssemblyErrorKind::FieldCountLimitExceeded {
                    count: field_count,
                    limit: limits.max_fields,
                },
            ));
        }

        let assembly_name = resolution
            .assembly
            .as_ref()
            .map(|assembly| assembly.name.to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| file_stem(file_name));

        let mut types = Vec::with_capacity(resolution.type_definitions.len());
        for (definition_index, definition) in resolution.enumerate_type_definitions() {
            let fields = definition
                .fields
                .iter()
                .map(|field| ManagedFieldDefinition {
                    name: field.name.to_string(),
                    signature: field.return_type.show(&resolution),
                    type_signature: managed_type_signature(
                        &field.return_type,
                        &resolution,
                        &assembly_name,
                    ),
                    is_public: field.accessibility.to_string() == "public",
                    is_static: field.static_member,
                    is_literal: field.literal,
                    is_not_serialized: field.not_serialized,
                    is_init_only: field.init_only,
                    attributes: field
                        .attributes
                        .iter()
                        .filter_map(|attribute| {
                            attribute_type_name(attribute.constructor, &resolution)
                        })
                        .collect(),
                })
                .collect();
            types.push(ManagedTypeDefinition {
                namespace: definition
                    .namespace
                    .as_deref()
                    .unwrap_or_default()
                    .to_owned(),
                name: definition.name.to_string(),
                full_name: definition_full_name(definition_index, &resolution, 0)
                    .unwrap_or_else(|| definition.name.to_string()),
                is_abstract: definition.flags.abstract_type,
                is_interface: matches!(definition.flags.kind, dotnetdll::prelude::Kind::Interface),
                is_serializable: definition.flags.serializable,
                generic_parameter_count: definition.generic_parameters.len(),
                base_type: definition
                    .extends
                    .as_ref()
                    .map(|base_type| base_type.show(&resolution)),
                base_type_signature: definition.extends.as_ref().map(|base_type| {
                    managed_type_source_signature(base_type, &resolution, &assembly_name)
                }),
                attributes: definition
                    .attributes
                    .iter()
                    .filter_map(|attribute| attribute_type_name(attribute.constructor, &resolution))
                    .collect(),
                fields,
            });
        }

        Ok(Self {
            file_name: file_name.to_owned(),
            assembly_name,
            types,
        })
    }

    pub fn find_type(&self, namespace: &str, name: &str) -> Option<&ManagedTypeDefinition> {
        self.types.iter().find(|candidate| {
            candidate.namespace == namespace
                && (candidate.name == name || candidate.full_name == name)
        })
    }
}

impl ManagedAssemblyCatalog {
    pub fn assemblies(&self) -> &[ManagedAssembly] {
        &self.assemblies
    }

    /// Read the source MonoManager assembly paths from the mounted VFS. Native and malformed PE
    /// files without a CLR directory are skipped as `MonoManager.TryLoad` does; a file that
    /// advertises CLR metadata but cannot be parsed remains an initialization error.
    pub fn load_from_vfs(
        filesystem: &VirtualFileSystem,
        paths: &[DependencyFilePath],
        limits: ManagedAssemblyCatalogLimits,
    ) -> Result<(Self, ManagedAssemblyLoadReport), Box<ManagedAssemblyLoadError>> {
        let mut report = ManagedAssemblyLoadReport {
            candidate_count: paths.len(),
            ..ManagedAssemblyLoadReport::default()
        };
        if paths.len() > limits.max_assemblies {
            return Err(managed_assembly_load_error(
                None,
                ManagedAssemblyLoadErrorKind::AssemblyCountLimitExceeded {
                    count: paths.len(),
                    limit: limits.max_assemblies,
                },
            ));
        }

        let mut catalog = Self::default();
        for path in paths {
            if !is_mono_assembly(&path.file_name) {
                continue;
            }
            let stream = filesystem.open_file(&path.virtual_path).map_err(|source| {
                managed_assembly_load_error(
                    Some(path.virtual_path.clone()),
                    ManagedAssemblyLoadErrorKind::FileSystem(source),
                )
            })?;
            let file_bytes = stream.length().map_err(|source| {
                managed_assembly_load_error(
                    Some(path.virtual_path.clone()),
                    ManagedAssemblyLoadErrorKind::FileSystem(source),
                )
            })?;
            report.processed_bytes =
                report
                    .processed_bytes
                    .checked_add(file_bytes)
                    .ok_or_else(|| {
                        managed_assembly_load_error(
                            Some(path.virtual_path.clone()),
                            ManagedAssemblyLoadErrorKind::TotalFileByteCountOverflow,
                        )
                    })?;
            if report.processed_bytes > limits.max_total_file_bytes {
                return Err(managed_assembly_load_error(
                    Some(path.virtual_path.clone()),
                    ManagedAssemblyLoadErrorKind::TotalFileBytesLimitExceeded {
                        byte_count: report.processed_bytes,
                        limit: limits.max_total_file_bytes,
                    },
                ));
            }
            if file_bytes > u64::try_from(limits.assembly.max_file_bytes).unwrap_or(u64::MAX) {
                return Err(managed_assembly_load_error(
                    Some(path.virtual_path.clone()),
                    ManagedAssemblyLoadErrorKind::FileSizeLimitExceeded {
                        file_bytes,
                        limit: limits.assembly.max_file_bytes,
                    },
                ));
            }
            let maximum_bytes = usize::try_from(file_bytes).map_err(|_| {
                managed_assembly_load_error(
                    Some(path.virtual_path.clone()),
                    ManagedAssemblyLoadErrorKind::FileSizeLimitExceeded {
                        file_bytes,
                        limit: limits.assembly.max_file_bytes,
                    },
                )
            })?;
            let bytes = filesystem
                .read_all_bytes(&path.virtual_path, maximum_bytes)
                .map_err(|source| {
                    managed_assembly_load_error(
                        Some(path.virtual_path.clone()),
                        ManagedAssemblyLoadErrorKind::FileSystem(source),
                    )
                })?;
            if !has_cli_directory(&bytes) {
                report.skipped_non_cli.push(path.file_name.clone());
                continue;
            }
            catalog
                .insert(&path.file_name, &bytes, limits.assembly)
                .map_err(|source| {
                    managed_assembly_load_error(
                        Some(path.virtual_path.clone()),
                        ManagedAssemblyLoadErrorKind::Assembly(*source),
                    )
                })?;
            report.loaded_count += 1;
        }
        Ok((catalog, report))
    }

    pub fn contains_assembly(&self, assembly: &str) -> bool {
        self.aliases.contains_key(assembly)
    }

    pub fn insert(
        &mut self,
        file_name: &str,
        bytes: &[u8],
        limits: ManagedAssemblyLimits,
    ) -> Result<usize, Box<ManagedAssemblyError>> {
        let assembly = ManagedAssembly::parse(file_name, bytes, limits)?;
        let path_alias = file_stem(file_name);
        if let Some(previous) = self.aliases.get(&path_alias) {
            return Err(managed_assembly_error(
                file_name,
                ManagedAssemblyErrorKind::DuplicatePathAlias {
                    alias: path_alias,
                    previous_file: self.assemblies[*previous].file_name.clone(),
                },
            ));
        }

        let index = self.assemblies.len();
        self.assemblies.push(assembly);
        self.aliases.insert(path_alias, index);
        let manifest_alias = self.assemblies[index].assembly_name.clone();
        // `BaseManager.Add` assigns the manifest-name dictionary key, replacing a previous
        // manifest alias while keeping the path-derived `Dictionary.Add` collision strict.
        self.aliases.insert(manifest_alias, index);
        Ok(index)
    }

    pub fn find_type(&self, script_id: &ScriptIdentifier) -> Option<&ManagedTypeDefinition> {
        let name = script_id.name.as_deref()?;
        let assembly_index = *self.aliases.get(&script_id.assembly)?;
        self.assemblies[assembly_index].find_type(&script_id.namespace, name)
    }

    /// Resolve a metadata signature through the same filename/manifest aliases as MonoManager.
    pub fn find_type_by_identity(
        &self,
        identity: &ManagedTypeIdentity,
    ) -> Option<&ManagedTypeDefinition> {
        let assembly_index = *self.aliases.get(&identity.assembly)?;
        self.assemblies[assembly_index].find_type(&identity.namespace, &identity.name)
    }

    pub fn is_present(&self, script_id: &ScriptIdentifier) -> bool {
        self.find_type(script_id).is_some()
    }

    pub fn is_valid(&self, script_id: &ScriptIdentifier) -> bool {
        self.find_type(script_id)
            .is_some_and(|definition| !definition.is_abstract)
    }
}

impl ManagedScriptResolutionReport {
    /// Resolve every decoded class-115 object through the loaded Mono assembly catalog.
    pub fn from_object_workspace(
        objects: &ObjectWorkspace,
        catalog: &ManagedAssemblyCatalog,
    ) -> Result<Self, ManagedScriptResolutionError> {
        let mut scripts = Vec::new();
        scripts.try_reserve(objects.objects().len()).map_err(|_| {
            ManagedScriptResolutionError::AllocationFailed {
                requested: objects.objects().len(),
            }
        })?;
        for object in objects
            .objects()
            .iter()
            .filter(|object| object.factory.class_id == 115)
        {
            scripts.push(resolve_mono_script_object(object, catalog));
        }
        Ok(Self { scripts })
    }

    pub fn resolved_count(&self) -> usize {
        self.scripts
            .iter()
            .filter(|script| {
                matches!(
                    script.outcome,
                    ManagedScriptResolutionOutcome::Resolved { .. }
                )
            })
            .count()
    }

    pub fn valid_count(&self) -> usize {
        self.scripts
            .iter()
            .filter(|script| {
                matches!(
                    script.outcome,
                    ManagedScriptResolutionOutcome::Resolved { is_valid: true }
                )
            })
            .count()
    }
}

pub(crate) fn resolve_mono_script_object(
    object: &WorkspaceObject,
    catalog: &ManagedAssemblyCatalog,
) -> ManagedScriptResolution {
    let payload = match &object.payload {
        Ok(payload) => payload,
        Err(error) => {
            return ManagedScriptResolution {
                file_index: object.file_index,
                object_index: object.object_index,
                path_id: object.path_id,
                script: None,
                outcome: ManagedScriptResolutionOutcome::MetadataUnavailable(error.to_string()),
            };
        }
    };
    resolve_mono_script_payload(object, payload, catalog)
}

/// Read the fields used by the source `ScriptExportCollection` from one materialized MonoScript.
///
/// This is separate from assembly identity resolution: project export needs the serialized
/// names and execution order even when the assembly manager cannot resolve the type.
pub(crate) fn mono_script_export_identity(
    object: &WorkspaceObject,
) -> Result<MonoScriptExportIdentity, String> {
    let payload = object
        .payload
        .as_ref()
        .map_err(|error| format!("MonoScript payload is unavailable: {error}"))?;
    let metadata = mono_script_identifier(&payload.root)?;
    let execution_order = match payload
        .root
        .unique_field("m_ExecutionOrder")
        .map_err(|error| format!("MonoScript field \"m_ExecutionOrder\": {error}"))?
    {
        None => 0,
        Some(SerializedValue::I32(value)) => *value as i16,
        Some(SerializedValue::I16(value)) => *value,
        Some(_) => {
            return Err("MonoScript field \"m_ExecutionOrder\" is not a signed integer".into())
        }
    };
    Ok(MonoScriptExportIdentity {
        assembly: crate::io::special_names::fix_assembly_name(&metadata.assembly),
        namespace: metadata.namespace,
        class: metadata.name,
        execution_order,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MonoScriptExportIdentity {
    pub assembly: String,
    pub namespace: String,
    pub class: String,
    pub execution_order: i16,
}

fn resolve_mono_script_payload(
    object: &WorkspaceObject,
    payload: &ParsedObjectPayload,
    catalog: &ManagedAssemblyCatalog,
) -> ManagedScriptResolution {
    let metadata = match mono_script_identifier(&payload.root) {
        Ok(script) => script,
        Err(reason) => {
            return ManagedScriptResolution {
                file_index: object.file_index,
                object_index: object.object_index,
                path_id: object.path_id,
                script: None,
                outcome: ManagedScriptResolutionOutcome::MetadataUnavailable(reason),
            };
        }
    };
    let script = ScriptIdentifier {
        assembly: fix_assembly_name(&metadata.assembly),
        namespace: metadata.namespace,
        name: Some(metadata.name),
    };
    let outcome = if !catalog.contains_assembly(&script.assembly) {
        ManagedScriptResolutionOutcome::AssemblyNotLoaded
    } else if let Some(definition) = catalog.find_type(&script) {
        ManagedScriptResolutionOutcome::Resolved {
            is_valid: !definition.is_abstract,
        }
    } else {
        ManagedScriptResolutionOutcome::TypeNotFound
    };
    ManagedScriptResolution {
        file_index: object.file_index,
        object_index: object.object_index,
        path_id: object.path_id,
        script: Some(script),
        outcome,
    }
}

struct MonoScriptMetadata {
    assembly: String,
    namespace: String,
    name: String,
}

fn mono_script_identifier(root: &SerializedValue) -> Result<MonoScriptMetadata, String> {
    let read = |field_name: &str, required: bool| -> Result<String, String> {
        let value = root
            .unique_field(field_name)
            .map_err(|error| format!("MonoScript field {field_name:?}: {error}"))?;
        let Some(value) = value else {
            return if required {
                Err(format!(
                    "MonoScript is missing required field {field_name:?}"
                ))
            } else {
                Ok(String::new())
            };
        };
        match value {
            SerializedValue::StringBytes(bytes) => String::from_utf8(bytes.clone())
                .map_err(|error| format!("MonoScript field {field_name:?} is not UTF-8: {error}")),
            _ => Err(format!("MonoScript field {field_name:?} is not a string")),
        }
    };
    Ok(MonoScriptMetadata {
        assembly: read("m_AssemblyName", false)?,
        namespace: read("m_Namespace", false)?,
        name: read("m_ClassName", true)?,
    })
}

/// Bounds-checked PE/COFF inspection for IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR.
///
/// AssetRipper's `MonoManager.TryLoad` ignores malformed PE files and valid native images before
/// reading managed metadata. A nonzero CLR data directory opts the file into the strict metadata
/// parser and any subsequent parse failure is retained as a manager-initialization error.
fn has_cli_directory(bytes: &[u8]) -> bool {
    let read_u16 = |offset: usize| -> Option<u16> {
        let bytes = bytes.get(offset..offset.checked_add(2)?)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    };
    let read_u32 = |offset: usize| -> Option<u32> {
        let bytes = bytes.get(offset..offset.checked_add(4)?)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    if bytes.get(..2) != Some(b"MZ") {
        return false;
    }
    let Some(pe_offset) = read_u32(0x3c).and_then(|value| usize::try_from(value).ok()) else {
        return false;
    };
    if bytes.get(pe_offset..pe_offset.saturating_add(4)) != Some(b"PE\0\0") {
        return false;
    }
    let Some(optional_header_size) = read_u16(pe_offset.saturating_add(20)) else {
        return false;
    };
    let optional_header_offset = pe_offset.saturating_add(24);
    let Some(optional_header_end) =
        optional_header_offset.checked_add(usize::from(optional_header_size))
    else {
        return false;
    };
    if optional_header_end > bytes.len() {
        return false;
    }
    let Some(magic) = read_u16(optional_header_offset) else {
        return false;
    };
    let (directory_count_offset, directory_table_offset) = match magic {
        0x10b => (92usize, 96usize),
        0x20b => (108usize, 112usize),
        _ => return false,
    };
    if read_u32(optional_header_offset.saturating_add(directory_count_offset))
        .is_none_or(|count| count <= 14)
    {
        return false;
    }
    let directory = optional_header_offset
        .saturating_add(directory_table_offset)
        .saturating_add(14 * 8);
    if directory
        .checked_add(8)
        .is_none_or(|end| end > optional_header_end)
    {
        return false;
    }
    let Some(rva) = read_u32(directory) else {
        return false;
    };
    let Some(size) = read_u32(directory.saturating_add(4)) else {
        return false;
    };
    rva != 0 && size != 0
}

#[derive(Debug)]
pub struct ManagedAssemblyLoadError {
    pub virtual_path: Option<String>,
    pub kind: ManagedAssemblyLoadErrorKind,
}

#[derive(Debug)]
pub enum ManagedAssemblyLoadErrorKind {
    AssemblyCountLimitExceeded { count: usize, limit: usize },
    TotalFileByteCountOverflow,
    TotalFileBytesLimitExceeded { byte_count: u64, limit: u64 },
    FileSizeLimitExceeded { file_bytes: u64, limit: usize },
    FileSystem(io::Error),
    Assembly(ManagedAssemblyError),
}

impl Display for ManagedAssemblyLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.virtual_path {
            write!(formatter, "managed assembly input {path:?}: ")?;
        }
        match &self.kind {
            ManagedAssemblyLoadErrorKind::AssemblyCountLimitExceeded { count, limit } => {
                write!(
                    formatter,
                    "assembly count {count} exceeds the {limit}-file limit"
                )
            }
            ManagedAssemblyLoadErrorKind::TotalFileByteCountOverflow => {
                formatter.write_str("aggregate input byte count overflows")
            }
            ManagedAssemblyLoadErrorKind::TotalFileBytesLimitExceeded { byte_count, limit } => {
                write!(
                    formatter,
                    "aggregate input uses {byte_count} bytes, exceeding the {limit}-byte limit"
                )
            }
            ManagedAssemblyLoadErrorKind::FileSizeLimitExceeded { file_bytes, limit } => {
                write!(
                    formatter,
                    "file uses {file_bytes} bytes, exceeding the {limit}-byte limit"
                )
            }
            ManagedAssemblyLoadErrorKind::FileSystem(error) => {
                write!(formatter, "VFS read failed: {error}")
            }
            ManagedAssemblyLoadErrorKind::Assembly(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for ManagedAssemblyLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ManagedAssemblyLoadErrorKind::FileSystem(error) => Some(error),
            ManagedAssemblyLoadErrorKind::Assembly(error) => Some(error),
            _ => None,
        }
    }
}

fn managed_assembly_load_error(
    virtual_path: Option<String>,
    kind: ManagedAssemblyLoadErrorKind,
) -> Box<ManagedAssemblyLoadError> {
    Box::new(ManagedAssemblyLoadError { virtual_path, kind })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedScriptResolutionError {
    AllocationFailed { requested: usize },
}

impl Display for ManagedScriptResolutionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { requested } => write!(
                formatter,
                "could not allocate {requested} MonoScript resolution records"
            ),
        }
    }
}

impl Error for ManagedScriptResolutionError {}

fn file_stem(file_name: &str) -> String {
    let normalized = file_name.replace('\\', "/");
    let leaf = normalized.rsplit('/').next().unwrap_or(&normalized);
    leaf.rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(leaf)
        .to_owned()
}

fn attribute_type_name(constructor: UserMethod, resolution: &Resolution<'_>) -> Option<String> {
    match constructor {
        UserMethod::Definition(method_index) => {
            Some(resolution[method_index.parent_type()].type_name())
        }
        UserMethod::Reference(method_index) => match &resolution[method_index].parent {
            MethodReferenceParent::Type(MethodType::Base(base)) => match base.as_ref() {
                BaseType::Type { source, .. } => Some(match source {
                    TypeSource::User(user_type) => user_type.type_name(resolution),
                    TypeSource::Generic { base, .. } => base.type_name(resolution),
                }),
                _ => None,
            },
            MethodReferenceParent::Type(MethodType::TypeGeneric(_))
            | MethodReferenceParent::Type(MethodType::MethodGeneric(_))
            | MethodReferenceParent::Module(_) => None,
            MethodReferenceParent::VarargMethod(method_index) => {
                Some(resolution[method_index.parent_type()].type_name())
            }
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedAssemblyErrorKind {
    FileSizeLimitExceeded {
        file_bytes: usize,
        limit: usize,
    },
    MetadataParse(String),
    TypeCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FieldCountOverflow,
    FieldCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    DuplicatePathAlias {
        alias: String,
        previous_file: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedAssemblyError {
    pub file_name: String,
    pub kind: ManagedAssemblyErrorKind,
}

impl Display for ManagedAssemblyError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "managed assembly {:?}: ", self.file_name)?;
        match &self.kind {
            ManagedAssemblyErrorKind::FileSizeLimitExceeded { file_bytes, limit } => {
                write!(
                    formatter,
                    "file has {file_bytes} bytes, exceeding the {limit}-byte limit"
                )
            }
            ManagedAssemblyErrorKind::MetadataParse(reason) => {
                write!(formatter, "CLR metadata could not be parsed: {reason}")
            }
            ManagedAssemblyErrorKind::TypeCountLimitExceeded { count, limit } => {
                write!(
                    formatter,
                    "type count {count} exceeds the {limit}-type limit"
                )
            }
            ManagedAssemblyErrorKind::FieldCountOverflow => {
                formatter.write_str("field count overflows")
            }
            ManagedAssemblyErrorKind::FieldCountLimitExceeded { count, limit } => {
                write!(
                    formatter,
                    "field count {count} exceeds the {limit}-field limit"
                )
            }
            ManagedAssemblyErrorKind::DuplicatePathAlias {
                alias,
                previous_file,
            } => write!(
                formatter,
                "path alias {alias:?} duplicates an assembly already loaded from {previous_file:?}"
            ),
        }
    }
}

impl Error for ManagedAssemblyError {}

fn managed_assembly_error(
    file_name: &str,
    kind: ManagedAssemblyErrorKind,
) -> Box<ManagedAssemblyError> {
    Box::new(ManagedAssemblyError {
        file_name: file_name.to_owned(),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{
        has_cli_directory, resolve_mono_script_object, ManagedAssembly, ManagedAssemblyCatalog,
        ManagedAssemblyCatalogLimits, ManagedAssemblyErrorKind, ManagedAssemblyLimits,
        ManagedAssemblyLoadErrorKind, ManagedScriptResolutionOutcome,
    };
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::processing::asset_factory::{
        FactoryAssetKind, FactoryLogLevel, FactoryObjectState, ObjectTailOutcome,
    };
    use crate::processing::script_identity::ScriptIdentifier;
    use crate::serialized_file::object::{
        ParsedObjectPayload, SerializedFieldValue, SerializedValue,
    };
    use crate::serialized_file::object_workspace::WorkspaceObject;

    const ASSEMBLY_BYTES: &[u8] = include_bytes!("managed_assembly_fixture.dll");

    fn script_id(assembly: &str, namespace: &str, name: &str) -> ScriptIdentifier {
        ScriptIdentifier::try_new(assembly, namespace, name)
            .expect("valid fixture script identifier")
    }

    #[test]
    fn managed_assembly_metadata_resolves_script_types_and_field_attributes() {
        let assembly = ManagedAssembly::parse(
            "Assembly-CSharp.dll",
            ASSEMBLY_BYTES,
            ManagedAssemblyLimits::default(),
        )
        .unwrap();

        assert_eq!(assembly.assembly_name, "FixtureScripts");
        let hero = assembly
            .find_type("Game", "HeroController")
            .expect("fixture HeroController");
        assert!(!hero.is_abstract);
        assert_eq!(hero.base_type.as_deref(), Some("Game.BaseController"));
        assert_eq!(
            hero.fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["health", "speed", "ignored", "staticValue", "locked"]
        );
        assert_eq!(hero.fields[0].signature, "int");
        assert!(hero.fields[0].is_public);
        assert!(hero.fields[1]
            .attributes
            .iter()
            .any(|name| name == "UnityEngine.SerializeField"));
        assert!(hero.fields[1].signature.contains("float"));
        assert!(hero.fields[2].is_not_serialized);
        assert!(hero.fields[3].is_static);
        assert!(hero.fields[4].is_init_only);
    }

    #[test]
    fn assembly_catalog_matches_source_type_lookup_keys_and_validity() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Managed/Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();

        assert!(catalog.is_present(&script_id("Assembly-CSharp", "Game", "HeroController")));
        assert!(catalog.is_present(&script_id("FixtureScripts", "Game", "HeroController")));
        assert!(catalog.is_valid(&script_id("Assembly-CSharp", "Game", "HeroController")));
        assert!(catalog.is_present(&script_id("Assembly-CSharp", "Game", "AbstractController")));
        assert!(!catalog.is_valid(&script_id("Assembly-CSharp", "Game", "AbstractController")));
        assert!(!catalog.is_present(&script_id("assembly-csharp", "Game", "HeroController")));
        assert!(!catalog.is_present(&script_id("Assembly-CSharp", "game", "HeroController")));
        assert!(!catalog.is_present(&script_id("Assembly-CSharp", "Game", "MissingController")));
    }

    #[test]
    fn assembly_catalog_replays_the_frozen_source_mono_manager_queries() {
        let fixture = include_str!("managed_assembly_fixture.txt");
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();

        let mut query_count = 0;
        let hero = catalog
            .find_type(&script_id("Assembly-CSharp", "Game", "HeroController"))
            .expect("source fixture resolves HeroController");
        let mut field_count = 0;
        for (line_index, line) in fixture.lines().enumerate() {
            let fields: Vec<_> = line.split('|').collect();
            match fields.first().copied() {
                Some("QUERY") => {
                    assert_eq!(fields.len(), 7, "fixture line {}", line_index + 1);
                    let script_id = ScriptIdentifier::try_new(fields[1], fields[2], fields[3])
                        .unwrap_or_else(|error| panic!("fixture line {}: {error}", line_index + 1));
                    let resolved = catalog.find_type(&script_id);
                    let unique_name = resolved
                        .map(|_| script_id.unique_name())
                        .unwrap_or_else(|| "<default>".to_owned());
                    let present = resolved.is_some();
                    let valid = resolved.is_some_and(|definition| !definition.is_abstract);
                    assert_eq!(unique_name, fields[4], "fixture line {}", line_index + 1);
                    assert_eq!(
                        source_bool(present),
                        fields[5],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(valid),
                        fields[6],
                        "fixture line {}",
                        line_index + 1
                    );
                    query_count += 1;
                }
                Some("FIELD") => {
                    assert_eq!(fields.len(), 9, "fixture line {}", line_index + 1);
                    let actual = hero.fields.get(field_count).unwrap_or_else(|| {
                        panic!("unexpected extra source field on line {}", line_index + 1)
                    });
                    assert_eq!(actual.name, fields[1], "fixture line {}", line_index + 1);
                    assert_eq!(
                        source_clr_type_name(&actual.signature),
                        fields[2],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_public),
                        fields[3],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_static),
                        fields[4],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_literal),
                        fields[5],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_not_serialized),
                        fields[6],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_init_only),
                        fields[7],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        actual.attributes.join(","),
                        fields[8],
                        "fixture line {}",
                        line_index + 1
                    );
                    field_count += 1;
                }
                _ => {}
            }
        }
        assert_eq!(query_count, 6, "all source manager outcomes are replayed");
        assert_eq!(
            field_count,
            hero.fields.len(),
            "all source field rows are replayed"
        );
    }

    fn source_bool(value: bool) -> &'static str {
        if value {
            "True"
        } else {
            "False"
        }
    }

    fn source_clr_type_name(signature: &str) -> &str {
        match signature {
            "bool" => "System.Boolean",
            "char" => "System.Char",
            "sbyte" => "System.SByte",
            "byte" => "System.Byte",
            "short" => "System.Int16",
            "ushort" => "System.UInt16",
            "int" => "System.Int32",
            "uint" => "System.UInt32",
            "long" => "System.Int64",
            "ulong" => "System.UInt64",
            "float" => "System.Single",
            "double" => "System.Double",
            "string" => "System.String",
            "object" => "System.Object",
            other => other,
        }
    }

    #[test]
    fn assembly_catalog_rejects_duplicate_path_aliases_without_replacing_types() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();
        let error = catalog
            .insert(
                "other/Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ManagedAssemblyErrorKind::DuplicatePathAlias { ref alias, .. }
                if alias == "Assembly-CSharp"
        ));
        assert_eq!(catalog.assemblies().len(), 1);
        assert!(catalog.is_valid(&script_id("Assembly-CSharp", "Game", "HeroController")));
    }

    #[test]
    fn assembly_catalog_reports_unmanaged_and_oversized_inputs_with_context() {
        let malformed = ManagedAssembly::parse(
            "scripts/not-a-managed.dll",
            b"MZ not a valid PE image",
            ManagedAssemblyLimits::default(),
        )
        .unwrap_err();
        assert_eq!(malformed.file_name, "scripts/not-a-managed.dll");
        assert!(matches!(
            malformed.kind,
            ManagedAssemblyErrorKind::MetadataParse(_)
        ));

        let oversized = ManagedAssembly::parse(
            "scripts/too-large.dll",
            &[0; 8],
            ManagedAssemblyLimits {
                max_file_bytes: 7,
                ..ManagedAssemblyLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            oversized.kind,
            ManagedAssemblyErrorKind::FileSizeLimitExceeded {
                file_bytes: 8,
                limit: 7
            }
        ));
    }

    #[test]
    fn mounted_catalog_loads_managed_assemblies_and_skips_native_pe_inputs() {
        assert!(has_cli_directory(ASSEMBLY_BYTES));
        assert!(!has_cli_directory(b"MZ not a valid PE image"));

        let mut filesystem = VirtualFileSystem::new();
        filesystem
            .write_all_bytes("/Assembly-CSharp.dll", ASSEMBLY_BYTES)
            .unwrap();
        filesystem
            .write_all_bytes("/Native.dll", b"MZ native or malformed PE")
            .unwrap();
        let paths = [
            DependencyFilePath {
                file_name: "Assembly-CSharp.dll".to_owned(),
                virtual_path: "/Assembly-CSharp.dll".to_owned(),
            },
            DependencyFilePath {
                file_name: "Native.dll".to_owned(),
                virtual_path: "/Native.dll".to_owned(),
            },
        ];
        let (catalog, report) = super::ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &paths,
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(report.candidate_count, 2);
        assert_eq!(report.loaded_count, 1);
        assert_eq!(report.skipped_non_cli, ["Native.dll"]);
        assert!(report.processed_bytes > ASSEMBLY_BYTES.len() as u64);
        assert!(catalog.contains_assembly("Assembly-CSharp"));
        assert!(catalog.contains_assembly("FixtureScripts"));

        let error = super::ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &paths,
            ManagedAssemblyCatalogLimits {
                max_assemblies: 1,
                ..ManagedAssemblyCatalogLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            ManagedAssemblyLoadErrorKind::AssemblyCountLimitExceeded { count: 2, limit: 1 }
        ));
    }

    #[test]
    fn decoded_monoscript_identity_reaches_source_manager_validity_rules() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();
        let object = monoscript_object("Assembly-CSharp", "Game", "HeroController");
        assert_eq!(
            resolve_mono_script_object(&object, &catalog).outcome,
            ManagedScriptResolutionOutcome::Resolved { is_valid: true }
        );
        let abstract_object = monoscript_object("Assembly-CSharp", "Game", "AbstractController");
        assert_eq!(
            resolve_mono_script_object(&abstract_object, &catalog).outcome,
            ManagedScriptResolutionOutcome::Resolved { is_valid: false }
        );
        let missing = monoscript_object("Assembly-CSharp", "Game", "MissingController");
        assert_eq!(
            resolve_mono_script_object(&missing, &catalog).outcome,
            ManagedScriptResolutionOutcome::TypeNotFound
        );
        let absent_assembly = monoscript_object("MissingAssembly", "Game", "HeroController");
        assert_eq!(
            resolve_mono_script_object(&absent_assembly, &catalog).outcome,
            ManagedScriptResolutionOutcome::AssemblyNotLoaded
        );
    }

    fn monoscript_object(assembly: &str, namespace: &str, name: &str) -> WorkspaceObject {
        let string_field = |field_name: &str, value: &str| SerializedFieldValue {
            name: field_name.to_owned(),
            value: SerializedValue::StringBytes(value.as_bytes().to_vec()),
        };
        let payload = ParsedObjectPayload {
            object_index: 0,
            path_id: 7,
            type_name: "MonoScript".to_owned(),
            root: SerializedValue::Struct(vec![
                string_field("m_ClassName", name),
                string_field("m_Namespace", namespace),
                string_field("m_AssemblyName", assembly),
            ]),
            consumed_bytes: 0,
            trailing_bytes: Vec::new(),
            allocated_bytes: 0,
        };
        WorkspaceObject {
            file_index: 0,
            object_index: 0,
            path_id: 7,
            serialized_type_index: Some(0),
            raw_type_id: 115,
            resolved_type_index: Some(0),
            class_id: Some(115),
            script_type_index: None,
            is_destroyed: None,
            is_stripped: Some(false),
            raw_payload_range: 0..0,
            factory: FactoryObjectState {
                object_index: 0,
                path_id: 7,
                class_id: 115,
                kind: FactoryAssetKind::Generated("MonoScript".to_owned()),
                class_name: Cow::Borrowed("MonoScript"),
                name: name.to_owned(),
                tail: Some(ObjectTailOutcome::Exact),
                chinese_texture_fields: None,
                structure: None,
                log_level: FactoryLogLevel::Silent,
                log_message: None,
            },
            recovered_type_tree: None,
            payload: Ok(payload),
        }
    }
}
