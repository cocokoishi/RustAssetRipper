//! Class-83 `AudioClip` field view and audio-data source resolution.
//!
//! Ports the field reads behind `AudioClipExtensions.GetAudioData` and
//! `CheckAssetIntegrity`: inline `m_AudioData` bytes win when present and nonempty,
//! otherwise the `m_Resource` streamed descriptor supplies the content, otherwise the
//! clip has no data. Content fetching and resource integrity checks need the asset
//! collection and stay caller-side; this module resolves only which source applies.
//!
//! Also ports the gated discriminants behind `GetSoundType`/`GetSoundFormat`: the
//! `m_Type`/`m_Format` Int32s surface as `Some` only when the field is present
//! (`Has_Type`/`Has_Format`); the int-backed enum casts are identity on the stored
//! value, so callers map the discriminant with the extension tables. `GetSoundFormat`
//! has no callers in the frozen baseline; it is pinned for surface completeness.
//!
//! Deliberately open: the commented-out `StreamingInfo`/`LoadType` branches in
//! `AudioClipExtensions` are not live behavior (commented in the source) and stay
//! unported; FSB-container decoding needs the external `Fmod5Sharp` owner (see
//! `export::audio_export`); numeric `FmodSoundFormat` discriminants have no
//! `ToRawExtension` in the baseline, so no format-extension table is invented here.
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
/// The serialized sound-format field (Unity 3.5 layouts, behind `GetSoundFormat`).
pub const AUDIO_FORMAT_FIELD: &str = "m_Format";
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
    /// Raw `m_Format` value, when present.
    pub format: Option<i32>,
}

impl AudioClipView {
    /// Reads the export-relevant fields from a decoded `AudioClip` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, AudioClipViewError> {
        Ok(Self {
            name: optional_string(root, AUDIO_NAME_FIELD)?,
            audio_data: optional_bytes(root, AUDIO_DATA_FIELD)?,
            resource: optional_stream(root, AUDIO_RESOURCE_FIELD)?,
            sound_type: optional_i32(root, AUDIO_TYPE_FIELD)?,
            format: optional_i32(root, AUDIO_FORMAT_FIELD)?,
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

    /// `AudioClipExtensions.GetSoundType`: the `FmodSoundType` discriminant, present
    /// only when `Has_Type` holds. Field presence is the gate, so a stored value
    /// surfaces as-is (the int-backed cast is identity) and an absent field is `None`.
    pub fn sound_type_discriminant(&self) -> Option<i32> {
        self.sound_type
    }

    /// `AudioClipExtensions.GetSoundFormat`: the `FmodSoundFormat` discriminant,
    /// present only when `Has_Format` holds, with the same presence-as-gate rule.
    pub fn sound_format_discriminant(&self) -> Option<i32> {
        self.format
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
        let mut sound_types: Vec<(&str, i32)> = Vec::new();
        let mut sound_formats: Vec<(&str, i32)> = Vec::new();
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
                "ACLASS" => {}
                "FSF" => {
                    sound_formats.push((parts[1], parts[2].parse().expect("a value")));
                }
                "FST" => {
                    sound_types.push((parts[1], parts[2].parse().expect("a value")));
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
        // Every shipped `FmodSoundType` member resolves through the numeric extension
        // table; expectations are name-based (read off the C# switch) while the kernel
        // is value-based, so the fixture's name|value rows join the two sides.
        let expected_types: Vec<(&str, i32, &str)> = vec![
            ("Unknown", 0, "audioClip"),
            ("Acc", 1, "m4a"),
            ("Aiff", 2, "aif"),
            ("Asf", 3, "audioClip"),
            ("At3", 4, "audioClip"),
            ("Cdda", 5, "audioClip"),
            ("Dls", 6, "audioClip"),
            ("Flac", 7, "audioClip"),
            ("Fsb", 8, "audioClip"),
            ("Gcadpcm", 9, "audioClip"),
            ("It", 10, "it"),
            ("Midi", 11, "audioClip"),
            ("Mod", 12, "mod"),
            ("Mpeg", 13, "mp3"),
            ("Oggvorbis", 14, "ogg"),
            ("Playlist", 15, "audioClip"),
            ("Raw", 16, "audioClip"),
            ("S3m", 17, "s3m"),
            ("Sf2", 18, "audioClip"),
            ("User", 19, "audioClip"),
            ("Wav", 20, "wav"),
            ("Xm", 21, "xm"),
            ("Xma", 22, "wav"),
            ("Vag", 23, "vag"),
            ("Audioqueue", 24, "fsb"),
            ("Xwma", 25, "audioClip"),
            ("Bcwav", 26, "audioClip"),
            ("At9", 27, "audioClip"),
            ("Vorbis", 28, "audioClip"),
            ("MediaFoundation", 29, "audioClip"),
            ("Max", 30, "audioClip"),
            ("Forceint", 65536, "audioClip"),
        ];
        assert_eq!(
            sound_types,
            expected_types
                .iter()
                .map(|(name, value, _)| (*name, *value))
                .collect::<Vec<_>>(),
            "the shipped FmodSoundType layout"
        );
        for (name, value, extension) in &expected_types {
            assert_eq!(
                fmod_sound_type_raw_extension(*value),
                *extension,
                "extension for {name}"
            );
        }
        // The `FmodSoundFormat` discriminants behind `sound_format_discriminant` run
        // 0..=16 with the same 65536 force-int sentinel; the baseline defines no
        // `ToRawExtension` for them, so the table pins values only.
        assert_eq!(
            sound_formats,
            vec![
                ("None", 0),
                ("Pcm8", 1),
                ("Pcm16", 2),
                ("Pcm24", 3),
                ("Pcm32", 4),
                ("Pcmfloat", 5),
                ("Gcadpcm", 6),
                ("Imaadpcm", 7),
                ("Vag", 8),
                ("Hevag", 9),
                ("Xma", 10),
                ("Mpeg", 11),
                ("Celt", 12),
                ("At9", 13),
                ("Xwma", 14),
                ("Vorbis", 15),
                ("Max", 16),
                ("Forceint", 65536),
            ]
        );
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
    fn gated_discriminants_follow_field_presence() {
        // Unity 3.5 layouts carry m_Type/m_Format; the discriminants surface as-is.
        let legacy = SerializedValue::Struct(vec![
            field("m_Type", SerializedValue::I32(14)),
            field("m_Format", SerializedValue::I32(15)),
        ]);
        let view = AudioClipView::from_value(&legacy).unwrap();
        assert_eq!(view.sound_type_discriminant(), Some(14));
        assert_eq!(view.sound_format_discriminant(), Some(15));
        assert_eq!(view.format, Some(15));
        // Modern layouts carry neither: both gates read absent.
        let modern = SerializedValue::Struct(Vec::new());
        let view = AudioClipView::from_value(&modern).unwrap();
        assert_eq!(view.sound_type_discriminant(), None);
        assert_eq!(view.sound_format_discriminant(), None);
        // A mistyped m_Format is an explicit error, like m_Type.
        let root = SerializedValue::Struct(vec![field("m_Format", string_value("vorbis"))]);
        assert_eq!(
            AudioClipView::from_value(&root),
            Err(AudioClipViewError::WrongType {
                field: "m_Format",
                actual: "string"
            })
        );
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

    #[test]
    fn empty_inline_data_still_prefers_the_resource() {
        // Empty inline bytes fall through to the resource for both source selection
        // and integrity, exactly like a missing field.
        let root = SerializedValue::Struct(vec![
            field("m_AudioData", SerializedValue::Bytes(Vec::new())),
            field(
                "m_Resource",
                SerializedValue::Struct(vec![
                    field("path", string_value("audio.resS")),
                    field("offset", SerializedValue::U64(8)),
                    field("size", SerializedValue::U32(4)),
                ]),
            ),
        ]);
        let view = AudioClipView::from_value(&root).unwrap();
        match view.audio_data_source() {
            AudioDataSource::Resource(info) => assert_eq!(info.path, "audio.resS"),
            other => panic!("unexpected source {other:?}"),
        }
        assert!(!view.check_asset_integrity(|| false));
        assert!(view.check_asset_integrity(|| true));
    }

    #[test]
    fn reader_arms_coerce_arrays_wrap_ints_and_reject_malformed_streams() {
        // Byte arrays decode element-wise; a non-byte element is an explicit error.
        let root = SerializedValue::Struct(vec![field(
            "m_AudioData",
            SerializedValue::Array(vec![SerializedValue::U8(9)]),
        )]);
        assert_eq!(
            AudioClipView::from_value(&root).unwrap().audio_data,
            Some(vec![9])
        );
        let root = SerializedValue::Struct(vec![field(
            "m_AudioData",
            SerializedValue::Array(vec![SerializedValue::I32(9)]),
        )]);
        assert_eq!(
            AudioClipView::from_value(&root),
            Err(AudioClipViewError::WrongType {
                field: "m_AudioData",
                actual: "int"
            })
        );
        // Unsigned bit patterns reinterpret as signed, matching the unchecked cast.
        let root = SerializedValue::Struct(vec![field("m_Type", SerializedValue::U32(u32::MAX))]);
        assert_eq!(
            AudioClipView::from_value(&root).unwrap().sound_type,
            Some(-1)
        );
        // A present-but-malformed resource descriptor is a stream error, not silence.
        let root = SerializedValue::Struct(vec![field("m_Resource", SerializedValue::I32(1))]);
        assert!(matches!(
            AudioClipView::from_value(&root),
            Err(AudioClipViewError::StreamInfo {
                field: "m_Resource",
                ..
            })
        ));
        // The failure messages name the field and the observed kind.
        assert_eq!(
            AudioClipViewError::WrongType {
                field: "m_Type",
                actual: "string"
            }
            .to_string(),
            "AudioClip field 'm_Type' has kind string, expected bytes, a resource, or a number"
        );
    }
}
