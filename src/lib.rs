#![forbid(unsafe_code)]

//! Independent Rust implementation of AssetRipper.
//!
//! This crate is under active development. Modules are added only as their source behavior
//! is audited and implemented; the presence of this crate does not imply product parity.

pub mod compression;
pub mod container;
pub mod export;
pub mod io;
pub mod json;
pub mod localization;
pub mod numerics;
pub mod processing;
pub mod product;
pub mod resource;
pub mod resources;
pub mod scheme;
pub mod serialized_file;
#[cfg(test)]
mod test_support;
pub mod web_paths;
