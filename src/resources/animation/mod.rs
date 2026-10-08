//! Animation processing modules added by the animation workstream.
//!
//! Submodules here hold the wiring, caches, and conversion steps that connect the ported
//! clip/controller/avatar kernels to materialized assets. Existing animation modules live one
//! level up in `src/resources/` and belong to the same workstream; this directory exists so
//! new animation modules can be registered without touching `src/resources/mod.rs`, which the
//! resource-exporter workstream owns.
//!
//! The conversion pipeline is:
//!
//! 1. A caller materializes an asset graph and fills [`scene_facts::SceneFactsTable`].
//! 2. [`scene_facts::build_path_checksum_cache`] reproduces `PathChecksumCache.BuildPathsCache`
//!    (avatar TOS, animator/animation hierarchy paths).
//! 3. A mutable [`clip_object::AnimationClipObject`] carries the serialized clip data.
//! 4. [`converter::process_clip`] runs the full `AnimationClipConverter` sequence and emits
//!    curves into the clip object.

pub mod clip_object;
pub mod converter;
pub mod field_hashes;
pub mod scene_facts;
