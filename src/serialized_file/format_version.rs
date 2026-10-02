use std::fmt::{self, Display, Formatter};

/// SerializedFile format generations recognized by the audited AssetRipper snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum FormatVersion {
    Unsupported = 1,
    Unknown2 = 2,
    Unknown3 = 3,
    Unknown5 = 5,
    Unknown6 = 6,
    Unknown7 = 7,
    Unknown8 = 8,
    Unknown9 = 9,
    Unknown10 = 10,
    HasScriptTypeIndex = 11,
    Unknown12 = 12,
    HasTypeTreeHashes = 13,
    Unknown14 = 14,
    SupportsStrippedObject = 15,
    RefactoredClassId = 16,
    RefactorTypeData = 17,
    RefactorShareableTypeTreeData = 18,
    TypeTreeNodeWithTypeFlags = 19,
    SupportsRefObject = 20,
    StoresTypeDependencies = 21,
    LargeFilesSupport = 22,
}

impl FormatVersion {
    pub const fn raw(self) -> i32 {
        self as i32
    }

    /// Generation 9 added the endian marker after the legacy 16-byte prefix.
    pub const fn has_endianness(self) -> bool {
        self.raw() >= Self::Unknown9.raw()
    }

    /// Generation 22 replaced the legacy size/offset fields with extended fields.
    pub const fn has_large_file_support(self) -> bool {
        self.raw() >= Self::LargeFilesSupport.raw()
    }

    /// Generations before 9 place metadata at the end of the SerializedFile.
    pub const fn metadata_at_end(self) -> bool {
        self.raw() < Self::Unknown9.raw()
    }

    pub const fn has_signature(self) -> bool {
        self.raw() >= Self::Unknown7.raw()
    }

    pub const fn has_platform(self) -> bool {
        self.raw() >= Self::Unknown8.raw()
    }

    pub const fn has_enable_type_tree(self) -> bool {
        self.raw() >= Self::HasTypeTreeHashes.raw()
    }

    pub const fn has_long_file_id(self) -> bool {
        self.raw() >= Self::Unknown7.raw() && self.raw() < Self::Unknown14.raw()
    }

    pub const fn has_script_types(self) -> bool {
        self.raw() >= Self::HasScriptTypeIndex.raw()
    }

    pub const fn has_user_information(self) -> bool {
        self.raw() >= Self::Unknown5.raw()
    }

    pub const fn has_ref_types(self) -> bool {
        self.raw() >= Self::SupportsRefObject.raw()
    }
}

impl TryFrom<i32> for FormatVersion {
    type Error = UnsupportedFormatVersion;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Unsupported),
            2 => Ok(Self::Unknown2),
            3 => Ok(Self::Unknown3),
            5 => Ok(Self::Unknown5),
            6 => Ok(Self::Unknown6),
            7 => Ok(Self::Unknown7),
            8 => Ok(Self::Unknown8),
            9 => Ok(Self::Unknown9),
            10 => Ok(Self::Unknown10),
            11 => Ok(Self::HasScriptTypeIndex),
            12 => Ok(Self::Unknown12),
            13 => Ok(Self::HasTypeTreeHashes),
            14 => Ok(Self::Unknown14),
            15 => Ok(Self::SupportsStrippedObject),
            16 => Ok(Self::RefactoredClassId),
            17 => Ok(Self::RefactorTypeData),
            18 => Ok(Self::RefactorShareableTypeTreeData),
            19 => Ok(Self::TypeTreeNodeWithTypeFlags),
            20 => Ok(Self::SupportsRefObject),
            21 => Ok(Self::StoresTypeDependencies),
            22 => Ok(Self::LargeFilesSupport),
            _ => Err(UnsupportedFormatVersion(value)),
        }
    }
}

impl Display for FormatVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnsupportedFormatVersion(pub i32);

impl Display for UnsupportedFormatVersion {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "unsupported SerializedFile format generation {}", self.0)
    }
}

impl std::error::Error for UnsupportedFormatVersion {}

#[cfg(test)]
mod tests {
    use super::{FormatVersion, UnsupportedFormatVersion};

    #[test]
    fn every_declared_source_generation_is_recognized() {
        let expected = [
            (1, FormatVersion::Unsupported),
            (2, FormatVersion::Unknown2),
            (3, FormatVersion::Unknown3),
            (5, FormatVersion::Unknown5),
            (6, FormatVersion::Unknown6),
            (7, FormatVersion::Unknown7),
            (8, FormatVersion::Unknown8),
            (9, FormatVersion::Unknown9),
            (10, FormatVersion::Unknown10),
            (11, FormatVersion::HasScriptTypeIndex),
            (12, FormatVersion::Unknown12),
            (13, FormatVersion::HasTypeTreeHashes),
            (14, FormatVersion::Unknown14),
            (15, FormatVersion::SupportsStrippedObject),
            (16, FormatVersion::RefactoredClassId),
            (17, FormatVersion::RefactorTypeData),
            (18, FormatVersion::RefactorShareableTypeTreeData),
            (19, FormatVersion::TypeTreeNodeWithTypeFlags),
            (20, FormatVersion::SupportsRefObject),
            (21, FormatVersion::StoresTypeDependencies),
            (22, FormatVersion::LargeFilesSupport),
        ];

        for (raw, version) in expected {
            assert_eq!(FormatVersion::try_from(raw), Ok(version));
        }
        assert_eq!(FormatVersion::try_from(4), Err(UnsupportedFormatVersion(4)));
        assert_eq!(
            FormatVersion::try_from(23),
            Err(UnsupportedFormatVersion(23))
        );
    }

    #[test]
    fn header_layout_predicates_match_generation_boundaries() {
        for (raw, has_endianness, has_large_files, metadata_at_end) in [
            (8, false, false, true),
            (9, true, false, false),
            (21, true, false, false),
            (22, true, true, false),
        ] {
            let version = FormatVersion::try_from(raw).unwrap();
            assert_eq!(version.has_endianness(), has_endianness);
            assert_eq!(version.has_large_file_support(), has_large_files);
            assert_eq!(version.metadata_at_end(), metadata_at_end);
        }
    }

    #[test]
    fn metadata_predicates_match_each_source_boundary() {
        for (raw, signature, platform, type_tree, long_file_id, scripts, user_info, ref_types) in [
            (5, false, false, false, false, false, true, false),
            (6, false, false, false, false, false, true, false),
            (7, true, false, false, true, false, true, false),
            (8, true, true, false, true, false, true, false),
            (10, true, true, false, true, false, true, false),
            (11, true, true, false, true, true, true, false),
            (12, true, true, false, true, true, true, false),
            (13, true, true, true, true, true, true, false),
            (14, true, true, true, false, true, true, false),
            (19, true, true, true, false, true, true, false),
            (20, true, true, true, false, true, true, true),
            (22, true, true, true, false, true, true, true),
        ] {
            let version = FormatVersion::try_from(raw).unwrap();
            assert_eq!(version.has_signature(), signature, "generation {raw}");
            assert_eq!(version.has_platform(), platform, "generation {raw}");
            assert_eq!(
                version.has_enable_type_tree(),
                type_tree,
                "generation {raw}"
            );
            assert_eq!(version.has_long_file_id(), long_file_id, "generation {raw}");
            assert_eq!(version.has_script_types(), scripts, "generation {raw}");
            assert_eq!(
                version.has_user_information(),
                user_info,
                "generation {raw}"
            );
            assert_eq!(version.has_ref_types(), ref_types, "generation {raw}");
        }
    }
}
