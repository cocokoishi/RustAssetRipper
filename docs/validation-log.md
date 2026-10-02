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


## Desktop platform root detection

- Added VFS detection for Windows, Linux, and macOS roots, including Windows/Linux executable and _Data selections, named/fallback Data directories, and macOS Contents/Resources/Data and ordered DataPaths.
- Three synthetic tests passed for Windows and Linux root resolution, macOS path ordering, detection priority, and invalid/missing selections. cargo test --workspace --locked passed 179 tests; warnings-denied Clippy, format check, 1,449-file baseline verification, and diff check passed.
- Mobile, console, WebGL/WebPlayer/Windows Phone root detectors, platform file collection, and real-file/C# differential validation remain outstanding.


## Mobile, console, and Web platform roots

- Extended full-priority platform detection to Android APK/OBB, iOS, Switch, PS4, WebGL, WebPlayer, Wii U, and Windows Phone. It returns consumed paths, root/name, optional game data path, and ordered DataPaths; duplicate Android APK/OBB candidates are explicit errors.
- Seven synthetic platform tests cover Android with OBB and duplicate rejection, iOS, PS4, Switch, Wii U, WebGL precedence, WebPlayer pairing, and Windows-before-Windows-Phone source priority. cargo test --workspace --locked passed 183 tests; warnings-denied Clippy, formatting, baseline verification (1,449 reference files), and diff check passed.
- Real-file/C# differential validation remains outstanding.


## Platform game-file registration

- Added bounded platform file registration for source primary file names, split0 logical names, `data.unity3d`/`datapack.unity3d`, WebGL data files, WebPlayer matched bundles, and header-confirmed streaming bundles. Android bundle scanning follows the APK assets layout and omits the direct `bin` subtree.
- Seven synthetic platform inventory tests cover source naming, DataPath registration, streaming traversal and skip mode, Android exclusion, Web platform special files, missing DataPath reporting, and scan/depth limits.
- Android's APK asset-bundle scan runs even when `skip_streaming_assets` is true, matching the source override. The optional platform StreamingAssets traversal is skipped by that setting.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all`: exit 0. `cargo test --workspace --locked`: exit 0; 190 tests passed, 0 failed, 0 doc tests.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files remain unchanged. `git diff --check`: exit 0.
- Product input-flow integration and real-file/C# differential remain incomplete.


## Platform scripting backend and Mono assemblies

- Added platform-specific IL2CPP marker checks for Windows, Linux, macOS, Android, iOS, Switch, PS4, and Windows Phone. IL2CPP wins only when both its expected game assembly and metadata exist; otherwise direct ManagedPath DLL presence selects Mono. Wii U remains Mono-only; WebGL/WebPlayer remain Unknown.
- Mono assembly inventory follows the source ManagedPath per platform, collects only direct lowercase `.dll` filenames, bounds candidates, and reports duplicate basenames while retaining the first deterministic path. Android `libil2cpp.so` search uses bounded recursive directory traversal.
- Four focused tests cover direct-only Mono DLL collection plus limits, requiring both IL2CPP markers, marker precedence for all eight layouts, and Wii U's Mono-only behavior.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all`: exit 0. `cargo test --workspace --locked --quiet`: exit 0; 196 tests passed, 0 failed, 0 doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0. `tools/verify-baseline.ps1`: exit 0; 1,449 reference files unchanged. `git diff --check`: exit 0.
- No .NET or real platform-build differential has run. Product import flow, top-level unreferenced bundles, and WebData initial import remain outstanding.


## Platform inputs to the SerializedFile workspace

- Platform inventories now expose source-named primary file inputs separately from all registered files. `PlatformStructure::load_primary_serialized_workspace` passes those inputs, the full file registry, and source-ordered DataPaths to the existing recursive dependency loader.
- Synthetic VFS coverage verifies a `globalgamemanagers` file loads and resolves `shared.assets` from its DataPath, as well as an explicit error when a platform has no primary SerializedFile inputs.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all`: exit 0. `cargo test --workspace --locked --quiet`: exit 0; 192 tests passed, 0 failed, 0 doc tests.
- Warnings-denied workspace Clippy, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 unchanged reference files), and `git diff --check` all passed for this integration.
- Unreferenced top-level bundles and WebData are not initial workspace inputs. General container import, product input-flow integration, and real-file/C# differential remain incomplete.


## Registered platform container initial expansion

- `PlatformStructure::load_primary_serialized_workspace` now dispatches every source-registered non-primary file before creating the dependency workspace. Direct SerializedFiles become initial inputs; Resource candidates are retained; registered UnityFS, UnityRaw, UnityWeb, and WebData containers are recursively mounted under isolated VFS roots, with SerializedFile and Resource leaves registered by their relative container paths.
- The dependency workspace is seeded with those expanded container identities. A later external reference to the source container is reported as `ContainerAlreadyExpanded`, while references to a leaf already in the workspace report `AlreadyLoaded`. Aggregate container inputs, visited entries, mounted bytes, initial container count, SerializedFile count, and dependency registry size are bounded.
- Synthetic WebData coverage verifies one initial leaf load and both dependency outcomes. A separate fixture verifies the initial SerializedFile count limit. Full workspace suite: 198 passed, 0 failed, 0 doc tests.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0. `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files unchanged. `git diff --check`: exit 0.
- Arbitrary unregistered paths, complete product input/UI flow, real-file/.NET differential validation, and the Phase 1 gate remain incomplete. User AssetBundles and UABEA were not inspected.


## Partial directory-inspection command

- `assetripper-rs inspect <input-directory> [input-directory ...]` now shares directory/input/byte budgets, detects platform layouts against the original file paths, and calls source platform registration plus initial container expansion. If no platform matches, it recursively remounts the selected directories and scans/loads them through the Mixed registry.
- Four binary tests cover aggregate budget/report arithmetic, a synthetic mixed-directory-to-workspace path, and a synthetic Windows platform directory with `globalgamemanagers` referring to `shared.assets` inside initial WebData. The platform test asserts both the leaf's `AlreadyLoaded` and container's `ContainerAlreadyExpanded` outcomes.
- PowerShell `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all -- --check`: exit 0. `cargo test --workspace --locked --quiet`: exit 0; 200 library tests and 4 binary tests passed, 0 failed, 0 doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo run --locked --quiet -- --help` and `--version`: exit 0. Inspecting a missing input directory exits 1 as expected.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files unchanged. `git diff --check`: exit 0. No external or user AssetBundle was inspected.
- This is an inspection entry point only. Full AssetRipper CLI options, configuration, exporters, UI, source C# differential, and the Phase 1 gate remain incomplete.


## File-backed import preserving directory container identities

- Added `SmartStream::from_reader` and exposed VFS insertion for owned readable streams. `SchemeReader::mount_directory_preserving_containers` imports bounded regular-file discoveries by relative path, assembles logical `.splitN` inputs, and stores their raw bytes behind read-only seekable streams without expanding containers.
- The synthetic split WebData fixture confirms the original logical filename remains a VFS file, the full raw bytes match, and later scheme dispatch recognizes UnityWebData. The platform CLI path then expands source-registered containers through `PlatformStructure::load_primary_serialized_workspace`; mixed input still uses recursive directory mounting.
- This directory-import increment passed 200 library tests and 4 binary tests; warnings-denied Clippy and formatting passed. `tools/verify-baseline.ps1` confirmed all 1,449 reference files unchanged and `git diff --check` passed. Real-file/.NET differential validation remains outstanding.


## File-scoped object graph navigation

- `ObjectReferenceGraph` now exposes all table indexes matching a PathID without merging duplicates and exposes outgoing edges per source object. `MultiFileReferenceGraph` now returns normalized file-scoped identities, outgoing edges for one source object, and incoming edges only for uniquely resolved targets.
- Synthetic tests exercise duplicate PathIDs, missing object indexes, normalized file identity, and incoming/outgoing references across loaded SerializedFiles.
- `cargo fmt --all`: exit 0. `cargo test --workspace --locked --quiet`: exit 0; 201 library tests and 4 binary tests passed, 0 failed, 0 doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1`, and `git diff --check`: exit 0; all 1,449 reference files remain unchanged.
- No real Unity file or source-runtime differential was used for this API; full runtime-class construction and processing remain incomplete.


## Typed serialized-value access

- `SerializedValue` exposes ordered struct fields, ordered array elements, and the original PPtr fileID/PathID pair. Unique-field access distinguishes a missing name, a non-struct value, and duplicate field names without selecting an arbitrary duplicate.
- Synthetic tests cover unique and missing fields, duplicate-name errors, array order, PPtr identity, and non-struct diagnostics.
- `cargo fmt --all`: exit 0. `cargo test --workspace --locked --quiet`: exit 0; 203 library tests and 4 binary tests passed, 0 failed, 0 doc tests.
- Warnings-denied workspace Clippy, formatting check, baseline verification (1,449 source files unchanged), and `git diff --check` passed. No real Unity file or source-runtime differential was run; generated object classes and complete processors remain unimplemented.


## Initial scene relationship model

- Added a bounded file-scoped scene graph for object identity records and selected GameObject/Transform/RectTransform fields. GameObjects retain raw `m_Name` bytes and active state; `m_Component`, `m_GameObject`, `m_Father`, and ordered `m_Children` pointers become explicit typed relationships.
- Local PathIDs resolve through a sorted index. Null, missing, duplicate, external-table, and invalid-FileID results remain separate. Required-field omissions, duplicate fields, wrong value shapes, and object payload decode errors remain explicit issues. The builder does not load external dependencies or reconcile father/child inconsistencies.
- Synthetic root tests cover relationship order and classifications, nested `Base` fields, malformed data, duplicate/incomplete root sets, duplicate object PathIDs, limits, and modern object class IDs derived from resolved serialized types. A complete generated generation-10 SerializedFile with a GameObject TypeTree exercises `SceneGraph::build` end to end.
- The first full-file fixture attempt failed because the fixture incorrectly emitted an enable-TypeTree flag for generation 10; the decoder correctly treated those bytes as part of the type count. The fixture now follows generation 10's implicit TypeTree-present rule, and the focused integration case passes.
- `cargo test --workspace --locked --quiet`: exit 0; 210 library tests and 4 binary tests passed, 0 failed, 0 doc tests. Warnings-denied workspace Clippy, formatting check, baseline verification (1,449 reference files unchanged), and `git diff --check` passed.
- This does not validate an actual Unity-produced file or AssetRipper runtime behavior. Transform full-file coverage, scene roots, Prefabs, transform values, and export remain incomplete; dependency discovery/loading is performed by the workspace before the scene graph is joined.


## Multi-file scene PPtr resolution

- `SceneGraph::resolve_with_multi_file_graph` now validates the caller file name and every object-table PathID against the joined graph before resolving relation fields. It matches only the exact field path or a prefix made entirely of inherited `Base` nodes, then applies targets atomically after all scene edges match.
- Local/unique and cross-file/unique targets remain file-scoped. Missing local objects, duplicate local PathIDs, missing/ambiguous dependencies, missing/ambiguous target objects, invalid FileIDs, and null PPtrs retain distinct states. A field-path mismatch returns an error and leaves the original scene targets unchanged.
- Generated generation-10 SerializedFiles verify a local GameObject component pointer, a cross-file component pointer from `main.assets` to `shared.assets`, unresolved dependency status, filename mismatch, and transactional field-path failure. No user samples or real Unity files were used.
- `cargo test --workspace --locked --quiet`: exit 0; 213 library tests and 4 binary tests passed, 0 failed, 0 doc tests. Warnings-denied Clippy, formatting check, baseline verification (1,449 reference files unchanged), and `git diff --check` passed.
- This resolves scene edges only against a caller-built workspace graph. Parent/child consistency policy, full Transform/RectTransform layouts, scene roots, Prefabs, Unity runtime comparison, and export remain open.


## Transform and RectTransform values

- Scene objects now retain typed local position, quaternion rotation, scale, optional root order, and optional local Euler hint. RectTransform objects also retain anchor min/max, anchored position, size delta, and pivot. Values stay in serialized source coordinates; the processor does not manufacture defaults or convert rotations.
- The common Transform fields and five RectTransform layout fields produce explicit missing-field/type issues when absent from those object TypeTrees. Root order and Euler hint remain optional and are read only when the active TypeTree contains them. Malformed vector members and duplicate names remain explicit issues.
- Synthetic TypeTree-shaped roots cover the RectTransform values and a layout without root order/Euler hint. A generated generation-10 complete SerializedFile with a 2019.4 Transform TypeTree exercises `SceneGraph::build` through the payload decoder for position, rotation, scale, root order, Euler hint, and relationship pointers. This fixture is generated locally and is not independent evidence from a Unity-produced file.
- `cargo test --workspace --locked --quiet`: exit 0; 216 library tests and 4 binary tests passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check`: exit 0. `tools/verify-baseline.ps1` verified all 1,449 reference files unchanged. `git diff --check`: exit 0, with Git reporting the existing CRLF-to-LF normalization warning for `docs/feature-parity.md`.
- Transform values are now available in the Rust scene model; parent/child consistency checks, scene roots, Prefabs, original Unity-file comparisons, runtime behavior comparisons, and Unity project export remain unimplemented. Phase 1 remains open.


## Local Transform hierarchy audit

- The local AssetRipper baseline's `TransformExtensions.CalculateRootOrder` returns zero for a root Transform and otherwise searches the father's ordered child PPtrs for the same PathID and resolved object; it throws if it cannot find that exact child. `EditorFormatConverterAsync` invokes this for versions with RootOrder. The Rust audit reports contradictions without rewriting the graph.
- `SceneGraph::audit_local_transform_links` builds a deterministic index bounded by the scene graph's existing relationship limit. It reports a missing reciprocal child, a missing or mismatched father, duplicate reciprocal links, unknown target class, or a target object that is not a Transform. Nonlocal/missing/ambiguous pointers stay in their existing explicit relationship states. Cross-file reciprocity is not audited because a `SceneGraph` holds decoded scene roots for only its source file.
- Synthetic cases cover a valid two-node hierarchy, missing parent-to-child reciprocity, null father against a populated parent child list, duplicate child pointers, and a father PPtr targeting a non-Transform class. No real Unity file was used.
- `cargo test --workspace --locked --quiet`: exit 0; 219 library tests and 4 binary tests passed, 0 failed, 0 doc tests. Warnings-denied workspace Clippy, `cargo fmt --all -- --check`, and `git diff --check` passed. `tools/verify-baseline.ps1` verified all 1,449 reference files unchanged. Git emitted CRLF-to-LF normalization warnings for the three updated Markdown files.
- Scene-root discovery, cross-file reciprocal audits, Prefab reconstruction, Unity runtime comparison, and export remain open; Phase 1 remains open.


## File-local scene root candidates

- The local baseline's `SceneHierarchyObject.GetRoots` treats GameObjects without a Transform or with a null father as scene roots. `PrefabProcessor` separately adds missing Transforms before prefab root traversal; this Rust increment only identifies existing file-local candidates and does not synthesize assets.
- `SceneGraph::discover_local_roots` processes GameObject component PPtrs in serialized order and selects the first locally resolved Transform, matching `TryGetComponent` ordering. A complete empty/non-Transform component list produces a no-Transform root. A selected Transform is a root only when its father PPtr is explicitly null. Incomplete component lists, unknown classes, multiple transforms, missing/duplicate fathers, and unresolved/cross-file pointers remain issues or non-root states. An incomplete component list prevents a root claim even if another known Transform appears parentless.
- Root and issue output are bounded by `SceneRootDiscoveryLimits`; lookup work uses a sorted relationship index rather than rescanning the full edge set for each GameObject. Synthetic cases cover a parented child, null-father root, no-Transform root, unresolved component plus a possible Transform, unresolved father, and root/issue limits.
- `cargo test --workspace --locked --quiet`: exit 0; 221 library tests and 4 binary tests passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check` passed. `tools/verify-baseline.ps1` verified all 1,449 reference files unchanged. `git diff --check` passed; Git emitted CRLF-to-LF normalization warnings for the three updated Markdown files.
- Results are candidates for one SerializedFile only. Source SceneDefinition membership, paths and GUIDs, scene-root objects, Prefab grouping and recursion, missing-Transform synthesis, cross-file hierarchy joining, and Unity project export remain unimplemented; Phase 1 remains open.


## Scene collection names and BuildSettings paths

- `scene_naming` ports the local `SceneHelpers` index rules and inverse naming: before 5.3, `maindata` maps to scene 0 and `levelN` maps to N+1; from 5.3 onward, `levelN` maps to N. Filename case and the source no-leading-zero pattern are preserved, and integer overflow returns no mapping instead of indexing with a wrapped value.
- BuildSettings path recovery strips extensions for direct `Assets/` and `Library/PackageCache/` paths, extracts those prefixes from rooted paths, rejects other rooted paths and empty entries, and places other relative names under `Assets/Scenes/`. Exact duplicate path detection and stale scene-index handling are included.
- Five synthetic tests cover the 5.3 boundary, inverse names, malformed/release versions, signed 32-bit index bounds, direct/rooted/fallback paths, stale indexes, and duplicate strings. This does not decode BuildSettings assets or integrate streamed scene bundle metadata.
- `cargo test --workspace --locked --quiet`: exit 0; 226 library tests and 4 binary tests passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check` passed after replacing the manual separator predicate. `tools/verify-baseline.ps1` verified all 1,449 reference files unchanged. `git diff --check` passed; Git emitted CRLF-to-LF normalization warnings for the three updated Markdown files.
- Scene collection discovery, BuildSettings object decoding, SceneGUID recovery, streamed-scene SceneHashes/container mapping, and SceneDefinition aggregation remain unimplemented; Phase 1 remains open.
