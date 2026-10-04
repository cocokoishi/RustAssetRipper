//! Audio export decisions ported from the frozen baseline.
//!
//! The mode enum itself lives in `settings`; everything here is control flow and
//! byte-level dispatch from snapshot text; the actual codec work (`Fmod5Sharp` rebuilds,
//! `AudioConverter.OggToWav`, streamed-content reads) stays with its external owners:
//!
//! - `AudioExportFormat` has four modes, but only `Default` and `PreferWav` are supported
//!   export formats.
//! - `AudioClipExporter.TryCreateCollection` admits only audio clips, warns and refuses
//!   when decoding fails or yields nothing, and substitutes `wav` for an `ogg` decode
//!   under `PreferWav`. Decoding runs again at export time, so success at creation does
//!   not guarantee success at export.
//! - `AudioClipDecoder.TryDecode` dispatches on magic bytes: empty data fails, typed clips
//!   pass through with the sound-type extension, `FSB5` defers to `Fmod5Sharp` (open), and
//!   the tracker magics (`IMPM`, `Extended Module: `, `SCRM` at 156, seven `MOD` magics at
//!   1080) pass through with fixed extensions. Anything else fails with a printable/hex
//!   diagnostic built by exact fill-and-copy rules.
//! - Both raw-extension tables are pinned by snapshot enum name; the generated numeric
//!   values behind the names stay open.
//! - `NativeAudioExporter` refuses empty payloads with a warning and otherwise writes raw
//!   bytes; its collection extension follows the sound type when present and the
//!   compression format otherwise.
//! - `YamlAudioExportCollection.ExportInner` stages streamed content into a `.resS`
//!   sidecar, rewrites the resource reference to a relative path, exports, then restores
//!   the original reference; missing content zeroes the reference instead. The relative
//!   path computation stays caller-side; only the sidecar path rule and the
//!   save/rewrite/restore discipline are pinned here.

use super::settings::AudioExportFormat;

/// `AudioClipExporter.IsSupportedExportFormat`: only `Default` and `PreferWav`.
pub const fn is_supported_export_format(format: AudioExportFormat) -> bool {
    matches!(
        format,
        AudioExportFormat::Default | AudioExportFormat::PreferWav
    )
}

/// How raw audio bytes classify in `AudioClipDecoder.TryDecode`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioDataKind {
    /// No bytes at all; decoding fails before any magic check.
    Empty,
    /// `FSB5` at 0: deferred to the external `Fmod5Sharp` path.
    Fsb5,
    /// `IMPM` at 0: Impulse Tracker, passed through as `it`.
    Impm,
    /// `Extended Module: ` at 0: passed through as `xm`.
    ExtendedModule,
    /// `SCRM` at 156: Scream Tracker 3, passed through as `s3m`.
    S3m,
    /// Any of the seven MOD magics at 1080: passed through as `mod`.
    Mod,
    /// None of the magics matched; decoding fails with a diagnostic.
    Unknown,
}

/// The seven MOD magic words, all checked at offset 1080.
pub const MOD_MAGICS: &[&[u8]] = &[
    b"M.K.", b"M!K!", b"FLT4", b"FLT8", b"4CHN", b"6CHN", b"8CHN",
];

/// Byte-level dispatch of `AudioClipDecoder.TryDecode` before the external FSB path.
///
/// Short buffers simply miss every magic (the length guard replicates the source bounds
/// check), and typed-clip handling stays caller-side because `Has_Type` needs the asset.
pub fn classify_audio_data(data: &[u8]) -> AudioDataKind {
    if data.is_empty() {
        return AudioDataKind::Empty;
    }
    if check_magic(data, b"FSB5", 0) {
        AudioDataKind::Fsb5
    } else if check_magic(data, b"IMPM", 0) {
        AudioDataKind::Impm
    } else if check_magic(data, b"Extended Module: ", 0) {
        AudioDataKind::ExtendedModule
    } else if check_magic(data, b"SCRM", 156) {
        AudioDataKind::S3m
    } else if MOD_MAGICS
        .iter()
        .any(|magic| check_magic(data, magic, 1080))
    {
        AudioDataKind::Mod
    } else {
        AudioDataKind::Unknown
    }
}

/// `CheckMagic`: the buffer must cover the magic at the offset and match exactly.
fn check_magic(data: &[u8], magic: &[u8], start: usize) -> bool {
    data.len() >= magic.len() + start && data[start..start + magic.len()] == *magic
}

/// The fixed passthrough extension for each tracker kind.
pub const fn tracker_extension(kind: AudioDataKind) -> Option<&'static str> {
    match kind {
        AudioDataKind::Impm => Some("it"),
        AudioDataKind::ExtendedModule => Some("xm"),
        AudioDataKind::S3m => Some("s3m"),
        AudioDataKind::Mod => Some("mod"),
        _ => None,
    }
}

/// `AudioCompressionFormatExtentions.ToRawExtension` by snapshot enum name.
pub fn compression_format_raw_extension(name: &str) -> &'static str {
    match name {
        "PCM" | "Vorbis" | "ADPCM" | "MP3" | "GCADPCM" => "fsb",
        "VAG" | "HEVAG" => "vag",
        "XMA" => "wav",
        "AAC" => "m4a",
        "ATRAC9" => "at9",
        _ => "audioClip",
    }
}

/// `FmodSoundTypeExtensions.ToRawExtension` by snapshot enum name.
pub fn sound_type_raw_extension(name: &str) -> &'static str {
    match name {
        "Acc" => "m4a",
        "Aiff" => "aif",
        "It" => "it",
        "Mod" => "mod",
        "Mpeg" => "mp3",
        "Oggvorbis" => "ogg",
        "S3m" => "s3m",
        "Wav" => "wav",
        "Xm" => "xm",
        "Xma" => "wav",
        "Vag" => "vag",
        "Audioqueue" => "fsb",
        _ => "audioClip",
    }
}

/// The four printable characters of the unknown-audio diagnostic.
///
/// `CopyPrintable`: fills with `'O'`, then copies ASCII printable bytes except tab.
pub fn diagnostic_printable(data: &[u8]) -> [char; 4] {
    let mut out = ['O'; 4];
    for (index, character) in out.iter_mut().enumerate() {
        if index >= data.len() {
            break;
        }
        let candidate = data[index] as char;
        // Mirrors `IsAsciiPrintable` (space through tilde); tab is already outside that
        // window, and the explicit exclusion is preserved rather than optimized away.
        if (' '..='~').contains(&candidate) && candidate != '\t' {
            *character = candidate;
        }
    }
    out
}

/// The eight hex characters of the unknown-audio diagnostic.
///
/// `CopyHex`: fills with `'0'`, then renders up to four bytes as uppercase nibbles.
pub fn diagnostic_hex(data: &[u8]) -> [char; 8] {
    const LOOKUP: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = ['0'; 8];
    for index in 0..4 {
        if index >= data.len() {
            break;
        }
        out[2 * index] = LOOKUP[(data[index] >> 4) as usize] as char;
        out[2 * index + 1] = LOOKUP[(data[index] & 0x0F) as usize] as char;
    }
    out
}

/// Facts `AudioClipExporter.TryCreateCollection` needs from the decode attempt.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AudioDecodeFact<'a> {
    /// The asset is an audio clip at all.
    pub is_audio_clip: bool,
    /// `AudioClipDecoder.TryDecode` succeeded.
    pub decode_succeeded: bool,
    /// The decoded payload is empty.
    pub decoded_is_empty: bool,
    /// The decoder-reported file extension.
    pub decoded_extension: &'a str,
    /// The configured `AudioExportFormat`.
    pub format: AudioExportFormat,
}

/// Why `TryCreateCollection` refused an audio asset.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioRefusal {
    /// Not an audio clip.
    NotAudioClip,
    /// Decoding failed; the warning carries the decoder message.
    DecodeFailed,
    /// Decoding yielded no bytes.
    DecodedEmpty,
}

/// `AudioClipExporter.TryCreateCollection`: non-clips and failed or empty decodes are
/// refused; `PreferWav` substitutes `wav` for an `ogg` decode, everything else keeps the
/// decoded extension.
pub fn plan_audio_clip_collection(fact: AudioDecodeFact<'_>) -> Result<String, AudioRefusal> {
    if !fact.is_audio_clip {
        return Err(AudioRefusal::NotAudioClip);
    }
    if !fact.decode_succeeded {
        return Err(AudioRefusal::DecodeFailed);
    }
    if fact.decoded_is_empty {
        return Err(AudioRefusal::DecodedEmpty);
    }
    if fact.format == AudioExportFormat::PreferWav && fact.decoded_extension == "ogg" {
        Ok("wav".to_owned())
    } else {
        Ok(fact.decoded_extension.to_owned())
    }
}

/// Whether `AudioClipExportCollection.ExportInner` converts the decoded payload.
///
/// Conversion happens only when the decoder reported `ogg` while the collection was
/// planned with a `wav` extension.
pub const fn converts_ogg_to_wav(decoded_extension: &str, collection_extension: &str) -> bool {
    matches!(
        (
            decoded_extension.as_bytes(),
            collection_extension.as_bytes()
        ),
        (b"ogg", b"wav")
    )
}

/// `NativeAudioExporter.Export`: empty payloads warn and fail; anything else is written.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NativeAudioExport {
    /// Write the raw payload bytes.
    WriteRaw,
    /// Log a warning naming the asset and report failure.
    WarnAndFail,
}

/// Empty data fails; the length check is `data.Length == 0`, so only truly empty buffers
/// refuse.
pub const fn plan_native_audio_export(data_is_empty: bool) -> NativeAudioExport {
    if data_is_empty {
        NativeAudioExport::WarnAndFail
    } else {
        NativeAudioExport::WriteRaw
    }
}

/// `NativeAudioExportCollection.GetExportExtension`: the sound type wins when the asset
/// exposes one, otherwise the compression format decides. Names follow the snapshot
/// enums; resolution of the names themselves stays caller-side.
pub fn native_audio_extension(
    has_sound_type: bool,
    sound_type_name: &str,
    compression_format_name: &str,
) -> &'static str {
    if has_sound_type {
        sound_type_raw_extension(sound_type_name)
    } else {
        compression_format_raw_extension(compression_format_name)
    }
}

/// The `.resS` staging discipline of `YamlAudioExportCollection.ExportInner`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamedResourcePlan {
    /// No resource: export the asset directly with no save or restore.
    NoResource,
    /// Content resolved: save the reference, write `{filePath}.resS`, rewrite the
    /// reference to the caller-computed relative path, export, then restore.
    StageSidecar {
        /// `{filePath}.resS`.
        sidecar_path: String,
    },
    /// Content missing: zero the reference, export, then restore the originals.
    ZeroAndRestore,
}

/// Plan the `.resS` staging from the resource presence and content outcome.
///
/// Only the sidecar path rule lives here; relative-path computation and the actual
/// save/restore stay caller-side.
pub fn plan_streamed_resource(
    has_resource: bool,
    content_resolved: bool,
    file_path: &str,
) -> StreamedResourcePlan {
    if !has_resource {
        StreamedResourcePlan::NoResource
    } else if content_resolved {
        StreamedResourcePlan::StageSidecar {
            sidecar_path: format!("{file_path}.resS"),
        }
    } else {
        StreamedResourcePlan::ZeroAndRestore
    }
}

#[cfg(test)]
mod tests {
    use super::{
        check_magic, classify_audio_data, compression_format_raw_extension, converts_ogg_to_wav,
        diagnostic_hex, diagnostic_printable, is_supported_export_format, native_audio_extension,
        plan_audio_clip_collection, plan_native_audio_export, plan_streamed_resource,
        sound_type_raw_extension, tracker_extension, AudioDataKind, AudioDecodeFact, AudioRefusal,
        NativeAudioExport, StreamedResourcePlan, MOD_MAGICS,
    };
    use crate::export::settings::AudioExportFormat;

    #[test]
    fn export_format_support_matches_is_supported_export_format() {
        assert!(is_supported_export_format(AudioExportFormat::Default));
        assert!(is_supported_export_format(AudioExportFormat::PreferWav));
        assert!(!is_supported_export_format(AudioExportFormat::Yaml));
        assert!(!is_supported_export_format(AudioExportFormat::Native));
    }

    #[test]
    fn magic_dispatch_matches_decoder_order_and_bounds() {
        assert_eq!(classify_audio_data(&[]), AudioDataKind::Empty);
        assert_eq!(classify_audio_data(b"FSB5...."), AudioDataKind::Fsb5);
        assert_eq!(classify_audio_data(b"IMPM...."), AudioDataKind::Impm);
        assert_eq!(
            classify_audio_data(b"Extended Module: .."),
            AudioDataKind::ExtendedModule
        );
        // Short buffers miss every magic and fall through to unknown.
        assert_eq!(classify_audio_data(b"FSB"), AudioDataKind::Unknown);
        assert_eq!(classify_audio_data(b"IMPM"), AudioDataKind::Impm);
        assert_eq!(classify_audio_data(b"ABCD"), AudioDataKind::Unknown);
        let mut s3m = vec![0u8; 160];
        s3m[156..160].copy_from_slice(b"SCRM");
        assert_eq!(classify_audio_data(&s3m), AudioDataKind::S3m);
        assert_eq!(classify_audio_data(&[0u8; 160]), AudioDataKind::Unknown);
        for magic in MOD_MAGICS {
            let mut mod_data = vec![0u8; 1084];
            mod_data[1080..1084].copy_from_slice(magic);
            assert_eq!(
                classify_audio_data(&mod_data),
                AudioDataKind::Mod,
                "{magic:?}"
            );
        }
        assert_eq!(
            classify_audio_data(&vec![0u8; 1084]),
            AudioDataKind::Unknown
        );
        // Offsets are exact: SCRM at 0 or MOD words at 0 do not match.
        assert_eq!(classify_audio_data(b"SCRM"), AudioDataKind::Unknown);
        assert_eq!(classify_audio_data(b"M.K."), AudioDataKind::Unknown);
        assert!(!check_magic(b"AB", b"ABC", 0));
        assert_eq!(tracker_extension(AudioDataKind::Impm), Some("it"));
        assert_eq!(tracker_extension(AudioDataKind::ExtendedModule), Some("xm"));
        assert_eq!(tracker_extension(AudioDataKind::S3m), Some("s3m"));
        assert_eq!(tracker_extension(AudioDataKind::Mod), Some("mod"));
        assert_eq!(tracker_extension(AudioDataKind::Fsb5), None);
        assert_eq!(tracker_extension(AudioDataKind::Unknown), None);
    }

    #[test]
    fn raw_extension_tables_match_snapshot_fallbacks() {
        for name in ["PCM", "Vorbis", "ADPCM", "MP3", "GCADPCM"] {
            assert_eq!(compression_format_raw_extension(name), "fsb", "{name}");
        }
        assert_eq!(compression_format_raw_extension("VAG"), "vag");
        assert_eq!(compression_format_raw_extension("HEVAG"), "vag");
        assert_eq!(compression_format_raw_extension("XMA"), "wav");
        assert_eq!(compression_format_raw_extension("AAC"), "m4a");
        assert_eq!(compression_format_raw_extension("ATRAC9"), "at9");
        assert_eq!(compression_format_raw_extension("Whatever"), "audioClip");
        for (name, expected) in [
            ("Acc", "m4a"),
            ("Aiff", "aif"),
            ("It", "it"),
            ("Mod", "mod"),
            ("Mpeg", "mp3"),
            ("Oggvorbis", "ogg"),
            ("S3m", "s3m"),
            ("Wav", "wav"),
            ("Xm", "xm"),
            ("Xma", "wav"),
            ("Vag", "vag"),
            ("Audioqueue", "fsb"),
        ] {
            assert_eq!(sound_type_raw_extension(name), expected, "{name}");
        }
        assert_eq!(sound_type_raw_extension("Whatever"), "audioClip");
    }

    #[test]
    fn diagnostics_fill_then_copy_exactly() {
        assert_eq!(diagnostic_printable(b"ABCD"), ['A', 'B', 'C', 'D']);
        // Control bytes and DEL stay filled; tab counts as unprintable here.
        assert_eq!(
            diagnostic_printable(b"\x01\x02\t\x7f"),
            ['O', 'O', 'O', 'O']
        );
        assert_eq!(diagnostic_printable(b"A"), ['A', 'O', 'O', 'O']);
        assert_eq!(
            diagnostic_hex(&[0xDE, 0xAD]),
            ['D', 'E', 'A', 'D', '0', '0', '0', '0']
        );
        assert_eq!(diagnostic_hex(&[]), ['0'; 8]);
    }

    #[test]
    fn clip_collection_plans_match_try_create_matrix() {
        fn decode(extension: &str, format: AudioExportFormat) -> AudioDecodeFact<'_> {
            AudioDecodeFact {
                is_audio_clip: true,
                decode_succeeded: true,
                decoded_is_empty: false,
                decoded_extension: extension,
                format,
            }
        }
        // PreferWav substitutes wav for ogg only.
        assert_eq!(
            plan_audio_clip_collection(decode("ogg", AudioExportFormat::PreferWav)),
            Ok("wav".to_owned())
        );
        assert_eq!(
            plan_audio_clip_collection(decode("mp3", AudioExportFormat::PreferWav)),
            Ok("mp3".to_owned())
        );
        assert_eq!(
            plan_audio_clip_collection(decode("ogg", AudioExportFormat::Default)),
            Ok("ogg".to_owned())
        );
        // Refusals follow the non-clip, failed-decode, empty-decode order.
        assert_eq!(
            plan_audio_clip_collection(AudioDecodeFact {
                is_audio_clip: false,
                ..decode("ogg", AudioExportFormat::Default)
            }),
            Err(AudioRefusal::NotAudioClip)
        );
        assert_eq!(
            plan_audio_clip_collection(AudioDecodeFact {
                decode_succeeded: false,
                ..decode("ogg", AudioExportFormat::Default)
            }),
            Err(AudioRefusal::DecodeFailed)
        );
        assert_eq!(
            plan_audio_clip_collection(AudioDecodeFact {
                decoded_is_empty: true,
                ..decode("ogg", AudioExportFormat::Default)
            }),
            Err(AudioRefusal::DecodedEmpty)
        );
        // Export-time conversion mirrors the collection-planning substitution.
        assert!(converts_ogg_to_wav("ogg", "wav"));
        assert!(!converts_ogg_to_wav("ogg", "ogg"));
        assert!(!converts_ogg_to_wav("mp3", "wav"));
        // Native exports refuse only empty payloads.
        assert_eq!(
            plan_native_audio_export(true),
            NativeAudioExport::WarnAndFail
        );
        assert_eq!(plan_native_audio_export(false), NativeAudioExport::WriteRaw);
        // The native extension prefers the sound type, else the compression format.
        assert_eq!(native_audio_extension(true, "Oggvorbis", "PCM"), "ogg");
        assert_eq!(native_audio_extension(false, "Oggvorbis", "PCM"), "fsb");
        assert_eq!(
            native_audio_extension(false, "Oggvorbis", "Nope"),
            "audioClip"
        );
    }

    #[test]
    fn streamed_resource_plans_match_export_inner_discipline() {
        assert_eq!(
            plan_streamed_resource(false, false, "clip.audioclip"),
            StreamedResourcePlan::NoResource
        );
        assert_eq!(
            plan_streamed_resource(false, true, "clip.audioclip"),
            StreamedResourcePlan::NoResource
        );
        assert_eq!(
            plan_streamed_resource(true, true, "clip.audioclip"),
            StreamedResourcePlan::StageSidecar {
                sidecar_path: "clip.audioclip.resS".to_owned(),
            }
        );
        assert_eq!(
            plan_streamed_resource(true, false, "clip.audioclip"),
            StreamedResourcePlan::ZeroAndRestore
        );
    }
}
