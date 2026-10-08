# RustAssetRipper

This repository is the independent Rust port of the local AssetRipper 2.0.0 source snapshot. The port is in its first phase; it is not yet a usable AssetRipper product and must not be treated as having full format or export coverage.

The current implementation units are a bounded binary reader, a read-only complete SerializedFile entry point, versioned header and metadata parsers, TypeTree records with the audited Unity common-string table, checked object-data ranges, and a bounded TypeTree-driven object payload decoder. UnityFS has partial header, metadata, entry, LZ4, LZMA, and Zstandard support. Legacy UnityRaw and UnityWeb have partial versioned header, directory, and entry parsing; UnityWeb's size-prefixed LZMA chunk is bounded and checked. Unity WebData 1.0 directory parsing and writing use bounded offsets and entry sizes. The source-compatible scheme dispatcher recognizes the current container parsers and recursively decodes bounded GZip/Brotli wrappers and WebData entries. Committed isolated source-runtime differentials cover the listed IO scheme/container lanes and selected file-backed paths; this is not Unity-produced or user-sample evidence, and broader parser/version coverage remains incomplete. Codec dependencies are pure Rust; the Rust product does not invoke AssetRipper, UABEA, .NET, or another Unity parser. This parser work is still only a small part of the full product migration.

## Development

The manifest declares Rust 1.87 as its minimum because the selected pure-Rust Zstandard decoder requires that toolchain. The current code has been checked with Rust/Cargo 1.98.1 on `stable-aarch64-pc-windows-msvc`; Rust 1.87 itself has not been run. If Cargo is on `PATH`, run:

```powershell
cargo fmt -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

On this workstation Cargo is installed under `%USERPROFILE%\.cargo\bin` but that directory is not on `PATH`; in PowerShell, prefix each command with `& "$env:USERPROFILE\.cargo\bin\cargo.exe"`. The latest Rust run and outstanding validation status are recorded in the [validation log](docs/validation-log.md). The isolated AssetRipper reference solution builds with .NET SDK 10.0.401; its suite reports 339 passes and 20 CRLF-versus-LF assertion failures, documented in the handoff. See also the [continuation status](docs/continuation.md) and [dependency/license record](docs/dependencies-and-licenses.md).

## Current command-line entry point

The partial CLI provides directory inspection, explicit-file inspection, and an initial YAML project export. `inspect` mounts local input directories, detects a platform layout when possible, loads recognized SerializedFiles and their external dependencies, and prints a structural summary. `inspect-files` mounts explicitly selected local file paths in input order, registers top-level files dispatched as ResourceFiles as GameBundle root resources, then inspects any selected SerializedFiles:

```powershell
cargo run -- inspect <input-directory> [input-directory ...]
cargo run -- inspect-files <input-file> [input-file ...]
cargo run -- export-project <new-empty-output-directory> <input-file-or-directory> [input-file-or-directory ...]
```

`export-project` loads mixed file and directory inputs, resolves workspace references and streamed resources, and writes processed Scene collections as `.unity` YAML with `DefaultImporter` `.meta` files and modern processed Prefab hierarchies as `.prefab` YAML with `PrefabImporter` metadata. Generated Prefab Transforms and the hierarchy's object pointers are included in the collection export plan. Other routed objects currently use generic YAML `.asset` outputs, followed by `ProjectSettings/ProjectVersion.txt` and a default `Packages/manifest.json`. Duplicate BuildSettings paths keep the first Scene output. The baseline generated Scene hierarchy's effective `AssetBundleName` remains null, so its importer bundle-name field is empty. AssetBundle-derived Prefab importer metadata, generated legacy Prefab payloads, class-specific exporter/importer families, full settings application, progress/cancel behavior, and the source product's GUI and remaining workflows are unfinished. The workflow stages output before writing and rejects undecoded objects, unresolved references, or objects omitted by YAML conversion. The repository is not yet a complete AssetRipper replacement. Root ResourceFiles are registered and reported; streamed Mesh and image-texture bytes are materialized for bounded YAML routes, including Texture2D. Other resource classes and complete exporter integration remain unfinished.

`cargo run -- --help` and `cargo run -- --version` show command help and version. The local .NET SDK is available at `C:\Users\koishi\AppData\Local\Microsoft\dotnet\dotnet.exe`; the isolated reference solution builds, while 20 reference tests have documented CRLF/LF assertion differences. Baseline integrity and Rust validation status are recorded in the [validation log](docs/validation-log.md) and [handoff](docs/next-agent-handoff.md).

The source baseline and its local checksum manifest are documented in [baseline.md](docs/baseline.md). Port coverage is tracked in [feature-parity.md](docs/feature-parity.md).

This repository is distributed under GPL-3.0-only; the baseline license text is included in [LICENSE.md](LICENSE.md).
