//! Caller-side scene facts for the animation conversion pipeline.
//!
//! `AnimationClipConverter` and `CustomCurveResolver` need the materialized asset graph:
//! the clip's root GameObjects (`AnimationClipExtensions.FindRoots`), the transform
//! child walk (`TransformExtensions.FindChild`), component lookups
//! (`GameObjectExtensions.TryGetComponent`), the mesh blend-shape CRC search
//! (`MeshExtensions.FindBlendShapeNameByCRC`), the renderer material-property CRC search
//! (`RendererExtensions.FindMaterialPropertyNameByCRC28`), and the
//! `PathChecksumCache.BuildPathsCache` hierarchy/avatar/TOS population.
//!
//! This module is that boundary: [`AnimationSceneFacts`] is the query surface the
//! converter uses, [`SceneFactsTable`] is a concrete table a production caller fills from
//! materialized assets, and [`build_path_checksum_cache`] reproduces the source
//! `BuildPathsCache` walk over the same facts.

use crate::resources::curve_wrappers::ScriptReference;
use crate::resources::path_checksum::{ChecksumField, PathChecksumCache, PathChecksumError};

/// The query surface the converter needs from a materialized asset graph.
pub trait AnimationSceneFacts {
    /// The clip's root GameObjects in `FindRoots` order.
    fn roots(&self) -> &[i64];

    /// `IGameObject.Name` for a GameObject.
    fn game_object_name(&self, game_object: i64) -> Option<&str>;

    /// The child GameObjects of a GameObject, in transform child order.
    fn children(&self, game_object: i64) -> &[i64];

    /// `TransformExtensions.FindChild(root.GetTransform(), path)`: the GameObject of the
    /// first child whose name matches the next path segment.
    fn find_child(&self, root: i64, path: &str) -> Option<i64>;

    /// `GameObjectExtensions.TryGetComponent` for a set of acceptable class ids, in
    /// serialized component order (first match wins).
    fn component(&self, game_object: i64, class_ids: &[i32]) -> Option<i64>;

    /// `ISkinnedMeshRenderer.MeshP` for a skinned-mesh-renderer asset.
    fn skinned_mesh(&self, renderer: i64) -> Option<i64>;

    /// `MeshExtensions.FindBlendShapeNameByCRC`.
    fn blend_shape_name(&self, mesh: i64, crc: u32) -> Option<&str>;

    /// `RendererExtensions.FindMaterialPropertyNameByCRC28` over the renderer's
    /// materials in order.
    fn material_property_name(&self, renderer: i64, crc28: u32) -> Option<&str>;

    /// `PathChecksumCache.Add(IMonoScript)`: the script's identity plus its serializable
    /// field shapes, or `None` when no managed type is known.
    fn script_facts(&self, script: ScriptReference) -> Option<&ScriptFacts>;
}

/// One script's `SerializableType` facts for the checksum-cache field walk.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptFacts {
    /// A stable identity for the `processedAssets` set (the source `AssetInfo`).
    pub identity: String,
    /// `SerializableType.Fields`.
    pub fields: Vec<ChecksumField>,
}

/// A scene-facts implementation with no assets: the converter then resolves no
/// scene-dependent custom curve and no hierarchy path, exactly like a clip whose roots
/// are absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptySceneFacts;

impl AnimationSceneFacts for EmptySceneFacts {
    fn roots(&self) -> &[i64] {
        &[]
    }

    fn game_object_name(&self, _game_object: i64) -> Option<&str> {
        None
    }

    fn children(&self, _game_object: i64) -> &[i64] {
        &[]
    }

    fn find_child(&self, _root: i64, _path: &str) -> Option<i64> {
        None
    }

    fn component(&self, _game_object: i64, _class_ids: &[i32]) -> Option<i64> {
        None
    }

    fn skinned_mesh(&self, _renderer: i64) -> Option<i64> {
        None
    }

    fn blend_shape_name(&self, _mesh: i64, _crc: u32) -> Option<&str> {
        None
    }

    fn material_property_name(&self, _renderer: i64, _crc28: u32) -> Option<&str> {
        None
    }

    fn script_facts(&self, _script: ScriptReference) -> Option<&ScriptFacts> {
        None
    }
}

/// One materialized GameObject in a [`SceneFactsTable`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameObjectFacts {
    /// `Name`.
    pub name: String,
    /// The component assets in `Components` order, as `(class id, asset id)`.
    pub components: Vec<(i32, i64)>,
    /// The child GameObjects in transform child order.
    pub children: Vec<i64>,
}

/// A concrete [`AnimationSceneFacts`] table over materialized assets.
#[derive(Clone, Debug, Default)]
pub struct SceneFactsTable {
    roots: Vec<i64>,
    game_objects: std::collections::HashMap<i64, GameObjectFacts>,
    skinned_meshes: std::collections::HashMap<i64, i64>,
    blend_shapes: std::collections::HashMap<(i64, u32), String>,
    material_properties: std::collections::HashMap<(i64, u32), String>,
    scripts: std::collections::HashMap<i64, ScriptFacts>,
}

impl SceneFactsTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the clip's root GameObjects in `FindRoots` order.
    pub fn set_roots(&mut self, roots: Vec<i64>) {
        self.roots = roots;
    }

    /// Register one GameObject with its ordered components and children.
    pub fn insert_game_object(&mut self, id: i64, facts: GameObjectFacts) {
        self.game_objects.insert(id, facts);
    }

    /// Register the mesh resolved from a skinned-mesh renderer's `Mesh` pointer.
    pub fn insert_skinned_mesh(&mut self, renderer: i64, mesh: i64) {
        self.skinned_meshes.insert(renderer, mesh);
    }

    /// Register a blend-shape name recovered by CRC for a mesh.
    pub fn insert_blend_shape(&mut self, mesh: i64, crc: u32, name: &str) {
        self.blend_shapes.insert((mesh, crc), name.to_owned());
    }

    /// Register a material property name recovered by CRC-28 for a renderer.
    pub fn insert_material_property(&mut self, renderer: i64, crc28: u32, name: &str) {
        self.material_properties
            .insert((renderer, crc28), name.to_owned());
    }

    /// Register a script's serializable facts under its path id.
    pub fn insert_script(&mut self, path_id: i64, facts: ScriptFacts) {
        self.scripts.insert(path_id, facts);
    }

    fn find_child_inner(&self, game_object: i64, path: &str, start: usize) -> Option<i64> {
        let separator = path[start..].find('/').map(|offset| start + offset);
        let name = match separator {
            Some(index) => &path[start..index],
            None => &path[start..],
        };
        for child in self.children(game_object) {
            let Some(facts) = self.game_objects.get(child) else {
                continue;
            };
            if facts.name == name {
                return match separator {
                    None => Some(*child),
                    Some(index) => self.find_child_inner(*child, path, index + 1),
                };
            }
        }
        None
    }
}

impl AnimationSceneFacts for SceneFactsTable {
    fn roots(&self) -> &[i64] {
        &self.roots
    }

    fn game_object_name(&self, game_object: i64) -> Option<&str> {
        self.game_objects
            .get(&game_object)
            .map(|facts| facts.name.as_str())
    }

    fn children(&self, game_object: i64) -> &[i64] {
        self.game_objects
            .get(&game_object)
            .map(|facts| facts.children.as_slice())
            .unwrap_or(&[])
    }

    fn find_child(&self, root: i64, path: &str) -> Option<i64> {
        if path.is_empty() {
            return Some(root);
        }
        self.find_child_inner(root, path, 0)
    }

    fn component(&self, game_object: i64, class_ids: &[i32]) -> Option<i64> {
        let facts = self.game_objects.get(&game_object)?;
        facts
            .components
            .iter()
            .find(|(class_id, _)| class_ids.contains(class_id))
            .map(|(_, id)| *id)
    }

    fn skinned_mesh(&self, renderer: i64) -> Option<i64> {
        self.skinned_meshes.get(&renderer).copied()
    }

    fn blend_shape_name(&self, mesh: i64, crc: u32) -> Option<&str> {
        self.blend_shapes.get(&(mesh, crc)).map(String::as_str)
    }

    fn material_property_name(&self, renderer: i64, crc28: u32) -> Option<&str> {
        self.material_properties
            .get(&(renderer, crc28))
            .map(String::as_str)
    }

    fn script_facts(&self, script: ScriptReference) -> Option<&ScriptFacts> {
        self.scripts.get(&script.path_id)
    }
}

/// One asset in the `PathChecksumCache.BuildPathsCache` walk, in `FetchAssets` order.
#[derive(Clone, Debug, PartialEq)]
pub enum PathCacheAsset {
    /// An `IAvatar`: its TOS table.
    Avatar {
        /// `Avatar.TOS`.
        tos: Vec<(u32, String)>,
    },
    /// An `IAnimator`: its avatar TOS plus its GameObject hierarchy.
    Animator {
        /// The avatar's TOS table, when the animator has an avatar.
        avatar_tos: Vec<(u32, String)>,
        /// `Has_HasTransformHierarchy()`.
        has_has_transform_hierarchy: bool,
        /// `HasTransformHierarchy`.
        has_transform_hierarchy: bool,
        /// `GameObjectP`.
        game_object: Option<i64>,
    },
    /// An `IAnimation`: its GameObject hierarchy.
    Animation {
        /// `GameObjectP`.
        game_object: Option<i64>,
    },
}

/// `PathChecksumCache.BuildPathsCache`: walks the fetched assets in order, adding avatar
/// TOS entries and GameObject hierarchy paths.
///
/// The animator arm returns early when `Has_HasTransformHierarchy()` holds and
/// `HasTransformHierarchy` is false; a missing GameObject also returns without adding.
pub fn build_path_checksum_cache(
    cache: &mut PathChecksumCache,
    assets: &[PathCacheAsset],
    scene: &dyn AnimationSceneFacts,
) -> Result<(), PathChecksumError> {
    for asset in assets {
        match asset {
            PathCacheAsset::Avatar { tos } => {
                add_avatar_tos(cache, tos)?;
            }
            PathCacheAsset::Animator {
                avatar_tos,
                has_has_transform_hierarchy,
                has_transform_hierarchy,
                game_object,
            } => {
                add_avatar_tos(cache, avatar_tos)?;
                if *has_has_transform_hierarchy && !*has_transform_hierarchy {
                    continue;
                }
                if let Some(game_object) = game_object {
                    add_game_object_paths(cache, scene, *game_object, "")?;
                }
            }
            PathCacheAsset::Animation { game_object } => {
                if let Some(game_object) = game_object {
                    add_game_object_paths(cache, scene, *game_object, "")?;
                }
            }
        }
    }
    Ok(())
}

fn add_avatar_tos(
    cache: &mut PathChecksumCache,
    tos: &[(u32, String)],
) -> Result<(), PathChecksumError> {
    for (key, value) in tos {
        add_keys(cache, *key, value)?;
    }
    Ok(())
}

/// `AddKeys`: stores the path and its checksum in both directions. The source dictionary
/// assignment overwrites any previous entry for the same path, so the reverse direction
/// is overwritten the same way; the entry limit is checked like [`PathChecksumCache::add`].
fn add_keys(
    cache: &mut PathChecksumCache,
    checksum: u32,
    property_name: &str,
) -> Result<(), PathChecksumError> {
    cache.insert_direct(property_name, checksum)
}

fn add_game_object_paths(
    cache: &mut PathChecksumCache,
    scene: &dyn AnimationSceneFacts,
    parent: i64,
    parent_path: &str,
) -> Result<(), PathChecksumError> {
    for child in scene.children(parent) {
        let Some(name) = scene.game_object_name(*child) else {
            continue;
        };
        let path = if parent_path.is_empty() {
            name.to_owned()
        } else {
            format!("{parent_path}/{name}")
        };
        let path_hash = crate::resources::blend_shape::crc32_ieee(path.as_bytes());
        add_keys(cache, path_hash, &path)?;
        add_game_object_paths(cache, scene, *child, &path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> SceneFactsTable {
        let mut table = SceneFactsTable::new();
        table.insert_game_object(
            1,
            GameObjectFacts {
                name: "Root".to_owned(),
                components: vec![(4, 11)],
                children: vec![2, 4],
            },
        );
        table.insert_game_object(
            2,
            GameObjectFacts {
                name: "Body".to_owned(),
                components: vec![(137, 12), (4, 13)],
                children: vec![3],
            },
        );
        table.insert_game_object(
            3,
            GameObjectFacts {
                name: "Head".to_owned(),
                components: vec![(4, 14)],
                children: Vec::new(),
            },
        );
        table.insert_game_object(
            4,
            GameObjectFacts {
                name: "Body".to_owned(),
                components: Vec::new(),
                children: Vec::new(),
            },
        );
        table.insert_skinned_mesh(12, 90);
        table.set_roots(vec![1]);
        table
    }

    #[test]
    fn child_walk_matches_the_transform_search() {
        let table = table();
        // The empty path returns the root itself.
        assert_eq!(table.find_child(1, ""), Some(1));
        assert_eq!(table.find_child(1, "Body"), Some(2));
        assert_eq!(table.find_child(1, "Body/Head"), Some(3));
        // The first same-named sibling wins.
        assert_eq!(table.find_child(1, "Body"), Some(2));
        assert_eq!(table.find_child(1, "Missing"), None);
        assert_eq!(table.find_child(1, "Body/Missing"), None);
    }

    #[test]
    fn component_lookup_respects_order_and_the_class_set() {
        let table = table();
        assert_eq!(table.component(2, &[137]), Some(12));
        // IRenderer covers both renderers; the first serialized match wins.
        assert_eq!(table.component(2, &[137, 23]), Some(12));
        assert_eq!(table.component(2, &[23, 137]), Some(12));
        assert_eq!(table.component(2, &[4]), Some(13));
        assert_eq!(table.component(1, &[137]), None);
    }

    #[test]
    fn crc_lookups_are_per_asset() {
        let mut table = table();
        table.insert_blend_shape(90, 0x1234, "Shape A");
        table.insert_material_property(12, 0x99, "_Color");
        assert_eq!(table.blend_shape_name(90, 0x1234), Some("Shape A"));
        assert_eq!(table.blend_shape_name(90, 0x1235), None);
        assert_eq!(table.material_property_name(12, 0x99), Some("_Color"));
        assert_eq!(table.material_property_name(13, 0x99), None);
    }

    #[test]
    fn hierarchy_paths_enter_the_checksum_cache_like_the_source() {
        let table = table();
        let mut cache =
            PathChecksumCache::new(crate::resources::path_checksum::PathChecksumLimits::default())
                .unwrap();
        build_path_checksum_cache(
            &mut cache,
            &[PathCacheAsset::Animation {
                game_object: Some(1),
            }],
            &table,
        )
        .unwrap();
        // The root itself is never added; descendants are added depth-first.
        assert_eq!(
            cache.try_get_path(crate::resources::blend_shape::crc32_ieee(b"Body")),
            Some("Body")
        );
        assert_eq!(
            cache.try_get_path(crate::resources::blend_shape::crc32_ieee(b"Body/Head")),
            Some("Body/Head")
        );
        assert_eq!(
            cache.try_get_path(crate::resources::blend_shape::crc32_ieee(b"Root")),
            None
        );
    }

    #[test]
    fn avatar_tos_and_the_animator_hierarchy_gate_follow_the_source() {
        let table = table();
        let mut cache =
            PathChecksumCache::new(crate::resources::path_checksum::PathChecksumLimits::default())
                .unwrap();
        let tos = vec![(
            crate::resources::blend_shape::crc32_ieee(b"Spine"),
            "Spine".to_owned(),
        )];
        build_path_checksum_cache(
            &mut cache,
            &[
                PathCacheAsset::Avatar { tos: tos.clone() },
                // A disabled transform hierarchy returns before the walk.
                PathCacheAsset::Animator {
                    avatar_tos: tos,
                    has_has_transform_hierarchy: true,
                    has_transform_hierarchy: false,
                    game_object: Some(1),
                },
            ],
            &table,
        )
        .unwrap();
        assert_eq!(
            cache.try_get_path(crate::resources::blend_shape::crc32_ieee(b"Spine")),
            Some("Spine")
        );
        assert_eq!(
            cache.try_get_path(crate::resources::blend_shape::crc32_ieee(b"Body")),
            None
        );
        // With the gate open the hierarchy walk runs.
        let mut cache =
            PathChecksumCache::new(crate::resources::path_checksum::PathChecksumLimits::default())
                .unwrap();
        build_path_checksum_cache(
            &mut cache,
            &[PathCacheAsset::Animator {
                avatar_tos: Vec::new(),
                has_has_transform_hierarchy: true,
                has_transform_hierarchy: true,
                game_object: Some(1),
            }],
            &table,
        )
        .unwrap();
        assert_eq!(
            cache.try_get_path(crate::resources::blend_shape::crc32_ieee(b"Body")),
            Some("Body")
        );
    }
}
