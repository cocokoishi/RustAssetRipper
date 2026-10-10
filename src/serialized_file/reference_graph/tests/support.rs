use super::ObjectReferenceGraph;
use crate::io::binary::Endian;
use crate::serialized_file::metadata::{FileIdentifier, ObjectInfo, SerializedFileMetadata};
use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
pub(super) fn metadata(path_ids: &[i64]) -> SerializedFileMetadata {
    SerializedFileMetadata {
        prefix: SerializedFileMetadataPrefix {
            metadata_offset: 0,
            metadata_size: 0,
            body_endian: Endian::Little,
            unity_version_text: None,
            target_platform: None,
            has_type_tree: false,
            consumed_bytes: 0,
        },
        types: Vec::new(),
        long_file_id: None,
        objects: path_ids.iter().map(|path_id| object(*path_id)).collect(),
        script_types: Vec::new(),
        externals: vec![external("shared.assets"), external("scene.assets")],
        reference_types: Vec::new(),
        user_information: None,
        trailing_metadata: Vec::new(),
        consumed_bytes: 0,
    }
}

pub(super) fn object(path_id: i64) -> ObjectInfo {
    ObjectInfo {
        path_id,
        byte_start: 0,
        byte_size: 0,
        serialized_type_index: None,
        raw_type_id: 0,
        class_id: None,
        script_type_index: None,
        is_destroyed: None,
        is_stripped: None,
        resolved_type_index: None,
    }
}

pub(super) fn external(path: &str) -> FileIdentifier {
    FileIdentifier {
        asset_path: Some(path.to_owned()),
        guid: None,
        file_type: None,
        path_name_origin: path.to_owned(),
    }
}

pub(super) fn empty_graph(path_ids: &[i64]) -> ObjectReferenceGraph {
    let mut path_id_index: Vec<_> = path_ids
        .iter()
        .enumerate()
        .map(|(index, path_id)| (*path_id, index))
        .collect();
    path_id_index.sort_unstable();
    ObjectReferenceGraph {
        object_count: path_ids.len(),
        attempted_objects: path_ids.len(),
        path_id_index,
        references: Vec::new(),
        managed_objects: Vec::new(),
        managed_references: Vec::new(),
        issues: Vec::new(),
    }
}
