use super::*;

/// Workspace key of one serialized object.
type TerrainObjectKey = (usize, usize);

pub(super) struct PreparedTerrainCollections {
    pub(super) roots: HashMap<TerrainObjectKey, TerrainCollectionPlan>,
    pub(super) member_keys: HashSet<TerrainObjectKey>,
}

pub(super) struct TerrainCollectionPlan {
    pub(super) children: Vec<TerrainMemberPlan>,
}

pub(super) struct TerrainMemberPlan {
    pub(super) key: TerrainObjectKey,
    pub(super) file_id: i64,
}

/// Gather the splat-alpha textures each TerrainData owns.
///
/// The frozen `TerrainYamlExportCollection` starts from the terrain data and adds every
/// `SplatDatabase.AlphaTextures` texture whose `MainAsset` is that terrain data.
/// `MainAssetProcessor` assigns that pairing with `alphaTexture.MainAsset ??= terrainData`,
/// so a texture shared by two terrain data blocks stays with the first one in workspace
/// order, and `AssetsExportCollection.AddAsset` keeps the first registration on duplicates.
/// Each added member receives `GetPseudoRandomExportId(asset, m_exportIDs.Count)`: the main
/// asset is not registered as a member, so the first added texture is seeded with 0.
pub(super) fn prepare_terrain_collections(
    objects: &ObjectWorkspace,
    graph: &MultiFileReferenceGraph<'_>,
    already_owned: &HashSet<ExportAssetObjectKey>,
    project_version: SceneUnityVersion,
    limits: ExportPointerMapLimits,
) -> Result<PreparedTerrainCollections, Box<dyn Error>> {
    let mut prepared = PreparedTerrainCollections {
        roots: HashMap::new(),
        member_keys: HashSet::new(),
    };
    let mut claimed = HashSet::<TerrainObjectKey>::new();
    let mut planned_assets = 0usize;
    for root in objects
        .objects()
        .iter()
        .filter(|object| object.factory.class_id == crate::export::res_wiring::TERRAIN_CLASS_ID)
    {
        let key = (root.file_index, root.object_index);
        if already_owned.contains(&source_object_key(key.0, key.1)) {
            continue;
        }
        let Ok(payload) = root.payload.as_ref() else {
            // A terrain data block without a decoded payload exports through its fallback
            // route and cannot contribute splat-alpha members.
            continue;
        };
        let Some(splat_database) = payload
            .root
            .unique_field_in_base_chain("m_SplatDatabase")
            .map_err(|error| {
                invalid_project(format!(
                    "TerrainData {}:{} m_SplatDatabase lookup failed: {error}",
                    root.file_index, root.path_id
                ))
            })?
        else {
            continue;
        };
        let Some(alpha_textures) =
            splat_database
                .unique_field("m_AlphaTextures")
                .map_err(|error| {
                    invalid_project(format!(
                        "TerrainData {}:{} m_AlphaTextures lookup failed: {error}",
                        root.file_index, root.path_id
                    ))
                })?
        else {
            continue;
        };
        let Some(elements) = alpha_textures.array_elements() else {
            return Err(invalid_project(format!(
                "TerrainData {}:{} m_AlphaTextures is not an array",
                root.file_index, root.path_id
            )));
        };
        let mut children = Vec::new();
        children.try_reserve_exact(elements.len()).map_err(|_| {
            invalid_project("could not allocate terrain splat-alpha collection members")
        })?;
        for element in elements {
            let Some((file_id, path_id)) = element.pptr() else {
                return Err(invalid_project(format!(
                    "TerrainData {}:{} m_AlphaTextures element is not a PPtr",
                    root.file_index, root.path_id
                )));
            };
            if path_id == 0 {
                continue;
            }
            let Some(target) = resolve_splat_alpha_target(graph, root, file_id, path_id)? else {
                // Predefined engine textures keep themselves as their main asset, so the
                // frozen collection never adopts them.
                continue;
            };
            push_splat_alpha_member(&mut claimed, &mut children, target, project_version)?;
        }
        if children.is_empty() {
            continue;
        }
        planned_assets = planned_assets
            .checked_add(children.len())
            .filter(|count| *count <= limits.max_assets)
            .ok_or_else(|| {
                invalid_project("TerrainData collections exceed the export asset limit")
            })?;
        prepared
            .member_keys
            .try_reserve(children.len())
            .map_err(|_| invalid_project("could not allocate terrain splat-alpha member lookup"))?;
        prepared
            .member_keys
            .extend(children.iter().map(|child| child.key));
        prepared
            .roots
            .try_reserve(1)
            .map_err(|_| invalid_project("could not allocate TerrainData collections"))?;
        prepared
            .roots
            .insert(key, TerrainCollectionPlan { children });
    }
    Ok(prepared)
}

/// Add one resolved splat-alpha texture to the terrain collection that owns it.
///
/// `MainAssetProcessor` pairs a texture with the first terrain data that lists it
/// (`alphaTexture.MainAsset ??= terrainData`), so `claimed` keeps that ownership across
/// terrain collections and a texture another terrain already owns is refused here.
/// `AssetsExportCollection.AddAsset` refuses the primary asset and duplicates and seeds
/// each added member with `GetPseudoRandomExportId(texture, m_exportIDs.Count)`, which is
/// the member insertion count, so the first added texture is seeded with 0.
fn push_splat_alpha_member(
    claimed: &mut HashSet<TerrainObjectKey>,
    children: &mut Vec<TerrainMemberPlan>,
    target: TerrainObjectKey,
    project_version: SceneUnityVersion,
) -> Result<(), Box<dyn Error>> {
    if !claimed.insert(target) {
        return Ok(());
    }
    let seed = i32::try_from(children.len()).map_err(|_| {
        invalid_project("terrain splat-alpha member count exceeds the export ID seed range")
    })?;
    let file_id = crate::export::ids::get_pseudo_random_export_id(
        crate::export::res_wiring::TEXTURE2D_CLASS_ID,
        seed,
        project_version,
    );
    children.push(TerrainMemberPlan {
        key: target,
        file_id,
    });
    Ok(())
}

/// Resolve one `m_AlphaTextures` pointer to its workspace key.
///
/// Predefined engine textures are not members (their main asset is themselves, not the
/// terrain data). A pointer the graph classifies as null after a non-null PathID, or any
/// pointer that stays unresolved, reports a diagnostic instead of being dropped.
fn resolve_splat_alpha_target(
    graph: &MultiFileReferenceGraph<'_>,
    root: &WorkspaceObject,
    file_id: i32,
    path_id: i64,
) -> Result<Option<TerrainObjectKey>, Box<dyn Error>> {
    let mut edges = graph
        .references_from_object(root.file_index, root.object_index)
        .filter(|edge| edge.file_id == file_id && edge.path_id == path_id);
    let Some(edge) = edges.next() else {
        return Err(invalid_project(format!(
            "TerrainData {}:{} splat-alpha PPtr ({file_id}, {path_id}) has no matching reference-graph edge",
            root.file_index, root.path_id
        )));
    };
    match &edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => Ok(Some((*target_file_index, *target_object_index))),
        MultiFilePPtrResolution::PredefinedEngineAsset { .. } => Ok(None),
        MultiFilePPtrResolution::Null => Err(invalid_project(format!(
            "TerrainData {}:{} non-null splat-alpha PPtr ({file_id}, {path_id}) was classified as null",
            root.file_index, root.path_id
        ))),
        unresolved => Err(invalid_project(format!(
            "TerrainData {}:{} splat-alpha PPtr ({file_id}, {path_id}) is unresolved: {unresolved:?}",
            root.file_index, root.path_id
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::push_splat_alpha_member;
    use crate::processing::scene_naming::SceneUnityVersion;
    use std::collections::HashSet;

    fn version() -> SceneUnityVersion {
        SceneUnityVersion::parse("2022.3.22f1").unwrap()
    }

    #[test]
    fn splat_alpha_members_use_insertion_seeded_ids_and_the_first_owner_keeps_them() {
        let mut claimed = HashSet::new();
        let mut children = Vec::new();
        push_splat_alpha_member(&mut claimed, &mut children, (0, 11), version()).unwrap();
        push_splat_alpha_member(&mut claimed, &mut children, (0, 12), version()).unwrap();
        // A texture listed twice in one array registers one member, mirroring `AddAsset`.
        push_splat_alpha_member(&mut claimed, &mut children, (0, 11), version()).unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].key, (0, 11));
        assert_eq!(children[1].key, (0, 12));
        // `GetPseudoRandomExportId(Texture2D, insertion count)` prefixes class 28 with the
        // seed-zero xxHash64 value, so the first added member is seeded with 0.
        assert_eq!(children[0].file_id, 28_709_254_077_376_921);
        assert_eq!(children[1].file_id, 28_730_242_686_860_875);

        // A second terrain data listing the same texture does not take it over.
        let mut second_terrain = Vec::new();
        push_splat_alpha_member(&mut claimed, &mut second_terrain, (0, 11), version()).unwrap();
        assert!(second_terrain.is_empty());
        // A texture only the second terrain lists still starts its own collection at seed 0.
        push_splat_alpha_member(&mut claimed, &mut second_terrain, (0, 13), version()).unwrap();
        assert_eq!(second_terrain.len(), 1);
        assert_eq!(second_terrain[0].file_id, 28_709_254_077_376_921);
    }
}
