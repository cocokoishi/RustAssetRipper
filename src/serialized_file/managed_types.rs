//! Managed serialization types ported from `AssetRipper.SerializationLogic`.
//!
//! Only the assembly-independent parts are ported here; everything that reads Cecil metadata
//! stays open:
//!
//! - `PrimitiveType` with its `GetSize`, `IsCSharpPrimitive`, and `ToSystemTypeName` rules.
//!   Sizes are signed with -1 for variable-length kinds, exactly like the source.
//! - ECMA-335 element-type codes to `PrimitiveType` (`ToPrimitiveType`), with unmapped codes
//!   falling back to `Complex`. The codes are the CLI standard `ELEMENT_TYPE_*` values.
//! - `SerializableType.Field` shape (type, array depth, name, alignment flag) with the
//!   `IsArray` rule and the `Type[][] Name` display form.
//! - `MaxDepth` rules: primitives (including strings and PPtr shapes) have depth 0, arrays do
//!   not add depth, a type takes one plus the maximum field depth, and any unknown field depth
//!   sticks the whole type at -1.
//! - The `ManagedReferenceTypes` registry schema: every type, field, and array-depth constant
//!   plus the canonical field layouts the baseline normalizes type trees onto. Decoding registry
//!   payloads stays open; this records the schema our decoder must eventually accept.
//! - The `EngineTypePredicates` string tables with the direct-comparison rules
//!   (`IsSerializableUnityStruct` including the `LazyLoadReference` prefix, and the
//!   should-have-had-serializable value-type set). Assignability checks need a runtime type
//!   system and stay open.

/// One `PrimitiveType` variant of the baseline serialization logic.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrimitiveType {
    Void,
    Bool,
    Char,
    SByte,
    Byte,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    Single,
    Double,
    String,
    Pair,
    MapPair,
    Complex,
}

impl PrimitiveType {
    /// All variants in source declaration order.
    pub const ALL: [Self; 17] = [
        Self::Void,
        Self::Bool,
        Self::Char,
        Self::SByte,
        Self::Byte,
        Self::Short,
        Self::UShort,
        Self::Int,
        Self::UInt,
        Self::Long,
        Self::ULong,
        Self::Single,
        Self::Double,
        Self::String,
        Self::Pair,
        Self::MapPair,
        Self::Complex,
    ];

    /// `PrimitiveTypeExtensions.GetSize`: byte size, or -1 for variable-length kinds.
    pub const fn size(self) -> i32 {
        match self {
            Self::Void => 0,
            Self::Bool | Self::Byte | Self::SByte => 1,
            Self::Char | Self::Short | Self::UShort => 2,
            Self::Int | Self::UInt | Self::Single => 4,
            Self::Long | Self::ULong | Self::Double => 8,
            Self::Pair | Self::MapPair | Self::String | Self::Complex => -1,
        }
    }

    /// `PrimitiveTypeExtensions.IsCSharpPrimitive`: strings and every positive-size kind.
    pub const fn is_csharp_primitive(self) -> bool {
        match self {
            Self::String => true,
            _ => self.size() > 0,
        }
    }

    /// `PrimitiveTypeExtensions.ToSystemTypeName`; `None` for kinds the source rejects.
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
            _ => None,
        }
    }
}

/// ECMA-335 `ELEMENT_TYPE_*` codes mapped by `ToPrimitiveType`.
pub mod element_type {
    use super::PrimitiveType;

    pub const BOOLEAN: u8 = 0x02;
    pub const CHAR: u8 = 0x03;
    pub const I1: u8 = 0x04;
    pub const U1: u8 = 0x05;
    pub const I2: u8 = 0x06;
    pub const U2: u8 = 0x07;
    pub const I4: u8 = 0x08;
    pub const U4: u8 = 0x09;
    pub const I8: u8 = 0x0A;
    pub const U8: u8 = 0x0B;
    pub const R4: u8 = 0x0C;
    pub const R8: u8 = 0x0D;
    pub const STRING: u8 = 0x0E;

    /// Map one element-type code, falling back to `Complex` like the source default arm.
    pub const fn to_primitive_type(code: u8) -> PrimitiveType {
        match code {
            BOOLEAN => PrimitiveType::Bool,
            CHAR => PrimitiveType::Char,
            I1 => PrimitiveType::SByte,
            U1 => PrimitiveType::Byte,
            I2 => PrimitiveType::Short,
            U2 => PrimitiveType::UShort,
            I4 => PrimitiveType::Int,
            U4 => PrimitiveType::UInt,
            I8 => PrimitiveType::Long,
            U8 => PrimitiveType::ULong,
            R4 => PrimitiveType::Single,
            R8 => PrimitiveType::Double,
            STRING => PrimitiveType::String,
            _ => PrimitiveType::Complex,
        }
    }
}

/// One `SerializableType.Field`: its type name, array depth, field name, and alignment flag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldShape {
    pub type_name: &'static str,
    pub array_depth: usize,
    pub name: &'static str,
    pub align: bool,
}

impl FieldShape {
    /// `Field.IsArray`: exactly one array level.
    pub const fn is_array(self_array_depth: usize) -> bool {
        self_array_depth == 1
    }

    /// The `Type[][] Name` display form of `Field.ToString`.
    pub fn display(&self) -> String {
        let mut text = String::with_capacity(
            self.type_name.len() + self.array_depth * 2 + 1 + self.name.len(),
        );
        text.push_str(self.type_name);
        for _ in 0..self.array_depth {
            text.push_str("[]");
        }
        text.push(' ');
        text.push_str(self.name);
        text
    }
}

/// `ManagedReferenceTypes` name constants for the `[SerializeReference]` registry schema.
pub mod managed_reference {
    pub const MANAGED_REFERENCE_NAME: &str = "managedReference";
    pub const MANAGED_REFERENCE_ARRAY_ITEM_NAME: &str = "managedRefArrayItem";
    pub const REGISTRY_NAME: &str = "ManagedReferencesRegistry";
    pub const REFERENCED_OBJECT_NAME: &str = "ReferencedObject";
    pub const REFERENCED_MANAGED_TYPE_NAME: &str = "ReferencedManagedType";
    pub const REFERENCED_OBJECT_DATA_NAME: &str = "ReferencedObjectData";
    pub const REGISTRY_FIELD_NAME: &str = "references";
    pub const VERSION_FIELD_NAME: &str = "version";
    pub const REFERENCE_IDS_FIELD_NAME: &str = "RefIds";
    pub const REFERENCE_ID_FIELD_NAME: &str = "rid";
    pub const INDEX_FIELD_NAME: &str = "id";
    pub const TYPE_FIELD_NAME: &str = "type";
    pub const DATA_FIELD_NAME: &str = "data";
    pub const CLASS_FIELD_NAME: &str = "class";
    pub const NAMESPACE_FIELD_NAME: &str = "ns";
    pub const ASSEMBLY_FIELD_NAME: &str = "asm";
}

/// One canonical managed-reference type shape with its field layout in source order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ManagedReferenceShape {
    pub type_name: &'static str,
    pub flow_mapped_in_yaml: bool,
    pub fields: &'static [FieldShape],
}

/// The six canonical shapes of `ManagedReferenceTypes` in source declaration order.
pub const MANAGED_REFERENCE_SHAPES: &[ManagedReferenceShape] = &[
    ManagedReferenceShape {
        type_name: managed_reference::MANAGED_REFERENCE_NAME,
        flow_mapped_in_yaml: false,
        fields: &[FieldShape {
            type_name: "Long",
            array_depth: 0,
            name: managed_reference::REFERENCE_ID_FIELD_NAME,
            align: false,
        }],
    },
    ManagedReferenceShape {
        type_name: managed_reference::MANAGED_REFERENCE_NAME,
        flow_mapped_in_yaml: false,
        fields: &[FieldShape {
            type_name: "Int",
            array_depth: 0,
            name: managed_reference::INDEX_FIELD_NAME,
            align: false,
        }],
    },
    ManagedReferenceShape {
        type_name: managed_reference::REFERENCED_MANAGED_TYPE_NAME,
        flow_mapped_in_yaml: true,
        fields: &[
            FieldShape {
                type_name: "String",
                array_depth: 0,
                name: managed_reference::CLASS_FIELD_NAME,
                align: false,
            },
            FieldShape {
                type_name: "String",
                array_depth: 0,
                name: managed_reference::NAMESPACE_FIELD_NAME,
                align: false,
            },
            FieldShape {
                type_name: "String",
                array_depth: 0,
                name: managed_reference::ASSEMBLY_FIELD_NAME,
                align: false,
            },
        ],
    },
    ManagedReferenceShape {
        type_name: managed_reference::REFERENCED_OBJECT_DATA_NAME,
        flow_mapped_in_yaml: false,
        fields: &[],
    },
    ManagedReferenceShape {
        type_name: managed_reference::REFERENCED_OBJECT_NAME,
        flow_mapped_in_yaml: false,
        fields: &[
            FieldShape {
                type_name: "Long",
                array_depth: 0,
                name: managed_reference::REFERENCE_ID_FIELD_NAME,
                align: false,
            },
            FieldShape {
                type_name: managed_reference::REFERENCED_MANAGED_TYPE_NAME,
                array_depth: 0,
                name: managed_reference::TYPE_FIELD_NAME,
                align: false,
            },
            FieldShape {
                type_name: managed_reference::REFERENCED_OBJECT_DATA_NAME,
                array_depth: 0,
                name: managed_reference::DATA_FIELD_NAME,
                align: false,
            },
        ],
    },
    ManagedReferenceShape {
        type_name: managed_reference::REGISTRY_NAME,
        flow_mapped_in_yaml: false,
        fields: &[
            FieldShape {
                type_name: "Int",
                array_depth: 0,
                name: managed_reference::VERSION_FIELD_NAME,
                align: false,
            },
            FieldShape {
                type_name: managed_reference::REFERENCED_OBJECT_NAME,
                array_depth: 1,
                name: managed_reference::REFERENCE_IDS_FIELD_NAME,
                align: true,
            },
        ],
    },
];

/// Unity value types that should have had the serializable attribute.
pub const UNITY_VALUE_TYPES_NEEDING_SERIALIZABLE: [&str; 14] = [
    "Vector3",
    "Vector2",
    "Vector4",
    "Rect",
    "RectInt",
    "Quaternion",
    "Matrix4x4",
    "Color",
    "Color32",
    "LayerMask",
    "Bounds",
    "BoundsInt",
    "Vector3Int",
    "Vector2Int",
];

/// Unity classes treated as serializable by name.
pub const SERIALIZABLE_UNITY_CLASSES: [&str; 4] =
    ["AnimationCurve", "Gradient", "GUIStyle", "RectOffset"];

/// Unity structs treated as serializable by full name.
pub const SERIALIZABLE_UNITY_STRUCTS: [&str; 4] = [
    "UnityEngine.Color32",
    "UnityEngine.Matrix4x4",
    "UnityEngine.Rendering.SphericalHarmonicsL2",
    "UnityEngine.PropertyName",
];

/// The `LazyLoadReference`1` prefix that is always serializable.
pub const LAZY_LOAD_REFERENCE_PREFIX: &str = "UnityEngine.LazyLoadReference`1";

/// `EngineTypePredicates.IsSerializableUnityStruct` for one full type name.
///
/// Structs cannot inherit, so the check is a direct comparison plus the lazy-load prefix.
pub fn is_serializable_unity_struct(full_name: &str) -> bool {
    SERIALIZABLE_UNITY_STRUCTS.contains(&full_name)
        || full_name.starts_with(LAZY_LOAD_REFERENCE_PREFIX)
}

/// `EngineTypePredicates.IsUnityEngineValueType` for a namespace and type name pair.
pub fn is_unity_engine_value_type(namespace: &str, name: &str) -> bool {
    namespace == "UnityEngine" && UNITY_VALUE_TYPES_NEEDING_SERIALIZABLE.contains(&name)
}

/// `MonoType.SetDepth` over already-resolved field depths.
///
/// `None` marks an unknown field depth and sticks the result at -1; otherwise the result is
/// one plus the maximum field depth, or 0 when there are no fields.
pub fn max_depth_of(field_depths: &[Option<i32>]) -> i32 {
    let mut max_depth = 0;
    for depth in field_depths {
        match depth {
            Some(depth) => max_depth = max_depth.max(depth + 1),
            None => return -1,
        }
    }
    max_depth
}

#[cfg(test)]
mod tests {
    use super::{
        element_type, is_serializable_unity_struct, is_unity_engine_value_type, managed_reference,
        max_depth_of, FieldShape, PrimitiveType, MANAGED_REFERENCE_SHAPES,
        SERIALIZABLE_UNITY_STRUCTS, UNITY_VALUE_TYPES_NEEDING_SERIALIZABLE,
    };

    #[test]
    fn primitive_sizes_and_names_match_the_source_tables() {
        assert_eq!(PrimitiveType::ALL.len(), 17);
        for (primitive, size) in [
            (PrimitiveType::Void, 0),
            (PrimitiveType::Bool, 1),
            (PrimitiveType::Byte, 1),
            (PrimitiveType::SByte, 1),
            (PrimitiveType::Char, 2),
            (PrimitiveType::Short, 2),
            (PrimitiveType::UShort, 2),
            (PrimitiveType::Int, 4),
            (PrimitiveType::UInt, 4),
            (PrimitiveType::Single, 4),
            (PrimitiveType::Long, 8),
            (PrimitiveType::ULong, 8),
            (PrimitiveType::Double, 8),
            (PrimitiveType::Pair, -1),
            (PrimitiveType::MapPair, -1),
            (PrimitiveType::String, -1),
            (PrimitiveType::Complex, -1),
        ] {
            assert_eq!(primitive.size(), size, "{primitive:?}");
        }
        assert!(PrimitiveType::String.is_csharp_primitive());
        assert!(PrimitiveType::Int.is_csharp_primitive());
        assert!(!PrimitiveType::Void.is_csharp_primitive());
        assert!(!PrimitiveType::Pair.is_csharp_primitive());
        assert!(!PrimitiveType::Complex.is_csharp_primitive());
        assert_eq!(PrimitiveType::Short.system_type_name(), Some("Int16"));
        assert_eq!(PrimitiveType::Long.system_type_name(), Some("Int64"));
        assert_eq!(PrimitiveType::String.system_type_name(), Some("String"));
        assert_eq!(PrimitiveType::Void.system_type_name(), None);
        assert_eq!(PrimitiveType::Pair.system_type_name(), None);
    }

    #[test]
    fn element_type_codes_map_with_complex_fallback() {
        assert_eq!(element_type::to_primitive_type(0x02), PrimitiveType::Bool);
        assert_eq!(element_type::to_primitive_type(0x04), PrimitiveType::SByte);
        assert_eq!(element_type::to_primitive_type(0x08), PrimitiveType::Int);
        assert_eq!(element_type::to_primitive_type(0x0C), PrimitiveType::Single);
        assert_eq!(element_type::to_primitive_type(0x0D), PrimitiveType::Double);
        assert_eq!(element_type::to_primitive_type(0x0E), PrimitiveType::String);
        assert_eq!(
            element_type::to_primitive_type(element_type::BOOLEAN),
            PrimitiveType::Bool
        );
        assert_eq!(
            element_type::to_primitive_type(element_type::STRING),
            PrimitiveType::String
        );
        // Unmapped CLI codes fall back to Complex like the source default arm.
        assert_eq!(
            element_type::to_primitive_type(0x00),
            PrimitiveType::Complex
        );
        assert_eq!(
            element_type::to_primitive_type(0x1C),
            PrimitiveType::Complex
        );
        assert_eq!(
            element_type::to_primitive_type(0xFF),
            PrimitiveType::Complex
        );
    }

    #[test]
    fn field_shapes_follow_array_and_display_rules() {
        assert!(FieldShape::is_array(1));
        assert!(!FieldShape::is_array(0));
        assert!(!FieldShape::is_array(2));
        let field = FieldShape {
            type_name: "ReferencedObject",
            array_depth: 1,
            name: "RefIds",
            align: true,
        };
        assert_eq!(field.display(), "ReferencedObject[] RefIds");
        let scalar = FieldShape {
            type_name: "Int",
            array_depth: 0,
            name: "version",
            align: false,
        };
        assert_eq!(scalar.display(), "Int version");
    }

    #[test]
    fn managed_reference_schema_matches_the_baseline_layouts() {
        assert_eq!(
            managed_reference::MANAGED_REFERENCE_NAME,
            "managedReference"
        );
        assert_eq!(
            managed_reference::MANAGED_REFERENCE_ARRAY_ITEM_NAME,
            "managedRefArrayItem"
        );
        assert_eq!(
            managed_reference::REGISTRY_NAME,
            "ManagedReferencesRegistry"
        );
        assert_eq!(managed_reference::REGISTRY_FIELD_NAME, "references");
        assert_eq!(MANAGED_REFERENCE_SHAPES.len(), 6);
        let registry = MANAGED_REFERENCE_SHAPES
            .iter()
            .find(|shape| shape.type_name == managed_reference::REGISTRY_NAME)
            .unwrap();
        assert!(!registry.flow_mapped_in_yaml);
        assert_eq!(registry.fields.len(), 2);
        assert_eq!(
            registry.fields[0].name,
            managed_reference::VERSION_FIELD_NAME
        );
        assert_eq!(registry.fields[0].type_name, "Int");
        assert_eq!(
            registry.fields[1].name,
            managed_reference::REFERENCE_IDS_FIELD_NAME
        );
        assert_eq!(registry.fields[1].array_depth, 1);
        assert!(registry.fields[1].align);
        let managed_type = MANAGED_REFERENCE_SHAPES
            .iter()
            .find(|shape| shape.type_name == managed_reference::REFERENCED_MANAGED_TYPE_NAME)
            .unwrap();
        assert!(managed_type.flow_mapped_in_yaml);
        assert_eq!(
            managed_type
                .fields
                .iter()
                .map(|field| field.name)
                .collect::<Vec<_>>(),
            ["class", "ns", "asm"]
        );
    }

    #[test]
    fn engine_predicate_tables_match_the_source_sets() {
        assert_eq!(UNITY_VALUE_TYPES_NEEDING_SERIALIZABLE.len(), 14);
        assert!(is_unity_engine_value_type("UnityEngine", "Quaternion"));
        assert!(is_unity_engine_value_type("UnityEngine", "Vector2Int"));
        assert!(!is_unity_engine_value_type("UnityEngine", "GameObject"));
        assert!(!is_unity_engine_value_type("System", "Quaternion"));
        assert!(!is_unity_engine_value_type("", "Vector3"));
        assert_eq!(SERIALIZABLE_UNITY_STRUCTS.len(), 4);
        for full_name in SERIALIZABLE_UNITY_STRUCTS {
            assert!(is_serializable_unity_struct(full_name), "{full_name}");
        }
        assert!(is_serializable_unity_struct(
            "UnityEngine.LazyLoadReference`1[[UnityEngine.Texture2D]]"
        ));
        assert!(!is_serializable_unity_struct("UnityEngine.Vector3"));
        assert!(!is_serializable_unity_struct("UnityEngine.Gradient"));
    }

    #[test]
    fn max_depth_follows_set_depth_rules() {
        assert_eq!(max_depth_of(&[]), 0);
        assert_eq!(max_depth_of(&[Some(0)]), 1);
        assert_eq!(max_depth_of(&[Some(0), Some(2)]), 3);
        // Arrays do not add depth: callers pass the element depth through.
        assert_eq!(max_depth_of(&[Some(1), None]), -1);
        assert_eq!(max_depth_of(&[None]), -1);
    }
}
