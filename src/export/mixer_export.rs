//! Audio mixer export paths ported from the frozen baseline.
//!
//! The mixer object model (groups, controllers, effects, snapshots) lives in generated
//! classes, so member reads stay caller-side. What the snapshot determines exactly is
//! pinned here:
//!
//! - `AudioMixerExporter.TryCreateCollection` routes on the asset's main asset: a mixer
//!   controller main yields an `AudioMixerExportCollection`, anything else yields a
//!   `FailExportCollection`. Both arms succeed, so creation never fails.
//! - The mixer collection gathers same-bundle child groups (matched by back-pointer),
//!   controller effects, and mixer snapshots; export IDs stride by two from the shared
//!   main-ID rule; the file extension is the base-table `mixer` value (no override).
//! - `ExportInner` applies the `AudioMixerPostprocessor` patch unconditionally — unlike
//!   the version-gated shader patches — then runs the base YAML export. The patch text is
//!   vendored verbatim under `shader_data/` (same mechanical extraction as the shader
//!   texts) and written through the shared patch mechanics (skip-if-exists, UTF-8 bytes).
//! - The processor's pure domain kernels: plugin-effect classification, the negative
//!   effect-type names, the `Param_{j}` dummy parameter names (recovered by the patch
//!   at Unity import), and the NUL-separated plugin-effect name buffer split. The
//!   non-negative effect-type names resolve through the FMOD `FmodDspType` numerics,
//!   which the frozen snapshot does not contain, so they stay open; the processor
//!   orchestration itself (virtual collections, GUID tables, snapshot wiring) stays
//!   caller-side with the generated mixer classes.

/// The mixer patch name passed to `ApplyPatchFromText`.
pub const AUDIO_MIXER_PATCH_NAME: &str = "AudioMixerPostprocessor";

/// The vendored mixer patch text, verbatim (CRLF included, like the source const).
pub const AUDIO_MIXER_PATCH_TEXT: &str = include_str!("shader_data/audio_mixer_patch.txt");

/// The base-table file extension for mixer exports (no collection override).
pub const MIXER_EXPORT_EXTENSION: &str = "mixer";

/// Which collection `AudioMixerExporter.TryCreateCollection` builds.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MixerCollectionKind {
    /// The main asset is a mixer controller: gather and export as one YAML asset.
    MixerExportCollection,
    /// Anything else: warn at export and emit missing-reference pointers.
    FailExportCollection,
}

/// The routing decision plus the source's always-true creation result.
pub const fn plan_mixer_collection(main_is_mixer_controller: bool) -> (MixerCollectionKind, bool) {
    if main_is_mixer_controller {
        (MixerCollectionKind::MixerExportCollection, true)
    } else {
        (MixerCollectionKind::FailExportCollection, true)
    }
}

/// Member reads of the `AudioMixerExportCollection` constructor, in source order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MixerGatherRead {
    /// What is read.
    pub reads: &'static str,
    /// The filter or conversion applied.
    pub rule: &'static str,
}

/// Constructor gathering: same-bundle child groups by back-pointer, controller effects,
/// then mixer snapshots.
pub const MIXER_GATHER_READS: &[MixerGatherRead] = &[
    MixerGatherRead {
        reads: "Collection.Bundle hierarchy assets of IAudioMixerGroup type",
        rule: "keep groups whose AudioMixer back-pointer is this mixer",
    },
    MixerGatherRead {
        reads: "group controller EffectsP",
        rule: "append every effect of group controllers",
    },
    MixerGatherRead {
        reads: "mixer Snapshots",
        rule: "append every snapshot",
    },
];

/// Export-ID rule: the shared main-ID rule from a counter starting at zero, stepping by
/// two per member — the same stride as the texture collections.
pub const MIXER_EXPORT_ID_STRIDE: i64 = 2;
/// The export-ID counter seed for a fresh mixer collection.
pub const MIXER_EXPORT_ID_SEED: i64 = 0;

/// Advance a stride-two export-ID counter, mirroring the `m_nextExportID += 2` stepping.
/// ID allocation itself stays with the shared main-ID rule; only the stepping is pinned.
pub const fn next_strided_counter(counter: i64) -> i64 {
    counter + MIXER_EXPORT_ID_STRIDE
}

/// `AudioEffectDefinitions.IsPluginEffect`: custom plugin effects start at 1000.
pub const fn is_plugin_effect(effect_type: i32) -> bool {
    effect_type >= 1000
}

/// The negative `UnityEffectType` arm of `AudioEffectDefinitions.EffectTypeToName`.
///
/// Non-negative types resolve through the FMOD `FmodDspType` numerics, which the
/// frozen snapshot does not contain, so they (and plugin types, which the source
/// refuses) read as `None` here; that mapping stays open.
pub const fn unity_effect_type_name(effect_type: i32) -> Option<&'static str> {
    match effect_type {
        -2 => Some("Attenuation"),
        -3 => Some("Send"),
        -4 => Some("Receive"),
        -5 => Some("Duck Volume"),
        _ => None,
    }
}

/// The dummy effect parameter name (`$"Param_{j}"`).
///
/// The processor assigns these because release assets carry no parameter names;
/// the vendored `AudioMixerPostprocessor` patch recovers the real names when
/// Unity imports the exported mixer.
pub fn dummy_effect_parameter_name(parameter_index: usize) -> String {
    format!("Param_{parameter_index}")
}

/// Splits a `PluginEffectNameBuffer` into its NUL-separated effect names.
///
/// Mirrors `AudioMixerProcessor.ParseNameBuffer`'s loop, which terminates at the
/// first empty segment (real buffers are NUL-padded fixed arrays, so the names
/// are always a leading run). Two deliberate hardenings: an empty or
/// unterminated input yields the successfully split prefix instead of throwing
/// past the end, and non-UTF-8 bytes become the replacement character.
pub fn parse_plugin_effect_names(buffer: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for segment in buffer.split(|byte| *byte == 0) {
        if segment.is_empty() {
            break;
        }
        names.push(String::from_utf8_lossy(segment).into_owned());
    }
    names
}

#[cfg(test)]
mod tests {
    use super::{
        dummy_effect_parameter_name, is_plugin_effect, next_strided_counter,
        parse_plugin_effect_names, plan_mixer_collection, unity_effect_type_name,
        MixerCollectionKind, AUDIO_MIXER_PATCH_NAME, AUDIO_MIXER_PATCH_TEXT,
        MIXER_EXPORT_EXTENSION, MIXER_EXPORT_ID_SEED, MIXER_EXPORT_ID_STRIDE, MIXER_GATHER_READS,
    };

    #[test]
    fn mixer_routing_covers_controller_and_fallback_arms() {
        assert_eq!(
            plan_mixer_collection(true),
            (MixerCollectionKind::MixerExportCollection, true)
        );
        // Non-controller mains still succeed, routing to the failing collection.
        assert_eq!(
            plan_mixer_collection(false),
            (MixerCollectionKind::FailExportCollection, true)
        );
        assert_eq!(MIXER_EXPORT_EXTENSION, "mixer");
    }

    #[test]
    fn mixer_gathering_and_stride_match_collection_source() {
        assert_eq!(MIXER_GATHER_READS.len(), 3);
        assert_eq!(MIXER_EXPORT_ID_STRIDE, 2);
        assert_eq!(MIXER_EXPORT_ID_SEED, 0);
        assert_eq!(next_strided_counter(0), 2);
        assert_eq!(next_strided_counter(2), 4);
    }

    #[test]
    fn mixer_patch_applies_unconditionally_with_vendored_text() {
        assert_eq!(AUDIO_MIXER_PATCH_NAME, "AudioMixerPostprocessor");
        // Unlike the shader patches, no export-version gate guards this application.
        assert_eq!(AUDIO_MIXER_PATCH_TEXT.len(), 3199);
        assert!(AUDIO_MIXER_PATCH_TEXT.contains("AudioMixerPostprocessor"));
        assert!(AUDIO_MIXER_PATCH_TEXT.contains("PreallocateGUIDs"));
        assert!(AUDIO_MIXER_PATCH_TEXT.contains("\r\n"));
    }

    #[test]
    fn mixer_effect_classification_matches_the_definitions_source() {
        assert!(!is_plugin_effect(999));
        assert!(is_plugin_effect(1000));
        assert!(is_plugin_effect(1001));
        assert!(!is_plugin_effect(0));
        assert!(!is_plugin_effect(-2));
        assert_eq!(unity_effect_type_name(-2), Some("Attenuation"));
        assert_eq!(unity_effect_type_name(-3), Some("Send"));
        assert_eq!(unity_effect_type_name(-4), Some("Receive"));
        assert_eq!(unity_effect_type_name(-5), Some("Duck Volume"));
        // Unknown negative types read as None like the source switch default;
        // non-negative names need the FMOD numerics and stay open.
        assert_eq!(unity_effect_type_name(-1), None);
        assert_eq!(unity_effect_type_name(-6), None);
        assert_eq!(unity_effect_type_name(0), None);
        assert_eq!(unity_effect_type_name(3), None);
        assert_eq!(unity_effect_type_name(1000), None);
    }

    #[test]
    fn mixer_dummy_parameter_names_follow_the_processor_format() {
        assert_eq!(dummy_effect_parameter_name(0), "Param_0");
        assert_eq!(dummy_effect_parameter_name(7), "Param_7");
    }

    #[test]
    fn mixer_plugin_effect_names_split_at_nul_boundaries() {
        assert_eq!(
            parse_plugin_effect_names(b"Echo\0SFX Reverb\0\0\0"),
            ["Echo".to_string(), "SFX Reverb".to_string()]
        );
        assert_eq!(parse_plugin_effect_names(b"Single"), ["Single".to_string()]);
        assert_eq!(
            parse_plugin_effect_names(b"a\0b"),
            ["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            parse_plugin_effect_names(b"a\0b\0"),
            ["a".to_string(), "b".to_string()]
        );
        assert!(parse_plugin_effect_names(b"").is_empty());
        assert!(parse_plugin_effect_names(b"\0padding").is_empty());
        assert_eq!(
            parse_plugin_effect_names(b"a\xffb\0"),
            ["a\u{fffd}b".to_string()]
        );
    }
}
