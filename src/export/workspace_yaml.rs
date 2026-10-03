//! Unity YAML export bridge for objects decoded from a SerializedFile workspace.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::object_workspace::WorkspaceObject;
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::pointer_map::{
    ExportPointerMapError, ProjectExportPointerMap, ProjectPointerRequest,
    ProjectPointerResolution, UnresolvedProjectPointer,
};
use super::scene_collection::{ExportAssetCollectionIdentity, ExportAssetIdentity};
use super::unity_yaml::UnityYamlDocument;
use super::value_yaml::{
    serialized_object_value_to_unity_yaml_document, UnityYamlObjectConversionError,
    UnityYamlObjectConversionHints, UnityYamlObjectConversionLimits, UnityYamlPPtrRequest,
};

/// Convert one workspace-decoded SerializedFile object into the project YAML form.
///
/// The object identity, its TypeTree and version, reference scan, and project pointer map are all
/// checked against the same workspace indices. The reference graph must have completed scanning
/// the source object; undecoded references and trailing object bytes remain explicit errors.
#[allow(clippy::too_many_arguments)]
pub fn workspace_object_to_unity_yaml_document(
    workspace: &SerializedFileWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    object: &WorkspaceObject,
    source: ExportAssetIdentity,
    file_id: i64,
    current_collection_index: usize,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
) -> Result<UnityYamlDocument, UnityYamlWorkspaceExportError> {
    let file_index = match source.collection {
        ExportAssetCollectionIdentity::SerializedFile { file_index } => file_index,
        ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
            return Err(UnityYamlWorkspaceExportError::ProcessedObjectIdentity { source });
        }
    };
    if object.file_index != file_index
        || object.object_index != source.object_index
        || object.path_id != source.path_id
    {
        return Err(UnityYamlWorkspaceExportError::WorkspaceIdentityMismatch {
            source,
            object_file_index: object.file_index,
            object_index: object.object_index,
            path_id: object.path_id,
        });
    }

    let file = workspace
        .files()
        .get(file_index)
        .ok_or(UnityYamlWorkspaceExportError::MissingWorkspaceFile { file_index })?;
    let metadata_object = file.metadata.objects.get(object.object_index).ok_or(
        UnityYamlWorkspaceExportError::MissingMetadataObject {
            file_index,
            object_index: object.object_index,
        },
    )?;
    if metadata_object.path_id != source.path_id
        || metadata_object.resolved_type_index != object.resolved_type_index
        || metadata_object.class_id.map(i32::from) != object.class_id.map(i32::from)
        || metadata_object.is_stripped != object.is_stripped
    {
        return Err(UnityYamlWorkspaceExportError::MetadataIdentityMismatch {
            file_index,
            object_index: object.object_index,
        });
    }
    let class_id =
        object
            .class_id
            .map(i32::from)
            .ok_or(UnityYamlWorkspaceExportError::MissingClassId {
                file_index,
                object_index: object.object_index,
            })?;
    if source.class_id != class_id {
        return Err(UnityYamlWorkspaceExportError::ClassIdMismatch {
            source_class_id: source.class_id,
            object_class_id: class_id,
        });
    }
    let stripped =
        object
            .is_stripped
            .ok_or(UnityYamlWorkspaceExportError::MissingStrippedState {
                file_index,
                object_index: object.object_index,
            })?;

    let payload =
        object
            .payload
            .as_ref()
            .map_err(|source| UnityYamlWorkspaceExportError::ObjectDecode {
                file_index,
                object_index: object.object_index,
                message: source.to_string(),
            })?;
    if payload.object_index != object.object_index || payload.path_id != source.path_id {
        return Err(UnityYamlWorkspaceExportError::PayloadIdentityMismatch {
            file_index,
            object_index: object.object_index,
            payload_object_index: payload.object_index,
            payload_path_id: payload.path_id,
        });
    }
    if !payload.trailing_bytes.is_empty() {
        return Err(UnityYamlWorkspaceExportError::UnparsedTrailingPayload {
            file_index,
            object_index: object.object_index,
            byte_count: payload.trailing_bytes.len(),
        });
    }

    let type_index =
        object
            .resolved_type_index
            .ok_or(UnityYamlWorkspaceExportError::MissingResolvedType {
                file_index,
                object_index: object.object_index,
            })?;
    let serialized_type = file.metadata.types.get(type_index).ok_or(
        UnityYamlWorkspaceExportError::InvalidResolvedType {
            file_index,
            object_index: object.object_index,
            type_index,
            type_count: file.metadata.types.len(),
        },
    )?;
    let type_tree = serialized_type.type_tree.as_ref().ok_or(
        UnityYamlWorkspaceExportError::MissingTypeTree {
            file_index,
            object_index: object.object_index,
            type_index,
        },
    )?;
    let type_root =
        type_tree
            .nodes
            .first()
            .ok_or(UnityYamlWorkspaceExportError::EmptyTypeTree {
                file_index,
                object_index: object.object_index,
                type_index,
            })?;
    let unity_version_text = file.metadata.prefix.unity_version_text.as_deref();
    let unity_version = unity_version_text
        .and_then(SceneUnityVersion::parse)
        .ok_or_else(|| UnityYamlWorkspaceExportError::UnityVersionUnavailable {
            file_index,
            unity_version_text: unity_version_text.map(str::to_owned),
        })?;
    let hints = UnityYamlObjectConversionHints::from_type_tree(type_tree, unity_version, limits)
        .map_err(UnityYamlWorkspaceExportError::Conversion)?;

    let graph_identity = reference_graph
        .object_identity(file_index, object.object_index)
        .ok_or(
            UnityYamlWorkspaceExportError::ReferenceGraphIdentityMissing {
                file_index,
                object_index: object.object_index,
            },
        )?;
    if graph_identity.path_id != source.path_id {
        return Err(
            UnityYamlWorkspaceExportError::ReferenceGraphIdentityMismatch {
                file_index,
                object_index: object.object_index,
                expected_path_id: source.path_id,
                graph_path_id: graph_identity.path_id,
            },
        );
    }
    if reference_graph.object_class_id(file_index, object.object_index) != Some(class_id) {
        return Err(
            UnityYamlWorkspaceExportError::ReferenceGraphClassIdMismatch {
                file_index,
                object_index: object.object_index,
                expected_class_id: class_id,
                graph_class_id: reference_graph.object_class_id(file_index, object.object_index),
            },
        );
    }
    if reference_graph.object_scan_is_complete(file_index, object.object_index) != Some(true) {
        return Err(UnityYamlWorkspaceExportError::ReferenceScanIncomplete {
            file_index,
            object_index: object.object_index,
        });
    }

    let mut pointer_error = None;
    let document = serialized_object_value_to_unity_yaml_document(
        source,
        &payload.type_name,
        type_root.version,
        file_id,
        stripped,
        &payload.root,
        &hints,
        unity_version,
        missing_reference_guid,
        limits,
        |request| {
            resolve_workspace_pointer(
                request,
                reference_graph,
                pointer_map,
                current_collection_index,
                &mut pointer_error,
            )
        },
    );
    if let Some(error) = pointer_error {
        return Err(error);
    }
    document.map_err(UnityYamlWorkspaceExportError::Conversion)
}

fn resolve_workspace_pointer(
    request: UnityYamlPPtrRequest<'_>,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    current_collection_index: usize,
    error: &mut Option<UnityYamlWorkspaceExportError>,
) -> ProjectPointerResolution {
    let ExportAssetCollectionIdentity::SerializedFile { file_index } = request.source.collection
    else {
        *error = Some(UnityYamlWorkspaceExportError::ProcessedObjectIdentity {
            source: request.source,
        });
        return ProjectPointerResolution::Null;
    };
    let mut matching_edges = reference_graph
        .references_from_object(file_index, request.source.object_index)
        .filter(|edge| {
            edge.field_path == request.field_path
                && edge.file_id == request.file_id
                && edge.path_id == request.path_id
        });
    let Some(edge) = matching_edges.next() else {
        *error = Some(UnityYamlWorkspaceExportError::ReferenceEdgeMissing {
            file_index,
            object_index: request.source.object_index,
            field_path: request.field_path.to_owned(),
            file_id: request.file_id,
            path_id: request.path_id,
        });
        return ProjectPointerResolution::Null;
    };
    if matching_edges.next().is_some() {
        *error = Some(UnityYamlWorkspaceExportError::ReferenceEdgeAmbiguous {
            file_index,
            object_index: request.source.object_index,
            field_path: request.field_path.to_owned(),
            file_id: request.file_id,
            path_id: request.path_id,
        });
        return ProjectPointerResolution::Null;
    }

    let pointer_request = match &edge.resolution {
        MultiFilePPtrResolution::Null => {
            *error = Some(
                UnityYamlWorkspaceExportError::NonNullPointerResolvedAsNull {
                    file_index,
                    object_index: request.source.object_index,
                    field_path: request.field_path.to_owned(),
                    path_id: request.path_id,
                },
            );
            return ProjectPointerResolution::Null;
        }
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => {
            let Some(identity) =
                reference_graph.object_identity(*target_file_index, *target_object_index)
            else {
                *error = Some(
                    UnityYamlWorkspaceExportError::ReferenceTargetIdentityMissing {
                        file_index: *target_file_index,
                        object_index: *target_object_index,
                    },
                );
                return ProjectPointerResolution::Null;
            };
            let Some(class_id) =
                reference_graph.object_class_id(*target_file_index, *target_object_index)
            else {
                *error = Some(
                    UnityYamlWorkspaceExportError::ReferenceTargetClassIdMissing {
                        file_index: *target_file_index,
                        object_index: *target_object_index,
                    },
                );
                return ProjectPointerResolution::Null;
            };
            ProjectPointerRequest::ResolvedAsset(ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::SerializedFile {
                    file_index: *target_file_index,
                },
                object_index: *target_object_index,
                path_id: identity.path_id,
                class_id,
            })
        }
        resolution => ProjectPointerRequest::UnresolvedInput(UnresolvedProjectPointer {
            source_file_index: file_index,
            source_object_index: request.source.object_index,
            source_path_id: request.source.path_id,
            field_path: request.field_path.to_owned(),
            serialized_file_id: request.file_id,
            serialized_path_id: request.path_id,
            expected_class_id: None,
            resolution: resolution.clone(),
        }),
    };
    match pointer_map.resolve(pointer_request, current_collection_index) {
        Ok(resolution) => resolution,
        Err(source) => {
            *error = Some(UnityYamlWorkspaceExportError::PointerMap(source));
            ProjectPointerResolution::Null
        }
    }
}

#[derive(Debug)]
pub enum UnityYamlWorkspaceExportError {
    ProcessedObjectIdentity {
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
    MissingStrippedState {
        file_index: usize,
        object_index: usize,
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
    PointerMap(ExportPointerMapError),
    Conversion(UnityYamlObjectConversionError),
}

impl Display for UnityYamlWorkspaceExportError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProcessedObjectIdentity { source } => {
                write!(f, "workspace YAML source is not a SerializedFile object: {source:?}")
            }
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
            Self::MissingStrippedState {
                file_index,
                object_index,
            } => write!(
                f,
                "SerializedFile {file_index} object {object_index} has no resolved stripped state"
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
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::export::pointer_map::{
        ExportCollectionPointerMetadata, ExportPointerMapLimits, ExportedAssetPointerEntry,
        UnityProjectAssetType,
    };
    use crate::export::scene_collection::ExportAssetIdentity;
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
    use crate::serialized_file::reference_graph::MultiFileReferenceGraphLimits;
    use crate::serialized_file::workspace::{
        SerializedFileWorkspace, SerializedFileWorkspaceLimits,
    };

    const COLLECTION_INDEX: usize = 7;
    const GAME_OBJECT_PATH_ID: i64 = 42;
    const TRANSFORM_PATH_ID: i64 = 43;

    fn asset(object_index: usize, path_id: i64, class_id: i32) -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 0 },
            object_index,
            path_id,
            class_id,
        }
    }

    fn pointer_map() -> ProjectExportPointerMap {
        let assets = [
            ExportedAssetPointerEntry {
                asset: asset(0, GAME_OBJECT_PATH_ID, 1),
                file_id: GAME_OBJECT_PATH_ID,
                collection_index: COLLECTION_INDEX,
            },
            ExportedAssetPointerEntry {
                asset: asset(1, TRANSFORM_PATH_ID, 4),
                file_id: TRANSFORM_PATH_ID,
                collection_index: COLLECTION_INDEX,
            },
        ];
        let collections = [ExportCollectionPointerMetadata {
            collection_index: COLLECTION_INDEX,
            guid: [0x71; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }];
        ProjectExportPointerMap::new(&assets, &collections, ExportPointerMapLimits::default())
            .unwrap()
    }

    fn load_workspaces(target_path_id: i64) -> (VirtualFileSystem, SerializedFileWorkspace) {
        let bytes = make_game_object_and_transform_file(target_path_id);
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        filesystem
            .write_all_bytes("/game/main.assets", &bytes)
            .unwrap();
        let input = [DependencyFilePath {
            file_name: "main.assets".to_owned(),
            virtual_path: "/game/main.assets".to_owned(),
        }];
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &input,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        (filesystem, workspace)
    }

    fn make_game_object_and_transform_file(component_path_id: i64) -> Vec<u8> {
        let game_object_tree = [
            (0, 6, "GameObject", "", -1),
            (1, 1, "vector", "m_Component", -1),
            (2, 1, "Array", "Array", -1),
            (3, 1, "int", "size", 4),
            (3, 1, "ComponentPair", "data", -1),
            (4, 1, "PPtr<Transform>", "component", 12),
            (5, 1, "SInt32", "m_FileID", 4),
            (5, 1, "SInt64", "m_PathID", 8),
        ];
        let transform_tree = [
            (0, 2, "Transform", "", -1),
            (1, 1, "PPtr<GameObject>", "m_GameObject", 12),
            (2, 1, "SInt32", "m_FileID", 4),
            (2, 1, "SInt64", "m_PathID", 8),
        ];
        let mut metadata = b"2019.4.0f1\0".to_vec();
        metadata.extend_from_slice(&0u32.to_le_bytes());
        metadata.extend_from_slice(&2i32.to_le_bytes());
        write_type_tree(&mut metadata, 1, &game_object_tree);
        write_type_tree(&mut metadata, 4, &transform_tree);

        metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&2i32.to_le_bytes());
        let mut game_object_payload = Vec::new();
        game_object_payload.extend_from_slice(&1i32.to_le_bytes()); // m_Component size
        game_object_payload.extend_from_slice(&0i32.to_le_bytes()); // component FileID
        game_object_payload.extend_from_slice(&component_path_id.to_le_bytes());
        let mut transform_payload = Vec::new();
        transform_payload.extend_from_slice(&0i32.to_le_bytes()); // m_GameObject FileID
        transform_payload.extend_from_slice(&GAME_OBJECT_PATH_ID.to_le_bytes());

        write_object_record(
            &mut metadata,
            GAME_OBJECT_PATH_ID,
            0,
            game_object_payload.len(),
            1,
            1,
        );
        write_object_record(
            &mut metadata,
            TRANSFORM_PATH_ID,
            game_object_payload.len(),
            transform_payload.len(),
            4,
            4,
        );
        metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
        metadata.push(0); // user information string

        let data_offset = 20 + metadata.len();
        let file_size = data_offset + game_object_payload.len() + transform_payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&game_object_payload);
        file.extend_from_slice(&transform_payload);
        file
    }

    fn write_type_tree(output: &mut Vec<u8>, class_id: i32, nodes: &[(u8, i32, &str, &str, i32)]) {
        let mut string_buffer = Vec::new();
        let mut offsets = Vec::new();
        for (_, _, type_name, name, _) in nodes {
            let type_offset = push_type_tree_string(&mut string_buffer, type_name);
            let name_offset = push_type_tree_string(&mut string_buffer, name);
            offsets.push((type_offset, name_offset));
        }
        output.extend_from_slice(&class_id.to_le_bytes());
        output.extend_from_slice(&(nodes.len() as i32).to_le_bytes());
        output.extend_from_slice(&(string_buffer.len() as i32).to_le_bytes());
        for ((level, version, _, _, byte_size), (type_offset, name_offset)) in
            nodes.iter().zip(offsets)
        {
            output.extend_from_slice(&(*version as u16).to_le_bytes());
            output.push(*level);
            output.push(0);
            output.extend_from_slice(&type_offset.to_le_bytes());
            output.extend_from_slice(&name_offset.to_le_bytes());
            output.extend_from_slice(&byte_size.to_le_bytes());
            output.extend_from_slice(&0i32.to_le_bytes());
            output.extend_from_slice(&0u32.to_le_bytes());
        }
        output.extend_from_slice(&string_buffer);
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn write_object_record(
        output: &mut Vec<u8>,
        path_id: i64,
        data_start: usize,
        data_size: usize,
        raw_type_id: i32,
        class_id: i16,
    ) {
        output.extend_from_slice(&i32::try_from(path_id).unwrap().to_le_bytes());
        output.extend_from_slice(&u32::try_from(data_start).unwrap().to_le_bytes());
        output.extend_from_slice(&i32::try_from(data_size).unwrap().to_le_bytes());
        output.extend_from_slice(&raw_type_id.to_le_bytes());
        output.extend_from_slice(&class_id.to_le_bytes());
        output.extend_from_slice(&0u16.to_le_bytes());
    }

    #[test]
    fn exports_workspace_gameobject_component_and_transform_back_reference() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        assert_eq!(
            reference_graph.references()[0].field_path,
            "m_Component[0].component"
        );
        assert_eq!(
            reference_graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 1,
            }
        );

        let pointer_map = pointer_map();
        let game_object = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            asset(0, GAME_OBJECT_PATH_ID, 1),
            GAME_OBJECT_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let transform = workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[1],
            asset(1, TRANSFORM_PATH_ID, 4),
            TRANSFORM_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[game_object, transform],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("--- !u!1 &42"));
        assert!(output.contains("serializedVersion: 6"));
        assert!(output.contains("component: {fileID: 43}"));
        assert!(output.contains("m_GameObject: {fileID: 42}"));
    }

    #[test]
    fn refuses_workspace_payload_bytes_or_reference_edges_that_do_not_match() {
        let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let pointer_map = pointer_map();

        let mut with_trailing_bytes = objects.objects()[0].clone();
        with_trailing_bytes
            .payload
            .as_mut()
            .unwrap()
            .trailing_bytes
            .push(0xaa);
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &with_trailing_bytes,
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::UnparsedTrailingPayload { byte_count: 1, .. })
        ));

        let mut with_wrong_pointer = objects.objects()[0].clone();
        let payload = with_wrong_pointer.payload.as_mut().unwrap();
        let crate::serialized_file::object::SerializedValue::Struct(fields) = &mut payload.root
        else {
            panic!("fixture root is a struct")
        };
        let crate::serialized_file::object::SerializedValue::Array(components) =
            &mut fields[0].value
        else {
            panic!("fixture component field is an array")
        };
        let crate::serialized_file::object::SerializedValue::Struct(pair_fields) =
            &mut components[0]
        else {
            panic!("fixture component value is a struct")
        };
        pair_fields[0].value = crate::serialized_file::object::SerializedValue::PPtr {
            file_id: 0,
            path_id: 999,
        };
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &with_wrong_pointer,
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::ReferenceEdgeMissing { path_id: 999, .. })
        ));
    }

    #[test]
    fn missing_local_component_reference_remains_an_export_error() {
        let (filesystem, workspace) = load_workspaces(999);
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let pointer_map = pointer_map();
        assert!(matches!(
            workspace_object_to_unity_yaml_document(
                &workspace,
                &reference_graph,
                &pointer_map,
                &objects.objects()[0],
                asset(0, GAME_OBJECT_PATH_ID, 1),
                GAME_OBJECT_PATH_ID,
                COLLECTION_INDEX,
                None,
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlWorkspaceExportError::Conversion(
                UnityYamlObjectConversionError::PointerConversion { .. }
            ))
        ));
    }
}
