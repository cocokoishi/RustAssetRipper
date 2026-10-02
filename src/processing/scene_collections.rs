//! Classification of serialized scene-collection manager objects.

/// AssetRipper's generated ClassIDType extension marks these four classes as
/// inheriting from LevelGameManager.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneManagerKind {
    OcclusionCullingSettings,
    RenderSettings,
    LightmapSettings,
    NavMeshSettings,
}

impl SceneManagerKind {
    pub const fn class_id(self) -> i32 {
        match self {
            Self::OcclusionCullingSettings => 29,
            Self::RenderSettings => 104,
            Self::LightmapSettings => 157,
            Self::NavMeshSettings => 196,
        }
    }
}

/// Return the source LevelGameManager implementation represented by a class ID.
pub const fn scene_manager_kind(class_id: Option<i32>) -> Option<SceneManagerKind> {
    match class_id {
        Some(29) => Some(SceneManagerKind::OcclusionCullingSettings),
        Some(104) => Some(SceneManagerKind::RenderSettings),
        Some(157) => Some(SceneManagerKind::LightmapSettings),
        Some(196) => Some(SceneManagerKind::NavMeshSettings),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{scene_manager_kind, SceneManagerKind};

    #[test]
    fn identifies_the_baseline_level_game_manager_class_ids() {
        for (class_id, expected) in [
            (29, SceneManagerKind::OcclusionCullingSettings),
            (104, SceneManagerKind::RenderSettings),
            (157, SceneManagerKind::LightmapSettings),
            (196, SceneManagerKind::NavMeshSettings),
        ] {
            assert_eq!(scene_manager_kind(Some(class_id)), Some(expected));
            assert_eq!(expected.class_id(), class_id);
        }
        for class_id in [None, Some(1), Some(4), Some(141), Some(238), Some(1045)] {
            assert_eq!(scene_manager_kind(class_id), None);
        }
    }
}
