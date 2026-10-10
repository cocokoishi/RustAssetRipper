#[test]
fn object_read_flags_select_release_for_ordinary_game_files() {
    use super::TransferInstructionFlags;
    use crate::serialized_file::format_version::FormatVersion;

    // An ordinary platformed file reads as a game release.
    let flags = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(7),
        "/game/level0",
        "level0",
        crate::io::binary::Endian::Little,
    );
    assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    assert!(flags.is_release());
    // A generation without a platform field never takes the scene branch,
    // even with a NoTarget stand-in and a `.unity` path.
    let flags = super::serialized_file_object_read_flags(
        FormatVersion::Unknown7,
        None,
        "/game/Level.unity",
        "Level",
        crate::io::binary::Endian::Little,
    );
    assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
}

#[test]
fn object_read_flags_select_editor_scene_for_unity_paths_without_target() {
    use super::TransferInstructionFlags;
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;

    let scene = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(super::BUILD_TARGET_NO_TARGET),
        "Assets/Scenes/Main.unity",
        "Main",
        Endian::Little,
    );
    assert_eq!(
        scene,
        TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
    );
    assert!(scene.is_editor_scene());
    assert!(!scene.is_release());
    // A NoTarget platform without a `.unity` path carries bare flags.
    let bare = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(super::BUILD_TARGET_NO_TARGET),
        "/game/sharedassets0.assets",
        "sharedassets0.assets",
        Endian::Little,
    );
    assert_eq!(bare, TransferInstructionFlags::NONE);
    // The suffix check is ordinal and case-sensitive, like the source.
    let upper = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(super::BUILD_TARGET_NO_TARGET),
        "Assets/Scenes/Main.UNITY",
        "Main",
        Endian::Little,
    );
    assert_eq!(upper, TransferInstructionFlags::NONE);
    let backup = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(super::BUILD_TARGET_NO_TARGET),
        "Assets/Scenes/Main.unity.backup",
        "Main",
        Endian::Little,
    );
    assert_eq!(backup, TransferInstructionFlags::NONE);
    // A real platform target always reads as a game release.
    let release = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(7),
        "Assets/Scenes/Main.unity",
        "Main",
        Endian::Little,
    );
    assert_eq!(release, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
}

#[test]
fn object_read_flags_mark_builtin_resource_files() {
    use super::TransferInstructionFlags;
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;

    // Engine resources set the builtin bit on any generation.
    for name in [
        super::DEFAULT_RESOURCE_NAME_1,
        super::DEFAULT_RESOURCE_NAME_2,
        super::EDITOR_RESOURCE_NAME,
    ] {
        let flags = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "/game/resources.assets",
            name,
            Endian::Little,
        );
        assert_eq!(
            flags,
            TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                .with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE)
        );
        assert!(flags.is_builtin_resources());
    }
    // Builtin-extra names only count below generation 10.
    for name in [super::BUILTIN_EXTRA_NAME_1, super::BUILTIN_EXTRA_NAME_2] {
        let old = super::serialized_file_object_read_flags(
            FormatVersion::Unknown9,
            Some(super::BUILD_TARGET_NO_TARGET),
            "/game/extra.assets",
            name,
            Endian::Little,
        );
        assert!(old.is_builtin_resources());
        let cutoff = super::serialized_file_object_read_flags(
            FormatVersion::Unknown10,
            Some(7),
            "/game/extra.assets",
            name,
            Endian::Little,
        );
        assert!(!cutoff.is_builtin_resources());
        assert_eq!(cutoff, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    }
    // Ordinary names never set the builtin bit.
    let plain = super::serialized_file_object_read_flags(
        FormatVersion::Unknown9,
        None,
        "/game/sharedassets0.assets",
        "sharedassets0.assets",
        Endian::Little,
    );
    assert!(!plain.is_builtin_resources());
}

#[test]
fn object_read_flags_swap_endianness_for_big_endian_bodies() {
    use super::TransferInstructionFlags;
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;

    let big = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(7),
        "/game/data.assets",
        "data.assets",
        Endian::Big,
    );
    assert_eq!(
        big,
        TransferInstructionFlags::SERIALIZE_GAME_RELEASE
            .with(TransferInstructionFlags::SWAP_ENDIANESS)
    );
    // The endian bit composes with the scene and builtin bits.
    let scene_big = super::serialized_file_object_read_flags(
        FormatVersion::LargeFilesSupport,
        Some(super::BUILD_TARGET_NO_TARGET),
        "Assets/Scenes/Main.unity",
        super::DEFAULT_RESOURCE_NAME_2,
        Endian::Big,
    );
    assert!(scene_big.is_editor_scene());
    assert!(scene_big.is_builtin_resources());
    assert_eq!(
        scene_big.0,
        TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE.0
            | TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE.0
            | TransferInstructionFlags::SWAP_ENDIANESS.0
    );
}

#[test]
fn special_file_name_predicates_match_exact_names_only() {
    assert!(super::is_engine_resource_name("unity default resources"));
    assert!(super::is_engine_resource_name("unity_default_resources"));
    assert!(super::is_engine_resource_name("unity editor resources"));
    assert!(!super::is_engine_resource_name("unity builtin extra"));
    // Ordinal comparison: case and normalization matter.
    assert!(!super::is_engine_resource_name("Unity Default Resources"));
    assert!(!super::is_engine_resource_name(
        "library/unity default resources"
    ));
    assert!(!super::is_engine_resource_name(""));
    assert!(super::is_builtin_extra_name("unity builtin extra"));
    assert!(super::is_builtin_extra_name("unity_builtin_extra"));
    assert!(!super::is_builtin_extra_name("Unity Builtin Extra"));
    assert!(!super::is_builtin_extra_name("unity editor resources"));
}
