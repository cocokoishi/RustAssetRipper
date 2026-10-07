//! Custom animation-curve attribute names from `CustomCurveResolver.ToAttributeName`.
//!
//! Ports every data-driven branch of
//! `AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs`: the six
//! constraint property tables (selected by the low nibble of the attribute,
//! with indexed entries reading `attribute >> 8`), the `Renderer` array
//! element path, the `RendererMaterial` bit layout, the `SpriteRenderer` zero
//! rule, the `UnknownPathRegex` shape, the eleven component `TryGetPath` hash
//! tables, and the `Crc32Algorithm.ReverseAscii` manufactured fallbacks. The
//! top-level [`to_attribute_name`] resolves all of them over
//! [`CustomCurveSceneFacts`]: the `Roots`/`FindChild` walks, the component
//! lookups, and the CRC searches (`Mesh.FindBlendShapeNameByCRC`,
//! `Renderer.FindMaterialPropertyNameByCRC28`) need the asset collection and
//! stay caller-side, while every name assembled from the answers is a pure
//! function here.
//!
//! Source facts: `BindingCustomType` (`byte`, `None = 0` .. `MeshFilter = 41`) and
//! `TransformType` (`Translation = 1` .. `EulerRotation = 4`) with
//! `TransformTypeExtensions.GetDimension` live in the frozen snapshot under
//! `AssetRipper.SourceGenerated.Extensions/Enums/AnimationClip/GenericBinding/`.

use std::error::Error;
use std::fmt::{self, Display};

/// `BindingCustomType` discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BindingCustomType(pub u8);

impl BindingCustomType {
    /// No custom binding.
    pub const NONE: Self = Self(0);
    /// Transform curves.
    pub const TRANSFORM: Self = Self(4);
    /// Humanoid muscle curves.
    pub const ANIMATOR_MUSCLE: Self = Self(8);
    /// Blend-shape weight curves.
    pub const BLEND_SHAPE: Self = Self(20);
    /// Renderer element curves.
    pub const RENDERER: Self = Self(21);
    /// Renderer material property curves.
    pub const RENDERER_MATERIAL: Self = Self(22);
    /// Sprite curves.
    pub const SPRITE_RENDERER: Self = Self(23);
    /// Script property curves.
    pub const MONO_BEHAVIOUR: Self = Self(24);
    /// Light property curves.
    pub const LIGHT: Self = Self(25);
    /// Shadow property curves.
    pub const RENDERER_SHADOWS: Self = Self(26);
    /// Particle-system property curves.
    pub const PARTICLE_SYSTEM: Self = Self(27);
    /// Rect-transform property curves.
    pub const RECT_TRANSFORM: Self = Self(28);
    /// Line-renderer property curves.
    pub const LINE_RENDERER: Self = Self(29);
    /// Trail-renderer property curves.
    pub const TRAIL_RENDERER: Self = Self(30);
    /// Position-constraint property curves.
    pub const POSITION_CONSTRAINT: Self = Self(31);
    /// Rotation-constraint property curves.
    pub const ROTATION_CONSTRAINT: Self = Self(32);
    /// Scale-constraint property curves.
    pub const SCALE_CONSTRAINT: Self = Self(33);
    /// Aim-constraint property curves.
    pub const AIM_CONSTRAINT: Self = Self(34);
    /// Parent-constraint property curves.
    pub const PARENT_CONSTRAINT: Self = Self(35);
    /// Look-at-constraint property curves.
    pub const LOOK_AT_CONSTRAINT: Self = Self(36);
    /// Camera property curves.
    pub const CAMERA: Self = Self(37);
    /// Visual-effect property curves.
    pub const VISUAL_EFFECT: Self = Self(38);
    /// Particle-force-field property curves.
    pub const PARTICLE_FORCE_FIELD: Self = Self(39);
    /// User-defined property curves.
    pub const USER_DEFINED: Self = Self(40);
    /// Mesh-filter property curves.
    pub const MESH_FILTER: Self = Self(41);

    /// The source enum identifier (`None`, `Transform`, ..., `MeshFilter`).
    ///
    /// Mirrors C# enum formatting: defined discriminants render as their name,
    /// anything else as the decimal value.
    pub const fn name(self) -> Option<&'static str> {
        match self.0 {
            0 => Some("None"),
            4 => Some("Transform"),
            8 => Some("AnimatorMuscle"),
            20 => Some("BlendShape"),
            21 => Some("Renderer"),
            22 => Some("RendererMaterial"),
            23 => Some("SpriteRenderer"),
            24 => Some("MonoBehaviour"),
            25 => Some("Light"),
            26 => Some("RendererShadows"),
            27 => Some("ParticleSystem"),
            28 => Some("RectTransform"),
            29 => Some("LineRenderer"),
            30 => Some("TrailRenderer"),
            31 => Some("PositionConstraint"),
            32 => Some("RotationConstraint"),
            33 => Some("ScaleConstraint"),
            34 => Some("AimConstraint"),
            35 => Some("ParentConstraint"),
            36 => Some("LookAtConstraint"),
            37 => Some("Camera"),
            38 => Some("VisualEffect"),
            39 => Some("ParticleForceField"),
            40 => Some("UserDefined"),
            41 => Some("MeshFilter"),
            _ => None,
        }
    }

    /// Whether this discriminant names one of the six constraint tables below.
    pub const fn is_constraint(self) -> bool {
        matches!(self.0, 31..=36)
    }
}

/// `TransformType` discriminant.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TransformType(pub i32);

impl TransformType {
    /// Translation curves (3 dimensions).
    pub const TRANSLATION: Self = Self(1);
    /// Rotation curves (4 dimensions).
    pub const ROTATION: Self = Self(2);
    /// Scaling curves (3 dimensions).
    pub const SCALING: Self = Self(3);
    /// Euler-rotation curves (3 dimensions).
    pub const EULER_ROTATION: Self = Self(4);

    /// `TransformTypeExtensions.IsValid`.
    pub const fn is_valid(self) -> bool {
        matches!(self.0, 1..=4)
    }

    /// `TransformTypeExtensions.GetDimension`: 3 for translation/scaling/Euler, 4 for
    /// rotation. Anything else is an explicit error like the source
    /// `ArgumentOutOfRangeException`.
    pub const fn dimension(self) -> Result<usize, CustomCurveError> {
        match self.0 {
            1 | 3 | 4 => Ok(3),
            2 => Ok(4),
            other => Err(CustomCurveError::InvalidTransformType { value: other }),
        }
    }
}

/// A custom-curve attribute resolution failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CustomCurveError {
    /// The `TransformType` discriminant is outside 1..=4.
    InvalidTransformType {
        /// The offending discriminant.
        value: i32,
    },
    /// The attribute nibble selects no constraint property, or a `SpriteRenderer`
    /// attribute is nonzero. Mirrors the source `ArgumentException` throws.
    UnknownAttribute {
        /// The custom binding type.
        custom_type: u8,
        /// The offending attribute.
        attribute: u32,
    },
    /// The binding type has no data-driven branch here (scene or checksum lookups
    /// stay caller-side). Mirrors the source "not implemented" `ArgumentException`.
    UnsupportedCustomType {
        /// The custom binding type.
        custom_type: u8,
    },
    /// `ToAttributeName`'s default branch for enum values without a resolver.
    BindingTypeNotImplemented {
        /// The binding type discriminant.
        custom_type: u8,
    },
}

impl Display for CustomCurveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTransformType { value } => {
                write!(formatter, "invalid TransformType value {value}")
            }
            Self::UnknownAttribute {
                custom_type,
                attribute,
            } => {
                let custom = BindingCustomType(*custom_type);
                match custom.name() {
                    Some(name) => write!(
                        formatter,
                        "Unknown attribute 0x{attribute:X} ({attribute}) for {name}"
                    ),
                    None => write!(
                        formatter,
                        "Unknown attribute 0x{attribute:X} ({attribute}) for {custom_type}"
                    ),
                }
            }
            Self::UnsupportedCustomType { custom_type } => write!(
                formatter,
                "custom binding type {custom_type} needs scene or checksum lookups"
            ),
            Self::BindingTypeNotImplemented { custom_type } => {
                let custom = BindingCustomType(*custom_type);
                match custom.name() {
                    Some(name) => write!(
                        formatter,
                        "Binding type {name} not implemented (Parameter 'type')"
                    ),
                    None => write!(
                        formatter,
                        "Binding type {custom_type} not implemented (Parameter 'type')"
                    ),
                }
            }
        }
    }
}

impl Error for CustomCurveError {}

/// `AnimationClipConverter.UnknownPathPrefix`.
pub const UNKNOWN_PATH_PREFIX: &str = "path_";
/// `AnimationClipConverter.MissedPropertyPrefix`.
pub const MISSED_PROPERTY_PREFIX: &str = "missed_";
/// `AnimationClipConverter.ScriptPropertyPrefix`.
pub const SCRIPT_PROPERTY_PREFIX: &str = "script_";
/// `AnimationClipConverter.TypeTreePropertyPrefix`.
pub const TYPE_TREE_PROPERTY_PREFIX: &str = "typetree_";
/// The `BlendShape` attribute prefix.
pub const BLEND_SHAPE_PREFIX: &str = "blendShape.";
/// The `RendererMaterial` attribute prefix.
pub const MATERIAL_PREFIX: &str = "material.";

/// `CustomCurveResolver` `Renderer` branch: `m_Materials.Array.data[{attribute}]`.
pub fn renderer_attribute_path(attribute: u32) -> String {
    format!("m_Materials.Array.data[{attribute}]")
}

/// `CustomCurveResolver` `SpriteRenderer` branch: attribute `0` is `m_Sprite`;
/// anything else throws in the source.
pub fn sprite_renderer_attribute_path(attribute: u32) -> Result<&'static str, CustomCurveError> {
    if attribute == 0 {
        Ok("m_Sprite")
    } else {
        Err(CustomCurveError::UnknownAttribute {
            custom_type: BindingCustomType::SPRITE_RENDERER.0,
            attribute,
        })
    }
}

/// Which sub-property a `RendererMaterial` attribute addresses.
///
/// Bit 31 set means the material property itself; otherwise bits 28-29 select the
/// channel (`x`/`y`/`z`/`w`, or `r`/`g`/`b`/`a` when bit 30 marks RGBA data).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RendererMaterialSelector {
    /// The whole property.
    Direct,
    /// One channel of the property.
    Channel {
        /// Whether the property holds RGBA data (bit 30).
        is_rgba: bool,
        /// The channel letter.
        channel: char,
    },
}

/// Splits a `RendererMaterial` attribute into its CRC-28 property hash and selector.
///
/// The low 28 bits are the material-property CRC the caller resolves (for example
/// through `Renderer.FindMaterialPropertyNameByCRC28`); the top nibble is decoded
/// here.
pub fn split_renderer_material_attribute(attribute: u32) -> (u32, RendererMaterialSelector) {
    let crc28 = attribute & 0x0FFF_FFFF;
    if attribute & 0x8000_0000 != 0 {
        return (crc28, RendererMaterialSelector::Direct);
    }
    let is_rgba = attribute & 0x4000_0000 != 0;
    let channel = match (attribute >> 28) & 3 {
        0 => {
            if is_rgba {
                'r'
            } else {
                'x'
            }
        }
        1 => {
            if is_rgba {
                'g'
            } else {
                'y'
            }
        }
        2 => {
            if is_rgba {
                'b'
            } else {
                'z'
            }
        }
        _ => {
            if is_rgba {
                'a'
            } else {
                'w'
            }
        }
    };
    (
        crc28,
        RendererMaterialSelector::Channel { is_rgba, channel },
    )
}

/// Assembles a resolved `RendererMaterial` attribute name: `material.{property}` or
/// `material.{property}.{channel}`.
pub fn renderer_material_attribute_name(property: &str, attribute: u32) -> String {
    match split_renderer_material_attribute(attribute).1 {
        RendererMaterialSelector::Direct => format!("{MATERIAL_PREFIX}{property}"),
        RendererMaterialSelector::Channel { channel, .. } => {
            format!("{MATERIAL_PREFIX}{property}.{channel}")
        }
    }
}

/// The six constraint property tables of `CustomCurveResolver.ToAttributeName`.
///
/// `attribute & 0xF` selects the property; indexed entries (`m_Sources`,
/// `m_TranslationOffsets`, `m_RotationOffsets`) read the source index from
/// `attribute >> 8`. Anything else is the source `ArgumentException`.
pub fn constraint_attribute_name(
    custom_type: BindingCustomType,
    attribute: u32,
) -> Result<String, CustomCurveError> {
    let property = attribute & 0xF;
    let source_index = attribute >> 8;
    match custom_type {
        BindingCustomType::POSITION_CONSTRAINT => Ok(match property {
            0 => "m_RestTranslation.x".to_owned(),
            1 => "m_RestTranslation.y".to_owned(),
            2 => "m_RestTranslation.z".to_owned(),
            3 => "m_Weight".to_owned(),
            4 => "m_TranslationOffset.x".to_owned(),
            5 => "m_TranslationOffset.y".to_owned(),
            6 => "m_TranslationOffset.z".to_owned(),
            7 => "m_AffectTranslationX".to_owned(),
            8 => "m_AffectTranslationY".to_owned(),
            9 => "m_AffectTranslationZ".to_owned(),
            10 => "m_Active".to_owned(),
            11 => format!("m_Sources.Array.data[{source_index}].sourceTransform"),
            12 => format!("m_Sources.Array.data[{source_index}].weight"),
            _ => {
                return Err(CustomCurveError::UnknownAttribute {
                    custom_type: custom_type.0,
                    attribute,
                });
            }
        }),
        BindingCustomType::ROTATION_CONSTRAINT => Ok(match property {
            0 => "m_RestRotation.x".to_owned(),
            1 => "m_RestRotation.y".to_owned(),
            2 => "m_RestRotation.z".to_owned(),
            3 => "m_Weight".to_owned(),
            4 => "m_RotationOffset.x".to_owned(),
            5 => "m_RotationOffset.y".to_owned(),
            6 => "m_RotationOffset.z".to_owned(),
            7 => "m_AffectRotationX".to_owned(),
            8 => "m_AffectRotationY".to_owned(),
            9 => "m_AffectRotationZ".to_owned(),
            10 => "m_Active".to_owned(),
            11 => format!("m_Sources.Array.data[{source_index}].sourceTransform"),
            12 => format!("m_Sources.Array.data[{source_index}].weight"),
            _ => {
                return Err(CustomCurveError::UnknownAttribute {
                    custom_type: custom_type.0,
                    attribute,
                });
            }
        }),
        BindingCustomType::SCALE_CONSTRAINT => Ok(match property {
            0 => "m_ScaleAtRest.x".to_owned(),
            1 => "m_ScaleAtRest.y".to_owned(),
            2 => "m_ScaleAtRest.z".to_owned(),
            3 => "m_Weight".to_owned(),
            4 => "m_ScalingOffset.x".to_owned(),
            5 => "m_ScalingOffset.y".to_owned(),
            6 => "m_ScalingOffset.z".to_owned(),
            7 => "m_AffectScalingX".to_owned(),
            8 => "m_AffectScalingY".to_owned(),
            9 => "m_AffectScalingZ".to_owned(),
            10 => "m_Active".to_owned(),
            11 => format!("m_Sources.Array.data[{source_index}].sourceTransform"),
            12 => format!("m_Sources.Array.data[{source_index}].weight"),
            _ => {
                return Err(CustomCurveError::UnknownAttribute {
                    custom_type: custom_type.0,
                    attribute,
                });
            }
        }),
        BindingCustomType::AIM_CONSTRAINT => Ok(match property {
            0 => "m_Weight".to_owned(),
            1 => "m_AffectRotationX".to_owned(),
            2 => "m_AffectRotationY".to_owned(),
            3 => "m_AffectRotationZ".to_owned(),
            4 => "m_Active".to_owned(),
            5 => "m_WorldUpObject".to_owned(),
            6 => format!("m_Sources.Array.data[{source_index}].sourceTransform"),
            7 => format!("m_Sources.Array.data[{source_index}].weight"),
            _ => {
                return Err(CustomCurveError::UnknownAttribute {
                    custom_type: custom_type.0,
                    attribute,
                });
            }
        }),
        BindingCustomType::PARENT_CONSTRAINT => Ok(match property {
            0 => "m_Weight".to_owned(),
            1 => "m_AffectTranslationX".to_owned(),
            2 => "m_AffectTranslationY".to_owned(),
            3 => "m_AffectTranslationZ".to_owned(),
            4 => "m_AffectRotationX".to_owned(),
            5 => "m_AffectRotationY".to_owned(),
            6 => "m_AffectRotationZ".to_owned(),
            7 => "m_Active".to_owned(),
            8 => format!("m_TranslationOffsets.Array.data[{source_index}].x"),
            9 => format!("m_TranslationOffsets.Array.data[{source_index}].y"),
            10 => format!("m_TranslationOffsets.Array.data[{source_index}].z"),
            11 => format!("m_RotationOffsets.Array.data[{source_index}].x"),
            12 => format!("m_RotationOffsets.Array.data[{source_index}].y"),
            13 => format!("m_RotationOffsets.Array.data[{source_index}].z"),
            14 => format!("m_Sources.Array.data[{source_index}].sourceTransform"),
            15 => format!("m_Sources.Array.data[{source_index}].weight"),
            _ => {
                return Err(CustomCurveError::UnknownAttribute {
                    custom_type: custom_type.0,
                    attribute,
                });
            }
        }),
        BindingCustomType::LOOK_AT_CONSTRAINT => Ok(match property {
            0 => "m_Weight".to_owned(),
            1 => "m_Active".to_owned(),
            2 => "m_WorldUpObject".to_owned(),
            3 => format!("m_Sources.Array.data[{source_index}].sourceTransform"),
            4 => format!("m_Sources.Array.data[{source_index}].weight"),
            5 => "m_Roll".to_owned(),
            _ => {
                return Err(CustomCurveError::UnknownAttribute {
                    custom_type: custom_type.0,
                    attribute,
                });
            }
        }),
        _ => Err(CustomCurveError::UnsupportedCustomType {
            custom_type: custom_type.0,
        }),
    }
}

/// `CustomCurveResolver.UnknownPathRegex`:
/// `^path_0x[0-9A-F]{1,10}_[H-Wh-w]{6}[HJLN]$`.
///
/// Implemented without a regex dependency: the literal `path_0x` prefix, one to ten
/// uppercase hex digits, an underscore, six characters in `H-W`/`h-w`, and a final
/// character in `HJLN`.
pub fn is_unknown_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    const PREFIX: &[u8] = b"path_0x";
    if !bytes.starts_with(PREFIX) {
        return false;
    }
    let mut index = PREFIX.len();
    let mut hex_digits = 0;
    while index < bytes.len() && bytes[index] != b'_' {
        if !bytes[index].is_ascii_hexdigit() || bytes[index].is_ascii_lowercase() {
            return false;
        }
        hex_digits += 1;
        if hex_digits > 10 {
            return false;
        }
        index += 1;
    }
    if hex_digits == 0 || index >= bytes.len() {
        return false;
    }
    // Skip the underscore; exactly seven characters must remain.
    index += 1;
    if bytes.len() - index != 7 {
        return false;
    }
    for position in 0..6 {
        let byte = bytes[index + position];
        let in_upper = (b'H'..=b'W').contains(&byte);
        let in_lower = (b'h'..=b'w').contains(&byte);
        if !in_upper && !in_lower {
            return false;
        }
    }
    matches!(bytes[index + 6], b'H' | b'J' | b'L' | b'N')
}

/// `Crc32Algorithm.ReverseAscii`: the manufactured fallback attribute name.
///
/// `ReverseAscii(prefix, attribute)` returns the prefix with seven derived
/// characters appended (or the seven characters alone for an empty prefix).
///
/// Algorithm recovered instruction-for-instruction from the shipped
/// `AssetRipper.Checksum.dll` (`Crc32Algorithm.ReverseAscii(uint,
/// ReadOnlySpan<char>)` delegates to the private
/// `Reverse(uint, ReadOnlySpan<char>, uint)` with `HashAscii(prefix)`; the
/// disassembly was read with a reflection IL dump, probe
/// `Temp/opencode/ildump`):
/// `GetMaskFromBit(value, bit)` is `~((value >> (bit & 31)) & 1) + 1`, i.e.
/// `0xFFFF_FFFF` when the bit is set and `0` otherwise. `Reverse` xors the
/// attribute with the prefix hash, then runs 32 rounds
/// `mask = GetMaskFromBit(state, bit); state ^= mask & constant`, keeping each
/// mask. A fixed back-substitution xors the masks down from bit 29 to bit 0,
/// and six groups of five masks plus two leftover masks each xor into `'H'`
/// (`72`): bits 0/1/2/3-4/5 of the letter for the first six characters (the
/// `[H-Wh-w]{6}` of `UnknownPathRegex`), bits 1/2 for the last (the `[HJLN]`).
/// An empty prefix returns the seven characters alone, otherwise the prefix is
/// prepended (both arms are pinned live).
///
/// The source calls `HashAscii(prefix, "HHHHHHH")`, so its seven-H marker is appended to
/// the prefix before hashing. This uses the shared reflected CRC-32 update
/// routine without allocating a concatenated buffer. All resolver call sites
/// pass ASCII prefixes, so callers must too.
pub fn reverse_ascii_crc32(prefix: &str, attribute: u32) -> String {
    /// `(bit, constant)` of each of the 31 state-mixing rounds, in order; the
    /// 32nd mask (bit 31) is recorded without a further mix.
    const ROUNDS: [(u32, i32); 31] = [
        (2, -1_502_147_660),
        (0, -1_751_183_063),
        (1, 1_674_589_498),
        (7, -1_011_956_864),
        (3, 1_774_858_792),
        (4, -1_820_394_736),
        (5, 1_127_435_296),
        (9, 606_026_240),
        (6, -192_328_384),
        (8, -82_759_424),
        (10, -711_824_384),
        (11, -379_230_208),
        (14, 734_150_656),
        (12, -268_292_096),
        (13, -1_349_181_440),
        (19, 1_624_768_512),
        (15, 1_609_007_104),
        (18, 1_858_338_816),
        (17, -1_179_516_928),
        (20, 596_639_744),
        (21, -1_944_059_904),
        (24, 1_392_508_928),
        (30, 1_073_741_824),
        (27, -1_744_830_464),
        (16, -1_275_002_880),
        (25, 570_425_344),
        (22, 71_303_168),
        (28, 268_435_456),
        (23, 545_259_520),
        (26, -1_543_503_872),
        (29, -1_610_612_736),
    ];
    let state = super::blend_shape::crc32_ieee_update(0xFFFF_FFFF, prefix.as_bytes());
    let prefix_hash = !super::blend_shape::crc32_ieee_update(state, b"HHHHHHH");
    let mut state = attribute ^ prefix_hash;
    let mut mask = [0u32; 32];
    for (index, (bit, constant)) in ROUNDS.iter().enumerate() {
        let bit_set = (state >> (bit & 31)) & 1;
        let round_mask = (!bit_set).wrapping_add(1);
        state ^= round_mask & (*constant as u32);
        mask[index] = round_mask;
    }
    mask[31] = (!((state >> 31) & 1)).wrapping_add(1);
    let v = &mut mask;
    v[29] ^= v[30];
    v[28] ^= v[29];
    v[27] ^= v[30];
    v[26] ^= v[29];
    v[25] ^= v[26] ^ v[27] ^ v[28] ^ v[31];
    v[24] ^= v[25] ^ v[26] ^ v[29] ^ v[31];
    v[23] ^= v[24] ^ v[25] ^ v[27] ^ v[28] ^ v[29] ^ v[30];
    v[22] ^= v[23] ^ v[24] ^ v[26] ^ v[28] ^ v[29] ^ v[30] ^ v[31];
    v[21] ^= v[22] ^ v[25] ^ v[31];
    v[20] ^= v[22] ^ v[28] ^ v[31];
    v[19] ^= v[21] ^ v[24] ^ v[26] ^ v[29] ^ v[31];
    v[18] ^= v[19] ^ v[22] ^ v[25] ^ v[26] ^ v[29] ^ v[31];
    v[17] ^= v[20] ^ v[23] ^ v[26] ^ v[28] ^ v[31];
    v[16] ^= v[17] ^ v[18] ^ v[19] ^ v[22] ^ v[24] ^ v[27];
    v[15] ^= v[18] ^ v[20] ^ v[22] ^ v[24] ^ v[25] ^ v[26] ^ v[28];
    v[14] ^= v[16] ^ v[19] ^ v[20] ^ v[23] ^ v[25] ^ v[28];
    v[13] ^= v[14] ^ v[16] ^ v[17] ^ v[18] ^ v[19] ^ v[22] ^ v[23] ^ v[25] ^ v[28] ^ v[29] ^ v[31];
    v[12] ^= v[13] ^ v[14] ^ v[19] ^ v[21] ^ v[23] ^ v[25] ^ v[30];
    v[11] ^= v[12] ^ v[13] ^ v[18] ^ v[19] ^ v[21] ^ v[23] ^ v[24] ^ v[25] ^ v[28] ^ v[31];
    v[10] ^= v[13] ^ v[18] ^ v[19] ^ v[20] ^ v[22] ^ v[24] ^ v[25] ^ v[26] ^ v[29] ^ v[30];
    v[9] ^= v[10] ^ v[17] ^ v[18] ^ v[19] ^ v[20] ^ v[25] ^ v[26] ^ v[28] ^ v[29];
    v[8] ^= v[11]
        ^ v[12]
        ^ v[13]
        ^ v[15]
        ^ v[17]
        ^ v[18]
        ^ v[21]
        ^ v[22]
        ^ v[23]
        ^ v[24]
        ^ v[25]
        ^ v[28];
    v[7] ^= v[8] ^ v[11] ^ v[13] ^ v[15] ^ v[16] ^ v[23] ^ v[25] ^ v[27] ^ v[28] ^ v[29] ^ v[31];
    v[6] ^= v[7]
        ^ v[10]
        ^ v[11]
        ^ v[14]
        ^ v[16]
        ^ v[17]
        ^ v[18]
        ^ v[19]
        ^ v[21]
        ^ v[23]
        ^ v[24]
        ^ v[27]
        ^ v[28]
        ^ v[29]
        ^ v[30]
        ^ v[31];
    v[5] ^= v[6]
        ^ v[7]
        ^ v[8]
        ^ v[9]
        ^ v[10]
        ^ v[13]
        ^ v[14]
        ^ v[15]
        ^ v[16]
        ^ v[17]
        ^ v[18]
        ^ v[19]
        ^ v[20]
        ^ v[26]
        ^ v[27]
        ^ v[28]
        ^ v[29]
        ^ v[30]
        ^ v[31];
    v[4] ^= v[5]
        ^ v[6]
        ^ v[7]
        ^ v[8]
        ^ v[9]
        ^ v[15]
        ^ v[16]
        ^ v[22]
        ^ v[23]
        ^ v[25]
        ^ v[26]
        ^ v[29]
        ^ v[30];
    v[3] ^= v[7]
        ^ v[9]
        ^ v[10]
        ^ v[12]
        ^ v[14]
        ^ v[15]
        ^ v[16]
        ^ v[17]
        ^ v[19]
        ^ v[20]
        ^ v[21]
        ^ v[23]
        ^ v[24]
        ^ v[26]
        ^ v[27]
        ^ v[29]
        ^ v[30];
    v[2] ^=
        v[6] ^ v[7] ^ v[8] ^ v[9] ^ v[13] ^ v[16] ^ v[17] ^ v[19] ^ v[20] ^ v[21] ^ v[26] ^ v[29];
    v[1] ^=
        v[2] ^ v[3] ^ v[5] ^ v[6] ^ v[13] ^ v[15] ^ v[16] ^ v[17] ^ v[20] ^ v[25] ^ v[28] ^ v[31];
    v[0] ^= v[4]
        ^ v[5]
        ^ v[7]
        ^ v[8]
        ^ v[9]
        ^ v[14]
        ^ v[15]
        ^ v[17]
        ^ v[19]
        ^ v[20]
        ^ v[21]
        ^ v[22]
        ^ v[27]
        ^ v[30];
    let mut suffix = String::with_capacity(7);
    for group in 0..6 {
        let value = 72u32
            ^ (1 & v[5 * group])
            ^ (2 & v[5 * group + 1])
            ^ (4 & v[5 * group + 2])
            ^ (24 & v[5 * group + 3])
            ^ (32 & v[5 * group + 4]);
        suffix.push(char::from_u32(value).expect("a manufactured letter"));
    }
    let last = 72u32 ^ (2 & v[30]) ^ (4 & v[31]);
    suffix.push(char::from_u32(last).expect("a manufactured letter"));
    if prefix.is_empty() {
        suffix
    } else {
        format!("{prefix}{suffix}")
    }
}

/// The component field-path table for a custom binding type, if it has one.
///
/// Each table is the frozen `s_FieldPathDictionary` of the matching generated
/// component companion class, committed in
/// [`crate::resources::custom_curve_tables`].
fn component_table(custom_type: BindingCustomType) -> Option<&'static [(u32, &'static str)]> {
    use super::custom_curve_tables::*;
    match custom_type {
        BindingCustomType::MONO_BEHAVIOUR => Some(MONO_BEHAVIOUR_PATHS),
        BindingCustomType::LIGHT => Some(LIGHT_PATHS),
        BindingCustomType::RENDERER_SHADOWS => Some(RENDERER_PATHS),
        BindingCustomType::PARTICLE_SYSTEM => Some(PARTICLE_SYSTEM_PATHS),
        BindingCustomType::RECT_TRANSFORM => Some(RECT_TRANSFORM_PATHS),
        BindingCustomType::LINE_RENDERER => Some(LINE_RENDERER_PATHS),
        BindingCustomType::TRAIL_RENDERER => Some(TRAIL_RENDERER_PATHS),
        BindingCustomType::CAMERA => Some(CAMERA_PATHS),
        BindingCustomType::VISUAL_EFFECT => Some(VISUAL_EFFECT_PATHS),
        BindingCustomType::PARTICLE_FORCE_FIELD => Some(PARTICLE_SYSTEM_FORCE_FIELD_PATHS),
        BindingCustomType::MESH_FILTER => Some(MESH_FILTER_PATHS),
        _ => None,
    }
}

/// `<Companion>.TryGetPath`: the field path stored under the attribute hash, if
/// any. The committed tables are sorted by hash.
pub fn component_attribute_path(
    custom_type: BindingCustomType,
    attribute: u32,
) -> Option<&'static str> {
    let table = component_table(custom_type)?;
    table
        .binary_search_by_key(&attribute, |(hash, _)| *hash)
        .ok()
        .map(|index| table[index].1)
}

/// Scene facts the caller resolves before naming a custom-curve attribute.
///
/// `Roots`/`FindChild` walks, component lookups, and the CRC searches
/// (`Mesh.FindBlendShapeNameByCRC`, `Renderer.FindMaterialPropertyNameByCRC28`)
/// need the asset collection and stay caller-side; every name assembled from
/// the answers is a pure function below.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CustomCurveSceneFacts<'a> {
    /// The blend-shape name the CRC resolved to, if the skinned-mesh walk hit.
    pub blend_shape: Option<&'a str>,
    /// The material property the CRC-28 resolved to, if the renderer walk hit.
    pub material_property: Option<&'a str>,
}

/// `CustomCurveResolver.ToAttributeName` over caller-resolved scene facts.
///
/// Unknown paths pass through verbatim; hits compose the shipped prefixes;
/// misses manufacture the shipped `ReverseAscii` fallbacks; table misses
/// either throw (like the source `ArgumentException`) or manufacture, per
/// binding type. Constraint, `Renderer`, and `SpriteRenderer` branches reuse
/// their kernels.
pub fn to_attribute_name(
    custom_type: BindingCustomType,
    attribute: u32,
    path: &str,
    scene: &CustomCurveSceneFacts<'_>,
) -> Result<String, CustomCurveError> {
    match custom_type {
        BindingCustomType::BLEND_SHAPE => {
            if is_unknown_path(path) {
                return Ok(format!("{BLEND_SHAPE_PREFIX}{path}"));
            }
            match scene.blend_shape {
                Some(shape) => Ok(format!("{BLEND_SHAPE_PREFIX}{shape}")),
                None => Ok(format!(
                    "{BLEND_SHAPE_PREFIX}{}",
                    reverse_ascii_crc32(&format!("path_0x{attribute:X}_"), attribute)
                )),
            }
        }
        BindingCustomType::RENDERER => Ok(renderer_attribute_path(attribute)),
        BindingCustomType::RENDERER_MATERIAL => {
            if is_unknown_path(path) {
                return Ok(format!("{MATERIAL_PREFIX}{path}"));
            }
            match scene.material_property {
                Some(property) => Ok(renderer_material_attribute_name(property, attribute)),
                None => Ok(format!(
                    "{MATERIAL_PREFIX}{}",
                    reverse_ascii_crc32(&format!("path_0x{attribute:X}_"), attribute)
                )),
            }
        }
        BindingCustomType::SPRITE_RENDERER => {
            sprite_renderer_attribute_path(attribute).map(str::to_owned)
        }
        BindingCustomType::MONO_BEHAVIOUR
        | BindingCustomType::LIGHT
        | BindingCustomType::RENDERER_SHADOWS
        | BindingCustomType::RECT_TRANSFORM
        | BindingCustomType::LINE_RENDERER
        | BindingCustomType::TRAIL_RENDERER
        | BindingCustomType::CAMERA => match component_attribute_path(custom_type, attribute) {
            Some(found) => Ok(found.to_owned()),
            None => Err(CustomCurveError::UnknownAttribute {
                custom_type: custom_type.0,
                attribute,
            }),
        },
        BindingCustomType::PARTICLE_SYSTEM
        | BindingCustomType::VISUAL_EFFECT
        | BindingCustomType::PARTICLE_FORCE_FIELD
        | BindingCustomType::MESH_FILTER => {
            match component_attribute_path(custom_type, attribute) {
                Some(found) => Ok(found.to_owned()),
                None => {
                    let word = custom_type
                        .name()
                        .expect("a table-backed binding type has a name");
                    Ok(reverse_ascii_crc32(
                        &format!("{word}_0x{attribute:X}_"),
                        attribute,
                    ))
                }
            }
        }
        BindingCustomType::USER_DEFINED => Ok(reverse_ascii_crc32(
            &format!("UserDefined_0x{attribute:X}_"),
            attribute,
        )),
        other if other.is_constraint() => constraint_attribute_name(other, attribute),
        other => Err(CustomCurveError::BindingTypeNotImplemented {
            custom_type: other.0,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_custom_type_discriminants_match_the_snapshot() {
        assert_eq!(BindingCustomType::NONE.0, 0);
        assert_eq!(BindingCustomType::TRANSFORM.0, 4);
        assert_eq!(BindingCustomType::ANIMATOR_MUSCLE.0, 8);
        assert_eq!(BindingCustomType::BLEND_SHAPE.0, 20);
        assert_eq!(BindingCustomType::RENDERER.0, 21);
        assert_eq!(BindingCustomType::RENDERER_MATERIAL.0, 22);
        assert_eq!(BindingCustomType::SPRITE_RENDERER.0, 23);
        assert_eq!(BindingCustomType::MONO_BEHAVIOUR.0, 24);
        assert_eq!(BindingCustomType::LIGHT.0, 25);
        assert_eq!(BindingCustomType::RENDERER_SHADOWS.0, 26);
        assert_eq!(BindingCustomType::PARTICLE_SYSTEM.0, 27);
        assert_eq!(BindingCustomType::RECT_TRANSFORM.0, 28);
        assert_eq!(BindingCustomType::LINE_RENDERER.0, 29);
        assert_eq!(BindingCustomType::TRAIL_RENDERER.0, 30);
        assert_eq!(BindingCustomType::POSITION_CONSTRAINT.0, 31);
        assert_eq!(BindingCustomType::ROTATION_CONSTRAINT.0, 32);
        assert_eq!(BindingCustomType::SCALE_CONSTRAINT.0, 33);
        assert_eq!(BindingCustomType::AIM_CONSTRAINT.0, 34);
        assert_eq!(BindingCustomType::PARENT_CONSTRAINT.0, 35);
        assert_eq!(BindingCustomType::LOOK_AT_CONSTRAINT.0, 36);
        assert_eq!(BindingCustomType::CAMERA.0, 37);
        assert_eq!(BindingCustomType::VISUAL_EFFECT.0, 38);
        assert_eq!(BindingCustomType::PARTICLE_FORCE_FIELD.0, 39);
        assert_eq!(BindingCustomType::USER_DEFINED.0, 40);
        assert_eq!(BindingCustomType::MESH_FILTER.0, 41);
        for custom in [
            BindingCustomType::POSITION_CONSTRAINT,
            BindingCustomType::ROTATION_CONSTRAINT,
            BindingCustomType::SCALE_CONSTRAINT,
            BindingCustomType::AIM_CONSTRAINT,
            BindingCustomType::PARENT_CONSTRAINT,
            BindingCustomType::LOOK_AT_CONSTRAINT,
        ] {
            assert!(custom.is_constraint(), "{custom:?}");
        }
        assert!(!BindingCustomType::RENDERER.is_constraint());
        assert!(!BindingCustomType::NONE.is_constraint());
    }

    #[test]
    fn transform_type_dimensions_match_the_snapshot() {
        assert_eq!(TransformType::TRANSLATION.dimension(), Ok(3));
        assert_eq!(TransformType::SCALING.dimension(), Ok(3));
        assert_eq!(TransformType::EULER_ROTATION.dimension(), Ok(3));
        assert_eq!(TransformType::ROTATION.dimension(), Ok(4));
        assert_eq!(
            TransformType(0).dimension(),
            Err(CustomCurveError::InvalidTransformType { value: 0 })
        );
        assert_eq!(
            TransformType(5).dimension(),
            Err(CustomCurveError::InvalidTransformType { value: 5 })
        );
        assert!(TransformType::TRANSLATION.is_valid());
        assert!(!TransformType(9).is_valid());
    }

    #[test]
    fn constraint_tables_cover_every_documented_property() {
        assert_eq!(
            constraint_attribute_name(BindingCustomType::POSITION_CONSTRAINT, 0).as_deref(),
            Ok("m_RestTranslation.x")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::POSITION_CONSTRAINT, 10).as_deref(),
            Ok("m_Active")
        );
        // Indexed entries read the source index from the high bits.
        assert_eq!(
            constraint_attribute_name(BindingCustomType::POSITION_CONSTRAINT, 11 | (3 << 8))
                .as_deref(),
            Ok("m_Sources.Array.data[3].sourceTransform")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::ROTATION_CONSTRAINT, 4).as_deref(),
            Ok("m_RotationOffset.x")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::SCALE_CONSTRAINT, 0).as_deref(),
            Ok("m_ScaleAtRest.x")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::AIM_CONSTRAINT, 5).as_deref(),
            Ok("m_WorldUpObject")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::PARENT_CONSTRAINT, 8).as_deref(),
            Ok("m_TranslationOffsets.Array.data[0].x")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::PARENT_CONSTRAINT, 13 | (7 << 8))
                .as_deref(),
            Ok("m_RotationOffsets.Array.data[7].z")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::PARENT_CONSTRAINT, 15).as_deref(),
            Ok("m_Sources.Array.data[0].weight")
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::LOOK_AT_CONSTRAINT, 5).as_deref(),
            Ok("m_Roll")
        );
        // Out-of-table nibbles are explicit errors carrying type and attribute.
        assert_eq!(
            constraint_attribute_name(BindingCustomType::AIM_CONSTRAINT, 8),
            Err(CustomCurveError::UnknownAttribute {
                custom_type: 34,
                attribute: 8
            })
        );
        assert_eq!(
            constraint_attribute_name(BindingCustomType::POSITION_CONSTRAINT, 13),
            Err(CustomCurveError::UnknownAttribute {
                custom_type: 31,
                attribute: 13
            })
        );
        // Non-constraint types have no table here.
        assert_eq!(
            constraint_attribute_name(BindingCustomType::LIGHT, 0),
            Err(CustomCurveError::UnsupportedCustomType { custom_type: 25 })
        );
    }

    #[test]
    fn renderer_and_sprite_branches_match_the_snapshot() {
        assert_eq!(renderer_attribute_path(0), "m_Materials.Array.data[0]");
        assert_eq!(renderer_attribute_path(41), "m_Materials.Array.data[41]");
        assert_eq!(sprite_renderer_attribute_path(0), Ok("m_Sprite"));
        assert_eq!(
            sprite_renderer_attribute_path(2),
            Err(CustomCurveError::UnknownAttribute {
                custom_type: 23,
                attribute: 2
            })
        );
    }

    #[test]
    fn renderer_material_bits_decode_like_the_snapshot() {
        // Bit 31 addresses the whole property; the low 28 bits are the CRC.
        assert_eq!(
            split_renderer_material_attribute(0x8000_0000 | 0x0D6A_83C9),
            (0x0D6A_83C9, RendererMaterialSelector::Direct)
        );
        assert_eq!(
            renderer_material_attribute_name("_MainTex", 0x8000_0000 | 0x0D6A_83C9),
            "material._MainTex"
        );
        // Channel bits without RGBA use x/y/z/w.
        assert_eq!(
            split_renderer_material_attribute(0x0D6A_83C9),
            (
                0x0D6A_83C9,
                RendererMaterialSelector::Channel {
                    is_rgba: false,
                    channel: 'x'
                }
            )
        );
        for (shift, channel) in [(0, 'x'), (1, 'y'), (2, 'z'), (3, 'w')] {
            let attribute = (shift << 28) | 0x0D6A_83C9;
            assert_eq!(
                renderer_material_attribute_name("_Color", attribute),
                format!("material._Color.{channel}"),
                "shift {shift}"
            );
        }
        // Bit 30 switches to r/g/b/a.
        for (shift, channel) in [(0, 'r'), (1, 'g'), (2, 'b'), (3, 'a')] {
            let attribute = 0x4000_0000 | (shift << 28) | 0x0D6A_83C9;
            assert_eq!(
                renderer_material_attribute_name("_Color", attribute),
                format!("material._Color.{channel}"),
                "rgba shift {shift}"
            );
        }
    }

    #[test]
    fn unknown_path_shape_matches_the_snapshot_regex() {
        // Plausible manufactured paths match.
        assert!(is_unknown_path("path_0x1A2B_HJKLMNN"));
        assert!(is_unknown_path("path_0x0_HHHHHHH"));
        assert!(is_unknown_path("path_0xFFFFFFFFFF_hhhhhhN"));
        assert!(is_unknown_path("path_0xDEADBEEF1_mnopqrL"));
        // Wrong prefixes, hex case, lengths, tails, and alphabets do not.
        assert!(!is_unknown_path("path_0x1A2B_HJKLMN"));
        assert!(!is_unknown_path("path_0x1A2B_HJKLMNQQ"));
        assert!(!is_unknown_path("path_0x1a2b_HJKLMNN"));
        assert!(!is_unknown_path("typetree_0x1A_HJKLMNN"));
        assert!(!is_unknown_path("path_0x_HJKLMNN"));
        assert!(!is_unknown_path("path_0x123456789AB_HJKLMNN"));
        assert!(!is_unknown_path("path_0x1A2B_ABCDEFH"));
        assert!(!is_unknown_path("path_0x1A2B_HJKLMNX"));
        assert!(!is_unknown_path("path_0x1A2B_HJKLMNQ"));
        assert!(!is_unknown_path("path_0x1A2B_HJKLMN "));
        assert!(!is_unknown_path("path_0x1A2B_HJKLMN\n"));
        assert!(!is_unknown_path(""));
        assert!(!is_unknown_path("path_0x"));
    }

    /// Differential against the real .NET regex behind
    /// `CustomCurveResolver.UnknownPathRegex` (probe `Temp/opencode/rpathprobe`):
    /// every probed path must classify identically.
    #[test]
    fn unknown_path_matches_the_dotnet_regex() {
        let fixture = include_str!("custom_curve_rpath_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, '|');
            assert_eq!(parts.next(), Some("RPATH"), "{line}");
            let path = parts.next().expect("a probed path");
            let expected = parts.next().expect("a 0/1 word");
            assert!(parts.next().is_none(), "{line}");
            assert_eq!(is_unknown_path(path), expected == "1", "{line:?}");
            rows += 1;
        }
        assert_eq!(rows, 21, "all probed paths are asserted");
    }

    #[test]
    fn converter_prefixes_match_the_snapshot_literals() {
        assert_eq!(UNKNOWN_PATH_PREFIX, "path_");
        assert_eq!(MISSED_PROPERTY_PREFIX, "missed_");
        assert_eq!(SCRIPT_PROPERTY_PREFIX, "script_");
        assert_eq!(TYPE_TREE_PROPERTY_PREFIX, "typetree_");
        assert_eq!(BLEND_SHAPE_PREFIX, "blendShape.");
        assert_eq!(MATERIAL_PREFIX, "material.");
    }

    #[test]
    fn precedence_boundaries_and_messages_match_the_snapshot() {
        // Bit 31 wins over the channel bits: the source checks it first.
        assert_eq!(
            split_renderer_material_attribute(0xC000_0000 | 0x0D6A_83C9),
            (0x0D6A_83C9, RendererMaterialSelector::Direct)
        );
        assert_eq!(
            renderer_material_attribute_name("_MainTex", 0xC000_0000 | 0x0D6A_83C9),
            "material._MainTex"
        );
        // The constraint range is exactly 31-36.
        assert!(!BindingCustomType(30).is_constraint());
        assert!(!BindingCustomType(37).is_constraint());
        // The failure messages mirror the source exception spellings.
        assert_eq!(
            CustomCurveError::InvalidTransformType { value: 9 }.to_string(),
            "invalid TransformType value 9"
        );
        assert_eq!(
            CustomCurveError::UnknownAttribute {
                custom_type: 34,
                attribute: 8
            }
            .to_string(),
            "Unknown attribute 0x8 (8) for AimConstraint"
        );
        assert_eq!(
            CustomCurveError::UnsupportedCustomType { custom_type: 25 }.to_string(),
            "custom binding type 25 needs scene or checksum lookups"
        );
    }

    /// Differential against the IL-decoded checksum package (fixture
    /// `custom_curve_resolve_fixture.txt`, `CREV` rows from probe
    /// `Temp/opencode/curveresolve` calling the shipped
    /// `Crc32Algorithm.ReverseAscii`, SDK 10.0.401): every manufactured name
    /// must reproduce exactly, including the empty-prefix arm.
    #[test]
    fn reverse_ascii_matches_the_shipped_checksum() {
        let fixture = include_str!("custom_curve_resolve_fixture.txt");
        let mut rows = 0;
        let mut hash_rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cells = line.split('|');
            match cells.next().expect("a prefix") {
                "CHASH" => {
                    let input = cells.next().expect("a hash input");
                    let expected = u32::from_str_radix(cells.next().expect("a hash word"), 16)
                        .expect("a hex hash word");
                    assert_eq!(
                        super::super::blend_shape::crc32_ieee(input.as_bytes()),
                        expected,
                        "{line}"
                    );
                    hash_rows += 1;
                    continue;
                }
                "CREV" => {}
                _ => continue,
            }
            rows += 1;
            let attribute = u32::from_str_radix(cells.next().expect("an attribute"), 16)
                .expect("a hex attribute");
            let prefix = cells.next().expect("a prefix");
            let expected = cells.next().expect("a name");
            assert_eq!(reverse_ascii_crc32(prefix, attribute), expected, "{line}");
        }
        assert_eq!(rows, 14, "all manufactured-name vectors are asserted");
        assert_eq!(hash_rows, 2, "both HashAscii marker hashes are asserted");
    }

    /// Differential against the shipped resolver over a live generated scene
    /// (probe `Temp/opencode/curveresolve`, `CURVE` rows in
    /// `custom_curve_resolve_fixture.txt`): a real 2022.3 hierarchy (root plus
    /// `Arm`/`Leg` children, an `Animation` referencing the clip, a skinned
    /// `Arm` mesh with a `Smile` channel, a `Leg` renderer with a `_MainTex`
    /// material) through the shipped `ToAttributeName`. The replay feeds the
    /// probe-setup facts (the `Arm` shape hit, the `Leg` material hit, misses
    /// everywhere else) and must reproduce every name, every shipped
    /// `ArgumentException` message, and the live CRC words.
    #[test]
    fn to_attribute_name_matches_the_reference_runtime() {
        assert_eq!(
            super::super::blend_shape::crc32_ieee(b"Smile"),
            0xC34B_2C0B,
            "the probe smile hash"
        );
        assert_eq!(
            super::super::material::crc28(b"_MainTex_ST"),
            0x0686_C589,
            "the probe material hash"
        );
        let fixture = include_str!("custom_curve_resolve_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cells = line.split('|');
            if cells.next().expect("a prefix") != "CURVE" {
                continue;
            }
            let tag = cells.next().expect("a row kind");
            if tag == "crcst" || tag == "smilecrc" {
                continue;
            }
            rows += 1;
            let custom = BindingCustomType(tag.parse().expect("a binding type"));
            let attribute = u32::from_str_radix(cells.next().expect("an attribute"), 16)
                .expect("a hex attribute");
            let path = cells.next().expect("a path");
            let result = cells.next().expect("a result");
            // Probe-setup facts: the `Arm` skinned mesh resolves `Smile`, the
            // `Leg` renderer resolves `_MainTex_ST`, every other path misses.
            // Unknown paths short-circuit before the facts are read.
            let scene = CustomCurveSceneFacts {
                blend_shape: if path == "Arm" && attribute == 0xC34B_2C0B {
                    Some("Smile")
                } else {
                    None
                },
                material_property: if path == "Leg" && attribute & 0x0FFF_FFFF == 0x0686_C589 {
                    Some("_MainTex_ST")
                } else {
                    None
                },
            };
            match to_attribute_name(custom, attribute, path, &scene) {
                Ok(name) => assert_eq!(name, result, "{line}"),
                Err(error) => {
                    let message = cells.next().expect("a shipped message");
                    assert_eq!(
                        message.strip_prefix("ArgumentException:"),
                        Some(error.to_string()).as_deref(),
                        "{line}"
                    );
                }
            }
        }
        assert_eq!(rows, 39, "all resolver vectors are asserted");
    }

    /// Every committed component table matches its shipped dump entry for
    /// entry (raw dumps in `custom_curve_table_dumps/`, dumped from the
    /// shipped `AssetRipper.SourceGenerated.dll` by probe
    /// `Temp/opencode/extdump`), and every entry round-trips through the
    /// binary-search lookup.
    #[test]
    fn component_tables_match_the_shipped_dumps() {
        const TABLES: [(BindingCustomType, &str, &[u8]); 11] = [
            (
                BindingCustomType::MONO_BEHAVIOUR,
                "MonoBehaviour",
                include_bytes!("custom_curve_table_dumps/MonoBehaviour.txt"),
            ),
            (
                BindingCustomType::LIGHT,
                "Light",
                include_bytes!("custom_curve_table_dumps/Light.txt"),
            ),
            (
                BindingCustomType::RENDERER_SHADOWS,
                "Renderer",
                include_bytes!("custom_curve_table_dumps/Renderer.txt"),
            ),
            (
                BindingCustomType::PARTICLE_SYSTEM,
                "ParticleSystem",
                include_bytes!("custom_curve_table_dumps/ParticleSystem.txt"),
            ),
            (
                BindingCustomType::RECT_TRANSFORM,
                "RectTransform",
                include_bytes!("custom_curve_table_dumps/RectTransform.txt"),
            ),
            (
                BindingCustomType::LINE_RENDERER,
                "LineRenderer",
                include_bytes!("custom_curve_table_dumps/LineRenderer.txt"),
            ),
            (
                BindingCustomType::TRAIL_RENDERER,
                "TrailRenderer",
                include_bytes!("custom_curve_table_dumps/TrailRenderer.txt"),
            ),
            (
                BindingCustomType::CAMERA,
                "Camera",
                include_bytes!("custom_curve_table_dumps/Camera.txt"),
            ),
            (
                BindingCustomType::VISUAL_EFFECT,
                "VisualEffect",
                include_bytes!("custom_curve_table_dumps/VisualEffect.txt"),
            ),
            (
                BindingCustomType::PARTICLE_FORCE_FIELD,
                "ParticleSystemForceField",
                include_bytes!("custom_curve_table_dumps/ParticleSystemForceField.txt"),
            ),
            (
                BindingCustomType::MESH_FILTER,
                "MeshFilter",
                include_bytes!("custom_curve_table_dumps/MeshFilter.txt"),
            ),
        ];
        let mut entries = 0;
        for (custom, name, dump) in TABLES {
            let text = std::str::from_utf8(dump).expect("an ASCII dump");
            let mut dumped: Vec<(u32, &str)> = Vec::new();
            for line in text.lines() {
                let (hash, path) = line.split_once('|').expect("a hash|path row");
                dumped.push((u32::from_str_radix(hash, 16).expect("a hex hash"), path));
            }
            dumped.sort();
            let table = component_table(custom).expect("a table");
            assert_eq!(table, dumped.as_slice(), "{name} matches its dump");
            for (hash, path) in &dumped {
                assert_eq!(
                    component_attribute_path(custom, *hash),
                    Some(*path),
                    "{name} 0x{hash:X}"
                );
            }
            entries += dumped.len();
        }
        assert_eq!(entries, 2551, "all dumped entries are asserted");
    }
}
