# Validation log

## Environment

- OS/runtime: Windows on `stable-aarch64-pc-windows-msvc`.
- `rustc 1.98.1 (48a229cea 2026-09-01)`.
- `cargo 1.98.1 (797e8a9bc 2026-08-05)`.
- Rust tools are installed under `%USERPROFILE%\.cargo\bin`, which is not on `PATH`; commands below use the full Cargo path in PowerShell.
- `dotnet` is unavailable on `PATH`; the local AssetRipper C# test/build commands cannot currently run.
- `Cargo.toml` declares Rust 1.74. That minimum toolchain has not been run here.

## Latest results

| Command | Exit code | Result |
|---|---:|---|
| `& "$env:USERPROFILE\.cargo\bin\cargo.exe" fmt -- --check` | 0 | All Rust files are formatted |
| `& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --workspace` | 0 | 35 unit tests passed; 0 failed; 0 doc tests |
| `& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy --workspace --all-targets -- -D warnings` | 0 | Passed with warnings denied |
| `& '.\tools\verify-baseline.ps1'` | 0 | Verified 1,449 baseline files; none missing, changed, or unlisted |
| `dotnet --info` | 1 | Not run: PowerShell could not find `dotnet` |

The 35 Rust tests comprise 9 binary-reader tests, 1 common-string lookup test, 3 format-generation tests, 8 header tests, 10 metadata tests, and 4 metadata-prefix tests. Metadata fixtures cover generations 8, 10–14, 17, 19, 21, and 22, including both endian modes, legacy and string-buffer TypeTrees, type/hash records, object IDs and data ranges, script IDs, references, and malformed bounds. They are hand-constructed fixtures checked against inspected source branches; they are not real Unity files or differential runs against the C# implementation.

## Fixed transient failures

- The first metadata test build lacked a signed-16-bit fixture writer; it was added.
- Five initial metadata fixture cases failed because the test bytes omitted source-defined fields or used the wrong absolute alignment. The fixtures were corrected to include legacy TypeTree child counts, the generations 7–13 `LongFileID` field, generation-16+ type flags, generation-17+ script indices, generation-14+ alignment, and the required user-information string. The complete 35-test suite then passed.
- An initial Clippy run reported two style lints in the new parser. Both were corrected; the latest warnings-denied run passed.

## Baseline and remaining verification

The reference-source checksum verifier is `tools/verify-baseline.ps1`. Its latest run found 1,449 entries with zero missing, changed, or unlisted files. The manifest SHA-256 is `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`.

These results do not pass the first-phase gate. AssetRipper's .NET build and tests, real-file differential checks, containers and compression, object construction and reference resolution, resource processing/export, product entry points, full source parity audit, and required end-to-end coverage remain unimplemented or unverified. No user bundle or UABEA content was read or run.
