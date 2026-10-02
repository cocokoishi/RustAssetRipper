# Validation log

## Environment

- OS/runtime: Windows on `stable-aarch64-pc-windows-msvc`.
- `rustc 1.98.1 (48a229cea 2026-09-01)`.
- `cargo 1.98.1 (797e8a9bc 2026-08-05)`.
- Rust tools are installed under `%USERPROFILE%\.cargo\bin`, which is not on `PATH`; commands below use the full Cargo path in PowerShell.
- `dotnet` is unavailable on `PATH`; the local AssetRipper C# test/build commands cannot currently run.
- `Cargo.toml` declares Rust 1.87 because `ruzstd` 0.9.0 requires it. That minimum toolchain has not been run here.

## Latest results

| Command | Exit code | Result |
|---|---:|---|
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt -- --check` | 0 | All Rust files are formatted |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked` | 0 | 85 unit tests passed; 0 failed; 0 doc tests |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings` | 0 | Passed with warnings denied |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' tree --locked` | 0 | Locked graph includes `brotli-decompressor` 5.0.3, `flate2` 1.1.10 with pure-Rust `miniz_oxide` 0.9.1, `lzma-rs` 0.3.0, and `ruzstd` 0.9.0; full graph is recorded in `docs/dependencies-and-licenses.md` |
| `& '.\tools\verify-baseline.ps1'` | 0 | Verified 1,449 baseline files; none missing, changed, or unlisted |
| `dotnet --info` | 1 | Not run: PowerShell could not find `dotnet` |

The 85 Rust tests comprise 10 binary-reader tests, 2 raw LZ4 tests, 6 LZMA tests, 5 Zstandard tests, 6 wrapper-codec tests, 4 UnityRaw/UnityWeb tests, 8 UnityFS parser/entry tests, 1 common-string lookup test, 3 format-generation tests, 8 header tests, 10 metadata tests, 4 metadata-prefix tests, 12 object-payload tests, 2 complete-file entry-point tests, and 4 PPtr-resolution tests. Metadata fixtures cover generations 8, 10–14, 17, 19, 21, and 22, including both endian modes, legacy and string-buffer TypeTrees, type/hash records, object IDs and data ranges, script IDs, references, and malformed bounds. Payload fixtures cover both endian modes, primitive and character fields, nested structs and arrays, UTF-8 byte strings, aligned primitive arrays, explicit field alignment, unknown fixed-size leaves, 64-bit and legacy 32-bit PPtr payloads, no-TypeTree errors, managed-reference rejection, and caller limits. PPtr-resolution fixtures cover null, unique/missing/ambiguous local targets, valid one-based external IDs, and invalid IDs. UnityFS fixtures exercise uncompressed, raw LZ4, and raw LZMA metadata; LZMA storage blocks; Zstandard storage-block signature fallback; version-7 alignment; metadata-at-end placement; padding; directory range validation; and mixed storage-block entry reads. UnityRaw/UnityWeb fixtures cover header generations 1–8, version-specific hash/CRC and size fields, aligned directory metadata, direct raw entry ranges, final-scene selection, size-prefixed LZMA decode, malformed ranges, and configured limits. Wrapper fixtures cover manually framed stored-DEFLATE GZip members and checksums, concatenated members, malformed data, output bounds, a bounded GZip writer, Unity's Brotli marker, and a Brotli decoder vector. LZMA fixtures use `lzma-rs`; the size-prefixed fixture replaces the encoder's unknown-size sentinel with the known decoded length. They exercise integration but are not independent codec evidence. The Brotli vector is from the pinned decoder crate's own test suite, not an independent codec run. Zstandard unit fixtures independently construct raw/RLE block frames from the RFC format; Huffman/FSE entropy-compressed blocks are not exercised. These synthetic fixtures are not real Unity files or differential runs against the C# implementation.

## Fixed transient failures

- The first metadata test build lacked a signed-16-bit fixture writer; it was added.
- Five initial metadata fixture cases failed because the test bytes omitted source-defined fields or used the wrong absolute alignment. The fixtures were corrected to include legacy TypeTree child counts, the generations 7–13 `LongFileID` field, generation-16+ type flags, generation-17+ script indices, generation-14+ alignment, and the required user-information string. The complete 35-test suite then passed.
- The first object-decoder build exposed mismatched recursive return types and an array fixture inferred as `u8` while expressing negative `i32` bytes. The parser now reserves named fields separately from array values, and the malformed-count fixture uses explicit raw bytes; subsequent full test runs passed.
- An initial Clippy run reported two style lints in the new parser. Both were corrected; the latest warnings-denied run passed.
- The first object-decoder Clippy run reported that three recursive parsing functions carry explicit context arguments. Those arguments are kept explicit so the input tree, field path, version alignment rule, object identity, and allocation budget remain visible at each parse step; the three functions now have local lint allowances with that reason documented. The latest warnings-denied run passed.
- After raising the declared minimum Rust version to 1.87 for `ruzstd`, Clippy 1.98 flagged two modulo alignment checks and one `Option::map_or` expression. They were changed to APIs stable by Rust 1.87; the warnings-denied run passed.
- A test-only attempt to create an entropy-compressed fixture with `ruzstd`'s encoder hit its explicit `not implemented` path. That was an encoder limitation, not a decoder failure; the attempted fixture was replaced with independent RFC raw/RLE frame fixtures. No production decoder code was bypassed or marked successful by that failure.
- The first Raw/Web test build caught a borrow conflict in the synthetic metadata writer. After fixing that fixture, tests exposed two fixture errors: `lzma-rs` emits an all-ones unknown-size field by default, and the first Raw/Web metadata-size fixture included entry payload bytes. The fixtures now encode their known decoded lengths and count only the aligned directory records; the Raw/Web change passed its then-current 79-test run. Clippy also flagged test-only limit setup using post-`Default` field assignments; the tests now use struct update syntax.
- The first wrapper build emitted an unused-import warning in the new wrapper module; that import was removed before the warnings-denied Clippy run.

## Baseline and remaining verification

The reference-source checksum verifier is `tools/verify-baseline.ps1`. Its latest run found 1,449 entries with zero missing, changed, or unlisted files. The manifest SHA-256 is `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`.

These results do not pass the first-phase gate. AssetRipper's .NET build and tests, real-file differential checks, Archive behavior, generic scheme dispatch, independent GZip/Brotli wrapper evidence, independently produced LZMA and Huffman/FSE Zstandard fixtures, recursive wrapper processing, no-TypeTree recovery, managed-reference decoding, full object-graph construction and multi-file resolution, resource processing/export, product entry points, full source parity audit, and required end-to-end coverage remain unimplemented or unverified. UnityFS and Raw/Web parsing and wrapper codec paths have only synthetic fixture evidence. No user bundle or UABEA content was read or run.
