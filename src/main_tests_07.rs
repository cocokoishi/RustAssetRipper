    #[test]
    fn export_project_material_saved_property_maps_match_the_reference_2018_2_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_properties_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_multiple_saved_property_entries_preserve_source_order() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_tpk_with_multiple_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_multi_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_duplicate_saved_property_keys_match_reference_yaml() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_tpk_with_duplicate_saved_property_keys(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_duplicate_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2018_3_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2018_3_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_2018_3_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2019_4_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2019_4_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_2019_4_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2022_3_project_probe() {
        let fixture = include_str!("export/material_tpk_2022_3_fixture.txt");
        assert!(fixture.contains("GEN|21|Material_2022_1_0_b14"));
        assert!(fixture.contains("GEN|48|Shader_2022_3_20"));
        assert!(fixture.contains("TPK|48|2022.3.22f1|release"));
        assert!(fixture.contains("NODE|1|vector|m_ValidKeywords|"));
        assert!(fixture.contains("NODE|1|PPtr<Material>|m_Parent|"));
        assert!(fixture.contains("NODE|1|unsigned int|m_ModifiedSerializedProperties|"));
        assert!(fixture.contains("NODE|1|string|m_LockedProperties|"));
        assert!(fixture.contains("NODE|1|vector|m_BuildTextureStacks|"));
        assert!(fixture.contains("m_ParsedForm"));

        assert_material_project_yaml_matches_fixture(
            make_material_file_with_reference_2022_3_tpk_and_saved_properties(),
            include_str!("export/material_project_2022_3_fixture.yaml"),
        );
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_unity6_project_probe() {
        let fixture = include_str!("export/material_tpk_6000_4_fixture.txt");
        assert!(fixture.contains("GEN|21|Material_2023_2_8"));
        assert!(fixture.contains("GEN|48|Shader_6000_3_0_b6"));
        assert!(fixture.contains("TPK|48|6000.4.0f1|release"));
        assert!(fixture.contains("NODE|1|vector|m_ValidKeywords|"));
        assert!(fixture.contains("NODE|1|PPtr<Material>|m_Parent|"));
        assert!(fixture.contains("NODE|1|unsigned int|m_ModifiedSerializedProperties|"));
        assert!(fixture.contains("NODE|1|string|m_LockedProperties|"));
        assert!(fixture.contains("NODE|1|vector|m_BuildTextureStacks|"));
        assert!(fixture.contains("NODE|1|bool|m_AllowLocking|"));
        assert!(fixture.contains("m_ParsedForm"));

        assert_material_project_yaml_matches_fixture(
            make_material_file_with_reference_6000_4_tpk_and_saved_properties(),
            include_str!("export/material_project_6000_4_fixture.yaml"),
        );
    }

    #[test]
    fn export_project_material_resolves_saved_texture_pointer_to_exported_texture_guid() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        let texture_path = assets_directory.join("texture.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2019_4_tpk_and_texture_reference(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        fs::write(
            &texture_path,
            make_minimal_texture_file("TextureFixture", 88),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![
                material_path.clone(),
                shader_path.clone(),
                texture_path.clone(),
            ],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path, texture_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let texture_meta =
            fs::read_to_string(output_root.join("Assets/Texture2D/TextureFixture.texture2D.meta"))
                .unwrap();
        let texture_guid = texture_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Texture2D sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml
            .replace(shader_guid, "<SHADER_GUID>")
            .replace(texture_guid, "<TEXTURE_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_2019_4_texture_ref_fixture.yaml")
        );
        assert!(output_root
            .join("Assets/Texture2D/TextureFixture.texture2D")
            .is_file());

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_maps_multiple_texture_pointers_to_their_own_exported_guids() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        let texture_a_path = assets_directory.join("texture_a.assets");
        let texture_b_path = assets_directory.join("texture_b.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2019_4_tpk_and_two_texture_references(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        fs::write(&texture_a_path, make_minimal_texture_file("TextureA", 88)).unwrap();
        fs::write(&texture_b_path, make_minimal_texture_file("TextureB", 89)).unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![
                material_path.clone(),
                shader_path.clone(),
                texture_a_path.clone(),
                texture_b_path.clone(),
            ],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("Assets/Texture2D"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path, texture_a_path, texture_b_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let texture_a_meta =
            fs::read_to_string(output_root.join("Assets/Texture2D/TextureA.texture2D.meta"))
                .unwrap();
        let texture_a_guid = texture_a_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("first exported Texture2D sidecar has a GUID");
        let texture_b_meta =
            fs::read_to_string(output_root.join("Assets/Texture2D/TextureB.texture2D.meta"))
                .unwrap();
        let texture_b_guid = texture_b_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("second exported Texture2D sidecar has a GUID");
        assert_ne!(texture_a_guid, texture_b_guid);

        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let main_texture_pointer = format!(
            "      _MainTex:\n        m_Texture: {{fileID: 2800000, guid: {texture_a_guid}, type: 2}}"
        );
        let additional_texture_pointer = format!(
            "      _ATexture:\n        m_Texture: {{fileID: 2800000, guid: {texture_b_guid}, type: 2}}"
        );
        assert!(material_yaml.contains(&main_texture_pointer));
        assert!(material_yaml.contains(&additional_texture_pointer));
        assert!(
            material_yaml.find("_MainTex:").unwrap() < material_yaml.find("_ATexture:").unwrap(),
            "Material texture properties preserve serialized order"
        );
        assert!(output_root
            .join("Assets/Texture2D/TextureA.texture2D")
            .is_file());
        assert!(output_root
            .join("Assets/Texture2D/TextureB.texture2D")
            .is_file());

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_material_yaml_matches_the_reference_2017_4_project_probe() {
        let directory = TestInputDirectory::new_root();
        let assets_directory = directory.join("assets");
        fs::create_dir(&assets_directory).unwrap();
        let output_root = directory.join("project");
        fs::create_dir(&output_root).unwrap();
        let material_path = assets_directory.join("material.assets");
        let shader_path = assets_directory.join("shader.assets");
        fs::write(
            &material_path,
            make_material_file_with_reference_2017_4_tpk_and_saved_properties(),
        )
        .unwrap();
        fs::write(
            &shader_path,
            make_shader_file_with_non_modifiable_texture_null(),
        )
        .unwrap();
        let mut input = TestInputDirectory {
            directory,
            files: vec![material_path.clone(), shader_path.clone()],
            subdirectories: vec![
                assets_directory,
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/Material"),
                output_root.join("Assets/Shader"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::export_project_to(
            &[material_path, shader_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap();

        let shader_meta =
            fs::read_to_string(output_root.join("Assets/Shader/Test_Simple.shader.meta")).unwrap();
        let shader_guid = shader_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .expect("exported Shader sidecar has a GUID");
        let material_yaml =
            fs::read_to_string(output_root.join("Assets/Material/MaterialFixture.mat")).unwrap();
        let normalized = material_yaml.replace(shader_guid, "<SHADER_GUID>");
        assert_eq!(
            normalized,
            include_str!("export/material_project_2017_4_fixture.yaml")
        );

        input.files.extend(
            project_tree(&output_root)
                .iter()
                .map(|path| output_root.join(path)),
        );
        drop(input);
    }

    #[test]
    fn export_project_rejects_unresolved_references_before_writing_project_files() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let output_root = directory.join("project");
        fs::write(
            &source_path,
            make_generation_10_file_with_pointer_path_id(&[], 99),
        )
        .unwrap();
        fs::create_dir(&output_root).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: vec![output_root.clone()],
        };

        let error = super::export_project_to(
            &[source_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(
            error.to_string().contains("unresolved")
                || error.to_string().contains("complete workspace PPtr scan"),
            "unexpected project export rejection: {error}"
        );
        assert_eq!(fs::read_dir(&output_root).unwrap().count(), 0);
        drop(input);
    }

    #[test]
    fn export_project_tolerates_missing_external_dependencies_with_missing_markers() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let output_root = directory.join("project");
        // The pointer names an external file that is never supplied, so the
        // reference resolves as MissingDependency rather than InvalidFileId.
        fs::write(
            &source_path,
            make_generation_10_file_with_pointer(&["other.assets"], 1, 99),
        )
        .unwrap();
        fs::create_dir(&output_root).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source_path.clone()],
            subdirectories: vec![output_root.clone()],
        };

        super::export_project_to(
            &[source_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .expect("missing external dependencies export with missing markers");

        assert_ne!(fs::read_dir(&output_root).unwrap().count(), 0);
        // The dangling pointer exports with the baseline MissingReference
        // GUID instead of failing the whole project.
        let mut marker_found = false;
        let mut stack = vec![output_root.clone()];
        while let Some(directory) = stack.pop() {
            for entry in fs::read_dir(&directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|extension| {
                    extension == "asset"
                        || extension == "unity"
                        || extension == "prefab"
                        || extension == "controller"
                }) {
                    let text = fs::read_to_string(&path).unwrap();
                    marker_found |= text.contains("0000000deadbeef15deadf00d0000000");
                }
            }
        }
        assert!(marker_found, "exported project has no missing marker");
        drop(input);
    }

    #[test]
    fn export_project_refuses_a_nonempty_output_directory_without_touching_it() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let output_root = directory.join("project");
        let existing_file = output_root.join("keep.txt");
        fs::write(&source_path, make_generation_10_file()).unwrap();
        fs::create_dir(&output_root).unwrap();
        fs::write(&existing_file, b"existing project file").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![source_path.clone(), existing_file.clone()],
            subdirectories: vec![output_root.clone()],
        };

        let error = super::export_project_to(
            &[source_path],
            &output_root,
            false,
            &default_export_settings(),
            &mut Vec::new(),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("User declined to delete existing export directory. Aborting export."));
        assert!(error.to_string().contains("--delete-existing"));
        assert_eq!(fs::read(&existing_file).unwrap(), b"existing project file");
        assert_eq!(fs::read_dir(&output_root).unwrap().count(), 1);
        drop(input);
    }

    #[test]
    fn export_project_replaces_a_nonempty_output_directory_when_delete_existing_leads() {
        let directory = TestInputDirectory::new_root();
        let source_path = directory.join("main.assets");
        let output_root = directory.join("project");
        let stale_file = output_root.join("keep.txt");
        fs::write(&source_path, make_generation_10_file()).unwrap();
        fs::create_dir(&output_root).unwrap();
        fs::write(&stale_file, b"stale project content").unwrap();
        let mut input = TestInputDirectory {
            directory: directory.clone(),
            files: vec![source_path.clone()],
            // Drop removes directories in reverse order, so keep parents before children.
            subdirectories: vec![
                output_root.clone(),
                output_root.join("Assets"),
                output_root.join("Assets/GameObject"),
                output_root.join("ProjectSettings"),
                output_root.join("Packages"),
            ],
        };

        super::run(
            [
                std::ffi::OsString::from("export-project"),
                std::ffi::OsString::from("--delete-existing"),
                output_root.clone().into_os_string(),
                source_path.clone().into_os_string(),
            ]
            .into_iter(),
        )
        .unwrap();

        let asset = output_root.join("Assets/GameObject/GameObject.asset");
        let meta = output_root.join("Assets/GameObject/GameObject.asset.meta");
        assert!(
            !stale_file.exists(),
            "the stale export root content survives"
        );
        assert!(fs::read_to_string(&asset)
            .unwrap()
            .contains("--- !u!1 &100000"));
        assert!(fs::read_to_string(output_root.join("ProjectSettings/ProjectVersion.txt")).is_ok());
        input.files.extend([
            asset,
            meta,
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
        drop(input);
    }

    #[test]
    fn export_project_accepts_delete_existing_after_the_inputs() {
        let mut input = TestInputDirectory::new();
        let source_path = input.files[0].clone();
        let output_root = input.directory.join("project");

        // Dispatch hands the command parser the arguments after the command name.
        super::export_project_from_arguments(
            [
                output_root.clone().into_os_string(),
                source_path.into_os_string(),
                std::ffi::OsString::from("--delete-existing"),
            ]
            .into_iter(),
        )
        .unwrap();

        let asset = output_root.join("Assets/GameObject/GameObject.asset");
        assert!(asset.is_file());
        input.files.extend([
            asset,
            output_root.join("Assets/GameObject/GameObject.asset.meta"),
            output_root.join("ProjectSettings/ProjectVersion.txt"),
            output_root.join("Packages/manifest.json"),
        ]);
        input.subdirectories.extend([
            output_root.clone(),
            output_root.join("Assets"),
            output_root.join("Assets/GameObject"),
            output_root.join("ProjectSettings"),
            output_root.join("Packages"),
        ]);
        drop(input);
    }

    #[test]
    fn export_project_treats_delete_existing_as_a_flag_not_as_the_output_directory() {
        // Dispatch hands the command parser the arguments after the command name.
        let error = super::export_project_from_arguments(
            [std::ffi::OsString::from("--delete-existing")].into_iter(),
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("export-project requires a new output directory"));
    }

    #[test]
    fn export_file_yaml_reports_baseline_omitted_image_textures_without_creating_output() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let output_path = directory.join("serialized-file.yaml");
        fs::write(
            &serialized_path,
            make_streamed_image_texture_file_with_stream("", 3),
        )
        .unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), output_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut status = Vec::new();
        super::export_file_yaml_to(&[serialized_path], 0, &output_path, &mut status).unwrap();
        assert!(!output_path.exists());
        assert!(String::from_utf8(status).unwrap().contains(
            "all 1 objects were omitted because the baseline YAML exporter found ImageTexture objects without usable inline or streamed data"
        ));
        drop(input);
    }

    #[test]
    fn export_file_yaml_rejects_unresolved_and_cross_file_references_without_output() {
        let unresolved_directory = TestInputDirectory::new_root();
        let unresolved_path = unresolved_directory.join("main.assets");
        let unresolved_output = unresolved_directory.join("unresolved.yaml");
        fs::write(
            &unresolved_path,
            make_generation_10_file_with_pointer_path_id(&[], 99),
        )
        .unwrap();
        let unresolved_input = TestInputDirectory {
            directory: unresolved_directory,
            files: vec![unresolved_path.clone(), unresolved_output.clone()],
            subdirectories: Vec::new(),
        };
        let unresolved_error = super::export_file_yaml_to(
            std::slice::from_ref(&unresolved_path),
            0,
            &unresolved_output,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(unresolved_error
            .to_string()
            .contains("unresolved reference"));
        assert!(!unresolved_output.exists());
        drop(unresolved_input);

        let cross_file_directory = TestInputDirectory::new_root();
        let source_path = cross_file_directory.join("main.assets");
        let target_path = cross_file_directory.join("other.assets");
        let cross_file_output = cross_file_directory.join("cross-file.yaml");
        fs::write(
            &source_path,
            make_generation_10_file_with_pointer(&["other.assets"], 1, 42),
        )
        .unwrap();
        fs::write(&target_path, make_generation_10_file()).unwrap();
        let cross_file_input = TestInputDirectory {
            directory: cross_file_directory,
            files: vec![
                source_path.clone(),
                target_path.clone(),
                cross_file_output.clone(),
            ],
            subdirectories: Vec::new(),
        };
        let cross_file_error = super::export_file_yaml_to(
            &[source_path, target_path],
            0,
            &cross_file_output,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(cross_file_error
            .to_string()
            .contains("cannot assign a foreign collection GUID"));
        assert!(!cross_file_output.exists());
        drop(cross_file_input);
    }

    #[test]
    fn explicit_file_inspection_resolves_uint32_streaming_info_offset_from_selected_root_resource()
    {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let resource_path = directory.join("texture.resS");
        fs::write(
            &serialized_path,
            make_streaming_info_file_with_offset_width("texture.resS", 2, 3, false),
        )
        .unwrap();
        fs::write(&resource_path, b"0123456789").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), resource_path.clone()],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(&[serialized_path, resource_path], &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains(
            "StreamingInfo resources: 1 candidates, 0 unset, 0 bundle ranges, 1 explicit root ranges, 0 external ranges, 0 missing, 0 invalid ranges, 0 resolution errors, 0 scan errors, 0 undecoded objects"
        ));
        assert!(summary.contains("complete=true"));
        assert!(!summary.contains("StreamingInfo diagnostic:"));
        drop(input);
    }

    #[test]
    fn explicit_file_inspection_keeps_unselected_sibling_resource_missing() {
        let directory = TestInputDirectory::new_root();
        let serialized_path = directory.join("main.assets");
        let unselected_resource_path = directory.join("missing.resS");
        fs::write(
            &serialized_path,
            make_streaming_info_file("missing.resS", 2, 3),
        )
        .unwrap();
        fs::write(&unselected_resource_path, b"not mounted as an input").unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![serialized_path.clone(), unselected_resource_path],
            subdirectories: Vec::new(),
        };

        let mut output = Vec::new();
        super::inspect_explicit_files_to(&[serialized_path], &mut output).unwrap();
        let summary = String::from_utf8(output).unwrap();

        assert!(summary.contains(
            "StreamingInfo resources: 1 candidates, 0 unset, 0 bundle ranges, 0 explicit root ranges, 0 external ranges, 1 missing"
        ));
        assert!(summary.contains("complete=false"));
        assert!(summary.contains("path \"missing.resS\""));
        assert!(summary.contains("resource missing"));
        drop(input);
    }

    #[test]
    fn directory_inspection_decodes_scene_collection_guid_words() {
        let directory = TestInputDirectory::new_root();
        let file = directory.join("level0");
        let expected = [0x0123_4567, 0x89ab_cdef, 3, 4];
        let bytes = make_occlusion_settings_file(expected);
        let parsed = assetripper::serialized_file::file::SerializedFile::parse(&bytes).unwrap();
        let payload = parsed.decode_object_payload(0).unwrap();
        assert_eq!(
            assetripper::processing::scene_collections::extract_occlusion_scene_guid(&payload.root)
                .unwrap()
                .unwrap()
                .data,
            expected
        );
        fs::write(&file, bytes).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![file],
            subdirectories: Vec::new(),
        };

        super::inspect_directories(std::slice::from_ref(&input.directory)).unwrap();
    }

    #[test]
    fn workspace_summary_maps_scene_collection_path_and_guid() {
        let directory = TestInputDirectory::new_root();
        let scene_file = directory.join("level0");
        let build_settings_file = directory.join("globalgamemanagers");
        let prefab_file = directory.join("prefab.assets");
        fs::write(
            &scene_file,
            make_occlusion_settings_file([0x0123_4567, 0x89ab_cdef, 3, 4]),
        )
        .unwrap();
        fs::write(
            &build_settings_file,
            make_build_settings_file(&["Assets/Scenes/Main.unity"]),
        )
        .unwrap();
        fs::write(&prefab_file, make_generation_10_file()).unwrap();
        let input = TestInputDirectory {
            directory,
            files: vec![scene_file, build_settings_file, prefab_file],
            subdirectories: Vec::new(),
        };
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(std::slice::from_ref(&input.directory), false).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependencies) = super::load_workspace_with_dependencies(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
        )
        .unwrap();
        let mut output = Vec::new();
        super::print_workspace_summary_to(&mut output, &filesystem, &workspace, &dependencies)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary
            .contains("Object payload decode: 3/3 decoded, 0 undecoded, 0 raw fallback bytes"));
        assert!(summary.contains("Workspace collections: 3 (3 objects, 0 external identifiers)"));
        assert!(summary.contains("TypeTree reference graph:"));
        assert!(
            summary.contains("SceneGUID words: [0x01234567, 0x89abcdef, 0x00000003, 0x00000004]")
        );
        assert!(summary.contains(
            "Scene path mapping for collection \"level0\": Assets/Scenes/Main (BuildSettings index 0, duplicate path=false)"
        ));
        assert!(summary.contains("Non-scene hierarchy collection graphs: 1"));
        assert!(summary.contains(": 1 GameObjects, 0 Transforms,"));
    }

    #[test]
    fn workspace_summary_refuses_handbuilt_game_object_data_that_conflicts_with_source_factory() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let directory = TestInputDirectory::new_root();
        fs::write(
            directory.join("level0"),
            make_occlusion_settings_file([1, 2, 3, 4]),
        )
        .unwrap();
        fs::write(
            directory.join("hero-go"),
            make_inspect_game_object_file("Hero", 10, &[(1, 20)], Some("hero-transform")),
        )
        .unwrap();
        fs::write(
            directory.join("hero-transform"),
            make_inspect_transform_file(20, (1, 10), &[], (0, 0), 3, &["hero-go"]),
        )
        .unwrap();
        fs::write(
            directory.join("side-go"),
            make_inspect_game_object_file("Sidekick", 11, &[(1, 21)], Some("side-transform")),
        )
        .unwrap();
        fs::write(
            directory.join("side-transform"),
            make_inspect_transform_file(21, (0, 9999), &[], (1, 20), 5, &["hero-transform"]),
        )
        .unwrap();
        let (mut filesystem, selected_paths, _) =
            super::mount_input_directories(std::slice::from_ref(&directory), false).unwrap();
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependencies) = super::load_workspace_with_dependencies(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
        )
        .unwrap();
        let mut output = Vec::new();
        let error =
            super::print_workspace_summary_to(&mut output, &filesystem, &workspace, &dependencies)
                .expect_err(
                    "the source-generated GameObject schema rejects this handbuilt payload",
                );
        let summary = String::from_utf8(output).unwrap();
        assert!(error
            .to_string()
            .contains("no matching decoded reference at 'm_Component[0]'"));
        assert!(summary
            .contains("Object payload decode: 0/5 decoded, 5 undecoded, 230 raw fallback bytes"));
        assert!(summary.contains("UnreadableObject raw fallback"));
        assert!(!summary.contains("Scene graph:"));
    }

    #[test]
    fn reference_graph_summary_distinguishes_null_resolved_and_unresolved_pointers() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        for (pointer_path_id, expected_counts, complete) in [
            (0, "2 null, 0 uniquely resolved, 0 unresolved", true),
            (42, "1 null, 1 uniquely resolved, 0 unresolved", true),
            (99, "1 null, 0 uniquely resolved, 1 unresolved", false),
        ] {
            let mut filesystem = VirtualFileSystem::new();
            insert_vfs_file(
                &mut filesystem,
                "/game/main.assets",
                &make_mono_behaviour_with_game_object_pointer(i64::from(pointer_path_id)),
            );
            let workspace = super::load_workspace_from_vfs(
                &filesystem,
                &[assetripper::io::dependency::DependencyFilePath {
                    file_name: "main.assets".to_owned(),
                    virtual_path: "/game/main.assets".to_owned(),
                }],
                assetripper::serialized_file::workspace::SerializedFileWorkspaceLimits::default(),
            )
            .unwrap();
            let graph = workspace
                .reference_graph(
                    assetripper::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
                )
                .unwrap();
            let mut output = Vec::new();
            super::print_reference_graph_summary_to(&mut output, &workspace, &graph).unwrap();
            let summary = String::from_utf8(output).unwrap();
            assert!(summary.contains(expected_counts), "{summary}");
            assert!(
                summary.contains(&format!("complete={complete}")),
                "{summary}"
            );
            if pointer_path_id == 99 {
                assert!(summary.contains("MissingLocalObject { path_id: 99 }"));
            }
        }
    }

    #[test]
    fn workspace_scene_processing_projects_unresolved_no_type_tree_components_as_absent() {
        let _source_layouts = super::SourceFactoryLayoutGuard::use_source_factory();
        let fixture = include_str!("processing/scene_unresolved_source_fixture.txt");
        let input_hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("INPUT_HEX|"))
            .expect("scene processor fixture contains its source-authored input bytes");
        assert!(
            input_hex.len().is_multiple_of(2),
            "fixture hex has even length"
        );
        let input_bytes = input_hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16)
                    .expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let mut filesystem = VirtualFileSystem::new();
        let virtual_path = "/game/UnresolvedComponent.unity";
        insert_vfs_file(&mut filesystem, virtual_path, &input_bytes);
        let input = DependencyFilePath {
            file_name: "UnresolvedComponent.unity".to_owned(),
            virtual_path: virtual_path.to_owned(),
        };
        let (workspace, dependencies) = super::load_workspace_with_dependencies(
            &mut filesystem,
            std::slice::from_ref(&input),
            std::slice::from_ref(&input),
            &[],
            super::WorkspaceLoadLimits::default(),
        )
        .unwrap();

        let mut output = Vec::new();
        super::print_workspace_summary_to(&mut output, &filesystem, &workspace, &dependencies)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains(
            "field \"m_Component[0].component\": MissingDependency { external_index: 0, path_id: 99 }"
        ));
        assert!(summary.contains(
            "Scene graph: 1 GameObjects, 0 Transforms, 1 relationships, 0 decode issues, complete=false"
        ));
        assert!(summary.contains("Workspace root candidates: 1, root issues: 0"));
        assert!(
            summary.contains("Generated Prefab Transforms planned for non-scene GameObjects: 1")
        );
        assert!(summary.contains(
            "hierarchy file 0: 1 GameObjects, 0 Transforms, 0 component candidates, 1 roots, 0 root issues, 1 relationships, complete=false"
        ));
        assert!(summary.contains(
            "1 generated hierarchies, 1 virtual Transform entries in planned hierarchies"
        ));
        assert!(summary.contains("complete=true, stopped=false"));
    }

    #[test]
    fn workspace_summary_selects_last_build_settings_in_source_collection_order() {
        let mut filesystem = VirtualFileSystem::new();
        insert_vfs_file(
            &mut filesystem,
            "/game/a.assets",
            &make_build_settings_file_with_path_id(&["Assets/Scenes/First.unity"], 42),
        );
        insert_vfs_file(
            &mut filesystem,
            "/game/z.assets",
            &make_build_settings_file_with_path_id(&["Assets/Scenes/Last.unity"], 43),
        );
        let selected_paths = vec!["/game/a.assets".to_owned(), "/game/z.assets".to_owned()];
        let mixed = super::MixedGameStructure::from_vfs_paths(
            &filesystem,
            &selected_paths,
            super::MixedGameStructureLimits::default(),
        )
        .unwrap();
        let (workspace, dependencies) = super::load_workspace_with_dependencies(
            &mut filesystem,
            &mixed.serialized_file_inputs,
            &mixed.registered_files,
            &mixed.data_paths,
            super::WorkspaceLoadLimits::default(),
        )
        .unwrap();

        let mut output = Vec::new();
        super::print_workspace_summary_to(&mut output, &filesystem, &workspace, &dependencies)
            .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(summary.contains(
            "BuildSettings object 42: 1 scene paths (selected by source collection traversal)"
        ));
        assert!(summary.contains("Generated EditorBuildSettings: 1 scenes"));
        assert!(summary.contains("scene[0]: enabled=true, path=\"Assets/Scenes/First.unity\""));
        assert!(!summary.contains(
            "BuildSettings object 43: 1 scene paths (selected by source collection traversal)"
        ));
    }

    #[test]
    fn workspace_summary_applies_streamed_scene_bundle_path_override() {
        let mut filesystem = VirtualFileSystem::new();
        let scene_file = make_occlusion_settings_file([1, 2, 3, 4]);
        let streamed_bundle = make_asset_bundle_file();
        let webdata = make_webdata_fixture(&[
            ("CAB-Scene", &scene_file),
            ("assetbundle.assets", &streamed_bundle),
        ]);
        insert_vfs_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file(),
        );
        insert_vfs_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };
        let loaded = structure
            .load_primary_serialized_workspace_with_layout_source(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
                super::active_class_layout_source(),
            )
            .unwrap();

        let object_workspace = load_object_workspace(
            &filesystem,
            &loaded.workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let collection_workspace = WorkspaceAssetCollections::build(
            &loaded.workspace,
            &object_workspace,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let bundle_name_plan =
            assetripper::processing::asset_bundle::plan_workspace_asset_bundle_names(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                Default::default(),
            )
            .unwrap();
        assert!(
            bundle_name_plan.is_complete(),
            "{:?}",
            bundle_name_plan.issues
        );
        assert_eq!(bundle_name_plan.bundles.len(), 1);
        assert_eq!(bundle_name_plan.bundles[0].source_name, "data.unity3d");
        assert_eq!(bundle_name_plan.bundles[0].asset_name, "data.unity3d");
        assert_eq!(bundle_name_plan.container_references.len(), 1);
        assert_eq!(
            bundle_name_plan.container_references[0].mapping.target,
            assetripper::processing::asset_bundle::AssetBundleContainerTarget::Null
        );
        assert!(bundle_name_plan.assignments.is_empty());

        let mut output = Vec::new();
        super::print_workspace_summary_to(
            &mut output,
            &filesystem,
            &loaded.workspace,
            &loaded.dependencies,
        )
        .unwrap();
        let summary = String::from_utf8(output).unwrap();
        assert!(
            summary.contains("Streamed-scene AssetBundle PathID 42: SceneHashes, 1 path mappings")
        );
        assert!(summary.contains("collection \"CAB-Scene\" (workspace file"));
        assert!(summary.contains(": Assets/Scenes/BundleScene"));
        assert!(summary.contains(
            "effective scene path: Assets/Scenes/BundleScene (streamed-scene override; assignments=1, conflicting=false)"
        ));
        assert!(summary.contains(
            "SceneDefinition model 0: name=\"BundleScene\", path=\"Assets/Scenes/BundleScene\" (streamed-scene AssetBundle)"
        ));
        assert!(summary.contains("collection files: [0]"));
        assert!(summary
            .contains("scene GUID: preserve [0x00000001, 0x00000002, 0x00000003, 0x00000004]"));
        assert!(
            summary.contains("hierarchy AssetBundleName: null"),
            "{summary}"
        );
        assert!(
            summary.contains(
                "hierarchy AssetBundleName candidate: asset \"data.unity3d\" (first IAssetBundle in FetchAssets order)"
            ),
            "{summary}"
        );
        assert!(summary.contains("GeneratedObjectBase.AssetBundleName setter is a no-op"));
        assert!(
            summary.contains("Generated Prefab Transforms planned for non-scene GameObjects: 0")
        );
        assert!(summary.contains(
            "PrefabProcessor identity plan: 0 scene GameObjects, 0 existing PrefabInstances, 0 existing hierarchies, 0 existing instances skipped, 0 generated hierarchies, 0 virtual Transform entries in planned hierarchies, 0 remaining GameObjects skipped, 0 processed GameObjects, 0 hierarchy entries, 1 root issues, 0 incomplete hierarchies, complete=false, stopped=false"
        ), "{summary}");
        assert!(summary.contains(
            "GetRoot is indeterminate: Some(IncompleteCollectionGraph { file_index: 2 })"
        ));
        assert!(summary.contains(
            "hierarchy file 0: 0 GameObjects, 0 Transforms, 1 scene assets (1 managers, 0 PrefabInstances, 0 SceneRoots), 0 component candidates (0 Transform IDs, 0 GameObject edges, 0 m_GameObject fields), 0 missing Transform plans, 0 relationships, 0 roots, 0 graph issues, 0 root issues, 0 reciprocal issues, scene graph complete=true, reference scans complete=true, overall complete=true"
        ));
    }

    #[test]
    fn workspace_asset_bundle_plan_assigns_only_unique_local_container_targets() {
        let mut filesystem = VirtualFileSystem::new();
        let scene_file = make_occlusion_settings_file([1, 2, 3, 4]);
        let streamed_bundle = make_asset_bundle_file_with_local_reference();
        let webdata = make_webdata_fixture(&[
            ("CAB-Scene", &scene_file),
            ("assetbundle.assets", &streamed_bundle),
        ]);
        insert_vfs_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file(),
        );
        insert_vfs_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };
        let loaded = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();
        let object_workspace = load_object_workspace(
            &filesystem,
            &loaded.workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let collection_workspace = WorkspaceAssetCollections::build(
            &loaded.workspace,
            &object_workspace,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();
        let plan = assetripper::processing::asset_bundle::plan_workspace_asset_bundle_names(
            &loaded.workspace,
            &object_workspace,
            &collection_workspace,
            Default::default(),
        )
        .unwrap();

        assert!(plan.is_complete(), "{:?}", plan.issues);
        assert_eq!(plan.bundles.len(), 2);
        let source_bundle = plan
            .bundles
            .iter()
            .position(|bundle| bundle.source_path_id == 42)
            .unwrap();
        assert_eq!(plan.bundles[source_bundle].source_name, "data.unity3d");
        assert_eq!(plan.bundles[source_bundle].container_references, 0..1);
        assert_eq!(plan.assignments.len(), 1);
        assert_eq!(plan.assignments[0].bundle_record_index, source_bundle);
        assert_eq!(plan.assignments[0].target_object_index, 1);
        assert_eq!(plan.assignments[0].target_path_id, 43);
        assert_eq!(
            plan.asset_name_for_assignment(&plan.assignments[0]),
            Some("data.unity3d")
        );
        assert_eq!(
            plan.container_references[plan.assignments[0].reference_record_index]
                .mapping
                .target,
            assetripper::processing::asset_bundle::AssetBundleContainerTarget::LocalObject {
                object_index: 1
            }
        );
        let bundle_limit_error =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::DirectExport,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathLimits {
                    max_bundles: 1,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            bundle_limit_error,
            assetripper::processing::asset_bundle::WorkspaceAssetBundlePathError::CountLimitExceeded {
                resource: "AssetBundle path source records",
                count: 2,
                limit: 1,
            }
        ));
        let target_file_index = plan.assignments[0].target_file_index;
        let target_object_index = plan.assignments[0].target_object_index;
        let bundle_source_file_index = plan.bundles[source_bundle].source_file_index;
        let bundle_source_object_index = plan.bundles[source_bundle].source_object_index;

        let (direct_paths, direct_report) =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::DirectExport,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                Default::default(),
            )
            .unwrap();
        let direct_target = direct_paths
            .get(target_file_index, target_object_index)
            .expect("assigned target path metadata");
        assert_eq!(
            direct_target.asset_bundle_name.as_deref(),
            Some("data.unity3d")
        );
        assert_eq!(
            direct_target.original_path.as_deref(),
            Some("Assets/Scenes/Other.unity")
        );
        assert_eq!(direct_report.assigned_bundle_names, 1);
        assert_eq!(direct_report.assigned_original_paths, 1);

        let retained_path = "Assets/Prior/Existing.asset";
        let retained_directory = "Assets/Prior";
        let prior_bundle_name = "prior.bundle";
        let initial_paths =
            assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::from_records(
                vec![
                    assetripper::processing::asset_bundle::WorkspaceAssetExportPath {
                        file_index: target_file_index,
                        object_index: target_object_index,
                        path_id: plan.assignments[0].target_path_id,
                        asset_bundle_name: Some(prior_bundle_name.to_owned()),
                        original_path: Some(retained_path.to_owned()),
                        original_directory: Some(retained_directory.to_owned()),
                        ..Default::default()
                    },
                ],
                Default::default(),
            )
            .unwrap();
        let (type_grouped_paths, type_grouped_report) =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::GroupByAssetType,
                initial_paths,
                Default::default(),
            )
            .unwrap();
        let type_grouped_target = type_grouped_paths
            .get(target_file_index, target_object_index)
            .expect("retained export path metadata");
        assert_eq!(
            type_grouped_target.asset_bundle_name.as_deref(),
            Some("data.unity3d")
        );
        assert_eq!(
            type_grouped_target.original_path.as_deref(),
            Some(retained_path)
        );
        assert_eq!(
            type_grouped_target.original_directory.as_deref(),
            Some(retained_directory)
        );
        assert_eq!(
            type_grouped_report.metadata_string_bytes,
            "data.unity3d".len()
                + retained_path.len()
                + retained_directory.len()
                + "Existing".len()
                + "asset".len()
        );

        let (grouped_paths, grouped_report) =
            assetripper::processing::asset_bundle::apply_workspace_asset_bundle_paths(
                &loaded.workspace,
                &object_workspace,
                &collection_workspace,
                &plan,
                assetripper::processing::asset_bundle::BundledAssetsExportMode::GroupByBundleName,
                assetripper::processing::asset_bundle::WorkspaceAssetExportPathTable::empty(),
                Default::default(),
            )
            .unwrap();
        let grouped_target = grouped_paths
            .get(target_file_index, target_object_index)
            .expect("assigned target path metadata");
        let group_path = format!(
            "Assets/AssetBundles{}data.unity3d{}Scenes/Other.unity",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        let group_directory = format!(
            "Assets/AssetBundles{}data.unity3d{}Scenes",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            grouped_target.original_path.as_deref(),
            Some(group_path.as_str())
        );
        assert_eq!(
            grouped_target.original_directory.as_deref(),
            Some(group_directory.as_str())
        );
        let bundle_source = grouped_paths
            .get(bundle_source_file_index, bundle_source_object_index)
            .expect("GroupByBundleName collection default");
        let group_type_directory = format!(
            "Assets/AssetBundles{}data.unity3d{}AssetBundle",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            bundle_source.original_directory.as_deref(),
            Some(group_type_directory.as_str())
        );
        let scene_collection = collection_workspace
            .collections()
            .iter()
            .find(|collection| collection.fixed_name == "cab-scene")
            .expect("source container scene collection");
        let scene_manager = object_workspace
            .objects()
            .get(scene_collection.object_range.start)
            .expect("scene manager object");
        let scene_manager_paths = grouped_paths
            .get(scene_manager.file_index, scene_manager.object_index)
            .expect("GroupByBundleName scene collection default");
        let scene_group_directory = format!(
            "Assets/AssetBundles{}data.unity3d{}OcclusionCullingSettings",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            scene_manager_paths.original_directory.as_deref(),
            Some(scene_group_directory.as_str())
        );
        assert_eq!(grouped_report.assigned_bundle_names, 1);
        assert_eq!(grouped_report.assigned_original_paths, 1);
        assert_eq!(grouped_report.assigned_collection_directories, 2);
    }
