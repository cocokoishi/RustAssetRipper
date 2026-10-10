//! Workspace-backed inputs for the AnimationClip editor-format processor.
//!
//! The source `EditorFormatProcessor` builds one path checksum cache from the loaded game
//! bundle, then processes every AnimationClip. This module adapts the same loaded workspace
//! and its already-resolved scene graph for that processing pass.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::export::scene_wiring::{
    MaterializedComponentTarget, MaterializedSceneGraph, MaterializedTransform,
};
use crate::io::binary::Endian;
use crate::processing::managed_assembly::{mono_script_export_identity, ManagedAssemblyCatalog};
use crate::processing::managed_assembly_schema::{
    build_mono_behaviour_schema, ManagedSchemaLimits,
};
use crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity;
use crate::processing::script_identity::ScriptIdentifier;
use crate::resources::animation::clip_object::{AnimationClipObject, PPtrValue};
use crate::resources::animation::converter::{process_clip, ConverterContext, RENDERER_CLASS_IDS};
use crate::resources::animation::scene_facts::ScriptFacts;
use crate::resources::animation::scene_facts::{
    build_path_checksum_cache, GameObjectFacts, PathCacheAsset, SceneFactsTable,
};
use crate::resources::animation::serialized_clip::{
    read_animation_clip_for_conversion, write_animation_clip_conversion,
};
use crate::resources::blend_shape::BlendShapeLookupView;
use crate::resources::curve_wrappers::ScriptReference;
use crate::resources::custom_curve::BindingCustomType;
use crate::resources::material::{MaterialView, CRC28_MASK_FIELD};
use crate::resources::path_checksum::{PathChecksumCache, PathChecksumLimits, ProcessedScriptSet};
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
use crate::serialized_file::object::SerializedValue;
use crate::serialized_file::object_workspace::ObjectWorkspace;
use crate::serialized_file::reference_graph::{
    MultiFileObjectReference, MultiFilePPtrResolution, MultiFileReferenceGraph,
};
use crate::serialized_file::workspace::SerializedFileWorkspace;

type AnimationClipIdentity = (usize, usize);
type AnimationRootsByClip = HashMap<AnimationClipIdentity, Vec<i64>>;
type AnimationWorkspaceAssets = (Vec<PathCacheAsset>, AnimationRootsByClip);

/// Run the source editor-format AnimationClip pass for every loaded class-74 object and return
/// only payload roots which have a clip-binding constant and therefore participate in conversion.
pub(super) fn process_workspace_animation_clips(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    references: &MultiFileReferenceGraph<'_>,
    graph: &MaterializedSceneGraph,
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
    max_array_elements: usize,
) -> Result<HashMap<(usize, usize), SerializedValue>, WorkspaceAnimationError> {
    let mut processing = WorkspaceAnimationProcessing::new(
        workspace,
        objects,
        references,
        graph,
        managed_assemblies,
    )?;
    let mut converted_roots = HashMap::new();

    for (file_index, file) in workspace.files().iter().enumerate() {
        for (object_index, metadata_object) in file.metadata.objects.iter().enumerate() {
            if references.object_class_id(file_index, object_index) != Some(74) {
                continue;
            }
            let object = objects.get(file_index, object_index).ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip file {file_index} object {object_index} is missing from the object workspace"
                ))
            })?;
            let payload = object.payload.as_ref().map_err(|error| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip file {file_index} object {object_index} PathID {} could not be decoded: {error}",
                    metadata_object.path_id
                ))
            })?;
            let version_text = file
                .metadata
                .prefix
                .unity_version_text
                .as_deref()
                .ok_or_else(|| {
                    WorkspaceAnimationError::new(format!(
                        "AnimationClip file {file_index} has no Unity version"
                    ))
                })?;
            let version = UnityVersion::parse(version_text).ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip file {file_index} has invalid Unity version {version_text:?}"
                ))
            })?;
            let endian = match file.metadata.prefix.body_endian {
                Endian::Little => EndianType::Little,
                Endian::Big => EndianType::Big,
            };
            let processed = processing.process_clip(
                file_index,
                object_index,
                &payload.root,
                version,
                endian,
                references,
            )?;
            let converted = processed.clip;
            if !converted.has_clip_binding_constant {
                continue;
            }

            let type_index = object.resolved_type_index.ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip file {file_index} object {object_index} has no resolved TypeTree index"
                ))
            })?;
            let serialized_type = file.metadata.types.get(type_index).ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip file {file_index} object {object_index} TypeTree index {type_index} is out of range"
                ))
            })?;
            let type_tree = object
                .recovered_type_tree
                .as_deref()
                .or(serialized_type.type_tree.as_ref())
                .ok_or_else(|| {
                    WorkspaceAnimationError::new(format!(
                        "AnimationClip file {file_index} object {object_index} has no TypeTree for converted serialization"
                    ))
                })?;
            let mut root = payload.root.clone();
            let mut serialization_clip = converted.clone();
            restore_source_script_path_ids(
                &mut serialization_clip,
                &processed.source_script_pointers,
            )?;
            write_animation_clip_conversion(
                &mut root,
                type_tree,
                object_index,
                &serialization_clip,
                max_array_elements,
            )
            .map_err(|error| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip file {file_index} object {object_index} PathID {} could not serialize editor-format conversion: {error}",
                    metadata_object.path_id
                ))
            })?;
            converted_roots.insert((file_index, object_index), root);
        }
    }

    Ok(converted_roots)
}

/// Per-workspace scene facts, checksum cache, and clip root ordering used during conversion.
struct WorkspaceAnimationProcessing {
    scene: SceneFactsTable,
    checksum_cache: PathChecksumCache,
    processed_scripts: ProcessedScriptSet,
    roots_by_clip: AnimationRootsByClip,
    ids: WorkspaceObjectIds,
}

struct ProcessedAnimationClip {
    clip: AnimationClipObject,
    source_script_pointers: HashMap<i64, PPtrValue>,
}

impl WorkspaceAnimationProcessing {
    /// Build the caller state from the loaded source workspace and its materialized scene graph.
    fn new(
        workspace: &SerializedFileWorkspace,
        objects: &ObjectWorkspace,
        references: &MultiFileReferenceGraph<'_>,
        graph: &MaterializedSceneGraph,
        managed_assemblies: Option<&ManagedAssemblyCatalog>,
    ) -> Result<Self, WorkspaceAnimationError> {
        let ids = WorkspaceObjectIds::new(workspace)?;
        let mut scene = scene_facts_from_graph(workspace, objects, references, graph, &ids)?;
        populate_animation_script_facts(
            workspace,
            objects,
            references,
            &ids,
            managed_assemblies,
            &mut scene,
        )?;
        let (blend_shape_crcs, material_property_crcs) =
            animation_curve_lookup_requests(workspace, objects, references)?;
        populate_animation_crc_facts(
            objects,
            references,
            &ids,
            &mut scene,
            &blend_shape_crcs,
            &material_property_crcs,
        )?;
        let (path_assets, roots_by_clip) =
            animation_assets_and_roots(workspace, objects, references, &ids)?;
        let mut checksum_cache = PathChecksumCache::new(PathChecksumLimits::default())
            .map_err(|error| WorkspaceAnimationError::new(error.to_string()))?;
        build_path_checksum_cache(&mut checksum_cache, &path_assets, &scene)
            .map_err(|error| WorkspaceAnimationError::new(error.to_string()))?;
        scene.set_roots(Vec::new());
        Ok(Self {
            scene,
            checksum_cache,
            processed_scripts: ProcessedScriptSet::default(),
            roots_by_clip,
            ids,
        })
    }

    /// Run the production converter for one materialized AnimationClip.
    fn process_clip(
        &mut self,
        file_index: usize,
        object_index: usize,
        root: &SerializedValue,
        version: UnityVersion,
        endian: EndianType,
        references: &MultiFileReferenceGraph<'_>,
    ) -> Result<ProcessedAnimationClip, WorkspaceAnimationError> {
        self.scene.set_roots(
            self.roots_by_clip
                .get(&(file_index, object_index))
                .cloned()
                .unwrap_or_default(),
        );
        let mut clip = read_animation_clip_for_conversion(root, version, endian)
            .map_err(|error| WorkspaceAnimationError::new(error.to_string()))?;
        if !clip.has_clip_binding_constant {
            return Ok(ProcessedAnimationClip {
                clip,
                source_script_pointers: HashMap::new(),
            });
        }
        let source_script_pointers = materialize_script_references(
            &mut clip,
            file_index,
            object_index,
            references,
            &self.ids,
        )?;
        process_clip(
            &mut clip,
            &mut ConverterContext {
                scene: &self.scene,
                checksum_cache: &mut self.checksum_cache,
                processed_scripts: &mut self.processed_scripts,
            },
        )
        .map_err(|error| WorkspaceAnimationError::new(error.to_string()))?;
        Ok(ProcessedAnimationClip {
            clip,
            source_script_pointers,
        })
    }
}

#[derive(Debug)]
pub(super) struct WorkspaceAnimationError(String);

impl WorkspaceAnimationError {
    fn new(message: String) -> Self {
        Self(message)
    }
}

impl Display for WorkspaceAnimationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for WorkspaceAnimationError {}

struct WorkspaceObjectIds {
    file_offsets: Vec<usize>,
}

impl WorkspaceObjectIds {
    fn new(workspace: &SerializedFileWorkspace) -> Result<Self, WorkspaceAnimationError> {
        let mut file_offsets = Vec::new();
        file_offsets
            .try_reserve_exact(workspace.files().len())
            .map_err(|_| {
                WorkspaceAnimationError::new(
                    "could not allocate animation object identity offsets".to_owned(),
                )
            })?;
        let mut next = 0usize;
        for file in workspace.files() {
            file_offsets.push(next);
            next = next
                .checked_add(file.metadata.objects.len())
                .ok_or_else(|| {
                    WorkspaceAnimationError::new(
                        "animation workspace object count overflows".to_owned(),
                    )
                })?;
        }
        if next >= i64::MAX as usize {
            return Err(WorkspaceAnimationError::new(
                "animation workspace has too many objects for the converter identity space"
                    .to_owned(),
            ));
        }
        Ok(Self { file_offsets })
    }

    fn get(&self, file_index: usize, object_index: usize) -> Option<i64> {
        let id = self
            .file_offsets
            .get(file_index)?
            .checked_add(object_index)?
            .checked_add(1)?;
        i64::try_from(id).ok()
    }

    fn identity(&self, identity: WorkspaceSceneObjectIdentity) -> Option<i64> {
        self.get(identity.file_index, identity.object_index)
    }
}

fn populate_animation_script_facts(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    references: &MultiFileReferenceGraph<'_>,
    ids: &WorkspaceObjectIds,
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
    scene: &mut SceneFactsTable,
) -> Result<(), WorkspaceAnimationError> {
    let Some(managed_assemblies) = managed_assemblies else {
        return Ok(());
    };
    for object in objects.objects().iter().filter(|object| {
        references.object_class_id(object.file_index, object.object_index) == Some(115)
    }) {
        let identity = mono_script_export_identity(object).map_err(|error| {
            WorkspaceAnimationError::new(format!(
                "MonoScript {}:{} could not provide animation property identity: {error}",
                object.file_index, object.path_id
            ))
        })?;
        let script = ScriptIdentifier {
            assembly: identity.assembly,
            namespace: identity.namespace,
            name: Some(identity.class),
        };
        if !managed_assemblies.is_valid(&script) {
            continue;
        }
        let unity_version = workspace
            .files()
            .get(object.file_index)
            .and_then(|file| file.metadata.prefix.unity_version_text.as_deref())
            .ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "MonoScript {}:{} has no Unity version for animation field resolution",
                    object.file_index, object.path_id
                ))
            })?;
        let schema = build_mono_behaviour_schema(
            managed_assemblies,
            &script,
            unity_version,
            ManagedSchemaLimits::default(),
        )
        .map_err(|error| {
            WorkspaceAnimationError::new(format!(
                "MonoScript {}:{} animation field schema could not be resolved: {error}",
                object.file_index, object.path_id
            ))
        })?;
        let script_id = ids
            .get(object.file_index, object.object_index)
            .ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "MonoScript {}:{} has no animation workspace identity",
                    object.file_index, object.path_id
                ))
            })?;
        scene.insert_script(
            script_id,
            ScriptFacts {
                identity: format!("{}:{}", object.file_index, object.path_id),
                fields: schema.path_checksum_fields,
            },
        );
    }
    Ok(())
}

fn materialize_script_references(
    clip: &mut AnimationClipObject,
    file_index: usize,
    object_index: usize,
    references: &MultiFileReferenceGraph<'_>,
    ids: &WorkspaceObjectIds,
) -> Result<HashMap<i64, PPtrValue>, WorkspaceAnimationError> {
    let mut source_script_pointers = HashMap::new();
    visit_animation_script_references(clip, |script| {
        let source_pointer = PPtrValue {
            file_id: script.file_id,
            path_id: script.path_id,
        };
        let workspace_id = resolve_mono_script_id(
            file_index,
            object_index,
            source_pointer.file_id,
            source_pointer.path_id,
            references,
            ids,
        )?;
        source_script_pointers
            .entry(workspace_id)
            .or_insert(source_pointer);
        // Curve identity uses the resolved script object, independent of its source file index.
        script.file_id = 0;
        script.path_id = workspace_id;
        Ok(())
    })?;
    Ok(source_script_pointers)
}

fn resolve_mono_script_id(
    file_index: usize,
    object_index: usize,
    file_id: i32,
    path_id: i64,
    references: &MultiFileReferenceGraph<'_>,
    ids: &WorkspaceObjectIds,
) -> Result<i64, WorkspaceAnimationError> {
    let mut target_identity = None;
    for reference in references
        .references_from_object(file_index, object_index)
        .filter(|reference| reference.file_id == file_id && reference.path_id == path_id)
    {
        let target = resolved_object_identity(reference)?.ok_or_else(|| {
            WorkspaceAnimationError::new(format!(
                "AnimationClip {file_index}:{object_index} Script PPtr fileID {file_id} PathID {path_id} resolved to null"
            ))
        })?;
        if references.object_class_id(target.0, target.1) != Some(115) {
            return Err(WorkspaceAnimationError::new(format!(
                "AnimationClip {file_index}:{object_index} Script PPtr fileID {file_id} PathID {path_id} targets non-MonoScript object {}:{}",
                target.0, target.1
            )));
        }
        if target_identity.is_some_and(|previous| previous != target) {
            return Err(WorkspaceAnimationError::new(format!(
                "AnimationClip {file_index}:{object_index} Script PPtr fileID {file_id} PathID {path_id} resolves ambiguously"
            )));
        }
        target_identity = Some(target);
    }
    let target = target_identity.ok_or_else(|| {
        WorkspaceAnimationError::new(format!(
            "AnimationClip {file_index}:{object_index} Script PPtr fileID {file_id} PathID {path_id} has no resolved reference edge"
        ))
    })?;
    ids.get(target.0, target.1).ok_or_else(|| {
        WorkspaceAnimationError::new(format!(
            "AnimationClip {file_index}:{object_index} Script target {}:{} has no animation workspace identity",
            target.0, target.1
        ))
    })
}

fn restore_source_script_path_ids(
    clip: &mut AnimationClipObject,
    source_script_pointers: &HashMap<i64, PPtrValue>,
) -> Result<(), WorkspaceAnimationError> {
    visit_animation_script_references(clip, |script| {
        let source_pointer = source_script_pointers.get(&script.path_id).ok_or_else(|| {
            WorkspaceAnimationError::new(format!(
                "converted AnimationClip Script workspace identity {} has no source PathID mapping",
                script.path_id
            ))
        })?;
        script.file_id = source_pointer.file_id;
        script.path_id = source_pointer.path_id;
        Ok(())
    })
}

fn visit_animation_script_references(
    clip: &mut AnimationClipObject,
    mut visit: impl FnMut(&mut ScriptReference) -> Result<(), WorkspaceAnimationError>,
) -> Result<(), WorkspaceAnimationError> {
    for binding in &mut clip.bindings {
        if let Some(script) = &mut binding.script {
            visit(script)?;
        }
    }
    for curve in &mut clip.float_curves {
        if let Some(script) = &mut curve.script {
            visit(script)?;
        }
    }
    for curve in &mut clip.pptr_curves {
        if let Some(script) = &mut curve.script {
            visit(script)?;
        }
    }
    Ok(())
}

fn scene_facts_from_graph(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    references: &MultiFileReferenceGraph<'_>,
    graph: &MaterializedSceneGraph,
    ids: &WorkspaceObjectIds,
) -> Result<SceneFactsTable, WorkspaceAnimationError> {
    let mut transforms = HashMap::<(usize, usize), &MaterializedTransform>::new();
    for collection in graph.collections() {
        for transform in &collection.transforms {
            transforms.insert(
                (
                    transform.identity.file_index,
                    transform.identity.object_index,
                ),
                transform,
            );
        }
    }

    let mut facts = SceneFactsTable::new();
    for collection in graph.collections() {
        for game_object in &collection.game_objects {
            let game_object_id = ids.identity(game_object.identity).ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "GameObject file {} object {} has no converter identity",
                    game_object.identity.file_index, game_object.identity.object_index
                ))
            })?;
            let name = game_object
                .name
                .clone()
                .or_else(|| {
                    game_object
                        .name_bytes
                        .as_deref()
                        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
                })
                .unwrap_or_default();
            let mut components = Vec::new();
            for slot in &game_object.components {
                let MaterializedComponentTarget::Source { target, .. } = &slot.target else {
                    continue;
                };
                let Some((target_file, target_object)) =
                    scene_target_identity(game_object.identity.file_index, target)
                else {
                    continue;
                };
                let Some(component_id) = ids.get(target_file, target_object) else {
                    return Err(WorkspaceAnimationError::new(format!(
                        "GameObject file {} object {} component slot {} has no converter identity",
                        game_object.identity.file_index,
                        game_object.identity.object_index,
                        slot.source_array_index
                    )));
                };
                let class_id = slot
                    .class_id
                    .or_else(|| references.object_class_id(target_file, target_object));
                if let Some(class_id) = class_id {
                    components.push((class_id, component_id));
                }
            }

            let mut children = Vec::new();
            let transform_identity = game_object.components.iter().find_map(|slot| {
                if !matches!(slot.class_id, Some(4 | 224)) {
                    return None;
                }
                let MaterializedComponentTarget::Source { target, .. } = &slot.target else {
                    return None;
                };
                scene_target_identity(game_object.identity.file_index, target)
            });
            if let Some(transform_identity) = transform_identity {
                if let Some(transform) = transforms.get(&transform_identity) {
                    for child in &transform.children {
                        let Some((child_file, child_object)) =
                            scene_target_identity(transform.identity.file_index, &child.target)
                        else {
                            continue;
                        };
                        let Some(child_transform) = transforms.get(&(child_file, child_object))
                        else {
                            continue;
                        };
                        let Some(game_object_edge) = child_transform.game_object.as_ref() else {
                            continue;
                        };
                        let Some((child_game_object_file, child_game_object_object)) =
                            scene_target_identity(child_file, &game_object_edge.target)
                        else {
                            continue;
                        };
                        if let Some(child_id) =
                            ids.get(child_game_object_file, child_game_object_object)
                        {
                            children.push(child_id);
                        }
                    }
                }
            }
            facts.insert_game_object(
                game_object_id,
                GameObjectFacts {
                    name,
                    components,
                    children,
                },
            );
        }
    }

    for collection in graph.collections() {
        for game_object in &collection.game_objects {
            for slot in &game_object.components {
                if slot.class_id != Some(137) {
                    continue;
                }
                let MaterializedComponentTarget::Source { target, .. } = &slot.target else {
                    continue;
                };
                let Some((renderer_file, renderer_object)) =
                    scene_target_identity(game_object.identity.file_index, target)
                else {
                    continue;
                };
                let Some(renderer_id) = ids.get(renderer_file, renderer_object) else {
                    continue;
                };
                let Some(mesh_reference) = matching_references(
                    references,
                    renderer_file,
                    renderer_object,
                    &["m_Mesh", "Mesh"],
                )
                .next() else {
                    continue;
                };
                if let Some((mesh_file, mesh_object)) = resolved_object_identity(mesh_reference)? {
                    if let Some(mesh_id) = ids.get(mesh_file, mesh_object) {
                        facts.insert_skinned_mesh(renderer_id, mesh_id);
                    }
                }
            }
        }
    }
    let _ = (workspace, objects);
    Ok(facts)
}

fn animation_curve_lookup_requests(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<(HashSet<u32>, HashSet<u32>), WorkspaceAnimationError> {
    let mut blend_shape_crcs = HashSet::new();
    let mut material_property_crcs = HashSet::new();
    for (file_index, file) in workspace.files().iter().enumerate() {
        for object_index in 0..file.metadata.objects.len() {
            if references.object_class_id(file_index, object_index) != Some(74) {
                continue;
            }
            let object = objects.get(file_index, object_index).ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip file {file_index} object {object_index} is missing"
                ))
            })?;
            let payload = object.payload.as_ref().map_err(|error| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip {file_index}:{object_index} could not be decoded while collecting custom-curve lookup keys: {error}"
                ))
            })?;
            let binding_constant = unique_field_alias(
                &payload.root,
                &["m_ClipBindingConstant", "clipBindingConstant"],
            )
            .map_err(WorkspaceAnimationError::new)?;
            let Some(binding_constant) = binding_constant else {
                continue;
            };
            let bindings = unique_field_alias(
                binding_constant,
                &["genericBindings", "GenericBindings"],
            )
            .map_err(WorkspaceAnimationError::new)?
            .ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "AnimationClip {file_index}:{object_index} binding constant has no genericBindings array"
                ))
            })?;
            let SerializedValue::Array(bindings) = bindings else {
                return Err(WorkspaceAnimationError::new(format!(
                    "AnimationClip {file_index}:{object_index} genericBindings is not an array"
                )));
            };
            for (binding_index, binding) in bindings.iter().enumerate() {
                let custom_type = unique_field_alias(binding, &["customType", "CustomType"])
                    .map_err(WorkspaceAnimationError::new)?
                    .and_then(unsigned_value)
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or_else(|| {
                        WorkspaceAnimationError::new(format!(
                            "AnimationClip {file_index}:{object_index} generic binding {binding_index} has an invalid customType"
                        ))
                    })?;
                let attribute = unique_field_alias(binding, &["attribute", "Attribute"])
                    .map_err(WorkspaceAnimationError::new)?
                    .and_then(unsigned_value)
                    .ok_or_else(|| {
                        WorkspaceAnimationError::new(format!(
                            "AnimationClip {file_index}:{object_index} generic binding {binding_index} has an invalid attribute"
                        ))
                    })?;
                match BindingCustomType(custom_type) {
                    BindingCustomType::BLEND_SHAPE => {
                        blend_shape_crcs.insert(attribute);
                    }
                    BindingCustomType::RENDERER_MATERIAL => {
                        material_property_crcs.insert(attribute & CRC28_MASK_FIELD);
                    }
                    _ => {}
                }
            }
        }
    }
    Ok((blend_shape_crcs, material_property_crcs))
}

fn populate_animation_crc_facts(
    objects: &ObjectWorkspace,
    references: &MultiFileReferenceGraph<'_>,
    ids: &WorkspaceObjectIds,
    facts: &mut SceneFactsTable,
    blend_shape_crcs: &HashSet<u32>,
    material_property_crcs: &HashSet<u32>,
) -> Result<(), WorkspaceAnimationError> {
    if !blend_shape_crcs.is_empty() {
        for mesh in objects.objects().iter().filter(|object| {
            references.object_class_id(object.file_index, object.object_index) == Some(43)
        }) {
            let mesh_id = ids.get(mesh.file_index, mesh.object_index).ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "Mesh {}:{} has no converter identity",
                    mesh.file_index, mesh.object_index
                ))
            })?;
            let payload = mesh.payload.as_ref().map_err(|error| {
                WorkspaceAnimationError::new(format!(
                    "Mesh {}:{} could not be decoded for AnimationClip blend-shape lookup: {error}",
                    mesh.file_index, mesh.object_index
                ))
            })?;
            let view = BlendShapeLookupView::from_value(&payload.root).map_err(|error| {
                WorkspaceAnimationError::new(format!(
                    "Mesh {}:{} blend-shape lookup failed: {error}",
                    mesh.file_index, mesh.object_index
                ))
            })?;
            for crc in blend_shape_crcs {
                if let Some(name) = view.find_shape_name_by_crc(*crc) {
                    facts.insert_blend_shape(mesh_id, *crc, name);
                }
            }
        }
    }

    if !material_property_crcs.is_empty() {
        for renderer in objects.objects().iter().filter(|object| {
            references
                .object_class_id(object.file_index, object.object_index)
                .is_some_and(|class_id| RENDERER_CLASS_IDS.contains(&class_id))
        }) {
            let renderer_id = ids
                .get(renderer.file_index, renderer.object_index)
                .ok_or_else(|| {
                    WorkspaceAnimationError::new(format!(
                        "Renderer {}:{} has no converter identity",
                        renderer.file_index, renderer.object_index
                    ))
                })?;
            let mut materials = Vec::new();
            for reference in references
                .references_from_object(renderer.file_index, renderer.object_index)
                .filter(|reference| {
                    field_path_contains_any(reference.field_path, &["m_Materials", "Materials"])
                })
            {
                let Some((material_file, material_object)) = resolved_object_identity(reference)?
                else {
                    materials.push(None);
                    continue;
                };
                if references.object_class_id(material_file, material_object) != Some(21) {
                    materials.push(None);
                    continue;
                }
                let material = objects.get(material_file, material_object).ok_or_else(|| {
                    WorkspaceAnimationError::new(format!(
                        "Renderer {}:{} material target {material_file}:{material_object} is missing",
                        renderer.file_index, renderer.object_index
                    ))
                })?;
                let payload = material.payload.as_ref().map_err(|error| {
                    WorkspaceAnimationError::new(format!(
                        "Renderer {}:{} material {material_file}:{material_object} could not be decoded: {error}",
                        renderer.file_index, renderer.object_index
                    ))
                })?;
                materials.push(Some(MaterialView::from_value(&payload.root).map_err(
                    |error| {
                        WorkspaceAnimationError::new(format!(
                            "Material {material_file}:{material_object} property lookup failed: {error}"
                        ))
                    },
                )?));
            }
            let material_views = materials.iter().map(Option::as_ref).collect::<Vec<_>>();
            for crc28 in material_property_crcs {
                if let Some(name) =
                    MaterialView::find_material_property_name_by_crc28(&material_views, *crc28)
                {
                    facts.insert_material_property(renderer_id, *crc28, &name);
                }
            }
        }
    }
    Ok(())
}

fn animation_assets_and_roots(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    references: &MultiFileReferenceGraph<'_>,
    ids: &WorkspaceObjectIds,
) -> Result<AnimationWorkspaceAssets, WorkspaceAnimationError> {
    let mut path_assets = Vec::new();
    let mut roots_by_clip = HashMap::<(usize, usize), Vec<i64>>::new();
    let mut avatar_tos = HashMap::<(usize, usize), Vec<(u32, String)>>::new();

    for (file_index, file) in workspace.files().iter().enumerate() {
        for object_index in 0..file.metadata.objects.len() {
            if references.object_class_id(file_index, object_index) != Some(90) {
                continue;
            }
            let object = objects.get(file_index, object_index).ok_or_else(|| {
                WorkspaceAnimationError::new(format!(
                    "Avatar workspace object {file_index}:{object_index} is missing"
                ))
            })?;
            let payload = object.payload.as_ref().map_err(|error| {
                WorkspaceAnimationError::new(format!(
                    "Avatar {file_index}:{object_index} could not be decoded: {error}"
                ))
            })?;
            let tos = read_avatar_tos(&payload.root).map_err(|error| {
                WorkspaceAnimationError::new(format!(
                    "Avatar {file_index}:{object_index} TOS could not be read: {error}"
                ))
            })?;
            avatar_tos.insert((file_index, object_index), tos);
        }
    }

    for (file_index, file) in workspace.files().iter().enumerate() {
        for object_index in 0..file.metadata.objects.len() {
            let Some(object) = objects.get(file_index, object_index) else {
                return Err(WorkspaceAnimationError::new(format!(
                    "animation workspace object {file_index}:{object_index} is missing"
                )));
            };
            let class_id = references.object_class_id(file_index, object_index);
            if class_id == Some(90) {
                let payload = object.payload.as_ref().map_err(|error| {
                    WorkspaceAnimationError::new(format!(
                        "Avatar {file_index}:{object_index} could not be decoded: {error}"
                    ))
                })?;
                let tos = read_avatar_tos(&payload.root).map_err(|error| {
                    WorkspaceAnimationError::new(format!(
                        "Avatar {file_index}:{object_index} TOS could not be read: {error}"
                    ))
                })?;
                path_assets.push(PathCacheAsset::Avatar { tos });
            }
            if class_id == Some(95) {
                let payload = object.payload.as_ref().map_err(|error| {
                    WorkspaceAnimationError::new(format!(
                        "Animator {file_index}:{object_index} could not be decoded: {error}"
                    ))
                })?;
                let game_object = resolve_field_target(
                    references,
                    file_index,
                    object_index,
                    &["m_GameObject", "GameObject"],
                )?
                .and_then(|identity| ids.get(identity.0, identity.1));
                let (has_has_transform_hierarchy, has_transform_hierarchy) =
                    transform_hierarchy_flags(&payload.root)?;
                let tos = resolve_field_target(
                    references,
                    file_index,
                    object_index,
                    &["m_Avatar", "Avatar"],
                )?
                .and_then(|identity| avatar_tos.get(&identity).cloned())
                .unwrap_or_default();
                path_assets.push(PathCacheAsset::Animator {
                    avatar_tos: tos,
                    has_has_transform_hierarchy,
                    has_transform_hierarchy,
                    game_object,
                });

                let Some(controller) = resolve_field_target(
                    references,
                    file_index,
                    object_index,
                    &["m_Controller", "Controller"],
                )?
                else {
                    continue;
                };
                let mut contained_clips = Vec::new();
                collect_controller_clips(
                    references,
                    controller,
                    &mut HashSet::new(),
                    &mut contained_clips,
                );
                for clip_identity in contained_clips {
                    if let Some(root_id) = game_object {
                        roots_by_clip
                            .entry(clip_identity)
                            .or_default()
                            .push(root_id);
                    }
                }
            }
            if class_id == Some(111) {
                let Some(game_object) = resolve_field_target(
                    references,
                    file_index,
                    object_index,
                    &["m_GameObject", "GameObject"],
                )?
                .and_then(|identity| ids.get(identity.0, identity.1)) else {
                    path_assets.push(PathCacheAsset::Animation { game_object: None });
                    continue;
                };
                path_assets.push(PathCacheAsset::Animation {
                    game_object: Some(game_object),
                });
                for reference in references.references_from_object(file_index, object_index) {
                    if !field_path_matches_array(
                        reference.field_path,
                        &["m_Animations", "Animations"],
                    ) {
                        continue;
                    }
                    let Some((clip_file, clip_object)) = resolved_object_identity(reference)?
                    else {
                        continue;
                    };
                    if references.object_class_id(clip_file, clip_object) == Some(74) {
                        roots_by_clip
                            .entry((clip_file, clip_object))
                            .or_default()
                            .push(game_object);
                    }
                }
            }
        }
    }
    Ok((path_assets, roots_by_clip))
}

fn collect_controller_clips(
    references: &MultiFileReferenceGraph<'_>,
    controller: (usize, usize),
    visited: &mut HashSet<(usize, usize)>,
    clips: &mut Vec<(usize, usize)>,
) {
    if !visited.insert(controller) {
        return;
    }
    let Some(class_id) = references.object_class_id(controller.0, controller.1) else {
        return;
    };
    let field_names: &'static [&'static str] = if class_id == 91 {
        &["m_AnimationClips", "AnimationClips"]
    } else if class_id == 221 {
        &["m_Clips", "Clips"]
    } else {
        return;
    };
    for reference in references.references_from_object(controller.0, controller.1) {
        if class_id == 91 && !field_path_matches_array(reference.field_path, field_names) {
            continue;
        }
        if class_id == 221 && !field_path_contains_any(reference.field_path, field_names) {
            continue;
        }
        if let Ok(Some(target)) = resolved_object_identity(reference) {
            if references.object_class_id(target.0, target.1) == Some(74)
                && !clips.contains(&target)
            {
                clips.push(target);
            }
        }
    }
    if class_id == 221 {
        for reference in matching_references(
            references,
            controller.0,
            controller.1,
            &["m_Controller", "Controller"],
        ) {
            if let Ok(Some(target)) = resolved_object_identity(reference) {
                collect_controller_clips(references, target, visited, clips);
            }
        }
    }
}

fn read_avatar_tos(root: &SerializedValue) -> Result<Vec<(u32, String)>, String> {
    let value = unique_field_alias(root, &["m_TOS", "TOS"])?;
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let pairs = match value {
        SerializedValue::Array(values) => values.as_slice(),
        SerializedValue::Struct(_) => {
            let Some(data) = unique_field_alias(value, &["data", "Array"])? else {
                return Err("TOS wrapper has no data array".to_owned());
            };
            let SerializedValue::Array(values) = data else {
                return Err("TOS wrapper data field is not an array".to_owned());
            };
            values.as_slice()
        }
        _ => return Err("TOS field is not an array or array wrapper".to_owned()),
    };
    let mut tos = Vec::new();
    tos.try_reserve_exact(pairs.len())
        .map_err(|_| "could not allocate Avatar TOS pairs".to_owned())?;
    for (index, pair) in pairs.iter().enumerate() {
        let key = unique_field_alias(pair, &["first", "key", "Key"])?
            .ok_or_else(|| format!("TOS pair {index} has no key"))?;
        let value = unique_field_alias(pair, &["second", "value", "Value"])?
            .ok_or_else(|| format!("TOS pair {index} has no value"))?;
        let key =
            unsigned_value(key).ok_or_else(|| format!("TOS pair {index} key is not an integer"))?;
        let value =
            string_value(value).ok_or_else(|| format!("TOS pair {index} value is not a string"))?;
        tos.push((key, value));
    }
    Ok(tos)
}

fn transform_hierarchy_flags(
    root: &SerializedValue,
) -> Result<(bool, bool), WorkspaceAnimationError> {
    let field = unique_field_alias(root, &["m_HasTransformHierarchy", "hasTransformHierarchy"])
        .map_err(WorkspaceAnimationError::new)?;
    let Some(value) = field else {
        return Ok((false, false));
    };
    let SerializedValue::Bool(has_transform_hierarchy) = value else {
        return Err(WorkspaceAnimationError::new(
            "Animator HasTransformHierarchy field is not boolean".to_owned(),
        ));
    };
    Ok((true, *has_transform_hierarchy))
}

fn resolve_field_target(
    references: &MultiFileReferenceGraph<'_>,
    file_index: usize,
    object_index: usize,
    names: &'static [&'static str],
) -> Result<Option<(usize, usize)>, WorkspaceAnimationError> {
    let mut matching = matching_references(references, file_index, object_index, names);
    let Some(reference) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        return Err(WorkspaceAnimationError::new(format!(
            "object {file_index}:{object_index} has multiple PPtrs for field {}",
            names.join(" or ")
        )));
    }
    resolved_object_identity(reference)
}

fn matching_references<'borrow, 'graph>(
    references: &'borrow MultiFileReferenceGraph<'graph>,
    file_index: usize,
    object_index: usize,
    names: &'static [&'static str],
) -> impl Iterator<Item = &'borrow MultiFileObjectReference<'graph>> + 'borrow {
    references
        .references_from_object(file_index, object_index)
        .filter(move |reference| field_path_matches(reference.field_path, names))
}

fn field_path_matches(path: &str, names: &[&str]) -> bool {
    path.split('.').any(|segment| {
        names.iter().any(|name| {
            segment == *name
                || segment
                    .strip_prefix(name)
                    .is_some_and(|suffix| suffix.starts_with('[') && suffix.ends_with(']'))
        })
    })
}

fn field_path_matches_array(path: &str, names: &[&str]) -> bool {
    path.split('.').any(|segment| {
        names.iter().any(|name| {
            segment
                .strip_prefix(name)
                .is_some_and(|suffix| suffix.starts_with('[') && suffix.ends_with(']'))
        })
    })
}

fn field_path_contains_any(path: &str, names: &[&str]) -> bool {
    path.split('.').any(|segment| {
        names.iter().any(|name| {
            segment == *name
                || segment
                    .strip_prefix(name)
                    .is_some_and(|suffix| suffix.starts_with('[') || suffix.starts_with('.'))
        })
    })
}

fn resolved_object_identity(
    reference: &MultiFileObjectReference<'_>,
) -> Result<Option<(usize, usize)>, WorkspaceAnimationError> {
    match &reference.resolution {
        MultiFilePPtrResolution::Null => Ok(None),
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => Ok(Some((*target_file_index, *target_object_index))),
        ref resolution => Err(WorkspaceAnimationError::new(format!(
            "animation PPtr {}:{} {} (fileID {}, PathID {}) is unresolved: {resolution:?}",
            reference.source_file_index,
            reference.source_object_index,
            reference.field_path,
            reference.file_id,
            reference.path_id
        ))),
    }
}

fn scene_target_identity(
    source_file_index: usize,
    target: &crate::processing::scene::SceneRelationshipTarget,
) -> Option<(usize, usize)> {
    use crate::processing::scene::SceneRelationshipTarget;
    match target {
        SceneRelationshipTarget::LocalObject { object_index } => {
            Some((source_file_index, *object_index))
        }
        SceneRelationshipTarget::WorkspaceObject {
            file_index,
            object_index,
        } => Some((*file_index, *object_index)),
        _ => None,
    }
}

fn unique_field_alias<'a>(
    value: &'a SerializedValue,
    names: &[&str],
) -> Result<Option<&'a SerializedValue>, String> {
    let mut found = None;
    for name in names {
        let candidate = value
            .unique_field_in_base_chain(name)
            .map_err(|error| format!("field {name} lookup failed: {error}"))?;
        if candidate.is_some() {
            if found.is_some() {
                return Err(format!(
                    "serialized object contains multiple aliases for {}",
                    names.join(" or ")
                ));
            }
            found = candidate;
        }
    }
    Ok(found)
}

fn unsigned_value(value: &SerializedValue) -> Option<u32> {
    match value {
        SerializedValue::U8(value) => Some(u32::from(*value)),
        SerializedValue::U16(value) => Some(u32::from(*value)),
        SerializedValue::U32(value) => Some(*value),
        SerializedValue::I8(value) => u32::try_from(*value).ok(),
        SerializedValue::I16(value) => u32::try_from(*value).ok(),
        SerializedValue::I32(value) => u32::try_from(*value).ok(),
        SerializedValue::I64(value) => u32::try_from(*value).ok(),
        SerializedValue::U64(value) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn string_value(value: &SerializedValue) -> Option<String> {
    match value {
        SerializedValue::StringBytes(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::scene_wiring::{
        process_workspace_scenes_and_prefabs, WorkspaceSceneProcessingLimits,
    };
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::processing::managed_assembly::ManagedAssemblyCatalogLimits;
    use crate::resources::animation::scene_facts::AnimationSceneFacts;
    use crate::serialized_file::object_workspace::ObjectWorkspaceLimits;
    use crate::serialized_file::reference_graph::MultiFileReferenceGraphLimits;
    use crate::serialized_file::workspace::SerializedFileWorkspaceLimits;

    #[test]
    fn source_managed_monoscript_populates_animation_checksum_facts() {
        let source_fixture =
            include_str!("../processing/managed_assembly_path_checksum_2022_3_fixture.txt");
        let input_hex = source_fixture
            .lines()
            .find_map(|line| {
                line.strip_prefix("INPUT_HEX|")
                    .and_then(|row| row.split_once('|').map(|(_, hex)| hex))
            })
            .expect("source-managed schema fixture input bytes");
        assert_eq!(input_hex.len() % 2, 0);
        let input_bytes = input_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("source fixture hexadecimal input")
            })
            .collect::<Vec<_>>();
        assert!(source_fixture
            .lines()
            .any(|line| line == "UNITY_VERSION|2022.3.22f1"));
        let assembly_bytes =
            include_bytes!("../processing/managed_assembly_schema_complex_fixture.dll");

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/managed_schema.assets", &input_bytes)
            .unwrap();
        filesystem
            .write_all_bytes("/game/ComplexFixtureScripts.dll", assembly_bytes)
            .unwrap();
        let input_paths = [
            DependencyFilePath {
                file_name: "managed_schema.assets".to_owned(),
                virtual_path: "/game/managed_schema.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "ComplexFixtureScripts.dll".to_owned(),
                virtual_path: "/game/ComplexFixtureScripts.dll".to_owned(),
            },
        ];
        let (managed_assemblies, report) = ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &input_paths,
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(report.loaded_count, 1);

        let serialized_paths = [input_paths[0].clone()];
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &serialized_paths,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        assert_eq!(
            workspace.files()[0]
                .metadata
                .prefix
                .unity_version_text
                .as_deref(),
            Some("2022.3.22f1")
        );
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let references = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let ids = WorkspaceObjectIds::new(&workspace).unwrap();
        let mono_script = objects
            .objects()
            .iter()
            .find(|object| {
                object.path_id == 7
                    && references.object_class_id(object.file_index, object.object_index)
                        == Some(115)
            })
            .expect("source-authored MonoScript object");
        let script_id = ids
            .get(mono_script.file_index, mono_script.object_index)
            .expect("MonoScript workspace identity");
        let mut scene = SceneFactsTable::new();
        populate_animation_script_facts(
            &workspace,
            &objects,
            &references,
            &ids,
            Some(&managed_assemblies),
            &mut scene,
        )
        .unwrap();
        let facts = scene
            .script_facts(ScriptReference {
                file_id: 0,
                path_id: script_id,
            })
            .expect("managed MonoScript animation facts");
        assert_eq!(facts.identity, "0:7");

        let source_paths = source_fixture
            .lines()
            .filter_map(|line| line.strip_prefix("SCRIPT_PATH|"))
            .map(|line| {
                let (path, checksum) = line.split_once('|').expect("source path checksum");
                (
                    path,
                    checksum.parse::<u32>().expect("source checksum integer"),
                )
            })
            .collect::<Vec<_>>();
        let mut cache = PathChecksumCache::new(PathChecksumLimits::default()).unwrap();
        cache.add_fields(&facts.fields, "", 0).unwrap();
        assert_eq!(source_paths.len(), 8);
        for (path, checksum) in source_paths {
            assert_eq!(cache.try_get_path(checksum), Some(path));
        }
        let source_lookups = source_fixture
            .lines()
            .filter_map(|line| line.strip_prefix("SCRIPT_PATH_LOOKUP|"))
            .map(|line| {
                let mut parts = line.splitn(3, '|');
                let path = parts.next().expect("source lookup path");
                let checksum = parts
                    .next()
                    .expect("source lookup checksum")
                    .parse::<u32>()
                    .expect("source lookup checksum integer");
                let expected = parts.next().expect("source lookup result");
                (path, checksum, expected)
            })
            .collect::<Vec<_>>();
        assert_eq!(source_lookups.len(), 3);
        for (path, checksum, expected) in source_lookups {
            assert_eq!(
                cache.try_get_path(checksum).unwrap_or("<null>"),
                expected,
                "MonoScript path lookup for {path:?} matches source"
            );
        }
    }

    #[test]
    fn source_generated_workspace_builds_animation_roots_and_hierarchy_paths() {
        let bytes = include_bytes!(
            "../resources/animation/animation_workspace_existing_curves_fixture.assets"
        );
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/animation")).unwrap();
        filesystem
            .write_all_bytes(
                "/animation/animation_workspace_existing_curves.assets",
                bytes,
            )
            .unwrap();
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[DependencyFilePath {
                file_name: "animation_workspace_existing_curves.assets".to_owned(),
                virtual_path: "/animation/animation_workspace_existing_curves.assets".to_owned(),
            }],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let references = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let scene_processing = process_workspace_scenes_and_prefabs(
            &filesystem,
            &workspace,
            &objects,
            &references,
            WorkspaceSceneProcessingLimits::default(),
        )
        .unwrap();

        let mut animation = WorkspaceAnimationProcessing::new(
            &workspace,
            &objects,
            &references,
            &scene_processing.graph,
            None,
        )
        .unwrap();
        let clip_object = objects
            .objects()
            .iter()
            .find(|object| object.factory.class_id == 74)
            .expect("source AnimationClip object");
        let payload = clip_object.payload.as_ref().unwrap();
        let processed = animation
            .process_clip(
                clip_object.file_index,
                clip_object.object_index,
                &payload.root,
                UnityVersion::parse("2022.3.0f1").unwrap(),
                EndianType::Little,
                &references,
            )
            .unwrap();
        let clip = processed.clip;
        crate::resources::animation::converter_fixture::assert_existing_curves_match_source(&clip);
        let mut converted_root = payload.root.clone();
        let serialized_type = clip_object.resolved_type_index.and_then(|index| {
            workspace.files()[clip_object.file_index]
                .metadata
                .types
                .get(index)
        });
        let type_tree = clip_object
            .recovered_type_tree
            .as_deref()
            .or_else(|| {
                serialized_type.and_then(|serialized_type| serialized_type.type_tree.as_ref())
            })
            .expect("source-generated AnimationClip TypeTree");

        let mut external_script_root = payload.root.clone();
        let float_curves = external_script_root
            .unique_field_in_base_chain_mut("m_FloatCurves")
            .unwrap()
            .expect("source AnimationClip float curves");
        let SerializedValue::Array(float_curves) = float_curves else {
            panic!("source AnimationClip float curves are not an array");
        };
        let script_pointer = float_curves[0]
            .unique_field_in_base_chain_mut("script")
            .unwrap()
            .expect("source float curve script pointer");
        *script_pointer = SerializedValue::PPtr {
            file_id: 1,
            path_id: 7,
        };
        let mut external_script_clip = read_animation_clip_for_conversion(
            &external_script_root,
            UnityVersion::parse("2022.3.0f1").unwrap(),
            EndianType::Little,
        )
        .unwrap();
        assert_eq!(
            external_script_clip.float_curves[0].script,
            Some(ScriptReference {
                file_id: 1,
                path_id: 7,
            })
        );
        let mut external_script_round_trip = external_script_root.clone();
        crate::resources::animation::serialized_clip::write_animation_clip_conversion(
            &mut external_script_round_trip,
            type_tree,
            clip_object.object_index,
            &external_script_clip,
            1_000_000,
        )
        .unwrap();
        external_script_clip = read_animation_clip_for_conversion(
            &external_script_round_trip,
            UnityVersion::parse("2022.3.0f1").unwrap(),
            EndianType::Little,
        )
        .unwrap();
        assert_eq!(
            external_script_clip.float_curves[0].script,
            Some(ScriptReference {
                file_id: 1,
                path_id: 7,
            }),
            "conversion serialization preserves the external MonoScript PPtr pair"
        );

        let mut serialization_clip = clip.clone();
        restore_source_script_path_ids(&mut serialization_clip, &processed.source_script_pointers)
            .unwrap();
        crate::resources::animation::serialized_clip::write_animation_clip_conversion(
            &mut converted_root,
            type_tree,
            clip_object.object_index,
            &serialization_clip,
            1_000_000,
        )
        .unwrap();
        let serialized_clip = read_animation_clip_for_conversion(
            &converted_root,
            UnityVersion::parse("2022.3.0f1").unwrap(),
            EndianType::Little,
        )
        .unwrap();
        let serialized_expected = clip.clone();
        assert!(!type_tree
            .nodes
            .iter()
            .any(|node| { matches!(node.name.as_deref(), Some("tangentMode" | "TangentMode")) }));
        let expected_root = scene_processing
            .graph
            .collections()
            .iter()
            .flat_map(|collection| &collection.game_objects)
            .find(|game_object| game_object.name.as_deref() == Some("Root"))
            .expect("source Animator root GameObject");
        let expected_root_id = WorkspaceObjectIds::new(&workspace)
            .unwrap()
            .identity(expected_root.identity)
            .unwrap();

        assert_eq!(clip.position_curves[0].path, "preserved/vector");
        assert_eq!(clip.rotation_curves[0].path, "preserved/rotation");
        assert_eq!(clip.float_curves[0].path, "preserved/float");
        assert_eq!(clip.pptr_curves[0].path, "preserved/pointer");
        assert_eq!(
            serialized_clip.position_curves,
            serialized_expected.position_curves
        );
        assert_eq!(
            serialized_clip.rotation_curves,
            serialized_expected.rotation_curves
        );
        assert_eq!(
            serialized_clip.scale_curves,
            serialized_expected.scale_curves
        );
        assert_eq!(
            serialized_clip.euler_curves,
            serialized_expected.euler_curves
        );
        assert_eq!(
            serialized_clip.float_curves,
            serialized_expected.float_curves
        );
        assert_eq!(serialized_clip.pptr_curves, clip.pptr_curves);
        assert_eq!(clip.position_curves[0].curve[0].time.to_bits(), 0x3E00_0000);
        assert_eq!(clip.pptr_curves[0].curve[0].value.unwrap().path_id, 1010);
        assert_eq!(
            animation
                .checksum_cache
                .try_get_path(crate::resources::blend_shape::crc32_ieee(b"Arm")),
            Some("Arm")
        );
        assert_eq!(
            animation
                .roots_by_clip
                .get(&(clip_object.file_index, clip_object.object_index)),
            Some(&vec![expected_root_id])
        );

        let root_catalog = crate::resource::RootResourceFileCatalog::default();
        let provider = crate::io::platform_structure::PlatformResourceProvider::new(None, None);
        let settings = crate::export::settings_file::ExportSettingsFile::from(
            crate::export::settings::ExportDefaults::default(),
        );
        let output_root = crate::test_support::unique_test_directory("animation-clip-project-e2e");
        crate::export::project_workflow::export_workspace_yaml_project_with_settings(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &references,
            &settings,
            crate::export::project_workflow::WorkspaceProjectExportLimits::default(),
        )
        .unwrap();
        let mut pending_directories = vec![output_root.clone()];
        let mut animation_documents = Vec::new();
        while let Some(directory) = pending_directories.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    pending_directories.push(path);
                } else if path.extension().and_then(|extension| extension.to_str()) == Some("anim")
                {
                    animation_documents.push(std::fs::read_to_string(path).unwrap());
                }
            }
        }
        assert_eq!(animation_documents.len(), 1);
        assert!(animation_documents[0].contains("path: Arm"));
        assert!(animation_documents[0].contains("m_EulerCurves:"));
        assert!(animation_documents[0].contains("m_PPtrCurves:"));
        std::fs::remove_dir_all(output_root).unwrap();
    }

    #[test]
    fn source_external_monoscript_binding_matches_project_yaml_identity() {
        const ANIMATION_BYTES: &[u8] = include_bytes!(
            "../resources/animation/animation_workspace_external_script_2022_3_fixture.assets"
        );
        const SCRIPT_FILE_BYTES: &[u8] = include_bytes!(
            "../resources/animation/animation_external_script_target_2022_3_fixture.assets"
        );
        const ASSEMBLY_BYTES: &[u8] =
            include_bytes!("../processing/managed_assembly_schema_complex_fixture.dll");
        const SOURCE_FIXTURE: &str = include_str!(
            "../resources/animation/animation_external_script_project_2022_3_fixture.txt"
        );

        assert!(SOURCE_FIXTURE
            .lines()
            .any(|line| line == "UNITY_VERSION|2022.3.22f1"));
        for (file_name, bytes) in [
            ("animation_workspace.assets", ANIMATION_BYTES),
            ("scripts.assets", SCRIPT_FILE_BYTES),
            ("ComplexFixtureScripts.dll", ASSEMBLY_BYTES),
        ] {
            let input_row = SOURCE_FIXTURE
                .lines()
                .filter_map(|line| {
                    line.strip_prefix("INPUT|")
                        .or_else(|| line.strip_prefix("ASSEMBLY|"))
                })
                .find(|row| row.starts_with(&format!("{file_name}|")));
            let input_row = input_row
                .unwrap_or_else(|| panic!("source fixture has no input identity for {file_name}"));
            let mut columns = input_row.split('|');
            assert_eq!(columns.next(), Some(file_name));
            let byte_count = columns
                .next()
                .expect("source input byte count")
                .parse::<usize>()
                .expect("source input byte count is numeric");
            assert_eq!(bytes.len(), byte_count, "source input size for {file_name}");
            assert!(columns.next().is_some_and(|sha| sha.len() == 64));
        }

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/animation")).unwrap();
        filesystem
            .write_all_bytes("/animation/animation_workspace.assets", ANIMATION_BYTES)
            .unwrap();
        filesystem
            .write_all_bytes("/animation/scripts.assets", SCRIPT_FILE_BYTES)
            .unwrap();
        filesystem
            .write_all_bytes("/animation/ComplexFixtureScripts.dll", ASSEMBLY_BYTES)
            .unwrap();
        let assembly_paths = [DependencyFilePath {
            file_name: "ComplexFixtureScripts.dll".to_owned(),
            virtual_path: "/animation/ComplexFixtureScripts.dll".to_owned(),
        }];
        let (managed_assemblies, assembly_report) = ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &assembly_paths,
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(assembly_report.loaded_count, 1);
        let serialized_paths = [
            DependencyFilePath {
                file_name: "animation_workspace.assets".to_owned(),
                virtual_path: "/animation/animation_workspace.assets".to_owned(),
            },
            DependencyFilePath {
                file_name: "scripts.assets".to_owned(),
                virtual_path: "/animation/scripts.assets".to_owned(),
            },
        ];
        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &serialized_paths,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let mut objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let recovery = objects
            .recover_mono_behaviour_structures(
                &filesystem,
                &mut workspace,
                &managed_assemblies,
                ObjectWorkspaceLimits::default(),
                ManagedSchemaLimits::default(),
            )
            .unwrap();
        assert_eq!(recovery.behaviours.len(), 1);
        assert!(matches!(
            recovery.behaviours[0].outcome,
            crate::serialized_file::object_workspace::ManagedBehaviourRecoveryOutcome::Recovered { .. }
        ));
        let references = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let clip = objects
            .objects()
            .iter()
            .find(|object| object.factory.class_id == 74)
            .expect("source-generated AnimationClip");
        let script_edges = references
            .references_from_object(clip.file_index, clip.object_index)
            .filter(|reference| reference.file_id == 1 && reference.path_id == 7)
            .collect::<Vec<_>>();
        assert_eq!(script_edges.len(), 1, "external MonoScript PPtr is scanned");
        let script_identity = resolved_object_identity(script_edges[0])
            .unwrap()
            .expect("external MonoScript PPtr resolves");
        assert_eq!(
            references.object_class_id(script_identity.0, script_identity.1),
            Some(115),
            "external script target is a MonoScript"
        );
        assert!(managed_assemblies.is_valid(
            &ScriptIdentifier::try_new("ComplexFixtureScripts", "Game", "RichController").unwrap()
        ));

        let root_catalog = crate::resource::RootResourceFileCatalog::default();
        let provider = crate::io::platform_structure::PlatformResourceProvider::new(None, None);
        let settings = crate::export::settings_file::ExportSettingsFile::from(
            crate::export::settings::ExportDefaults::default(),
        );
        let output_root =
            crate::test_support::unique_test_directory("animation-external-script-project-e2e");
        crate::export::project_workflow::export_workspace_yaml_project_with_assembly_catalog(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &references,
            &settings,
            true,
            Some(&managed_assemblies),
            crate::export::project_workflow::WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let mut pending_directories = vec![output_root.clone()];
        let mut animation_documents = Vec::new();
        while let Some(directory) = pending_directories.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    pending_directories.push(path);
                } else if path.extension().and_then(|extension| extension.to_str()) == Some("anim")
                {
                    animation_documents.push(std::fs::read_to_string(path).unwrap());
                }
            }
        }
        assert_eq!(animation_documents.len(), 1);
        let plugin_meta = std::fs::read_to_string(
            output_root.join("Assets/Plugins/ComplexFixtureScripts.dll.meta"),
        )
        .unwrap();
        let plugin_guid = plugin_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported assembly metadata GUID");
        let source_plugin_guid = SOURCE_FIXTURE
            .lines()
            .find_map(|line| line.strip_prefix("PLUGIN_GUID|"))
            .expect("source assembly metadata GUID");
        assert_eq!(plugin_guid, source_plugin_guid);

        let lines = animation_documents[0].lines().collect::<Vec<_>>();
        let attribute_index = lines
            .iter()
            .position(|line| line.trim() == "attribute: health")
            .expect("managed property curve emitted to AnimationClip YAML");
        let block_start = (0..attribute_index)
            .rev()
            .find(|index| lines[*index].starts_with("  - serializedVersion:"))
            .expect("script curve block start");
        let block_end = (attribute_index + 1..lines.len())
            .find(|index| lines[*index].starts_with("  - serializedVersion:"))
            .unwrap_or(lines.len());
        let source_outputs = SOURCE_FIXTURE
            .lines()
            .filter_map(|line| line.strip_prefix("SCRIPT_OUTPUT|"))
            .map(|row| {
                let (name, value) = row.split_once('|').expect("source script output row");
                (name, value)
            })
            .collect::<Vec<_>>();
        assert_eq!(source_outputs.len(), 4);
        for (name, expected_value) in source_outputs {
            let prefix = format!("{name}:");
            let actual = lines[block_start..block_end]
                .iter()
                .map(|line| line.trim())
                .find(|line| line.starts_with(&prefix))
                .unwrap_or_else(|| panic!("script curve output has no {name} field"));
            let actual = actual.replace(plugin_guid, "__PLUGIN_GUID__");
            assert_eq!(actual, format!("{name}: {expected_value}"));
        }
        let tangent_row = SOURCE_FIXTURE
            .lines()
            .find_map(|line| line.strip_prefix("SCRIPT_KEY|"))
            .expect("source health-curve keyframe projection");
        let mut tangent_columns = tangent_row.split('|');
        assert_eq!(tangent_columns.next(), Some("tangentMode"));
        let tangent_value = tangent_columns.next().expect("source tangentMode value");
        let tangent_count = tangent_columns
            .next()
            .expect("source tangentMode count")
            .parse::<usize>()
            .expect("source tangentMode count is numeric");
        assert!(tangent_columns.next().is_none(), "source tangent row shape");
        let actual_tangent_count = lines[block_start..block_end]
            .iter()
            .map(|line| line.trim())
            .filter(|line| *line == format!("tangentMode: {tangent_value}"))
            .count();
        assert_eq!(actual_tangent_count, tangent_count);
        std::fs::remove_dir_all(output_root).unwrap();
    }
}
