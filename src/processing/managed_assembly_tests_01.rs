    use std::borrow::Cow;

    use dotnetdll::prelude::{
        FieldReferenceParent, MethodReferenceParent, ReadOptions, Resolution, ResolvedDebug,
    };

    use super::{
        has_cli_directory, resolve_mono_script_object, sha1_digest, ManagedAssembly,
        ManagedAssemblyCatalog, ManagedAssemblyCatalogLimits, ManagedAssemblyErrorKind,
        ManagedAssemblyLimits, ManagedAssemblyLoadErrorKind, ManagedScriptResolutionOutcome,
    };
    use crate::io::dependency::DependencyFilePath;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::processing::asset_factory::{
        FactoryAssetKind, FactoryLogLevel, FactoryObjectState, ObjectTailOutcome,
    };
    use crate::processing::script_identity::ScriptIdentifier;
    use crate::serialized_file::object::{
        ParsedObjectPayload, SerializedFieldValue, SerializedValue,
    };
    use crate::serialized_file::object_workspace::WorkspaceObject;

    const ASSEMBLY_BYTES: &[u8] = include_bytes!("managed_assembly_fixture.dll");
    const UNSAFE_FORWARDER_SOURCE: &[u8] =
        include_bytes!("managed_assembly_unsafe_forwarder_source_fixture.dll");
    const INTEROP_FORWARDER_SOURCE: &[u8] =
        include_bytes!("managed_assembly_interop_services_forwarder_source_fixture.dll");

    #[test]
    fn strong_name_sha1_matches_standard_vectors() {
        fn hex(bytes: &[u8]) -> String {
            bytes.iter().map(|byte| format!("{byte:02x}")).collect()
        }

        assert_eq!(
            hex(&sha1_digest(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hex(&sha1_digest(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn unsafe_forwarder_matches_source_helper_metadata() {
        let mscorlib = include_bytes!("managed_assembly_mscorlib2_fixture.dll");
        let output = ManagedAssembly::create_unsafe_forwarder(
            "mscorlib.dll",
            mscorlib,
            ManagedAssemblyLimits::default(),
        )
        .expect("generated helper assembly serializes");
        std::fs::write("target/unsafe-forwarder-rust.dll", &output)
            .expect("save Rust helper for independent PEReader comparison");
        let source = Resolution::parse(UNSAFE_FORWARDER_SOURCE, ReadOptions::default())
            .expect("source-generated helper parses");
        let generated = Resolution::parse(&output, ReadOptions::default())
            .expect("Rust-generated helper parses");

        let source_assembly = source.assembly.as_ref().expect("source assembly identity");
        let generated_assembly = generated
            .assembly
            .as_ref()
            .expect("generated assembly identity");
        assert_eq!(source_assembly.name, generated_assembly.name);
        assert_eq!(
            (
                source_assembly.version.major,
                source_assembly.version.minor,
                source_assembly.version.build,
                source_assembly.version.revision,
            ),
            (
                generated_assembly.version.major,
                generated_assembly.version.minor,
                generated_assembly.version.build,
                generated_assembly.version.revision,
            )
        );
        assert_eq!(source.module.name, generated.module.name);
        assert_eq!(source.module.mvid, generated.module.mvid);
        assert_eq!(source.assembly_references.len(), 1);
        assert_eq!(generated.assembly_references.len(), 1);
        assert_eq!(
            source.assembly_references[0].name,
            generated.assembly_references[0].name
        );
        assert_eq!(
            (
                source.assembly_references[0].version.major,
                source.assembly_references[0].version.minor,
                source.assembly_references[0].version.build,
                source.assembly_references[0].version.revision,
            ),
            (
                generated.assembly_references[0].version.major,
                generated.assembly_references[0].version.minor,
                generated.assembly_references[0].version.build,
                generated.assembly_references[0].version.revision,
            )
        );
        assert_eq!(
            source.assembly_references[0].public_key_or_token.as_deref(),
            generated.assembly_references[0]
                .public_key_or_token
                .as_deref()
        );
        assert_eq!(source.exported_types.len(), 1);
        assert_eq!(generated.exported_types.len(), 1);
        for exported_type in [&source.exported_types[0], &generated.exported_types[0]] {
            assert_eq!(exported_type.name, "Unsafe");
            assert_eq!(
                exported_type.namespace.as_deref(),
                Some("System.Runtime.CompilerServices")
            );
            assert!(matches!(
                exported_type.flags.accessibility,
                dotnetdll::prelude::TypeAccessibility::NotPublic
            ));
            assert!(matches!(
                exported_type.implementation,
                dotnetdll::prelude::TypeImplementation::TypeForwarder(_)
            ));
        }
        assert_eq!(
            super::managed_pe_is_32_bit("source Unsafe.dll", UNSAFE_FORWARDER_SOURCE).unwrap(),
            super::managed_pe_is_32_bit("Rust Unsafe.dll", &output).unwrap()
        );
    }

    #[test]
    fn interop_forwarder_matches_source_helper_metadata() {
        let mscorlib = include_bytes!("managed_assembly_mscorlib2_fixture.dll");
        let output = ManagedAssembly::create_interop_services_forwarder(
            "mscorlib.dll",
            mscorlib,
            ManagedAssemblyLimits::default(),
        )
        .expect("generated helper assembly serializes");
        std::fs::write("target/interop-forwarder-rust.dll", &output)
            .expect("save Rust helper for independent PEReader comparison");
        let source = Resolution::parse(INTEROP_FORWARDER_SOURCE, ReadOptions::default())
            .expect("source-generated helper parses");
        let generated = Resolution::parse(&output, ReadOptions::default())
            .expect("Rust-generated helper parses");

        let source_assembly = source.assembly.as_ref().expect("source assembly identity");
        let generated_assembly = generated
            .assembly
            .as_ref()
            .expect("generated assembly identity");
        assert_eq!(source_assembly.name, generated_assembly.name);
        assert_eq!(
            (
                source_assembly.version.major,
                source_assembly.version.minor,
                source_assembly.version.build,
                source_assembly.version.revision,
            ),
            (
                generated_assembly.version.major,
                generated_assembly.version.minor,
                generated_assembly.version.build,
                generated_assembly.version.revision,
            )
        );
        assert_eq!(source.module.name, generated.module.name);
        assert_eq!(source.module.mvid, generated.module.mvid);
        assert_eq!(source.assembly_references.len(), 1);
        assert_eq!(generated.assembly_references.len(), 1);
        assert_eq!(
            source.assembly_references[0].name,
            generated.assembly_references[0].name
        );
        assert_eq!(
            source.assembly_references[0].public_key_or_token.as_deref(),
            generated.assembly_references[0]
                .public_key_or_token
                .as_deref()
        );
        assert_eq!(source.exported_types.len(), generated.exported_types.len());
        for (source_type, generated_type) in
            source.exported_types.iter().zip(&generated.exported_types)
        {
            assert_eq!(source_type.namespace, generated_type.namespace);
            assert_eq!(source_type.name, generated_type.name);
            assert!(matches!(
                source_type.implementation,
                dotnetdll::prelude::TypeImplementation::TypeForwarder(_)
            ));
            assert!(matches!(
                generated_type.implementation,
                dotnetdll::prelude::TypeImplementation::TypeForwarder(_)
            ));
        }
        assert_eq!(
            super::managed_pe_is_32_bit("source InteropServices.dll", INTEROP_FORWARDER_SOURCE)
                .unwrap(),
            super::managed_pe_is_32_bit("Rust InteropServices.dll", &output).unwrap()
        );
    }

    #[test]
    fn managed_reference_assembly_writer_preserves_the_source_module_model() {
        assert_writer_matches_source_saved_model(
            "ManagedReferenceFixtureScripts.dll",
            include_bytes!("managed_reference_schema_fixture.dll"),
            include_bytes!("managed_reference_schema_source_saved_fixture.dll"),
            "target/managed-reference-dotnetdll-save.dll",
        );
    }

    #[test]
    fn managed_assembly_writer_preserves_complex_schema_module_model() {
        assert_writer_matches_source_saved_model(
            "ComplexFixtureScripts.dll",
            include_bytes!("managed_assembly_schema_complex_fixture.dll"),
            include_bytes!("managed_assembly_schema_complex_source_saved_fixture.dll"),
            "target/complex-dotnetdll-save.dll",
        );
    }

    #[test]
    fn managed_assembly_writer_preserves_saved_script_module_model() {
        assert_writer_matches_source_saved_model(
            "FixtureScripts.dll",
            include_bytes!("managed_assembly_fixture.dll"),
            include_bytes!("managed_assembly_saved_source_saved_fixture.dll"),
            "target/saved-script-dotnetdll-save.dll",
        );
    }

    fn assert_writer_matches_source_saved_model(
        file_name: &str,
        input: &[u8],
        source_saved: &[u8],
        rust_output_path: &str,
    ) {
        let source_resolution = Resolution::parse(source_saved, ReadOptions::default())
            .expect("source-reserialized assembly parses with method bodies");
        let output = ManagedAssembly::serialize_for_export(
            file_name,
            input,
            ManagedAssemblyLimits::default(),
        )
        .expect("managed assembly serializes as a PE image");
        assert_eq!(
            super::managed_pe_is_32_bit(file_name, &output).unwrap(),
            super::managed_pe_is_32_bit(file_name, source_saved).unwrap(),
            "Rust writer preserves the source PE bitness"
        );
        let reparsed = Resolution::parse(&output, ReadOptions::default())
            .expect("serialized PE can be parsed again");
        std::fs::write(rust_output_path, &output)
            .expect("write the isolated serialization experiment under target");
        let source_model =
            ManagedAssembly::parse(file_name, source_saved, ManagedAssemblyLimits::default())
                .expect("source-saved assembly metadata parses");
        let rust_model =
            ManagedAssembly::parse(file_name, &output, ManagedAssemblyLimits::default())
                .expect("Rust-saved assembly metadata parses");
        assert_eq!(
            rust_model.types, source_model.types,
            "Rust writer preserves every script type and field represented by the source save"
        );
        assert_eq!(
            source_resolution
                .type_definitions
                .iter()
                .flat_map(|ty| ty.methods.iter())
                .filter(|method| method.body.is_some())
                .count(),
            reparsed
                .type_definitions
                .iter()
                .flat_map(|ty| ty.methods.iter())
                .filter(|method| method.body.is_some())
                .count(),
            "writer preserves the number of IL method bodies"
        );
        assert_eq!(
            method_body_rows(&reparsed),
            method_body_rows(&source_resolution),
            "Rust writer preserves source-saved method signatures and IL bodies"
        );
    }

    #[test]
    fn managed_assembly_writer_rejects_malformed_and_unsupported_pe_headers() {
        let malformed = ManagedAssembly::serialize_for_export(
            "malformed.dll",
            b"not a PE image",
            ManagedAssemblyLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            malformed.kind,
            ManagedAssemblyErrorKind::MalformedPeHeader { .. }
        ));

        let mut unsupported_machine =
            include_bytes!("managed_reference_schema_fixture.dll").to_vec();
        let pe_offset = u32::from_le_bytes(
            unsupported_machine[0x3c..0x40]
                .try_into()
                .expect("DOS header has a PE offset"),
        ) as usize;
        unsupported_machine[pe_offset + 4..pe_offset + 6].copy_from_slice(&0x01c0u16.to_le_bytes());
        let unsupported = ManagedAssembly::serialize_for_export(
            "unsupported-machine.dll",
            &unsupported_machine,
            ManagedAssemblyLimits::default(),
        )
        .unwrap_err();
        assert!(matches!(
            unsupported.kind,
            ManagedAssemblyErrorKind::UnsupportedPeMachine { machine: 0x01c0 }
        ));
    }

    fn method_body_rows(resolution: &Resolution<'_>) -> Vec<String> {
        let method_references = resolution
            .method_references
            .iter()
            .enumerate()
            .map(|(index, reference)| {
                let parent = match &reference.parent {
                    MethodReferenceParent::Type(method_type) => method_type.show(resolution),
                    MethodReferenceParent::Module(module_index) => {
                        resolution[*module_index].name.to_string()
                    }
                    MethodReferenceParent::VarargMethod(method_index) => {
                        resolution[method_index.parent_type()].nested_type_name(resolution)
                    }
                };
                (
                    format!("MethodRefIndex({index})"),
                    format!(
                        "{parent}::{}{}",
                        reference.name,
                        reference.signature.show(resolution)
                    ),
                )
            })
            .collect::<Vec<_>>();
        let field_references = resolution
            .field_references
            .iter()
            .enumerate()
            .map(|(index, reference)| {
                let parent = match &reference.parent {
                    FieldReferenceParent::Type(field_type) => field_type.show(resolution),
                    FieldReferenceParent::Module(module_index) => {
                        resolution[*module_index].name.to_string()
                    }
                };
                (
                    format!("FieldRefIndex({index})"),
                    format!(
                        "{parent}::{} {}",
                        reference.name,
                        reference.field_type.show(resolution)
                    ),
                )
            })
            .collect::<Vec<_>>();
        let mut definition_indices = Vec::new();
        let mut type_indices = Vec::new();
        for (type_index, ty) in resolution.type_definitions.iter().enumerate() {
            let owner = ty.nested_type_name(resolution);
            type_indices.push((format!("TypeIndex({type_index})"), format!("Type<{owner}>")));
            for (field_index, field) in ty.fields.iter().enumerate() {
                definition_indices.push((
                    format!(
                        "FieldIndex {{ parent_type: TypeIndex({type_index}), field: {field_index} }}"
                    ),
                    format!("Field<{owner}::{}>", field.name),
                ));
            }
            for (method_index, method) in ty.methods.iter().enumerate() {
                definition_indices.push((
                    format!(
                        "MethodIndex {{ parent_type: TypeIndex({type_index}), member: Method({method_index}) }}"
                    ),
                    format!(
                        "Method<{owner}::{}{}>",
                        method.name,
                        method.signature.show(resolution)
                    ),
                ));
            }
            for (property_index, property) in ty.properties.iter().enumerate() {
                if let Some(method) = &property.getter {
                    definition_indices.push((
                        format!(
                            "MethodIndex {{ parent_type: TypeIndex({type_index}), member: PropertyGetter({property_index}) }}"
                        ),
                        format!(
                            "Method<{owner}::{}{}>",
                            method.name,
                            method.signature.show(resolution)
                        ),
                    ));
                }
                if let Some(method) = &property.setter {
                    definition_indices.push((
                        format!(
                            "MethodIndex {{ parent_type: TypeIndex({type_index}), member: PropertySetter({property_index}) }}"
                        ),
                        format!(
                            "Method<{owner}::{}{}>",
                            method.name,
                            method.signature.show(resolution)
                        ),
                    ));
                }
                for (other_index, method) in property.other.iter().enumerate() {
                    definition_indices.push((
                        format!(
                            "MethodIndex {{ parent_type: TypeIndex({type_index}), member: PropertyOther {{ property: {property_index}, other: {other_index} }} }}"
                        ),
                        format!(
                            "Method<{owner}::{}{}>",
                            method.name,
                            method.signature.show(resolution)
                        ),
                    ));
                }
            }
            for (event_index, event) in ty.events.iter().enumerate() {
                for (member, method) in [
                    ("EventAdd", Some(&event.add_listener)),
                    ("EventRemove", Some(&event.remove_listener)),
                    ("EventRaise", event.raise_event.as_ref()),
                ] {
                    if let Some(method) = method {
                        definition_indices.push((
                            format!(
                                "MethodIndex {{ parent_type: TypeIndex({type_index}), member: {member}({event_index}) }}"
                            ),
                            format!(
                                "Method<{owner}::{}{}>",
                                method.name,
                                method.signature.show(resolution)
                            ),
                        ));
                    }
                }
                for (other_index, method) in event.other.iter().enumerate() {
                    definition_indices.push((
                        format!(
                            "MethodIndex {{ parent_type: TypeIndex({type_index}), member: EventOther {{ event: {event_index}, other: {other_index} }} }}"
                        ),
                        format!(
                            "Method<{owner}::{}{}>",
                            method.name,
                            method.signature.show(resolution)
                        ),
                    ));
                }
            }
        }
        let mut rows = Vec::new();
        for ty in &resolution.type_definitions {
            let owner = ty.nested_type_name(resolution);
            macro_rules! append_method {
                ($method:expr) => {{
                    let method = $method;
                    let mut body = format!("{:?}", method.body);
                    for (index, identity) in &definition_indices {
                        body = body.replace(index, identity);
                    }
                    for (index, identity) in &method_references {
                        body = body.replace(index, &format!("MethodRef<{identity}>"));
                    }
                    for (index, identity) in &field_references {
                        body = body.replace(index, &format!("FieldRef<{identity}>"));
                    }
                    for (index, identity) in &type_indices {
                        body = body.replace(index, identity);
                    }
                    rows.push(format!(
                        "{owner}::{}{}|{:?}|{:?}|{body}",
                        method.name,
                        method.signature.show(resolution),
                        method.body_format,
                        method.body_management
                    ));
                }};
            }
            for method in &ty.methods {
                append_method!(method);
            }
            for property in &ty.properties {
                if let Some(method) = &property.getter {
                    append_method!(method);
                }
                if let Some(method) = &property.setter {
                    append_method!(method);
                }
                for method in &property.other {
                    append_method!(method);
                }
            }
            for event in &ty.events {
                append_method!(&event.add_listener);
                append_method!(&event.remove_listener);
                if let Some(method) = &event.raise_event {
                    append_method!(method);
                }
                for method in &event.other {
                    append_method!(method);
                }
            }
        }
        rows.sort();
        rows
    }

    fn script_id(assembly: &str, namespace: &str, name: &str) -> ScriptIdentifier {
        ScriptIdentifier::try_new(assembly, namespace, name)
            .expect("valid fixture script identifier")
    }

    #[test]
    fn managed_assembly_metadata_resolves_script_types_and_field_attributes() {
        let assembly = ManagedAssembly::parse(
            "Assembly-CSharp.dll",
            ASSEMBLY_BYTES,
            ManagedAssemblyLimits::default(),
        )
        .unwrap();

        assert_eq!(assembly.assembly_name, "FixtureScripts");
        let hero = assembly
            .find_type("Game", "HeroController")
            .expect("fixture HeroController");
        assert!(!hero.is_abstract);
        assert_eq!(hero.base_type.as_deref(), Some("Game.BaseController"));
        assert_eq!(
            hero.fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            ["health", "speed", "ignored", "staticValue", "locked"]
        );
        assert_eq!(hero.fields[0].signature, "int");
        assert!(hero.fields[0].is_public);
        assert!(hero.fields[1]
            .attributes
            .iter()
            .any(|name| name == "UnityEngine.SerializeField"));
        assert!(hero.fields[1].signature.contains("float"));
        assert!(hero.fields[2].is_not_serialized);
        assert!(hero.fields[3].is_static);
        assert!(hero.fields[4].is_init_only);
    }

    #[test]
    fn assembly_catalog_matches_source_type_lookup_keys_and_validity() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Managed/Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();

        assert!(catalog.is_present(&script_id("Assembly-CSharp", "Game", "HeroController")));
        assert!(catalog.is_present(&script_id("FixtureScripts", "Game", "HeroController")));
        assert!(catalog.is_valid(&script_id("Assembly-CSharp", "Game", "HeroController")));
        assert!(catalog.is_present(&script_id("Assembly-CSharp", "Game", "AbstractController")));
        assert!(!catalog.is_valid(&script_id("Assembly-CSharp", "Game", "AbstractController")));
        assert!(!catalog.is_present(&script_id("assembly-csharp", "Game", "HeroController")));
        assert!(!catalog.is_present(&script_id("Assembly-CSharp", "game", "HeroController")));
        assert!(!catalog.is_present(&script_id("Assembly-CSharp", "Game", "MissingController")));
    }

    #[test]
    fn assembly_catalog_replays_the_frozen_source_mono_manager_queries() {
        let fixture = include_str!("managed_assembly_fixture.txt");
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();

        let mut query_count = 0;
        let hero = catalog
            .find_type(&script_id("Assembly-CSharp", "Game", "HeroController"))
            .expect("source fixture resolves HeroController");
        let mut field_count = 0;
        for (line_index, line) in fixture.lines().enumerate() {
            let fields: Vec<_> = line.split('|').collect();
            match fields.first().copied() {
                Some("QUERY") => {
                    assert_eq!(fields.len(), 7, "fixture line {}", line_index + 1);
                    let script_id = ScriptIdentifier::try_new(fields[1], fields[2], fields[3])
                        .unwrap_or_else(|error| panic!("fixture line {}: {error}", line_index + 1));
                    let resolved = catalog.find_type(&script_id);
                    let unique_name = resolved
                        .map(|_| script_id.unique_name())
                        .unwrap_or_else(|| "<default>".to_owned());
                    let present = resolved.is_some();
                    let valid = resolved.is_some_and(|definition| !definition.is_abstract);
                    assert_eq!(unique_name, fields[4], "fixture line {}", line_index + 1);
                    assert_eq!(
                        source_bool(present),
                        fields[5],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(valid),
                        fields[6],
                        "fixture line {}",
                        line_index + 1
                    );
                    query_count += 1;
                }
                Some("FIELD") => {
                    assert_eq!(fields.len(), 9, "fixture line {}", line_index + 1);
                    let actual = hero.fields.get(field_count).unwrap_or_else(|| {
                        panic!("unexpected extra source field on line {}", line_index + 1)
                    });
                    assert_eq!(actual.name, fields[1], "fixture line {}", line_index + 1);
                    assert_eq!(
                        source_clr_type_name(&actual.signature),
                        fields[2],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_public),
                        fields[3],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_static),
                        fields[4],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_literal),
                        fields[5],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_not_serialized),
                        fields[6],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        source_bool(actual.is_init_only),
                        fields[7],
                        "fixture line {}",
                        line_index + 1
                    );
                    assert_eq!(
                        actual.attributes.join(","),
                        fields[8],
                        "fixture line {}",
                        line_index + 1
                    );
                    field_count += 1;
                }
                _ => {}
            }
        }
        assert_eq!(query_count, 6, "all source manager outcomes are replayed");
        assert_eq!(
            field_count,
            hero.fields.len(),
            "all source field rows are replayed"
        );
    }

    fn source_bool(value: bool) -> &'static str {
        if value {
            "True"
        } else {
            "False"
        }
    }

    fn source_clr_type_name(signature: &str) -> &str {
        match signature {
            "bool" => "System.Boolean",
            "char" => "System.Char",
            "sbyte" => "System.SByte",
            "byte" => "System.Byte",
            "short" => "System.Int16",
            "ushort" => "System.UInt16",
            "int" => "System.Int32",
            "uint" => "System.UInt32",
            "long" => "System.Int64",
            "ulong" => "System.UInt64",
            "float" => "System.Single",
            "double" => "System.Double",
            "string" => "System.String",
            "object" => "System.Object",
            other => other,
        }
    }

    #[test]
    fn assembly_catalog_rejects_duplicate_path_aliases_without_replacing_types() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();
        let error = catalog
            .insert(
                "other/Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error.kind,
            ManagedAssemblyErrorKind::DuplicatePathAlias { ref alias, .. }
                if alias == "Assembly-CSharp"
        ));
        assert_eq!(catalog.assemblies().len(), 1);
        assert!(catalog.is_valid(&script_id("Assembly-CSharp", "Game", "HeroController")));
    }

    #[test]
    fn assembly_catalog_reports_unmanaged_and_oversized_inputs_with_context() {
        let malformed = ManagedAssembly::parse(
            "scripts/not-a-managed.dll",
            b"MZ not a valid PE image",
            ManagedAssemblyLimits::default(),
        )
        .unwrap_err();
        assert_eq!(malformed.file_name, "scripts/not-a-managed.dll");
        assert!(matches!(
            malformed.kind,
            ManagedAssemblyErrorKind::MetadataParse(_)
        ));

        let oversized = ManagedAssembly::parse(
            "scripts/too-large.dll",
            &[0; 8],
            ManagedAssemblyLimits {
                max_file_bytes: 7,
                ..ManagedAssemblyLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            oversized.kind,
            ManagedAssemblyErrorKind::FileSizeLimitExceeded {
                file_bytes: 8,
                limit: 7
            }
        ));
    }

    #[test]
    fn mounted_catalog_loads_managed_assemblies_and_skips_native_pe_inputs() {
        assert!(has_cli_directory(ASSEMBLY_BYTES));
        assert!(!has_cli_directory(b"MZ not a valid PE image"));

        let mut filesystem = VirtualFileSystem::new();
        filesystem
            .write_all_bytes("/Assembly-CSharp.dll", ASSEMBLY_BYTES)
            .unwrap();
        filesystem
            .write_all_bytes("/Native.dll", b"MZ native or malformed PE")
            .unwrap();
        let paths = [
            DependencyFilePath {
                file_name: "Assembly-CSharp.dll".to_owned(),
                virtual_path: "/Assembly-CSharp.dll".to_owned(),
            },
            DependencyFilePath {
                file_name: "Native.dll".to_owned(),
                virtual_path: "/Native.dll".to_owned(),
            },
        ];
        let (catalog, report) = super::ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &paths,
            ManagedAssemblyCatalogLimits::default(),
        )
        .unwrap();
        assert_eq!(report.candidate_count, 2);
        assert_eq!(report.loaded_count, 1);
        assert_eq!(report.skipped_non_cli, ["Native.dll"]);
        assert!(report.processed_bytes > ASSEMBLY_BYTES.len() as u64);
        assert!(catalog.contains_assembly("Assembly-CSharp"));
        assert!(catalog.contains_assembly("FixtureScripts"));
        let assembly = catalog.assembly("FixtureScripts").unwrap();
        assert_eq!(assembly.version_major, Some(1));
        assert_eq!(
            assembly.source_virtual_path.as_deref(),
            Some("/Assembly-CSharp.dll")
        );
        assert!(catalog.assembly("mscorlib").is_none());

        let error = super::ManagedAssemblyCatalog::load_from_vfs(
            &filesystem,
            &paths,
            ManagedAssemblyCatalogLimits {
                max_assemblies: 1,
                ..ManagedAssemblyCatalogLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            ManagedAssemblyLoadErrorKind::AssemblyCountLimitExceeded { count: 2, limit: 1 }
        ));
    }

    #[test]
    fn decoded_monoscript_identity_reaches_source_manager_validity_rules() {
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                ASSEMBLY_BYTES,
                ManagedAssemblyLimits::default(),
            )
            .unwrap();
        let object = monoscript_object("Assembly-CSharp", "Game", "HeroController");
        assert_eq!(
            resolve_mono_script_object(&object, &catalog).outcome,
            ManagedScriptResolutionOutcome::Resolved { is_valid: true }
        );
        let abstract_object = monoscript_object("Assembly-CSharp", "Game", "AbstractController");
        assert_eq!(
            resolve_mono_script_object(&abstract_object, &catalog).outcome,
            ManagedScriptResolutionOutcome::Resolved { is_valid: false }
        );
        let missing = monoscript_object("Assembly-CSharp", "Game", "MissingController");
        assert_eq!(
            resolve_mono_script_object(&missing, &catalog).outcome,
            ManagedScriptResolutionOutcome::TypeNotFound
        );
        let absent_assembly = monoscript_object("MissingAssembly", "Game", "HeroController");
        assert_eq!(
            resolve_mono_script_object(&absent_assembly, &catalog).outcome,
            ManagedScriptResolutionOutcome::AssemblyNotLoaded
        );
    }

    fn monoscript_object(assembly: &str, namespace: &str, name: &str) -> WorkspaceObject {
        let string_field = |field_name: &str, value: &str| SerializedFieldValue {
            name: field_name.to_owned(),
            value: SerializedValue::StringBytes(value.as_bytes().to_vec()),
        };
        let payload = ParsedObjectPayload {
            object_index: 0,
            path_id: 7,
            type_name: "MonoScript".to_owned(),
            root: SerializedValue::Struct(vec![
                string_field("m_ClassName", name),
                string_field("m_Namespace", namespace),
                string_field("m_AssemblyName", assembly),
            ]),
            consumed_bytes: 0,
            trailing_bytes: Vec::new(),
            allocated_bytes: 0,
        };
        WorkspaceObject {
            file_index: 0,
            object_index: 0,
            path_id: 7,
            serialized_type_index: Some(0),
            raw_type_id: 115,
            resolved_type_index: Some(0),
            class_id: Some(115),
            script_type_index: None,
            is_destroyed: None,
            is_stripped: Some(false),
            raw_payload_range: 0..0,
            factory: FactoryObjectState {
                object_index: 0,
                path_id: 7,
                class_id: 115,
                kind: FactoryAssetKind::Generated("MonoScript".to_owned()),
                class_name: Cow::Borrowed("MonoScript"),
                name: name.to_owned(),
                tail: Some(ObjectTailOutcome::Exact),
                chinese_texture_fields: None,
                structure: None,
                log_level: FactoryLogLevel::Silent,
                log_message: None,
            },
            recovered_type_tree: None,
            payload: Ok(payload),
        }
    }
