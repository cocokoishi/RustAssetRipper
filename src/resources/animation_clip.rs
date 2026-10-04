//! `AnimationClipExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/AnimationClipExtensions.cs`. The legacy flag is
//! read directly when the clip serializes it, otherwise it is derived from the animation type. The
//! negative-infinity-slope support is a version check.

/// `AnimationType.Legacy`.
pub const ANIMATION_TYPE_LEGACY: i32 = 1;

/// An animation-clip view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AnimationClipView {
    /// `Has_Legacy_C74`.
    pub has_legacy: bool,
    /// `Legacy_C74`.
    pub legacy: bool,
    /// `AnimationType_C74`.
    pub animation_type: i32,
    /// The collection's Unity major version.
    pub version_major: u32,
}

impl AnimationClipView {
    /// `AnimationClipExtensions.GetLegacy`.
    pub fn get_legacy(&self) -> bool {
        if self.has_legacy {
            self.legacy
        } else {
            self.animation_type == ANIMATION_TYPE_LEGACY
        }
    }

    /// `AnimationClipExtensions.SupportsNegativeInfinitySlopes`.
    pub fn supports_negative_infinity_slopes(&self) -> bool {
        self.version_major > 2021
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn major(version: &str) -> u32 {
        version.split('.').next().unwrap().parse().unwrap()
    }

    /// Byte-for-byte differential against the reference `AnimationClipExtensions` (probe
    /// `Temp/opencode/clip2probe`). Every version reachable through the factory serializes
    /// `Legacy_C74`, so the animation-type fallback is implemented but not exercised by the
    /// reference runtime; it is asserted directly instead.
    #[test]
    fn animation_clip_matches_the_reference_runtime() {
        let fixture = include_str!("animation_clip_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "AC", "{line}");
            let has_legacy = parts[2] == "1";
            let base = AnimationClipView {
                has_legacy,
                legacy: false,
                animation_type: 0,
                version_major: major(parts[1]),
            };
            assert_eq!(base.get_legacy(), parts[3] == "1", "{line}");
            let after = if has_legacy {
                AnimationClipView {
                    legacy: true,
                    ..base
                }
            } else {
                AnimationClipView {
                    animation_type: ANIMATION_TYPE_LEGACY,
                    ..base
                }
            };
            assert_eq!(after.get_legacy(), parts[4] == "1", "{line}");
            assert_eq!(
                base.supports_negative_infinity_slopes(),
                parts[5] == "1",
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 9);

        let fallback = AnimationClipView {
            has_legacy: false,
            legacy: false,
            animation_type: ANIMATION_TYPE_LEGACY,
            version_major: 2019,
        };
        assert!(fallback.get_legacy());
    }
}
