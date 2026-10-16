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

use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
use assetripper::export::settings_file::{
    decide_settings_persistence, SerializedSettingsFile, SettingsPersistenceAction,
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
        Ok(Self {
            messages,
            settings,
            executable_directory,
            headless,
            language,
            loaded: None,
            tree: None,
            log: Vec::new(),
            static_content: Vec::new(),
        })
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
        assetripper::export::json_walker::write_json_value(&mut output, &payload.root);
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
        if had_data {
            self.log.push("Data was reset.".to_owned());
        }
        had_data
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

#[cfg(test)]
mod tests {
    use super::super::localization::{Messages, FALLBACK_LANGUAGE};
    use super::{ExportDecision, SerializedSettingsFile, WebState};
    use assetripper::export::settings::{ExportDefaults, ImportDefaults, ProcessingDefaults};
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
        assert!(!state.reset());
        assert!(state.take_log().is_empty());
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
