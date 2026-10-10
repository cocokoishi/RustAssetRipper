//! Source-derived asset factory decisions and SerializedFile object materialization.

mod decisions;
mod layouts;
mod materialize;
mod model;

pub use self::decisions::{
    bump_patch_build, chinese_texture_warning_message, classify_created_asset,
    classify_object_tail, classify_read_data_path, classify_read_path,
    decide_monobehaviour_structure, decide_normal_object, decide_try_read_object,
    engine_asset_kind, incorrect_byte_count_message, is_engine_collection_name,
    is_player_settings_class, monobehaviour_layout_mismatch_message,
    monobehaviour_structure_start_index, raw_data_hash, read_exception_message,
    resolve_read_class_id, select_structure_resolver, select_typetree_read_plan,
    unknown_asset_name, unreadable_asset_name, AssetReadPath, CreatedAssetKind, EngineAssetKind,
    MonoBehaviourReadFacts, MonoBehaviourReadLog, MonoBehaviourStructureOutcome, NormalObjectLog,
    NormalObjectOutcome, NormalReadFacts, ObjectTailOutcome, StructureResolverKind, TryReadFacts,
    TryReadLog, TryReadOutcome, TypeTreeReadPlan, CHINESE_TEXTURE_EXTRA_BYTES,
    CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET, CHINESE_TEXTURE_ORIGINAL_GUID_SIZE,
    CHINESE_TEXTURE_ORIGINAL_HEIGHT_OFFSET, CHINESE_TEXTURE_ORIGINAL_WIDTH_OFFSET,
    MONO_BEHAVIOUR_CLASS_ID, MONO_BEHAVIOUR_EDITOR_CLASS_ID_FIELD, MONO_BEHAVIOUR_NAME_FIELD,
    PLAYER_SETTINGS_CLASS_ID,
};
pub use self::layouts::{
    decode_no_usable_type_tree_payload, materialize_file_objects, reference_class_layouts,
    ReferenceClassLayouts, TypeTreePayloadRecovery, PROBE_FIXTURE,
};
pub use self::materialize::{
    materialize_object, materialize_object_for_workspace, object_read_class_id,
};
pub use self::model::{
    ChineseTextureFields, ClassLayoutSource, FactoryAsset, FactoryAssetKind, FactoryEngineError,
    FactoryFileFacts, FactoryLogLevel, FactoryObjectMaterialization, FactoryObjectState,
    FactoryStructure, FactoryUnityVersion, FactoryVersionType,
};
pub use crate::processing::source_factory_dispatch::SourceFactoryLookupError;
pub use crate::processing::source_tpk::{SourceTpkLookupError, SourceTpkTrees};

#[cfg(test)]
pub(crate) use self::model::FileTypeTreeTestLayouts;

#[cfg(test)]
#[path = "asset_factory/tests/mod.rs"]
mod tests;
