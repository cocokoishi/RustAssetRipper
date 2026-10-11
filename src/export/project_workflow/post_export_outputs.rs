use super::*;
use crate::io::filesystem::SearchOption;
use crate::io::stream::SmartStream;

/// The source post-exporters copy original files first, then save every manager
/// assembly. Keep stable stream ranges through execution instead of buffering
/// the StreamingAssets payloads or copying them into temporary files.
pub(super) fn plan_streamed_post_exports(
    filesystem: &VirtualFileSystem,
    provider: &PlatformResourceProvider<'_>,
    catalog: Option<&ManagedAssemblyCatalog>,
    streaming_assets_mode: StreamingAssetsMode,
    project_files: &[PlannedProjectFile],
    already_planned_bytes: usize,
    limits: WorkspaceProjectExportLimits,
) -> Result<ProjectStreamPostExports, Box<dyn Error>> {
    let mut result = ProjectStreamPostExports::default();
    let mut budget = PostExportBudget {
        total_bytes: already_planned_bytes,
        entries: 0,
        limits,
    };
    budget.add_bytes(0)?;
    if !streaming_assets_mode.skip_streaming_assets() {
        if let Some(input_directory) = provider
            .platform
            .and_then(|platform| platform.streaming_assets_path())
        {
            let source = provider.streaming_assets_filesystem().unwrap_or(filesystem);
            if source.directory_exists(Some(&input_directory))? {
                budget.add_entry()?;
                result.directories.push("Assets/StreamingAssets".to_owned());
                for path in source.enumerate_directories(
                    Some(&input_directory),
                    "*",
                    SearchOption::AllDirectories,
                )? {
                    budget.add_entry()?;
                    result.directories.try_reserve(1).map_err(|_| {
                        invalid_project("could not allocate StreamingAssets directories")
                    })?;
                    result
                        .directories
                        .push(streaming_destination(&input_directory, &path)?);
                }
                for path in source.enumerate_files(
                    Some(&input_directory),
                    "*",
                    SearchOption::AllDirectories,
                )? {
                    budget.add_entry()?;
                    let input = source.open_file(&path)?;
                    let byte_count = input.length()?;
                    budget.add_bytes(byte_count)?;
                    let (stream, _) = input.create_partial_file_backed(0, byte_count)?;
                    result.files.try_reserve(1).map_err(|_| {
                        invalid_project("could not allocate StreamingAssets file ranges")
                    })?;
                    result.files.push(PlannedProjectStreamFile {
                        relative_path: streaming_destination(&input_directory, &path)?,
                        stream,
                        byte_count,
                    });
                }
            }
        }
    }
    if let Some(catalog) = catalog {
        let assemblies = plan_manager_assembly_exports(catalog)?;
        if !assemblies.is_empty() {
            budget.add_entry()?;
            result
                .directories
                .push("AuxiliaryFiles/GameAssemblies".to_owned());
        }
        for assembly in assemblies {
            budget.add_entry()?;
            let file_name = add_assembly_file_extension(&assembly.assembly_name);
            if file_name.contains(['/', '\\']) {
                return Err(invalid_project(format!(
                    "manager assembly has an invalid file name {file_name:?}"
                )));
            }
            // When Plugins already contains the manager-saved result, reuse its
            // exact bytes. PE rewriting needs contiguous input; ordinary copied
            // StreamingAssets files above do not enter this serializer.
            let bytes = match project_files.iter().find(|file| {
                file.relative_path == assembly.relative_path
                    && file.kind == ExportCollectionKind::Script
            }) {
                Some(file) => {
                    budget.add_bytes(file.bytes.len() as u64)?;
                    let mut bytes = Vec::new();
                    bytes.try_reserve_exact(file.bytes.len()).map_err(|_| {
                        invalid_project("could not retain saved auxiliary assembly")
                    })?;
                    bytes.extend_from_slice(&file.bytes);
                    bytes
                }
                None => {
                    let remaining = limits
                        .max_total_asset_payload_bytes
                        .saturating_sub(budget.total_bytes);
                    let bytes = materialize_assembly_export(filesystem, &assembly, remaining)?;
                    budget.add_bytes(bytes.len() as u64)?;
                    bytes
                }
            };
            result
                .files
                .try_reserve(1)
                .map_err(|_| invalid_project("could not allocate auxiliary assembly outputs"))?;
            result.files.push(PlannedProjectStreamFile {
                relative_path: format!("AuxiliaryFiles/GameAssemblies/{file_name}"),
                byte_count: bytes.len() as u64,
                stream: SmartStream::from_memory(bytes),
            });
        }
    }
    Ok(result)
}

fn streaming_destination(root: &str, path: &str) -> Result<String, Box<dyn Error>> {
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let relative = path
        .strip_prefix(&prefix)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            invalid_project(format!("StreamingAssets path {path:?} is outside {root:?}"))
        })?;
    Ok(format!("Assets/StreamingAssets/{relative}"))
}

struct PostExportBudget {
    total_bytes: usize,
    entries: usize,
    limits: WorkspaceProjectExportLimits,
}

impl PostExportBudget {
    fn add_entry(&mut self) -> Result<(), Box<dyn Error>> {
        self.entries = self
            .entries
            .checked_add(1)
            .ok_or_else(|| invalid_project("post-export entry count overflows"))?;
        if self.entries > self.limits.max_post_export_entries {
            return Err(invalid_project("post-export entry count exceeds its limit"));
        }
        Ok(())
    }

    fn add_bytes(&mut self, bytes: u64) -> Result<(), Box<dyn Error>> {
        self.total_bytes = usize::try_from(bytes)
            .ok()
            .and_then(|bytes| self.total_bytes.checked_add(bytes))
            .ok_or_else(|| invalid_project("post-export byte count overflows"))?;
        if self.total_bytes > self.limits.max_total_asset_payload_bytes {
            return Err(invalid_project(
                "post-export payloads exceed the project byte limit",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::settings::StreamingAssetsMode;
    use crate::io::dependency::DependencyFilePath;
    use crate::io::platform_structure::{PlatformKind, PlatformStructure};
    use crate::processing::managed_assembly::ManagedAssemblyCatalogLimits;
    use std::collections::BTreeSet;
    use std::io::{Cursor, Read, Seek, SeekFrom};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    const FIXTURE: &str = include_str!("../post_export_source_2022_3_fixture.txt");
    const INPUT_ROOT: &str = "/copy/game_Data/StreamingAssets";

    fn decode_hex(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2));
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    struct CountReads {
        bytes: Cursor<Vec<u8>>,
        reads: Arc<AtomicUsize>,
    }
    impl Read for CountReads {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = self.bytes.read(buffer)?;
            self.reads.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }
    impl Seek for CountReads {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(position)
        }
    }

    fn originals() -> (VirtualFileSystem, Arc<AtomicUsize>) {
        let mut source = VirtualFileSystem::new();
        source.create_directory(Some(INPUT_ROOT)).unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        for line in FIXTURE.lines() {
            if let Some(relative) = line.strip_prefix("INPUT_DIR|") {
                source
                    .create_directory(Some(&format!("{INPUT_ROOT}/{relative}")))
                    .unwrap();
            } else if let Some(file) = line.strip_prefix("INPUT_FILE|") {
                let (relative, hex) = file.split_once('|').unwrap();
                let stream = SmartStream::from_reader(Box::new(CountReads {
                    bytes: Cursor::new(decode_hex(hex)),
                    reads: Arc::clone(&reads),
                }))
                .unwrap();
                source
                    .insert_file_stream_with_parents(&format!("{INPUT_ROOT}/{relative}"), stream)
                    .unwrap();
            }
        }
        (source, reads)
    }

    fn platform() -> PlatformStructure {
        PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/copy".to_owned(),
            consumed_paths: vec!["/copy".to_owned()],
            root_path: "/copy".to_owned(),
            name: Some("game".to_owned()),
            game_data_path: Some("/copy/game_Data".to_owned()),
            data_paths: vec!["/copy/game_Data".to_owned()],
        }
    }

    fn empty_plan() -> ProjectExportPlan {
        ProjectExportPlan {
            project_version_text: "2022.3.22f1".to_owned(),
            project_version_file: "m_EditorVersion: 2022.3.22f1\n".to_owned(),
            package_manifest: "{}".to_owned(),
            files: Vec::new(),
            support_files: Vec::new(),
            post_export_files: Vec::new(),
            skipped: Vec::new(),
            fallbacks: Vec::new(),
        }
    }

    fn output_root() -> std::path::PathBuf {
        static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
        std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "assetripper-post-export-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ))
    }

    #[test]
    fn original_stream_ranges_and_all_manager_assemblies_match_frozen_post_exporters() {
        let (source, reads) = originals();
        let mut processing = VirtualFileSystem::new();
        processing.create_directory(Some(INPUT_ROOT)).unwrap();
        // This view represents mounted/decoded containers and must not be copied.
        processing
            .write_all_bytes(&format!("{INPUT_ROOT}/wrapped.bytes"), b"expanded child")
            .unwrap();
        let assembly_bytes = include_bytes!("../../processing/managed_assembly_fixture.dll");
        processing.create_directory(Some("/manager")).unwrap();
        processing
            .write_all_bytes("/manager/FixtureScripts.dll", assembly_bytes)
            .unwrap();
        let (catalog, report) = ManagedAssemblyCatalog::load_from_vfs(
            &processing,
            &[DependencyFilePath {
                file_name: "FixtureScripts.dll".to_owned(),
                virtual_path: "/manager/FixtureScripts.dll".to_owned(),
            }],
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(report.loaded_count, 1);
        let structure = platform();
        let provider = PlatformResourceProvider::new(Some(&structure), None)
            .with_streaming_assets_filesystem(&source);
        let plan = empty_plan();
        let outputs = plan_streamed_post_exports(
            &processing,
            &provider,
            Some(&catalog),
            StreamingAssetsMode::Extract,
            &[],
            0,
            WorkspaceProjectExportLimits::default(),
        )
        .unwrap();
        assert_eq!(
            reads.load(Ordering::Relaxed),
            0,
            "planning retains ranges without reading payloads"
        );
        let root = output_root();
        let report = execute_project_export_with_streams(
            &root,
            false,
            &plan,
            &outputs,
            Default::default(),
            Default::default(),
        )
        .unwrap();
        assert_eq!(report.post_export_file_count, outputs.files.len());
        let mut expected_files = BTreeSet::new();
        for line in FIXTURE.lines() {
            if let Some(path) = line.strip_prefix("SOURCE_DIR|extract|") {
                let path = if path == "ExportedProject" {
                    ""
                } else {
                    path.strip_prefix("ExportedProject/").unwrap_or(path)
                };
                assert!(root.join(path).is_dir(), "source directory {path}");
            } else if let Some(file) = line.strip_prefix("SOURCE_FILE|extract|") {
                let (path, hex) = file.split_once('|').unwrap();
                let path = path.strip_prefix("ExportedProject/").unwrap_or(path);
                expected_files.insert(path.to_owned());
                let actual = std::fs::read(root.join(path)).unwrap();
                let expected = decode_hex(hex);
                if path.starts_with("AuxiliaryFiles/GameAssemblies/") {
                    let expected =
                        ManagedAssembly::parse(path, &expected, Default::default()).unwrap();
                    let actual = ManagedAssembly::parse(path, &actual, Default::default()).unwrap();
                    assert_eq!(actual.assembly_name, expected.assembly_name);
                    assert_eq!(actual.version_major, expected.version_major);
                    assert_eq!(
                        actual.types, expected.types,
                        "source manager-saved type/field model"
                    );
                } else {
                    assert_eq!(actual, expected, "original bytes at {path}");
                }
            }
        }
        assert_eq!(
            outputs
                .files
                .iter()
                .map(|file| file.relative_path.clone())
                .collect::<BTreeSet<_>>(),
            expected_files
        );
        assert!(outputs
            .files
            .iter()
            .all(|file| !root.join(format!("{}.meta", file.relative_path)).exists()));
        assert!(reads.load(Ordering::Relaxed) > 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn conditional_skips_and_post_export_limits_preserve_unread_input() {
        let (source, reads) = originals();
        let mut structure = platform();
        for scenario in ["ignore", "no-platform", "missing", "null-path"] {
            assert!(!FIXTURE
                .lines()
                .any(|line| line.starts_with(&format!("SOURCE_FILE|{scenario}|"))
                    || line.starts_with(&format!("SOURCE_DIR|{scenario}|"))));
            let mode = if scenario == "ignore" {
                StreamingAssetsMode::Ignore
            } else {
                StreamingAssetsMode::Extract
            };
            structure.game_data_path = match scenario {
                "missing" => Some("/absent".to_owned()),
                "null-path" => None,
                _ => Some("/copy/game_Data".to_owned()),
            };
            let provider = PlatformResourceProvider::new(
                (scenario != "no-platform").then_some(&structure),
                None,
            );
            let outputs = plan_streamed_post_exports(
                &source,
                &provider,
                None,
                mode,
                &[],
                0,
                Default::default(),
            )
            .unwrap();
            assert!(outputs.files.is_empty());
            assert!(outputs.directories.is_empty());
        }
        structure.game_data_path = Some("/copy/game_Data".to_owned());
        let provider = PlatformResourceProvider::new(Some(&structure), None);
        for limits in [
            WorkspaceProjectExportLimits {
                max_post_export_entries: 1,
                ..Default::default()
            },
            WorkspaceProjectExportLimits {
                max_total_asset_payload_bytes: 2,
                ..Default::default()
            },
        ] {
            assert!(plan_streamed_post_exports(
                &source,
                &provider,
                None,
                StreamingAssetsMode::Extract,
                &[],
                0,
                limits
            )
            .is_err());
        }
        assert_eq!(reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn auxiliary_collisions_and_invalid_paths_fail_before_output_root_deletion() {
        let root = output_root();
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("keep.txt"), b"existing output").unwrap();
        let mut plan = empty_plan();
        plan.files.push(PlannedProjectFile {
            collection_name: "AssetExportCollection",
            kind: ExportCollectionKind::Asset,
            relative_path: "Assets/item.asset".to_owned(),
            guid: [1; 16],
            bytes: vec![1],
            meta: Some(PlannedProjectMeta {
                importer_class: "NativeFormatImporter".to_owned(),
                importer: native_format_importer_for_version(
                    1,
                    SceneUnityVersion::parse("2022.3.22f1").unwrap(),
                    None,
                )
                .unwrap(),
            }),
            label: "item".to_owned(),
        });
        for path in [
            "Assets/item.asset.meta",
            "Assets/item.asset/child",
            "Assets/../outside",
            "Assets/StreamingAssets/CON",
        ] {
            if path.ends_with("/CON") && !cfg!(windows) {
                continue;
            }
            let outputs = ProjectStreamPostExports {
                directories: vec![],
                files: vec![PlannedProjectStreamFile {
                    relative_path: path.to_owned(),
                    stream: SmartStream::from_memory(vec![2]),
                    byte_count: 1,
                }],
            };
            assert!(
                execute_project_export_with_streams(
                    &root,
                    true,
                    &plan,
                    &outputs,
                    Default::default(),
                    Default::default()
                )
                .is_err(),
                "{path}"
            );
            assert_eq!(
                std::fs::read(root.join("keep.txt")).unwrap(),
                b"existing output"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
