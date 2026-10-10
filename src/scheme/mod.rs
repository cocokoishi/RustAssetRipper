//! Source-compatible detection, recursive decoding, and bounded container mounting.

mod batch_mount;
mod container_mount;
mod errors;
mod models;
mod reader;

pub use self::errors::{
    BatchMountError, BatchMountErrorKind, ContainerMountError, ContainerMountErrorKind,
    SchemeError, SchemeErrorKind,
};
pub use self::models::{
    is_container_kind, BatchContainerTree, BatchFailedContainerEntry, BatchMountLimits,
    BatchMountReport, ContainerMountLimits, ContainerMountReport, FailedContainerEntry, FileKind,
    MountedContainerBundle, MountedExplicitResourceFile, MountedResourceFile,
    MountedSerializedFile, PathMountInput, SchemeReaderLimits, WrapperKind, WrapperLayer,
};
pub use self::reader::{DispatchedFile, ParsedFile, SchemeReader};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
