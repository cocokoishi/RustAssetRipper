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
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked` | 0 | 122 unit tests passed; 0 failed; 0 doc tests |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings` | 0 | Passed with warnings denied |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' tree --locked` | 0 | Locked graph includes `brotli-decompressor` 5.0.3, `flate2` 1.1.10 with pure-Rust `miniz_oxide` 0.9.1, `lzma-rs` 0.3.0, and `ruzstd` 0.9.0; full graph is recorded in `docs/dependencies-and-licenses.md` |
| `& '.\tools\verify-baseline.ps1'` | 0 | Verified 1,449 baseline files; none missing, changed, or unlisted |
| `dotnet --info` | 1 | Not run: PowerShell could not find `dotnet` |

The 122 Rust tests comprise 10 binary-reader tests, 5 virtual-filesystem tests, 15 seekable-stream and SmartStream tests, 2 raw LZ4 tests, 6 LZMA tests, 5 Zstandard tests, 6 wrapper-codec tests, 5 UnityRaw/UnityWeb tests, 9 UnityFS parser/entry tests, 5 WebData tests, 10 scheme-dispatch tests, 1 common-string lookup test, 3 format-generation tests, 8 header tests, 10 metadata tests, 4 metadata-prefix tests, 12 object-payload tests, 2 complete-file entry-point tests, and 4 PPtr-resolution tests. Metadata fixtures cover generations 8, 10–14, 17, 19, 21, and 22, including both endian modes, legacy and string-buffer TypeTrees, type/hash records, object IDs and data ranges, script IDs, references, and malformed bounds. Payload fixtures cover both endian modes, primitive and character fields, nested structs and arrays, UTF-8 byte strings, aligned primitive arrays, explicit field alignment, unknown fixed-size leaves, 64-bit and legacy 32-bit PPtr payloads, no-TypeTree errors, managed-reference rejection, and caller limits. PPtr-resolution fixtures cover null, unique/missing/ambiguous local targets, valid one-based external IDs, and invalid IDs. UnityFS fixtures exercise uncompressed, raw LZ4, and raw LZMA metadata; LZMA storage blocks; Zstandard storage-block signature fallback; version-7 alignment; metadata-at-end placement; padding; directory range validation; and mixed storage-block entry reads. UnityRaw/UnityWeb fixtures cover header generations 1–8, version-specific hash/CRC and size fields, aligned directory metadata, direct raw entry ranges, final-scene selection, size-prefixed LZMA decode, malformed ranges, and configured limits. WebData fixtures cover directory records, borrowed data ranges, 7-bit variable-length name prefixes, output alignment, malformed bounds, and limits. Scheme tests cover source detector ordering, generation-22 SerializedFile recognition, explicit Archive failure, GZip nesting, nested UnityFS/UnityRaw/UnityWeb/WebData entry dispatch, and wrapper limits. GZip fixtures manually frame stored-DEFLATE members and checksums, concatenated members, malformed data, output bounds, and a bounded writer. Brotli fixtures use RFC 7932 bit fields to construct a valid Unity metadata marker and uncompressed payload for WBITS 10–24; these are spec-derived and were not emitted by an independent encoder. The upstream Brotli decoder vector remains separately tested. LZMA fixtures use `lzma-rs`; the size-prefixed fixture replaces the encoder's unknown-size sentinel with the known decoded length. They exercise integration but are not independent codec evidence. The Brotli vector is from the pinned decoder crate's own test suite, not an independent codec run. Zstandard unit fixtures independently construct raw/RLE block frames from the RFC format; Huffman/FSE entropy-compressed blocks are not exercised. These synthetic fixtures are not real Unity files or differential runs against the C# implementation.

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
- The first WebData invalid-header fixture used a header length of 20. The source reader treats that as an empty directory followed by trailing bytes, so the malformed-record fixture was changed to end inside a declared directory record.
- An RFC 7932 WebData Brotli fixture showed the copied byte-level detector rejected the valid 36-byte metadata marker because it compared the stored MSKIPLEN - 1 value directly to the marker length. The Rust detector now parses the bit fields and adds one; the source discrepancy and unavailable C# execution are recorded in docs/differential-findings.md.
- The first suite after correcting Brotli detection exposed an old test fixture built around the source's nonstandard length comparison. That fixture now asserts rejection of the old encoding; the RFC-built marked stream test covers valid detection and decoding. At that point, the complete 98-test suite passed.
- Warnings-denied Clippy flagged byte-boundary checks written as `% 8 != 0`. Those checks now use a power-of-two bit mask, and Clippy passes without raising the declared MSRV.

## Baseline and remaining verification

The reference-source checksum verifier is `tools/verify-baseline.ps1`. Its latest run found 1,449 entries with zero missing, changed, or unlisted files. The manifest SHA-256 is `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`.

These results do not pass the first-phase gate. AssetRipper's .NET build and tests, real-file differential checks, independently produced LZMA and Huffman/FSE Zstandard fixtures, external-file resolution, no-TypeTree recovery, managed-reference decoding, full object-graph construction, resource processing/export, product entry points, full source parity audit, and required end-to-end coverage remain unimplemented or unverified. UnityFS, Raw/Web, WebData, and scheme dispatch have synthetic fixture evidence only. UnityArchive reading/writing is explicitly unsupported by the source baseline. No user bundle or UABEA content was read or run.
## Recursive container entry integration

- cargo test --workspace --locked: exit 0; 100 unit tests passed, 0 failed, 0 doc tests.
- cargo clippy --workspace --all-targets --locked -- -D warnings: exit 0.
- UnityFS, UnityRaw, and UnityWeb synthetic entries containing GZip-wrapped resources each pass through DispatchedFile::dispatch_entry and resolve to the inner resource.
- UnityRaw entries remain borrowed; UnityWeb entries are copied from the owned decompressed scene chunk into a bounded entry buffer before nested dispatch.
- These tests are synthetic and do not establish real-file or C# differential compatibility.
## Seekable stream and SmartStream groundwork

- cargo fmt --all -- --check: exit 0.
- cargo test --workspace --locked: exit 0; 117 unit tests passed, 0 failed, 0 doc tests.
- cargo clippy --workspace --all-targets --locked -- -D warnings: exit 0.
- PartialStream, FileRangeStream, MultiFileStream, and SmartStream fixtures cover bounded reads, nested file ranges, independent positions, Arc references, temporary-file cleanup, memory modes, seek behavior, split boundaries, zero-length parts, overflow, and configured part-count bounds. Synthetic filesystem fixtures cover direct paths, numeric split ordering, input via a split-part path, and missing-part errors.
- SchemeReader::read_stream and read_path dispatch bounded complete seekable inputs; FileRangeStream provides independent nested views over a shared file handle. An integration fixture splits a GZip header/payload across two parts and verifies the decoded resource.
- The full input is currently materialized in memory up to the configured 2 GiB default. SmartStream references and ownership are implemented; memory-range construction copies input bytes and temporary-file partial views use bounded copies; Unity FileSystem integration, direct OS positional reads, and real-file/C# differentials remain outstanding.
## Virtual filesystem groundwork

- cargo fmt --all -- --check: exit 0.
- cargo test --workspace --locked: exit 0; 122 unit tests passed, 0 failed, 0 doc tests.
- cargo clippy --workspace --all-targets --locked -- -D warnings: exit 0.
- Five synthetic tests cover the source VirtualFileSystem root/path rules, recursive directory creation, SmartStream-backed create/open/truncate/delete/clear behavior, recursive file and directory enumeration, extension matching, and path/entry limits.
- Directory creation checks the full addition against the configured entry bound before mutating the tree. Existing open references remain valid when their path is deleted or the filesystem is cleared.
- Rust returns deterministic lexical enumeration order; the source uses dictionary enumeration. Only the source-supported * and *.<extension> file patterns and * directory pattern are implemented.
- The local filesystem adapter and parsed-container entry mounting are still missing. No C# differential or real Unity file exercised this VFS.
