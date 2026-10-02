//! Bounded loading of named SerializedFiles from the mounted VFS.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

use crate::io::dependency::DependencyFilePath;
use crate::io::filesystem::VirtualFileSystem;
use crate::io::special_names::fix_file_identifier;
use crate::scheme::{FileKind, ParsedFile, SchemeError, SchemeReader, SchemeReaderLimits};

use super::header::SerializedFileHeader;
use super::metadata::SerializedFileMetadata;
use super::reference_graph::{
    MultiFileReferenceGraph, MultiFileReferenceGraphBuildError, MultiFileReferenceGraphLimits,
    NamedObjectReferenceGraph, ObjectReferenceGraph, ReferenceGraphBuildError,
    ReferenceGraphLimits,
};

/// Resource bounds for loading and scanning a named set of VFS SerializedFiles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SerializedFileWorkspaceLimits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_input_bytes: u64,
    pub max_file_name_bytes: usize,
    pub max_total_file_name_bytes: usize,
    pub max_total_objects: usize,
    pub max_total_reference_edges: usize,
    pub scheme: SchemeReaderLimits,
    pub reference_graph: ReferenceGraphLimits,
}

impl Default for SerializedFileWorkspaceLimits {
    fn default() -> Self {
        Self {
            max_files: 1_000_000,
            max_file_bytes: 2 * 1024 * 1024 * 1024,
            max_total_input_bytes: 16 * 1024 * 1024 * 1024,
            max_file_name_bytes: 1024 * 1024,
            max_total_file_name_bytes: 64 * 1024 * 1024,
            max_total_objects: 20_000_000,
            max_total_reference_edges: 5_000_000,
            scheme: SchemeReaderLimits::default(),
            reference_graph: ReferenceGraphLimits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SerializedFileWorkspaceReport {
    pub input_files: usize,
    pub completed_files: usize,
    pub total_input_bytes: u64,
    pub completed_input_bytes: u64,
    pub total_objects: usize,
    pub total_reference_edges: usize,
}

/// One fully parsed SerializedFile with its header, owned metadata, and per-file PPtr graph.
///
/// Raw input bytes remain in the source VFS and are identified by `virtual_path`; this workspace
/// retains indexes rather than duplicating potentially large object payload buffers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSerializedFile {
    pub file_name: String,
    pub virtual_path: String,
    pub header: SerializedFileHeader,
    pub metadata: SerializedFileMetadata,
    pub reference_graph: ObjectReferenceGraph,
}

/// A loaded set of SerializedFiles whose cross-file references can be joined on demand.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SerializedFileWorkspace {
    files: Vec<LoadedSerializedFile>,
    report: SerializedFileWorkspaceReport,
}

impl SerializedFileWorkspace {
    /// Parse caller-identified SerializedFiles from the VFS and build their per-file reference
    /// graphs. All file sizes are preflighted before any file contents are read.
    pub fn load_from_vfs(
        filesystem: &VirtualFileSystem,
        inputs: &[DependencyFilePath],
        limits: SerializedFileWorkspaceLimits,
    ) -> Result<Self, Box<SerializedFileWorkspaceError>> {
        let mut report = SerializedFileWorkspaceReport {
            input_files: inputs.len(),
            ..SerializedFileWorkspaceReport::default()
        };
        if inputs.len() > limits.max_files {
            return Err(workspace_error(
                report,
                None,
                SerializedFileWorkspaceErrorKind::FileCountLimitExceeded {
                    count: inputs.len(),
                    limit: limits.max_files,
                },
            ));
        }

        let mut seen_paths = BTreeSet::new();
        let mut total_name_bytes = 0usize;
        for (index, input) in inputs.iter().enumerate() {
            if input.file_name.len() > limits.max_file_name_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileNameTooLong {
                        input_index: index,
                        byte_count: input.file_name.len(),
                        limit: limits.max_file_name_bytes,
                    },
                ));
            }
            if !seen_paths.insert(input.virtual_path.as_str()) {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::DuplicateVirtualPath,
                ));
            }
            let normalized_name = fix_file_identifier(&input.file_name);
            if normalized_name.len() > limits.max_file_name_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileNameTooLong {
                        input_index: index,
                        byte_count: normalized_name.len(),
                        limit: limits.max_file_name_bytes,
                    },
                ));
            }
            total_name_bytes = total_name_bytes
                .checked_add(normalized_name.len())
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::FileNameByteCountOverflow,
                    )
                })?;
            if total_name_bytes > limits.max_total_file_name_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalFileNameBytesLimitExceeded {
                        byte_count: total_name_bytes,
                        limit: limits.max_total_file_name_bytes,
                    },
                ));
            }

            let stream = filesystem
                .open_file(&input.virtual_path)
                .map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::FileSystem(source),
                    )
                })?;
            let file_bytes = stream.length().map_err(|source| {
                workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileSystem(source),
                )
            })?;
            let scheme_file_limit =
                u64::try_from(limits.scheme.max_input_bytes).unwrap_or(u64::MAX);
            let effective_file_limit = limits.max_file_bytes.min(scheme_file_limit);
            if file_bytes > effective_file_limit {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::FileSizeLimitExceeded {
                        file_bytes,
                        limit: effective_file_limit,
                    },
                ));
            }
            report.total_input_bytes = report
                .total_input_bytes
                .checked_add(file_bytes)
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::InputByteCountOverflow,
                    )
                })?;
            if report.total_input_bytes > limits.max_total_input_bytes {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalInputLimitExceeded {
                        requested_total: report.total_input_bytes,
                        limit: limits.max_total_input_bytes,
                    },
                ));
            }
        }

        let mut files = Vec::new();
        files.try_reserve_exact(inputs.len()).map_err(|_| {
            workspace_error(
                report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "loaded SerializedFiles",
                    requested: inputs.len(),
                },
            )
        })?;

        for input in inputs {
            let max_file_bytes = usize::try_from(limits.max_file_bytes).unwrap_or(usize::MAX);
            let bytes = filesystem
                .read_all_bytes(&input.virtual_path, max_file_bytes)
                .map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::FileSystem(source),
                    )
                })?;
            let dispatched =
                SchemeReader::read_with_limits(&bytes, limits.scheme).map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::Scheme(source),
                    )
                })?;
            if dispatched.kind() != FileKind::SerializedFile {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::NotSerializedFile {
                        detected_kind: dispatched.kind(),
                    },
                ));
            }
            let (header, metadata, reference_graph) = {
                let ParsedFile::SerializedFile(parsed) = dispatched.parse().map_err(|source| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::Scheme(source),
                    )
                })?
                else {
                    return Err(workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::NotSerializedFile {
                            detected_kind: dispatched.kind(),
                        },
                    ));
                };
                let graph =
                    ObjectReferenceGraph::build_with_limits(&parsed, limits.reference_graph)
                        .map_err(|source| {
                            workspace_error(
                                report,
                                Some(&input.virtual_path),
                                SerializedFileWorkspaceErrorKind::ReferenceGraph(source),
                            )
                        })?;
                (parsed.header, parsed.metadata, graph)
            };

            let next_total_objects = report
                .total_objects
                .checked_add(reference_graph.object_count())
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::ObjectCountOverflow,
                    )
                })?;
            if next_total_objects > limits.max_total_objects {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalObjectLimitExceeded {
                        requested_total: next_total_objects,
                        limit: limits.max_total_objects,
                    },
                ));
            }
            let next_total_edges = report
                .total_reference_edges
                .checked_add(reference_graph.references().len())
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::ReferenceCountOverflow,
                    )
                })?;
            if next_total_edges > limits.max_total_reference_edges {
                return Err(workspace_error(
                    report,
                    Some(&input.virtual_path),
                    SerializedFileWorkspaceErrorKind::TotalReferenceLimitExceeded {
                        requested_total: next_total_edges,
                        limit: limits.max_total_reference_edges,
                    },
                ));
            }

            report.completed_input_bytes = report
                .completed_input_bytes
                .checked_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| {
                    workspace_error(
                        report,
                        Some(&input.virtual_path),
                        SerializedFileWorkspaceErrorKind::InputByteCountOverflow,
                    )
                })?;
            report.completed_files += 1;
            report.total_objects = next_total_objects;
            report.total_reference_edges = next_total_edges;
            files.push(LoadedSerializedFile {
                file_name: input.file_name.clone(),
                virtual_path: input.virtual_path.clone(),
                header,
                metadata,
                reference_graph,
            });
        }

        Ok(Self { files, report })
    }

    pub fn files(&self) -> &[LoadedSerializedFile] {
        &self.files
    }

    pub fn report(&self) -> SerializedFileWorkspaceReport {
        self.report
    }

    /// Join every loaded file's per-file PPtr scan by normalized SerializedFile name.
    pub fn reference_graph(
        &self,
        limits: MultiFileReferenceGraphLimits,
    ) -> Result<MultiFileReferenceGraph<'_>, MultiFileReferenceGraphBuildError> {
        let mut named_files = Vec::new();
        named_files
            .try_reserve_exact(self.files.len())
            .map_err(|_| MultiFileReferenceGraphBuildError::AllocationFailed {
                resource: "named SerializedFile graph entries",
                requested: self.files.len(),
            })?;
        named_files.extend(self.files.iter().map(|file| NamedObjectReferenceGraph {
            file_name: &file.file_name,
            metadata: &file.metadata,
            graph: &file.reference_graph,
        }));
        MultiFileReferenceGraph::build_with_limits(&named_files, limits)
    }
}

#[derive(Debug)]
pub struct SerializedFileWorkspaceError {
    pub report: SerializedFileWorkspaceReport,
    pub input_path: Option<String>,
    pub kind: SerializedFileWorkspaceErrorKind,
}

#[derive(Debug)]
pub enum SerializedFileWorkspaceErrorKind {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    FileCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FileNameTooLong {
        input_index: usize,
        byte_count: usize,
        limit: usize,
    },
    FileNameByteCountOverflow,
    TotalFileNameBytesLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    DuplicateVirtualPath,
    FileSizeLimitExceeded {
        file_bytes: u64,
        limit: u64,
    },
    TotalInputLimitExceeded {
        requested_total: u64,
        limit: u64,
    },
    InputByteCountOverflow,
    NotSerializedFile {
        detected_kind: FileKind,
    },
    TotalObjectLimitExceeded {
        requested_total: usize,
        limit: usize,
    },
    ObjectCountOverflow,
    TotalReferenceLimitExceeded {
        requested_total: usize,
        limit: usize,
    },
    ReferenceCountOverflow,
    FileSystem(io::Error),
    Scheme(SchemeError),
    ReferenceGraph(ReferenceGraphBuildError),
}

impl Display for SerializedFileWorkspaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.input_path {
            write!(f, "SerializedFile workspace input '{path}' failed")?;
        } else {
            f.write_str("SerializedFile workspace input failed")?;
        }
        write!(
            f,
            " after {} completed files and {} bytes: {}",
            self.report.completed_files, self.report.completed_input_bytes, self.kind
        )
    }
}

impl Display for SerializedFileWorkspaceErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::FileCountLimitExceeded { count, limit } => {
                write!(f, "input file count {count} exceeds limit {limit}")
            }
            Self::FileNameTooLong {
                input_index,
                byte_count,
                limit,
            } => write!(
                f,
                "file name at input {input_index} has {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::FileNameByteCountOverflow => f.write_str("file name byte count overflows"),
            Self::TotalFileNameBytesLimitExceeded { byte_count, limit } => write!(
                f,
                "total normalized file-name bytes {byte_count} exceed limit {limit}"
            ),
            Self::DuplicateVirtualPath => f.write_str("virtual path occurs more than once"),
            Self::FileSizeLimitExceeded { file_bytes, limit } => {
                write!(f, "file size {file_bytes} bytes exceeds limit {limit}")
            }
            Self::TotalInputLimitExceeded {
                requested_total,
                limit,
            } => write!(
                f,
                "aggregate input size {requested_total} bytes exceeds limit {limit}"
            ),
            Self::InputByteCountOverflow => f.write_str("aggregate input byte count overflows"),
            Self::NotSerializedFile { detected_kind } => {
                write!(f, "expected a SerializedFile, detected {detected_kind:?}")
            }
            Self::TotalObjectLimitExceeded {
                requested_total,
                limit,
            } => write!(f, "object count {requested_total} exceeds limit {limit}"),
            Self::ObjectCountOverflow => f.write_str("aggregate object count overflows"),
            Self::TotalReferenceLimitExceeded {
                requested_total,
                limit,
            } => write!(f, "reference count {requested_total} exceeds limit {limit}"),
            Self::ReferenceCountOverflow => f.write_str("aggregate reference count overflows"),
            Self::FileSystem(error) => write!(f, "VFS error: {error}"),
            Self::Scheme(error) => write!(f, "file scheme error: {error}"),
            Self::ReferenceGraph(error) => write!(f, "per-file reference graph error: {error}"),
        }
    }
}

impl Error for SerializedFileWorkspaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SerializedFileWorkspaceErrorKind::FileSystem(error) => Some(error),
            SerializedFileWorkspaceErrorKind::Scheme(error) => Some(error),
            SerializedFileWorkspaceErrorKind::ReferenceGraph(error) => Some(error),
            _ => None,
        }
    }
}

impl Error for SerializedFileWorkspaceErrorKind {}

fn workspace_error(
    report: SerializedFileWorkspaceReport,
    input_path: Option<&str>,
    kind: SerializedFileWorkspaceErrorKind,
) -> Box<SerializedFileWorkspaceError> {
    Box::new(SerializedFileWorkspaceError {
        report,
        input_path: input_path.map(str::to_owned),
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        SerializedFileWorkspace, SerializedFileWorkspaceErrorKind, SerializedFileWorkspaceLimits,
    };
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::scheme::FileKind;
    use crate::serialized_file::reference_graph::{
        MultiFilePPtrResolution, MultiFileReferenceGraphLimits,
    };

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn put_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
    }

    fn make_generation_10_file() -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Root");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let value_name = push_type_tree_string(&mut string_buffer, "m_Value");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let target_name = push_type_tree_string(&mut string_buffer, "m_Target");
        let pptr_type = push_type_tree_string(&mut string_buffer, "PPtr<GameObject>");
        let file_id_name = push_type_tree_string(&mut string_buffer, "m_FileID");
        let int32_type = push_type_tree_string(&mut string_buffer, "SInt32");
        let path_id_name = push_type_tree_string(&mut string_buffer, "m_PathID");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 5);
        put_i32_le(&mut metadata, string_buffer.len() as i32);
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, int_type, value_name, 4, 1);
        put_node(&mut metadata, 1, pptr_type, target_name, 8, 2);
        put_node(&mut metadata, 2, int32_type, file_id_name, 4, 3);
        put_node(&mut metadata, 2, int32_type, path_id_name, 4, 4);
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0x1234_5678);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 42);
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&(metadata.len() as i32).to_be_bytes());
        file.extend_from_slice(&(file_size as u32).to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&(data_offset as u32).to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    fn virtual_file_system(path: &str, bytes: &[u8]) -> VirtualFileSystem {
        let mut filesystem = VirtualFileSystem::new();
        let parent = path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        filesystem.create_directory(Some(parent)).unwrap();
        filesystem.write_all_bytes(path, bytes).unwrap();
        filesystem
    }

    fn input(path: &str) -> DependencyFilePath {
        DependencyFilePath {
            file_name: path.rsplit('/').next().unwrap().to_owned(),
            virtual_path: path.to_owned(),
        }
    }

    #[test]
    fn loads_named_serialized_file_and_builds_cross_file_graph() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();

        assert_eq!(workspace.files().len(), 1);
        assert_eq!(workspace.files()[0].header.format_version.raw(), 10);
        assert_eq!(workspace.files()[0].metadata.objects[0].path_id, 42);
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert_eq!(
            workspace.report().completed_input_bytes,
            u64::try_from(bytes.len()).unwrap()
        );
        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn preflights_aggregate_input_before_loading_any_file() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits {
                max_total_input_bytes: 1,
                ..SerializedFileWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 0);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalInputLimitExceeded { limit: 1, .. }
        ));
    }

    #[test]
    fn rejects_nonserialized_files_and_object_count_overruns_with_context() {
        let resource_filesystem = virtual_file_system("/game/resource.resS", &[1, 2, 3]);
        let resource_inputs = [input("/game/resource.resS")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &resource_filesystem,
            &resource_inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap_err();
        assert_eq!(error.input_path.as_deref(), Some("/game/resource.resS"));
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::NotSerializedFile {
                detected_kind: FileKind::Resource
            }
        ));

        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let error = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits {
                max_total_objects: 0,
                ..SerializedFileWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.report.completed_files, 0);
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::TotalObjectLimitExceeded {
                requested_total: 1,
                limit: 0
            }
        ));
    }
}
