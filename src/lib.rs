#![forbid(unsafe_code)]

//! Independent Rust implementation of AssetRipper.
//!
//! This crate is under active development. Modules are added only as their source behavior
//! is audited and implemented; the presence of this crate does not imply product parity.

pub mod io;
pub mod serialized_file;
