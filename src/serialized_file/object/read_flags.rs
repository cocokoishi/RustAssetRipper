use crate::io::binary::Endian;

use super::super::format_version::FormatVersion;
use super::super::transfer_flags::TransferInstructionFlags;
pub const BUILD_TARGET_NO_TARGET: u32 = 0xFFFFFFFE;

/// First engine default-resource file name in `SpecialFileNames`.
pub const DEFAULT_RESOURCE_NAME_1: &str = "unity default resources";
/// Second engine default-resource file name in `SpecialFileNames`.
pub const DEFAULT_RESOURCE_NAME_2: &str = "unity_default_resources";
/// Engine editor-resource file name in `SpecialFileNames`.
pub const EDITOR_RESOURCE_NAME: &str = "unity editor resources";
/// First builtin-extra file name in `SpecialFileNames`.
pub const BUILTIN_EXTRA_NAME_1: &str = "unity builtin extra";
/// Second builtin-extra file name in `SpecialFileNames`.
pub const BUILTIN_EXTRA_NAME_2: &str = "unity_builtin_extra";

/// `SpecialFileNames.IsEngineResource` over a caller-supplied file name.
///
/// The source compares with ordinal, case-sensitive equality against the raw
/// `FileBase.Name` (not `NameFixed`), so this does the same: callers must pass
/// the raw name unchanged. A `FixFileIdentifier` output such as
/// `"library/unity default resources"` therefore never matches here.
pub fn is_engine_resource_name(file_name: &str) -> bool {
    file_name == DEFAULT_RESOURCE_NAME_1
        || file_name == DEFAULT_RESOURCE_NAME_2
        || file_name == EDITOR_RESOURCE_NAME
}

/// `SpecialFileNames.IsBuiltinExtra` over a caller-supplied file name.
///
/// Same exact-match semantics as [`is_engine_resource_name`].
pub fn is_builtin_extra_name(file_name: &str) -> bool {
    file_name == BUILTIN_EXTRA_NAME_1 || file_name == BUILTIN_EXTRA_NAME_2
}

/// Transfer flags governing how a SerializedFile's objects are read.
///
/// Mirrors `SerializedFile.Flags`
/// (`AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/SerializedFile.cs`):
/// a `NoTarget` platform on a platform-carrying generation reads editor scenes
/// (`.unity` paths) as `SerializeEditorMinimalScene` and anything else with bare
/// flags, while every other file reads as `SerializeGameRelease`. Engine
/// resources (and, below generation 10, builtin-extra files) additionally set
/// `IsBuiltinResourcesFile`, and big-endian bodies set `SwapEndianess`.
///
/// Caller facts: `generation` is the file generation (`HasPlatform` is
/// `generation >= Unknown_8`, shared with the metadata prefix reader);
/// `platform` is the parsed target-platform value (`None` when the generation
/// carries no platform field — only `Some(BUILD_TARGET_NO_TARGET)` on a
/// platform-carrying generation takes the scene branch); `file_path` is the raw
/// `FileBase.FilePath` (the `.unity` check is an ordinal, case-sensitive suffix
/// on the full path, matching `EndsWith(".unity", Ordinal)`); `file_name` is
/// the raw `FileBase.Name` (not `NameFixed`); `body_endian` is the file body
/// endianness (the source's `EndianType` after `GetEndianType`).
///
/// The generated per-class release/editor read dispatch and the export
/// collections that carry `Flags` stay caller-side: this kernel only derives the
/// flag word. `SpecialFileNames.FixFileIdentifier` (`NameFixed`) normalization
/// is likewise out of scope here.
pub fn serialized_file_object_read_flags(
    generation: FormatVersion,
    platform: Option<u32>,
    file_path: &str,
    file_name: &str,
    body_endian: Endian,
) -> TransferInstructionFlags {
    let mut flags = if generation.has_platform() && platform == Some(BUILD_TARGET_NO_TARGET) {
        if file_path.ends_with(".unity") {
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
        } else {
            TransferInstructionFlags::NONE
        }
    } else {
        TransferInstructionFlags::SERIALIZE_GAME_RELEASE
    };
    let builtin_extra_below_generation_10 =
        generation.raw() < FormatVersion::Unknown10.raw() && is_builtin_extra_name(file_name);
    if is_engine_resource_name(file_name) || builtin_extra_below_generation_10 {
        flags = flags.with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE);
    }
    if body_endian == Endian::Big {
        flags = flags.with(TransferInstructionFlags::SWAP_ENDIANESS);
    }
    flags
}
