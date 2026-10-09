use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::{BinaryError, BinaryReader, Endian};

use super::format_version::FormatVersion;
use super::header::{HeaderError, SerializedFileHeader};
use super::managed_types::managed_reference as managed_names;
use super::metadata::{
    unity_version_aligns_arrays, ObjectInfo, SerializedFileMetadata, TypeTree, TypeTreeNode,
};
use super::transfer_flags::TransferInstructionFlags;

const ALIGN_BYTES_FLAG: u32 = 0x4000;
const CHAR_PROPERTY_MASK_FLAG: u32 = 0x0200_0000;
const MAX_TYPE_TREE_DEPTH: usize = 256;

mod writer;
pub use writer::{
    ObjectDefaultValueLimits, ObjectEncodeError, ObjectEncodeErrorKind, ObjectEncodeLimits,
};

/// Per-object bounds for dynamic TypeTree decoding. Callers can raise these limits when
/// processing trusted large assets or lower them for untrusted input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectDecodeLimits {
    pub max_decoded_allocation_bytes: usize,
    pub max_array_elements: usize,
    pub max_string_bytes: usize,
    pub max_type_tree_depth: usize,
    /// Maximum number of managed-reference registries recursively nested in payload data.
    pub max_managed_registry_depth: usize,
}

impl Default for ObjectDecodeLimits {
    fn default() -> Self {
        Self {
            max_decoded_allocation_bytes: 256 * 1024 * 1024,
            max_array_elements: 16 * 1024 * 1024,
            max_string_bytes: 64 * 1024 * 1024,
            max_type_tree_depth: MAX_TYPE_TREE_DEPTH,
            max_managed_registry_depth: 64,
        }
    }
}

struct DecodeBudget<'a> {
    limits: &'a ObjectDecodeLimits,
    allocated_bytes: usize,
    managed_type_references: &'a [super::metadata::SerializedTypeReference],
    managed_reference_type_resolver:
        Option<&'a dyn super::managed_registry::ManagedReferenceTypeTreeResolver>,
    stream_offset: usize,
    managed_registry_depth: usize,
}

#[derive(Clone, Copy)]
struct ObjectDecodeTarget<'a> {
    index: usize,
    object: &'a ObjectInfo,
    fallback_type_id: i32,
}

pub(crate) struct ManagedPayloadDecodeContext<'a> {
    pub entry_index: usize,
    pub stream_offset: usize,
    pub managed_registry_depth: usize,
    pub managed_reference_type_resolver:
        Option<&'a dyn super::managed_registry::ManagedReferenceTypeTreeResolver>,
}

impl DecodeBudget<'_> {
    fn charge(
        &mut self,
        requested: usize,
        offset: usize,
        resource: &str,
    ) -> Result<(), ObjectDecodeError> {
        let total = self.allocated_bytes.checked_add(requested).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::AllocationSizeOverflow {
                    resource: resource.to_owned(),
                },
            )
        })?;
        if total > self.limits.max_decoded_allocation_bytes {
            return Err(ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::DecodeLimitExceeded {
                    resource: resource.to_owned(),
                    requested: total,
                    limit: self.limits.max_decoded_allocation_bytes,
                },
            ));
        }
        self.allocated_bytes = total;
        Ok(())
    }

    fn charge_items<T>(
        &mut self,
        count: usize,
        offset: usize,
        resource: &str,
    ) -> Result<(), ObjectDecodeError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::AllocationSizeOverflow {
                    resource: resource.to_owned(),
                },
            )
        })?;
        self.charge(bytes, offset, resource)
    }

    fn remaining_allocation_bytes(&self) -> usize {
        self.limits
            .max_decoded_allocation_bytes
            .saturating_sub(self.allocated_bytes)
    }
}

struct TypeTreeLayout<'a> {
    nodes: &'a [TypeTreeNode],
    first_child: Vec<Option<usize>>,
    next_sibling: Vec<Option<usize>>,
}

impl<'a> TypeTreeLayout<'a> {
    fn new(
        type_tree: &'a TypeTree,
        object_index: usize,
        offset: usize,
    ) -> Result<Self, ObjectDecodeError> {
        let nodes = type_tree.nodes.as_slice();
        if nodes.is_empty() {
            return Err(ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::EmptyTypeTree { object_index },
            ));
        }
        if nodes[0].level != 0 {
            return Err(invalid_tree_levels(offset, object_index, 0, 0));
        }

        let mut first_child = reserve_option_indices(nodes.len(), offset)?;
        let mut next_sibling = reserve_option_indices(nodes.len(), offset)?;
        let mut last_child = reserve_option_indices(nodes.len(), offset)?;
        let mut stack = Vec::new();
        stack
            .try_reserve_exact(nodes.len())
            .map_err(|_| allocation_error(offset, "TypeTree traversal stack", nodes.len()))?;
        stack.push(0usize);

        for index in 1..nodes.len() {
            let level = nodes[index].level;
            while let Some(&parent_index) = stack.last() {
                if nodes[parent_index].level >= level {
                    stack.pop();
                } else {
                    break;
                }
            }
            let Some(&parent_index) = stack.last() else {
                return Err(invalid_tree_levels(offset, object_index, index - 1, index));
            };
            if nodes[parent_index].level.checked_add(1) != Some(level) {
                return Err(invalid_tree_levels(
                    offset,
                    object_index,
                    parent_index,
                    index,
                ));
            }
            if let Some(previous_child) = last_child[parent_index] {
                next_sibling[previous_child] = Some(index);
            } else {
                first_child[parent_index] = Some(index);
            }
            last_child[parent_index] = Some(index);
            stack.push(index);
        }

        Ok(Self {
            nodes,
            first_child,
            next_sibling,
        })
    }
}

/// A structured view of one object's serialized payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedObjectPayload {
    pub object_index: usize,
    pub path_id: i64,
    pub type_name: String,
    pub root: SerializedValue,
    pub consumed_bytes: usize,
    /// Bytes after the fields described by the TypeTree remain visible here.
    pub trailing_bytes: Vec<u8>,
    pub allocated_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SerializedFieldValue {
    pub name: String,
    pub value: SerializedValue,
}

/// Typed field data decoded directly from a SerializedFile object payload.
#[derive(Clone, Debug, PartialEq)]
pub enum SerializedValue {
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    Char(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    /// Raw UTF-8 bytes are retained so invalid or non-canonical source strings are not lost.
    StringBytes(Vec<u8>),
    Bytes(Vec<u8>),
    Array(Vec<SerializedValue>),
    Struct(Vec<SerializedFieldValue>),
    /// A Unity object reference retaining its file-table index and signed PathID.
    PPtr {
        file_id: i32,
        path_id: i64,
    },
    /// A SerializeReference identifier. Its registry lives on the owning serialized object;
    /// keeping the identifier as an edge avoids recursive expansion and preserves cycles.
    ManagedReferenceId {
        reference_id: i64,
        indexed: bool,
    },
    /// The dynamically typed SerializeReference registry attached to one serialized object.
    ManagedReferenceRegistry(Box<super::managed_registry::ManagedRegistryRead>),
    /// Unknown leaf data with a nonnegative TypeTree byte size is preserved without decoding.
    Raw {
        type_name: String,
        bytes: Vec<u8>,
    },
}

impl SerializedValue {
    /// Returns the ordered fields when this value is a struct.
    pub fn struct_fields(&self) -> Option<&[SerializedFieldValue]> {
        match self {
            Self::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Finds a field only when its name is unique in this struct.
    ///
    /// Duplicate names are preserved by TypeTree decoding and reported instead of silently
    /// selecting one. Non-struct values are also reported distinctly from a missing field.
    pub fn unique_field(
        &self,
        name: &str,
    ) -> Result<Option<&SerializedValue>, SerializedValueLookupError> {
        let Self::Struct(fields) = self else {
            return Err(SerializedValueLookupError::NotStruct);
        };
        let mut matching = fields.iter().filter(|field| field.name == name);
        let Some(first) = matching.next() else {
            return Ok(None);
        };
        if matching.next().is_some() {
            let match_count = fields.iter().filter(|field| field.name == name).count();
            return Err(SerializedValueLookupError::DuplicateField {
                name: name.to_owned(),
                match_count,
            });
        }
        Ok(Some(&first.value))
    }

    /// Finds a field in this struct and nested `Base` structs without discarding duplicates.
    ///
    /// Unity TypeTrees may encode inherited object fields below a field named `Base`; following
    /// only that chain avoids searching unrelated nested resource records or array elements.
    pub fn unique_field_in_base_chain(
        &self,
        name: &str,
    ) -> Result<Option<&SerializedValue>, SerializedValueLookupError> {
        if !matches!(self, Self::Struct(_)) {
            return Err(SerializedValueLookupError::NotStruct);
        }
        let mut scope = self;
        let mut found = None;
        while let Self::Struct(_) = scope {
            if let Some(value) = scope.unique_field(name)? {
                if found.is_some() {
                    return Err(SerializedValueLookupError::DuplicateField {
                        name: name.to_owned(),
                        match_count: 2,
                    });
                }
                found = Some(value);
            }
            let Some(base) = scope.unique_field("Base")? else {
                break;
            };
            if !matches!(base, Self::Struct(_)) {
                break;
            }
            scope = base;
        }
        Ok(found)
    }

    /// Finds one field in this struct and nested `Base` structs for a controlled value update.
    /// Duplicate fields and duplicate matches across the base chain are rejected.
    pub fn unique_field_in_base_chain_mut(
        &mut self,
        name: &str,
    ) -> Result<Option<&mut SerializedValue>, SerializedValueMutationError> {
        if !matches!(self, Self::Struct(_)) {
            return Err(SerializedValueMutationError::NotStruct);
        }

        let mut current_path = Vec::<usize>::new();
        let mut found_path = None::<Vec<usize>>;
        let mut current: &SerializedValue = self;
        while let Self::Struct(fields) = current {
            if let Some(field_index) = unique_field_index(fields, name)? {
                if found_path.is_some() {
                    return Err(SerializedValueMutationError::DuplicateField {
                        name: name.to_owned(),
                        match_count: 2,
                    });
                }
                let path_length = current_path
                    .len()
                    .checked_add(1)
                    .ok_or(SerializedValueMutationError::FieldPathDepthOverflow)?;
                let mut path = Vec::new();
                path.try_reserve_exact(path_length).map_err(|_| {
                    SerializedValueMutationError::AllocationFailed {
                        requested: path_length,
                    }
                })?;
                path.extend_from_slice(&current_path);
                path.push(field_index);
                found_path = Some(path);
            }

            let Some(base_index) = unique_field_index(fields, "Base")? else {
                break;
            };
            let base = &fields[base_index].value;
            if !matches!(base, Self::Struct(_)) {
                break;
            }
            let requested_path_depth = current_path
                .len()
                .checked_add(1)
                .ok_or(SerializedValueMutationError::FieldPathDepthOverflow)?;
            current_path.try_reserve(1).map_err(|_| {
                SerializedValueMutationError::AllocationFailed {
                    requested: requested_path_depth,
                }
            })?;
            current_path.push(base_index);
            current = base;
        }

        let Some(path) = found_path else {
            return Ok(None);
        };
        let mut target = self;
        for field_index in path {
            let Self::Struct(fields) = target else {
                return Err(SerializedValueMutationError::InvalidFieldPath);
            };
            let Some(field) = fields.get_mut(field_index) else {
                return Err(SerializedValueMutationError::InvalidFieldPath);
            };
            target = &mut field.value;
        }
        Ok(Some(target))
    }

    /// Returns the ordered elements when this value is an array.
    pub fn array_elements(&self) -> Option<&[SerializedValue]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns the array elements mutably while leaving their order and values intact.
    pub fn array_elements_mut(&mut self) -> Option<&mut [SerializedValue]> {
        match self {
            Self::Array(values) => Some(values.as_mut_slice()),
            _ => None,
        }
    }

    /// Appends one value to an array after checking both the caller's element bound and capacity.
    pub fn push_array_element_bounded(
        &mut self,
        value: SerializedValue,
        maximum_elements: usize,
    ) -> Result<(), SerializedValueMutationError> {
        let Self::Array(values) = self else {
            return Err(SerializedValueMutationError::NotArray);
        };
        let requested = values
            .len()
            .checked_add(1)
            .ok_or(SerializedValueMutationError::ArrayElementCountOverflow)?;
        if requested > maximum_elements {
            return Err(SerializedValueMutationError::ArrayElementLimitExceeded {
                requested,
                limit: maximum_elements,
            });
        }
        values
            .try_reserve(1)
            .map_err(|_| SerializedValueMutationError::AllocationFailed { requested })?;
        values.push(value);
        Ok(())
    }

    /// Returns the raw file-table index and PathID when this value is a PPtr.
    pub fn pptr(&self) -> Option<(i32, i64)> {
        match self {
            Self::PPtr { file_id, path_id } => Some((*file_id, *path_id)),
            _ => None,
        }
    }
}

fn unique_field_index(
    fields: &[SerializedFieldValue],
    name: &str,
) -> Result<Option<usize>, SerializedValueMutationError> {
    let mut matching = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.name == name);
    let Some((index, _)) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        let match_count = fields.iter().filter(|field| field.name == name).count();
        return Err(SerializedValueMutationError::DuplicateField {
            name: name.to_owned(),
            match_count,
        });
    }
    Ok(Some(index))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedValueLookupError {
    NotStruct,
    DuplicateField { name: String, match_count: usize },
}

impl Display for SerializedValueLookupError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStruct => f.write_str("field lookup requires a struct value"),
            Self::DuplicateField { name, match_count } => write!(
                f,
                "field '{name}' occurs {match_count} times in the decoded struct"
            ),
        }
    }
}

impl Error for SerializedValueLookupError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedValueMutationError {
    NotStruct,
    NotArray,
    DuplicateField { name: String, match_count: usize },
    FieldPathDepthOverflow,
    InvalidFieldPath,
    ArrayElementCountOverflow,
    ArrayElementLimitExceeded { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
}

impl Display for SerializedValueMutationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStruct => f.write_str("field mutation requires a struct value"),
            Self::NotArray => f.write_str("array append requires an array value"),
            Self::DuplicateField { name, match_count } => write!(
                f,
                "field '{name}' occurs {match_count} times along the decoded base chain"
            ),
            Self::FieldPathDepthOverflow => f.write_str("field path depth overflows usize"),
            Self::InvalidFieldPath => f.write_str("decoded field path changed during mutation"),
            Self::ArrayElementCountOverflow => f.write_str("array element count overflows usize"),
            Self::ArrayElementLimitExceeded { requested, limit } => write!(
                f,
                "array append would create {requested} elements, exceeding the limit of {limit}"
            ),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not reserve capacity for {requested} serialized value entries"
            ),
        }
    }
}

impl Error for SerializedValueMutationError {}

/// Reader-side TypeTree usability behind the absent-TypeTree recovery row
/// (`SER-TT-ABSENT-001`).
///
/// This states only what the reader knows without assemblies or generated
/// layouts: whether the resolved serialized type carries a TypeTree and whether
/// that tree has nodes. The factory's `MonoBehaviourReadFacts.has_type_tree`
/// consumes this fact; layout construction (`TypeTreeLayout::new`) and any
/// parse-acceptance decision stay downstream. A present-but-unparseable tree
/// reports `Present` here and surfaces its layout error when built, exactly
/// like the previous inline prelude checks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TypeTreeUsability {
    /// No TypeTree: recovery must come from another structure source.
    Missing,
    /// A TypeTree with no nodes: undecodable like a missing one.
    Empty,
    /// A TypeTree with nodes: decode proceeds to layout construction.
    Present,
}

/// Classify the resolved serialized type's TypeTree for the decode prelude and
/// the absent-tree recovery facts.
pub fn classify_type_tree_usability(type_tree: Option<&TypeTree>) -> TypeTreeUsability {
    match type_tree {
        None => TypeTreeUsability::Missing,
        Some(tree) if tree.nodes.is_empty() => TypeTreeUsability::Empty,
        Some(_) => TypeTreeUsability::Present,
    }
}

/// `GameAssetFactory.ReadAsset` version gate over caller facts: assets from Unity
/// versions before 3.5 (stripped-version assets compare below it) cannot be read,
/// so the reader refuses them before touching the payload. Callers supply the parsed
/// major/minor pair; unknown or unparseable versions are NOT refused here, preserving
/// current behavior for files without version text.
pub const fn version_is_unsupported(major: u32, minor: u32) -> bool {
    major < 3 || (major == 3 && minor < 5)
}

/// Split `major.minor` off a Unity version string for the readability gate; anything
/// else proceeds like today.
fn version_major_minor(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// `ClassIDType` member names for recovery naming
/// (`AssetRipper...ClassIDType`, decompiled Unity class IDs).
///
/// `RawDataObject.ClassName` renders the enum member name for defined IDs; unknown
/// IDs fall back to decimal (see [`raw_data_class_name`]). `PlayerSettings` (129)
/// deliberately has no entry: it is not a member of the source enum.
pub fn class_id_type_name(class_id: i32) -> Option<&'static str> {
    match class_id {
        1 => Some("GameObject"),
        2 => Some("Component"),
        4 => Some("Transform"),
        5 => Some("TimeManager"),
        11 => Some("AudioManager"),
        12 => Some("ParticleAnimator"),
        13 => Some("InputManager"),
        15 => Some("EllipsoidParticleEmitter"),
        17 => Some("Pipeline"),
        19 => Some("Physics2DSettings"),
        20 => Some("Camera"),
        21 => Some("Material"),
        23 => Some("MeshRenderer"),
        26 => Some("ParticleRenderer"),
        28 => Some("Texture2D"),
        29 => Some("OcclusionCullingSettings"),
        30 => Some("GraphicsSettings"),
        33 => Some("MeshFilter"),
        41 => Some("OcclusionPortal"),
        43 => Some("Mesh"),
        45 => Some("Skybox"),
        47 => Some("QualitySettings"),
        48 => Some("Shader"),
        49 => Some("TextAsset"),
        50 => Some("Rigidbody2D"),
        52 => Some("NotificationManager"),
        54 => Some("Rigidbody"),
        55 => Some("PhysicsManager"),
        58 => Some("CircleCollider2D"),
        59 => Some("HingeJoint"),
        60 => Some("PolygonCollider2D"),
        61 => Some("BoxCollider2D"),
        62 => Some("PhysicsMaterial2D"),
        64 => Some("MeshCollider"),
        65 => Some("BoxCollider"),
        66 => Some("CompositeCollider2D"),
        68 => Some("EdgeCollider2D"),
        70 => Some("CapsuleCollider2D"),
        71 => Some("AnimationManager"),
        72 => Some("ComputeShader"),
        74 => Some("AnimationClip"),
        75 => Some("ConstantForce"),
        76 => Some("WorldParticleCollider"),
        78 => Some("TagManager"),
        81 => Some("AudioListener"),
        82 => Some("AudioSource"),
        83 => Some("AudioClip"),
        84 => Some("RenderTexture"),
        86 => Some("CustomRenderTexture"),
        87 => Some("MeshParticleEmitter"),
        89 => Some("Cubemap"),
        90 => Some("Avatar"),
        91 => Some("AnimatorController"),
        92 => Some("GUILayer"),
        93 => Some("RuntimeAnimatorController"),
        94 => Some("ShaderNameRegistry"),
        95 => Some("Animator"),
        96 => Some("TrailRenderer"),
        98 => Some("DelayedCallManager"),
        102 => Some("TextMesh"),
        104 => Some("RenderSettings"),
        108 => Some("Light"),
        109 => Some("ShaderInclude"),
        111 => Some("Animation"),
        114 => Some("MonoBehaviour"),
        115 => Some("MonoScript"),
        116 => Some("MonoManager"),
        117 => Some("Texture3D"),
        118 => Some("NewAnimationTrack"),
        119 => Some("Projector"),
        120 => Some("LineRenderer"),
        121 => Some("Flare"),
        122 => Some("Halo"),
        123 => Some("LensFlare"),
        124 => Some("FlareLayer"),
        125 => Some("HaloLayer"),
        126 => Some("NavMeshProjectSettings"),
        127 => Some("HaloManager"),
        128 => Some("Font"),
        131 => Some("GUITexture"),
        132 => Some("GUIText"),
        134 => Some("PhysicsMaterial"),
        135 => Some("SphereCollider"),
        136 => Some("CapsuleCollider"),
        137 => Some("SkinnedMeshRenderer"),
        138 => Some("FixedJoint"),
        140 => Some("RaycastCollider"),
        141 => Some("BuildSettings"),
        142 => Some("AssetBundle"),
        143 => Some("CharacterController"),
        144 => Some("CharacterJoint"),
        145 => Some("SpringJoint"),
        146 => Some("WheelCollider"),
        147 => Some("ResourceManager"),
        148 => Some("NetworkView"),
        149 => Some("NetworkManager"),
        150 => Some("PreloadData"),
        152 => Some("MovieTexture"),
        153 => Some("ConfigurableJoint"),
        154 => Some("TerrainCollider"),
        155 => Some("MasterServerInterface"),
        156 => Some("TerrainData"),
        157 => Some("LightmapSettings"),
        158 => Some("WebCamTexture"),
        159 => Some("EditorSettings"),
        160 => Some("InteractiveCloth"),
        161 => Some("ClothRenderer"),
        162 => Some("EditorUserSettings"),
        163 => Some("SkinnedCloth"),
        164 => Some("AudioReverbFilter"),
        165 => Some("AudioHighPassFilter"),
        166 => Some("AudioChorusFilter"),
        167 => Some("AudioReverbZone"),
        168 => Some("AudioEchoFilter"),
        169 => Some("AudioLowPassFilter"),
        170 => Some("AudioDistortionFilter"),
        171 => Some("SparseTexture"),
        182 => Some("WindZone"),
        183 => Some("Cloth"),
        184 => Some("SubstanceArchive"),
        185 => Some("ProceduralMaterial"),
        186 => Some("ProceduralTexture"),
        187 => Some("Texture2DArray"),
        188 => Some("CubemapArray"),
        191 => Some("OffMeshLink"),
        192 => Some("OcclusionArea"),
        193 => Some("Tree"),
        194 => Some("NavMeshData_194"),
        195 => Some("NavMeshAgent"),
        196 => Some("NavMeshSettings"),
        197 => Some("LightProbes_197"),
        198 => Some("ParticleSystem"),
        199 => Some("ParticleSystemRenderer"),
        200 => Some("ShaderVariantCollection"),
        205 => Some("LODGroup"),
        206 => Some("BlendTree"),
        208 => Some("NavMeshObstacle"),
        210 => Some("SortingGroup"),
        212 => Some("SpriteRenderer"),
        213 => Some("Sprite"),
        214 => Some("CachedSpriteAtlas"),
        215 => Some("ReflectionProbe"),
        218 => Some("Terrain"),
        220 => Some("LightProbeGroup"),
        221 => Some("AnimatorOverrideController"),
        222 => Some("CanvasRenderer"),
        223 => Some("Canvas"),
        224 => Some("RectTransform"),
        225 => Some("CanvasGroup"),
        226 => Some("BillboardAsset"),
        227 => Some("BillboardRenderer"),
        228 => Some("SpeedTreeWindAsset"),
        231 => Some("SpringJoint2D"),
        232 => Some("DistanceJoint2D"),
        233 => Some("HingeJoint2D"),
        234 => Some("SliderJoint2D"),
        235 => Some("WheelJoint2D"),
        236 => Some("ClusterInputManager"),
        238 => Some("NavMeshData_238"),
        240 => Some("AudioMixer"),
        241 => Some("AudioMixerController"),
        243 => Some("AudioMixerGroupController"),
        244 => Some("AudioMixerEffectController"),
        245 => Some("AudioMixerSnapshotController"),
        247 => Some("ConstantForce2D"),
        249 => Some("AreaEffector2D"),
        250 => Some("PointEffector2D"),
        251 => Some("PlatformEffector2D"),
        252 => Some("SurfaceEffector2D"),
        253 => Some("BuoyancyEffector2D"),
        254 => Some("RelativeJoint2D"),
        255 => Some("FixedJoint2D"),
        256 => Some("FrictionJoint2D"),
        257 => Some("TargetJoint2D"),
        258 => Some("LightProbes_258"),
        259 => Some("LightProbeProxyVolume"),
        271 => Some("SampleClip"),
        272 => Some("AudioMixerSnapshot"),
        273 => Some("AudioMixerGroup"),
        280 => Some("NScreenBridge"),
        290 => Some("AssetBundleManifest"),
        292 => Some("UnityAdsManager"),
        300 => Some("RuntimeInitializeOnLoadManager"),
        301 => Some("CloudWebServicesManager"),
        302 => Some("CloudServiceHandlerBehaviour"),
        303 => Some("UnityAnalyticsManager"),
        304 => Some("CrashReportManager"),
        305 => Some("PerformanceReportingManager"),
        310 => Some("UnityConnectSettings"),
        319 => Some("AvatarMask_319"),
        320 => Some("PlayableDirector"),
        327 => Some("VideoClip_327"),
        328 => Some("VideoPlayer"),
        329 => Some("VideoClip_329"),
        330 => Some("ParticleSystemForceField"),
        331 => Some("SpriteMask"),
        362 => Some("WorldAnchor"),
        363 => Some("OcclusionCullingData"),
        900 => Some("MarshallingTestObject"),
        1001 => Some("PrefabInstance"),
        1002 => Some("EditorExtensionImpl"),
        1004 => Some("AssetDatabaseV1"),
        1005 => Some("Mesh3DSImporter"),
        1006 => Some("TextureImporter"),
        1007 => Some("ShaderImporter"),
        1008 => Some("ComputeShaderImporter"),
        1011 => Some("AvatarMask_1011"),
        1012 => Some("AvatarSkeletonMask"),
        1020 => Some("AudioImporter"),
        1026 => Some("HierarchyState"),
        1027 => Some("GUIDSerializer"),
        1028 => Some("AssetMetaData"),
        1029 => Some("DefaultAsset"),
        1030 => Some("DefaultImporter"),
        1031 => Some("TextScriptImporter"),
        1032 => Some("SceneAsset"),
        1034 => Some("NativeFormatImporter"),
        1035 => Some("MonoImporter"),
        1037 => Some("AssetServerCache"),
        1038 => Some("LibraryAssetImporter"),
        1041 => Some("FBXImporter"),
        1042 => Some("TrueTypeFontImporter"),
        1044 => Some("MovieImporter"),
        1045 => Some("EditorBuildSettings"),
        1046 => Some("DDSImporter"),
        1048 => Some("InspectorExpandedState"),
        1049 => Some("AnnotationManager"),
        1050 => Some("PluginImporter"),
        1051 => Some("EditorUserBuildSettings"),
        1052 => Some("PVRImporter"),
        1053 => Some("ASTCImporter"),
        1054 => Some("KTXImporter"),
        1055 => Some("IHVImageFormatImporter"),
        1101 => Some("AnimatorStateTransition"),
        1102 => Some("AnimatorState"),
        1105 => Some("HumanTemplate"),
        1107 => Some("AnimatorStateMachine"),
        1108 => Some("PreviewAnimationClip"),
        1109 => Some("AnimatorTransition"),
        1110 => Some("SpeedTreeImporter"),
        1111 => Some("AnimatorTransitionBase"),
        1112 => Some("SubstanceImporter"),
        1113 => Some("LightmapParameters"),
        1120 => Some("LightingDataAsset"),
        1124 => Some("SketchUpImporter"),
        1125 => Some("BuildReport"),
        1126 => Some("PackedAssets"),
        1127 => Some("VideoClipImporter"),
        1128 => Some("StyleSheetImporter"),
        19719996 => Some("TilemapCollider2D"),
        41386430 => Some("ImportLog"),
        55640938 => Some("GraphicsStateCollection"),
        73398921 => Some("VFXRenderer"),
        76251197 => Some("SerializableManagedRefTestClass"),
        156049354 => Some("Grid"),
        156483287 => Some("ScenesUsingAssets"),
        171741748 => Some("ArticulationBody"),
        181963792 => Some("Preset"),
        277625683 => Some("EmptyObject"),
        293259124 => Some("TestObjectWithSpecialLayoutOne"),
        294290339 => Some("AssemblyDefinitionReferenceImporter"),
        334799969 => Some("SiblingDerived"),
        342846651 => Some("TestObjectWithSerializedMapStringNonAlignedStruct"),
        355983997 => Some("AudioResource"),
        367388927 => Some("SubDerived"),
        369655926 => Some("AssetImportInProgressProxy"),
        382020655 => Some("PluginBuildInfo"),
        387306366 => Some("MemorySettings"),
        403037116 => Some("BuildMetaDataImporter"),
        403037117 => Some("BuildInstructionImporter"),
        426301858 => Some("EditorProjectAccess"),
        468431735 => Some("PrefabImporter"),
        478637458 => Some("TestObjectWithSerializedArray"),
        478637459 => Some("TestObjectWithSerializedAnimationCurve"),
        483693784 => Some("TilemapRenderer"),
        488575907 => Some("ScriptableCamera"),
        612988286 => Some("SpriteAtlasAsset"),
        638013454 => Some("SpriteAtlasDatabase"),
        641289076 => Some("AudioBuildInfo"),
        644342135 => Some("CachedSpriteAtlasRuntimeData"),
        655991488 => Some("MultiplayerManager"),
        662584278 => Some("AssemblyDefinitionReferenceAsset"),
        668709126 => Some("BuiltAssetBundleInfoSet"),
        687078895 => Some("SpriteAtlas"),
        702665669 => Some("DifferentMarshallingTestObject"),
        747330370 => Some("RayTracingShaderImporter"),
        780535461 => Some("BuildArchiveImporter"),
        815301076 => Some("PreviewImporter"),
        825902497 => Some("RayTracingShader"),
        850595691 => Some("LightingSettings"),
        877146078 => Some("PlatformModuleSetup"),
        890905787 => Some("VersionControlSettings"),
        893571522 => Some("CustomCollider2D"),
        895512359 => Some("AimConstraint"),
        937362698 => Some("VFXManager"),
        947337230 => Some("RoslynAnalyzerConfigAsset"),
        954905827 => Some("RuleSetFileAsset"),
        994735403 => Some("VisualEffectSubgraphOperator"),
        994735404 => Some("VisualEffectSubgraphBlock"),
        1001480554 => Some("Prefab"),
        1027052791 => Some("LocalizationImporter"),
        1091556383 => Some("Derived"),
        1111377672 => Some("PropertyModificationsTargetTestObject"),
        1114811875 => Some("ReferencesArtifactGenerator"),
        1152215463 => Some("AssemblyDefinitionAsset"),
        1154873562 => Some("SceneVisibilityState"),
        1183024399 => Some("LookAtConstraint"),
        1210832254 => Some("SpriteAtlasImporter"),
        1223240404 => Some("MultiArtifactTestImporter"),
        1233149941 => Some("AudioContainerElement"),
        1268269756 => Some("GameObjectRecorder"),
        1307931743 => Some("AudioRandomContainer"),
        1325145578 => Some("LightingDataAssetParent"),
        1386491679 => Some("PresetManager"),
        1392443030 => Some("TestObjectWithSpecialLayoutTwo"),
        1403656975 => Some("StreamingManager"),
        1480428607 => Some("LowerResBlitTexture"),
        1521398425 => Some("VideoBuildInfo"),
        1541671625 => Some("C4DImporter"),
        1542919678 => Some("StreamingController"),
        1557264870 => Some("ShaderContainer"),
        1571458007 => Some("RenderPassAttachment"),
        1597193336 => Some("RoslynAdditionalFileAsset"),
        1628831178 => Some("TestObjectVectorPairStringBool"),
        1642787288 => Some("RoslynAdditionalFileImporter"),
        1652712579 => Some("MultiplayerRolesData"),
        1660057539 => Some("SceneRoots"),
        1731078267 => Some("BrokenPrefabAsset"),
        1736697216 => Some("AndroidAssetPackImporter"),
        1740304944 => Some("VulkanDeviceFilterLists"),
        1766753193 => Some("AssemblyDefinitionImporter"),
        1773428102 => Some("ParentConstraint"),
        1777034230 => Some("RuleSetFileImporter"),
        1818360608 => Some("PositionConstraint"),
        1818360609 => Some("RotationConstraint"),
        1818360610 => Some("ScaleConstraint"),
        1839735485 => Some("Tilemap"),
        1896753125 => Some("PackageManifest"),
        1896753126 => Some("PackageManifestImporter"),
        1903396204 => Some("RoslynAnalyzerConfigImporter"),
        1931382933 => Some("UIRenderer"),
        1953259897 => Some("TerrainLayer"),
        1971053207 => Some("SpriteShapeRenderer"),
        1977754360 => Some("NativeObjectType"),
        1981279845 => Some("TestObjectWithSerializedMapStringBool"),
        1995898324 => Some("SerializableManagedHost"),
        2058629509 => Some("VisualEffectAsset"),
        2058629510 => Some("VisualEffectImporter"),
        2058629511 => Some("VisualEffectResource"),
        2083052967 => Some("VisualEffect"),
        2083778819 => Some("LocalizationAsset"),
        2089858483 => Some("ScriptedImporter"),
        2103361453 => Some("ShaderIncludeImporter"),
        2126867596 => Some("TilemapEditorUserSettings"),
        _ => None,
    }
}

/// `RawDataObject.ClassName` over a caller-supplied class ID: the `ClassIDType`
/// member name for defined IDs, otherwise the decimal ID. This mirrors
/// `((ClassIDType)ClassID).ToString()`, which renders undefined enum values
/// numerically. Recovery naming (`Unknown…`/`Unreadable…` plus the data hash,
/// factory-side) consumes this fact.
pub fn raw_data_class_name(class_id: i32) -> Cow<'static, str> {
    match class_id_type_name(class_id) {
        Some(name) => Cow::Borrowed(name),
        None => Cow::Owned(class_id.to_string()),
    }
}

impl SerializedFileMetadata {
    /// Decode one object's payload using its resolved serialized type and TypeTree.
    ///
    /// A missing TypeTree is reported explicitly. This method does not claim coverage for the
    /// source project's no-TypeTree type recovery, MonoBehaviour assembly resolver, or managed
    /// reference registry paths; it reports the reader-side usability fact
    /// (`classify_type_tree_usability`) those paths consume.
    pub fn decode_object_payload(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        object_index: usize,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        self.decode_object_payload_with_limits(
            file_bytes,
            header,
            object_index,
            &ObjectDecodeLimits::default(),
        )
    }

    /// Decode one object using caller-selected bounds for dynamic allocations and recursion.
    pub fn decode_object_payload_with_limits(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        object_index: usize,
        limits: &ObjectDecodeLimits,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        // The source refuses pre-3.5 assets before reading anything; refused payloads
        // flow into the existing raw-retention path downstream like any decode failure.
        self.ensure_object_version_supported(object_index)?;
        header
            .validate_for_file_size(file_bytes.len())
            .map_err(ObjectDecodeError::from_header)?;
        let object = self.objects.get(object_index).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::InvalidObjectIndex {
                    object_index,
                    object_count: self.objects.len(),
                },
            )
        })?;
        let type_index = object.resolved_type_index.ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectTypeUnresolved { object_index },
            )
        })?;
        let serialized_type = self.types.get(type_index).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::InvalidResolvedTypeIndex {
                    object_index,
                    type_index,
                    type_count: self.types.len(),
                },
            )
        })?;
        let type_tree = match classify_type_tree_usability(serialized_type.type_tree.as_ref()) {
            TypeTreeUsability::Missing => {
                return Err(ObjectDecodeError::new(
                    0,
                    ObjectDecodeErrorKind::MissingTypeTree { object_index },
                ));
            }
            TypeTreeUsability::Empty => {
                return Err(ObjectDecodeError::new(
                    0,
                    ObjectDecodeErrorKind::EmptyTypeTree { object_index },
                ));
            }
            TypeTreeUsability::Present => serialized_type.type_tree.as_ref().ok_or_else(|| {
                ObjectDecodeError::new(0, ObjectDecodeErrorKind::MissingTypeTree { object_index })
            })?,
        };
        let mut budget = DecodeBudget {
            limits,
            allocated_bytes: 0,
            managed_type_references: &self.reference_types,
            managed_reference_type_resolver: None,
            stream_offset: 0,
            managed_registry_depth: 0,
        };
        self.decode_object_payload_from_tree(
            file_bytes,
            header,
            ObjectDecodeTarget {
                index: object_index,
                object,
                fallback_type_id: serialized_type.raw_type_id,
            },
            type_tree,
            &mut budget,
        )
    }

    /// Decode one object with a source-supplied TypeTree. The asset factory uses this
    /// when a generated class has a usable versioned TPK layout but the file has no
    /// usable TypeTree.
    pub(crate) fn decode_object_payload_using_type_tree_with_limits(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        object_index: usize,
        type_tree: &TypeTree,
        limits: &ObjectDecodeLimits,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        self.ensure_object_version_supported(object_index)?;
        header
            .validate_for_file_size(file_bytes.len())
            .map_err(ObjectDecodeError::from_header)?;
        let object = self.objects.get(object_index).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::InvalidObjectIndex {
                    object_index,
                    object_count: self.objects.len(),
                },
            )
        })?;
        let mut budget = DecodeBudget {
            limits,
            allocated_bytes: 0,
            managed_type_references: &self.reference_types,
            managed_reference_type_resolver: None,
            stream_offset: 0,
            managed_registry_depth: 0,
        };
        self.decode_object_payload_from_tree(
            file_bytes,
            header,
            ObjectDecodeTarget {
                index: object_index,
                object,
                fallback_type_id: object.raw_type_id,
            },
            type_tree,
            &mut budget,
        )
    }

    fn ensure_object_version_supported(
        &self,
        object_index: usize,
    ) -> Result<(), ObjectDecodeError> {
        if let Some((major, minor)) = self
            .prefix
            .unity_version_text
            .as_deref()
            .and_then(version_major_minor)
        {
            if version_is_unsupported(major, minor) {
                return Err(ObjectDecodeError::new(
                    0,
                    ObjectDecodeErrorKind::UnreadableUnsupportedVersion { object_index },
                ));
            }
        }
        Ok(())
    }

    fn decode_object_payload_from_tree(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        target: ObjectDecodeTarget<'_>,
        type_tree: &TypeTree,
        budget: &mut DecodeBudget<'_>,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        let ObjectDecodeTarget {
            index: object_index,
            object,
            fallback_type_id,
        } = target;
        let type_root = type_tree.nodes.first().ok_or_else(|| {
            ObjectDecodeError::new(0, ObjectDecodeErrorKind::EmptyTypeTree { object_index })
        })?;
        let type_layout = TypeTreeLayout::new(type_tree, object_index, 0)?;
        let data_range = object.data_range(header).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let start = usize::try_from(data_range.start).map_err(|_| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let end = usize::try_from(data_range.end).map_err(|_| {
            ObjectDecodeError::new(
                start,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let payload = file_bytes.get(start..end).ok_or_else(|| {
            ObjectDecodeError::new(
                start,
                ObjectDecodeErrorKind::ObjectDataOutOfBounds {
                    object_index,
                    start: data_range.start,
                    end: data_range.end,
                    file_size: file_bytes.len(),
                },
            )
        })?;
        let mut reader = BinaryReader::with_base_offset(payload, self.prefix.body_endian, start)
            .map_err(ObjectDecodeError::from_binary)?;
        let arrays_align = unity_version_aligns_arrays(self.prefix.unity_version_text.as_deref());

        let root = decode_root(
            &mut reader,
            &type_layout,
            arrays_align,
            object_index,
            type_root.type_name.as_deref().unwrap_or(""),
            budget,
        )?;
        let consumed_bytes = reader.position();
        let trailing_length = reader.remaining();
        budget.charge(trailing_length, reader.offset(), "trailing object bytes")?;
        let trailing_bytes = reader
            .read_exact(reader.remaining())
            .map_err(|error| binary_error(error, "trailing object payload bytes"))?
            .to_vec();

        Ok(ParsedObjectPayload {
            object_index,
            path_id: object.path_id,
            type_name: type_root
                .type_name
                .clone()
                .unwrap_or_else(|| format!("type {fallback_type_id}")),
            root,
            consumed_bytes,
            trailing_bytes,
            allocated_bytes: budget.allocated_bytes,
        })
    }
}

/// Decode one freestanding payload against a caller-built node list (managed
/// reference registry entries, whose trees come from the type resolver rather
/// than a file). The node list must be level-nested exactly like a file
/// TypeTree. Managed-reference ID nodes use the same ID representation as
/// ordinary file-object fields. Nested registries inherit the owning object
/// stream offset and registry-depth bound.
pub(crate) fn decode_typetree_payload(
    type_tree: &TypeTree,
    payload: &[u8],
    endian: Endian,
    arrays_align: bool,
    managed_type_references: &[super::metadata::SerializedTypeReference],
    context: ManagedPayloadDecodeContext<'_>,
    limits: &ObjectDecodeLimits,
) -> Result<(SerializedValue, usize, usize), ObjectDecodeError> {
    decode_typetree_payload_with_resolver(
        type_tree,
        payload,
        endian,
        arrays_align,
        managed_type_references,
        context,
        limits,
    )
}

pub(crate) fn decode_typetree_payload_with_resolver(
    type_tree: &TypeTree,
    payload: &[u8],
    endian: Endian,
    arrays_align: bool,
    managed_type_references: &[super::metadata::SerializedTypeReference],
    context: ManagedPayloadDecodeContext<'_>,
    limits: &ObjectDecodeLimits,
) -> Result<(SerializedValue, usize, usize), ObjectDecodeError> {
    let mut budget = DecodeBudget {
        limits,
        allocated_bytes: 0,
        managed_type_references,
        managed_reference_type_resolver: context.managed_reference_type_resolver,
        stream_offset: context.stream_offset,
        managed_registry_depth: context.managed_registry_depth,
    };
    let type_root = type_tree.nodes.first().ok_or_else(|| {
        ObjectDecodeError::new(
            0,
            ObjectDecodeErrorKind::EmptyTypeTree {
                object_index: context.entry_index,
            },
        )
    })?;
    let type_layout = TypeTreeLayout::new(type_tree, context.entry_index, 0)?;
    let mut reader = BinaryReader::with_base_offset(payload, endian, context.stream_offset)
        .map_err(ObjectDecodeError::from_binary)?;
    let root = decode_root(
        &mut reader,
        &type_layout,
        arrays_align,
        context.entry_index,
        type_root.type_name.as_deref().unwrap_or(""),
        &mut budget,
    )?;
    Ok((root, reader.position(), budget.allocated_bytes))
}

fn decode_root(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    arrays_align: bool,
    object_index: usize,
    root_name: &str,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedValue, ObjectDecodeError> {
    decode_node_value(
        reader,
        type_tree,
        0,
        arrays_align,
        object_index,
        root_name,
        0,
        false,
        budget,
    )
}

// Keep the reader, tree, field identity, version rule, and allocation budget explicit at this
// parser boundary; bundling these per-call values in mutable global state would hide provenance.
#[allow(clippy::too_many_arguments)]
fn decode_field(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    arrays_align: bool,
    object_index: usize,
    parent_path: &str,
    depth: usize,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedFieldValue, ObjectDecodeError> {
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    let field_name = node.name.as_deref().unwrap_or("");
    let path = if parent_path.is_empty() {
        field_name.to_owned()
    } else if field_name.is_empty() {
        parent_path.to_owned()
    } else {
        format!("{parent_path}.{field_name}")
    };
    let align_after = field_aligns(type_tree, node_index, object_index, reader.offset())?;
    let value = decode_node_value(
        reader,
        type_tree,
        node_index,
        arrays_align,
        object_index,
        &path,
        depth,
        align_after,
        budget,
    )?;
    if align_after {
        reader
            .align_relative(4)
            .map_err(|error| binary_error(error, &format!("{path} alignment")))?;
    }
    Ok(SerializedFieldValue {
        name: field_name.to_owned(),
        value,
    })
}

#[allow(clippy::too_many_arguments)]
fn serialized_value_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "i8",
        SerializedValue::U8(_) => "u8",
        SerializedValue::I16(_) => "i16",
        SerializedValue::U16(_) => "u16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "i32",
        SerializedValue::U32(_) => "u32",
        SerializedValue::I64(_) => "i64",
        SerializedValue::U64(_) => "u64",
        SerializedValue::F32(_) => "f32",
        SerializedValue::F64(_) => "f64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::ManagedReferenceId { .. } => "managed-reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed-reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_node_value(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    arrays_align: bool,
    object_index: usize,
    path: &str,
    depth: usize,
    align_nested_arrays: bool,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedValue, ObjectDecodeError> {
    if depth > budget.limits.max_type_tree_depth {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::TypeTreeDepthExceeded {
                object_index,
                path: path.to_owned(),
                depth,
            },
        ));
    }
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    let type_name = node.type_name.as_deref().unwrap_or("");
    let node_name = node.name.as_deref().unwrap_or("");

    let children = direct_children(type_tree, node_index, object_index, reader.offset())?;
    if is_managed_reference_element(type_tree, node_index, object_index, reader.offset())? {
        let is_indexed = is_indexed_managed_reference_element(
            type_tree,
            node_index,
            object_index,
            reader.offset(),
        )?;
        let child_index = *children.first().ok_or_else(|| {
            ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::UnsupportedManagedReferences {
                    object_index,
                    type_name: type_name.to_owned(),
                    field_name: node_name.to_owned(),
                    is_indexed,
                },
            )
        })?;
        let identifier = decode_field(
            reader,
            type_tree,
            child_index,
            arrays_align,
            object_index,
            path,
            depth + 1,
            budget,
        )?;
        let reference_id = match identifier.value {
            SerializedValue::I32(value) if is_indexed => i64::from(value),
            SerializedValue::I64(value) if !is_indexed => value,
            value => {
                return Err(ObjectDecodeError::new(
                    reader.offset(),
                    ObjectDecodeErrorKind::InvalidManagedReferenceId {
                        object_index,
                        path: path.to_owned(),
                        indexed: is_indexed,
                        value: serialized_value_kind(&value).to_owned(),
                    },
                ));
            }
        };
        return Ok(SerializedValue::ManagedReferenceId {
            reference_id,
            indexed: is_indexed,
        });
    }

    if is_managed_references_registry(type_name, node_name, children.len()) {
        let registry_depth = budget
            .managed_registry_depth
            .checked_add(1)
            .ok_or_else(|| {
                ObjectDecodeError::new(
                    reader.offset(),
                    ObjectDecodeErrorKind::ManagedRegistryDepthExceeded {
                        object_index,
                        path: path.to_owned(),
                        requested: usize::MAX,
                        limit: budget.limits.max_managed_registry_depth,
                    },
                )
            })?;
        if registry_depth > budget.limits.max_managed_registry_depth {
            return Err(ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::ManagedRegistryDepthExceeded {
                    object_index,
                    path: path.to_owned(),
                    requested: registry_depth,
                    limit: budget.limits.max_managed_registry_depth,
                },
            ));
        }
        let registry_limits = ObjectDecodeLimits {
            max_decoded_allocation_bytes: budget.remaining_allocation_bytes(),
            ..*budget.limits
        };
        let options = super::managed_registry::ManagedRegistryReadOptions {
            endian: reader.endian(),
            arrays_align,
            stream_offset: budget
                .stream_offset
                .checked_add(reader.position())
                .ok_or_else(|| {
                    ObjectDecodeError::new(
                        reader.offset(),
                        ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
                    )
                })?,
            registry_depth,
            object_decode_limits: registry_limits,
            max_entries: budget.limits.max_array_elements,
            max_total_decoded_allocation_bytes: budget.remaining_allocation_bytes(),
        };
        let registry = super::managed_registry::read_managed_registry_with_resolver(
            reader.remaining_slice(),
            options,
            budget.managed_type_references,
            budget.managed_reference_type_resolver,
        )
        .map_err(|error| {
            ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::ManagedRegistryDecode {
                    object_index,
                    path: path.to_owned(),
                    message: error.to_string(),
                },
            )
        })?;
        reader
            .skip(registry.consumed_bytes)
            .map_err(ObjectDecodeError::from_binary)?;
        budget.charge(
            registry.allocated_bytes,
            reader.offset(),
            "managed reference registry",
        )?;
        return Ok(SerializedValue::ManagedReferenceRegistry(Box::new(
            registry,
        )));
    }

    if is_referenced_object_data(type_name, node_name, children.len()) {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::UnsupportedReferencedObjectData {
                object_index,
                path: path.to_owned(),
            },
        ));
    }

    if matches!(type_name, "string" | "Utf8String")
        && is_string_node(type_tree, node_index, object_index, reader.offset())?
    {
        let length = read_i32(reader, &format!("{path} string byte length"))?;
        let length = nonnegative_length(length, reader, object_index, path)?;
        if length > budget.limits.max_string_bytes {
            return Err(decode_limit_error(
                reader.offset(),
                format!("{path} string bytes"),
                length,
                budget.limits.max_string_bytes,
            ));
        }
        if length > reader.remaining() {
            return Err(ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::LengthExceedsPayload {
                    object_index,
                    path: path.to_owned(),
                    length,
                    remaining: reader.remaining(),
                },
            ));
        }
        budget.charge(length, reader.offset(), "decoded string bytes")?;
        let bytes = reader
            .read_exact(length)
            .map_err(|error| binary_error(error, &format!("{path} string bytes")))?
            .to_vec();
        reader
            .align_relative(4)
            .map_err(|error| binary_error(error, &format!("{path} string alignment")))?;
        return Ok(SerializedValue::StringBytes(bytes));
    }

    if type_name == "TypelessData"
        && array_shape(type_tree, node_index, object_index, reader.offset())?.is_some()
    {
        let length = read_i32(reader, &format!("{path} typeless data length"))?;
        let length = nonnegative_length(length, reader, object_index, path)?;
        if length > budget.limits.max_array_elements {
            return Err(decode_limit_error(
                reader.offset(),
                format!("{path} typeless data bytes"),
                length,
                budget.limits.max_array_elements,
            ));
        }
        if length > reader.remaining() {
            return Err(ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::LengthExceedsPayload {
                    object_index,
                    path: path.to_owned(),
                    length,
                    remaining: reader.remaining(),
                },
            ));
        }
        budget.charge(length, reader.offset(), "decoded typeless data bytes")?;
        let bytes = reader
            .read_exact(length)
            .map_err(|error| binary_error(error, &format!("{path} typeless data")))?;
        let result = SerializedValue::Bytes(bytes.to_vec());
        if arrays_align || align_nested_arrays {
            reader
                .align_relative(4)
                .map_err(|error| binary_error(error, &format!("{path} array alignment")))?;
        }
        return Ok(result);
    }

    if let Some(array_node_index) =
        array_node_for_value(type_tree, node_index, object_index, reader.offset())?
    {
        return decode_array(
            reader,
            type_tree,
            array_node_index,
            arrays_align,
            object_index,
            path,
            depth + 1,
            align_nested_arrays,
            budget,
        );
    }

    if let Some((file_id_node, path_id_node)) =
        pptr_shape(type_tree, node_index, object_index, reader.offset())?
    {
        let pointer_offset = reader.offset();
        let file_id = decode_field(
            reader,
            type_tree,
            file_id_node,
            arrays_align,
            object_index,
            path,
            depth + 1,
            budget,
        )?;
        let path_id = decode_field(
            reader,
            type_tree,
            path_id_node,
            arrays_align,
            object_index,
            path,
            depth + 1,
            budget,
        )?;
        let file_id_value = match file_id.value {
            SerializedValue::I32(value) => value,
            _ => {
                return Err(invalid_pptr_component(
                    pointer_offset,
                    object_index,
                    path,
                    &file_id.name,
                    type_tree.nodes[file_id_node]
                        .type_name
                        .as_deref()
                        .unwrap_or(""),
                ));
            }
        };
        let path_id_value = match path_id.value {
            SerializedValue::I32(value) => i64::from(value),
            SerializedValue::I64(value) => value,
            _ => {
                return Err(invalid_pptr_component(
                    pointer_offset,
                    object_index,
                    path,
                    &path_id.name,
                    type_tree.nodes[path_id_node]
                        .type_name
                        .as_deref()
                        .unwrap_or(""),
                ));
            }
        };
        return Ok(SerializedValue::PPtr {
            file_id: file_id_value,
            path_id: path_id_value,
        });
    }

    if let Some(value) = decode_primitive(
        reader,
        type_name,
        node.meta_flags & CHAR_PROPERTY_MASK_FLAG != 0,
        path,
    )? {
        return Ok(value);
    }

    if !children.is_empty() {
        let mut fields = reserve_fields(children.len(), reader.offset(), path, budget)?;
        for child_index in children {
            fields.push(decode_field(
                reader,
                type_tree,
                child_index,
                arrays_align,
                object_index,
                path,
                depth + 1,
                budget,
            )?);
        }
        return Ok(SerializedValue::Struct(fields));
    }

    let raw_size = node.byte_size;
    let size = usize::try_from(raw_size).map_err(|_| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::UnknownLeafWithoutSize {
                object_index,
                path: path.to_owned(),
                type_name: type_name.to_owned(),
                byte_size: raw_size,
            },
        )
    })?;
    if size > reader.remaining() {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::LengthExceedsPayload {
                object_index,
                path: path.to_owned(),
                length: size,
                remaining: reader.remaining(),
            },
        ));
    }
    budget.charge(size, reader.offset(), "unknown raw leaf bytes")?;
    reader
        .read_exact(size)
        .map(|bytes| SerializedValue::Raw {
            type_name: type_name.to_owned(),
            bytes: bytes.to_vec(),
        })
        .map_err(|error| binary_error(error, &format!("{path} raw field")))
}

#[allow(clippy::too_many_arguments)]
fn decode_array(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    array_node_index: usize,
    arrays_align: bool,
    object_index: usize,
    path: &str,
    depth: usize,
    align_nested_arrays: bool,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedValue, ObjectDecodeError> {
    let (size_node, data_node) =
        array_shape(type_tree, array_node_index, object_index, reader.offset())?.ok_or_else(
            || {
                ObjectDecodeError::new(
                    reader.offset(),
                    ObjectDecodeErrorKind::InvalidArrayShape {
                        object_index,
                        path: path.to_owned(),
                        node_index: array_node_index,
                    },
                )
            },
        )?;
    let count = read_i32(reader, &format!("{path} array count"))?;
    let count = nonnegative_length(count, reader, object_index, path)?;
    if count > budget.limits.max_array_elements {
        return Err(decode_limit_error(
            reader.offset(),
            format!("{path} array elements"),
            count,
            budget.limits.max_array_elements,
        ));
    }
    let minimum_size =
        minimum_encoded_size(type_tree, data_node, object_index, reader.offset(), 0)?;
    if count > 0 && minimum_size == 0 {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::ZeroWidthArrayElement {
                object_index,
                path: path.to_owned(),
                count,
            },
        ));
    }
    let minimum_bytes = count.checked_mul(minimum_size).ok_or_else(|| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::ArraySizeOverflow {
                object_index,
                path: path.to_owned(),
                count,
                minimum_element_size: minimum_size,
            },
        )
    })?;
    if minimum_bytes > reader.remaining() {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::ArrayExceedsPayload {
                object_index,
                path: path.to_owned(),
                count,
                minimum_element_size: minimum_size,
                required: minimum_bytes,
                remaining: reader.remaining(),
            },
        ));
    }

    let mut values = reserve_values(count, reader.offset(), path, budget)?;
    for index in 0..count {
        let element_path = format!("{path}[{index}]");
        values.push(decode_node_value(
            reader,
            type_tree,
            data_node,
            arrays_align,
            object_index,
            &element_path,
            depth + 1,
            align_nested_arrays,
            budget,
        )?);
    }
    if align_nested_arrays
        || (arrays_align
            && array_base_is_primitive(type_tree, data_node, object_index, reader.offset())?)
    {
        reader
            .align_relative(4)
            .map_err(|error| binary_error(error, &format!("{path} array alignment")))?;
    }
    let _ = size_node;
    Ok(SerializedValue::Array(values))
}

fn decode_primitive(
    reader: &mut BinaryReader<'_>,
    type_name: &str,
    treat_integer_as_char: bool,
    path: &str,
) -> Result<Option<SerializedValue>, ObjectDecodeError> {
    let result = match type_name {
        "bool" => Some(SerializedValue::Bool(read_u8(reader, path)? != 0)),
        "char" | "UInt8" => Some(SerializedValue::U8(read_u8(reader, path)?)),
        "SInt8" => Some(SerializedValue::I8(read_i8(reader, path)?)),
        "short" | "SInt16" => Some(SerializedValue::I16(read_i16(reader, path)?)),
        "ushort" | "UInt16" | "unsigned short" => Some(if treat_integer_as_char {
            SerializedValue::Char(read_u16(reader, path)?)
        } else {
            SerializedValue::U16(read_u16(reader, path)?)
        }),
        "int" | "SInt32" | "Type*" => Some(SerializedValue::I32(read_i32(reader, path)?)),
        "uint" | "UInt32" | "unsigned int" => Some(SerializedValue::U32(read_u32(reader, path)?)),
        "SInt64" | "long long" => Some(SerializedValue::I64(read_i64(reader, path)?)),
        "UInt64" | "FileSize" | "unsigned long long" => {
            Some(SerializedValue::U64(read_u64(reader, path)?))
        }
        "float" => Some(SerializedValue::F32(read_f32(reader, path)?)),
        "double" => Some(SerializedValue::F64(read_f64(reader, path)?)),
        _ => None,
    };
    Ok(result)
}

fn array_node_for_value(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<Option<usize>, ObjectDecodeError> {
    if array_shape(type_tree, node_index, object_index, offset)?.is_some() {
        return Ok(Some(node_index));
    }
    let node = &type_tree.nodes[node_index];
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 1 {
        return Ok(None);
    }
    let child_index = children[0];
    let child = &type_tree.nodes[child_index];
    let is_vector = matches!(
        node.type_name.as_deref(),
        Some("vector" | "staticvector" | "set" | "map")
    );
    let is_named_vector = if child.name.as_deref() == Some("Array") {
        if let Some((_, element_index)) = array_shape(type_tree, child_index, object_index, offset)?
        {
            let element_type = type_tree.nodes[element_index].type_name.as_deref();
            element_type == node.type_name.as_deref()
                || element_type == Some("Generic Mono")
                || is_managed_reference_element(type_tree, element_index, object_index, offset)?
        } else {
            false
        }
    } else {
        false
    };
    if (is_vector || is_named_vector)
        && array_shape(type_tree, child_index, object_index, offset)?.is_some()
    {
        Ok(Some(child_index))
    } else {
        Ok(None)
    }
}

fn array_shape(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<Option<(usize, usize)>, ObjectDecodeError> {
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    if !matches!(node.type_name.as_deref(), Some("Array" | "TypelessData")) {
        return Ok(None);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 2
        || type_tree.nodes[children[0]].name.as_deref() != Some("size")
        || type_tree.nodes[children[1]].name.as_deref() != Some("data")
        || !direct_children(type_tree, children[0], object_index, offset)?.is_empty()
    {
        return Ok(None);
    }
    Ok(Some((children[0], children[1])))
}

fn is_string_node(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 1 {
        return Ok(false);
    }
    let Some((_, data_node)) = array_shape(type_tree, children[0], object_index, offset)? else {
        return Ok(false);
    };
    let data = &type_tree.nodes[data_node];
    Ok(matches!(data.type_name.as_deref(), Some("char" | "UInt8"))
        && direct_children(type_tree, data_node, object_index, offset)?.is_empty())
}

/// Matches AssetRipper's TypeTree PPtr shape test: a `PPtr<...>` node with exactly two leaf
/// children named `m_FileID` and `m_PathID` in that order.
fn pptr_shape(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<Option<(usize, usize)>, ObjectDecodeError> {
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    let type_name = type_name(node);
    if !type_name.starts_with("PPtr<") || !type_name.ends_with('>') {
        return Ok(None);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 2
        || type_tree.nodes[children[0]].name.as_deref() != Some("m_FileID")
        || type_tree.nodes[children[1]].name.as_deref() != Some("m_PathID")
        || !direct_children(type_tree, children[0], object_index, offset)?.is_empty()
        || !direct_children(type_tree, children[1], object_index, offset)?.is_empty()
    {
        return Ok(None);
    }
    Ok(Some((children[0], children[1])))
}

fn invalid_pptr_component(
    offset: usize,
    object_index: usize,
    path: &str,
    field_name: &str,
    type_name: &str,
) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::InvalidPPtrComponent {
            object_index,
            path: path.to_owned(),
            field_name: field_name.to_owned(),
            type_name: type_name.to_owned(),
        },
    )
}

fn field_aligns(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let mut current = node_index;
    let mut aligned = false;
    let mut visited = 0;
    loop {
        let node = type_tree.nodes.get(current).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex {
                    node_index: current,
                },
            )
        })?;
        aligned |= node.meta_flags & ALIGN_BYTES_FLAG != 0;
        if visited > MAX_TYPE_TREE_DEPTH {
            return Err(ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::TypeTreeDepthExceeded {
                    object_index,
                    path: format!("TypeTree node {node_index}"),
                    depth: visited,
                },
            ));
        }
        visited += 1;
        if let Some((_, data_node)) = array_shape(type_tree, current, object_index, offset)? {
            current = data_node;
            continue;
        }
        let node = &type_tree.nodes[current];
        let children = direct_children(type_tree, current, object_index, offset)?;
        if children.len() == 1
            && (matches!(
                node.type_name.as_deref(),
                Some("vector" | "staticvector" | "set" | "map")
            ) || (type_tree.nodes[children[0]].name.as_deref() == Some("Array")
                && array_shape(type_tree, children[0], object_index, offset)?.is_some()))
        {
            current = children[0];
            continue;
        }
        return Ok(aligned);
    }
}

fn array_base_is_primitive(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let mut current = node_index;
    for _ in 0..=MAX_TYPE_TREE_DEPTH {
        if matches!(
            type_tree.nodes[current].type_name.as_deref(),
            Some("string" | "Utf8String")
        ) && is_string_node(type_tree, current, object_index, offset)?
        {
            return Ok(true);
        }
        if decode_primitive_name(type_name(&type_tree.nodes[current])).is_some() {
            return Ok(true);
        }
        if let Some((_, data_node)) = array_shape(type_tree, current, object_index, offset)? {
            current = data_node;
            continue;
        }
        let children = direct_children(type_tree, current, object_index, offset)?;
        if children.len() == 1 {
            current = children[0];
            continue;
        }
        return Ok(false);
    }
    Err(ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::TypeTreeDepthExceeded {
            object_index,
            path: format!("TypeTree node {node_index}"),
            depth: MAX_TYPE_TREE_DEPTH + 1,
        },
    ))
}

fn minimum_encoded_size(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
    depth: usize,
) -> Result<usize, ObjectDecodeError> {
    if depth > MAX_TYPE_TREE_DEPTH {
        return Err(ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::TypeTreeDepthExceeded {
                object_index,
                path: format!("TypeTree node {node_index}"),
                depth,
            },
        ));
    }
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    if matches!(node.type_name.as_deref(), Some("string" | "Utf8String"))
        && is_string_node(type_tree, node_index, object_index, offset)?
    {
        return Ok(4);
    }
    if array_shape(type_tree, node_index, object_index, offset)?.is_some()
        || array_node_for_value(type_tree, node_index, object_index, offset)?.is_some()
    {
        return Ok(4);
    }
    if let Some(size) = primitive_width(type_name(node)) {
        return Ok(size);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if !children.is_empty() {
        let mut total = 0usize;
        for child in children {
            total = total
                .checked_add(minimum_encoded_size(
                    type_tree,
                    child,
                    object_index,
                    offset,
                    depth + 1,
                )?)
                .ok_or_else(|| {
                    ObjectDecodeError::new(offset, ObjectDecodeErrorKind::LayoutOverflow)
                })?;
        }
        return Ok(total);
    }
    usize::try_from(node.byte_size).map_err(|_| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::UnknownLeafWithoutSize {
                object_index,
                path: format!("TypeTree node {node_index}"),
                type_name: type_name(node).to_owned(),
                byte_size: node.byte_size,
            },
        )
    })
}

/// `TypeTreeNodeStruct.IsManagedReference` for an array element node: the element type
/// is `managedReference` (or `managedRefArrayItem`) holding one `rid` (or legacy
/// `id`) child. This is the predicate `IsNamedVector` applies to SerializeReference
/// array elements.
fn is_managed_reference_element(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let node = &type_tree.nodes[node_index];
    if !matches!(
        node.type_name.as_deref(),
        Some(
            managed_names::MANAGED_REFERENCE_NAME
                | managed_names::MANAGED_REFERENCE_ARRAY_ITEM_NAME
        )
    ) {
        return Ok(false);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    Ok(children.len() == 1
        && matches!(
            type_tree.nodes[children[0]].name.as_deref(),
            Some(managed_names::REFERENCE_ID_FIELD_NAME | managed_names::INDEX_FIELD_NAME)
        ))
}

/// `TypeTreeNodeStruct.IsIndexedManagedReference` for an array element node: a
/// managed-reference element (see `is_managed_reference_element`) whose identifier
/// child is the legacy `id` field. The source uses this to select the
/// `IndexedManagedReference` field shape over the `rid`-addressed
/// `ManagedReference` shape.
fn is_indexed_managed_reference_element(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    if !is_managed_reference_element(type_tree, node_index, object_index, offset)? {
        return Ok(false);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    Ok(children.len() == 1
        && type_tree.nodes[children[0]].name.as_deref() == Some(managed_names::INDEX_FIELD_NAME))
}

/// `TypeTreeNodeStruct.IsManagedReferencesRegistry` over caller facts: the
/// `ManagedReferencesRegistry` type holding the `references` field with more than one
/// subnode (the identifier plus at least one referenced object).
fn is_managed_references_registry(type_name: &str, node_name: &str, child_count: usize) -> bool {
    type_name == managed_names::REGISTRY_NAME
        && node_name == managed_names::REGISTRY_FIELD_NAME
        && child_count > 1
}

/// `TypeTreeNodeStruct.IsReferencedObjectData` over caller facts: the
/// `ReferencedObjectData` dynamic payload holder, which carries no subnodes because the
/// object content is read dynamically, not from the tree. A same-named node WITH
/// children is an ordinary struct here, not dynamic data.
fn is_referenced_object_data(type_name: &str, node_name: &str, child_count: usize) -> bool {
    type_name == managed_names::REFERENCED_OBJECT_DATA_NAME
        && node_name == managed_names::DATA_FIELD_NAME
        && child_count == 0
}

fn direct_children(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    _object_index: usize,
    offset: usize,
) -> Result<Vec<usize>, ObjectDecodeError> {
    let mut children = Vec::new();
    let mut child = *type_tree.first_child.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    while let Some(child_index) = child {
        children
            .try_reserve(1)
            .map_err(|_| allocation_error(offset, "TypeTree children", children.len() + 1))?;
        children.push(child_index);
        child = type_tree.next_sibling[child_index];
    }
    Ok(children)
}

fn reserve_option_indices(
    count: usize,
    offset: usize,
) -> Result<Vec<Option<usize>>, ObjectDecodeError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| allocation_error(offset, "TypeTree child index", count))?;
    values.resize(count, None);
    Ok(values)
}

fn invalid_tree_levels(
    offset: usize,
    object_index: usize,
    node_index: usize,
    child_index: usize,
) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::InvalidTypeTreeLevels {
            object_index,
            node_index,
            child_index,
        },
    )
}

fn reserve_values(
    count: usize,
    offset: usize,
    path: &str,
    budget: &mut DecodeBudget<'_>,
) -> Result<Vec<SerializedValue>, ObjectDecodeError> {
    budget.charge_items::<SerializedValue>(count, offset, path)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| allocation_error(offset, path, count))?;
    Ok(values)
}

fn reserve_fields(
    count: usize,
    offset: usize,
    path: &str,
    budget: &mut DecodeBudget<'_>,
) -> Result<Vec<SerializedFieldValue>, ObjectDecodeError> {
    budget.charge_items::<SerializedFieldValue>(count, offset, path)?;
    let mut fields = Vec::new();
    fields
        .try_reserve_exact(count)
        .map_err(|_| allocation_error(offset, path, count))?;
    Ok(fields)
}

fn nonnegative_length(
    value: i32,
    reader: &BinaryReader<'_>,
    object_index: usize,
    path: &str,
) -> Result<usize, ObjectDecodeError> {
    usize::try_from(value).map_err(|_| {
        ObjectDecodeError::new(
            reader.offset().saturating_sub(4),
            ObjectDecodeErrorKind::NegativeArrayLength {
                object_index,
                path: path.to_owned(),
                length: value,
            },
        )
    })
}

fn primitive_width(type_name: &str) -> Option<usize> {
    match type_name {
        "bool" | "char" | "UInt8" | "SInt8" => Some(1),
        "short" | "SInt16" | "ushort" | "UInt16" | "unsigned short" => Some(2),
        "int" | "SInt32" | "Type*" | "uint" | "UInt32" | "unsigned int" | "float" => Some(4),
        "SInt64" | "long long" | "UInt64" | "FileSize" | "unsigned long long" | "double" => Some(8),
        _ => None,
    }
}

fn decode_primitive_name(type_name: &str) -> Option<()> {
    primitive_width(type_name).map(|_| ())
}

fn type_name(node: &super::metadata::TypeTreeNode) -> &str {
    node.type_name.as_deref().unwrap_or("")
}

fn allocation_error(offset: usize, path: &str, count: usize) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::AllocationFailed {
            path: path.to_owned(),
            count,
        },
    )
}

fn decode_limit_error(
    offset: usize,
    resource: String,
    requested: usize,
    limit: usize,
) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::DecodeLimitExceeded {
            resource,
            requested,
            limit,
        },
    )
}

macro_rules! read_primitive {
    ($name:ident, $ty:ty, $reader_method:ident) => {
        fn $name(reader: &mut BinaryReader<'_>, context: &str) -> Result<$ty, ObjectDecodeError> {
            reader
                .$reader_method()
                .map_err(|error| binary_error(error, context))
        }
    };
}

read_primitive!(read_u8, u8, read_u8);
read_primitive!(read_i8, i8, read_i8);
read_primitive!(read_u16, u16, read_u16);
read_primitive!(read_i16, i16, read_i16);
read_primitive!(read_u32, u32, read_u32);
read_primitive!(read_i32, i32, read_i32);
read_primitive!(read_u64, u64, read_u64);
read_primitive!(read_i64, i64, read_i64);
read_primitive!(read_f32, f32, read_f32);
read_primitive!(read_f64, f64, read_f64);

fn binary_error(error: BinaryError, context: &str) -> ObjectDecodeError {
    let offset = error.offset();
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::Binary(error.with_context(context.to_owned())),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectDecodeError {
    offset: usize,
    kind: ObjectDecodeErrorKind,
}

impl ObjectDecodeError {
    fn new(offset: usize, kind: ObjectDecodeErrorKind) -> Self {
        Self { offset, kind }
    }

    fn from_binary(error: BinaryError) -> Self {
        let offset = error.offset();
        Self::new(offset, ObjectDecodeErrorKind::Binary(error))
    }

    fn from_header(error: HeaderError) -> Self {
        let offset = error.offset();
        Self::new(offset, ObjectDecodeErrorKind::Header(error))
    }

    pub(crate) fn source_type_tree_lookup(
        object_index: usize,
        class_id: i32,
        version: String,
        message: String,
    ) -> Self {
        Self::new(
            0,
            ObjectDecodeErrorKind::SourceTypeTreeLookup {
                object_index,
                class_id,
                version,
                message,
            },
        )
    }

    pub(crate) fn factory_object_outcome(
        object_index: usize,
        class_id: i32,
        outcome: String,
        diagnostic: Option<String>,
    ) -> Self {
        Self::new(
            0,
            ObjectDecodeErrorKind::FactoryObjectOutcome {
                object_index,
                class_id,
                outcome,
                diagnostic,
            },
        )
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &ObjectDecodeErrorKind {
        &self.kind
    }
}

impl Display for ObjectDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ObjectDecodeErrorKind::Binary(error) => Display::fmt(error, f),
            ObjectDecodeErrorKind::Header(error) => Display::fmt(error, f),
            kind => write!(f, "{kind} at byte offset {}", self.offset),
        }
    }
}

impl Error for ObjectDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ObjectDecodeErrorKind::Binary(error) => Some(error),
            ObjectDecodeErrorKind::Header(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectDecodeErrorKind {
    Binary(BinaryError),
    Header(HeaderError),
    InvalidObjectIndex {
        object_index: usize,
        object_count: usize,
    },
    ObjectTypeUnresolved {
        object_index: usize,
    },
    /// The source factory retained this object as unknown or unreadable raw data.
    FactoryObjectOutcome {
        object_index: usize,
        class_id: i32,
        outcome: String,
        diagnostic: Option<String>,
    },
    /// The file declares a Unity version before 3.5, whose assets the source
    /// does not read.
    UnreadableUnsupportedVersion {
        object_index: usize,
    },
    InvalidResolvedTypeIndex {
        object_index: usize,
        type_index: usize,
        type_count: usize,
    },
    MissingTypeTree {
        object_index: usize,
    },
    SourceTypeTreeLookup {
        object_index: usize,
        class_id: i32,
        version: String,
        message: String,
    },
    EmptyTypeTree {
        object_index: usize,
    },
    UnsupportedManagedReferences {
        object_index: usize,
        type_name: String,
        field_name: String,
        /// The legacy `id` flavor (`IsIndexedManagedReference`) versus `rid`-addressed.
        is_indexed: bool,
    },
    InvalidManagedReferenceId {
        object_index: usize,
        path: String,
        indexed: bool,
        value: String,
    },
    ManagedRegistryDecode {
        object_index: usize,
        path: String,
        message: String,
    },
    UnsupportedReferencedObjectData {
        object_index: usize,
        path: String,
    },
    ManagedRegistryDepthExceeded {
        object_index: usize,
        path: String,
        requested: usize,
        limit: usize,
    },
    InvalidPPtrComponent {
        object_index: usize,
        path: String,
        field_name: String,
        type_name: String,
    },
    ObjectDataRangeOverflow {
        object_index: usize,
    },
    ObjectDataOutOfBounds {
        object_index: usize,
        start: u64,
        end: u64,
        file_size: usize,
    },
    InvalidTypeTreeNodeIndex {
        node_index: usize,
    },
    InvalidTypeTreeLevels {
        object_index: usize,
        node_index: usize,
        child_index: usize,
    },
    TypeTreeDepthExceeded {
        object_index: usize,
        path: String,
        depth: usize,
    },
    InvalidArrayShape {
        object_index: usize,
        path: String,
        node_index: usize,
    },
    NegativeArrayLength {
        object_index: usize,
        path: String,
        length: i32,
    },
    LengthExceedsPayload {
        object_index: usize,
        path: String,
        length: usize,
        remaining: usize,
    },
    ArraySizeOverflow {
        object_index: usize,
        path: String,
        count: usize,
        minimum_element_size: usize,
    },
    ArrayExceedsPayload {
        object_index: usize,
        path: String,
        count: usize,
        minimum_element_size: usize,
        required: usize,
        remaining: usize,
    },
    ZeroWidthArrayElement {
        object_index: usize,
        path: String,
        count: usize,
    },
    UnknownLeafWithoutSize {
        object_index: usize,
        path: String,
        type_name: String,
        byte_size: i32,
    },
    DecodeLimitExceeded {
        resource: String,
        requested: usize,
        limit: usize,
    },
    AllocationSizeOverflow {
        resource: String,
    },
    AllocationFailed {
        path: String,
        count: usize,
    },
    LayoutOverflow,
}

impl Display for ObjectDecodeErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(error) => Display::fmt(error, f),
            Self::Header(error) => Display::fmt(error, f),
            Self::InvalidObjectIndex {
                object_index,
                object_count,
            } => write!(
                f,
                "object index {object_index} is outside the {object_count}-entry object table"
            ),
            Self::ObjectTypeUnresolved { object_index } => {
                write!(f, "object {object_index} has no resolved serialized type")
            }
            Self::FactoryObjectOutcome {
                object_index,
                class_id,
                outcome,
                diagnostic,
            } => {
                write!(f, "object {object_index} class {class_id} produced {outcome}")?;
                if let Some(diagnostic) = diagnostic {
                    write!(f, ": {diagnostic}")?;
                }
                Ok(())
            }
            Self::UnreadableUnsupportedVersion { object_index } => write!(
                f,
                "object {object_index} comes from a Unity version before 3.5 and cannot be read"
            ),
            Self::InvalidResolvedTypeIndex {
                object_index,
                type_index,
                type_count,
            } => write!(
                f,
                "object {object_index} resolves to type {type_index}, outside the {type_count}-entry type table"
            ),
            Self::MissingTypeTree { object_index } => {
                write!(f, "object {object_index} has no TypeTree")
            }
            Self::SourceTypeTreeLookup {
                object_index,
                class_id,
                version,
                message,
            } => write!(
                f,
                "object {object_index} class {class_id} has no usable source TPK TypeTree at {version}: {message}"
            ),
            Self::EmptyTypeTree { object_index } => {
                write!(f, "object {object_index} references an empty TypeTree")
            }
            Self::UnsupportedManagedReferences {
                object_index,
                type_name,
                field_name,
                is_indexed,
            } => write!(
                f,
                "object {object_index} contains {} managed-reference TypeTree node {type_name} {field_name} without an ID child",
                if *is_indexed {
                    "legacy indexed"
                } else {
                    "rid-addressed"
                }
            ),
            Self::InvalidManagedReferenceId {
                object_index,
                path,
                indexed,
                value,
            } => write!(
                f,
                "object {object_index} field {path} has {value} for a {} managed-reference identifier",
                if *indexed { "legacy indexed" } else { "rid-addressed" }
            ),
            Self::ManagedRegistryDecode {
                object_index,
                path,
                message,
            } => write!(
                f,
                "object {object_index} managed-reference registry at {path} could not be decoded: {message}"
            ),
            Self::UnsupportedReferencedObjectData { object_index, path } => write!(
                f,
                "object {object_index} field {path} contains an unresolved ReferencedObjectData placeholder"
            ),
            Self::ManagedRegistryDepthExceeded {
                object_index,
                path,
                requested,
                limit,
            } => write!(
                f,
                "object {object_index} field {path} nests managed-reference registry depth {requested}, exceeding limit {limit}"
            ),
            Self::InvalidPPtrComponent {
                object_index,
                path,
                field_name,
                type_name,
            } => write!(
                f,
                "object {object_index} field {path} has unsupported PPtr component {field_name} of type {type_name}"
            ),
            Self::ObjectDataRangeOverflow { object_index } => {
                write!(f, "object {object_index} data range overflows the address space")
            }
            Self::ObjectDataOutOfBounds {
                object_index,
                start,
                end,
                file_size,
            } => write!(
                f,
                "object {object_index} payload range {start}..{end} exceeds input size {file_size}"
            ),
            Self::InvalidTypeTreeNodeIndex { node_index } => {
                write!(f, "TypeTree node index {node_index} is out of bounds")
            }
            Self::InvalidTypeTreeLevels {
                object_index,
                node_index,
                child_index,
            } => write!(
                f,
                "object {object_index} TypeTree node {child_index} has an invalid level relative to node {node_index}"
            ),
            Self::TypeTreeDepthExceeded {
                object_index,
                path,
                depth,
            } => write!(
                f,
                "object {object_index} field {path} exceeds TypeTree decode depth at {depth}"
            ),
            Self::InvalidArrayShape {
                object_index,
                path,
                node_index,
            } => write!(
                f,
                "object {object_index} field {path} has an invalid array node at index {node_index}"
            ),
            Self::NegativeArrayLength {
                object_index,
                path,
                length,
            } => write!(
                f,
                "object {object_index} field {path} has negative array/string length {length}"
            ),
            Self::LengthExceedsPayload {
                object_index,
                path,
                length,
                remaining,
            } => write!(
                f,
                "object {object_index} field {path} requires {length} bytes, but only {remaining} remain"
            ),
            Self::ArraySizeOverflow {
                object_index,
                path,
                count,
                minimum_element_size,
            } => write!(
                f,
                "object {object_index} field {path} array size overflows for {count} elements of minimum size {minimum_element_size}"
            ),
            Self::ArrayExceedsPayload {
                object_index,
                path,
                count,
                minimum_element_size,
                required,
                remaining,
            } => write!(
                f,
                "object {object_index} field {path} array count {count} requires at least {required} bytes ({minimum_element_size} per element), but only {remaining} remain"
            ),
            Self::ZeroWidthArrayElement {
                object_index,
                path,
                count,
            } => write!(
                f,
                "object {object_index} field {path} has {count} array elements with no bounded serialized width"
            ),
            Self::UnknownLeafWithoutSize {
                object_index,
                path,
                type_name,
                byte_size,
            } => write!(
                f,
                "object {object_index} field {path} has unknown leaf type {type_name} with invalid byte size {byte_size}"
            ),
            Self::DecodeLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                f,
                "object payload decode requires {requested} units for {resource}, above the configured limit {limit}"
            ),
            Self::AllocationSizeOverflow { resource } => {
                write!(f, "decoded allocation size overflows while reserving {resource}")
            }
            Self::AllocationFailed { path, count } => {
                write!(f, "could not reserve {count} values for field {path}")
            }
            Self::LayoutOverflow => write!(f, "object payload layout arithmetic overflowed"),
        }
    }
}

/// Raw `BuildTarget` value meaning "no target".
///
/// Mirrors `BuildTarget.NoTarget` (`0xFFFFFFFE`, from
/// `AssetRipper/Source/AssetRipper.IO.Files/BuildTarget.cs`). The full
/// platform-enum mapping stays open (see `SER-META-PLATFORM-001`); the flags
/// kernel only needs this one comparison, exactly like `SerializedFile.Flags`.
pub const BUILD_TARGET_NO_TARGET: u32 = 0xFFFFFFFE;

/// First engine default-resource file name in `SpecialFileNames`.
pub const DEFAULT_RESOURCE_NAME_1: &str = "unity default resources";
/// Second engine default-resource file name in `SpecialFileNames`.
pub const DEFAULT_RESOURCE_NAME_2: &str = "unity_default_resources";
/// Engine editor-resource file name in `SpecialFileNames`.
pub const EDITOR_RESOURCE_NAME: &str = "unity editor resources";
/// First builtin-extra file name in `SpecialFileNames`.
pub const BUILTIN_EXTRA_NAME_1: &str = "unity builtin extra";
/// Second builtin-extra file name in `SpecialFileNames`.
pub const BUILTIN_EXTRA_NAME_2: &str = "unity_builtin_extra";

/// `SpecialFileNames.IsEngineResource` over a caller-supplied file name.
///
/// The source compares with ordinal, case-sensitive equality against the raw
/// `FileBase.Name` (not `NameFixed`), so this does the same: callers must pass
/// the raw name unchanged. A `FixFileIdentifier` output such as
/// `"library/unity default resources"` therefore never matches here.
pub fn is_engine_resource_name(file_name: &str) -> bool {
    file_name == DEFAULT_RESOURCE_NAME_1
        || file_name == DEFAULT_RESOURCE_NAME_2
        || file_name == EDITOR_RESOURCE_NAME
}

/// `SpecialFileNames.IsBuiltinExtra` over a caller-supplied file name.
///
/// Same exact-match semantics as [`is_engine_resource_name`].
pub fn is_builtin_extra_name(file_name: &str) -> bool {
    file_name == BUILTIN_EXTRA_NAME_1 || file_name == BUILTIN_EXTRA_NAME_2
}

/// Transfer flags governing how a SerializedFile's objects are read.
///
/// Mirrors `SerializedFile.Flags`
/// (`AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/SerializedFile.cs`):
/// a `NoTarget` platform on a platform-carrying generation reads editor scenes
/// (`.unity` paths) as `SerializeEditorMinimalScene` and anything else with bare
/// flags, while every other file reads as `SerializeGameRelease`. Engine
/// resources (and, below generation 10, builtin-extra files) additionally set
/// `IsBuiltinResourcesFile`, and big-endian bodies set `SwapEndianess`.
///
/// Caller facts: `generation` is the file generation (`HasPlatform` is
/// `generation >= Unknown_8`, shared with the metadata prefix reader);
/// `platform` is the parsed target-platform value (`None` when the generation
/// carries no platform field — only `Some(BUILD_TARGET_NO_TARGET)` on a
/// platform-carrying generation takes the scene branch); `file_path` is the raw
/// `FileBase.FilePath` (the `.unity` check is an ordinal, case-sensitive suffix
/// on the full path, matching `EndsWith(".unity", Ordinal)`); `file_name` is
/// the raw `FileBase.Name` (not `NameFixed`); `body_endian` is the file body
/// endianness (the source's `EndianType` after `GetEndianType`).
///
/// The generated per-class release/editor read dispatch and the export
/// collections that carry `Flags` stay caller-side: this kernel only derives the
/// flag word. `SpecialFileNames.FixFileIdentifier` (`NameFixed`) normalization
/// is likewise out of scope here.
pub fn serialized_file_object_read_flags(
    generation: FormatVersion,
    platform: Option<u32>,
    file_path: &str,
    file_name: &str,
    body_endian: Endian,
) -> TransferInstructionFlags {
    let mut flags = if generation.has_platform() && platform == Some(BUILD_TARGET_NO_TARGET) {
        if file_path.ends_with(".unity") {
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
        } else {
            TransferInstructionFlags::NONE
        }
    } else {
        TransferInstructionFlags::SERIALIZE_GAME_RELEASE
    };
    let builtin_extra_below_generation_10 =
        generation.raw() < FormatVersion::Unknown10.raw() && is_builtin_extra_name(file_name);
    if is_engine_resource_name(file_name) || builtin_extra_below_generation_10 {
        flags = flags.with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE);
    }
    if body_endian == Endian::Big {
        flags = flags.with(TransferInstructionFlags::SWAP_ENDIANESS);
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::{
        ObjectDecodeErrorKind, ObjectEncodeErrorKind, ObjectEncodeLimits, SerializedFieldValue,
        SerializedValue, SerializedValueLookupError,
    };
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::header::SerializedFileHeader;
    use crate::serialized_file::metadata::{SerializedFileMetadata, TypeTree, TypeTreeNode};

    fn layout_node(level: u8, name: &str, type_name: &str) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size: 0,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    fn element_layout(nodes: Vec<TypeTreeNode>) -> super::TypeTreeLayout<'static> {
        // The layout borrows the tree; leak the box so the test can hold it.
        let tree: &'static TypeTree = Box::leak(Box::new(TypeTree {
            nodes,
            string_buffer: Vec::new(),
        }));
        super::TypeTreeLayout::new(tree, 0, 0).unwrap()
    }

    #[test]
    fn managed_reference_elements_require_the_full_source_predicate() {
        // `managedReference` holding one `rid` child matches.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The legacy `id` child matches too.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedRefArrayItem"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // A wrong child name does not match.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "value", "SInt64"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Two children do not match.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
            layout_node(1, "extra", "SInt32"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // No children do not match.
        let layout = element_layout(vec![layout_node(0, "item", "managedRefArrayItem")]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Other element types never match, even with a `rid` child.
        let layout = element_layout(vec![
            layout_node(0, "item", "Generic Mono"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    }

    #[test]
    fn indexed_managed_references_require_the_legacy_identifier() {
        // The legacy `id` child selects the indexed shape.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The modern `rid` child does not.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The array-item spelling participates as well.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedRefArrayItem"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Non-reference elements never select the indexed shape.
        let layout = element_layout(vec![
            layout_node(0, "item", "Generic Mono"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Two children do not match, even when the first is `id`.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "id", "SInt32"),
            layout_node(1, "extra", "SInt32"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
    }

    #[test]
    fn managed_registry_and_data_nodes_follow_the_source_predicates() {
        // `IsManagedReferencesRegistry`: the name pair plus more than one subnode.
        assert!(super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "references",
            2
        ));
        assert!(!super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "references",
            1
        ));
        assert!(!super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "references",
            0
        ));
        assert!(!super::is_managed_references_registry(
            "ManagedReferencesRegistry",
            "other",
            2
        ));
        assert!(!super::is_managed_references_registry(
            "other",
            "references",
            2
        ));
        // `IsReferencedObjectData`: the dynamic holder carries no subnodes; a
        // same-named node with children is an ordinary struct, not dynamic data.
        assert!(super::is_referenced_object_data(
            "ReferencedObjectData",
            "data",
            0
        ));
        assert!(!super::is_referenced_object_data(
            "ReferencedObjectData",
            "data",
            1
        ));
        assert!(!super::is_referenced_object_data(
            "ReferencedObjectData",
            "other",
            0
        ));
        assert!(!super::is_referenced_object_data("other", "data", 0));
    }

    #[test]
    fn structured_values_support_unique_fields_arrays_and_pointer_identity() {
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "target".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 2,
                path_id: 41,
            },
        }]);
        let target = value.unique_field("target").unwrap().unwrap();
        assert_eq!(target.pptr(), Some((2, 41)));
        assert_eq!(value.unique_field("missing").unwrap(), None);
        assert_eq!(value.array_elements(), None);
        assert_eq!(target.struct_fields(), None);

        let array = SerializedValue::Array(vec![SerializedValue::U8(7)]);
        assert_eq!(array.array_elements().unwrap(), &[SerializedValue::U8(7)]);
        assert_eq!(array.pptr(), None);
    }

    #[test]
    fn structured_field_lookup_rejects_duplicate_names_and_non_structs() {
        let duplicate = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "value".to_owned(),
                value: SerializedValue::I32(1),
            },
            SerializedFieldValue {
                name: "value".to_owned(),
                value: SerializedValue::I32(2),
            },
        ]);
        assert_eq!(
            duplicate.unique_field("value"),
            Err(SerializedValueLookupError::DuplicateField {
                name: "value".to_owned(),
                match_count: 2,
            })
        );
        assert_eq!(
            SerializedValue::Bool(true).unique_field("value"),
            Err(SerializedValueLookupError::NotStruct)
        );
    }

    #[test]
    fn inherited_field_lookup_follows_only_base_structs_and_detects_duplicates() {
        let inherited = SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"child".to_vec()),
            }]),
        }]);
        assert_eq!(
            inherited.unique_field_in_base_chain("m_Name").unwrap(),
            Some(&SerializedValue::StringBytes(b"child".to_vec()))
        );

        let duplicate = SerializedValue::Struct(vec![
            super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"derived".to_vec()),
            },
            super::SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                    name: "m_Name".to_owned(),
                    value: SerializedValue::StringBytes(b"base".to_vec()),
                }]),
            },
        ]);
        assert_eq!(
            duplicate.unique_field_in_base_chain("m_Name"),
            Err(SerializedValueLookupError::DuplicateField {
                name: "m_Name".to_owned(),
                match_count: 2,
            })
        );
    }

    #[test]
    fn inherited_component_array_mutation_preserves_source_slots_and_checks_bounds() {
        let mut root = SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(vec![
                    SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 0,
                    },
                    SerializedValue::PPtr {
                        file_id: 2,
                        path_id: 41,
                    },
                ]),
            }]),
        }]);

        let components = root
            .unique_field_in_base_chain_mut("m_Component")
            .unwrap()
            .unwrap();
        components
            .push_array_element_bounded(
                SerializedValue::PPtr {
                    file_id: 3,
                    path_id: 99,
                },
                3,
            )
            .unwrap();
        assert!(matches!(
            components.push_array_element_bounded(
                SerializedValue::PPtr {
                    file_id: 4,
                    path_id: 100,
                },
                3
            ),
            Err(
                super::SerializedValueMutationError::ArrayElementLimitExceeded {
                    requested: 4,
                    limit: 3,
                }
            )
        ));

        let components = root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap();
        assert_eq!(
            components.array_elements().unwrap(),
            &[
                SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
                SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 41,
                },
                SerializedValue::PPtr {
                    file_id: 3,
                    path_id: 99,
                },
            ]
        );
    }

    #[test]
    fn inherited_field_mutation_rejects_duplicate_matches() {
        let mut root = SerializedValue::Struct(vec![
            super::SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(Vec::new()),
            },
            super::SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                    name: "m_Component".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                }]),
            },
        ]);

        assert!(matches!(
            root.unique_field_in_base_chain_mut("m_Component"),
            Err(super::SerializedValueMutationError::DuplicateField {
                name,
                match_count: 2,
            }) if name == "m_Component"
        ));
    }

    fn node(
        level: u8,
        name: &str,
        type_name: &str,
        byte_size: i32,
        meta_flags: u32,
    ) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size,
            index: 0,
            meta_flags,
            reference_type_hash: None,
        }
    }

    fn make_file(payload: &[u8]) -> (Vec<u8>, SerializedFileHeader, SerializedFileMetadata) {
        let generation = FormatVersion::Unknown9;
        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2022.3.1f1\0");
        metadata.extend_from_slice(&0u32.to_le_bytes()); // target platform
        metadata.extend_from_slice(&[1]); // type tree enabled
        let metadata_offset = 20usize;
        let data_offset = metadata_offset + metadata.len();
        let object_bytes = payload.to_vec();
        let file_size = data_offset + object_bytes.len();
        let mut file = vec![0; 20];
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&object_bytes);
        let header = SerializedFileHeader {
            metadata_size: metadata.len() as u64,
            file_size: file_size as u64,
            format_version: generation,
            data_offset: data_offset as u64,
            data_endian: Some(Endian::Little),
            reserved_v22: None,
            header_size: 20,
        };
        let mut metadata = SerializedFileMetadata {
            prefix: super::super::metadata_prefix::SerializedFileMetadataPrefix {
                metadata_offset,
                metadata_size: metadata.len(),
                body_endian: Endian::Little,
                unity_version_text: Some("2022.3.1f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 16,
            },
            types: vec![super::super::metadata::SerializedType {
                raw_type_id: 1,
                type_id_for_hash: 1,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(TypeTree {
                    nodes: vec![
                        node(0, "Root", "Root", -1, 0),
                        node(1, "m_Number", "int", 4, 0),
                        node(1, "m_Flag", "bool", 1, 0),
                    ],
                    string_buffer: Vec::new(),
                }),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: vec![super::super::metadata::ObjectInfo {
                path_id: 42,
                byte_start: 0,
                byte_size: payload.len() as u32,
                serialized_type_index: None,
                raw_type_id: 1,
                class_id: Some(1),
                script_type_index: None,
                is_destroyed: None,
                is_stripped: None,
                resolved_type_index: Some(0),
            }],
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 16,
        };
        metadata.prefix.metadata_size = metadata.prefix.consumed_bytes;
        (file, header, metadata)
    }

    #[test]
    fn decodes_primitive_fields_and_preserves_trailing_payload_bytes() {
        let payload = [0x78, 0x56, 0x34, 0x12, 1, 0xaa];
        let (file, header, metadata) = make_file(&payload);
        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();

        assert_eq!(object.path_id, 42);
        assert_eq!(object.type_name, "Root");
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Number".to_owned(),
                    value: SerializedValue::I32(0x1234_5678),
                },
                super::SerializedFieldValue {
                    name: "m_Flag".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
        assert_eq!(object.consumed_bytes, 5);
        assert_eq!(object.trailing_bytes, [0xaa]);
    }

    #[test]
    fn encodes_typed_object_fields_and_retains_trailing_bytes_in_both_endian_modes() {
        let little_endian_payload = [0x78, 0x56, 0x34, 0x12, 1, 0xaa];
        let (file, header, metadata) = make_file(&little_endian_payload);
        let mut object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            little_endian_payload
        );

        *object
            .root
            .unique_field_in_base_chain_mut("m_Number")
            .unwrap()
            .unwrap() = SerializedValue::I32(0x0102_0304);
        let encoded = metadata
            .encode_object_payload(&object, ObjectEncodeLimits::default())
            .unwrap();
        assert_eq!(encoded, [0x04, 0x03, 0x02, 0x01, 1, 0xaa]);
        let data_range = metadata.objects[0].data_range(&header).unwrap();
        let data_start = usize::try_from(data_range.start).unwrap();
        let mut edited_file = file.clone();
        edited_file[data_start..data_start + encoded.len()].copy_from_slice(&encoded);
        let decoded = metadata
            .decode_object_payload(&edited_file, &header, 0)
            .unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Number".to_owned(),
                    value: SerializedValue::I32(0x0102_0304),
                },
                super::SerializedFieldValue {
                    name: "m_Flag".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
        assert_eq!(decoded.trailing_bytes, [0xaa]);
        let error = metadata
            .encode_object_payload(
                &object,
                ObjectEncodeLimits {
                    max_encoded_payload_bytes: 4,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::EncodedPayloadLimitExceeded { limit: 4, .. }
        ));

        let big_endian_payload = [0x12, 0x34, 0x56, 0x78, 1, 0xaa];
        let (file, header, mut metadata) = make_file(&big_endian_payload);
        metadata.prefix.body_endian = Endian::Big;
        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            big_endian_payload
        );
    }

    #[test]
    fn tree_usability_distinguishes_absent_and_empty_trees() {
        use super::TypeTreeUsability;
        assert_eq!(
            super::classify_type_tree_usability(None),
            TypeTreeUsability::Missing
        );
        let empty = TypeTree {
            nodes: Vec::new(),
            string_buffer: Vec::new(),
        };
        assert_eq!(
            super::classify_type_tree_usability(Some(&empty)),
            TypeTreeUsability::Empty
        );
        let present = TypeTree {
            nodes: vec![node(0, "Root", "Root", -1, 0)],
            string_buffer: Vec::new(),
        };
        assert_eq!(
            super::classify_type_tree_usability(Some(&present)),
            TypeTreeUsability::Present
        );
        // The decode prelude reports each fact with its own error.
        let (file, header, mut metadata) = make_file(&[0]);
        metadata.types[0].type_tree = Some(empty);
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::EmptyTypeTree { object_index: 0 }
        ));
        metadata.types[0].type_tree = None;
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::MissingTypeTree { object_index: 0 }
        ));
    }

    #[test]
    fn pre_3_5_payloads_are_refused_before_decoding() {
        assert!(!super::version_is_unsupported(3, 5));
        assert!(!super::version_is_unsupported(4, 0));
        assert!(!super::version_is_unsupported(2022, 3));
        assert!(super::version_is_unsupported(3, 4));
        assert!(super::version_is_unsupported(2, 6));
        assert!(super::version_is_unsupported(0, 0));
        // A positively identified pre-3.5 file is refused with its own error.
        let full = [0x78, 0x56, 0x34, 0x12, 1];
        let (file, header, mut metadata) = make_file(&full);
        metadata.prefix.unity_version_text = Some("3.4.2f3".to_owned());
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::UnreadableUnsupportedVersion { object_index: 0 }
        ));
        // The 3.5 boundary itself, missing text, and garbage text all proceed.
        metadata.prefix.unity_version_text = Some("3.5.0b1".to_owned());
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
        metadata.prefix.unity_version_text = None;
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
        metadata.prefix.unity_version_text = Some("not-a-version".to_owned());
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
    }

    #[test]
    fn recovery_class_names_follow_the_source_enum_rule() {
        // Defined IDs render the enum member name.
        assert_eq!(super::class_id_type_name(1), Some("GameObject"));
        assert_eq!(super::class_id_type_name(28), Some("Texture2D"));
        assert_eq!(super::class_id_type_name(48), Some("Shader"));
        assert_eq!(super::class_id_type_name(114), Some("MonoBehaviour"));
        assert_eq!(super::class_id_type_name(115), Some("MonoScript"));
        assert_eq!(super::class_id_type_name(194), Some("NavMeshData_194"));
        assert_eq!(
            super::class_id_type_name(2126867596),
            Some("TilemapEditorUserSettings")
        );
        // `PlayerSettings` (129) is not a member; gaps and unknown IDs read `None`.
        assert_eq!(super::class_id_type_name(129), None);
        assert_eq!(super::class_id_type_name(3), None);
        assert_eq!(super::class_id_type_name(99999), None);
        // `RawDataObject.ClassName` renders undefined values numerically.
        assert_eq!(super::raw_data_class_name(48).as_ref(), "Shader");
        assert_eq!(super::raw_data_class_name(3).as_ref(), "3");
        assert_eq!(super::raw_data_class_name(99999).as_ref(), "99999");
        assert_eq!(super::raw_data_class_name(-5).as_ref(), "-5");
        // The fact composes with the factory naming shape: `Unknown{ClassName}_{Hash:X}`.
        assert_eq!(
            format!("Unknown{}_1234ABCD", super::raw_data_class_name(28)),
            "UnknownTexture2D_1234ABCD"
        );
    }

    #[test]
    fn rejects_negative_array_counts_and_missing_type_trees() {
        let (file, header, mut metadata) = make_file(&[0xff, 0xff, 0xff, 0xff]);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Values", "Array", -1, 0),
                node(2, "size", "int", 4, 0),
                node(2, "data", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::NegativeArrayLength { length: -1, .. }
        ));

        metadata.types[0].type_tree = None;
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::MissingTypeTree { object_index: 0 }
        ));
    }

    #[test]
    fn decodes_modern_and_legacy_managed_reference_ids() {
        let modern_id = 7i64;
        let (file, header, mut metadata) = make_file(&modern_id.to_le_bytes());
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "managedReference", 8, 0),
                node(2, "rid", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Target".to_owned(),
                value: SerializedValue::ManagedReferenceId {
                    reference_id: 7,
                    indexed: false,
                },
            }])
        );

        let legacy_id = 3i32;
        let (file, header, mut metadata) = make_file(&legacy_id.to_le_bytes());
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "managedReference", 4, 0),
                node(2, "id", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Target".to_owned(),
                value: SerializedValue::ManagedReferenceId {
                    reference_id: 3,
                    indexed: true,
                },
            }])
        );
    }

    #[test]
    fn decodes_and_encodes_named_vectors_of_managed_reference_array_items() {
        let mut modern_payload = Vec::new();
        modern_payload.extend_from_slice(&2i32.to_le_bytes());
        modern_payload.extend_from_slice(&7i64.to_le_bytes());
        modern_payload.extend_from_slice(&9i64.to_le_bytes());
        let (file, header, mut metadata) = make_file(&modern_payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Items", "managedReference", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "managedRefArrayItem", -1, 0),
                node(4, "rid", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root.unique_field("m_Items").unwrap(),
            Some(&SerializedValue::Array(vec![
                SerializedValue::ManagedReferenceId {
                    reference_id: 7,
                    indexed: false,
                },
                SerializedValue::ManagedReferenceId {
                    reference_id: 9,
                    indexed: false,
                },
            ]))
        );
        assert_eq!(
            metadata
                .encode_object_payload(&decoded, ObjectEncodeLimits::default())
                .unwrap(),
            modern_payload
        );

        let mut legacy_payload = Vec::new();
        legacy_payload.extend_from_slice(&2i32.to_le_bytes());
        legacy_payload.extend_from_slice(&0i32.to_le_bytes());
        legacy_payload.extend_from_slice(&1i32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&legacy_payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Items", "managedReference", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "managedRefArrayItem", -1, 0),
                node(4, "id", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let decoded = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            decoded.root.unique_field("m_Items").unwrap(),
            Some(&SerializedValue::Array(vec![
                SerializedValue::ManagedReferenceId {
                    reference_id: 0,
                    indexed: true,
                },
                SerializedValue::ManagedReferenceId {
                    reference_id: 1,
                    indexed: true,
                },
            ]))
        );
        assert_eq!(
            metadata
                .encode_object_payload(&decoded, ObjectEncodeLimits::default())
                .unwrap(),
            legacy_payload
        );
    }

    #[test]
    fn decodes_runtime_types_shared_ids_and_cyclic_managed_payloads() {
        let fixture =
            crate::serialized_file::managed_reference_test_support::managed_reference_fixture();
        let decoded = fixture
            .metadata
            .decode_object_payload(&fixture.file, &fixture.header, 0)
            .unwrap();
        assert!(decoded.trailing_bytes.is_empty());
        assert_eq!(decoded.path_id, 500);
        let SerializedValue::Struct(fields) = decoded.root else {
            panic!("expected object struct");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::ManagedReferenceId {
                reference_id: 7,
                indexed: false,
            }
        );
        assert_eq!(
            fields[1].value,
            SerializedValue::ManagedReferenceId {
                reference_id: 7,
                indexed: false,
            }
        );
        assert_eq!(
            fields[2].value,
            SerializedValue::ManagedReferenceId {
                reference_id: 9,
                indexed: false,
            }
        );
        let SerializedValue::ManagedReferenceRegistry(registry) = &fields[3].value else {
            panic!("expected decoded registry");
        };
        assert_eq!(registry.entries.len(), 2);
        assert_eq!(registry.entries[0].reference_id, 7);
        assert_eq!(registry.entries[0].class_name, "NodeA");
        assert_eq!(registry.entries[1].reference_id, 9);
        assert_eq!(registry.entries[1].class_name, "NodeB");
        let crate::serialized_file::managed_registry::ManagedRegistryEntryData::Content(node_a) =
            &registry.entries[0].data
        else {
            panic!("expected NodeA content");
        };
        let crate::serialized_file::managed_registry::ManagedRegistryEntryData::Content(node_b) =
            &registry.entries[1].data
        else {
            panic!("expected NodeB content");
        };
        assert_eq!(
            node_a.unique_field("m_Value").unwrap(),
            Some(&SerializedValue::I32(100))
        );
        assert_eq!(
            node_b.unique_field("m_Value").unwrap(),
            Some(&SerializedValue::I32(200))
        );
        assert_eq!(
            node_a.unique_field("m_Link").unwrap(),
            Some(&SerializedValue::ManagedReferenceId {
                reference_id: 9,
                indexed: false,
            })
        );
        assert_eq!(
            node_b.unique_field("m_Link").unwrap(),
            Some(&SerializedValue::ManagedReferenceId {
                reference_id: 7,
                indexed: false,
            })
        );
        assert_eq!(
            node_a.unique_field("m_Asset").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 1,
                path_id: 42,
            })
        );
        assert_eq!(fields[4].value, SerializedValue::I32(0x1234_5678));

        let decoded = fixture
            .metadata
            .decode_object_payload(&fixture.file, &fixture.header, 0)
            .unwrap();
        let encoded = fixture
            .metadata
            .encode_object_payload(&decoded, ObjectEncodeLimits::default())
            .unwrap();
        let start = usize::try_from(fixture.header.data_offset).unwrap();
        assert_eq!(
            encoded.len(),
            fixture.metadata.objects[0].byte_size as usize
        );
        assert_eq!(encoded, fixture.file[start..start + encoded.len()]);
    }

    #[test]
    fn decodes_terminated_legacy_registry_and_reencodes_exactly() {
        let fixture = crate::serialized_file::managed_reference_test_support::
            terminated_managed_reference_fixture();
        let decoded = fixture
            .metadata
            .decode_object_payload(&fixture.file, &fixture.header, 0)
            .unwrap();
        let SerializedValue::Struct(fields) = &decoded.root else {
            panic!("expected object struct");
        };
        for (field, expected_id) in [("m_First", 0), ("m_Shared", 0), ("m_Second", 1)] {
            assert_eq!(
                fields
                    .iter()
                    .find(|value| value.name == field)
                    .map(|value| &value.value),
                Some(&SerializedValue::ManagedReferenceId {
                    reference_id: expected_id,
                    indexed: true,
                })
            );
        }
        let SerializedValue::ManagedReferenceRegistry(registry) = &fields[3].value else {
            panic!("expected terminated registry");
        };
        assert_eq!(registry.version, 1);
        assert_eq!(registry.entries.len(), 2);
        assert_eq!(registry.entries[0].reference_id, 0);
        assert_eq!(registry.entries[1].reference_id, 1);
        let encoded = fixture
            .metadata
            .encode_object_payload(&decoded, ObjectEncodeLimits::default())
            .unwrap();
        let start = usize::try_from(fixture.header.data_offset).unwrap();
        assert_eq!(
            encoded.len(),
            fixture.metadata.objects[0].byte_size as usize
        );
        assert_eq!(encoded, fixture.file[start..start + encoded.len()]);
    }

    #[test]
    fn registry_failures_keep_the_object_and_type_tree_path() {
        let fixture =
            crate::serialized_file::managed_reference_test_support::managed_reference_fixture();
        let mut malformed = fixture.file.clone();
        let registry_start = usize::try_from(fixture.header.data_offset).unwrap() + 24;
        malformed[registry_start..registry_start + 4].copy_from_slice(&9i32.to_le_bytes());
        let error = fixture
            .metadata
            .decode_object_payload(&malformed, &fixture.header, 0)
            .unwrap_err();
        assert!(
            matches!(
                error.kind(),
                ObjectDecodeErrorKind::ManagedRegistryDecode {
                    object_index: 0,
                    path,
                    message,
                } if path == "Root.references" && message.contains("format 9")
            ),
            "unexpected contextual decode error: {error}"
        );
    }

    #[test]
    fn decodes_byte_arrays_with_versioned_alignment_before_the_next_field() {
        let payload = [1, 0, 0, 0, 0x7f, 0, 0, 0, 1];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Bytes", "vector", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "UInt8", 1, 0),
                node(1, "m_Active", "bool", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].name, "m_Bytes");
        assert_eq!(
            fields[0].value,
            SerializedValue::Array(vec![SerializedValue::U8(0x7f)])
        );
        assert_eq!(fields[1].name, "m_Active");
        assert_eq!(fields[1].value, SerializedValue::Bool(true));
        assert_eq!(object.consumed_bytes, payload.len());
        assert!(object.trailing_bytes.is_empty());

        let mut shortened = metadata.decode_object_payload(&file, &header, 0).unwrap();
        *shortened
            .root
            .unique_field_in_base_chain_mut("m_Bytes")
            .unwrap()
            .unwrap() = SerializedValue::Array(Vec::new());
        let encoded = metadata
            .encode_object_payload(
                &shortened,
                ObjectEncodeLimits {
                    max_encoded_payload_bytes: 5,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(encoded, [0, 0, 0, 0, 1]);
        let data_start =
            usize::try_from(metadata.objects[0].data_range(&header).unwrap().start).unwrap();
        let mut edited_file = file.clone();
        edited_file[data_start..data_start + encoded.len()].copy_from_slice(&encoded);
        let mut edited_metadata = metadata.clone();
        edited_metadata.objects[0].byte_size = encoded.len() as u32;
        let decoded = edited_metadata
            .decode_object_payload(&edited_file, &header, 0)
            .unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Bytes".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
                super::SerializedFieldValue {
                    name: "m_Active".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
    }

    #[test]
    fn decodes_aligned_utf8_byte_strings_before_later_fields() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&3i32.to_le_bytes());
        payload.extend_from_slice(b"abc");
        payload.push(0); // string alignment to a four-byte boundary
        payload.extend_from_slice(&0xaabb_ccddu32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Name", "string", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "char", 1, 0),
                node(1, "m_Flags", "uint", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::StringBytes(b"abc".to_vec())
        );
        assert_eq!(fields[1].value, SerializedValue::U32(0xaabb_ccdd));
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_and_reencodes_utf8string_type_tree_nodes() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&5i32.to_le_bytes());
        payload.extend_from_slice(b"track");
        payload.extend_from_slice(&[0; 3]); // string alignment to a four-byte boundary
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Name", "Utf8String", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "char", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"track".to_vec()),
            }])
        );
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
    }

    #[test]
    fn honors_field_align_bytes_and_keeps_unknown_sized_leaves() {
        let payload = [7, 1, 0, 0, 0, 0, 0, 0, 0xaa, 0xbb];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_First", "UInt8", 1, 0),
                node(1, "m_Number", "int", 4, 0x4000),
                node(1, "m_Unknown", "VendorThing", 2, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::U8(7));
        assert_eq!(fields[1].value, SerializedValue::I32(1));
        assert_eq!(
            fields[2].value,
            SerializedValue::Raw {
                type_name: "VendorThing".to_owned(),
                bytes: vec![0xaa, 0xbb],
            }
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_typed_pptr_nodes_without_losing_file_or_path_identity() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&2i32.to_le_bytes());
        payload.extend_from_slice(&0x1234_5678_9abc_def0i64.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "PPtr<$GameObject>", 12, 0),
                node(2, "m_FileID", "SInt32", 4, 0),
                node(2, "m_PathID", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Target".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 0x1234_5678_9abc_def0,
                },
            }])
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn sign_extends_legacy_32_bit_pptr_path_ids() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(-1i32).to_le_bytes());
        payload.extend_from_slice(&(-42i32).to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "PPtr<Object>", "PPtr<Object>", 8, 0),
                node(1, "m_FileID", "SInt32", 4, 0),
                node(1, "m_PathID", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::PPtr {
                file_id: -1,
                path_id: -42,
            }
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_nested_vector_struct_fields() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1.0f32.to_le_bytes());
        payload.extend_from_slice(&(-2.5f32).to_le_bytes());
        payload.extend_from_slice(&3.25f32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Position", "Vector3f", -1, 0),
                node(2, "x", "float", 4, 0),
                node(2, "y", "float", 4, 0),
                node(2, "z", "float", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(1.0),
                },
                super::SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(-2.5),
                },
                super::SerializedFieldValue {
                    name: "z".to_owned(),
                    value: SerializedValue::F32(3.25),
                },
            ])
        );
    }

    #[test]
    fn honors_the_type_tree_integer_as_character_flag() {
        let payload = 0x0041u16.to_le_bytes();
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Character", "UInt16", 2, 0x0200_0000),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::Char(0x0041));
    }

    #[test]
    fn decodes_object_payload_scalars_using_metadata_endianness() {
        let payload = [0x12, 0x34, 0x56, 0x78];
        let (file, mut header, mut metadata) = make_file(&payload);
        header.data_endian = Some(Endian::Big);
        metadata.prefix.body_endian = Endian::Big;
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Value", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::I32(0x1234_5678));
    }

    #[test]
    fn caller_decode_limits_reject_excessive_array_storage() {
        let payload = [2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Values", "vector", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let limits = super::ObjectDecodeLimits {
            max_array_elements: 1,
            ..super::ObjectDecodeLimits::default()
        };

        let error = metadata
            .decode_object_payload_with_limits(&file, &header, 0, &limits)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::DecodeLimitExceeded {
                requested: 2,
                limit: 1,
                ..
            }
        ));
    }

    #[test]
    fn aligns_each_inner_complex_array_when_the_field_requests_alignment() {
        let payload = [
            2, 0, 0, 0, // outer array count
            1, 0, 0, 0, // first inner count
            0x11, 0, 0, 0, // first byte plus inner-array padding
            1, 0, 0, 0, // second inner count
            0x22, 0, 0, 0, // second byte plus inner-array padding
        ];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Nested", "vector", -1, 0x4000),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "vector", -1, 0),
                node(4, "Array", "Array", -1, 0),
                node(5, "size", "int", 4, 0),
                node(5, "data", "Custom", -1, 0),
                node(6, "m_Value", "UInt8", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::Array(vec![
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    super::SerializedFieldValue {
                        name: "m_Value".to_owned(),
                        value: SerializedValue::U8(0x11),
                    },
                ])]),
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    super::SerializedFieldValue {
                        name: "m_Value".to_owned(),
                        value: SerializedValue::U8(0x22),
                    },
                ])]),
            ])
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn object_read_flags_select_release_for_ordinary_game_files() {
        use super::TransferInstructionFlags;
        use crate::serialized_file::format_version::FormatVersion;

        // An ordinary platformed file reads as a game release.
        let flags = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "/game/level0",
            "level0",
            crate::io::binary::Endian::Little,
        );
        assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
        assert!(flags.is_release());
        // A generation without a platform field never takes the scene branch,
        // even with a NoTarget stand-in and a `.unity` path.
        let flags = super::serialized_file_object_read_flags(
            FormatVersion::Unknown7,
            None,
            "/game/Level.unity",
            "Level",
            crate::io::binary::Endian::Little,
        );
        assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    }

    #[test]
    fn object_read_flags_select_editor_scene_for_unity_paths_without_target() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        let scene = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity",
            "Main",
            Endian::Little,
        );
        assert_eq!(
            scene,
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
        );
        assert!(scene.is_editor_scene());
        assert!(!scene.is_release());
        // A NoTarget platform without a `.unity` path carries bare flags.
        let bare = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "/game/sharedassets0.assets",
            "sharedassets0.assets",
            Endian::Little,
        );
        assert_eq!(bare, TransferInstructionFlags::NONE);
        // The suffix check is ordinal and case-sensitive, like the source.
        let upper = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.UNITY",
            "Main",
            Endian::Little,
        );
        assert_eq!(upper, TransferInstructionFlags::NONE);
        let backup = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity.backup",
            "Main",
            Endian::Little,
        );
        assert_eq!(backup, TransferInstructionFlags::NONE);
        // A real platform target always reads as a game release.
        let release = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "Assets/Scenes/Main.unity",
            "Main",
            Endian::Little,
        );
        assert_eq!(release, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    }

    #[test]
    fn object_read_flags_mark_builtin_resource_files() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        // Engine resources set the builtin bit on any generation.
        for name in [
            super::DEFAULT_RESOURCE_NAME_1,
            super::DEFAULT_RESOURCE_NAME_2,
            super::EDITOR_RESOURCE_NAME,
        ] {
            let flags = super::serialized_file_object_read_flags(
                FormatVersion::LargeFilesSupport,
                Some(7),
                "/game/resources.assets",
                name,
                Endian::Little,
            );
            assert_eq!(
                flags,
                TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                    .with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE)
            );
            assert!(flags.is_builtin_resources());
        }
        // Builtin-extra names only count below generation 10.
        for name in [super::BUILTIN_EXTRA_NAME_1, super::BUILTIN_EXTRA_NAME_2] {
            let old = super::serialized_file_object_read_flags(
                FormatVersion::Unknown9,
                Some(super::BUILD_TARGET_NO_TARGET),
                "/game/extra.assets",
                name,
                Endian::Little,
            );
            assert!(old.is_builtin_resources());
            let cutoff = super::serialized_file_object_read_flags(
                FormatVersion::Unknown10,
                Some(7),
                "/game/extra.assets",
                name,
                Endian::Little,
            );
            assert!(!cutoff.is_builtin_resources());
            assert_eq!(cutoff, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
        }
        // Ordinary names never set the builtin bit.
        let plain = super::serialized_file_object_read_flags(
            FormatVersion::Unknown9,
            None,
            "/game/sharedassets0.assets",
            "sharedassets0.assets",
            Endian::Little,
        );
        assert!(!plain.is_builtin_resources());
    }

    #[test]
    fn object_read_flags_swap_endianness_for_big_endian_bodies() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        let big = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "/game/data.assets",
            "data.assets",
            Endian::Big,
        );
        assert_eq!(
            big,
            TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                .with(TransferInstructionFlags::SWAP_ENDIANESS)
        );
        // The endian bit composes with the scene and builtin bits.
        let scene_big = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity",
            super::DEFAULT_RESOURCE_NAME_2,
            Endian::Big,
        );
        assert!(scene_big.is_editor_scene());
        assert!(scene_big.is_builtin_resources());
        assert_eq!(
            scene_big.0,
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE.0
                | TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE.0
                | TransferInstructionFlags::SWAP_ENDIANESS.0
        );
    }

    #[test]
    fn special_file_name_predicates_match_exact_names_only() {
        assert!(super::is_engine_resource_name("unity default resources"));
        assert!(super::is_engine_resource_name("unity_default_resources"));
        assert!(super::is_engine_resource_name("unity editor resources"));
        assert!(!super::is_engine_resource_name("unity builtin extra"));
        // Ordinal comparison: case and normalization matter.
        assert!(!super::is_engine_resource_name("Unity Default Resources"));
        assert!(!super::is_engine_resource_name(
            "library/unity default resources"
        ));
        assert!(!super::is_engine_resource_name(""));
        assert!(super::is_builtin_extra_name("unity builtin extra"));
        assert!(super::is_builtin_extra_name("unity_builtin_extra"));
        assert!(!super::is_builtin_extra_name("Unity Builtin Extra"));
        assert!(!super::is_builtin_extra_name("unity editor resources"));
    }

    fn object_field_fixture(case: &str) -> (Vec<u8>, String) {
        fn hex_to_bytes(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length for {hex}");
            let mut out = Vec::with_capacity(hex.len() / 2);
            let bytes = hex.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let hi = match bytes[i] {
                    b'0'..=b'9' => bytes[i] - b'0',
                    b'a'..=b'f' => bytes[i] - b'a' + 10,
                    b'A'..=b'F' => bytes[i] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                let lo = match bytes[i + 1] {
                    b'0'..=b'9' => bytes[i + 1] - b'0',
                    b'a'..=b'f' => bytes[i + 1] - b'a' + 10,
                    b'A'..=b'F' => bytes[i + 1] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                out.push((hi << 4) | lo);
                i += 2;
            }
            out
        }

        // Probe Temp/opencode/objfieldprobe (SerializableTreeType.FromRootNode
        // plus SerializableStructure.Read over C#-constructed gen12 files with
        // ITypeResolver.Null, SDK 10.0.401). Nothing Rust-generated.
        let fixture = include_str!("object_field_fixture.txt");
        let mut file = None;
        let mut dump = None;
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("OBJ_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad OBJ_HEX {line}");
                if parts[1] == case {
                    file = Some(hex_to_bytes(parts[2]));
                }
            } else if line.starts_with("OBJ_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad OBJ_READ {line}");
                if parts[1] == case {
                    dump = Some(parts[2].to_string());
                }
            }
        }
        (
            file.unwrap_or_else(|| panic!("missing {case} hex")),
            dump.unwrap_or_else(|| panic!("missing {case} dump")),
        )
    }

    fn decode_fixture_object(case: &str, expected_dump: &str) -> super::ParsedObjectPayload {
        let (bytes, dump) = object_field_fixture(case);
        assert!(dump.contains(expected_dump), "{case} oracle {dump}");
        let header =
            SerializedFileHeader::parse(&bytes).unwrap_or_else(|e| panic!("{case} header: {e:?}"));
        let metadata = SerializedFileMetadata::parse_file(&bytes, &header)
            .unwrap_or_else(|e| panic!("{case} metadata: {e:?}"));
        metadata
            .decode_object_payload(&bytes, &header, 0)
            .unwrap_or_else(|e| panic!("{case} decode: {e:?}"))
    }

    fn require_field<'a>(root: &'a SerializedValue, name: &str) -> &'a SerializedValue {
        root.unique_field(name)
            .unwrap_or_else(|e| panic!("{name} lookup: {e:?}"))
            .unwrap_or_else(|| panic!("missing field {name}"))
    }

    #[test]
    fn object_scalar_string_nested_fields_match_reference() {
        let payload = decode_fixture_object(
            "F1",
            "INT=305419896|FLT=1.5|STR=hi|NEST=-5,2.5|INTS=10,20,30|BYTES=1,2,3,4,5",
        );
        assert_eq!(payload.path_id, 7001);
        let root = &payload.root;
        assert_eq!(
            require_field(root, "m_Int"),
            &SerializedValue::I32(0x12345678)
        );
        assert_eq!(
            require_field(root, "m_Float"),
            &SerializedValue::F32(1.5f32)
        );
        assert_eq!(
            require_field(root, "m_String"),
            &SerializedValue::StringBytes(b"hi".to_vec())
        );
        let nested = require_field(root, "m_Nested");
        let nested_fields = nested.struct_fields().expect("m_Nested is a struct");
        assert_eq!(nested_fields.len(), 2);
        assert_eq!(nested_fields[0].name, "a");
        assert_eq!(nested_fields[0].value, SerializedValue::I32(-5));
        assert_eq!(nested_fields[1].name, "b");
        assert_eq!(nested_fields[1].value, SerializedValue::F32(2.5f32));
        assert_eq!(
            require_field(root, "m_Ints"),
            &SerializedValue::Array(vec![
                SerializedValue::I32(10),
                SerializedValue::I32(20),
                SerializedValue::I32(30),
            ])
        );
        assert_eq!(
            require_field(root, "m_Bytes"),
            &SerializedValue::Array(vec![
                SerializedValue::U8(1),
                SerializedValue::U8(2),
                SerializedValue::U8(3),
                SerializedValue::U8(4),
                SerializedValue::U8(5),
            ])
        );
        // The byte vector carries AlignBytes (0x4000) with pad on both
        // sides, so no bytes remain after the last field either.
        assert!(payload.trailing_bytes.is_empty());
    }

    #[test]
    fn object_string_nested_empty_arrays_match_reference() {
        let payload = decode_fixture_object("F2", "STRS=a,bb,ccc|MAT=1,2;3|EMPTY=0");
        assert_eq!(payload.path_id, 7001);
        let root = &payload.root;
        assert_eq!(
            require_field(root, "m_Strings"),
            &SerializedValue::Array(vec![
                SerializedValue::StringBytes(b"a".to_vec()),
                SerializedValue::StringBytes(b"bb".to_vec()),
                SerializedValue::StringBytes(b"ccc".to_vec()),
            ])
        );
        assert_eq!(
            require_field(root, "m_Matrix"),
            &SerializedValue::Array(vec![
                SerializedValue::Array(vec![SerializedValue::I32(1), SerializedValue::I32(2),]),
                SerializedValue::Array(vec![SerializedValue::I32(3)]),
            ])
        );
        assert_eq!(
            require_field(root, "m_Empty"),
            &SerializedValue::Array(vec![])
        );
        assert!(payload.trailing_bytes.is_empty());
    }

    #[test]
    fn object_encode_reproduces_reference_authored_payload() {
        // The expected bytes below are the C#-constructed F1 payload from
        // object_field_fixture.txt (reference-readable, never Rust-encoded).
        // Re-encoding the decoded values must reproduce them exactly.
        let (bytes, _) = object_field_fixture("F1");
        let header = SerializedFileHeader::parse(&bytes).expect("F1 header");
        let metadata = SerializedFileMetadata::parse_file(&bytes, &header).expect("F1 metadata");
        let object = &metadata.objects[0];
        let range = object.data_range(&header).expect("F1 range");
        let original: Vec<u8> = bytes[range.start as usize..range.end as usize].to_vec();
        assert_eq!(original.len(), 52);

        let payload = metadata
            .decode_object_payload(&bytes, &header, 0)
            .expect("F1 decode");
        let encoded = metadata
            .encode_object_payload(&payload, ObjectEncodeLimits::default())
            .expect("F1 encode");
        assert_eq!(encoded, original);

        // Mutating scalar and variable-length fields re-encodes with the
        // reference layout (little-endian int, length-prefixed aligned
        // string) while the untouched tail stays byte-identical.
        let mut mutated = payload;
        *mutated
            .root
            .unique_field_in_base_chain_mut("m_Int")
            .expect("m_Int lookup")
            .expect("m_Int present") = SerializedValue::I32(0x0102_0304);
        *mutated
            .root
            .unique_field_in_base_chain_mut("m_String")
            .expect("m_String lookup")
            .expect("m_String present") = SerializedValue::StringBytes(b"hello!".to_vec());
        let recoded = metadata
            .encode_object_payload(&mutated, ObjectEncodeLimits::default())
            .expect("F1 recode");
        // "hi" section (8 bytes) becomes "hello!" (12 bytes): +4 overall.
        // Layout is int, float, string, nested, vectors: the float sits
        // between the mutated int and the grown string.
        assert_eq!(recoded.len(), 56);
        assert_eq!(recoded[0..4], [0x04, 0x03, 0x02, 0x01]);
        assert_eq!(recoded[4..8], [0x00, 0x00, 0xC0, 0x3F]);
        assert_eq!(recoded[8..12], [0x06, 0x00, 0x00, 0x00]);
        assert_eq!(recoded[12..18], *b"hello!");
        assert_eq!(recoded[18..20], [0x00, 0x00]);
        // Nested struct onwards shifts by 4 but is otherwise unchanged.
        assert_eq!(recoded[20..], original[16..]);
    }

    #[test]
    fn large_payload_measurements_justify_decode_defaults() {
        use crate::io::binary::BinaryReader;
        use std::time::Instant;

        // Representative large-file shapes must decode inside the product
        // defaults (256 MB allocation, 16 M array elements, 64 MB strings,
        // depth 256). Timings are reported for the record; success (not
        // speed) is asserted.
        let limits = super::ObjectDecodeLimits::default();

        // 2M-element int vector (8 MB payload).
        {
            let layout = element_layout(vec![
                layout_node(0, "m_Large", "vector"),
                layout_node(1, "Array", "Array"),
                layout_node(2, "size", "int"),
                layout_node(2, "data", "int"),
            ]);
            let count = 2_000_000usize;
            let mut payload = Vec::with_capacity(4 + count * 4);
            payload.extend_from_slice(&(count as i32).to_le_bytes());
            payload.extend(std::iter::repeat_n(0u8, count * 4));
            let mut budget = super::DecodeBudget {
                limits: &limits,
                allocated_bytes: 0,
                managed_type_references: &[],
                managed_reference_type_resolver: None,
                stream_offset: 0,
                managed_registry_depth: 0,
            };
            let mut reader = BinaryReader::new(&payload, Endian::Little);
            let start = Instant::now();
            let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
                .expect("2M int vector decodes");
            let elapsed = start.elapsed();
            match field.value {
                SerializedValue::Array(elements) => assert_eq!(elements.len(), count),
                value => panic!("expected array, got {value:?}"),
            }
            println!(
                "LIMITS_MEASURE int-vector-2M elapsed_ms={}",
                elapsed.as_millis()
            );
        }

        // 8 MB string (length-prefixed, 4-aligned).
        {
            let layout = element_layout(vec![
                layout_node(0, "m_Big", "string"),
                layout_node(1, "Array", "Array"),
                layout_node(2, "size", "int"),
                layout_node(2, "data", "char"),
            ]);
            let length = 8_000_000usize;
            let mut payload = Vec::with_capacity(4 + length + 4);
            payload.extend_from_slice(&(length as i32).to_le_bytes());
            payload.extend(std::iter::repeat_n(b'x', length));
            while payload.len() % 4 != 0 {
                payload.push(0);
            }
            let mut budget = super::DecodeBudget {
                limits: &limits,
                allocated_bytes: 0,
                managed_type_references: &[],
                managed_reference_type_resolver: None,
                stream_offset: 0,
                managed_registry_depth: 0,
            };
            let mut reader = BinaryReader::new(&payload, Endian::Little);
            let start = Instant::now();
            let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
                .expect("8MB string decodes");
            let elapsed = start.elapsed();
            match field.value {
                SerializedValue::StringBytes(bytes) => assert_eq!(bytes.len(), length),
                value => panic!("expected string, got {value:?}"),
            }
            println!(
                "LIMITS_MEASURE string-8M elapsed_ms={}",
                elapsed.as_millis()
            );
        }

        // 65-level struct chain (root plus 64 nested, well under the 256 depth cap).
        {
            let mut nodes = vec![layout_node(0, "m_Root", "S0")];
            for depth in 1..=64 {
                nodes.push(layout_node(
                    depth as u8,
                    Box::leak(format!("m_D{depth}").into_boxed_str()),
                    Box::leak(format!("S{depth}").into_boxed_str()),
                ));
            }
            nodes.push(layout_node(65, "m_Value", "int"));
            let layout = element_layout(nodes);
            let payload = 12345i32.to_le_bytes().to_vec();
            let mut budget = super::DecodeBudget {
                limits: &limits,
                allocated_bytes: 0,
                managed_type_references: &[],
                managed_reference_type_resolver: None,
                stream_offset: 0,
                managed_registry_depth: 0,
            };
            let mut reader = BinaryReader::new(&payload, Endian::Little);
            let start = Instant::now();
            let field = super::decode_field(&mut reader, &layout, 0, false, 0, "", 0, &mut budget)
                .expect("64-deep nesting decodes");
            let elapsed = start.elapsed();
            let mut scope = &field.value;
            for _ in 0..65 {
                let fields = scope.struct_fields().expect("nested struct");
                assert_eq!(fields.len(), 1);
                scope = &fields[0].value;
            }
            assert_eq!(scope, &SerializedValue::I32(12345));
            println!(
                "LIMITS_MEASURE nesting-64 elapsed_ms={}",
                elapsed.as_millis()
            );
        }
    }
}
