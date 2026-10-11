use std::error::Error;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

const YAML_EXTENSIONS: &[&str] = &[
    "asset",
    "mat",
    "anim",
    "controller",
    "unity",
    "prefab",
    "playable",
];

#[derive(Debug)]
struct Pointer {
    file_id: String,
    guid: Option<String>,
}

#[derive(Debug)]
struct ObjectDocument {
    document_index: usize,
    class_id: String,
    file_id: String,
    type_name: Option<String>,
    name: Option<String>,
    pointers: Vec<Pointer>,
}

fn json_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0c}' => escaped.push_str("\\f"),
            control if control.is_control() => {
                let _ = write!(escaped, "\\u{:04x}", control as u32);
            }
            ordinary => escaped.push(ordinary),
        }
    }
    escaped.push('"');
    escaped
}

fn open_read(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        let path_text = path.to_string_lossy();
        let extended = if path_text.starts_with(r"\\?\") {
            path_text.into_owned()
        } else if let Some(unc_path) = path_text.strip_prefix(r"\\") {
            format!(r"\\?\UNC\{unc_path}")
        } else {
            format!(r"\\?\{path_text}")
        };
        File::open(extended)
    }
    #[cfg(not(windows))]
    File::open(path)
}

fn collect_yaml_files(directory: &Path, output: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let path = entry.path();
        if kind.is_dir() {
            collect_yaml_files(&path, output)?;
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    YAML_EXTENSIONS
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
                })
        {
            output.push(path);
        }
    }
    Ok(())
}

fn parse_header(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("--- !u!")?;
    let (class_id, remainder) = rest.split_once(' ')?;
    let file_id = remainder.trim_start().strip_prefix('&')?;
    let file_id = file_id
        .split(|character: char| character.is_ascii_whitespace())
        .next()?;
    if class_id.is_empty()
        || file_id.is_empty()
        || !class_id.bytes().all(|byte| byte.is_ascii_digit())
        || !(file_id.bytes().all(|byte| byte.is_ascii_digit())
            || (file_id.starts_with('-') && file_id[1..].bytes().all(|byte| byte.is_ascii_digit())))
    {
        return None;
    }
    Some((class_id.to_owned(), file_id.to_owned()))
}

fn value_after<'a>(segment: &'a str, key: &str) -> Option<&'a str> {
    let start = segment.find(key)? + key.len();
    let value = segment[start..].trim_start();
    let end = value
        .find(|character: char| {
            character == ',' || character == '}' || character.is_ascii_whitespace()
        })
        .unwrap_or(value.len());
    Some(&value[..end])
}

fn parse_pointer_segment(segment: &str) -> Option<Pointer> {
    let file_id = value_after(segment, "fileID:")?;
    if file_id.is_empty()
        || !(file_id.bytes().all(|byte| byte.is_ascii_digit())
            || (file_id.starts_with('-') && file_id[1..].bytes().all(|byte| byte.is_ascii_digit())))
    {
        return None;
    }
    let guid = value_after(segment, "guid:").map(|value| value.to_ascii_lowercase());
    Some(Pointer {
        file_id: file_id.to_owned(),
        guid,
    })
}

fn append_pointers(line: &str, document: &mut ObjectDocument) {
    let mut remaining = line;
    while let Some(open) = remaining.find('{') {
        remaining = &remaining[open + 1..];
        let Some(close) = remaining.find('}') else {
            break;
        };
        if let Some(pointer) = parse_pointer_segment(&remaining[..close]) {
            document.pointers.push(pointer);
        }
        remaining = &remaining[close + 1..];
    }
}

fn parse_root_type(line: &str) -> Option<String> {
    let candidate = line.strip_suffix(':')?;
    if candidate.is_empty()
        || candidate.as_bytes()[0].is_ascii_whitespace()
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || candidate.as_bytes()[0].is_ascii_digit()
    {
        return None;
    }
    Some(candidate.to_owned())
}

fn emit_document(
    output: &mut impl Write,
    relative_path: &str,
    document: ObjectDocument,
) -> io::Result<()> {
    let mut pointers = document.pointers.into_iter();
    write!(
        output,
        "{{\"path\":{},\"documentIndex\":{},\"classId\":{},\"fileId\":{},\"type\":{},\"name\":{},\"pointers\":[",
        json_string(relative_path),
        document.document_index,
        document.class_id,
        json_string(&document.file_id),
        document
            .type_name
            .as_deref()
            .map(json_string)
            .unwrap_or_else(|| "null".to_owned()),
        document
            .name
            .as_deref()
            .map(json_string)
            .unwrap_or_else(|| "null".to_owned()),
    )?;
    let mut first = true;
    for pointer in &mut pointers {
        if !first {
            output.write_all(b",")?;
        }
        first = false;
        write!(
            output,
            "{{\"fileId\":{},\"guid\":{}}}",
            json_string(&pointer.file_id),
            pointer
                .guid
                .as_deref()
                .map(json_string)
                .unwrap_or_else(|| "null".to_owned()),
        )?;
    }
    output.write_all(b"]}\n")
}

fn scan_file(root: &Path, path: &Path, output: &mut impl Write) -> Result<(), Box<dyn Error>> {
    let relative_path = path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/");
    let file = open_read(path)?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut line = String::new();
    let mut document: Option<ObjectDocument> = None;
    let mut document_index = 0usize;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        if let Some((class_id, file_id)) = parse_header(&line) {
            if let Some(previous) = document.take() {
                emit_document(output, &relative_path, previous)?;
            }
            document = Some(ObjectDocument {
                document_index,
                class_id,
                file_id,
                type_name: None,
                name: None,
                pointers: Vec::new(),
            });
            document_index = document_index
                .checked_add(1)
                .ok_or("YAML document index overflowed")?;
            continue;
        }
        if let Some(document) = document.as_mut() {
            if document.type_name.is_none() {
                document.type_name = parse_root_type(line.trim_end_matches(['\r', '\n']));
            }
            if document.name.is_none() {
                if let Some(value) = line.strip_prefix("  m_Name:") {
                    document.name = Some(value.trim().to_owned());
                }
            }
            if line.contains("fileID:") {
                append_pointers(&line, document);
            }
        }
    }
    if let Some(document) = document {
        emit_document(output, &relative_path, document)?;
    }
    Ok(())
}

fn argument_path(value: OsString) -> PathBuf {
    PathBuf::from(value)
}

fn run() -> Result<(), Box<dyn Error>> {
    let root = std::env::args_os()
        .nth(1)
        .map(argument_path)
        .ok_or("usage: real_project_yaml_inventory <project-root>")?;
    if std::env::args_os().nth(2).is_some() {
        return Err("usage: real_project_yaml_inventory <project-root>".into());
    }
    let root = root.canonicalize()?;
    let mut files = Vec::new();
    collect_yaml_files(&root, &mut files)?;
    files.sort();

    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    for path in files {
        scan_file(&root, &path, &mut output)?;
    }
    output.flush()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("real-project-yaml-inventory: {error}");
        std::process::exit(1);
    }
}
