//! Export file-name planning ported from `AssetRipper.IO.Files.FileSystem`.
//!
//! Covers the filename half of the `.meta` pipeline slice: suffix cleanup, invalid
//! character fixing, reserved-name detection, UTF-8 truncation, extension splitting,
//! and unique-name planning (`GetUniqueName` plus the `GetUniqueFileName` composition
//! behind `AssetExportCollection.Export`/`ExportCollection.ExportAsset`).
//!
//! Host dependence is inherited from the source, which queries the runtime for invalid
//! characters and the OS for reserved names: on Windows the invalid sets contain
//! `"<>| : *?\` and `/` (plus `,[]` and C0 controls on every host); elsewhere only `/`
//! and NUL join the portable additions. `IsReservedName` is Windows-only in the source
//! and reads false everywhere else. The probe ran on Windows, so the `FIXFN`/`FIXPATH`
//! rows pin Windows behavior; OS-independent rows are asserted on every host. Separator
//! handling mirrors Windows (both slashes split); a backslash inside a Unix file name
//! stays open and unprobed.
//!
//! `GetUniqueName` reads the real filesystem in the source; the Rust planner takes
//! caller-supplied existence facts and keeps the per-directory counter map, with the
//! same truncation, reserved-name, and `name_counter.ext` rules. Writing the asset and
//! its `{file}.meta` sidecar stays with the export pipeline (`project_output.rs`,
//! which keeps the add-only refusal where the source overwrites).
//!
//! Differential evidence: `file_names_fixture.txt` (probe `Temp/opencode/filenameprobe`
//! against the shipped `AssetRipper.IO.Files.dll`, including `LocalFileSystem`
//! against real temporary directories, SDK 10.0.401, Windows host).

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display};

/// The filesystem filename limit the source plans against.
pub const ACTUAL_MAX_FILE_NAME_LENGTH: usize = 255;

/// Characters reserved per name for conflict disambiguation (`_NNNNNNNNN`).
pub const RESERVED_FILE_NAME_CHARACTER_COUNT: usize = 10;

/// `FileSystem.MaxFileNameLength`: the usable budget after the reservation.
pub const MAX_FILE_NAME_LENGTH: usize =
    ACTUAL_MAX_FILE_NAME_LENGTH - RESERVED_FILE_NAME_CHARACTER_COUNT;

/// Length of the `.meta` sidecar extension, reserved from the file-name budget by
/// `GetUniqueFileName` callers.
pub const META_EXTENSION_RESERVED_LENGTH: usize = ".meta".len();

/// `FileSystem.RemoveCloneSuffixes`: delete every `(Clone)` marker.
pub fn remove_clone_suffixes(name: &str) -> String {
    name.replace("(Clone)", "")
}

/// `FileSystem.RemoveInstanceSuffixes`: delete every `(Instance)` marker.
pub fn remove_instance_suffixes(name: &str) -> String {
    name.replace("(Instance)", "")
}

fn is_windows_invalid_file_name_char(value: char) -> bool {
    matches!(value, '"' | '<' | '>' | '|' | ':' | '*' | '?' | '\\' | '/')
}

fn is_unix_invalid_file_name_char(value: char) -> bool {
    matches!(value, '/' | '\0')
}

fn host_invalid_file_name_char(value: char) -> bool {
    if cfg!(windows) {
        is_windows_invalid_file_name_char(value)
    } else {
        is_unix_invalid_file_name_char(value)
    }
}

fn is_common_invalid_char(value: char) -> bool {
    // The source regex adds commas, square brackets, and ASCII controls on every host.
    matches!(value, ',' | '[' | ']') || value < '\u{20}'
}

/// Whether `value` is replaced by `FixInvalidFileNameCharacters` on this host.
pub fn is_invalid_file_name_char(value: char) -> bool {
    is_common_invalid_char(value) || host_invalid_file_name_char(value)
}

/// `FileSystem.FixInvalidFileNameCharacters`: replace every invalid character with `_`.
pub fn fix_invalid_file_name_characters(name: &str) -> String {
    name.chars()
        .map(|value| {
            if is_invalid_file_name_char(value) {
                '_'
            } else {
                value
            }
        })
        .collect()
}

fn is_invalid_path_char(value: char) -> bool {
    // The path regex drops the separators from the host set but keeps everything else.
    if matches!(value, '/' | '\\') {
        return false;
    }
    is_common_invalid_char(value) || host_invalid_file_name_char(value)
}

/// `FileSystem.FixInvalidPathCharacters`: replace invalid path characters with `_`,
/// then trim each `/`-separated entry (dropping empties) when any entry has
/// surrounding spaces, else trim the whole path.
pub fn fix_invalid_path_characters(path: &str) -> String {
    let fixed: String = path
        .chars()
        .map(|value| {
            if is_invalid_path_char(value) {
                '_'
            } else {
                value
            }
        })
        .collect();
    if fixed.contains(" /") || fixed.contains("/ ") {
        fixed
            .split('/')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        fixed.trim().to_owned()
    }
}

/// `FileSystem.IsReservedName`: Windows-only exact set of length 3 or 4,
/// matched case-insensitively. Lengths count UTF-16 code units like the source.
pub fn is_reserved_name(name: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }
    if !(name.encode_utf16().count() == 3 || name.encode_utf16().count() == 4) {
        return false;
    }
    const RESERVED: &[&str] = &[
        "aux", "con", "nul", "prn", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
        "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    RESERVED
        .iter()
        .any(|reserved| reserved.eq_ignore_ascii_case(name))
}

/// `Utf8Truncation.TruncateToUTF8ByteLength`: keep the longest byte prefix within
/// `max_bytes` that ends on a character boundary (Rust strings are always valid
/// UTF-8, so unpaired surrogates have no counterpart here).
pub fn truncate_to_utf8_byte_length(text: &str, max_bytes: usize) -> String {
    let mut length = max_bytes.min(text.len());
    while !text.is_char_boundary(length) {
        length -= 1;
    }
    text[..length].to_owned()
}

/// Split the base name (after the last separator) into stem and extension with
/// `Path.GetExtension` semantics: the extension runs from the last `.`, unless the dot
/// is missing, first, or last in the base name. `.bashrc` keeps its dot as the
/// extension with an empty stem; `a.` has no extension.
fn base_name(file_name: &str) -> &str {
    let base_start = file_name
        .rfind(['/', '\\'])
        .map(|index| index + 1)
        .unwrap_or(0);
    &file_name[base_start..]
}

/// `Path.GetExtension`: the extension runs from the last `.` of the base name, unless
/// the dot is missing, first, or last (`.bashrc` keeps its dot as the extension with
/// an empty stem; `a.` has no extension).
pub fn file_extension(file_name: &str) -> &str {
    let base = base_name(file_name);
    match base.rfind('.') {
        None => "",
        // A trailing dot means no extension, including the bare `.` and `..`.
        Some(dot) if dot + 1 == base.len() => "",
        Some(0) => base,
        Some(dot) => &base[dot..],
    }
}

/// `Path.GetFileNameWithoutExtension`: the base name cut at its last `.`
/// (a separate rule from `file_extension`: `a.` stems to `a` with no extension).
pub fn file_name_without_extension(file_name: &str) -> &str {
    let base = base_name(file_name);
    match base.rfind('.') {
        None => base,
        Some(dot) => &base[..dot],
    }
}

/// Why a unique file name cannot be planned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UniqueNameError {
    /// Every `name_counter.ext` candidate up to `i32::MAX` exists.
    CounterExhausted,
}

impl Display for UniqueNameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CounterExhausted => {
                write!(
                    formatter,
                    "cannot generate a unique file name: all counters exist"
                )
            }
        }
    }
}

impl Error for UniqueNameError {}

/// The per-directory `name_counter` memory behind `GetUniqueName`, keyed by the
/// joined initial path like the source `UniqueNamesByInitialPath` map.
#[derive(Clone, Debug, Default)]
pub struct UniqueNamePlanner {
    counters: HashMap<String, u64>,
}

impl UniqueNamePlanner {
    /// `FileSystem.GetUniqueName` over caller-supplied facts: truncate overlong names
    /// to `max_name_bytes` UTF-8 bytes (extension preserved), return the name when the
    /// directory is missing or the name is free and unreserved, else append
    /// `_counter` from the remembered start value until a free name appears.
    /// `join` builds display paths for existence checks and map keys.
    pub fn unique_name(
        &mut self,
        directory: &str,
        file_name: &str,
        max_name_bytes: usize,
        directory_exists: bool,
        join: impl Fn(&str, &str) -> String,
        file_exists: impl Fn(&str) -> bool,
    ) -> Result<String, UniqueNameError> {
        let mut valid = file_name.to_owned();
        if valid.len() > max_name_bytes {
            let extension = file_extension(&valid).to_owned();
            let keep = max_name_bytes.saturating_sub(extension.len());
            valid = truncate_to_utf8_byte_length(file_name, keep) + extension.as_str();
        }
        if !directory_exists {
            return Ok(valid);
        }
        let stem = file_name_without_extension(&valid).to_owned();
        let extension = file_extension(&valid);
        if !is_reserved_name(&stem) && !file_exists(&join(directory, &valid)) {
            return Ok(valid);
        }
        let key = join(directory, &format!("{stem}{extension}"));
        let start = self.counters.get(&key).copied().unwrap_or(0);
        let mut counter = start;
        loop {
            if counter >= i32::MAX as u64 {
                return Err(UniqueNameError::CounterExhausted);
            }
            let proposed = format!("{stem}_{counter}{extension}");
            if !file_exists(&join(directory, &proposed)) {
                self.counters.insert(key, counter);
                return Ok(proposed);
            }
            counter += 1;
        }
    }
}

/// Caller facts behind the `ExportCollection.GetUniqueFileName` composition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportFileNameFacts<'a> {
    /// The export directory the name is planned in.
    pub directory: &'a str,
    /// The asset best name (markers stripped, trimmed, fixed).
    pub best_name: &'a str,
    /// The class-name fallback for blank names.
    pub class_name: &'a str,
    /// The export extension, appended after a dot.
    pub extension: &'a str,
    /// Whether the export directory already exists.
    pub directory_exists: bool,
}

/// The `ExportCollection.GetUniqueFileName` composition over caller facts: best name
/// with clone/instance markers removed and trimmed (falling back to the class name),
/// invalid characters fixed, the caller extension appended, then unique-planned with
/// the `.meta` reservation held back from the file-name budget.
pub fn unique_export_file_name(
    planner: &mut UniqueNamePlanner,
    facts: &ExportFileNameFacts<'_>,
    join: impl Fn(&str, &str) -> String,
    file_exists: impl Fn(&str) -> bool,
) -> Result<String, UniqueNameError> {
    let mut file_name = remove_instance_suffixes(&remove_clone_suffixes(facts.best_name));
    file_name = file_name.trim().to_owned();
    if file_name.is_empty() {
        file_name = facts.class_name.to_owned();
    } else {
        file_name = fix_invalid_file_name_characters(&file_name);
    }
    file_name.push('.');
    file_name.push_str(facts.extension);
    planner.unique_name(
        facts.directory,
        &file_name,
        MAX_FILE_NAME_LENGTH - META_EXTENSION_RESERVED_LENGTH,
        facts.directory_exists,
        join,
        file_exists,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Unescape the fixture `\\`, `\n`, `\r`, `\t`, `\xNN`, and `\uNNNN` spellings
    /// (surrogate pairs combine into one astral character, mirroring the probe).
    fn unescape(text: &str) -> String {
        let mut output = String::new();
        let mut chars = text.chars();
        while let Some(char) = chars.next() {
            if char != '\\' {
                output.push(char);
                continue;
            }
            match chars.next() {
                Some('n') => output.push('\n'),
                Some('t') => output.push('\t'),
                Some('r') => output.push('\r'),
                Some('\\') => output.push('\\'),
                Some('x') => {
                    let digits: String = chars.by_ref().take(2).collect();
                    let byte = u8::from_str_radix(&digits, 16).unwrap();
                    output.push(byte as char);
                }
                Some('u') => {
                    let digits: String = chars.by_ref().take(4).collect();
                    let unit = u32::from_str_radix(&digits, 16).unwrap();
                    if (0xD800..0xDC00).contains(&unit) {
                        let marker: String = chars.by_ref().take(2).collect();
                        assert_eq!(marker, "\\u", "split surrogate pair in {text}");
                        let low_digits: String = chars.by_ref().take(4).collect();
                        let low = u32::from_str_radix(&low_digits, 16).unwrap();
                        assert!(
                            (0xDC00..0xE000).contains(&low),
                            "lone high surrogate in {text}"
                        );
                        let scalar = 0x1_0000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
                        output.push(char::from_u32(scalar).unwrap());
                    } else {
                        output.push(char::from_u32(unit).unwrap());
                    }
                }
                other => panic!("unexpected escape {other:?}"),
            }
        }
        output
    }

    /// Inputs free of Windows-only invalid characters, asserted on every host.
    const PORTABLE_FIX_INPUTS: &[&str] = &[
        "plain",
        "a,b",
        "a[b]",
        "a]b",
        "a/b",
        "caf\u{e9}",
        "a",
        " a ",
        "a/b",
        "a /b",
        "a/ b",
        "a  /  b",
        "/a/b/",
    ];

    fn is_portable_fix_input(text: &str) -> bool {
        // Portable rows avoid every character whose validity differs by host.
        !text.chars().any(|value| {
            matches!(value, '"' | '<' | '>' | '|' | ':' | '*' | '?' | '\\')
                || value < '\u{20}'
                || value == '\u{7f}'
        })
    }

    #[test]
    fn suffix_extension_and_truncation_helpers_match_the_reference_runtime() {
        let fixture = include_str!("file_names_fixture.txt");
        let mut clones = 0;
        let mut instances = 0;
        let mut truncations = 0;
        let mut extensions = 0;
        let mut stems = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            match parts.next() {
                Some("CLONE") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("an output");
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(remove_clone_suffixes(input), expected, "{line}");
                    clones += 1;
                }
                Some("INST") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("an output");
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(remove_instance_suffixes(input), expected, "{line}");
                    instances += 1;
                }
                Some("TRUNC") => {
                    let input = unescape(parts.next().expect("an input"));
                    let max: usize = parts.next().expect("a max").parse().unwrap();
                    let expected = unescape(parts.next().expect("an output"));
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(
                        truncate_to_utf8_byte_length(&input, max),
                        expected,
                        "{line}"
                    );
                    truncations += 1;
                }
                Some("EXT") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("an output");
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(file_extension(input), expected, "{line}");
                    extensions += 1;
                }
                Some("NOWEXT") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("an output");
                    assert!(parts.next().is_none(), "{line}");
                    // The stem is asserted on the base name: inputs carry no directory.
                    assert_eq!(file_name_without_extension(input), expected, "{line}");
                    stems += 1;
                }
                _ => {}
            }
        }
        assert_eq!(clones, 5, "all probed clone rows are asserted");
        assert_eq!(instances, 5, "all probed instance rows are asserted");
        assert_eq!(truncations, 9, "all probed truncation rows are asserted");
        assert_eq!(extensions, 10, "all probed extension rows are asserted");
        assert_eq!(stems, 10, "all probed stem rows are asserted");
    }

    #[test]
    fn invalid_character_fixing_matches_the_reference_runtime() {
        for input in PORTABLE_FIX_INPUTS {
            assert!(is_portable_fix_input(input), "{input} is not portable");
        }
        let fixture = include_str!("file_names_fixture.txt");
        let mut names = 0;
        let mut paths = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Inputs may contain pipes (which the source replaces), so the input
            // is everything between the first and the last pipe of these rows.
            let (kind, rest) = line.split_once('|').expect("a row kind");
            match kind {
                "FIXFN" => {
                    let (input, expected) = rest.rsplit_once('|').expect("an input and an output");
                    let input = unescape(input);
                    let expected = unescape(expected);
                    if cfg!(windows) || is_portable_fix_input(&input) {
                        assert_eq!(fix_invalid_file_name_characters(&input), expected, "{line}");
                    }
                    names += 1;
                }
                "FIXPATH" => {
                    let (input, expected) = rest.rsplit_once('|').expect("an input and an output");
                    let input = unescape(input);
                    let expected = unescape(expected);
                    if cfg!(windows) || is_portable_fix_input(&input) {
                        assert_eq!(fix_invalid_path_characters(&input), expected, "{line}");
                    }
                    paths += 1;
                }
                _ => {}
            }
        }
        assert_eq!(names, 15, "all probed file-name rows are present");
        assert_eq!(paths, 9, "all probed path rows are present");
    }

    #[test]
    fn reserved_names_and_limits_match_the_reference_runtime() {
        let fixture = include_str!("file_names_fixture.txt");
        let mut reserved = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            match parts.next() {
                Some("RESERVED") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("a flag") == "1";
                    assert!(parts.next().is_none(), "{line}");
                    // The source gate is Windows-only, so probed positives read
                    // false on other hosts exactly like the source would.
                    assert_eq!(is_reserved_name(input), expected && cfg!(windows), "{line}");
                    reserved += 1;
                }
                Some("MAXLEN") => {
                    let limit: usize = parts.next().expect("a limit").parse().unwrap();
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(limit, MAX_FILE_NAME_LENGTH, "{line}");
                    assert_eq!(
                        ACTUAL_MAX_FILE_NAME_LENGTH - RESERVED_FILE_NAME_CHARACTER_COUNT,
                        limit
                    );
                }
                _ => {}
            }
        }
        assert_eq!(reserved, 16, "all probed reserved rows are asserted");
        assert_eq!(META_EXTENSION_RESERVED_LENGTH, 5);
    }

    #[test]
    fn unique_name_planning_matches_the_reference_runtime() {
        fn join(directory: &str, file: &str) -> String {
            format!("{directory}/{file}")
        }
        // The probe ran sequential calls against one planner and real directories:
        // replay the same scripted filesystem states in order.
        let mut planner = UniqueNamePlanner::default();
        let existing = std::cell::RefCell::new(HashSet::new());
        let exists = |path: &str| existing.borrow().contains(path);
        let exists = &exists;
        // Missing directory: the name passes through (truncated when overlong).
        assert_eq!(
            planner
                .unique_name("missing", "name.ext", 245, false, join, exists)
                .unwrap(),
            "name.ext"
        );
        // A real collision takes the `_0` suffix; the file still does not exist,
        // so the remembered counter replays the same answer like the source.
        existing.borrow_mut().insert(join("collision", "name.ext"));
        assert_eq!(
            planner
                .unique_name("collision", "name.ext", 245, true, join, exists)
                .unwrap(),
            "name_0.ext"
        );
        assert_eq!(
            planner
                .unique_name("collision", "name.ext", 245, true, join, exists)
                .unwrap(),
            "name_0.ext"
        );
        // Reserved stems are suffixed even without a collision. The reserved gate
        // is Windows-only in the source, so other hosts take the free name.
        assert_eq!(
            planner
                .unique_name("collision", "con.txt", 245, true, join, exists)
                .unwrap(),
            if cfg!(windows) {
                "con_0.txt"
            } else {
                "con.txt"
            }
        );
        // Overlong names truncate to the byte budget with the extension kept.
        let long_unicode = "a".repeat(240) + "\u{e9}.txt";
        let planned = planner
            .unique_name("missing", &long_unicode, 245, false, join, exists)
            .unwrap();
        assert_eq!(planned.len(), 244);
        assert!(planned.ends_with(".txt"), "{planned}");
        assert_eq!(&planned[..240], "a".repeat(240).as_str());
        let long_bare = "b".repeat(250);
        assert_eq!(
            planner
                .unique_name("missing", &long_bare, 245, false, join, exists)
                .unwrap()
                .len(),
            245
        );
        // The probed long outputs match byte for byte.
        let fixture = include_str!("file_names_fixture.txt");
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            if parts.next() != Some("UQ") {
                continue;
            }
            let case = parts.next().expect("a case");
            let expected = parts.next().expect("an output");
            assert!(parts.next().is_none(), "{line}");
            match case {
                "missing-dir" => assert_eq!(expected, "name.ext"),
                "first-collision" | "second-collision" => assert_eq!(expected, "name_0.ext"),
                "reserved" => assert_eq!(expected, "con_0.txt"),
                "long-unicode" => assert_eq!(expected, planned),
                "long-noext" => assert_eq!(expected, "b".repeat(245)),
                other => panic!("unexpected unique case {other}"),
            }
        }
    }

    #[test]
    fn unique_export_file_names_compose_the_source_steps() {
        fn join(directory: &str, file: &str) -> String {
            format!("{directory}/{file}")
        }
        let mut planner = UniqueNamePlanner::default();
        let empty: HashSet<String> = HashSet::new();
        fn facts(best_name: &str) -> ExportFileNameFacts<'_> {
            ExportFileNameFacts {
                directory: "out",
                best_name,
                class_name: "Mesh",
                extension: "mat",
                directory_exists: false,
            }
        }
        // Markers are stripped and the extension appended within budget.
        assert_eq!(
            unique_export_file_name(&mut planner, &facts("Sword(Clone)"), join, |path| {
                empty.contains(path)
            })
            .unwrap(),
            "Sword.mat"
        );
        // Blank names fall back to the class name.
        assert_eq!(
            unique_export_file_name(&mut planner, &facts("   "), join, |path| {
                empty.contains(path)
            })
            .unwrap(),
            "Mesh.mat"
        );
    }
}
