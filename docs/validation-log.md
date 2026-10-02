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
| `& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --workspace` | 0 | 48 unit tests passed; 0 failed; 0 doc tests |
| `& "$env:USERPROFILE\.cargo\bin\cargo.exe" clippy --workspace --all-targets -- -D warnings` | 0 | Passed with warnings denied |
| `& '.\tools\verify-baseline.ps1'` | 0 | Verified 1,449 baseline files; none missing, changed, or unlisted |
| `dotnet --info` | 1 | Not run: PowerShell could not find `dotnet` |

The 48 Rust tests comprise 10 binary-reader tests, 1 common-string lookup test, 3 format-generation tests, 8 header tests, 10 metadata tests, 4 metadata-prefix tests, 10 object-payload tests, and 2 complete-file entry-point tests. Metadata fixtures cover generations 8, 10–14, 17, 19, 21, and 22, including both endian modes, legacy and string-buffer TypeTrees, type/hash records, object IDs and data ranges, script IDs, references, and malformed bounds. Payload fixtures cover both endian modes, primitive and character fields, nested structs and arrays, UTF-8 byte strings, aligned primitive arrays, explicit field alignment, unknown fixed-size leaves, no-TypeTree errors, managed-reference rejection, and caller limits. The entry-point fixture encodes a real generation-10 header followed by metadata and an object payload. All are hand-constructed fixtures checked against inspected source branches; they are not real Unity files or differential runs against the C# implementation.

## Fixed transient failures

- The first metadata test build lacked a signed-16-bit fixture writer; it was added.
- Five initial metadata fixture cases failed because the test bytes omitted source-defined fields or used the wrong absolute alignment. The fixtures were corrected to include legacy TypeTree child counts, the generations 7–13 `LongFileID` field, generation-16+ type flags, generation-17+ script indices, generation-14+ alignment, and the required user-information string. The complete 35-test suite then passed.
- The first object-decoder build exposed mismatched recursive return types and an array fixture inferred as `u8` while expressing negative `i32` bytes. The parser now reserves named fields separately from array values, and the malformed-count fixture uses explicit raw bytes; subsequent full test runs passed.
- An initial Clippy run reported two style lints in the new parser. Both were corrected; the latest warnings-denied run passed.
- The first object-decoder Clippy run reported that three recursive parsing functions carry explicit context arguments. Those arguments are kept explicit so the input tree, field path, version alignment rule, object identity, and allocation budget remain visible at each parse step; the three functions now have local lint allowances with that reason documented. The latest warnings-denied run passed.

## Baseline and remaining verification

The reference-source checksum verifier is `tools/verify-baseline.ps1`. Its latest run found 1,449 entries with zero missing, changed, or unlisted files. The manifest SHA-256 is `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`.

These results do not pass the first-phase gate. AssetRipper's .NET build and tests, real-file differential checks, containers and compression, no-TypeTree recovery, managed-reference decoding, object graph construction and reference resolution, resource processing/export, product entry points, full source parity audit, and required end-to-end coverage remain unimplemented or unverified. No user bundle or UABEA content was read or run.
