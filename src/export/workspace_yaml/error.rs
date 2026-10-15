use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::export::prefab_collection::PrefabAssetRepresentation;
use crate::export::scene_collection::{ExportAssetIdentity, SceneYamlAssetOrderError};
use crate::resource::StreamedResourceContentError;

use super::super::pointer_map::ExportPointerMapError;
use super::super::streamed_assets::StreamedYamlAssetError;
use super::super::unity_yaml::UnityYamlWriteError;
use super::super::value_yaml::UnityYamlObjectConversionError;

#[derive(Debug)]
pub enum UnityYamlWorkspaceExportError {
    ProcessedObjectIdentity {
        source: ExportAssetIdentity,
    },
    GeneratedTransformInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    GeneratedComponentOverlayInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    GeneratedPrefabInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    GeneratedLightingDataInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    SpriteAtlasInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    NavMeshBuildSettingsInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    AnimationClipEditorFormatInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    SceneManagerEditorFormatInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    SceneObjectEditorFormatInvariant {
        source: ExportAssetIdentity,
        message: String,
    },
    ImageTextureSkippedWithoutContent {
        source: ExportAssetIdentity,
    },
    WorkspaceIdentityMismatch {
        source: ExportAssetIdentity,
        object_file_index: usize,
        object_index: usize,
        path_id: i64,
    },
    MissingWorkspaceFile {
        file_index: usize,
    },
    MissingMetadataObject {
        file_index: usize,
        object_index: usize,
    },
    MetadataIdentityMismatch {
        file_index: usize,
        object_index: usize,
    },
    MissingClassId {
        file_index: usize,
        object_index: usize,
    },
    ClassIdMismatch {
        source_class_id: i32,
        object_class_id: i32,
    },
    ObjectDecode {
        file_index: usize,
        object_index: usize,
        message: String,
    },
    PayloadIdentityMismatch {
        file_index: usize,
        object_index: usize,
        payload_object_index: usize,
        payload_path_id: i64,
    },
    UnparsedTrailingPayload {
        file_index: usize,
        object_index: usize,
        byte_count: usize,
    },
    MissingResolvedType {
        file_index: usize,
        object_index: usize,
    },
    InvalidResolvedType {
        file_index: usize,
        object_index: usize,
        type_index: usize,
        type_count: usize,
    },
    MissingTypeTree {
        file_index: usize,
        object_index: usize,
        type_index: usize,
    },
    EmptyTypeTree {
        file_index: usize,
        object_index: usize,
        type_index: usize,
    },
    UnityVersionUnavailable {
        file_index: usize,
        unity_version_text: Option<String>,
    },
    ReferenceGraphIdentityMissing {
        file_index: usize,
        object_index: usize,
    },
    ReferenceGraphIdentityMismatch {
        file_index: usize,
        object_index: usize,
        expected_path_id: i64,
        graph_path_id: i64,
    },
    ReferenceGraphClassIdMismatch {
        file_index: usize,
        object_index: usize,
        expected_class_id: i32,
        graph_class_id: Option<i32>,
    },
    ReferenceScanIncomplete {
        file_index: usize,
        object_index: usize,
    },
    ReferenceEdgeMissing {
        file_index: usize,
        object_index: usize,
        field_path: String,
        file_id: i32,
        path_id: i64,
    },
    ReferenceEdgeAmbiguous {
        file_index: usize,
        object_index: usize,
        field_path: String,
        file_id: i32,
        path_id: i64,
    },
    NonNullPointerResolvedAsNull {
        file_index: usize,
        object_index: usize,
        field_path: String,
        path_id: i64,
    },
    ReferenceTargetIdentityMissing {
        file_index: usize,
        object_index: usize,
    },
    ReferenceTargetClassIdMissing {
        file_index: usize,
        object_index: usize,
    },
    StreamedAssetPreparation {
        file_index: usize,
        object_index: usize,
        source: Box<StreamedYamlAssetError>,
    },
    StreamedResourceResolution {
        file_index: usize,
        object_index: usize,
        path: String,
        source: Box<StreamedResourceContentError>,
    },
    StreamedResourceMissing {
        file_index: usize,
        object_index: usize,
        path: String,
        unavailable_data_paths: Vec<String>,
    },
    StreamedResourceUnexpectedUnset {
        file_index: usize,
        object_index: usize,
        path: String,
    },
    StreamedAssetInvariant {
        file_index: usize,
        object_index: usize,
        message: String,
    },
    PointerMap(ExportPointerMapError),
    Conversion(UnityYamlObjectConversionError),
}

impl Display for UnityYamlWorkspaceExportError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProcessedObjectIdentity { source } => {
                write!(f, "workspace YAML source is not a SerializedFile object: {source:?}")
            }
            Self::GeneratedTransformInvariant { source, message } => write!(
                f,
                "generated Transform YAML export failed for {source:?}: {message}"
            ),
            Self::GeneratedComponentOverlayInvariant { source, message } => write!(
                f,
                "generated GameObject component overlay export failed for {source:?}: {message}"
            ),
            Self::GeneratedPrefabInvariant { source, message } => write!(
                f,
                "generated legacy Prefab YAML export failed for {source:?}: {message}"
            ),
            Self::GeneratedLightingDataInvariant { source, message } => write!(
                f,
                "generated LightingDataAsset YAML export failed for {source:?}: {message}"
            ),
            Self::SpriteAtlasInvariant { source, message } => write!(
                f,
                "SpriteAtlas YAML reconstruction failed for {source:?}: {message}"
            ),
            Self::NavMeshBuildSettingsInvariant { source, message } => write!(
                f,
                "NavMesh build-settings reconstruction failed for {source:?}: {message}"
            ),
            Self::AnimationClipEditorFormatInvariant { source, message } => write!(
                f,
                "processed AnimationClip YAML export failed for {source:?}: {message}"
            ),
            Self::SceneManagerEditorFormatInvariant { source, message } => write!(
                f,
                "Scene manager editor-format materialization failed for {source:?}: {message}"
            ),
            Self::SceneObjectEditorFormatInvariant { source, message } => write!(
                f,
                "Scene object editor-format materialization failed for {source:?}: {message}"
            ),
            Self::ImageTextureSkippedWithoutContent { source } => write!(
                f,
                "baseline YAML export skips image texture {source:?} because it has no inline or streamed data"
            ),
            Self::WorkspaceIdentityMismatch {
                source,
                object_file_index,
                object_index,
                path_id,
            } => write!(
                f,
                "workspace object file {object_file_index} object {object_index} PathID {path_id} does not match export identity {source:?}"
            ),
            Self::MissingWorkspaceFile { file_index } => {
                write!(f, "SerializedFile workspace has no file at index {file_index}")
            }
            Self::MissingMetadataObject {
                file_index,
                object_index,
            } => write!(
                f,
                "SerializedFile {file_index} has no metadata object {object_index}"
            ),
            Self::MetadataIdentityMismatch {
                file_index,
                object_index,
            } => write!(
                f,
                "workspace object {file_index}:{object_index} does not match its SerializedFile metadata"
            ),
            Self::MissingClassId {
                file_index,
                object_index,
            } => write!(f, "SerializedFile {file_index} object {object_index} has no class ID"),
            Self::ClassIdMismatch {
                source_class_id,
                object_class_id,
            } => write!(
                f,
                "export identity class ID {source_class_id} differs from decoded class ID {object_class_id}"
            ),
            Self::ObjectDecode {
                file_index,
                object_index,
                message,
            } => write!(
                f,
                "could not decode SerializedFile {file_index} object {object_index}: {message}"
            ),
            Self::PayloadIdentityMismatch {
                file_index,
                object_index,
                payload_object_index,
                payload_path_id,
            } => write!(
                f,
                "decoded payload {file_index}:{payload_object_index} PathID {payload_path_id} does not match workspace object {object_index}"
            ),
            Self::UnparsedTrailingPayload {
                file_index,
                object_index,
                byte_count,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} has {byte_count} unparsed trailing payload bytes"
            ),
            Self::MissingResolvedType {
                file_index,
                object_index,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} has no resolved type index"
            ),
            Self::InvalidResolvedType {
                file_index,
                object_index,
                type_index,
                type_count,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} has type index {type_index}, but only {type_count} types exist"
            ),
            Self::MissingTypeTree {
                file_index,
                object_index,
                type_index,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} type {type_index} has no TypeTree"
            ),
            Self::EmptyTypeTree {
                file_index,
                object_index,
                type_index,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} type {type_index} has an empty TypeTree"
            ),
            Self::UnityVersionUnavailable {
                file_index,
                unity_version_text,
            } => write!(
                f,
                "SerializedFile {file_index} has no parseable Unity version: {unity_version_text:?}"
            ),
            Self::ReferenceGraphIdentityMissing {
                file_index,
                object_index,
            } => write!(
                f,
                "reference graph has no identity for SerializedFile {file_index} object {object_index}"
            ),
            Self::ReferenceGraphIdentityMismatch {
                file_index,
                object_index,
                expected_path_id,
                graph_path_id,
            } => write!(
                f,
                "reference graph identity {file_index}:{object_index} PathID {graph_path_id} differs from expected PathID {expected_path_id}"
            ),
            Self::ReferenceGraphClassIdMismatch {
                file_index,
                object_index,
                expected_class_id,
                graph_class_id,
            } => write!(
                f,
                "reference graph identity {file_index}:{object_index} class ID {graph_class_id:?} differs from expected class ID {expected_class_id}"
            ),
            Self::ReferenceScanIncomplete {
                file_index,
                object_index,
            } => write!(
                f,
                "reference scan for SerializedFile {file_index} object {object_index} is incomplete"
            ),
            Self::ReferenceEdgeMissing {
                file_index,
                object_index,
                field_path,
                file_id,
                path_id,
            } => write!(
                f,
                "reference graph has no edge for SerializedFile {file_index} object {object_index} field {field_path:?} FileID {file_id} PathID {path_id}"
            ),
            Self::ReferenceEdgeAmbiguous {
                file_index,
                object_index,
                field_path,
                file_id,
                path_id,
            } => write!(
                f,
                "reference graph has multiple edges for SerializedFile {file_index} object {object_index} field {field_path:?} FileID {file_id} PathID {path_id}"
            ),
            Self::NonNullPointerResolvedAsNull {
                file_index,
                object_index,
                field_path,
                path_id,
            } => write!(
                f,
                "non-null PPtr at SerializedFile {file_index} object {object_index} field {field_path:?} PathID {path_id} was classified as null"
            ),
            Self::ReferenceTargetIdentityMissing {
                file_index,
                object_index,
            } => write!(
                f,
                "reference graph has no identity for target {file_index}:{object_index}"
            ),
            Self::ReferenceTargetClassIdMissing {
                file_index,
                object_index,
            } => write!(
                f,
                "reference graph has no class ID for target {file_index}:{object_index}"
            ),
            Self::StreamedAssetPreparation {
                file_index,
                object_index,
                source,
            } => write!(
                f,
                "could not stage streamed asset {file_index}:{object_index} for YAML: {source}"
            ),
            Self::StreamedResourceResolution {
                file_index,
                object_index,
                path,
                source,
            } => write!(
                f,
                "could not resolve streamed resource {path:?} for asset {file_index}:{object_index}: {source}"
            ),
            Self::StreamedResourceMissing {
                file_index,
                object_index,
                path,
                unavailable_data_paths,
            } => write!(
                f,
                "streamed resource {path:?} is missing for asset {file_index}:{object_index}; unavailable DataPaths: {unavailable_data_paths:?}"
            ),
            Self::StreamedResourceUnexpectedUnset {
                file_index,
                object_index,
                path,
            } => write!(
                f,
                "streamed resource {path:?} became unset while exporting asset {file_index}:{object_index}"
            ),
            Self::StreamedAssetInvariant {
                file_index,
                object_index,
                message,
            } => write!(
                f,
                "streamed asset export invariant failed for {file_index}:{object_index}: {message}"
            ),
            Self::PointerMap(source) => Display::fmt(source, f),
            Self::Conversion(source) => Display::fmt(source, f),
        }
    }
}

impl Error for UnityYamlWorkspaceExportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PointerMap(source) => Some(source),
            Self::Conversion(source) => Some(source),
            Self::StreamedAssetPreparation { source, .. } => Some(source),
            Self::StreamedResourceResolution { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Convert a materialized PrefabProcessor Transform into a Scene YAML document.
///
/// The processed collection's constructor PPtr is resolved through its planned dependency
/// FileID and then the project export pointer map. Only the planned non-null `m_GameObject`
/// pointer is accepted; default null PPtrs remain null and any other non-null pointer fails.
#[allow(clippy::too_many_arguments)]
#[derive(Debug)]
pub enum UnityYamlWorkspaceCollectionError {
    DocumentLimitExceeded {
        count: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    SceneOrder(SceneYamlAssetOrderError),
    DuplicateAssetIdentity {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    DuplicateDocumentFileId {
        assignment_index: usize,
        file_id: i64,
    },
    ProcessedAssetRequiresGeneratedWalker {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    PrefabAssignmentNotRegistered {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    PrefabAssignmentRegisteredMoreThanOnce {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    PrefabPlanEmissionMismatch {
        emit_prefab_asset: bool,
        representation: PrefabAssetRepresentation,
    },
    WorkspaceObjectMissing {
        assignment_index: usize,
        asset: ExportAssetIdentity,
    },
    ObjectExport {
        assignment_index: usize,
        source: Box<UnityYamlWorkspaceExportError>,
    },
    YamlWrite(UnityYamlWriteError),
}

impl Display for UnityYamlWorkspaceCollectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DocumentLimitExceeded { count, limit } => write!(
                f,
                "workspace YAML collection has {count} documents, exceeding limit {limit}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(f, "could not allocate {requested} entries for {resource}"),
            Self::SceneOrder(source) => Display::fmt(source, f),
            Self::DuplicateAssetIdentity {
                assignment_index,
                asset,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} duplicates asset identity {asset:?}"
            ),
            Self::DuplicateDocumentFileId {
                assignment_index,
                file_id,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} duplicates document fileID {file_id}"
            ),
            Self::ProcessedAssetRequiresGeneratedWalker {
                assignment_index,
                asset,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} is a processed asset without a generated YAML walker: {asset:?}"
            ),
            Self::PrefabAssignmentNotRegistered {
                assignment_index,
                asset,
            } => write!(
                f,
                "Prefab YAML assignment {assignment_index} has no registered export ID: {asset:?}"
            ),
            Self::PrefabAssignmentRegisteredMoreThanOnce {
                assignment_index,
                asset,
            } => write!(
                f,
                "Prefab YAML assignment {assignment_index} has duplicate registered export IDs: {asset:?}"
            ),
            Self::PrefabPlanEmissionMismatch {
                emit_prefab_asset,
                representation,
            } => write!(
                f,
                "Prefab export plan says emit_prefab_asset={emit_prefab_asset} for representation {representation:?}"
            ),
            Self::WorkspaceObjectMissing {
                assignment_index,
                asset,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} has no decoded source object: {asset:?}"
            ),
            Self::ObjectExport {
                assignment_index,
                source,
            } => write!(
                f,
                "workspace YAML assignment {assignment_index} failed object conversion: {source}"
            ),
            Self::YamlWrite(source) => Display::fmt(source, f),
        }
    }
}

impl Error for UnityYamlWorkspaceCollectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ObjectExport { source, .. } => Some(source),
            Self::SceneOrder(source) => Some(source),
            Self::YamlWrite(source) => Some(source),
            _ => None,
        }
    }
}
