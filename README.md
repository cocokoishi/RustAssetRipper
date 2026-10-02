# RustAssetRipper

This repository is the independent Rust port of the local AssetRipper 2.0.0 source snapshot. The port is in its first phase; it is not yet a usable AssetRipper product and must not be treated as having full format or export coverage.

The current implementation units are a bounded binary reader, a read-only complete SerializedFile entry point, versioned header and metadata parsers, TypeTree records with the audited Unity common-string table, checked object-data ranges, and a bounded TypeTree-driven object payload decoder. UnityFS has partial header, metadata, entry, LZ4, LZMA, and Zstandard support. Legacy UnityRaw and UnityWeb have partial versioned header, directory, and entry parsing; UnityWeb's size-prefixed LZMA chunk is bounded and checked. `lzma-rs` and `ruzstd` provide codec implementations; the Rust product does not invoke AssetRipper, UABEA, .NET, or another Unity parser. This parser work is still only a small part of the full product migration.

## Development

The manifest declares Rust 1.87 as its minimum because the selected pure-Rust Zstandard decoder requires that toolchain. The current code has been checked with Rust/Cargo 1.98.1 on `stable-aarch64-pc-windows-msvc`; Rust 1.87 itself has not been run. If Cargo is on `PATH`, run:

```powershell
cargo fmt -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

On this workstation Cargo is installed under `%USERPROFILE%\.cargo\bin` but that directory is not on `PATH`; in PowerShell, prefix each command with `& "$env:USERPROFILE\.cargo\bin\cargo.exe"`. The latest Rust run and outstanding validation status are recorded in the [validation log](docs/validation-log.md). The local AssetRipper source tests remain unrun because `dotnet` is unavailable. See also the [continuation status](docs/continuation.md) and [dependency/license record](docs/dependencies-and-licenses.md).

The source baseline and its local checksum manifest are documented in [baseline.md](docs/baseline.md). Port coverage is tracked in [feature-parity.md](docs/feature-parity.md).

This repository is distributed under GPL-3.0-only; the baseline license text is included in [LICENSE.md](LICENSE.md).
