use super::model::UnityYamlObjectConversionError;
use super::paths::try_copy_string;
pub(super) fn baseline_type_scoped_field_aliases(
    type_name: &str,
) -> &'static [(&'static str, &'static str)] {
    const ANIMATION_CLIP_ALIASES: &[(&str, &str)] =
        &[("m_AnimationClipSettings", "m_MuscleClipInfo")];
    const GENERIC_BINDING_ALIASES: &[(&str, &str)] = &[("m_TypeID", "m_ClassID")];
    const ANIMATOR_CONTROLLER_ALIASES: &[(&str, &str)] = &[
        ("m_AnimatorEvents", "m_AnimatorParameters"),
        ("m_Layers", "m_AnimatorLayers"),
    ];
    const ANIMATOR_CONTROLLER_LAYER_ALIASES: &[(&str, &str)] = &[("m_HumanMask", "m_Mask")];
    const BLEND_TREE_ALIASES: &[(&str, &str)] = &[
        ("m_BlendEvent", "m_BlendParameter"),
        ("m_BlendEventY", "m_BlendParameterY"),
    ];
    const LIGHTMAP_SETTINGS_ALIASES: &[(&str, &str)] =
        &[("m_LightmapSnapshot", "m_LightingDataAsset")];
    const TEXTURE_3D_ALIASES: &[(&str, &str)] = &[
        ("m_DataSize", "m_CompleteImageSize"),
        ("m_ImageCount", "m_Depth"),
    ];
    const MESH_RENDERER_ALIASES: &[(&str, &str)] = &[("m_StitchSeams", "m_StitchLightmapSeams")];
    const PREFAB_ALIASES: &[(&str, &str)] = &[
        ("m_IsPrefabParent", "m_IsPrefabAsset"),
        ("m_ParentPrefab", "m_SourcePrefab"),
    ];
    const LAYER_CONSTANT_ALIASES: &[(&str, &str)] = &[(
        "m_StateMachineMotionSetIndex",
        "m_StateMachineSynchronizedLayerIndex",
    )];
    const CONTROLLER_CONSTANT_ALIASES: &[(&str, &str)] = &[("m_HumanLayerArray", "m_LayerArray")];
    const STATE_MACHINE_CONSTANT_ALIASES: &[(&str, &str)] =
        &[("m_MotionSetCount", "m_SynchronizedLayerCount")];
    const BUILD_SETTINGS_ALIASES: &[(&str, &str)] = &[("m_Levels", "m_Scenes")];
    const HEIGHTMAP_ALIASES: &[(&str, &str)] = &[
        (
            "m_EnableSurfaceMaskTextureCompression",
            "m_EnableHolesTextureCompression",
        ),
        ("m_SurfaceMask", "m_Holes"),
        ("m_SurfaceMaskLOD", "m_HolesLOD"),
    ];
    const GRAPHICS_SETTINGS_ALIASES: &[(&str, &str)] = &[
        ("m_BuildTargetShaderSettings", "m_TierSettings"),
        ("m_AlbedoSwatchInfos", "m_AlbedoSwitchInfos"),
    ];
    const TEXTURE_IMPORTER_ALIASES: &[(&str, &str)] =
        &[("m_BuildTargetSettings", "m_PlatformSettings")];
    const BUILD_TARGET_SHADER_SETTINGS_ALIASES: &[(&str, &str)] =
        &[("m_ShaderSettings", "m_Settings")];
    const BINDING_ALIASES: &[(&str, &str)] = &[("m_Set", "m_EncodedData")];
    const CONSTANT_BUFFER_PARAMETER_ALIASES: &[(&str, &str)] =
        &[("m_Index", "m_OffsetInConstantBuffer")];
    const COMPUTE_SHADER_VARIANT_ALIASES: &[(&str, &str)] = &[("m_Kernels", "m_KernelParents")];
    const SHADER_ALIASES: &[(&str, &str)] = &[("m_SubProgramBlob", "m_CompressedBlob")];
    const GAME_OBJECT_ALIASES: &[(&str, &str)] = &[("m_Component", "m_Components")];

    match type_name {
        "AnimationClip" => ANIMATION_CLIP_ALIASES,
        "GenericBinding" => GENERIC_BINDING_ALIASES,
        "AnimatorController" => ANIMATOR_CONTROLLER_ALIASES,
        "AnimatorControllerLayer" => ANIMATOR_CONTROLLER_LAYER_ALIASES,
        "BlendTree" => BLEND_TREE_ALIASES,
        "LightmapSettings" => LIGHTMAP_SETTINGS_ALIASES,
        "Texture3D" => TEXTURE_3D_ALIASES,
        "MeshRenderer" => MESH_RENDERER_ALIASES,
        "Prefab" => PREFAB_ALIASES,
        "HumanLayerConstant" | "LayerConstant" => LAYER_CONSTANT_ALIASES,
        "ControllerConstant" => CONTROLLER_CONSTANT_ALIASES,
        "StateMachineConstant" => STATE_MACHINE_CONSTANT_ALIASES,
        "BuildSettings" => BUILD_SETTINGS_ALIASES,
        "Heightmap" => HEIGHTMAP_ALIASES,
        "GraphicsSettings" => GRAPHICS_SETTINGS_ALIASES,
        "TextureImporter" => TEXTURE_IMPORTER_ALIASES,
        "BuildTargetShaderSettings" => BUILD_TARGET_SHADER_SETTINGS_ALIASES,
        "Binding" => BINDING_ALIASES,
        "MatrixParameter" | "StructParameter" | "VectorParameter" => {
            CONSTANT_BUFFER_PARAMETER_ALIASES
        }
        "ComputeShaderPlatformVariant" => COMPUTE_SHADER_VARIANT_ALIASES,
        "Shader" => SHADER_ALIASES,
        "GameObject" => GAME_OBJECT_ALIASES,
        _ => &[],
    }
}

pub(super) fn has_conditional_field_aliases(type_name: &str) -> bool {
    matches!(
        type_name,
        "ParticleSystem" | "ShapeModule" | "ClipMuscleConstant" | "Xform" | "ValueArray" | "Mesh"
    )
}

pub(super) fn baseline_type_scoped_field_alias(
    parent_type: &str,
    field_name: &str,
    field_type: &str,
) -> Option<&'static str> {
    match (parent_type, field_name) {
        ("ClipMuscleConstant", "m_AverageSpeed") => match field_type {
            "Vector4Float" | "float4" => Some("m_AverageSpeed4"),
            "Vector3Float" | "float3" => Some("m_AverageSpeed3"),
            _ => None,
        },
        ("Xform", "m_T") => match field_type {
            "Vector4Float" | "float4" => Some("m_T4"),
            "Vector3Float" | "float3" => Some("m_T3"),
            _ => None,
        },
        ("Xform", "m_S") => match field_type {
            "Vector4Float" | "float4" => Some("m_S4"),
            "Vector3Float" | "float3" => Some("m_S3"),
            _ => None,
        },
        ("ParticleSystem", "m_StartDelay") => Some(if field_type == "float" {
            "m_StartDelay_Single"
        } else {
            "m_StartDelay_MinMaxCurve"
        }),
        ("ShapeModule", "m_Arc") if field_type != "float" => Some("m_ArcParameter"),
        ("ShapeModule", "m_Radius") if field_type != "float" => Some("m_RadiusParameter"),
        ("Mesh", "m_Shapes") if field_type == "vector" => Some("m_ShapesList"),
        _ => baseline_type_scoped_field_aliases(parent_type)
            .iter()
            .find(|(source_name, _)| *source_name == field_name)
            .map(|(_, alias)| *alias),
    }
}

pub(super) fn unsupported_contextual_field_alias_reason(
    parent_type: &str,
    field_name: &str,
    field_type: &str,
) -> Option<&'static str> {
    let recognized_type = match (parent_type, field_name) {
        ("ClipMuscleConstant", "m_AverageSpeed") => {
            matches!(
                field_type,
                "Vector4Float" | "Vector3Float" | "float4" | "float3"
            )
        }
        ("Xform", "m_T" | "m_S") => {
            matches!(
                field_type,
                "Vector4Float" | "Vector3Float" | "float4" | "float3"
            )
        }
        _ => true,
    };
    (!recognized_type).then_some("unsupported child TypeTree type for baseline field alias")
}

pub(super) fn baseline_yaml_field_name(
    field_name: &str,
) -> Result<String, UnityYamlObjectConversionError> {
    let replacement = match field_name {
        "m_Image_data" => Some("m_ImageData"),
        "m_TextureFormat" => Some("m_Format"),
        "m_TextureDimension" => Some("m_Dimension"),
        "m_ObjectHideFlags" => Some("m_HideFlags"),
        "m_Val" => Some("m_Value"),
        _ => None,
    };
    if let Some(replacement) = replacement {
        return try_copy_string(replacement, "Unity baseline YAML field name");
    }

    for (source_prefix, output_prefix) in [
        ("m_Dst", "m_Destination"),
        ("m_Dest", "m_Destination"),
        ("m_Src", "m_Source"),
    ] {
        let Some(suffix) = field_name.strip_prefix(source_prefix) else {
            continue;
        };
        if !baseline_prefix_has_uppercase_initial(suffix) {
            continue;
        }
        let requested = output_prefix.len().checked_add(suffix.len()).ok_or(
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Unity baseline YAML field name",
                requested: usize::MAX,
            },
        )?;
        let mut output_name = String::new();
        output_name.try_reserve_exact(requested).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Unity baseline YAML field name",
                requested,
            }
        })?;
        output_name.push_str(output_prefix);
        output_name.push_str(suffix);
        return Ok(output_name);
    }

    try_copy_string(field_name, "Unity baseline YAML field name")
}

pub(super) fn baseline_prefix_has_uppercase_initial(suffix: &str) -> bool {
    suffix
        .encode_utf16()
        .next()
        .and_then(|unit| char::from_u32(u32::from(unit)))
        .is_some_and(char::is_uppercase)
}
