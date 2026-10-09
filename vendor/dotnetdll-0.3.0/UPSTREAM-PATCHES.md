# Local patches to dotnetdll 0.3.0

This directory contains the upstream crates.io source package for dotnetdll 0.3.0 with two documented source fixes and its upstream `LICENSE` retained. The package is selected through the workspace Cargo.toml path override and remains under its upstream GPL-3.0-or-later license.

The local changes are:

1. `src/resolution/read.rs` maps ECMA-335 `MethodImplAttributes.ManagedMask` value 0 to managed and value 4 to unmanaged, matching the writer and the standard.
2. `src/resolution/write.rs` records the next MethodDef row as every TypeDef's method-list start. The row includes regular methods and property/event accessors; an empty direct-method list does not mean the type owns no methods.

These defects were observable in a source-generated Unity 2019.4 managed assembly: the upstream reader/writer changed a property getter's implementation flags, and the upstream writer attributed a constructor to the wrong type. Rust regression coverage compares the saved PE model with isolated AssetRipper output. Do not remove either patch without rerunning those differential checks.
