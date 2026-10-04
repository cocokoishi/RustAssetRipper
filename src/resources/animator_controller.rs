//! `AnimatorControllerExtensions`, `AnimatorOverrideControllerExtensions`, and
//! `RuntimeAnimatorControllerExtensions`.
//!
//! Mirrors the `ContainsAnimationClip` overloads: an animator controller owns a clip pointer list,
//! an override controller owns original/override pairs plus a base controller, and the runtime
//! dispatch hands off to whichever of the two concrete controller kinds it holds.

use super::pptr::{is_asset, AssetRef, PPtrView};

/// A failure while resolving a runtime animator controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerError {
    /// The runtime controller was neither an animator controller nor an override controller.
    UnsupportedRuntimeAnimatorController,
}

impl core::fmt::Display for ControllerError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedRuntimeAnimatorController => {
                write!(formatter, "unsupported runtime animator controller type")
            }
        }
    }
}

impl std::error::Error for ControllerError {}

/// An animator-controller view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatorControllerView {
    /// `AnimationClips`.
    pub animation_clips: Vec<PPtrView>,
}

impl AnimatorControllerView {
    /// `AnimatorControllerExtensions.ContainsAnimationClip`.
    pub fn contains_animation_clip(
        &self,
        file: usize,
        dependencies: &[usize],
        clip: AssetRef,
    ) -> bool {
        self.animation_clips
            .iter()
            .any(|pointer| is_asset(*pointer, file, dependencies, clip))
    }
}

/// One original/override clip pair.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipOverrideView {
    /// `OriginalClip`.
    pub original_clip: PPtrView,
    /// `OverrideClip`.
    pub override_clip: PPtrView,
}

/// An animator-override-controller view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatorOverrideControllerView {
    /// `Clips`.
    pub clips: Vec<ClipOverrideView>,
    /// `Controller`.
    pub controller: Option<RuntimeAnimatorControllerRef>,
}

impl AnimatorOverrideControllerView {
    /// `AnimatorOverrideControllerExtensions.ContainsAnimationClip`.
    pub fn contains_animation_clip(
        &self,
        file: usize,
        dependencies: &[usize],
        clip: AssetRef,
    ) -> Result<bool, ControllerError> {
        for pair in &self.clips {
            if is_asset(pair.original_clip, file, dependencies, clip)
                || is_asset(pair.override_clip, file, dependencies, clip)
            {
                return Ok(true);
            }
        }
        match &self.controller {
            Some(base) => runtime_contains(base, file, dependencies, clip),
            None => Ok(false),
        }
    }
}

/// A runtime animator controller the dispatch can hold.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeAnimatorControllerRef {
    /// An `AnimatorController`.
    Animator(AnimatorControllerView),
    /// An `AnimatorOverrideController`.
    Override(Box<AnimatorOverrideControllerView>),
    /// Any other runtime controller type, which the reference cannot handle.
    Other,
}

/// `RuntimeAnimatorControllerExtensions.ContainsAnimationClip`.
pub fn runtime_contains(
    controller: &RuntimeAnimatorControllerRef,
    file: usize,
    dependencies: &[usize],
    clip: AssetRef,
) -> Result<bool, ControllerError> {
    match controller {
        RuntimeAnimatorControllerRef::Animator(animator) => {
            Ok(animator.contains_animation_clip(file, dependencies, clip))
        }
        RuntimeAnimatorControllerRef::Override(override_controller) => {
            override_controller.contains_animation_clip(file, dependencies, clip)
        }
        RuntimeAnimatorControllerRef::Other => {
            Err(ControllerError::UnsupportedRuntimeAnimatorController)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(path_id: i64) -> PPtrView {
        PPtrView {
            path_id,
            file_id: 0,
        }
    }

    /// Byte-for-byte differential against the reference controller extensions (probe
    /// `Temp/opencode/animctrlprobe`).
    #[test]
    fn controller_contains_animation_clip_matches_the_reference_runtime() {
        let clip_a = AssetRef {
            collection: 1,
            path_id: 1,
        };
        let clip_b = AssetRef {
            collection: 1,
            path_id: 2,
        };
        let clip_c = AssetRef {
            collection: 1,
            path_id: 3,
        };
        let controller = AnimatorControllerView {
            animation_clips: vec![pointer(1)],
        };
        let base = AnimatorControllerView {
            animation_clips: vec![pointer(3)],
        };
        let override_controller = AnimatorOverrideControllerView {
            clips: vec![ClipOverrideView {
                original_clip: pointer(1),
                override_clip: pointer(2),
            }],
            controller: Some(RuntimeAnimatorControllerRef::Animator(base)),
        };

        let fixture = include_str!("animator_controller_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "ACTC", "{line}");
            assert_eq!(
                controller.contains_animation_clip(1, &[], clip_a),
                parts[2] == "1",
                "{line}"
            );
            assert_eq!(
                controller.contains_animation_clip(1, &[], clip_b),
                parts[3] == "1",
                "{line}"
            );
            assert_eq!(
                override_controller
                    .contains_animation_clip(1, &[], clip_a)
                    .unwrap(),
                parts[4] == "1",
                "{line}"
            );
            assert_eq!(
                override_controller
                    .contains_animation_clip(1, &[], clip_b)
                    .unwrap(),
                parts[5] == "1",
                "{line}"
            );
            assert_eq!(
                override_controller
                    .contains_animation_clip(1, &[], clip_c)
                    .unwrap(),
                parts[6] == "1",
                "{line}"
            );
            assert_eq!(
                runtime_contains(
                    &RuntimeAnimatorControllerRef::Animator(controller.clone()),
                    1,
                    &[],
                    clip_a
                )
                .unwrap(),
                parts[7] == "1",
                "{line}"
            );
            assert_eq!(
                runtime_contains(
                    &RuntimeAnimatorControllerRef::Override(Box::new(override_controller.clone())),
                    1,
                    &[],
                    clip_c
                )
                .unwrap(),
                parts[8] == "1",
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);

        assert_eq!(
            runtime_contains(&RuntimeAnimatorControllerRef::Other, 1, &[], clip_a),
            Err(ControllerError::UnsupportedRuntimeAnimatorController)
        );
    }
}
