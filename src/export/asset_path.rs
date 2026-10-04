//! Export path routing and unique-name rules used by AssetRipper's Unity project collections.
//!
//! Every rule here is ported from the frozen baseline:
//!
//! - `IUnityObjectBase.GetBestDirectory`, `GetBestName`, and `GetBestExtension`
//! - `ExportCollection.GetUniqueFileName`
//! - `FileSystem.GetUniqueName`, `RemoveCloneSuffixes`, `RemoveInstanceSuffixes`,
//!   `FixInvalidFileNameCharacters`, `FixInvalidPathCharacters`, and `IsReservedName`
//! - `Utf8Truncation.TruncateToUTF8ByteLength`
//!
//! The baseline resolves names against a live filesystem and keeps an in-memory counter cache.
//! This port keeps that behaviour but expresses it as a pure function over an
//! [`ExportFileSystem`] probe, so the collision rules can be tested without touching the disk.
//!
//! `ExportCollection.GetExportExtension` is not ported here: it selects an extension by generated
//! marker interface, which needs the generated class schemas this crate does not have yet. The
//! caller supplies the extension string and `DEFAULT_ASSET_EXTENSION` documents the source default.

use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// `FileSystem.ActualMaxFileNameLength` minus the reserved conflict-handling characters.
pub const MAX_FILE_NAME_LENGTH: usize = 245;
/// `ExportCollection.MetaExtension`.
pub const META_EXTENSION: &str = ".meta";
/// `ExportCollection.MetaExtension.Length`.
pub const META_EXTENSION_LENGTH: usize = 5;
/// `FileSystem.MaxFileNameLength - ExportCollection.MetaExtension.Length`.
pub const MAX_ASSET_FILE_NAME_LENGTH: usize = MAX_FILE_NAME_LENGTH - META_EXTENSION_LENGTH;
/// `ExportCollection.AssetExtension`, the fallback extension of `GetExportExtension`.
pub const DEFAULT_ASSET_EXTENSION: &str = "asset";

/// Host path conventions the baseline inherits from `System.IO.Path`.
///
/// The invalid-character sets and the reserved-name check are platform dependent in the source,
/// so the port selects them explicitly instead of depending on the running operating system.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostPathPlatform {
    Windows,
    Unix,
}

impl HostPathPlatform {
    /// The `Path.GetInvalidFileNameChars()` set plus the baseline's always-added characters.
    ///
    /// The baseline appends `:` when the host set does not already contain it, and its regular
    /// expression always adds `,`, `[`, `]`, and `U+0000..=U+001F`.
    fn invalid_file_name_characters(self) -> &'static [char] {
        match self {
            Self::Windows => &['"', '<', '>', '|', ':', ',', '[', ']'],
            Self::Unix => &['/', ':', ',', '[', ']'],
        }
    }

    /// `Path.GetInvalidFileNameChars()` minus `\\` and `/`, plus the source's `,`, `[`, `]`.
    ///
    /// The baseline does not add `:` to this set, so a colon survives `FixInvalidPathCharacters`.
    fn invalid_path_characters(self) -> &'static [char] {
        match self {
            Self::Windows => &['"', '<', '>', '|', ',', '[', ']'],
            Self::Unix => &[',', '[', ']'],
        }
    }

    /// `PathInternal.IsDirectorySeparator`.
    fn is_directory_separator(self, value: char) -> bool {
        match self {
            Self::Windows => value == '/' || value == '\\',
            Self::Unix => value == '/',
        }
    }

    /// The character `Path.Join` inserts between two non-empty parts.
    fn separator(self) -> char {
        match self {
            Self::Windows => '\\',
            Self::Unix => '/',
        }
    }
}

/// Bounds applied to the unbounded source loops in `FileSystem.GetUniqueName`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UniqueNameLimits {
    /// Maximum number of `File.Exists` probes for one name.
    pub max_counter_probes: u64,
    /// Maximum number of cached initial-path counters, mirroring `UniqueNamesByInitialPath`.
    pub max_cached_keys: usize,
}

impl Default for UniqueNameLimits {
    fn default() -> Self {
        Self {
            max_counter_probes: 1_000_000,
            max_cached_keys: 1_000_000,
        }
    }
}

/// Error returned when the source name rules cannot produce a unique file name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetPathError {
    /// The extension alone does not fit into the configured name budget.
    ExtensionBudgetExceeded {
        extension_byte_count: usize,
        max_name_length: usize,
    },
    /// The conflict counter was probed more often than the caller allows.
    CounterProbeLimitExceeded { limit: u64 },
    /// The conflict-counter cache cannot accept another entry.
    CounterCacheLimitExceeded { limit: usize },
    /// The source loops over the whole positive `int` counter range without a free name.
    CounterSpaceExhausted,
    /// A bounded allocation for an export name could not be reserved.
    AllocationFailed { resource: &'static str },
}

impl Display for AssetPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExtensionBudgetExceeded {
                extension_byte_count,
                max_name_length,
            } => write!(
                formatter,
                "export extension needs {extension_byte_count} bytes but the file name budget is \
                 {max_name_length} bytes"
            ),
            Self::CounterProbeLimitExceeded { limit } => write!(
                formatter,
                "export file name conflict resolution exceeded {limit} existence probes"
            ),
            Self::CounterCacheLimitExceeded { limit } => write!(
                formatter,
                "export file name conflict cache exceeded {limit} entries"
            ),
            Self::CounterSpaceExhausted => formatter
                .write_str("no unique export file name exists in the conflict counter range"),
            Self::AllocationFailed { resource } => {
                write!(formatter, "could not allocate {resource}")
            }
        }
    }
}

impl Error for AssetPathError {}

/// The export path identity of one asset, matching `IUnityObjectBase`'s path properties.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExportAssetPathIdentity {
    /// `IUnityObjectBase.ClassName`; the source guarantees it is not empty.
    pub class_name: String,
    /// `INamed.Name` for assets that implement `INamed`.
    pub named_name: Option<String>,
    /// `IUnityObjectBase.OriginalDirectory`.
    pub original_directory: Option<String>,
    /// `IUnityObjectBase.OriginalName`.
    pub original_name: Option<String>,
    /// `IUnityObjectBase.OriginalExtension`.
    pub original_extension: Option<String>,
    /// `IUnityObjectBase.OverrideDirectory`.
    pub override_directory: Option<String>,
    /// `IUnityObjectBase.OverrideName`.
    pub override_name: Option<String>,
    /// `IUnityObjectBase.OverrideExtension`.
    pub override_extension: Option<String>,
}

impl ExportAssetPathIdentity {
    /// `IUnityObjectBase.GetBestDirectory`.
    ///
    /// The fallback allocates because the source concatenates `"Assets/" + ClassName`.
    pub fn best_directory(&self) -> Cow<'_, str> {
        if self.override_directory.is_some() || self.override_name.is_some() {
            Cow::Borrowed(self.override_directory.as_deref().unwrap_or("Assets"))
        } else if self.original_directory.is_some() || self.original_name.is_some() {
            Cow::Borrowed(self.original_directory.as_deref().unwrap_or("Assets"))
        } else {
            let mut value = String::with_capacity("Assets/".len() + self.class_name.len());
            value.push_str("Assets/");
            value.push_str(&self.class_name);
            Cow::Owned(value)
        }
    }

    /// `IUnityObjectBase.GetBestName`.
    pub fn best_name(&self) -> &str {
        if let Some(name) = &self.override_name {
            name
        } else if let Some(name) = &self.named_name {
            // `Utf8String.IsNullOrEmpty` treats a null and an empty value alike.
            if !name.is_empty() {
                name
            } else {
                self.original_name.as_deref().unwrap_or(&self.class_name)
            }
        } else {
            self.original_name.as_deref().unwrap_or(&self.class_name)
        }
    }

    /// `IUnityObjectBase.GetBestExtension`.
    pub fn best_extension(&self) -> Option<&str> {
        self.override_extension
            .as_deref()
            .or(self.original_extension.as_deref())
    }
}

/// Filesystem probe used by the source's `Directory.Exists` and `File.Exists` checks.
pub trait ExportFileSystem {
    /// `Directory.Exists(directory)`.
    fn directory_exists(&self, directory: &str) -> bool;

    /// `File.Exists(Path.Join(directory, name))`.
    fn file_exists(&self, directory: &str, name: &str) -> bool;
}

/// The `UniqueNamesByInitialPath` counter cache of one export run.
#[derive(Debug)]
pub struct UniqueNameState {
    limits: UniqueNameLimits,
    counters: HashMap<String, i64>,
}

impl Default for UniqueNameState {
    fn default() -> Self {
        Self::new(UniqueNameLimits::default())
    }
}

impl UniqueNameState {
    /// Create a cache using the default bounds.
    pub fn new(limits: UniqueNameLimits) -> Self {
        Self {
            limits,
            counters: HashMap::new(),
        }
    }

    /// The counter the source dictionary would report for one initial path.
    pub fn cached_counter(&self, initial_path: &str) -> Option<i64> {
        self.counters.get(initial_path).copied()
    }

    /// Number of cached initial paths.
    pub fn cached_key_count(&self) -> usize {
        self.counters.len()
    }
}

/// `FileSystem.RemoveCloneSuffixes`, which removes every occurrence.
pub fn remove_clone_suffixes(value: &str) -> String {
    value.replace("(Clone)", "")
}

/// `FileSystem.RemoveInstanceSuffixes`, which removes every occurrence.
pub fn remove_instance_suffixes(value: &str) -> String {
    value.replace("(Instance)", "")
}

/// `FileSystem.FixInvalidFileNameCharacters`.
pub fn fix_invalid_file_name_characters(
    value: &str,
    platform: HostPathPlatform,
) -> Result<String, AssetPathError> {
    replace_invalid(value, platform.invalid_file_name_characters())
}

/// `FileSystem.FixInvalidPathCharacters`, including its `TrimEntries` helper.
pub fn fix_invalid_path_characters(
    value: &str,
    platform: HostPathPlatform,
) -> Result<String, AssetPathError> {
    trim_entries(replace_invalid(value, platform.invalid_path_characters())?)
}

fn replace_invalid(value: &str, invalid: &[char]) -> Result<String, AssetPathError> {
    let mut result = String::new();
    result
        .try_reserve(value.len())
        .map_err(|_| AssetPathError::AllocationFailed {
            resource: "export file name",
        })?;
    for character in value.chars() {
        // The source regular expression always covers U+0000..=U+001F.
        let is_control = (character as u32) <= 0x1F;
        if is_control || invalid.contains(&character) {
            result.push('_');
        } else {
            result.push(character);
        }
    }
    Ok(result)
}

/// The `TrimEntries` helper of `FixInvalidPathCharacters`.
fn trim_entries(value: String) -> Result<String, AssetPathError> {
    if !(value.contains(" /") || value.contains("/ ")) {
        return Ok(value.trim().to_owned());
    }
    let mut joined = String::new();
    joined
        .try_reserve(value.len())
        .map_err(|_| AssetPathError::AllocationFailed {
            resource: "export path",
        })?;
    for entry in value.split('/').filter(|entry| !entry.trim().is_empty()) {
        if !joined.is_empty() {
            joined.push('/');
        }
        joined.push_str(entry.trim());
    }
    Ok(joined)
}

/// `FileSystem.IsReservedName`, which the baseline applies on Windows only.
pub fn is_reserved_name(name: &str, platform: HostPathPlatform) -> bool {
    if platform != HostPathPlatform::Windows {
        return false;
    }
    let lowercase = name.to_lowercase();
    match lowercase.chars().count() {
        3 => matches!(lowercase.as_str(), "aux" | "con" | "nul" | "prn"),
        4 => {
            let bytes = lowercase.as_bytes();
            bytes[0] == b'c'
                && bytes[1] == b'o'
                && bytes[2] == b'm'
                && matches!(bytes[3], b'1'..=b'9')
                || bytes[0] == b'l'
                    && bytes[1] == b'p'
                    && bytes[2] == b't'
                    && matches!(bytes[3], b'1'..=b'9')
        }
        _ => false,
    }
}

/// `Path.GetExtension` for the supplied platform.
pub fn get_extension(value: &str, platform: HostPathPlatform) -> &str {
    let bytes = value.as_bytes();
    for index in (0..bytes.len()).rev() {
        let character = bytes[index] as char;
        if character == '.' {
            return if index + 1 != bytes.len() {
                &value[index..]
            } else {
                ""
            };
        }
        if platform.is_directory_separator(character) {
            break;
        }
    }
    ""
}

/// `Path.GetFileNameWithoutExtension` for the supplied platform.
pub fn get_file_name_without_extension(value: &str, platform: HostPathPlatform) -> &str {
    let bytes = value.as_bytes();
    let mut start = 0;
    for index in (0..bytes.len()).rev() {
        if platform.is_directory_separator(bytes[index] as char) {
            start = index + 1;
            break;
        }
    }
    let name = &value[start..];
    match name.rfind('.') {
        Some(period) => &name[..period],
        None => name,
    }
}

/// `Path.Join(path1, path2)` for the supplied platform.
pub fn join_path(first: &str, second: &str, platform: HostPathPlatform) -> String {
    if first.is_empty() {
        return second.to_owned();
    }
    if second.is_empty() {
        return first.to_owned();
    }
    let separator = platform.separator();
    let first_is_separated = first.ends_with(separator);
    let mut joined = String::with_capacity(first.len() + second.len() + 1);
    joined.push_str(first);
    if !first_is_separated {
        joined.push(separator);
    }
    let mut tail = second;
    while let Some(rest) = tail.strip_prefix(separator) {
        tail = rest;
    }
    joined.push_str(tail);
    joined
}

/// `Utf8Truncation.TruncateToUTF8ByteLength`.
pub fn truncate_to_utf8_byte_length(value: &str, max_length: usize) -> String {
    let bytes = value.as_bytes();
    let valid = find_valid_byte_length(bytes, max_length);
    String::from_utf8_lossy(&bytes[..valid]).into_owned()
}

/// `Utf8Truncation.FindValidByteLength`.
fn find_valid_byte_length(bytes: &[u8], max_length: usize) -> usize {
    if max_length >= bytes.len() {
        return bytes.len();
    }
    let mut valid_length = max_length;
    // A non-continuation byte at the cut means every sequence before the cut is complete.
    if bytes[max_length] & 0b1100_0000 != 0b1000_0000 {
        return valid_length;
    }
    for index in (0..max_length).rev() {
        let current = bytes[index];
        if current & 0b1100_0000 == 0b1000_0000 {
            // Continuation byte.
            valid_length -= 1;
        } else if current & 0b1000_0000 == 0b1000_0000 {
            // Start of a sequence that the cut would halve.
            valid_length -= 1;
            break;
        } else {
            // ASCII byte.
            break;
        }
    }
    valid_length
}

/// `FileSystem.GetUniqueName`.
pub fn get_unique_name(
    state: &mut UniqueNameState,
    platform: HostPathPlatform,
    file_system: &impl ExportFileSystem,
    directory: &str,
    file_name: &str,
    max_name_length: usize,
) -> Result<String, AssetPathError> {
    // The source assigns `name` and `ext` only when truncation happens and reuses them later.
    let mut truncated_stem: Option<String> = None;
    let mut truncated_extension: Option<String> = None;
    let valid_file_name = if file_name.len() > max_name_length {
        let extension = get_extension(file_name, platform);
        let extension_byte_count = extension.len();
        if extension_byte_count > max_name_length {
            // The source subtracts the extension first and throws inside the decoder.
            return Err(AssetPathError::ExtensionBudgetExceeded {
                extension_byte_count,
                max_name_length,
            });
        }
        let stem = truncate_to_utf8_byte_length(file_name, max_name_length - extension_byte_count);
        truncated_extension = Some(extension.to_owned());
        let combined = owned_concat(stem.as_str(), extension, "export file name")?;
        truncated_stem = Some(stem);
        combined
    } else {
        file_name.to_owned()
    };

    if !file_system.directory_exists(directory) {
        return Ok(valid_file_name);
    }

    let stem = match &truncated_stem {
        Some(stem) => stem.clone(),
        None => get_file_name_without_extension(&valid_file_name, platform).to_owned(),
    };
    let extension = match &truncated_extension {
        Some(extension) => extension.as_str(),
        None => get_extension(&valid_file_name, platform),
    };

    if !is_reserved_name(&stem, platform) && !file_system.file_exists(directory, &valid_file_name) {
        return Ok(valid_file_name);
    }

    let key = join_path(
        directory,
        &owned_concat(&stem, extension, "export name key")?,
        platform,
    );
    let mut counter = state.cached_counter(&key).unwrap_or(0);
    let mut probes: u64 = 0;
    while counter < i64::from(i32::MAX) {
        probes += 1;
        if probes > state.limits.max_counter_probes {
            return Err(AssetPathError::CounterProbeLimitExceeded {
                limit: state.limits.max_counter_probes,
            });
        }
        let suffix = owned_concat("_", &counter.to_string(), "export conflict suffix")?;
        let candidate = owned_concat(
            &owned_concat(&stem, &suffix, "export conflict name")?,
            extension,
            "export conflict name",
        )?;
        if !file_system.file_exists(directory, &candidate) {
            if state.counters.len() >= state.limits.max_cached_keys
                && !state.counters.contains_key(&key)
            {
                return Err(AssetPathError::CounterCacheLimitExceeded {
                    limit: state.limits.max_cached_keys,
                });
            }
            state.counters.insert(key, counter);
            return Ok(candidate);
        }
        counter += 1;
    }
    Err(AssetPathError::CounterSpaceExhausted)
}

/// `ExportCollection.GetUniqueFileName` for one asset and one `GetExportExtension` result.
pub fn get_unique_file_name(
    identity: &ExportAssetPathIdentity,
    extension: &str,
    platform: HostPathPlatform,
    state: &mut UniqueNameState,
    file_system: &impl ExportFileSystem,
    directory: &str,
) -> Result<String, AssetPathError> {
    let stem = remove_clone_suffixes(identity.best_name());
    let stem = remove_instance_suffixes(&stem);
    let trimmed = stem.trim();
    let mut file_name = if trimmed.is_empty() {
        // The source uses the class name verbatim, without character fixing.
        identity.class_name.clone()
    } else {
        fix_invalid_file_name_characters(trimmed, platform)?
    };
    file_name.reserve(1 + extension.len());
    file_name.push('.');
    file_name.push_str(extension);
    get_unique_name(
        state,
        platform,
        file_system,
        directory,
        &file_name,
        MAX_ASSET_FILE_NAME_LENGTH,
    )
}

fn owned_concat(
    first: &str,
    second: &str,
    resource: &'static str,
) -> Result<String, AssetPathError> {
    let mut value = String::new();
    value
        .try_reserve(first.len() + second.len())
        .map_err(|_| AssetPathError::AllocationFailed { resource })?;
    value.push_str(first);
    value.push_str(second);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{
        find_valid_byte_length, fix_invalid_file_name_characters, fix_invalid_path_characters,
        get_extension, get_file_name_without_extension, get_unique_file_name, get_unique_name,
        is_reserved_name, join_path, remove_clone_suffixes, remove_instance_suffixes,
        truncate_to_utf8_byte_length, AssetPathError, ExportAssetPathIdentity, ExportFileSystem,
        HostPathPlatform, UniqueNameLimits, UniqueNameState, MAX_ASSET_FILE_NAME_LENGTH,
        MAX_FILE_NAME_LENGTH,
    };
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct TestFileSystem {
        directories: BTreeSet<String>,
        files: BTreeSet<String>,
    }

    impl TestFileSystem {
        fn joined(&self, directory: &str, name: &str) -> String {
            join_path(directory, name, HostPathPlatform::Windows)
        }

        fn add_directory(&mut self, directory: &str) -> &mut Self {
            self.directories.insert(directory.to_owned());
            self
        }

        fn add_file(&mut self, directory: &str, name: &str) -> &mut Self {
            let path = self.joined(directory, name);
            self.files.insert(path);
            self
        }
    }

    impl ExportFileSystem for TestFileSystem {
        fn directory_exists(&self, directory: &str) -> bool {
            self.directories.contains(directory)
        }

        fn file_exists(&self, directory: &str, name: &str) -> bool {
            self.files.contains(&self.joined(directory, name))
        }
    }

    fn identity(class_name: &str) -> ExportAssetPathIdentity {
        ExportAssetPathIdentity {
            class_name: class_name.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn best_path_properties_follow_the_source_preference_order() {
        let mut asset = identity("Mesh");
        assert_eq!(asset.best_directory(), "Assets/Mesh");
        assert_eq!(asset.best_name(), "Mesh");
        assert_eq!(asset.best_extension(), None);

        asset.original_extension = Some("fbx".to_owned());
        assert_eq!(asset.best_extension(), Some("fbx"));
        asset.override_extension = Some("asset".to_owned());
        assert_eq!(asset.best_extension(), Some("asset"));

        asset.named_name = Some(String::new());
        asset.original_name = Some("Cube.fbx".to_owned());
        assert_eq!(asset.best_name(), "Cube.fbx");
        assert_eq!(asset.best_directory(), "Assets");

        asset.named_name = Some("Cube(Clone)".to_owned());
        assert_eq!(asset.best_name(), "Cube(Clone)");

        asset.original_directory = Some("Assets/Models".to_owned());
        assert_eq!(asset.best_directory(), "Assets/Models");

        asset.override_name = Some("Renamed".to_owned());
        assert_eq!(asset.best_name(), "Renamed");
        assert_eq!(asset.best_directory(), "Assets");

        asset.override_directory = Some("Assets/Override".to_owned());
        assert_eq!(asset.best_directory(), "Assets/Override");
    }

    #[test]
    fn best_name_falls_back_through_original_name_to_class_name() {
        let mut asset = identity("Texture2D");
        assert_eq!(asset.best_name(), "Texture2D");
        asset.original_name = Some("Icon".to_owned());
        assert_eq!(asset.best_name(), "Icon");
        asset.named_name = Some("Icon2".to_owned());
        assert_eq!(asset.best_name(), "Icon2");
        asset.named_name = Some(String::new());
        assert_eq!(asset.best_name(), "Icon");
        // The baseline returns `OriginalName` when it is non-null, even when empty; the
        // interface documents that it is never empty, so an empty value is preserved here
        // instead of falling back to the class name.
        asset.original_name = Some(String::new());
        assert_eq!(asset.best_name(), "");
        asset.original_name = None;
        asset.named_name = None;
        assert_eq!(asset.best_name(), "Texture2D");
    }

    #[test]
    fn clone_and_instance_suffixes_are_removed_everywhere_in_the_name() {
        assert_eq!(remove_clone_suffixes("Cube(Clone)"), "Cube");
        assert_eq!(
            remove_clone_suffixes("Cube(Clone)(Clone)_a(Clone)"),
            "Cube_a"
        );
        assert_eq!(remove_instance_suffixes("A(Instance)B"), "AB");
        assert_eq!(remove_clone_suffixes("Clone"), "Clone");
    }

    #[test]
    fn invalid_name_characters_follow_the_platform_sets() {
        assert_eq!(
            fix_invalid_file_name_characters("a<b>c|d:e,f[g]h\ti", HostPathPlatform::Windows),
            Ok("a_b_c_d_e_f_g_h_i".to_owned())
        );
        assert_eq!(
            fix_invalid_file_name_characters("a<b>c:d[e]", HostPathPlatform::Unix),
            Ok("a<b>c_d_e_".to_owned())
        );
        assert_eq!(
            fix_invalid_path_characters("Assets\\Meshes/a<b>[c]", HostPathPlatform::Windows),
            Ok("Assets\\Meshes/a_b__c_".to_owned())
        );
        assert_eq!(
            fix_invalid_path_characters("Assets / Meshes", HostPathPlatform::Windows),
            Ok("Assets/Meshes".to_owned())
        );
    }

    #[test]
    fn path_fixing_keeps_colons_and_trims_entries_like_the_source() {
        assert_eq!(
            fix_invalid_path_characters("Assets:Old/Meshes", HostPathPlatform::Windows),
            Ok("Assets:Old/Meshes".to_owned())
        );
        assert_eq!(
            fix_invalid_path_characters("  Assets/Meshes  ", HostPathPlatform::Windows),
            Ok("Assets/Meshes".to_owned())
        );
        assert_eq!(
            fix_invalid_path_characters("a / b / c", HostPathPlatform::Windows),
            Ok("a/b/c".to_owned())
        );
    }

    #[test]
    fn reserved_names_follow_the_windows_only_source_rule() {
        for name in ["con", "NUL", "Prn", "aux", "COM1", "lpt9"] {
            assert!(is_reserved_name(name, HostPathPlatform::Windows), "{name}");
            assert!(!is_reserved_name(name, HostPathPlatform::Unix), "{name}");
        }
        for name in ["co", "com0", "com10", "lpt", "console", "conx"] {
            assert!(!is_reserved_name(name, HostPathPlatform::Windows), "{name}");
        }
    }

    #[test]
    fn path_helpers_match_the_source_semantics() {
        assert_eq!(get_extension("Icon.png", HostPathPlatform::Windows), ".png");
        assert_eq!(get_extension("Icon", HostPathPlatform::Windows), "");
        assert_eq!(get_extension("Icon.", HostPathPlatform::Windows), "");
        assert_eq!(
            get_extension("a.b/Icon.png", HostPathPlatform::Windows),
            ".png"
        );
        assert_eq!(get_extension("a.b/Icon", HostPathPlatform::Windows), "");
        assert_eq!(
            get_file_name_without_extension("a.b/Icon.png", HostPathPlatform::Windows),
            "Icon"
        );
        assert_eq!(
            get_file_name_without_extension("a.b/Icon", HostPathPlatform::Windows),
            "Icon"
        );
        assert_eq!(
            get_file_name_without_extension("C:\\dir/Icon.png", HostPathPlatform::Windows),
            "Icon"
        );
        assert_eq!(
            get_file_name_without_extension("C:\\dir/Icon", HostPathPlatform::Unix),
            "Icon"
        );
        assert_eq!(
            join_path("Assets", "a.png", HostPathPlatform::Windows),
            "Assets\\a.png"
        );
        assert_eq!(
            join_path("Assets\\", "\\a.png", HostPathPlatform::Windows),
            "Assets\\a.png"
        );
        assert_eq!(join_path("", "a.png", HostPathPlatform::Unix), "a.png");
        assert_eq!(join_path("Assets", "", HostPathPlatform::Unix), "Assets");
    }

    #[test]
    fn utf8_truncation_only_ever_cuts_at_a_sequence_boundary() {
        assert_eq!(truncate_to_utf8_byte_length("abcdef", 3), "abc");
        // 'a' = 0x61, U+03B2 = CE B2, U+03B3 = CE B3, U+03B4 = CE B4.
        let text = "a\u{03b2}\u{03b3}\u{03b4}";
        assert_eq!(text.as_bytes(), [0x61, 0xCE, 0xB2, 0xCE, 0xB3, 0xCE, 0xB4]);
        assert_eq!(truncate_to_utf8_byte_length(text, 3), "a\u{03b2}");
        assert_eq!(truncate_to_utf8_byte_length(text, 4), "a\u{03b2}");
        assert_eq!(truncate_to_utf8_byte_length(text, 5), "a\u{03b2}\u{03b3}");
        assert_eq!(truncate_to_utf8_byte_length(text, 6), "a\u{03b2}\u{03b3}");
        assert_eq!(truncate_to_utf8_byte_length(text, 7), text);
        assert_eq!(truncate_to_utf8_byte_length(text, 64), text);
        assert_eq!(find_valid_byte_length(&[0xF0, 0x9F], 1), 0);
    }

    #[test]
    fn unique_names_skip_conflicts_and_cache_the_source_counter() {
        let mut file_system = TestFileSystem::default();
        file_system
            .add_directory("Assets")
            .add_file("Assets", "Cube.asset");
        let mut state = UniqueNameState::default();

        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Windows,
                &file_system,
                "Assets",
                "Cube.asset",
                240
            ),
            Ok("Cube_0.asset".to_owned())
        );
        assert_eq!(state.cached_counter("Assets\\Cube.asset"), Some(0));

        file_system.add_file("Assets", "Cube_0.asset");
        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Windows,
                &file_system,
                "Assets",
                "Cube.asset",
                240
            ),
            Ok("Cube_1.asset".to_owned())
        );
        assert_eq!(state.cached_counter("Assets\\Cube.asset"), Some(1));
        assert_eq!(state.cached_key_count(), 1);

        // A different directory is tracked separately.
        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Windows,
                &file_system,
                "Assets\\Other",
                "Cube.asset",
                240
            ),
            Ok("Cube.asset".to_owned())
        );
        assert_eq!(state.cached_key_count(), 1);
    }

    #[test]
    fn unique_names_only_treat_reserved_names_as_conflicting_on_windows() {
        let mut file_system = TestFileSystem::default();
        let mut state = UniqueNameState::default();
        file_system
            .add_directory("Assets")
            .add_file("Assets", "con.asset");
        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Unix,
                &file_system,
                "Assets",
                "con.asset",
                240
            ),
            Ok("con_0.asset".to_owned())
        );

        file_system.add_file("Assets", "aux.asset");
        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Windows,
                &file_system,
                "Assets",
                "aux.asset",
                240
            ),
            Ok("aux_0.asset".to_owned())
        );
        assert_eq!(state.cached_key_count(), 2);
    }

    #[test]
    fn unique_names_are_truncated_before_the_directory_check() {
        let file_system = TestFileSystem::default();
        let mut state = UniqueNameState::default();
        let long_name = format!("{}.asset", "a".repeat(300));

        // The directory does not exist, but the source still truncates first: 240 - 6
        // extension bytes leaves 234 stem bytes, all 'a' here.
        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Windows,
                &file_system,
                "Missing",
                &long_name,
                240
            ),
            Ok(format!("{}.asset", "a".repeat(234)))
        );

        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Windows,
                &file_system,
                "Missing",
                "tiny.asset",
                240
            ),
            Ok("tiny.asset".to_owned())
        );
    }

    #[test]
    fn unique_names_report_extension_budget_and_probe_bounds() {
        let mut file_system = TestFileSystem::default();
        file_system
            .add_directory("Assets")
            .add_file("Assets", "Cube.asset")
            .add_file("Assets", "Cube_0.asset")
            .add_file("Assets", "Cube_1.asset");
        let mut state = UniqueNameState::default();

        assert_eq!(
            get_unique_name(
                &mut state,
                HostPathPlatform::Windows,
                &file_system,
                "Assets",
                "Cube.asset",
                3
            ),
            Err(AssetPathError::ExtensionBudgetExceeded {
                extension_byte_count: 6,
                max_name_length: 3,
            })
        );

        let mut bounded = UniqueNameState::new(UniqueNameLimits {
            max_counter_probes: 1,
            max_cached_keys: 1,
        });
        assert_eq!(
            get_unique_name(
                &mut bounded,
                HostPathPlatform::Windows,
                &file_system,
                "Assets",
                "Cube.asset",
                240
            ),
            Err(AssetPathError::CounterProbeLimitExceeded { limit: 1 })
        );

        let mut cache_bounded = UniqueNameState::new(UniqueNameLimits {
            max_counter_probes: 1_000_000,
            max_cached_keys: 0,
        });
        assert_eq!(
            get_unique_name(
                &mut cache_bounded,
                HostPathPlatform::Windows,
                &file_system,
                "Assets",
                "Cube.asset",
                240
            ),
            Err(AssetPathError::CounterCacheLimitExceeded { limit: 0 })
        );
    }

    #[test]
    fn unique_file_names_strip_clone_suffixes_and_fix_characters() {
        let mut file_system = TestFileSystem::default();
        file_system.add_directory("Assets");
        let mut state = UniqueNameState::default();

        let mut asset = identity("Mesh");
        asset.named_name = Some("Cube(Clone)".to_owned());
        assert_eq!(
            get_unique_file_name(
                &asset,
                "asset",
                HostPathPlatform::Windows,
                &mut state,
                &file_system,
                "Assets"
            ),
            Ok("Cube.asset".to_owned())
        );

        asset.named_name = Some("  a<b>(Instance)  ".to_owned());
        assert_eq!(
            get_unique_file_name(
                &asset,
                "asset",
                HostPathPlatform::Windows,
                &mut state,
                &file_system,
                "Assets"
            ),
            Ok("a_b_.asset".to_owned())
        );

        // An empty best name falls back to the class name, which is not character-fixed.
        asset.named_name = Some(String::new());
        assert_eq!(
            get_unique_file_name(
                &asset,
                "asset",
                HostPathPlatform::Windows,
                &mut state,
                &file_system,
                "Assets"
            ),
            Ok("Mesh.asset".to_owned())
        );

        assert_eq!(MAX_ASSET_FILE_NAME_LENGTH, 240);
        assert_eq!(MAX_FILE_NAME_LENGTH, 245);
    }
}
