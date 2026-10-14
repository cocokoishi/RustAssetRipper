use super::*;

use crate::export::pointer_map::{
    ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
    ProjectExportPointerMap, ProjectExportPointerMapBuilder, UnityProjectAssetType,
};
use crate::export::prefab_collection::PrefabAssetRepresentation;
use crate::export::scene_collection::{
    ExportAssetCollectionIdentity, ExportAssetIdentity, SceneExportIdAssignment,
};
use crate::export::streamed_assets::StreamedYamlAssetError;
use crate::export::streamed_assets::StreamedYamlAssetLimits;
use crate::export::unity_yaml::{write_unity_yaml_documents, UnityYamlWriteLimits};
use crate::export::value_yaml::UnityYamlObjectConversionError;
use crate::export::value_yaml::UnityYamlObjectConversionLimits;
use crate::io::dependency::DependencyFilePath;
use crate::io::filesystem::VirtualFileSystem;
use crate::io::platform_structure::PlatformResourceProvider;
use crate::processing::asset_factory::FileTypeTreeTestLayouts;
use crate::processing::scene_naming::SceneUnityVersion;
use crate::resource::{ExplicitRootResourceInput, RootResourceCatalogLimits};
use crate::resource::{RootResourceFileCatalog, StreamedResourceContentLimits};
use crate::scheme::{SchemeReader, SchemeReaderLimits};
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::metadata::{
    SerializedFileMetadata, SerializedType, TypeTree, TypeTreeNode,
};
use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
use crate::serialized_file::reference_graph::{
    MultiFilePPtrResolution, MultiFileReferenceGraph, MultiFileReferenceGraphLimits,
};
use crate::serialized_file::workspace::{SerializedFileWorkspace, SerializedFileWorkspaceLimits};

const COLLECTION_INDEX: usize = 7;
const GAME_OBJECT_PATH_ID: i64 = 42;
const TRANSFORM_PATH_ID: i64 = 43;

mod assignment_order;
mod generated_assets;
mod object_and_reference_integrity;
mod prefab_header_backfill;
mod streamed_resources;
mod support;

use support::*;

pub(crate) use support::{
    load_workspace_bytes, load_workspace_bytes_with_source_factory,
    load_workspace_named_bytes_with_source_factory, make_existing_prefab_instance_file,
    make_game_object_without_transform_file,
};
