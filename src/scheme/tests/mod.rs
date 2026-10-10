use super::{
    is_container_kind, BatchMountErrorKind, BatchMountLimits, ContainerMountErrorKind,
    ContainerMountLimits, DispatchedFile, FileKind, MountedContainerBundle,
    MountedExplicitResourceFile, MountedResourceFile, MountedSerializedFile, ParsedFile,
    PathMountInput, SchemeErrorKind, SchemeReader, SchemeReaderLimits, WrapperKind,
};
use crate::compression::wrappers::{compress_gzip, decompress_brotli, has_brotli_file_signature};
use crate::io::filesystem::VirtualFileSystem;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::PathBuf;

mod batch_mount;
mod container_mount;
mod reader;
mod support;
