use super::*;

pub(super) fn prepend_lighting_data_object_fields(
    root: &mut SerializedValue,
    version: &FactoryUnityVersion,
) -> Result<(), Box<dyn Error>> {
    if !matches!(root, SerializedValue::Struct(_)) {
        return Err(invalid_project(
            "LightingDataAsset reference TypeTree root is not a struct",
        ));
    }
    let uses_corresponding_source_object = (version.major, version.minor) >= (2018, 2);
    let uses_prefab_asset_fields = (version.major, version.minor) >= (2018, 3);
    let mut base_names = vec!["m_ObjectHideFlags"];
    base_names.push(if uses_corresponding_source_object {
        "m_CorrespondingSourceObject"
    } else {
        "m_PrefabParentObject"
    });
    if uses_prefab_asset_fields {
        base_names.extend(["m_PrefabInstance", "m_PrefabAsset"]);
    } else {
        base_names.push("m_PrefabInternal");
    }
    for name in base_names.iter().copied() {
        if unique_lighting_data_field_index(root, name)?.is_some() {
            return Err(invalid_project(format!(
                "LightingDataAsset reference TypeTree unexpectedly contains inherited field {name}"
            )));
        }
    }
    if unique_lighting_data_field_index(root, "m_Name")?.is_none() {
        return Err(invalid_project(
            "LightingDataAsset reference TypeTree has no m_Name field",
        ));
    }
    let SerializedValue::Struct(fields) = root else {
        unreachable!("LightingDataAsset root shape was validated")
    };
    let mut combined = Vec::new();
    combined
        .try_reserve_exact(fields.len().saturating_add(base_names.len()))
        .map_err(|_| invalid_project("could not allocate LightingDataAsset base fields"))?;
    for name in base_names {
        let value = if name == "m_ObjectHideFlags" {
            SerializedValue::U32(0)
        } else {
            null_project_pointer()
        };
        combined.push(SerializedFieldValue {
            name: name.to_owned(),
            value,
        });
    }
    combined.append(fields);
    *root = SerializedValue::Struct(combined);
    Ok(())
}

pub(super) fn set_lighting_data_guid_field(
    root: &mut SerializedValue,
    name: &str,
    guid: [u8; 16],
) -> Result<(), Box<dyn Error>> {
    const WORD_NAMES: [[&str; 2]; 4] = [
        ["m_Data_0_", "data[0]"],
        ["m_Data_1_", "data[1]"],
        ["m_Data_2_", "data[2]"],
        ["m_Data_3_", "data[3]"],
    ];
    let index = unique_lighting_data_field_index(root, name)?.ok_or_else(|| {
        invalid_project(format!("LightingDataAsset TypeTree has no {name} field"))
    })?;
    let SerializedValue::Struct(root_fields) = root else {
        return Err(invalid_project("LightingDataAsset value is not a struct"));
    };
    let SerializedValue::Struct(guid_fields) = &mut root_fields[index].value else {
        return Err(invalid_project(format!(
            "LightingDataAsset {name} field is not a GUID struct"
        )));
    };
    if guid_fields.len() != WORD_NAMES.len() {
        return Err(invalid_project(format!(
            "LightingDataAsset {name} GUID has {} fields; expected {}",
            guid_fields.len(),
            WORD_NAMES.len()
        )));
    }
    for (word_index, expected_names) in WORD_NAMES.iter().enumerate() {
        let mut matches = guid_fields
            .iter_mut()
            .filter(|field| expected_names.contains(&field.name.as_str()));
        let field = matches.next().ok_or_else(|| {
            invalid_project(format!(
                "LightingDataAsset {name} GUID lacks data word {word_index}"
            ))
        })?;
        if matches.next().is_some() {
            return Err(invalid_project(format!(
                "LightingDataAsset {name} GUID duplicates data word {word_index}"
            )));
        }
        let start = word_index * 4;
        field.value = SerializedValue::U32(u32::from_le_bytes(
            guid[start..start + 4]
                .try_into()
                .expect("a GUID word has four bytes"),
        ));
    }
    Ok(())
}

fn null_project_pointer() -> SerializedValue {
    SerializedValue::PPtr {
        file_id: 0,
        path_id: 0,
    }
}

pub(super) fn unique_lighting_data_field_index(
    root: &SerializedValue,
    name: &str,
) -> Result<Option<usize>, Box<dyn Error>> {
    let SerializedValue::Struct(fields) = root else {
        return Err(invalid_project("LightingDataAsset value is not a struct"));
    };
    let mut matches = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.name == name);
    let first = matches.next().map(|(index, _)| index);
    if matches.next().is_some() {
        return Err(invalid_project(format!(
            "LightingDataAsset has duplicate {name} fields"
        )));
    }
    Ok(first)
}

pub(super) fn set_required_lighting_data_field(
    root: &mut SerializedValue,
    name: &str,
    value: SerializedValue,
) -> Result<(), Box<dyn Error>> {
    let index = unique_lighting_data_field_index(root, name)?.ok_or_else(|| {
        invalid_project(format!("LightingDataAsset TypeTree has no {name} field"))
    })?;
    let SerializedValue::Struct(fields) = root else {
        return Err(invalid_project("LightingDataAsset value is not a struct"));
    };
    fields[index].value = value;
    Ok(())
}

pub(super) fn copy_required_lighting_data_field(
    source: &SerializedValue,
    destination: &mut SerializedValue,
    source_name: &str,
    destination_name: &str,
) -> Result<(), Box<dyn Error>> {
    let value = unique_lighting_data_field_index(source, source_name)?
        .map(|index| {
            source.struct_fields().expect("validated struct")[index]
                .value
                .clone()
        })
        .ok_or_else(|| {
            invalid_project(format!(
                "source LightmapSettings has no required {source_name} field"
            ))
        })?;
    ensure_lighting_data_value_shape(destination, destination_name, &value)?;
    set_required_lighting_data_field(destination, destination_name, value)
}

pub(super) fn copy_optional_lighting_data_field(
    source: &SerializedValue,
    destination: &mut SerializedValue,
    source_name: &str,
    destination_name: &str,
) -> Result<(), Box<dyn Error>> {
    let Some(source_index) = unique_lighting_data_field_index(source, source_name)? else {
        return Ok(());
    };
    let value = source.struct_fields().expect("validated struct")[source_index]
        .value
        .clone();
    let Some(destination_index) = unique_lighting_data_field_index(destination, destination_name)?
    else {
        if is_default_lighting_data_value(&value) {
            return Ok(());
        }
        return Err(invalid_project(format!(
            "source LightmapSettings {source_name} is non-default, but this LightingDataAsset version has no {destination_name} field"
        )));
    };
    ensure_lighting_data_value_shape(destination, destination_name, &value)?;
    let SerializedValue::Struct(fields) = destination else {
        return Err(invalid_project("LightingDataAsset value is not a struct"));
    };
    fields[destination_index].value = value;
    Ok(())
}

fn is_default_lighting_data_value(value: &SerializedValue) -> bool {
    match value {
        SerializedValue::Bool(value) => !value,
        SerializedValue::I8(value) => *value == 0,
        SerializedValue::U8(value) => *value == 0,
        SerializedValue::I16(value) => *value == 0,
        SerializedValue::U16(value) => *value == 0,
        SerializedValue::Char(value) => *value == 0,
        SerializedValue::I32(value) => *value == 0,
        SerializedValue::U32(value) => *value == 0,
        SerializedValue::I64(value) => *value == 0,
        SerializedValue::U64(value) => *value == 0,
        SerializedValue::F32(value) => *value == 0.0,
        SerializedValue::F64(value) => *value == 0.0,
        SerializedValue::StringBytes(value) | SerializedValue::Bytes(value) => value.is_empty(),
        SerializedValue::PPtr { file_id, path_id } => *file_id == 0 && *path_id == 0,
        SerializedValue::Array(values) => values.is_empty(),
        SerializedValue::Struct(fields) => fields
            .iter()
            .all(|field| is_default_lighting_data_value(&field.value)),
        SerializedValue::ManagedReferenceId { .. }
        | SerializedValue::ManagedReferenceRegistry(_)
        | SerializedValue::Raw { .. } => false,
    }
}

pub(super) fn copy_render_settings_probe(
    render_settings: &SerializedValue,
    destination: &mut SerializedValue,
    source_name: &str,
    singular_name: &str,
    plural_name: &str,
) -> Result<(), Box<dyn Error>> {
    let Some(source_index) = unique_lighting_data_field_index(render_settings, source_name)? else {
        return Ok(());
    };
    let value = render_settings.struct_fields().expect("validated struct")[source_index]
        .value
        .clone();
    if unique_lighting_data_field_index(destination, singular_name)?.is_some() {
        ensure_lighting_data_value_shape(destination, singular_name, &value)?;
        set_required_lighting_data_field(destination, singular_name, value)
    } else if let Some(index) = unique_lighting_data_field_index(destination, plural_name)? {
        let SerializedValue::Struct(fields) = destination else {
            return Err(invalid_project("LightingDataAsset value is not a struct"));
        };
        let SerializedValue::Array(values) = &mut fields[index].value else {
            return Err(invalid_project(format!(
                "LightingDataAsset {plural_name} field is not an array"
            )));
        };
        values.try_reserve(1).map_err(|_| {
            invalid_project(format!(
                "could not allocate LightingDataAsset {plural_name}"
            ))
        })?;
        values.push(value);
        Ok(())
    } else {
        Err(invalid_project(format!(
            "RenderSettings contains {source_name}, but the LightingDataAsset TypeTree has no compatible baked-probe field"
        )))
    }
}

fn ensure_lighting_data_value_shape(
    root: &SerializedValue,
    name: &str,
    value: &SerializedValue,
) -> Result<(), Box<dyn Error>> {
    let index = unique_lighting_data_field_index(root, name)?.ok_or_else(|| {
        invalid_project(format!("LightingDataAsset TypeTree has no {name} field"))
    })?;
    let current = &root.struct_fields().expect("validated struct")[index].value;
    let compatible = matches!(
        (current, value),
        (SerializedValue::Bool(_), SerializedValue::Bool(_))
            | (SerializedValue::I8(_), SerializedValue::I8(_))
            | (SerializedValue::U8(_), SerializedValue::U8(_))
            | (SerializedValue::I16(_), SerializedValue::I16(_))
            | (SerializedValue::U16(_), SerializedValue::U16(_))
            | (SerializedValue::Char(_), SerializedValue::Char(_))
            | (SerializedValue::I32(_), SerializedValue::I32(_))
            | (SerializedValue::U32(_), SerializedValue::U32(_))
            | (SerializedValue::I64(_), SerializedValue::I64(_))
            | (SerializedValue::U64(_), SerializedValue::U64(_))
            | (SerializedValue::F32(_), SerializedValue::F32(_))
            | (SerializedValue::F64(_), SerializedValue::F64(_))
            | (
                SerializedValue::StringBytes(_),
                SerializedValue::StringBytes(_)
            )
            | (SerializedValue::Bytes(_), SerializedValue::Bytes(_))
            | (SerializedValue::Array(_), SerializedValue::Array(_))
            | (SerializedValue::Struct(_), SerializedValue::Struct(_))
            | (SerializedValue::PPtr { .. }, SerializedValue::PPtr { .. })
    );
    if !compatible {
        return Err(invalid_project(format!(
            "source value for LightingDataAsset {name} has a different serialized shape"
        )));
    }
    Ok(())
}

pub(super) fn lighting_data_bytes_field_value(
    root: &SerializedValue,
    name: &str,
    bytes: Vec<u8>,
) -> Result<SerializedValue, Box<dyn Error>> {
    let index = unique_lighting_data_field_index(root, name)?.ok_or_else(|| {
        invalid_project(format!("LightingDataAsset TypeTree has no {name} field"))
    })?;
    match &root.struct_fields().expect("validated struct")[index].value {
        SerializedValue::Bytes(_) => Ok(SerializedValue::Bytes(bytes)),
        SerializedValue::Array(_) => Ok(SerializedValue::Array(
            bytes.into_iter().map(SerializedValue::U8).collect(),
        )),
        _ => Err(invalid_project(format!(
            "LightingDataAsset {name} field is neither bytes nor a byte array"
        ))),
    }
}

pub(super) fn contains_non_null_pointer(value: &SerializedValue) -> bool {
    match value {
        SerializedValue::PPtr { path_id, .. } => *path_id != 0,
        SerializedValue::Array(values) => values.iter().any(contains_non_null_pointer),
        SerializedValue::Struct(fields) => fields
            .iter()
            .any(|field| contains_non_null_pointer(&field.value)),
        _ => false,
    }
}

pub(super) fn create_empty_lighting_enlighten_data(
    version: &FactoryUnityVersion,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let bundle_version: i32 = if (version.major, version.minor) >= (2022, 2) {
        8
    } else if version.major >= 2020 {
        7
    } else if version.major > 5
        || (version.major == 5
            && (version.minor > 2
                || (version.minor == 2
                    && (version.build > 0
                        || matches!(
                            version.version_type,
                            FactoryVersionType::Release
                                | FactoryVersionType::Patch
                                | FactoryVersionType::China
                        )))))
    {
        6
    } else {
        3
    };
    let version_text = version.text.as_bytes();
    let mut bytes = Vec::new();
    let header_size = 8usize
        .checked_add(4)
        .and_then(|size| size.checked_add(b"5.x.x\0".len()))
        .and_then(|size| size.checked_add(version_text.len().checked_add(1)?))
        .and_then(|size| size.checked_add(8 + 4 + 4 + 4))
        .ok_or_else(|| invalid_project("empty EnlightenData header length overflows"))?;
    let padding = if bundle_version >= 7 {
        (16 - header_size % 16) % 16
    } else {
        0
    };
    bytes
        .try_reserve_exact(header_size + padding + 20)
        .map_err(|_| invalid_project("could not allocate empty EnlightenData bundle"))?;
    bytes.extend_from_slice(b"UnityFS\0");
    bytes.extend_from_slice(&bundle_version.to_be_bytes());
    bytes.extend_from_slice(b"5.x.x\0");
    bytes.extend_from_slice(version_text);
    bytes.push(0);
    bytes.extend_from_slice(&0i64.to_be_bytes());
    bytes.extend_from_slice(&0i32.to_be_bytes());
    bytes.extend_from_slice(&0i32.to_be_bytes());
    bytes.extend_from_slice(&0i32.to_be_bytes());
    bytes.resize(bytes.len() + padding, 0);
    // Empty BlocksInfo: Hash128(16 zero bytes), followed by a zero StorageBlock count.
    bytes.extend_from_slice(&[0; 20]);
    Ok(bytes)
}

pub(super) fn generated_lighting_data_asset_to_yaml(
    asset: &RoutedLightingDataAsset,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    routed: &[RoutedObject<'_>],
    limits: WorkspaceProjectExportLimits,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let root_type = asset
        .type_tree
        .nodes
        .first()
        .ok_or_else(|| invalid_project("LightingDataAsset TypeTree has no root node"))?;
    let yaml_type_name = root_type
        .type_name
        .as_deref()
        .filter(|name| matches!(*name, "LightingDataAsset" | "LightmapSnapshot"))
        .ok_or_else(|| {
            invalid_project(format!(
                "class-1120 reference TypeTree root is {:?}, expected LightingDataAsset or LightmapSnapshot",
                root_type.type_name
            ))
        })?;
    let mut hints = UnityYamlObjectConversionHints::from_type_tree(
        &asset.type_tree,
        asset.source_version,
        UnityYamlObjectConversionLimits::default(),
    )?;
    // Generated class trees retain the source's serialized UnityEngine.Object field names.
    hints.preserve_type_tree_field_names = true;
    let expects_scene_pointer = unique_lighting_data_field_index(&asset.root, "m_Scene")?.is_some();
    let mut pointer_error = None;
    let mut scene_pointer_seen = false;
    let document = serialized_object_value_to_unity_yaml_document(
        asset.identity,
        yaml_type_name,
        root_type.version,
        112_000_000,
        false,
        &asset.root,
        &hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        asset.source_version,
        None,
        UnityYamlObjectConversionLimits::default(),
        |request| {
            resolve_generated_lighting_data_pointer(
                request,
                asset,
                reference_graph,
                pointer_map,
                routed,
                &mut scene_pointer_seen,
                &mut pointer_error,
            )
        },
    )?;
    if let Some(message) = pointer_error {
        return Err(invalid_project(format!(
            "LightingDataAsset {}:{} pointer resolution failed: {message}",
            asset.collection_index, asset.identity.path_id
        )));
    }
    if expects_scene_pointer && !scene_pointer_seen {
        return Err(invalid_project(
            "generated LightingDataAsset m_Scene pointer was not visited by the YAML converter",
        ));
    }
    Ok(write_unity_yaml_documents(&[document], limits.yaml_write)?.into_bytes())
}

fn resolve_generated_lighting_data_pointer(
    request: UnityYamlPPtrRequest<'_>,
    asset: &RoutedLightingDataAsset,
    reference_graph: &MultiFileReferenceGraph<'_>,
    pointer_map: &ProjectExportPointerMap,
    routed: &[RoutedObject<'_>],
    scene_pointer_seen: &mut bool,
    error: &mut Option<String>,
) -> ProjectPointerResolution {
    let fail = |error: &mut Option<String>, message: String| {
        if error.is_none() {
            *error = Some(message);
        }
        ProjectPointerResolution::Null
    };
    if request.source != asset.identity {
        return fail(
            error,
            format!(
                "PPtr source identity {:?} does not match generated asset {:?}",
                request.source, asset.identity
            ),
        );
    }
    if request.field_path == "m_Scene" {
        if request.file_id != 0 || request.path_id != i64::MIN {
            return fail(
                error,
                format!(
                    "generated m_Scene sentinel is ({}, {}), expected (0, {})",
                    request.file_id,
                    request.path_id,
                    i64::MIN
                ),
            );
        }
        if *scene_pointer_seen {
            return fail(
                error,
                "generated m_Scene pointer was visited twice".to_owned(),
            );
        }
        *scene_pointer_seen = true;
        if asset.scene_guid == [0; 16] {
            return fail(error, "owning Scene has a zero GUID".to_owned());
        }
        return ProjectPointerResolution::Mapped(UnityMetaPointer {
            file_id: 102_900_000,
            guid: ExportPointerGuid::Collection(asset.scene_guid),
            asset_type: UnityProjectAssetType::Meta,
        });
    }

    let ExportAssetCollectionIdentity::SerializedFile { file_index } =
        asset.source_owner.collection
    else {
        return fail(
            error,
            "LightingDataAsset source owner is not in a SerializedFile".to_owned(),
        );
    };
    let mut matching_edges = reference_graph
        .references_from_object(file_index, asset.source_owner.object_index)
        .filter(|edge| {
            edge.field_path == request.field_path
                && edge.file_id == request.file_id
                && edge.path_id == request.path_id
        });
    let Some(edge) = matching_edges.next() else {
        return fail(
            error,
            format!(
                "copied PPtr at {:?} ({}, {}) has no matching source LightmapSettings edge",
                request.field_path, request.file_id, request.path_id
            ),
        );
    };
    if matching_edges.next().is_some() {
        return fail(
            error,
            format!(
                "copied PPtr at {:?} ({}, {}) matches multiple source LightmapSettings edges",
                request.field_path, request.file_id, request.path_id
            ),
        );
    }

    let mut target_asset = None;
    let pointer_request = match &edge.resolution {
        MultiFilePPtrResolution::Null => {
            return fail(
                error,
                format!(
                    "non-null copied PPtr at {:?} resolved to null in the source graph",
                    request.field_path
                ),
            )
        }
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => {
            let Some(identity) =
                reference_graph.object_identity(*target_file_index, *target_object_index)
            else {
                return fail(
                    error,
                    format!(
                        "copied PPtr at {:?} has no target identity",
                        request.field_path
                    ),
                );
            };
            let Some(class_id) =
                reference_graph.object_class_id(*target_file_index, *target_object_index)
            else {
                return fail(
                    error,
                    format!(
                        "copied PPtr at {:?} has no target class ID",
                        request.field_path
                    ),
                );
            };
            let identity = ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::SerializedFile {
                    file_index: *target_file_index,
                },
                object_index: *target_object_index,
                path_id: identity.path_id,
                class_id,
            };
            target_asset = Some(identity);
            ProjectPointerRequest::ResolvedAsset(identity)
        }
        resolution => ProjectPointerRequest::UnresolvedInput(UnresolvedProjectPointer {
            source_file_index: file_index,
            source_object_index: asset.source_owner.object_index,
            source_path_id: asset.source_owner.path_id,
            field_path: request.field_path.to_owned(),
            serialized_file_id: request.file_id,
            serialized_path_id: request.path_id,
            expected_class_id: None,
            resolution: resolution.clone(),
        }),
    };
    let resolution = match pointer_map.resolve(pointer_request, asset.collection_index) {
        Ok(resolution) => resolution,
        Err(source) => {
            return fail(
                error,
                format!("project pointer-map lookup failed: {source}"),
            )
        }
    };
    match resolution {
        ProjectPointerResolution::Mapped(mut pointer)
            if target_asset.is_some_and(|target_identity| {
                is_lighting_data_texture_field(request.field_path)
                    && routed.iter().any(|target| {
                        identity_of(target) == target_identity
                            && target.class_id == 28
                            && target.route == ObjectRoute::TextureImage
                            && target.original_directory.as_deref()
                                == Some(asset.original_directory.as_str())
                    })
            }) =>
        {
            pointer.asset_type = UnityProjectAssetType::Meta;
            ProjectPointerResolution::Mapped(pointer)
        }
        ProjectPointerResolution::Mapped(pointer) => ProjectPointerResolution::Mapped(pointer),
        ProjectPointerResolution::MissingExportedTarget { .. } => fail(
            error,
            format!(
                "copied PPtr at {:?} targets an object omitted from project export",
                request.field_path
            ),
        ),
        ProjectPointerResolution::UnresolvedInput(unresolved) => fail(
            error,
            format!(
                "copied PPtr at {:?} is unresolved: {:?}",
                request.field_path, unresolved.resolution
            ),
        ),
        ProjectPointerResolution::Null => fail(
            error,
            format!(
                "copied PPtr at {:?} unexpectedly mapped to null",
                request.field_path
            ),
        ),
    }
}

fn is_lighting_data_texture_field(field_path: &str) -> bool {
    field_path.starts_with("m_Lightmaps[")
        && matches!(
            field_path.rsplit('.').next(),
            Some("m_Lightmap" | "m_DirLightmap" | "m_ShadowMask")
        )
}
