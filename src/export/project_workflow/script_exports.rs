use super::*;

pub(super) fn plan_script_export_details(
    object: &WorkspaceObject,
    settings: &ExportSettingsFile,
    assembly_manager_is_set: bool,
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
    unity_version_text: &str,
    reference_assembly_maps: Option<&ReferenceAssemblyMaps>,
) -> Result<Option<ScriptExportDetails>, Box<dyn Error>> {
    let metadata = mono_script_export_identity(object).map_err(|error| {
        invalid_project(format!(
            "file {} object {} PathID {} MonoScript export fields are invalid: {error}",
            object.file_index, object.object_index, object.path_id
        ))
    })?;
    let identity = MonoScriptIdentity {
        class: metadata.class,
        namespace: metadata.namespace,
        assembly: metadata.assembly,
    };
    if identity.is_injected() {
        return Ok(None);
    }
    let reference_guid = match reference_assembly_maps {
        Some(maps) => {
            let version = FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
                invalid_project(format!(
                    "file {} object {} PathID {} MonoScript reference lookup cannot parse Unity version {unity_version_text:?}",
                    object.file_index, object.object_index, object.path_id
                ))
            })?;
            maps.guid_for(&version, &identity.assembly)
                .map_err(|error| {
                    invalid_project(format!(
                        "file {} object {} PathID {} MonoScript reference lookup failed: {error}",
                        object.file_index, object.object_index, object.path_id
                    ))
                })?
        }
        None if assembly_manager_is_set => {
            return Err(invalid_project(format!(
                "file {} object {} PathID {} MonoScript reference classification requires the manager-backed reference map, which is unavailable for this mscorlib configuration",
                object.file_index, object.object_index, object.path_id
            )))
        }
        None => None,
    };
    let export_type = assembly_export_type(
        reference_guid.is_some(),
        assembly_manager_is_set,
        settings.script_export_mode,
        &identity.assembly,
    );
    let mut missing_assembly = false;
    let mut source_script_present = None;
    if export_type == AssemblyExportType::Save {
        let catalog = managed_assemblies.ok_or_else(|| {
            invalid_project(format!(
                "file {} object {} PathID {} MonoScript selects Save for ScriptExportMode::{:?}, but the imported assembly catalog was not supplied to project export",
                object.file_index,
                object.object_index,
                object.path_id,
                settings.script_export_mode
            ))
        })?;
        match catalog.assembly(&identity.assembly) {
            Some(assembly) if assembly.source_virtual_path.is_none() => {
                return Err(invalid_project(format!(
                    "file {} object {} PathID {} MonoScript assembly {:?} has no source VFS stream to save",
                    object.file_index, object.object_index, object.path_id, identity.assembly
                )))
            }
            Some(_) => {}
            None => missing_assembly = true,
        }
    } else if export_type == AssemblyExportType::Decompile && assembly_manager_is_set {
        let catalog = managed_assemblies.ok_or_else(|| {
            invalid_project(format!(
                "file {} object {} PathID {} MonoScript type presence requires the imported assembly catalog",
                object.file_index, object.object_index, object.path_id
            ))
        })?;
        missing_assembly = !catalog.contains_assembly(&identity.assembly);
        source_script_present = Some(catalog.is_present(&ScriptIdentifier {
            assembly: identity.assembly.clone(),
            namespace: identity.namespace.clone(),
            name: Some(identity.class.clone()),
        }));
    } else if export_type != AssemblyExportType::Decompile
        && !(export_type == AssemblyExportType::Skip && reference_guid.is_some())
    {
        return Err(invalid_project(format!(
            "file {} object {} PathID {} MonoScript selects {export_type:?} for ScriptExportMode::{:?}; that script export route is not wired into the project path",
            object.file_index,
            object.object_index,
            object.path_id,
            settings.script_export_mode
        )));
    }
    let script_guid = calculate_script_guid_str(
        &identity.assembly,
        &identity.namespace,
        &identity.class,
        ScriptHashLimits::default(),
    )
    .map_err(|error| {
        invalid_project(format!(
            "file {} object {} PathID {} MonoScript GUID could not be calculated: {error}",
            object.file_index, object.object_index, object.path_id
        ))
    })?;
    let (pointer_guid, pointer_file_id) = match export_type {
        AssemblyExportType::Decompile => (
            script_guid.to_byte_array(),
            crate::export::script_export_flow::decompiled_file_id().map_err(|error| {
                invalid_project(format!(
                    "MonoScript export ID could not be planned: {error}"
                ))
            })?,
        ),
        AssemblyExportType::Skip => {
            let file_id = calculate_script_file_id_str(
                &identity.namespace,
                &identity.class,
                ScriptHashLimits::default(),
            )
            .map_err(|error| {
                invalid_project(format!(
                    "file {} object {} PathID {} MonoScript file ID could not be calculated: {error}",
                    object.file_index, object.object_index, object.path_id
                ))
            })?;
            (
                reference_guid
                    .ok_or_else(|| invalid_project("skipped MonoScript has no reference GUID"))?
                    .to_byte_array(),
                i64::from(file_id),
            )
        }
        AssemblyExportType::Save => {
            let file_id = calculate_script_file_id_str(
                &identity.namespace,
                &identity.class,
                ScriptHashLimits::default(),
            )
            .map_err(|error| {
                invalid_project(format!(
                    "file {} object {} PathID {} MonoScript file ID could not be calculated: {error}",
                    object.file_index, object.object_index, object.path_id
                ))
            })?;
            (
                calculate_assembly_guid(&identity.assembly).to_byte_array(),
                i64::from(file_id),
            )
        }
    };
    Ok(Some(ScriptExportDetails {
        identity,
        execution_order: metadata.execution_order,
        guid: script_guid.to_byte_array(),
        pointer_guid,
        pointer_file_id,
        export_type,
        missing_assembly,
        source_script_present,
    }))
}

pub(super) fn source_reference_assembly_maps(
    has_mono_scripts: bool,
    managed_assemblies: Option<&ManagedAssemblyCatalog>,
) -> Result<Option<ReferenceAssemblyMaps>, Box<dyn Error>> {
    if !has_mono_scripts {
        return Ok(None);
    }

    let has_mscorlib2 = match managed_assemblies.and_then(|catalog| catalog.assembly("mscorlib")) {
        Some(assembly) => match assembly.version_major {
            Some(major) => major == 2,
            None => {
                return Err(invalid_project(
                    "Mono reference lookup found mscorlib without a manifest version",
                ))
            }
        },
        // MonoManager falls back to the .NET runtime's version 4 mscorlib when the
        // imported platform does not contain one. An absent manager also selects false.
        None => false,
    };

    ReferenceAssemblyMaps::from_source_fixture_with_mscorlib2(has_mscorlib2)
        .map(Some)
        .map_err(|error| {
            invalid_project(format!(
                "source reference assembly map could not be loaded: {error}"
            ))
        })
}

pub(super) fn plan_saved_assembly_exports(
    catalog: &ManagedAssemblyCatalog,
    reference_assembly_maps: Option<&ReferenceAssemblyMaps>,
    unity_version_text: &str,
    settings: &ExportSettingsFile,
) -> Result<Vec<SavedAssemblyExport>, Box<dyn Error>> {
    let version = FactoryUnityVersion::parse(unity_version_text).ok_or_else(|| {
        invalid_project(format!(
            "saved assembly export cannot parse Unity version {unity_version_text:?}"
        ))
    })?;
    let reference_assembly_maps = reference_assembly_maps.ok_or_else(|| {
        invalid_project(
            "saved assembly export requires a source reference map for this mscorlib configuration",
        )
    })?;
    let mut exports = plan_manager_assembly_exports(catalog)?;
    let mut filtered = Vec::new();
    filtered
        .try_reserve_exact(exports.len())
        .map_err(|_| invalid_project("could not allocate selected assembly export plans"))?;
    for export in exports.drain(..) {
        let is_reference = reference_assembly_maps
            .guid_for(&version, &export.assembly_name)?
            .is_some();
        if assembly_export_type(
            is_reference,
            true,
            settings.script_export_mode,
            &export.assembly_name,
        ) == AssemblyExportType::Save
        {
            filtered.push(export);
        }
    }
    Ok(filtered)
}

/// Every assembly retained by the imported manager, independent of script mode
/// or whether an individual MonoScript references it. Forwarders belong to the
/// manager output as well as to the optional Plugins export.
pub(super) fn plan_manager_assembly_exports(
    catalog: &ManagedAssemblyCatalog,
) -> Result<Vec<SavedAssemblyExport>, Box<dyn Error>> {
    let mut exports = Vec::new();
    let capacity = catalog
        .assemblies()
        .len()
        .checked_add(2)
        .ok_or_else(|| invalid_project("manager assembly export count overflows"))?;
    exports
        .try_reserve_exact(capacity)
        .map_err(|_| invalid_project("could not allocate saved assembly export plans"))?;
    for assembly in catalog.assemblies() {
        let source_virtual_path = assembly.source_virtual_path.clone().ok_or_else(|| {
            invalid_project(format!(
                "saved assembly {:?} has no source VFS stream",
                assembly.assembly_name
            ))
        })?;
        let file_name = add_assembly_file_extension(&assembly.assembly_name);
        exports.push(SavedAssemblyExport {
            assembly_name: assembly.assembly_name.clone(),
            source_virtual_path,
            relative_path: format!("Assets/Plugins/{file_name}"),
            guid: calculate_assembly_guid(&file_name).to_byte_array(),
            generated_forwarder: None,
        });
    }

    if let Some(mscorlib) = catalog.assembly("mscorlib") {
        let mscorlib_path = mscorlib.source_virtual_path.clone().ok_or_else(|| {
            invalid_project("loaded mscorlib has no source VFS stream for forwarding modules")
        })?;
        for helper_name in [
            "System.Runtime.CompilerServices.Unsafe",
            "System.Runtime.InteropServices",
        ] {
            if catalog.assembly(helper_name).is_some() {
                continue;
            }
            let file_name = add_assembly_file_extension(helper_name);
            exports.push(SavedAssemblyExport {
                assembly_name: helper_name.to_owned(),
                source_virtual_path: mscorlib_path.clone(),
                relative_path: format!("Assets/Plugins/{file_name}"),
                guid: calculate_assembly_guid(&file_name).to_byte_array(),
                generated_forwarder: Some(helper_name.to_owned()),
            });
        }
    }
    Ok(exports)
}

pub(super) fn materialize_assembly_export(
    filesystem: &VirtualFileSystem,
    assembly: &SavedAssemblyExport,
    maximum_bytes: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let input_bytes = filesystem
        .read_all_bytes(
            &assembly.source_virtual_path,
            maximum_bytes.min(ManagedAssemblyLimits::default().max_file_bytes),
        )
        .map_err(|error| {
            invalid_project(format!(
                "managed assembly {:?} could not be read from {}: {error}",
                assembly.assembly_name, assembly.source_virtual_path,
            ))
        })?;
    let file_name = add_assembly_file_extension(&assembly.assembly_name);
    let limits = ManagedAssemblyLimits {
        max_file_bytes: maximum_bytes.min(ManagedAssemblyLimits::default().max_file_bytes),
        ..ManagedAssemblyLimits::default()
    };
    let serialized = match assembly.generated_forwarder.as_deref() {
        Some("System.Runtime.CompilerServices.Unsafe") => ManagedAssembly::create_unsafe_forwarder(
            &assembly.source_virtual_path,
            &input_bytes,
            limits,
        ),
        Some("System.Runtime.InteropServices") => {
            ManagedAssembly::create_interop_services_forwarder(
                &assembly.source_virtual_path,
                &input_bytes,
                limits,
            )
        }
        Some(name) => {
            return Err(invalid_project(format!(
                "unknown synthesized forwarding assembly {name:?}"
            )))
        }
        None => ManagedAssembly::serialize_for_export(&file_name, &input_bytes, limits),
    };
    serialized.map_err(|error| {
        invalid_project(format!(
            "managed assembly {:?} could not be exported from {}: {error}",
            assembly.assembly_name, assembly.source_virtual_path,
        ))
    })
}
