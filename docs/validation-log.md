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
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all -- --check` | 0 | All Rust files are formatted |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked` | 0 | 168 unit tests passed; 0 failed; 0 doc tests |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings` | 0 | Passed with warnings denied |
| `& 'C:\Users\koishi\.cargo\bin\cargo.exe' tree --locked` | 0 | Locked graph includes `brotli-decompressor` 5.0.3, `flate2` 1.1.10 with pure-Rust `miniz_oxide` 0.9.1, `lzma-rs` 0.3.0, and `ruzstd` 0.9.0; full graph is recorded in `docs/dependencies-and-licenses.md` |
| `& '.\tools\verify-baseline.ps1'` | 0 | Verified 1,449 baseline files; none missing, changed, or unlisted |
| `dotnet --info` | 1 | Not run: PowerShell could not find `dotnet` |

The earlier 126-test suite comprised 10 binary-reader tests, 5 virtual-filesystem tests, 15 seekable-stream and SmartStream tests, 4 per-file reference-graph tests, 2 raw LZ4 tests, 6 LZMA tests, 5 Zstandard tests, 6 wrapper-codec tests, 5 UnityRaw/UnityWeb tests, 9 UnityFS parser/entry tests, 5 WebData tests, 10 scheme-dispatch tests, 1 common-string lookup test, 3 format-generation tests, 8 header tests, 10 metadata tests, 4 metadata-prefix tests, 12 object-payload tests, 2 complete-file entry-point tests, and 4 PPtr-resolution tests. Metadata fixtures cover generations 8, 10–14, 17, 19, 21, and 22, including both endian modes, legacy and string-buffer TypeTrees, type/hash records, object IDs and data ranges, script IDs, references, and malformed bounds. Payload fixtures cover both endian modes, primitive and character fields, nested structs and arrays, UTF-8 byte strings, aligned primitive arrays, explicit field alignment, unknown fixed-size leaves, 64-bit and legacy 32-bit PPtr payloads, no-TypeTree errors, managed-reference rejection, and caller limits. PPtr-resolution fixtures cover null, unique/missing/ambiguous local targets, valid one-based external IDs, and invalid IDs. UnityFS fixtures exercise uncompressed, raw LZ4, and raw LZMA metadata; LZMA storage blocks; Zstandard storage-block signature fallback; version-7 alignment; metadata-at-end placement; padding; directory range validation; and mixed storage-block entry reads. UnityRaw/UnityWeb fixtures cover header generations 1–8, version-specific hash/CRC and size fields, aligned directory metadata, direct raw entry ranges, final-scene selection, size-prefixed LZMA decode, malformed ranges, and configured limits. WebData fixtures cover directory records, borrowed data ranges, 7-bit variable-length name prefixes, output alignment, malformed bounds, and limits. Scheme tests cover source detector ordering, generation-22 SerializedFile recognition, explicit Archive failure, GZip nesting, nested UnityFS/UnityRaw/UnityWeb/WebData entry dispatch, and wrapper limits. GZip fixtures manually frame stored-DEFLATE members and checksums, concatenated members, malformed data, output bounds, and a bounded writer. Brotli fixtures use RFC 7932 bit fields to construct a valid Unity metadata marker and uncompressed payload for WBITS 10–24; these are spec-derived and were not emitted by an independent encoder. The upstream Brotli decoder vector remains separately tested. LZMA fixtures use `lzma-rs`; the size-prefixed fixture replaces the encoder's unknown-size sentinel with the known decoded length. They exercise integration but are not independent codec evidence. The Brotli vector is from the pinned decoder crate's own test suite, not an independent codec run. Zstandard unit fixtures independently construct raw/RLE block frames from the RFC format; Huffman/FSE entropy-compressed blocks are not exercised. These synthetic fixtures are not real Unity files or differential runs against the C# implementation.

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
## Per-SerializedFile reference graph groundwork

- cargo fmt --all -- --check: exit 0.
- cargo test --workspace --locked: exit 0; 126 unit tests passed, 0 failed, 0 doc tests.
- cargo clippy --workspace --all-targets --locked -- -D warnings: exit 0.
- A generated generation-10 SerializedFile contains a decoded PPtr field; the graph records source object index, source PathID, field path, FileID, and target PathID, then resolves the local target through the sorted PathID index.
- Synthetic graph tests exercise nested struct/array paths, duplicate PathIDs, null/local/external/missing/ambiguous/invalid targets, raw leaves, and explicit edge/object/issue/path limits.
- Object payload decode errors, raw TypeTree leaves, trailing payload bytes, and scan bounds are explicit; they prevent the graph from reporting complete. External files are only classified, not loaded.
- This is per-file reference evidence only. Cross-file bundle matching, full object construction, and export identity mapping remain unimplemented. No real-file or C# differential has run.

## Direct parsed-container entry mounting

- cargo fmt --all: exit 0; cargo fmt --all -- --check: exit 0.
- cargo test --workspace --locked: exit 0; 134 unit tests passed, 0 failed, 0 doc tests.
- cargo clippy --workspace --all-targets --locked -- -D warnings: exit 0.
- tools/verify-baseline.ps1: exit 0; all 1,449 reference files match the manifest.
- git diff --check: exit 0.
- VirtualFileSystem can insert an already-owned readable SmartStream while creating missing parents, enforcing the total entry limit before mutation and rejecting duplicate paths without replacing existing content.
- DispatchedFile::mount_container_entries mounts direct UnityFS, UnityRaw/UnityWeb, and WebData entry bytes beneath an optional virtual destination. It preflights entry count and aggregate declared bytes, rejects parent-directory path components, and reports exact partial progress if a later entry fails.
- WebData, UnityFS, UnityRaw, and UnityWeb synthetic tests cover direct entry path mounting and exact bytes. WebData tests additionally cover count and byte limits before mutation, duplicate-path collisions, partial reports, and path traversal rejection. These fixtures do not establish real-file or C# differential compatibility.
- The initial test run had one fixture setup failure because the legacy write helper requires a pre-existing parent directory. The fixture now creates that directory first; the complete suite then passed.
## Recursive parsed-container tree mounting

- cargo fmt --all: exit 0.
- cargo test --workspace --locked: exit 0; 137 unit tests passed, 0 failed, 0 doc tests.
- cargo clippy --workspace --all-targets --locked -- -D warnings: exit 0.
- The recursive API traverses supported nested UnityFS, UnityRaw/UnityWeb, and WebData containers and GZip/Brotli wrapper dispatch under parent-entry virtual paths. It mounts terminal Resource and SerializedFile bytes as leaves; nested container payloads are represented by their child path.
- ContainerMountLimits now bounds visited entry count across the tree, mounted leaf byte totals, nesting depth (default 64), and active owned nested-container buffers (default 1 GiB). Errors preserve the count of files and bytes mounted before failure.
- Synthetic fixtures cover nested WebData, a GZip-wrapped nested WebData container, nested UnityFS, global entry-count/depth/active-buffer/output limits, and partial progress.
- The recursive API is not yet connected to the product root-input or batch pipeline. No real-file or C# differential has run.
## Single-file path to VFS import

- cargo fmt --all: exit 0.
- cargo test --workspace --locked: exit 0; 140 unit tests passed, 0 failed, 0 doc tests.
- cargo clippy --workspace --all-targets --locked -- -D warnings: exit 0.
- SchemeReader::mount_path reads one regular or .splitN logical file. Root containers use recursive tree mounting; standalone Resource and SerializedFile bytes use their logical filename.
- Temporary-file tests exercise a regular Resource, invocation by a .split1 part path with base-name mapping, and a root WebData file flowing into recursive mounting.
- This covers one path per call only. Directory/batch discovery and automatic external dependency loading remain unimplemented; no real-file or C# differential has run.

## Directory and batch input to VFS

- `SchemeReader::mount_directory` recursively discovers regular files in deterministic lexical order, preserves standalone file paths and namespaces each root container beneath its logical input path; numbered `.splitN` families count as one logical input.
- The directory scanner rejects symbolic links and special filesystem entries, bounds directory entry count and depth, checks UTF-8 virtual paths, and rejects a regular file that collides with a split group.
- Batch limits preflight aggregate physical input bytes before modifying the VFS and apply one output-byte budget across all imported roots. A mount failure returns completed-input and partial mounted-entry/byte counts.
- Five synthetic temporary-directory tests cover mixed standalone/nested/container/split input, aggregate input preflight before mutation, cumulative output limits with partial progress, two containers with the same internal path, directory/input/depth bounds, and regular/split name ambiguity. All directory fixtures are created by the tests; no user AssetBundle paths are scanned.
- `cargo fmt --all`: exit 0. `cargo fmt --all -- --check`: exit 0.
- `cargo test --workspace --locked`: exit 0; 145 unit tests passed, 0 failed, 0 doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. `git diff --check`: exit 0.
- The implementation has not yet been differentially checked against real AssetRipper input directories or C# `MultiFileStream` execution. It does not load external resource dependencies or provide the broader local `FileSystem` adapter.

## Explicit path-list batch import

- `SchemeReader::mount_paths` imports caller-ordered `PathMountInput` values, using each supplied destination root for that input's container entries or standalone-file parent.
- Preflight opens each local regular or split logical stream to determine its complete byte length, rejects directory/symbolic-link inputs and duplicate logical path/destination pairs, and checks the aggregate input budget before mounting.
- Two synthetic temporary-file tests cover Resource and WebData imports at separate destinations, total input size rejection before VFS mutation, and repeated `.splitN` members rejected before mutation.
- `cargo test --workspace --locked`: exit 0; 147 unit tests passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0.
- No real-file or C# differential has run; callers must represent each split family with one path.

## Serialized external-name normalization

- Added source-compatible `fix_file_identifier`, `fix_dependency_name`, and `fix_resource_path` helpers from `SpecialFileNames`; `FileIdentifier::fixed_path_name` exposes the normalized lookup path without replacing the original serialized name.
- Synthetic tests cover `library/` and `resources/` prefixes, `archive:/` basename extraction, lowercase normalization, unchanged non-prefixed paths, helper case behavior, and preservation of the source name.
- `cargo test --workspace --locked`: exit 0; 150 unit tests passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. No actual dependency file lookup/loading, GUID-derived Meta lookup, or C# differential is implemented by this increment.

## Cross-file PPtr graph

- Added `MultiFileReferenceGraph` for an explicitly supplied set of named SerializedFiles with per-file reference scans. It applies source filename normalization, resolves unique external file names and target PathIDs, and retains explicit results for missing names, duplicate names, absent/duplicate target PathIDs, nulls, and invalid FileIDs.
- Added limits for loaded file count, reference count, individual normalized file names, and aggregate normalized file-name bytes. The graph does not discover or load absent dependencies from the VFS or host filesystem.
- Synthetic tests cover successful cross-file object resolution, missing and ambiguous file names, a missing target PathID, normalized aliases, duplicate candidate enumeration, and file/reference/name-byte bounds.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all -- --check`: exit 0.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked`: exit 0; 153 tests passed, 0 failed, 0 doc tests.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. `git diff --check`: exit 0.
- Real-file/C# differential validation, VFS/host dependency discovery and loading, and `AssetType.Meta` GUID identity lookup have not run or been implemented.

## Mounted VFS dependency lookup and scheme dispatch

- Added `DependencyIndex` for caller-supplied registered file identities/paths and ordered DataPaths. Lookup follows the source stages: registered name, exact DataPath path, then the default-resource or builtin-extra aliases under DataPath, `Resources`, and legacy `unity`.
- Unique results can be opened as VFS SmartStreams and dispatched through the bounded `SchemeReader::read_stream` path. Missing and multiple viable candidates remain separate results. Unsafe absolute/traversing names and candidate/index limits return errors.
- Synthetic VFS tests cover registered-name precedence, nested DataPath paths, missing and ambiguous results, engine alias/directory ordering, scheme dispatch, path traversal rejection, and limits. A first test attempt read an empty stream because the in-memory VFS shares its file cursor; the corrected test uses the same scheme dispatch entry, which seeks to byte zero before reading.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked --quiet`: exit 0; 159 unit tests passed, 0 failed, 0 doc tests.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. `git diff --check`: exit 0.
- The import orchestrator still needs to build the registry/DataPaths and recursively parse dispatched dependencies into the bundle graph. No host directory or user sample was scanned; no real-file/C# differential has run.

## VFS SerializedFile workspace load

- Added bounded `SerializedFileWorkspace::load_from_vfs`: preflights caller-supplied named VFS paths and aggregate input/name limits, dispatches each input through `SchemeReader`, requires a SerializedFile, parses its header and metadata, builds its per-file PPtr graph, and enforces aggregate object/reference limits.
- The workspace retains source name/path, header, metadata, and reference graph. Input bytes remain owned by VFS and are not copied into the workspace result. `reference_graph()` can join the loaded set by normalized names; absent dependencies remain visible in its result.
- Three synthetic tests cover successful generation-10 VFS load and self-reference resolution, total-input preflight before processing, a Resource rejected as non-SerializedFile, and aggregate object limits. A warnings-denied Clippy run initially flagged the contextual error type's size; boxing the error resolved the lint without suppressing it.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked --quiet`: exit 0; 162 unit tests passed, 0 failed, 0 doc tests.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. `git diff --check`: exit 0.
- Recursive dependency loading, automatic discovery of all serialized-file leaves, and real-file/C# differential validation have not run or been implemented. No user sample was read.

## Recursive mounted SerializedFile dependency walk

- Added a bounded queue over each loaded file's external table. The registry includes mounted VFS leaf paths and caller-supplied source identities; DataPaths remain caller-supplied and ordered.
- Unique SerializedFile candidates are parsed and appended to the same workspace, so dependencies of newly loaded files are visited. Existing identities prevent cycles. Missing, ambiguous, Resource, and container-needs-expansion outcomes remain explicit.
- Four synthetic tests cover recursive loading across three files and two dependency hops, cycle detection, file-scoped external PPtr resolution, missing/ambiguous/Resource/container results, dependency-request limits, and aggregate-object limits. No user AssetBundle or real Unity file was read.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all -- --check`: exit 0.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked`: exit 0; 166 tests passed, 0 failed, 0 doc tests.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. `git diff --check`: exit 0.
- Container candidates are reported but not expanded yet. Platform-specific DataPath construction, GUID-derived `AssetType.Meta` matching, and real-file/C# differential validation remain outstanding.

## Dependency container expansion

- The recursive dependency walk now dispatches and mounts supported UnityFS, UnityRaw, UnityWeb, and WebData candidates into unique internal VFS roots. It rebuilds its bounded dependency index, registers each leaf by its path relative to that root, and queues uniquely loaded SerializedFiles.
- Aggregate dispatched-container bytes, mounted entries, and mounted output bytes are bounded across the walk. Per-container recursion/entry/output limits and the VFS entry cap still apply. UnityArchive remains unsupported by the source baseline parser.
- A synthetic WebData fixture contains a generation-10 SerializedFile named to match the external identifier. The test verifies the expanded file is loaded, the dependency report records a unique identity match, and the cross-file PPtr resolves. An empty WebData dependency is separately reported as expanded with no SerializedFiles. Additional fixtures enforce aggregate input, mounted-entry, and output-byte limits. No user AssetBundle was read.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all -- --check`: exit 0.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked`: exit 0; 168 tests passed, 0 failed, 0 doc tests.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. `git diff --check`: exit 0.
- Platform-specific DataPath discovery, GUID-derived `AssetType.Meta` identity, and real-file/C# differential validation remain outstanding.


## GUID-derived Meta dependency identity

- Implemented the local source rule that FileIdentifier.GetFilePath returns the Unity GUID for AssetType.Meta, while other types use normalized PathName. GUID formatting follows the four serialized words in order, each lowercase and padded to eight hexadecimal digits.
- Focused synthetic cases passed: `GetFilePath` emits the four-word GUID for Meta and normalized PathName for ordinary file types; the platform dependency index still resolves a Meta external request by PathName; malformed missing-GUID bundle identity is explicit. No C# runtime, real AssetBundle, user sample, or UABEA was used.
- Full verification passed: `cargo test --workspace --locked` (171 passed, 0 failed); warnings-denied workspace Clippy; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 unchanged reference files); and `git diff --check`. No real-file or C# differential has run.


## Mixed mounted input structure

- Added a VFS-based mixed-input scanner that takes explicit selected paths, recognizes SerializedFile and bundle headers, recognizes the source WebGL data suffixes, and records .dll assembly paths separately. It derives deterministic DataPaths from direct-file parents and directories that yielded registered files.
- A raw header probe classifies bundle signatures and SerializedFile headers from at most 32 bytes plus the stream length; it does not read resource bodies or decode outer wrappers. Inventory bounds are configurable and default to 1,000,000 scanned files, 2 GiB per file, and 16 GiB aggregate declared bytes.
- Four synthetic tests cover directory and direct-file behavior, source-style names, DataPaths, and configured selection/file/byte/path errors. cargo test --workspace --locked passed 176 tests; warnings-denied Clippy, cargo fmt --all -- --check, baseline verification (1,449 reference files unchanged), and git diff --check passed. No real-file/C# differential or user sample was used.
- Platform root detection and automatic platform DataPath order remain incomplete.
