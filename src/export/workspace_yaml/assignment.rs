use std::collections::HashSet;

use crate::serialized_file::reference_graph::MultiFileReferenceGraph;
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::super::pointer_map::ProjectExportPointerMap;
use super::super::prefab_collection::{
    PrefabAssetRepresentation, PrefabExportIdAssignment, PrefabExportIdPlan,
};
use super::super::scene_collection::{
    order_scene_yaml_assignments, ExportAssetCollectionIdentity, ExportAssetIdentity,
    SceneExportIdAssignment, SceneYamlAssetOrderLimits,
};
use super::super::unity_yaml::write_unity_yaml_documents;
use super::error::UnityYamlWorkspaceCollectionError;
use super::error::UnityYamlWorkspaceExportError;
use super::generated_assets::{
    workspace_generated_component_yaml_overlay,
    workspace_generated_prefab_assignment_to_unity_yaml_document,
    workspace_generated_prefab_internal_pointer,
    workspace_generated_transform_assignment_to_unity_yaml_document,
    WorkspaceGeneratedTransformYamlContext,
};
use super::model::{
    UnityYamlWorkspaceCollectionLimits, WorkspaceGeneratedLightingDataYamlPointer,
    WorkspaceStreamedYamlResourceContext, WorkspaceYamlExportContext,
};
use super::object_conversion::WorkspaceYamlObjectConversionRequest;
use super::sprite_atlas::workspace_sprite_atlas_yaml_overlay;

/// A typed Scene or Prefab export-ID assignment usable by the YAML collection executor.
pub trait UnityYamlCollectionAssignment {
    fn asset(&self) -> ExportAssetIdentity;
    fn file_id(&self) -> i64;
}

impl UnityYamlCollectionAssignment for SceneExportIdAssignment {
    fn asset(&self) -> ExportAssetIdentity {
        self.asset
    }

    fn file_id(&self) -> i64 {
        self.file_id
    }
}

impl UnityYamlCollectionAssignment for PrefabExportIdAssignment {
    fn asset(&self) -> ExportAssetIdentity {
        self.asset
    }

    fn file_id(&self) -> i64 {
        self.file_id
    }
}

struct WorkspaceYamlExecutionContext<'workspace, 'graph_data, 'provider, 'generated> {
    objects: &'workspace crate::serialized_file::object_workspace::ObjectWorkspace,
    export: WorkspaceYamlExportContext<'workspace, 'graph_data, 'provider>,
    limits: UnityYamlWorkspaceCollectionLimits,
    generated_transforms: Option<WorkspaceGeneratedTransformYamlContext<'generated>>,
    generated_lighting_data: Option<WorkspaceGeneratedLightingDataYamlPointer>,
}

impl<'workspace, 'graph_data, 'provider, 'generated>
    WorkspaceYamlExecutionContext<'workspace, 'graph_data, 'provider, 'generated>
{
    fn new(
        workspace: &'workspace SerializedFileWorkspace,
        objects: &'workspace crate::serialized_file::object_workspace::ObjectWorkspace,
        reference_graph: &'workspace MultiFileReferenceGraph<'graph_data>,
        pointer_map: &'workspace ProjectExportPointerMap,
        current_collection_index: usize,
        missing_reference_guid: Option<[u8; 16]>,
        limits: UnityYamlWorkspaceCollectionLimits,
    ) -> Self {
        Self {
            objects,
            export: WorkspaceYamlExportContext::new(
                workspace,
                reference_graph,
                pointer_map,
                current_collection_index,
                missing_reference_guid,
                limits.object_conversion,
            ),
            limits,
            generated_transforms: None,
            generated_lighting_data: None,
        }
    }

    fn with_streamed_resources(
        mut self,
        resources: &WorkspaceStreamedYamlResourceContext<'workspace, 'provider>,
    ) -> Self {
        self.export = self.export.with_streamed_resources(resources);
        self
    }

    fn with_generated_assets(
        mut self,
        transforms: Option<&WorkspaceGeneratedTransformYamlContext<'generated>>,
        lighting_data: Option<&WorkspaceGeneratedLightingDataYamlPointer>,
    ) -> Self {
        self.generated_transforms = transforms.copied();
        self.generated_lighting_data = lighting_data.copied();
        self
    }
}

/// Execute one ordered Scene or Prefab YAML collection using workspace-decoded source objects.
///
/// Assignment order is preserved exactly. Callers provide the baseline `ExportableAssets` order;
/// processed objects without a workspace payload fail explicitly until their generated YAML
/// walkers are connected.
#[allow(clippy::too_many_arguments)]
pub fn workspace_assignments_to_unity_yaml<A: UnityYamlCollectionAssignment>(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_yaml_order: &[A],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let context = WorkspaceYamlExecutionContext::new(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    );
    workspace_assignments_to_unity_yaml_with_context(assignments_in_yaml_order, &context)
}

/// Execute the YAML members of a planned Prefab collection in baseline hierarchy order.
///
/// `PrefabExportCollection.ExportableAssets` excludes hidden modern PrefabInstance markers but
/// includes the legacy Prefab object. This context-free helper fails explicitly for a generated
/// legacy main object; the generated-aware project route supplies its TPK payload schema and
/// `m_PrefabInternal` overlays. This function writes the `.prefab` YAML body only; importer
/// `.meta` output is a separate export path.
#[allow(clippy::too_many_arguments)]
pub fn workspace_prefab_hierarchy_to_unity_yaml(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    plan: &PrefabExportIdPlan,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let context = WorkspaceYamlExecutionContext::new(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    );
    workspace_prefab_hierarchy_to_unity_yaml_with_context(&context, plan)
}

/// Execute a Prefab hierarchy that includes PrefabProcessor-generated Transform objects.
#[allow(clippy::too_many_arguments)]
pub fn workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    plan: &PrefabExportIdPlan,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    generated_transforms: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let context = WorkspaceYamlExecutionContext::new(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
    .with_generated_assets(Some(generated_transforms), None);
    workspace_prefab_hierarchy_to_unity_yaml_with_context(&context, plan)
}

fn workspace_prefab_hierarchy_to_unity_yaml_with_context(
    context: &WorkspaceYamlExecutionContext<'_, '_, '_, '_>,
    plan: &PrefabExportIdPlan,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let limits = context.limits;
    let expected_emit_prefab_asset =
        plan.representation == PrefabAssetRepresentation::LegacyPrefabMarker;
    if plan.emit_prefab_asset != expected_emit_prefab_asset {
        return Err(
            UnityYamlWorkspaceCollectionError::PrefabPlanEmissionMismatch {
                emit_prefab_asset: plan.emit_prefab_asset,
                representation: plan.representation,
            },
        );
    }
    if plan.exportable_assets.len() > limits.max_documents {
        return Err(UnityYamlWorkspaceCollectionError::DocumentLimitExceeded {
            count: plan.exportable_assets.len(),
            limit: limits.max_documents,
        });
    }

    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(plan.exportable_assets.len())
        .map_err(|_| UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "Prefab YAML assignments",
            requested: plan.exportable_assets.len(),
        })?;
    for (assignment_index, asset) in plan.exportable_assets.iter().copied().enumerate() {
        let mut matching = plan
            .registered_assets
            .iter()
            .filter(|registered| registered.asset == asset);
        let Some(assignment) = matching.next().copied() else {
            return Err(
                UnityYamlWorkspaceCollectionError::PrefabAssignmentNotRegistered {
                    assignment_index,
                    asset,
                },
            );
        };
        if matching.next().is_some() {
            return Err(
                UnityYamlWorkspaceCollectionError::PrefabAssignmentRegisteredMoreThanOnce {
                    assignment_index,
                    asset,
                },
            );
        }
        assignments.push(assignment);
    }

    workspace_assignments_to_unity_yaml_with_context(&assignments, context)
}

/// Execute a YAML collection while resolving and embedding streamed Mesh/texture content.
#[allow(clippy::too_many_arguments)]
pub fn workspace_assignments_to_unity_yaml_with_streamed_resources<
    A: UnityYamlCollectionAssignment,
>(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_yaml_order: &[A],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let context = WorkspaceYamlExecutionContext::new(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
    .with_streamed_resources(streamed_resources);
    workspace_assignments_to_unity_yaml_with_context(assignments_in_yaml_order, &context)
}

fn workspace_assignments_to_unity_yaml_with_context<A: UnityYamlCollectionAssignment>(
    assignments_in_yaml_order: &[A],
    context: &WorkspaceYamlExecutionContext<'_, '_, '_, '_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let export_context = &context.export;
    let workspace = export_context.workspace;
    let objects = context.objects;
    let limits = context.limits;
    let generated_transforms = context.generated_transforms.as_ref();
    let generated_lighting_data = context.generated_lighting_data.as_ref();
    let document_count = assignments_in_yaml_order.len();
    if document_count > limits.max_documents {
        return Err(UnityYamlWorkspaceCollectionError::DocumentLimitExceeded {
            count: document_count,
            limit: limits.max_documents,
        });
    }

    let mut seen_assets = HashSet::new();
    seen_assets.try_reserve(document_count).map_err(|_| {
        UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "workspace YAML asset identity index",
            requested: document_count,
        }
    })?;
    let mut seen_file_ids = HashSet::new();
    seen_file_ids.try_reserve(document_count).map_err(|_| {
        UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "workspace YAML document fileID index",
            requested: document_count,
        }
    })?;
    let mut documents = Vec::new();
    documents.try_reserve_exact(document_count).map_err(|_| {
        UnityYamlWorkspaceCollectionError::AllocationFailed {
            resource: "workspace YAML documents",
            requested: document_count,
        }
    })?;
    for (assignment_index, assignment) in assignments_in_yaml_order.iter().enumerate() {
        let asset = assignment.asset();
        if !seen_assets.insert(asset.object_key()) {
            return Err(UnityYamlWorkspaceCollectionError::DuplicateAssetIdentity {
                assignment_index,
                asset,
            });
        }
        let file_id = assignment.file_id();
        if !seen_file_ids.insert(file_id) {
            return Err(UnityYamlWorkspaceCollectionError::DuplicateDocumentFileId {
                assignment_index,
                file_id,
            });
        }
        let document = match asset.collection {
            ExportAssetCollectionIdentity::SerializedFile { file_index } => {
                let object = objects.get(file_index, asset.object_index).ok_or(
                    UnityYamlWorkspaceCollectionError::WorkspaceObjectMissing {
                        assignment_index,
                        asset,
                    },
                )?;
                let generated_component_overlay = generated_transforms
                    .map(|context| {
                        workspace_generated_component_yaml_overlay(
                            workspace, object, asset, context,
                        )
                    })
                    .transpose()
                    .map_err(|source| UnityYamlWorkspaceCollectionError::ObjectExport {
                        assignment_index,
                        source: Box::new(source),
                    })?
                    .flatten();
                let generated_prefab_internal = if let Some(context) = generated_transforms {
                    workspace_generated_prefab_internal_pointer(asset, context).map_err(
                        |source| UnityYamlWorkspaceCollectionError::ObjectExport {
                            assignment_index,
                            source: Box::new(source),
                        },
                    )?
                } else {
                    None
                };
                let sprite_atlas_overlay = if asset.class_id == 213 {
                    workspace_sprite_atlas_yaml_overlay(objects, reference_graph, object, asset)
                        .map_err(|message| UnityYamlWorkspaceCollectionError::ObjectExport {
                            assignment_index,
                            source: Box::new(UnityYamlWorkspaceExportError::SpriteAtlasInvariant {
                                source: asset,
                                message,
                            }),
                        })?
                } else {
                    None
                };
                export_context
                    .convert(WorkspaceYamlObjectConversionRequest {
                        object,
                        source: asset,
                        file_id,
                        processed_animation_root: None,
                        animation_clip_editor_format: false,
                        generated_component_overlay: generated_component_overlay.as_ref(),
                        generated_prefab_internal: generated_prefab_internal.as_ref(),
                        generated_lighting_data,
                        sprite_atlas_overlay: sprite_atlas_overlay.as_ref(),
                    })
                    .map_err(|source| UnityYamlWorkspaceCollectionError::ObjectExport {
                        assignment_index,
                        source: Box::new(source),
                    })?
            }
            ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
                let Some(generated_context) = generated_transforms else {
                    return Err(
                        UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                            assignment_index,
                            asset,
                        },
                    );
                };
                let document = match asset.class_id {
                    4 => workspace_generated_transform_assignment_to_unity_yaml_document(
                        export_context,
                        asset,
                        file_id,
                        generated_context,
                    ),
                    1001 => workspace_generated_prefab_assignment_to_unity_yaml_document(
                        export_context,
                        asset,
                        file_id,
                        generated_context,
                    ),
                    _ => {
                        return Err(
                            UnityYamlWorkspaceCollectionError::ProcessedAssetRequiresGeneratedWalker {
                                assignment_index,
                                asset,
                            },
                        );
                    }
                }
                .map_err(|source| UnityYamlWorkspaceCollectionError::ObjectExport {
                    assignment_index,
                    source: Box::new(source),
                })?;
                Some(document)
            }
        };
        if let Some(document) = document {
            documents.push(document);
        }
    }

    write_unity_yaml_documents(&documents, limits.yaml_write)
        .map_err(UnityYamlWorkspaceCollectionError::YamlWrite)
}

/// Order hierarchy-listed Scene assignments using the baseline manager-first stable comparator,
/// then execute them through the workspace YAML collection writer.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    workspace_assignments_to_unity_yaml(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        &ordered_assignments,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
}

/// Order and export Scene assignments with bounded streamed Mesh/texture resolution.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_streamed_resources(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    workspace_assignments_to_unity_yaml_with_streamed_resources(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        &ordered_assignments,
        current_collection_index,
        missing_reference_guid,
        limits,
        streamed_resources,
    )
}

/// Order and export Scene YAML assignments with the planned generated Transform walker.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_generated_transforms(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    generated_transforms: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    let context = WorkspaceYamlExecutionContext::new(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
    .with_generated_assets(Some(generated_transforms), None);
    workspace_assignments_to_unity_yaml_with_context(&ordered_assignments, &context)
}

/// Scene YAML export with both streamed asset staging and generated Transform support.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_streamed_resources_and_generated_transforms(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
    generated_transforms: &WorkspaceGeneratedTransformYamlContext<'_>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    workspace_scene_assignments_to_unity_yaml_with_project_generated_assets(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        assignments_in_hierarchy_order,
        current_collection_index,
        missing_reference_guid,
        limits,
        streamed_resources,
        Some(generated_transforms),
        None,
    )
}

/// Scene YAML export with streamed assets and the generated collections owned by that Scene.
#[allow(clippy::too_many_arguments)]
pub fn workspace_scene_assignments_to_unity_yaml_with_project_generated_assets(
    workspace: &SerializedFileWorkspace,
    objects: &crate::serialized_file::object_workspace::ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlWorkspaceCollectionLimits,
    streamed_resources: &WorkspaceStreamedYamlResourceContext<'_, '_>,
    generated_transforms: Option<&WorkspaceGeneratedTransformYamlContext<'_>>,
    generated_lighting_data: Option<&WorkspaceGeneratedLightingDataYamlPointer>,
) -> Result<String, UnityYamlWorkspaceCollectionError> {
    let ordered_assignments = order_scene_yaml_assignments(
        assignments_in_hierarchy_order,
        SceneYamlAssetOrderLimits {
            max_assets: limits.max_documents,
        },
    )
    .map_err(UnityYamlWorkspaceCollectionError::SceneOrder)?;
    let context = WorkspaceYamlExecutionContext::new(
        workspace,
        objects,
        reference_graph,
        pointer_map,
        current_collection_index,
        missing_reference_guid,
        limits,
    )
    .with_streamed_resources(streamed_resources)
    .with_generated_assets(generated_transforms, generated_lighting_data);
    workspace_assignments_to_unity_yaml_with_context(&ordered_assignments, &context)
}
