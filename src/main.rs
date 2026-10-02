use std::error::Error;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use assetripper::io::dependency::DependencyIndexLimits;
use assetripper::io::filesystem::VirtualFileSystem;
use assetripper::io::game_structure::{MixedGameStructure, MixedGameStructureLimits};
use assetripper::io::platform_file_inventory::{
    PlatformFileInventoryLimits, PlatformFileInventoryOptions,
};
use assetripper::io::platform_structure::{detect_platform_structure, PlatformStructureLimits};
use assetripper::scheme::{
    BatchMountLimits, BatchMountReport, ContainerMountLimits, SchemeReader, SchemeReaderLimits,
};
use assetripper::serialized_file::workspace::{
    SerializedFileDependencyReport, SerializedFileWorkspace, SerializedFileWorkspaceLimits,
};

const MAX_INPUT_DIRECTORIES: usize = 100_000;

fn main() {
    if let Err(error) = run(std::env::args_os().skip(1)) {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run(arguments: impl Iterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    let mut arguments = arguments;
    let Some(command) = arguments.next() else {
        print_usage();
        return Err(invalid_input("a command is required"));
    };
    let command = command
        .into_string()
        .map_err(|_| invalid_input("the command name must be valid UTF-8"))?;
    if command == "--help" || command == "-h" || command == "help" {
        print_usage();
        return Ok(());
    }
    if command == "--version" || command == "-V" {
        println!("assetripper-rs {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if command != "inspect" {
        print_usage();
        return Err(invalid_input(format!("unknown command '{command}'")));
    }

    let mut input_directories = Vec::new();
    for argument in arguments {
        if input_directories.len() >= MAX_INPUT_DIRECTORIES {
            return Err(invalid_input(format!(
                "input directory count exceeds {MAX_INPUT_DIRECTORIES}"
            )));
        }
        input_directories.push(PathBuf::from(argument));
    }
    if input_directories.is_empty() {
        print_usage();
        return Err(invalid_input("inspect requires at least one directory"));
    }

    inspect_directories(&input_directories)
}

fn inspect_directories(input_directories: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let mut filesystem = VirtualFileSystem::new();
    let batch_limits = BatchMountLimits::default();
    let mut total_mount_report = BatchMountReport::default();
    let mut selected_paths = Vec::new();
    selected_paths
        .try_reserve_exact(input_directories.len())
        .map_err(|_| invalid_input("could not allocate the selected input list"))?;

    for (index, input_directory) in input_directories.iter().enumerate() {
        if !input_directory.is_dir() {
            return Err(invalid_input(format!(
                "input '{}' is not a directory",
                input_directory.display()
            )));
        }

        let destination_root = format!("/inputs/{index}");
        let remaining_limits = BatchMountLimits {
            max_inputs: remaining_budget_usize(
                batch_limits.max_inputs,
                total_mount_report.discovered_inputs,
                "logical input count",
            )?,
            max_directory_entries: remaining_budget_usize(
                batch_limits.max_directory_entries,
                total_mount_report.directory_entries_scanned,
                "directory entry count",
            )?,
            max_directory_depth: batch_limits.max_directory_depth,
            max_total_input_bytes: remaining_budget_u64(
                batch_limits.max_total_input_bytes,
                total_mount_report.discovered_source_bytes,
                "source input byte count",
            )?,
            max_total_output_bytes: remaining_budget_u64(
                batch_limits.max_total_output_bytes,
                total_mount_report.mounted_bytes,
                "mounted output byte count",
            )?,
        };
        let report = SchemeReader::mount_directory(
            input_directory,
            &mut filesystem,
            Some(&destination_root),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            remaining_limits,
        )?;
        merge_mount_report(&mut total_mount_report, &report)?;
        println!(
            "mounted {}: {} logical inputs, {} leaf files, {} output bytes",
            input_directory.display(),
            report.discovered_inputs,
            report.mounted_entries,
            report.mounted_bytes
        );
        selected_paths.push(destination_root);
    }

    println!(
        "total: {} logical inputs, {} leaf files, {} output bytes",
        total_mount_report.discovered_inputs,
        total_mount_report.mounted_entries,
        total_mount_report.mounted_bytes
    );

    let structure = detect_platform_structure(
        &filesystem,
        &selected_paths,
        PlatformStructureLimits::default(),
    )?;
    let platform_inventory = structure
        .as_ref()
        .map(|platform| {
            platform.collect_game_files(
                &filesystem,
                PlatformFileInventoryOptions::default(),
                PlatformFileInventoryLimits::default(),
            )
        })
        .transpose()?;
    let mixed = MixedGameStructure::from_vfs_paths(
        &filesystem,
        &selected_paths,
        MixedGameStructureLimits::default(),
    )?;
    if mixed.serialized_file_inputs.is_empty() {
        return Err(invalid_input(
            "the selected directories contain no recognized SerializedFile inputs",
        ));
    }
    let (workspace, dependencies) = SerializedFileWorkspace::load_from_vfs_with_dependencies(
        &mut filesystem,
        &mixed.serialized_file_inputs,
        &mixed.registered_files,
        &mixed.data_paths,
        SerializedFileWorkspaceLimits::default(),
        DependencyIndexLimits::default(),
        ContainerMountLimits::default(),
    )?;

    if let (Some(structure), Some(inventory)) = (structure, platform_inventory) {
        println!(
            "platform: {:?}; source registrations: {}; scripting backend: {:?}; assemblies: {}",
            structure.platform,
            inventory.registered_files.len(),
            inventory.scripting_backend,
            inventory.assemblies.len()
        );
        println!("workspace import: recursively mounted selected directories");
    } else {
        println!(
            "platform: mixed; registered files: {}; assemblies: {}",
            mixed.registered_files.len(),
            mixed.assemblies.len()
        );
    }
    print_workspace_summary(&workspace, &dependencies);

    Ok(())
}

fn print_workspace_summary(
    workspace: &SerializedFileWorkspace,
    dependencies: &SerializedFileDependencyReport,
) {
    println!("SerializedFiles: {}", workspace.files().len());
    for file in workspace.files() {
        println!(
            "  {:?}: {:?}, {} objects",
            file.file_name,
            file.header.format_version,
            file.metadata.objects.len()
        );
    }
    println!(
        "external requests: {}; newly loaded dependencies: {}; resolved: {}",
        dependencies.requests.len(),
        dependencies.loaded_serialized_files,
        dependencies.is_complete()
    );
}

fn remaining_budget_u64(limit: u64, consumed: u64, description: &str) -> Result<u64, io::Error> {
    limit.checked_sub(consumed).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("aggregate {description} exceeds its configured limit"),
        )
    })
}

fn remaining_budget_usize(
    limit: usize,
    consumed: usize,
    description: &str,
) -> Result<usize, io::Error> {
    limit.checked_sub(consumed).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("aggregate {description} exceeds its configured limit"),
        )
    })
}

fn merge_mount_report(
    total: &mut BatchMountReport,
    next: &BatchMountReport,
) -> Result<(), io::Error> {
    total.directory_entries_scanned = total
        .directory_entries_scanned
        .checked_add(next.directory_entries_scanned)
        .ok_or_else(|| invalid_data("directory entry count overflows"))?;
    total.discovered_inputs = total
        .discovered_inputs
        .checked_add(next.discovered_inputs)
        .ok_or_else(|| invalid_data("logical input count overflows"))?;
    total.discovered_source_bytes = total
        .discovered_source_bytes
        .checked_add(next.discovered_source_bytes)
        .ok_or_else(|| invalid_data("source input byte count overflows"))?;
    total.completed_inputs = total
        .completed_inputs
        .checked_add(next.completed_inputs)
        .ok_or_else(|| invalid_data("completed input count overflows"))?;
    total.mounted_entries = total
        .mounted_entries
        .checked_add(next.mounted_entries)
        .ok_or_else(|| invalid_data("mounted file count overflows"))?;
    total.mounted_bytes = total
        .mounted_bytes
        .checked_add(next.mounted_bytes)
        .ok_or_else(|| invalid_data("mounted output byte count overflows"))?;
    Ok(())
}

fn invalid_input(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn print_usage() {
    println!(
        "Usage:\n  assetripper-rs inspect <input-directory> [input-directory ...]\n  assetripper-rs --help\n  assetripper-rs --version\n\nMounts each directory into an isolated VFS root, detects platform or mixed inputs, and reports the parsed SerializedFiles."
    );
}

#[cfg(test)]
mod tests {
    use super::{merge_mount_report, remaining_budget_u64, remaining_budget_usize};
    use assetripper::scheme::BatchMountReport;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicUsize = AtomicUsize::new(0);

    struct TestInputDirectory {
        directory: PathBuf,
        file: PathBuf,
    }

    impl TestInputDirectory {
        fn new() -> Self {
            let index = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::current_dir()
                .unwrap()
                .join("target")
                .join(format!("cli-input-{}-{index}", std::process::id()));
            fs::create_dir(&directory).unwrap();
            let file = directory.join("main.assets");
            fs::write(&file, make_generation_10_file()).unwrap();
            Self { directory, file }
        }
    }

    impl Drop for TestInputDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.file);
            let _ = fs::remove_dir(&self.directory);
        }
    }

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn put_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
    }

    fn make_generation_10_file() -> Vec<u8> {
        let mut string_buffer = Vec::new();
        let root_type = push_type_tree_string(&mut string_buffer, "Root");
        let empty_name = push_type_tree_string(&mut string_buffer, "");
        let value_name = push_type_tree_string(&mut string_buffer, "m_Value");
        let int_type = push_type_tree_string(&mut string_buffer, "int");
        let target_name = push_type_tree_string(&mut string_buffer, "m_Target");
        let pptr_type = push_type_tree_string(&mut string_buffer, "PPtr<GameObject>");
        let file_id_name = push_type_tree_string(&mut string_buffer, "m_FileID");
        let int32_type = push_type_tree_string(&mut string_buffer, "SInt32");
        let path_id_name = push_type_tree_string(&mut string_buffer, "m_PathID");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 5);
        put_i32_le(&mut metadata, i32::try_from(string_buffer.len()).unwrap());
        put_node(&mut metadata, 0, root_type, empty_name, -1, 0);
        put_node(&mut metadata, 1, int_type, value_name, 4, 1);
        put_node(&mut metadata, 1, pptr_type, target_name, 8, 2);
        put_node(&mut metadata, 2, int32_type, file_id_name, 4, 3);
        put_node(&mut metadata, 2, int32_type, path_id_name, 4, 4);
        metadata.extend_from_slice(&string_buffer);

        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let data_offset = 20usize + metadata.len();
        let mut payload = Vec::new();
        put_i32_le(&mut payload, 0x1234_5678);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut payload, 42);
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    #[test]
    fn aggregate_remaining_budget_rejects_a_consumed_overrun() {
        assert_eq!(remaining_budget_usize(10, 4, "input count").unwrap(), 6);
        assert!(remaining_budget_usize(10, 11, "input count").is_err());
        assert_eq!(remaining_budget_u64(20, 7, "input bytes").unwrap(), 13);
        assert!(remaining_budget_u64(20, 21, "input bytes").is_err());
    }

    #[test]
    fn aggregate_mount_report_adds_all_counters_with_checked_arithmetic() {
        let mut total = BatchMountReport {
            directory_entries_scanned: 1,
            discovered_inputs: 2,
            discovered_source_bytes: 10,
            completed_inputs: 1,
            mounted_entries: 3,
            mounted_bytes: 12,
        };
        let next = BatchMountReport {
            directory_entries_scanned: 4,
            discovered_inputs: 4,
            discovered_source_bytes: 20,
            completed_inputs: 3,
            mounted_entries: 5,
            mounted_bytes: 30,
        };
        merge_mount_report(&mut total, &next).unwrap();
        assert_eq!(total.directory_entries_scanned, 5);
        assert_eq!(total.discovered_inputs, 6);
        assert_eq!(total.discovered_source_bytes, 30);
        assert_eq!(total.completed_inputs, 4);
        assert_eq!(total.mounted_entries, 8);
        assert_eq!(total.mounted_bytes, 42);
    }

    #[test]
    fn directory_inspection_loads_a_synthetic_serialized_file() {
        let input = TestInputDirectory::new();
        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
    }
}
