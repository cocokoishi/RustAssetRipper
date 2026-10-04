//! Generated Unity class identities and the export extension selected by the frozen baseline.
//!
//! `ExportCollection.GetExportExtension` selects an output extension by matching the asset against
//! generated marker interfaces with top-down first-match wins. The marker names below are exactly
//! the interface patterns tested by the source switch in `ExportCollection.cs`; the generated
//! package that declares those interfaces is referenced but absent from the frozen snapshot, so
//! no hierarchy beyond the switch order itself is encoded here. Callers supply the satisfied
//! marker set explicitly, which keeps the ordering visible instead of hiding it behind an
//! unported generic `is`-pattern.
//!
//! Source behaviour, in the exact order the switch evaluates it:
//!
//! | Marker interface | Extension |
//! |---|---|
//! | `IShader` | `shader` |
//! | `IMaterial` | `mat` |
//! | `IAnimationClip` | `anim` |
//! | `IAnimatorController` | `controller` |
//! | `IAnimatorOverrideController` | `overrideController` |
//! | `IAudioMixer` | `mixer` |
//! | `IAvatarMask` | `mask` |
//! | `IShaderVariantCollection` | `shadervariants` |
//! | `ICubemap` | `cubemap` |
//! | `ITexture2D` | `texture2D` |
//! | `IFlare` | `flare` |
//! | `ILightingSettings` | `lighting` |
//! | `ILightmapParameters` | `giparams` |
//! | `IPhysicsMaterial` | `physicMaterial` |
//! | `IPhysicsMaterial2D` | `physicsMaterial2D` |
//! | `IRenderTexture` | `renderTexture` |
//! | `ITerrainLayer` | `terrainlayer` |
//! | `IWebCamTexture` | `webCamTexture` |
//! | `IAnimatorState` | `state` |
//! | `IAnimatorStateMachine` | `statemachine` |
//! | `IAnimatorTransition` | `transition` |
//! | `IBlendTree` | `blendtree` |
//! | anything else | `asset` |
//!
//! Order matters: a `Texture2D` is also an `ITexture`/`INamedObject`, and a `Cubemap` is a
//! `ITexture2D`, so `ICubemap` must be evaluated before `ITexture2D` exactly as the source switch
//! is written. [`ExportClassMarker`] keeps the source order, and
//! [`ExportExtensionTable::extension_for`] walks it in that order.
//!
//! The extensions outside this base switch stay owned by their collections and are *not*
//! returned by [`ExportExtensionTable`]: `YamlAudioExportCollection` returns `audioclip`,
//! `AudioClipExportCollection` returns its exporter-chosen file extension, `NativeAudioExportCollection`
//! returns the raw sound-type extension, `MovieTextureAssetExportCollection` returns `ogv`,
//! `VideoClipExportCollection` returns the source path extension, and texture collections compute
//! image extensions. Those collections determine their extension without the base switch, so the
//! caller supplies the resulting string to
//! [`get_unique_file_name`](super::asset_path::get_unique_file_name) instead.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// `ExportCollection.AssetExtension`, the fallback arm of `GetExportExtension`.
pub const DEFAULT_EXPORT_EXTENSION: &str = "asset";

/// One arm of the `GetExportExtension` switch, in source evaluation order.
///
/// Each variant corresponds to one generated marker interface. The discriminants are not the Unity
/// class IDs: they are an internal ordering key that makes the source's first-match rule explicit.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ExportClassMarker {
    Shader,
    Material,
    AnimationClip,
    AnimatorController,
    AnimatorOverrideController,
    AudioMixer,
    AvatarMask,
    ShaderVariantCollection,
    Cubemap,
    Texture2D,
    Flare,
    LightingSettings,
    LightmapParameters,
    PhysicsMaterial,
    PhysicsMaterial2D,
    RenderTexture,
    TerrainLayer,
    WebCamTexture,
    AnimatorState,
    AnimatorStateMachine,
    AnimatorTransition,
    BlendTree,
}

impl ExportClassMarker {
    /// The extension the source returns for this marker.
    ///
    /// These strings are case sensitive in the source switch: `texture2D`, `physicMaterial`,
    /// `physicsMaterial2D`, `webCamTexture`, and `overrideController` are not uniform.
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Shader => "shader",
            Self::Material => "mat",
            Self::AnimationClip => "anim",
            Self::AnimatorController => "controller",
            Self::AnimatorOverrideController => "overrideController",
            Self::AudioMixer => "mixer",
            Self::AvatarMask => "mask",
            Self::ShaderVariantCollection => "shadervariants",
            Self::Cubemap => "cubemap",
            Self::Texture2D => "texture2D",
            Self::Flare => "flare",
            Self::LightingSettings => "lighting",
            Self::LightmapParameters => "giparams",
            Self::PhysicsMaterial => "physicMaterial",
            Self::PhysicsMaterial2D => "physicsMaterial2D",
            Self::RenderTexture => "renderTexture",
            Self::TerrainLayer => "terrainlayer",
            Self::WebCamTexture => "webCamTexture",
            Self::AnimatorState => "state",
            Self::AnimatorStateMachine => "statemachine",
            Self::AnimatorTransition => "transition",
            Self::BlendTree => "blendtree",
        }
    }

    /// The generated marker interface this arm tests.
    pub const fn marker_interface(self) -> &'static str {
        match self {
            Self::Shader => "IShader",
            Self::Material => "IMaterial",
            Self::AnimationClip => "IAnimationClip",
            Self::AnimatorController => "IAnimatorController",
            Self::AnimatorOverrideController => "IAnimatorOverrideController",
            Self::AudioMixer => "IAudioMixer",
            Self::AvatarMask => "IAvatarMask",
            Self::ShaderVariantCollection => "IShaderVariantCollection",
            Self::Cubemap => "ICubemap",
            Self::Texture2D => "ITexture2D",
            Self::Flare => "IFlare",
            Self::LightingSettings => "ILightingSettings",
            Self::LightmapParameters => "ILightmapParameters",
            Self::PhysicsMaterial => "IPhysicsMaterial",
            Self::PhysicsMaterial2D => "IPhysicsMaterial2D",
            Self::RenderTexture => "IRenderTexture",
            Self::TerrainLayer => "ITerrainLayer",
            Self::WebCamTexture => "IWebCamTexture",
            Self::AnimatorState => "IAnimatorState",
            Self::AnimatorStateMachine => "IAnimatorStateMachine",
            Self::AnimatorTransition => "IAnimatorTransition",
            Self::BlendTree => "IBlendTree",
        }
    }
}

/// Every arm of the switch, in source order.
///
/// The source evaluates top-down with first-match wins, so `Cubemap` precedes `Texture2D` and
/// `AnimatorState` precedes `AnimatorStateMachine` and `AnimatorTransition`. Preserving the
/// order here is what makes first-match resolution match the source.
pub const EXPORT_CLASS_MARKERS: [ExportClassMarker; 22] = [
    ExportClassMarker::Shader,
    ExportClassMarker::Material,
    ExportClassMarker::AnimationClip,
    ExportClassMarker::AnimatorController,
    ExportClassMarker::AnimatorOverrideController,
    ExportClassMarker::AudioMixer,
    ExportClassMarker::AvatarMask,
    ExportClassMarker::ShaderVariantCollection,
    ExportClassMarker::Cubemap,
    ExportClassMarker::Texture2D,
    ExportClassMarker::Flare,
    ExportClassMarker::LightingSettings,
    ExportClassMarker::LightmapParameters,
    ExportClassMarker::PhysicsMaterial,
    ExportClassMarker::PhysicsMaterial2D,
    ExportClassMarker::RenderTexture,
    ExportClassMarker::TerrainLayer,
    ExportClassMarker::WebCamTexture,
    ExportClassMarker::AnimatorState,
    ExportClassMarker::AnimatorStateMachine,
    ExportClassMarker::AnimatorTransition,
    ExportClassMarker::BlendTree,
];

/// The generated marker interfaces one serialized object satisfies.
///
/// The source tests interfaces with `is` patterns, so an object matches an arm when it implements
/// that interface. The satisfied set is supplied explicitly by the caller because the generated
/// package that declares the full hierarchies is absent from the frozen snapshot; for example,
/// a caller asserting a cubemap satisfies both `ICubemap` and `ITexture2D` gets the source's
/// first-match winner. Modelling the set explicitly keeps ordering decisions auditable instead
/// of hiding them behind a single class ID.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExportClassIdentity {
    /// Unity class ID from the serialized object table.
    pub class_id: i32,
    /// Generated version-variant type name, for example `Texture2D_2017` or `GameObject_3_5`.
    pub version_type_name: Option<String>,
    /// The highest-precedence arm that applies, in [`EXPORT_CLASS_MARKERS`] order.
    ///
    /// `None` means the object implements none of the switch's marker interfaces, so the source
    /// returns the default `asset` extension.
    pub matched_marker: Option<ExportClassMarker>,
    /// Every marker the object satisfies, so callers can audit ordering decisions.
    pub satisfied_markers: Vec<ExportClassMarker>,
}

impl ExportClassIdentity {
    /// Builds an identity for an object that satisfies exactly one generated marker.
    pub fn single(
        class_id: i32,
        version_type_name: Option<String>,
        marker: ExportClassMarker,
    ) -> Self {
        Self {
            class_id,
            version_type_name,
            matched_marker: Some(marker),
            satisfied_markers: vec![marker],
        }
    }

    /// Builds an identity for an object that satisfies none of the switch's markers.
    pub fn none(class_id: i32, version_type_name: Option<String>) -> Self {
        Self {
            class_id,
            version_type_name,
            matched_marker: None,
            satisfied_markers: Vec::new(),
        }
    }

    /// Builds an identity from a full satisfied-marker set, resolving the winner by source order.
    ///
    /// Unknown markers are ignored rather than rejected, because the generated package may add
    /// markers that this switch does not test; those simply cannot select an extension.
    pub fn from_markers(
        class_id: i32,
        version_type_name: Option<String>,
        markers: impl IntoIterator<Item = ExportClassMarker>,
    ) -> Self {
        let satisfied: Vec<ExportClassMarker> = markers.into_iter().collect();
        let matched_marker = EXPORT_CLASS_MARKERS
            .iter()
            .copied()
            .find(|marker| satisfied.contains(marker));
        Self {
            class_id,
            version_type_name,
            matched_marker,
            satisfied_markers: satisfied,
        }
    }

    /// `ExportCollection.GetExportExtension` for this identity.
    ///
    /// Equivalent to [`ExportExtensionTable::extension_for`]; provided so call sites that already
    /// hold an identity do not need the table type in scope.
    pub fn export_extension(&self) -> &'static str {
        ExportExtensionTable::extension_for(self)
    }
}

/// Error returned when a caller supplies a marker set that cannot be resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportExtensionError {
    /// The resolved marker is not one of the switch's arms.
    UnknownMarker { marker: ExportClassMarker },
}

impl Display for ExportExtensionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMarker { marker } => {
                write!(
                    formatter,
                    "marker {} ({}) is not an arm of GetExportExtension",
                    marker.marker_interface(),
                    marker.extension()
                )
            }
        }
    }
}

impl Error for ExportExtensionError {}

/// The `GetExportExtension` switch, exposed as a lookup.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExportExtensionTable;

impl ExportExtensionTable {
    /// `ExportCollection.GetExportExtension` for one generated class identity.
    ///
    /// Returns the marker's extension, or `asset` when the object satisfies no tested marker. This
    /// reproduces the source's first-match behaviour, including the `Cubemap` before `Texture2D`
    /// and `AnimatorState` before `AnimatorStateMachine` precedence.
    pub fn extension_for(identity: &ExportClassIdentity) -> &'static str {
        identity
            .matched_marker
            .map_or(DEFAULT_EXPORT_EXTENSION, ExportClassMarker::extension)
    }

    /// Resolves a marker directly, rejecting markers outside the switch.
    pub fn extension_for_marker(
        marker: ExportClassMarker,
    ) -> Result<&'static str, ExportExtensionError> {
        if EXPORT_CLASS_MARKERS.contains(&marker) {
            Ok(marker.extension())
        } else {
            Err(ExportExtensionError::UnknownMarker { marker })
        }
    }

    /// The ordered marker list, so a caller can reproduce or audit the switch's evaluation order.
    pub fn markers() -> &'static [ExportClassMarker] {
        &EXPORT_CLASS_MARKERS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_switch_arm_has_a_distinct_extension() {
        let mut seen: Vec<&str> = EXPORT_CLASS_MARKERS
            .iter()
            .map(|marker| marker.extension())
            .collect();
        seen.sort_unstable();
        let total = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), total, "source extensions must be distinct");
    }

    #[test]
    fn marker_list_matches_the_source_switch_order_and_size() {
        // The frozen `ExportCollection.GetExportExtension` switch has 22 typed arms plus the default.
        assert_eq!(EXPORT_CLASS_MARKERS.len(), 22);
        let names: Vec<&str> = EXPORT_CLASS_MARKERS
            .iter()
            .map(|marker| marker.marker_interface())
            .collect();
        assert_eq!(names.first(), Some(&"IShader"));
        assert_eq!(names.last(), Some(&"IBlendTree"));
        // Precedence pairs that the generated hierarchy makes ambiguous.
        let index = |needle: &str| names.iter().position(|name| *name == needle).unwrap();
        assert!(index("ICubemap") < index("ITexture2D"));
        assert!(index("IAnimatorState") < index("IAnimatorStateMachine"));
        assert!(index("IAnimatorState") < index("IAnimatorTransition"));
        assert!(index("IAnimatorStateMachine") < index("IAnimatorTransition"));
    }

    #[test]
    fn extension_strings_keep_the_source_casing() {
        // These five are not uniform in the source and a lowercasing refactor would be wrong.
        assert_eq!(ExportClassMarker::Texture2D.extension(), "texture2D");
        assert_eq!(
            ExportClassMarker::PhysicsMaterial.extension(),
            "physicMaterial"
        );
        assert_eq!(
            ExportClassMarker::PhysicsMaterial2D.extension(),
            "physicsMaterial2D"
        );
        assert_eq!(
            ExportClassMarker::WebCamTexture.extension(),
            "webCamTexture"
        );
        assert_eq!(
            ExportClassMarker::AnimatorOverrideController.extension(),
            "overrideController"
        );
    }

    #[test]
    fn unmarked_object_uses_the_default_asset_extension() {
        let identity = ExportClassIdentity::none(1, Some("GameObject_3_5".to_string()));
        assert_eq!(ExportClassIdentity::export_extension(&identity), "asset");
    }

    #[test]
    fn cubemap_wins_over_texture2d_in_source_order() {
        // When a caller reports both markers, the source evaluates ICubemap first.
        let identity = ExportClassIdentity::from_markers(
            89,
            Some("Cubemap_2017".to_string()),
            [ExportClassMarker::Texture2D, ExportClassMarker::Cubemap],
        );
        assert_eq!(identity.matched_marker, Some(ExportClassMarker::Cubemap));
        assert_eq!(ExportClassIdentity::export_extension(&identity), "cubemap");
    }

    #[test]
    fn texture2d_alone_selects_texture2d() {
        let identity = ExportClassIdentity::single(
            28,
            Some("Texture2D_2017".to_string()),
            ExportClassMarker::Texture2D,
        );
        assert_eq!(
            ExportClassIdentity::export_extension(&identity),
            "texture2D"
        );
    }

    #[test]
    fn animator_state_wins_over_state_machine_and_transition() {
        let identity = ExportClassIdentity::from_markers(
            1101,
            Some("AnimatorState_5".to_string()),
            [
                ExportClassMarker::BlendTree,
                ExportClassMarker::AnimatorTransition,
                ExportClassMarker::AnimatorStateMachine,
                ExportClassMarker::AnimatorState,
            ],
        );
        assert_eq!(
            identity.matched_marker,
            Some(ExportClassMarker::AnimatorState)
        );
        assert_eq!(ExportClassIdentity::export_extension(&identity), "state");
    }

    #[test]
    fn shader_wins_over_material_for_a_shader_material() {
        // The source checks IShader before IMaterial, so a caller reporting both exports as shader.
        let identity = ExportClassIdentity::from_markers(
            48,
            Some("Shader_5".to_string()),
            [ExportClassMarker::Material, ExportClassMarker::Shader],
        );
        assert_eq!(identity.matched_marker, Some(ExportClassMarker::Shader));
        assert_eq!(ExportClassIdentity::export_extension(&identity), "shader");
    }

    #[test]
    fn every_known_marker_resolves_through_the_table() {
        for marker in EXPORT_CLASS_MARKERS {
            let identity = ExportClassIdentity::single(0, None, marker);
            assert_eq!(
                ExportExtensionTable::extension_for(&identity),
                marker.extension(),
                "{} must resolve to its own extension",
                marker.marker_interface()
            );
            assert_eq!(
                ExportExtensionTable::extension_for_marker(marker),
                Ok(marker.extension())
            );
        }
    }

    #[test]
    fn satisfied_markers_are_retained_for_auditing() {
        let identity = ExportClassIdentity::from_markers(
            28,
            Some("Texture2D_2017".to_string()),
            [ExportClassMarker::Texture2D, ExportClassMarker::Shader],
        );
        // Shader sorts first in source order even though the caller listed Texture2D first.
        assert_eq!(identity.matched_marker, Some(ExportClassMarker::Shader));
        assert_eq!(identity.satisfied_markers.len(), 2);
        assert!(identity
            .satisfied_markers
            .contains(&ExportClassMarker::Texture2D));
    }
}
