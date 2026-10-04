//! Asset processing pipeline order ported from `ExportHandler.GetProcessors`.
//!
//! The baseline runs every `IAssetProcessor` in a fixed order after loading and before
//! exporting. Assembly processors run first, then asset processors, with three configuration
//! gates and two processors that capture the export mode:
//!
//! - `MethodStubbingProcessor` runs only when `ScriptContentLevel` is `Level1`.
//! - `NullRefReturnProcessor` always runs but captures the script content level.
//! - `NullableRemovalProcessor` runs only when `RemoveNullableAttributes` is set.
//! - `SafeAssemblyPublicizingProcessor` runs only when `PublicizeAssemblies` is set.
//! - `OriginalPathProcessor` and `EditorFormatProcessor` capture `BundledAssetsExportMode`.
//! - `LightingDataProcessor` runs after the static-mesh-separation position (which currently
//!   holds no processor) and before `PrefabProcessor`.
//!
//! Processor internals are not ported here; the table records the sequence, the phase each
//! processor belongs to, and the gate around it, so later per-processor ports can slot into
//! the verified order.

/// The configuration gate around one pipeline processor.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PipelineGate {
    /// Runs on every export.
    Always,
    /// Runs only when `ScriptContentLevel` is `Level1`.
    ScriptContentLevel1,
    /// Runs only when `RemoveNullableAttributes` is set.
    RemoveNullableAttributes,
    /// Runs only when `PublicizeAssemblies` is set.
    PublicizeAssemblies,
}

/// Which pipeline phase one processor belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PipelinePhase {
    /// Mono Cecil assembly rewriting before asset processing.
    Assembly,
    /// Unity asset processing after the assemblies settle.
    Asset,
}

/// One `yield return` step of `ExportHandler.GetProcessors` in source order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PipelineProcessor {
    /// The processor type name.
    pub processor: &'static str,
    /// Assembly processors run before asset processors.
    pub phase: PipelinePhase,
    /// The configuration gate around the step.
    pub gate: PipelineGate,
    /// True for the two processors constructed with `BundledAssetsExportMode`.
    pub captures_export_mode: bool,
}

/// Every asset processing pipeline step in `ExportHandler.GetProcessors` source order.
pub const PIPELINE_PROCESSORS: &[PipelineProcessor] = &[
    PipelineProcessor {
        processor: "AttributePolyfillGenerator",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "MonoExplicitPropertyRepairProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "ObfuscationRepairProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "ForwardingAssemblyGenerator",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "MethodStubbingProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::ScriptContentLevel1,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "NullRefReturnProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "UnmanagedConstraintRecoveryProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "NullableRemovalProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::RemoveNullableAttributes,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "SafeAssemblyPublicizingProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::PublicizeAssemblies,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "RemoveAssemblyKeyFileAttributeProcessor",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "InternalsVisibileToPublicKeyRemover",
        phase: PipelinePhase::Assembly,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "SceneDefinitionProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "OriginalPathProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: true,
    },
    PipelineProcessor {
        processor: "MainAssetProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "AnimatorControllerProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "AudioMixerProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "EditorFormatProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: true,
    },
    PipelineProcessor {
        processor: "LightingDataProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "PrefabProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "SpriteProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
    PipelineProcessor {
        processor: "ScriptableObjectProcessor",
        phase: PipelinePhase::Asset,
        gate: PipelineGate::Always,
        captures_export_mode: false,
    },
];

/// All processors of one phase, in source order.
pub fn processors_in_phase(
    phase: PipelinePhase,
) -> impl Iterator<Item = &'static PipelineProcessor> {
    PIPELINE_PROCESSORS
        .iter()
        .filter(move |processor| processor.phase == phase)
}

#[cfg(test)]
mod tests {
    use super::{processors_in_phase, PipelineGate, PipelinePhase, PIPELINE_PROCESSORS};

    #[test]
    fn table_covers_every_pipeline_step_in_source_order() {
        // 11 assembly processors followed by 10 asset processors.
        assert_eq!(PIPELINE_PROCESSORS.len(), 21);
        assert_eq!(
            PIPELINE_PROCESSORS.first().unwrap().processor,
            "AttributePolyfillGenerator"
        );
        assert_eq!(
            PIPELINE_PROCESSORS.last().unwrap().processor,
            "ScriptableObjectProcessor"
        );
        let phases: Vec<PipelinePhase> = PIPELINE_PROCESSORS
            .iter()
            .map(|processor| processor.phase)
            .collect();
        let first_asset = phases
            .iter()
            .position(|phase| *phase == PipelinePhase::Asset)
            .unwrap();
        assert_eq!(first_asset, 11);
        assert!(phases[first_asset..]
            .iter()
            .all(|phase| *phase == PipelinePhase::Asset));
    }

    #[test]
    fn lighting_data_sits_between_editor_format_and_prefab() {
        let names: Vec<&str> = processors_in_phase(PipelinePhase::Asset)
            .map(|processor| processor.processor)
            .collect();
        assert_eq!(
            names,
            [
                "SceneDefinitionProcessor",
                "OriginalPathProcessor",
                "MainAssetProcessor",
                "AnimatorControllerProcessor",
                "AudioMixerProcessor",
                "EditorFormatProcessor",
                "LightingDataProcessor",
                "PrefabProcessor",
                "SpriteProcessor",
                "ScriptableObjectProcessor",
            ]
        );
    }

    #[test]
    fn gated_processors_and_export_mode_capture_match_the_source() {
        let gated: Vec<(&str, PipelineGate)> = PIPELINE_PROCESSORS
            .iter()
            .filter(|processor| processor.gate != PipelineGate::Always)
            .map(|processor| (processor.processor, processor.gate))
            .collect();
        assert_eq!(
            gated,
            [
                ("MethodStubbingProcessor", PipelineGate::ScriptContentLevel1),
                (
                    "NullableRemovalProcessor",
                    PipelineGate::RemoveNullableAttributes
                ),
                (
                    "SafeAssemblyPublicizingProcessor",
                    PipelineGate::PublicizeAssemblies
                ),
            ]
        );
        let capturing: Vec<&str> = PIPELINE_PROCESSORS
            .iter()
            .filter(|processor| processor.captures_export_mode)
            .map(|processor| processor.processor)
            .collect();
        assert_eq!(
            capturing,
            ["OriginalPathProcessor", "EditorFormatProcessor"]
        );
    }
}
