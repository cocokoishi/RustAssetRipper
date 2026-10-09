//! Full `AnimationClipConverter.Process` replay over the frozen source-runtime fixture.
//!
//! The input below is the deterministic generated-object setup in
//! `tools/probes/anim-convert/Program.cs::RunConvert`; the expected output is captured by the
//! shipped AssetRipper.Processing runtime in `clip_convert_fixture.txt`. This checks orchestration
//! across streamed, dense, and constant curves, scene lookups, binding classification, events,
//! and PPtr curves in one comparison. It does not replace the missing ObjectWorkspace caller.

use std::fmt::Write as _;

use crate::io::dependency::DependencyFilePath;
use crate::io::filesystem::VirtualFileSystem;
use crate::numerics::{Quat, Vec3};
use crate::resources::animation::clip_object::{
    AnimationClipObject, BindingObject, ClipEventObject, ConstantClipObject, DenseClipObject,
    MuscleClipObject, PPtrValue, StreamedClipObject,
};
use crate::resources::animation::converter::{process_clip, ConverterContext};
use crate::resources::animation::scene_facts::{
    build_path_checksum_cache, GameObjectFacts, PathCacheAsset, SceneFactsTable,
};
use crate::resources::muscle_clip_info::{MuscleClipConstantView, MuscleClipInfoView};
use crate::resources::path_checksum::{PathChecksumCache, PathChecksumLimits, ProcessedScriptSet};
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
use crate::serialized_file::object::SerializedValue;
use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
use crate::serialized_file::workspace::{SerializedFileWorkspace, SerializedFileWorkspaceLimits};

#[test]
fn complete_clip_process_matches_the_source_generated_asset() {
    let (mut clip, scene, path_assets) = source_probe_input();
    let mut checksum_cache = PathChecksumCache::new(PathChecksumLimits::default()).unwrap();
    build_path_checksum_cache(&mut checksum_cache, &path_assets, &scene).unwrap();
    let mut processed_scripts = ProcessedScriptSet::default();
    process_clip(
        &mut clip,
        &mut ConverterContext {
            scene: &scene,
            checksum_cache: &mut checksum_cache,
            processed_scripts: &mut processed_scripts,
        },
    )
    .unwrap();

    let expected = include_str!("clip_convert_fixture.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(format_clip(&clip), expected);
}

#[test]
fn source_generated_nondefault_animation_clip_materializes_through_object_workspace() {
    let path = "/animation/animation_workspace.assets";
    let bytes = include_bytes!("animation_workspace_fixture.assets");
    let mut filesystem = VirtualFileSystem::new();
    filesystem
        .create_directory(Some("/animation"))
        .expect("create fixture directory");
    filesystem
        .write_all_bytes(path, bytes)
        .expect("write fixture to the test VFS");
    let workspace = SerializedFileWorkspace::load_from_vfs(
        &filesystem,
        &[DependencyFilePath {
            file_name: "animation_workspace.assets".to_owned(),
            virtual_path: path.to_owned(),
        }],
        SerializedFileWorkspaceLimits::default(),
    )
    .expect("source-generated SerializedFile parses and scans");

    assert_eq!(workspace.files().len(), 1);
    assert_eq!(workspace.files()[0].metadata.objects.len(), 13);
    assert!(workspace.files()[0]
        .metadata
        .types
        .iter()
        .all(|serialized_type| serialized_type
            .type_tree
            .as_ref()
            .is_none_or(|tree| tree.nodes.is_empty())));
    assert!(workspace.files()[0].reference_graph.is_complete());
    assert_eq!(workspace.files()[0].reference_graph.references().len(), 58);

    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .expect("ObjectWorkspace uses the source-generated TPK layouts");
    assert_eq!(objects.report().object_count, 13);
    assert!(objects.report().is_complete());
    let clip = objects
        .objects()
        .iter()
        .find(|object| object.factory.class_id == 74)
        .expect("generated AnimationClip object");
    let payload = clip
        .payload
        .as_ref()
        .expect("AnimationClip payload materializes");

    let sample_rate = unique_field(&payload.root, "m_SampleRate");
    assert!(matches!(sample_rate, SerializedValue::F32(value) if *value == 30.0));

    let binding_constant = unique_field(&payload.root, "m_ClipBindingConstant");
    assert_eq!(array_field(binding_constant, "genericBindings").len(), 23);
    let pptr_mapping = array_field(binding_constant, "pptrCurveMapping");
    assert_eq!(pptr_mapping.len(), 2);
    assert!(matches!(
        pptr_mapping[0],
        SerializedValue::PPtr {
            file_id: 0,
            path_id: 1010
        }
    ));
    assert!(matches!(
        pptr_mapping[1],
        SerializedValue::PPtr {
            file_id: 0,
            path_id: 0
        }
    ));

    let events = array_field(&payload.root, "m_Events");
    assert_eq!(events.len(), 1);
    assert_eq!(
        string_field(events.first().unwrap(), "functionName"),
        "OnThing"
    );
    assert_eq!(string_field(events.first().unwrap(), "data"), "payload");

    let muscle_clip = unique_field(&payload.root, "m_MuscleClip");
    let clip_data = unique_field(unique_field(muscle_clip, "m_Clip"), "data");
    let streamed_data = array_field(unique_field(clip_data, "m_StreamedClip"), "data");
    assert!(!streamed_data.is_empty());
    let dense_clip = unique_field(clip_data, "m_DenseClip");
    assert!(matches!(
        unique_field(dense_clip, "m_FrameCount"),
        SerializedValue::I32(2)
    ));
    let dense_samples = array_field(dense_clip, "m_SampleArray");
    assert_eq!(dense_samples.len(), 6);
    assert!(matches!(dense_samples.first(), Some(SerializedValue::F32(value)) if *value == 100.0));
    let constant_samples = array_field(unique_field(clip_data, "m_ConstantClip"), "data");
    assert_eq!(constant_samples.len(), 2);
    assert!(
        matches!(constant_samples.first(), Some(SerializedValue::F32(value)) if *value == 777.0)
    );
}

fn unique_field<'a>(value: &'a SerializedValue, name: &str) -> &'a SerializedValue {
    value
        .unique_field_in_base_chain(name)
        .expect("field lookup is unambiguous")
        .unwrap_or_else(|| panic!("missing serialized field {name}"))
}

fn array_field<'a>(value: &'a SerializedValue, name: &str) -> &'a [SerializedValue] {
    match unique_field(value, name) {
        SerializedValue::Array(items) => items,
        other => panic!("serialized field {name} is not an array: {other:?}"),
    }
}

fn string_field<'a>(value: &'a SerializedValue, name: &str) -> &'a str {
    match unique_field(value, name) {
        SerializedValue::StringBytes(bytes) => {
            std::str::from_utf8(bytes).expect("source-generated string is UTF-8")
        }
        other => panic!("serialized field {name} is not a string: {other:?}"),
    }
}

fn source_probe_input() -> (AnimationClipObject, SceneFactsTable, Vec<PathCacheAsset>) {
    let version = UnityVersion::parse("2022.3.0f1").unwrap();
    let paths = [
        "Root", "Root", "Root", "Arm", "Root", "Leg", "Root", "Root", "Arm", "Leg", "Leg", "Leg",
        "Root", "Root", "Root", "Root", "Root", "Root", "Root", "Root", "Leg", "Leg", "Root",
    ];
    let binding_specs: [(i32, u32, u8, bool); 23] = [
        (4, 1, 4, false),
        (4, 2, 4, false),
        (4, 3, 4, false),
        (4, 4, 2, false),
        (1, crc("m_IsActive"), 0, false),
        (23, crc("m_Enabled"), 0, false),
        (23, 0xDEAD_BEEF, 0, false),
        (114, crc("myField"), 0, false),
        (137, 0xC34B_2C0B, 20, false),
        (23, 0x8686_C589, 22, false),
        (23, 3, 21, false),
        (212, 0, 23, false),
        (114, 0x1234_5678, 40, false),
        (181, 4, 31, false),
        (95, 42, 8, false),
        (198, 0x9242_84C0, 27, false),
        (20, 0x9B5A_57EC, 0, true),
        (224, 1, 4, false),
        (4, 1, 4, false),
        (23, crc("m_ReceiveShadows"), 0, false),
        (23, crc("m_LightmapIndex"), 0, false),
        (23, crc("m_CastShadows"), 0, false),
        (114, 0x9299_AC34, 24, false),
    ];
    let bindings = paths
        .iter()
        .zip(binding_specs)
        .map(
            |(path, (class_id, attribute, custom_type, is_pptr_curve))| BindingObject {
                path: crc(path),
                attribute,
                class_id,
                custom_type,
                is_pptr_curve,
                script: None,
            },
        )
        .collect();

    // The source probe writes StreamedFrame values with BinaryWriter (little endian), then stores
    // those bytes as the generated StreamedClip.Data UInt32 words on a little-endian host.
    let mut streamed_bytes = Vec::new();
    write_frame(&mut streamed_bytes, f32::MIN, &[(25, 0.0, 0.0, 0.0, 0.0)]);
    let mut frame1 = vec![
        (0, 0.1, 0.2, 0.3, 1.0),
        (1, 0.0, 0.0, 0.0, 2.0),
        (2, 0.0, 0.0, 0.0, 3.0),
        (3, 0.1, 0.2, 0.3, 0.5),
    ];
    for index in 4..=24 {
        frame1.push((index, 0.0, 0.0, 0.0, 10.0 + index as f32));
    }
    for index in 26..=28 {
        frame1.push((index, 0.0, 0.0, 0.0, 40.0 + index as f32));
    }
    write_frame(&mut streamed_bytes, 1.0 / 30.0, &frame1);
    let mut frame2 = vec![
        (13, 0.0, 0.0, 0.0, 99.0),
        (13, 0.0, 0.0, 0.0, 8.0),
        (0, 0.0, 0.0, 0.0, 1.5),
        (1, 0.0, 0.0, 0.0, 2.0),
        (2, 0.0, 0.0, 0.0, 4.0),
    ];
    for index in 3..=24 {
        frame2.push((index, 0.0, 0.0, 0.0, 20.0 + index as f32));
    }
    for index in 26..=28 {
        frame2.push((index, 0.0, 0.0, 0.0, 50.0 + index as f32));
    }
    write_frame(&mut streamed_bytes, 2.0 / 30.0, &frame2);
    write_frame(&mut streamed_bytes, f32::INFINITY, &[]);
    let streamed_words = streamed_bytes
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect();

    let constant_view = MuscleClipConstantView {
        start_time: 0.0,
        stop_time: 2.0,
        level: 0.25,
        loop_time: true,
        ..MuscleClipConstantView::default()
    };
    let clip = AnimationClipObject {
        version,
        endian: EndianType::Little,
        has_clip_binding_constant: true,
        bindings,
        pptr_curve_mapping: vec![
            Some(PPtrValue {
                file_id: 0,
                path_id: 1010,
            }),
            None,
        ],
        muscle_clip: MuscleClipObject {
            streamed: StreamedClipObject {
                data: streamed_words,
                curve_count_u32: Some(29),
                ..StreamedClipObject::default()
            },
            dense: DenseClipObject {
                frame_count: 2,
                curve_count: 3,
                sample_rate: 30.0,
                begin_time: 0.5,
                sample_array: (100..106).map(|value| value as f32).collect(),
            },
            has_constant_clip: true,
            constant: ConstantClipObject {
                data: vec![777.0, 888.0],
            },
            constant_view,
        },
        muscle_clip_info: MuscleClipInfoView::default(),
        has_euler_curves: true,
        has_pptr_curves: true,
        sample_rate: 30.0,
        has_legacy: true,
        legacy: false,
        animation_type: 0,
        position_curves: Vec::new(),
        rotation_curves: Vec::new(),
        scale_curves: Vec::new(),
        euler_curves: Vec::new(),
        float_curves: Vec::new(),
        pptr_curves: Vec::new(),
        events: vec![ClipEventObject {
            function_name: "OnThing".to_owned(),
            data: "payload".to_owned(),
            float_parameter: 1.5,
            int_parameter: 7,
            time: 0.75,
            ..ClipEventObject::default()
        }],
    };

    let mut scene = SceneFactsTable::new();
    scene.set_roots(vec![1010]);
    scene.insert_game_object(
        1010,
        GameObjectFacts {
            name: "Root".to_owned(),
            components: vec![(4, 1020), (95, 1090)],
            children: vec![1011, 1012],
        },
    );
    scene.insert_game_object(
        1011,
        GameObjectFacts {
            name: "Arm".to_owned(),
            components: vec![(4, 1021), (137, 1030)],
            children: Vec::new(),
        },
    );
    scene.insert_game_object(
        1012,
        GameObjectFacts {
            name: "Leg".to_owned(),
            components: vec![(4, 1022), (23, 1050)],
            children: Vec::new(),
        },
    );
    scene.insert_skinned_mesh(1030, 1040);
    let path_assets = vec![PathCacheAsset::Animator {
        avatar_tos: Vec::new(),
        has_has_transform_hierarchy: true,
        has_transform_hierarchy: true,
        game_object: Some(1010),
    }];
    (clip, scene, path_assets)
}

fn write_frame(output: &mut Vec<u8>, time: f32, keys: &[(i32, f32, f32, f32, f32)]) {
    output.extend_from_slice(&time.to_le_bytes());
    output.extend_from_slice(&(keys.len() as i32).to_le_bytes());
    for (index, coefficient_x, coefficient_y, coefficient_z, right_limit) in keys {
        output.extend_from_slice(&index.to_le_bytes());
        output.extend_from_slice(&coefficient_x.to_le_bytes());
        output.extend_from_slice(&coefficient_y.to_le_bytes());
        output.extend_from_slice(&coefficient_z.to_le_bytes());
        output.extend_from_slice(&right_limit.to_le_bytes());
    }
}

fn crc(value: &str) -> u32 {
    crate::resources::blend_shape::crc32_ieee(value.as_bytes())
}

fn format_clip(clip: &AnimationClipObject) -> String {
    let mut output = String::new();
    writeln!(
        output,
        "INFO|sample_rate|{:08X}",
        clip.sample_rate.to_bits()
    )
    .unwrap();
    let info = clip.muscle_clip_info;
    write!(
        output,
        "MINFO|{:08X}|{:08X}|{:08X}|{:08X}|{:08X}|{:08X}",
        info.start_time.to_bits(),
        info.stop_time.to_bits(),
        info.orientation_offset_y.to_bits(),
        info.level.to_bits(),
        info.cycle_offset.to_bits(),
        info.additive_reference_pose_time.to_bits()
    )
    .unwrap();
    for value in [
        info.loop_time,
        info.loop_blend,
        info.loop_blend_orientation,
        info.loop_blend_position_y,
        info.loop_blend_position_xz,
        info.keep_original_orientation,
        info.keep_original_position_y,
        info.keep_original_position_xz,
        info.height_from_feet,
        info.has_additive_reference_pose,
        info.mirror,
    ] {
        write!(output, "|{}", u8::from(value)).unwrap();
    }
    output.push('\n');

    for (index, event) in clip.events.iter().enumerate() {
        writeln!(
            output,
            "EVENT|{index}|{}|{}|{:08X}|{}|{:08X}",
            event.function_name,
            event.data,
            event.float_parameter.to_bits(),
            event.int_parameter,
            event.time.to_bits()
        )
        .unwrap();
    }
    format_vector3_curves(&mut output, "POS", &clip.position_curves);
    format_quaternion_curves(&mut output, "ROT", &clip.rotation_curves);
    format_vector3_curves(&mut output, "SCL", &clip.scale_curves);
    format_vector3_curves(&mut output, "EUL", &clip.euler_curves);
    format_float_curves(&mut output, clip);
    for (index, curve) in clip.pptr_curves.iter().enumerate() {
        writeln!(
            output,
            "PPTR|{index}|{}|{}|{}|{}",
            curve.path, curve.attribute, curve.class_id, curve.flags
        )
        .unwrap();
        for key in &curve.curve {
            let value = key.value.unwrap_or_default();
            writeln!(
                output,
                "PK|{index}|{:08X}|{}|{}",
                key.time.to_bits(),
                value.file_id,
                value.path_id
            )
            .unwrap();
        }
    }
    output.pop();
    output
}

fn format_vector3_curves(
    output: &mut String,
    tag: &str,
    curves: &[super::clip_object::Vector3CurveObject],
) {
    for (index, curve) in curves.iter().enumerate() {
        writeln!(
            output,
            "{tag}|{index}|{}|{}|{}|{}",
            curve.path,
            curve.envelope.pre_infinity,
            curve.envelope.post_infinity,
            curve.envelope.rotation_order.unwrap_or(-1)
        )
        .unwrap();
        for key in &curve.curve {
            write!(output, "{tag}K|{index}|{:08X}", key.time.to_bits()).unwrap();
            write_vec3(output, key.value);
            write_vec3(output, key.in_slope);
            write_vec3(output, key.out_slope);
            write_vec3(output, key.in_weight.unwrap_or(key.in_slope));
            write_vec3(output, key.out_weight.unwrap_or(key.out_slope));
            writeln!(
                output,
                "|{}|{}",
                key.tangent_mode,
                key.weighted_mode.unwrap_or(-1)
            )
            .unwrap();
        }
    }
}

fn format_quaternion_curves(
    output: &mut String,
    tag: &str,
    curves: &[super::clip_object::QuaternionCurveObject],
) {
    for (index, curve) in curves.iter().enumerate() {
        writeln!(
            output,
            "{tag}|{index}|{}|{}|{}|{}",
            curve.path,
            curve.envelope.pre_infinity,
            curve.envelope.post_infinity,
            curve.envelope.rotation_order.unwrap_or(-1)
        )
        .unwrap();
        for key in &curve.curve {
            write!(output, "{tag}K|{index}|{:08X}", key.time.to_bits()).unwrap();
            write_quat(output, key.value);
            write_quat(output, key.in_slope);
            write_quat(output, key.out_slope);
            write_quat(output, key.in_weight.unwrap_or(key.in_slope));
            write_quat(output, key.out_weight.unwrap_or(key.out_slope));
            writeln!(
                output,
                "|{}|{}",
                key.tangent_mode,
                key.weighted_mode.unwrap_or(-1)
            )
            .unwrap();
        }
    }
}

fn format_float_curves(output: &mut String, clip: &AnimationClipObject) {
    for (index, curve) in clip.float_curves.iter().enumerate() {
        writeln!(
            output,
            "FLOAT|{index}|{}|{}|{}|{}|{}|{}|{}",
            curve.path,
            curve.attribute,
            curve.class_id,
            // The generated `Script` PPtr struct exists even when its target is null; source
            // `PathID__Int64` therefore reads as zero rather than an absent field.
            curve.script.map_or(0, |script| script.path_id),
            curve.envelope.pre_infinity,
            curve.envelope.post_infinity,
            curve.envelope.rotation_order.unwrap_or(-1)
        )
        .unwrap();
        for key in &curve.curve {
            writeln!(
                output,
                "FK|{index}|{:08X}|{:08X}|{:08X}|{:08X}|{}|{}|{}|{}",
                key.time.to_bits(),
                key.value.to_bits(),
                key.in_slope.to_bits(),
                key.out_slope.to_bits(),
                optional_float_bits(key.in_weight),
                optional_float_bits(key.out_weight),
                key.tangent_mode,
                key.weighted_mode.unwrap_or(-1)
            )
            .unwrap();
        }
    }
}

fn optional_float_bits(value: Option<f32>) -> String {
    value.map_or_else(
        || "-1".to_owned(),
        |value| format!("{:08X}", value.to_bits()),
    )
}

fn write_vec3(output: &mut String, value: Vec3) {
    write!(
        output,
        "|{:08X}|{:08X}|{:08X}",
        value.x.to_bits(),
        value.y.to_bits(),
        value.z.to_bits()
    )
    .unwrap();
}

fn write_quat(output: &mut String, value: Quat) {
    write!(
        output,
        "|{:08X}|{:08X}|{:08X}|{:08X}",
        value.x.to_bits(),
        value.y.to_bits(),
        value.z.to_bits(),
        value.w.to_bits()
    )
    .unwrap();
}
