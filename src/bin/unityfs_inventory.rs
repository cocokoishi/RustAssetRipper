use std::error::Error;
use std::fmt::Write as FmtWrite;
use std::io::{self, Write};

use assetripper::container::unityfs::{UnityFsLimits, UnityFsStreamBundle};
use assetripper::io::stream::SmartStream;

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

fn run() -> Result<(), Box<dyn Error>> {
    let input_path = std::env::args_os()
        .nth(1)
        .ok_or("usage: unityfs-inventory <bundle-path>")?;
    if std::env::args_os().nth(2).is_some() {
        return Err("usage: unityfs-inventory <bundle-path>".into());
    }

    let source = SmartStream::open_read(&input_path)?;
    let bundle = UnityFsStreamBundle::parse(&source, UnityFsLimits::default(), usize::MAX)?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());

    writeln!(
        output,
        "{{\"kind\":\"header\",\"version\":{},\"unityVersion\":{},\"unityRevision\":{},\"declaredSize\":{},\"metadataCompression\":{},\"blockCount\":{},\"entryCount\":{},\"uncompressedDataSize\":{}}}",
        bundle.header.version,
        json_string(&bundle.header.unity_version),
        json_string(&bundle.header.unity_revision),
        bundle.header.declared_size,
        bundle.header.metadata_compression(),
        bundle.storage_blocks.len(),
        bundle.nodes.len(),
        bundle.uncompressed_data_size,
    )?;

    for (index, node) in bundle.nodes.iter().enumerate() {
        writeln!(
            output,
            "{{\"kind\":\"entry\",\"index\":{},\"path\":{},\"offset\":{},\"size\":{},\"flags\":{},\"serializedFileFlag\":{}}}",
            index,
            json_string(&node.path),
            node.offset,
            node.size,
            node.flags,
            node.is_serialized_file(),
        )?;
    }
    output.flush()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("unityfs-inventory: {error}");
        std::process::exit(1);
    }
}
