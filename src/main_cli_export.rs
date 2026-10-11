use super::*;

pub(super) fn export_file_yaml_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let serialized_file_index =
        parse_required_argument::<usize>(arguments.next(), "SerializedFile index")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-file-yaml requires an output YAML path"))?;
    let mut input_files = Vec::new();
    for argument in arguments {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-file-yaml requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_file_yaml_to(
        &input_files,
        serialized_file_index,
        &output_path,
        &mut output,
    )
}

pub(super) fn export_object_yaml_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let serialized_file_index =
        parse_required_argument::<usize>(arguments.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(arguments.next(), "object PathID")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-object-yaml requires an output YAML path"))?;
    let mut input_files = Vec::new();
    for argument in arguments {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-object-yaml requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_object_yaml_to(
        &input_files,
        serialized_file_index,
        path_id,
        &output_path,
        &mut output,
    )
}

pub(super) fn export_texture_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    // The configured fallback defaults to `ExportSettings.ImageExportFormat` (`Png`),
    // with original-extension preference defaulting to true.
    let mut configured = ExportDefaults::default().image_export_format;
    let mut prefer_original = ExportDefaults::default().prefer_original_texture_extension;
    // Optional leading flags precede the positional arguments.
    let mut positional: Vec<OsString> = Vec::new();
    let mut explicit_settings = false;
    while let Some(next) = arguments.next() {
        if next == "--settings" {
            let path = arguments.next().ok_or_else(|| {
                invalid_input("export-texture --settings requires a settings file path")
            })?;
            let export = load_cli_export_settings(Path::new(&path))?;
            configured = export.image_export_format;
            prefer_original = export.prefer_original_texture_extension;
            explicit_settings = true;
        } else {
            positional.push(next);
        }
    }
    // An explicit flag beats the default-path file; the default-path file beats
    // built-in defaults, mirroring the startup `LoadFromDefaultPath` step.
    if !explicit_settings {
        if let Some(export) = try_load_default_export_settings()? {
            configured = export.image_export_format;
            prefer_original = export.prefer_original_texture_extension;
        }
    }
    let mut positional = positional.into_iter();
    let serialized_file_index =
        parse_required_argument::<usize>(positional.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(positional.next(), "object PathID")?;
    let output_path = positional
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-texture requires an output image path"))?;
    let mut input_files = Vec::new();
    for argument in positional {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-texture requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_texture_to(
        &input_files,
        serialized_file_index,
        path_id,
        configured,
        prefer_original,
        &output_path,
        &mut output,
    )
}

pub(super) fn export_texture_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    configured: SettingsImageFormat,
    prefer_original_texture_extension: bool,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let container_bundle_index = file.container_bundle_index;
    let mut matching_objects = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.path_id == path_id);
    let (object_index, _) = matching_objects.next().ok_or_else(|| {
        invalid_input(format!(
            "SerializedFile {serialized_file_index} has no object with PathID {path_id}"
        ))
    })?;
    if matching_objects.next().is_some() {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has multiple objects with PathID {path_id}"
        ))
        .into());
    }

    let objects = load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    let class_id = Some(object.factory.class_id);
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    enum TextureExportTarget {
        Texture2D { view: Texture2DView, flip_y: bool },
        Texture3D { view: Texture3DView },
        Texture2DArray { view: Texture2DArrayView },
        CubemapArray { view: CubemapArrayView },
    }
    let target = match class_id {
        Some(28) | Some(CUBEMAP_CLASS_ID) => {
            let view = Texture2DView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Texture2D: {error}"
                ))
            })?;
            // Cubemaps (class 89) share the `ITexture2D` fields but skip the
            // vertical flip (`texture is not ICubemap`).
            let flip_y = !matches!(class_id, Some(CUBEMAP_CLASS_ID));
            TextureExportTarget::Texture2D { view, flip_y }
        }
        Some(TEXTURE3D_CLASS_ID) => {
            let view = Texture3DView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Texture3D: {error}"
                ))
            })?;
            TextureExportTarget::Texture3D { view }
        }
        Some(TEXTURE2DARRAY_CLASS_ID) => {
            let view = Texture2DArrayView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Texture2DArray: {error}"
                ))
            })?;
            TextureExportTarget::Texture2DArray { view }
        }
        Some(CUBEMAPARRAY_CLASS_ID) => {
            let view = CubemapArrayView::from_value(&payload.root).map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable CubemapArray: {error}"
                ))
            })?;
            TextureExportTarget::CubemapArray { view }
        }
        _ => {
            return Err(invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-28 Texture2D, class-89 Cubemap, class-117 Texture3D, class-187 Texture2DArray, or class-188 CubemapArray",
                object.class_id
            ))
            .into());
        }
    };
    let (image_is_empty, stream_data) = match &target {
        TextureExportTarget::Texture2D { view, .. } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
        TextureExportTarget::Texture3D { view } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
        TextureExportTarget::Texture2DArray { view } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
        TextureExportTarget::CubemapArray { view } => {
            (view.image_data.is_empty(), view.stream_data.as_ref())
        }
    };
    let stream_bytes: Option<Vec<u8>> = match stream_data {
        Some(info) if image_is_empty && info.is_set() => {
            let provider =
                PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
            let resolution = resolve_and_read_streamed_resource_info(
                &filesystem,
                &workspace,
                container_bundle_index,
                info,
                &root_resources,
                &provider,
                StreamedResourceContentLimits::default(),
            )
            .map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' could not be read: {error}",
                    info.path
                ))
            })?;
            match resolution {
                StreamedResourceContentResolution::Resolved { bytes, .. } => Some(bytes),
                StreamedResourceContentResolution::Missing {
                    unavailable_data_paths,
                } => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' is missing; unavailable data paths: {unavailable_data_paths:?}",
                        info.path
                    ))
                    .into());
                }
                StreamedResourceContentResolution::Unset => None,
            }
        }
        _ => None,
    };

    // The output extension selects the format; otherwise `GetTextureExportFormat`
    // picks the original extension's format when preferred and recognized, else the
    // configured settings format. Bundle-decoded assets carry no original path, so the
    // best extension is `None` and the fallback reduces to the configured format.
    // With default settings (`Png`) this matches the previous hardcoded fallback.
    let fallback = SettingsImageFormat::texture_export_format(
        prefer_original_texture_extension,
        None,
        configured,
    );
    let format = ImageExportFormat::from_settings_format(
        output_path
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(SettingsImageFormat::from_extension)
            .unwrap_or(fallback),
    );
    // The collection platform and endianness drive the `GetImageData` transforms
    // (Xbox360 swap, Switch unswizzle); the file's own metadata prefix is the
    // honest available equivalent. Discriminant 0 matches no `BuildTarget` and
    // transforms nothing.
    // Cubemaps (class 89) share the `ITexture2D` fields but skip the vertical
    // flip (`texture is not ICubemap`); the flag travels with the target below.
    let platform = file
        .metadata
        .prefix
        .target_platform
        .and_then(|platform| i32::try_from(platform).ok())
        .unwrap_or(0);
    let platform_blob_little_endian =
        file.metadata.prefix.body_endian == assetripper::io::binary::Endian::Little;
    let image = match &target {
        TextureExportTarget::Texture2D { view, flip_y } => {
            let unity_version = file
                .metadata
                .prefix
                .unity_version_text
                .as_deref()
                .and_then(UnityVersion::parse);
            export_texture2d_image_with_unity_version(
                view,
                stream_bytes.as_deref(),
                platform,
                platform_blob_little_endian,
                *flip_y,
                unity_version,
                format,
            )
        }
        TextureExportTarget::Texture3D { view } => {
            export_texture3d_image(view, stream_bytes.as_deref(), format)
        }
        TextureExportTarget::Texture2DArray { view } => {
            export_texture2d_array_image(view, stream_bytes.as_deref(), format)
        }
        TextureExportTarget::CubemapArray { view } => {
            export_cubemap_array_image(view, stream_bytes.as_deref(), format)
        }
    }
    .map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} texture export failed: {error}"
        ))
    })?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&image)?;
    writeln!(
        status_output,
        "Wrote a {} texture from SerializedFile {serialized_file_index} PathID {path_id} to {:?}.",
        format.file_extension(),
        output_path
    )?;
    Ok(())
}

pub(super) fn export_mesh_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut arguments = arguments;
    let serialized_file_index =
        parse_required_argument::<usize>(arguments.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(arguments.next(), "object PathID")?;
    let output_path = arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-mesh requires an output .glb path"))?;
    let mut input_files = Vec::new();
    for argument in arguments {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-mesh requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_mesh_to(
        &input_files,
        serialized_file_index,
        path_id,
        &output_path,
        &mut output,
    )
}

pub(super) fn export_mesh_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let (object_index, _) = select_unique_object(&workspace, serialized_file_index, path_id)?;

    let objects = load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    if object.factory.class_id != 43 {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-43 Mesh",
            object.class_id
        ))
        .into());
    }
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    let view = MeshView::from_value(
        &payload.root,
        file_unity_version(&workspace, serialized_file_index)?,
        file_endian_type(&workspace, serialized_file_index),
    )
    .map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Mesh: {error}"
        ))
    })?;
    let mesh_data = view
        .read_mesh_data(PackedBitVectorLimits::default())
        .map_err(|error| {
            invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} PathID {path_id} mesh data could not be read: {error}"
            ))
        })?
        .ok_or_else(|| {
            invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} PathID {path_id} has no mesh data to export"
            ))
        })?;
    let name = mesh_export_name(&payload.root)
        .unwrap_or_else(|| format!("Mesh_{serialized_file_index}_{path_id}"));
    let exported = build_glb(&name, &mesh_data).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} GLB export failed: {error}"
        ))
    })?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&exported.bytes)?;
    for warning in &exported.warnings {
        writeln!(status_output, "warning: {warning}")?;
    }
    let vertex_count = mesh_data.vertices.len();
    let sub_mesh_count = exported.emitted_source_indices.len();
    let emitted_indices: usize = exported.emitted_source_indices.iter().map(Vec::len).sum();
    writeln!(
        status_output,
        "Wrote a GLB with {vertex_count} vertices, {sub_mesh_count} sub-meshes, and \
         {emitted_indices} emitted indices from SerializedFile {serialized_file_index} \
         PathID {path_id} to {output_path:?}."
    )?;
    Ok(())
}

/// Loads an `AssetRipper.Settings.json` file for a `--settings` CLI flag.
///
/// Only the export record is consumed today; the remaining records stay available for
/// future commands. Missing files, invalid UTF-8, oversized files, and schema violations
/// are explicit input errors carrying the path.
pub(super) fn load_cli_export_settings(path: &Path) -> Result<ExportSettingsFile, Box<dyn Error>> {
    read_export_settings_file(path)
}

/// Reads one `AssetRipper.Settings.json` file, shared by the explicit `--settings` flag
/// and the default-path fallback. Every failure mode names the file.
pub(super) fn read_export_settings_file(path: &Path) -> Result<ExportSettingsFile, Box<dyn Error>> {
    read_serialized_settings_file(path).map(|settings| settings.export)
}

/// `SerializedSettings.TryLoadFromDefaultPath` for one caller-supplied directory: an
/// absent file yields `None` (the caller keeps built-in defaults), while a present file
/// loads or fails with a path-carrying error, mirroring the source throw on corrupt
/// content. The directory is a parameter so tests stay hermetic; product code passes
/// the executable directory (see `try_load_default_export_settings`).
pub(super) fn load_default_export_settings_from_dir(
    directory: &Path,
) -> Result<Option<ExportSettingsFile>, Box<dyn Error>> {
    load_default_serialized_settings_from_dir(directory)
        .map(|settings| settings.map(|settings| settings.export))
}

pub(super) fn load_default_serialized_settings_from_dir(
    directory: &Path,
) -> Result<Option<assetripper::export::settings_file::SerializedSettingsFile>, Box<dyn Error>> {
    use assetripper::export::asset_path::HostPathPlatform;
    use assetripper::export::settings_file::default_settings_file_path;

    let platform = if cfg!(windows) {
        HostPathPlatform::Windows
    } else {
        HostPathPlatform::Unix
    };
    let directory_text = directory
        .to_str()
        .ok_or_else(|| invalid_input("the executable directory is not valid UTF-8"))?;
    let path = PathBuf::from(default_settings_file_path(directory_text, platform));
    if !path.exists() {
        return Ok(None);
    }
    read_serialized_settings_file(&path).map(Some)
}

/// Resolve the executable directory (`AppContext.BaseDirectory` in the source).
/// The process query stays in product code; tests pass directories explicitly.
pub(super) fn executable_directory() -> Result<PathBuf, Box<dyn Error>> {
    let executable = std::env::current_exe()
        .map_err(|source| invalid_input(format!("could not locate the executable: {source}")))?;
    executable
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| invalid_input("the executable path has no parent directory"))
}

/// `SerializedSettings.SaveToDefaultPath` for one caller-supplied directory: the
/// record is rendered byte-exactly (same writer as the load path) and overwritten
/// unconditionally, mirroring `File.Create`. Every failure mode names the file.
/// Returns the written path. The directory is a parameter so tests stay hermetic;
/// product code passes the executable directory.
pub(super) fn save_serialized_settings_to_dir(
    settings: &assetripper::export::settings_file::SerializedSettingsFile,
    directory: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    use assetripper::export::asset_path::HostPathPlatform;
    use assetripper::export::settings_file::{default_settings_file_path, write_settings_file};

    let platform = if cfg!(windows) {
        HostPathPlatform::Windows
    } else {
        HostPathPlatform::Unix
    };
    let directory_text = directory
        .to_str()
        .ok_or_else(|| invalid_input("the executable directory is not valid UTF-8"))?;
    let path = PathBuf::from(default_settings_file_path(directory_text, platform));
    let text = write_settings_file(settings).map_err(|error| {
        invalid_input(format!(
            "settings for {} could not be rendered: {error}",
            path.display()
        ))
    })?;
    fs::write(&path, text).map_err(|source| {
        invalid_input(format!(
            "could not write settings file {}: {source}",
            path.display()
        ))
    })?;
    Ok(path)
}

/// `SerializedSettings.DeleteDefaultPath` for one caller-supplied directory: an
/// absent file is a success without effect (mirroring the source existence check),
/// a present file is removed. Returns whether a file was removed. Every failure
/// mode names the file.
pub(super) fn delete_serialized_settings_in_dir(directory: &Path) -> Result<bool, Box<dyn Error>> {
    use assetripper::export::asset_path::HostPathPlatform;
    use assetripper::export::settings_file::default_settings_file_path;

    let platform = if cfg!(windows) {
        HostPathPlatform::Windows
    } else {
        HostPathPlatform::Unix
    };
    let directory_text = directory
        .to_str()
        .ok_or_else(|| invalid_input("the executable directory is not valid UTF-8"))?;
    let path = PathBuf::from(default_settings_file_path(directory_text, platform));
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path).map_err(|source| {
        invalid_input(format!(
            "could not delete settings file {}: {source}",
            path.display()
        ))
    })?;
    Ok(true)
}

/// Read one full `AssetRipper.Settings.json` record, shared by `--settings` inputs
/// that need more than the export section. Every failure mode names the file.
pub(super) fn read_serialized_settings_file(
    path: &Path,
) -> Result<assetripper::export::settings_file::SerializedSettingsFile, Box<dyn Error>> {
    use assetripper::export::settings_file::{parse_settings_file, SettingsFileLimits};

    let limits = SettingsFileLimits::default();
    let bytes = fs::read(path).map_err(|source| {
        invalid_input(format!(
            "could not read settings file {}: {source}",
            path.display()
        ))
    })?;
    if bytes.len() > limits.max_file_bytes {
        return Err(invalid_input(format!(
            "settings file {} uses {} bytes, exceeding limit {}",
            path.display(),
            bytes.len(),
            limits.max_file_bytes
        )));
    }
    let text = String::from_utf8(bytes).map_err(|_| {
        invalid_input(format!(
            "settings file {} is not valid UTF-8",
            path.display()
        ))
    })?;
    parse_settings_file(&text, limits).map_err(|error| {
        invalid_input(format!(
            "settings file {} could not be parsed: {error}",
            path.display()
        ))
    })
}

/// `SerializedSettings.TryLoadFromDefaultPath` plus the `LoadFromDefaultPath` startup
/// step for the CLI: settings stored next to the executable apply when the caller gave
/// no explicit `--settings` flag. `AppContext.BaseDirectory` arrives here as the
/// executable directory, keeping the process query in product code.
pub(super) fn try_load_default_export_settings(
) -> Result<Option<ExportSettingsFile>, Box<dyn Error>> {
    let directory = executable_directory()?;
    load_default_export_settings_from_dir(&directory)
}

/// `save-default-settings [--settings <path>]`: persist settings next to the
/// executable (`SerializedSettings.SaveToDefaultPath`). With `--settings`, the
/// named file is validated and stored; without it, built-in defaults are stored.
/// The file is overwritten unconditionally, mirroring `File.Create`.
pub(super) fn save_default_settings_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
    use assetripper::export::settings_file::SerializedSettingsFile;

    let mut arguments = arguments;
    let mut source: Option<PathBuf> = None;
    while let Some(next) = arguments.next() {
        let text = next
            .into_string()
            .map_err(|_| invalid_input("the --settings value must be valid UTF-8"))?;
        if text == "--settings" {
            if source.is_some() {
                return Err(invalid_input(
                    "save-default-settings accepts --settings at most once",
                ));
            }
            let path = arguments.next().ok_or_else(|| {
                invalid_input("save-default-settings --settings requires a settings file path")
            })?;
            source = Some(PathBuf::from(path));
        } else {
            return Err(invalid_input(format!(
                "unexpected save-default-settings argument '{text}'; expected --settings <path>"
            )));
        }
    }
    let settings = match source {
        Some(path) => read_serialized_settings_file(&path)?,
        None => SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        ),
    };
    let directory = executable_directory()?;
    let path = save_serialized_settings_to_dir(&settings, &directory)?;
    println!("saved settings to {}", path.display());
    Ok(())
}

/// `delete-default-settings`: remove the executable-adjacent settings file
/// (`SerializedSettings.DeleteDefaultPath`). An absent file is a success without
/// effect, mirroring the source existence check.
pub(super) fn delete_default_settings_from_arguments(
    arguments: impl Iterator<Item = OsString>,
) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    if let Some(next) = arguments.next() {
        let text = next
            .into_string()
            .unwrap_or_else(|_| "<invalid UTF-8>".to_owned());
        return Err(invalid_input(format!(
            "unexpected delete-default-settings argument '{text}'; this command takes no arguments"
        )));
    }
    let directory = executable_directory()?;
    // Recompute the display path through the same helper so the report cannot drift.
    let removed = delete_serialized_settings_in_dir(&directory)?;
    if removed {
        println!("deleted the default settings file");
    } else {
        println!("no default settings file exists");
    }
    Ok(())
}

/// Parses the optional leading `--shader-export-mode <dummy|yaml|decompile>` flag of
/// `export-shader`; the default matches `ExportSettings.ShaderExportMode` (`Dummy`).
pub(super) fn parse_shader_export_mode(
    argument: Option<OsString>,
) -> Result<ShaderExportMode, Box<dyn Error>> {
    let Some(argument) = argument else {
        return Ok(ShaderExportMode::Dummy);
    };
    let text = argument
        .into_string()
        .map_err(|_| invalid_input("the --shader-export-mode value must be valid UTF-8"))?;
    match text.as_str() {
        "dummy" => Ok(ShaderExportMode::Dummy),
        "yaml" => Ok(ShaderExportMode::Yaml),
        "decompile" => Ok(ShaderExportMode::Decompile),
        _ => Err(invalid_input(format!(
            "unknown --shader-export-mode '{text}'; expected dummy, yaml, or decompile"
        ))),
    }
}

pub(super) fn export_shader_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut arguments = arguments;
    let mut flag_mode: Option<ShaderExportMode> = None;
    // The settings-file fallback defaults to the `ExportSettings` default (`Dummy`).
    let mut settings_mode = ShaderExportMode::Dummy;
    let mut explicit_settings = false;
    // Optional leading flags precede the positional arguments.
    loop {
        let Some(next) = arguments.next() else {
            return Err(invalid_input(
                "export-shader requires a SerializedFile index",
            ));
        };
        if next == "--shader-export-mode" {
            flag_mode = Some(parse_shader_export_mode(arguments.next())?);
        } else if next == "--settings" {
            let path = arguments.next().ok_or_else(|| {
                invalid_input("export-shader --settings requires a settings file path")
            })?;
            settings_mode = load_cli_export_settings(Path::new(&path))?.shader_export_mode;
            explicit_settings = true;
        } else {
            let serialized_file_index = next
                .into_string()
                .ok()
                .and_then(|text| text.parse::<usize>().ok())
                .ok_or_else(|| invalid_input("export-shader requires a SerializedFile index"))?;
            let path_id = parse_required_argument::<i64>(arguments.next(), "object PathID")?;
            let output_path = arguments
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| invalid_input("export-shader requires an output .shader path"))?;
            let mut input_files = Vec::new();
            for argument in arguments {
                if input_files.len() >= MAX_INPUT_PATHS {
                    return Err(invalid_input(format!(
                        "input path count exceeds {MAX_INPUT_PATHS}"
                    )));
                }
                input_files.push(PathBuf::from(argument));
            }
            if input_files.is_empty() {
                return Err(invalid_input(
                    "export-shader requires at least one explicitly selected input file",
                ));
            }
            let stdout = io::stdout();
            let mut output = stdout.lock();
            // An explicit flag beats the settings file on any order; the
            // default-path file fills the settings fallback when no `--settings`
            // flag was given.
            if !explicit_settings {
                if let Some(export) = try_load_default_export_settings()? {
                    settings_mode = export.shader_export_mode;
                }
            }
            let mode = flag_mode.unwrap_or(settings_mode);
            return export_shader_to(
                &input_files,
                serialized_file_index,
                path_id,
                mode,
                &output_path,
                &mut output,
            );
        }
    }
}

pub(super) fn export_shader_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    mode: ShaderExportMode,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let (object_index, _) = select_unique_object(&workspace, serialized_file_index, path_id)?;

    let objects = load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    if object.factory.class_id != SHADER_CLASS_ID {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-48 Shader",
            object.class_id
        ))
        .into());
    }
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    let view = ShaderView::from_value(&payload.root).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable Shader: {error}"
        ))
    })?;
    let text = export_shader_text(&view, mode).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} shader export failed: {error}"
        ))
    })?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&text)?;
    writeln!(
        status_output,
        "Wrote a shader from SerializedFile {serialized_file_index} PathID {path_id} to {output_path:?}."
    )?;
    Ok(())
}

pub(super) fn export_audio_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut arguments = arguments;
    // The configured format defaults to `ExportSettings.AudioExportFormat` (`Default`).
    let mut configured = ExportDefaults::default().audio_export_format;
    // Optional leading flags precede the positional arguments.
    let mut positional: Vec<OsString> = Vec::new();
    let mut explicit_settings = false;
    while let Some(next) = arguments.next() {
        if next == "--settings" {
            let path = arguments.next().ok_or_else(|| {
                invalid_input("export-audio --settings requires a settings file path")
            })?;
            configured = load_cli_export_settings(Path::new(&path))?.audio_export_format;
            explicit_settings = true;
        } else {
            positional.push(next);
        }
    }
    // An explicit flag beats the default-path file; the default-path file beats
    // built-in defaults, mirroring the startup `LoadFromDefaultPath` step.
    if !explicit_settings {
        if let Some(export) = try_load_default_export_settings()? {
            configured = export.audio_export_format;
        }
    }
    let mut positional = positional.into_iter();
    let serialized_file_index =
        parse_required_argument::<usize>(positional.next(), "SerializedFile index")?;
    let path_id = parse_required_argument::<i64>(positional.next(), "object PathID")?;
    let output_path = positional
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_input("export-audio requires an output audio path"))?;
    let mut input_files = Vec::new();
    for argument in positional {
        if input_files.len() >= MAX_INPUT_PATHS {
            return Err(invalid_input(format!(
                "input path count exceeds {MAX_INPUT_PATHS}"
            )));
        }
        input_files.push(PathBuf::from(argument));
    }
    if input_files.is_empty() {
        return Err(invalid_input(
            "export-audio requires at least one explicitly selected input file",
        ));
    }
    let stdout = io::stdout();
    let mut output = stdout.lock();
    export_audio_to(
        &input_files,
        serialized_file_index,
        path_id,
        configured,
        &output_path,
        &mut output,
    )
}

pub(super) fn export_project_from_arguments<I>(arguments: I) -> Result<(), Box<dyn Error>>
where
    I: Iterator<Item = OsString>,
{
    let mut output_root: Option<PathBuf> = None;
    let mut delete_existing = false;
    let mut settings_path: Option<PathBuf> = None;
    let mut inputs = Vec::new();
    let mut arguments = arguments;
    while let Some(argument) = arguments.next() {
        if argument == "--delete-existing" {
            delete_existing = true;
            continue;
        }
        if argument == "--settings" {
            let value = arguments
                .next()
                .ok_or_else(|| invalid_input("--settings requires a settings file path"))?;
            settings_path = Some(PathBuf::from(value));
            continue;
        }
        match &output_root {
            None => output_root = Some(PathBuf::from(argument)),
            Some(_) => {
                if inputs.len() >= MAX_INPUT_PATHS {
                    return Err(invalid_input(format!(
                        "input path count exceeds {MAX_INPUT_PATHS}"
                    )));
                }
                inputs.push(PathBuf::from(argument));
            }
        }
    }
    let output_root = output_root
        .ok_or_else(|| invalid_input("export-project requires a new output directory"))?;
    if inputs.is_empty() {
        return Err(invalid_input(
            "export-project requires at least one input file or directory",
        ));
    }
    let settings = match settings_path {
        Some(path) => read_serialized_settings_file(&path)?,
        None => load_default_serialized_settings_from_dir(&executable_directory()?)?
            .unwrap_or_else(|| {
                assetripper::export::settings_file::SerializedSettingsFile::from_defaults(
                    Default::default(),
                    Default::default(),
                    Default::default(),
                )
            }),
    };
    let stdout = io::stdout();
    let mut status_output = stdout.lock();
    export_project_to_with_serialized_settings(
        &inputs,
        &output_root,
        delete_existing,
        &settings,
        &mut status_output,
    )
}

#[cfg(test)]
pub(super) fn export_project_to<W: io::Write>(
    input_paths: &[PathBuf],
    output_root: &Path,
    delete_existing: bool,
    settings: &ExportSettingsFile,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let mut full_settings =
        assetripper::export::settings_file::SerializedSettingsFile::from_defaults(
            Default::default(),
            Default::default(),
            Default::default(),
        );
    full_settings.export = settings.clone();
    export_project_to_with_serialized_settings(
        input_paths,
        output_root,
        delete_existing,
        &full_settings,
        status_output,
    )
}

pub(super) fn export_project_to_with_serialized_settings<W: io::Write>(
    input_paths: &[PathBuf],
    output_root: &Path,
    delete_existing: bool,
    full_settings: &assetripper::export::settings_file::SerializedSettingsFile,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) = mount_project_inputs(input_paths)?;
    ensure_no_batch_container_failures(&mount_report)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected paths contain no recognized SerializedFile inputs",
        ));
    }
    let platform = detect_platform_structure(
        &filesystem,
        &selected_paths,
        PlatformStructureLimits::default(),
    )?;
    let original_streaming_assets = if full_settings
        .import
        .streaming_assets_mode
        .skip_streaming_assets()
    {
        None
    } else {
        platform
            .as_ref()
            .and_then(|platform| platform.streaming_assets_path())
            .map(|path| mount_original_streaming_assets(input_paths, &selected_paths, &path))
            .transpose()?
    };
    let platform_inventory = platform
        .as_ref()
        .map(|structure| {
            structure.collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions {
                    skip_streaming_assets: full_settings
                        .import
                        .streaming_assets_mode
                        .skip_streaming_assets(),
                    disable_script_import: full_settings
                        .import
                        .script_content_level
                        .disables_script_import(),
                },
                PlatformFileInventoryLimits::default(),
            )
        })
        .transpose()?;
    let platform_backend = platform_inventory
        .as_ref()
        .map(|inventory| inventory.scripting_backend);
    let mixed_backend = if mixed.assemblies.is_empty() {
        PlatformScriptingBackend::Unknown
    } else {
        PlatformScriptingBackend::Mono
    };
    let scripting_backend = select_scripting_backend(
        full_settings
            .import
            .script_content_level
            .disables_script_import(),
        platform_backend,
        Some(mixed_backend),
    );
    let manager_assemblies = if scripting_backend == PlatformScriptingBackend::Mono {
        if platform.is_some() {
            platform_inventory
                .as_ref()
                .map(|inventory| inventory.assemblies.as_slice())
                .unwrap_or_default()
        } else {
            mixed.assemblies.as_slice()
        }
    } else {
        &[]
    };
    let mut managed_assemblies = ManagedAssemblyCatalog::default();
    let mut managed_assembly_load_report = None;
    let mut mono_manager_initialized = false;
    if scripting_backend == PlatformScriptingBackend::Mono {
        match ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            manager_assemblies,
            ManagedAssemblyCatalogLimits::default(),
        ) {
            Ok((catalog, report)) => {
                managed_assemblies = catalog;
                managed_assembly_load_report = Some(report);
                mono_manager_initialized = true;
            }
            Err(error) => {
                writeln!(
                    status_output,
                    "Mono assembly manager initialization failed and script type recovery is disabled for this import, matching the source fallback: {error}"
                )?;
            }
        }
    }
    let root_catalog = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    let (mut workspace, _) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let mut objects =
        load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let empty_behaviours = objects.recover_header_only_null_script_behaviours(
        &mut workspace,
        ObjectWorkspaceLimits::default(),
        ReferenceGraphLimits::default(),
    )?;
    if empty_behaviours != 0 {
        writeln!(
            status_output,
            "Header-only MonoBehaviours: {empty_behaviours} complete."
        )?;
    }
    if mono_manager_initialized {
        let recovery = objects.recover_mono_behaviour_structures(
            &filesystem,
            &mut workspace,
            &managed_assemblies,
            ObjectWorkspaceLimits::default(),
            ManagedSchemaLimits::default(),
        )?;
        let recovered_count = recovery
            .behaviours
            .iter()
            .filter(|behaviour| {
                matches!(
                    behaviour.outcome,
                    assetripper::serialized_file::object_workspace::ManagedBehaviourRecoveryOutcome::Recovered { .. }
                )
            })
            .count();
        let unresolved_count = recovery.behaviours.len().saturating_sub(recovered_count);
        writeln!(
            status_output,
            "MonoBehaviour script structures: {recovered_count} recovered, {unresolved_count} unresolved."
        )?;
        for behaviour in recovery.behaviours.iter().filter(|behaviour| {
            matches!(
                behaviour.outcome,
                assetripper::serialized_file::object_workspace::ManagedBehaviourRecoveryOutcome::Unresolved(_)
            )
        }).take(10) {
            let identity = behaviour
                .script
                .as_ref()
                .map(|script| script.unique_name())
                .unwrap_or_else(|| "<unavailable MonoScript identity>".to_owned());
            writeln!(
                status_output,
                "  - file {} object {} PathID {} {identity}: {:?}",
                behaviour.file_index,
                behaviour.object_index,
                behaviour.path_id,
                behaviour.outcome
            )?;
        }
        let report =
            ManagedScriptResolutionReport::from_object_workspace(&objects, &managed_assemblies)?;
        let abstract_count = report
            .scripts
            .iter()
            .filter(|script| {
                matches!(
                    script.outcome,
                    ManagedScriptResolutionOutcome::Resolved { is_valid: false }
                )
            })
            .count();
        let unresolved_count = report.scripts.len().saturating_sub(report.resolved_count());
        let load_report = managed_assembly_load_report
            .as_ref()
            .expect("successful Mono manager initialization retains its load report");
        writeln!(
            status_output,
            "Mono assembly metadata: {} loaded from {} candidates ({} native/non-CLI skipped); MonoScript types: {} resolved, {} valid, {} abstract, {} unresolved.",
            load_report.loaded_count,
            load_report.candidate_count,
            load_report.skipped_non_cli.len(),
            report.resolved_count(),
            report.valid_count(),
            abstract_count,
            unresolved_count
        )?;
        for script in report
            .scripts
            .iter()
            .filter(|script| {
                !matches!(
                    script.outcome,
                    ManagedScriptResolutionOutcome::Resolved { .. }
                )
            })
            .take(10)
        {
            let identity = script
                .script
                .as_ref()
                .map(|script| script.unique_name())
                .unwrap_or_else(|| "<unavailable MonoScript metadata>".to_owned());
            writeln!(
                status_output,
                "  - file {} object {} PathID {} {identity}: {:?}",
                script.file_index, script.object_index, script.path_id, script.outcome
            )?;
        }
    } else if scripting_backend == PlatformScriptingBackend::Il2Cpp {
        writeln!(
            status_output,
            "Scripting backend is IL2CPP; its assembly recovery path is not wired into this project exporter."
        )?;
    }
    let reference_graph = workspace.reference_graph(MultiFileReferenceGraphLimits::default())?;
    let mut provider =
        PlatformResourceProvider::with_mixed_data_paths(platform.as_ref(), Some(&mixed.data_paths));
    if let Some(source) = &original_streaming_assets {
        provider = provider.with_streaming_assets_filesystem(source);
    }
    let report = export_workspace_yaml_project_with_serialized_settings(
        output_root,
        delete_existing,
        &filesystem,
        &root_catalog,
        &provider,
        &workspace,
        &objects,
        &reference_graph,
        full_settings,
        mono_manager_initialized,
        mono_manager_initialized.then_some(&managed_assemblies),
        WorkspaceProjectExportLimits::default(),
    )?;
    writeln!(
        status_output,
        "Wrote a Unity project to {:?}: {} SerializedFiles, {} objects in {} asset files ({} with importer metadata; support-file count {}; post-exporter file count {}), {} bytes (project version {}).",
        output_root,
        report.serialized_file_count,
        report.exported_object_count,
        report.asset_file_count,
        report.meta_file_count,
        report.support_file_count,
        report.post_export_file_count,
        report.total_asset_payload_bytes,
        report.project_version.major
    )?;
    if !report.skipped_objects.is_empty() {
        writeln!(
            status_output,
            "{} objects claimed a non-exportable baseline collection and were not written (the baseline writes nothing for them):",
            report.skipped_objects.len()
        )?;
        for notice in report.skipped_objects.iter().take(10) {
            writeln!(status_output, "  - {}: {}", notice.subject, notice.reason)?;
        }
    }
    if !report.unresolved_script_content.is_empty() {
        writeln!(
            status_output,
            "{} MonoScript entries have unavailable source content; source pointers are retained:",
            report.unresolved_script_content.len()
        )?;
        for notice in report.unresolved_script_content.iter().take(10) {
            writeln!(status_output, "  - {}: {}", notice.subject, notice.reason)?;
        }
    }
    if !report.processing_warnings.is_empty() {
        writeln!(
            status_output,
            "{} Scene/Prefab processing warnings:",
            report.processing_warnings.len()
        )?;
        for warning in report.processing_warnings.iter().take(10) {
            writeln!(status_output, "  - {warning}")?;
        }
    }
    if !report.fallbacks.is_empty() {
        writeln!(
            status_output,
            "{} files used the generic asset-family renderer because their specialized exporter is not wired yet:",
            report.fallbacks.len()
        )?;
        for notice in report.fallbacks.iter().take(10) {
            writeln!(status_output, "  - {}: {}", notice.subject, notice.reason)?;
        }
    }
    if !report.opaque_fallbacks.is_empty() {
        writeln!(
            status_output,
            "{} Unknown/Unreadable objects were preserved as raw bytes without Unity metadata; their fields and embedded references remain opaque:",
            report.opaque_fallbacks.len()
        )?;
        for notice in report.opaque_fallbacks.iter().take(10) {
            writeln!(status_output, "  - {}: {}", notice.subject, notice.reason)?;
        }
    }
    writeln!(
        status_output,
        "Mounted {} logical inputs ({} bytes). Processed Scene collections and modern/legacy Prefab collection routes are wired; remaining class-specific exporters are incomplete.",
        mount_report.discovered_inputs,
        mount_report.discovered_source_bytes
    )?;
    Ok(())
}

/// Retain physical StreamingAssets independently of the expanded processing VFS.
/// Copying must preserve wrapper bytes, container filenames, numbered split parts
/// and empty directories. Only metadata and owning file handles are mounted here.
pub(super) fn mount_original_streaming_assets(
    input_paths: &[PathBuf],
    selected_paths: &[String],
    virtual_root: &str,
) -> Result<VirtualFileSystem, Box<dyn Error>> {
    let inputs = deduplicate_logical_input_paths(input_paths);
    let physical_root = selected_paths
        .iter()
        .zip(&inputs)
        .find_map(|(selected, physical)| {
            let relative = virtual_root.strip_prefix(selected)?;
            if !relative.is_empty() && !relative.starts_with('/') {
                return None;
            }
            let mut root = physical.clone();
            for component in relative
                .trim_start_matches('/')
                .split('/')
                .filter(|part| !part.is_empty())
            {
                if component == "." || component == ".." {
                    return None;
                }
                root.push(component);
            }
            Some(root)
        })
        .ok_or_else(|| {
            invalid_input(format!(
                "cannot locate original StreamingAssets for {virtual_root:?}"
            ))
        })?;
    let mut source = VirtualFileSystem::new();
    let metadata = match fs::symlink_metadata(&physical_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(source),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(invalid_input(format!(
            "StreamingAssets root {:?} is not a regular directory",
            physical_root
        )));
    }
    let limits = WorkspaceProjectExportLimits::default();
    let mut entries = 1usize;
    let mut total_bytes = 0u64;
    source.create_directory(Some(virtual_root))?;
    let mut pending = vec![(physical_root, virtual_root.to_owned(), 0usize)];
    while let Some((directory, destination, depth)) = pending.pop() {
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            entries = entries
                .checked_add(1)
                .ok_or_else(|| invalid_input("StreamingAssets entry count overflows"))?;
            if entries > limits.max_post_export_entries {
                return Err(invalid_input(
                    "StreamingAssets entry count exceeds its limit",
                ));
            }
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(invalid_input(format!(
                    "StreamingAssets input {:?} is a symbolic link",
                    path
                )));
            }
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or_else(|| invalid_input("StreamingAssets filename is not valid UTF-8"))?;
            // The VFS normalizes backslashes even on POSIX; accepting one would
            // silently change an original filename during post-export copying.
            if name.contains(['/', '\\']) {
                return Err(invalid_input(format!(
                    "StreamingAssets filename {name:?} cannot be represented by the VFS"
                )));
            }
            let virtual_path = format!("{destination}/{name}");
            if virtual_path.len() > source.limits().max_path_bytes {
                return Err(invalid_input("StreamingAssets path exceeds its limit"));
            }
            if metadata.is_dir() {
                let next_depth = depth
                    .checked_add(1)
                    .ok_or_else(|| invalid_input("StreamingAssets directory depth overflows"))?;
                if next_depth > limits.project_output.max_path_components {
                    return Err(invalid_input(
                        "StreamingAssets directory depth exceeds its limit",
                    ));
                }
                source.create_directory(Some(&virtual_path))?;
                pending
                    .try_reserve(1)
                    .map_err(|_| invalid_input("could not retain StreamingAssets directories"))?;
                pending.push((path, virtual_path, next_depth));
            } else if metadata.is_file() {
                total_bytes = total_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| invalid_input("StreamingAssets byte count overflows"))?;
                if total_bytes > limits.max_total_asset_payload_bytes as u64 {
                    return Err(invalid_input(
                        "StreamingAssets bytes exceed the project limit",
                    ));
                }
                let stream = assetripper::io::stream::SmartStream::open_read(&path)?;
                if stream.length()? != metadata.len() {
                    return Err(invalid_input(format!(
                        "StreamingAssets input {:?} changed while opening it",
                        path
                    )));
                }
                source.insert_file_stream_with_parents(&virtual_path, stream)?;
            } else {
                return Err(invalid_input(format!(
                    "StreamingAssets input {:?} is not a regular file",
                    path
                )));
            }
        }
    }
    Ok(source)
}

pub(super) fn export_audio_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    format: SettingsAudioFormat,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let container_bundle_index = file.container_bundle_index;
    let (object_index, _) = select_unique_object(&workspace, serialized_file_index, path_id)?;

    let objects = load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    if object.factory.class_id != AUDIO_CLIP_CLASS_ID {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} has class {:?}, not a class-83 AudioClip",
            object.class_id
        ))
        .into());
    }
    let payload = object.payload.as_ref().map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} could not be decoded: {error}"
        ))
    })?;
    let view = AudioClipView::from_value(&payload.root).map_err(|error| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} is not an exportable AudioClip: {error}"
        ))
    })?;
    let raw_data: Vec<u8> = match view.audio_data_source() {
        AudioDataSource::Inline(bytes) => bytes.to_vec(),
        AudioDataSource::Resource(info) => {
            if !info.is_set() {
                return Err(invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} has an unset audio resource"
                ))
                .into());
            }
            let provider =
                PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
            let resolution = resolve_and_read_streamed_resource_range(
                &filesystem,
                &workspace,
                container_bundle_index,
                StreamedResourceRange {
                    path: &info.path,
                    offset: info.offset,
                    size: info.size,
                },
                &root_resources,
                &provider,
                StreamedResourceContentLimits::default(),
            )
            .map_err(|error| {
                invalid_data_owned(format!(
                    "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' could not be read: {error}",
                    info.path
                ))
            })?;
            match resolution {
                StreamedResourceContentResolution::Resolved { bytes, .. } => bytes,
                StreamedResourceContentResolution::Missing {
                    unavailable_data_paths,
                } => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} PathID {path_id} streamed resource '{}' is missing; unavailable data paths: {unavailable_data_paths:?}",
                        info.path
                    ))
                    .into());
                }
                StreamedResourceContentResolution::Unset => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} PathID {path_id} has an unset audio resource"
                    ))
                    .into());
                }
            }
        }
        AudioDataSource::Empty => Vec::new(),
    };
    let name = view
        .name
        .as_deref()
        .and_then(|bytes| std::str::from_utf8(bytes).ok())
        .unwrap_or_default();
    let (decoded, extension) =
        try_decode_audio(name, view.sound_type, &raw_data).map_err(|error| {
            invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} audio export failed: {error}"
        ))
        })?;
    // `AudioClipExporter.TryCreateCollection` picks the collection extension from the
    // decode outcome plus the configured `AudioExportFormat`; only `PreferWav` meeting
    // an `ogg` decode changes anything. The class guard above already established the
    // clip, and a successful decode is never empty.
    let collection_extension = plan_audio_clip_collection(AudioDecodeFact {
        is_audio_clip: true,
        decode_succeeded: true,
        decoded_is_empty: decoded.is_empty(),
        decoded_extension: extension,
        format,
    })
    .map_err(|refusal| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} audio export refused: {refusal:?}"
        ))
    })?;
    if converts_ogg_to_wav(extension, &collection_extension) {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} PathID {path_id} requires an Ogg-to-WAV conversion, which needs a Vorbis decoder that is not available"
        ))
        .into());
    }
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(&decoded)?;
    writeln!(
        status_output,
        "Wrote a {collection_extension} audio from SerializedFile {serialized_file_index} PathID {path_id} to {output_path:?}."
    )?;
    Ok(())
}

/// `Mesh.Name`, when the payload carries a readable UTF-8 name.
pub(super) fn mesh_export_name(root: &SerializedValue) -> Option<String> {
    root.unique_field_in_base_chain("m_Name")
        .ok()
        .flatten()
        .and_then(|value| match value {
            SerializedValue::StringBytes(bytes) => {
                std::str::from_utf8(bytes).ok().map(str::to_owned)
            }
            SerializedValue::Bytes(bytes) => std::str::from_utf8(bytes).ok().map(str::to_owned),
            _ => None,
        })
        .filter(|name| !name.is_empty())
}

/// Finds the single object with `path_id`, rejecting a missing or duplicated PathID.
pub(super) fn select_unique_object(
    workspace: &SerializedFileWorkspace,
    serialized_file_index: usize,
    path_id: i64,
) -> Result<(usize, &ObjectInfo), Box<dyn Error>> {
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let mut matching = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.path_id == path_id);
    let found = matching.next().ok_or_else(|| {
        invalid_input(format!(
            "SerializedFile {serialized_file_index} has no object with PathID {path_id}"
        ))
    })?;
    if matching.next().is_some() {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has multiple objects with PathID {path_id}"
        ))
        .into());
    }
    Ok(found)
}

/// The numeric Unity version used by the vertex-data branches.
///
/// An absent or unparseable version string is reported rather than silently defaulted, because the
/// channel and stream layouts differ across versions.
pub(super) fn file_unity_version(
    workspace: &SerializedFileWorkspace,
    serialized_file_index: usize,
) -> Result<UnityVersion, Box<dyn Error>> {
    let text = workspace
        .files()
        .get(serialized_file_index)
        .and_then(|file| file.metadata.prefix.unity_version_text.as_deref())
        .ok_or_else(|| {
            invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} has no Unity version string, so its mesh \
                 channel layout cannot be resolved"
            ))
        })?;
    UnityVersion::parse(text).ok_or_else(|| {
        invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} Unity version '{text}' is unparseable, so its \
             mesh channel layout cannot be resolved"
        ))
        .into()
    })
}

/// The file's body byte order.
///
/// Pre-generation-9 files keep the flag in the metadata; a file whose header does not declare one
/// is little endian, matching `EndianBinaryReader`'s default.
pub(super) fn file_endian_type(
    workspace: &SerializedFileWorkspace,
    serialized_file_index: usize,
) -> EndianType {
    match workspace
        .files()
        .get(serialized_file_index)
        .and_then(|file| file.header.data_endian)
    {
        Some(Endian::Big) => EndianType::Big,
        Some(Endian::Little) | None => EndianType::Little,
    }
}

pub(super) fn parse_required_argument<T>(
    argument: Option<OsString>,
    description: &'static str,
) -> Result<T, Box<dyn Error>>
where
    T: std::str::FromStr,
{
    let argument = argument
        .ok_or_else(|| invalid_input(format!("export-object-yaml requires {description}")))?;
    let argument = argument
        .into_string()
        .map_err(|_| invalid_input(format!("{description} must be valid UTF-8")))?;
    argument
        .parse()
        .map_err(|_| invalid_input(format!("invalid {description} '{argument}'")))
}

pub(super) fn export_object_yaml_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    path_id: i64,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let mut matching_objects = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .filter(|(_, object)| object.path_id == path_id);
    let (object_index, _) = matching_objects.next().ok_or_else(|| {
        invalid_input(format!(
            "SerializedFile {serialized_file_index} has no object with PathID {path_id}"
        ))
    })?;
    if matching_objects.next().is_some() {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has multiple objects with PathID {path_id}"
        ))
        .into());
    }

    let objects = load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let object = objects
        .get(serialized_file_index, object_index)
        .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
    let class_id = object
        .class_id
        .map(i32::from)
        .ok_or_else(|| invalid_data("selected object has no class ID"))?;
    let source = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: serialized_file_index,
        },
        object_index,
        path_id,
        class_id,
    };
    let const_collection_index = 0;
    let const_file_id = 1;
    let pointer_map = ProjectExportPointerMap::new(
        &[ExportedAssetPointerEntry {
            asset: source,
            file_id: const_file_id,
            collection_index: const_collection_index,
        }],
        &[ExportCollectionPointerMetadata {
            collection_index: const_collection_index,
            // This one-object YAML probe writes no Unity .meta file or foreign collection GUID.
            guid: [0; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }],
        ExportPointerMapLimits::default(),
    )?;
    let reference_graph = workspace.reference_graph(MultiFileReferenceGraphLimits::default())?;
    let provider = PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
    let resource_context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &root_resources,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };
    let Some(document) = workspace_object_to_unity_yaml_document_with_streamed_resources(
        &workspace,
        &reference_graph,
        &pointer_map,
        object,
        source,
        const_file_id,
        const_collection_index,
        None,
        UnityYamlObjectConversionLimits::default(),
        &resource_context,
    )?
    else {
        writeln!(
            status_output,
            "Skipped SerializedFile {serialized_file_index} PathID {path_id}: the baseline YAML exporter omits image textures without usable inline or streamed data."
        )?;
        return Ok(());
    };
    let yaml = write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default())?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(yaml.as_bytes())?;
    writeln!(
        status_output,
        "Wrote one YAML object from SerializedFile {serialized_file_index} PathID {path_id} to {:?}.",
        output_path
    )?;
    Ok(())
}

pub(super) fn export_file_yaml_to<W: io::Write>(
    input_files: &[PathBuf],
    serialized_file_index: usize,
    output_path: &Path,
    status_output: &mut W,
) -> Result<(), Box<dyn Error>> {
    let (mut filesystem, selected_paths, mut mount_report) =
        mount_input_files_for_export(input_files)?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    let root_resources = RootResourceFileCatalog::from_mounted_explicit_resource_files(
        &filesystem,
        &mount_report.explicit_root_resource_files,
        RootResourceCatalogLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected files contain no recognized SerializedFile input",
        ));
    }
    let (workspace, _) = load_workspace_with_mount_report(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        WorkspaceLoadLimits::default(),
        &mut mount_report,
    )?;
    ensure_no_workspace_container_failures(&workspace)?;
    let file = workspace
        .files()
        .get(serialized_file_index)
        .ok_or_else(|| {
            invalid_input(format!(
                "SerializedFile index {serialized_file_index} is out of range for {} loaded files",
                workspace.files().len()
            ))
        })?;
    let object_count = file.metadata.objects.len();
    let collection_limits = UnityYamlWorkspaceCollectionLimits::default();
    if object_count > collection_limits.max_documents {
        return Err(invalid_data_owned(format!(
            "SerializedFile {serialized_file_index} has {object_count} objects, exceeding the YAML document limit {}",
            collection_limits.max_documents
        ))
        .into());
    }

    let objects = load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())?;
    let reference_graph = workspace.reference_graph(MultiFileReferenceGraphLimits::default())?;
    let mut assignments = Vec::new();
    assignments.try_reserve_exact(object_count).map_err(|_| {
        invalid_data_owned(format!("unable to reserve {object_count} YAML assignments"))
    })?;
    let mut pointer_entries = Vec::new();
    pointer_entries
        .try_reserve_exact(object_count)
        .map_err(|_| {
            invalid_data_owned(format!("unable to reserve {object_count} YAML pointers"))
        })?;

    for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
        if object_info.path_id == 0 {
            return Err(invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} object {object_index} has PathID 0, which cannot be assigned as a Unity YAML document ID"
            ))
            .into());
        }
        if reference_graph.object_scan_is_complete(serialized_file_index, object_index)
            != Some(true)
        {
            return Err(invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} object {object_index} PathID {} has no complete TypeTree reference scan",
                object_info.path_id
            ))
            .into());
        }
        for reference in reference_graph.references_from_object(serialized_file_index, object_index)
        {
            match &reference.resolution {
                MultiFilePPtrResolution::Null => {}
                MultiFilePPtrResolution::Object {
                    target_file_index, ..
                } if *target_file_index == serialized_file_index => {}
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} object {object_index} PathID {} references object {target_object_index} in SerializedFile {target_file_index}; one-file YAML export cannot assign a foreign collection GUID",
                        object_info.path_id
                    ))
                    .into());
                }
                unresolved => {
                    return Err(invalid_data_owned(format!(
                        "SerializedFile {serialized_file_index} object {object_index} PathID {} has unresolved reference at {:?}: {unresolved:?}",
                        object_info.path_id, reference.field_path
                    ))
                    .into());
                }
            }
        }

        let object = objects
            .get(serialized_file_index, object_index)
            .ok_or_else(|| invalid_data("selected workspace object is missing"))?;
        let class_id = object
            .class_id
            .map(i32::from)
            .ok_or_else(|| invalid_data_owned(format!(
                "SerializedFile {serialized_file_index} object {object_index} PathID {} has no class ID",
                object_info.path_id
            )))?;
        let asset = ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile {
                file_index: serialized_file_index,
            },
            object_index,
            path_id: object_info.path_id,
            class_id,
        };
        assignments.push(SceneExportIdAssignment {
            asset,
            file_id: object_info.path_id,
        });
        pointer_entries.push(ExportedAssetPointerEntry {
            asset,
            file_id: object_info.path_id,
            collection_index: 0,
        });
    }

    let pointer_map = ProjectExportPointerMap::new(
        &pointer_entries,
        &[ExportCollectionPointerMetadata {
            collection_index: 0,
            guid: [0; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }],
        ExportPointerMapLimits::default(),
    )?;
    let provider = PlatformResourceProvider::with_mixed_data_paths(None, Some(&mixed.data_paths));
    let resource_context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &root_resources,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };
    let yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments,
        0,
        None,
        collection_limits,
        &resource_context,
    )?;
    let exported_document_count = yaml.match_indices("--- !u!").count();
    let omitted_object_count = object_count
        .checked_sub(exported_document_count)
        .ok_or_else(|| invalid_data("YAML exporter wrote more documents than source objects"))?;
    if object_count > 0 && exported_document_count == 0 {
        writeln!(
            status_output,
            "Skipped SerializedFile {serialized_file_index}: all {object_count} objects were omitted because the baseline YAML exporter found ImageTexture objects without usable inline or streamed data."
        )?;
        return Ok(());
    }
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_path)?;
    destination.write_all(yaml.as_bytes())?;
    if omitted_object_count == 0 {
        writeln!(
            status_output,
            "Wrote {exported_document_count} ordered YAML objects from SerializedFile {serialized_file_index} to {:?}.",
            output_path
        )?;
    } else {
        writeln!(
            status_output,
            "Wrote {exported_document_count} ordered YAML objects from {object_count} objects in SerializedFile {serialized_file_index} to {:?}; the baseline YAML exporter omitted {omitted_object_count} ImageTexture objects without usable inline or streamed data.",
            output_path
        )?;
    }
    Ok(())
}
