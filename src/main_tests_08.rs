    #[test]
    fn resource_manager_paths_select_longest_container_path_and_restore_asset_name() {
        let mut filesystem = VirtualFileSystem::new();
        let input_path = "/inputs/resource.assets";
        insert_vfs_file(
            &mut filesystem,
            input_path,
            &make_resource_manager_file_with_local_reference(),
        );
        let workspace = load_workspace_from_vfs(
            &filesystem,
            &[DependencyFilePath {
                file_name: "resource.assets".to_owned(),
                virtual_path: input_path.to_owned(),
            }],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects =
            load_object_workspace(&filesystem, &workspace, ObjectWorkspaceLimits::default())
                .unwrap();
        let collections = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let resource_plan =
            assetripper::processing::asset_bundle::plan_workspace_resource_manager_paths(
                &workspace,
                &objects,
                &collections,
                Default::default(),
            )
            .unwrap();
        assert!(resource_plan.is_complete(), "{:?}", resource_plan.issues);
        assert_eq!(resource_plan.resource_manager_count, 1);
        assert_eq!(resource_plan.container_entry_count, 2);
        assert_eq!(resource_plan.assignments.len(), 2);
        assert_eq!(
            resource_plan.assignments[0].path,
            "Textures/inner/wood.shader"
        );
        assert_eq!(resource_plan.assignments[1].path, "wood.shader");
        assert_eq!(resource_plan.assignments[0].file_id, 0);
        let assignment_limit_error =
            assetripper::processing::asset_bundle::plan_workspace_resource_manager_paths(
                &workspace,
                &objects,
                &collections,
                assetripper::processing::asset_bundle::WorkspaceResourceManagerPathLimits {
                    max_assignments: 1,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            assignment_limit_error,
            assetripper::processing::asset_bundle::WorkspaceResourceManagerPathPlanError::CountLimitExceeded {
                resource: "ResourceManager path assignments",
                count: 2,
                limit: 1,
            }
        ));
        assert!(matches!(
            objects.objects()[1]
                .payload
                .as_ref()
                .unwrap()
                .root
                .unique_field_in_base_chain("m_Name")
                .unwrap(),
            Some(assetripper::serialized_file::object::SerializedValue::StringBytes(bytes))
                if bytes == b"Wood"
        ));

        let bundle_plan =
            assetripper::processing::asset_bundle::WorkspaceAssetBundleNamePlan::default();
        let (paths, report) =
            assetripper::processing::asset_bundle::apply_workspace_original_paths(
                &workspace,
                &objects,
                &collections,
                assetripper::processing::asset_bundle::WorkspaceOriginalPathPlans {
                    asset_bundles: &bundle_plan,
                    resource_managers: &resource_plan,
                },
                assetripper::processing::asset_bundle::BundledAssetsExportMode::DirectExport,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                Default::default(),
            )
            .unwrap();
        let target = paths.get(0, 1).expect("ResourceManager target metadata");
        let expected_path = format!(
            "Assets/Resources{}Textures/inner{}Wood.shader",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            target.original_path.as_deref(),
            Some(expected_path.as_str())
        );
        assert_eq!(target.original_name.as_deref(), Some("Wood"));
        let expected_shader_directory = format!(
            "Assets/Resources{}Textures/inner",
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            target.override_directory.as_deref(),
            Some(expected_shader_directory.as_str())
        );
        assert_eq!(target.override_name.as_deref(), Some("Wood"));
        assert_eq!(target.override_extension.as_deref(), Some("shader"));
        assert_eq!(report.assigned_resource_manager_original_paths, 1);
        assert_eq!(report.restored_original_names, 1);
        assert_eq!(report.initialized_shader_overrides, 1);
    }

    #[test]
    fn platform_directory_inspection_preserves_and_expands_registered_containers() {
        let input = TestInputDirectory::new_windows_platform();
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(std::slice::from_ref(&input.directory), true).unwrap();
        let platform = detect_platform_structure(
            &filesystem,
            &selected_paths,
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(platform.platform, PlatformKind::Windows);

        let loaded = platform
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();

        assert_eq!(loaded.workspace.files().len(), 2);
        assert_eq!(loaded.initial_container_expansions.len(), 1);
        assert_eq!(
            loaded.initial_container_expansions[0].kind,
            FileKind::UnityWebData
        );
        assert!(matches!(
            loaded.dependencies.requests[0].outcome,
            SerializedFileDependencyOutcome::AlreadyLoaded { .. }
        ));
        assert_eq!(
            loaded.dependencies.requests[1].outcome,
            SerializedFileDependencyOutcome::ContainerAlreadyExpanded {
                virtual_path: format!("{}/data.unity3d", platform.data_paths[0]),
                serialized_files_loaded: 1,
                matching_serialized_files: 0,
            }
        );
        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
    }

    #[test]
    fn platform_directory_inspection_unions_mixed_inputs_and_resolves_their_references() {
        let platform_input = TestInputDirectory::new_windows_platform();
        let mixed_input = TestInputDirectory::new();
        let primary_path = platform_input
            .files
            .iter()
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name == "globalgamemanagers")
            })
            .expect("synthetic platform input has globalgamemanagers");
        fs::write(
            primary_path,
            make_generation_10_file_with_externals(&["main.assets"]),
        )
        .unwrap();

        let selected_directories = [
            platform_input.directory.clone(),
            mixed_input.directory.clone(),
        ];
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(&selected_directories, true).unwrap();
        let platform = detect_platform_structure(
            &filesystem,
            &selected_paths,
            PlatformStructureLimits::default(),
        )
        .unwrap()
        .expect("the first selected directory is a Windows platform root");
        let mixed = super::MixedGameStructure::from_vfs_paths_with_consumed_paths(
            &filesystem,
            &selected_paths,
            &platform.consumed_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        assert_eq!(mixed.serialized_file_inputs.len(), 1);

        let loaded = platform
            .load_primary_serialized_workspace_with_additional_inputs(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
                &mixed.serialized_file_inputs,
                &mixed.registered_files,
                &mixed.data_paths,
            )
            .unwrap();
        assert_eq!(loaded.workspace.files().len(), 3);
        let main_assets_request = loaded
            .dependencies
            .requests
            .iter()
            .find(|request| request.dependency_name == "main.assets")
            .expect("platform external table requests the mixed SerializedFile");
        let main_assets_index = match main_assets_request.outcome {
            SerializedFileDependencyOutcome::AlreadyLoaded { file_index } => file_index,
            ref outcome => {
                panic!("mixed SerializedFile was not resolved in the common workspace: {outcome:?}")
            }
        };
        assert_eq!(
            loaded.workspace.files()[main_assets_index].file_name,
            "main.assets"
        );

        let mut output = Vec::new();
        super::inspect_directories_to(&selected_directories, &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains("mixed registrations: 1"), "{summary}");
        assert!(summary.contains("SerializedFiles: 3"), "{summary}");
        assert!(summary.contains("external requests: 1"), "{summary}");
        assert!(summary.contains("resolved: true"), "{summary}");
        let source_row = include_str!("io/cli_inspect_fixture.txt")
            .lines()
            .find(|line| line.starts_with("PLATFORM_MIXED|"))
            .expect("source runtime fixture contains the platform/mixed result");
        assert_eq!(
            source_row,
            "PLATFORM_MIXED|input_names=globalgamemanagers,main.assets|processors_requested=1"
        );
    }

    fn os_args(words: &[&str]) -> Vec<std::ffi::OsString> {
        words.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn cli_command_classifier_pins_every_accepted_spelling() {
        use super::{classify_cli_command, CliCommand};
        assert_eq!(classify_cli_command("--help"), CliCommand::Help);
        assert_eq!(classify_cli_command("-h"), CliCommand::Help);
        assert_eq!(classify_cli_command("help"), CliCommand::Help);
        assert_eq!(classify_cli_command("--version"), CliCommand::Version);
        assert_eq!(classify_cli_command("-V"), CliCommand::Version);
        assert_eq!(
            classify_cli_command("export-object-yaml"),
            CliCommand::ExportObjectYaml
        );
        assert_eq!(
            classify_cli_command("export-file-yaml"),
            CliCommand::ExportFileYaml
        );
        assert_eq!(
            classify_cli_command("export-texture"),
            CliCommand::ExportTexture
        );
        assert_eq!(classify_cli_command("export-mesh"), CliCommand::ExportMesh);
        assert_eq!(
            classify_cli_command("export-shader"),
            CliCommand::ExportShader
        );
        assert_eq!(
            classify_cli_command("export-audio"),
            CliCommand::ExportAudio
        );
        assert_eq!(
            classify_cli_command("export-project"),
            CliCommand::ExportProject
        );
        assert_eq!(classify_cli_command("inspect"), CliCommand::Inspect);
        assert_eq!(
            classify_cli_command("inspect-files"),
            CliCommand::InspectFiles
        );
        // Matching is exact and case sensitive; near misses stay unknown.
        for unknown in [
            "",
            "Inspect",
            "INSPECT",
            "export",
            "--he",
            "--hel",
            "-v",
            "--Version",
            "serve",
            "inspect files",
        ] {
            assert_eq!(
                classify_cli_command(unknown),
                CliCommand::Unknown,
                "{unknown:?}"
            );
        }
    }

    #[test]
    fn cli_dispatch_rejects_missing_and_unknown_commands() {
        // No command at all.
        let error = super::run(Vec::new().into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "a command is required");
        // Unknown commands name the offender.
        let error = super::run(os_args(&["frobnicate"]).into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "unknown command 'frobnicate'");
        // Inspect commands without inputs name the command.
        let error = super::run(os_args(&["inspect"]).into_iter()).unwrap_err();
        assert_eq!(error.to_string(), "inspect requires at least one input");
        let error = super::run(os_args(&["inspect-files"]).into_iter()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "inspect-files requires at least one input"
        );
    }

    #[test]
    fn cli_dispatch_accepts_help_and_version_flags() {
        for command in ["--help", "-h", "help", "--version", "-V"] {
            super::run(os_args(&[command]).into_iter()).unwrap();
        }
    }

    #[test]
    fn load_process_plan_follows_the_bundle_presence_gate() {
        use super::{plan_load_process, LoadProcessPlan};
        // Any loaded collection runs processing, like `HasAnyAssetCollections()`.
        assert_eq!(plan_load_process(1), LoadProcessPlan::Process);
        assert_eq!(plan_load_process(2), LoadProcessPlan::Process);
        assert_eq!(plan_load_process(usize::MAX), LoadProcessPlan::Process);
        // An empty bundle warns, skips processing, and still returns.
        assert_eq!(plan_load_process(0), LoadProcessPlan::ReturnUnprocessed);
    }

    #[test]
    fn explicit_inspect_plan_prefers_workspace_then_roots_then_failure() {
        use super::{plan_explicit_inspect, ExplicitInspectPlan};
        // Serialized inputs always load, even alongside roots.
        assert_eq!(
            plan_explicit_inspect(1, 0),
            ExplicitInspectPlan::LoadWorkspace
        );
        assert_eq!(
            plan_explicit_inspect(2, 3),
            ExplicitInspectPlan::LoadWorkspace
        );
        // Roots without serialized inputs still report, like `FromPaths` keeping
        // `ResourceFile` inputs without any `SerializedFile`.
        assert_eq!(
            plan_explicit_inspect(0, 1),
            ExplicitInspectPlan::ReportRootsOnly
        );
        // Neither names nothing to inspect.
        assert_eq!(
            plan_explicit_inspect(0, 0),
            ExplicitInspectPlan::FailNoRecognizedInputs
        );
    }

    #[test]
    fn explicit_file_inspection_reports_roots_without_serialized_inputs() {
        let directory = TestInputDirectory::new_root();
        let resource_path = directory.join("direct.resS");
        fs::write(&resource_path, b"explicit root stream").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![resource_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(std::slice::from_ref(&resource_path), &mut output)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains("Explicit GameBundle root ResourceFiles: 1"));
        assert!(summary.contains("No SerializedFile inputs were selected."));
        assert!(!summary.contains("explicit file import:"));
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_fails_without_recognized_inputs() {
        let directory = TestInputDirectory::new_root();
        // A web-data container with no serialized entries mounts cleanly but
        // registers nothing, so the selection names nothing to inspect.
        let container_path = directory.join("payload.bin");
        fs::write(
            &container_path,
            make_webdata_fixture(&[("notes.txt", b"plain resource bytes")]),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![container_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        let error =
            super::inspect_explicit_files_to(std::slice::from_ref(&container_path), &mut output)
                .unwrap_err();
        assert_eq!(
            error.to_string(),
            "the selected files contain no recognized SerializedFile or root ResourceFile inputs"
        );
        drop(input);
    }
