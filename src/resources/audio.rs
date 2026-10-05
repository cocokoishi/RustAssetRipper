//! Class-83 `AudioClip` field view and audio-data source resolution.
//!
//! Ports the field reads behind `AudioClipExtensions.GetAudioData` and
//! `CheckAssetIntegrity`: inline `m_AudioData` bytes win when present and nonempty,
//! otherwise the `m_Resource` streamed descriptor supplies the content, otherwise the
//! clip has no data. Content fetching and resource integrity checks need the asset
//! collection and stay caller-side; this module resolves only which source applies.
//!
//! The version gates are read by field presence: Unity 3.5 carries inline
//! `m_AudioData` with `m_Type`/`m_Format` (`Has_AudioData`/`Has_Type`/`Has_Format`
//! true, `Has_Resource` false on `AudioClip_3_5`); Unity 5.0+ carries `m_Resource`
//! with `m_CompressionFormat` (gates reversed on `AudioClip_5` and later, confirmed
//! by probing the shipped `AssetRipper.SourceGenerated.dll`). The `m_`-prefixed
//! serialized names follow the generated-property convention; no class-83 TypeTree
//! dump has been run to confirm them.
//!
//! Differential evidence: `audio_generated_api_fixture.txt` (probe
//! `Temp/opencode/audioprops`, `IAudioClip` properties/gates, per-version gate
//! values, and the `FmodSoundType`/`FmodSoundFormat` discriminants).

use std::error::Error;
use std::fmt::{self, Display};

use crate::resource::{
    extract_streamed_resource_info, StreamedResourceInfo, StreamedResourceInfoError,
};
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

/// The serialized inline audio bytes field.
pub const AUDIO_DATA_FIELD: &str = "m_AudioData";
/// The serialized streamed-resource field.
pub const AUDIO_RESOURCE_FIELD: &str = "m_Resource";
/// The serialized sound-type field (Unity 3.5 layouts).
pub const AUDIO_TYPE_FIELD: &str = "m_Type";
/// The serialized clip name field.
pub const AUDIO_NAME_FIELD: &str = "m_Name";

/// The class ID of `AudioClip` assets.
pub const AUDIO_CLIP_CLASS_ID: i32 = 83;

/// A failure while reading an `AudioClip` field view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioClipViewError {
    /// A struct field could not be looked up.
    FieldLookup {
        /// The field being read.
        field: &'static str,
        /// The lookup failure.
        source: SerializedValueLookupError,
    },
    /// A present field has an unexpected value kind.
    WrongType {
        /// The field being read.
        field: &'static str,
        /// The observed value kind.
        actual: &'static str,
    },
    /// A present streamed-resource descriptor is malformed.
    StreamInfo {
        /// The field being read.
        field: &'static str,
        /// The extraction failure.
        source: StreamedResourceInfoError,
    },
}

impl Display for AudioClipViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(
                    formatter,
                    "could not read AudioClip field '{field}': {source}"
                )
            }
            Self::WrongType { field, actual } => {
                write!(
                    formatter,
                    "AudioClip field '{field}' has kind {actual}, expected bytes, a resource, or a number"
                )
            }
            Self::StreamInfo { field, source } => {
                write!(
                    formatter,
                    "could not read AudioClip field '{field}': {source}"
                )
            }
        }
    }
}

impl Error for AudioClipViewError {}

/// Where `AudioClipExtensions.GetAudioData` would read the clip bytes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioDataSource<'a> {
    /// Present, nonempty inline `m_AudioData`.
    Inline(&'a [u8]),
    /// The `m_Resource` streamed descriptor; content fetching stays caller-side.
    Resource(&'a StreamedResourceInfo),
    /// Neither source applies.
    Empty,
}

/// The fields `AudioClipDecoder` and the audio exporters read from a clip.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AudioClipView {
    /// Raw `m_Name` bytes, when present.
    pub name: Option<Vec<u8>>,
    /// Raw inline `m_AudioData` bytes, when present (possibly empty).
    pub audio_data: Option<Vec<u8>>,
    /// The `m_Resource` streamed descriptor, when present.
    pub resource: Option<StreamedResourceInfo>,
    /// Raw `m_Type` value, when present.
    pub sound_type: Option<i32>,
}

impl AudioClipView {
    /// Reads the export-relevant fields from a decoded `AudioClip` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, AudioClipViewError> {
        Ok(Self {
            name: optional_string(root, AUDIO_NAME_FIELD)?,
            audio_data: optional_bytes(root, AUDIO_DATA_FIELD)?,
            resource: optional_stream(root, AUDIO_RESOURCE_FIELD)?,
            sound_type: optional_i32(root, AUDIO_TYPE_FIELD)?,
        })
    }

    /// `AudioClipExtensions.GetAudioData` source order: nonempty inline bytes,
    /// then the streamed resource, then nothing.
    pub fn audio_data_source(&self) -> AudioDataSource<'_> {
        match &self.audio_data {
            Some(bytes) if !bytes.is_empty() => AudioDataSource::Inline(bytes),
            _ => match &self.resource {
                Some(info) => AudioDataSource::Resource(info),
                None => AudioDataSource::Empty,
            },
        }
    }

    /// `AudioClipExtensions.CheckAssetIntegrity`: inline nonempty data passes, a
    /// streamed resource defers to the caller-supplied collection check, and the
    /// absence of both passes trivially.
    pub fn check_asset_integrity(&self, check_resource: impl FnOnce() -> bool) -> bool {
        match &self.audio_data {
            Some(bytes) if !bytes.is_empty() => true,
            _ => match &self.resource {
                Some(_) => check_resource(),
                None => true,
            },
        }
    }
}

/// `FmodSoundTypeExtensions.ToRawExtension` by numeric discriminant, pinned by the
/// shipped `AssetRipper.SourceGenerated.dll` (`FmodSoundType`: `Acc = 1` ..
/// `MediaFoundation = 29`).
pub const fn fmod_sound_type_raw_extension(value: i32) -> &'static str {
    match value {
        1 => "m4a",
        2 => "aif",
        10 => "it",
        12 => "mod",
        13 => "mp3",
        14 => "ogg",
        17 => "s3m",
        20 => "wav",
        21 => "xm",
        22 => "wav",
        23 => "vag",
        24 => "fsb",
        _ => "audioClip",
    }
}

fn serialized_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "int8",
        SerializedValue::U8(_) => "uint8",
        SerializedValue::I16(_) => "int16",
        SerializedValue::U16(_) => "uint16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "int",
        SerializedValue::U32(_) => "unsigned int",
        SerializedValue::I64(_) => "long",
        SerializedValue::U64(_) => "ulong",
        SerializedValue::F32(_) => "float",
        SerializedValue::F64(_) => "double",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "typeless data",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw data",
    }
}

fn lookup<'a>(
    root: &'a SerializedValue,
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, AudioClipViewError> {
    root.unique_field_in_base_chain(name)
        .map_err(|source| AudioClipViewError::FieldLookup {
            field: name,
            source,
        })
}

fn optional_string(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<Vec<u8>>, AudioClipViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::StringBytes(bytes)) => Ok(Some(bytes.clone())),
        Some(value) => Err(AudioClipViewError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_bytes(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<Vec<u8>>, AudioClipViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::Bytes(bytes)) => Ok(Some(bytes.clone())),
        Some(SerializedValue::Array(values)) => {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(values.len())
                .map_err(|_| AudioClipViewError::WrongType {
                    field: name,
                    actual: "an unallocatable byte array",
                })?;
            for value in values {
                match value {
                    SerializedValue::U8(byte) => bytes.push(*byte),
                    other => {
                        return Err(AudioClipViewError::WrongType {
                            field: name,
                            actual: serialized_kind(other),
                        });
                    }
                }
            }
            Ok(Some(bytes))
        }
        Some(value) => Err(AudioClipViewError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_i32(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<i32>, AudioClipViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::I32(value)) => Ok(Some(*value)),
        Some(SerializedValue::U32(value)) => Ok(Some(*value as i32)),
        Some(value) => Err(AudioClipViewError::WrongType {
            field: name,
            actual: serialized_kind(value),
        }),
    }
}

fn optional_stream(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<StreamedResourceInfo>, AudioClipViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(value) => {
            let info = extract_streamed_resource_info(value).map_err(|source| {
                AudioClipViewError::StreamInfo {
                    field: name,
                    source,
                }
            })?;
            Ok(Some(info))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::object::SerializedFieldValue;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn string_value(text: &str) -> SerializedValue {
        SerializedValue::StringBytes(text.as_bytes().to_vec())
    }

    /// Differential fixture produced by the isolated dotnet probe
    /// `Temp/opencode/audioprops` against the shipped
    /// `AssetRipper.SourceGenerated.dll`.
    #[test]
    fn generated_api_fixture_supports_the_field_name_mapping() {
        let fixture = include_str!("audio_generated_api_fixture.txt");
        let mut relied_upon = std::collections::HashSet::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "APROP" => {
                    relied_upon.insert(parts[1].to_owned());
                }
                "AGATE" if parts.len() == 2 => {
                    relied_upon.insert(parts[1].to_owned());
                }
                "AGATE" if parts.len() == 4 => {
                    // Per-version gate values: 3.5 carries inline data and a
                    // type; 5.0+ carries a resource instead.
                    let script_shape = (parts[1], parts[2], parts[3]);
                    match script_shape {
                        ("3.5.0a0", "Has_AudioData", "True")
                        | ("3.5.0a0", "Has_Resource", "False")
                        | ("3.5.0a0", "Has_Type", "True")
                        | ("3.5.0a0", "Has_Format", "True")
                        | ("3.5.0a0", "Has_CompressionFormat", "False") => {}
                        (version, "Has_AudioData", "False")
                        | (version, "Has_Resource", "True")
                        | (version, "Has_Type", "False")
                        | (version, "Has_Format", "False")
                        | (version, "Has_CompressionFormat", "True")
                            if version != "3.5.0a0" => {}
                        _ => panic!("unexpected gate row {line}"),
                    }
                }
                "ACLASS" | "FSF" => {}
                "FST" => {
                    // Spot-check the discriminants behind the extension table.
                    match parts[1] {
                        "Unknown" => assert_eq!(parts[2], "0", "{line}"),
                        "Oggvorbis" => assert_eq!(parts[2], "14", "{line}"),
                        "Wav" => assert_eq!(parts[2], "20", "{line}"),
                        "Xma" => assert_eq!(parts[2], "22", "{line}"),
                        "Audioqueue" => assert_eq!(parts[2], "24", "{line}"),
                        _ => {}
                    }
                }
                _ => panic!("unexpected fixture row {line}"),
            }
        }
        for relied in [
            "Byte[] AudioData",
            "IStreamedResource Resource",
            "Int32 Type",
            "Int32 Format",
            "Utf8String Name_R",
            "Has_AudioData",
            "Has_Resource",
            "Has_Type",
        ] {
            assert!(relied_upon.contains(relied), "missing {relied}");
        }
    }

    #[test]
    fn sound_type_numbers_map_to_snapshot_extensions() {
        assert_eq!(fmod_sound_type_raw_extension(1), "m4a");
        assert_eq!(fmod_sound_type_raw_extension(2), "aif");
        assert_eq!(fmod_sound_type_raw_extension(10), "it");
        assert_eq!(fmod_sound_type_raw_extension(12), "mod");
        assert_eq!(fmod_sound_type_raw_extension(13), "mp3");
        assert_eq!(fmod_sound_type_raw_extension(14), "ogg");
        assert_eq!(fmod_sound_type_raw_extension(17), "s3m");
        assert_eq!(fmod_sound_type_raw_extension(20), "wav");
        assert_eq!(fmod_sound_type_raw_extension(21), "xm");
        assert_eq!(fmod_sound_type_raw_extension(22), "wav");
        assert_eq!(fmod_sound_type_raw_extension(23), "vag");
        assert_eq!(fmod_sound_type_raw_extension(24), "fsb");
        assert_eq!(fmod_sound_type_raw_extension(0), "audioClip");
        assert_eq!(fmod_sound_type_raw_extension(28), "audioClip");
        assert_eq!(fmod_sound_type_raw_extension(-1), "audioClip");
    }

    #[test]
    fn inline_bytes_win_over_resource_and_emptiness() {
        let inline = SerializedValue::Struct(vec![
            field("m_Name", string_value("clip")),
            field("m_AudioData", SerializedValue::Bytes(vec![1, 2, 3])),
            field("m_Type", SerializedValue::I32(20)),
        ]);
        let view = AudioClipView::from_value(&inline).unwrap();
        assert_eq!(view.name, Some(b"clip".to_vec()));
        assert_eq!(view.sound_type, Some(20));
        assert_eq!(
            view.audio_data_source(),
            AudioDataSource::Inline(&[1, 2, 3])
        );
        assert!(view.check_asset_integrity(|| panic!("must not check")));
    }

    #[test]
    fn empty_inline_data_falls_through_to_resource_then_nothing() {
        let empty = SerializedValue::Struct(vec![field(
            "m_AudioData",
            SerializedValue::Bytes(Vec::new()),
        )]);
        let view = AudioClipView::from_value(&empty).unwrap();
        assert_eq!(view.audio_data_source(), AudioDataSource::Empty);
        assert!(view.check_asset_integrity(|| panic!("must not check")));
    }

    #[test]
    fn integrity_defers_to_the_resource_check_only_when_needed() {
        // No data and no resource passes trivially without consulting the closure.
        let bare = SerializedValue::Struct(Vec::new());
        let view = AudioClipView::from_value(&bare).unwrap();
        assert_eq!(view.audio_data_source(), AudioDataSource::Empty);
        assert!(view.check_asset_integrity(|| panic!("must not check")));
    }

    #[test]
    fn present_resource_selects_the_streamed_source() {
        let root = SerializedValue::Struct(vec![field(
            "m_Resource",
            SerializedValue::Struct(vec![
                field("path", string_value("audio.resS")),
                field("offset", SerializedValue::U64(8)),
                field("size", SerializedValue::U32(4)),
            ]),
        )]);
        let view = AudioClipView::from_value(&root).unwrap();
        match view.audio_data_source() {
            AudioDataSource::Resource(info) => {
                assert_eq!(info.path, "audio.resS");
                assert_eq!(info.offset, 8);
                assert_eq!(info.size, 4);
            }
            other => panic!("unexpected source {other:?}"),
        }
        assert!(view.check_asset_integrity(|| true));
        assert!(!view.check_asset_integrity(|| false));
    }

    #[test]
    fn mistyped_fields_are_explicit_errors() {
        let root = SerializedValue::Struct(vec![field("m_Type", string_value("wav"))]);
        assert_eq!(
            AudioClipView::from_value(&root),
            Err(AudioClipViewError::WrongType {
                field: "m_Type",
                actual: "string"
            })
        );
        let root = SerializedValue::Struct(vec![field("m_AudioData", SerializedValue::I32(4))]);
        assert_eq!(
            AudioClipView::from_value(&root),
            Err(AudioClipViewError::WrongType {
                field: "m_AudioData",
                actual: "int"
            })
        );
    }
}
