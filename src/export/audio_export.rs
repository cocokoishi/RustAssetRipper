//! Audio export decisions ported from the frozen baseline.
//!
//! The mode enum itself lives in `settings`; this module owns byte-level dispatch and
//! bounded FSB5 Vorbis reconstruction and Ogg-to-WAV conversion. Streamed-content reads
//! remain with their respective owners:
//!
//! - `AudioExportFormat` has four modes, but only `Default` and `PreferWav` are supported
//!   export formats.
//! - `AudioClipExporter.TryCreateCollection` admits only audio clips, warns and refuses
//!   when decoding fails or yields nothing, and substitutes `wav` for an `ogg` decode
//!   under `PreferWav`. Decoding runs again at export time, so success at creation does
//!   not guarantee success at export.
//! - `AudioClipDecoder.TryDecode` dispatches on magic bytes: empty data fails, typed clips
//!   pass through with the sound-type extension, `FSB5` reconstructs supported single-stream
//!   Vorbis as Ogg, and
//!   the tracker magics (`IMPM`, `Extended Module: `, `SCRM` at 156, seven `MOD` magics at
//!   1080) pass through with fixed extensions. Anything else fails with a printable/hex
//!   diagnostic built by exact fill-and-copy rules.
//! - Both raw-extension tables are pinned by snapshot enum name; the generated numeric
//!   values behind the names stay open.
//! - `NativeAudioExporter` refuses empty payloads with a warning and otherwise writes raw
//!   bytes; its collection extension follows the sound type when present and the
//!   compression format otherwise. Both exporter warning texts are pinned below.
//! - `AudioConverter.OggToWav` emits IEEE-float RIFF/WAV from Vorbis Ogg. The converter
//!   uses the encoded stream's final granule position to trim decoder overlap samples and
//!   enforces the caller's output cap.
//! - `YamlAudioExportCollection.ExportInner` stages streamed content into a `.resS`
//!   sidecar, rewrites the resource reference to a relative path, exports, then restores
//!   the original reference; missing content zeroes the reference instead. The relative
//!   path computation stays caller-side; only the sidecar path rule and the
//!   save/rewrite/restore discipline are pinned here.

use std::io::{self, Cursor, Read, Write};

use super::settings::AudioExportFormat;
use lewton::{
    audio::{read_audio_packet_generic, PreviousWindowRight},
    header::{read_header_comment, read_header_ident, read_header_setup, IdentHeader, SetupHeader},
    samples::InterleavedSamples,
};
use ogg::{Packet, PacketReader};

#[rustfmt::skip]
mod vorbis_lookup;

/// Default cap for decoded audio. Project export further intersects this with its output cap.
pub const MAX_DECODED_AUDIO_BYTES: usize = 512 * 1024 * 1024;

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
    /// `FSB5` at 0: parsed for supported Vorbis reconstruction.
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
/// `CopyPrintable`: fills with `'Ø'` (U+00D8), then copies ASCII printable bytes
/// except tab.
pub fn diagnostic_printable(data: &[u8]) -> [char; 4] {
    let mut out = ['Ø'; 4];
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

/// `NativeAudioExporter.Export` warning for an empty payload.
///
/// The source names the asset with `GetBestName`.
pub fn native_audio_empty_warning(best_name: &str) -> String {
    format!("Can't export '{best_name}' because no valid data was found")
}

/// `AudioClipExporter.TryCreateCollection` warning when decoding yields no bytes.
///
/// The source names the asset with its plain `Name`.
pub fn empty_decoded_audio_warning(name: &str) -> String {
    format!("Decoded audio data is empty for '{name}'")
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

/// `AudioClipDecoder.TryDecode` composed over raw bytes: empty data fails with the
/// reference message, a present 3.5 sound type passes the bytes through with the
/// numeric extension, supported single-stream FSB5 Vorbis is reconstructed as Ogg, tracker
/// magics pass through with fixed extensions, and anything else fails with the
/// printable/hex diagnostic.
///
/// FSB5 reconstruction is bounded and never silently writes container bytes as audio.
pub fn try_decode_audio(
    name: &str,
    sound_type: Option<i32>,
    data: &[u8],
) -> Result<(Vec<u8>, &'static str), AudioDecodeError> {
    try_decode_audio_with_limit(name, sound_type, data, MAX_DECODED_AUDIO_BYTES)
}

/// Decodes audio while bounding any FSB5 reconstruction output.
pub fn try_decode_audio_with_limit(
    name: &str,
    sound_type: Option<i32>,
    data: &[u8],
    max_decoded_bytes: usize,
) -> Result<(Vec<u8>, &'static str), AudioDecodeError> {
    if data.is_empty() {
        return Err(AudioDecodeError::NoAudioData {
            name: name.to_owned(),
        });
    }
    if let Some(sound_type) = sound_type {
        return Ok((
            data.to_vec(),
            crate::resources::audio::fmod_sound_type_raw_extension(sound_type),
        ));
    }
    match classify_audio_data(data) {
        AudioDataKind::Fsb5 => decode_fsb5_vorbis(data, max_decoded_bytes),
        kind => match tracker_extension(kind) {
            Some(extension) => Ok((data.to_vec(), extension)),
            None => Err(AudioDecodeError::UnknownEncoding {
                printable: diagnostic_printable(data).iter().collect(),
                hex: diagnostic_hex(data).iter().collect(),
            }),
        },
    }
}

const FLOAT_WAV_HEADER_LEN: usize = 58;
const FLOAT_WAV_RIFF_OVERHEAD: u64 = 50;

/// Converts a Vorbis Ogg stream to the 32-bit IEEE-float WAV written by the baseline
/// `AudioConverter.OggToWav` path. The limit includes the WAV header and sample data.
pub fn convert_ogg_to_wav_with_limit(
    data: &[u8],
    max_output_bytes: usize,
) -> Result<Vec<u8>, AudioDecodeError> {
    if data.is_empty() {
        return Ok(Vec::new());
    }

    let mut packet_reader = PacketReader::new(Cursor::new(data));
    let first_packet = read_ogg_packet(&mut packet_reader, "identification header")?;
    let mut headers = read_ogg_vorbis_headers(&mut packet_reader, first_packet)?;
    let channels = u16::from(headers.ident.audio_channels);
    let sample_rate = headers.ident.audio_sample_rate;
    let block_align = channels
        .checked_mul(4)
        .ok_or_else(|| ogg_to_wav_failed("channel count overflows WAV block alignment"))?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| ogg_to_wav_failed("sample rate overflows WAV byte rate"))?;

    let mut output = LimitedAudioWriter::new(max_output_bytes);
    let header = float_wav_header(channels, sample_rate, byte_rate, block_align);
    output
        .write_all(&header)
        .map_err(|error| ogg_output_error(error, &output, max_output_bytes))?;

    let mut total_frames = 0u64;
    let mut stream_frames = 0u64;
    let mut previous_window = PreviousWindowRight::new();
    let mut stream_ended = headers.ended_in_headers;

    while let Some(packet) = packet_reader
        .read_packet()
        .map_err(|error| ogg_to_wav_failed(format!("invalid Ogg packet stream: {error}")))?
    {
        if packet.stream_serial() != headers.serial {
            if packet.first_in_stream() {
                if !stream_ended {
                    return Err(ogg_to_wav_failed(
                        "a new logical stream began before the previous Vorbis stream ended",
                    ));
                }
                let next_headers = read_ogg_vorbis_headers(&mut packet_reader, packet)?;
                if next_headers.ident.audio_channels != headers.ident.audio_channels
                    || next_headers.ident.audio_sample_rate != headers.ident.audio_sample_rate
                {
                    return Err(ogg_to_wav_failed(format!(
                        "chained Vorbis stream has {} channels at {} Hz; the WAV already uses {} channels at {} Hz",
                        next_headers.ident.audio_channels,
                        next_headers.ident.audio_sample_rate,
                        headers.ident.audio_channels,
                        headers.ident.audio_sample_rate
                    )));
                }
                headers = next_headers;
                stream_frames = 0;
                previous_window = PreviousWindowRight::new();
                stream_ended = headers.ended_in_headers;
                continue;
            }
            // The source Vorbis reader ignores packets from other serials unless they
            // begin a chained logical stream.
            continue;
        }
        if stream_ended {
            return Err(ogg_to_wav_failed(
                "Vorbis audio packets follow the logical stream's EOS packet",
            ));
        }

        let decoded: InterleavedSamples<f32> = read_audio_packet_generic(
            &headers.ident,
            &headers.setup,
            &packet.data,
            &mut previous_window,
        )
        .map_err(|error| ogg_to_wav_failed(format!("invalid Vorbis audio packet: {error:?}")))?;
        if decoded.channel_count == 0
            || decoded.channel_count != usize::from(channels)
            || !decoded.samples.len().is_multiple_of(decoded.channel_count)
        {
            return Err(ogg_to_wav_failed(format!(
                "decoded Vorbis packet has {} samples across {} channels, expected {} channels",
                decoded.samples.len(),
                decoded.channel_count,
                channels
            )));
        }
        if packet.last_in_stream() && !packet.last_in_page() {
            return Err(ogg_to_wav_failed(
                "the Vorbis EOS packet is not the last packet on its Ogg page",
            ));
        }

        let packet_frames = decoded.samples.len() / decoded.channel_count;
        let decoded_stream_end = stream_frames
            .checked_add(u64::try_from(packet_frames).map_err(|_| {
                ogg_to_wav_failed("decoded Vorbis packet frame count exceeds 64-bit range")
            })?)
            .ok_or_else(|| ogg_to_wav_failed("decoded Vorbis frame count overflowed"))?;
        let frames_to_write = if packet.last_in_page() {
            let page_granule = packet.absgp_page();
            if packet.last_in_stream() {
                if page_granule < stream_frames || page_granule > decoded_stream_end {
                    return Err(ogg_to_wav_failed(format!(
                        "Vorbis EOS granule {page_granule} is outside the decoded frame range {}..={decoded_stream_end}",
                        stream_frames
                    )));
                }
                stream_ended = true;
                usize::try_from(page_granule - stream_frames).map_err(|_| {
                    ogg_to_wav_failed("Vorbis EOS frame count exceeds address space")
                })?
            } else {
                if page_granule != decoded_stream_end {
                    return Err(ogg_to_wav_failed(format!(
                        "Ogg page granule {page_granule} does not match {decoded_stream_end} decoded Vorbis frames"
                    )));
                }
                packet_frames
            }
        } else {
            packet_frames
        };

        append_float_wav_samples(
            &mut output,
            &decoded.samples,
            frames_to_write,
            channels,
            block_align,
            &mut total_frames,
            max_output_bytes,
        )?;
        stream_frames = if packet.last_in_stream() {
            packet.absgp_page()
        } else {
            decoded_stream_end
        };
    }

    if !stream_ended {
        return Err(ogg_to_wav_failed(
            "Vorbis Ogg data ended without a logical-stream EOS packet",
        ));
    }
    finish_float_wav(&mut output.bytes, total_frames, block_align)?;
    Ok(output.bytes)
}

struct OggVorbisHeaders {
    serial: u32,
    ident: IdentHeader,
    setup: SetupHeader,
    ended_in_headers: bool,
}

fn read_ogg_vorbis_headers<R: Read + io::Seek>(
    packet_reader: &mut PacketReader<R>,
    first_packet: Packet,
) -> Result<OggVorbisHeaders, AudioDecodeError> {
    if !first_packet.first_in_stream() {
        return Err(ogg_to_wav_failed(
            "the Vorbis identification header is not at the start of an Ogg logical stream",
        ));
    }
    let serial = first_packet.stream_serial();
    let ident = read_header_ident(&first_packet.data).map_err(|error| {
        ogg_to_wav_failed(format!("invalid Vorbis identification header: {error:?}"))
    })?;
    let comment_packet = read_next_ogg_packet_in_stream(packet_reader, serial, "comment header")?;
    read_header_comment(&comment_packet.data)
        .map_err(|error| ogg_to_wav_failed(format!("invalid Vorbis comment header: {error:?}")))?;
    let setup_packet = read_next_ogg_packet_in_stream(packet_reader, serial, "setup header")?;
    let setup = read_header_setup(
        &setup_packet.data,
        ident.audio_channels,
        (ident.blocksize_0, ident.blocksize_1),
    )
    .map_err(|error| ogg_to_wav_failed(format!("invalid Vorbis setup header: {error:?}")))?;
    if setup_packet.last_in_stream() && setup_packet.absgp_page() != 0 {
        return Err(ogg_to_wav_failed(format!(
            "header-only Vorbis stream has nonzero EOS granule {}",
            setup_packet.absgp_page()
        )));
    }
    Ok(OggVorbisHeaders {
        serial,
        ident,
        setup,
        ended_in_headers: setup_packet.last_in_stream(),
    })
}

fn read_next_ogg_packet_in_stream<R: Read + io::Seek>(
    packet_reader: &mut PacketReader<R>,
    serial: u32,
    header_name: &str,
) -> Result<Packet, AudioDecodeError> {
    loop {
        let packet = read_ogg_packet(packet_reader, header_name)?;
        if packet.stream_serial() == serial {
            return Ok(packet);
        }
    }
}

fn read_ogg_packet<R: Read + io::Seek>(
    packet_reader: &mut PacketReader<R>,
    expected: &str,
) -> Result<Packet, AudioDecodeError> {
    packet_reader
        .read_packet()
        .map_err(|error| ogg_to_wav_failed(format!("could not read Ogg {expected}: {error}")))?
        .ok_or_else(|| ogg_to_wav_failed(format!("Ogg stream ended before its {expected}")))
}

fn float_wav_header(
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
) -> [u8; FLOAT_WAV_HEADER_LEN] {
    let mut header = [0u8; FLOAT_WAV_HEADER_LEN];
    header[..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(FLOAT_WAV_RIFF_OVERHEAD as u32).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&18u32.to_le_bytes());
    header[20..22].copy_from_slice(&3u16.to_le_bytes());
    header[22..24].copy_from_slice(&channels.to_le_bytes());
    header[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    header[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    header[32..34].copy_from_slice(&block_align.to_le_bytes());
    header[34..36].copy_from_slice(&32u16.to_le_bytes());
    header[36..38].copy_from_slice(&0u16.to_le_bytes());
    header[38..42].copy_from_slice(b"fact");
    header[42..46].copy_from_slice(&4u32.to_le_bytes());
    header[50..54].copy_from_slice(b"data");
    header
}

fn append_float_wav_samples(
    output: &mut LimitedAudioWriter,
    samples: &[f32],
    frames: usize,
    channels: u16,
    block_align: u16,
    total_frames: &mut u64,
    max_output_bytes: usize,
) -> Result<(), AudioDecodeError> {
    let sample_count = frames
        .checked_mul(usize::from(channels))
        .ok_or_else(|| ogg_to_wav_failed("decoded Vorbis sample count overflowed"))?;
    if sample_count > samples.len() {
        return Err(ogg_to_wav_failed(format!(
            "Vorbis packet contains {} samples, fewer than the {sample_count} WAV samples requested",
            samples.len()
        )));
    }
    let new_total_frames = total_frames
        .checked_add(
            u64::try_from(frames)
                .map_err(|_| ogg_to_wav_failed("WAV frame count exceeds 64-bit range"))?,
        )
        .ok_or_else(|| ogg_to_wav_failed("WAV frame count overflowed"))?;
    if new_total_frames > u64::from(u32::MAX) {
        return Err(ogg_to_wav_failed(
            "WAV fact chunk cannot represent more than 2^32-1 sample frames",
        ));
    }
    let data_bytes = new_total_frames
        .checked_mul(u64::from(block_align))
        .ok_or_else(|| ogg_to_wav_failed("WAV data length overflowed"))?;
    let riff_bytes = FLOAT_WAV_RIFF_OVERHEAD
        .checked_add(data_bytes)
        .ok_or_else(|| ogg_to_wav_failed("WAV RIFF chunk length overflowed"))?;
    if data_bytes > u64::from(u32::MAX) || riff_bytes > u64::from(u32::MAX) {
        return Err(ogg_to_wav_failed(
            "WAV data exceeds the RIFF 32-bit chunk-size limit",
        ));
    }
    let sample_bytes = sample_count
        .checked_mul(4)
        .ok_or_else(|| ogg_to_wav_failed("WAV packet byte length overflowed"))?;
    let new_output_len = output.bytes.len().checked_add(sample_bytes).ok_or(
        AudioDecodeError::DecodedDataLimitExceeded {
            limit: max_output_bytes,
        },
    )?;
    if new_output_len > max_output_bytes {
        return Err(AudioDecodeError::DecodedDataLimitExceeded {
            limit: max_output_bytes,
        });
    }
    let mut sample_data = Vec::new();
    sample_data
        .try_reserve_exact(sample_bytes)
        .map_err(|_| AudioDecodeError::OutputAllocationFailed)?;
    for sample in &samples[..sample_count] {
        // The baseline's NVorbis reader clips decoded samples to the normalized
        // floating-point audio range before NAudio writes them to WAV.
        sample_data.extend_from_slice(&sample.clamp(-1.0, 1.0).to_le_bytes());
    }
    output
        .write_all(&sample_data)
        .map_err(|error| ogg_output_error(error, output, max_output_bytes))?;
    *total_frames = new_total_frames;
    Ok(())
}

fn finish_float_wav(
    output: &mut [u8],
    total_frames: u64,
    block_align: u16,
) -> Result<(), AudioDecodeError> {
    let data_len = total_frames
        .checked_mul(u64::from(block_align))
        .ok_or_else(|| ogg_to_wav_failed("WAV data length overflowed"))?;
    let riff_len = FLOAT_WAV_RIFF_OVERHEAD
        .checked_add(data_len)
        .ok_or_else(|| ogg_to_wav_failed("WAV RIFF chunk length overflowed"))?;
    let data_len = u32::try_from(data_len)
        .map_err(|_| ogg_to_wav_failed("WAV data exceeds the RIFF 32-bit chunk-size limit"))?;
    let riff_len = u32::try_from(riff_len)
        .map_err(|_| ogg_to_wav_failed("WAV RIFF chunk exceeds the 32-bit size limit"))?;
    let frame_count = u32::try_from(total_frames)
        .map_err(|_| ogg_to_wav_failed("WAV fact chunk exceeds the 32-bit sample-frame limit"))?;
    if output.len() < FLOAT_WAV_HEADER_LEN {
        return Err(ogg_to_wav_failed("WAV output is shorter than its header"));
    }
    output[4..8].copy_from_slice(&riff_len.to_le_bytes());
    output[46..50].copy_from_slice(&frame_count.to_le_bytes());
    output[54..58].copy_from_slice(&data_len.to_le_bytes());
    Ok(())
}

fn ogg_output_error(
    error: io::Error,
    output: &LimitedAudioWriter,
    limit: usize,
) -> AudioDecodeError {
    if output.limit_exceeded {
        AudioDecodeError::DecodedDataLimitExceeded { limit }
    } else if output.allocation_failed || error.kind() == io::ErrorKind::OutOfMemory {
        AudioDecodeError::OutputAllocationFailed
    } else {
        ogg_to_wav_failed(format!("could not write WAV output: {error}"))
    }
}

fn ogg_to_wav_failed(reason: impl Into<String>) -> AudioDecodeError {
    AudioDecodeError::OggToWavFailed {
        reason: reason.into(),
    }
}

fn decode_fsb5_vorbis(
    data: &[u8],
    max_decoded_bytes: usize,
) -> Result<(Vec<u8>, &'static str), AudioDecodeError> {
    let sample = Fsb5VorbisSample::parse(data)?;
    let setup_header = *vorbis_lookup::VORBIS_LOOKUP
        .get(&sample.setup_crc32)
        .ok_or_else(|| AudioDecodeError::Fsb5DecodeFailed {
            reason: format!(
                "Vorbis setup header CRC32 {:#010X} is not in the supported lookup table",
                sample.setup_crc32
            ),
        })?;
    let identification_packet =
        build_vorbis_identification_packet(sample.channels, sample.sample_rate);
    decode_fsb5_vorbis_with_headers(
        sample,
        &identification_packet,
        setup_header,
        max_decoded_bytes,
    )
}

fn decode_fsb5_vorbis_with_headers(
    sample: Fsb5VorbisSample<'_>,
    identification_packet: &[u8],
    setup_header: &[u8],
    max_decoded_bytes: usize,
) -> Result<(Vec<u8>, &'static str), AudioDecodeError> {
    let identification =
        lewton::header::read_header_ident(identification_packet).map_err(|error| {
            AudioDecodeError::Fsb5DecodeFailed {
                reason: format!("invalid generated Vorbis identification header: {error}"),
            }
        })?;
    let setup = lewton::header::read_header_setup(
        setup_header,
        sample.channels,
        (identification.blocksize_0, identification.blocksize_1),
    )
    .map_err(|error| AudioDecodeError::Fsb5DecodeFailed {
        reason: format!("Vorbis setup header could not be parsed: {error}"),
    })?;

    let mut output = LimitedAudioWriter::new(max_decoded_bytes);
    let mut ogg = OggPacketWriter::default();
    let comment_packet = build_vorbis_comment_packet();
    let packets = [
        identification_packet,
        comment_packet.as_slice(),
        setup_header,
    ];
    for packet in packets {
        if let Err(error) = ogg.write_packet(&mut output, packet, 0, false) {
            return Err(audio_output_error(error, &output, max_decoded_bytes));
        }
    }
    if let Err(error) = ogg.flush_page(&mut output, false) {
        return Err(audio_output_error(error, &output, max_decoded_bytes));
    }

    let mut packet_reader = FsbVorbisPacketReader::new(sample.packet_data);
    let mut current_packet = packet_reader.next_packet()?;
    if current_packet.is_none() {
        return Err(AudioDecodeError::NoDecodedAudioData);
    }
    let mut previous_window = lewton::audio::PreviousWindowRight::new();
    let mut granule_position = 0u64;
    while let Some(packet) = current_packet {
        let next_packet = packet_reader.next_packet()?;
        let is_last_packet = next_packet.is_none();
        let first_audio_packet = previous_window.is_empty();
        let decoded: Vec<Vec<f32>> = lewton::audio::read_audio_packet_generic(
            &identification,
            &setup,
            packet,
            &mut previous_window,
        )
        .map_err(|error| AudioDecodeError::Fsb5DecodeFailed {
            reason: format!("Vorbis packet could not be validated: {error}"),
        })?;
        if decoded.len() != usize::from(sample.channels) {
            return Err(AudioDecodeError::Fsb5DecodeFailed {
                reason: format!(
                    "Vorbis packet decoded {} channels, but FSB5 declares {}",
                    decoded.len(),
                    sample.channels
                ),
            });
        }
        if !first_audio_packet {
            let frame_count = decoded.first().map_or(0, Vec::len);
            if decoded.iter().any(|channel| channel.len() != frame_count) {
                return Err(AudioDecodeError::Fsb5DecodeFailed {
                    reason: "Vorbis packet channels have inconsistent decoded lengths".to_owned(),
                });
            }
            granule_position = granule_position
                .checked_add(frame_count as u64)
                .ok_or_else(|| AudioDecodeError::Fsb5DecodeFailed {
                    reason: "Vorbis granule position overflowed".to_owned(),
                })?
                .min(u64::from(sample.sample_count));
        }
        let reached_declared_sample_count = granule_position == u64::from(sample.sample_count);
        if let Err(error) = ogg.write_packet(
            &mut output,
            packet,
            granule_position,
            is_last_packet || reached_declared_sample_count,
        ) {
            return Err(audio_output_error(error, &output, max_decoded_bytes));
        }
        if reached_declared_sample_count {
            break;
        }
        current_packet = next_packet;
    }
    if granule_position < u64::from(sample.sample_count) {
        return Err(AudioDecodeError::Fsb5DecodeFailed {
            reason: format!(
                "Vorbis packets decode to {granule_position} samples, but FSB5 declares {}",
                sample.sample_count
            ),
        });
    }
    if let Err(error) = ogg.flush_page(&mut output, false) {
        return Err(audio_output_error(error, &output, max_decoded_bytes));
    }

    if output.bytes.is_empty() {
        return Err(AudioDecodeError::NoDecodedAudioData);
    }
    Ok((output.bytes, "ogg"))
}

fn audio_output_error(
    error: io::Error,
    output: &LimitedAudioWriter,
    limit: usize,
) -> AudioDecodeError {
    if output.limit_exceeded {
        AudioDecodeError::DecodedDataLimitExceeded { limit }
    } else if output.allocation_failed || error.kind() == io::ErrorKind::OutOfMemory {
        AudioDecodeError::OutputAllocationFailed
    } else {
        AudioDecodeError::Fsb5DecodeFailed {
            reason: format!("could not write reconstructed Ogg data: {error}"),
        }
    }
}

fn build_vorbis_identification_packet(channels: u8, sample_rate: u32) -> [u8; 30] {
    let mut packet = [0u8; 30];
    packet[0] = 1;
    packet[1..7].copy_from_slice(b"vorbis");
    packet[11] = channels;
    packet[12..16].copy_from_slice(&sample_rate.to_le_bytes());
    packet[28] = 0b1011_1000;
    packet[29] = 1;
    packet
}

fn build_vorbis_comment_packet() -> Vec<u8> {
    const VENDOR: &[u8] = b"Fmod5Sharp (Samboy063)";
    let mut packet = Vec::with_capacity(7 + 4 + VENDOR.len() + 4 + 1);
    packet.push(3);
    packet.extend_from_slice(b"vorbis");
    packet.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
    packet.extend_from_slice(VENDOR);
    packet.extend_from_slice(&0u32.to_le_bytes());
    packet.push(1);
    packet
}

#[derive(Clone, Copy, Debug)]
struct Fsb5VorbisSample<'a> {
    channels: u8,
    sample_rate: u32,
    sample_count: u32,
    setup_crc32: u32,
    packet_data: &'a [u8],
}

impl<'a> Fsb5VorbisSample<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, AudioDecodeError> {
        if data.get(..4) != Some(b"FSB5") {
            return Err(fsb5_error("missing FSB5 signature"));
        }
        let version = fsb5_u32(data, 4, "version")?;
        let header_size = match version {
            0 => 64usize,
            1 => 60usize,
            other => return Err(fsb5_error(format!("unsupported FSB5 version {other}"))),
        };
        let stream_count = fsb5_u32(data, 8, "stream count")?;
        if stream_count != 1 {
            return Err(AudioDecodeError::Fsb5StreamCountUnsupported {
                count: stream_count,
            });
        }
        let stream_headers_size = usize::try_from(fsb5_u32(data, 12, "stream header size")?)
            .map_err(|_| fsb5_error("stream header size does not fit this platform"))?;
        let name_table_size = usize::try_from(fsb5_u32(data, 16, "name table size")?)
            .map_err(|_| fsb5_error("name table size does not fit this platform"))?;
        let stream_data_size = usize::try_from(fsb5_u32(data, 20, "stream data size")?)
            .map_err(|_| fsb5_error("stream data size does not fit this platform"))?;
        let mode = fsb5_u32(data, 24, "audio format")?;
        if mode != 15 {
            return Err(AudioDecodeError::Fsb5FormatUnsupported {
                format: fsb5_format_name(mode).to_owned(),
            });
        }
        fsb5_slice(data, 0, header_size, "base header")?;

        let stream_headers_start = header_size;
        let stream_headers_end = stream_headers_start
            .checked_add(stream_headers_size)
            .ok_or_else(|| fsb5_error("stream header range overflows"))?;
        if stream_headers_size < 8 {
            return Err(fsb5_error(
                "stream header region is shorter than its sample header",
            ));
        }
        fsb5_slice(
            data,
            stream_headers_start,
            stream_headers_size,
            "stream headers",
        )?;
        let sample_header = fsb5_u64(data, stream_headers_start, "sample header")?;
        let has_chunks = sample_header & 1 != 0;
        let mut sample_rate = match ((sample_header >> 1) & 0x0f) as u8 {
            0 => 4_000,
            1 => 8_000,
            2 => 11_000,
            3 => 11_025,
            4 => 16_000,
            5 => 22_050,
            6 => 24_000,
            7 => 32_000,
            8 => 44_100,
            9 => 48_000,
            10 => 96_000,
            code => return Err(fsb5_error(format!("unknown sample-rate code {code}"))),
        };
        let channel_code = ((sample_header >> 5) & 0x03) as u8;
        let mut channels = match channel_code {
            0 => 1,
            1 => 2,
            2 => 6,
            3 => 8,
            _ => unreachable!(),
        };
        let sample_count = ((sample_header >> 34) & 0x3fff_ffff) as u32;
        if sample_count == 0 {
            return Err(fsb5_error("sample count is zero"));
        }
        let stream_offset = (((sample_header >> 7) & 0x07ff_ffff) as u32)
            .checked_mul(32)
            .ok_or_else(|| fsb5_error("stream data offset overflows"))?
            as usize;
        let mut setup_crc32 = None;
        let mut cursor = stream_headers_start + 8;
        if has_chunks {
            loop {
                let chunk_header_end = cursor
                    .checked_add(4)
                    .ok_or_else(|| fsb5_error("sample metadata cursor overflows"))?;
                if chunk_header_end > stream_headers_end {
                    return Err(fsb5_error(
                        "sample metadata header extends beyond the declared stream-header region",
                    ));
                }
                let raw_chunk = fsb5_u32(data, cursor, "sample metadata chunk header")?;
                cursor = chunk_header_end;
                let has_more = raw_chunk & 1 != 0;
                let chunk_size = ((raw_chunk >> 1) & 0x00ff_ffff) as usize;
                let chunk_kind = (raw_chunk >> 25) as u8;
                let chunk_end = cursor
                    .checked_add(chunk_size)
                    .ok_or_else(|| fsb5_error("sample metadata range overflows"))?;
                if chunk_end > stream_headers_end {
                    return Err(fsb5_error(
                        "sample metadata extends beyond the declared stream-header region",
                    ));
                }
                let chunk_data = fsb5_slice(data, cursor, chunk_size, "sample metadata chunk")?;
                cursor = chunk_end;
                match chunk_kind {
                    1 => {
                        if chunk_data.len() != 1 || chunk_data[0] == 0 {
                            return Err(fsb5_error("invalid channel-count metadata chunk"));
                        }
                        channels = chunk_data[0];
                    }
                    2 => {
                        if chunk_data.len() != 4 {
                            return Err(fsb5_error("invalid sample-rate metadata chunk"));
                        }
                        sample_rate = u32::from_le_bytes(chunk_data.try_into().unwrap());
                        if sample_rate == 0 {
                            return Err(fsb5_error("sample-rate metadata is zero"));
                        }
                    }
                    11 => {
                        if chunk_data.len() < 4 {
                            return Err(fsb5_error(
                                "Vorbis metadata chunk is shorter than its CRC32",
                            ));
                        }
                        if setup_crc32.is_none() {
                            setup_crc32 =
                                Some(u32::from_le_bytes(chunk_data[..4].try_into().unwrap()));
                        }
                    }
                    _ => {}
                }
                if !has_more {
                    break;
                }
            }
        }
        if cursor > stream_headers_end {
            return Err(fsb5_error("sample metadata exceeds its declared region"));
        }
        let setup_crc32 =
            setup_crc32.ok_or_else(|| fsb5_error("missing Vorbis setup CRC32 metadata"))?;

        let data_start = stream_headers_end
            .checked_add(name_table_size)
            .ok_or_else(|| fsb5_error("stream data start overflows"))?;
        let data_end = data_start
            .checked_add(stream_data_size)
            .ok_or_else(|| fsb5_error("stream data end overflows"))?;
        fsb5_slice(data, data_start, stream_data_size, "stream data")?;
        if stream_offset >= stream_data_size {
            return Err(fsb5_error(
                "sample data offset is outside the stream-data region",
            ));
        }
        let packet_data_start = data_start
            .checked_add(stream_offset)
            .ok_or_else(|| fsb5_error("sample packet-data start overflows"))?;
        let packet_data = fsb5_slice(
            data,
            packet_data_start,
            data_end - packet_data_start,
            "sample packet data",
        )?;
        Ok(Self {
            channels,
            sample_rate,
            sample_count,
            setup_crc32,
            packet_data,
        })
    }
}

fn fsb5_format_name(mode: u32) -> &'static str {
    match mode {
        0 => "None",
        1 => "Pcm8",
        2 => "Pcm16",
        3 => "Pcm24",
        4 => "Pcm32",
        5 => "PcmFloat",
        6 => "GcAdpcm",
        7 => "ImaAdpcm",
        8 => "Vag",
        9 => "HeVag",
        10 => "Xma",
        11 => "Mpeg",
        12 => "Celt",
        13 => "Atrac9",
        14 => "Xwma",
        15 => "Vorbis",
        16 => "FAdpcm",
        17 => "Opus",
        _ => "Unknown",
    }
}

fn fsb5_error(reason: impl Into<String>) -> AudioDecodeError {
    AudioDecodeError::Fsb5DecodeFailed {
        reason: reason.into(),
    }
}

fn fsb5_u32(data: &[u8], offset: usize, name: &str) -> Result<u32, AudioDecodeError> {
    let bytes = fsb5_slice(data, offset, 4, name)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn fsb5_u64(data: &[u8], offset: usize, name: &str) -> Result<u64, AudioDecodeError> {
    let bytes = fsb5_slice(data, offset, 8, name)?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn fsb5_slice<'a>(
    data: &'a [u8],
    offset: usize,
    size: usize,
    name: &str,
) -> Result<&'a [u8], AudioDecodeError> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| fsb5_error(format!("{name} range overflows")))?;
    data.get(offset..end)
        .ok_or_else(|| fsb5_error(format!("truncated {name}")))
}

struct FsbVorbisPacketReader<'a> {
    data: &'a [u8],
    cursor: usize,
    stopped: bool,
}

impl<'a> FsbVorbisPacketReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            cursor: 0,
            stopped: false,
        }
    }

    fn next_packet(&mut self) -> Result<Option<&'a [u8]>, AudioDecodeError> {
        if self.stopped || self.cursor.saturating_add(2) >= self.data.len() {
            self.stopped = true;
            return Ok(None);
        }
        let packet_size = u16::from_le_bytes([self.data[self.cursor], self.data[self.cursor + 1]]);
        self.cursor += 2;
        if packet_size == 0 || packet_size == u16::MAX {
            self.stopped = true;
            return Ok(None);
        }
        let packet_size = usize::from(packet_size);
        let packet = fsb5_slice(self.data, self.cursor, packet_size, "Vorbis audio packet")?;
        self.cursor = self
            .cursor
            .checked_add(packet_size)
            .ok_or_else(|| fsb5_error("Vorbis packet cursor overflows"))?;
        Ok(Some(packet))
    }
}

#[derive(Default)]
struct OggPacketWriter {
    page_sequence: u32,
    pending_lacing: Vec<u8>,
    pending_body: Vec<u8>,
    pending_packet_count: usize,
    pending_granule_position: Option<u64>,
}

impl OggPacketWriter {
    fn write_packet(
        &mut self,
        output: &mut LimitedAudioWriter,
        packet: &[u8],
        granule_position: u64,
        end_of_stream: bool,
    ) -> io::Result<()> {
        let lacing = ogg_packet_lacing(packet.len())?;
        if lacing.len() > 255 {
            self.flush_page(output, false)?;
            return self.write_fragmented_packet(
                output,
                packet,
                &lacing,
                granule_position,
                end_of_stream,
            );
        }

        if self.pending_lacing.len() + lacing.len() > 255
            || self.pending_body.len() + packet.len() > 65_025
        {
            self.flush_page(output, false)?;
        }
        self.pending_lacing.try_reserve(lacing.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate Ogg lacing data",
            )
        })?;
        self.pending_body.try_reserve(packet.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate Ogg packet data",
            )
        })?;
        self.pending_lacing.extend_from_slice(&lacing);
        self.pending_body.extend_from_slice(packet);
        self.pending_packet_count += 1;
        self.pending_granule_position = Some(granule_position);

        if end_of_stream {
            self.flush_page(output, true)
        } else if self.pending_lacing.len() >= 255
            || self.pending_body.len() >= 65_025
            || (self.pending_packet_count >= 4 && self.pending_body.len() >= 4_096)
        {
            self.flush_page(output, false)
        } else {
            Ok(())
        }
    }

    fn flush_page(
        &mut self,
        output: &mut LimitedAudioWriter,
        end_of_stream: bool,
    ) -> io::Result<()> {
        if self.pending_lacing.is_empty() {
            return Ok(());
        }
        write_ogg_page(
            output,
            &mut self.page_sequence,
            &self.pending_lacing,
            &self.pending_body,
            self.pending_granule_position.unwrap_or(0),
            false,
            end_of_stream,
        )?;
        self.pending_lacing.clear();
        self.pending_body.clear();
        self.pending_packet_count = 0;
        self.pending_granule_position = None;
        Ok(())
    }

    fn write_fragmented_packet(
        &mut self,
        output: &mut LimitedAudioWriter,
        packet: &[u8],
        lacing: &[u8],
        granule_position: u64,
        end_of_stream: bool,
    ) -> io::Result<()> {
        let page_count = lacing.len().div_ceil(255);
        let mut body_offset = 0usize;
        for (page_index, segments) in lacing.chunks(255).enumerate() {
            let body_size = segments
                .iter()
                .map(|size| usize::from(*size))
                .sum::<usize>();
            let body_end = body_offset.checked_add(body_size).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Ogg page body size overflows")
            })?;
            let final_page = page_index + 1 == page_count;
            write_ogg_page(
                output,
                &mut self.page_sequence,
                segments,
                &packet[body_offset..body_end],
                if final_page {
                    granule_position
                } else {
                    u64::MAX
                },
                page_index > 0,
                end_of_stream && final_page,
            )?;
            body_offset = body_end;
        }
        Ok(())
    }
}

fn write_ogg_page(
    output: &mut LimitedAudioWriter,
    page_sequence: &mut u32,
    lacing: &[u8],
    body: &[u8],
    granule_position: u64,
    continued_packet: bool,
    end_of_stream: bool,
) -> io::Result<()> {
    let next_sequence = page_sequence
        .checked_add(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Ogg page sequence overflows"))?;
    let mut header = [0u8; 27];
    header[..4].copy_from_slice(b"OggS");
    header[4] = 0;
    header[5] = (if continued_packet { 0x01 } else { 0 })
        | (if *page_sequence == 0 { 0x02 } else { 0 })
        | (if end_of_stream { 0x04 } else { 0 });
    header[6..14].copy_from_slice(&granule_position.to_le_bytes());
    header[14..18].copy_from_slice(&1u32.to_le_bytes());
    header[18..22].copy_from_slice(&page_sequence.to_le_bytes());
    header[26] = lacing.len() as u8;

    let mut checksum = ogg_crc32_update(0, &header);
    checksum = ogg_crc32_update(checksum, lacing);
    checksum = ogg_crc32_update(checksum, body);
    header[22..26].copy_from_slice(&checksum.to_le_bytes());

    output.write_all(&header)?;
    output.write_all(lacing)?;
    output.write_all(body)?;
    *page_sequence = next_sequence;
    Ok(())
}

fn ogg_packet_lacing(packet_size: usize) -> io::Result<Vec<u8>> {
    let segment_count = packet_size / 255 + 1;
    let mut lacing = Vec::new();
    lacing.try_reserve(segment_count).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "could not allocate Ogg lacing data",
        )
    })?;
    lacing.resize(packet_size / 255, 255);
    lacing.push((packet_size % 255) as u8);
    Ok(lacing)
}

fn ogg_crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

struct LimitedAudioWriter {
    bytes: Vec<u8>,
    limit: usize,
    limit_exceeded: bool,
    allocation_failed: bool,
}

impl LimitedAudioWriter {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            limit_exceeded: false,
            allocation_failed: false,
        }
    }
}

impl Write for LimitedAudioWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let new_len = self.bytes.len().checked_add(buffer.len()).ok_or_else(|| {
            self.limit_exceeded = true;
            io::Error::new(io::ErrorKind::InvalidData, "decoded audio size overflows")
        })?;
        if new_len > self.limit {
            self.limit_exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decoded audio exceeds configured output limit",
            ));
        }
        if self.bytes.try_reserve(buffer.len()).is_err() {
            self.allocation_failed = true;
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate decoded audio output",
            ));
        }
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Why `AudioClipDecoder.TryDecode` refused raw bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AudioDecodeError {
    /// No bytes at all; mirrors the reference early failure (the message carries
    /// the clip name like the source format string).
    NoAudioData {
        /// The clip name for the message.
        name: String,
    },
    /// FSB5 parsing or Vorbis reconstruction failed.
    Fsb5DecodeFailed {
        /// Decoder diagnostic.
        reason: String,
    },
    /// The FSB contains more than the one sample accepted by the source AudioClip decoder.
    Fsb5StreamCountUnsupported {
        /// Number of streams found.
        count: u32,
    },
    /// This FSB codec has no reconstructed standard-file writer in this port.
    Fsb5FormatUnsupported {
        /// Decoder format name.
        format: String,
    },
    /// Decoding exceeded the caller's explicit output cap.
    DecodedDataLimitExceeded {
        /// Maximum number of decoded bytes.
        limit: usize,
    },
    /// FSB output allocation failed.
    OutputAllocationFailed,
    /// Ogg/Vorbis validation, decoding, or WAV conversion failed.
    OggToWavFailed {
        /// Decoder or writer diagnostic.
        reason: String,
    },
    /// A valid container yielded no encoded audio bytes.
    NoDecodedAudioData,
    /// None of the magics matched; carries the printable/hex diagnostic words.
    UnknownEncoding {
        /// The four printable characters.
        printable: String,
        /// The eight hex characters.
        hex: String,
    },
}

impl core::fmt::Display for AudioDecodeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoAudioData { name } => write!(
                formatter,
                "Can't decode audio clip '{name}' with default decoder because its audio data could not be found."
            ),
            Self::Fsb5DecodeFailed { reason } => write!(
                formatter,
                "FSB5 audio could not be decoded: {reason}"
            ),
            Self::Fsb5StreamCountUnsupported { count } => write!(
                formatter,
                "FSB5 audio contains {count} streams; exactly one is required"
            ),
            Self::Fsb5FormatUnsupported { format } => write!(
                formatter,
                "FSB5 audio format {format} is unsupported for standard-file reconstruction"
            ),
            Self::DecodedDataLimitExceeded { limit } => write!(
                formatter,
                "decoded audio exceeds the output limit of {limit} bytes"
            ),
            Self::OutputAllocationFailed => write!(
                formatter,
                "could not allocate decoded audio output"
            ),
            Self::OggToWavFailed { reason } => {
                write!(formatter, "Ogg-to-WAV conversion failed: {reason}")
            }
            Self::NoDecodedAudioData => write!(
                formatter,
                "FSB5 decoder produced no audio bytes"
            ),
            Self::UnknownEncoding { printable, hex } => write!(
                formatter,
                "Failed to convert audio starting with '{printable}' ({hex})"
            ),
        }
    }
}

impl std::error::Error for AudioDecodeError {}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::{
        append_float_wav_samples, check_magic, classify_audio_data,
        compression_format_raw_extension, convert_ogg_to_wav_with_limit, converts_ogg_to_wav,
        diagnostic_hex, diagnostic_printable, empty_decoded_audio_warning,
        is_supported_export_format, native_audio_empty_warning, native_audio_extension,
        plan_audio_clip_collection, plan_native_audio_export, plan_streamed_resource,
        sound_type_raw_extension, tracker_extension, try_decode_audio, AudioDataKind,
        AudioDecodeError, AudioDecodeFact, AudioRefusal, LimitedAudioWriter, NativeAudioExport,
        StreamedResourcePlan, MOD_MAGICS,
    };
    use crate::export::settings::AudioExportFormat;

    #[test]
    fn ogg_to_wav_clips_decoded_float_samples_to_unit_range() {
        let samples = [-1.25, -1.0, -0.25, 0.0, 0.25, 1.0, 1.25];
        let expected = [-1.0, -1.0, -0.25, 0.0, 0.25, 1.0, 1.0];
        let mut output = LimitedAudioWriter::new(samples.len() * 4);
        let mut total_frames = 0;

        append_float_wav_samples(
            &mut output,
            &samples,
            samples.len(),
            1,
            4,
            &mut total_frames,
            samples.len() * 4,
        )
        .unwrap();

        let written: Vec<_> = output
            .bytes
            .chunks_exact(4)
            .map(|sample| f32::from_le_bytes(sample.try_into().unwrap()))
            .collect();
        assert_eq!(written, expected);
        assert_eq!(total_frames, samples.len() as u64);
    }

    #[test]
    fn ogg_to_wav_matches_reference_float_wav_and_decoded_frames() {
        let ogg = include_bytes!("ogg_to_wav_test_fixture.ogg");
        let reference = include_bytes!("ogg_to_wav_test_fixture_reference.wav");
        let converted = convert_ogg_to_wav_with_limit(ogg, 1024 * 1024).unwrap();

        assert_eq!(converted.len(), reference.len());
        assert_eq!(&converted[..58], &reference[..58]);
        assert_eq!(&converted[..4], b"RIFF");
        assert_eq!(&converted[8..12], b"WAVE");
        assert_eq!(&converted[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes(converted[20..22].try_into().unwrap()), 3);
        assert_eq!(u16::from_le_bytes(converted[22..24].try_into().unwrap()), 1);
        assert_eq!(
            u32::from_le_bytes(converted[24..28].try_into().unwrap()),
            8000
        );
        assert_eq!(
            u16::from_le_bytes(converted[34..36].try_into().unwrap()),
            32
        );
        assert_eq!(&converted[38..42], b"fact");
        assert_eq!(
            u32::from_le_bytes(converted[46..50].try_into().unwrap()),
            544
        );
        assert_eq!(&converted[50..54], b"data");
        assert_eq!(
            u32::from_le_bytes(converted[54..58].try_into().unwrap()),
            544 * 4
        );

        let largest_sample_error = converted[58..]
            .chunks_exact(4)
            .zip(reference[58..].chunks_exact(4))
            .map(|(actual, expected)| {
                let actual = f32::from_le_bytes(actual.try_into().unwrap());
                let expected = f32::from_le_bytes(expected.try_into().unwrap());
                (actual - expected).abs()
            })
            .fold(0.0f32, f32::max);
        // The source and Rust decoders both return IEEE binary32 samples. Allow eight
        // binary32 ULP at unit magnitude for decoder implementation rounding.
        assert!(
            largest_sample_error <= 1.0e-6,
            "largest reference sample error was {largest_sample_error}"
        );
    }

    #[test]
    fn ogg_to_wav_reports_invalid_input_and_enforces_the_complete_output_limit() {
        assert_eq!(convert_ogg_to_wav_with_limit(&[], 0), Ok(Vec::new()));
        assert!(matches!(
            convert_ogg_to_wav_with_limit(b"not an Ogg stream", 1024),
            Err(AudioDecodeError::OggToWavFailed { .. })
        ));

        let ogg = include_bytes!("ogg_to_wav_test_fixture.ogg");
        let exact_output_limit = include_bytes!("ogg_to_wav_test_fixture_reference.wav").len();
        assert_eq!(
            convert_ogg_to_wav_with_limit(ogg, exact_output_limit)
                .unwrap()
                .len(),
            exact_output_limit
        );
        assert!(matches!(
            convert_ogg_to_wav_with_limit(ogg, exact_output_limit - 1),
            Err(AudioDecodeError::DecodedDataLimitExceeded { limit })
                if limit == exact_output_limit - 1
        ));
        assert!(matches!(
            convert_ogg_to_wav_with_limit(ogg, 57),
            Err(AudioDecodeError::DecodedDataLimitExceeded { limit: 57 })
        ));
    }

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
        // The fill is U+00D8 LATIN CAPITAL LETTER O WITH STROKE per the source
        // `FillCharacter` (a prior revision mistakenly used ASCII 'O').
        assert_eq!(
            diagnostic_printable(b"\x01\x02\t\x7f"),
            ['Ø', 'Ø', 'Ø', 'Ø']
        );
        assert_eq!(diagnostic_printable(b"A"), ['A', 'Ø', 'Ø', 'Ø']);
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

    fn hex_bytes(word: &str) -> Vec<u8> {
        (0..word.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&word[index..index + 2], 16).unwrap())
            .collect()
    }

    /// Differential against the reference `AudioClipDecoder.TryDecode` (probe
    /// `Temp/opencode/audioprobe`): real 3.5 clip instances with `AudioData` and
    /// `Type` set, asserting the early message, the numeric extension mapping
    /// with byte passthrough, and the unmapped default.
    #[test]
    fn try_decode_matches_the_reference_runtime() {
        let fixture = include_str!("audio_decode_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(6, '|');
            assert_eq!(parts.next(), Some("ADECODE"), "{line}");
            let tag = parts.next().expect("a probe tag");
            let ok = parts
                .next()
                .expect("an ok word")
                .strip_prefix("ok=")
                .expect("an ok prefix");
            let extension = parts
                .next()
                .expect("an extension word")
                .strip_prefix("ext=")
                .expect("an extension prefix");
            let data_word = parts
                .next()
                .expect("a data word")
                .strip_prefix("data=")
                .expect("a data prefix");
            let message = parts
                .next()
                .expect("a message")
                .strip_prefix("msg=")
                .expect("a message prefix");
            assert!(parts.next().is_none(), "{line}");
            let (data, sound_type) = match tag {
                "empty" => (Vec::new(), Some(20)),
                "typed-wav" => (hex_bytes(data_word), Some(20)),
                "typed-ogg" => (hex_bytes(data_word), Some(14)),
                "typed-default" => (hex_bytes(data_word), Some(999)),
                _ => panic!("unexpected fixture tag {tag}"),
            };
            match try_decode_audio("", sound_type, &data) {
                Ok((decoded, decoded_extension)) => {
                    assert_eq!(ok, "1", "{line}");
                    assert_eq!(decoded_extension, extension, "{line}");
                    assert_eq!(decoded, data, "{line}");
                    assert_eq!(message, "null", "{line}");
                }
                Err(error) => {
                    assert_eq!(ok, "0", "{line}");
                    assert_eq!(extension, "null", "{line}");
                    assert_eq!(data_word, "null", "{line}");
                    assert_eq!(error.to_string(), message, "{line}");
                }
            }
            rows += 1;
        }
        assert_eq!(rows, 4, "all probed decode rows are asserted");
    }

    #[test]
    fn try_decode_rejects_malformed_fsb5_and_unknown_bytes_explicitly() {
        assert!(matches!(
            try_decode_audio("clip", None, b"FSB5...."),
            Err(AudioDecodeError::Fsb5DecodeFailed { .. })
        ));
        let error = try_decode_audio("clip", None, b"ABCD").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Failed to convert audio starting with 'ABCD' (41424344)"
        );
        // Tracker magics pass through untouched with fixed extensions.
        let mut mod_data = vec![0u8; 1084];
        mod_data[1080..1084].copy_from_slice(b"M.K.");
        assert_eq!(
            try_decode_audio("clip", None, &mod_data).map(|pair| pair.1),
            Ok("mod")
        );
    }

    #[test]
    fn bounded_audio_writer_rejects_output_beyond_its_limit() {
        let mut writer = super::LimitedAudioWriter::new(4);
        assert_eq!(writer.write(b"1234").unwrap(), 4);
        assert_eq!(writer.bytes, b"1234");
        assert!(writer.write(b"5").is_err());
        assert!(writer.limit_exceeded);
        assert_eq!(writer.bytes, b"1234");
    }

    #[test]
    fn ogg_packet_writer_emits_valid_crc_and_continuation_pages() {
        let mut output = super::LimitedAudioWriter::new(70_000);
        let mut writer = super::OggPacketWriter::default();
        let packet = vec![0x5a; 65_025];
        writer
            .write_packet(&mut output, &packet, 123, true)
            .unwrap();

        let first_page_size = 27 + 255 + 65_025;
        assert_eq!(&output.bytes[..4], b"OggS");
        assert_eq!(output.bytes[5], 0x02);
        assert_eq!(output.bytes[26], 255);
        assert_eq!(&output.bytes[27 + 255..first_page_size], packet);
        assert_eq!(output.bytes[first_page_size + 5], 0x05);
        assert_eq!(output.bytes[first_page_size + 26], 1);
        assert_eq!(output.bytes[first_page_size + 27], 0);
        assert_eq!(
            u64::from_le_bytes(
                output.bytes[first_page_size + 6..first_page_size + 14]
                    .try_into()
                    .unwrap()
            ),
            123
        );

        for page_start in [0, first_page_size] {
            let segment_count = usize::from(output.bytes[page_start + 26]);
            let header_end = page_start + 27 + segment_count;
            let body_size = output.bytes[page_start + 27..header_end]
                .iter()
                .map(|size| usize::from(*size))
                .sum::<usize>();
            let page_end = header_end + body_size;
            let expected_crc = u32::from_le_bytes(
                output.bytes[page_start + 22..page_start + 26]
                    .try_into()
                    .unwrap(),
            );
            let mut page = output.bytes[page_start..page_end].to_vec();
            page[22..26].fill(0);
            assert_eq!(super::ogg_crc32_update(0, &page), expected_crc);
        }
    }

    #[test]
    fn ogg_packet_writer_batches_full_pages_at_the_reference_watermark() {
        let mut output = super::LimitedAudioWriter::new(8_000);
        let mut writer = super::OggPacketWriter::default();
        let packet = vec![0x31; 1_024];
        for granule in 1..=4 {
            writer
                .write_packet(&mut output, &packet, granule, false)
                .unwrap();
        }
        let first_page_size = 27 + 20 + 4_096;
        assert_eq!(output.bytes.len(), first_page_size);
        assert_eq!(output.bytes[5], 0x02);
        assert_eq!(output.bytes[26], 20);

        writer.write_packet(&mut output, &packet, 5, true).unwrap();
        assert_eq!(output.bytes[first_page_size + 5], 0x04);
        assert_eq!(output.bytes[first_page_size + 26], 5);
    }

    #[test]
    fn fsb5_stream_count_is_rejected_before_metadata_parsing() {
        let mut data = vec![0u8; 60];
        data[..4].copy_from_slice(b"FSB5");
        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        data[8..12].copy_from_slice(&2u32.to_le_bytes());
        assert!(matches!(
            super::Fsb5VorbisSample::parse(&data),
            Err(AudioDecodeError::Fsb5StreamCountUnsupported { count: 2 })
        ));
    }

    #[test]
    fn fsb5_vorbis_parser_reads_versioned_headers_metadata_and_aligned_packets() {
        for (version, base_header_size) in [(0u32, 64usize), (1, 60)] {
            let mut data = vec![0u8; base_header_size];
            data[..4].copy_from_slice(b"FSB5");
            data[4..8].copy_from_slice(&version.to_le_bytes());
            data[8..12].copy_from_slice(&1u32.to_le_bytes());
            data[12..16].copy_from_slice(&29u32.to_le_bytes());
            data[20..24].copy_from_slice(&67u32.to_le_bytes());
            data[24..28].copy_from_slice(&15u32.to_le_bytes());

            // FSB5 sample header: 12,345 samples, a 64-byte data offset encoded in
            // 32-byte units, stereo, 44.1 kHz, and a following metadata chain.
            data.extend_from_slice(&[0x31, 0x01, 0, 0, 0xE4, 0xC0, 0, 0]);
            data.extend_from_slice(&[0x03, 0, 0, 0x02, 6]); // channels override, next chunk
            data.extend_from_slice(&[0x09, 0, 0, 0x04]); // 32 kHz override, next chunk
            data.extend_from_slice(&32_000u32.to_le_bytes());
            data.extend_from_slice(&[0x08, 0, 0, 0x16]); // Vorbis setup CRC, final chunk
            data.extend_from_slice(&0xA722_97FFu32.to_le_bytes());
            data.extend_from_slice(&[0u8; 64]);
            data.extend_from_slice(&[1, 0, 0]); // one length-prefixed one-byte packet

            let sample = super::Fsb5VorbisSample::parse(&data).unwrap();
            assert_eq!(sample.channels, 6);
            assert_eq!(sample.sample_rate, 32_000);
            assert_eq!(sample.sample_count, 12_345);
            assert_eq!(sample.setup_crc32, 0xA722_97FF);
            assert_eq!(sample.packet_data, &[1, 0, 0][..]);
            assert!(super::vorbis_lookup::VORBIS_LOOKUP.contains_key(&sample.setup_crc32));

            let mut packets = super::FsbVorbisPacketReader::new(sample.packet_data);
            assert_eq!(packets.next_packet().unwrap(), Some(&[0][..]));
            assert_eq!(packets.next_packet().unwrap(), None);
        }
    }

    #[test]
    fn fsb5_vorbis_reconstruction_preserves_audio_packets_and_source_samples() {
        let source_ogg = include_bytes!("ogg_to_wav_test_fixture.ogg");
        let source_wav = include_bytes!("ogg_to_wav_test_fixture_reference.wav");
        let mut reader = ogg::PacketReader::new(Cursor::new(&source_ogg[..]));
        let mut source_packets = Vec::new();
        while let Some(packet) = reader.read_packet().unwrap() {
            source_packets.push(packet.data);
        }
        assert!(source_packets.len() > 3);
        assert_eq!(&source_packets[0][..7], b"\x01vorbis");
        assert_eq!(&source_packets[2][..7], b"\x05vorbis");

        let identification = lewton::header::read_header_ident(&source_packets[0]).unwrap();
        let sample_count = u32::from_le_bytes(source_wav[46..50].try_into().unwrap());
        assert_eq!(identification.audio_channels, 1);
        assert_eq!(identification.audio_sample_rate, 8_000);
        assert_eq!(sample_count, 544);
        let setup_crc32 = crate::resources::blend_shape::crc32_ieee(&source_packets[2]);
        assert_eq!(setup_crc32, 0xE258_E714);
        assert!(!super::vorbis_lookup::VORBIS_LOOKUP.contains_key(&setup_crc32));

        let mut packet_data = Vec::new();
        for packet in &source_packets[3..] {
            let packet_len = u16::try_from(packet.len()).unwrap();
            packet_data.extend_from_slice(&packet_len.to_le_bytes());
            packet_data.extend_from_slice(packet);
        }

        let mut fsb5 = vec![0u8; 60];
        fsb5[..4].copy_from_slice(b"FSB5");
        fsb5[4..8].copy_from_slice(&1u32.to_le_bytes());
        fsb5[8..12].copy_from_slice(&1u32.to_le_bytes());
        fsb5[12..16].copy_from_slice(&16u32.to_le_bytes());
        fsb5[20..24].copy_from_slice(&u32::try_from(packet_data.len()).unwrap().to_le_bytes());
        fsb5[24..28].copy_from_slice(&15u32.to_le_bytes());
        let sample_header = (u64::from(sample_count) << 34) | (1 << 1) | 1;
        fsb5.extend_from_slice(&sample_header.to_le_bytes());
        fsb5.extend_from_slice(&[0x08, 0, 0, 0x16]);
        fsb5.extend_from_slice(&setup_crc32.to_le_bytes());
        fsb5.extend_from_slice(&packet_data);

        let sample = super::Fsb5VorbisSample::parse(&fsb5).unwrap();
        assert_eq!(sample.setup_crc32, setup_crc32);
        let (rebuilt_ogg, extension) = super::decode_fsb5_vorbis_with_headers(
            sample,
            &source_packets[0],
            &source_packets[2],
            1024 * 1024,
        )
        .unwrap();
        assert_eq!(extension, "ogg");

        let mut reader = ogg::PacketReader::new(Cursor::new(rebuilt_ogg.as_slice()));
        let mut rebuilt_packets = Vec::new();
        while let Some(packet) = reader.read_packet().unwrap() {
            rebuilt_packets.push(packet.data);
        }
        assert_eq!(rebuilt_packets.len(), source_packets.len());
        assert_eq!(rebuilt_packets[0], source_packets[0]);
        assert_eq!(rebuilt_packets[2], source_packets[2]);
        assert_eq!(&rebuilt_packets[3..], &source_packets[3..]);

        let rebuilt_wav = convert_ogg_to_wav_with_limit(&rebuilt_ogg, 1024 * 1024).unwrap();
        assert_eq!(rebuilt_wav.len(), source_wav.len());
        assert_eq!(&rebuilt_wav[..58], &source_wav[..58]);
        let largest_sample_error = rebuilt_wav[58..]
            .chunks_exact(4)
            .zip(source_wav[58..].chunks_exact(4))
            .map(|(actual, expected)| {
                let actual = f32::from_le_bytes(actual.try_into().unwrap());
                let expected = f32::from_le_bytes(expected.try_into().unwrap());
                (actual - expected).abs()
            })
            .fold(0.0f32, f32::max);
        assert!(
            largest_sample_error <= 1.0e-6,
            "largest source sample error was {largest_sample_error}"
        );
    }

    #[test]
    fn vorbis_lookup_setups_parse_with_production_identification() {
        // Every table key must be the CRC32 of its setup bytes, and every
        // setup must parse under lewton with the production identification
        // shape (sample-declared channels, 256/2048 block sizes). A probe
        // over all entries showed stereo-coupled setups parse only with two
        // channels while coupling-free setups parse with any layout; both
        // classes parse with two channels, which is what this test pins
        // alongside key integrity. Real-audio decode through the table is
        // covered by the table-backed reconstruction test below.
        let mut entries = 0u32;
        for (crc, header) in super::vorbis_lookup::VORBIS_LOOKUP.entries() {
            let header: &[u8] = header;
            assert_eq!(
                crate::resources::blend_shape::crc32_ieee(header),
                *crc,
                "lookup key must be the setup CRC32"
            );
            lewton::header::read_header_setup(header, 2, (8, 11))
                .expect("table setup must parse with stereo identification");
            entries += 1;
        }
        assert_eq!(entries, 161, "audited lookup-table size");
    }

    #[test]
    fn production_identification_packet_parses_with_declared_fields() {
        let packet = super::build_vorbis_identification_packet(2, 48_000);
        let identification = lewton::header::read_header_ident(&packet).unwrap();
        assert_eq!(identification.audio_channels, 2);
        assert_eq!(identification.audio_sample_rate, 48_000);
        assert_eq!(identification.blocksize_0, 8);
        assert_eq!(identification.blocksize_1, 11);
    }

    /// Minimal version-1 container holding two one-byte zero packets and a
    /// declared sample count of 128, mirroring the reconstruction test layout.
    fn silent_table_backed_container(channel_code: u64, rate_code: u64, setup_crc: u32) -> Vec<u8> {
        let packet_data = [1u8, 0, 0, 1, 0, 0];
        let mut fsb5 = vec![0u8; 60];
        fsb5[..4].copy_from_slice(b"FSB5");
        fsb5[4..8].copy_from_slice(&1u32.to_le_bytes());
        fsb5[8..12].copy_from_slice(&1u32.to_le_bytes());
        fsb5[12..16].copy_from_slice(&16u32.to_le_bytes());
        fsb5[20..24].copy_from_slice(&(packet_data.len() as u32).to_le_bytes());
        fsb5[24..28].copy_from_slice(&15u32.to_le_bytes());
        let sample_header = (u64::from(128u32) << 34) | (channel_code << 5) | (rate_code << 1) | 1;
        fsb5.extend_from_slice(&sample_header.to_le_bytes());
        fsb5.extend_from_slice(&[0x08, 0, 0, 0x16]);
        fsb5.extend_from_slice(&setup_crc.to_le_bytes());
        fsb5.extend_from_slice(&packet_data);
        fsb5
    }

    #[test]
    fn production_table_backed_fsb5_decode_rebuilds_silent_audio() {
        // Table-backed end-to-end proof through decode_fsb5_vorbis: the two
        // zero packets prime lewton's overlap window and then decode to 128
        // silent frames under the table setup, so the rebuilt Ogg preserves
        // the packets and the WAV conversion yields exact silence.
        for (channels, rate_code, setup_crc) in
            [(2u8, 8u64, 0xA722_97FFu32), (1u8, 9u64, 0x070B_A3B6u32)]
        {
            let channel_code = u64::from(channels - 1);
            let fsb5 = silent_table_backed_container(channel_code, rate_code, setup_crc);
            let (rebuilt_ogg, extension) = super::decode_fsb5_vorbis(&fsb5, 1024 * 1024).unwrap();
            assert_eq!(extension, "ogg");

            let mut reader = ogg::PacketReader::new(Cursor::new(rebuilt_ogg.as_slice()));
            let mut rebuilt_packets = Vec::new();
            while let Some(packet) = reader.read_packet().unwrap() {
                rebuilt_packets.push(packet.data);
            }
            assert_eq!(rebuilt_packets.len(), 5);
            assert_eq!(
                rebuilt_packets[2].as_slice(),
                *super::vorbis_lookup::VORBIS_LOOKUP.get(&setup_crc).unwrap()
            );
            assert_eq!(rebuilt_packets[3], [0]);
            assert_eq!(rebuilt_packets[4], [0]);

            let wav = super::convert_ogg_to_wav_with_limit(&rebuilt_ogg, 1024 * 1024).unwrap();
            assert_eq!(u32::from_le_bytes(wav[46..50].try_into().unwrap()), 128);
            assert_eq!(wav.len(), 58 + 128 * 4 * usize::from(channels));
            for sample in wav[58..].chunks_exact(4) {
                assert_eq!(f32::from_le_bytes(sample.try_into().unwrap()), 0.0);
            }
        }
    }

    #[test]
    fn fsb5_decode_rejects_unknown_setup_crc_before_audio() {
        // Unknown setups must fail loudly at the lookup stage, never by
        // decoding audio against a mismatched setup header.
        let unknown_crc = 0xDEAD_BEEFu32;
        assert!(!super::vorbis_lookup::VORBIS_LOOKUP.contains_key(&unknown_crc));
        let fsb5 = silent_table_backed_container(1, 8, unknown_crc);
        let error = super::decode_fsb5_vorbis(&fsb5, 1024 * 1024).unwrap_err();
        match error {
            super::AudioDecodeError::Fsb5DecodeFailed { reason } => assert!(
                reason.contains("not in the supported lookup table"),
                "unexpected reason: {reason}"
            ),
            other => panic!("expected table-miss failure, got {other}"),
        }
    }

    #[test]
    fn exporter_warnings_name_the_asset_like_the_source() {
        assert_eq!(
            native_audio_empty_warning("Music"),
            "Can't export 'Music' because no valid data was found"
        );
        assert_eq!(
            empty_decoded_audio_warning("Music"),
            "Decoded audio data is empty for 'Music'"
        );
    }
}
