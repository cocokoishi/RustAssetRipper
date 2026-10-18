//! Product web-surface state: the loaded game, the settings record and the log tail.
//!
//! The frozen product keeps a single `GameData` value plus one `FullConfiguration` in
//! `GameFileLoader`. The port keeps the same two pieces of state in [`WebState`]: the settings
//! record loaded from the default path beside the executable, and an optional [`LoadedGame`]
//! produced by the shared `load_game` pipeline that the CLI export commands already use.
//!
//! Every workflow here mirrors the baseline gate sequence: a load resets first, an export
//! applies `IsValidExportDirectory` and the non-empty-directory deletion consent, and settings
//! are written back only when `SaveSettingsToDisk` is set.

use std::error::Error;
use std::path::{Path, PathBuf};

use assetripper::export::project_workflow::ProjectExportOutputLayout;
use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
use assetripper::export::settings_file::{
    decide_settings_persistence, parse_export_settings_text, parse_import_settings_text,
    parse_processing_settings_text, write_export_settings_text, write_import_settings_text,
    write_processing_settings_text, SerializedSettingsFile, SettingsFileError, SettingsFileLimits,
    SettingsPersistenceAction,
};
use assetripper::localization::{decide_load_language, LoadLanguageOutcome};
use assetripper::product::{
    deletion_consent, is_valid_export_directory, maybe_append_timestamped_subfolder,
    resolve_export_flow, ExportFlow,
};
use assetripper::resources::vertex_data_blob::UnityVersion;
use assetripper::web_paths::FailedFilePath;

use crate::export_commands::{
    delete_serialized_settings_in_dir, executable_directory, export_loaded_game,
    load_default_serialized_settings_from_dir, load_game, save_serialized_settings_to_dir,
    LoadedGame,
};
use crate::primary_content_commands::export_loaded_primary_content;
use assetripper::configuration::{ListData, SingletonData};
use assetripper::export::scene_wiring::{
    process_workspace_scenes_and_prefabs, WorkspaceSceneProcessingLimits,
};
use assetripper::processing::scene_collections::WorkspaceSceneDefinitions;
use assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits;

use super::localization::{Messages, FALLBACK_LANGUAGE};
use super::tree::BundleTree;

/// The largest virtual-filesystem read one resource data response serves, matching the
/// export's per-file payload bound.
const MAX_SOURCE_RESPONSE_BYTES: usize = 4 * 1024 * 1024 * 1024;

/// What the export routes should do for one submitted path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportDecision {
    /// No game is loaded, or the submitted path is empty: the baseline does nothing.
    Skip,
    /// The path is a protected system directory: the baseline logs an error and stops.
    InvalidDirectory,
    /// The directory holds files and the user has not confirmed deletion yet.
    NeedsConfirmation,
    /// Run the export, deleting the existing directory first when it is non-empty.
    Proceed { delete_existing: bool },
}

/// The loaded state plus everything else a page needs.
pub struct WebState {
    messages: Messages,
    settings: SerializedSettingsFile,
    executable_directory: PathBuf,
    headless: bool,
    language: String,
    loaded: Option<LoadedGame>,
    tree: Option<BundleTree>,
    /// The workspace's scene definitions, computed once on first use.
    ///
    /// The definitions come from the scene-definition pass, which scans every collection and object;
    /// the reference computes it once at load and holds it on the game data. The port computes it on
    /// first request and caches it here rather than repeating the scan for every `/Scenes/View`.
    scene_definitions: Option<WorkspaceSceneDefinitions>,
    /// `CoreConfiguration.SingletonData`: the named configuration texts the settings page edits.
    ///
    /// In-memory for the session and not carried in the settings file. `GameFileLoader.Reset`
    /// drops only the loaded game, so these entries survive that user-facing reset.
    singleton_data: SingletonData,
    /// `CoreConfiguration.ListData`: the named configuration lists the settings page edits.
    list_data: ListData,
    log: Vec<String>,
    static_content: Vec<(String, Vec<u8>)>,
}

/// The mesh's `m_Name`, matching the CLI exporter's name rule.
///
/// The CLI helper is `pub(super)` inside the binary root, so the web layer mirrors its two-line
/// behaviour rather than widening that helper's visibility for one caller.
fn mesh_name(root: &assetripper::serialized_file::object::SerializedValue) -> Option<String> {
    use assetripper::serialized_file::object::SerializedValue;

    root.unique_field_in_base_chain("m_Name")
        .ok()
        .flatten()
        .and_then(|value| match value {
            SerializedValue::StringBytes(bytes) | SerializedValue::Bytes(bytes) => {
                std::str::from_utf8(bytes).ok().map(str::to_owned)
            }
            _ => None,
        })
        .filter(|name| !name.is_empty())
}

/// A video clip's streamed content with the extension taken from its original path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VideoData {
    /// The `m_ExternalResources` range bytes.
    pub bytes: Vec<u8>,
    /// The container extension named by the clip's `m_OriginalPath`.
    pub extension: String,
}

/// A font asset's binary data with the extension and MIME the frozen route reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontData {
    /// The font's `m_FontData` bytes.
    pub bytes: Vec<u8>,
    /// The sniffed container extension (`otf`, `ttf`, `ttc`, or `dat`).
    pub extension: &'static str,
    /// The MIME type paired with `extension`.
    pub mime: &'static str,
}

impl WebState {
    /// Load the settings record and the message tables the way the baseline launcher does.
    ///
    /// `LoadFromDefaultPath` keeps the class defaults when no file exists, and writes nothing at
    /// startup, so a missing file is not an error.
    pub fn new(headless: bool) -> Result<Self, Box<dyn Error>> {
        let messages = Messages::load()?;
        let directory = executable_directory()?;
        let settings =
            load_default_serialized_settings_from_dir(&directory)?.unwrap_or_else(|| {
                SerializedSettingsFile::from_defaults(
                    ImportDefaults::default(),
                    ProcessingDefaults::default(),
                    ExportDefaults::default(),
                )
            });
        Self::with_settings(messages, settings, directory, headless)
    }

    /// Build state from an explicit settings record, executable directory and headless flag.
    pub fn with_settings(
        messages: Messages,
        settings: SerializedSettingsFile,
        executable_directory: PathBuf,
        headless: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let language =
            match decide_load_language(FALLBACK_LANGUAGE, settings.export.language_code.as_deref())
            {
                LoadLanguageOutcome::Switched { code } => code,
                LoadLanguageOutcome::Unchanged => FALLBACK_LANGUAGE.to_owned(),
            };
        let mut state = Self {
            messages,
            settings,
            executable_directory,
            headless,
            language,
            loaded: None,
            tree: None,
            scene_definitions: None,
            singleton_data: SingletonData::new(),
            list_data: ListData::new(),
            log: Vec::new(),
            static_content: Vec::new(),
        };
        state.refresh_configuration_singletons()?;
        Ok(state)
    }

    /// The message tables.
    pub fn messages(&self) -> &Messages {
        &self.messages
    }

    /// The current language code.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Whether a browser window is suppressed; headless exports consent to directory deletion.
    ///
    /// The export gate reads the stored flag; only the tests read it directly.
    #[cfg(test)]
    pub fn headless(&self) -> bool {
        self.headless
    }

    /// The loaded settings record.
    pub fn settings(&self) -> &SerializedSettingsFile {
        &self.settings
    }

    /// The settings record for a form handler to update.
    pub fn settings_mut(&mut self) -> &mut SerializedSettingsFile {
        &mut self.settings
    }

    /// Keep the configuration-files page's typed settings entries in sync with the settings
    /// records used by import, processing, and export.
    pub fn refresh_configuration_singletons(&mut self) -> Result<(), SettingsFileError> {
        let import = write_import_settings_text(&self.settings.import)?;
        let processing = write_processing_settings_text(&self.settings.processing)?;
        let export = write_export_settings_text(&self.settings.export)?;
        *self.singleton_data.get_or_add("ImportSettings") = import;
        *self.singleton_data.get_or_add("ProcessingSettings") = processing;
        *self.singleton_data.get_or_add("ExportSettings") = export;
        // The baseline registers EngineResourceData as a typed singleton whose initial Text is
        // empty. Project export consumes its resource dictionaries; the currently wired typed
        // cache key is TextAsset, while other known resource kinds remain an open parity item.
        self.singleton_data.get_or_add("EngineResourceData");
        Ok(())
    }

    /// Apply a submitted configuration-file singleton. The three settings records use the
    /// baseline's typed JSON serializer: malformed text creates a fresh default record, while
    /// ordinary string entries retain their submitted text.
    pub fn set_singleton_data(
        &mut self,
        key: &str,
        content: &str,
    ) -> Result<(), SettingsFileError> {
        let limits = SettingsFileLimits::default();
        match key {
            "ImportSettings" => {
                self.settings.import =
                    parse_import_settings_text(content, limits).unwrap_or_else(|_| {
                        assetripper::export::settings_file::ImportSettingsFile::from_defaults(
                            ImportDefaults::default(),
                        )
                    });
            }
            "ProcessingSettings" => {
                self.settings.processing = parse_processing_settings_text(content, limits)
                    .unwrap_or_else(|_| {
                        assetripper::export::settings_file::ProcessingSettingsFile::from(
                            ProcessingDefaults::default(),
                        )
                    });
            }
            "ExportSettings" => {
                self.settings.export =
                    parse_export_settings_text(content, limits).unwrap_or_else(|_| {
                        assetripper::export::settings_file::ExportSettingsFile::from(
                            ExportDefaults::default(),
                        )
                    });
            }
            "EngineResourceData" => {
                let text = normalize_empty_engine_resource_data_json(content)
                    .unwrap_or_else(|| content.to_owned());
                *self.singleton_data.get_or_add(key) = text;
                return Ok(());
            }
            _ => {
                *self.singleton_data.get_or_add(key) = content.to_owned();
                return Ok(());
            }
        }
        self.refresh_configuration_singletons()
    }

    /// Clear one configuration singleton. Typed settings entries reset to their source defaults;
    /// ordinary strings become empty while retaining their key.
    pub fn clear_singleton_data(&mut self, key: &str) -> Result<bool, SettingsFileError> {
        match key {
            "ImportSettings" => {
                self.settings.import =
                    assetripper::export::settings_file::ImportSettingsFile::from_defaults(
                        ImportDefaults::default(),
                    );
                self.refresh_configuration_singletons()?;
                Ok(true)
            }
            "ProcessingSettings" => {
                self.settings.processing =
                    assetripper::export::settings_file::ProcessingSettingsFile::from(
                        ProcessingDefaults::default(),
                    );
                self.refresh_configuration_singletons()?;
                Ok(true)
            }
            "ExportSettings" => {
                self.settings.export = assetripper::export::settings_file::ExportSettingsFile::from(
                    ExportDefaults::default(),
                );
                self.refresh_configuration_singletons()?;
                Ok(true)
            }
            _ => Ok(self.singleton_data.clear(key)),
        }
    }

    /// Register the `--local-web-file` overrides as URL path to content.
    pub fn set_static_content(&mut self, entries: Vec<(String, Vec<u8>)>) {
        self.static_content = entries;
    }

    /// The override registered for a URL path, when one is present.
    pub fn static_content(&self, path: &str) -> Option<&[u8]> {
        self.static_content
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map(|(_, content)| content.as_slice())
    }

    /// Whether a game is loaded.
    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    /// The bundle-tree snapshot, present exactly when a game is loaded.
    pub fn tree(&self) -> Option<&BundleTree> {
        self.tree.as_ref()
    }

    /// Read one virtual-filesystem file, bounding the response like the export's
    /// per-file payload bound does. The resource data routes serve these bytes.
    pub fn read_resource_bytes(&self, virtual_path: &str) -> Result<Vec<u8>, String> {
        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        game.filesystem
            .read_all_bytes(virtual_path, MAX_SOURCE_RESPONSE_BYTES)
            .map_err(|error| error.to_string())
    }

    /// Read one asset's served text, if it has any.
    ///
    /// Only TextAsset scripts are served: the frozen `HasText` also covers shaders and
    /// MonoScripts, whose text ports are still open. Anything else reads as no text,
    /// matching the frozen route's empty reply for assets without text.
    pub fn read_asset_text(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<Option<Vec<u8>>, String> {
        use assetripper::export::project_workflow::text_asset_script_bytes;

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let payload = object.payload.as_ref().map_err(|error| error.to_string())?;
        text_asset_script_bytes(payload).map_err(|error| error.to_string())
    }

    /// Read one asset's raw payload bytes.
    ///
    /// Undecodable objects serve their raw range, mirroring the frozen `RawDataObject`
    /// bytes; anything else reads as no bytes, matching the frozen route's empty reply
    /// for assets without binary data.
    pub fn read_asset_raw_bytes(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<Vec<u8>, String> {
        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        if object.payload.is_ok() {
            return Ok(Vec::new());
        }
        let file = game
            .workspace
            .files()
            .get(file_index)
            .ok_or_else(|| "the snapshot row has no workspace file".to_owned())?;
        let bytes = self.read_resource_bytes(&file.virtual_path)?;
        let start = usize::try_from(object.raw_payload_range.start)
            .map_err(|_| "the raw payload offset does not fit the host address space".to_owned())?;
        let end = usize::try_from(object.raw_payload_range.end)
            .map_err(|_| "the raw payload end does not fit the host address space".to_owned())?;
        bytes
            .get(start..end)
            .map(<[u8]>::to_vec)
            .ok_or_else(|| "the raw payload range is outside the file bytes".to_owned())
    }

    /// The loaded game, when one is present.
    pub fn loaded(&self) -> Option<&LoadedGame> {
        self.loaded.as_ref()
    }

    /// Read one texture asset's image bytes.
    ///
    /// Inline payload bytes are used when present; otherwise a streamed resource is resolved
    /// against the workspace's data paths, mirroring the export's own streamed-resource
    /// lookup. Assets without an image read as `None`, matching the frozen route's empty
    /// reply.
    pub fn read_asset_image_bytes(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<Option<Vec<u8>>, String> {
        use assetripper::io::platform_structure::PlatformResourceProvider;
        use assetripper::resources::texture::Texture2DView;

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let Ok(payload) = object.payload.as_ref() else {
            return Ok(None);
        };
        let Ok(view) = Texture2DView::from_value(&payload.root) else {
            return Ok(None);
        };
        if let Some(bytes) = view.image_bytes(None) {
            return Ok(Some(bytes.to_vec()));
        }
        let Some(info) = view.stream_data.as_ref().filter(|info| info.is_set()) else {
            return Ok(None);
        };
        let file = game
            .workspace
            .files()
            .get(file_index)
            .ok_or_else(|| "the snapshot row has no workspace file".to_owned())?;
        let provider =
            PlatformResourceProvider::with_mixed_data_paths(None, Some(&game.data_paths));
        let resolution = assetripper::resource::resolve_and_read_streamed_resource_info(
            &game.filesystem,
            &game.workspace,
            file.container_bundle_index,
            info,
            &game.root_catalog,
            &provider,
            assetripper::resource::StreamedResourceContentLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        Ok(match resolution {
            assetripper::resource::StreamedResourceContentResolution::Resolved {
                bytes, ..
            } => Some(bytes.to_vec()),
            assetripper::resource::StreamedResourceContentResolution::Missing { .. }
            | assetripper::resource::StreamedResourceContentResolution::Unset => None,
        })
    }

    /// The stack trace for one failed container entry, mirroring `FailedFile.StackTrace`.
    ///
    /// The frozen `StackTrace` is the caught exception's `ToString()`, set where the failure was
    /// recorded. The port keeps the same failure as a `UnityFsError`, so this renders that error's
    /// full diagnostic chain in place of a .NET stack trace.
    pub fn failed_entry_stack_trace(
        &self,
        path: &FailedFilePath,
    ) -> Result<Option<String>, String> {
        Ok(self
            .failed_entry(path)?
            .map(|entry| entry.error.to_string()))
    }

    /// Resolve a failed-file path to its workspace entry.
    fn failed_entry(
        &self,
        path: &FailedFilePath,
    ) -> Result<Option<&assetripper::scheme::FailedContainerEntry>, String> {
        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let tree = self
            .tree
            .as_ref()
            .ok_or_else(|| "a game is loaded but its bundle tree is missing".to_owned())?;
        let Some((_bundle, failed_index)) = tree.resolve_failed(path) else {
            return Ok(None);
        };
        Ok(game
            .workspace
            .failed_container_entries()
            .get(failed_index)
            .map(|failed| &failed.entry))
    }

    /// Render one mesh asset as GLB bytes.
    ///
    /// Mirrors `AssetAPI.GetModelData`: `GlbMeshBuilder.Build` reads the mesh, then
    /// `GlbWriter.TryWrite` serializes it. The frozen route answers `model/gltf-binary` with the
    /// download name `model.glb` regardless of the asset's own name.
    ///
    /// A mesh that cannot be read or built reports the failure rather than serving a partial
    /// file, matching the frozen route's `404 Model data could not be decoded.`
    pub fn render_asset_model(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<Option<Vec<u8>>, String> {
        use assetripper::resources::glb::build_glb;
        use assetripper::resources::mesh::MeshView;
        use assetripper::resources::packed_bit_vector::PackedBitVectorLimits;

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let Ok(payload) = object.payload.as_ref() else {
            return Ok(None);
        };
        let file = game
            .workspace
            .files()
            .get(file_index)
            .ok_or_else(|| "the snapshot row has no workspace file".to_owned())?;
        let version = file
            .metadata
            .prefix
            .unity_version_text
            .as_deref()
            .and_then(assetripper::resources::vertex_data_blob::UnityVersion::parse)
            .unwrap_or(assetripper::resources::vertex_data_blob::UnityVersion {
                major: 5,
                minor: 0,
                patch: 0,
            });
        let endian = match file.header.data_endian {
            Some(assetripper::io::binary::Endian::Big) => {
                assetripper::resources::vertex_data_blob::EndianType::Big
            }
            _ => assetripper::resources::vertex_data_blob::EndianType::Little,
        };
        let view = match MeshView::from_value(&payload.root, version, endian) {
            Ok(view) => view,
            // A payload that is not a readable mesh is "no model data", not a server error.
            Err(_) => return Ok(None),
        };
        let Some(mesh_data) = view
            .read_mesh_data(PackedBitVectorLimits::default())
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let name = mesh_name(&payload.root).unwrap_or_else(|| object.factory.name.clone());
        match build_glb(&name, &mesh_data) {
            Ok(exported) => Ok(Some(exported.bytes)),
            Err(error) => Err(error.to_string()),
        }
    }

    /// Read one video clip's streamed content with the extension the frozen route reports.
    ///
    /// Mirrors `AssetAPI.GetVideoData`: `VideoClipExtensions.TryGetExtensionFromPath` names the
    /// container from the clip's `m_OriginalPath`, and `TryGetContent` reads the
    /// `m_ExternalResources` range. Both must succeed, so a clip without a path extension or
    /// without readable content reports no video rather than serving bytes under a guess.
    pub fn read_asset_video(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<Option<VideoData>, String> {
        use assetripper::resources::video_clip::VideoClipView;

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let Ok(payload) = object.payload.as_ref() else {
            return Ok(None);
        };
        let Ok(view) = VideoClipView::from_value(&payload.root) else {
            return Ok(None);
        };
        // `TryGetExtensionFromPath` is one of the two gates the frozen route requires.
        let Some(extension) = view.try_get_extension_from_path() else {
            return Ok(None);
        };
        let extension = extension.to_owned();
        let Some(info) = view
            .external_resources
            .as_ref()
            .filter(|info| info.is_set())
        else {
            return Ok(None);
        };
        let file = game
            .workspace
            .files()
            .get(file_index)
            .ok_or_else(|| "the snapshot row has no workspace file".to_owned())?;
        use assetripper::io::platform_structure::PlatformResourceProvider;
        let provider =
            PlatformResourceProvider::with_mixed_data_paths(None, Some(&game.data_paths));
        let resolution = assetripper::resource::resolve_and_read_streamed_resource_range(
            &game.filesystem,
            &game.workspace,
            file.container_bundle_index,
            assetripper::resource::StreamedResourceRange {
                path: &info.path,
                offset: info.offset,
                size: u64::from(info.size),
            },
            &game.root_catalog,
            &provider,
            assetripper::resource::StreamedResourceContentLimits::default(),
        )
        .map_err(|error| error.to_string())?;
        match resolution {
            assetripper::resource::StreamedResourceContentResolution::Resolved {
                bytes, ..
            } => Ok(Some(VideoData {
                bytes: bytes.to_vec(),
                extension,
            })),
            _ => Ok(None),
        }
    }

    /// Read one font asset's binary data with the extension and MIME the frozen route reports.
    ///
    /// Mirrors `AssetAPI.TryGetFontData`: the payload's `m_FontData` is read, and the first four
    /// bytes select the extension and MIME. Data shorter than four bytes counts as no font data.
    pub fn read_asset_font(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<Option<FontData>, String> {
        use assetripper::export::resource_extensions::{font_data_bytes, font_extension_and_mime};

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let Ok(payload) = object.payload.as_ref() else {
            return Ok(None);
        };
        let Some(data) = font_data_bytes(&payload.root).map_err(|error| error.to_string())? else {
            return Ok(None);
        };
        Ok(
            font_extension_and_mime(&data).map(|(extension, mime)| FontData {
                bytes: data,
                extension,
                mime,
            }),
        )
    }

    /// Decode one audio clip's playable bytes.
    ///
    /// Mirrors the frozen `AssetAPI.GetAudioData`: the clip's raw audio is read the way
    /// `AudioClipExtensions.GetAudioData` reads it (nonempty inline bytes, then the streamed
    /// resource), `AudioClipDecoder.TryDecode` classifies it, and an Ogg result is transcoded to
    /// WAV when the caller asks for that extension. Returns the bytes plus the served extension.
    ///
    /// A clip whose audio cannot be decoded reports the decoder's error rather than serving
    /// container bytes as audio.
    pub fn read_asset_audio(
        &self,
        file_index: usize,
        object_index: usize,
        desired_extension: Option<&str>,
    ) -> Result<Option<(Vec<u8>, &'static str)>, String> {
        use assetripper::export::audio_export::{
            convert_ogg_to_wav_with_limit, converts_ogg_to_wav, try_decode_audio,
            MAX_DECODED_AUDIO_BYTES,
        };
        use assetripper::resources::audio::AudioClipView;

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let Ok(payload) = object.payload.as_ref() else {
            return Ok(None);
        };
        let Ok(view) = AudioClipView::from_value(&payload.root) else {
            return Ok(None);
        };
        let name = view
            .name
            .as_deref()
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default();
        // `GetAudioData` source order: nonempty inline bytes, then the streamed resource.
        let inline = view
            .audio_data
            .as_deref()
            .filter(|bytes| !bytes.is_empty())
            .map(<[u8]>::to_vec);
        let raw = match inline {
            Some(bytes) => bytes,
            None => match view.resource.as_ref().filter(|info| info.is_set()) {
                Some(info) => {
                    let file = game
                        .workspace
                        .files()
                        .get(file_index)
                        .ok_or_else(|| "the snapshot row has no workspace file".to_owned())?;
                    use assetripper::io::platform_structure::PlatformResourceProvider;
                    let provider = PlatformResourceProvider::with_mixed_data_paths(
                        None,
                        Some(&game.data_paths),
                    );
                    let resolution =
                        assetripper::resource::resolve_and_read_streamed_resource_range(
                            &game.filesystem,
                            &game.workspace,
                            file.container_bundle_index,
                            assetripper::resource::StreamedResourceRange {
                                path: &info.path,
                                offset: info.offset,
                                size: info.size,
                            },
                            &game.root_catalog,
                            &provider,
                            assetripper::resource::StreamedResourceContentLimits::default(),
                        )
                        .map_err(|error| error.to_string())?;
                    match resolution {
                        assetripper::resource::StreamedResourceContentResolution::Resolved {
                            bytes,
                            ..
                        } => bytes.to_vec(),
                        _ => return Ok(None),
                    }
                }
                None => return Ok(None),
            },
        };
        let (decoded, extension) =
            try_decode_audio(&name, view.sound_type, &raw).map_err(|error| error.to_string())?;
        if converts_ogg_to_wav(extension, desired_extension.unwrap_or("")) {
            // A failed transcode keeps the Ogg bytes, matching the frozen route, which only
            // swaps the result in when the converter produced a non-empty WAV.
            if let Ok(wav) = convert_ogg_to_wav_with_limit(&decoded, MAX_DECODED_AUDIO_BYTES) {
                if !wav.is_empty() {
                    return Ok(Some((wav, "wav")));
                }
            }
        }
        Ok(Some((decoded, extension)))
    }

    /// Render one asset's decoded payload as a standalone Unity YAML document.    ///
    /// The frozen route walks the asset with `YamlWalker` and anchors the document with
    /// `ExportIdHandler.GetMainExportID(asset)`. This port converts the already-decoded value
    /// through the same object-document path the project export uses, so the tab shows the same
    /// text the exporter would write, anchored with the class-prefixed main export id.
    ///
    /// References resolve to null: the frozen standalone walker has no project export map, so a
    /// PPtr has no target to name. That matches the route rather than degrading it.
    pub fn render_asset_yaml(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<String, String> {
        use assetripper::export::ids::get_main_export_id;
        use assetripper::export::pointer_map::ProjectPointerResolution;
        use assetripper::export::scene_collection::{
            ExportAssetCollectionIdentity, ExportAssetIdentity,
        };
        use assetripper::export::unity_yaml::{write_unity_yaml_documents, UnityYamlWriteLimits};
        use assetripper::export::value_yaml::{
            serialized_object_value_to_unity_yaml_document, UnityYamlObjectConversionHints,
            UnityYamlObjectConversionLimits, UnityYamlObjectFieldMode,
        };

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let payload = object
            .payload
            .as_ref()
            .map_err(|error| error.to_string())?
            .clone();
        let file = game
            .workspace
            .files()
            .get(file_index)
            .ok_or_else(|| "the snapshot row has no workspace file".to_owned())?;
        let unity_version =
            parse_scene_unity_version(file.metadata.prefix.unity_version_text.as_deref());

        // The asset's own TypeTree is the schema the value was decoded with, so its hints are
        // the right ones. An asset decoded from a class layout instead has no file tree; default
        // hints still emit every field.
        let limits = UnityYamlObjectConversionLimits::default();
        let file_tree = object
            .serialized_type_index
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| file.metadata.types.get(index))
            .and_then(|serialized_type| serialized_type.type_tree.as_ref());
        let hints = match file_tree {
            Some(tree) => {
                UnityYamlObjectConversionHints::from_type_tree(tree, unity_version, limits)
                    .map_err(|error| error.to_string())?
            }
            None => UnityYamlObjectConversionHints::default(),
        };
        let export_id =
            get_main_export_id(object.factory.class_id, 0).map_err(|error| error.to_string())?;
        let document = serialized_object_value_to_unity_yaml_document(
            ExportAssetIdentity {
                collection: ExportAssetCollectionIdentity::ProcessedCollection {
                    collection_index: 0,
                },
                object_index,
                path_id: object.factory.path_id,
                class_id: object.factory.class_id,
            },
            &payload.type_name,
            0,
            export_id,
            false,
            &payload.root,
            &hints,
            UnityYamlObjectFieldMode::ProjectAsset,
            unity_version,
            None,
            limits,
            |_pointer| ProjectPointerResolution::Null,
        )
        .map_err(|error| error.to_string())?;
        write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default())
            .map_err(|error| error.to_string())
    }

    /// Render one asset's decoded payload as JSON.
    ///
    /// The frozen `DefaultJsonWalker` writes JSON during traversal; this port builds the same
    /// text from the decoded value, which carries the same ordered fields and PPtr shapes. An
    /// asset with no decoded payload reads as an empty string, matching the frozen route's
    /// empty reply for assets it cannot walk.
    pub fn render_asset_json(
        &self,
        file_index: usize,
        object_index: usize,
    ) -> Result<String, String> {
        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let Ok(payload) = object.payload.as_ref() else {
            return Ok(String::new());
        };
        let mut output = String::new();
        // The reference's `/Assets/Json` route renders the asset through `DefaultJsonWalker` over a
        // `StringWriter` whose `NewLine` it sets to `"\n"` explicitly, so the response is the
        // indented walker text with LF line breaks on every platform. Emitting the compact form
        // here (`write_json_value`) produced a body that parsed the same but did not match the
        // reference's text at all.
        let mut walker = assetripper::export::json_walker::JsonWalker::with_line_break("\n");
        assetripper::export::json_walker::write_json_body(&mut walker, &payload.root);
        output.push_str(walker.as_str());
        Ok(output)
    }

    /// Render one texture asset to an encoded image.
    ///
    /// The requested format comes from the route's `Extension` parameter; an unrecognized
    /// extension falls back to PNG, matching the frozen route's default.
    pub fn render_texture_image(
        &self,
        file_index: usize,
        object_index: usize,
        format: assetripper::resources::image_export::ImageExportFormat,
    ) -> Result<Vec<u8>, String> {
        use assetripper::resources::texture::Texture2DView;

        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let object = game
            .objects
            .get(file_index, object_index)
            .ok_or_else(|| "the snapshot row has no workspace object".to_owned())?;
        let payload = object.payload.as_ref().map_err(|error| error.to_string())?;
        let view = Texture2DView::from_value(&payload.root).map_err(|error| error.to_string())?;
        let streamed = self.read_asset_image_bytes(file_index, object_index)?;
        let unity_version = game
            .workspace
            .files()
            .get(file_index)
            .and_then(|file| file.metadata.prefix.unity_version_text.as_deref())
            .and_then(UnityVersion::parse);
        assetripper::resources::texture::export_texture2d_image_with_unity_version(
            &view,
            streamed.as_deref(),
            0,
            false,
            true,
            unity_version,
            format,
        )
        .map_err(|error| error.to_string())
    }

    /// The maximum Unity version text over the loaded files, matching
    /// `GameBundle.GetMaxUnityVersion` as far as the port's version type reaches.
    pub fn max_unity_version_text(&self) -> Option<String> {
        let game = self.loaded.as_ref()?;
        let mut best: Option<(UnityVersion, String)> = None;
        for file in game.workspace.files() {
            let Some(text) = file.metadata.prefix.unity_version_text.as_deref() else {
                continue;
            };
            let Some(version) = UnityVersion::parse(text) else {
                continue;
            };
            if best.as_ref().is_none_or(|(current, _)| version > *current) {
                best = Some((version, text.to_owned()));
            }
        }
        best.map(|(_, text)| text)
    }

    /// Log lines produced since the last [`WebState::take_log`].
    pub fn take_log(&mut self) -> Vec<String> {
        core::mem::take(&mut self.log)
    }

    /// Switch language like `Localization.LoadLanguage`, returning whether it changed.
    pub fn set_language(&mut self, requested: Option<&str>) -> bool {
        match decide_load_language(&self.language, requested) {
            LoadLanguageOutcome::Switched { code } => {
                self.language = code;
                true
            }
            LoadLanguageOutcome::Unchanged => false,
        }
    }

    /// Persist or delete the settings file according to `SaveSettingsToDisk`.
    pub fn apply_settings_persistence(&mut self) -> Result<SettingsPersistenceAction, String> {
        match decide_settings_persistence(self.settings.export.save_settings_to_disk) {
            SettingsPersistenceAction::SaveToDefaultPath => {
                save_serialized_settings_to_dir(&self.settings, &self.executable_directory)
                    .map_err(|error| error.to_string())?;
                Ok(SettingsPersistenceAction::SaveToDefaultPath)
            }
            SettingsPersistenceAction::DeleteDefaultPath => {
                let deleted = delete_serialized_settings_in_dir(&self.executable_directory)
                    .map_err(|error| error.to_string())?;
                if deleted {
                    self.log
                        .push("Deleted the stored settings file.".to_owned());
                }
                Ok(SettingsPersistenceAction::DeleteDefaultPath)
            }
            SettingsPersistenceAction::LeaveStoredFile => {
                Ok(SettingsPersistenceAction::LeaveStoredFile)
            }
        }
    }

    /// Record a failure the way the baseline's error middleware reports one.
    pub fn log_error(&mut self, message: &str) {
        self.log.push(message.to_owned());
    }

    /// `MaybeSaveToDefaultPath`: write the settings file only when `SaveSettingsToDisk` is set.
    pub fn maybe_save_settings(&mut self) -> Result<(), String> {
        if !self.settings.export.save_settings_to_disk {
            return Ok(());
        }
        save_serialized_settings_to_dir(&self.settings, &self.executable_directory)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    /// `GameFileLoader.LoadAndProcess`: reset, then mount and decode every submitted path.
    pub fn load(&mut self, paths: &[String]) -> Result<(), String> {
        self.reset();
        if paths.is_empty() {
            return Err("no input path was provided".to_owned());
        }
        let path_bufs: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let mut status = Vec::new();
        let game = load_game(&path_bufs, &self.settings, &mut status)
            .map_err(|error| error.to_string())?;
        self.push_status(&status);
        let tree =
            BundleTree::from_loaded(&game.workspace, &game.objects, game.root_catalog.files())
                .map_err(|error| error.to_string())?;
        self.log
            .push(format!("Loaded {} input path(s).", paths.len()));
        self.loaded = Some(game);
        self.tree = Some(tree);
        Ok(())
    }

    /// `GameFileLoader.Reset`: drop the loaded game, logging only when one was present.
    pub fn reset(&mut self) -> bool {
        let had_data = self.loaded.take().is_some();
        self.tree = None;
        self.scene_definitions = None;
        if had_data {
            self.log.push("Data was reset.".to_owned());
        }
        had_data
    }

    /// The workspace's scene definitions, computed on first use and cached.
    ///
    /// The definitions come from the scene-definition pass, which scans every collection and object
    /// once. The reference holds the result on the game data; the port caches it here so a page view
    /// does not repeat the scan.
    ///
    /// `None` means there is no loaded game, or the pass could not run - the caller reports that as an
    /// unresolvable scene rather than an empty one.
    pub fn ensure_scene_definitions(&mut self) -> bool {
        if self.scene_definitions.is_none() {
            let Some(game) = self.loaded.as_ref() else {
                return false;
            };
            let Ok(references) = game
                .workspace
                .reference_graph(MultiFileReferenceGraphLimits::default())
            else {
                return false;
            };
            let Ok(processing) = process_workspace_scenes_and_prefabs(
                &game.filesystem,
                &game.workspace,
                &game.objects,
                &references,
                WorkspaceSceneProcessingLimits::default(),
            ) else {
                return false;
            };
            self.scene_definitions = Some(processing.definitions);
        }
        true
    }

    /// The cached scene definitions, after [`WebState::ensure_scene_definitions`].
    pub fn scene_definitions(&self) -> Option<&WorkspaceSceneDefinitions> {
        self.scene_definitions.as_ref()
    }

    /// The named configuration lists, for the configuration-files handlers.
    pub fn list_data(&mut self) -> &mut ListData {
        &mut self.list_data
    }

    /// The named configuration texts, for reading.
    pub fn singletons(&self) -> &SingletonData {
        &self.singleton_data
    }

    /// The named configuration lists, for reading.
    pub fn lists(&self) -> &ListData {
        &self.list_data
    }

    /// The export gate for one submitted path, with the browser confirmation result.
    pub fn plan_export(&self, path: &str, confirmed: bool) -> ExportDecision {
        if path.is_empty() {
            return ExportDecision::Skip;
        }
        let directory_valid = is_valid_export_directory(path);
        let non_empty = assetripper::export::export_root::is_non_empty_directory(Path::new(path));
        let consent = deletion_consent(self.headless, if confirmed { Some(true) } else { None });
        match resolve_export_flow(self.loaded.is_some(), directory_valid, non_empty, consent) {
            ExportFlow::AbortedNotLoaded => ExportDecision::Skip,
            ExportFlow::AbortedInvalidDirectory => ExportDecision::InvalidDirectory,
            ExportFlow::AbortedDeclined => ExportDecision::NeedsConfirmation,
            ExportFlow::ProceedDeleteFirst => ExportDecision::Proceed {
                delete_existing: true,
            },
            ExportFlow::ProceedCreateOnly => ExportDecision::Proceed {
                delete_existing: false,
            },
        }
    }

    /// Apply `CreateSubfolder` to a submitted export path.
    pub fn resolve_export_path(
        &self,
        path: &str,
        create_subfolder: bool,
        timestamp: &str,
    ) -> String {
        maybe_append_timestamped_subfolder(path, create_subfolder, timestamp)
    }

    /// Export the loaded game as a Unity project.
    pub fn export_unity_project(
        &mut self,
        path: &str,
        delete_existing: bool,
    ) -> Result<(), String> {
        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let mut status = Vec::new();
        export_loaded_game(
            game,
            Path::new(path),
            delete_existing,
            &self.settings,
            self.singleton_data
                .get("EngineResourceData")
                .map(String::as_str),
            &mut status,
            ProjectExportOutputLayout::AssetRipperExportRoot,
        )
        .map_err(|error| error.to_string())?;
        self.push_status(&status);
        Ok(())
    }

    /// Export the loaded game's primary content.
    ///
    /// The gate is the caller's, as it is for the Unity-project export: `GameFileLoader` applies
    /// `IsLoaded && IsValidExportDirectory`, asks for consent on a non-empty directory, deletes and
    /// recreates it, and only then exports. This mirrors that order.
    pub fn export_primary_content(
        &mut self,
        path: &str,
        delete_existing: bool,
    ) -> Result<(), String> {
        let game = self
            .loaded
            .as_ref()
            .ok_or_else(|| "no game is loaded".to_owned())?;
        let mut status = Vec::new();
        export_loaded_primary_content(
            game,
            Path::new(path),
            delete_existing,
            &self.settings,
            &mut status,
        )
        .map_err(|error| error.to_string())?;
        self.push_status(&status);
        Ok(())
    }

    fn push_status(&mut self, status: &[u8]) {
        let text = String::from_utf8_lossy(status);
        for line in text.lines().filter(|line| !line.is_empty()) {
            self.log.push(line.to_owned());
        }
    }
}

/// The Unity version fields for the YAML rules, falling back when the file declares none.
///
/// The frozen `YamlWalker` reads the asset's collection version, which a SerializedFile always
/// carries. A file whose version text is absent or unparseable still has to render, so the
/// oldest supported shape is used rather than refusing to serve the tab.
fn parse_scene_unity_version(
    text: Option<&str>,
) -> assetripper::processing::scene_naming::SceneUnityVersion {
    use assetripper::processing::scene_naming::SceneUnityVersion;

    text.and_then(SceneUnityVersion::parse)
        .unwrap_or(SceneUnityVersion {
            major: 5,
            minor: 0,
            patch: 0,
        })
}

/// Applies the nullable JSON serializer behavior for EngineResourceData values whose resource
/// dictionaries are empty. Non-empty dictionaries remain as submitted so project export can
/// decode their typed cache entries.
fn normalize_empty_engine_resource_data_json(content: &str) -> Option<String> {
    use serde_json::{Map, Value};

    let value = match serde_json::from_str::<Value>(content) {
        Ok(value) => value,
        // JsonDataSerializer catches malformed input and resets a nullable value to null.
        Err(_) => return Some(String::new()),
    };
    let Some(object) = value.as_object() else {
        // A scalar or array cannot deserialize as EngineResourceData and also resets to null.
        return Some(String::new());
    };

    let mut normalized = Map::new();
    for key in ["DefaultResources", "ExtraResources"] {
        let entries = match object.get(key) {
            None | Some(Value::Null) => Value::Object(Map::new()),
            Some(Value::Object(entries)) if entries.is_empty() => Value::Object(Map::new()),
            Some(Value::Object(_)) => return None,
            Some(_) => {
                // A field with the wrong JSON type makes JsonSerializer.Deserialize fail; the
                // source serializer then creates the nullable default value.
                return Some(String::new());
            }
        };
        normalized.insert(key.to_owned(), entries);
    }

    let json = serde_json::to_string_pretty(&Value::Object(normalized)).ok()?;
    let newline = if cfg!(windows) { "\r\n" } else { "\n" };
    Some(json.replace('\n', newline))
}

#[cfg(test)]
mod tests {
    use super::super::localization::{Messages, FALLBACK_LANGUAGE};
    use super::{ExportDecision, SerializedSettingsFile, WebState};
    use assetripper::export::settings::{
        ExportDefaults, ImportDefaults, ProcessingDefaults, ScriptContentLevel, StreamingAssetsMode,
    };
    use std::path::PathBuf;

    fn state(headless: bool) -> WebState {
        let settings = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        WebState::with_settings(
            Messages::load().expect("tables load"),
            settings,
            PathBuf::from("/tmp/assetripper-test"),
            headless,
        )
        .expect("state builds")
    }

    #[test]
    fn startup_state_matches_the_launcher_defaults() {
        let state = state(false);
        assert!(!state.is_loaded());
        assert!(!state.headless());
        assert_eq!(state.language(), FALLBACK_LANGUAGE);
        assert!(!state.settings().export.save_settings_to_disk);
        assert!(state.loaded().is_none());
    }

    #[test]
    fn configuration_files_start_with_the_source_singletons_in_insertion_order() {
        let state = state(false);
        assert_eq!(
            state
                .singletons()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "ImportSettings",
                "ProcessingSettings",
                "ExportSettings",
                "EngineResourceData"
            ]
        );
        assert_eq!(
            state.singletons().get("EngineResourceData"),
            Some(&String::new())
        );
        assert!(state.lists().is_empty());
    }

    #[test]
    fn engine_resource_data_json_matches_nullable_and_empty_dictionary_text() {
        let mut state = state(false);
        for content in ["", "null", "not-json", "[]", r#"{"DefaultResources":[]}"#] {
            state
                .set_singleton_data("EngineResourceData", content)
                .expect("nullable JSON settings accept malformed text by resetting to default");
            assert_eq!(
                state.singletons().get("EngineResourceData"),
                Some(&String::new()),
                "content {content:?} resets to a null EngineResourceData"
            );
        }

        let newline = if cfg!(windows) { "\r\n" } else { "\n" };
        let expected = [
            "{",
            "  \"DefaultResources\": {},",
            "  \"ExtraResources\": {}",
            "}",
        ]
        .join(newline);
        for content in [
            "{}",
            r#"{"DefaultResources":null,"ExtraResources":{}}"#,
            r#"{"DefaultResources":{},"ExtraResources":{}}"#,
        ] {
            state
                .set_singleton_data("EngineResourceData", content)
                .expect("empty resource dictionaries parse");
            assert_eq!(
                state.singletons().get("EngineResourceData"),
                Some(&expected),
                "content {content:?} serializes with both source dictionary defaults"
            );
        }

        state
            .clear_singleton_data("EngineResourceData")
            .expect("the typed nullable entry clears");
        assert_eq!(
            state.singletons().get("EngineResourceData"),
            Some(&String::new())
        );
    }

    #[test]
    fn web_project_export_uses_source_roots_and_stored_textasset_engine_resource_data() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../processing/asset_factory_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|textasset_notree|"))
            .expect("source fixture includes a no-TypeTree TextAsset");
        let source_bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let directory = test_directory("engine-resource-textasset");
        let source_path = directory.join("textasset_notree.assets");
        std::fs::write(&source_path, &source_bytes).expect("write generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load source-generated TextAsset");

        let embedded_output = directory.join("project-embedded");
        let embedded_output_text = embedded_output.to_string_lossy().into_owned();
        state
            .export_unity_project(&embedded_output_text, false)
            .expect("the default embedded TPK cache exports the TextAsset");
        let embedded_project = embedded_output.join("ExportedProject");
        assert_eq!(
            std::fs::read(embedded_project.join("Assets/TextAsset/Hero.txt"))
                .expect("embedded cache leaves a nonmatching TextAsset exportable"),
            b"print(1)"
        );
        assert!(embedded_project
            .join("ProjectSettings/ProjectVersion.txt")
            .is_file());
        assert!(embedded_project.join("Packages/manifest.json").is_file());
        assert!(embedded_output
            .join("AuxiliaryFiles/path_id_map.json")
            .is_file());
        assert!(!embedded_output.join("Assets").exists());
        assert!(!embedded_project
            .join("AuxiliaryFiles/path_id_map.json")
            .exists());

        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"TextAsset","Name":"Hero","Length":8,"Hash":130469715505528193672212848297838865176}},"ExtraResources":{}}"#,
            )
            .expect("store source-shaped custom TextAsset resource data");
        let custom_output = directory.join("project-custom");
        let custom_output_text = custom_output.to_string_lossy().into_owned();
        state
            .export_unity_project(&custom_output_text, false)
            .expect("the custom cache redirects a matching TextAsset");
        let custom_project = custom_output.join("ExportedProject");
        assert!(
            !custom_project.join("Assets/TextAsset/Hero.txt").exists(),
            "a TextAsset matched by name and UInt128 MD5 is redirected"
        );
        assert!(!custom_project
            .join("Assets/TextAsset/Hero.txt.meta")
            .exists());
        assert!(custom_output
            .join("AuxiliaryFiles/path_id_map.json")
            .is_file());
        assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    }

    #[test]
    fn web_project_export_redirects_stored_texture2d_engine_resource_data() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../export/texture_project_source_2022_3_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|TextureFixture.assets|"))
            .expect("source fixture includes the Texture2D SerializedFile");
        let source_bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let directory = test_directory("engine-resource-texture2d");
        let source_path = directory.join("TextureFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load source-generated Texture2D");
        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Texture2D","Name":"TextureFixture","Width":2,"Height":2}},"ExtraResources":{}}"#,
            )
            .expect("store source-shaped custom Texture2D resource data");

        let output_root = directory.join("project-custom");
        let output_root_text = output_root.to_string_lossy().into_owned();
        state
            .export_unity_project(&output_root_text, false)
            .expect("the custom cache redirects a matching Texture2D");

        let project = output_root.join("ExportedProject");
        assert!(
            !project.join("Assets").exists(),
            "a Texture2D matched by name and dimensions is redirected"
        );
        assert!(project.join("ProjectSettings/ProjectVersion.txt").is_file());
        assert!(project.join("Packages/manifest.json").is_file());
        let path_map: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap(),
        )
        .expect("path ID map is valid JSON");
        assert_eq!(path_map["Files"][0]["Assets"][0]["PathID"], 44);
        assert_eq!(path_map["Files"][0]["Assets"][0]["Name"], "TextureFixture");
        assert_eq!(path_map["Files"][0]["Assets"][0]["Type"], "Texture2D");
        assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    }

    #[test]
    fn web_project_export_redirects_stored_cubemap_engine_resource_data() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../export/cubemap_project_source_2022_3_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|TextureFixture.assets|"))
            .expect("source fixture includes the Cubemap SerializedFile");
        let source_bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let directory = test_directory("engine-resource-cubemap");
        let source_path = directory.join("TextureFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write generated Cubemap fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load source-generated Cubemap");
        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Cubemap","Name":"TextureFixture","Width":2,"Height":2}},"ExtraResources":{}}"#,
            )
            .expect("store source-shaped custom Cubemap resource data");

        let output_root = directory.join("project-custom");
        let output_root_text = output_root.to_string_lossy().into_owned();
        state
            .export_unity_project(&output_root_text, false)
            .expect("the custom cache redirects a matching Cubemap");

        let project = output_root.join("ExportedProject");
        assert!(
            !project.join("Assets").exists(),
            "a Cubemap matched by name and dimensions is redirected"
        );
        assert!(project.join("ProjectSettings/ProjectVersion.txt").is_file());
        assert!(project.join("Packages/manifest.json").is_file());
        let path_map: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap(),
        )
        .expect("path ID map is valid JSON");
        assert_eq!(path_map["Files"][0]["Assets"][0]["PathID"], 44);
        assert_eq!(path_map["Files"][0]["Assets"][0]["Name"], "TextureFixture");
        assert_eq!(path_map["Files"][0]["Assets"][0]["Type"], "Cubemap");
        assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    }

    #[test]
    fn web_project_export_redirects_stored_audioclip_engine_resource_data() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../export/audio_project_stream_source_2019_4_fixture.txt");

        fn fixture_bytes(fixture: &str, prefix: &str) -> Vec<u8> {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("AudioClip fixture is missing {prefix}"));
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                        .expect("fixture hex is valid")
                })
                .collect()
        }

        let source_bytes = fixture_bytes(fixture, "INPUT_HEX|AudioFixture.assets|");
        let resource_bytes = fixture_bytes(fixture, "INPUT_HEX|AudioFixture.resS|");
        let directory = test_directory("engine-resource-audio-clip");
        let input_root = directory.join("audio-input");
        std::fs::create_dir_all(&input_root).expect("create AudioClip fixture directory");
        let source_path = input_root.join("AudioFixture.assets");
        let resource_path = input_root.join("AudioFixture.resS");
        std::fs::write(&source_path, &source_bytes).expect("write generated AudioClip fixture");
        std::fs::write(&resource_path, &resource_bytes).expect("write generated resource sidecar");

        let input_root_text = input_root.to_string_lossy().into_owned();
        let mut state = state(true);
        state
            .load(&[input_root_text])
            .expect("load source-generated AudioClip and its resource sidecar");
        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"AudioClip","Name":"AudioFixture","Channels":0,"Frequency":0,"Length":0.0}},"ExtraResources":{}}"#,
            )
            .expect("store source-shaped custom AudioClip resource data");

        let output_root = directory.join("project-custom");
        let output_root_text = output_root.to_string_lossy().into_owned();
        state
            .export_unity_project(&output_root_text, false)
            .expect("the custom cache redirects a matching AudioClip");

        let project = output_root.join("ExportedProject");
        assert!(
            !project.join("Assets").exists(),
            "an AudioClip matched by name, channels, frequency, and length is redirected"
        );
        assert!(project.join("ProjectSettings/ProjectVersion.txt").is_file());
        assert!(project.join("Packages/manifest.json").is_file());
        let path_map: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap(),
        )
        .expect("path ID map is valid JSON");
        assert_eq!(path_map["Files"][0]["Assets"][0]["PathID"], 303);
        assert_eq!(path_map["Files"][0]["Assets"][0]["Name"], "AudioFixture");
        assert_eq!(path_map["Files"][0]["Assets"][0]["Type"], "AudioClip");
        assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
        assert_eq!(std::fs::read(&resource_path).unwrap(), resource_bytes);
    }

    fn source_material_shader_fixture_bytes() -> Vec<u8> {
        let fixture = include_str!("../export/material_engine_resource_project_2019_4_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|MaterialFixture.assets|"))
            .expect("source fixture includes the Material and Shader SerializedFile");
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect()
    }

    #[test]
    fn web_project_export_redirects_stored_material_engine_resource_data() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let source_bytes = source_material_shader_fixture_bytes();
        let directory = test_directory("engine-resource-material");
        let source_path = directory.join("MaterialFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load source-generated Material and Shader");
        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Material","Name":"MaterialFixture","Shader":"ShaderFixture"}},"ExtraResources":{}}"#,
            )
            .expect("store source-shaped custom Material resource data");

        let output_root = directory.join("project-custom");
        let output_root_text = output_root.to_string_lossy().into_owned();
        state
            .export_unity_project(&output_root_text, false)
            .expect("the custom cache redirects a matching Material");

        let project = output_root.join("ExportedProject");
        assert!(
            !project.join("Assets/Material/MaterialFixture.mat").exists(),
            "a Material matched by name and resolved shader name is redirected"
        );
        assert!(project.join("Assets/Shader/ShaderFixture.shader").is_file());
        assert!(project.join("ProjectSettings/ProjectVersion.txt").is_file());
        assert!(project.join("Packages/manifest.json").is_file());
        let path_map: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output_root.join("AuxiliaryFiles/path_id_map.json")).unwrap(),
        )
        .expect("path ID map is valid JSON");
        assert!(path_map["Files"][0]["Assets"]
            .as_array()
            .is_some_and(Vec::is_empty));
        assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    }

    #[test]
    fn web_project_export_redirects_stored_shader_engine_resource_data() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let source_bytes = source_material_shader_fixture_bytes();
        let directory = test_directory("engine-resource-shader");
        let source_path = directory.join("MaterialFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load source-generated Shader and Material");
        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Shader","Name":"ShaderFixture","PropertyNames":[]}},"ExtraResources":{}}"#,
            )
            .expect("store source-shaped custom Shader resource data");

        let matching_output = directory.join("project-matching");
        let matching_output_text = matching_output.to_string_lossy().into_owned();
        state
            .export_unity_project(&matching_output_text, false)
            .expect("the custom cache redirects a matching Shader");
        let matching_project = matching_output.join("ExportedProject");
        assert!(!matching_project
            .join("Assets/Shader/ShaderFixture.shader")
            .exists());
        assert!(matching_project
            .join("Assets/Material/MaterialFixture.mat")
            .is_file());

        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Shader","Name":"ShaderFixture","PropertyNames":["DifferentProperty"]}},"ExtraResources":{}}"#,
            )
            .expect("store a mismatching Shader property list");
        let mismatching_output = directory.join("project-mismatching");
        let mismatching_output_text = mismatching_output.to_string_lossy().into_owned();
        state
            .export_unity_project(&mismatching_output_text, false)
            .expect("a Shader with different properties remains a normal export");
        assert!(mismatching_output
            .join("ExportedProject/Assets/Shader/ShaderFixture.shader")
            .is_file());
    }

    #[test]
    fn web_project_export_redirects_stored_mesh_and_matches_5_6_texture_yaml() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../export/lightmap_project_source_5_6_meshfilter_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|LightmapFixture.assets|"))
            .expect("source fixture includes its SerializedFile input");
        let source_bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let directory = test_directory("engine-resource-mesh");
        let source_path = directory.join("LightmapFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write source-generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load source-generated MeshFilter and Mesh");
        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Mesh","Name":"LightmappedMesh","VertexCount":0,"SubMeshCount":0,"LocalAABB":{"Center":{"X":1,"Y":0,"Z":0},"Extent":{"X":0,"Y":0,"Z":0}}}},"ExtraResources":{}}"#,
            )
            .expect("store source-shaped Mesh resource data");

        let matching_output = directory.join("project-matching");
        let matching_output_text = matching_output.to_string_lossy().into_owned();
        state
            .export_unity_project(&matching_output_text, false)
            .expect("a matching stored Mesh redirects to the configured builtin identity");
        let matching_project = matching_output.join("ExportedProject");
        assert!(!matching_project
            .join("Assets/Mesh/LightmappedMesh.asset")
            .exists());
        let matching_texture = std::fs::read_to_string(
            matching_project.join("Assets/Scenes/lightmapfixture.assets/LightmapFixture.texture2D"),
        )
        .expect("Texture2D YAML is exported beside the Scene");
        assert_eq!(
            matching_texture,
            include_str!("../export/lightmap_project_web_texture2d_source_5_6.yaml"),
            "Unity 5.6 Texture2D YAML differs from frozen source"
        );
        let matching_scene = std::fs::read_to_string(
            matching_project.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .expect("scene YAML is exported");
        assert!(matching_scene
            .contains("m_Mesh: {fileID: 777, guid: 0000000000000000e000000000000000, type: 0}"));

        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Mesh","Name":"LightmappedMesh","VertexCount":1,"SubMeshCount":0,"LocalAABB":{"Center":{"X":1,"Y":0,"Z":0},"Extent":{"X":0,"Y":0,"Z":0}}}},"ExtraResources":{}}"#,
            )
            .expect("store a Mesh resource with a mismatching vertex count");
        let mismatching_output = directory.join("project-mismatching");
        let mismatching_output_text = mismatching_output.to_string_lossy().into_owned();
        state
            .export_unity_project(&mismatching_output_text, false)
            .expect("a nonmatching stored Mesh remains normally exportable");
        let mismatching_project = mismatching_output.join("ExportedProject");
        let mesh_asset = mismatching_project.join("Assets/Mesh/LightmappedMesh.asset");
        assert!(mesh_asset.is_file());
        let mesh_meta = std::fs::read_to_string(mesh_asset.with_extension("asset.meta"))
            .expect("normal Mesh export has metadata");
        let mesh_guid = mesh_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("Mesh metadata contains its generated GUID");
        let mismatching_scene = std::fs::read_to_string(
            mismatching_project.join("Assets/Scenes/lightmapfixture.assets.unity"),
        )
        .expect("scene YAML is exported for the cache miss");
        assert!(mismatching_scene.contains(&format!(
            "m_Mesh: {{fileID: 4300000, guid: {mesh_guid}, type: 2}}"
        )));
    }

    #[test]
    fn web_project_export_matches_2019_4_texture2d_yaml() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../export/lightmap_project_source_2019_4_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|LightmapFixture.assets|"))
            .expect("source fixture includes its SerializedFile input");
        let source_bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let directory = test_directory("texture-yaml-2019-4");
        let source_path = directory.join("LightmapFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write source-generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load the source-generated Lightmap Texture2D");
        let output = directory.join("project");
        state
            .export_unity_project(&output.to_string_lossy(), false)
            .expect("export the project through the Web default settings");

        let texture_path = output
            .join("ExportedProject/Assets/Scenes/lightmapfixture.assets/LightmapFixture.texture2D");
        let texture = std::fs::read_to_string(&texture_path)
            .expect("the Web YAML route exports the Lightmap Texture2D");
        assert_eq!(
            texture,
            include_str!("../export/lightmap_project_web_texture2d_source_2019_4.yaml"),
            "Unity 2019.4 Texture2D YAML differs from frozen source"
        );
    }

    #[test]
    fn web_project_export_matches_2022_3_texture2d_yaml() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../export/lightmap_project_source_2022_3_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|LightmapFixture.assets|"))
            .expect("source fixture includes its SerializedFile input");
        let source_bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let directory = test_directory("texture-yaml-2022-3");
        let source_path = directory.join("LightmapFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write source-generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load the source-generated Lightmap Texture2D");
        let output = directory.join("project");
        state
            .export_unity_project(&output.to_string_lossy(), false)
            .expect("export the project through the Web default settings");

        let texture_path = output
            .join("ExportedProject/Assets/Scenes/lightmapfixture.assets/LightmapFixture.texture2D");
        let texture = std::fs::read_to_string(&texture_path)
            .expect("the Web YAML route exports the Lightmap Texture2D");
        assert_eq!(
            texture,
            include_str!("../export/lightmap_project_web_texture2d_source_2022_3.yaml"),
            "Unity 2022.3 Texture2D YAML differs from frozen source"
        );
    }

    #[test]
    fn web_project_export_applies_stored_sprite_texture_group_gate() {
        let _source_layouts = crate::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("../export/sprite_atlas_project_source_2019_4_fixture.txt");
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|SpriteAtlasFixture.assets|"))
            .expect("source fixture includes its SerializedFile input");
        let source_bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let directory = test_directory("engine-resource-sprite");
        let source_path = directory.join("SpriteAtlasFixture.assets");
        std::fs::write(&source_path, &source_bytes).expect("write source-generated fixture");
        let source_path_text = source_path.to_string_lossy().into_owned();

        let mut state = state(true);
        state
            .load(&[source_path_text])
            .expect("load source-generated Texture2D, Sprite, and SpriteAtlas");
        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"778":{"$type":"Sprite","Name":"PackedSpriteFixture","Texture":"AtlasTextureFixture"}},"ExtraResources":{}}"#,
            )
            .expect("store a Sprite key without its required texture-group key");
        let sprite_only_output = directory.join("project-sprite-only");
        state
            .export_unity_project(&sprite_only_output.to_string_lossy(), false)
            .expect("a Sprite key without a matching texture cache stays on the normal route");
        let sprite_only_project = sprite_only_output.join("ExportedProject");
        assert!(sprite_only_project
            .join("Assets/Sprite/PackedSpriteFixture.asset")
            .is_file());
        assert!(
            std::fs::read_dir(sprite_only_project.join("Assets/Texture2D"))
                .expect("an uncached Texture2D remains exportable")
                .next()
                .is_some()
        );

        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Texture2D","Name":"AtlasTextureFixture","Width":64,"Height":48}},"ExtraResources":{}}"#,
            )
            .expect("store the SpriteInformationObject texture-group key");
        let texture_only_output = directory.join("project-texture-only");
        state
            .export_unity_project(&texture_only_output.to_string_lossy(), false)
            .expect("a texture cache hit suppresses unmatched Sprite group members");
        let texture_only_project = texture_only_output.join("ExportedProject");
        assert!(!texture_only_project
            .join("Assets/Sprite/PackedSpriteFixture.asset")
            .exists());
        assert!(!texture_only_project
            .join("Assets/Texture2D/AtlasTextureFixture.png")
            .exists());
        assert!(!texture_only_project
            .join("Assets/Texture2D/AtlasTextureFixture.bmp")
            .exists());

        state
            .set_singleton_data(
                "EngineResourceData",
                r#"{"DefaultResources":{"777":{"$type":"Texture2D","Name":"AtlasTextureFixture","Width":64,"Height":48},"778":{"$type":"Sprite","Name":"PackedSpriteFixture","Texture":"AtlasTextureFixture"}},"ExtraResources":{}}"#,
            )
            .expect("store matching Texture2D and Sprite cache keys");
        let matching_output = directory.join("project-matching");
        state
            .export_unity_project(&matching_output.to_string_lossy(), false)
            .expect("a matching Sprite key redirects inside its cached texture group");
        let matching_project = matching_output.join("ExportedProject");
        assert!(!matching_project
            .join("Assets/Sprite/PackedSpriteFixture.asset")
            .exists());
        assert!(!matching_project
            .join("Assets/Texture2D/AtlasTextureFixture.png")
            .exists());
        assert!(!matching_project
            .join("Assets/Texture2D/AtlasTextureFixture.bmp")
            .exists());
    }

    #[test]
    fn typed_configuration_singletons_update_and_reset_the_live_settings() {
        let mut state = state(false);
        state
            .set_singleton_data(
                "ImportSettings",
                r#"{"ScriptContentLevel":0,"StreamingAssetsMode":0,"DefaultVersion":"2019.4.40f1","TargetVersion":"2022.3.22f1"}"#,
            )
            .expect("valid typed setting parses");
        assert_eq!(
            state.settings().import.script_content_level,
            ScriptContentLevel::Level0
        );
        assert_eq!(
            state.settings().import.streaming_assets_mode,
            StreamingAssetsMode::Ignore
        );
        assert_eq!(
            state.settings().import.default_version.as_str(),
            "2019.4.40f1"
        );
        assert_eq!(
            state.settings().import.target_version.as_str(),
            "2022.3.22f1"
        );
        assert!(state
            .singletons()
            .get("ImportSettings")
            .expect("setting stays registered")
            .contains(r#""ScriptContentLevel": 0"#));

        state
            .clear_singleton_data("ImportSettings")
            .expect("typed setting resets");
        assert_eq!(
            state.settings().import.script_content_level,
            ScriptContentLevel::Level2
        );
        assert_eq!(
            state.settings().import.streaming_assets_mode,
            StreamingAssetsMode::Extract
        );
    }

    #[test]
    fn language_switching_follows_the_load_language_rules() {
        let mut state = state(false);
        assert!(!state.set_language(None));
        assert!(!state.set_language(Some(FALLBACK_LANGUAGE)));
        assert!(!state.set_language(Some("xx-YY")));
        assert!(state.set_language(Some("de")));
        assert_eq!(state.language(), "de");
        // The underscored file stem is normalized before the switch.
        assert!(state.set_language(Some("zh_Hans")));
        assert_eq!(state.language(), "zh-Hans");
        assert!(!state.set_language(Some("zh-Hans")));
    }

    #[test]
    fn reset_reports_whether_data_was_present() {
        let mut state = state(false);
        state.list_data().add_range("Keep", ["value"]);
        let singleton_data = state.singletons().clone();
        assert!(!state.reset());
        assert!(state.take_log().is_empty());
        assert_eq!(state.singletons(), &singleton_data);
        assert_eq!(state.lists().get("Keep"), Some(&vec!["value".to_owned()]));
    }

    #[test]
    fn loading_without_paths_is_rejected_without_touching_state() {
        let mut state = state(false);
        let error = state.load(&[]).unwrap_err();
        assert_eq!(error, "no input path was provided");
        assert!(!state.is_loaded());
    }

    #[test]
    fn export_gates_mirror_the_baseline_flow() {
        let state_without_game = state(false);
        assert_eq!(
            state_without_game.plan_export("/tmp/export", true),
            ExportDecision::Skip
        );
        // The empty path short-circuits before any directory check.
        assert_eq!(
            state_without_game.plan_export("", false),
            ExportDecision::Skip
        );
    }

    #[test]
    fn subfolder_timestamp_uses_the_shared_helper() {
        let state = state(false);
        assert_eq!(
            state.resolve_export_path("/tmp/out", false, "20260101_000000"),
            "/tmp/out"
        );
        assert_eq!(
            state.resolve_export_path("/tmp/out", true, "20260101_000000"),
            "/tmp/out/AssetRipper_export_20260101_000000"
        );
    }

    /// A writable directory beside the test binary, unique to `name`.
    fn test_directory(name: &str) -> PathBuf {
        let mut directory = std::env::current_exe().expect("test executable path");
        directory.pop();
        directory.push(format!("web-{name}-test"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    #[test]
    fn settings_persistence_writes_and_deletes_the_default_path() {
        let directory = test_directory("settings");
        let settings = SerializedSettingsFile::from_defaults(
            ImportDefaults::default(),
            ProcessingDefaults::default(),
            ExportDefaults::default(),
        );
        let mut state = WebState::with_settings(
            Messages::load().expect("tables load"),
            settings,
            directory.clone(),
            false,
        )
        .expect("state builds");
        state.settings_mut().export.save_settings_to_disk = true;
        state.apply_settings_persistence().expect("save");
        let written = directory.join("AssetRipper.Settings.json");
        assert!(written.exists(), "{} should exist", written.display());
        state.settings_mut().export.save_settings_to_disk = false;
        state.apply_settings_persistence().expect("delete");
        assert!(!written.exists());
        // `MaybeSaveToDefaultPath` writes only when the flag is set and never deletes.
        state.maybe_save_settings().expect("no-op");
        assert!(!written.exists());
        state.settings_mut().export.save_settings_to_disk = true;
        state.maybe_save_settings().expect("save");
        assert!(written.exists());
        state.settings_mut().export.save_settings_to_disk = false;
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn static_content_overrides_are_keyed_by_url_path() {
        let mut state = state(false);
        assert!(state.static_content("/css/site.css").is_none());
        state.set_static_content(vec![("/css/site.css".to_owned(), b"x".to_vec())]);
        assert_eq!(state.static_content("/css/site.css"), Some(b"x".as_slice()));
        assert!(state.static_content("/js/site.js").is_none());
    }
}
