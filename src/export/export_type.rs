//! Export pointer types ported from `AssetType` and the exporter `ToExportType` matrix.
//!
//! `AssetType` lives in the snapshot (`AssetRipper.IO.Files/AssetType.cs`) with fixed numeric
//! values: `Internal = 0`, `Cached = 1`, `Serialized = 2`, `Meta = 3`. A binary asset keeps its
//! parameters in its `.meta` file (`Meta`); a serialized asset carries them inline (`Serialized`);
//! engine assets use `Internal`, and Unity 3 caches map to `Cached`.
//!
//! Which type an exporter reports is determined by its base class, with four direct
//! implementations repeating the binary answer and three special cases:
//!
//! - Every `YamlExporterBase` subclass reports `Serialized` from both `ToExportType` and
//!   `ToUnknownExportType`.
//! - Every `BinaryAssetExporter` subclass (including `ShaderExporterBase` and its shader
//!   exporters) reports `Meta` from both.
//! - `SceneAssetExporter`, `UnknownObjectExporter`, `UnreadableObjectExporter`, and
//!   `ScriptExporter` implement `IAssetExporter` directly and each report `Meta` from both.
//! - `EngineAssetsExporter` reports `Internal` from both.
//! - `DeletedAssetsExporter` throws from `ToExportType` and returns false from
//!   `ToUnknownExportType`: it never contributes a pointer type.
//! - `DummyAssetExporter.Get(isEmptyCollection, isMetaType)` reports `Meta` when `isMetaType`
//!   and `Serialized` otherwise, from both methods; the empty/skip choice only selects the
//!   collection behavior, never the type.
//!
//! `ProjectExporter.ToExportType` walks the handler stack and returns the first exporter whose
//! `ToUnknownExportType` accepts, throwing `NotSupportedException` when none does. Pointers
//! themselves follow `MetaPtr`: a zero GUID serializes as `{fileID}` alone, otherwise as
//! fileID plus GUID plus type, except that Unity versions below 4 substitute `Cached` for
//! `Meta` (only YAML meshes are exported, so the redirect is safe). Unknown assets map through
//! `MetaPtr.CreateMissingReference` with an explicit `Meta` type.

/// The snapshot `AssetType` with its fixed numeric values.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExportAssetType {
    /// Used by released games; engine-asset pointers use this.
    Internal = 0,
    /// Library cache files; the Unity 3 fallback for non-model `Meta` pointers.
    Cached = 1,
    /// Serialized asset files carrying their parameters inline.
    Serialized = 2,
    /// Binary asset files carrying their parameters in the `.meta` file.
    Meta = 3,
}

impl ExportAssetType {
    /// All variants in numeric order.
    pub const ALL: [Self; 4] = [Self::Internal, Self::Cached, Self::Serialized, Self::Meta];

    /// The `(int)` cast value used in pointer YAML.
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// The inverse of the `(int)` cast; `None` for undefined values.
    pub const fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Internal),
            1 => Some(Self::Cached),
            2 => Some(Self::Serialized),
            3 => Some(Self::Meta),
            _ => None,
        }
    }
}

/// What one exporter answers from `ToExportType` and `ToUnknownExportType`.
///
/// `None` on either method models refusal: `DeletedAssetsExporter` throws from `ToExportType`
/// and returns false from `ToUnknownExportType`, while `EngineAssetsExporter` answers
/// `Internal` from `ToExportType` but still returns false from `ToUnknownExportType`, so the
/// type stack skips both.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExporterExportType {
    /// The exporter type name.
    pub exporter: &'static str,
    /// The `ToExportType` answer, or `None` when it throws.
    pub to_export_type: Option<ExportAssetType>,
    /// The `ToUnknownExportType` answer, or `None` when it returns false.
    pub to_unknown_export_type: Option<ExportAssetType>,
}

/// Every concrete exporter in the snapshot with its pointer-type answer.
///
/// Order follows the exporter file inventory; resolution order comes from the registration
/// stack in `registration.rs`, not from this table.
pub const EXPORTER_EXPORT_TYPES: &[ExporterExportType] = &[
    ExporterExportType {
        exporter: "AnimatorControllerExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "AudioClipExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "AudioMixerExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "DefaultYamlExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "DeletedAssetsExporter",
        to_export_type: None,
        to_unknown_export_type: None,
    },
    ExporterExportType {
        exporter: "DummyShaderTextExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "EngineAssetsExporter",
        to_export_type: Some(ExportAssetType::Internal),
        // Answers Internal but still returns false, so the type stack skips it.
        to_unknown_export_type: None,
    },
    ExporterExportType {
        exporter: "FontAssetExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "LightmapTextureAssetExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "ManagerAssetExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "MovieTextureAssetExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "NativeAudioExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "RawTextureExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "SceneAssetExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "SceneYamlExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "ScriptExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "ScriptableObjectExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "ScriptableObjectGroupExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "SimpleShaderExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "TerrainYamlExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "TextAssetExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "TextureArrayAssetExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "TextureAssetExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "UnknownObjectExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "UnreadableObjectExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "VideoClipExporter",
        to_export_type: Some(ExportAssetType::Meta),
        to_unknown_export_type: Some(ExportAssetType::Meta),
    },
    ExporterExportType {
        exporter: "YamlAudioExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "YamlShaderExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "YamlSpriteExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
    ExporterExportType {
        exporter: "YamlStreamedAssetExporter",
        to_export_type: Some(ExportAssetType::Serialized),
        to_unknown_export_type: Some(ExportAssetType::Serialized),
    },
];

/// `DummyAssetExporter.Get(isEmptyCollection, isMetaType)` type answer.
///
/// The empty/skip choice selects collection behavior only; the pointer type follows
/// `isMetaType` alone.
pub const fn dummy_export_type(is_meta_type: bool) -> ExportAssetType {
    if is_meta_type {
        ExportAssetType::Meta
    } else {
        ExportAssetType::Serialized
    }
}

/// Look up one exporter's type answers by name.
pub fn export_type_for(exporter: &str) -> Option<ExporterExportType> {
    EXPORTER_EXPORT_TYPES
        .iter()
        .find(|entry| entry.exporter == exporter)
        .copied()
}

/// Failure to resolve a pointer type: no exporter on the stack accepts the asset type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NoExportTypeError;

impl core::fmt::Display for NoExportTypeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "there is no exporter that knows the asset type for this asset"
        )
    }
}

impl std::error::Error for NoExportTypeError {}

/// `ProjectExporter.ToExportType`: the first stack answer that accepts wins.
///
/// Each element is one stack exporter's `ToUnknownExportType` outcome in stack order:
/// `Some` accepts with that type, `None` declines like `DeletedAssetsExporter`. An empty
/// acceptance is the `NotSupportedException` path.
pub fn resolve_stack_export_type(
    stack_answers: &[Option<ExportAssetType>],
) -> Result<ExportAssetType, NoExportTypeError> {
    if let Some(asset_type) = stack_answers.iter().flatten().next() {
        return Ok(*asset_type);
    }
    Err(NoExportTypeError)
}

/// The Unity 3 `Meta`-to-`Cached` redirect in `MetaPtr.ExportYaml`.
///
/// Below major version 4 a `Meta` pointer serializes as `Cached`; every other combination
/// keeps its type. Only the major version participates in the comparison.
pub const fn yaml_pointer_type(
    asset_type: ExportAssetType,
    export_major_version: u32,
) -> ExportAssetType {
    match (asset_type, export_major_version < 4) {
        (ExportAssetType::Meta, true) => ExportAssetType::Cached,
        _ => asset_type,
    }
}

/// The pointer type in `PrefabExportCollection.GenerateMetaPtrForPrefab`.
///
/// A legacy prefab marker (`EmitPrefabAsset`) points at `Serialized`; a modern prefab instance
/// points at `Meta`.
pub const fn prefab_pointer_type(emit_prefab_asset: bool) -> ExportAssetType {
    if emit_prefab_asset {
        ExportAssetType::Serialized
    } else {
        ExportAssetType::Meta
    }
}

/// One rendered export pointer in the `{fileID} / {fileID, guid, type}` mapping form.
///
/// `guid_text` carries the already-rendered GUID, or `None` for a zero GUID (local pointer).
/// The YAML form applies the Unity 3 redirect; the string form always prints the true type.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedPointer {
    pub yaml: String,
    pub display: String,
}

pub fn render_pointer(
    file_id: i64,
    guid_text: Option<&str>,
    asset_type: ExportAssetType,
    export_major_version: u32,
) -> RenderedPointer {
    match guid_text {
        None => {
            let text = format!("{{fileID: {file_id}}}");
            RenderedPointer {
                yaml: text.clone(),
                display: text,
            }
        }
        Some(guid) => {
            let yaml_type = yaml_pointer_type(asset_type, export_major_version).as_i32();
            RenderedPointer {
                yaml: format!("{{fileID: {file_id}, guid: {guid}, type: {yaml_type}}}"),
                display: format!(
                    "{{fileID: {file_id}, guid: {guid}, type: {}}}",
                    asset_type.as_i32()
                ),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        dummy_export_type, export_type_for, prefab_pointer_type, render_pointer,
        resolve_stack_export_type, yaml_pointer_type, ExportAssetType, NoExportTypeError,
        EXPORTER_EXPORT_TYPES,
    };

    #[test]
    fn asset_type_numerics_match_the_snapshot_enum() {
        assert_eq!(ExportAssetType::ALL.len(), 4);
        for (asset_type, value) in [
            (ExportAssetType::Internal, 0),
            (ExportAssetType::Cached, 1),
            (ExportAssetType::Serialized, 2),
            (ExportAssetType::Meta, 3),
        ] {
            assert_eq!(asset_type.as_i32(), value, "{asset_type:?}");
            assert_eq!(ExportAssetType::from_i32(value), Some(asset_type));
        }
        assert_eq!(ExportAssetType::from_i32(-1), None);
        assert_eq!(ExportAssetType::from_i32(4), None);
    }

    #[test]
    fn exporter_table_follows_base_class_answers() {
        use super::ExporterExportType;
        assert_eq!(EXPORTER_EXPORT_TYPES.len(), 30);
        fn both(exporter: &'static str, asset_type: ExportAssetType) -> ExporterExportType {
            ExporterExportType {
                exporter,
                to_export_type: Some(asset_type),
                to_unknown_export_type: Some(asset_type),
            }
        }
        // Yaml exporters serialize inline.
        for exporter in [
            "AnimatorControllerExporter",
            "AudioMixerExporter",
            "DefaultYamlExporter",
            "ManagerAssetExporter",
            "SceneYamlExporter",
            "ScriptableObjectExporter",
            "ScriptableObjectGroupExporter",
            "TerrainYamlExporter",
            "YamlAudioExporter",
            "YamlShaderExporter",
            "YamlSpriteExporter",
            "YamlStreamedAssetExporter",
        ] {
            assert_eq!(
                export_type_for(exporter),
                Some(both(exporter, ExportAssetType::Serialized)),
                "{exporter}"
            );
        }
        // Binary exporters (including both shader exporters) point at the .meta sidecar.
        for exporter in [
            "AudioClipExporter",
            "DummyShaderTextExporter",
            "FontAssetExporter",
            "LightmapTextureAssetExporter",
            "MovieTextureAssetExporter",
            "NativeAudioExporter",
            "RawTextureExporter",
            "SimpleShaderExporter",
            "TextAssetExporter",
            "TextureArrayAssetExporter",
            "TextureAssetExporter",
            "VideoClipExporter",
        ] {
            assert_eq!(
                export_type_for(exporter),
                Some(both(exporter, ExportAssetType::Meta)),
                "{exporter}"
            );
        }
        // Direct IAssetExporter implementations with the binary answer.
        for exporter in [
            "SceneAssetExporter",
            "ScriptExporter",
            "UnknownObjectExporter",
            "UnreadableObjectExporter",
        ] {
            assert_eq!(
                export_type_for(exporter),
                Some(both(exporter, ExportAssetType::Meta)),
                "{exporter}"
            );
        }
        // EngineAssetsExporter answers Internal but still declines the unknown-type walk.
        assert_eq!(
            export_type_for("EngineAssetsExporter"),
            Some(ExporterExportType {
                exporter: "EngineAssetsExporter",
                to_export_type: Some(ExportAssetType::Internal),
                to_unknown_export_type: None,
            })
        );
        // DeletedAssetsExporter never contributes a type.
        assert_eq!(
            export_type_for("DeletedAssetsExporter"),
            Some(ExporterExportType {
                exporter: "DeletedAssetsExporter",
                to_export_type: None,
                to_unknown_export_type: None,
            })
        );
        assert_eq!(export_type_for("NoSuchExporter"), None);
        // The dummy type follows isMetaType alone.
        assert_eq!(dummy_export_type(true), ExportAssetType::Meta);
        assert_eq!(dummy_export_type(false), ExportAssetType::Serialized);
    }

    #[test]
    fn stack_resolution_skips_decliners_and_fails_empty() {
        use ExportAssetType::{Internal, Meta, Serialized};
        // Deleted-style decliners are skipped; the first accepter wins.
        assert_eq!(
            resolve_stack_export_type(&[None, Some(Meta), Some(Serialized)]),
            Ok(Meta)
        );
        assert_eq!(resolve_stack_export_type(&[Some(Internal)]), Ok(Internal));
        // No accepter is the NotSupportedException path.
        assert_eq!(
            resolve_stack_export_type(&[None, None]),
            Err(NoExportTypeError)
        );
        assert_eq!(resolve_stack_export_type(&[]), Err(NoExportTypeError));
    }

    #[test]
    fn pointer_forms_follow_meta_ptr_rules() {
        // Local pointers carry the file ID alone in both forms.
        let local = render_pointer(42, None, ExportAssetType::Serialized, 5);
        assert_eq!(local.yaml, "{fileID: 42}");
        assert_eq!(local.display, "{fileID: 42}");
        // Non-local pointers add the GUID and the numeric type.
        let remote = render_pointer(7, Some("abc123"), ExportAssetType::Meta, 5);
        assert_eq!(remote.yaml, "{fileID: 7, guid: abc123, type: 3}");
        assert_eq!(remote.display, "{fileID: 7, guid: abc123, type: 3}");
        // Below Unity 4 the YAML form redirects Meta to Cached (1) while the
        // display form keeps the true type (3).
        let legacy = render_pointer(7, Some("abc123"), ExportAssetType::Meta, 3);
        assert_eq!(legacy.yaml, "{fileID: 7, guid: abc123, type: 1}");
        assert_eq!(legacy.display, "{fileID: 7, guid: abc123, type: 3}");
        assert_eq!(
            yaml_pointer_type(ExportAssetType::Meta, 4),
            ExportAssetType::Meta
        );
        assert_eq!(
            yaml_pointer_type(ExportAssetType::Serialized, 3),
            ExportAssetType::Serialized
        );
        // The prefab pointer follows the legacy marker.
        assert_eq!(prefab_pointer_type(true), ExportAssetType::Serialized);
        assert_eq!(prefab_pointer_type(false), ExportAssetType::Meta);
    }
}
