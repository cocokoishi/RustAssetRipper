//! Export GUID ownership across AssetRipper's Unity project collections.
//!
//! The frozen baseline assigns a `.meta` GUID per export collection, but each family owns its
//! GUID differently:
//!
//! - `AssetExportCollection<T>.GUID` is `UnityGuid.NewGuid()`: a fresh random GUID per
//!   collection instance, so every export run renames every asset GUID.
//! - `SceneExportCollection.GUID` is `Scene.GUID`. `SceneDefinition.FromName` and `FromPath`
//!   preserve a supplied nonzero GUID and substitute `UnityGuid.NewGuid()` for a zero GUID, and
//!   document that the result is never zero.
//! - Script and assembly GUIDs are deterministic per identity (`ScriptHashing` documents them as
//!   stable "for consistency"). Their MD5-based byte layout lives in the external
//!   `AssetRipper.Primitives` package rather than the frozen snapshot, but it is now ported
//!   and runtime-pinned in `script_hashing` (`UnityGuid.Md5Hash` plus `ConvertSystemOrUnityBytes`
//!   over `script_guid_fixture.txt`), so the word values are known.
//! - `UnityGuid.MissingReference` marks fail, skip, and missing-reference pointers. Its words
//!   (`0xD0000000, 0x1FEEBDAE, 0x00FDAED5, 0x0000000D`, rendering
//!   `0000000deadbeef15deadf00d0000000`) are captured by the same probe and re-asserted in
//!   `script_hashing`.
//! - `Meta` itself rejects a zero GUID, which the Rust envelope serializer already enforces.
//!
//! Random GUID generation needs an entropy source the snapshot does not pin down, so this
//! module records the policy table and the pure scene rule without minting GUIDs.

/// The four words of the predefined GUID for `unity default resources` (`EGUID`).
pub const ENGINE_DEFAULT_RESOURCES_GUID_WORDS: [u32; 4] =
    [0x00000000, 0x00000000, 0x0000000E, 0x00000000];
/// The four words of the predefined GUID for `unity builtin extra` (`FGUID`).
pub const ENGINE_BUILTIN_EXTRA_GUID_WORDS: [u32; 4] =
    [0x00000000, 0x00000000, 0x0000000F, 0x00000000];
/// The four words of the private `DGUID` engine constant, recorded for completeness.
pub const ENGINE_INTERNAL_D_GUID_WORDS: [u32; 4] = [0x00000000, 0x00000000, 0x0000000D, 0x00000000];

/// How one export family owns the GUID written into its `.meta` sidecar.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExportGuidSource {
    /// `AssetExportCollection<T>`: one fresh random GUID per collection instance.
    RandomPerCollection,
    /// `SceneExportCollection`: the preserved `Scene.GUID`, with a random substitute for zero.
    PreservedScene,
    /// Script and assembly GUIDs: deterministic per script or assembly identity.
    DeterministicScript,
    /// Engine assets: the predefined `EGUID`/`FGUID` constants above.
    PredefinedEngine,
    /// The `MissingReference` sentinel for fail, skip, and missing-reference pointers.
    MissingReference,
}

/// Stability of a GUID source across repeated exports of the same input.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GuidStability {
    /// The same input always yields the same GUID.
    Stable,
    /// The GUID is only stable when a stored nonzero value exists to preserve.
    StableWhenStored,
    /// Every export run mints fresh GUIDs.
    Unstable,
}

/// How `SceneDefinition.FromName` and `FromPath` resolve a supplied GUID.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneGuidResolution {
    /// Keep the supplied nonzero GUID.
    PreserveStored,
    /// Mint a fresh random GUID for a zero GUID.
    GenerateRandom,
}

impl ExportGuidSource {
    /// All GUID sources known to this audit.
    pub const ALL: [Self; 5] = [
        Self::RandomPerCollection,
        Self::PreservedScene,
        Self::DeterministicScript,
        Self::PredefinedEngine,
        Self::MissingReference,
    ];

    /// Whether the source yields the same GUID when the same input is exported again.
    pub fn stability(self) -> GuidStability {
        match self {
            Self::DeterministicScript | Self::PredefinedEngine | Self::MissingReference => {
                GuidStability::Stable
            }
            Self::PreservedScene => GuidStability::StableWhenStored,
            Self::RandomPerCollection => GuidStability::Unstable,
        }
    }

    /// Whether the exporter can produce the exact GUID bytes from snapshot-pinned rules plus
    /// stored or caller-supplied facts (no entropy source needed).
    ///
    /// Script hashes and the missing-reference sentinel depend on the external primitives
    /// package, whose `UnityGuid` layout is now ported and runtime-pinned in
    /// `script_hashing`. Only the per-collection random source stays open.
    pub fn byte_layout_in_snapshot(self) -> bool {
        match self {
            Self::RandomPerCollection => false,
            Self::DeterministicScript
            | Self::PreservedScene
            | Self::PredefinedEngine
            | Self::MissingReference => true,
        }
    }
}

/// The `guid.IsZero ? UnityGuid.NewGuid() : guid` rule shared by both `SceneDefinition` factories.
pub fn resolve_scene_guid(stored_is_zero: bool) -> SceneGuidResolution {
    if stored_is_zero {
        SceneGuidResolution::GenerateRandom
    } else {
        SceneGuidResolution::PreserveStored
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_scene_guid, ExportGuidSource, GuidStability, SceneGuidResolution,
        ENGINE_BUILTIN_EXTRA_GUID_WORDS, ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
        ENGINE_INTERNAL_D_GUID_WORDS,
    };

    #[test]
    fn predefined_engine_guid_words_match_the_baseline_constants() {
        assert_eq!(
            ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
            [0x00000000, 0x00000000, 0x0000000E, 0x00000000]
        );
        assert_eq!(
            ENGINE_BUILTIN_EXTRA_GUID_WORDS,
            [0x00000000, 0x00000000, 0x0000000F, 0x00000000]
        );
        // The private `DGUID` engine constant is recorded alongside the public pair.
        assert_eq!(
            ENGINE_INTERNAL_D_GUID_WORDS,
            [0x00000000, 0x00000000, 0x0000000D, 0x00000000]
        );
        assert_eq!(ExportGuidSource::ALL.len(), 5);
    }

    #[test]
    fn scene_guid_resolution_preserves_nonzero_and_regenerates_zero() {
        assert_eq!(
            resolve_scene_guid(false),
            SceneGuidResolution::PreserveStored
        );
        assert_eq!(
            resolve_scene_guid(true),
            SceneGuidResolution::GenerateRandom
        );
    }

    #[test]
    fn stability_matches_each_family_ownership_rule() {
        assert_eq!(
            ExportGuidSource::RandomPerCollection.stability(),
            GuidStability::Unstable
        );
        assert_eq!(
            ExportGuidSource::PreservedScene.stability(),
            GuidStability::StableWhenStored
        );
        for source in [
            ExportGuidSource::DeterministicScript,
            ExportGuidSource::PredefinedEngine,
            ExportGuidSource::MissingReference,
        ] {
            assert_eq!(source.stability(), GuidStability::Stable, "{source:?}");
        }
    }

    #[test]
    fn only_the_random_source_stays_open() {
        for source in [
            ExportGuidSource::DeterministicScript,
            ExportGuidSource::PreservedScene,
            ExportGuidSource::PredefinedEngine,
            ExportGuidSource::MissingReference,
        ] {
            assert!(source.byte_layout_in_snapshot(), "{source:?}");
        }
        assert!(!ExportGuidSource::RandomPerCollection.byte_layout_in_snapshot());
    }
}
