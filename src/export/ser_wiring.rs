//! Lane integration surface for the serialization, asset-factory, and reference-graph workstream.
//!
//! This module exists so that workstream can wire its ported kernels into the product path
//! without editing `src/main.rs`, `src/export/project_workflow.rs`, or another lane's files.
//!
//! The entry point below is the product call the export spine adds:
//!
//! ```ignore
//! let layouts = assetripper_rs::export::ser_wiring::reference_class_layouts();
//! let assets = assetripper_rs::export::ser_wiring::materialize_workspace_file(
//!     &parsed_serialized_file,
//!     &loaded_file.collection_name, // `FileBase.NameFixed`
//!     &loaded_file.file_path,
//!     &loaded_file.platform_display, // `BuildTarget.ToString()`
//!     &ObjectDecodeLimits::default(),
//! )?;
//! ```
//!
//! `materialize_workspace_file` runs the ported `GameAssetFactory` dispatch over the
//! file's real object bytes: generated-class identity, TPK-backed `TypeTreeObject`,
//! `UnknownObject`, and `UnreadableObject` fallbacks, plus the version gate, the
//! engine-collection tolerance, and the Patch retry. `reference_class_layouts()` is the
//! reference-pinned class table (committed probe output) the dispatch consults, the
//! Rust equivalent of `AssetFactory.CreateSerialized` + `TypeTreeNodeStruct.TryMakeFromTpk`.

use std::sync::OnceLock;

use crate::processing::asset_factory::{
    materialize_file_objects, FactoryAsset, FactoryEngineError, ReferenceClassLayouts,
    PROBE_FIXTURE,
};
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::object::ObjectDecodeLimits;

/// The reference-pinned class identity and TPK layout tables.
pub fn reference_class_layouts() -> &'static ReferenceClassLayouts {
    static LAYOUTS: OnceLock<ReferenceClassLayouts> = OnceLock::new();
    LAYOUTS.get_or_init(|| ReferenceClassLayouts::parse(PROBE_FIXTURE))
}

/// Materialize every object of one parsed SerializedFile through the ported factory.
pub fn materialize_workspace_file(
    file: &SerializedFile<'_>,
    collection_name: &str,
    file_path: &str,
    platform: &str,
    limits: &ObjectDecodeLimits,
) -> Result<Vec<FactoryAsset>, FactoryEngineError> {
    materialize_file_objects(
        file,
        collection_name,
        file_path,
        platform,
        reference_class_layouts(),
        limits,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::asset_factory::FactoryAssetKind;

    /// The wired entry point reproduces the reference factory for the probe fixture's
    /// unknown-class case (`UnknownObject` with the synthetic name) and class-id 7
    /// has no generated class in the pinned table.
    #[test]
    fn wired_factory_reproduces_the_unknown_object_case() {
        let fixture = PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|unknown_class|"))
            .expect("the unknown_class case is committed");
        let bytes = decode_hex(hex);
        let file = SerializedFile::parse(&bytes).expect("fixture parses");
        let assets = materialize_workspace_file(
            &file,
            "unknown_class.assets",
            "",
            "StandaloneWin64Player",
            &ObjectDecodeLimits::default(),
        )
        .expect("materialization");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].kind, FactoryAssetKind::Unknown);
        assert_eq!(assets[0].class_name.as_ref(), "7");
        assert_eq!(assets[0].name, "Unknown7_3FCA88C5");
        assert_eq!(assets[0].raw, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn reference_layouts_cover_the_probed_version() {
        let layouts = reference_class_layouts();
        assert_eq!(layouts.probed_version(), Some("2019.4.40f1"));
    }

    fn decode_hex(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2), "odd hex length");
        let bytes = hex.as_bytes();
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            other => panic!("invalid hex byte {other}"),
        };
        (0..bytes.len() / 2)
            .map(|index| (digit(bytes[index * 2]) << 4) | digit(bytes[index * 2 + 1]))
            .collect()
    }
}
