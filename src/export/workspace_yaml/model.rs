use crate::export::scene_collection::ExportAssetIdentity;
use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::resource::{RootResourceFileCatalog, StreamedResourceContentLimits};
use crate::serialized_file::object::ParsedObjectPayload;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::super::pointer_map::ProjectExportPointerMap;
use super::super::streamed_assets::StreamedYamlAssetLimits;
use super::super::value_yaml::UnityYamlObjectConversionLimits;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityYamlWorkspaceCollectionLimits {
    pub max_documents: usize,
    pub object_conversion: super::super::value_yaml::UnityYamlObjectConversionLimits,
    pub yaml_write: super::super::unity_yaml::UnityYamlWriteLimits,
}

impl Default for UnityYamlWorkspaceCollectionLimits {
    fn default() -> Self {
        Self {
            max_documents: 1_000_000,
            object_conversion: super::super::value_yaml::UnityYamlObjectConversionLimits::default(),
            yaml_write: super::super::unity_yaml::UnityYamlWriteLimits::default(),
        }
    }
}

/// Resource lookup inputs for streamed Mesh and image-texture YAML export.
#[derive(Clone, Copy)]
pub struct WorkspaceStreamedYamlResourceContext<'a, 'provider> {
    pub filesystem: &'a VirtualFileSystem,
    pub root_catalog: &'a RootResourceFileCatalog,
    pub provider: &'a PlatformResourceProvider<'provider>,
    pub content_limits: StreamedResourceContentLimits,
    pub staging_limits: StreamedYamlAssetLimits,
}

pub(super) struct WorkspaceYamlExportContext<'workspace, 'graph_data, 'provider> {
    pub(super) workspace: &'workspace SerializedFileWorkspace,
    pub(super) reference_graph: &'workspace MultiFileReferenceGraph<'graph_data>,
    pub(super) pointer_map: &'workspace ProjectExportPointerMap,
    pub(super) current_collection_index: usize,
    pub(super) missing_reference_guid: Option<[u8; 16]>,
    pub(super) limits: UnityYamlObjectConversionLimits,
    pub(super) streamed_resources:
        Option<WorkspaceStreamedYamlResourceContext<'workspace, 'provider>>,
}

impl<'workspace, 'graph_data, 'provider>
    WorkspaceYamlExportContext<'workspace, 'graph_data, 'provider>
{
    pub(super) fn new(
        workspace: &'workspace SerializedFileWorkspace,
        reference_graph: &'workspace MultiFileReferenceGraph<'graph_data>,
        pointer_map: &'workspace ProjectExportPointerMap,
        current_collection_index: usize,
        missing_reference_guid: Option<[u8; 16]>,
        limits: UnityYamlObjectConversionLimits,
    ) -> Self {
        Self {
            workspace,
            reference_graph,
            pointer_map,
            current_collection_index,
            missing_reference_guid,
            limits,
            streamed_resources: None,
        }
    }

    pub(super) fn with_streamed_resources(
        mut self,
        streamed_resources: &WorkspaceStreamedYamlResourceContext<'workspace, 'provider>,
    ) -> Self {
        self.streamed_resources = Some(*streamed_resources);
        self
    }
}

pub(super) struct WorkspaceGeneratedComponentYamlPointer {
    pub(super) source: ExportAssetIdentity,
    pub(super) field_path: String,
    pub(super) file_id: i32,
    pub(super) path_id: i64,
    pub(super) target: ExportAssetIdentity,
}

pub(super) struct WorkspaceGeneratedComponentYamlOverlay {
    pub(super) payload: ParsedObjectPayload,
    pub(super) pointer: WorkspaceGeneratedComponentYamlPointer,
}

/// Scene-manager field overlay for a generated LightingDataAsset collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGeneratedLightingDataYamlPointer {
    pub source: ExportAssetIdentity,
    pub target: ExportAssetIdentity,
}

#[derive(Clone, Copy)]
pub(super) struct WorkspaceGeneratedPrefabInternalPointer {
    pub(super) source: ExportAssetIdentity,
    pub(super) target: ExportAssetIdentity,
}
