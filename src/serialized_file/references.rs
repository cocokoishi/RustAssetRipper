//! File-scoped classification of serialized Unity object pointers.
//!
//! Covers the read side (`AssetCollection.TryGetAsset` file-table convention: FileID zero is the
//! local file, positive FileIDs select external entries starting at one) and the write-side
//! construction choice (`AssetCollection.CreatePPtr` refuses collections absent from the
//! dependency table while `ForceCreatePPtr` appends them). Loading an external dependency,
//! checking dependency compatibility, and appending to a live table stay caller-side.

use super::metadata::{FileIdentifier, SerializedFileMetadata};

/// What a serialized PPtr identifies within one SerializedFile's file table.
///
/// An external dependency is identified but not loaded here. Resolving it requires the bundle or
/// workspace dependency provider, which owns filename matching and multi-file loading.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PPtrResolution<'a> {
    /// Unity reserves PathID zero to represent a null pointer.
    Null,
    /// The pointer resolves to the unique object with this PathID in the current file.
    LocalObject { object_index: usize },
    /// The current file has no object with this nonzero PathID.
    MissingLocalObject { path_id: i64 },
    /// More than one current-file object claims this PathID; choosing one would hide bad input.
    AmbiguousLocalObject { path_id: i64, match_count: usize },
    /// The one-based PPtr FileID selects an entry in this file's external table.
    ExternalDependency {
        file_id: i32,
        external_index: usize,
        path_id: i64,
        identifier: &'a FileIdentifier,
    },
    /// The FileID is negative or selects beyond the external table.
    InvalidFileId { file_id: i32, external_count: usize },
}

impl SerializedFileMetadata {
    /// Classify a PPtr using AssetRipper's file-table convention: FileID zero is this file and
    /// positive FileIDs select external entries starting at one. This does not claim to load an
    /// external dependency or to build a transitive reference graph.
    pub fn resolve_pptr(&self, file_id: i32, path_id: i64) -> PPtrResolution<'_> {
        if path_id == 0 {
            return PPtrResolution::Null;
        }

        if file_id == 0 {
            let mut object_index = None;
            let mut match_count = 0usize;
            for (index, object) in self.objects.iter().enumerate() {
                if object.path_id == path_id {
                    object_index.get_or_insert(index);
                    match_count += 1;
                }
            }
            return match (object_index, match_count) {
                (Some(object_index), 1) => PPtrResolution::LocalObject { object_index },
                (None, 0) => PPtrResolution::MissingLocalObject { path_id },
                (_, count) => PPtrResolution::AmbiguousLocalObject {
                    path_id,
                    match_count: count,
                },
            };
        }

        let Some(external_index) = file_id
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
        else {
            return PPtrResolution::InvalidFileId {
                file_id,
                external_count: self.externals.len(),
            };
        };
        match self.externals.get(external_index) {
            Some(identifier) => PPtrResolution::ExternalDependency {
                file_id,
                external_index,
                path_id,
                identifier,
            },
            None => PPtrResolution::InvalidFileId {
                file_id,
                external_count: self.externals.len(),
            },
        }
    }
}

/// What constructing a PPtr for an asset identity decides.
///
/// Mirrors `AssetCollection.CreatePPtr` (strict) and `ForceCreatePPtr` (appending)
/// (`AssetRipper/Source/AssetRipper.Assets/Collections/AssetCollection.cs`, lines
/// 92-116), which back the generated `IPPtr<T>.SetAsset` implementations: the asset's
/// collection selects its dependency-table index (the local collection is index zero,
/// like FileID zero), and a null asset always encodes as the default `(0, 0)` pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PPtrConstruction {
    /// A null asset encodes as `(0, 0)` in both modes.
    Null,
    /// The asset's collection already sits at this dependency-table index, so both
    /// modes reuse it (`IndexOf` hit / `AddDependency` early return).
    Existing { file_id: i32, path_id: i64 },
    /// Force mode appended the missing collection at this index. Table growth and the
    /// compatibility check stay caller-side, like `AddDependency`.
    Appended { file_id: i32, path_id: i64 },
    /// Strict mode refuses a collection absent from the dependency table; the source
    /// throws `ArgumentException` here instead of producing a pointer.
    MissingDependency,
}

/// Plan `CreatePPtr` (`force == false`) or `ForceCreatePPtr` (`force == true`) over caller
/// facts: the asset's PathID (`None` is the null asset), the asset collection's
/// dependency-table index (`None` when absent), and the index an append would take
/// (the caller's current table length).
pub const fn plan_pptr_construction(
    path_id: Option<i64>,
    dependency_index: Option<i32>,
    force: bool,
    next_dependency_index: i32,
) -> PPtrConstruction {
    match path_id {
        None => PPtrConstruction::Null,
        Some(path_id) => match dependency_index {
            Some(file_id) => PPtrConstruction::Existing { file_id, path_id },
            None => {
                if force {
                    PPtrConstruction::Appended {
                        file_id: next_dependency_index,
                        path_id,
                    }
                } else {
                    PPtrConstruction::MissingDependency
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{PPtrConstruction, PPtrResolution, SerializedFileMetadata};
    use crate::io::binary::Endian;
    use crate::serialized_file::metadata::{FileIdentifier, ObjectInfo};
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;

    fn metadata() -> SerializedFileMetadata {
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
            objects: vec![object(101), object(202)],
            script_types: Vec::new(),
            externals: vec![external("shared.assets"), external("scene.assets")],
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn object(path_id: i64) -> ObjectInfo {
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

    fn external(path_name_origin: &str) -> FileIdentifier {
        FileIdentifier {
            asset_path: Some(path_name_origin.to_owned()),
            guid: None,
            file_type: None,
            path_name_origin: path_name_origin.to_owned(),
        }
    }

    #[test]
    fn path_id_zero_is_null_even_when_file_id_is_not_local() {
        assert_eq!(metadata().resolve_pptr(999, 0), PPtrResolution::Null);
    }

    #[test]
    fn local_file_id_resolves_only_unique_nonzero_path_ids() {
        let metadata = metadata();
        assert_eq!(
            metadata.resolve_pptr(0, 202),
            PPtrResolution::LocalObject { object_index: 1 }
        );
        assert_eq!(
            metadata.resolve_pptr(0, 303),
            PPtrResolution::MissingLocalObject { path_id: 303 }
        );
    }

    #[test]
    fn external_file_ids_are_one_based_and_keep_the_identifier_and_path_id() {
        let metadata = metadata();
        assert_eq!(
            metadata.resolve_pptr(2, 404),
            PPtrResolution::ExternalDependency {
                file_id: 2,
                external_index: 1,
                path_id: 404,
                identifier: &metadata.externals[1],
            }
        );
    }

    #[test]
    fn invalid_file_ids_and_duplicate_local_targets_remain_explicit() {
        let mut metadata = metadata();
        assert_eq!(
            metadata.resolve_pptr(-1, 101),
            PPtrResolution::InvalidFileId {
                file_id: -1,
                external_count: 2,
            }
        );
        assert_eq!(
            metadata.resolve_pptr(3, 101),
            PPtrResolution::InvalidFileId {
                file_id: 3,
                external_count: 2,
            }
        );
        metadata.objects.push(object(101));
        assert_eq!(
            metadata.resolve_pptr(0, 101),
            PPtrResolution::AmbiguousLocalObject {
                path_id: 101,
                match_count: 2,
            }
        );
    }

    #[test]
    fn pptr_construction_follows_the_strict_and_force_branches() {
        use super::plan_pptr_construction;
        // A null asset encodes as (0, 0) in both modes.
        assert_eq!(
            plan_pptr_construction(None, None, false, 3),
            PPtrConstruction::Null
        );
        assert_eq!(
            plan_pptr_construction(None, Some(0), true, 3),
            PPtrConstruction::Null
        );
        // A known collection is reused by both modes, including the local index zero.
        assert_eq!(
            plan_pptr_construction(Some(101), Some(0), false, 3),
            PPtrConstruction::Existing {
                file_id: 0,
                path_id: 101
            }
        );
        assert_eq!(
            plan_pptr_construction(Some(101), Some(2), true, 3),
            PPtrConstruction::Existing {
                file_id: 2,
                path_id: 101
            }
        );
        // A missing collection is refused in strict mode but appended in force mode.
        assert_eq!(
            plan_pptr_construction(Some(101), None, false, 3),
            PPtrConstruction::MissingDependency
        );
        assert_eq!(
            plan_pptr_construction(Some(101), None, true, 3),
            PPtrConstruction::Appended {
                file_id: 3,
                path_id: 101
            }
        );
    }
}
