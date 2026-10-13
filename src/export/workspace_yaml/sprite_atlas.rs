use crate::export::pointer_map::{ProjectPointerRequest, UnresolvedProjectPointer};
use crate::export::scene_collection::{ExportAssetCollectionIdentity, ExportAssetIdentity};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::reference_graph::{
    MultiFileObjectReference, MultiFilePPtrResolution, MultiFileReferenceGraph,
};

pub(super) struct WorkspaceSpriteAtlasYamlOverlay {
    pub(super) source: ExportAssetIdentity,
    pub(super) root: SerializedValue,
    pub(super) pointers: Vec<WorkspaceSpriteAtlasPointer>,
}

pub(super) struct WorkspaceSpriteAtlasPointer {
    pub(super) field_path: String,
    pub(super) path_id: i64,
    pub(super) target: ProjectPointerRequest,
}

/// Rebuild the render data copied by AssetRipper's SpriteProcessor for an atlas-backed Sprite.
///
/// The atlas map and all pointer edges are resolved in the atlas object's own file scope. The
/// copied Sprite values use fileID zero only as a temporary YAML-walker token; each non-null
/// pointer is paired with its graph-resolved target and replaced through the export pointer map.
pub(super) fn workspace_sprite_atlas_yaml_overlay(
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    sprite: &WorkspaceObject,
    source: ExportAssetIdentity,
) -> Result<Option<WorkspaceSpriteAtlasYamlOverlay>, String> {
    if source.class_id != 213 || sprite.factory.class_id != 213 {
        return Err(format!(
            "Sprite atlas overlay received source class {} and workspace class {}",
            source.class_id, sprite.factory.class_id
        ));
    }
    let sprite_payload = sprite
        .payload
        .as_ref()
        .map_err(|error| format!("Sprite payload could not be decoded: {error}"))?;
    let atlas_pointer = sprite_payload
        .root
        .unique_field_in_base_chain("m_SpriteAtlas")
        .map_err(|error| format!("could not inspect Sprite.m_SpriteAtlas: {error}"))?;
    let Some(atlas_pointer) = atlas_pointer else {
        return Ok(None);
    };
    let SerializedValue::PPtr { file_id, path_id } = atlas_pointer else {
        return Err(format!(
            "Sprite.m_SpriteAtlas has value {atlas_pointer:?}, expected a PPtr"
        ));
    };
    if *path_id == 0 {
        return Ok(None);
    }

    let atlas_field_path = field_path_in_base_chain(&sprite_payload.root, "m_SpriteAtlas")?
        .ok_or_else(|| "Sprite.m_SpriteAtlas has no serialized field path".to_owned())?;
    let atlas_reference = unique_reference_for_field(
        reference_graph,
        sprite.file_index,
        sprite.object_index,
        &atlas_field_path,
        *file_id,
        *path_id,
    )?;
    let (atlas_file_index, atlas_object_index) = match &atlas_reference.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => (*target_file_index, *target_object_index),
        resolution => {
            return Err(format!(
                "Sprite.m_SpriteAtlas at {atlas_field_path:?} did not resolve to one loaded object: {resolution:?}"
            ));
        }
    };
    if reference_graph.object_class_id(atlas_file_index, atlas_object_index) != Some(687_078_895) {
        return Err(format!(
            "Sprite.m_SpriteAtlas resolves to class {:?}, expected SpriteAtlas class 687078895",
            reference_graph.object_class_id(atlas_file_index, atlas_object_index)
        ));
    }
    if reference_graph.object_scan_is_complete(atlas_file_index, atlas_object_index) != Some(true) {
        return Err(format!(
            "SpriteAtlas file {atlas_file_index} object {atlas_object_index} has an incomplete reference scan"
        ));
    }
    let atlas = objects
        .get(atlas_file_index, atlas_object_index)
        .ok_or_else(|| {
            format!(
                "SpriteAtlas file {atlas_file_index} object {atlas_object_index} is absent from the decoded object workspace"
            )
        })?;
    let atlas_payload = atlas
        .payload
        .as_ref()
        .map_err(|error| format!("SpriteAtlas payload could not be decoded: {error}"))?;
    let render_data_key = sprite_payload
        .root
        .unique_field_in_base_chain("m_RenderDataKey")
        .map_err(|error| format!("could not inspect Sprite.m_RenderDataKey: {error}"))?
        .ok_or_else(|| "atlas-backed Sprite has no m_RenderDataKey".to_owned())?;
    let render_data_map = atlas_payload
        .root
        .unique_field_in_base_chain("m_RenderDataMap")
        .map_err(|error| format!("could not inspect SpriteAtlas.m_RenderDataMap: {error}"))?
        .ok_or_else(|| "SpriteAtlas has no m_RenderDataMap".to_owned())?;
    let render_data_map_path = field_path_in_base_chain(&atlas_payload.root, "m_RenderDataMap")?
        .ok_or_else(|| "SpriteAtlas.m_RenderDataMap has no serialized field path".to_owned())?;
    let entries = map_entries(render_data_map)?;
    let (entry_index, atlas_render_data, data_name) =
        find_render_data_entry(entries, render_data_key)?;
    let atlas_data_path = format!("{render_data_map_path}[{entry_index}].{data_name}");
    let sprite_render_data = sprite_payload
        .root
        .unique_field_in_base_chain("m_RD")
        .map_err(|error| format!("could not inspect Sprite.m_RD: {error}"))?
        .ok_or_else(|| "Sprite has no m_RD".to_owned())?;
    let sprite_render_data_path = field_path_in_base_chain(&sprite_payload.root, "m_RD")?
        .ok_or_else(|| "Sprite.m_RD has no serialized field path".to_owned())?;

    let mut pointers = Vec::new();
    let mut rebuilt_render_data = sprite_render_data.clone();
    copy_atlas_render_data_fields(
        atlas_render_data,
        &atlas_data_path,
        &mut rebuilt_render_data,
        &sprite_render_data_path,
        atlas_file_index,
        atlas_object_index,
        reference_graph,
        &mut pointers,
    )?;

    let mut root = sprite_payload.root.clone();
    *root
        .unique_field_in_base_chain_mut("m_RD")
        .map_err(|error| format!("could not update Sprite.m_RD: {error}"))?
        .ok_or_else(|| "Sprite has no m_RD to update".to_owned())? = rebuilt_render_data;
    let atlas_pointer = root
        .unique_field_in_base_chain_mut("m_SpriteAtlas")
        .map_err(|error| format!("could not clear Sprite.m_SpriteAtlas: {error}"))?
        .ok_or_else(|| {
            "Sprite.m_SpriteAtlas disappeared while rebuilding its payload".to_owned()
        })?;
    *atlas_pointer = SerializedValue::PPtr {
        file_id: 0,
        path_id: 0,
    };
    pointers.push(WorkspaceSpriteAtlasPointer {
        field_path: atlas_field_path,
        path_id: 0,
        target: ProjectPointerRequest::Null,
    });
    if let Some(atlas_tags) = root
        .unique_field_in_base_chain_mut("m_AtlasTags")
        .map_err(|error| format!("could not inspect Sprite.m_AtlasTags: {error}"))?
    {
        let SerializedValue::Array(tags) = atlas_tags else {
            return Err(format!(
                "Sprite.m_AtlasTags has value {atlas_tags:?}, expected an array"
            ));
        };
        tags.clear();
    }

    Ok(Some(WorkspaceSpriteAtlasYamlOverlay {
        source,
        root,
        pointers,
    }))
}

#[allow(clippy::too_many_arguments)]
fn copy_atlas_render_data_fields(
    atlas_data: &SerializedValue,
    atlas_data_path: &str,
    sprite_render_data: &mut SerializedValue,
    sprite_render_data_path: &str,
    atlas_file_index: usize,
    atlas_object_index: usize,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_overlays: &mut Vec<WorkspaceSpriteAtlasPointer>,
) -> Result<(), String> {
    let fields = [
        ("texture", true),
        ("alphaTexture", false),
        ("textureRect", true),
        // SpriteExtensions.GetSpriteCoordinatesInAtlas reads this atlas-owned
        // crop offset before SpriteProcessor overwrites SpriteRenderData's value.
        ("textureRectOffset", true),
        ("atlasRectOffset", false),
        ("settingsRaw", true),
        ("uvTransform", false),
        ("downscaleMultiplier", true),
        ("secondaryTextures", false),
    ];
    for (field_name, required) in fields {
        let source_value = atlas_data
            .unique_field_in_base_chain(field_name)
            .map_err(|error| format!("could not inspect SpriteAtlasData.{field_name}: {error}"))?;
        let target_exists = sprite_render_data
            .unique_field_in_base_chain(field_name)
            .map_err(|error| format!("could not inspect SpriteRenderData.{field_name}: {error}"))?
            .is_some();
        match (source_value, target_exists) {
            (Some(source_value), true) => {
                let source_field_path = field_path_in_base_chain(atlas_data, field_name)?
                    .ok_or_else(|| format!("SpriteAtlasData.{field_name} has no field path"))?;
                let target_field_path = field_path_in_base_chain(sprite_render_data, field_name)?
                    .ok_or_else(|| {
                    format!("SpriteRenderData.{field_name} has no field path")
                })?;
                let mut value = source_value.clone();
                normalize_atlas_pointers(
                    source_value,
                    &mut value,
                    &join_field_path(atlas_data_path, &source_field_path),
                    &join_field_path(sprite_render_data_path, &target_field_path),
                    atlas_file_index,
                    atlas_object_index,
                    reference_graph,
                    pointer_overlays,
                )?;
                *sprite_render_data
                    .unique_field_in_base_chain_mut(field_name)
                    .map_err(|error| {
                        format!("could not update SpriteRenderData.{field_name}: {error}")
                    })?
                    .ok_or_else(|| {
                        format!("SpriteRenderData.{field_name} disappeared while copying")
                    })? = value;
            }
            (None, false) if !required => {}
            (Some(_), false) if !required => {}
            (None, true) if !required => {}
            (Some(_), false) => {
                return Err(format!(
                    "SpriteRenderData has no required {field_name} field present in SpriteAtlasData"
                ));
            }
            (None, true) => {
                return Err(format!(
                    "SpriteAtlasData has no required {field_name} field present in SpriteRenderData"
                ));
            }
            (None, false) => {
                return Err(format!(
                    "SpriteAtlasData and SpriteRenderData both lack required {field_name}"
                ));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn normalize_atlas_pointers(
    source: &SerializedValue,
    target: &mut SerializedValue,
    source_path: &str,
    target_path: &str,
    atlas_file_index: usize,
    atlas_object_index: usize,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_overlays: &mut Vec<WorkspaceSpriteAtlasPointer>,
) -> Result<(), String> {
    let mut resolve = |source_path: &str, file_id, path_id| {
        let reference = unique_reference_for_field(
            reference_graph,
            atlas_file_index,
            atlas_object_index,
            source_path,
            file_id,
            path_id,
        )?;
        project_pointer_request(reference, reference_graph)
    };
    normalize_atlas_pointers_with_resolver(
        source,
        target,
        source_path,
        target_path,
        &mut resolve,
        pointer_overlays,
    )
}

fn normalize_atlas_pointers_with_resolver<F>(
    source: &SerializedValue,
    target: &mut SerializedValue,
    source_path: &str,
    target_path: &str,
    resolve: &mut F,
    pointer_overlays: &mut Vec<WorkspaceSpriteAtlasPointer>,
) -> Result<(), String>
where
    F: FnMut(&str, i32, i64) -> Result<ProjectPointerRequest, String>,
{
    match (source, target) {
        (
            SerializedValue::PPtr {
                file_id: source_file_id,
                path_id: source_path_id,
            },
            SerializedValue::PPtr { file_id, path_id },
        ) => {
            if *source_path_id == 0 {
                *file_id = 0;
                *path_id = 0;
                pointer_overlays.push(WorkspaceSpriteAtlasPointer {
                    field_path: target_path.to_owned(),
                    path_id: 0,
                    target: ProjectPointerRequest::Null,
                });
                return Ok(());
            }
            if *file_id != *source_file_id || *path_id != *source_path_id {
                return Err(format!(
                    "copied atlas pointer at {source_path:?} changed before scope conversion"
                ));
            }
            let request = resolve(source_path, *source_file_id, *source_path_id)?;
            pointer_overlays.push(WorkspaceSpriteAtlasPointer {
                field_path: target_path.to_owned(),
                path_id: *source_path_id,
                target: request,
            });
            *file_id = 0;
            Ok(())
        }
        (SerializedValue::Struct(source_fields), SerializedValue::Struct(target_fields)) => {
            validate_unique_field_names(source_fields, source_path)?;
            validate_unique_field_names(target_fields, target_path)?;
            for source_field in source_fields {
                let target_field = target_fields
                    .iter_mut()
                    .find(|field| field.name == source_field.name)
                    .ok_or_else(|| {
                        format!(
                            "copied atlas field {source_path}.{} is absent at {target_path}",
                            source_field.name
                        )
                    })?;
                normalize_atlas_pointers_with_resolver(
                    &source_field.value,
                    &mut target_field.value,
                    &join_field_path(source_path, &source_field.name),
                    &join_field_path(target_path, &target_field.name),
                    resolve,
                    pointer_overlays,
                )?;
            }
            Ok(())
        }
        (SerializedValue::Array(source_items), SerializedValue::Array(target_items)) => {
            if source_items.len() != target_items.len() {
                return Err(format!(
                    "copied atlas array at {source_path:?} changed length from {} to {}",
                    source_items.len(),
                    target_items.len()
                ));
            }
            for (index, (source_item, target_item)) in
                source_items.iter().zip(target_items).enumerate()
            {
                normalize_atlas_pointers_with_resolver(
                    source_item,
                    target_item,
                    &join_array_path(source_path, index),
                    &join_array_path(target_path, index),
                    resolve,
                    pointer_overlays,
                )?;
            }
            Ok(())
        }
        (SerializedValue::PPtr { .. }, _) | (_, SerializedValue::PPtr { .. }) => Err(format!(
            "SpriteAtlasData pointer at {source_path:?} does not match copied Sprite field {target_path:?}"
        )),
        (SerializedValue::Struct(_), _) | (_, SerializedValue::Struct(_)) => Err(format!(
            "SpriteAtlasData struct at {source_path:?} does not match copied Sprite field {target_path:?}"
        )),
        (SerializedValue::Array(_), _) | (_, SerializedValue::Array(_)) => Err(format!(
            "SpriteAtlasData array at {source_path:?} does not match copied Sprite field {target_path:?}"
        )),
        _ => Ok(()),
    }
}

fn unique_reference_for_field<'graph, 'data>(
    reference_graph: &'graph MultiFileReferenceGraph<'data>,
    file_index: usize,
    object_index: usize,
    field_path: &str,
    file_id: i32,
    path_id: i64,
) -> Result<&'graph MultiFileObjectReference<'data>, String> {
    let mut matches = reference_graph
        .references_from_object(file_index, object_index)
        .filter(|reference| reference.field_path == field_path);
    let Some(reference) = matches.next() else {
        return Err(format!(
            "reference graph has no edge for object {object_index} field {field_path:?}"
        ));
    };
    if matches.next().is_some() {
        return Err(format!(
            "reference graph has multiple edges for object {object_index} field {field_path:?}"
        ));
    }
    if reference.file_id != file_id || reference.path_id != path_id {
        return Err(format!(
            "reference graph edge for {field_path:?} has ({}, {}), serialized value has ({file_id}, {path_id})",
            reference.file_id, reference.path_id
        ));
    }
    Ok(reference)
}

fn project_pointer_request(
    reference: &MultiFileObjectReference<'_>,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<ProjectPointerRequest, String> {
    match &reference.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => {
            let identity = reference_graph
                .object_identity(*target_file_index, *target_object_index)
                .ok_or_else(|| "atlas pointer target has no object identity".to_owned())?;
            let class_id = reference_graph
                .object_class_id(*target_file_index, *target_object_index)
                .ok_or_else(|| "atlas pointer target has no class ID".to_owned())?;
            Ok(ProjectPointerRequest::ResolvedAsset(ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::SerializedFile {
                    file_index: *target_file_index,
                },
                object_index: *target_object_index,
                path_id: identity.path_id,
                class_id,
            }))
        }
        MultiFilePPtrResolution::PredefinedEngineAsset {
            resource, path_id, ..
        } => Ok(ProjectPointerRequest::PredefinedEngineAsset {
            path_id: *path_id,
            resource: *resource,
        }),
        MultiFilePPtrResolution::Null => Err(format!(
            "non-null atlas pointer at {:?} resolved as null",
            reference.field_path
        )),
        resolution => Ok(ProjectPointerRequest::UnresolvedInput(
            UnresolvedProjectPointer {
                source_file_index: reference.source_file_index,
                source_object_index: reference.source_object_index,
                source_path_id: reference.source_path_id,
                field_path: reference.field_path.to_owned(),
                serialized_file_id: reference.file_id,
                serialized_path_id: reference.path_id,
                expected_class_id: None,
                resolution: resolution.clone(),
            },
        )),
    }
}

fn map_entries(value: &SerializedValue) -> Result<&[SerializedValue], String> {
    match value {
        SerializedValue::Array(entries) => Ok(entries),
        _ => Err(format!(
            "SpriteAtlas.m_RenderDataMap has value {value:?}, expected an array"
        )),
    }
}

fn find_render_data_entry<'a>(
    entries: &'a [SerializedValue],
    wanted_key: &SerializedValue,
) -> Result<(usize, &'a SerializedValue, String), String> {
    let mut found = None;
    for (index, entry) in entries.iter().enumerate() {
        let (key, data, data_name) = map_pair(entry)?;
        if key == wanted_key {
            if found.is_some() {
                return Err(format!(
                    "SpriteAtlas.m_RenderDataMap has multiple entries for Sprite.m_RenderDataKey {wanted_key:?}"
                ));
            }
            found = Some((index, data, data_name));
        }
    }
    found.ok_or_else(|| {
        "SpriteAtlas.m_RenderDataMap has no entry for Sprite.m_RenderDataKey".to_owned()
    })
}

fn map_pair(
    value: &SerializedValue,
) -> Result<(&SerializedValue, &SerializedValue, String), String> {
    let SerializedValue::Struct(fields) = value else {
        return Err(format!(
            "SpriteAtlas render-data entry has value {value:?}, expected a pair struct"
        ));
    };
    validate_unique_field_names(fields, "SpriteAtlas.m_RenderDataMap pair")?;
    let key = one_pair_member(fields, &["first", "key", "Key"])?;
    let data = one_pair_member(fields, &["second", "value", "Value"])?;
    if key.0 == data.0 {
        return Err(
            "SpriteAtlas render-data pair uses one field for both key and value".to_owned(),
        );
    }
    Ok((&key.1.value, &data.1.value, data.1.name.clone()))
}

fn one_pair_member<'a>(
    fields: &'a [SerializedFieldValue],
    names: &[&str],
) -> Result<(usize, &'a SerializedFieldValue), String> {
    let mut matches = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| names.contains(&field.name.as_str()));
    let Some(first) = matches.next() else {
        return Err(format!(
            "SpriteAtlas render-data pair has no member named one of {names:?}"
        ));
    };
    if matches.next().is_some() {
        return Err(format!(
            "SpriteAtlas render-data pair has multiple members named one of {names:?}"
        ));
    }
    Ok(first)
}

fn field_path_in_base_chain(value: &SerializedValue, name: &str) -> Result<Option<String>, String> {
    let mut scope = value;
    let mut base_depth = 0usize;
    loop {
        if scope
            .unique_field(name)
            .map_err(|error| error.to_string())?
            .is_some()
        {
            let mut path = String::new();
            for _ in 0..base_depth {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str("Base");
            }
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(name);
            return Ok(Some(path));
        }
        let Some(base) = scope
            .unique_field("Base")
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        if !matches!(base, SerializedValue::Struct(_)) {
            return Ok(None);
        }
        scope = base;
        base_depth = base_depth
            .checked_add(1)
            .ok_or_else(|| "Sprite serialized Base path depth overflows".to_owned())?;
    }
}

fn validate_unique_field_names(fields: &[SerializedFieldValue], path: &str) -> Result<(), String> {
    for (index, field) in fields.iter().enumerate() {
        if fields[index + 1..]
            .iter()
            .any(|other| other.name == field.name)
        {
            return Err(format!(
                "duplicate field {:?} in Sprite atlas data at {path:?}",
                field.name
            ));
        }
    }
    Ok(())
}

fn join_field_path(prefix: &str, field: &str) -> String {
    if prefix.is_empty() {
        field.to_owned()
    } else if field.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}.{field}")
    }
}

fn join_array_path(prefix: &str, index: usize) -> String {
    format!("{prefix}[{index}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(first: SerializedValue, second: SerializedValue) -> SerializedValue {
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "first".to_owned(),
                value: first,
            },
            SerializedFieldValue {
                name: "second".to_owned(),
                value: second,
            },
        ])
    }

    #[test]
    fn sprite_atlas_render_data_map_requires_one_exact_key_match() {
        let wanted = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "first".to_owned(),
                value: SerializedValue::U64(0x1234),
            },
            SerializedFieldValue {
                name: "second".to_owned(),
                value: SerializedValue::I64(91),
            },
        ]);
        let other = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "first".to_owned(),
                value: SerializedValue::U64(0x1234),
            },
            SerializedFieldValue {
                name: "second".to_owned(),
                value: SerializedValue::I64(90),
            },
        ]);
        let entries = SerializedValue::Array(vec![
            pair(other, SerializedValue::I32(1)),
            pair(wanted.clone(), SerializedValue::I32(2)),
        ]);
        let values = map_entries(&entries).unwrap();
        let (index, data, data_name) = find_render_data_entry(values, &wanted).unwrap();
        assert_eq!(index, 1);
        assert_eq!(data, &SerializedValue::I32(2));
        assert_eq!(data_name, "second");

        let duplicate = SerializedValue::Array(vec![
            pair(wanted.clone(), SerializedValue::I32(2)),
            pair(wanted, SerializedValue::I32(3)),
        ]);
        assert!(find_render_data_entry(map_entries(&duplicate).unwrap(), &render_key()).is_err());
        assert!(find_render_data_entry(values, &SerializedValue::I64(0)).is_err());
    }

    fn render_key() -> SerializedValue {
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "first".to_owned(),
                value: SerializedValue::U64(0x1234),
            },
            SerializedFieldValue {
                name: "second".to_owned(),
                value: SerializedValue::I64(91),
            },
        ])
    }

    #[test]
    fn copied_atlas_pointer_is_resolved_in_atlas_scope_then_rebased_for_sprite_yaml() {
        let source =
            SerializedValue::Array(vec![SerializedValue::Struct(vec![SerializedFieldValue {
                name: "texture".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 45,
                },
            }])]);
        let mut target = source.clone();
        let target_identity = ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 7 },
            object_index: 4,
            path_id: 45,
            class_id: 28,
        };
        let mut overlays = Vec::new();
        let mut resolver = |path: &str, file_id: i32, path_id: i64| {
            assert_eq!(
                path,
                "m_RenderDataMap[3].second.secondaryTextures[0].texture"
            );
            assert_eq!((file_id, path_id), (2, 45));
            Ok(ProjectPointerRequest::ResolvedAsset(target_identity))
        };
        normalize_atlas_pointers_with_resolver(
            &source,
            &mut target,
            "m_RenderDataMap[3].second.secondaryTextures",
            "m_RD.secondaryTextures",
            &mut resolver,
            &mut overlays,
        )
        .unwrap();

        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].field_path, "m_RD.secondaryTextures[0].texture");
        assert_eq!(overlays[0].path_id, 45);
        assert_eq!(
            overlays[0].target,
            ProjectPointerRequest::ResolvedAsset(target_identity)
        );
        let SerializedValue::Array(entries) = target else {
            panic!("copied secondary textures is not an array");
        };
        assert_eq!(
            entries[0].unique_field("texture").unwrap().unwrap(),
            &SerializedValue::PPtr {
                file_id: 0,
                path_id: 45
            }
        );
    }

    #[test]
    fn copied_null_atlas_pointer_is_overlaid_instead_of_using_old_sprite_edge() {
        let source = SerializedValue::PPtr {
            file_id: 4,
            path_id: 0,
        };
        let mut target = source.clone();
        let mut overlays = Vec::new();
        let mut resolver = |_: &str, _: i32, _: i64| -> Result<ProjectPointerRequest, String> {
            panic!("null PPtr must not be resolved against an atlas graph edge")
        };
        normalize_atlas_pointers_with_resolver(
            &source,
            &mut target,
            "m_RenderDataMap[0].second.texture",
            "m_RD.texture",
            &mut resolver,
            &mut overlays,
        )
        .unwrap();
        assert_eq!(
            target,
            SerializedValue::PPtr {
                file_id: 0,
                path_id: 0
            }
        );
        assert_eq!(overlays.len(), 1);
        assert_eq!(overlays[0].field_path, "m_RD.texture");
        assert_eq!(overlays[0].target, ProjectPointerRequest::Null);
    }

    #[test]
    fn atlas_crop_offset_is_materialized_for_geometry_recalculation() {
        let vec2 = |x, y| {
            SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(x),
                },
                SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(y),
                },
            ])
        };
        let rect = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "x".to_owned(),
                value: SerializedValue::F32(1.0),
            },
            SerializedFieldValue {
                name: "y".to_owned(),
                value: SerializedValue::F32(2.0),
            },
            SerializedFieldValue {
                name: "width".to_owned(),
                value: SerializedValue::F32(10.0),
            },
            SerializedFieldValue {
                name: "height".to_owned(),
                value: SerializedValue::F32(12.0),
            },
        ]);
        let atlas_offset = vec2(3.0, 4.0);
        let atlas_data = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "texture".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
            },
            SerializedFieldValue {
                name: "textureRect".to_owned(),
                value: rect.clone(),
            },
            SerializedFieldValue {
                name: "textureRectOffset".to_owned(),
                value: atlas_offset.clone(),
            },
            SerializedFieldValue {
                name: "settingsRaw".to_owned(),
                value: SerializedValue::U32(7),
            },
            SerializedFieldValue {
                name: "downscaleMultiplier".to_owned(),
                value: SerializedValue::F32(0.5),
            },
        ]);
        let mut sprite_render_data = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "texture".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
            },
            SerializedFieldValue {
                name: "textureRect".to_owned(),
                value: rect,
            },
            SerializedFieldValue {
                name: "textureRectOffset".to_owned(),
                value: vec2(0.0, 0.0),
            },
            SerializedFieldValue {
                name: "settingsRaw".to_owned(),
                value: SerializedValue::U32(0),
            },
            SerializedFieldValue {
                name: "downscaleMultiplier".to_owned(),
                value: SerializedValue::F32(1.0),
            },
        ]);
        let graph = MultiFileReferenceGraph::build(&[]).unwrap();
        let mut pointers = Vec::new();
        copy_atlas_render_data_fields(
            &atlas_data,
            "m_RenderDataMap[0].second",
            &mut sprite_render_data,
            "m_RD",
            0,
            0,
            &graph,
            &mut pointers,
        )
        .unwrap();
        assert_eq!(
            sprite_render_data
                .unique_field("textureRectOffset")
                .unwrap()
                .unwrap(),
            &atlas_offset
        );
        assert_eq!(pointers.len(), 1);
        assert_eq!(pointers[0].field_path, "m_RD.texture");
        assert_eq!(pointers[0].target, ProjectPointerRequest::Null);
    }
}
