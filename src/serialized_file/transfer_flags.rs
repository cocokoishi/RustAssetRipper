//! Serialized-file transfer instruction flags.
//!
//! `TransferInstructionFlags` is a `uint` bitmask read from SerializedFile headers and
//! TypeTree nodes and carried by every export collection's `Flags`. All 32 values below
//! match the frozen snapshot exactly, including the remarks Unity itself uses (remapping
//! instance IDs, meta-only assets, prefab-system serialization, release builds).
//!
//! The four `Is*` predicates mask single bits without shifting: release, prefab system,
//! editor-minimal scene, and builtin-resources file. Prefab export collections OR the
//! `SerializeForPrefabSystem` bit onto their base flags.

/// Raw transfer-instruction flag words.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TransferInstructionFlags(pub u32);

impl TransferInstructionFlags {
    /// No flags set.
    pub const NONE: Self = Self(0x0);
    /// Store memory instance IDs instead of remapping through path/file IDs.
    pub const NEEDS_INSTANCE_ID_REMAPPING: Self = Self(0x1);
    /// Only serialize data needed for `.meta` files.
    pub const ASSET_META_DATA_ONLY: Self = Self(0x2);
    /// Handle driven properties (global PPtr references in YAML).
    pub const YAML_GLOBAL_PPTR_REFERENCE: Self = Self(0x4);
    pub const LOAD_AND_UNLOAD_ASSETS_DURING_BUILD: Self = Self(0x8);
    /// Serialize debug properties such as private Mono fields.
    pub const SERIALIZE_DEBUG_PROPERTIES: Self = Self(0x10);
    /// Ignore debug properties when calculating the TypeTree index.
    pub const IGNORE_DEBUG_PROPERTIES_FOR_INDEX: Self = Self(0x20);
    /// Let the build player cull unused material properties.
    pub const BUILD_PLAYER_ONLY_SERIALIZE_BUILD_PROPERTIES: Self = Self(0x40);
    /// The Unity 3.5 mesh serialization workaround (`IsCloningObject`).
    pub const WORKAROUND_35_MESH_SERIALIZATION_FUCKUP: Self = Self(0x80);
    /// A game file rather than a project file.
    pub const SERIALIZE_GAME_RELEASE: Self = Self(0x100);
    /// Swap endianness when reading or writing the file.
    pub const SWAP_ENDIANESS: Self = Self(0x200);
    /// Save global managers when writing the game build.
    pub const SAVE_GLOBAL_MANAGERS: Self = Self(0x400);
    pub const DONT_READ_OBJECTS_FROM_DISK_BEFORE_WRITING: Self = Self(0x800);
    /// Back up Mono variables for an assembly reload.
    pub const SERIALIZE_MONO_RELOAD: Self = Self(0x1000);
    /// Skip metadata work when serializing Mono data.
    pub const DONT_REQUIRE_ALL_META_FLAGS: Self = Self(0x2000);
    /// Serialize for the prefab system (set by prefab export collections).
    pub const SERIALIZE_FOR_PREFAB_SYSTEM: Self = Self(0x4000);
    /// Serialize for the slim player (warns about leaked objects).
    pub const WARN_ABOUT_LEAKED_OBJECTS: Self = Self(0x8000);
    pub const LOAD_PREFAB_AS_SCENE: Self = Self(0x10000);
    pub const SERIALIZE_COPY_PASTE_TRANSFER: Self = Self(0x20000);
    /// Skip serializing to a temporary file (editor play mode).
    pub const EDITOR_PLAY_MODE: Self = Self(0x40000);
    pub const BUILD_RESOURCE_IMAGE: Self = Self(0x80000);
    pub const DONT_WRITE_UNITY_VERSION: Self = Self(0x100000);
    /// Binary editor scene files; hides `PrefabInstance.RootGameObject` from type trees.
    pub const SERIALIZE_EDITOR_MINIMAL_SCENE: Self = Self(0x200000);
    pub const GENERATE_BAKED_PHYSIX_MESHES: Self = Self(0x400000);
    pub const THREADED_SERIALIZATION: Self = Self(0x800000);
    pub const IS_BUILTIN_RESOURCES_FILE: Self = Self(0x1000000);
    pub const PERFORM_UNLOAD_DEPENDENCY_TRACKING: Self = Self(0x2000000);
    pub const DISABLE_WRITE_TYPE_TREE: Self = Self(0x4000000);
    pub const AUTOREPLACE_EDITOR_WINDOW: Self = Self(0x8000000);
    pub const DONT_CREATE_MONO_BEHAVIOUR_SCRIPT_WRAPPER: Self = Self(0x10000000);
    pub const SERIALIZE_FOR_INSPECTOR: Self = Self(0x20000000);
    /// Let later Unity versions attempt to read type-tree-disabled files.
    pub const SERIALIZED_ASSET_BUNDLE_VERSION: Self = Self(0x40000000);
    pub const ALLOW_TEXT_SERIALIZATION: Self = Self(0x80000000);

    /// All flags: no flags plus each bit in ascending order.
    pub const ALL: [Self; 33] = [
        Self::NONE,
        Self::NEEDS_INSTANCE_ID_REMAPPING,
        Self::ASSET_META_DATA_ONLY,
        Self::YAML_GLOBAL_PPTR_REFERENCE,
        Self::LOAD_AND_UNLOAD_ASSETS_DURING_BUILD,
        Self::SERIALIZE_DEBUG_PROPERTIES,
        Self::IGNORE_DEBUG_PROPERTIES_FOR_INDEX,
        Self::BUILD_PLAYER_ONLY_SERIALIZE_BUILD_PROPERTIES,
        Self::WORKAROUND_35_MESH_SERIALIZATION_FUCKUP,
        Self::SERIALIZE_GAME_RELEASE,
        Self::SWAP_ENDIANESS,
        Self::SAVE_GLOBAL_MANAGERS,
        Self::DONT_READ_OBJECTS_FROM_DISK_BEFORE_WRITING,
        Self::SERIALIZE_MONO_RELOAD,
        Self::DONT_REQUIRE_ALL_META_FLAGS,
        Self::SERIALIZE_FOR_PREFAB_SYSTEM,
        Self::WARN_ABOUT_LEAKED_OBJECTS,
        Self::LOAD_PREFAB_AS_SCENE,
        Self::SERIALIZE_COPY_PASTE_TRANSFER,
        Self::EDITOR_PLAY_MODE,
        Self::BUILD_RESOURCE_IMAGE,
        Self::DONT_WRITE_UNITY_VERSION,
        Self::SERIALIZE_EDITOR_MINIMAL_SCENE,
        Self::GENERATE_BAKED_PHYSIX_MESHES,
        Self::THREADED_SERIALIZATION,
        Self::IS_BUILTIN_RESOURCES_FILE,
        Self::PERFORM_UNLOAD_DEPENDENCY_TRACKING,
        Self::DISABLE_WRITE_TYPE_TREE,
        Self::AUTOREPLACE_EDITOR_WINDOW,
        Self::DONT_CREATE_MONO_BEHAVIOUR_SCRIPT_WRAPPER,
        Self::SERIALIZE_FOR_INSPECTOR,
        Self::SERIALIZED_ASSET_BUNDLE_VERSION,
        Self::ALLOW_TEXT_SERIALIZATION,
    ];

    /// `IsRelease`: the game-release bit is set.
    pub const fn is_release(self) -> bool {
        self.0 & Self::SERIALIZE_GAME_RELEASE.0 != 0
    }

    /// `IsForPrefab`: the prefab-system bit is set.
    pub const fn is_for_prefab(self) -> bool {
        self.0 & Self::SERIALIZE_FOR_PREFAB_SYSTEM.0 != 0
    }

    /// `IsEditorScene`: the editor-minimal-scene bit is set.
    pub const fn is_editor_scene(self) -> bool {
        self.0 & Self::SERIALIZE_EDITOR_MINIMAL_SCENE.0 != 0
    }

    /// `IsBuiltinResources`: the builtin-resources bit is set.
    pub const fn is_builtin_resources(self) -> bool {
        self.0 & Self::IS_BUILTIN_RESOURCES_FILE.0 != 0
    }

    /// Set one flag bit.
    pub const fn with(self, flag: Self) -> Self {
        Self(self.0 | flag.0)
    }
}

/// `PrefabExportCollection.Flags`: the base collection flags plus prefab-system serialization.
pub const fn prefab_export_flags(base: TransferInstructionFlags) -> TransferInstructionFlags {
    base.with(TransferInstructionFlags::SERIALIZE_FOR_PREFAB_SYSTEM)
}

#[cfg(test)]
mod tests {
    use super::{prefab_export_flags, TransferInstructionFlags};

    #[test]
    fn all_flags_cover_each_bit_exactly_once() {
        assert_eq!(TransferInstructionFlags::ALL.len(), 33);
        let mut seen = 0u32;
        for (index, flag) in TransferInstructionFlags::ALL.iter().enumerate() {
            if index == 0 {
                assert_eq!(flag.0, 0);
                continue;
            }
            assert_eq!(flag.0, 1u32 << (index as u32 - 1), "bit {index}");
            assert_eq!(seen & flag.0, 0);
            seen |= flag.0;
        }
        assert_eq!(seen, 0xFFFF_FFFF);
        // Spot-check values against the snapshot enum.
        assert_eq!(TransferInstructionFlags::SERIALIZE_GAME_RELEASE.0, 0x100);
        assert_eq!(
            TransferInstructionFlags::SERIALIZE_FOR_PREFAB_SYSTEM.0,
            0x4000
        );
        assert_eq!(
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE.0,
            0x200000
        );
        assert_eq!(
            TransferInstructionFlags::ALLOW_TEXT_SERIALIZATION.0,
            0x80000000
        );
    }

    #[test]
    fn predicates_mask_single_bits() {
        let release = TransferInstructionFlags::SERIALIZE_GAME_RELEASE;
        assert!(release.is_release());
        assert!(!release.is_for_prefab());
        let prefab = TransferInstructionFlags::SERIALIZE_FOR_PREFAB_SYSTEM;
        assert!(prefab.is_for_prefab());
        assert!(!prefab.is_release());
        let combined = release.with(prefab);
        assert!(combined.is_release());
        assert!(combined.is_for_prefab());
        assert!(!combined.is_editor_scene());
        assert!(!combined.is_builtin_resources());
        assert!(TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE.is_editor_scene());
        assert!(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE.is_builtin_resources());
        assert!(!TransferInstructionFlags::NONE.is_release());
    }

    #[test]
    fn prefab_collections_add_the_prefab_system_bit() {
        assert_eq!(
            prefab_export_flags(TransferInstructionFlags::NONE),
            TransferInstructionFlags::SERIALIZE_FOR_PREFAB_SYSTEM
        );
        let base = TransferInstructionFlags::SERIALIZE_GAME_RELEASE;
        let exported = prefab_export_flags(base);
        assert!(exported.is_for_prefab());
        assert!(exported.is_release());
        // Setting the bit twice keeps it set.
        assert_eq!(
            prefab_export_flags(TransferInstructionFlags::SERIALIZE_FOR_PREFAB_SYSTEM),
            TransferInstructionFlags::SERIALIZE_FOR_PREFAB_SYSTEM
        );
    }
}
