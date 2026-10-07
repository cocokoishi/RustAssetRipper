//! Script identity ported from `AssetRipper.Import.Structure.Assembly`.
//!
//! Covers the first audited slice of `TYPE-SCRIPT-001`: `ScriptIdentifier`
//! (construction validation, `ToUniqueName` overloads, `ToString`, `IsDefault`,
//! `UniqueName`), the `ScriptingBackend` discriminants, the `MonoManager.IsMonoAssembly`
//! file-name predicate and mscorlib discovery (`TryGetMscorlibPath`), the pure string kernels
//! behind `MonoScriptExtensions.GetFullName`, `GetValidAssemblyName`, and `HasAssemblyName`,
//! the `ModuleExtensions.GetType` match rule with the `BaseManager.IsPresent`/`IsValid`
//! gates, and the `PropertiesHash` fallback bytes behind `GetPropertiesHash` (the
//! `IMonoScript`/assembly-manager lookups themselves stay outside: they need live
//! managed assemblies).
//!
//! Differential evidence: `src/io/script_identity_fixture.txt` (probe
//! `Temp/opencode/scriptidprobe` against the shipped `AssetRipper.Import.dll` and
//! `AssetRipper.IO.Files.dll`, SDK 10.0.401). The null-namespace rejection arm is
//! discharged by the type system rather than modeled: every Rust `&str`/`String`
//! is non-null by construction, so a null namespace cannot arrive; `try_new`
//! models exactly the two representable rejections (empty assembly, empty name),
//! and the legal empty namespace is accepted like the source (pinned by the
//! `THROW` rows replayed in `constructor_rejections_match_the_reference_runtime`).

use std::error::Error;
use std::fmt::{self, Display};

use crate::io::special_names::{fix_assembly_name, remove_assembly_file_extension};
use crate::serialized_file::metadata::{SerializedTypeReference, TypeTree, TypeTreeNode};

/// `ScriptingBackend`: the source scripting backend discriminants.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(i32)]
pub enum ScriptingBackend {
    /// Backend could not be determined.
    Unknown = 0,
    /// Mono scripting backend.
    Mono = 1,
    /// IL2Cpp scripting backend.
    IL2Cpp = 2,
}

impl ScriptingBackend {
    /// The inverse of the implicit enum-to-int conversion; unknown values yield `None`.
    pub fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Mono),
            2 => Some(Self::IL2Cpp),
            _ => None,
        }
    }
}

/// Why a script identifier cannot be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptIdError {
    /// The assembly name is empty (`ArgumentException` in the source).
    EmptyAssembly,
    /// The type name is empty (`ArgumentException` in the source).
    EmptyName,
}

impl Display for ScriptIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAssembly => write!(formatter, "script assembly name is empty"),
            Self::EmptyName => write!(formatter, "script type name is empty"),
        }
    }
}

impl Error for ScriptIdError {}

/// The `.NET` fallback `ScriptIdentifier.ToString` renders for the default value
/// (`ValueType.ToString()`), pinned by the probe.
pub const DEFAULT_SCRIPT_IDENTIFIER_DISPLAY: &str =
    "AssetRipper.Import.Structure.Assembly.ScriptIdentifier";

/// `MonoManager.AssemblyExtension`, the file-name suffix that marks a Mono assembly.
pub const ASSEMBLY_EXTENSION: &str = ".dll";

/// `MonoManager.IsMonoAssembly`: a file is a Mono assembly candidate exactly when its
/// name ends with `.dll` under an ordinal (case-sensitive) comparison.
pub fn is_mono_assembly(file_name: &str) -> bool {
    file_name.ends_with(ASSEMBLY_EXTENSION)
}

/// `MonoManager` mscorlib discovery over caller facts (`TryGetMscorlibPath`).
///
/// The exact `mscorlib.dll` entry wins first; otherwise the first
/// case-insensitive match in iteration order wins; otherwise there is none.
/// Callers pass the discovered assemblies in enumeration order. The scan uses
/// ASCII case folding like the tree's other ordinal-ignorecase renderings.
pub fn find_mscorlib_path(assemblies: &[(String, String)]) -> Option<&str> {
    if let Some((_, path)) = assemblies.iter().find(|(name, _)| name == "mscorlib.dll") {
        return Some(path);
    }
    assemblies
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("mscorlib.dll"))
        .map(|(_, path)| path.as_str())
}

/// `MonoScriptExtensions.GetPropertiesHash` fallback over caller facts.
///
/// When the `PropertiesHash_Hash128_5` field is absent, the source expands the legacy
/// `PropertiesHash_UInt32` into the first four hash bytes in collection byte order and
/// leaves the remaining twelve bytes at the struct default (zero). `hash` is the legacy
/// value; `big_endian` is whether the owning collection reads big-endian (the source
/// checks `monoScript.Collection.EndianType`, noting most platforms are little-endian).
pub fn properties_hash_fallback_bytes(hash: u32, big_endian: bool) -> [u8; 16] {
    let words = if big_endian {
        hash.to_be_bytes()
    } else {
        hash.to_le_bytes()
    };
    [
        words[0], words[1], words[2], words[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]
}

/// `ScriptIdentifier`: an assembly-qualified script type identity.
///
/// `name` is `None` exactly for the default value (the source's null `Name`), which
/// is also the only state where `assembly`/`namespace` are empty.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptIdentifier {
    /// The assembly name.
    pub assembly: String,
    /// The namespace, possibly empty.
    pub namespace: String,
    /// The type name; `None` is the default value.
    pub name: Option<String>,
}

impl ScriptIdentifier {
    /// The source constructor: assembly and name must be non-empty; the namespace
    /// must be non-null (unrepresentable, and therefore always satisfied, in Rust).
    pub fn try_new(assembly: &str, namespace: &str, name: &str) -> Result<Self, ScriptIdError> {
        if assembly.is_empty() {
            return Err(ScriptIdError::EmptyAssembly);
        }
        if name.is_empty() {
            return Err(ScriptIdError::EmptyName);
        }
        Ok(Self {
            assembly: assembly.to_owned(),
            namespace: namespace.to_owned(),
            name: Some(name.to_owned()),
        })
    }

    /// `IsDefault`: the name is absent.
    pub fn is_default(&self) -> bool {
        self.name.is_none()
    }

    /// `UniqueName` for this identifier.
    pub fn unique_name(&self) -> String {
        to_unique_name(
            &self.assembly,
            &self.namespace,
            self.name.as_deref().unwrap_or_default(),
        )
    }

    /// `ToString`: the default value renders as the .NET type name; otherwise the
    /// bare name, qualified by the namespace when it is non-empty.
    pub fn to_display(&self) -> String {
        match &self.name {
            None => DEFAULT_SCRIPT_IDENTIFIER_DISPLAY.to_owned(),
            Some(name) if self.namespace.is_empty() => name.clone(),
            Some(name) => format!("{}.{}", self.namespace, name),
        }
    }
}

/// `ScriptIdentifier.ToUniqueName(assembly, namespace, name)`.
pub fn to_unique_name(assembly: &str, namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        format!("[{assembly}]{name}")
    } else {
        format!("[{assembly}]{namespace}.{name}")
    }
}

/// `ScriptIdentifier.ToUniqueName(assembly, fullName)`.
pub fn to_unique_name_full(assembly: &str, full_name: &str) -> String {
    format!("[{assembly}]{full_name}")
}

/// `MonoScriptExtensions.GetFullName` over caller facts: the bare class name, or
/// `namespace.class` when a namespace is present.
pub fn get_full_name(namespace: &str, class_name: &str) -> String {
    if namespace.is_empty() {
        class_name.to_owned()
    } else {
        format!("{namespace}.{class_name}")
    }
}

/// `MonoScriptExtensions.GetValidAssemblyName` over caller facts: the fixed
/// assembly name, defaulting an empty result to `Assembly-CSharp`.
pub fn get_valid_assembly_name(assembly: &str) -> String {
    let fixed = fix_assembly_name(assembly);
    if fixed.is_empty() {
        "Assembly-CSharp".to_owned()
    } else {
        fixed
    }
}

/// `MonoScriptExtensions.HasAssemblyName` over caller facts: the assembly name is present
/// in release collections, and otherwise exactly when the field is not release-only
/// (the source note reads "Before 2018.2 or Release or after 2022.2"). The generated
/// field gate arrives pre-evaluated; only the outcome is modeled here.
pub fn has_assembly_name(collection_is_release: bool, assembly_name_is_release_only: bool) -> bool {
    collection_is_release || !assembly_name_is_release_only
}

/// The `TypeTreeResolver.TryGetSerializableType` failure when no reference matches.
pub const TYPE_REFERENCE_NOT_FOUND: &str =
    "The type reference was not found in the set of available type trees.";

/// The `TypeTreeResolver.TryGetSerializableType` failure when the matched `OldType`
/// cannot build nodes. Node building (`TryMakeFromTypeTree` plus
/// `SerializableTreeType.FromReferencedObjectNode`) is ported below.
pub const TYPE_TREE_NODE_BUILD_FAILED: &str = "Failed to create nodes from the type tree.";

/// The `TypeTreeResolver` three-way match predicate: class name, namespace, and the
/// extension-stripped assembly name, all compared ordinally.
///
/// Absent Rust fields read as empty, matching the source `Utf8String.Empty` defaults.
/// A default identifier (absent name) never matches: the source compares against a null
/// name, which no stored class name equals.
pub fn type_reference_matches_script(
    reference: &SerializedTypeReference,
    script: &ScriptIdentifier,
) -> bool {
    let Some(name) = script.name.as_deref() else {
        return false;
    };
    reference.class_name.as_deref().unwrap_or_default() == name
        && reference.namespace.as_deref().unwrap_or_default() == script.namespace
        && remove_assembly_file_extension(reference.assembly_name.as_deref().unwrap_or_default())
            == script.assembly
}

/// The `TypeTreeResolver` linear scan: the index of the first matching reference, or
/// `None` with `TYPE_REFERENCE_NOT_FOUND` as the failure reason.
pub fn find_type_reference_index(
    references: &[SerializedTypeReference],
    script: &ScriptIdentifier,
) -> Option<usize> {
    references
        .iter()
        .position(|reference| type_reference_matches_script(reference, script))
}

/// `ModuleExtensions.GetType` over caller facts: the index of the first top-level type
/// whose namespace (a null namespace reads empty) and name match, both compared
/// ordinally. Callers pass the module's top-level types in order; nested types are
/// never consulted. Live Cecil lookup stays outside: only the match rule is modeled.
pub fn find_module_type_index(
    types: &[(Option<&str>, &str)],
    namespace: &str,
    name: &str,
) -> Option<usize> {
    types.iter().position(|(type_namespace, type_name)| {
        type_namespace.unwrap_or_default() == namespace && *type_name == name
    })
}

/// `BaseManager.IsPresent` over caller facts: a set manager finds a non-default script.
pub const fn script_is_present(is_set: bool, is_default: bool, found: bool) -> bool {
    is_set && !is_default && found
}

/// `TransferMetaFlags` bits consumed by node building
/// (`AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/TransferMetaFlags.cs`).
/// Align-bytes fields contribute their flag to the built field alignment.
pub const META_FLAG_ALIGN_BYTES: u32 = 0x4000;
/// `TransferMetaFlags.TransferUsingFlowMappingStyle`: flow-mapped YAML rendering.
pub const META_FLAG_FLOW_MAPPING: u32 = 0x200000;
/// `TransferMetaFlags.CharPropertyMask`: integer fields surface as `Char`.
pub const META_FLAG_CHAR_MASK: u32 = 0x2000000;

/// `BaseManager.IsValid` over caller facts: like `IsPresent`, and the found type is
/// not abstract.
pub const fn script_is_valid(
    is_set: bool,
    is_default: bool,
    found: bool,
    is_abstract: bool,
) -> bool {
    is_set && !is_default && found && !is_abstract
}

/// `ManagedReferenceTypes` node names
/// (`AssetRipper/Source/AssetRipper.SerializationLogic/ManagedReferenceTypes.cs`).
/// A `[SerializeReference]` field holding a registry object identifier.
pub const MANAGED_REFERENCE_NAME: &str = "managedReference";
/// An element of an array or list with `[SerializeReference]`.
pub const MANAGED_REF_ARRAY_ITEM_NAME: &str = "managedRefArrayItem";
/// The per-asset registry of referenced objects.
pub const MANAGED_REGISTRY_NAME: &str = "ManagedReferencesRegistry";
/// The asset-root field holding the registry.
pub const MANAGED_REGISTRY_FIELD_NAME: &str = "references";
/// The stable 64-bit reference identifier.
pub const MANAGED_REFERENCE_ID_FIELD_NAME: &str = "rid";
/// The legacy 32-bit reference index.
pub const MANAGED_INDEX_FIELD_NAME: &str = "id";

/// `PrimitiveType` (`AssetRipper/Source/AssetRipper.SerializationLogic/PrimitiveType.cs`):
/// the primitive classification behind every built type node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreePrimitiveType {
    /// `bool`.
    Bool,
    /// A `Char`-masked integer.
    Char,
    /// `char`/`UInt8`.
    Byte,
    /// `SInt8`.
    SByte,
    /// `short`/`SInt16`.
    Short,
    /// `ushort`/`UInt16`.
    UShort,
    /// `int`/`SInt32`.
    Int,
    /// `uint`/`UInt32`.
    UInt,
    /// `SInt64`/`long long`.
    Long,
    /// `UInt64`/`FileSize`.
    ULong,
    /// `float`.
    Single,
    /// `double`.
    Double,
    /// A `string` node.
    String,
    /// A `pair` node outside a map.
    Pair,
    /// A `pair` node inside a `map`.
    MapPair,
    /// Anything else, including structures and pointers.
    Complex,
}

impl TreePrimitiveType {
    /// The source enum variant name, as the differential fixture renders it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "Bool",
            Self::Char => "Char",
            Self::Byte => "Byte",
            Self::SByte => "SByte",
            Self::Short => "Short",
            Self::UShort => "UShort",
            Self::Int => "Int",
            Self::UInt => "UInt",
            Self::Long => "Long",
            Self::ULong => "ULong",
            Self::Single => "Single",
            Self::Double => "Double",
            Self::String => "String",
            Self::Pair => "Pair",
            Self::MapPair => "MapPair",
            Self::Complex => "Complex",
        }
    }

    /// `PrimitiveTypeExtensions.ToSystemTypeName`: the .NET type backing a primitive.
    pub const fn system_type_name(self) -> Option<&'static str> {
        match self {
            Self::Bool => Some("Boolean"),
            Self::Char => Some("Char"),
            Self::Byte => Some("Byte"),
            Self::SByte => Some("SByte"),
            Self::Short => Some("Int16"),
            Self::UShort => Some("UInt16"),
            Self::Int => Some("Int32"),
            Self::UInt => Some("UInt32"),
            Self::Long => Some("Int64"),
            Self::ULong => Some("UInt64"),
            Self::Single => Some("Single"),
            Self::Double => Some("Double"),
            Self::String => Some("String"),
            Self::Pair | Self::MapPair | Self::Complex => None,
        }
    }
}

/// `TypeTreeNodeStruct`: a level-nested TypeTree node
/// (`AssetRipper/Source/AssetRipper.Import/Structure/Assembly/TypeTrees/TypeTreeNodeStruct.cs`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuiltTypeNode {
    /// The node type name.
    pub type_name: String,
    /// The node field name.
    pub name: String,
    /// The node version.
    pub version: i32,
    /// The raw transfer meta flags.
    pub meta_flags: u32,
    /// The nested children in declaration order.
    pub children: Vec<BuiltTypeNode>,
}

impl BuiltTypeNode {
    fn child(&self, index: usize) -> Option<&BuiltTypeNode> {
        self.children.get(index)
    }

    fn align_bytes(&self) -> bool {
        self.meta_flags & META_FLAG_ALIGN_BYTES != 0
    }

    fn treat_integer_as_char(&self) -> bool {
        self.meta_flags & META_FLAG_CHAR_MASK != 0
    }

    fn flow_mapped_in_yaml(&self) -> bool {
        self.meta_flags & META_FLAG_FLOW_MAPPING != 0
    }

    /// `TypeTreeNodeStruct.IsArray`: `Array`/`TypelessData` with `size` and `data`.
    pub fn is_array(&self) -> Option<bool> {
        if (self.type_name == "Array" || self.type_name == "TypelessData")
            && self.children.len() == 2
        {
            let size = self.child(0)?;
            let data = self.child(1)?;
            return Some(size.name == "size" && size.children.is_empty() && data.name == "data");
        }
        Some(false)
    }

    /// `TypeTreeNodeStruct.IsVector`: a vector wrapper around an array node.
    pub fn is_vector(&self) -> Option<bool> {
        if (self.type_name == "vector"
            || self.type_name == "staticvector"
            || self.type_name == "set")
            && self.children.len() == 1
        {
            return self.child(0)?.is_array();
        }
        Some(false)
    }

    /// `TypeTreeNodeStruct.IsNamedVector`: a list named after its element type.
    pub fn is_named_vector(&self) -> Option<bool> {
        if self.children.len() == 1 {
            let array = self.child(0)?;
            if array.is_array()? && array.name == "Array" {
                let element = array.child(1)?;
                return Some(
                    element.type_name == self.type_name
                        || element.type_name == "Generic Mono"
                        || element.is_managed_reference()?,
                );
            }
        }
        Some(false)
    }

    /// `TypeTreeNodeStruct.IsPair`: a `first`/`second` pair.
    pub fn is_pair(&self) -> bool {
        self.type_name == "pair"
            && self.children.len() == 2
            && self.children[0].name == "first"
            && self.children[1].name == "second"
    }

    /// `TypeTreeNodeStruct.IsMap`: a `map` wrapper around an array of pairs.
    pub fn is_map(&self) -> Option<bool> {
        if self.type_name == "map" && self.children.len() == 1 {
            let array = self.child(0)?;
            if array.is_array()? {
                let data = array.child(1)?;
                return Some(data.is_pair());
            }
        }
        Some(false)
    }

    /// `TypeTreeNodeStruct.IsString`: a `string` wrapper around a byte array.
    pub fn is_string(&self) -> Option<bool> {
        if self.children.len() == 1 && self.type_name == "string" {
            let array = self.child(0)?;
            if array.is_array()? {
                let data = array.child(1)?;
                return Some(data.is_byte());
            }
        }
        Some(false)
    }

    /// A leaf `char`/`UInt8` byte.
    fn is_byte(&self) -> bool {
        self.children.is_empty() && (self.type_name == "char" || self.type_name == "UInt8")
    }

    /// `TypeTreeNodeStruct.IsPPtr`: a `PPtr<T>` with `m_FileID`/`m_PathID` leaves.
    pub fn is_pptr(&self) -> bool {
        if !self.type_name.starts_with("PPtr<") || !self.type_name.ends_with('>') {
            return false;
        }
        if self.children.len() != 2 {
            return false;
        }
        let file_id = &self.children[0];
        let path_id = &self.children[1];
        file_id.name == "m_FileID"
            && file_id.children.is_empty()
            && path_id.name == "m_PathID"
            && path_id.children.is_empty()
    }

    /// `TypeTreeNodeStruct.IsManagedReferencesRegistry`: the per-asset registry node.
    pub fn is_managed_references_registry(&self) -> bool {
        self.type_name == MANAGED_REGISTRY_NAME
            && self.name == MANAGED_REGISTRY_FIELD_NAME
            && self.children.len() > 1
    }

    /// `TypeTreeNodeStruct.IsManagedReference`: a `[SerializeReference]` identifier.
    pub fn is_managed_reference(&self) -> Option<bool> {
        if (self.type_name == MANAGED_REFERENCE_NAME
            || self.type_name == MANAGED_REF_ARRAY_ITEM_NAME)
            && self.children.len() == 1
        {
            let id = self.child(0)?;
            return Some(
                id.name == MANAGED_REFERENCE_ID_FIELD_NAME || id.name == MANAGED_INDEX_FIELD_NAME,
            );
        }
        Some(false)
    }

    /// `TypeTreeNodeStruct.IsIndexedManagedReference`: the legacy 32-bit form.
    pub fn is_indexed_managed_reference(&self) -> Option<bool> {
        if self.is_managed_reference()? {
            let id = self.child(0)?;
            return Some(id.name == MANAGED_INDEX_FIELD_NAME);
        }
        Some(false)
    }
}

/// `TypeTreeNodeStruct.TryMakeFromTypeTree` plus `FromNodeList`: an empty node list
/// builds nothing; otherwise level nesting reconstructs the root. A malformed level
/// step fails the build where the source debug-asserts and then throws.
pub fn build_type_nodes(nodes: &[TypeTreeNode]) -> Option<BuiltTypeNode> {
    if nodes.is_empty() {
        return None;
    }
    build_type_node(nodes, 0)
}

fn build_type_node(nodes: &[TypeTreeNode], index: usize) -> Option<BuiltTypeNode> {
    let node = nodes.get(index)?;
    let level = node.level;
    let mut children = Vec::new();
    if index + 1 < nodes.len() && nodes[index + 1].level > level {
        debug_assert_eq!(nodes[index + 1].level, level + 1);
        let mut cursor = index + 1;
        while cursor < nodes.len() {
            let child_level = nodes[cursor].level;
            if child_level == level + 1 {
                children.push(build_type_node(nodes, cursor)?);
                cursor += subtree_span(nodes, cursor);
            } else if child_level <= level {
                break;
            } else {
                return None;
            }
        }
    }
    Some(BuiltTypeNode {
        type_name: node.type_name.clone()?,
        name: node.name.clone()?,
        version: node.version,
        meta_flags: node.meta_flags,
        children,
    })
}

/// The flat-list span of the subtree rooted at `index` (the node plus descendants).
fn subtree_span(nodes: &[TypeTreeNode], index: usize) -> usize {
    let level = nodes[index].level;
    let mut span = 1;
    while index + span < nodes.len() && nodes[index + span].level > level {
        span += 1;
    }
    span
}

/// The unwrapped primitive behind a built node
/// (`SerializableTreeType.ToPrimititeType`): array/vector wrappers contribute their
/// depth and alignment until the primitive carrier is reached.
pub struct UnwrappedPrimitive<'a> {
    /// The first non-wrapper type name.
    pub type_name: &'a str,
    /// The primitive classification.
    pub primitive: TreePrimitiveType,
    /// The wrapper depth.
    pub array_depth: i32,
    /// Whether any wrapper or carrier aligns bytes.
    pub align_bytes: bool,
    /// The primitive carrier node.
    pub node: &'a BuiltTypeNode,
}

/// Classify one built node exactly like `ToPrimititeType`, failing on malformed
/// wrapper shapes where the source throws.
pub fn unwrap_primitive(node: &BuiltTypeNode) -> Option<UnwrappedPrimitive<'_>> {
    let mut current = node;
    let mut type_name = "";
    let mut array_depth = 0;
    let mut align_bytes = false;
    let mut is_map = false;
    loop {
        align_bytes |= current.align_bytes();
        if current.is_array()? {
            array_depth += 1;
            current = current.child(1)?;
        } else if current.is_vector()? {
            align_bytes |= current.child(0)?.align_bytes();
            array_depth += 1;
            current = current.child(0)?.child(1)?;
        } else if current.is_named_vector()? {
            if type_name.is_empty() {
                type_name = &current.type_name;
            }
            align_bytes |= current.child(0)?.align_bytes();
            array_depth += 1;
            current = current.child(0)?.child(1)?;
        } else if current.is_map()? {
            is_map = true;
            align_bytes |= current.child(0)?.align_bytes();
            array_depth += 1;
            current = current.child(0)?.child(1)?;
        } else {
            if type_name.is_empty() {
                type_name = &current.type_name;
            }
            break;
        }
    }
    let primitive = if current.children.is_empty() {
        match current.type_name.as_str() {
            "bool" => TreePrimitiveType::Bool,
            "char" | "UInt8" => TreePrimitiveType::Byte,
            "SInt8" => TreePrimitiveType::SByte,
            "short" | "SInt16" => TreePrimitiveType::Short,
            "ushort" | "UInt16" | "unsigned short" => {
                if current.treat_integer_as_char() {
                    TreePrimitiveType::Char
                } else {
                    TreePrimitiveType::UShort
                }
            }
            "int" | "SInt32" | "Type*" => TreePrimitiveType::Int,
            "uint" | "UInt32" | "unsigned int" => TreePrimitiveType::UInt,
            "SInt64" | "long long" => TreePrimitiveType::Long,
            "UInt64" | "FileSize" | "unsigned long long" => TreePrimitiveType::ULong,
            "float" => TreePrimitiveType::Single,
            "double" => TreePrimitiveType::Double,
            _ => TreePrimitiveType::Complex,
        }
    } else if current.is_string()? {
        TreePrimitiveType::String
    } else if current.is_pair() {
        if is_map {
            TreePrimitiveType::MapPair
        } else {
            TreePrimitiveType::Pair
        }
    } else {
        TreePrimitiveType::Complex
    };
    Some(UnwrappedPrimitive {
        type_name,
        primitive,
        array_depth,
        align_bytes,
        node: current,
    })
}

/// `SerializableType.Field`: a named, possibly repeated member type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeField {
    /// The member type.
    pub field_type: SerializableTreeType,
    /// The wrapper depth.
    pub array_depth: i32,
    /// The member name.
    pub name: String,
    /// Whether the member aligns bytes.
    pub align: bool,
}

impl TreeField {
    /// `Field.IsArray`: exactly one wrapper level reads as an array.
    pub const fn is_array(&self) -> bool {
        self.array_depth == 1
    }
}

/// `SerializableTreeType`: a tree-built serializable type
/// (`AssetRipper/Source/AssetRipper.Import/Structure/Assembly/TypeTrees/SerializableTreeType.cs`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializableTreeType {
    /// The type name (`.NET` names for primitives live under `System`).
    pub name: String,
    /// The namespace; `None` renders the bare name.
    pub namespace: Option<String>,
    /// The primitive classification.
    pub primitive: TreePrimitiveType,
    /// The type version (`1` for shared types, the node version for tree types).
    pub version: i32,
    /// Whether YAML uses flow mapping for this type.
    pub flow_mapped_in_yaml: bool,
    /// The member fields.
    pub fields: Vec<TreeField>,
    /// The maximum structure depth (`-1` while unknown).
    pub max_depth: i32,
}

impl SerializableTreeType {
    /// `SerializableType.FullName`: the bare name, namespace-qualified when present.
    pub fn full_name(&self) -> String {
        match &self.namespace {
            None => self.name.clone(),
            Some(namespace) => format!("{namespace}.{}", self.name),
        }
    }

    /// `SerializableType.IsMaxDepthKnown`: non-negative once `set_max_depth` runs.
    pub const fn is_max_depth_known(&self) -> bool {
        self.max_depth >= 0
    }

    fn set_max_depth(&mut self) {
        let mut max_depth = 0;
        for field in &self.fields {
            debug_assert!(field.field_type.is_max_depth_known());
            max_depth = max_depth.max(field.field_type.max_depth + 1);
        }
        self.max_depth = max_depth;
    }

    /// `SerializablePointerType.Shared`: the shared `UnityEngine.Object` pointer.
    pub fn shared_pointer() -> Self {
        Self {
            name: "Object".to_owned(),
            namespace: Some("UnityEngine".to_owned()),
            primitive: TreePrimitiveType::Complex,
            version: 1,
            flow_mapped_in_yaml: false,
            fields: Vec::new(),
            max_depth: 0,
        }
    }

    /// `SerializablePrimitiveType.GetOrCreate`: the shared `System` primitive type.
    pub fn shared_primitive(primitive: TreePrimitiveType) -> Option<Self> {
        Some(Self {
            name: primitive.system_type_name()?.to_owned(),
            namespace: Some("System".to_owned()),
            primitive,
            version: 1,
            flow_mapped_in_yaml: false,
            fields: Vec::new(),
            max_depth: 0,
        })
    }

    /// `ManagedReferenceTypes.ManagedReference`: the 64-bit `rid` identifier type.
    pub fn managed_reference() -> Option<Self> {
        Some(Self {
            name: MANAGED_REFERENCE_NAME.to_owned(),
            namespace: None,
            primitive: TreePrimitiveType::Complex,
            version: 1,
            flow_mapped_in_yaml: false,
            fields: vec![TreeField {
                field_type: Self::shared_primitive(TreePrimitiveType::Long)?,
                array_depth: 0,
                name: MANAGED_REFERENCE_ID_FIELD_NAME.to_owned(),
                align: false,
            }],
            max_depth: 1,
        })
    }

    /// `ManagedReferenceTypes.IndexedManagedReference`: the legacy 32-bit `id` form.
    pub fn indexed_managed_reference() -> Option<Self> {
        Some(Self {
            name: MANAGED_REFERENCE_NAME.to_owned(),
            namespace: None,
            primitive: TreePrimitiveType::Complex,
            version: 1,
            flow_mapped_in_yaml: false,
            fields: vec![TreeField {
                field_type: Self::shared_primitive(TreePrimitiveType::Int)?,
                array_depth: 0,
                name: MANAGED_INDEX_FIELD_NAME.to_owned(),
                align: false,
            }],
            max_depth: 1,
        })
    }

    fn referenced_managed_type() -> Option<Self> {
        let mut built = Self {
            name: "ReferencedManagedType".to_owned(),
            namespace: None,
            primitive: TreePrimitiveType::Complex,
            version: 1,
            // `CustomSerializableType(ReferencedManagedTypeName, true, ...)`: the only
            // shared managed shape rendered with flow mapping.
            flow_mapped_in_yaml: true,
            fields: Vec::new(),
            max_depth: -1,
        };
        for name in ["class", "ns", "asm"] {
            built.fields.push(TreeField {
                field_type: Self::shared_primitive(TreePrimitiveType::String)?,
                array_depth: 0,
                name: name.to_owned(),
                align: false,
            });
        }
        built.set_max_depth();
        Some(built)
    }

    fn referenced_object_data() -> Self {
        Self {
            name: "ReferencedObjectData".to_owned(),
            namespace: None,
            primitive: TreePrimitiveType::Complex,
            version: 1,
            flow_mapped_in_yaml: false,
            fields: Vec::new(),
            max_depth: 0,
        }
    }

    fn referenced_object() -> Option<Self> {
        let mut built = Self {
            name: "ReferencedObject".to_owned(),
            namespace: None,
            primitive: TreePrimitiveType::Complex,
            version: 1,
            flow_mapped_in_yaml: false,
            fields: Vec::new(),
            max_depth: -1,
        };
        built.fields.push(TreeField {
            field_type: Self::shared_primitive(TreePrimitiveType::Long)?,
            array_depth: 0,
            name: MANAGED_REFERENCE_ID_FIELD_NAME.to_owned(),
            align: false,
        });
        built.fields.push(TreeField {
            field_type: Self::referenced_managed_type()?,
            array_depth: 0,
            name: "type".to_owned(),
            align: false,
        });
        built.fields.push(TreeField {
            field_type: Self::referenced_object_data(),
            array_depth: 0,
            name: "data".to_owned(),
            align: false,
        });
        built.set_max_depth();
        Some(built)
    }

    fn registry() -> Option<Self> {
        let mut built = Self {
            name: MANAGED_REGISTRY_NAME.to_owned(),
            namespace: None,
            primitive: TreePrimitiveType::Complex,
            version: 1,
            flow_mapped_in_yaml: false,
            fields: Vec::new(),
            max_depth: -1,
        };
        built.fields.push(TreeField {
            field_type: Self::shared_primitive(TreePrimitiveType::Int)?,
            array_depth: 0,
            name: "version".to_owned(),
            align: false,
        });
        built.fields.push(TreeField {
            field_type: Self::referenced_object()?,
            array_depth: 1,
            name: "RefIds".to_owned(),
            align: true,
        });
        built.set_max_depth();
        Some(built)
    }

    /// `ManagedReferenceTypes.RegistryField`: the nested-registry substitution field.
    pub fn registry_field() -> Option<TreeField> {
        Some(TreeField {
            field_type: Self::registry()?,
            array_depth: 0,
            name: MANAGED_REGISTRY_FIELD_NAME.to_owned(),
            align: false,
        })
    }

    /// `SerializableTreeType.FromReferencedObjectNode`: the referenced-object root
    /// skips the managed-references registry child; nested registries substitute
    /// the shared registry field instead.
    pub fn from_referenced_object_node(root: &BuiltTypeNode) -> Option<Self> {
        Self::from_root_node(root, false)
    }

    fn from_root_node(root: &BuiltTypeNode, include_registry: bool) -> Option<Self> {
        let unwrapped = unwrap_primitive(root)?;
        debug_assert_eq!(unwrapped.array_depth, 0);
        debug_assert!(std::ptr::eq(unwrapped.node, root));
        let mut built = Self {
            name: unwrapped.type_name.to_owned(),
            namespace: None,
            primitive: unwrapped.primitive,
            version: root.version,
            flow_mapped_in_yaml: root.flow_mapped_in_yaml(),
            fields: Vec::new(),
            max_depth: -1,
        };
        for child in &root.children {
            if !include_registry && child.is_managed_references_registry() {
                continue;
            }
            built.fields.push(add_tree_node(child)?);
        }
        built.set_max_depth();
        Some(built)
    }

    fn from_structure_node(
        name: &str,
        node: &BuiltTypeNode,
        primitive: TreePrimitiveType,
    ) -> Option<Self> {
        let mut built = Self {
            name: name.to_owned(),
            namespace: None,
            primitive,
            version: node.version,
            flow_mapped_in_yaml: node.flow_mapped_in_yaml(),
            fields: Vec::new(),
            max_depth: -1,
        };
        for child in &node.children {
            built.fields.push(add_tree_node(child)?);
        }
        built.set_max_depth();
        Some(built)
    }
}

/// `SerializableTreeType.AddNode`: one child node becomes one member field.
fn add_tree_node(node: &BuiltTypeNode) -> Option<TreeField> {
    if node.is_managed_references_registry() {
        return SerializableTreeType::registry_field();
    }
    let unwrapped = unwrap_primitive(node)?;
    let field_type = if unwrapped.node.is_managed_reference()? {
        if unwrapped.node.is_indexed_managed_reference()? {
            SerializableTreeType::indexed_managed_reference()?
        } else {
            SerializableTreeType::managed_reference()?
        }
    } else if matches!(
        unwrapped.primitive,
        TreePrimitiveType::Complex | TreePrimitiveType::Pair | TreePrimitiveType::MapPair
    ) {
        if unwrapped.node.is_pptr() {
            SerializableTreeType::shared_pointer()
        } else {
            SerializableTreeType::from_structure_node(
                unwrapped.type_name,
                unwrapped.node,
                unwrapped.primitive,
            )?
        }
    } else {
        let mut leaf = SerializableTreeType {
            name: unwrapped.type_name.to_owned(),
            namespace: None,
            primitive: unwrapped.primitive,
            version: unwrapped.node.version,
            flow_mapped_in_yaml: unwrapped.node.flow_mapped_in_yaml(),
            fields: Vec::new(),
            max_depth: 0,
        };
        leaf.set_max_depth();
        leaf
    };
    Some(TreeField {
        field_type,
        array_depth: unwrapped.array_depth,
        name: node.name.clone(),
        align: unwrapped.align_bytes,
    })
}

/// `TypeTreeResolver.TryGetSerializableType` with node building: the first matching
/// reference builds its `OldType` nodes into a `SerializableTreeType`. A missing
/// match reports `TYPE_REFERENCE_NOT_FOUND`; an absent or unbuildable tree reports
/// `TYPE_TREE_NODE_BUILD_FAILED`.
pub fn resolve_serializable_tree(
    references: &[SerializedTypeReference],
    tree: Option<&TypeTree>,
    script: &ScriptIdentifier,
) -> Result<SerializableTreeType, &'static str> {
    let _ = find_type_reference_index(references, script).ok_or(TYPE_REFERENCE_NOT_FOUND)?;
    let empty = TypeTree {
        nodes: Vec::new(),
        string_buffer: Vec::new(),
    };
    let nodes = tree.unwrap_or(&empty).nodes.as_slice();
    let root = build_type_nodes(nodes).ok_or(TYPE_TREE_NODE_BUILD_FAILED)?;
    SerializableTreeType::from_referenced_object_node(&root).ok_or(TYPE_TREE_NODE_BUILD_FAILED)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_rows(kind: &str) -> Vec<Vec<String>> {
        let fixture = include_str!("../io/script_identity_fixture.txt");
        let mut rows = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<String> = line.split('|').map(str::to_owned).collect();
            if parts.first().is_some_and(|first| first == kind) {
                rows.push(parts);
            }
        }
        rows
    }

    #[test]
    fn backend_discriminants_match_the_reference_runtime() {
        let mut rows = 0;
        for parts in fixture_rows("BACKEND") {
            assert_eq!(parts.len(), 3, "{parts:?}");
            let discriminant: i32 = parts[2].parse().unwrap();
            let backend = ScriptingBackend::from_discriminant(discriminant)
                .expect("a known backend discriminant");
            assert_eq!(
                match backend {
                    ScriptingBackend::Unknown => "Unknown",
                    ScriptingBackend::Mono => "Mono",
                    ScriptingBackend::IL2Cpp => "IL2Cpp",
                },
                parts[1],
                "{parts:?}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3, "all probed backend rows are asserted");
        assert_eq!(ScriptingBackend::from_discriminant(3), None);
        assert_eq!(ScriptingBackend::Unknown as i32, 0);
        assert_eq!(ScriptingBackend::Mono as i32, 1);
        assert_eq!(ScriptingBackend::IL2Cpp as i32, 2);
    }

    #[test]
    fn mono_assembly_predicate_matches_the_source_rule() {
        assert_eq!(ASSEMBLY_EXTENSION, ".dll");
        assert!(is_mono_assembly("Assembly-CSharp.dll"));
        assert!(is_mono_assembly("mscorlib.dll"));
        // Ordinal comparison: case, trailing dots, and other extensions do not match.
        assert!(!is_mono_assembly("Assembly-CSharp.DLL"));
        assert!(!is_mono_assembly("Assembly-CSharp.dll "));
        assert!(!is_mono_assembly("GameAssembly.so"));
        assert!(!is_mono_assembly("global-metadata.dat"));
        assert!(!is_mono_assembly(""));
        assert!(!is_mono_assembly(".dll.bak"));
    }

    #[test]
    fn mscorlib_discovery_prefers_the_exact_entry() {
        fn assemblies(names: &[(&str, &str)]) -> Vec<(String, String)> {
            names
                .iter()
                .map(|(name, path)| ((*name).to_owned(), (*path).to_owned()))
                .collect()
        }
        // The exact entry wins even when a case-variant comes first.
        let listed = assemblies(&[
            ("MSCorLib.DLL", "/lib/upper.dll"),
            ("mscorlib.dll", "/lib/exact.dll"),
        ]);
        assert_eq!(find_mscorlib_path(&listed), Some("/lib/exact.dll"));
        // Otherwise the first case-insensitive match in order wins.
        let listed = assemblies(&[
            ("MSCorLib.DLL", "/lib/first.dll"),
            ("Mscorlib.dll", "/lib/second.dll"),
        ]);
        assert_eq!(find_mscorlib_path(&listed), Some("/lib/first.dll"));
        // A lone exact entry matches, and an empty table matches nothing.
        let listed = assemblies(&[("mscorlib.dll", "/lib/only.dll")]);
        assert_eq!(find_mscorlib_path(&listed), Some("/lib/only.dll"));
        assert_eq!(find_mscorlib_path(&[]), None);
        // Near misses are not matches.
        let listed = assemblies(&[
            ("mscorlib.dll.bak", "/lib/bak.dll"),
            ("Assembly-CSharp.dll", "/lib/game.dll"),
        ]);
        assert_eq!(find_mscorlib_path(&listed), None);
    }

    #[test]
    fn identifier_names_match_the_reference_runtime() {
        let mut sid_rows = 0;
        for parts in fixture_rows("SID") {
            assert_eq!(parts.len(), 6, "{parts:?}");
            let id = ScriptIdentifier::try_new(&parts[1], &parts[2], &parts[3]).unwrap();
            assert!(!id.is_default(), "{parts:?}");
            assert_eq!(id.to_display(), parts[4], "{parts:?}");
            assert_eq!(id.unique_name(), parts[5], "{parts:?}");
            sid_rows += 1;
        }
        assert_eq!(sid_rows, 3, "all probed identifier rows are asserted");
        let mut unique3 = Vec::new();
        for parts in fixture_rows("UNIQ3") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            unique3.push(parts[1].clone());
        }
        assert_eq!(unique3, vec!["[A]N", "[A]G.N"], "probed overload outputs");
        assert_eq!(to_unique_name("A", "", "N"), "[A]N");
        assert_eq!(to_unique_name("A", "G", "N"), "[A]G.N");
        for parts in fixture_rows("UNIQ2") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            assert_eq!(parts[1], "[A]G.N", "{parts:?}");
        }
        assert_eq!(to_unique_name_full("A", "G.N"), "[A]G.N");
    }

    #[test]
    fn default_identifier_matches_the_reference_runtime() {
        for parts in fixture_rows("ISDEF") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            assert_eq!(parts[1], "1", "{parts:?}");
        }
        let def = ScriptIdentifier::default();
        assert!(def.is_default(), "the default value is default");
        for parts in fixture_rows("DEFTOSTRING") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            assert_eq!(def.to_display(), parts[1], "{parts:?}");
        }
        assert_eq!(def.to_display(), DEFAULT_SCRIPT_IDENTIFIER_DISPLAY);
    }

    #[test]
    fn constructor_rejections_match_the_reference_runtime() {
        let mut rows = 0;
        for parts in fixture_rows("THROW") {
            assert_eq!(parts.len(), 5, "{parts:?}");
            let namespace = if parts[2] == "<null>" {
                // Rust strings cannot be null; the source rejects a null
                // namespace with ArgumentNullException before any other check.
                // An empty namespace is the closest representable input and is
                // accepted, which is exactly the behavioral gap this documents.
                String::new()
            } else {
                parts[2].clone()
            };
            let result = ScriptIdentifier::try_new(&parts[1], &namespace, &parts[3]);
            match parts[1].is_empty() {
                true => assert_eq!(result, Err(ScriptIdError::EmptyAssembly), "{parts:?}"),
                false if parts[3].is_empty() => {
                    assert_eq!(result, Err(ScriptIdError::EmptyName), "{parts:?}")
                }
                false => assert!(result.is_ok(), "{parts:?}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 3, "all probed rejection rows are asserted");
    }

    #[test]
    fn full_and_assembly_names_follow_the_source_rules() {
        assert_eq!(get_full_name("", "Player"), "Player");
        assert_eq!(get_full_name("Game", "Player"), "Game.Player");
        assert_eq!(get_valid_assembly_name(""), "Assembly-CSharp");
        assert_eq!(get_valid_assembly_name("MyLib.dll"), "MyLib");
        assert_eq!(get_valid_assembly_name("CSharp"), "Assembly - CSharp");
    }

    #[test]
    fn assembly_name_presence_follows_release_and_field_gates() {
        // Release collections always have the name.
        assert!(has_assembly_name(true, false));
        assert!(has_assembly_name(true, true));
        // Otherwise only a non-release-only field is present.
        assert!(has_assembly_name(false, false));
        assert!(!has_assembly_name(false, true));
    }

    fn script_reference(
        class: Option<&str>,
        namespace: Option<&str>,
        assembly: Option<&str>,
    ) -> SerializedTypeReference {
        SerializedTypeReference {
            raw_type_id: 0,
            type_id_for_hash: 0,
            is_stripped_type: false,
            script_type_index: 0,
            script_id: None,
            old_type_hash: None,
            type_tree: None,
            class_name: class.map(str::to_owned),
            namespace: namespace.map(str::to_owned),
            assembly_name: assembly.map(str::to_owned),
        }
    }

    /// Differential against the shipped `AssetRipper.Import.dll` resolver
    /// (probe `Temp/opencode/scriptidprobe`, SDK 10.0.401): the `RESOLVE` rows pin
    /// both failure reasons. The `broken-tree` row proves the three-way match fired
    /// (a miss would report not-found), so the first test replays that match.
    #[test]
    fn type_reference_scan_matches_the_reference_runtime() {
        let mut rows = 0;
        for parts in fixture_rows("RESOLVE") {
            assert_eq!(parts.len(), 4, "{parts:?}");
            match parts[1].as_str() {
                "empty" => {
                    assert_eq!(parts[2], "0", "{parts:?}");
                    assert_eq!(parts[3], TYPE_REFERENCE_NOT_FOUND, "{parts:?}");
                }
                "broken-tree" => {
                    assert_eq!(parts[2], "0", "{parts:?}");
                    assert_eq!(parts[3], TYPE_TREE_NODE_BUILD_FAILED, "{parts:?}");
                }
                other => panic!("unexpected resolve case {other}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 2, "both probed resolve rows are asserted");
        // The probed broken-tree reference: Player/""/Assembly-CSharp.
        let script = ScriptIdentifier::try_new("Assembly-CSharp", "", "Player").unwrap();
        let references = vec![script_reference(
            Some("Player"),
            Some(""),
            Some("Assembly-CSharp"),
        )];
        assert_eq!(find_type_reference_index(&references, &script), Some(0));
        assert!(find_type_reference_index(&[], &script).is_none());
    }

    #[test]
    fn type_reference_match_covers_assembly_stripping_case_and_absence() {
        let script = ScriptIdentifier::try_new("Assembly-CSharp", "Game", "Player").unwrap();
        // The resolver strips the stored extension before comparing.
        assert!(type_reference_matches_script(
            &script_reference(Some("Player"), Some("Game"), Some("Assembly-CSharp.dll")),
            &script
        ));
        // Ordinal comparison: case differs, no match.
        assert!(!type_reference_matches_script(
            &script_reference(Some("player"), Some("Game"), Some("Assembly-CSharp")),
            &script
        ));
        // Absent Rust fields read as empty like Utf8String.Empty.
        let empty_ns = ScriptIdentifier::try_new("A", "", "N").unwrap();
        assert!(type_reference_matches_script(
            &script_reference(Some("N"), None, Some("A")),
            &empty_ns
        ));
        // A default identifier never matches, even against empty fields.
        assert!(!type_reference_matches_script(
            &script_reference(Some(""), Some(""), Some("")),
            &ScriptIdentifier::default()
        ));
        // First match wins in scan order.
        let first = script_reference(Some("Player"), Some("Game"), Some("Assembly-CSharp"));
        let references = vec![script_reference(Some("Other"), None, None), first];
        assert_eq!(find_type_reference_index(&references, &script), Some(1));
    }

    #[test]
    fn module_type_lookup_and_script_gates_follow_the_manager_rules() {
        // A null namespace reads empty; duplicates resolve first-wins in order.
        let types: [(Option<&str>, &str); 4] = [
            (Some("Game"), "Player"),
            (None, "Global"),
            (Some("Game"), "Player"),
            (Some(""), "EmptyNs"),
        ];
        assert_eq!(find_module_type_index(&types, "Game", "Player"), Some(0));
        assert_eq!(find_module_type_index(&types, "", "Global"), Some(1));
        assert_eq!(find_module_type_index(&types, "", "EmptyNs"), Some(3));
        assert_eq!(find_module_type_index(&types, "Game", "Missing"), None);
        assert_eq!(find_module_type_index(&types, "Other", "Player"), None);
        assert_eq!(find_module_type_index(&[], "Game", "Player"), None);
        // `IsPresent`: a set manager finds a non-default script.
        assert!(script_is_present(true, false, true));
        assert!(!script_is_present(false, false, true));
        assert!(!script_is_present(true, true, true));
        assert!(!script_is_present(true, false, false));
        // `IsValid` adds the abstract gate.
        assert!(script_is_valid(true, false, true, false));
        assert!(!script_is_valid(true, false, true, true));
        assert!(!script_is_valid(false, false, true, false));
        assert!(!script_is_valid(true, true, true, false));
        assert!(!script_is_valid(true, false, false, false));
    }

    #[test]
    fn properties_hash_fallback_expands_in_collection_byte_order() {
        assert_eq!(
            properties_hash_fallback_bytes(0x1234_5678, false),
            [0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            properties_hash_fallback_bytes(0x1234_5678, true),
            [0x12, 0x34, 0x56, 0x78, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(properties_hash_fallback_bytes(0, true), [0; 16]);
        assert_eq!(
            properties_hash_fallback_bytes(u32::MAX, false),
            [0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    /// Full-path differential for node building: the hand-built `NODESRC` trees in
    /// the fixture drive `resolve_serializable_tree`, and every rendered `NTTYPE` /
    /// `NTFIELD` row must match the shipped-resolver dump. This closes the
    /// `OldType` node-building half of `TypeTreeResolver.TryGetSerializableType`.
    #[test]
    fn serializable_trees_reproduce_the_reference_node_dump() {
        use crate::serialized_file::metadata::TypeTree;

        fn render_case(case: &str, built: &SerializableTreeType, out: &mut Vec<String>) {
            fn render_type(
                case: &str,
                path: &str,
                built: &SerializableTreeType,
                out: &mut Vec<String>,
            ) {
                out.push(format!(
                    "NTTYPE|{case}|{path}|{}|{}|{}|{}|{}",
                    built.full_name(),
                    built.primitive.as_str(),
                    built.version,
                    u8::from(built.flow_mapped_in_yaml),
                    built.max_depth,
                ));
                for field in &built.fields {
                    let field_path = if path.is_empty() {
                        field.name.clone()
                    } else {
                        format!("{path}.{}", field.name)
                    };
                    out.push(format!(
                        "NTFIELD|{case}|{field_path}|{}|{}|{}|{}",
                        field.name,
                        field.array_depth,
                        u8::from(field.align),
                        u8::from(field.is_array()),
                    ));
                    render_type(case, &field_path, &field.field_type, out);
                }
            }
            render_type(case, "", built, out);
        }

        let mut sources: std::collections::HashMap<String, Vec<TypeTreeNode>> =
            std::collections::HashMap::new();
        for parts in fixture_rows("NODESRC") {
            assert_eq!(parts.len(), 6, "{parts:?}");
            sources
                .entry(parts[1].clone())
                .or_default()
                .push(TypeTreeNode {
                    level: parts[4].parse().unwrap(),
                    version: 1,
                    type_flags: 0,
                    type_string_offset: None,
                    name_string_offset: None,
                    type_name: Some(parts[2].clone()),
                    name: Some(parts[3].clone()),
                    byte_size: 0,
                    index: 0,
                    meta_flags: parts[5].parse().unwrap(),
                    reference_type_hash: None,
                });
        }
        let script = ScriptIdentifier::try_new("Assembly-CSharp", "", "Player").unwrap();
        let references = vec![script_reference(
            Some("Player"),
            Some(""),
            Some("Assembly-CSharp"),
        )];
        let mut cases = 0;
        for parts in fixture_rows("NODETREE") {
            assert_eq!(parts.len(), 4, "{parts:?}");
            let case = parts[1].clone();
            let nodes = sources
                .remove(&case)
                .unwrap_or_else(|| panic!("no source {case}"));
            let tree = TypeTree {
                nodes,
                string_buffer: Vec::new(),
            };
            let built = resolve_serializable_tree(&references, Some(&tree), &script);
            assert_eq!(parts[2], "1", "{parts:?}");
            // The probe renders a null reason as `<null>`; the port yields `Ok`.
            assert!(parts[3].is_empty() || parts[3] == "<null>", "{parts:?}");
            let built = built.unwrap_or_else(|reason| panic!("{case} failed: {reason}"));
            let mut rendered = Vec::new();
            render_case(&case, &built, &mut rendered);
            let fixture = include_str!("../io/script_identity_fixture.txt");
            let mut expected = Vec::new();
            for line in fixture.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let row: Vec<&str> = line.split('|').collect();
                if (row[0] == "NTTYPE" || row[0] == "NTFIELD") && row[1] == case {
                    expected.push(line.to_owned());
                }
            }
            assert_eq!(rendered, expected, "case {case}");
            cases += 1;
        }
        assert!(sources.is_empty(), "every source case resolves");
        assert_eq!(cases, 6, "all probed node cases replay");
    }
}
