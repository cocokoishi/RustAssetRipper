//! Managed type classification ported from `AssetRipper.SerializationLogic.MonoUtils`
//! plus the `GameAssetFactory.CreateEngineAsset` name table.
//!
//! Covers the pure string kernels behind script/type resolution: the namespace+name
//! predicates (`IsPrimitive`, `IsObject`, `IsEngineStruct`, ...), `ToPrimitiveType`
//! with its `PrimitiveType` discriminants, the CorLib element-type branch, the
//! `GetSize`/`IsCSharpPrimitive` serialization tables, and the
//! engine-asset name switch (which arm takes a Unity version and which concrete
//! subclass 2019.4.0f1 resolves). `ToSystemTypeName` stays open: it is naming-only
//! (no classification decision).
//! Assembly loading, type resolution, and object construction stay outside: they need
//! live managed assemblies. The `ToPrimitiveType` enum branch takes only their outcomes
//! (whether the definition resolved to an enum, plus each field's resolved type).
//!
//! Differential evidence: `mono_utils_fixture.txt` (probe `Temp/opencode/monoutilsprobe`
//! against the shipped `AssetRipper.SerializationLogic.dll` and `AssetRipper.Import.dll`,
//! SDK 10.0.401). Predicate inputs are `(namespace, name)` pairs with absent values
//! reading as unmatched, mirroring the source null comparisons.

/// The `System` namespace constant.
pub const SYSTEM_NAMESPACE: &str = "System";
/// The `System.Collections.Generic` namespace constant.
pub const SYSTEM_COLLECTIONS_GENERIC_NAMESPACE: &str = "System.Collections.Generic";
/// The `UnityEngine` namespace constant.
pub const UNITY_ENGINE_NAMESPACE: &str = "UnityEngine";
/// The `UnityEngine.Rendering` namespace constant.
pub const UNITY_ENGINE_RENDERING_NAMESPACE: &str = "UnityEngine.Rendering";
/// The backing-field name `MonoUtils.ToPrimitiveType` scans enum definitions for.
pub const ENUM_VALUE_FIELD_NAME: &str = "value__";

/// `MonoUtils.IsPrimitive`: a `System` primitive by long or alias name.
pub fn is_primitive(namespace: Option<&str>, name: Option<&str>) -> bool {
    if namespace != Some(SYSTEM_NAMESPACE) {
        return false;
    }
    matches!(
        name,
        Some(
            "Void"
                | "void"
                | "Boolean"
                | "bool"
                | "SByte"
                | "sbyte"
                | "Byte"
                | "byte"
                | "Char"
                | "char"
                | "Int16"
                | "short"
                | "UInt16"
                | "ushort"
                | "Int32"
                | "int"
                | "UInt32"
                | "uint"
                | "Int64"
                | "long"
                | "UInt64"
                | "ulong"
                | "Single"
                | "float"
                | "Double"
                | "double"
        )
    )
}

/// `MonoUtils.IsObject`: `System.Object`/`object`.
pub fn is_object(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(SYSTEM_NAMESPACE) && matches!(name, Some("Object" | "object"))
}

/// `MonoUtils.IsList`: `System.Collections.Generic.List\`1`.
pub fn is_list(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(SYSTEM_COLLECTIONS_GENERIC_NAMESPACE) && name == Some("List`1")
}

/// `MonoUtils.IsEngineObject`: `UnityEngine.Object`.
pub fn is_engine_object(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(UNITY_ENGINE_NAMESPACE) && name == Some("Object")
}

/// `MonoUtils.IsScriptableObject`.
pub fn is_scriptable_object(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(UNITY_ENGINE_NAMESPACE) && name == Some("ScriptableObject")
}

/// `MonoUtils.IsComponent`.
pub fn is_component(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(UNITY_ENGINE_NAMESPACE) && name == Some("Component")
}

/// `MonoUtils.IsBehaviour`.
pub fn is_behaviour(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(UNITY_ENGINE_NAMESPACE) && name == Some("Behaviour")
}

/// `MonoUtils.IsMonoBehaviour`.
pub fn is_mono_behaviour(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(UNITY_ENGINE_NAMESPACE) && name == Some("MonoBehaviour")
}

/// `MonoUtils.IsEngineStruct`: the engine value types, plus `SphericalHarmonicsL2`
/// from `UnityEngine.Rendering`.
pub fn is_engine_struct(namespace: Option<&str>, name: Option<&str>) -> bool {
    if namespace == Some(UNITY_ENGINE_NAMESPACE) {
        matches!(
            name,
            Some(
                "GUID"
                    | "Hash128"
                    | "Vector2"
                    | "Vector2Int"
                    | "Vector3"
                    | "Vector3Int"
                    | "Vector4"
                    | "Rect"
                    | "Bounds"
                    | "BoundsInt"
                    | "Quaternion"
                    | "Matrix4x4"
                    | "Color"
                    | "Color32"
                    | "LayerMask"
                    | "AnimationCurve"
                    | "Gradient"
                    | "RectOffset"
                    | "GUIStyle"
                    | "PropertyName"
            )
        )
    } else if namespace == Some(UNITY_ENGINE_RENDERING_NAMESPACE) {
        name == Some("SphericalHarmonicsL2")
    } else {
        false
    }
}

/// `MonoUtils.IsExposedReference`: `UnityEngine.ExposedReference\`1`.
pub fn is_exposed_reference(namespace: Option<&str>, name: Option<&str>) -> bool {
    namespace == Some(UNITY_ENGINE_NAMESPACE) && name == Some("ExposedReference`1")
}

/// `MonoUtils.IsPrime`: objects and Mono primes.
pub fn is_prime(namespace: Option<&str>, name: Option<&str>) -> bool {
    is_object(namespace, name) || is_mono_prime(namespace, name)
}

/// `MonoUtils.IsMonoPrime`: MonoBehaviour/Behaviour/Component/EngineObject.
pub fn is_mono_prime(namespace: Option<&str>, name: Option<&str>) -> bool {
    is_mono_behaviour(namespace, name)
        || is_behaviour(namespace, name)
        || is_component(namespace, name)
        || is_engine_object(namespace, name)
}

/// `MonoUtils.IsBuiltinGeneric`: lists and exposed references.
pub fn is_builtin_generic(namespace: Option<&str>, name: Option<&str>) -> bool {
    is_list(namespace, name) || is_exposed_reference(namespace, name)
}

/// The `PrimitiveType` classification with source discriminants.
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
    /// The source enum discriminant.
    pub const fn discriminant(self) -> i32 {
        match self {
            Self::Void => 0,
            Self::Bool => 1,
            Self::Char => 2,
            Self::SByte => 3,
            Self::Byte => 4,
            Self::Short => 5,
            Self::UShort => 6,
            Self::Int => 7,
            Self::UInt => 8,
            Self::Long => 9,
            Self::ULong => 10,
            Self::Single => 11,
            Self::Double => 12,
            Self::String => 13,
            Self::Pair => 14,
            Self::MapPair => 15,
            Self::Complex => 16,
        }
    }

    /// Parse the probed member spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Void" => Some(Self::Void),
            "Bool" => Some(Self::Bool),
            "Char" => Some(Self::Char),
            "SByte" => Some(Self::SByte),
            "Byte" => Some(Self::Byte),
            "Short" => Some(Self::Short),
            "UShort" => Some(Self::UShort),
            "Int" => Some(Self::Int),
            "UInt" => Some(Self::UInt),
            "Long" => Some(Self::Long),
            "ULong" => Some(Self::ULong),
            "Single" => Some(Self::Single),
            "Double" => Some(Self::Double),
            "String" => Some(Self::String),
            "Pair" => Some(Self::Pair),
            "MapPair" => Some(Self::MapPair),
            "Complex" => Some(Self::Complex),
            _ => None,
        }
    }

    /// `PrimitiveTypeExtensions.GetSize`: the serialized size in bytes. Fixed-size
    /// kinds report their width; pairs, strings, and complex kinds report -1 like
    /// the source. Exhaustive over the enum, so the source `default: throw` arm has
    /// no reachable case here.
    pub const fn byte_size(self) -> i32 {
        match self {
            Self::Void => 0,
            Self::Bool | Self::Byte | Self::SByte => 1,
            Self::Char | Self::Short | Self::UShort => 2,
            Self::Int | Self::UInt | Self::Single => 4,
            Self::Long | Self::ULong | Self::Double => 8,
            Self::Pair | Self::MapPair | Self::String | Self::Complex => -1,
        }
    }

    /// `PrimitiveTypeExtensions.IsCSharpPrimitive`: strings and every fixed-size
    /// kind. This is the production caller behind the size table: field
    /// serialization dispatches on this predicate, not on raw sizes.
    pub const fn is_c_sharp_primitive(self) -> bool {
        matches!(self, Self::String) || self.byte_size() > 0
    }
}

/// `MonoUtils.ToPrimitiveType(namespace, name)`: only the long `System` names map;
/// C# aliases fall through to `Complex`.
pub fn to_primitive_type(namespace: Option<&str>, name: Option<&str>) -> PrimitiveType {
    if namespace == Some(SYSTEM_NAMESPACE) {
        match name {
            Some("Void") => PrimitiveType::Void,
            Some("Boolean") => PrimitiveType::Bool,
            Some("Char") => PrimitiveType::Char,
            Some("SByte") => PrimitiveType::SByte,
            Some("Byte") => PrimitiveType::Byte,
            Some("Int16") => PrimitiveType::Short,
            Some("UInt16") => PrimitiveType::UShort,
            Some("Int32") => PrimitiveType::Int,
            Some("UInt32") => PrimitiveType::UInt,
            Some("Int64") => PrimitiveType::Long,
            Some("UInt64") => PrimitiveType::ULong,
            Some("Single") => PrimitiveType::Single,
            Some("Double") => PrimitiveType::Double,
            Some("String") => PrimitiveType::String,
            _ => PrimitiveType::Complex,
        }
    } else {
        PrimitiveType::Complex
    }
}

/// The ECMA-335 element types behind `CorLibTypeSignature.ElementType`, restricted
/// to the spellings `PrimitiveTypeExtensions.ToPrimitiveType` classifies.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MonoElementType {
    Boolean,
    Char,
    I1,
    U1,
    I2,
    U2,
    I4,
    U4,
    I8,
    U8,
    R4,
    R8,
    String,
    /// Any other element type (including `Void`, pointers, and managed composites):
    /// the source `_ => Complex` default arm.
    Other,
}

/// `PrimitiveTypeExtensions.ToPrimitiveType(ElementType)`: the CorLib signature fast
/// path that field serialization tries before name resolution. Signature decoding
/// stays caller-side; only the decoded element type arrives here.
pub const fn element_type_to_primitive(element: MonoElementType) -> PrimitiveType {
    match element {
        MonoElementType::Boolean => PrimitiveType::Bool,
        MonoElementType::Char => PrimitiveType::Char,
        MonoElementType::I1 => PrimitiveType::SByte,
        MonoElementType::U1 => PrimitiveType::Byte,
        MonoElementType::I2 => PrimitiveType::Short,
        MonoElementType::U2 => PrimitiveType::UShort,
        MonoElementType::I4 => PrimitiveType::Int,
        MonoElementType::U4 => PrimitiveType::UInt,
        MonoElementType::I8 => PrimitiveType::Long,
        MonoElementType::U8 => PrimitiveType::ULong,
        MonoElementType::R4 => PrimitiveType::Single,
        MonoElementType::R8 => PrimitiveType::Double,
        MonoElementType::String => PrimitiveType::String,
        MonoElementType::Other => PrimitiveType::Complex,
    }
}

/// One enum backing-field fact for the `ToPrimitiveType` enum branch: the field name
/// plus the resolved field-type identity, which is absent when the signature, the
/// field type, or its resolution is missing (each null link in the source chain).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnumBackingField<'a> {
    pub name: &'a str,
    pub type_namespace: Option<&'a str>,
    pub type_name: Option<&'a str>,
}

/// The enum branch of `MonoUtils.ToPrimitiveType(ITypeDefOrRef?, RuntimeContext?)`
/// over caller facts: when the resolved definition is an enum, the first `value__`
/// field's type replaces the enum type for classification; a missing field (or a
/// non-enum/absent definition) keeps the original namespace+name, which the caller
/// feeds to `to_primitive_type`. Assembly loading, signature reads, and type
/// resolution stay caller-side; only their outcomes arrive here, in field order.
pub fn enum_backing_type_for_primitive<'a>(
    is_enum: bool,
    fields: &[EnumBackingField<'a>],
    namespace: Option<&'a str>,
    name: Option<&'a str>,
) -> (Option<&'a str>, Option<&'a str>) {
    if !is_enum {
        return (namespace, name);
    }
    match fields
        .iter()
        .find(|field| field.name == ENUM_VALUE_FIELD_NAME)
    {
        Some(field) => (field.type_namespace, field.type_name),
        None => (namespace, name),
    }
}

/// One `GameAssetFactory.CreateEngineAsset` arm.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EngineAssetKind {
    Guid,
    Hash128,
    Vector2,
    Vector2Int,
    Vector3,
    Vector3Int,
    Vector4,
    Rect,
    Bounds,
    BoundsInt,
    Quaternion,
    Matrix4x4,
    Color,
    Color32,
    LayerMask,
    AnimationCurve,
    Gradient,
    RectOffset,
    GuiStyle,
    PropertyName,
    SphericalHarmonicsL2,
}

impl EngineAssetKind {
    /// Whether the source constructor takes the collection Unity version.
    pub const fn takes_version(self) -> bool {
        matches!(
            self,
            Self::Hash128
                | Self::AnimationCurve
                | Self::Gradient
                | Self::GuiStyle
                | Self::PropertyName
                | Self::SphericalHarmonicsL2
        )
    }

    /// The concrete subclass the probe resolved at 2019.4.0f1.
    pub const fn concrete_class_2019_4(self) -> &'static str {
        match self {
            Self::Guid => "AssetRipper.SourceGenerated.Subclasses.GUID.GUID",
            Self::Hash128 => "AssetRipper.SourceGenerated.Subclasses.Hash128.Hash128_5",
            Self::Vector2 => "AssetRipper.SourceGenerated.Subclasses.Vector2f.Vector2f",
            Self::Vector2Int => {
                "AssetRipper.SourceGenerated.Subclasses.Vector2Int.Vector2Int"
            }
            Self::Vector3 => "AssetRipper.SourceGenerated.Subclasses.Vector3f.Vector3f",
            Self::Vector3Int => {
                "AssetRipper.SourceGenerated.Subclasses.Vector3Int.Vector3Int"
            }
            Self::Vector4 => "AssetRipper.SourceGenerated.Subclasses.Vector4f.Vector4f",
            Self::Rect => "AssetRipper.SourceGenerated.Subclasses.Rectf.Rectf",
            Self::Bounds => "AssetRipper.SourceGenerated.Subclasses.AABB.AABB",
            Self::BoundsInt => "AssetRipper.SourceGenerated.Subclasses.AABBInt.AABBInt",
            Self::Quaternion => {
                "AssetRipper.SourceGenerated.Subclasses.Quaternionf.Quaternionf"
            }
            Self::Matrix4x4 => {
                "AssetRipper.SourceGenerated.Subclasses.Matrix4x4f.Matrix4x4f"
            }
            Self::Color => "AssetRipper.SourceGenerated.Subclasses.ColorRGBAf.ColorRGBAf",
            Self::Color32 => "AssetRipper.SourceGenerated.Subclasses.ColorRGBA32.ColorRGBA32",
            Self::LayerMask => "AssetRipper.SourceGenerated.Subclasses.LayerMask.LayerMask",
            Self::AnimationCurve => "AssetRipper.SourceGenerated.Subclasses.AnimationCurve_Single.AnimationCurve_Single_2018",
            Self::Gradient => "AssetRipper.SourceGenerated.Subclasses.Gradient.Gradient_5_6",
            Self::RectOffset => {
                "AssetRipper.SourceGenerated.Subclasses.RectOffset.RectOffset"
            }
            Self::GuiStyle => {
                "AssetRipper.SourceGenerated.Subclasses.GUIStyle.GUIStyle_2017_1"
            }
            Self::PropertyName => {
                "AssetRipper.SourceGenerated.Subclasses.PropertyName.PropertyName_2019"
            }
            Self::SphericalHarmonicsL2 => "AssetRipper.SourceGenerated.Subclasses.SphericalHarmonicsL2.SphericalHarmonicsL2_5",
        }
    }
}

/// The `CreateEngineAsset` name switch: unknown names throw `NotSupportedException`.
pub fn engine_asset_kind(name: &str) -> Option<EngineAssetKind> {
    match name {
        "GUID" => Some(EngineAssetKind::Guid),
        "Hash128" => Some(EngineAssetKind::Hash128),
        "Vector2" => Some(EngineAssetKind::Vector2),
        "Vector2Int" => Some(EngineAssetKind::Vector2Int),
        "Vector3" => Some(EngineAssetKind::Vector3),
        "Vector3Int" => Some(EngineAssetKind::Vector3Int),
        "Vector4" => Some(EngineAssetKind::Vector4),
        "Rect" => Some(EngineAssetKind::Rect),
        "Bounds" => Some(EngineAssetKind::Bounds),
        "BoundsInt" => Some(EngineAssetKind::BoundsInt),
        "Quaternion" => Some(EngineAssetKind::Quaternion),
        "Matrix4x4" => Some(EngineAssetKind::Matrix4x4),
        "Color" => Some(EngineAssetKind::Color),
        "Color32" => Some(EngineAssetKind::Color32),
        "LayerMask" => Some(EngineAssetKind::LayerMask),
        "AnimationCurve" => Some(EngineAssetKind::AnimationCurve),
        "Gradient" => Some(EngineAssetKind::Gradient),
        "RectOffset" => Some(EngineAssetKind::RectOffset),
        "GUIStyle" => Some(EngineAssetKind::GuiStyle),
        "PropertyName" => Some(EngineAssetKind::PropertyName),
        "SphericalHarmonicsL2" => Some(EngineAssetKind::SphericalHarmonicsL2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn predicate_bits(namespace: Option<&str>, name: Option<&str>) -> String {
        let bits = [
            is_primitive(namespace, name),
            is_object(namespace, name),
            is_list(namespace, name),
            is_engine_object(namespace, name),
            is_scriptable_object(namespace, name),
            is_component(namespace, name),
            is_behaviour(namespace, name),
            is_mono_behaviour(namespace, name),
            is_engine_struct(namespace, name),
            is_exposed_reference(namespace, name),
            is_prime(namespace, name),
            is_mono_prime(namespace, name),
            is_builtin_generic(namespace, name),
        ];
        bits.iter()
            .map(|bit| if *bit { '1' } else { '0' })
            .collect()
    }

    fn fixture_lines() -> Vec<String> {
        let fixture = include_str!("mono_utils_fixture.txt");
        fixture
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn predicates_match_the_reference_runtime() {
        let mut rows = 0;
        for line in fixture_lines() {
            let mut parts = line.split('|');
            if parts.next() != Some("PRED") {
                continue;
            }
            let namespace = parts.next().expect("a namespace");
            let name = parts.next().expect("a name");
            let expected = parts.next().expect("a bitstring");
            assert!(parts.next().is_none(), "{line}");
            assert_eq!(
                predicate_bits(Some(namespace), Some(name)),
                expected,
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 469, "all probed predicate rows are asserted");
        // Absent values never match, mirroring the null comparisons.
        assert_eq!(predicate_bits(None, Some("int")), "0000000000000");
        assert_eq!(
            predicate_bits(Some(SYSTEM_NAMESPACE), None),
            "0000000000000"
        );
    }

    #[test]
    fn primitive_types_match_the_reference_runtime() {
        let mut enums = 0;
        let mut mappings = 0;
        for line in fixture_lines() {
            let mut parts = line.split('|');
            match parts.next() {
                Some("ENUM") => {
                    assert_eq!(parts.next(), Some("PrimitiveType"));
                    let member = parts.next().expect("a member");
                    let discriminant: i32 = parts.next().expect("a discriminant").parse().unwrap();
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(
                        PrimitiveType::from_name(member).map(|kind| kind.discriminant()),
                        Some(discriminant),
                        "{line}"
                    );
                    enums += 1;
                }
                Some("TOPRIM") => {
                    let namespace = parts.next().expect("a namespace");
                    let name = parts.next().expect("a name");
                    let expected = parts.next().expect("a type");
                    assert!(parts.next().is_none(), "{line}");
                    let actual = to_primitive_type(Some(namespace), Some(name));
                    let spelling = match actual {
                        PrimitiveType::Void => "Void",
                        PrimitiveType::Bool => "Bool",
                        PrimitiveType::Char => "Char",
                        PrimitiveType::SByte => "SByte",
                        PrimitiveType::Byte => "Byte",
                        PrimitiveType::Short => "Short",
                        PrimitiveType::UShort => "UShort",
                        PrimitiveType::Int => "Int",
                        PrimitiveType::UInt => "UInt",
                        PrimitiveType::Long => "Long",
                        PrimitiveType::ULong => "ULong",
                        PrimitiveType::Single => "Single",
                        PrimitiveType::Double => "Double",
                        PrimitiveType::String => "String",
                        PrimitiveType::Pair => "Pair",
                        PrimitiveType::MapPair => "MapPair",
                        PrimitiveType::Complex => "Complex",
                    };
                    assert_eq!(spelling, expected, "{line}");
                    mappings += 1;
                }
                _ => {}
            }
        }
        assert_eq!(enums, 17, "all probed enum rows are asserted");
        assert_eq!(mappings, 268, "all probed mapping rows are asserted");
    }

    #[test]
    fn engine_asset_names_match_the_reference_runtime() {
        let mut rows = 0;
        for line in fixture_lines() {
            let mut parts = line.split('|');
            if parts.next() != Some("ENGINE") {
                continue;
            }
            let name = parts.next().expect("a name");
            let outcome = parts.next().expect("an outcome");
            let detail = parts.next().expect("a detail");
            assert!(parts.next().is_none(), "{line}");
            match engine_asset_kind(name) {
                Some(kind) => {
                    assert_eq!(outcome, "ok", "{line}");
                    assert_eq!(kind.concrete_class_2019_4(), detail, "{line}");
                }
                None => assert_eq!(outcome, "NotSupportedException", "{line}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 23, "all probed engine rows are asserted");
        // The version-taking arms are exactly the versioned factory calls.
        assert!(EngineAssetKind::Hash128.takes_version());
        assert!(EngineAssetKind::Gradient.takes_version());
        assert!(!EngineAssetKind::Vector3.takes_version());
        assert!(!EngineAssetKind::Guid.takes_version());
    }

    #[test]
    fn primitive_sizes_and_csharp_coverage_match_the_source_tables() {
        use super::PrimitiveType;
        assert_eq!(PrimitiveType::Void.byte_size(), 0);
        for kind in [
            PrimitiveType::Bool,
            PrimitiveType::Byte,
            PrimitiveType::SByte,
        ] {
            assert_eq!(kind.byte_size(), 1, "{kind:?}");
            assert!(kind.is_c_sharp_primitive(), "{kind:?}");
        }
        for kind in [
            PrimitiveType::Char,
            PrimitiveType::Short,
            PrimitiveType::UShort,
        ] {
            assert_eq!(kind.byte_size(), 2, "{kind:?}");
            assert!(kind.is_c_sharp_primitive(), "{kind:?}");
        }
        for kind in [
            PrimitiveType::Int,
            PrimitiveType::UInt,
            PrimitiveType::Single,
        ] {
            assert_eq!(kind.byte_size(), 4, "{kind:?}");
            assert!(kind.is_c_sharp_primitive(), "{kind:?}");
        }
        for kind in [
            PrimitiveType::Long,
            PrimitiveType::ULong,
            PrimitiveType::Double,
        ] {
            assert_eq!(kind.byte_size(), 8, "{kind:?}");
            assert!(kind.is_c_sharp_primitive(), "{kind:?}");
        }
        // Strings read primitive despite their variable size; pairs, void, and
        // complex kinds do not.
        assert_eq!(PrimitiveType::String.byte_size(), -1);
        assert!(PrimitiveType::String.is_c_sharp_primitive());
        for kind in [
            PrimitiveType::Void,
            PrimitiveType::Pair,
            PrimitiveType::MapPair,
            PrimitiveType::Complex,
        ] {
            assert!(!kind.is_c_sharp_primitive(), "{kind:?}");
        }
        assert_eq!(PrimitiveType::Pair.byte_size(), -1);
        assert_eq!(PrimitiveType::MapPair.byte_size(), -1);
        assert_eq!(PrimitiveType::Complex.byte_size(), -1);
    }

    #[test]
    fn element_types_classify_like_the_corlib_signature_branch() {
        use super::MonoElementType;
        use super::PrimitiveType;
        let cases = [
            (MonoElementType::Boolean, PrimitiveType::Bool),
            (MonoElementType::Char, PrimitiveType::Char),
            (MonoElementType::I1, PrimitiveType::SByte),
            (MonoElementType::U1, PrimitiveType::Byte),
            (MonoElementType::I2, PrimitiveType::Short),
            (MonoElementType::U2, PrimitiveType::UShort),
            (MonoElementType::I4, PrimitiveType::Int),
            (MonoElementType::U4, PrimitiveType::UInt),
            (MonoElementType::I8, PrimitiveType::Long),
            (MonoElementType::U8, PrimitiveType::ULong),
            (MonoElementType::R4, PrimitiveType::Single),
            (MonoElementType::R8, PrimitiveType::Double),
            (MonoElementType::String, PrimitiveType::String),
            // Every other element type falls through to `Complex`.
            (MonoElementType::Other, PrimitiveType::Complex),
        ];
        for (element, expected) in cases {
            assert_eq!(super::element_type_to_primitive(element), expected);
        }
        // The fast path agrees with name resolution on the overlapping primitives.
        assert_eq!(
            super::element_type_to_primitive(MonoElementType::I4),
            super::to_primitive_type(Some(super::SYSTEM_NAMESPACE), Some("Int32"))
        );
    }

    #[test]
    fn enum_backing_fields_replace_only_resolved_enum_types() {
        use super::EnumBackingField;
        let backing = [EnumBackingField {
            name: super::ENUM_VALUE_FIELD_NAME,
            type_namespace: Some(super::SYSTEM_NAMESPACE),
            type_name: Some("Int32"),
        }];
        // An enum classifies as its backing field's type.
        let (namespace, name) =
            super::enum_backing_type_for_primitive(true, &backing, Some("Game"), Some("MyEnum"));
        assert_eq!((namespace, name), (Some("System"), Some("Int32")));
        assert_eq!(
            super::to_primitive_type(namespace, name),
            super::PrimitiveType::Int
        );
        // A non-enum keeps its own identity even when `value__` fields are reported.
        let (namespace, name) =
            super::enum_backing_type_for_primitive(false, &backing, Some("Game"), Some("MyEnum"));
        assert_eq!((namespace, name), (Some("Game"), Some("MyEnum")));
        assert_eq!(
            super::to_primitive_type(namespace, name),
            super::PrimitiveType::Complex
        );
        // The first `value__` field wins; later fields are ignored like the source break.
        let two = [
            EnumBackingField {
                name: super::ENUM_VALUE_FIELD_NAME,
                type_namespace: Some(super::SYSTEM_NAMESPACE),
                type_name: Some("Boolean"),
            },
            EnumBackingField {
                name: super::ENUM_VALUE_FIELD_NAME,
                type_namespace: Some(super::SYSTEM_NAMESPACE),
                type_name: Some("Int64"),
            },
        ];
        let (_, name) =
            super::enum_backing_type_for_primitive(true, &two, Some("Game"), Some("MyEnum"));
        assert_eq!(name, Some("Boolean"));
        // An enum without a `value__` field keeps the enum identity (which then
        // classifies `Complex` unless the enum itself names a primitive).
        let other = [EnumBackingField {
            name: "other",
            type_namespace: Some(super::SYSTEM_NAMESPACE),
            type_name: Some("Int32"),
        }];
        let (namespace, name) =
            super::enum_backing_type_for_primitive(true, &other, Some("Game"), Some("MyEnum"));
        assert_eq!((namespace, name), (Some("Game"), Some("MyEnum")));
        // A `value__` field with missing type facts (null signature/type/resolution
        // links) classifies `Complex`, like the source null chain.
        let missing = [EnumBackingField {
            name: super::ENUM_VALUE_FIELD_NAME,
            type_namespace: None,
            type_name: None,
        }];
        let (namespace, name) =
            super::enum_backing_type_for_primitive(true, &missing, Some("Game"), Some("MyEnum"));
        assert_eq!((namespace, name), (None, None));
        assert_eq!(
            super::to_primitive_type(namespace, name),
            super::PrimitiveType::Complex
        );
        // Absent types stay absent.
        assert_eq!(
            super::enum_backing_type_for_primitive(false, &[], None, None),
            (None, None)
        );
    }
}
