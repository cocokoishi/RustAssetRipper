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
