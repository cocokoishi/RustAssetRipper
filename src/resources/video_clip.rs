//! `VideoClipExtensions`.
//!
//! Mirrors the frozen extension methods the browser video route depends on:
//! `CheckIntegrity` (which delegates to `StreamedResourceExtensions.CheckIntegrity` over the
//! clip's `m_ExternalResources`), `TryGetContent`, and `TryGetExtensionFromPath` (which reads
//! the clip's `m_OriginalPath`).
//!
//! The frozen `TryGetExtensionFromPath` returns `Path.GetExtension(...)` with the leading dot
//! stripped, and fails when the path has no extension at all - so a path ending in a dot or
//! holding no dot reports no extension, while an empty path reports none too.

use crate::resource::{StreamedResourceInfo, StreamedResourceInfoError};
use crate::serialized_file::object::{
    SerializedFieldValue, SerializedValue, SerializedValueLookupError,
};

/// The class-329 `VideoClip` fields the browser video route reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VideoClipView {
    /// The `m_ExternalResources` streamed-resource descriptor, when present.
    pub external_resources: Option<StreamedResourceInfo>,
    /// Raw `m_OriginalPath` text, when present.
    pub original_path: Option<String>,
}

impl VideoClipView {
    /// Reads the route-relevant fields from a decoded `VideoClip` payload.
    pub fn from_value(root: &SerializedValue) -> Result<Self, VideoClipViewError> {
        Ok(Self {
            external_resources: optional_stream(root, EXTERNAL_RESOURCES_FIELD)?,
            original_path: optional_string(root, ORIGINAL_PATH_FIELD)?,
        })
    }

    /// The descriptor-local part of `VideoClipExtensions.CheckIntegrity`.
    ///
    /// The frozen method also resolves the resource in the owning collection. Callers that have
    /// that collection must use the resource resolver and pass its answer to the export gate. This
    /// method mirrors the checks available from the decoded descriptor alone: an empty path is
    /// considered intact before the size checks, while a named path needs a non-empty range whose
    /// end fits the source's signed stream limit.
    pub fn check_integrity(&self) -> bool {
        let Some(info) = self.external_resources.as_ref() else {
            return false;
        };
        if !info.is_set() {
            return true;
        }
        let maximum = i64::MAX as u64;
        let size = u64::from(info.size);
        size > 0
            && info.offset <= maximum
            && info
                .offset
                .checked_add(size)
                .is_some_and(|end| end <= maximum)
    }

    /// `VideoClipExtensions.TryGetExtensionFromPath`: the original path's extension, dot
    /// stripped, or `None` when the path carries no extension.
    pub fn try_get_extension_from_path(&self) -> Option<&str> {
        let path = self.original_path.as_deref()?;
        // `Path.GetExtension` is host-dependent: a backslash separates on Windows and does not on
        // Unix, so the host decides rather than a hard-coded separator set.
        crate::export::resource_extensions::video_extension_from_path(
            path,
            crate::export::asset_path::HostPathPlatform::host(),
        )
    }

    /// `VideoClipExtensions.GetExtensionFromPath`: the extension or the `bytes` fallback.
    pub fn get_extension_from_path(&self) -> &str {
        self.try_get_extension_from_path().unwrap_or("bytes")
    }
}

/// The `m_ExternalResources` field name.
pub const EXTERNAL_RESOURCES_FIELD: &str = "m_ExternalResources";
/// The `m_OriginalPath` field name.
pub const ORIGINAL_PATH_FIELD: &str = "m_OriginalPath";

/// Why a `VideoClip` payload's route fields could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VideoClipViewError {
    /// The `m_ExternalResources` descriptor is malformed.
    StreamInfo {
        /// The offending field name.
        field: &'static str,
        /// The underlying descriptor error.
        source: StreamedResourceInfoError,
    },
    /// A field held an unexpected value type.
    WrongType {
        /// The offending field name.
        field: &'static str,
        /// A readable kind name for the value found.
        actual: &'static str,
    },
    /// The field lookup failed.
    Lookup(SerializedValueLookupError),
}

impl std::fmt::Display for VideoClipViewError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StreamInfo { field, source } => {
                write!(
                    formatter,
                    "VideoClip.{field} is not a valid streamed resource: {source}"
                )
            }
            Self::WrongType { field, actual } => {
                write!(formatter, "VideoClip.{field} has unexpected type {actual}")
            }
            Self::Lookup(error) => write!(formatter, "VideoClip field lookup failed: {error}"),
        }
    }
}

impl std::error::Error for VideoClipViewError {}

impl From<SerializedValueLookupError> for VideoClipViewError {
    fn from(error: SerializedValueLookupError) -> Self {
        Self::Lookup(error)
    }
}

/// Look a field up in the base chain, mirroring the reference's inherited-field access.
fn lookup<'value>(
    root: &'value SerializedValue,
    name: &'static str,
) -> Result<Option<&'value SerializedValue>, SerializedValueLookupError> {
    root.unique_field_in_base_chain(name)
}

fn optional_stream(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<StreamedResourceInfo>, VideoClipViewError> {
    let Some(value) = lookup(root, name)? else {
        return Ok(None);
    };
    // Class 329 serializes `m_ExternalResources` as a `StreamedResource`, whose fields are
    // `m_Source`, `m_Offset` and `m_Size`. Newer `StreamingInfo` layouts spell the same three
    // fields `path`, `offset` and `size`. Both describe the identical descriptor, so the
    // reader accepts either spelling rather than forcing one class onto the other's layout.
    let normalized = normalize_resource_fields(value);
    let source: &SerializedValue = normalized.as_ref().unwrap_or(value);
    crate::resource::extract_streamed_resource_info(source)
        .map(Some)
        .map_err(|source| VideoClipViewError::StreamInfo {
            field: name,
            source,
        })
}

/// Rewrite a `StreamedResource`-spelled descriptor into the `StreamingInfo` names.
///
/// Returns `None` when the value already uses the `StreamingInfo` names, so the common path
/// borrows the decoded value without copying.
///
/// The two shapes also disagree on widths: `StreamedResource.m_Size` is a `UInt64` while
/// `StreamingInfo.size` is a `UInt32`. Narrowing here is safe because
/// [`crate::resource::extract_streamed_resource_info`] rejects sizes that do not fit, so a value
/// above `u32::MAX` still surfaces as an error rather than being silently wrapped.
fn normalize_resource_fields(value: &SerializedValue) -> Option<SerializedValue> {
    let SerializedValue::Struct(fields) = value else {
        return None;
    };
    let renamed = [
        ("m_Source", "path"),
        ("m_Offset", "offset"),
        ("m_Size", "size"),
    ];
    if !fields
        .iter()
        .any(|field| renamed.iter().any(|(from, _)| field.name == *from))
    {
        return None;
    }
    Some(SerializedValue::Struct(
        fields
            .iter()
            .map(|field| {
                let name = renamed
                    .iter()
                    .find(|(from, _)| field.name == *from)
                    .map(|(_, to)| (*to).to_owned())
                    .unwrap_or_else(|| field.name.clone());
                let value = if let SerializedValue::U64(size) = field.value {
                    match u32::try_from(size) {
                        Ok(size) => SerializedValue::U32(size),
                        // Out of range: keep the wide value so the reader reports the type
                        // mismatch rather than this function inventing a wrapped size.
                        Err(_) => SerializedValue::U64(size),
                    }
                } else {
                    field.value.clone()
                };
                SerializedFieldValue { name, value }
            })
            .collect(),
    ))
}

fn optional_string(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<String>, VideoClipViewError> {
    match lookup(root, name)? {
        None => Ok(None),
        Some(SerializedValue::StringBytes(bytes)) => {
            Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
        }
        Some(other) => Err(VideoClipViewError::WrongType {
            field: name,
            actual: serialized_kind(other),
        }),
    }
}

/// A short readable name for a value's variant, used in diagnostics.
fn serialized_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "I8",
        SerializedValue::U8(_) => "U8",
        SerializedValue::I16(_) => "I16",
        SerializedValue::U16(_) => "U16",
        SerializedValue::Char(_) => "Char",
        SerializedValue::I32(_) => "I32",
        SerializedValue::U32(_) => "U32",
        SerializedValue::I64(_) => "I64",
        SerializedValue::U64(_) => "U64",
        SerializedValue::F32(_) => "F32",
        SerializedValue::F64(_) => "F64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::ManagedReferenceId { .. } => "managed reference id",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn text(value: &str) -> SerializedValue {
        SerializedValue::StringBytes(value.as_bytes().to_vec())
    }

    fn stream(path: &str, offset: u64, size: u32) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("path", text(path)),
            field("offset", SerializedValue::U64(offset)),
            field("size", SerializedValue::U32(size)),
        ])
    }

    fn clip(path: &str, original: &str, size: u32) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("m_ExternalResources", stream(path, 16, size)),
            field("m_OriginalPath", text(original)),
        ])
    }

    #[test]
    fn descriptor_integrity_matches_stream_gate_prechecks() {
        let view = VideoClipView::from_value(&clip("archive:/video", "video.mp4", 64)).unwrap();
        assert!(view.check_integrity());
        // A zero size is not intact, matching the streamed-resource gate.
        let view = VideoClipView::from_value(&clip("archive:/video", "video.mp4", 0)).unwrap();
        assert!(!view.check_integrity());
        // The source returns true for an empty path before inspecting its zero size.
        let view = VideoClipView::from_value(&clip("", "video.mp4", 64)).unwrap();
        assert!(view.check_integrity());
        let view = VideoClipView::from_value(&clip("", "video.mp4", 0)).unwrap();
        assert!(view.check_integrity());
        // An absent descriptor is not intact.
        let view = VideoClipView::from_value(&SerializedValue::Struct(vec![field(
            "m_OriginalPath",
            text("video.mp4"),
        )]))
        .unwrap();
        assert!(!view.check_integrity());
        assert!(view.external_resources.is_none());

        // Named ranges that cannot fit the source's signed stream position are declined.
        let overflow = SerializedValue::Struct(vec![
            field(
                "m_ExternalResources",
                SerializedValue::Struct(vec![
                    field("path", text("archive:/video")),
                    field("offset", SerializedValue::U64(i64::MAX as u64)),
                    field("size", SerializedValue::U32(1)),
                ]),
            ),
            field("m_OriginalPath", text("video.mp4")),
        ]);
        assert!(!VideoClipView::from_value(&overflow)
            .unwrap()
            .check_integrity());
    }

    #[test]
    fn extension_comes_from_the_original_path_only() {
        let cases = [
            ("video.mp4", Some("mp4")),
            ("Assets/clip.webm", Some("webm")),
            ("no-extension", None),
            ("trailing.", None),
            ("", None),
            ("archive:/a/b/movie.mov", Some("mov")),
        ];
        for (original, expected) in cases {
            let view = VideoClipView::from_value(&clip("archive:/v", original, 4)).unwrap();
            assert_eq!(view.try_get_extension_from_path(), expected, "{original}");
            assert_eq!(
                view.get_extension_from_path(),
                expected.unwrap_or("bytes"),
                "{original}"
            );
        }
        // An absent original path reports no extension rather than failing.
        let view = VideoClipView::from_value(&SerializedValue::Struct(vec![field(
            "m_ExternalResources",
            stream("archive:/v", 0, 4),
        )]))
        .unwrap();
        assert_eq!(view.try_get_extension_from_path(), None);
        assert_eq!(view.get_extension_from_path(), "bytes");
    }

    /// Class 329 spells the descriptor `StreamedResource` with `m_Source`/`m_Offset`/`m_Size`,
    /// while newer layouts use `StreamingInfo` with `path`/`offset`/`size`. Both must read.
    #[test]
    fn both_streamed_resource_spellings_read() {
        let streamed = SerializedValue::Struct(vec![
            field("m_Source", text("probe.resS")),
            field("m_Offset", SerializedValue::U64(64)),
            field("m_Size", SerializedValue::U64(128)),
        ]);
        let clip_value = SerializedValue::Struct(vec![
            field("m_ExternalResources", streamed),
            field("m_OriginalPath", text("Assets/movie.mp4")),
        ]);
        let view = VideoClipView::from_value(&clip_value).unwrap();
        assert!(view.check_integrity());
        let info = view.external_resources.as_ref().expect("descriptor reads");
        assert_eq!(info.path, "probe.resS");
        assert_eq!(info.offset, 64);
        assert_eq!(info.size, 128);
        assert_eq!(view.try_get_extension_from_path(), Some("mp4"));

        // The `StreamingInfo` spelling still reads through the untouched path.
        let info = VideoClipView::from_value(&clip("archive:/v", "movie.webm", 8))
            .unwrap()
            .external_resources
            .expect("descriptor reads");
        assert_eq!(info.path, "archive:/v");
    }

    #[test]
    fn malformed_fields_report_the_field_name() {
        let malformed = SerializedValue::Struct(vec![
            field("m_ExternalResources", SerializedValue::I32(1)),
            field("m_OriginalPath", text("video.mp4")),
        ]);
        let error = VideoClipView::from_value(&malformed).unwrap_err();
        assert!(error.to_string().contains("m_ExternalResources"), "{error}");

        let wrong_path =
            SerializedValue::Struct(vec![field("m_OriginalPath", SerializedValue::I32(3))]);
        let error = VideoClipView::from_value(&wrong_path).unwrap_err();
        assert_eq!(
            error,
            VideoClipViewError::WrongType {
                field: "m_OriginalPath",
                actual: "I32"
            }
        );
    }
}
