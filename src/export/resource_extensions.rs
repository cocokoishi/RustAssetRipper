//! Export file-extension selection ported from the resource exporters.
//!
//! Every binary resource collection derives its file extension from the asset, with the
//! texture format choice reusing `ImageExportFormat` in `settings.rs`:
//!
//! - Textures keep the asset's best extension when original extensions are preferred and the
//!   extension names a known image format (matched case-sensitively, `jpg` included), and
//!   otherwise use the configured format. The bitmap encoder format follows the same
//!   selection but returns the format value rather than its extension.
//! - Text assets keep a safe best extension, remapping the eleven executable extensions
//!   (`cs`, `dll`, `exe`, `bat`, `cmd`, `ps1`, `vbs`, `js`, `msi`, `scr`, `com`, matched
//!   case-insensitively) to `txt`. Without a best extension the export mode decides: `Txt`
//!   writes `txt`, `Parse` sniffs JSON versus plain text versus binary, and anything else
//!   writes `bytes`.
//! - Fonts read their first four little-endian bytes: the `OTTO` magic means `otf` (OpenType
//!   with CFF outlines); anything shorter or different means `ttf`, including the short-data
//!   fallback the source keeps "just in case".
//! - Video clips take the extension of their original path (the dot is stripped, so a
//!   missing or trailing-dot extension yields nothing) and fall back to `bytes`.
//! - The best extension itself is the override extension, else the original extension.
//!
//! JSON sniffing follows the JSON text grammar with the `JsonDocument` default maximum depth
//! of 64; content rules (`IsPlainText`, control/whitespace classes) match Unicode behavior.
//! Number-range and encoder edge cases beyond the grammar stay open.

use super::asset_path::HostPathPlatform;
use super::settings::{ImageExportFormat, TextExportMode};
use crate::serialized_file::object::SerializedValue;

/// `TextAssetExportCollection.DangerousExtensions`, matched case-insensitively.
pub const DANGEROUS_EXTENSIONS: [&str; 11] = [
    "cs", "dll", "exe", "bat", "cmd", "ps1", "vbs", "js", "msi", "scr", "com",
];

/// The `OTTO` magic selecting `otf` over `ttf`, read little-endian.
pub const OTTO_MAGIC_U32: u32 = 0x4F54_544F;

/// The video/text fallback extension when no better one exists.
pub const BYTES_EXTENSION: &str = "bytes";
/// The text fallback extension.
pub const TXT_EXTENSION: &str = "txt";
/// The JSON extension for sniffed JSON text.
pub const JSON_EXTENSION: &str = "json";

/// `GetTextureExportFormat`: the encoder format behind the extension selection.
///
/// Mirrors `ImageExportFormat::texture_extension` but returns the format value.
pub fn texture_export_format(
    prefer_original_texture_extension: bool,
    image_export_format: ImageExportFormat,
    best_extension: Option<&str>,
) -> ImageExportFormat {
    if prefer_original_texture_extension {
        if let Some(original) = best_extension {
            if let Some(format) = ImageExportFormat::from_extension(original) {
                return format;
            }
        }
    }
    image_export_format
}

/// The best extension: the override extension, else the original extension.
pub const fn best_extension<'a>(
    override_extension: Option<&'a str>,
    original_extension: Option<&'a str>,
) -> Option<&'a str> {
    match override_extension {
        Some(extension) => Some(extension),
        None => original_extension,
    }
}

/// One dangerous-extension probe: ordinal, case-insensitive like the source set.
///
/// Only ASCII case pairs occur in the eleven entries; wider Unicode folding stays open.
pub fn is_dangerous_extension(extension: &str) -> bool {
    DANGEROUS_EXTENSIONS
        .iter()
        .any(|dangerous| dangerous.eq_ignore_ascii_case(extension))
}

/// `TextAssetExportCollection.GetExtension`: JSON, plain text, or binary by content.
pub fn sniff_text_extension(text: &str) -> &'static str {
    if is_valid_json_text(text) {
        JSON_EXTENSION
    } else if is_plain_text(text) {
        TXT_EXTENSION
    } else {
        BYTES_EXTENSION
    }
}

/// `TextAssetExportCollection.GetExportExtension` over caller-supplied asset facts.
///
/// A present best extension wins unless dangerous; otherwise the mode decides, with `Parse`
/// sniffing content. The null-only check is mirrored: an empty-but-present extension flows
/// through to the dangerous probe and is returned verbatim when safe.
pub fn text_export_extension(
    best_extension: Option<&str>,
    mode: TextExportMode,
    text: &str,
) -> String {
    if let Some(extension) = best_extension {
        if is_dangerous_extension(extension) {
            return TXT_EXTENSION.to_owned();
        }
        return extension.to_owned();
    }
    match mode {
        TextExportMode::Txt => TXT_EXTENSION.to_owned(),
        TextExportMode::Parse => sniff_text_extension(text).to_owned(),
        _ => BYTES_EXTENSION.to_owned(),
    }
}

/// `IsPlainText`: every character is non-control or whitespace.
///
/// Both classes are Unicode categories, matching the source exactly.
pub fn is_plain_text(text: &str) -> bool {
    text.chars().all(|c| !c.is_control() || c.is_whitespace())
}

/// Maximum `JsonDocument` nesting depth accepted by the default options.
pub const MAX_JSON_DEPTH: usize = 64;

/// JSON text validity behind `IsValidJson`.
///
/// Accepts one complete JSON value with surrounding whitespace only, enforcing the grammar
/// (objects, arrays, strings with escapes, well-formed numbers, `true`/`false`/`null`) and
/// the default maximum depth. Malformed numbers, bad escapes, trailing tokens, and
/// over-deep nesting fail. Number-range and encoder subtleties beyond the grammar stay open.
pub fn is_valid_json_text(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut parser = JsonValidator {
        bytes,
        position: 0,
        depth: 0,
    };
    parser.skip_whitespace();
    if !parser.parse_value() {
        return false;
    }
    parser.skip_whitespace();
    parser.position == bytes.len()
}

struct JsonValidator<'a> {
    bytes: &'a [u8],
    position: usize,
    depth: usize,
}

impl JsonValidator<'_> {
    fn skip_whitespace(&mut self) {
        while self.position < self.bytes.len()
            && matches!(self.bytes[self.position], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.position += 1;
        }
    }

    fn consume(&mut self, token: &str) -> bool {
        if self.bytes.len() - self.position >= token.len()
            && &self.bytes[self.position..self.position + token.len()] == token.as_bytes()
        {
            self.position += token.len();
            true
        } else {
            false
        }
    }

    fn parse_value(&mut self) -> bool {
        self.skip_whitespace();
        if self.position >= self.bytes.len() {
            return false;
        }
        match self.bytes[self.position] {
            b'{' => self.parse_object(),
            b'[' => self.parse_array(),
            b'"' => self.parse_string(),
            b't' => self.consume("true"),
            b'f' => self.consume("false"),
            b'n' => self.consume("null"),
            _ => self.parse_number(),
        }
    }

    fn enter_depth(&mut self) -> bool {
        if self.depth >= MAX_JSON_DEPTH {
            return false;
        }
        self.depth += 1;
        true
    }

    fn parse_object(&mut self) -> bool {
        if !self.enter_depth() {
            return false;
        }
        self.position += 1;
        self.skip_whitespace();
        if self.position < self.bytes.len() && self.bytes[self.position] == b'}' {
            self.position += 1;
            self.depth -= 1;
            return true;
        }
        loop {
            self.skip_whitespace();
            if self.position >= self.bytes.len() || self.bytes[self.position] != b'"' {
                return false;
            }
            if !self.parse_string() {
                return false;
            }
            self.skip_whitespace();
            if self.position >= self.bytes.len() || self.bytes[self.position] != b':' {
                return false;
            }
            self.position += 1;
            if !self.parse_value() {
                return false;
            }
            self.skip_whitespace();
            if self.position >= self.bytes.len() {
                return false;
            }
            match self.bytes[self.position] {
                b',' => self.position += 1,
                b'}' => {
                    self.position += 1;
                    self.depth -= 1;
                    return true;
                }
                _ => return false,
            }
        }
    }

    fn parse_array(&mut self) -> bool {
        if !self.enter_depth() {
            return false;
        }
        self.position += 1;
        self.skip_whitespace();
        if self.position < self.bytes.len() && self.bytes[self.position] == b']' {
            self.position += 1;
            self.depth -= 1;
            return true;
        }
        loop {
            if !self.parse_value() {
                return false;
            }
            self.skip_whitespace();
            if self.position >= self.bytes.len() {
                return false;
            }
            match self.bytes[self.position] {
                b',' => self.position += 1,
                b']' => {
                    self.position += 1;
                    self.depth -= 1;
                    return true;
                }
                _ => return false,
            }
        }
    }

    fn parse_string(&mut self) -> bool {
        // The caller guarantees an opening quote.
        self.position += 1;
        let mut chars = self.bytes[self.position..].iter();
        loop {
            let byte = match chars.next() {
                Some(byte) => *byte,
                None => return false,
            };
            self.position += 1;
            match byte {
                b'"' => return true,
                b'\\' => {
                    let escape = match chars.next() {
                        Some(escape) => *escape,
                        None => return false,
                    };
                    self.position += 1;
                    match escape {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            for _ in 0..4 {
                                match chars.next() {
                                    Some(digit) if digit.is_ascii_hexdigit() => {
                                        self.position += 1;
                                    }
                                    _ => return false,
                                }
                            }
                        }
                        _ => return false,
                    }
                }
                byte if byte < 0x20 => return false,
                _ => {}
            }
        }
    }

    fn parse_number(&mut self) -> bool {
        let start = self.position;
        if self.position < self.bytes.len() && self.bytes[self.position] == b'-' {
            self.position += 1;
        }
        let digits_start = self.position;
        while self.position < self.bytes.len() && self.bytes[self.position].is_ascii_digit() {
            self.position += 1;
        }
        if self.position == digits_start {
            self.position = start;
            return false;
        }
        // The grammar forbids leading zeros; negative zero alone stays valid.
        if self.position - digits_start > 1 && self.bytes[digits_start] == b'0' {
            self.position = start;
            return false;
        }
        if self.position < self.bytes.len() && self.bytes[self.position] == b'.' {
            self.position += 1;
            let fraction_start = self.position;
            while self.position < self.bytes.len() && self.bytes[self.position].is_ascii_digit() {
                self.position += 1;
            }
            if self.position == fraction_start {
                self.position = start;
                return false;
            }
        }
        if self.position < self.bytes.len()
            && (self.bytes[self.position] == b'e' || self.bytes[self.position] == b'E')
        {
            self.position += 1;
            if self.position < self.bytes.len()
                && (self.bytes[self.position] == b'+' || self.bytes[self.position] == b'-')
            {
                self.position += 1;
            }
            let exponent_start = self.position;
            while self.position < self.bytes.len() && self.bytes[self.position].is_ascii_digit() {
                self.position += 1;
            }
            if self.position == exponent_start {
                self.position = start;
                return false;
            }
        }
        true
    }
}

/// `FontExtensions.GetFontExtension`: `otf` for the `OTTO` magic, else `ttf`.
///
/// Short data falls back to `ttf` exactly like the source "just in case" path.
pub fn font_extension(font_data: &[u8]) -> &'static str {
    if font_data.len() >= 4 {
        let magic = u32::from_le_bytes([font_data[0], font_data[1], font_data[2], font_data[3]]);
        if magic == OTTO_MAGIC_U32 {
            return "otf";
        }
    }
    "ttf"
}

/// `AssetAPI.TryGetFontData`'s magic table: the extension and MIME type for font bytes.
///
/// The frozen route sniffs the first four bytes directly rather than reusing
/// `FontExtensions.GetFontExtension`, and it recognizes one case that helper does not: the
/// `ttcf` TrueType-collection magic. Anything else falls back to a generic binary, which is
/// what lets the route report success for data it cannot name.
///
/// `None` means the data is too short to identify, matching the frozen `Length: >= 4` test;
/// the route treats that as "no font data".
pub fn font_extension_and_mime(data: &[u8]) -> Option<(&'static str, &'static str)> {
    let magic = data.get(..4)?;
    Some(match magic {
        [0x4F, 0x54, 0x54, 0x4F] => ("otf", "font/otf"),
        [0x00, 0x01, 0x00, 0x00] => ("ttf", "font/ttf"),
        [0x74, 0x74, 0x63, 0x66] => ("ttc", "font/collection"),
        _ => ("dat", "application/octet-stream"),
    })
}

/// Read a `Font` payload's `m_FontData` as bytes.
///
/// The reference `IFont.FontData` exposes the field as bytes; the port's decoded value keeps it
/// either as a `Bytes` leaf or as a byte array, so both spellings are accepted. `None` means the
/// field is absent, which the frozen `HasFontData` treats as no font data.
///
/// Errors name the offending element rather than silently dropping it, matching the export
/// path's own refusal for a malformed array.
pub fn font_data_bytes(root: &SerializedValue) -> Result<Option<Vec<u8>>, FontDataReadError> {
    let Some(value) = root.unique_field_in_base_chain("m_FontData")? else {
        return Ok(None);
    };
    match value {
        SerializedValue::Bytes(bytes) => Ok(Some(bytes.clone())),
        SerializedValue::Array(values) => {
            let mut output = Vec::new();
            for (index, value) in values.iter().enumerate() {
                let SerializedValue::U8(value) = value else {
                    return Err(FontDataReadError::NotAByte { index });
                };
                output.push(*value);
            }
            Ok(Some(output))
        }
        _ => Err(FontDataReadError::WrongType),
    }
}

/// Why a `Font.m_FontData` field could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontDataReadError {
    /// A `m_FontData` array element is not a byte.
    NotAByte {
        /// The offending element index.
        index: usize,
    },
    /// `m_FontData` is neither bytes nor an array of bytes.
    WrongType,
    /// The field lookup failed.
    Lookup(String),
}

impl std::fmt::Display for FontDataReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAByte { index } => {
                write!(formatter, "Font m_FontData[{index}] is not a byte")
            }
            Self::WrongType => write!(
                formatter,
                "Font m_FontData is neither bytes nor an array of bytes"
            ),
            Self::Lookup(message) => write!(formatter, "Font m_FontData lookup failed: {message}"),
        }
    }
}

impl std::error::Error for FontDataReadError {}

impl From<crate::serialized_file::object::SerializedValueLookupError> for FontDataReadError {
    fn from(error: crate::serialized_file::object::SerializedValueLookupError) -> Self {
        Self::Lookup(error.to_string())
    }
}

/// `VideoClipExtensions.TryGetExtensionFromPath`: the original path's extension, dot stripped.
///
/// Delegates to [`crate::export::asset_path::get_extension`], which is `Path.GetExtension` and is
/// replayed against the reference runtime by
/// `asset_path::get_extension_matches_the_reference_runtime_rows`. That matters for a reason worth
/// recording: the earlier version of this function split on `/` alone, so a Windows-style path such
/// as `C:\dir.d\x` returned `d\x` where the reference - running on Windows - returns nothing,
/// because a backslash **is** a separator there. The host platform is therefore a parameter rather
/// than an assumption.
pub fn video_extension_from_path(original_path: &str, platform: HostPathPlatform) -> Option<&str> {
    let extension = crate::export::asset_path::get_extension(original_path, platform);
    // `Path.GetExtension` never returns a bare dot, so a non-empty result always has at least one
    // character after it.
    extension.strip_prefix('.').filter(|rest| !rest.is_empty())
}

/// `VideoClipExtensions.GetExtensionFromPath`: the path extension or `bytes`.
pub fn video_export_extension(original_path: &str, platform: HostPathPlatform) -> &str {
    video_extension_from_path(original_path, platform).unwrap_or(BYTES_EXTENSION)
}

#[cfg(test)]
mod tests {
    use super::super::asset_path::HostPathPlatform;
    use super::super::settings::{ImageExportFormat, TextExportMode};
    use super::{
        best_extension, font_extension, is_dangerous_extension, is_plain_text, is_valid_json_text,
        sniff_text_extension, text_export_extension, texture_export_format, video_export_extension,
        video_extension_from_path, BYTES_EXTENSION, DANGEROUS_EXTENSIONS, JSON_EXTENSION,
        OTTO_MAGIC_U32, TXT_EXTENSION,
    };

    #[test]
    fn texture_formats_follow_preference_and_recognition() {
        use ImageExportFormat::{Jpeg, Png};
        // A preferred, recognized original wins as both extension source and format.
        assert_eq!(texture_export_format(true, Png, Some("jpg")), Jpeg);
        // Unrecognized or missing originals fall back to the configured format.
        assert_eq!(texture_export_format(true, Png, Some("dds")), Png);
        assert_eq!(texture_export_format(true, Png, None), Png);
        assert_eq!(texture_export_format(false, Png, Some("jpg")), Png);
        // Recognition is case-sensitive lowercase: JPG is not an image format here.
        assert_eq!(texture_export_format(true, Png, Some("JPG")), Png);
        assert_eq!(best_extension(Some("a"), Some("b")), Some("a"));
        assert_eq!(best_extension(None, Some("b")), Some("b"));
        assert_eq!(best_extension(None, None), None);
    }

    #[test]
    fn dangerous_extensions_remap_while_safe_ones_pass_through() {
        assert_eq!(DANGEROUS_EXTENSIONS.len(), 11);
        for extension in ["cs", "DLL", "Exe", "PS1", "com"] {
            assert!(is_dangerous_extension(extension), "{extension}");
        }
        assert!(!is_dangerous_extension("txt"));
        assert!(!is_dangerous_extension("json"));
        assert!(!is_dangerous_extension(""));
        assert_eq!(
            text_export_extension(Some("cs"), TextExportMode::Bytes, ""),
            "txt"
        );
        assert_eq!(
            text_export_extension(Some("json"), TextExportMode::Bytes, ""),
            "json"
        );
        // An empty-but-present extension flows through the null-only check verbatim.
        assert_eq!(
            text_export_extension(Some(""), TextExportMode::Bytes, ""),
            ""
        );
        assert_eq!(
            text_export_extension(None, TextExportMode::Txt, "{}"),
            TXT_EXTENSION
        );
        assert_eq!(
            text_export_extension(None, TextExportMode::Bytes, "{}"),
            BYTES_EXTENSION
        );
        assert_eq!(
            text_export_extension(None, TextExportMode::Parse, "{}"),
            JSON_EXTENSION
        );
    }

    #[test]
    fn text_sniffing_separates_json_plain_and_binary() {
        assert_eq!(sniff_text_extension("{}"), JSON_EXTENSION);
        assert_eq!(
            sniff_text_extension("  { \"a\": [1, -2.5e3, true, null] }  "),
            JSON_EXTENSION
        );
        assert_eq!(sniff_text_extension("hello world"), TXT_EXTENSION);
        assert_eq!(sniff_text_extension(""), TXT_EXTENSION);
        assert_eq!(sniff_text_extension("a\x07b"), BYTES_EXTENSION);
        assert!(is_plain_text("tab\there newline\nand ünicode"));
        assert!(!is_plain_text("bell\x07"));
        // Whitespace-only controls stay plain text.
        assert!(is_plain_text(" \t\r\n"));
    }

    #[test]
    fn json_validity_follows_grammar_and_depth() {
        for valid in [
            "{}",
            "[]",
            "{\"a\":1}",
            "[1,[2,{\"b\":null}]]",
            "  42  ",
            "-0.5e-2",
            "\"esc \\\" \\u00e9\"",
            "true",
            "\"str\"",
        ] {
            assert!(is_valid_json_text(valid), "{valid}");
        }
        for invalid in [
            "",
            "   ",
            "{",
            "{\"a\":}",
            "{\"a\":1,}",
            "[1,]",
            "{'a':1}",
            "NaN",
            "01",
            "1.",
            "[1 2]",
            "{} trailing",
            "\"bad \x01\"",
            "\"bad \\x\"",
            "\"bad \\u12\"",
        ] {
            assert!(!is_valid_json_text(invalid), "{invalid}");
        }
        // Nesting past the default maximum depth fails.
        let deep = format!("{}{}{}", "[".repeat(65), "1", "]".repeat(65));
        assert!(!is_valid_json_text(&deep));
        let shallow = format!("{}{}{}", "[".repeat(64), "1", "]".repeat(64));
        assert!(is_valid_json_text(&shallow));
    }

    #[test]
    fn font_magic_selects_otf_with_ttf_fallback() {
        assert_eq!(OTTO_MAGIC_U32, 0x4F54_544F);
        assert_eq!(font_extension(&[0x4F, 0x54, 0x54, 0x4F, 0x00]), "otf");
        assert_eq!(font_extension(&[0x00, 0x01, 0x00, 0x00]), "ttf");
        assert_eq!(font_extension(&[0x4F, 0x54, 0x54]), "ttf");
        assert_eq!(font_extension(&[]), "ttf");
        // Little-endian read: near-magic bytes with a wrong tail are not magic.
        assert_eq!(font_extension(&[0x4F, 0x54, 0x54, 0x51]), "ttf");
    }

    #[test]
    fn video_extensions_come_from_the_path_or_bytes() {
        let windows = HostPathPlatform::Windows;
        assert_eq!(
            video_extension_from_path("Assets/Videos/clip.mp4", windows),
            Some("mp4")
        );
        assert_eq!(video_extension_from_path("clip.MP4", windows), Some("MP4"));
        assert_eq!(video_extension_from_path("a.b/c", windows), None);
        assert_eq!(video_extension_from_path("clip.", windows), None);
        assert_eq!(video_extension_from_path("clip", windows), None);
        assert_eq!(
            video_export_extension("Assets/Videos/clip.mp4", windows),
            "mp4"
        );
        assert_eq!(video_export_extension("clip", windows), BYTES_EXTENSION);
    }

    /// The separator set is the host's, which the runtime fixture pins for `Path.GetExtension`.
    ///
    /// The earlier implementation split on `/` alone, so it read an extension out of a
    /// Windows-style path where the reference - running on Windows - finds none.
    #[test]
    fn video_extension_separators_follow_the_platform() {
        let windows = HostPathPlatform::Windows;
        let unix = HostPathPlatform::Unix;
        assert_eq!(
            video_extension_from_path("C:\\dir.d\\x", windows),
            None,
            "a backslash separates on Windows"
        );
        assert_eq!(
            video_export_extension("C:\\dir.d\\x", windows),
            BYTES_EXTENSION
        );
        assert_eq!(
            video_extension_from_path("C:\\dir.d\\x", unix),
            Some("d\\x"),
            "a backslash is an ordinary character on Unix, so the dot after it wins"
        );
    }
}
