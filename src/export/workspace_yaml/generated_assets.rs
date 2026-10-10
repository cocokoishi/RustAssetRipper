use std::str;

use crate::export::scene_wiring::MaterializedGeneratedTransform;
use crate::processing::asset_factory::{
    reference_class_layouts, ClassLayoutSource, FactoryUnityVersion,
};
use crate::processing::prefab_processing::{
    materialize_planned_game_object_component_append,
    materialize_planned_generated_transform_payload, WorkspaceGeneratedConstructorPointer,
    WorkspaceGeneratedPointerField, WorkspaceGeneratedTransformPayload,
    WorkspaceProcessedCollectionKind, WorkspaceProcessedDependencyTarget,
    WorkspaceProcessedPrefabAssetKind, WorkspaceProcessedPrefabAssetPlan,
    WorkspaceSourceDependencyAppendTarget, WorkspaceSourceDependencyPointerField,
};
use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::metadata::{SerializedFileMetadata, SerializedType};
use crate::serialized_file::object::{
    ObjectDefaultValueLimits, ObjectEncodeLimits, SerializedValue,
};
use crate::serialized_file::object_workspace::WorkspaceObject;
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::super::pointer_map::{
    ProjectExportPointerMap, ProjectPointerRequest, ProjectPointerResolution,
};
use super::super::prefab_collection::{PrefabAssetRepresentation, PrefabExportIdPlan};
use super::super::scene_collection::{ExportAssetCollectionIdentity, ExportAssetIdentity};
use super::super::unity_yaml::UnityYamlDocument;
use super::super::value_yaml::{
    serialized_object_value_to_unity_yaml_document, UnityYamlObjectConversionHints,
    UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode, UnityYamlPPtrRequest,
};
use super::error::UnityYamlWorkspaceExportError;
use super::model::{
    WorkspaceGeneratedComponentYamlOverlay, WorkspaceGeneratedComponentYamlPointer,
    WorkspaceGeneratedPrefabInternalPointer, WorkspaceYamlExportContext,
};

/// Processed collection scope used by generated Transform YAML execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GeneratedTransformCollectionScope {
    Scene { scene_index: usize },
    Prefab,
}

impl GeneratedTransformCollectionScope {
    fn matches(self, kind: &WorkspaceProcessedCollectionKind) -> bool {
        match self {
            Self::Scene { scene_index } => {
                *kind == WorkspaceProcessedCollectionKind::SceneGeneratedAssets { scene_index }
            }
            Self::Prefab => *kind == WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
        }
    }

    fn description(self) -> String {
        match self {
            Self::Scene { scene_index } => {
                format!("the generated collection for Scene {scene_index}")
            }
            Self::Prefab => "the generated Transform collection for Prefabs".to_owned(),
        }
    }
}

/// Type metadata and processed-asset plan needed to execute generated Transform assignments.
#[derive(Clone, Copy)]
pub struct WorkspaceGeneratedTransformYamlContext<'a> {
    pub processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
    collection_scope: GeneratedTransformCollectionScope,
    materialized_transforms: Option<&'a [MaterializedGeneratedTransform]>,
    metadata: Option<&'a SerializedFileMetadata>,
    transform_type_index: Option<usize>,
    legacy_prefab_export: Option<WorkspaceGeneratedLegacyPrefabYamlContext<'a>>,
    pub default_value_limits: ObjectDefaultValueLimits,
    pub encode_limits: ObjectEncodeLimits,
}

#[derive(Clone, Copy)]
struct WorkspaceGeneratedLegacyPrefabYamlContext<'a> {
    export_ids: &'a PrefabExportIdPlan,
    unity_version_text: &'a str,
}

impl<'a> WorkspaceGeneratedTransformYamlContext<'a> {
    pub fn new(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        scene_index: usize,
        metadata: &'a SerializedFileMetadata,
        transform_type_index: usize,
    ) -> Self {
        Self {
            processed_assets,
            collection_scope: GeneratedTransformCollectionScope::Scene { scene_index },
            materialized_transforms: None,
            metadata: Some(metadata),
            transform_type_index: Some(transform_type_index),
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    pub fn for_prefabs(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        metadata: &'a SerializedFileMetadata,
        transform_type_index: usize,
    ) -> Self {
        Self {
            processed_assets,
            collection_scope: GeneratedTransformCollectionScope::Prefab,
            materialized_transforms: None,
            metadata: Some(metadata),
            transform_type_index: Some(transform_type_index),
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    /// Use the payloads already materialized by the workspace processing pass. Each generated
    /// Transform retains the source file and TypeTree that supplied its serialized layout, so a
    /// collection may safely contain generated objects from different input files or versions.
    pub fn with_materialized_scene_transforms(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        scene_index: usize,
        generated_transforms: &'a [MaterializedGeneratedTransform],
    ) -> Self {
        Self {
            processed_assets,
            collection_scope: GeneratedTransformCollectionScope::Scene { scene_index },
            materialized_transforms: Some(generated_transforms),
            metadata: None,
            transform_type_index: None,
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    /// Prefab counterpart to [`Self::with_materialized_scene_transforms`].
    pub fn with_materialized_prefab_transforms(
        processed_assets: &'a WorkspaceProcessedPrefabAssetPlan,
        generated_transforms: &'a [MaterializedGeneratedTransform],
    ) -> Self {
        Self {
            processed_assets,
            collection_scope: GeneratedTransformCollectionScope::Prefab,
            materialized_transforms: Some(generated_transforms),
            metadata: None,
            transform_type_index: None,
            legacy_prefab_export: None,
            default_value_limits: ObjectDefaultValueLimits::default(),
            encode_limits: ObjectEncodeLimits::default(),
        }
    }

    /// Attach the generated legacy Prefab identity and source version for one Prefab collection.
    pub fn with_legacy_prefab_export(
        mut self,
        export_ids: &'a PrefabExportIdPlan,
        unity_version_text: &'a str,
    ) -> Self {
        self.legacy_prefab_export = Some(WorkspaceGeneratedLegacyPrefabYamlContext {
            export_ids,
            unity_version_text,
        });
        self
    }
}

pub(super) fn workspace_generated_transform_assignment_to_unity_yaml_document(
    export_context: &WorkspaceYamlExportContext<'_, '_, '_>,
    source: ExportAssetIdentity,
    file_id: i64,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let workspace = export_context.workspace;
    let reference_graph = export_context.reference_graph;
    let pointer_map = export_context.pointer_map;
    let current_collection_index = export_context.current_collection_index;
    let missing_reference_guid = export_context.missing_reference_guid;
    let limits = export_context.limits;
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedTransformInvariant { source, message }
    };
    if !context.processed_assets.is_complete() {
        return Err(invariant_error(format!(
            "processed asset plan is incomplete: {}",
            context
                .processed_assets
                .incomplete_reason()
                .unwrap_or("no reason was recorded")
        )));
    }
    let ExportAssetCollectionIdentity::ProcessedCollection { collection_index } = source.collection
    else {
        return Err(invariant_error(
            "generated Transform assignment has SerializedFile collection scope".to_owned(),
        ));
    };
    let collection = context
        .processed_assets
        .collections()
        .get(collection_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "processed collection {collection_index} is absent from the asset plan"
            ))
        })?;
    if collection.collection_index != collection_index
        || !context.collection_scope.matches(&collection.kind)
    {
        return Err(invariant_error(format!(
            "processed collection {collection_index} is not {}",
            context.collection_scope.description()
        )));
    }
    let asset = collection.assets.get(source.object_index).ok_or_else(|| {
        invariant_error(format!(
            "processed collection {collection_index} has no planned asset at object index {}",
            source.object_index
        ))
    })?;
    if asset.identity.collection_index != collection_index
        || asset.identity.path_id != source.path_id
        || asset.identity.class_id != source.class_id
    {
        return Err(invariant_error(
            "processed asset identity disagrees with the YAML assignment".to_owned(),
        ));
    }
    let WorkspaceProcessedPrefabAssetKind::GeneratedTransform { game_object, .. } = &asset.kind
    else {
        return Err(invariant_error(format!(
            "processed asset {}:{} is not a generated Transform",
            collection_index, source.object_index
        )));
    };
    let mut constructor_pointers = context
        .processed_assets
        .constructor_pointers()
        .iter()
        .filter(|pointer| {
            pointer.source_asset == asset.identity
                && pointer.field == WorkspaceGeneratedPointerField::TransformGameObject
        });
    let constructor_pointer = constructor_pointers.next().copied().ok_or_else(|| {
        invariant_error("planned Transform has no constructor GameObject PPtr".to_owned())
    })?;
    if constructor_pointers.next().is_some() {
        return Err(invariant_error(
            "planned Transform has duplicate constructor GameObject PPtrs".to_owned(),
        ));
    }
    if constructor_pointer.target != *game_object {
        return Err(invariant_error(
            "constructor PPtr target disagrees with the generated Transform plan".to_owned(),
        ));
    }

    let target_file = workspace
        .files()
        .get(game_object.file_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "planned GameObject file {} is absent from the workspace",
                game_object.file_index
            ))
        })?;
    let target_object = target_file
        .metadata
        .objects
        .get(game_object.object_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "planned GameObject object {} is absent from file {}",
                game_object.object_index, game_object.file_index
            ))
        })?;
    if target_object.path_id != game_object.path_id || target_object.raw_type_id != 1 {
        return Err(invariant_error(
            "planned GameObject identity disagrees with its SerializedFile object record"
                .to_owned(),
        ));
    }
    let reference_identity = reference_graph
        .object_identity(game_object.file_index, game_object.object_index)
        .ok_or_else(|| {
            invariant_error("reference graph has no planned GameObject identity".to_owned())
        })?;
    if reference_identity.path_id != game_object.path_id
        || reference_graph.object_class_id(game_object.file_index, game_object.object_index)
            != Some(1)
    {
        return Err(invariant_error(
            "planned GameObject identity disagrees with the reference graph".to_owned(),
        ));
    }

    if let Some(materialized_transforms) = context.materialized_transforms {
        let mut matches = materialized_transforms
            .iter()
            .filter(|transform| transform.asset == asset.identity);
        let transform = matches.next().ok_or_else(|| {
            invariant_error(format!(
                "generated Transform {:?} was not materialized by workspace processing",
                asset.identity
            ))
        })?;
        if matches.next().is_some() {
            return Err(invariant_error(format!(
                "generated Transform {:?} has multiple materialized payloads",
                asset.identity
            )));
        }
        if transform.game_object != *game_object || transform.payload.asset != asset.identity {
            return Err(invariant_error(
                "materialized Transform identity disagrees with its processed asset plan"
                    .to_owned(),
            ));
        }
        let metadata = &workspace
            .files()
            .get(transform.type_file_index)
            .ok_or_else(|| {
                invariant_error(format!(
                    "generated Transform type file {} is absent from the workspace",
                    transform.type_file_index
                ))
            })?
            .metadata;
        generated_transform_payload_to_unity_yaml_document(
            &transform.payload,
            metadata,
            source,
            file_id,
            constructor_pointer,
            pointer_map,
            current_collection_index,
            missing_reference_guid,
            limits,
        )
    } else {
        let metadata = context.metadata.ok_or_else(|| {
            invariant_error("generated Transform YAML has no TypeTree metadata context".to_owned())
        })?;
        let type_index = context.transform_type_index.ok_or_else(|| {
            invariant_error("generated Transform YAML has no TypeTree index".to_owned())
        })?;
        let payload = materialize_planned_generated_transform_payload(
            context.processed_assets,
            asset.identity,
            metadata,
            type_index,
            context.default_value_limits,
            context.encode_limits,
        )
        .map_err(|error| invariant_error(error.to_string()))?;
        generated_transform_payload_to_unity_yaml_document(
            &payload,
            metadata,
            source,
            file_id,
            constructor_pointer,
            pointer_map,
            current_collection_index,
            missing_reference_guid,
            limits,
        )
    }
}

pub(super) fn workspace_generated_prefab_internal_pointer(
    source: ExportAssetIdentity,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<Option<WorkspaceGeneratedPrefabInternalPointer>, UnityYamlWorkspaceExportError> {
    let Some(legacy) = context.legacy_prefab_export else {
        return Ok(None);
    };
    let plan = legacy.export_ids;
    if plan.representation != PrefabAssetRepresentation::LegacyPrefabMarker
        || !plan.emit_prefab_asset
        || !plan.exportable_assets.contains(&source)
    {
        return Ok(None);
    }
    let mut main_assets = plan
        .registered_assets
        .iter()
        .filter(|asset| asset.is_main_asset);
    let Some(main) = main_assets.next() else {
        return Err(UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
            source,
            message: "legacy Prefab plan has no main asset identity".to_owned(),
        });
    };
    if main_assets.next().is_some()
        || main.asset.class_id != 1001
        || !matches!(
            main.asset.collection,
            ExportAssetCollectionIdentity::ProcessedCollection { .. }
        )
    {
        return Err(UnityYamlWorkspaceExportError::GeneratedPrefabInvariant {
            source,
            message: "legacy Prefab main asset is not one generated class-1001 asset".to_owned(),
        });
    }
    Ok(Some(WorkspaceGeneratedPrefabInternalPointer {
        source,
        target: main.asset,
    }))
}

pub(super) fn workspace_generated_component_yaml_overlay(
    workspace: &SerializedFileWorkspace,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<Option<WorkspaceGeneratedComponentYamlOverlay>, UnityYamlWorkspaceExportError> {
    if source.class_id != 1 {
        return Ok(None);
    }
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedComponentOverlayInvariant { source, message }
    };
    if !context.processed_assets.is_complete() {
        return Err(invariant_error(format!(
            "processed asset plan is incomplete: {}",
            context
                .processed_assets
                .incomplete_reason()
                .unwrap_or("no reason was recorded")
        )));
    }
    if object.file_index
        != match source.collection {
            ExportAssetCollectionIdentity::SerializedFile { file_index } => file_index,
            ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
                return Err(invariant_error(
                    "GameObject overlay source has processed-collection scope".to_owned(),
                ));
            }
        }
        || object.object_index != source.object_index
        || object.path_id != source.path_id
    {
        return Err(invariant_error(
            "GameObject overlay source identity differs from its workspace object".to_owned(),
        ));
    }

    let mut generated_collections = context
        .processed_assets
        .collections()
        .iter()
        .filter(|collection| context.collection_scope.matches(&collection.kind));
    let scene_generated_collection = generated_collections.next().ok_or_else(|| {
        invariant_error(format!(
            "processed plan has no {}",
            context.collection_scope.description()
        ))
    })?;
    if generated_collections.next().is_some() {
        return Err(invariant_error(format!(
            "processed plan has multiple collections for {}",
            context.collection_scope.description()
        )));
    }
    let generated_collection_index = scene_generated_collection.collection_index;
    let mut matching_overlays = context
        .processed_assets
        .component_list_overlays()
        .iter()
        .filter(|overlay| {
            overlay.game_object.file_index == object.file_index
                && overlay.game_object.object_index == object.object_index
                && overlay.game_object.path_id == source.path_id
                && overlay.game_object.class_id == Some(1)
                && overlay.generated_component.component_asset.collection_index
                    == generated_collection_index
        });
    let Some(overlay) = matching_overlays.next() else {
        return Ok(None);
    };
    if matching_overlays.next().is_some() {
        return Err(invariant_error(
            "source GameObject has duplicate generated-component overlays for the selected collection"
                .to_owned(),
        ));
    }
    if overlay.generated_component.game_object != overlay.game_object
        || overlay.generated_component.component_class_id != 4
        || overlay.generated_component.component_asset.class_id != 4
    {
        return Err(invariant_error(
            "generated component overlay identity or class is not a Transform".to_owned(),
        ));
    }

    let generated_collection = context
        .processed_assets
        .collections()
        .get(generated_collection_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "generated collection {generated_collection_index} is outside the processed plan"
            ))
        })?;
    let mut matching_generated_assets = generated_collection
        .assets
        .iter()
        .enumerate()
        .filter(|(_, asset)| asset.identity == overlay.generated_component.component_asset);
    let (generated_object_index, generated_asset) =
        matching_generated_assets.next().ok_or_else(|| {
            invariant_error("overlay Transform is absent from its generated collection".to_owned())
        })?;
    if matching_generated_assets.next().is_some()
        || !matches!(
            generated_asset.kind,
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform { game_object, .. }
                if game_object == overlay.game_object
        )
    {
        return Err(invariant_error(
            "overlay target is not the uniquely planned Transform for this GameObject".to_owned(),
        ));
    }
    let target_asset = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::ProcessedCollection {
            collection_index: generated_collection_index,
        },
        object_index: generated_object_index,
        path_id: generated_asset.identity.path_id,
        class_id: generated_asset.identity.class_id,
    };
    let mut matching_constructor_pointers = context
        .processed_assets
        .constructor_pointers()
        .iter()
        .filter(|pointer| {
            pointer.source_asset == generated_asset.identity
                && pointer.field == WorkspaceGeneratedPointerField::TransformGameObject
        });
    let constructor_pointer = matching_constructor_pointers.next().ok_or_else(|| {
        invariant_error("overlay Transform has no constructor GameObject PPtr".to_owned())
    })?;
    if matching_constructor_pointers.next().is_some()
        || constructor_pointer.target != overlay.game_object
        || constructor_pointer.path_id != overlay.game_object.path_id
    {
        return Err(invariant_error(
            "overlay Transform constructor PPtr does not target its GameObject".to_owned(),
        ));
    }
    let mut matching_dependencies = generated_collection
        .dependencies
        .iter()
        .filter(|dependency| dependency.file_id == constructor_pointer.file_id);
    let dependency = matching_dependencies.next().ok_or_else(|| {
        invariant_error("overlay Transform source dependency is absent".to_owned())
    })?;
    if matching_dependencies.next().is_some()
        || dependency.target
            != (WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                file_index: overlay.game_object.file_index,
            })
    {
        return Err(invariant_error(
            "overlay Transform dependency does not uniquely target its GameObject file".to_owned(),
        ));
    }

    let mut matching_dependency_appends = context
        .processed_assets
        .source_dependency_appends()
        .iter()
        .filter(|append| {
            append.source_file_index == overlay.game_object.file_index
                && append.pointer_field
                    == WorkspaceSourceDependencyPointerField::GameObjectComponent
                && append.target_collection
                    == (WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                        collection_index: generated_collection_index,
                    })
        });
    let dependency_append = matching_dependency_appends.next().ok_or_else(|| {
        invariant_error("source collection has no generated-Transform dependency append".to_owned())
    })?;
    if matching_dependency_appends.next().is_some() {
        return Err(invariant_error(
            "source collection has duplicate generated-Transform dependency appends".to_owned(),
        ));
    }
    let dependency_file_id = dependency_append.file_id.ok_or_else(|| {
        invariant_error("generated-Transform dependency FileID is deferred".to_owned())
    })?;
    if dependency_file_id <= 0 {
        return Err(invariant_error(format!(
            "generated-Transform dependency FileID {dependency_file_id} is not positive"
        )));
    }

    let file = workspace.files().get(object.file_index).ok_or(
        UnityYamlWorkspaceExportError::MissingWorkspaceFile {
            file_index: object.file_index,
        },
    )?;
    let type_index =
        object
            .resolved_type_index
            .ok_or(UnityYamlWorkspaceExportError::MissingResolvedType {
                file_index: object.file_index,
                object_index: object.object_index,
            })?;
    let serialized_type = file.metadata.types.get(type_index).ok_or(
        UnityYamlWorkspaceExportError::InvalidResolvedType {
            file_index: object.file_index,
            object_index: object.object_index,
            type_index,
            type_count: file.metadata.types.len(),
        },
    )?;
    if serialized_type.raw_type_id != 1 {
        return Err(invariant_error(format!(
            "GameObject object resolves to class ID {}, expected 1",
            serialized_type.raw_type_id
        )));
    }
    let type_tree = object
        .recovered_type_tree
        .as_deref()
        .or(serialized_type.type_tree.as_ref())
        .ok_or(UnityYamlWorkspaceExportError::MissingTypeTree {
            file_index: object.file_index,
            object_index: object.object_index,
            type_index,
        })?;
    let element_node_index =
        game_object_component_element_node_index(type_tree).map_err(invariant_error)?;
    let empty_element_template = file
        .metadata
        .default_value_for_type_tree_node(
            object.object_index,
            type_index,
            element_node_index,
            context.default_value_limits,
        )
        .map_err(|error| invariant_error(error.to_string()))?;
    let field_path = generated_component_pointer_field_path(
        &empty_element_template,
        overlay.generated_component.appended_index,
    )
    .map_err(invariant_error)?;
    let source_payload =
        object
            .payload
            .as_ref()
            .map_err(|error| UnityYamlWorkspaceExportError::ObjectDecode {
                file_index: object.file_index,
                object_index: object.object_index,
                message: error.to_string(),
            })?;
    let edited_payload = materialize_planned_game_object_component_append(
        context.processed_assets,
        source_payload,
        overlay,
        Some(&empty_element_template),
        context.encode_limits.max_array_elements,
    )
    .map_err(|error| invariant_error(error.to_string()))?;
    Ok(Some(WorkspaceGeneratedComponentYamlOverlay {
        payload: edited_payload,
        pointer: WorkspaceGeneratedComponentYamlPointer {
            source,
            field_path,
            file_id: dependency_file_id,
            path_id: generated_asset.identity.path_id,
            target: target_asset,
        },
    }))
}

fn game_object_component_element_node_index(
    type_tree: &crate::serialized_file::metadata::TypeTree,
) -> Result<usize, String> {
    let mut component_nodes = type_tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.name.as_deref() == Some("m_Component"));
    let (component_node_index, component_node) = component_nodes
        .next()
        .ok_or_else(|| "GameObject TypeTree has no m_Component field".to_owned())?;
    if component_nodes.next().is_some() {
        return Err("GameObject TypeTree has duplicate m_Component fields".to_owned());
    }
    if component_node.type_name.as_deref() != Some("vector") {
        return Err(format!(
            "GameObject m_Component has TypeTree type {:?}, expected vector",
            component_node.type_name
        ));
    }
    let array_node_index = type_tree_unique_direct_child(type_tree, component_node_index, "Array")?;
    let array_node = &type_tree.nodes[array_node_index];
    if array_node.type_name.as_deref() != Some("Array") {
        return Err("GameObject m_Component Array node has an unexpected type".to_owned());
    }
    let size_node_index = type_tree_unique_direct_child(type_tree, array_node_index, "size")?;
    let element_node_index = type_tree_unique_direct_child(type_tree, array_node_index, "data")?;
    if size_node_index >= element_node_index {
        return Err("GameObject m_Component TypeTree places data before size".to_owned());
    }
    Ok(element_node_index)
}

fn type_tree_unique_direct_child(
    type_tree: &crate::serialized_file::metadata::TypeTree,
    parent_index: usize,
    child_name: &str,
) -> Result<usize, String> {
    let parent = type_tree
        .nodes
        .get(parent_index)
        .ok_or_else(|| format!("TypeTree parent node {parent_index} is unavailable"))?;
    let child_level = parent
        .level
        .checked_add(1)
        .ok_or_else(|| format!("TypeTree node {parent_index} cannot have children"))?;
    let mut found = None;
    for (node_index, node) in type_tree.nodes.iter().enumerate().skip(parent_index + 1) {
        if node.level <= parent.level {
            break;
        }
        if node.level == child_level
            && node.name.as_deref() == Some(child_name)
            && found.replace(node_index).is_some()
        {
            return Err(format!(
                "TypeTree node {parent_index} has duplicate direct children named {child_name:?}"
            ));
        }
    }
    found.ok_or_else(|| {
        format!("TypeTree node {parent_index} has no direct child named {child_name:?}")
    })
}

fn generated_component_pointer_field_path(
    template: &SerializedValue,
    appended_index: usize,
) -> Result<String, String> {
    let suffix = if template.pptr().is_some() {
        String::new()
    } else if template
        .struct_fields()
        .and_then(|fields| {
            (fields.len() == 1 && fields[0].name == "component").then_some(&fields[0].value)
        })
        .and_then(SerializedValue::pptr)
        .is_some()
    {
        ".component".to_owned()
    } else {
        return Err(
            "GameObject m_Component element TypeTree is neither a PPtr nor a ComponentPair"
                .to_owned(),
        );
    };
    Ok(format!("m_Component[{appended_index}]{suffix}"))
}

/// `StrippedAssetExtensions.IsStripped` over caller facts: the asset's main asset is a
/// hierarchy object tracking a stripped set, and the asset is in that set
/// (`AssetRipper/Source/AssetRipper.Export.UnityProjects/StrippedAssetExtensions.cs`
/// lines 10-18). `YamlWalker.ExportYamlDocument` strips the walked fields to the
/// per-class allowlist and marks the document `stripped` exactly on this decision
/// (`AssetRipper/Source/AssetRipper.Export.UnityProjects/YamlWalker.cs` lines 80-85).
///
/// The workspace executor passes `false` today because neither fact is wired through the
/// workspace flow; this kernel records the decision shape so a caller supplying
/// hierarchy facts can replace the constant without re-deriving it.
pub const fn is_stripped_document(
    main_asset_is_hierarchy_object: bool,
    asset_in_stripped_set: bool,
) -> bool {
    main_asset_is_hierarchy_object && asset_in_stripped_set
}

/// Convert one workspace-decoded SerializedFile object into the project YAML form.
///
/// The object identity, its TypeTree and version, reference scan, and project pointer map are all
/// checked against the same workspace indices. The reference graph must have completed scanning
/// the source object; undecoded references and trailing object bytes remain explicit errors.
#[allow(clippy::too_many_arguments)]
pub fn generated_transform_payload_to_unity_yaml_document(
    payload: &WorkspaceGeneratedTransformPayload,
    metadata: &SerializedFileMetadata,
    source: ExportAssetIdentity,
    file_id: i64,
    constructor_pointer: WorkspaceGeneratedConstructorPointer,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedTransformInvariant { source, message }
    };
    if payload.asset.class_id != 4
        || source.class_id != 4
        || source.collection
            != (ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: payload.asset.collection_index,
            })
        || source.path_id != payload.asset.path_id
        || constructor_pointer.source_asset != payload.asset
        || constructor_pointer.field != WorkspaceGeneratedPointerField::TransformGameObject
        || constructor_pointer.file_id <= 0
        || constructor_pointer.path_id != constructor_pointer.target.path_id
        || constructor_pointer.target.class_id != Some(1)
    {
        return Err(invariant_error(
            "export identity or constructor PPtr does not match the planned Transform".to_owned(),
        ));
    }

    let serialized_type = metadata.types.get(payload.type_index).ok_or_else(|| {
        invariant_error(format!(
            "Transform type index {} is outside the {}-entry type table",
            payload.type_index,
            metadata.types.len()
        ))
    })?;
    if serialized_type.raw_type_id != 4 {
        return Err(invariant_error(format!(
            "serialized type {} has class ID {}, expected Transform class ID 4",
            payload.type_index, serialized_type.raw_type_id
        )));
    }
    let type_tree = serialized_type.type_tree.as_ref().ok_or_else(|| {
        invariant_error(format!(
            "Transform type {} has no TypeTree",
            payload.type_index
        ))
    })?;
    let type_root = type_tree.nodes.first().ok_or_else(|| {
        invariant_error(format!(
            "Transform type {} has an empty TypeTree",
            payload.type_index
        ))
    })?;
    let class_name = type_root.type_name.as_deref().ok_or_else(|| {
        invariant_error("Transform TypeTree root has no resolved class name".to_owned())
    })?;
    if class_name != "Transform" {
        return Err(invariant_error(format!(
            "class-ID-4 TypeTree root is named {class_name:?}, expected \"Transform\""
        )));
    }
    let unity_version_text = metadata.prefix.unity_version_text.as_deref();
    let unity_version = unity_version_text
        .and_then(SceneUnityVersion::parse)
        .ok_or_else(|| {
            invariant_error(format!(
                "Transform type has no parseable Unity version: {unity_version_text:?}"
            ))
        })?;

    let game_object_pointer = payload
        .root
        .unique_field_in_base_chain("m_GameObject")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| invariant_error("Transform has no m_GameObject field".to_owned()))?;
    if game_object_pointer.pptr()
        != Some((constructor_pointer.file_id, constructor_pointer.path_id))
    {
        return Err(invariant_error(
            "Transform m_GameObject value differs from the planned constructor PPtr".to_owned(),
        ));
    }

    let encoded = metadata
        .encode_object_payload_for_type(
            metadata.objects.len(),
            payload.type_index,
            &payload.root,
            &[],
            ObjectEncodeLimits::default(),
        )
        .map_err(|error| invariant_error(error.to_string()))?;
    if encoded != payload.bytes {
        return Err(invariant_error(
            "materialized Transform bytes do not match its typed value".to_owned(),
        ));
    }

    let hints = UnityYamlObjectConversionHints::from_type_tree(type_tree, unity_version, limits)
        .map_err(UnityYamlWorkspaceExportError::Conversion)?;
    let target_asset = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: constructor_pointer.target.file_index,
        },
        object_index: constructor_pointer.target.object_index,
        path_id: constructor_pointer.target.path_id,
        class_id: 1,
    };
    let mut found_game_object_pointer = false;
    let mut pointer_error = None;
    let document = serialized_object_value_to_unity_yaml_document(
        source,
        class_name,
        type_root.version,
        file_id,
        false,
        &payload.root,
        &hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        unity_version,
        missing_reference_guid,
        limits,
        |request: UnityYamlPPtrRequest<'_>| {
            if request.source != source {
                pointer_error = Some(invariant_error(
                    "YAML walker changed the generated Transform identity".to_owned(),
                ));
                return ProjectPointerResolution::Null;
            }
            if request.file_id == 0 && request.path_id == 0 {
                return match pointer_map
                    .resolve(ProjectPointerRequest::Null, current_collection_index)
                {
                    Ok(resolution) => resolution,
                    Err(error) => {
                        pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                        ProjectPointerResolution::Null
                    }
                };
            }
            let field_name = request.field_path.rsplit('.').next().unwrap_or_default();
            if field_name != "m_GameObject"
                || request.file_id != constructor_pointer.file_id
                || request.path_id != constructor_pointer.path_id
                || found_game_object_pointer
            {
                pointer_error = Some(invariant_error(format!(
                    "unexpected non-null Transform PPtr at {:?}: FileID {} PathID {}",
                    request.field_path, request.file_id, request.path_id
                )));
                return ProjectPointerResolution::Null;
            }
            found_game_object_pointer = true;
            match pointer_map.resolve(
                ProjectPointerRequest::ResolvedAsset(target_asset),
                current_collection_index,
            ) {
                Ok(ProjectPointerResolution::Mapped(pointer)) => {
                    ProjectPointerResolution::Mapped(pointer)
                }
                Ok(_) => {
                    pointer_error = Some(invariant_error(
                        "planned GameObject has no exported project pointer".to_owned(),
                    ));
                    ProjectPointerResolution::Null
                }
                Err(error) => {
                    pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                    ProjectPointerResolution::Null
                }
            }
        },
    );
    if let Some(error) = pointer_error {
        return Err(error);
    }
    if !found_game_object_pointer {
        return Err(invariant_error(
            "YAML walker did not encounter the planned m_GameObject PPtr".to_owned(),
        ));
    }
    document.map_err(UnityYamlWorkspaceExportError::Conversion)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn workspace_generated_prefab_assignment_to_unity_yaml_document(
    export_context: &WorkspaceYamlExportContext<'_, '_, '_>,
    source: ExportAssetIdentity,
    file_id: i64,
    context: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let workspace = export_context.workspace;
    let pointer_map = export_context.pointer_map;
    let current_collection_index = export_context.current_collection_index;
    let missing_reference_guid = export_context.missing_reference_guid;
    let limits = export_context.limits;
    let invariant_error = |message: String| {
        UnityYamlWorkspaceExportError::GeneratedPrefabInvariant { source, message }
    };
    if !context.processed_assets.is_complete() {
        return Err(invariant_error(format!(
            "processed asset plan is incomplete: {}",
            context
                .processed_assets
                .incomplete_reason()
                .unwrap_or("no reason was recorded")
        )));
    }
    let Some(legacy) = context.legacy_prefab_export else {
        return Err(invariant_error(
            "generated class-1001 assignment has no legacy Prefab export context".to_owned(),
        ));
    };
    if legacy.export_ids.representation != PrefabAssetRepresentation::LegacyPrefabMarker
        || !legacy.export_ids.emit_prefab_asset
    {
        return Err(invariant_error(
            "generated class-1001 assignment is not the emitted legacy Prefab asset".to_owned(),
        ));
    }
    let ExportAssetCollectionIdentity::ProcessedCollection { collection_index } = source.collection
    else {
        return Err(invariant_error(
            "generated Prefab assignment has SerializedFile collection scope".to_owned(),
        ));
    };
    let collection = context
        .processed_assets
        .collections()
        .get(collection_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "processed collection {collection_index} is absent from the asset plan"
            ))
        })?;
    if collection.collection_index != collection_index
        || collection.kind != WorkspaceProcessedCollectionKind::GeneratedPrefabs
    {
        return Err(invariant_error(format!(
            "processed collection {collection_index} is not the GeneratedPrefabs collection"
        )));
    }
    let asset = collection.assets.get(source.object_index).ok_or_else(|| {
        invariant_error(format!(
            "processed collection {collection_index} has no planned asset at object index {}",
            source.object_index
        ))
    })?;
    if asset.identity.collection_index != collection_index
        || asset.identity.path_id != source.path_id
        || asset.identity.class_id != source.class_id
        || source.class_id != 1001
        || source.path_id == 0
    {
        return Err(invariant_error(
            "generated Prefab identity disagrees with the YAML assignment".to_owned(),
        ));
    }
    let WorkspaceProcessedPrefabAssetKind::GeneratedPrefabInstance { root_game_object } =
        &asset.kind
    else {
        return Err(invariant_error(format!(
            "processed asset {}:{} is not a generated Prefab instance",
            collection_index, source.object_index
        )));
    };
    let main_assignments = legacy
        .export_ids
        .registered_assets
        .iter()
        .filter(|assignment| assignment.is_main_asset);
    let main = main_assignments
        .clone()
        .next()
        .ok_or_else(|| invariant_error("legacy Prefab plan has no main asset".to_owned()))?;
    if main_assignments.count() != 1
        || main.asset != source
        || main.file_id != file_id
        || !legacy.export_ids.exportable_assets.contains(&source)
    {
        return Err(invariant_error(
            "legacy Prefab main assignment differs from the generated YAML identity".to_owned(),
        ));
    }

    let mut constructor_pointers = context
        .processed_assets
        .constructor_pointers()
        .iter()
        .filter(|pointer| {
            pointer.source_asset == asset.identity
                && pointer.field == WorkspaceGeneratedPointerField::PrefabInstanceRootGameObject
        });
    let constructor_pointer = constructor_pointers.next().copied().ok_or_else(|| {
        invariant_error("generated Prefab has no constructor root GameObject PPtr".to_owned())
    })?;
    if constructor_pointers.next().is_some()
        || constructor_pointer.target != *root_game_object
        || constructor_pointer.file_id <= 0
        || constructor_pointer.path_id != root_game_object.path_id
    {
        return Err(invariant_error(
            "generated Prefab root GameObject PPtr disagrees with the processed plan".to_owned(),
        ));
    }

    let target_file = workspace
        .files()
        .get(root_game_object.file_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "root GameObject file {} is absent from the workspace",
                root_game_object.file_index
            ))
        })?;
    let target_object = target_file
        .metadata
        .objects
        .get(root_game_object.object_index)
        .ok_or_else(|| {
            invariant_error(format!(
                "root GameObject object {} is absent from file {}",
                root_game_object.object_index, root_game_object.file_index
            ))
        })?;
    if target_object.path_id != root_game_object.path_id || target_object.raw_type_id != 1 {
        return Err(invariant_error(
            "root GameObject identity disagrees with its SerializedFile object record".to_owned(),
        ));
    }

    let Some(factory_version) = FactoryUnityVersion::parse(legacy.unity_version_text) else {
        return Err(invariant_error(format!(
            "legacy Prefab has no parseable Unity version: {:?}",
            legacy.unity_version_text
        )));
    };
    let Some(unity_version) = SceneUnityVersion::parse(legacy.unity_version_text) else {
        return Err(invariant_error(format!(
            "legacy Prefab has no parseable Unity version: {:?}",
            legacy.unity_version_text
        )));
    };
    if unity_version.major > 2018 || (unity_version.major == 2018 && unity_version.minor >= 3) {
        return Err(invariant_error(format!(
            "Unity {}.{} uses PrefabImporter assets instead of legacy Prefab documents",
            unity_version.major, unity_version.minor
        )));
    }
    let (_, editor_tree) = reference_class_layouts()
        .tpk_trees(1001, &factory_version)
        .map_err(|error| invariant_error(format!("could not load class-1001 TPK layout: {error}")))?
        .ok_or_else(|| {
            invariant_error(format!(
                "source TPK has no class-1001 layout for Unity {}",
                factory_version.text
            ))
        })?;
    let editor_tree = editor_tree.as_ref();
    let type_root = editor_tree
        .nodes
        .first()
        .ok_or_else(|| invariant_error("class-1001 editor TypeTree is empty".to_owned()))?;
    if type_root.type_name.as_deref() != Some("Prefab") {
        return Err(invariant_error(format!(
            "class-1001 editor TypeTree root is {:?}, expected Prefab",
            type_root.type_name
        )));
    }

    let type_index = target_file.metadata.types.len();
    let mut metadata = target_file.metadata.clone();
    metadata.types.push(SerializedType {
        raw_type_id: 1001,
        type_id_for_hash: 1001,
        is_stripped_type: false,
        script_type_index: -1,
        script_id: None,
        old_type_hash: None,
        type_tree: Some(editor_tree.clone()),
        type_dependencies: Vec::new(),
    });
    let mut root = metadata
        .default_object_value_for_type(
            metadata.objects.len(),
            type_index,
            context.default_value_limits,
        )
        .map_err(|error| {
            invariant_error(format!(
                "could not construct legacy Prefab payload: {error}"
            ))
        })?;

    let hide_flags = root
        .unique_field_in_base_chain_mut("m_ObjectHideFlags")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| {
            invariant_error("Prefab TypeTree has no m_ObjectHideFlags field".to_owned())
        })?;
    match hide_flags {
        SerializedValue::U32(value) => *value = 1,
        value => {
            return Err(invariant_error(format!(
                "m_ObjectHideFlags has value shape {value:?}, expected UInt32"
            )))
        }
    }
    let root_pointer = root
        .unique_field_in_base_chain_mut("m_RootGameObject")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| {
            invariant_error("Prefab TypeTree has no m_RootGameObject field".to_owned())
        })?;
    let SerializedValue::PPtr {
        file_id: root_file_id,
        path_id: root_path_id,
    } = root_pointer
    else {
        return Err(invariant_error(format!(
            "m_RootGameObject has value shape {root_pointer:?}, expected PPtr"
        )));
    };
    *root_file_id = constructor_pointer.file_id;
    *root_path_id = constructor_pointer.path_id;
    let is_prefab_parent = root
        .unique_field_in_base_chain_mut("m_IsPrefabParent")
        .map_err(|error| invariant_error(error.to_string()))?
        .ok_or_else(|| {
            invariant_error("Prefab TypeTree has no m_IsPrefabParent field".to_owned())
        })?;
    match is_prefab_parent {
        SerializedValue::Bool(value) => *value = true,
        value => {
            return Err(invariant_error(format!(
                "m_IsPrefabParent has value shape {value:?}, expected Bool"
            )))
        }
    }
    metadata
        .encode_object_payload_for_type(
            metadata.objects.len(),
            type_index,
            &root,
            &[],
            context.encode_limits,
        )
        .map_err(|error| {
            invariant_error(format!(
                "legacy Prefab payload does not match its TypeTree: {error}"
            ))
        })?;

    let mut hints =
        UnityYamlObjectConversionHints::from_type_tree(editor_tree, unity_version, limits)
            .map_err(UnityYamlWorkspaceExportError::Conversion)?;
    hints.field_name_aliases.clear();
    hints.preserve_type_tree_field_names = true;
    let target_asset = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: root_game_object.file_index,
        },
        object_index: root_game_object.object_index,
        path_id: root_game_object.path_id,
        class_id: 1,
    };
    let mut root_pointer_seen = false;
    let mut pointer_error = None;
    let document = serialized_object_value_to_unity_yaml_document(
        source,
        "Prefab",
        type_root.version,
        file_id,
        false,
        &root,
        &hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        unity_version,
        missing_reference_guid,
        limits,
        |request: UnityYamlPPtrRequest<'_>| {
            if request.source != source {
                pointer_error = Some(invariant_error(
                    "YAML walker changed the generated Prefab identity".to_owned(),
                ));
                return ProjectPointerResolution::Null;
            }
            if request.file_id == 0 && request.path_id == 0 {
                return match pointer_map
                    .resolve(ProjectPointerRequest::Null, current_collection_index)
                {
                    Ok(resolution) => resolution,
                    Err(error) => {
                        pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                        ProjectPointerResolution::Null
                    }
                };
            }
            if request.field_path.rsplit('.').next() != Some("m_RootGameObject")
                || request.file_id != constructor_pointer.file_id
                || request.path_id != constructor_pointer.path_id
                || root_pointer_seen
            {
                pointer_error = Some(invariant_error(format!(
                    "unexpected non-null Prefab PPtr at {:?}: FileID {} PathID {}",
                    request.field_path, request.file_id, request.path_id
                )));
                return ProjectPointerResolution::Null;
            }
            root_pointer_seen = true;
            match pointer_map.resolve(
                ProjectPointerRequest::ResolvedAsset(target_asset),
                current_collection_index,
            ) {
                Ok(ProjectPointerResolution::Mapped(pointer)) => {
                    ProjectPointerResolution::Mapped(pointer)
                }
                Ok(_) => {
                    pointer_error = Some(invariant_error(
                        "planned root GameObject has no exported project pointer".to_owned(),
                    ));
                    ProjectPointerResolution::Null
                }
                Err(error) => {
                    pointer_error = Some(UnityYamlWorkspaceExportError::PointerMap(error));
                    ProjectPointerResolution::Null
                }
            }
        },
    );
    if let Some(error) = pointer_error {
        return Err(error);
    }
    if !root_pointer_seen {
        return Err(invariant_error(
            "YAML walker did not encounter the planned m_RootGameObject PPtr".to_owned(),
        ));
    }
    document.map_err(UnityYamlWorkspaceExportError::Conversion)
}
