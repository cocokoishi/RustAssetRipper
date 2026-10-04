//! Exporter stack order and winner resolution in the frozen baseline.
//!
//! `ObjectHandlerStack<T>` keeps registrations in order and pushes every applicable handler
//! onto a `Stack<T>`, so enumeration pops last-registered-first: the most recently
//! overridden exporter is tried first, and `CreateCollection` takes the first exporter
//! whose `TryCreateCollection` succeeds. A registration applies to an exact type match,
//! or to an assignable type when its inheritance flag is set. Every `OverrideHandler`
//! call clears the per-type cache; the cache only memoizes, so it has no observable
//! behavior beyond recomputation and is recorded here without a model.
//!
//! Two winner chains fall straight out of the override order plus the per-exporter
//! admission rules:
//!
//! - Shaders try `SimpleShaderExporter` first (decompiled source text only), then the
//!   mode exporter: `YamlShaderExporter` under `ShaderExportMode::Yaml` (always true for
//!   shaders), otherwise `DummyShaderTextExporter` (also always true via its base). Every
//!   shader matches something; non-shaders match nothing here.
//! - Clips try `AudioClipExporter` first when the format is supported (`Default` or
//!   `PreferWav`) and decoding is usable, then `NativeAudioExporter` under the `Native`
//!   format (always true for clips), then `YamlAudioExporter` (always true for clips).
//!   The mode lists come from `ProjectExporter.Overrides`; admission facts come from the
//!   exporter modules.

use super::settings::{AudioExportFormat, ShaderExportMode};

/// Whether a registration covers a queried asset type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HandlerApplicability {
    /// The queried type is exactly the registered type.
    pub is_exact_type: bool,
    /// The queried type derives from (or implements) the registered type.
    pub is_assignable_to_registered: bool,
    /// The registration's inheritance flag.
    pub allow_inheritance: bool,
}

/// `type == baseType || allowInheritance && type.IsAssignableTo(baseType)`.
pub const fn handler_applies(query: HandlerApplicability) -> bool {
    query.is_exact_type || (query.allow_inheritance && query.is_assignable_to_registered)
}

/// Stack enumeration order for applicable handlers: last registered is tried first.
pub fn handler_try_order<T: Clone>(applicable_in_registration_order: &[T]) -> Vec<T> {
    applicable_in_registration_order
        .iter()
        .rev()
        .cloned()
        .collect()
}

/// Which shader exporter wins for one shader asset.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderExporterChoice {
    /// Decompiled source text is present; writes `Script.Data` bytes.
    Simple,
    /// `ShaderExportMode::Yaml` fallback; always admits shaders.
    Yaml,
    /// Any other mode fallback; always admits shaders via its base.
    Dummy,
}

/// The shader stack is `[mode exporter, Simple]` in registration order, so `Simple` is
/// tried first and the mode exporter is an unconditional fallback for shaders.
///
/// `simple_admits` bundles the `TryCreateCollection` admission (shader with script whose
/// text passes [`has_decompiled_shader_text`]); the `Has_Script` gate itself lives in the
/// generated package, so its threshold stays caller-side.
pub const fn select_shader_exporter(
    mode: ShaderExportMode,
    simple_admits: bool,
) -> ShaderExporterChoice {
    if simple_admits {
        ShaderExporterChoice::Simple
    } else if matches!(mode, ShaderExportMode::Yaml) {
        ShaderExporterChoice::Yaml
    } else {
        ShaderExporterChoice::Dummy
    }
}

/// `SimpleShaderExporter.HasDecompiledShaderText`: nonempty text with neither `Program`
/// nor `SubProgram` inside, compared ordinally like the source `Contains` calls.
pub fn has_decompiled_shader_text(text: &str) -> bool {
    !text.is_empty() && !text.contains("Program") && !text.contains("SubProgram")
}

/// Which audio exporter wins for one asset.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioExporterChoice {
    /// Supported format with usable decode; keeps the decoded extension.
    ConvertedClip,
    /// `Native` format fallback; writes raw payload bytes.
    Native,
    /// Universal fallback; YAML asset plus `.resS` staging.
    Yaml,
}

/// The clip stack in registration order is `[Yaml, Native?, Converted?]` (`Native` only
/// under the `Native` format, `Converted` only under supported formats), tried in
/// reverse: usable decodes win under supported formats, native bytes win under `Native`,
/// and YAML catches every remaining clip. Non-clips match nothing here.
pub const fn select_audio_exporter(
    format: AudioExportFormat,
    is_audio_clip: bool,
    decode_usable: bool,
) -> Option<AudioExporterChoice> {
    if !is_audio_clip {
        return None;
    }
    if matches!(
        format,
        AudioExportFormat::Default | AudioExportFormat::PreferWav
    ) && decode_usable
    {
        Some(AudioExporterChoice::ConvertedClip)
    } else if matches!(format, AudioExportFormat::Native) {
        Some(AudioExporterChoice::Native)
    } else {
        Some(AudioExporterChoice::Yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handler_applies, handler_try_order, has_decompiled_shader_text, select_audio_exporter,
        select_shader_exporter, AudioExporterChoice, HandlerApplicability, ShaderExporterChoice,
    };
    use crate::export::settings::{AudioExportFormat, ShaderExportMode};

    #[test]
    fn handler_stacks_pop_last_registered_first_with_inheritance_filter() {
        assert_eq!(
            handler_try_order(&["first", "second", "third"]),
            ["third", "second", "first"]
        );
        assert!(handler_applies(HandlerApplicability {
            is_exact_type: true,
            is_assignable_to_registered: false,
            allow_inheritance: false,
        }));
        assert!(!handler_applies(HandlerApplicability {
            is_exact_type: false,
            is_assignable_to_registered: true,
            allow_inheritance: false,
        }));
        assert!(handler_applies(HandlerApplicability {
            is_exact_type: false,
            is_assignable_to_registered: true,
            allow_inheritance: true,
        }));
        assert!(!handler_applies(HandlerApplicability {
            is_exact_type: false,
            is_assignable_to_registered: false,
            allow_inheritance: true,
        }));
    }

    #[test]
    fn shader_text_gate_matches_has_decompiled_shader_text() {
        assert!(!has_decompiled_shader_text(""));
        assert!(!has_decompiled_shader_text(
            "Shader \"x\" {\nProgram \"vp\" {\n}\n}"
        ));
        assert!(!has_decompiled_shader_text(
            "Shader \"x\" {\nSubShader {\nSubProgram \"d3d11\" {\n}\n}\n}"
        ));
        // "SubShader" alone does not contain "SubProgram".
        assert!(has_decompiled_shader_text(
            "Shader \"x\" {\nSubShader {\nPass {\n}\n}\n}"
        ));
    }

    #[test]
    fn shader_winners_try_simple_first_then_the_mode_exporter() {
        // Decompiled text wins under every mode.
        for mode in [
            ShaderExportMode::Dummy,
            ShaderExportMode::Yaml,
            ShaderExportMode::Decompile,
        ] {
            assert_eq!(
                select_shader_exporter(mode, true),
                ShaderExporterChoice::Simple,
                "{mode:?}"
            );
        }
        // Otherwise the mode exporter admits every shader; Decompile falls to Dummy.
        assert_eq!(
            select_shader_exporter(ShaderExportMode::Yaml, false),
            ShaderExporterChoice::Yaml
        );
        assert_eq!(
            select_shader_exporter(ShaderExportMode::Dummy, false),
            ShaderExporterChoice::Dummy
        );
        assert_eq!(
            select_shader_exporter(ShaderExportMode::Decompile, false),
            ShaderExporterChoice::Dummy
        );
    }

    #[test]
    fn audio_winners_follow_format_stacks_and_decode_usability() {
        // Non-clips match nothing on this stack.
        assert_eq!(
            select_audio_exporter(AudioExportFormat::Default, false, true),
            None
        );
        // Supported formats prefer usable decodes, else fall back to YAML.
        assert_eq!(
            select_audio_exporter(AudioExportFormat::Default, true, true),
            Some(AudioExporterChoice::ConvertedClip)
        );
        assert_eq!(
            select_audio_exporter(AudioExportFormat::PreferWav, true, true),
            Some(AudioExporterChoice::ConvertedClip)
        );
        assert_eq!(
            select_audio_exporter(AudioExportFormat::Default, true, false),
            Some(AudioExporterChoice::Yaml)
        );
        // Native mode always takes raw bytes for clips, even with usable decodes.
        assert_eq!(
            select_audio_exporter(AudioExportFormat::Native, true, true),
            Some(AudioExporterChoice::Native)
        );
        assert_eq!(
            select_audio_exporter(AudioExportFormat::Native, true, false),
            Some(AudioExporterChoice::Native)
        );
        // The YAML mode has only the universal fallback.
        assert_eq!(
            select_audio_exporter(AudioExportFormat::Yaml, true, true),
            Some(AudioExporterChoice::Yaml)
        );
        assert_eq!(
            select_audio_exporter(AudioExportFormat::Yaml, true, false),
            Some(AudioExporterChoice::Yaml)
        );
    }
}
