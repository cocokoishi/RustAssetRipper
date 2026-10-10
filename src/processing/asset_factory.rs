//! Asset-creation dispatch ported from `AssetRipper.Import.AssetCreation.GameAssetFactory`.
//!
//! Covers the first audited slice of `SER-TT-ABSENT-001`: the `ReadAsset` top dispatch,
//! the `ReadNormalObject` fallback chain with its engine-collection gate, the
//! `TryReadNormalObject` tail rules with their control-flow composition, the `CreateAsset` three outcomes, the `CreateEngineAsset` name switch,
//! the `ReadMonoBehaviour` structure-source decision, the `MakeError_*` and
//! `LogMonoBehaviorReadException` diagnostic texts, the `RawDataObject`/`UnknownObject`/
//! `UnreadableObject` hash and synthetic-name rules, and the `ReadExtraTextureFields`
//! 24-byte tail layout, plus the structure-resolver selection, the player-settings
//! class predicate, the MonoBehaviour structure field-start rule, and the `ReadData`
//! stripped-TypeID fallback with its `ReadAsset` handoff, and the `TypeTreeObject`
//! read/convert plan. Byte reading,
//! node building, and assembly-backed MonoBehaviour structures stay outside: they need
//! generated objects and live assemblies. Version facts arrive precomputed because the
//! Rust tree has no parsed Unity-version model yet.
//!
//! The frozen `AssetFactory.CreateSerialized` class/version selection is now captured
//! as offline data for 404 class IDs across 1,838 source query versions. Deliberately
//! open: per-class generated serializers and generated-object materialization through
//! the product `ObjectWorkspace`/export pipeline; `SerializableStructure` and live-
//! assembly `UnloadedStructure` recovery for MonoBehaviours; and full fidelity for
//! reader tails, stripped-version defaults, and all engine-collection/error paths.
//! The selection table does not stand in for those object and product behaviors.
//!
//! Differential evidence: the `ENGINE` rows of `mono_utils_fixture.txt` (probe
//! `Temp/opencode/monoutilsprobe` resolving all 21 engine names at 2019.4.0f1, SDK
//! 10.0.401); the dispatch and fallback rules are source-read over caller facts.

include!("asset_factory_dispatch.rs");
include!("asset_factory_materialization.rs");

#[cfg(test)]
mod tests {
    include!("asset_factory_tests_01.rs");
}
