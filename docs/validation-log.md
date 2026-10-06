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


## BuildSettings scene-path value extraction

- Added bounded extraction from decoded BuildSettings roots. Current `m_Scenes` and source-normalized legacy `m_Levels` arrays are accepted; entries may be string values or structs with a unique inherited `path` string field. Entry order and exact path text are retained, and path extraction feeds the versioned `scene_path_for_collection` mapping.
- Both aliases present, no alias, wrong root/list/entry/path type, missing/duplicate fields, invalid UTF-8, count overflow, and configured count/byte limits produce explicit errors. Source confirms the legacy field rename in `Pass002_RenameSubnodes`; this code accepts that pre-decoded value but does not open or decode a workspace object itself.
- Four synthetic tests cover modern struct entries, legacy string entries under `Base`, the old-version index offset into decoded settings, malformed metadata, UTF-8, and limits. No original BuildSettings SerializedFile or source-runtime differential was used.
- `cargo test --workspace --locked --quiet`: exit 0; 230 library tests and 4 binary tests passed, 0 failed, 0 doc tests. The first Clippy run identified a needless explicit lifetime; it was elided, after which `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. `cargo fmt --all -- --check` passed. `tools/verify-baseline.ps1` verified all 1,449 reference files unchanged. `git diff --check` passed; Git emitted CRLF-to-LF normalization warnings for the three updated Markdown files.
- Workspace object decoding, BuildSettings file discovery, scene collection identity, SceneGUID recovery, streamed-scene `SceneHashes`, and SceneDefinition aggregation remain open; Phase 1 remains open.


## BuildSettings decode and inspect integration

- Added `decode_build_settings_scene_paths_from_parts`, which validates object-table bounds and class ID 141, decodes the original payload with `ObjectDecodeLimits`, and extracts paths under `BuildSettingsSceneLimits`. This lets workspace callers reuse the parsed metadata/header while reading original bytes from their VFS path.
- `assetripper-rs inspect` now looks for BuildSettings objects in each loaded SerializedFile. It reads that VFS file lazily once when the first class ID 141 object is encountered, decodes each matching object, and prints its ordered scene paths. Files without BuildSettings do not incur a second read. Decode or metadata errors are surfaced as command errors.
- A generated generation-10 complete BuildSettings SerializedFile exercises TypeTree payload decoding through `m_Scenes` string arrays, then resolves a collection name to its path. This is a locally generated fixture, not a Unity-produced BuildSettings file. Existing CLI workspace smoke tests continue to pass.
- `cargo test --workspace --locked --quiet`: exit 0; 231 library tests and 4 binary tests passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check` passed. `tools/verify-baseline.ps1` verified all 1,449 reference files unchanged. `git diff --check` passed; Git emitted CRLF-to-LF normalization warnings for the three updated Markdown files.
- The CLI reports paths but does not identify which LoadedSerializedFile is a source scene collection, attach paths/GUIDs, process streamed-scene `SceneHashes`, or build SceneDefinitions. Phase 1 remains open.

## CLI scene graph and root summary

- assetripper-rs inspect now detects workspace files with GameObject, Transform, or RectTransform objects, builds one bounded multi-file PPtr graph, reparses each matching file from its original VFS bytes, and reports SceneGraph counts, decode issues, local root candidates, and local father/child consistency issues.
- Only matching scene files are reread and parsed for scene values. Files with no scene classes do not build the multi-file graph or incur that read. Cross-file PPtrs resolve through the workspace graph; root discovery and reciprocity auditing remain scoped to each file-local SceneGraph.
- cargo test --workspace --locked --quiet: exit 0; 231 library tests and 4 binary tests passed. The CLI smoke fixture has class ID 1 but a generic Root TypeTree, so it exercises the new branch without validating GameObject fields or asserting the printed summary; component scene APIs have synthetic decoded-value and complete generated SerializedFile coverage. cargo clippy --workspace --all-targets --locked -- -D warnings, cargo fmt --all -- --check, tools/verify-baseline.ps1 (1,449 reference files unchanged), and git diff --check all exited 0. No Unity-produced file was used.
- Scene collection association, BuildSettings paths and SceneGUIDs, streamed-scene metadata, Prefab construction, missing-Transform synthesis, scene export, and runtime validation remain open; Phase 1 remains open.

## Scene collection manager classification

- The baseline ClassIDType extension defines IsSceneSettings as OcclusionCullingSettings, RenderSettings, LightmapSettings, and NavMeshSettings. Its generated ClassIDType declaration maps those names to ClassIDs 29, 104, 157, and 196; SceneDefinitionProcessor marks a collection as a scene when it contains an ILevelGameManager.
- Added a Rust mapping for those exact class IDs. inspect now reports each loaded file containing one or more of these manager objects as a scene-collection candidate and prints each manager class and PathID. It does not infer a candidate from file names alone.
- A unit test covers the four positive IDs and representative non-manager IDs. cargo test --workspace --locked --quiet passed 232 library tests and 4 binary tests; cargo clippy --workspace --all-targets --locked -- -D warnings, cargo fmt --all -- --check, tools/verify-baseline.ps1, and git diff --check passed. No original Unity scene file was used.
- OcclusionCullingSettings SceneGUID extraction, BuildSettings path association, streamed-scene SceneHashes/container mapping, SceneDefinition construction, Prefab processing, and export remain open; Phase 1 remains open.

## OcclusionCullingSettings SceneGUID extraction

- The baseline SceneDefinitionProcessor reads the optional SceneGUID from IOcclusionCullingSettings. GuidExtensions copies GUID.Data_0_ through GUID.Data_3_ to UnityGuid.Data0 through Data3; Pass201 confirms the generated serialized field names m_Data_0_ through m_Data_3_.
- Added extraction for optional SceneGUID/m_SceneGUID fields with four strictly typed u32 words. Missing fields remain absent; duplicate aliases, missing words, and malformed value types are explicit errors. inspect decodes class ID 29 objects and prints the four source words without byte-order rewriting.
- Synthetic tests cover inherited Base fields, all four exact words, absence, duplicate aliases, missing words, and invalid word types. A generated generation-10 OcclusionCullingSettings SerializedFile exercises TypeTree decoding and the inspect path. cargo test --workspace --locked --quiet passed 234 library tests and 5 binary tests; warnings-denied Clippy, formatting, baseline verification (1,449 reference files unchanged), and git diff --check passed.
- No Unity-produced OcclusionCullingSettings file was used. SceneGUID association with discovered collection identity, BuildSettings paths, streamed-scene SceneHashes/container mapping, SceneDefinition aggregation, and Unity project export remain open; Phase 1 remains open.

## BuildSettings paths on scene collection candidates

- The baseline SerializedAssetCollection.FromSerializedFile sets collection.Name from file.NameFixed; FileBase.Name normalizes that value through FixFileIdentifier. SceneHelpers.TryGetScenePath maps the collection name by OriginalVersion and retrieves the corresponding BuildSettings path. Collections without a supported name/index can later receive paths from streamed-scene bundle metadata.
- Added a resolver that applies the source name normalization and 5.3 scene-index boundary, recovers the Unity project path, reports stale indexes and unusable paths distinctly, and marks duplicate path entries. inspect now gathers decoded BuildSettings scene arrays and reports the path result for each manager-class candidate.
- If multiple differing BuildSettings arrays are loaded, inspect reports ambiguity instead of choosing by workspace order; source processing uses the last BuildSettings encountered in AssetCollection order, and equivalence of that order has not been established. If serialized version text is absent or malformed, no default version is guessed.
- Two resolver tests cover old/modern naming and the missing, ambiguous, unmapped, stale, and unusable states. A generated two-file workspace fixture runs the summary writer with an OcclusionCullingSettings collection plus BuildSettings and asserts both the four GUID words and the resolved Main scene path. cargo test --workspace --locked --quiet passed 236 library tests and 6 binary tests; warnings-denied Clippy, formatting, baseline verification (1,449 files unchanged), and git diff --check passed. No Unity-produced file was used.
- Streamed-scene SceneHashes/container path overrides, exact BuildSettings selection order, stripped-version default fallback, and SceneDefinition construction remain open; Phase 1 remains open.

## Streamed-scene bundle path mapping

- The baseline SceneDefinitionProcessor uses SceneHashes when the streamed AssetBundle has entries: each scene path loses its extension, rooted paths retain the Assets suffix when present, the Assets/ prefix is enforced, and the normalized collection name selects the first matching bundle collection. Without SceneHashes, Container path keys map in order to the next bundle collection already classified as a scene.
- Added bounded mapping helpers for those two source paths. SceneHashes mapping preserves input order and the first duplicate-name match. Container fallback advances a cursor through ordered scene flags. Missing collection matches and exhausted fallback candidates are errors; rooted path behavior is host-platform-specific like the source Path API.
- Three tests cover duplicate-name matching, rooted and relative paths, ordered container assignments, missing matches, and entry bounds. cargo test --workspace --locked --quiet passed 239 library tests and 6 binary tests; warnings-denied Clippy, formatting, 1,449-file baseline verification, and git diff --check passed.
- These helpers consume already decoded typed values. AssetBundle class ID 142 TypeTree decoding, streamed-bundle detection, bundle/collection grouping, and applying path overrides to scene candidates remain unimplemented; no Unity bundle or user sample was inspected.

## Streamed-scene AssetBundle field extraction

- Added a bounded extractor over generic TypeTree-decoded `SerializedValue` roots for the streamed-scene flag, ordered `SceneHashes` `(first, second)` text pairs, and ordered `Container` path keys. Missing fields stay distinguishable from `false` and empty arrays; duplicate aliases, malformed pair shapes, invalid UTF-8, aggregate entry overflow, and aggregate string-byte overflow are explicit errors.
- A generated minimal class ID 142 SerializedFile fixture exercises TypeTree decoding through field extraction and verifies the null `Container.second.asset` PPtr. The fields and pair shape are cross-checked against [Unity Technologies UnityDataTools AssetBundle format documentation](https://github.com/Unity-Technologies/UnityDataTools/blob/main/Documentation/assetbundle-format.md). The fixture is synthetic and partial; it does not prove version coverage or behavior on Unity-produced files. Bundle grouping and scene override application are not implemented.
- `cargo test --workspace --locked --quiet`: exit 0; 243 library tests and 6 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check`: exit 0. No Unity bundle, user sample, UABEA, or other repository was modified or inspected.

## Recursive container scope identities

- `ContainerMountReport` now records each visited container's virtual root in source traversal order, including nested entries after wrapper dispatch. This retains enough path identity to associate SerializedFiles with the exact source `FileContainer` before SceneHashes collection matching; no workspace association is implemented yet.
- Scope path storage has a 64 MiB aggregate per traversal limit, fallible reservations, and explicit partial-report errors. Synthetic WebData and GZip-wrapped WebData fixtures assert root/nested scope order and limit failure behavior.
- `cargo test --workspace --locked --quiet`: exit 0; 243 library tests and 6 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0. No input samples were inspected.
## Loaded SerializedFile container-scope association

- `LoadedSerializedFile` now carries the deepest known container virtual root. Initial platform expansion and dynamically expanded dependency containers both attach it; standalone files remain unscoped. The workspace limits aggregate copied scope strings to 64 MiB.
- A synthetic nested WebData test verifies that a direct leaf uses the outer container scope and a nested leaf uses the child container scope. A dependency-container fixture verifies scope propagation on the dependency path. This records container provenance but does not yet create AssetRipper `SerializedBundle`/`AssetCollection` objects or apply scene mappings.
- `cargo test --workspace --locked --quiet`: exit 0; 243 library tests and 6 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.

## SerializedFile source order in recursive container reports

- The baseline `FileContainer` keeps direct SerializedFiles in insertion order, and `SerializedBundle.FromFileContainer` creates the current bundle's collections in that order before recursively adding child bundles. `SceneDefinitionProcessor`'s Container fallback depends on this `Bundle.Collections` order.
- `ContainerMountReport` now stores each mounted SerializedFile's virtual path, source entry index, container-scope index, and zero-based collection index among direct SerializedFiles. Filtering by scope preserves direct collection order even when nested-container entries are visited between direct files. The report path metadata is bounded at 64 MiB with fallible reservations; the path limit is checked before the leaf is mounted.
- A synthetic nested WebData fixture uses minimal SerializedFile header signatures and places `z-first`, a child container, then `a-second`; it verifies root collection indexes remain 0 then 1 and the child has its own index 0 and scope. The mount report does not parse these leaf payloads. A second test verifies the metadata path limit fails before mounting the leaf.
- `cargo test --workspace --locked --quiet`: exit 0; 245 library tests and 6 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0. No user AssetBundle or UABEA content was inspected.
- Ordered leaf records are not yet propagated through the workspace or assembled into source-compatible `SerializedBundle` groups; Phase 1 remains open.

## Loaded SerializedFile source-order association

- Initial platform import now passes `ContainerMountReport.serialized_files` through `PreExpandedContainer`; dependency-container expansion builds the same order index from its mount report. The index validates path uniqueness, scope indexes, collection-order consistency, and the workspace file-count bound.
- `LoadedSerializedFile` now retains `container_entry_index` and `container_collection_index` beside its deepest container scope. A platform fixture mounts `z-first`, `shared`, a nested bundle, then `a-last`; despite sorted VFS enumeration, direct workspace files retain entry indexes 0, 1, 3 and collection indexes 0, 1, 2. The child collection has its own index 0. A dependency-container fixture verifies order metadata on dynamically loaded leaves.
- `cargo test --workspace --locked --quiet`: exit 0; 245 library tests and 6 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- The source-compatible bundle/group API and class ID 142 scene-path application remain open; Phase 1 remains open. No user AssetBundle or UABEA content was inspected.

## Per-container direct collection index API

- Added `SerializedFileWorkspace::ordered_collection_file_indices(scope)`, which returns loaded workspace file indexes sorted by the direct collection order retained from the container mount report. It reports missing order metadata and gaps rather than returning a potentially wrong order.
- The synthetic platform import fixture asserts the API returns `z-first.assets`, `shared.assets`, `a-last.assets` in source order despite lexical VFS enumeration, then injects a collection-index gap and checks for an explicit error. The nested scope and dependency-container fixtures check their separate per-container lists.
- `cargo test --workspace --locked --quiet`: exit 0; 245 library tests and 6 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- This API indexes direct collections only. AssetCollection construction, full parent/child SerializedBundle representation, and class ID 142 scene-path application remain open; Phase 1 remains open.

## Streamed-scene mapping branch selection

- Added `map_streamed_scene_bundle_paths`. It follows SceneDefinitionProcessor precedence: a nonempty SceneHashes list wins; otherwise ordered Container keys map to the next scene-flagged direct collection. A non-streamed flag returns no mappings; a selected streamed bundle with missing path metadata or mismatched collection arrays is an explicit error.
- Synthetic tests verify SceneHashes precedence over conflicting Container data, empty-SceneHashes fallback, first normalized duplicate-name match, Container cursor order, missing metadata, collection count mismatch, and the existing path/count limits. No Unity-produced class ID 142 bundle was used in these mapping tests.
- `cargo test --workspace --locked --quiet`: exit 0; 247 library tests and 6 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- The mapper is not yet called by CLI inspect or a scene exporter. Class ID 142 object discovery, scene-path override application, bundle hierarchy, and SceneDefinition construction remain open; Phase 1 remains open.

## CLI streamed-scene path mapping

- `print_workspace_summary_to` now scans class ID 142 objects, decodes their TypeTree payloads, and queues true streamed-scene bundles with their owner collection order. Mapping obtains the direct collection list from `ordered_collection_file_indices(scope)`, derives names relative to that container, and uses current manager classification plus earlier same-scope assignments for Container fallback. It reports all streamed overrides and the effective final assignment. Bundles outside a serialized container report that name recovery is unavailable.
- Aggregate inspect bounds are 1,000,000 class ID 142 objects, 1,000,000 metadata entries, 256 MiB metadata string bytes, and 1,000,000 total path mappings; each object also uses the existing 65,536 entry and 16 MiB decode/mapping limits.
- A synthetic WebData end-to-end fixture includes a class ID 29 scene collection and class ID 142 streamed AssetBundle with SceneHashes. It verifies the selected collection and effective output path `Assets/Scenes/BundleScene`. This is not a Unity-produced bundle and does not verify full `SerializedBundle` hierarchy or Unity import/runtime behavior.
- `cargo test --workspace --locked --quiet`: exit 0; 247 library tests and 7 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- CLI inspection now applies the helper, but it does not yet create SceneDefinition or AssetCollection models or export scenes. Phase 1 remains open. No user AssetBundle or UABEA content was inspected.

## SceneDefinition planning

- Added a pure `SceneDefinitionPlan` operation that retains the source workspace collection index and applies the baseline path precedence: streamed-scene override, resolved BuildSettings path, then the `FromName` path `Assets/Scenes/{NameFixed}`. `FromPath` names use the final slash- or backslash-separated component. The plan records whether the path came from each branch.
- Nonzero SceneGUID words are retained. Missing and all-zero GUIDs are represented as separate “generate at export” states, matching `SceneDefinition.FromName`/`FromPath` without inventing a GUID during inspection. Collection names have a 1 MiB bound; SceneDefinition names and paths have separate 16 MiB bounds.
- Synthetic unit tests cover path precedence, `FileBase.NameFixed` normalization, fallback naming, GUID preservation/deferred generation, and length limits. The existing synthetic WebData CLI fixture verifies the complete planned streamed path and a preserved nonzero four-word GUID. One initial boundary-test run used a collection name longer than the earlier collection-name bound; after correcting that test input, the final suite passed.
- `cargo test --workspace --locked --quiet`: exit 0; 250 library and 7 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- This operation reports a plan; it does not yet materialize source `AssetCollection` or `SceneDefinition` objects, preserve the complete parent/child `SerializedBundle` tree, generate EditorBuildSettings, or export a Unity project. No Unity-produced input or reference-tool differential was used. Phase 1 remains open, and no user AssetBundle or UABEA content was inspected.

## Recursive container hierarchy records

- `ContainerMountReport` now records every container node in recursive root-first traversal order. Each node points to its optional virtual scope and records its parent node plus the source entry index that contained it; a root node is retained even when no virtual destination was provided.
- The node list is bounded by the existing global entry limit plus one root record, and node reservation/count overflow are explicit errors. Existing scope and per-container SerializedFile ordering metadata remain intact.
- Synthetic nested and gzip-wrapped WebData fixtures assert exact parent indexes, source entry indexes, and scope links. A SerializedFile interleaving fixture verifies the tree addition does not change direct collection ordering. No Unity-produced file was used.
- `cargo test --workspace --locked --quiet`: exit 0; 250 library and 7 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- At this validation point, the hierarchy was available in the mount report only; workspace transfer is recorded in the following increment. Resource leaves are not associated with bundle nodes. No user AssetBundle or UABEA content was inspected, and Phase 1 remains open.

## Workspace container tree and collection links

- Initial platform pre-expansions now carry the mount report's parent/child container records into `SerializedFileWorkspace`. Dependency containers append nodes with remapped global parent indexes. Each node stores its scope path and its direct SerializedFile collection workspace indexes in source order; pathless root nodes still retain direct collections, and loaded files expose their global bundle-node index.
- The workspace validates parent ordering, one-to-one scope links, direct collection contiguity, and that every mounted SerializedFile resolves to a loaded workspace file. Aggregate defaults cap the workspace view at 3,000,000 bundle nodes and 64 MiB of copied node paths; errors are explicit.
- At this validation point, the workspace model linked only SerializedFile collections; ResourceFile links are covered in the following increment. A focused helper test exercises node-path and bundle-count limit failures. FailedFiles and Unity-generated AssetCollections remain unmodeled.
- `cargo test --workspace --locked --quiet`: exit 0; 252 library and 7 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0. No Unity-produced files, user AssetBundles, or UABEA content were inspected; Phase 1 remains open.

## Workspace ResourceFile leaf links

- `ContainerMountReport` records each ResourceFile leaf's mounted virtual path, local owning bundle index, source entry index, and direct `resource_index`. The workspace remaps bundle indexes globally and exposes stable resource-file indexes on each node; workspace resources are ordered by bundle-tree traversal and source order within each container.
- `ContainerMountLimits` bounds aggregate resource-path metadata at 64 MiB by default. `SerializedFileWorkspaceLimits` independently bounds copied workspace resource paths at 64 MiB. Resource bytes remain in the VFS and are not copied into the workspace metadata.
- Synthetic WebData coverage checks root and nested resource ownership/order, source entry indexes, path-byte limits at both mount and workspace layers, noncontiguous direct resource-order rejection, pathless root membership, and a dynamically expanded dependency container. The VFS still returns the exact synthetic stream bytes by recorded path.
- `cargo test --workspace --locked --quiet`: exit 0; 255 library and 7 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- No Unity-produced files, user AssetBundles, or UABEA content were inspected. FailedFiles, actual AssetCollection objects, resource decoding/export, and Unity/reference-tool differential checks remain open; Phase 1 is not passed.

## Streamed-scene paths use workspace bundle identity

- A streamed-scene AssetBundle object now selects its direct collection list through its owning `LoadedSerializedFile.container_bundle_index` and `WorkspaceSerializedBundle.direct_collection_file_indices`. This mirrors the source `assetBundleAsset.Collection.Bundle.Collections` relationship and avoids scope-path lookup as the identity key.
- Collection order and each loaded file's bundle/order indexes are checked before scene path mapping. A bundle with no virtual scope path uses the file identity stored on its direct collection rather than failing solely because a path string is absent. AssetBundles without a SerializedBundle owner still report scene-name recovery as unavailable.
- A focused pathless-node unit test verifies indexed owner lookup and direct collection order. The synthetic WebData CLI fixture still verifies SceneHashes mapping, streamed path override, and GUID preservation end to end. No actual AssetCollection or SceneDefinition object is materialized.
- `cargo test --workspace --locked --quiet`: exit 0; 255 library and 8 binary tests passed. Warnings-denied Clippy, format, the 1,449-file baseline verifier, and `git diff --check` pass.
- No Unity-produced files, user AssetBundles, or UABEA content were inspected. Phase 1 remains open.

## Source collection traversal order and BuildSettings selection

- Added `SerializedFileWorkspace::source_collection_file_indices()` to model `GameBundle` collection fetch order. It emits standalone workspace files in reverse loaded order, then root container bundles in reverse insertion order; every bundle emits direct collections before child bundles ordered by their source entry indexes. Invalid, duplicate, or incomplete file/bundle links return explicit workspace errors.
- `inspect` now chooses the final BuildSettings object in that traversal, matching `SceneDefinitionProcessor` overwrite behavior, and processes streamed-scene AssetBundle objects in collection/object order. It reports the selected BuildSettings object; differing BuildSettings lists no longer become ambiguous solely because several objects exist.
- Synthetic tests verify a six-file workspace with reversed standalone/root order, a nested tree whose siblings are ordered by source entry rather than node index, and a CLI workspace with two different BuildSettings objects where the final source-order object is selected.
- `cargo test --workspace --locked --quiet`: exit 0; 256 library and 9 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- Source platform registration paths are exposed through a `HashSet`, so original root-input order is runtime-dependent. Rust applies deterministic workspace insertion order and mirrors the defined reverse-stack and recursive bundle rules within it. No Unity-produced file or C# runtime differential was used; Phase 1 remains open and no user AssetBundle or UABEA content was inspected.

## Workspace SceneDefinition membership model

- Added `WorkspaceSceneDefinitions` and `WorkspaceSceneDefinition`. Scene plans are materialized as bounded workspace models with ordered collection-file membership and a collection-to-scene index. `add_collection` mirrors source single-scene ownership by rejecting a collection already assigned to any scene.
- CLI scene processing now builds these models before printing them; its output includes the SceneDefinition model index and linked workspace collection indexes.
- Synthetic tests cover initial membership, collection lookup, appending another collection, duplicate ownership, and invalid scene/collection indexes. The streamed-scene WebData CLI fixture asserts the model output and its collection link.
- `cargo test --workspace --locked --quiet`: exit 0; 257 library and 9 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- The model still refers to SerializedFile workspace indexes rather than materialized `AssetCollection` objects. Missing/zero GUIDs remain generation policies, and Prefab-generated collections, generated EditorBuildSettings assets, export, and runtime/reference-tool differential validation remain open. Phase 1 remains open; no user AssetBundle or UABEA content was inspected.

## EditorBuildSettings scene model

- Added a bounded `EditorBuildSettingsModel` copied from the final source-order BuildSettings path list. It preserves order and duplicate paths and marks every generated scene entry enabled, matching `SceneDefinitionProcessor`.
- CLI inspection builds and reports this model when a BuildSettings object exists; it does not emit a Unity asset.
- Synthetic tests cover path order, duplicates, enabled state, scene-count limits, and aggregate path-byte limits. A CLI fixture with two conflicting BuildSettings objects confirms the selected object's path is used.
- `cargo test --workspace --locked --quiet`: exit 0; 259 library and 9 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- The source Generated Settings `AssetCollection`, concrete `EditorBuildSettings` object serialization, `EditorSettings.SetToDefaults`, and Unity project export are still open. No Unity-produced file, user AssetBundle, or UABEA content was inspected; Phase 1 remains open.

## EditorSettings baseline defaults

- Read the local `EditorSettingsExtensions.SetToDefaults` implementation and modeled its semantic assignments: version-control modes, ForceText serialization, inline mappings, SpritePacker and ETC values, project-generation extensions/namespace, optional Collab and AssetPipeline settings, streaming/shader flags, Cache Server values, lightmap/probe flags, and enter-play options.
- `EditorSettingsCapabilities` makes the source `Has_CollabEditorSettings` branch and the version-conditional AssetPipelineMode field explicit. The model stores enum meanings rather than guessing generated numeric serialization constants.
- Synthetic tests assert every baseline value and both combinations where the optional fields are absent. `cargo test --workspace --locked --quiet`: exit 0; 261 library and 9 binary tests passed. Warnings-denied Clippy, format, baseline, and diff checks pass.
- The defaults model is not yet connected to a target Unity schema or CLI Generated Settings collection. Numeric enum serialization, concrete EditorSettings object construction, `AssetCollection` materialization, and Unity project export remain open. No Unity-produced file or reference-tool differential was used; Phase 1 remains open.

## Bounded object workspace TypeTree decoding

- Added `ObjectWorkspace::load_from_vfs` to decode all objects in workspace order while preserving file/object indexes, PathID, serialized/resolved type identity, class/script/destroyed/stripped metadata, and each source payload byte range.
- Object-count, per-file byte, aggregate decoded-allocation, and existing per-object decode bounds are enforced. Objects that cannot be decoded remain present with their contextual error and source VFS range; reports distinguish decoded objects, undecoded objects, decoded allocation, and raw fallback byte totals.
- CLI inspection now reports the workspace decode totals and up to 128 per-object failures with file, object, PathID, class, and byte-range context. The object model is dropped before later scene inspection to release decoded values.
- A synthetic generation-10 fixture verifies decoded nested values and PPtr identity, copied type metadata, zero aggregate decode allowance with source-range preservation, and an explicit object-count-limit failure. `cargo test --workspace --locked --quiet`: exit 0; 263 library and 9 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` exited 0.
- This is only a bounded TypeTree value view. It does not implement generated class-specific object materialization, no-TypeTree recovery, managed-reference registries, or full object processing. CLI inspection decodes the complete workspace within its limits. No Unity-produced files, user AssetBundles, UABEA, or C# runtime differential were used; Phase 1 remains open.

## CLI cross-file TypeTree reference summary

- `inspect` now builds the bounded multi-file join for every loaded workspace and reports recognized PPtr totals split into null, uniquely resolved, and unresolved targets, per-file scan issue counts, and graph completeness. Up to 128 unresolved PPtrs or scan issues are printed with source file/object/PathID/field context; any omitted count is reported.
- The join reuses per-file scans already built during workspace loading. It matches supplied files by normalized SerializedFile name and preserves missing, duplicate, invalid-FileID, and missing-target states. It does not discover missing inputs or resolve references hidden in undecodable payloads and raw fields.
- A CLI helper test uses a synthetic generation-10 TypeTree to check null, unique local, and missing local PPtrs, completeness, and unresolved diagnostics. The first compile attempt caught a misspelled crate path in the diagnostic helper; corrected it to `assetripper` and reran. `cargo test --workspace --locked --quiet`: exit 0; 263 library and 10 binary tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` passed.
- No Unity-produced files, user AssetBundles, UABEA, or C# runtime differential were used. Embedded-resource identity mapping and source-runtime differential remain open; Phase 1 remains open.

## Source-ordered SerializedFile collection model

- Added bounded `WorkspaceAssetCollections` records in the same collection order used by the workspace's source traversal. Each record retains its normalized collection name, source file and bundle indexes, external identifier count, and a checked range into the flat `ObjectWorkspace` records; a reverse file-to-collection index is also available.
- Collection, object, external-identifier, and aggregate normalized-name limits are enforced. The model validates that every flat object record matches its expected file and object index before grouping.
- CLI inspection now reports collection/object/dependency totals and labels each SerializedFile using its collection index and object range. Source external identifiers remain available through the linked `LoadedSerializedFile` metadata.
- A two-file synthetic fixture checks reversed standalone collection order, file-to-collection mapping, object ranges, name normalization, and collection/object/name bounds. The initial compile reported an error conversion from the source traversal error into the boxed model error; the conversion was fixed and rerun. `cargo test --workspace --locked --quiet`: exit 0; 264 library and 10 binary tests passed. Warnings-denied Clippy, format, the 1,449-file baseline verifier, and `git diff --check` also exited 0.
- This is a grouping model, not concrete `Bundle`, `AssetCollection`, or generated `IUnityObjectBase` construction. Source dependency initialization, default-version fallback, transfer flags, and export remain open. No Unity-produced files, user AssetBundles, UABEA, or C# differential were used; Phase 1 remains open.

## Source-scoped Bundle collection reference resolution

- `SerializedFileWorkspace::reference_graph` now resolves external collection identifiers using the mounted Bundle hierarchy instead of globally treating all same-name SerializedFiles as ambiguous. The lookup follows `Bundle.ResolveCollection`: inspect the owning bundle's direct collections, then its direct child bundles in insertion/source-entry order, climb to parent collections and sibling child bundles while excluding the branch just traversed, and inspect GameBundle root collections before root child bundles.
- The lookup applies `FileIdentifier.GetFilePath`, `SpecialFileNames.FixFileIdentifier`, and the source aliases for default resources and builtin extras. Once a target collection is selected, its local PathID index still distinguishes unique, missing, and ambiguous objects. Resolved or missing target collections are cached by external FileID within each source SerializedFile, matching dependency-list initialization before per-object PPtr access. The generic standalone `MultiFileReferenceGraph::build` retains filename-global behavior; the workspace product path uses the source-scoped resolver.
- Synthetic workspace tests construct duplicate `shared.assets` collections and verify root collection precedence over a child bundle, current-bundle collection precedence over root duplicates, first-match order among duplicate root bundles, and child selection by source entry index even when workspace node order is different. The initial compile found missing test imports for `SerializedFile` and the workspace report; imports were corrected and all tests rerun. `cargo test --workspace --locked --quiet`: exit 0; 267 library and 10 binary tests passed.
- Warnings-denied Clippy initially flagged a complex callback signature, a `let-else` that could use `?`, and then the resolver's argument count after adding the cache. A callback type alias, idiomatic `?`, and a grouped source-identity record fixed these warnings. Final `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 files unchanged), and `git diff --check` all exited 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used; Phase 1 remains open.


## Workspace-aware cross-file scene roots

- Added workspace-aware SceneGraph root discovery driven by the same ordered MultiFileReferenceGraph used to resolve PPtrs. Component references that resolve to objects in another loaded SerializedFile are classified from that target file's object metadata; Transform and RectTransform targets are followed to the target object's m_Father reference. A null father produces a root candidate. Missing, ambiguous, invalid, unknown-class, and absent-father states remain explicit issues. MultiFileObjectReference retains the serialized FileID/PathID pair, and scene graph joining rejects a same-named source graph when that pair differs.
- The CLI now reports workspace-aware root candidates per source file and includes the target Transform's file/object identity. Local Transform parent/child reciprocal auditing is still file-scoped and is labeled that way.
- Synthetic generation-10 SerializedFiles test an external GameObject-to-Transform component edge with a null father and a missing external Transform father. The latter produces no root candidate and retains a MissingLocalObject diagnostic. A separate mismatch fixture uses the same file name and object PathID but different serialized PPtr PathIDs, and verifies the original scene relationship remains unchanged after the graph join is rejected.
- The first targeted compile exposed an uninferred tuple type for the selected Transform identity; adding the explicit Option<(usize, usize)> resolved it. cargo test --workspace --locked --quiet then passed: 270 library tests, 10 binary tests, 0 doc tests. cargo clippy --workspace --all-targets --locked -- -D warnings, cargo fmt --all -- --check, tools/verify-baseline.ps1 (1,449 files unchanged), and git diff --check all exited 0.
- No Unity-produced asset, user VRChat AssetBundle, UABEA source or executable, or C# runtime differential was used. SceneDefinition export, workspace-wide reciprocal hierarchy auditing, Prefab processing, and Unity project export remain incomplete; the Phase 1 gate remains closed.
## Workspace reciprocal Transform hierarchy audit

- `SceneGraph::audit_workspace_transform_links` now resolves each source graph against the ordered multi-file reference graph, checks Transform/RectTransform parent and child PPtrs across SerializedFiles, and reports missing, duplicate, mismatched, unresolved, or incomplete-endpoint states. The CLI reports checked Transform-link and issue counts plus whether the audit completed without issues.
- The reciprocal audit checks that the source object scan is complete before making an edge-completeness claim. It also requires a complete target scan before reporting a missing reciprocal, so unknown/raw fields cannot be silently treated as absent pointers.
- Synthetic two-file generation-10 fixtures verify reciprocal child/parent links from both file directions, a truly absent reverse pointer, incomplete source and counterpart scans, strict array-index matching, and relationship/issue limits. Two compile-time issues were corrected during implementation: `.as_str()` on a borrowed `&str` used unstable `str_as_str`, and a helper-boundary test needed `super::` to address the private sibling helper. The focused and full suites were rerun successfully.
- `cargo test --lib --locked --quiet cross_file`: exit 0, 6 passed. The dedicated incomplete-source test: exit 0, 1 passed. `cargo test --workspace --locked --quiet`: exit 0, 275 library tests and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- No Unity-produced input, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open; scene export, Prefab processing, complete version coverage, and a dedicated CLI output assertion remain outstanding.
## SceneDefinition-owned workspace hierarchy model

- Added `WorkspaceSceneHierarchyBuilder` and `WorkspaceSceneHierarchies`. The inspect pipeline attaches each scene collection's known GameObject/Transform/RectTransform records, recognized PPtr relationships, root candidates, parse issues, and reciprocal audit results to the owning SceneDefinition plan. It retains external Transform identities even when the target collection is not itself scene-owned.
- The compact scene collection model tracks whether its scene graph fully resolves and whether every object in the owning SerializedFile completed its TypeTree reference scan. An incomplete payload/reference scan therefore keeps the scene hierarchy marked incomplete. Aggregate object, relationship, issue, name-byte, and diagnostic-byte budgets are checked before committing each collection snapshot; a budget failure leaves the builder unchanged.
- The inspect path now builds an empty graph for a manager-only scene collection and rebuilds only later streamed-scene target collections whose status becomes known after AssetBundle path mapping. A generated two-file API test verifies a GameObject's external Transform root is retained in the SceneDefinition collection model; the streamed-scene WebData CLI fixture asserts attachment of its manager-only collection graph.
- `cargo test --lib --locked --quiet scene_definition_owns_compacted_cross_file_hierarchy_data`: exit 0, 1 passed. `cargo test --bin assetripper-rs --locked --quiet workspace_summary_applies_streamed_scene_bundle_path_override`: exit 0, 1 passed. `cargo test --workspace --locked --quiet`: exit 0, 276 library and 10 binary tests passed (0 doc tests). The first warnings-denied Clippy run rejected a test's post-Default field reassignment; it was changed to a struct-update initializer before the final check. The first cargo check reported two unused imports, which were removed. Final `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 files unchanged), and `git diff --check` all exited 0.
- No Unity-produced scene, user AssetBundle, UABEA, or C# runtime differential was used. This model still lacks generated component/interface materialization, full source SceneHierarchyObject assets, Prefab processing, and Unity project export; Phase 1 remains open.

## Source SceneHierarchyObject identities and component candidates (2026-10-02)

- Added bounded per-SceneDefinition collection identities for the source `SceneHierarchyObject.Create` categories whose class IDs are available in the local baseline: GameObject (1), LevelGameManager (29, 104, 157, 196), PrefabInstance (1001), and SceneRoots (1660057539). Each identity retains workspace file index, object-table index, PathID, and class ID.
- Added a separate component-candidate index with explicit evidence: Transform/RectTransform class IDs; resolved `GameObject.m_Component` references, including cross-file target identities; and non-null TypeTree `m_GameObject` PPtrs after zero or more `Base.` prefixes. These are candidate identities only; generated `IComponent`/`IMonoBehaviour` interfaces and concrete component objects are not claimed.
- The CLI scene summary reports asset and candidate totals broken down by category/evidence. `max_scene_asset_records` defaults to 20,000,000 and covers retained asset identities plus deduplicated component candidates. Rejection is checked before committing a collection graph.
- Synthetic generation-10 API coverage verifies a GameObject asset identity, a referenced external Transform identity, and an over-limit rejection that leaves the builder unchanged. Unit cases cover every recognized source asset class ID and inherited `m_GameObject` path matching. The streamed-scene CLI fixture checks its manager-only asset count.
- `cargo check --workspace --locked`: exit 0. `cargo test --lib --locked --quiet scene_hierarchy::tests`: exit 0, 2 passed. `cargo test --lib --locked --quiet scene_definition_owns_compacted_cross_file_hierarchy_data`: exit 0, 1 passed. `cargo test --bin assetripper-rs --locked --quiet workspace_summary_applies_streamed_scene_bundle_path_override`: exit 0, 1 passed.
- `cargo test --workspace --locked --quiet`: exit 0, 278 library tests and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` exited 0.
- No Unity-produced scene, user AssetBundle, UABEA, or C# runtime differential was used. Generated component processing, Prefab processing, SceneAsset/project export, and the Phase 1 gate remain open.

## Scene-owned missing Transform plans (2026-10-02)

- Implemented the scene-owned portion of `PrefabProcessor.AddMissingTransforms`: every SceneDefinition GameObject that root discovery classifies as `NoTransformComponent` now receives a bounded `WorkspaceMissingTransformPlan` retaining its file/object/PathID identity.
- The plan records the baseline `TransformExtensions.InitializeDefault` semantics: local position zero, local rotation identity, local scale one, root order zero, and a zero Euler hint only when the chosen Unity schema contains that optional field. The type is explicitly a processing plan; it does not allocate a generated Unity Transform, assign a generated PathID, or mutate a component list.
- CLI scene summaries now expose the number of missing-Transform plans. The record budget includes these plans, and an over-limit insertion remains transactional.
- A synthetic generation-10 GameObject with an empty `m_Component` array verifies `NoTransformComponent`, stable source identity, all default values, and rejection at the one-record bound without partial graph insertion. The first test compile found one missing test-only import (`SceneHierarchyAuditLimits`); adding that import fixed compilation, and the focused test passed.
- `cargo check --workspace --locked`: exit 0. `cargo test --lib --locked --quiet scene_definition_plans_source_defaults_for_a_missing_game_object_transform`: exit 0, 1 passed. The first test compile omitted the test-only `SceneHierarchyAuditLimits` import; it was added and the test rerun successfully.
- The first full-suite rerun exposed a previous limit test that still expected the older error-resource label after the bound began covering generated-Transform plans. The assertion was updated to the current resource name, the focused test passed, and the complete suite passed.
- `cargo test --workspace --locked --quiet`: exit 0, 279 library tests and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` exited 0.
- This plan covers SceneDefinition-owned GameObjects only. Non-scene GameObjects, generated Transform objects and PathIDs, source component-list mutation, and the remainder of `PrefabProcessor` remain unimplemented. No user AssetBundle, UABEA, or Unity-produced file was inspected; Phase 1 remains open.

## Workspace-wide missing Transform routing (2026-10-02)

- Added `WorkspaceGeneratedTransformPlanBuilder` to collect `NoTransformComponent` identities from each loaded SerializedFile graph, with an aggregate 20,000,000-plan bound and duplicate-source protection. Final SceneDefinition ownership is applied only after streamed-scene path discovery and collection assignment are complete.
- Scene-owned GameObjects remain represented by their per-SceneDefinition plans. Non-scene GameObjects now appear in a bounded `WorkspaceNonSceneTransformPlans` list corresponding to the source `Missing Prefab Transforms` collection. Both carry the source-derived defaults but neither allocates an output Transform or PathID.
- The inspect pipeline contributes roots from every file containing GameObjects and avoids resubmitting already analyzed files when a late streamed-scene mapping requires a scene hierarchy snapshot. The CLI reports the non-scene plan count and bounded identity diagnostics.
- Synthetic coverage verifies routing to non-scene output when no SceneDefinition owns the collection, routing away from the non-scene list when a SceneDefinition does own it, the zero-plan limit's transactional failure, and the manager-only CLI summary.
- `cargo test --lib --locked --quiet scene_definition_plans_source_defaults_for_a_missing_game_object_transform`: exit 0, 1 passed. `cargo test --bin assetripper-rs --locked --quiet workspace_summary_applies_streamed_scene_bundle_path_override`: exit 0, 1 passed. `cargo test --workspace --locked --quiet`: exit 0, 279 library and 10 binary tests passed (0 doc tests).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` exited 0. No user AssetBundle, UABEA, or Unity-produced input was inspected; actual generated Transform objects/PathIDs, collection mutation, and the rest of Prefab processing remain open.

## Ordered GameObject component relationship index (2026-10-02)

- Added a per-GameObject compact range into ordered `GameObjectComponent` relationship indexes on each SceneDefinition-owned collection graph. The index is sorted by source object-table index and original relationship position, so `m_Component` slots preserve their serialized order without copying relationship payloads.
- Added checked iteration that reports out-of-range GameObject hierarchy indexes and validates the internal relationship range/index before returning edges. Existing scene-object and relationship limits bound the two new index allocations, which use fallible reservation.
- Extended the synthetic generation-10 cross-file GameObject fixture to contain one resolved external Transform PPtr followed by an explicit null slot. Coverage verifies array order and field paths, original file/path identity, resolved target identity, null preservation, and out-of-range diagnostics. The first focused assertion expected the generated-property suffix `.component`; the serialized TypeTree scanner correctly reports the source field path `m_Component[n]`, and the assertion was corrected.
- The first warnings-denied Clippy run identified a single-iteration `while` in the source-consistency error check. It was simplified to an `if`; the focused test and all validation were rerun afterward.
- `cargo test --lib --locked --quiet scene_definition_owns_compacted_cross_file_hierarchy_data`: exit 0, 1 passed. `cargo test --workspace --locked --quiet`: exit 0, 279 library tests and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 baseline files unchanged), and `git diff --check` exited 0.
- No user AssetBundle, UABEA, Unity-produced scene, or original-runtime differential was used. This index does not construct generated component objects or PrefabHierarchy objects, recursively traverse Transform children, or export a Unity project; Phase 1 remains open.

## Retain non-scene hierarchy graph snapshots (2026-10-02)

- The inspect path now adds every decoded GameObject/Transform collection graph to the bounded workspace hierarchy builder, including graphs that are not assigned to a SceneDefinition. Final routing occurs only after streamed-scene ownership is complete: assigned graphs remain with their scene, while other graphs are returned in workspace-file order as `non_scene_collections`.
- Renamed the aggregate graph-count limit to `max_workspace_collection_graphs` because it now covers both scene-owned and non-scene snapshots. The graph builder checks this limit before committing a snapshot; finalization also checks the definitions' workspace collection count. Non-scene missing-Transform plans are cleared from the retained input snapshot because the workspace-wide `Missing Prefab Transforms` result already owns those plans.
- Synthetic generation-10 API coverage verifies unassigned graph retention, scene grouping, and transactional rejection at a zero graph limit. The CLI summary fixture adds a non-scene GameObject collection and verifies the workspace reports one retained graph. Focused API and CLI tests passed.
- `cargo test --lib --locked --quiet scene_definition_owns_compacted_cross_file_hierarchy_data` and `cargo test --bin assetripper-rs --locked --quiet workspace_summary_maps_scene_collection_path_and_guid`: exit 0, 1 passed each. `cargo test --workspace --locked --quiet`: exit 0, 279 library and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 files unchanged), and `git diff --check` all exited 0.
- No user AssetBundle, UABEA, or Unity-produced asset was accessed. The snapshots remain relationship data only; PrefabInstance root resolution, recursive GameObject traversal, generated asset construction, and export remain open. Phase 1 is not complete.

## Ordered cross-file GameObject hierarchy walk (2026-10-02)

- Added per-Transform indexes for serialized-order `m_Children` and `m_GameObject` relationships, with indices validated against bounded relationship snapshots. The workspace graph lookup maps every retained collection file index to its SceneDefinition or non-scene graph without copying graph payloads.
- Added `WorkspaceSceneHierarchies::fetch_game_object_hierarchy`, which yields the root identity, each resolved non-null `m_Component` identity in serialized order, then recursively walks the last resolved Transform/RectTransform's children in source array order through each child Transform's `m_GameObject` edge. Repeated noncyclic references are retained. Active-path cycles, incomplete visited collection graphs, missing/unresolved targets, unknown component classes, and mismatched identities remain explicit issues; configured depth, entry, issue, pending-action, and allocation bounds fail explicitly instead of truncating.
- Synthetic model coverage verifies ordered output across two collection files, null component skipping, last-Transform selection, the child Transform-to-GameObject link, unresolved component reporting, cycle reporting, and depth/entry limit errors. Earlier SerializedFile-backed scene tests continue to cover relationship field and target decoding.
- `cargo test --lib --locked --quiet fetch_hierarchy_`: exit 0, 3 passed. `cargo test --lib --locked --quiet scene_definition_owns_compacted_cross_file_hierarchy_data`: exit 0, 1 passed. `cargo test --workspace --locked --quiet`: exit 0, 282 library and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings` exited 0.
- No Unity-produced asset, user AssetBundle, UABEA, or original-runtime differential was used. This is an identity traversal API only; it does not materialize `IEditorExtension` objects, select roots from PrefabInstance assets, reproduce PrefabProcessor global processing/dedup order, create PrefabHierarchy assets, or export a Unity project. Phase 1 remains open.

## Resolve PrefabInstance root GameObjects (2026-10-02)

- The local baseline `PrefabProcessor.Process` processes scene hierarchies first, existing PrefabInstances in `GameBundle.FetchAssets()` order second, and remaining GameObjects in that same workspace asset order third. `PrefabInstanceExtensions.GetRootGameObject` uses `RootGameObjectP`; the serialized TypeTree field is handled as `m_RootGameObject`.
- `SceneGraph` now parses class ID 1001 root PPtrs and cross-checks the field and PPtr value against the joined TypeTree reference graph. The hierarchy snapshot builds a sorted relationship index, enumerates PrefabInstances in workspace file/object order, and resolves the root to a GameObject identity across collection files. Null, unavailable fields, unresolved targets, unavailable identities, PathID mismatches, wrong target classes, and duplicate relationships are not silently accepted as null.
- The inspect summary reports resolved/null/unavailable/unresolved counts and at most 128 detailed unavailable or unresolved results. A file containing PrefabInstance records now enters the hierarchy inspection path even if it has no GameObject or Transform records itself.
- SerializedFile-backed TypeTree coverage verifies class ID 1001 root PPtr decoding and resolution through a joined two-file reference graph. Synthetic decoded-value fixtures verify null/missing field behavior; synthetic cross-file workspace graphs verify root identity resolution, workspace-order enumeration, wrong target class, PathID mismatch, and duplicate-reference handling. `cargo test --workspace --locked --quiet`: exit 0, 289 library and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `cargo fmt --all` completed before tests. Format-check, baseline checksum, and diff checks are run again for this increment before commit/push. No original AssetRipper runtime, Unity-produced asset, user AssetBundle, or UABEA was executed/accessed. Scene-owned membership, source processing order/deduplication, `SetPrefabInternal`, generated hierarchy-asset construction, and Unity project export remain open; Phase 1 is not complete.

## Follow source collection order for scene and PrefabInstance inspection (2026-10-02)

- Baseline inspection confirmed that `Bundle.FetchAssets()` visits each bundle's direct collections before recursively visiting its child bundles, while `SerializedAssetCollection.ReadData` inserts assets in SerializedFile object-table order. Workspace file index order can differ from this source sequence for nested bundles.
- SceneDefinition plans are now discovered in `source_collection_file_indices()` order. PrefabInstance root diagnostics use a new caller-ordered identity iterator with that same collection sequence and each graph's serialized asset order; the existing workspace-file-order helper remains available with an explicit contract.
- Added a regression test where workspace file index order is `[0, 1]` but the supplied source collection order is `[1, 0]`; PrefabInstance identities follow the supplied order.
- `cargo test --lib --locked --quiet enumerates_prefab_instances_in_supplied_source_collection_order`: exit 0, 1 passed. `cargo test --workspace --locked --quiet`: exit 0, 290 library and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 baseline files unchanged), and `git diff --check`: exit 0.
- This aligns discovery/diagnostic ordering only. SceneDefinition processed membership, GameObject GetRoot's first-Transform parent walk, global processing deduplication, `SetPrefabInternal`, generated PrefabInstance/PrefabHierarchy construction, and Unity project export remain open. No user AssetBundle, UABEA, original AssetRipper runtime, or Unity-produced input was inspected or executed; Phase 1 remains open.

## Resolve source GetRoot GameObject identities (2026-10-02)

- The local baseline `GameObjectExtensions.GetRoot` selects the first Transform from the GameObject component list, follows each Transform's `m_Father` while the parent has a non-null `m_GameObject`, and returns the last Transform's owner. `AddMissingTransforms` runs before this path; a source GameObject without a Transform is returned as its own root after that generated Transform is inserted.
- Added ordered per-Transform `m_Father` relationship ranges beside the existing `m_Children` and `m_GameObject` ranges. Added `WorkspaceSceneHierarchies::get_root_game_object`, which retains the first Transform and parent chain, resolves cross-file parent and owner references, distinguishes null from unavailable or unresolved fields, and detects wrong classes, duplicate fields, identity mismatches, cycles, incomplete graphs, and depth exhaustion.
- Synthetic workspace coverage verifies first-Transform selection despite a later RectTransform, a cross-file parent chain, an object requiring the source missing-Transform path, unresolved parent PPtrs, a parent with a null GameObject owner, cycles, and the configured depth limit. The focused `get_root_` test filter passed 3 tests before the additional null-owner assertion; the full suite below includes that assertion.
- `cargo test --workspace --locked --quiet`: exit 0, 293 library and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 baseline files unchanged), and `git diff --check`: exit 0.
- This resolves root identities only; the workspace-wide PrefabProcessor membership/dedup planner, `SetPrefabInternal`, generated asset construction, and Unity project export remain open. The API is not yet invoked by CLI processing. No user AssetBundle, UABEA, original AssetRipper runtime, or Unity-produced input was inspected or executed; Phase 1 remains open.

## Ordered PrefabProcessor identity plan (2026-10-02)

- Added `plan_workspace_prefab_processing`, following the local source pass order: seed the processed GameObject set from SceneDefinitions, process existing PrefabInstances in `GameBundle.FetchAssets()` order, then process remaining GameObjects in that same collection/object order. Processed membership is keyed by workspace file/object identity and retains first-insertion order for audit.
- Existing PrefabInstance root null, root unavailable, already-processed, completed hierarchy, and incomplete hierarchy outcomes remain distinct. The remaining GameObject pass records GetRoot results, skips roots already claimed by an earlier pass, and stops if a hierarchy walk is incomplete rather than reporting downstream processing as complete.
- Completed identity walks now include virtual generated-Transform plan entries after each GameObject's source components when `AddMissingTransforms` applies, including source default values. These are planning records only: no Unity object, component-list mutation, PathID, or generated collection is created.
- `inspect` now reports bounded plan totals and up to 128 root/hierarchy diagnostics. Synthetic tests cover scene seeding before existing instances, explicit null versus unavailable PrefabInstance roots, cross-file source ordering, hierarchy deduplication, incomplete-walk stop behavior, generated-Transform plan ordering/defaults, and event/entry limits. A CLI fixture checks the plan summary and surfaced incomplete-graph issue.
- `cargo test --workspace --locked --quiet`: exit 0, 298 library and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 baseline files unchanged), and `git diff --check`: exit 0.
- This is an identity plan, not AssetRipper's generated `SceneAsset`, `PrefabHierarchyObject`, or generated `PrefabInstance` materialization. It records `SetPrefabInternal` intent but does not mutate objects. No Unity-produced input, user AssetBundle, UABEA, or original AssetRipper runtime was used; Phase 1 remains open.

## Processed prefab asset PathID allocation plan (2026-10-02)

- Reference behavior was traced through the local ProcessedAssetCollection.CreateAssetInfo, ProcessedBundle.AddNewProcessedCollection, PrefabProcessor.Process, SceneHierarchyObject.Create, PrefabHierarchyObject.Create, and GameObjectExtensions.CreatePrefabForRoot. Generated PathIDs begin at 1 independently in each processed collection.
- Added an ordered allocation plan for the three initial collections, lazily created per-scene collections, missing Transforms in source FetchAssets() order, one SceneAsset per SceneDefinition, existing PrefabHierarchy assets, and generated PrefabInstance/PrefabHierarchy pairs. The plan records source object identities and class IDs (Transform 4, PrefabInstance/PrefabHierarchy 1001, SceneAsset 1032). Incomplete processing or an incomplete/missing collection graph yields an explicit incomplete result with no assigned IDs.
- The CLI reports only allocation-plan counts and per-collection PathID ranges. No generated Unity object is materialized or serialized by this change.
- An initial full test run exposed a valid inspect fixture with a selected collection that had no recovered hierarchy graph. The allocator was corrected to return an incomplete plan without IDs for that case; it no longer fails inspection or treats the graph as empty.
- Synthetic tests verify independent collection PathID sequences and the generated PrefabInstance-to-PrefabHierarchy link, non-scene missing-Transform default values and ordering, and the no-ID behavior when the processing plan is incomplete.
- cargo test --workspace --locked --quiet: exit 0, 301 library and 10 binary tests passed (0 doc tests). cargo clippy --workspace --all-targets --locked -- -D warnings, cargo fmt --all -- --check, tools/verify-baseline.ps1 (1,449 source files unchanged), and git diff --check: exit 0.
- No user AssetBundle content, UABEA source/executable, original AssetRipper runtime, or Unity-produced input was accessed or run. Phase 1 remains incomplete and Phase 2 remains locked.

## Generated Transform component-list append model (2026-10-02)

- Baseline GameObjectExtensions.AddComponent calls Components.AddNew(), stores the requested ClassID, and sets the component PPtr relative to the GameObject's AssetCollection.
- Added WorkspaceGeneratedComponentAddition to the processed asset allocation plan. It records the owner GameObject, Transform class ID, generated Transform identity, and the append index computed from the complete source component-array relationship range. Null entries count as serialized slots and are retained before the append.
- Added a checked WorkspaceSceneCollectionHierarchy component-slot query that verifies file/object/PathID identity and relationship indexes before returning the source array length. inspect now includes generated component-pair append totals.
- Synthetic coverage uses a pre-existing null m_Component slot and verifies the Transform pair is appended at index 1 and targets the separately allocated Transform asset. The focused case passed; full cargo test --workspace --locked --quiet passed 301 library and 10 binary tests (0 doc tests). Warnings-denied Clippy, format check, baseline verification (1,449 reference files unchanged), and git diff --check passed.
- Reference investigation also confirmed that SceneAssetExportCollection is a Meta/export trigger while SceneExportCollection serializes the SceneHierarchyObject group; PrefabExportCollection selects legacy Prefab asset vs PrefabImporter behavior based on IPrefabMarker. SetPrefabInternal only assigns PrefabInternal_C18P when the prefab implements IPrefabMarker.
- The component-pair record is still a plan. It does not mutate source SerializedValue arrays or resolve the target processed collection's serialized PPtr file index. Scene and Prefab export are not implemented; Phase 1 remains incomplete.

## Generated Transform component-array overlay (2026-10-02)

- Added a checked query for ordered source GameObject component relationships by verified file/object/PathID identity. It returns source relationship order and rejects missing hierarchy indexes or malformed relationship ranges.
- The generated-asset plan now includes a per-GameObject component-list overlay. It preserves each source slot's zero-based index, original PPtr fileID and PathID, and resolved target classification, including null entries; it then records the generated Transform append as a separate slot referencing the allocated processed asset identity.
- The overlay does not compact or rewrite source component slots. Its aggregate bound defaults to 40,000,000 source-plus-generated slots.
- A synthetic fixture contains one original null component entry and verifies its PPtr stays at index 0 while the generated Transform is appended at index 1. Full suite: 301 library and 10 CLI tests passed; warnings-denied Clippy, format, the 1,449-file baseline verifier, and diff checks passed.
- No serialized source array was mutated, and the generated PPtr's ProcessedBundle dependency-table file index is not assigned yet. No user bundle or UABEA data was accessed; Phase 1 remains incomplete.

## Processed collection constructor dependency indexes (2026-10-02)

- The local AssetCollection baseline keeps itself at Dependencies[0]. AddDependency returns the existing collection index when present, otherwise appends the compatible collection; ForceCreatePPtr uses that index and the target PathID. The processed virtual collections begin with only themselves.
- Added ordered processed-collection dependency tables for constructor-assigned generated Transform.GameObject and generated PrefabInstance.RootGameObject PPtrs. First source collection touch gets FileID 1; repeated references reuse that index. The plan retains the target source file index and PathID alongside the computed PPtr.
- Missing Prefab Transforms, Generated Prefabs, and scene-generated collections mark their current generated pointer dependency indexes complete. Prefab Hierarchies are explicitly incomplete because their AssetGroup membership, Root/Prefab, and list references have not yet been reconstructed.
- Synthetic tests verify source-file first-touch order, deduplication, per-collection fileID numbering, target PathIDs, and dependency limits. Full suite: 303 library and 10 CLI tests passed; warnings-denied Clippy, format, 1,449-file baseline verification, and diff checks passed.
- The generated GameObject component overlay's reverse PPtr into the processed collection is not assigned yet; source SerializedAssetCollection dependency tables must retain existing identifiers before appending it. No source arrays or external files were modified. Phase 1 remains incomplete.

## Source collection dependency append requests for generated Transform PPtrs (2026-10-02)

- Baseline audit of `ExportHandler.GetProcessors()` confirms that SceneDefinition, OriginalPath, MainAsset, AnimatorController, AudioMixer, EditorFormat, and LightingData asset processors run before PrefabProcessor. `AssetCollection.AddDependency` reuses the first matching collection entry or appends it, while `ForceCreatePPtr` stores the resulting collection index. A generated Transform component PPtr FileID therefore depends on the source collection's dependency list after all earlier processor effects.
- Added bounded, deduplicated `WorkspaceSourceCollectionDependencyAppend` requests keyed by source file and generated collection. Each request keeps the first owning GameObject and target generated asset; its `file_id` remains `None` until the earlier dependency-list mutations are represented. The inspect summary reports the append request count and that FileIDs are deferred.
- A focused unit test verifies same-source/same-generated-collection deduplication, first-owner retention, and the unset FileID. The hierarchy fixture verifies the request points to the generated Transform in the expected virtual collection.
- `cargo test --workspace --locked --quiet`: exit 0, 304 library and 10 binary tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check`: exit 0.
- No user AssetBundle, UABEA, Unity-produced asset, or original-runtime differential was inspected or run. Source component arrays are still not materialized or mutated, and FileIDs remain deferred; Phase 1 remains incomplete.

## Ordered source dependency FileID assignment (2026-10-02)

- Audited the pre-Prefab asset processors. SceneDefinition writes editor settings into a processed collection; OriginalPath and MainAsset update metadata; AudioMixer builds generated assets. AnimatorControllerProcessor also writes generated StateMachine PPtrs into source AnimatorController layer arrays, and LightingDataProcessor writes generated LightingDataAsset PPtrs into source LightmapSettings. EditorFormat copies already serialized PPtrs within the same source collection, so ForceCreatePPtr reuses an entry needed to resolve the input pointer.
- The workspace scene snapshot records class-157 objects whose complete TypeTree reference scan contains the terminal m_LightingDataAsset PPtr path. LightingData source requests select the first LightmapSettings in source scene/object order and require RenderSettings.
- ObjectWorkspace scanning detects source files containing AnimatorController class ID 91. Their layer-array dependency effects are not decoded yet, so all source dependency FileIDs in those files remain explicitly deferred. For other files, the plan orders LightingData requests before Prefab Transform component PPtrs and assigns FileIDs from self at index 0, original external identifiers, and first-touch appended collections. Missing external counts, overflow, and dependency-list limit errors stay explicit.
- Synthetic tests verify first-LightmapSettings selection, RenderSettings gating, field-name matching, append deduplication, first-owner retention, FileIDs 4 and 5 after three original identifiers when no AnimatorController barrier exists, and the conservative barrier behavior. Full cargo test --workspace --locked --quiet: exit 0, 309 library and 10 binary tests passed. Warnings-denied Clippy, format check, baseline verification (1,449 files unchanged), and git diff --check passed.
- No Unity-produced asset, user AssetBundle, UABEA, or C# runtime differential was used. AnimatorController layer layouts, source dependency table writes, typed GameObject component updates, and generated Prefab serialization/export remain incomplete. Phase 1 remains open.

## AnimatorController source dependency inventory (2026-10-02)

- Replaced the blanket class-91 source-file barrier with a TypeTree inventory for `m_Controller`, current `m_LayerArray` and historical `m_HumanLayerArray`, each layer's `Data.m_StateMachineIndex`, and `m_StateMachineArray` bounds. A source SerializedFile needs one generated AnimatorController dependency append if at least one valid controller has a non-empty layer array; self-controller and parameter PPtrs reuse FileID 0.
- The inventory marks a source file deferred if any AnimatorController payload or relevant field is undecoded, ambiguous, malformed, or contains an out-of-range state-machine index. Known AnimatorController appends are included before LightingData and Prefab when assigning later source dependency FileIDs. Only pending requests in an uncertain file are deferred.
- Synthetic tests cover the historical/current field aliases, empty/non-empty layer arrays, valid and invalid indexes, ambiguous/mistyped fields, deferred requests, and ordered FileIDs. With three original external identifiers, the modeled AnimatorController append takes FileID 4, then LightingData and Prefab requests take FileIDs 5 and 6. Without a layer append they remain 4 and 5.
- `cargo test --workspace --locked --quiet`: exit 0, 312 library and 10 CLI tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check`: exit 0.
- No user AssetBundle, UABEA, Unity-produced asset, or original-runtime differential was used. The inventory does not yet serialize AnimatorController layer PPtrs or mutate serialized source dependency tables. Typed GameObject component arrays and generated Prefab serialization/export remain incomplete; Phase 1 remains open.

## First-class AnimatorController source dependency append (2026-10-02)

- Each known source-file append now retains its first non-empty controller's first layer slot and the symbolic StateMachine target `(source controller file/object/PathID, StateMachineArray index)`. The AnimatorController request is a typed ordered source-dependency operation and is inserted before LightingData and Prefab requests.
- The generated StateMachine's output PathID is still unknown because the full AnimatorController generated collection and its state/motion allocation sequence are not modeled. The request therefore carries a symbolic target only; it does not claim a serializable PPtr yet.
- Synthetic tests verify direct and `Data`-wrapped layer values, both historical/current LayerArray names, target identity for layer 0, array bounds, and source append ordering. With three external identifiers, the AnimatorController, LightingData, and Prefab dependency requests receive FileIDs 4, 5, and 6.
- `cargo test --workspace --locked --quiet`: exit 0, 312 library and 10 CLI tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo fmt --all -- --check` pass; `tools/verify-baseline.ps1` confirms 1,449 reference files unchanged; `git diff --check` passes.
- No user AssetBundle, UABEA, or Unity-produced input was used. The generated StateMachine PathID, source PPtr serialization, source dependency table mutation, typed GameObject component writeback, and generated Prefab serialization/export remain incomplete; Phase 1 remains open.

## Bounded SerializedValue tree mutation (2026-10-02)

- Added a unique mutable field lookup through nested `Base` structs. It rejects duplicate direct names, duplicate matches across the inheritance chain, and malformed mutation paths; field-path allocation is fallible.
- Added bounded, fallible array append and mutable array-slice access. A synthetic component array keeps its null and existing cross-file PPtr slots at indexes 0 and 1, appends a new PPtr at index 2, rejects a fourth element at the configured limit without changing the array, and rejects duplicate inherited field names.
- `cargo test --workspace --locked --quiet`: exit 0, 314 library and 10 CLI tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check`: exit 0.
- This is only an in-memory typed-value edit. It does not encode TypeTree object bytes, adjust object sizes or metadata, or export generated components. No user AssetBundle, UABEA, or Unity-produced input was used; Phase 1 remains open.

## TypeTree-backed object payload encoder (2026-10-03)

- Added bounded TypeTree-backed `SerializedFileMetadata::encode_object_payload`. It preserves field order, endian-aware primitive values and PPtrs, string/array framing, versioned primitive-array alignment, `AlignBytes`, fixed-size raw leaves, and retained trailing bytes. It rejects mismatched paths/types, malformed shapes, managed-reference payloads, zero-width nonempty arrays, and configured output/array/string/depth limit violations with object and field context.
- Exact synthetic fixtures cover 32- and 64-bit PPtr PathIDs, little- and big-endian values, strings, aligned arrays, raw leaves, trailing bytes, mutation followed by semantic decode, array shrink under a tighter output cap, and bounded errors. Scalar writes use a stack buffer; output growth is checked and capped.
- `cargo test --workspace --locked --quiet`: exit 0; 320 library and 10 CLI tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check`: exit 0.
- This API encodes one object payload only; it does not update enclosing SerializedFile object sizes, offsets, metadata, or headers. Managed-reference payloads return an explicit unsupported error. No original-runtime differential, UABEA, or user AssetBundle content was used; Phase 1 remains incomplete.
## Complete SerializedFile payload replacement (2026-10-03)

- Added a bounded complete-file rewrite API for replacing existing object payloads. The metadata parser records the source object-record offset and size field locations only for the complete-file rewrite path; the regular metadata-only parser avoids allocating this sidecar. Rewrites preserve all raw metadata bytes outside those fixed-width fields and preserve unmodified object payload bytes.
- The writer rebuilds the data section in object-table order, aligns its start to 16 bytes and each object end to 8 bytes, updates generation-specific object offsets/sizes and header fields, and supports metadata-at-end files before generation 9 plus the generation-22 extended header. Duplicate, mismatched, out-of-range, unrepresentable, malformed, and over-limit writes are explicit errors. An empty replacement list returns an exact bounded copy.
- Synthetic fixtures verify generation-8 metadata-at-end relocation, generation-10 replacement growth moving a second object from offset 16 to 24, header/file-size updates, semantic reparsing, output limits, and generation-22 64-bit object-record/header encoding. `cargo test --workspace --locked --quiet`: exit 0; 323 library and 10 CLI tests passed (0 doc tests). `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check`: exit 0.
- This replaces payloads only. It does not insert/remove object records or alter type, script, external, reference-type, or dependency tables. No original-runtime differential, UABEA, or user AssetBundle content was used; Phase 1 remains incomplete.
## Object-record table layout provenance (2026-10-03)

- Extended the complete-file parser sidecar with the serialized object-count field position and the exact byte interval occupied by object records, alongside each PathID and payload offset/size field position. The rewrite path validates the count, record interval, and every patch target before changing bytes. The metadata-only parser does not allocate rewrite sidecars.
- Synthetic assertions cover the generation-10 count and interval. Existing generation-8 and generation-10 rewrite fixtures reparse after patching. `cargo test --workspace --locked --quiet`: exit 0; 323 library and 10 CLI tests passed (0 doc tests). Warnings-denied Clippy, format check, 1,449-file baseline verification, and diff check passed.
- This captures only the existing object table boundary. Insertion/removal and all other metadata tables remain unsupported. No original-runtime differential, UABEA, or user AssetBundle content was used; Phase 1 remains incomplete.

## Bounded object-record insertion with existing types (2026-10-03)

- Added `SerializedFile::rewrite_with_insertions` and `SerializedObjectInsertion`. New typed payloads are encoded through an existing serialized TypeTree entry; the writer appends generation-specific object records after the current table, increments its count, preserves following metadata bytes, appends payloads after original objects, and patches every record plus the file header. Existing `rewrite_object_payloads` remains the no-insertion entry point.
- The writer checks total object/insertion/output bounds, legacy PathID width, duplicate PathIDs, type-table identity, generation-dependent field presence, object payload sizes, and data-offset widths. It keeps the metadata-only parse path free of rewrite sidecar allocations.
- Synthetic end-to-end fixtures insert and decode objects in generation 8 (metadata at file end) and generation 10, including payload trailing bytes and byte-for-byte preservation of metadata after the previous object table. Record serialization checks cover generations 14, 15, 17, and 22 with big-endian values and 32/64-bit data offsets. Error cases cover duplicate PathID, unrepresentable legacy PathID, type mismatch, and insertion limit.
- Full workspace verification passed: `cargo test --workspace --locked --quiet` (327 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 local reference files unchanged); and `git diff --check` (all exit 0).
- This path appends records only and can only refer to an existing serialized type. It does not update type, script, external-file, reference-type, or dependency tables. It does not encode managed-reference payloads or complete the Unity project export path. No UABEA, user AssetBundle content, or remote third-party repository was accessed; Phase 1 remains incomplete.

## Planned GameObject component-list payload materialization (2026-10-03)

- Added `materialize_game_object_component_append` to turn one source-slot overlay into an edited typed GameObject payload. It verifies file/object/PathID identity, exact source array length and order, and each original slot's raw fileID/PathID; then it appends a Transform PPtr while retaining every original value and trailing payload byte.
- Added `materialize_planned_game_object_component_append` to use only the FileID assigned by a complete processed-asset plan. Direct PPtr array elements and one-field `ComponentPair.component` elements are supported. Empty arrays require an explicit TypeTree-derived element template; unsupported shapes, changed source values, deferred FileIDs, and element-limit violations return errors.
- This increment yields a payload replacement input but does not write the corresponding source external/dependency metadata or generate the Transform payload. It cannot be treated as a complete SerializedFile or Unity project export.
- Verification passed: `cargo test --workspace --locked --quiet` (330 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 local reference files unchanged); and `git diff --check` (all exit 0).
- Synthetic payloads only; no Unity runtime differential, UABEA, user AssetBundle, or remote third-party repository was accessed. Phase 1 remains incomplete and Phase 2 remains locked.

## Bounded external-file table append (2026-10-03)

- Added rewrite-only parsing sidecars for the external-file count and record interval; metadata-only parsing still avoids rewrite sidecar allocations. Added `SerializedExternalFileInsertion` and `SerializedFile::rewrite_with_changes`, which can replace payloads, append typed objects, and append external identifiers in one metadata reconstruction.
- External records follow generation-specific field presence: `asset_path` from generation 6, GUID plus asset type from generation 5, and path-name origin in every generation. Strings reject embedded NUL bytes; field-presence, insertion-count, signed table-count, and output-size violations are explicit errors. Existing external records, script records, reference types, user information, and trailing metadata are retained, with the appropriate table count fields patched in body endianness.
- Generation-10 end-to-end fixtures verify external-only append and combined object/external append by reparsing, including following user information. Big-endian record fixtures cover generations 14, 15, 17, and 22. Error fixtures cover count limits, generation-field mismatch, and embedded NUL.
- Full workspace verification passed: `cargo test --workspace --locked --quiet` (332 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 local reference files unchanged); and `git diff --check` (all exit 0).
- The caller still supplies the path and GUID for each external identifier. The writer does not generate processed collection identities, change type/reference/dependency tables, or build the generated Transform payload. No UABEA, user AssetBundle content, or remote third-party repository was accessed; Phase 1 remains incomplete and Phase 2 remains locked.

## Bounded TypeTree default-value construction (2026-10-03)

- Added `SerializedFileMetadata::default_object_value_for_type` for an existing TypeTree. It constructs zero-valued primitives, null PPtrs, empty strings/arrays, and zero-filled fixed-size unknown leaves; callers must assign class-specific nonzero defaults. Managed-reference records and malformed array/string/PPtr shapes fail explicitly.
- Added separate limits for total TypeTree nodes, materialized values, aggregate value allocations, and recursion depth. The generic builder is exercised using a synthetic Transform-shaped TypeTree and the existing typed payload encoder.
- Verification passed: `cargo test --workspace --locked --quiet` (335 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 local reference files unchanged); and `git diff --check` (all exit 0).
- This does not apply Transform's identity quaternion/unit scale, assign its GameObject PPtr, allocate a new serialized type, or complete Prefab/Unity project export. Synthetic TypeTree evidence only; Phase 1 remains incomplete and Phase 2 remains locked.

## Generated Transform typed-payload materialization (2026-10-03)

- Added `materialize_planned_generated_transform_payload`. It requires a complete processed-asset plan, a unique planned GeneratedTransform asset, its `TransformGameObject` constructor pointer, and a dependency-table entry that maps the assigned FileID back to the target source SerializedFile. It also requires an existing class-ID-4 serialized type with a TypeTree.
- The constructor fills `m_LocalPosition` with zero, `m_LocalRotation` with identity, `m_LocalScale` with one, `m_RootOrder` with zero, the optional `m_LocalEulerAnglesHint` with zero, and `m_GameObject` with the planned source PPtr. Other fields retain the generic schema defaults: null PPtrs, empty arrays/strings, zero primitives, and zero-filled fixed-size unknown leaves. Inherited fields are located through Base structs.
- Synthetic tests verify the full encoded byte sequence, father/children generic defaults, optional Euler omission, required-field errors, class mismatch, and dependency-target mismatch. Full workspace verification passed: `cargo test --workspace --locked --quiet` (338 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- The result is a typed root plus encoded bytes that verify the selected TypeTree schema. The baseline stores processed assets in virtual collections and exports them through Scene/Prefab Unity YAML; this helper is not connected to that output path. No source-runtime differential was run; Phase 1 remains incomplete and Phase 2 remains locked.

## Unity project export ID rules (2026-10-03)

- Local baseline inspection confirms `ProcessedAssetCollection.CreateAsset` creates an in-memory `AssetInfo` with a collection-local increasing PathID. `SceneExportCollection` and `PrefabExportCollection` then assign export-file IDs and the project YAML writer maps PPtrs through the selected export collection and its GUID. The processed collection itself is not a SerializedFile output target.
- Added `src/export/ids.rs` for the baseline `ExportIdHandler`: class-prefixed main IDs; xxHash32 pseudo IDs before Unity 5.5; xxHash64 pseudo IDs from 5.5; class-specific prefix thresholds; and seed order as the export collection enumerates assets. Invalid modifier cases return explicit errors.
- Verification covers standard empty-input xxHash32/64 vectors, modifiers, large-class behavior, 32/64-bit version and class thresholds, and the source test's million-seed collision probes. Full workspace verification passed: `cargo test --workspace --locked --quiet` (342 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- Project asset grouping, PPtr-to-export-collection resolution, Unity YAML formatting, and `.meta` GUID assignment remain unimplemented. No original AssetBundles or UABEA were accessed; Phase 1 remains incomplete and Phase 2 remains locked.

## Scene YAML collection export ID assignment (2026-10-03)

- Added `plan_scene_export_ids` for one already ordered Scene export asset list. Serialized-file assets keep their source PathID. Processed-collection assets use class-prefixed pseudo-random IDs, with the 32-bit seed advancing only for generated/processed assets as in `SceneExportCollection`.
- The plan retains full collection/object identity, keeps caller order, rejects duplicate identities and collection-size overruns, and bounds the source-compatible 32-bit seed.
- Synthetic tests cover mixed source/generated entries, same PathID values in different source files, generated-only seed order, Unity 5.4 and 2022 ID widths, duplicates, and limits. Full workspace verification passed: `cargo test --workspace --locked --quiet` (345 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- SceneHierarchyObject membership/order construction, PPtr export pointer resolution, Prefab collection rules, YAML document output, and GUID assignment remain open. Phase 1 remains incomplete and Phase 2 remains locked.

## Scene hierarchy export membership order (2026-10-03)

- Added `order_scene_hierarchy_assets` from the baseline `SceneHierarchyObject.Assets` sequence: GameObjects, Components, PrefabInstances, Managers, then optional SceneRoots. The API takes already classified identities and preserves order within each category; it deliberately does not apply the separate YAML export comparator or hidden-asset filter.
- Synthetic tests cover exact category concatenation, absent SceneRoots, and asset-count bounds. Full workspace verification passed: `cargo test --workspace --locked --quiet` (347 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- Classification of scene source objects into these groups, PPtr-to-export-pointer resolution, Prefab export IDs, YAML writing, and GUID assignment remain open. No original AssetBundles or UABEA were accessed; Phase 1 remains incomplete and Phase 2 remains locked.

## Project export PPtr mapping (2026-10-03)

- Added a bounded `ProjectExportPointerMap` keyed by collection-scoped object-table identity. It emits distinct null, local, external, and missing-export results; cross-collection pointers retain collection GUID and AssetType, while local pointers use the baseline's GUID-free serialized form.
- Input PPtrs that failed or ambiguously resolved remain `UnresolvedInput` with source file/object/PathID, field path, original serialized FileID/PathID, class ID, and the structured `MultiFilePPtrResolution`. They are rejected if mislabeled as null or uniquely resolved. Missing export membership uses the source main export ID and the missing-reference sentinel rather than becoming null.
- Synthetic coverage includes same PathID in distinct source files, pointer scope/type, unresolved dependency evidence, and duplicate or unregistered identities. Full workspace verification passed: `cargo test --workspace --locked --quiet` (351 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- The map is not yet populated from real Scene/Prefab membership and is not connected to Unity YAML serialization. No original AssetBundles or UABEA were accessed; Phase 1 remains incomplete and Phase 2 remains locked.

## Prefab collection membership and IDs (2026-10-03)

- Added `plan_prefab_export_ids` for the baseline Prefab collection's `GameObjects`, `Components`, `PrefabInstances`, appended Prefab asset, and final hierarchy-helper registration order. The main Prefab uses `GetMainExportID`; additional unique objects use contiguous generated seeds and each asset's own Unity version. The helper is registered for collection lookup but excluded from YAML exportable members.
- Visibility and source-pointer AssetType follow the baseline marker representation: legacy Prefab markers are emitted and use `Serialized`; modern PrefabInstance markers are hidden and use `Meta`; other implementations use a PrefabImporter and are not hidden by the hierarchy constructor. Conflicting identity/version metadata and zero collection GUID are explicit errors.
- Synthetic tests cover add order, per-asset version widths (5.4 and 2022), main asset exclusion, hidden filtering, source pointer IDs/types, duplicate handling, conflicting versions, and limits. Full workspace verification passed: `cargo test --workspace --locked --quiet` (355 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- Real source-object classification, collection discovery, complete importer fields, YAML output, and `.meta` writing remain open. No original AssetBundles or UABEA were accessed; Phase 1 remains incomplete and Phase 2 remains locked.

## Scene/Prefab registration in a shared pointer map (2026-10-03)

- Added `ProjectExportPointerMapBuilder` adapters for `SceneExportIdAssignment` and `PrefabExportIdPlan` assignments. They register collection GUID and AssetType together with each assigned fileID, then build the shared scope-aware PPtr map.
- Scene registrations use `Serialized`; Prefab registrations accept the exporter-selected AssetType. The builder validates nonzero Scene/Prefab GUIDs, applies collection/asset limits before appending, and fallibly reserves capacity. Duplicate collection indexes and asset identities are rejected by the final map build.
- Synthetic evidence registers one Scene and one Prefab collection, then resolves a cross-collection Scene pointer and local Prefab pointer; zero GUID and collection-limit failures are also covered. Full workspace verification passed: `cargo test --workspace --locked --quiet` (357 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- Actual collection discovery and other export collection families remain unwired. No original AssetBundles or UABEA were accessed; Phase 1 remains incomplete and Phase 2 remains locked.

## Unity YAML documents and project PPtr nodes (2026-10-03)

- Added an ordered Unity YAML node tree and bounded stream writer. It writes the source YAML 1.1 and `!u!` directives, document separators, class tags, anchors, stripped markers, block/flow mappings and sequences, scalar quoting, and baseline bool-as-integer output. Documents are separated without sorting fields or children.
- Added `meta_pointer_to_unity_yaml` and `project_pointer_to_unity_yaml`. They emit `fileID` plus optional `guid`/`type`, implement Unity <4 `Meta` to `Cached` fallback, require the actual baseline MissingReference GUID instead of inventing one, and reject unresolved input PPtrs while retaining source file/object/field and original serialized pair context.
- Synthetic evidence checks complete object streams, multiple documents, nested block sequence formatting, flow pointers, GUID word rendering, Unity 3 type fallback, null pointer output, unresolved-reference rejection, and document/node/depth/indent/output limits. Full workspace verification passed: `cargo test --workspace --locked --quiet` (363 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; baseline verifier (1,449 files); and `git diff --check`.
- Actual generated Unity object walkers and all importer/meta file paths remain unconnected. MissingReference GUID still requires a baseline-derived value at call time. No original AssetBundles or UABEA were accessed; Phase 1 remains incomplete and Phase 2 remains locked.

## Bounded SerializedValue to Unity YAML conversion (2026-10-03)

- Added serialized_object_value_to_unity_yaml_document with bounded value count, nesting, field paths and generated scalars. It retains field order, flattens Base, writes serializedVersion, and routes non-null PPtrs through the caller's collection-aware resolver; PathID 0 becomes null without resolver access.
- Integer arrays use the baseline reverse-endian hexadecimal representation. Empty arrays require caller-provided TypeTree element hints. Raw leaves, invalid UTF-8, unresolved pointers, and over-limit data return contextual errors rather than silently disappearing.
- Targeted tests passed: cargo test --lib export::value_yaml::tests --locked (3 passed). Full workspace passed: cargo test --workspace --locked --quiet (366 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; cargo fmt --all -- --check; tools/verify-baseline.ps1 (1,449 files); and git diff --check.
- No user AssetBundle content or UABEA was accessed. This generic conversion does not implement generated per-class YAML policies, special Hash128/GUID serialization, importer filtering, or full export wiring. Phase 1 remains incomplete; Phase 2 remains locked.
## TypeTree-derived Unity YAML representation hints (2026-10-03)

- Added UnityYamlObjectConversionHints::from_type_tree. It validates TypeTree levels and builds bounded parent/sibling indexes, maps TransferUsingFlowMappingStyle (0x00200000) onto root and nested struct paths, and uses the recognized Array/vector/staticvector/set/map TypeTree shapes to determine empty-array output.
- Integral, boolean, and character element types select the baseline reverse-endian hex scalar; other arrays select block sequences. Struct flow flags inside arrays use wildcard paths that match each concrete element index. Malformed hierarchy and ambiguous array paths return explicit errors.
- Focused converter tests passed: cargo test --lib export::value_yaml::tests --locked (5 passed). Full workspace passed: cargo test --workspace --locked --quiet (368 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; cargo fmt --all -- --check; tools/verify-baseline.ps1 (1,449 files); and git diff --check.
- No user AssetBundle content or UABEA was accessed. Generated per-class editor walks, dictionary key serialization, Hash128/GUID conversion, importer filtering, and complete project-file output remain incomplete. Phase 1 remains open; Phase 2 remains locked.
- Follow-up failure record: one intermediate full-suite run failed only in the new node-limit assertion because it expected 12 nodes while passing the separate 2-node root-flow fixture. Corrected the expected fixture count to 2; the subsequent full workspace run passed all 368 library tests and 10 CLI tests.
## Hash128 versioned Unity YAML serialization (2026-10-03)

- TypeTree hint construction now recognizes Hash128 nodes, validates the 16 uniquely named UInt8 leaf fields, and retains their source names and derives serializedVersion from the Unity version because the input TypeTree stays stale after Unity 5. Conversion checks all 16 decoded values and rejects missing, duplicated, signed, or non-byte data.
- Matched YamlWalker.HashHelper: version 1 emits serializedVersion and bytes[0] through bytes[15]; version 2 emits serializedVersion and Hash, formed from four little-endian 32-bit words as lowercase eight-digit hex segments. Synthetic bytes 0..15 verify the exact modern string 03020100070605040b0a09080f0e0d0c and both output layouts.
- Focused converter tests passed: cargo test --lib export::value_yaml::tests --locked (6 passed). Full workspace passed: cargo test --workspace --locked --quiet (369 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; cargo fmt --all -- --check; tools/verify-baseline.ps1 (1,449 files); and git diff --check.
- Follow-up failure record: an intermediate old-version assertion searched for the substring Hash:, which also occurs at the end of the enclosing field name m_Hash:. The assertion was narrowed to the complete modern hash payload; the focused and full suites then passed.
- No user AssetBundle content or UABEA was accessed. This TypeTree-driven nested conversion is not yet wired into a complete class-specific editor walker or project exporter. GUID conversion and other per-class rules remain open; Phase 1 remains incomplete and Phase 2 remains locked.
- Version-boundary audit: baseline Pass008 changes generated Hash128 serializedVersion to 2 starting at Unity 5 because the TypeTree itself was not updated. Hint generation therefore uses the export Unity version, not the stale TypeTree node version. Tests assert Unity 4.7 -> 1 and Unity 5.0 and 2022 -> 2 from the same version-1 TypeTree.
## TypeTree GUID to Unity YAML scalar conversion (2026-10-03)

- TypeTree hint construction now recognizes GUID and validates the four named UInt32 leaves m_Data_0_ through m_Data_3_. Conversion finds those fields by source name, requires all four decoded values to remain U32, writes each word as four little-endian bytes, and delegates canonical Unity GUID text formatting to the existing formatter used by project pointers.
- Synthetic values verify 01020304111213142122232431323334, explicit errors for missing words and non-U32 values, and GUID lookup through an array-element wildcard path. GUID-specific tests passed: cargo test --lib export::value_yaml::tests --locked (8 module tests total).
- Full workspace passed: cargo test --workspace --locked --quiet (371 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; cargo fmt --all -- --check; tools/verify-baseline.ps1 (1,449 files); and git diff --check.
- No user AssetBundle content or UABEA was accessed. Pass502 calls UnityGuid.ToString, but no direct execution differential against that helper was run; GUID conversion remains a nested generic TypeTree feature and the generated class walker/project writer are still incomplete. Phase 1 remains open; Phase 2 remains locked.

## SerializedFile workspace object to Unity YAML bridge (2026-10-03)

- Added `workspace_object_to_unity_yaml_document` to connect a workspace-decoded object to the generic YAML converter using its resolved TypeTree, source Unity version, source object identity, cross-file reference scan, and project export pointer map.
- The bridge rejects identity mismatches, incomplete reference scans, unknown class/TypeTree/version state, ambiguous or missing graph edges, and unparsed trailing payload bytes. Resolved targets use file-scoped object identity before project-map lookup; unresolved non-null PPtrs remain conversion errors. `UnresolvedProjectPointer.expected_class_id` is optional so unknown type metadata is recorded as unknown rather than guessed.
- Three generated SerializedFile integration tests cover a GameObject `m_Component` array resolving to a Transform and the Transform's back-reference, mismatched graph/payload evidence, trailing bytes, and missing local targets. These use a manually framed generation-10 SerializedFile fixture; no AssetBundle or C# runtime differential was used.
- Verification passed: `cargo test --workspace --locked --quiet` (374 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 local reference files unchanged); and `git diff --check` (all exit 0).
- No user AssetBundle content, UABEA, or other remote repository was accessed. Full class-specific YAML walking, collection discovery, importer filtering, and product-level project export remain incomplete. Phase 1 remains open and Phase 2 remains locked.

## Ordered Scene/Prefab workspace YAML execution (2026-10-03)

- Added `ObjectWorkspace::get(file_index, object_index)` using the workspace's stable file/object-table ordering, plus `workspace_assignments_to_unity_yaml` for Scene or Prefab assignments. It preserves caller-supplied YAML document order, resolves each source object through the workspace bridge, and produces one bounded multi-document Unity YAML stream.
- The collection executor rejects duplicate collection-scoped object identities, duplicate document fileIDs, document-count overruns, missing workspace objects, and processed assets that do not yet have a generated YAML walker. It does not skip or substitute unsupported entries.
- Five focused tests cover the GameObject/Transform reference cycle, the caller-selected document order, explicit processed-asset failure, stale reference edges, trailing bytes, and a missing local target. Full verification passed: `cargo test --workspace --locked --quiet` (376 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 local reference files unchanged); and `git diff --check` (all exit 0).
- No user AssetBundle content, UABEA, or other remote repository was accessed. Scene/Prefab membership discovery and baseline sorting, generated-object YAML walkers, and product-level output routing remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## Baseline Scene YAML asset ordering (2026-10-03)

- Source inspection of `SceneExportCollection.Compare` shows that LevelGameManager assets sort before other assets and manager pairs sort by ascending class ID. Pairs with equal class IDs compare equal; LINQ's stable `Order` preserves their existing hierarchy order. The recognized manager classes are OcclusionCullingSettings (29), RenderSettings (104), LightmapSettings (157), and NavMeshSettings (196).
- Added `order_scene_yaml_assignments` and `workspace_scene_assignments_to_unity_yaml`. The latter takes assignments already produced in SceneHierarchyObject hierarchy/exportable order, applies the stable source comparator, then executes the existing bounded object-to-YAML workspace bridge.
- Synthetic tests verify manager-first sorting, ascending manager IDs, stable same-class/non-manager ordering, limits, and the Scene execution route. Full verification passed: `cargo test --workspace --locked --quiet` (379 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format; baseline verifier (1,449 files); and `git diff --check`.
- The local C# comparator was source-inspected but not executed for a runtime differential. Automatic hierarchy membership discovery, processed-asset YAML, and project-level routing remain incomplete. No user AssetBundle content, UABEA, or other remote repository was accessed. Phase 1 remains open; Phase 2 remains locked.

## Scene hierarchy membership to export-ID planning (2026-10-03)

- Read the local baseline `SceneHierarchyObject.Create`, `SceneHierarchyObject.Assets`, `GameObjectHierarchyObject.Assets`, and `MonoBehaviourExtensions.IsComponentOnGameObject`. The source walks `SceneDefinition.Assets` in order, concatenates the category lists as GameObjects, Components, PrefabInstances, Managers, then appends the last encountered SceneRoots. MonoBehaviour membership specifically requires a non-null GameObject pointer.
- Added `WorkspaceSceneHierarchyScene::asset_membership` to construct ordered category groups from the scene's source-ordered collection graphs. It orders cross-file component candidates by their target collection and object-table position, deduplicates by file/object identity while detecting conflicting PathID/class metadata, and retains separate component-evidence kinds so MonoBehaviour filtering does not lose its GameObject-pointer proof.
- Transform class IDs and decoded GameObject `m_Component` references form direct component evidence. Class ID 114 requires the baseline non-null `m_GameObject` rule; explicit null excludes the MonoBehaviour, while missing evidence remains unresolved. Other `m_GameObject`-only candidates, incomplete scans, missing class IDs, out-of-scene component targets, and conflicting identities remain explicit membership issues.
- Added `plan_workspace_scene_export_ids`, which converts complete source membership to the baseline category order and then assigns Scene fileIDs. It refuses to plan IDs if any membership issue remains. YAML manager ordering remains a separate subsequent step.
- Synthetic tests cover multi-collection/category order, cross-file component ordering, last-SceneRoots selection, MonoBehaviour evidence requirements, preservation of distinct candidate evidence, scan/completeness and asset limits, export-ID order, and the separate YAML ordering.
- Full verification passed: `cargo test --workspace --locked --quiet` (384 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 reference files unchanged); and `git diff --check` (all exit 0).
- No baseline C# runtime differential was run. Hidden-asset filtering, generated processed assets, full component interface classification, Prefab membership/IDs, complete YAML walkers/writing, and product-level project export remain incomplete. No user AssetBundle content or UABEA was accessed; Phase 1 remains open and Phase 2 remains locked.

## TypeTree-derived component membership evidence (2026-10-03)

- Read the local baseline's AssemblyDumper `Pass010_InitializeInterfacesAndFactories`, `Pass011_ApplyInheritance`, and `SceneHierarchyObject.Create`. The generated class inheritance follows serialized class Base names, while scene membership includes objects implementing `IComponent`.
- Added resolved per-object TypeTree lookup to `MultiFileReferenceGraph::object_type_tree`. Scene candidate classification now follows direct nested `Base` TypeTree nodes and treats a proven `Component` ancestor as component evidence. Duplicate `Base` branches or a `Base` node without a type name are explicit ambiguous candidates; absent Base nodes mean this classifier has no component evidence, and absent TypeTrees remain unavailable. The class 114 MonoBehaviour rule still uses its separate non-null `m_GameObject` evidence.
- Focused TypeTree ancestry, ambiguous/missing metadata, reference-graph lookup, and Scene ID planner tests passed. Full verification passed: `cargo test --workspace --locked --quiet` (386 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- No C# runtime differential was run. TypeTrees without a definitive Base chain and other pointer-only candidates remain unresolved; generated processed assets, Prefab export, and complete project YAML writing remain incomplete. No user AssetBundle content, UABEA, or other remote repository was accessed. Phase 1 remains open; Phase 2 remains locked.

## Generated SerializedFile component membership to Scene IDs (2026-10-03)

- Added an independently assembled generation-10 SerializedFile fixture for a class-ID-23 MeshRenderer. Its TypeTree has the `MeshRenderer -> Base: Renderer -> Base: Component` ancestry and a decoded null `m_GameObject` pointer.
- The fixture parses through `SerializedFile`, completes `ObjectReferenceGraph` and `MultiFileReferenceGraph` scans, is classified by `WorkspaceSceneHierarchyBuilder`, appears as a complete Scene component member despite the null back-pointer, and receives its original PathID 9001 from `plan_workspace_scene_export_ids`.
- The fixture first exposed a malformed empty TypeTree leaf; the decoder correctly rejected it. The fixture was corrected to contain a typed, null GameObject PPtr, after which the focused end-to-end test passed.
- Focused test passed: `cargo test --locked --lib generated_serialized_file_type_tree_component_flows_into_scene_ids -- --nocapture`. Full workspace verification passed: `cargo test --workspace --locked --quiet` (387 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This generated fixture does not establish equivalence with an original Unity-produced file or a C# runtime differential. Scene YAML writing, other component-class paths, and the Phase 1 product-completeness gate remain open. No user AssetBundle content, UABEA, or other remote repository was accessed; Phase 2 remains locked.

## Scene membership IDs to workspace YAML (2026-10-03)

- Added a generated generation-10 MeshRenderer SerializedFile to the workspace YAML fixtures. The test loads it through the virtual filesystem, SerializedFileWorkspace, ObjectWorkspace, and complete MultiFileReferenceGraph.
- The fixture runs Scene hierarchy classification and ID planning, registers the derived assignment in `ProjectExportPointerMapBuilder`, and executes `workspace_scene_assignments_to_unity_yaml`. The emitted document uses class tag 23 and fileID 9001; the explicit null `m_GameObject` PPtr remains `{fileID: 0}`.
- Focused test passed: `cargo test --locked --lib scene_component_membership_ids_flow_through_workspace_yaml_export -- --nocapture`. Full workspace verification passed: `cargo test --workspace --locked --quiet` (388 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This fixture has only one component with a true-null back-pointer; it does not test mapped non-null PPtrs in the same Scene-ID-to-YAML pipeline or compare against Unity runtime output. Processed-asset walkers, Prefab output, complete project routing, and Phase 1 remain incomplete. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## Reciprocal GameObject and MeshRenderer PPtrs through Scene YAML (2026-10-03)

- Added a generated generation-10 SerializedFile with two object records: GameObject PathID 42 and MeshRenderer PathID 9001. The GameObject `m_Component` pointer targets the MeshRenderer, and the MeshRenderer TypeTree `Base.Base.m_GameObject` pointer targets the GameObject.
- Added `MultiFileReferenceGraph::file_object_count` and used it in `SceneGraph::resolve_with_multi_file_graph` to validate the complete source object-table count before joining scene relationships. The two-object test passes through complete object decoding/reference scans, Scene hierarchy construction, component membership, PathID-preserving ID planning, project pointer-map registration, and `workspace_scene_assignments_to_unity_yaml`.
- Both local PPtrs resolve uniquely and serialize to the other object's planned local fileID: GameObject `m_Component` -> 9001 and MeshRenderer `m_GameObject` -> 42. Focused workspace YAML tests passed (8); full verification passed: `cargo test --workspace --locked --quiet` (389 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This verifies reciprocal local references only, with no original Unity-produced fixture or runtime differential. Cross-collection Scene YAML, processed assets, Prefab output, complete project routing, and Phase 1 remain incomplete. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## Cross-collection Scene PPtr GUID/type emission (2026-10-03)

- Extended the generated GameObject/MeshRenderer fixture to split the two ID assignments across separate registered Scene export collections, each with its own GUID.
- `workspace_assignments_to_unity_yaml` writes the GameObject component PPtr with the MeshRenderer collection GUID and the MeshRenderer back-pointer with the GameObject collection GUID. Both mappings include Serialized asset type 2 and retain their respective planned fileIDs (9001 and 42).
- Focused test passed: `cargo test --locked --lib scene_component_pointers_to_other_collections_emit_guid_and_type -- --nocapture`. Full workspace verification passed: `cargo test --workspace --locked --quiet` (390 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Source PPtrs in this fixture are local to one SerializedFile; only their assigned output collections differ. Actual cross-file input references through the workspace YAML executor, original Unity runtime comparison, processed asset walkers, Prefab output, and Phase 1 remain open. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## Cross-file Scene PPtrs through YAML export (2026-10-03)

- Added a generated two-file generation-10 workspace with reciprocal GameObject/MeshRenderer PPtrs targeting objects in the other SerializedFile. Both files are loaded and scanned together; the test checks complete object and multi-file reference workspaces, cross-file target identities, Scene membership, and PathID-preserving assignments 42 and 9001.
- Registered the assignments in separate Scene output collections and verified the YAML documents preserve both cross-file relationships with the destination collection GUID, Serialized asset type 2, and corresponding fileID.
- Focused test passed: `cargo test --locked --lib cross_file_component_pptrs_reach_separate_scene_yaml_collections -- --nocapture`. Full workspace verification passed: `cargo test --workspace --locked --quiet` (391 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This is generated-fixture evidence, not a Unity-produced-file or runtime differential. Processed-asset YAML, Prefab project output, complete project routing, and Phase 1 remain open. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## Importer YAML field filtering (2026-10-03)

- Inspected the baseline `YamlWalker.EnterField`, its seven fixed importer exclusions, `Pass102_IgnoreFieldInMetaFilesMethods`, and `TransferMetaFlags.IgnoreInMetaFiles` (`0x00080000`). Added an explicit `UnityYamlObjectFieldMode` and TypeTree-derived importer exclusion paths, including wildcard resolution for array elements. The workspace Scene/Prefab executor explicitly stays in ProjectAsset mode.
- A generated TypeTree/value fixture verifies `m_ObjectHideFlags` and flagged fields are omitted in AssetImporter mode, including a flagged field inside an array element; the same values remain in ProjectAsset mode.
- Focused test passed: `cargo test --locked --lib importer_yaml_omits_baseline_and_type_tree_ignored_fields -- --nocapture`. Full workspace verification passed: 392 library tests, 10 CLI tests, 0 doc tests; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This validates the generic field filter with synthetic data only. Importer discovery, `.meta` routing, stripped-field selection, full project importer output, and any Unity runtime differential remain open. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## Stripped-object YAML field allowlists (2026-10-03)

- Inspected baseline `StrippedAssetExtensions`: ordinary stripped assets retain `m_CorrespondingSourceObject`, `m_PrefabAsset`, `m_PrefabInstance`, `m_PrefabInternal`, and `m_PrefabParentObject`; MonoBehaviour also retains `m_EditorClassIdentifier`, `m_EditorHideFlags`, `m_Enabled`, `m_GameObject`, `m_Name`, and `m_Script`. Implemented these allowlists in the value-to-YAML converter for an explicit stripped input. Excluded fields are skipped before conversion; inherited `Base` fields are evaluated at the flattened root level. `serializedVersion` is omitted and the document header carries `stripped`.
- Source inspection distinguishes the hierarchy `StrippedAssets` classification from SerializedFile `ObjectInfo.Stripped`, which is used to select the serialized type layout. The workspace YAML bridge no longer mistakes the SerializedFile bit for the output marker; because hierarchy membership is not wired yet, workspace documents remain unstripped.
- Focused tests passed: `cargo test --locked --lib stripped_asset_yaml_keeps_only_baseline_prefab_fields -- --nocapture` and `cargo test --locked --lib stripped_monobehaviour_yaml_keeps_baseline_identity_fields -- --nocapture`. Cross-file Scene YAML regression passed. Full workspace verification passed: 394 library tests, 10 CLI tests, 0 doc tests; warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This is synthetic converter evidence, not a Unity-produced fixture or runtime differential. Hierarchy-owned stripped classification is still unconnected to workspace export. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## MonoBehaviour m_Structure YAML ordering (2026-10-03)

- Ported the baseline `YamlWalker.ExitField` rule: after walking a MonoBehaviour field named `m_Structure`, its YAML mapping is removed from its current position and appended to the root mapping's end. Rust applies this only for class ID 114; other class IDs retain TypeTree order.
- Focused test passed: `cargo test --locked --lib monobehaviour_structure_field_is_written_last -- --nocapture`. It verifies both the MonoBehaviour reorder and non-MonoBehaviour order preservation.
- Full workspace verification passed: `cargo test --workspace --locked --quiet` (395 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This uses generated values only; no Unity-produced fixture or runtime differential. Complete generated MonoBehaviour walking, importer/project routing, and Phase 1 remain open. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.
## TypeTree-driven Unity dictionary YAML (2026-10-03)

- Inspected baseline `YamlWalker.EnterDictionary`, `EnterDictionaryPair`, `DivideDictionaryPair`, `UseHyphensInDictionaries`, and `UniversalNode.NodeType.Map/Pair`. Added dictionary representation hints from the serialized `map`/`pair` TypeTree shape and key type. Primitive, string, and GUID keys become mapping keys; keys use baseline `ToString()` formatting. Complex keys become `first`/`second` pair sequences from Unity 5.4 onward; earlier versions emit repeated `data` pair mappings. Empty dictionaries produce `{}` in each representation.
- Generated fixtures cover string map keys and empty maps, numeric/bool/char key stringification, TypeTree GUID keys, complex Vector3 keys at Unity 5.3 and 2022, and malformed pair and TypeTree rejection. A generated two-file SerializedFile integration fixture now puts a cross-file PPtr in a dictionary value, checks its exact `m_Targets[0].second` reference-graph edge, and checks the exported YAML fileID/GUID/type mapping.
- Focused tests passed: `cargo test --locked --lib dictionary_yaml_uses_type_tree_keys_and_unity_version_shape -- --nocapture`, `cargo test --locked --lib malformed_dictionary_pair_shapes_fail_with_context -- --nocapture`, and `cargo test --locked --lib pptr_dictionary_values_resolve_and_emit_exported_collection_pointer -- --nocapture`. Full workspace verification passed: `cargo test --workspace --locked --quiet` (398 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This is synthetic evidence from the parsed TypeTree/value pipeline, not a Unity-produced input or direct C# runtime differential. No-TypeTree dictionary recovery, remaining generated class walkers, and Phase 1 remain open. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## PropertyName scalar YAML and Utf8String payloads (2026-10-03)

- Inspected baseline `YamlWalker.EnterAsset`, `PropertyNameExtensions.GetIdString`, and `Pass002_RenameSubnodes.ChangeStringToUtf8String`. TypeTree hints now identify the raw one-field `string` wrapper and generated `PropertyName` wrapper, then emit Int32 IDs as decimal string scalars and UTF-8 IDs as string scalars. The SerializedFile reader and writer also accept the serialized `Utf8String` array-of-char shape.
- Focused tests passed: `cargo test --locked --lib property_name -- --nocapture` (3 tests) and `cargo test --locked --lib decodes_and_reencodes_utf8string_type_tree_nodes -- --nocapture` (1 test). Full workspace verification passed: `cargo test --workspace --locked --quiet` (402 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- The evidence is synthetic; no Unity-produced input or direct C# runtime differential was run. Remaining generated class walks and Phase 1 remain open. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## Editor ExposedReferenceTable sequence YAML (2026-10-03)

- Inspected baseline `Pass002_RenameSubnodes` and `Pass108_WalkMethods.GetName`: the Editor `ExposedReferenceTable.m_References` field is treated as a vector and its original serialized field name is retained for YAML. Rust TypeTree hints now force only this array-shaped field to a block sequence, bypassing dictionary key formatting while preserving the general mapping behavior for unrelated maps.
- Focused test passed: `cargo test --locked --lib exposed_reference_table_entries_use_block_sequences_instead_of_keyed_maps -- --nocapture`. It covers nonempty sequence output, ordinary keyed-map preservation, and empty block-sequence output. Full workspace verification passed: `cargo test --workspace --locked --quiet` (403 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This is synthetic evidence only; no Unity-produced fixture or direct C# runtime differential was run. Other generated type transformations and Phase 1 remain open. No UABEA, user AssetBundle content, or other remote repository was accessed; Phase 2 remains locked.

## SceneObjectIdentifier raw YAML fallback (2026-10-03)

- Inspected baseline `ProjectYamlWalker.EnterAsset` and `Pass508_LazySceneObjectIdentifier`. Added TypeTree-derived schema recognition for the signed 64-bit `TargetObject` and `TargetPrefab` fields and ProjectAsset YAML output using the fixed `targetObject` and `targetPrefab` keys. Array elements resolve through wildcard schema paths. Invalid or ambiguous layouts and invalid decoded values fail explicitly.
- Focused tests passed: `cargo test --locked --lib scene_object_identifier -- --nocapture` (3 tests). Full workspace verification passed: `cargo test --workspace --locked --quiet` (406 library tests, 10 CLI tests, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all -- --check`; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This covers only the raw serialized-ID fallback with synthetic TypeTrees and values. Baseline-injected `TargetObjectReference` and `TargetPrefabReference` are populated by processing and take precedence when available; that processing/link-resolution path is not yet connected to Rust YAML export. No Unity-produced fixture or direct C# runtime differential was run. Phase 1 remains open and Phase 2 remains locked.

## Baseline global YAML field aliases (2026-10-03)

- Ported five unconditional field-name conversions from `Pass002_RenameSubnodes.DoSecondaryRenaming`: `m_Image_data` to `m_ImageData`, `m_TextureFormat` to `m_Format`, `m_TextureDimension` to `m_Dimension`, `m_ObjectHideFlags` to `m_HideFlags`, and `m_Val` to `m_Value`. Filtering, stripped allowlists, and TypeTree schema paths continue to use the serialized source names; aliases are applied only to emitted YAML mapping keys.
- Focused test passed: `cargo test --locked --lib baseline_global_and_prefix_field_names_are_emitted_as_yaml_aliases -- --nocapture`. Full workspace passed: `cargo test --workspace --locked --quiet` (407 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This covers only the five named field aliases with synthetic values. No Unity-produced fixture or direct C# runtime differential was run. Other context-dependent Pass002 rules remain open. Phase 1 remains incomplete; no UABEA or user AssetBundle content was accessed.

## Baseline YAML field prefix aliases (2026-10-03)

- Ported the `m_Dst`, `m_Dest`, and `m_Src` field prefix rules from `Pass002_RenameSubnodes.DoSecondaryRenaming`. They become `m_Destination` or `m_Source` only when the next UTF-16 code unit is uppercase; suffix bytes remain unchanged. Empty and lowercase-initial suffixes remain unchanged, and output-name allocation is fallible.
- Focused test passed: `cargo test --locked --lib baseline_global_and_prefix_field_names_are_emitted_as_yaml_aliases -- --nocapture`. It verifies all three aliases, lowercase and empty suffixes, a nonmatching already-expanded name, BMP uppercase handling, and no rename for a supplementary uppercase initial surrogate pair. Full workspace passed: 407 library tests, 10 CLI tests, 0 doc tests; warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This uses synthetic decoded values only; no Unity-produced fixture or direct C# runtime differential was run. Remaining context-dependent Pass002 transformations remain open. Phase 1 remains incomplete; Phase 2 remains locked.

## AnimationClip settings YAML field alias (2026-10-03)

- Inspected `Pass002_RenameSubnodes.DoSecondaryRenaming` and the baseline class ID table (`AnimationClip = 74`). The TypeTree hint collector now records a path alias only when a TypeTree node is `AnimationClip` and directly owns `m_AnimationClipSettings`. YAML conversion writes that field as `m_MuscleClipInfo`; nested array paths use the same wildcard matching as other hints.
- Focused test passed: `cargo test --locked --lib animation_clip_settings_field_uses_type_tree_scoped_alias -- --nocapture`. It verifies the root field, an array-element field, and preservation of the same serialized field under a different TypeTree parent. Full workspace passed: `cargo test --workspace --locked --quiet` (408 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic; no Unity-produced TypeTree fixture or direct C# runtime differential was run. Other AnimationClip processing/export paths and the Phase 1 gate remain incomplete. Phase 2 remains locked.

## GenericBinding class ID YAML field alias (2026-10-03)

- Ported the `GenericBinding` branch in `Pass002_RenameSubnodes.DoSecondaryRenaming`: its direct `m_TypeID` child now emits as `m_ClassID`. The shared TypeTree path hint supports array elements and leaves unrelated same-named fields untouched.
- Focused test passed: `cargo test --locked --lib generic_binding_type_id_uses_type_tree_scoped_alias -- --nocapture`. It checks the wildcard path, emitted value, and nonmatching parent type. Full workspace passed: `cargo test --workspace --locked --quiet` (409 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic only; no Unity-produced TypeTree or direct C# runtime differential was run. Other animation binding and Pass002 rules remain open. Phase 1 remains incomplete; Phase 2 remains locked.

## AnimatorController, layer, and BlendTree YAML field aliases (2026-10-03)

- Ported three type-scoped `Pass002_RenameSubnodes` branches using the shared TypeTree alias registry: `AnimatorController.m_AnimatorEvents` to `m_AnimatorParameters`; `AnimatorController.m_Layers` to `m_AnimatorLayers`; `AnimatorControllerLayer.m_HumanMask` to `m_Mask`; and `BlendTree.m_BlendEvent`/`m_BlendEventY` to `m_BlendParameter`/`m_BlendParameterY`. The layer alias resolves from an array-element TypeTree path.
- Focused test passed: `cargo test --locked --lib animator_controller_layer_and_blend_tree_aliases_are_type_scoped -- --nocapture`. It verifies all emitted names and values, including a null mask PPtr. Full workspace passed: `cargo test --workspace --locked --quiet` (410 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Synthetic evidence only; no Unity-produced TypeTree fixture or C# runtime differential was run. Other AnimatorController, AnimatorControllerLayer, BlendTree, and animation export behavior remains open. Phase 1 remains incomplete; Phase 2 remains locked.

## Lightmap, Texture3D, MeshRenderer, and Prefab YAML field aliases (2026-10-03)

- Added TypeTree-scoped direct-child aliases for `LightmapSettings.m_LightmapSnapshot` to `m_LightingDataAsset`; `Texture3D.m_DataSize`/`m_ImageCount` to `m_CompleteImageSize`/`m_Depth`; `MeshRenderer.m_StitchSeams` to `m_StitchLightmapSeams`; and legacy `Prefab.m_IsPrefabParent`/`m_ParentPrefab` to `m_IsPrefabAsset`/`m_SourcePrefab`. Importantly, a same-named `m_DataSize` under another parent remains unchanged.
- Focused test passed: `cargo test --locked --lib lightmap_texture3d_mesh_renderer_and_prefab_aliases_are_type_scoped -- --nocapture`. It verifies exact paths, scalar values, and true-null PPtr YAML. Full workspace passed: `cargo test --workspace --locked --quiet` (411 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic only; no Unity-produced fixture or direct C# runtime differential was run. Lightmap, Texture3D, MeshRenderer, and Prefab processing/export paths remain incomplete. Phase 1 remains open and Phase 2 remains locked.

## Animation constant YAML field aliases (2026-10-03)

- Ported three direct-child rules from `Pass002_RenameSubnodes.DoSecondaryRenaming`: both `HumanLayerConstant` and `LayerConstant` map `m_StateMachineMotionSetIndex` to `m_StateMachineSynchronizedLayerIndex`; `ControllerConstant` maps `m_HumanLayerArray` to `m_LayerArray`; and `StateMachineConstant` maps `m_MotionSetCount` to `m_SynchronizedLayerCount`.
- Focused test passed: `cargo test --locked --lib animation_constant_field_aliases_follow_type_paths -- --nocapture`. It verifies every path/value and that a same-named root `m_MotionSetCount` remains unchanged. Full workspace passed: `cargo test --workspace --locked --quiet` (412 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic; no Unity-produced TypeTree fixture or direct C# runtime differential was run. Related animation constant processing and export remain open. Phase 1 remains incomplete; Phase 2 remains locked.

## BuildSettings and Heightmap YAML field aliases (2026-10-03)

- Added TypeTree-scoped aliases for legacy `BuildSettings.m_Levels` to `m_Scenes`, and the three `Heightmap` fields `m_EnableSurfaceMaskTextureCompression`, `m_SurfaceMask`, and `m_SurfaceMaskLOD` to their Holes names. A same-named root `m_Levels` remains unchanged.
- Focused test passed: `cargo test --locked --lib build_settings_and_heightmap_aliases_follow_type_paths -- --nocapture`. It verifies the scene path array, all Heightmap fields, and true-null texture PPtr output. Full workspace passed: `cargo test --workspace --locked --quiet` (413 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic legacy-shaped data only; no Unity-produced fixture or direct C# runtime differential was run. Scene selection, Heightmap payload semantics, and full export remain incomplete. Phase 1 remains open and Phase 2 remains locked.

## Graphics and texture importer YAML field aliases (2026-10-03)

- Ported the source-owned field aliases for `GraphicsSettings.m_BuildTargetShaderSettings`/`m_AlbedoSwatchInfos`, `TextureImporter.m_BuildTargetSettings`, and `BuildTargetShaderSettings.m_ShaderSettings`. Matching values are now emitted under `m_TierSettings`, `m_AlbedoSwitchInfos`, `m_PlatformSettings`, and `m_Settings`, respectively; same-named root fields remain unchanged.
- Focused test passed: `cargo test --locked --lib graphics_and_importer_setting_aliases_are_type_scoped -- --nocapture`. It verifies paths, output names/values, and the unaffected same-named root fields. Full workspace passed: `cargo test --workspace --locked --quiet` (414 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- These are synthetic TypeTree/value checks only. The baseline's `BuildTargetShaderSettings` to `TierSettings` type-name change and related version collision handling are still unimplemented. No Unity-produced fixture or direct C# runtime differential was run; Phase 1 remains incomplete and Phase 2 remains locked.

## Binding, shader parameter, VertexData, and ComputeShader aliases (2026-10-03)

- Added direct-child aliases for `Binding.m_Set` to `m_EncodedData`; `MatrixParameter`/`StructParameter`/`VectorParameter.m_Index` to `m_OffsetInConstantBuffer`; `VertexData.m_DataSize` to `m_Data`; and `ComputeShaderPlatformVariant.m_Kernels` to `m_KernelParents`. The shared collector preserves each TypeTree owner path.
- Focused test passed: `cargo test --locked --lib buffer_vertex_compute_yaml_aliases_are_type_scoped -- --nocapture`. It checks each owner type/value and that an unrelated root `m_Index` remains unchanged. Full workspace passed: `cargo test --workspace --locked --quiet` (415 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Synthetic evidence only; no Unity-produced fixture or direct C# runtime differential was run. ComputeShader's owning type rename and shader/Mesh processing are not implemented by this alias increment. Phase 1 remains open; Phase 2 remains locked.

## ParticleSystem and ShapeModule type-dependent YAML aliases (2026-10-03)

- Added conditional aliases from the baseline TypeTree child types: `ParticleSystem.m_StartDelay` selects `m_StartDelay_Single` for `float`, otherwise `m_StartDelay_MinMaxCurve`; `ShapeModule.m_Arc` and `.m_Radius` gain `Parameter` suffixes only when their child types are not `float`.
- Focused test passed: `cargo test --locked --lib particle_and_shape_module_aliases_follow_child_type -- --nocapture`. It verifies float/curve branches and preservation of unrelated/same-type fields. Full workspace passed: `cargo test --workspace --locked --quiet` (416 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic TypeTree/value data only; no Unity-produced fixture or direct C# runtime differential was run. Particle/Shape processing and animation/resource export remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## ClipMuscleConstant and Xform vector field aliases (2026-10-03)

- Added child-type-driven field aliases for `ClipMuscleConstant.m_AverageSpeed` (`Vector3Float` to `m_AverageSpeed3`, `Vector4Float` to `m_AverageSpeed4`) and `Xform.m_T`/`m_S` (`Vector3Float`/`Vector4Float` to the corresponding `m_T3`/`m_T4`/`m_S3`/`m_S4`). Unsupported Xform child types fail TypeTree hint construction explicitly.
- Focused test passed: `cargo test --locked --lib clip_muscle_and_xform_aliases_follow_vector_child_type -- --nocapture`. It verifies all five supported branches, emitted vector values, and an unsupported-type error. Full workspace passed: `cargo test --workspace --locked --quiet` (417 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Synthetic TypeTree/value evidence only; no Unity-produced fixture or C# runtime differential. The baseline `ClipMuscleConstant` to `MuscleClipConstant` type-name remap and full animation transformation/export remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## ValueArray vector field aliases (2026-10-03)

- Ported the baseline `ValueArray` field naming rule: `m_PositionValues` and `m_ScaleValues` select their `3` or `4` suffix from the exact `Vector3Float`/`Vector4Float` TypeTree node at `m_Array.m_Data`. Malformed nested paths and unsupported element types now return contextual TypeTree errors.
- Focused test passed: `cargo test --locked --lib value_array_aliases_follow_nested_vector_element_type -- --nocapture`. It verifies all four field/type combinations, output values, a same-named unrelated root field, and rejection of an unsupported element type. Full workspace passed: `cargo test --workspace --locked --quiet` (418 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic TypeTree/value data only; no Unity-produced fixture or direct C# runtime differential was run. Complete animation value processing/export remains incomplete. Phase 1 remains open; Phase 2 remains locked.

## Mesh shape and Shader blob YAML aliases (2026-10-03)

- Ported `Mesh.m_Shapes` to `m_ShapesList` only when the direct child TypeTree type is exactly `vector`, plus the owner-scoped `Shader.m_SubProgramBlob` to `m_CompressedBlob` alias.
- Focused test passed: `cargo test --locked --lib mesh_shape_and_shader_blob_aliases_follow_baseline_rules -- --nocapture`. It verifies the Mesh type predicate, same-named unrelated/root fields, Shader alias, and value preservation. Full workspace passed: `cargo test --workspace --locked --quiet` (419 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- Evidence is synthetic TypeTree/value data only; no Unity-produced fixture or direct C# runtime differential was run. Mesh blend-shape and Shader decoding/export remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## GameObject component field alias (2026-10-03)

- Ported the direct baseline alias `GameObject.m_Component` to `m_Components`; fields with the same name under other TypeTree owners remain unchanged. The separate legacy array-element `pair` to `ComponentPair` transform is not included.
- During investigation, a combined attempt to infer the nested pair shape caused eight existing workspace YAML tests to fail because their synthetic GameObject TypeTrees omit `m_Component.m_Array`. That pair-specific code was removed; no test expectation was changed. The direct-alias test then passed, and the final full workspace passed: `cargo test --workspace --locked --quiet` (420 library tests, 10 CLI tests, 0 doc tests). Warnings-denied Clippy, format check, `tools/verify-baseline.ps1` (1,449 baseline files unchanged), and `git diff --check` also passed.
- Evidence is synthetic TypeTree/value data only; no Unity-produced fixture or direct C# runtime differential was run. Legacy ComponentPair transformation and complete scene export remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## Streamed ResourceFile byte ranges (2026-10-03)

- Added a bounded ResourceFile range API in `src/resource.rs`. Empty paths represent absent external data; non-empty ranges reject zero length, offsets/sizes outside the signed stream boundary, missing VFS files, out-of-resource ranges, and configured copy/materialization limits. Range reads preserve the source VFS stream position.
- Five synthetic VFS tests and a SmartStream absolute-read test covering cursor preservation after success and failure passed. Full workspace passed: `cargo test --workspace` (426 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This is byte-range access only; it takes an already-resolved VFS path. Bundle name lookup, child/ancestor priority, external resource providers, and UInt32/UInt64 StreamingInfo field extraction remain incomplete. No Unity-produced resource or direct C# runtime differential was run. Phase 1 remains open; Phase 2 remains locked.

## Bundle-scoped ResourceFile lookup (2026-10-03)

- Preserved each original ResourceFile entry name separately from its mounted VFS path, with aggregate name-byte limits in container mounting and workspace retention. Added `SerializedFileWorkspace::resolve_bundled_resource_file`, which normalizes names using the source-compatible `FixFileIdentifier` path and follows current bundle resources, direct child resources, then parent scopes while excluding the traversed child branch.
- Synthetic tests cover archive path and case normalization, current-resource precedence over its child, parent-resource precedence over a sibling child, sibling lookup, missing outcomes, source-name retention and bounds, plus lookup-to-byte-range integration. Full workspace passed: `cargo test --workspace` (429 library tests, 10 CLI tests, 0 doc tests); warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- This API only searches mounted bundle resources. Root standalone resources, GameBundle `IResourceProvider` integration, and UInt32/UInt64 `StreamingInfo.GetOffset` field extraction remain incomplete. No Unity-produced ResourceFile or direct C# runtime differential was run. Phase 1 remains open; Phase 2 remains locked.

## Platform and mixed external ResourceFile provider (2026-10-03)

- Implemented `PlatformResourceProvider` over the mounted VFS. It applies the source `FixResourcePath` normalization, checks platform DataPaths in order before optional Mixed DataPaths, and retains unavailable-root diagnostics separately from a normal missing resource. Logical VFS reads now join contiguous `.splitN` parts, prefer a direct file when both forms exist, and report gaps rather than silently truncating. Streamed ResourceFile range reads use this logical-file API.
- Focused tests passed for ordered Platform/Mixed lookup, archive basename normalization, unavailable DataPaths, direct-versus-split precedence, contiguous parts, gap handling, and streamed reads from split files. Full workspace passed: 433 library tests, 10 CLI tests, 0 doc tests; warnings-denied Clippy; format check; `tools/verify-baseline.ps1` (1,449 baseline files unchanged); and `git diff --check` (all exit 0).
- No Unity-produced fixture or direct C# runtime differential was run. The complete GameBundle resolution chain is not yet integrated; root standalone ResourceFiles and UInt32/UInt64 `StreamingInfo.GetOffset` extraction remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## Unified bundle and external ResourceFile resolution (2026-10-03)

- Added `resolve_resource_file`, which resolves the mounted bundle hierarchy first and only then queries the optional Platform/Mixed provider. Results retain the selected VFS path, provider source and DataPath index, or the unavailable DataPaths that were skipped. Empty names return the source-compatible absent result without querying the provider.
- The synthetic integration test places the same resource in bundle, Platform, and Mixed scopes and verifies bundle precedence; it also verifies Platform over Mixed, Mixed fallback, and missing-resource diagnostics. Focused test passed. Full workspace passed: 434 library tests, 10 CLI tests, 0 doc tests; warnings-denied Clippy; format check; baseline verifier (1,449 files unchanged); and `git diff --check`.
- No Unity-produced fixture or direct C# runtime differential was run. Root standalone ResourceFiles and UInt32/UInt64 `StreamingInfo.GetOffset` extraction remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## StreamingInfo field extraction and range read (2026-10-03)

- Added strict extraction of `path`, `offset`, and `size` from decoded TypeTree values. UInt32 offsets widen losslessly to `u64`; UInt64 offsets retain their value. Empty-path `IsSet` behavior is represented explicitly, and malformed UTF-8, duplicate/missing fields, and wrong scalar types return contextual errors. Added a bounded range-read adapter for the extracted info when the resource path has already been resolved.
- Focused StreamingInfo tests passed: both offset widths, path and size validation, empty path, duplicate/missing fields, and resolved-path range reads. Full workspace passed: 437 library tests, 10 CLI tests, 0 doc tests; warnings-denied Clippy; format check; baseline verifier (1,449 files unchanged); and `git diff --check`.
- Evidence is synthetic decoded-value data only; no Unity-produced TypeTree or direct C# runtime differential was run. Generated per-version asset field extraction and exporter wiring remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## Explicit GameBundle root ResourceFiles (2026-10-03)

- Added a bounded root ResourceFile catalog for ordered explicit inputs already dispatched as `FileKind::Resource`. It rejects other dispatched kinds, missing VFS paths, empty names/paths, and count/name/path limit overruns while preserving input order and keeping these entries separate from platform inventory candidates.
- Added root-aware resolution so a matching explicit GameBundle resource is checked after the owning Bundle/ancestor path but before the GameBundle's direct child Bundles and external provider. A collision test verifies that explicit root resource wins over a same-named root sibling Bundle and Platform provider; catalog tests cover order, dispatch kind, presence, count, and aggregate-name limits.
- Focused tests and full workspace passed: 439 library tests, 10 CLI tests, 0 doc tests; warnings-denied Clippy; format check; baseline verifier (1,449 files unchanged); and `git diff --check`.
- Final verification commands and exit statuses: `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked --quiet` (0); `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings` (0); `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all -- --check` (0); `tools/verify-baseline.ps1` (PowerShell, 0, 1,449 files unchanged); `git diff --check` (0).
- No Unity-produced fixture or direct C# runtime differential was run. The library catalog is not yet connected to CLI/UI explicit-file selection. Phase 1 remains open; Phase 2 remains locked.

## Explicit file-list CLI import for root ResourceFiles (2026-10-03)

- Added `assetripper-rs inspect-files <input-file> [input-file ...]`. The mount report records selected top-level inputs only after `SchemeReader` dispatches them as `FileKind::Resource`; directory-discovered resources are not included in this root catalog. The CLI preserves selection order, validates mounted VFS paths through `RootResourceFileCatalog`, and lists the resulting root resources alongside the inspected SerializedFiles.
- The CLI fixture supplies a generated SerializedFile, two selected ResourceFiles in a deliberate order, and an unselected same-directory ResourceFile. It verifies both selected entries and their order are reported and the unselected file is absent. Scheme mount-path tests also assert Resource dispatch metadata and virtual path.
- Exact final commands and exit statuses: `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked explicit_file_inspection_registers_only_selected_resource_inputs_as_roots` (0; 1 CLI test); `& 'C:\Users\koishi\.cargo\bin\cargo.exe' test --workspace --locked --quiet` (0; 439 library, 11 CLI, 0 doc tests); `& 'C:\Users\koishi\.cargo\bin\cargo.exe' clippy --workspace --all-targets --locked -- -D warnings` (0); `& 'C:\Users\koishi\.cargo\bin\cargo.exe' fmt --all -- --check` (0); `.\tools\verify-baseline.ps1` (PowerShell, 0; 1,449 files unchanged); `git diff --check` (0).
- The root-aware resolver remains a library API with synthetic precedence tests; object decoding and exporters do not call it yet. No Unity-produced fixture or direct C# runtime differential was run. UI, export integration, broad Phase 1 parity, and Phase 2 remain incomplete.
## Nested StreamingInfo scan and range audit (2026-10-03)

- Added an iterative, bounded scan over already-decoded object values. It records nested field paths, extracts UInt32/UInt64 offsets, resolves bundle resources before explicit roots and Platform/Mixed DataPaths, and checks offset/size integrity without materializing resource bytes.
- The CLI now reports StreamingInfo outcomes for explicit-file, Platform-directory, and Mixed-directory imports. Missing resources, invalid ranges, resolver failures, malformed scan candidates, and undecoded objects remain distinct; diagnostics are capped at 128 lines and aggregate scan work has explicit bounds.
- MixedGameStructure DataPaths now have a direct provider adapter. CLI tests verify a selected root ResourceFile resolves a valid range and an unselected same-directory file remains missing. A provider test checks ordered MixedGameStructure paths and unavailable-root reporting.
- Exact validation commands and results: cargo test --workspace --locked explicit_file_inspection (exit 0; 3 CLI tests); cargo test --workspace --locked external_resource_lookup_accepts_mixed_game_structure_data_paths (exit 0; 1 library test); cargo test --workspace --locked (exit 0; 443 library, 13 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings (exit 0); cargo fmt --all -- --check (exit 0); tools/verify-baseline.ps1 (exit 0; 1,449 baseline files unchanged); git diff --check (exit 0).
- No Unity-produced StreamingInfo fixture or direct C# runtime differential was run. Generated per-version asset field extraction, resource consumers/exporters, UI parity, and broad Phase 1 coverage remain incomplete. Phase 2 remains locked.## Bounded StreamingInfo content resolution (2026-10-03)

- Added resolve_and_read_streamed_resource_info as a reusable consumer API. It resolves the ResourceFile through bundle, explicit-root, and external-provider scope, then returns exactly the requested bytes under separate lookup and materialization limits.
- Unset streams and missing resources remain distinct results. Invalid/out-of-file ranges, unavailable reads, and over-limit materialization return errors with their cause; the API does not substitute empty data.
- Focused test streamed_resource_content_keeps_unset_missing_and_read_errors_distinct passed. The explicit-root range test also verifies exact returned bytes.
- Full workspace passed: 444 library tests, 13 CLI tests, 0 doc tests. Clippy with warnings denied, format check, baseline verifier (1,449 files unchanged), and git diff --check passed.
- No Unity-produced fixture or direct C# runtime differential was run. The byte API is not yet called by Texture2D/Mesh or other asset exporters; generated type integration and Phase 1 parity remain incomplete.

## UInt32 StreamingInfo explicit-root SerializedFile fixture (2026-10-03)

- Extended the generated SerializedFile fixture builder to encode `StreamingInfo.offset` as either UInt32 or UInt64. The new `inspect-files` CLI fixture exercises the UInt32 TypeTree leaf through object decoding, root ResourceFile resolution, and range validation against an explicitly selected `texture.resS` input.
- Focused verification passed: `cargo test --locked explicit_file_inspection_resolves_uint32_streaming_info_offset_from_selected_root_resource` (exit 0; 1 CLI test). Full verification passed: `cargo test --workspace --locked --quiet` (exit 0; 444 library, 14 CLI, 0 doc tests); warnings-denied Clippy (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; 1,449 baseline files unchanged); and `git diff --check` (exit 0).
- This remains generated synthetic evidence; no Unity-produced TypeTree or direct C# runtime differential was run. Texture/Mesh exporter consumption and generated per-version field integration remain incomplete. Phase 1 remains open; Phase 2 remains locked.

## Resource-aware Mesh and image-texture YAML export (2026-10-03)

- Read-only baseline tracing of `ProjectExporter.Overrides`, `YamlStreamedAssetExportCollection.ExportMesh` and `ExportTexture` confirmed that the YAML override targets Mesh (class ID 43) and IImageTexture (class ID 189); Texture2D (class ID 28) uses the separate binary texture exporter. For IImageTexture, the baseline returns false when GetContent is null or empty; GetContent returns null for an unset path or zero-size stream before resource lookup. YAML export clears StreamingInfo in the serialization view and restores source values afterward.
- Added `src/export/streamed_assets.rs::prepare_streamed_yaml_asset` and resource-aware object, generic collection, and Scene YAML entry points in `src/export/workspace_yaml.rs`. They resolve against the source file's owning bundle, explicit root ResourceFiles, then the Platform/Mixed provider, stage exact bounded bytes into an export-owned value copy capped by `max_source_value_copy_bytes`, preserve existing inline data, and return distinct errors for missing resources, resolution failures, or malformed fields.
- Six adapter tests and five generated SerializedFile export fixtures passed: `workspace_yaml_export_resolves_and_embeds_streamed_image_texture_bytes`, `workspace_yaml_collection_skips_image_texture_with_unset_or_zero_length_stream`, `workspace_yaml_collection_preserves_multi_asset_order_and_streamed_bytes`, `workspace_yaml_export_resolves_and_embeds_streamed_mesh_bytes`, and `workspace_yaml_export_reports_missing_streamed_resource_without_null_or_empty_data` (including the temporary-copy bound). The full workspace passed: 455 library tests, 14 CLI tests, 0 doc tests. Warnings-denied Clippy, `cargo fmt --all -- --check`, baseline verification (1,449 reference files unchanged), and `git diff --check` passed.
- Object-level resource resolution through YAML serialization is covered for Mesh and image texture, including unchanged source values and explicit missing-resource failure. Generated fixtures exercise object, collection, and Scene entry points; empty inline image textures are omitted for an unset stream and a zero-length streamed range without looking up the zero-length resource. The two-file collection/Scene fixture verifies caller assignment order, maps each object to its own streamed bytes, preserves existing inline bytes even when that stream resource is absent, and leaves source payloads unchanged. No Unity-produced TypeTree or direct C# runtime differential was run; generated per-version field routing, CLI/UI project export, complete Phase 1 parity, and Phase 2 remain incomplete.


## Single-object streamed YAML CLI export (2026-10-03)

- Added `export-object-yaml <serialized-file-index> <path-id> <new-output-yaml> <input-file> [input-file ...]`. It mounts only explicitly selected files, resolves the selected object's streamed resource through the existing bundle/root/Mixed DataPaths flow, and writes one YAML document. Existing output paths are rejected; a baseline image-texture skip creates no file.
- Three synthetic CLI-handler tests passed: streamed ImageTexture bytes are embedded and StreamingInfo is cleared; a second write is rejected without changing the first output; a missing streamed resource fails without creating output; and the baseline empty-image skip creates no output. These are generated 2019.4 fixtures, not Unity-produced samples.
- Exact validation commands and results: `cargo test --workspace --locked --quiet` (exit 0; 455 library, 17 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; 1,449 reference files unchanged); and `git diff --check` (exit 0). Focused `cargo test --workspace --locked export_object_yaml_ -- --nocapture` passed (3 CLI tests).
- This is a single-object YAML command, not a complete Unity project exporter; `.meta` generation, full project export, UI workflow, generated per-version streamed-field routing, Phase 1 parity, and Phase 2 remain incomplete. No user AssetBundle or UABEA content was accessed.

## Class-specific streamed YAML field routing (2026-10-03)

- Read-only baseline tracing confirmed the YAML exporter reads Mesh `StreamData` and `VertexData.Data`, and class-189 ImageTexture `StreamData_C189` / `ImageData_C189`. Class-28 Texture2D uses `StreamData_C28` / `ImageData_C28` through the separate binary `TextureAssetExporter`. The source YAML TypeTree pass renames legacy `VertexData.m_DataSize` to `m_Data`; that spelling is shape-driven, not a special streamed-export version conditional.
- Tightened Rust ImageTexture routing to the exact class-189 fields. C28 and unsuffixed aliases no longer cause class-189 staging to select a different resource or image field. Mesh remains on `m_StreamData` with the existing TypeTree-derived vertex byte-data lookup.
- Added tests with C189 fields alongside C28 and unsuffixed decoys, and with only a C28-shaped Texture2D payload. They verify only the class-189 stream is materialized and unrelated fields remain unchanged; C28-only input is not staged as class-189 ImageTexture data. The streamed-assets adapter module passed all 8 tests.
- Full validation passed: `cargo test --workspace --locked --quiet` (exit 0; 457 library, 17 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; 1,449 reference files unchanged); and `git diff --check` (exit 0). No user AssetBundle, UABEA input, or original AssetRipper output was inspected.
- This proves the traced field-identity rule with source evidence and synthetic values; it does not prove generated schemas across all Unity versions or class-189 behavior on a Unity-produced file. Unity-produced differential, complete project export, remaining resource kinds, and Phase 1 remain open.

## Resource-aware SerializedFile YAML collection CLI (2026-10-03)

- Added `export-file-yaml <serialized-file-index> <new-output-yaml> <input-file> [input-file ...]`. It serializes one selected file's YAML-exportable objects in object-table order, stages streamed Mesh/ImageTexture bytes, and assigns source PathIDs to YAML document IDs so uniquely resolved intra-file PPtrs remain local references.
- Before writing, the command requires a complete TypeTree reference scan for every object and rejects null-external/missing/ambiguous references or references into another SerializedFile. It builds YAML in memory and creates output only after conversion succeeds. Existing output files are never replaced. The baseline's ImageTexture-without-data omission is reported; if all objects are omitted, no output file is created.
- Four synthetic CLI-handler tests verify two-object order and separate resource ranges, source and resource byte preservation, command argument dispatch plus self-reference mapping, explicit unresolved and cross-file errors before output creation, explicit all-omitted reporting, and overwrite rejection. Fixtures use generated Unity 2019.4 metadata; no Unity-produced file was used.
- During fixture development, the first multi-object test correctly failed because the synthetic writer placed generation-10 table fields inside each object record. The fixture was corrected to write the external table after all records; final validation then passed.
- Full validation: `cargo test --workspace --locked --quiet` (exit 0; 457 library, 21 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `cargo run --locked --quiet -- --help` (exit 0; command listed); `tools/verify-baseline.ps1` (exit 0; 1,449 reference files unchanged); and `git diff --check` (exit 0).
- This is a verified one-SerializedFile YAML slice. Source PathID IDs and table order are not yet proven equivalent to the baseline `ExportableAssets` assignments/order. Cross-file references, `.meta` output, complete Unity project export, GUI workflow, broad generated-schema coverage, Unity/C# differential validation, and Phase 1 remain incomplete. No UABEA or user AssetBundle content was read.

## Scene generated-Transform YAML ID planning (2026-10-03)

- Read-only baseline tracing confirmed that `PrefabProcessor.AddMissingTransforms` appends generated Transform assets to a per-Scene processed collection before `SceneHierarchyObject.Create`. `SceneDefinition.Assets` traverses source collections and then appended generated collections. `SceneHierarchyObject.Create` categorizes the generated Transform as a Component; it sees the newly created SceneAsset during iteration but does not classify it into `Hierarchy.Assets`. `SceneExportCollection` assigns IDs in hierarchy group order, then separately applies its stable manager-first YAML sort.
- Added `plan_workspace_scene_export_ids_with_processed_assets`. It requires a complete processed-asset plan and exactly one selected scene-generated collection, adds generated Transform identities after source Components in generated insertion order, validates collection scope/class/target membership, excludes the generated SceneAsset, and preserves the existing separate YAML ordering helper. Other generated asset kinds in this collection fail explicitly until modeled.
- A synthetic Unity 2019.4 scene test verifies source GameObject then processed Transform then manager ID order, the generated collection/object identity and pseudo-random ID seed, omission of the SceneAsset, and manager-first YAML ordering with the Transform ID unchanged.
- Focused test passed: `cargo test --workspace --locked scene_export_plan_includes_generated_transforms_and_keeps_yaml_sort_separate -- --nocapture` (exit 0; 1 test). Full workspace passed: `cargo test --workspace --locked --quiet` (exit 0; 458 library, 21 CLI, 0 doc tests). Also passed warnings-denied Clippy, format check, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` (all exit 0).
- Cloud push: local implementation commit `9f27f3d120bbfdb018dfa460ba4cfa9d4f5bf352` has tree `12d5be5aaaabd80359829249def18a5fdc40aee6`; remote `main` commit `31b31a56cbe0c7fa7cecf80ee0122a8dc1d75c22` has the same tree and parent `1aba48352bba09dfa0341e5de146358221cfed3a`. The non-forced push succeeded; the live ref was read back and the fetched-tree diff was empty.
- This is ID planning only. The workspace YAML executor still rejects `ProcessedCollection` assignments pending a generated-object YAML walker; scene `.meta` GUID output, full CLI/project export, and Phase 1 remain incomplete. No user AssetBundle bytes, UABEA content, or other repository were accessed.

## Generated Transform YAML document conversion (2026-10-03)

- Added `generated_transform_payload_to_unity_yaml_document`. It checks the processed identity and class, selected serialized Transform TypeTree and Unity version, re-encodes the typed payload to verify its bytes, and accepts only null pointers plus the planned non-null `m_GameObject` constructor PPtr. That pointer is resolved through the project export map; a missing exported target or unexpected PPtr fails explicitly.
- A synthetic Unity 2019.4 test obtains the payload through `materialize_planned_generated_transform_payload` and verifies a generated Transform YAML document with the assigned document ID, a local GameObject fileID, default Transform fields, and no external GUID for a same-Scene reference.
- Focused test passed: `cargo test --workspace --locked generated_transform_yaml_resolves_its_constructor_pointer_to_a_local_scene_id -- --nocapture` (exit 0; 1 test). Full workspace passed: `cargo test --workspace --locked --quiet` (exit 0; 459 library, 21 CLI, 0 doc tests). Also passed warnings-denied Clippy, format check, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` (all exit 0).
- Cloud push: local implementation commit `5664208ccfa0ec0d4fee6c55cd5f8fc97a8ce8b6` has tree `53817827da35f36720c7a911f8f476053f5e3b98`; remote `main` commit `2715c03bb1b3ba25480bda81ed66660d7aa9e4c5` has the same tree and parent `4e6ec3fe545d25deb69b2534ea5bf5273a9669f5`. The non-forced push succeeded; the live ref was read back and the fetched-tree diff was empty.
- This function emits one generated Transform document but is not yet called by the collection executor, which still rejects processed assets. The reciprocal GameObject component-list overlay and complete Scene/project output remain required. No user AssetBundle bytes, UABEA content, or other repository were accessed.

## Scene generated-Transform YAML executor integration (2026-10-03)

- Added generated-Transform dispatch to the bounded Scene YAML collection executor. The explicit context verifies that the assignment belongs to the selected SceneGeneratedAssets collection, matches the processed plan identity, has exactly one planned GameObject constructor pointer, and targets a source GameObject whose SerializedFile and reference-graph identities agree. The materializer validates the dependency FileID and TypeTree-backed payload before the YAML walker maps the pointer.
- Added a combined Scene entry point for generated Transforms plus streamed resources. Existing entry points without the explicit generated-Transform context continue to report processed assignments as requiring a generated walker; other processed asset kinds fail the generated-Transform invariant check.
- The synthetic Unity 2019.4 integration fixture begins with an active, named GameObject with no Transform, builds complete reference and hierarchy plans, allocates the generated Transform, registers Scene assignments in the export pointer map, and executes the generated assignment. It checks the generated YAML document ID, local GameObject fileID without an external GUID, and unit scale. The reciprocal GameObject m_Component overlay is not yet applied by the YAML executor.
- Focused validation: `cargo test --lib scene_executor_writes_a_planned_generated_transform_document` (exit 0; 1 test). Full validation: `cargo test --workspace --locked --quiet` (exit 0; 460 library, 21 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; all 1,449 reference files unchanged); and `git diff --check` (exit 0).
- Cloud push: local implementation commit `4c4843a3fd647498d5d1139c47098b0def2ef38f` has tree `73c43b9cfeed6222761b59a82ec467a1227c51f0`; remote `main` checkpoint `a5384497b77f25bff2a1edc88506484beaf7e685` has the same tree and parent `ddc824d7bf01a92ed3c87db47408008eef4bc705`. The non-forced push succeeded; the live ref was read back and the fetched-tree diff was empty.
- This increment validates generated Transform document dispatch only. Applying the generated component PPtr to the source GameObject YAML, other processed asset YAML walkers, complete Scene/project output, and Phase 1 remain incomplete. No user AssetBundle bytes, UABEA contents, or another repository were accessed.
## Scene generated-Transform GameObject component overlay export (2026-10-03)

- Extended the generated-Transform-aware Scene YAML executor to materialize the planned `m_Component` append on its source GameObject. Existing component slots are checked against `WorkspaceGameObjectComponentListOverlay` and retained. For an empty array, the appended element template is built from the `data` subtree of the source GameObject TypeTree; the new bounded `default_value_for_type_tree_node` validates and constructs only that subtree.
- The single appended PPtr is accepted only at its planned array-element field path with the dependency FileID and generated Transform PathID from the processed plan. Its target is resolved through `ProjectExportPointerMap`. All pre-existing PPtrs continue through the input `MultiFileReferenceGraph`; the executor errors if the planned pointer is absent, duplicated, mismatched, or not registered for export.
- Expanded `scene_executor_writes_a_planned_generated_transform_document` to run the full Scene assignment list and check both YAML documents: GameObject `component` points to the generated Transform's local export ID, and Transform `m_GameObject` points back to the GameObject's local export ID. The fixture's empty `m_Component` array exercises the TypeTree-derived ComponentPair template. `constructs_array_element_template_from_its_type_tree_subtree` checks the bounded subtree-default API. Existing component append tests also verify preservation of null and non-null original slots and rejection of unsupported shapes.

- Focused validation passed: `cargo test --lib constructs_array_element_template_from_its_type_tree_subtree` and `cargo test --lib scene_executor_writes_a_planned_generated_transform_document` (exit 0 each). Full validation passed: `cargo test --workspace --locked --quiet` (exit 0; 461 library, 21 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; 1,449 reference files unchanged); and `git diff --check` (exit 0).
- Cloud push: local implementation commit `1da8b3a3d62c0ea45ce7e84e9394338ead3b93e8` has tree `b8e52a155edc29c4be4e4f3123d684435f07b493`; remote `main` checkpoint `cdc443d0b8f7e32697152b49fe63d285cb69862d` has the same tree and parent `222ee6cef7e73fdcac6b4a499d14d18299aa8322`. The non-forced push succeeded; the live ref was read back and the fetched-tree diff was empty.
- This adds one verified Scene-generated Transform YAML path. Other processed asset YAML walkers, generated SceneAsset serialization, complete Scene/project output, `.meta`, CLI/UI parity, and Phase 1 remain incomplete. No user AssetBundle bytes, UABEA contents, or another repository were accessed.

## Scene and Prefab export collection scope audit (2026-10-03)

- Read-only source trace of `PrefabProcessor`, `SceneHierarchyObject`, and `SceneExportCollection` confirms the per-Scene processed collection is created by `AddMissingTransforms`; it contains only generated Transforms. `SceneHierarchyObject.Create` classifies those as Components. The synthetic SceneHierarchyObject itself is encountered in `SceneDefinition.Assets` but is not added to `Hierarchy.Assets`; exported Scene members are GameObjects, Components, PrefabInstances, managers, and optional SceneRoots. Scene IDs follow those groups, while the separate stable YAML sort puts LevelGameManagers first.
- This closes the `SceneGeneratedAssets` class inventory for Scene YAML: the generated-Transform reciprocal reference writer covers the only processed class in that path. Other generated classes belong to distinct project collections.
- The next generated object path is `CreatePrefabForRoot` -> `PrefabHierarchyObject` -> `PrefabExportCollection`. The baseline emits the main Prefab asset only for `IPrefabMarker`; modern `IPrefabInstanceMarker` assets are hidden from YAML and use a PrefabImporter. `SetPrefabInternal` also mutates hierarchy objects only for the legacy marker. Rust has identity/ID planning for these branches, but no generated Prefab payload writer or complete Prefab export executor yet.
- No test was run for this source-only audit. Existing Scene-generated Transform tests and prior full verification remain as recorded above; no AssetBundle or UABEA content was accessed.

## Modern Prefab hierarchy YAML body execution (2026-10-03)

- Added `workspace_prefab_hierarchy_to_unity_yaml`. It converts the plan's `exportable_assets` to registered assignments in the exact order supplied by `PrefabExportCollection.ExportableAssets`, validates the representation/`emit_prefab_asset` invariant, enforces the document cap, and fails on missing or duplicate ID registrations.
- The modern `IPrefabInstanceMarker` path excludes the hidden processed main asset from YAML while serializing the source GameObject and component documents through the workspace reference graph and Prefab export pointer map. The synthetic 2019.4 GameObject/MeshRenderer case verifies order and both local PPtrs. A legacy plan that includes the processed class-ID-1001 Prefab reaches the explicit missing-generated-walker error; no partial output is returned.
- Focused validation: `cargo test --workspace --locked modern_prefab_executor_omits_hidden_marker_and_preserves_hierarchy_order -- --nocapture` (exit 0; 1 test). Full workspace passed: `cargo test --workspace --locked --quiet` (exit 0; 462 library, 21 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; all 1,449 baseline files unchanged); and `git diff --check` (exit 0).
- Scope: YAML body execution only. PrefabImporter `.meta`, legacy generated Prefab payload and `m_PrefabInternal` overlays, generated Transform emission inside a Prefab collection, the integrated project exporter, and Unity-produced/C# differential checks remain incomplete. No user AssetBundle or UABEA content was accessed.
- Cloud push: local implementation commit `e1c7fe31cebd012873b537d8096a8c10fc582b28` has tree `65ba968ca642b07f8fcff635f704a21e3e07c978`; remote `main` checkpoint `b05597514076fb72e6cd939f0843ac7893ffd824` has the same tree and parent `e8ce13b2393fcdd62d81044080a6c9a82adae723`. The non-forced push succeeded; the live ref was read back and the fetched-tree diff was empty.

## AssetBundle Container pointer retention (2026-10-03)

- Added `extract_asset_bundle_container_references`. It decodes ordered `m_Container`/`Container` entries, validates the key string and AssetInfo struct, and preserves the nested `asset` PPtr FileID and PathID. Local FileID zero, nonzero shared-file IDs, and explicit null PPtrs remain distinct.
- Existing streamed-scene metadata still reads its path-only view and is unchanged. A raw generation-10 Unity 2019.4 AssetBundle fixture also passes through the new pointer decoder; synthetic tests cover non-null local/shared pointers, null pointers, inherited field lookup, missing and malformed entries, and entry/path limits.
- Focused validation: `cargo test --workspace --locked asset_bundle_container_decoder -- --nocapture` (exit 0; 3 tests) and `cargo test --workspace --locked decodes_assetbundle_scene_fields_from_a_complete_serialized_file -- --nocapture` (exit 0; 1 test). Full workspace passed: `cargo test --workspace --locked --quiet` (exit 0; 465 library, 21 CLI, 0 doc tests); warnings-denied Clippy, format check, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- Scope: these are decoded references only. Workspace resolution, AssetBundleName_R versus Bundle.Name selection, `.bundle` suffix behavior, local asset-name assignment, `.meta` generation, and full project processing remain incomplete. No user AssetBundle bytes or UABEA content was accessed.
- Cloud push: local implementation commit `23bd770a5dd0f25213267441f80d52bafaebbe0b` has tree `a0694d486db21e1a5d3fa542993d24f03d761221`; remote `main` checkpoint `276945976677ab9ea8cf3aecc8958865efe3fc78` has the same tree and parent `83db2ca68dad5fa5dbf64ef8d24f6a098f97eca7`. The non-forced push succeeded; the live ref was read back and fetched-tree verification matched.

## AssetBundle name and PPtr processing primitives (2026-10-03)

- Added AssetBundle name selection matching AssetBundleExtensions.GetAssetBundleName: a non-empty serialized name wins, otherwise the caller supplies the source Bundle name. Added the OriginalPathProcessor trailing .bundle removal rule for names assigned to assets.
- Added a resolver that classifies decoded Container PPtrs against the owning SerializedFile PathID graph as null, a unique local object, a shared-file object, missing local target, ambiguous local target, or invalid FileID. Local FileID-zero targets remain distinct from shared references and nulls.
- Focused validation: `cargo test --workspace --locked asset_bundle -- --nocapture` (exit 0; 8 tests, including the existing raw SerializedFile decoder fixture and graph resolution cases). Full validation: `cargo test --workspace --locked --quiet` (exit 0; 470 library, 21 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; all 1,449 reference files unchanged); and `git diff --check` (exit 0).
- Scope remains processing primitives: callers still need to derive the exact source Bundle.Name from mount identity and apply ordered local assignments to asset models. OriginalPath assignment, PrefabImporter .meta generation, complete project processing, and Phase 1 remain incomplete. No user AssetBundle or UABEA content was accessed.
- Cloud push: local implementation commit `78ad9c3a927b340e1f92a0ee3857d47f0bcae351` has tree `66393d8387d8c716386c16ac9dd04e9a3c197c5f`; remote main checkpoint `0a76ea3e486e4eaebd0061a4311d75b962f8b441` has the same tree and parent `388c97d6c5c89dbf72c021b73d31d737f686025d`. The non-forced push succeeded; the live ref was read back and fetched-tree verification matched.

## Workspace AssetBundle source names and ordered assignment plan (2026-10-03)

- Container mounting now retains baseline-fixed `FileContainer.Name` values for roots and recursively mounted children, separately from virtual paths. Platform file inventory supplies the input identity; nested mounts use their source entry name. The serialized workspace owns this bounded metadata and carries it into AssetBundle processing. Dependency-container expansion derives an exact name from its source virtual path when possible and leaves unavailable names unknown.
- Added `plan_workspace_asset_bundle_names`. It traverses workspace collections and objects in source order, chooses a non-empty serialized AssetBundle name or exact source Bundle-name fallback, retains ordered Container path/PPtr classifications, and returns assignments only for unique local FileID-zero targets. Null/shared references do not assign names. Missing, ambiguous, and invalid local targets remain explicit issues. Later source assignments remain later in the plan.
- Synthetic tests cover nested root/child fixed names, mount and workspace aggregate-name bounds, a complete synthetic container-to-inspect path with root `GameBundle` fallback and a null reference, and an ordered assignment to a unique local object from a generated 2019.4 two-object fixture.
- Validation passed: `cargo test --workspace --locked --quiet` (exit 0; 474 library, 22 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; all 1,449 reference files unchanged); and `git diff --check` (exit 0). No user AssetBundle contents or UABEA data were accessed.
- This is an auditable assignment plan, not mutation of asset models or OriginalPath/.meta/project emission. Unity-produced files and direct C# differential remain untested; Phase 1 remains incomplete.
- Cloud push: local implementation commit `21f007c8c399909b35ced2e8b2cc9a28b8a4f002` has tree `5f8d346d68825b3258902955c6ac8684693b5532`; remote `main` checkpoint `24200cee3deb24aa82a7fe2dd911b81e9909491f` has the same tree and parent `05d2d2175655244cf593aae9fcb98b3442993df2`. The non-forced push succeeded; the live ref was read back, fetched, and verified to have the same tree.

## AssetBundle export path metadata (2026-10-03)

- Added `WorkspaceAssetExportPathTable` as export-owned metadata separate from decoded object values, plus `apply_workspace_asset_bundle_paths` to apply a complete ordered AssetBundle name plan.
- `DirectExport` adds the Assets prefix and retains the Container key's internal path. `GroupByAssetType` updates AssetBundleName without replacing an existing OriginalPath. `GroupByBundleName` strips the baseline Assets and bundle-name prefixes where applicable, writes under `Assets/AssetBundles`, and applies missing type directories to all direct collections in each owning source Bundle after Container assignments.
- Focused tests passed: `cargo test --workspace --locked asset_bundle_original_paths_follow_all_export_modes -- --nocapture` and `cargo test --workspace --locked workspace_asset_bundle_plan_assigns_only_unique_local_container_targets -- --nocapture`. The end-to-end fixture also checks preexisting path retention, all direct source collections, and exact retained-string byte accounting.
- Full validation passed: `cargo test --workspace --locked --quiet` (exit 0; 475 library, 22 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; all 1,449 reference files unchanged); and `git diff --check` (exit 0).
- This does not implement ResourceManager Container paths, OriginalName casing restoration, Shader overrides, the persisted processing setting, a project exporter, or `.meta` output. Unicode case-insensitive parity and output path escape handling remain open. No user AssetBundle or UABEA data was accessed.
- Cloud push: local implementation commit `9d9783c4e16062a7e085d45b43cb75a39bb85909` has tree `69926c816fa566c5f6cba1bdb8798c4bcdb56157`; remote `main` checkpoint `3398f4faa9ab706756e5c0c488ca4a4e5176f294` has the same tree and parent `03275c3c713eadc9e0396b6426f535b327b8872b`. The non-forced push succeeded; the live ref was read back, fetched, and verified to have the same tree.

## AssetBundle OriginalName and Shader path behavior (2026-10-03)

- Added OriginalName/OriginalExtension and Shader override metadata to the export-owned path table. `OriginalPathProcessor` behavior now distinguishes paths that skip postprocessing from non-empty GroupByAssetType entries that preserve the existing path while still running name and Shader logic.
- Name restoration checks UTF-16 code-unit length, applies ASCII ordinal-insensitive comparison, and recalculates OriginalPath only when the baseline name setter runs. Same-length non-ASCII pairs remain unchanged and increment an explicit unverified counter. An undecoded target payload increments `named_asset_name_unavailable`; malformed UTF-8 and duplicate field lookups fail with object context.
- Shader class ID 48 fills only null override directory/name/extension components after case restoration. Existing overrides are retained. Metadata byte limits account for every new component.
- Focused path tests and the AssetBundle workspace integration test passed. Full validation passed: `cargo test --workspace --locked --quiet` (exit 0; 479 library, 22 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` (exit 0); `cargo fmt --all -- --check` (exit 0); `tools/verify-baseline.ps1` (exit 0; all 1,449 baseline files unchanged); and `git diff --check` (exit 0).
- Phase 1 remains open. ResourceManager Container paths, persisted mode settings, project exporter and `.meta` integration, exact Unicode OrdinalIgnoreCase parity, and output path-escape enforcement remain. No original AssetBundle or UABEA data was accessed.
- Cloud push: local implementation commit `dcafbe115233c6ed00fab22ee3246e4fb41a2111` has tree `6c1c3d1797eb19090ee0dcedc5b2827496027b49`; remote `main` checkpoint `29e8f77a1c38fb0f11b435854d48233d6b4ebbcc` has the same tree and parent `c590c8af41e79a8f1ade4538dc405860988324ec`. The non-forced push succeeded; the live remote ref and fetched tree were read back and matched. This verification note will be pushed as a documentation follow-up.

## ResourceManager Container path planning and application (2026-10-03)

- Added a bounded decoder for ordered class-ID-147 `ResourceManager.Container` entries and a workspace planner that associates each key with its resolved PPtr target. PPtr resolution uses the multi-file reference graph; null entries do not become assignments, and unresolved or incomplete scans remain explicit plan issues.
- Combined AssetBundle and ResourceManager path operations are sorted by source collection/object/entry order. ResourceManager candidates join `Assets/Resources` with their key and replace only an absent or shorter existing path measured in UTF-16 code units. Successful updates run OriginalName and Shader postprocessing.
- Focused test passed: `cargo test --workspace --locked resource_manager_paths_select_longest_container_path_and_restore_asset_name` (exit 0; 1 test). Full workspace passed: `cargo test --workspace --locked --quiet` (exit 0; 483 library, 23 CLI, 0 doc tests). Warnings-denied Clippy, `cargo fmt --all -- --check`, `tools/verify-baseline.ps1` (1,449 reference files unchanged), and `git diff --check` all exited 0.
- The generated Unity 2019.4 end-to-end fixture is synthetic and uses local PPtrs. No Unity-produced file or direct C# differential was run. The path table is not connected to complete project output or `.meta` writing. Phase 1 remains incomplete.

- Cloud verification: local implementation commit `4e133c651ee9eaf3b443b6f4fb8504753a478b81` has tree `a390cccfad2a0d7351e6baf7604971bc3ade7a57`; non-forced remote `main` checkpoint `8a5d09626d01e2ac4087d01b66a04470e7e79bd9` has the same tree and parent `6a3eae77748e1004f2afbdf8e71c934e5269d02e`. `ls-remote`, fetch, and fetched-tree comparison all matched.

## Unity .meta YAML envelope serializer (2026-10-03)

- Source trace: Meta.ExportYamlDocument writes fileFormatVersion 2, GUID, optional folder and license fields, then the importer-class mapping. ExportCollection.ExportMeta writes the resulting document as UTF-8 without a BOM and suppresses YAML stream headers/version tags. The source YAML writer formats importer mapping keys and emits booleans as numeric 1/0.
- Added UnityMetaFile and write_unity_yaml_mapping. The serializer validates a nonzero GUID, importer class identifier, and undecorated mapping root; preserves source field order; applies bounded node/depth/output serialization; and resolves time from an explicit value, SOURCE_DATE_EPOCH, or current UTC time.
- Focused checks passed: cargo test --workspace --locked meta_yaml (3 tests) and cargo test --workspace --locked writes_standalone_mapping_without_unity_stream_header (1 test). Full workspace: cargo test --workspace --locked --quiet (exit 0; 487 library, 23 CLI, 0 doc tests). cargo clippy --workspace --all-targets --locked -- -D warnings, cargo fmt --all -- --check, tools/verify-baseline.ps1 (1,449 files unchanged), and git diff --check all exited 0.
- This serializes only the metadata envelope around an existing importer mapping. Class-specific importer factories, per-asset path/GUID ownership, actual sidecar file writes, complete project export, and C# runtime differential remain unimplemented or unverified. No user AssetBundle or UABEA content was accessed.

- Cloud verification: local implementation commit 48daaf498a8d78489eb3f7ff9e73ef2cc59fc442 has tree f57b916cfd706e4dd43974f8a656dbe92758756a; non-forced remote main checkpoint a97359808afb84c99a4b0d13b2ef67ababef9c17 has the same tree and parent 82285b1deabf2f27272861d0ea505c239ad1911d. ls-remote, fetch, and fetched-tree comparison all matched.

## Add-only Unity `.meta` sidecar filesystem primitive (2026-10-03)

- Added `src/export/project_output.rs` and exported the module from `src/export/mod.rs`. `write_unity_meta_sidecar` serializes a supplied `UnityMetaFile`, validates portable relative paths with byte/component bounds, creates directories only below a canonical output root, rejects linked/non-directory ancestors, and opens the sidecar with `create_new` so an existing file is preserved. The asset file and importer selection are deliberately outside this function.
- Four synthetic tests cover successful add-only output and sibling preservation, refusal to overwrite an existing sidecar, traversal/absolute/device/invalid-path rejection before writes, and configured path limits.
- Initial focused run exposed a Windows test-only path comparison (`\\?\` canonical path versus a non-canonical expected path); the test now canonicalizes the expected root. The four focused tests pass.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked --quiet` exit 0 (491 library, 23 CLI, 0 doc tests); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0.
- The first warnings-denied Clippy run found a needless explicit lifetime in the new helper; it was elided and the final Clippy run passed. No real Unity file, user AssetBundle, UABEA, or AssetRipper runtime differential was used. The Phase 1 gate remains open.
- Detailed continuation and the goal-versus-AGENTS.md comparison are in `docs/next-agent-handoff.md`.
- Cloud checkpoint for the implementation and first handoff revision: the previous local `main` (`5b95e616d36bbf4020576e697fa59f89a2d6b82f`) and live remote `main` (`1ff88beb73b4b634cdc6a957999cf844bbaa7467`) had identical tree `4984d90072e592c786dba30e4bbe07987eef3bfc` but diverged histories. `git merge-tree --write-tree HEAD origin/main` previewed that same conflict-free tree; local merge commit `090d35a71b140ccaba0df974a3e19c1d714641f3` reconciled ancestry without changing files. Implementation/handoff commit `b786f331b5947929386b11314bd09fefe5eb2d35`, tree `4ec2b2011e43de6bd2940dca35e01b82240ba0de`, was pushed normally after verifying remote ancestry. `ls-remote`, fetch, fetched commit/tree, and `git diff --exit-code HEAD FETCH_HEAD` all confirmed remote `main` at `b786f331...` with the same tree. This verification record is included in the current documentation-only follow-up, which is also pushed and verified; use `git log -1` for its revision.

## Export asset path and unique-name routing (2026-10-04)

- Source trace: IUnityObjectBase.GetBestDirectory/GetBestName/GetBestExtension preference order, ExportCollection.GetUniqueFileName suffix removal and character fixing, FileSystem.GetUniqueName truncation/collision/counter-cache rules with MaxFileNameLength 245 minus 5 for the .meta extension, FixInvalidFileNameCharacters/FixInvalidPathCharacters platform sets with TrimEntries, Windows-only IsReservedName, and Utf8Truncation sequence-boundary cuts.
- Added src/export/asset_path.rs with an ExportFileSystem probe and UniqueNameState cache so the live-filesystem rules are testable without disk access; wired as src/export/asset_path. GetExportExtension mapping stays out because it needs generated class schemas.
- Thirteen synthetic tests cover preference order, suffix removal, platform character sets, path trimming, reserved names, path helpers, UTF-8 truncation, conflict caching, pre-check truncation, and budget/probe/cache errors. Six initial expectation errors were fixed against the source (empty-OriginalName preservation, Unix invalid sets, Unix separator semantics, double-underscore path output, 234-byte stem budget, and U+03B2 code points).
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (504 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## AssetExportCollection path planner (2026-10-04)

- Added plan_asset_export_paths mirroring the path-selection prefix of AssetExportCollection.Export: fix best directory, join below the project directory, pick a unique file name in that subdirectory, join the file path, and expose a slash-normalized relative asset path for the add-only sidecar writer. Directory.Create, asset bytes, importer selection, GUID assignment, and .meta emission stay outside.
- One synthetic test covers the class-fallback join, a backslash directory with a colliding file (Cube.asset to Cube_0.asset with cache), and invalid-directory fixing before joining.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (505 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## Export collection routing table (2026-10-04)

- Audited every IExportCollection.Export override in the frozen baseline: Asset/Scene/Prefab/Manager/Script/EmptyScript/Unknown/Unreadable/Fail/Skip/Empty/Deleted/Redirect families. Added src/export/collection_kind.rs recording the ProjectExporter Exportable gate, .meta emission, fixed extension (unity/prefab/asset/cs/unknown/unreadable; Asset stays virtual), and collision rule (unique-name, fixed-path, skip-if-exists, overwrite, not-exported).
- Five table tests cover all 13 families. Notable source details preserved: Manager writes a fixed .asset path with no sidecar; Unknown/Unreadable resolve unique names under AssetRipper/UnknownAssets and AssetRipper/UnreadableAssets with no sidecar; Script skips the .cs write when the file exists while EmptyScript overwrites; both log instead of writing the sidecar when it exists; Fail warns and returns false.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (510 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## Script FileID hashing (2026-10-04)

- Added src/export/script_hashing.rs: dependency-free MD4 (RFC 1320) plus CalculateScriptFileID byte layout s,0,0,0 plus namespace plus name with little-endian i32 digest read and a 16 MiB default input bound.
- Three tests: MD4 vectors cross-checked with Windows certutil MD4 on this machine (memorized strings proved unreliable for three of seven inputs; the implementation agrees with certutil on all checked inputs), all five baseline NUnit FileID vectors with str/bytes consistency, and empty-namespace plus bound behavior. One initial test bug used byte_count 4 for a 2+4 byte input and is fixed to 6.
- CalculateScriptGuid and CalculateAssemblyGuid are explicitly not ported: UnityGuid.Md5Hash lives in the external primitives package outside the frozen snapshot with no vectors, so porting it would be guessing.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (513 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## Scene and manager fixed export destinations (2026-10-04)

- Added src/export/project_plan.rs: plan_scene_export_paths mirrors SceneExportCollection path selection ($"{Scene.Path}.unity" joined below the project directory with parent folder) and plan_manager_export_paths mirrors ManagerExportCollection (ProjectSettings directory, GetCorrectName table, fixed .asset name, never .meta). Duplicate-scene detection stays out because it needs the export container and BuildSettings.
- Four tests cover the .unity join, empty-path rejection with bare-name project-directory fallback, the PlayerSettings/129/NavMeshProjectSettings/PhysicsManager correction table, and fixed destinations on both platforms. A first warnings-denied Clippy run found a needless explicit lifetime on the directory helper; it was elided.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (517 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## Export GUID ownership policy (2026-10-04)

- Added src/export/guid_policy.rs: per-family GUID ownership (random per AssetExportCollection instance, preserved Scene.GUID with random substitute for zero, deterministic script hashes, predefined EGUID/FGUID/DGUID words, MissingReference sentinel role), stability across runs, snapshot versus external-package byte-layout boundary, and the SceneDefinition zero-substitution rule.
- Four tests cover the predefined constants, preserve/regenerate rule, stability table, and layout boundary. No random minting is implemented: the snapshot does not pin an entropy source, and the package-backed byte values are explicitly left open rather than guessed.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (521 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## Reference baseline build and test availability (2026-10-03)

- `dotnet --version` reports `10.0.401` on this workstation. Earlier handoff records stated `dotnet` was unavailable, so the reference build, reference tests, and C# differential were recorded as blocked. That blocker is cleared.
- The reference was copied to an isolated directory outside the read-only tree and never built in place: `Copy-Item -Recurse` of all 1,449 files into `%LOCALAPPDATA%\Temp\opencode\ar-ref`.
- `dotnet restore AssetRipper.slnx` exited 0 for all 51 projects from the snapshot's `nuget.config` feed `https://nuget.samboy.dev/v3/index.json`. `AssetRipper.SourceGenerated` 1.4.0 resolved, so the generated class package the snapshot only references is now available: 18,299 types, 3,439 concrete generated Unity asset classes (for example `ClassID_28.Texture2D_2017`), plus a 106 MiB XML member document.
- `dotnet build AssetRipper.slnx -c Release` exited 0: 32 warnings, 0 errors, 25m42s.
- `dotnet test` over the 8 test projects: AssemblyDumper.Tests 9/9, Assets.Tests 57/57, GUI.Web.Tests 6/6, IO.Files.Tests 137/137, Numerics.Tests 65/65, SerializationLogic.Tests 54/54, Yaml.Tests 11/11 all passed; AssetRipper.Tests reported 151 passed and 20 failed. Reference total 339 passed, 20 failed.
- All 20 failures are `DefaultYamlWalkerTests` CRLF/LF mismatches recorded as `DIFF-REFERENCE-TEST-NEWLINE-001`. Root cause verified two ways: the test builds `StringWriter { NewLine = "\n" }` while its expected raw string literals inherit the `.cs` file's CRLF endings, and `DefaultYamlWalkerTests.cs` was measured as 354 LF of which 354 are CRLF and 0 are lone LF. This is a checkout artifact, not a source defect, and it confirms the source writer's intended newline is LF, which `src/export/unity_yaml.rs` already emits.
- `tools/verify-baseline.ps1` exited 0 after the reference build: 1,449 files unchanged. The read-only reference was not modified.
- Not run and not claimed: Unity Editor import/runtime checks, reference GUI/web runtime export, and end-to-end reference export of any bundle. A reflection probe that tried to invoke the reference `GetExportExtension` switch over the generated types was abandoned because specialised export collections require constructor state; that does not block the port, because the switch is fully readable in the snapshot.

## Export extension table port (2026-10-03)

- Traced `ExportCollection.GetExportExtension`: 22 typed marker arms plus the `asset` default, evaluated as a first-match `is`-pattern chain. Added `src/export/export_extension.rs` and registered it in `src/export/mod.rs`.
- `ExportClassMarker` reproduces every arm with its exact source extension string, including the non-uniform casing the source actually uses: `texture2D`, `physicMaterial`, `physicsMaterial2D`, `webCamTexture`, `overrideController`. `EXPORT_CLASS_MARKERS` preserves source evaluation order so `ICubemap` precedes `ITexture2D` and `IAnimatorState` precedes `IAnimatorStateMachine` and `IAnimatorTransition`; `ExportClassIdentity::from_markers` resolves the winner by that order and retains the full satisfied set for auditing.
- This closes the gap that `src/export/asset_path.rs` documented: its `get_unique_file_name` previously required the caller to supply an extension because `GetExportExtension` was unportable without the generated schemas. The extension table is now available; class-ID-to-marker discovery from a live SerializedFile is still the caller's job and is recorded as the remaining difference on ledger row EXP-EXPORT-EXTENSION-001.
- Focused validation: `cargo test --locked --lib export_extension` exited 0 with 10 tests passing. `cargo fmt --all` was applied; an earlier warnings-denied Clippy run reported a `type_complexity` error at `project_order.rs:178`, but that file already used its `CollectionMembers` alias, so the finding came from a stale build and the clean re-run passed.
- Full validation: `cargo fmt --all -- --check` exit 0; `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `cargo test --workspace --locked --quiet` exit 0 with 535 library tests and 23 CLI tests (the previous recorded count was 491/23, so the worktree already contained further unrecorded increments); `tools/verify-baseline.ps1` exit 0 with 1,449 files unchanged; `git diff --check` exit 0.
- No user AssetBundle bytes and no UABEA content were read. Phase 1 remains open and Phase 2 remains locked.

## Project export ordering (2026-10-04)

- Added src/export/project_order.rs: plan_project_export_order mirrors CreateCollections fetch-order walk with queued-set dedup, exportable counting, and per-collection progress steps; classify_collection_outcome mirrors the Exportable gate with silent skip versus Failed-to-export warning. Asset bytes, exporter-stack selection, and event subscribers stay outside.
- Four tests cover ordering with dedup, non-exportable progress, outcome classification with inconsistent gate/result rejection, and both bounds. Two warnings-denied Clippy type-complexity findings on the test harness were fixed with a named struct and alias types.
- Reconciliation note: a concurrent session committed this file together with src/export/export_extension.rs as 41d16b9 and pushed it while this increment was being validated. The extension file arrived with two overreaching claims (generated-package provenance and a conflated audioclip/fsb/ogv attribution); both were corrected against ExportCollection.cs, YamlAudioExportCollection.cs, AudioClipExportCollection.cs, NativeAudioExportCollection.cs, MovieTextureAssetExportCollection.cs, and VideoClipExportCollection.cs before validation. This log section and ledger row EXP-EXPORT-PROJECT-ORDER-001 cover the ordering increment; the extension increment is recorded under EXP-EXPORT-EXTENSION-001.
- dotnet presence independently confirmed on this workstation (SDK 10.0.401 responds to dotnet --info). The concurrent reference restore/build/test report (51 projects, 339 passed, 20 CRLF-artifact failures) and the isolated copy at Temp opencode ar-ref are noted but pending independent re-verification; no reference binaries were executed from this session.
- Final verification of HEAD 41d16b9 on Windows with Rust/Cargo 1.98.1, run from this session: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (535 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## Exporter registration matrix (2026-10-04)

- Audited every OverrideExporter call in ProjectExporter.Overrides.cs (53 constructor plus 2 DoFinalOverrides) with target, exporter, allowInheritance flag, and settings gate. Added src/export/registration.rs with the 55-entry source-order table and a per-target query helper.
- Five tests cover total order with engine/deleted finals last, ITexture2D TerrainYamlExporter shadowing, shader Yaml/Dummy plus Simple and audio triple pairs, the seven build-data Skip dummies plus Unknown/Unreadable real-or-Skip pairs with empty/meta arguments, and catch-all/group positions. An initial count assertion used 52 from a hand miscount; the table holds 55 and the test now asserts that.
- A lifetime elision Clippy finding on the query helper was fixed with an anonymous lifetime bound. Dummy exporter Empty-versus-Skip and Meta-versus-Serialized semantics were verified in DummyAssetExporter.cs.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (540 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. The isolated reference copy shows restore artifacts only with no test binaries, so the reported reference build/test outcome stays pending independent re-verification. No Unity-produced file, user AssetBundle, UABEA, or C# runtime differential was used. Phase 1 remains open.

## MD4 reference-implementation differential (2026-10-04)

- Built an isolated dotnet probe at Temp opencode md4diff (net10.0 console, direct DLL reference to the restored AssetRipper.HashAlgorithms 1.0.0 netstandard2.0 binary, no restore needed, no bundle or reference-tree access) that runs the baseline MD4.HashData over nine inputs. Overlapping vectors agree with the Rust port exactly, including the previously certutil-corroborated message-digest value.
- Added md4_matches_reference_implementation_vectors: 55-byte (one-block) versus 56-byte (two-block) padding boundary, a full 64-byte block, a 1000-byte patterned multi-block input, and the real s+nulls+UnityEngine+Component script preimage, all with reference digests, plus a preimage FileID cross-check against the NUnit value 468326297. The preimage digest head independently reproduces that NUnit expectation.
- A test-authoring slip replaced the NUnit test header instead of adding a new test; the header was restored and the file verified to contain all four tests before the focused run. All four focused tests pass.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (541 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. The isolated reference copy still shows restore artifacts only with no test binaries, so the reported full reference build/test outcome stays pending independent re-verification. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Processing pipeline order (2026-10-04)

- Audited every yield step of ExportHandler.GetProcessors (11 assembly plus 10 asset processors) with phase, gate, and export-mode capture. Added src/processing/pipeline.rs with the 21-entry source-order table and a per-phase query helper.
- Three tests cover total order with the phase split at index 11, the LightingData position after the empty static-mesh-separation slot and before Prefab, the three settings gates, and the OriginalPath/EditorFormat export-mode capture.
- Independently executed the reference AssetRipper.Numerics.Tests from the isolated copy (dotnet test, Release): 65/65 passed, matching the reported per-project count and confirming the isolated restore/build path works from this session. Full eight-project re-verification stays open.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (544 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Post-export outputs (2026-10-04)

- Audited all five GetPostExporters yields with destinations, skip conditions, and content rules. Added src/export/post_export.rs with the source-order kind table, the ProjectVersion body builder, the 31-module default manifest builder with its 2019.2 androidjni gate, and the four PathIdMap asset kinds.
- Four tests cover order/destinations/conditionals, exact version bodies including the Unity 5 extra line, the manifest gate, and the asset-kind filter.
- Independently executed four more reference test projects from the isolated copy (dotnet test, Release): IO.Files 137/137, Yaml 11/11, SerializationLogic 54/54, Assets 57/57, all passing. Combined with the earlier Numerics 65/65, this session has independently verified 324 reference tests with 0 failures; only AssemblyDumper, GUI.Web, and the large AssetRipper.Tests project remain unrun from this session.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (548 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Product settings surface (2026-10-04)

- Audited ImportSettings, CoreConfiguration, ProcessingSettings, ExportSettings, FullConfiguration, and twelve enum files with every default. Added src/export/settings.rs with the three default records, all option enums, the snapshot-pinned AutoSafe/AutoExperimental discriminants, and the fully snapshot-determined ImageExportFormat selection (GetFileExtension, TryGetFromExtension with jpg, prefer-original over GetBestExtension).
- Four tests cover all defaults, extension mapping with case sensitivity, original-extension preference, and the dead TerrainExportMode finding: the mode is declared in export configuration yet consumed nowhere outside a GUI dropdown, since no ExportSettings field holds it and no exporter reads it.
- Independently executed the last three reference test projects from the isolated copy (dotnet test, Release): AssemblyDumper 9/9, GUI.Web 6/6, and AssetRipper.Tests 151 passed with 20 failed, all 20 in DefaultYamlWalkerTests. The CRLF root cause was independently re-measured on the read-only reference file (354 CRLF sequences, 0 lone LF), confirming the checkout artifact. Full eight-project reference total from this session: 490 passed, 20 failed on the artifact, 0 other failures.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (552 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Importer field values (2026-10-04)

- Added src/export/importer_fields.rs with only snapshot-determined importer behavior: the eight-importer AssetBundleName propagation with the source null-only guard, GenerateTextureImporter unconditional literals, OnAssemblyExported Any/Editor platform entries, the PrefabExportCollection legacy/modern branch, and the MonoImporter i32-to-i16 wrapping cast.
- Five tests cover propagation including empty-string preservation, truncation wrapping at i32 extremes, branch selection, every texture literal, and every plugin platform entry. The Has version gates were confirmed absent from the snapshot (only three handwritten importer extension files exist there); their thresholds live in generated code and stay open rather than guessed.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (557 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Diagnostic and script collection destinations (2026-10-04)

- Added src/export/collection_paths.rs: verbatim unknown destinations with static unique-name resolution, path-fixed unreadable destinations, and script sub-paths from ordinal assembly stripping, firstpass folder mapping, platform namespace separators, and fixed class names, all joined below Assets with slash-normalized relative paths.
- Five tests cover verbatim-vs-fixed naming asymmetry, collision suffixes, firstpass Plugins folders, empty-namespace folders, and ordinal case-sensitive extension stripping.
- Two warnings-denied Clippy findings were fixed: a private cross-module separator made pub(crate), and an eight-argument helper regrouped into a request struct.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (562 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Web product surface (2026-10-04)

- Audited the actual user-facing product: a single ASP.NET web app (GUI.Free only launches it) with POSIX arguments, 46 routes, and five post commands. Added src/product.rs with launch defaults, the full route table in registration order, command redirect rules, local-web-file prefixes, and the log/export name builders.
- Four tests cover defaults (booleans at compile time), route counts with per-command POST coverage and the Model.glb outlier, redirect rules, and name builders. An initial miscount of AssetAPI endpoints (10, not 11) was caught by the count test and corrected in the test, not the table.
- A warnings-denied Clippy assertions-on-constants finding moved the boolean default checks into compile-time asserts.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (566 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Loader workflow (2026-10-04)

- Extended src/product.rs with the GameFileLoader state machine: reset logging only when loaded, ordinal empty/protected directory rejection (desktop exports while Desktop does not), headless auto-consent with dismissed-dialog decline, and the five-state export flow shared by both export commands.
- Four tests cover each rule. The PrimaryContentExporter registration table and SingleExportCollection flow were read alongside (Json default, 20 empty handlers, Glb/Binary/Audio/Texture/Script handlers, Deleted last; Add-first dedup with empty-skip; ExportRootPath root; full-length unique names; no .meta) and are recorded here as the traced next increment rather than implemented.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (570 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Primary-content pipeline (2026-10-04)

- Added src/export/primary_content.rs: the 36-entry default handler table (Json catch-all, 20 empty handlers, Glb/Binary/Audio/Texture/Script handlers, Deleted tail), the Add-first dedup with empty-skip assembly delta, the ExportRootPath root with one-based progress lines, and SingleExportCollection destination planning with the full name budget and no sidecar.
- Three tests cover the table split with binary/model shadowing, progress lines, and full-budget paths. A lifetime elision Clippy finding on the query helper was fixed with an anonymous lifetime bound.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (573 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Project coverage backbone (2026-10-04)

- Added docs/project-coverage.md: all 51 solution projects with capability, Rust counterpart, and honest status (libraries 3 investigated, 8 in progress, 11 uninvestigated; 15 tools inventoried by entry point; 8 test projects as executed evidence; 6 generators with outputs as audit target). Capability lines for unexamined projects come from project file listings, not behavior audits.
- A summary miscount (7 vs 8 in-progress libraries) was caught on recount and corrected. This document is the gate backbone, not the gate: every uninvestigated row still needs behavior-ledger evidence.

## Managed serialization types (2026-10-04)

- Added src/serialized_file/managed_types.rs: the 17-variant PrimitiveType with size/system-name/C# rules, ECMA-335 element-code mapping with Complex fallback, Field shape with array/display rules, MaxDepth rules including sticky unknown, all six ManagedReferenceTypes registry shapes with every field constant, and the engine predicate tables with direct-comparison rules.
- Six tests cover every table. An initial variant-count assertion used 19 from a miscount; the enum holds 17 and the test now asserts that. Cecil-backed resolution, assignability, and registry payload decoding stay open by necessity.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (579 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## CreateImporter dispatch table (2026-10-04)

- Extended src/export/importer_fields.rs with the full `CreateImporter` dispatch found by source search: the `AssetExportCollection<T>` base default (`NativeFormatImporter` with the main export ID plus bundle propagation) and all nine overrides (audio, font, text, video, prefab, shader, lightmap, texture-array, texture), each mapped to its importer kind and factory shape. The texture convert flag selects `GenerateTextureImporter` versus `GenerateIHVImporter`, and the unconditional font/video/IHV/platform literals are pinned by name without claiming generated enum numerics.
- Six tests now cover the module: the five pre-existing importer-field tests plus a new table test asserting all 10 dispatch rows, the factory-only texture triple, the prefab modern row, the convert-flag branch, and every fixed literal. Asset-copied fields, generated `Has_*` gates, enum numeric values, `MainObjectFileID` wiring, and YAML writing stay outside by necessity.
- A warnings-denied Clippy assertions-on-constants finding on three boolean literal asserts was fixed by binding each constant to a local before asserting, matching the codebase's runtime-test style.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (580 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Meta emission sites and collision plan (2026-10-04)

- Added src/export/meta_emission.rs: all five product `.meta` emission sites from a whole-`Source` `new Meta(` search (the two other hits build an unrelated HTML `Meta`), each with its GUID source, importer kind, asset-first order, and collision rule; the shared `ExportMeta` mechanics (`.meta` suffix, UTF-8 without BOM, no default tag/version, formatted keys); and `plan_meta_write`, which connects each rule to the add-only sidecar writer without authorizing overwrites.
- Key source finding: no snapshot caller passes `isFolder: true`, so the `folderAsset` envelope branch is never emitted and folder sidecars are recorded as unwritten rather than invented. Four sites overwrite via `File.Create` (kept visibly unwired as `BlockedOverwrite` when a sidecar exists); only the per-script path skips with an error, which the add-only writer matches exactly. Extended `CreateImporterKind` with the verbatim `Default`, `Mono`, and `Plugin` importer names for the scene/script/assembly sites.
- An early audit note is corrected here: the prior `CreateImporter` search used a non-recursive PowerShell glob, but a proper recursive re-search confirms the same 10 declarations, so EXP-IMPORTER-CREATE-001 stands as written.
- Four tests cover the five-site table with the single skip rule, asset-first order with no folder rows, the full collision truth table, and every writer mechanic.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (584 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Export pointer types (2026-10-04)

- Added src/export/export_type.rs: the snapshot `AssetType` with fixed numerics (`Internal = 0`, `Cached = 1`, `Serialized = 2`, `Meta = 3`); all 30 concrete exporters mapped to their `ToExportType`/`ToUnknownExportType` answers by base class (12 YAML exporters to `Serialized`, binary exporters including both shader exporters to `Meta`, four direct implementations to `Meta`, engine assets to `Internal`, deleted declining both); the parameterized dummy answer; first-accepter stack resolution with an explicit failure; and `MetaPtr` rendering with the local `{fileID}` form, the Unity 3 `Meta`-to-`Cached` YAML redirect (display keeps the true type), and the prefab legacy/modern branch.
- Four tests cover numerics with round trip, every table row by group, stack skip/fail behavior, and pointer forms. Two warnings-denied Clippy findings were fixed in sequence: a manual-flatten loop rewritten per the suggestion, then the suggested loop rewritten again as a `flatten().next()` check after a never-loop finding.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (588 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Exporter collection claims (2026-10-04)

- Added src/export/collection_claims.rs: every concrete exporter's `TryCreateCollection` predicate over caller-supplied facts, with exact collection-type outcomes including the mixer never-declines (fail-collection fallback), the sprite-atlas empty branch, the component-on-GameObject empty branch, the stateful script exporter (first creates, later redirects), the engine redirect/decline order, and the audio decode-to-wav-override chain plus `IsSupportedExportFormat` and `HasDecompiledShaderText` pure rules.
- This increment also corrects last turn's EXP-EXPORT-TYPE-001: `EngineAssetsExporter.ToUnknownExportType` sets `Internal` but returns false, so the type table now records `to_export_type` and `to_unknown_export_type` separately (engine: `Some(Internal)`/`None`; deleted: `None`/`None`) with updated tests.
- Six claim tests plus the four updated type tests pass. Three warnings-denied Clippy findings were fixed: a `&str` helper bound tightened to `&'static str`, a match-like-matches macro rewritten with `matches!`, and two earlier loop-shape findings in the type module.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (594 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Scene YAML member order (2026-10-04)

- Added src/export/scene_member_order.rs: the `SceneExportCollection.Compare` display comparator (equal classes equal, managers by class ID, managers before non-managers, non-manager pairs equal) plus a stable-sort helper, since the baseline snapshots `ExportableAssets.Order(this)` (stable LINQ `Order`) before writing scene YAML. The workspace planner's documented no-sort behavior is unchanged; callers compose the two.
- Two tests cover every comparator arm and a mixed six-member stable sort. No Clippy findings.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (596 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Diagnostic collection references (2026-10-04)

- Added src/export/diagnostic_pointers.rs: the Skip/Fail/Empty export outcomes (throwing versus warning-plus-false), the non-local missing-reference pointer mapping that never nulls references, the local-reference and empty-collection errors, and the family display-name and membership rules. Routing (exportable/meta/extension/collision) stays in collection_kind.rs.
- Three tests cover outcomes, pointer mapping with both exporter types, and names with membership. No Clippy findings.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (599 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Redirect collection mechanics (2026-10-04)

- Added src/export/redirect_collection.rs: the multi-redirect add guards (nonzero file ID, real GUID, no duplicates), redirect-then-missing member order, the single-member best-name rule, missing-first pointer resolution with redirection-only export IDs, never-local pointers, and the single-redirect identity membership with its fixed pointer. Members are caller-supplied keys preserving insertion order.
- Four tests cover guards, ordering with name branches, resolution scope with errors, and single-redirect rules. A non-const `Vec` indexing helper and two stale call sites were fixed during the build; no Clippy findings remain.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (603 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Multi-asset collection mechanics (2026-10-04)

- Added src/export/multi_asset.rs: the `AssetsExportCollection<T>` gathering rules (primary/duplicate refusal, count-seeded pseudo-random IDs, stored member IDs with stranger failure, insertion-order enumeration) with the three `GenerateExportID` schemes (default pseudo-random, texture stride-2 main IDs, font plain main IDs), reusing `ids.rs` arithmetic exactly.
- Four tests cover refusal with seed-0/seed-1 IDs, membership, the 21_300_000/21_300_002 stride pair, and shared font IDs. A `Hash` derive that `SceneUnityVersion` cannot satisfy was removed from the scheme enum.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (607 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Scene helper rules (2026-10-04)

- Added src/export/scene_helpers.rs: the `SceneHelpers` index/file-name rules with the 5.3 main-data gate and strict `level` tail shape, exact-path duplicate detection with explicit missing/out-of-range outcomes, full collection-to-path mapping (prefix/rooted/bare/stale/empty branches), and the scene-compatibility switch.
- Five tests cover the gate, round trips, duplicates, every path branch, and compatibility. Two test-fixture bugs were caught by the implementation (single-entry lists with `level1`, a dropped `#[test]` found by Clippy dead-code); both fixed in the fixtures, not the rules.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (612 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Project container mappings (2026-10-04)

- Added src/export/project_container.rs: the `ProjectAssetContainer` ownership map with double-claim refusal, collection-then-main export-ID resolution, current-collection locality for pointer delegation, and first-match scene GUIDs with a zero default. Missing-asset pointers reuse the explicit-`Meta` rule from `export_type.rs`; duplicate-index checks reuse `scene_helpers.rs`.
- Four tests cover construction, ID resolution, locality, and scene GUIDs. No Clippy findings.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (616 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Empty-script export (2026-10-04)

- Added src/export/empty_script.rs: verbatim dummy templates (global + namespaced) with generic `Name<T1, T2>` expansion, the arity regex with int-parse tail, script identity with injected detection, first-wins dedup with injected skip, the six predefined assemblies, and the 2017.3 definition gate.
- Byte fidelity was verified mechanically: both rendered outputs (2698 and 2768 bytes) match a live reconstruction from the frozen `.cs` bytes exactly. The check caught three paste-added tabs, fixed in the arrays. Whitespace-only template lines render empty per an isolated `dotnet run` raw-string probe; the throwaway dump example and probe were deleted afterwards.
- Five tests cover arity, templates with CRLF purity, markers, write plans, and gates. Four Clippy/build findings fixed (two const-fn limits, crate name, matches! macro).
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (621 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Managed script export flow (2026-10-04)

- Added src/export/script_export_flow.rs: the ordered `GetExportType` check chain with all four settings modes, the pointer triple with the checked 11_500_000 decompiled file ID (MonoScript = class 115 from the `ClassID_115` imports), constructor dedup with export filtering, write-once per-script guards, ordinal `.dll` appending, and `.asmdef` bodies with reference filtering and the shared 2017.3 gate.
- Five tests cover check order, pointer routing, dedup/filtering, guards, and definition bodies. No Clippy findings.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (626 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Manager-family export flows (2026-10-04)

- Added src/export/manager_flows.rs: the manager fixed ID 1 with throwing pointers and always-true export, editor-build-settings GUID stamping reusing the scene path/container lookups, and scene-asset naming with caller-parameterized IDs and locality-ignoring pointers.
- Three tests cover IDs/pointers, stamping, and scene-asset rules. Two findings fixed: `!`-typed returns are unstable (used `Infallible`), and a needless borrow on the extension strip.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (629 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Transfer instruction flags (2026-10-04)

- Added src/serialized_file/transfer_flags.rs: all 32 `TransferInstructionFlags` bits with snapshot values and summaries, the four `Is*` predicates, and the prefab-collection OR rule. A missing top bit in the table was caught by the bit-exactness test and fixed.
- Three tests cover values, predicates, and the prefab rule. No Clippy findings.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (632 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Deleted-asset collection (2026-10-04)

- Added src/export/deleted_collection.rs: record-plus-deleted membership with append order, the fixed collection name, throwing export/IDs, the verbatim reference warning, and class-ID/stack-type pointer resolution that ignores locality unlike Skip/Fail.
- Three tests cover membership, throwing paths, warnings, and pointers. One unstable `!`-type return was replaced with `Infallible`.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (635 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Resource extension selection (2026-10-04)

- Added src/export/resource_extensions.rs: texture format/extension preference selection, text dispatch with the dangerous set and JSON/plain/binary sniffing (hand-rolled grammar with the 64-deep default bound), font `OTTO` magic with `ttf` fallback, video path extension with `bytes` fallback, and best-extension coalescing.
- Six tests cover every branch. Two fixture findings fixed in tests/rules: leading-zero numbers rejected by the grammar, and a palindromic magic test vector replaced with a near-miss.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (641 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Prefab exportable sets and pointers (2026-10-04)

- Added src/export/prefab_export.rs: first-wins gathering with hierarchy-self append, hierarchy-order exportable sets with file tracking, and pointer composition from class IDs, GUIDs, and legacy branches.
- Three tests cover pointers, gathering with duplicates, and exportable sets. No Clippy findings.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (644 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Exporter registration re-audit (2026-10-04)

- Re-audited `ProjectExporter.Overrides.cs` call-by-call against the 55-row table: order, inheritance flags (including the two non-inheriting real exporters and inheriting Skip dummies), gates, and final overrides all match; the earlier "53 + 2" count confirmed by recount.
- Added the two remaining selection rules to registration.rs: the lightmap Exr-vs-configured image format mapping and the stored-vs-embedded engine source selection, with a new test.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (645 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Ledger report tool and cleanup (2026-10-04)

- Added tools/parity-report.ps1: parses the feature-parity table by closed status vocabulary (robust to pipes inside fields), reports tallies, non-verified IDs, duplicate IDs, and malformed rows as UTF-8 JSON, and exits 1 unless every row is verified. The script file carries a UTF-8 BOM so Windows PowerShell 5.1 decodes the vocabulary literals; without it every CJK comparison silently fails (diagnosed via byte-level comparison after a false malformed flood).
- First trustworthy run: 246 rows = 144 verified + 59 implemented-unverified + 14 implementing + 13 uninvestigated + 16 `???` placeholders, plus 1 duplicate ID and 1 glued row pair, gate OPEN. Fixed all three ledger defects: 17 `???` status cells (each with full verification evidence in its row) normalized, the byte-identical duplicate `EXP-YAML-GUID-001` row removed, and the glued `EXP-META-SIDECAR-FS-001`/`EXP-EXPORT-PATH-001` line split. Re-run: 160 verified, 86 open, 0 placeholders/duplicates/malformed, gate still OPEN, exit 1 as designed.
- No Rust source changed, so no cargo runs were needed for this increment. `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Rotation and vector math with runtime differential (2026-10-04)

- Added src/numerics.rs: Euler/quaternion conversions with Unity sign conventions, indexed quaternion access, dot/unit/zero/near-equality predicates, vector normalize/dot/near-equality, X inversion, and three-point clockwise angles.
- Ran a genuine original-runtime differential: an isolated net10.0 probe (Temp `numdiff`, referencing the prior isolated build's `AssetRipper.Numerics.dll`, no bundles read) evaluated 12 euler vectors with round trips plus identity/radian/half quaternions, 4 angle cases, normalization, and dot predicates as hex float bits. All 35 lines match the Rust port bit-exactly except `Normalize`, which is a confirmed source no-op (by-value struct, zero snapshot callers) while Rust returns true unit vectors.
- Six tests include the pinned probe vectors and reference round-trip cases. Findings fixed along the way: an FMA-vs-multiply transcription slip, a wrong `EqualsByDot(q,-q)` expectation (dot is -1 by definition), and two unread matrix entries now underscore-marked.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked --quiet exit 0 (650 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Transformation math with runtime differential (2026-10-04)

- Extended src/numerics.rs with `Vec4`, row-major `Mat4`, and `Transformation`: scale/translation/quaternion-rotation creation, row-vector multiplication, affine point transforms, transpose, fourth-row translation removal, component inverses with zero-rotation-to-identity fallback, plus `Quaternion::inverse` and `Quaternion::create_from_yaw_pitch_roll`.
- Ran a genuine original-runtime differential: an isolated net10.0 probe (Temp `transdiff2`, referencing the prior isolated build's `AssetRipper.Numerics.dll`, no bundles read) dumped hex float bits for yaw/pitch/roll construction, quaternion inverses, all rotation/scale/translation creators, four composed products, identities, affine point transform, full and component inversions, transpose, translation removal, and singular-matrix failure. The Rust test pins the bit-exact cases and compares transcendental/inverse cases within 1e-6/1e-5; one probe surprise is now documented in code (the source point transform is affine and ignores the projection column).
- Clippy findings fixed: `Mat4::mul` carries an explicit allow explaining the source `*` mapping, and 2D cofactor loops carry scoped range-loop allows because cofactor indexing needs explicit row/column pairs.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (651 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Numerics value types with runtime differential (2026-10-04)

- Extended src/numerics.rs with `Vector2i`/`Vector3i` (mod-wrapped member lookup with fall-through-to-Z, throwing member-by-value search), `BoneWeight1`/`BoneWeight4` (exact-sum normalization with uniform fallback), and `Color32`/`ColorFloat` (little-endian packing, truncating/saturating byte conversion, component arithmetic, black/white constants, display strings).
- Ran a genuine original-runtime differential: an isolated net10.0 probe (Temp `valuediff2`, referencing the prior isolated build's `AssetRipper.Numerics.dll`, no bundles read) dumped RGBA words, conversion bits, ten byte-mapping edges, seven clamp lanes, arithmetic words, weight normalization/flags/strings, and integer member rules. Three findings fixed during porting: the ARGB expectation (alpha-first `0x280A141E`, not the RGBA word), NaN clamp propagation (predicate, not bits, since .NET uses `0xFFC00000` and Rust `0x7FC00000`), and `f32::clamp` confirmed as the correct NaN-propagating match for the source min/max chain.
- Clippy findings fixed: scoped allows for the 8-argument source-mirroring weight constructor, the source-named add/sub methods (operator traits are also implemented), and reference-exact test decimals.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (652 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Numerics range algebra with runtime differential (2026-10-04)

- Added generic `Range<T>`/`DiscontinuousRange<T>` to src/numerics.rs: half-open containment, strict ordering, intersection, adjacency-aware union, ordered first-union/last-union merging, two-pointer intersect/subtract, bounded negate, continuous conversion, and display. Bounds are `PartialOrd + Copy` so future version-gating instantiations (e.g. Unity versions) fit; NaN follows IEEE partial order rather than the .NET total order (reference suite has no NaN cases).
- Ran a genuine original-runtime differential: an isolated net10.0 probe (Temp `rangediff2`, referencing the prior isolated build's `AssetRipper.Numerics.dll`, no bundles read) confirmed three predicted quirks empirically: multi-member `Negate` throws `ArgumentException`, subtracting a strictly lesser/greater cutter yields empty, and trailing members drop once the cutter list exhausts. All three are replicated exactly and pinned in tests — not silently fixed, since no production caller exists and any future ported caller must observe baseline behavior; flagged for upstream instead.
- One implementation bug caught by tests: the `DiscontinuousRange` display separator flag never flipped (emitted `"0 : 34 : 57 : 9"`); fixed and pinned. One structural fix: split unbounded/bounded impl blocks so display and emptiness need no ordering bounds.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (655 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Numerics distance metrics with runtime differential (2026-10-04)

- Added `Vec2`, `Vec3`/`Vec4` distance ops, `Mat4::as_array`, `Vec4::as_vec3`, and the `relative_distance` module to src/numerics.rs: scalar/squared, 2D/3D/4D vector (+squared 2D), color-via-vector, four-lane weight mean, sixteen-lane matrix mean, and one checked generic averaging core covering all seven source array overloads. The source has no production callers; the port preserves exact formulas for future semantic comparison work.
- Ran a genuine original-runtime differential: an isolated net10.0 probe (Temp `distdiff2`, referencing the prior isolated build's `AssetRipper.Numerics.dll`, no bundles read) pinned eleven words reproduced bit-exactly, including opposite-sign saturation at 1.0, the 1/9 squared-vector ratio, matrix identity-vs-scale at 1/16, and the array sum/count pair.
- Reference property tests were mirrored deterministically (no randomness): self-zero, nonnegativity, bit-exact symmetry, triangle inequality over a fixed -2..2 fixture, exact cross-dimension equality, and zero handling — plus a mismatch-length error case. `AsVector3` was traced to an out-of-snapshot definition, so only its lane-drop behavior is ported; array mismatches are explicit errors where the source only debug-asserts.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (656 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Importer field-read matrix (2026-10-04)

- Added src/export/importer_reads.rs: per-path asset-field read tables for all ten `CreateImporter` rows (audio 11, font 10, video 7, shader 2, texture-2D 10, texture-array 7, cubemap-array 7, texture-3D 9→8, IHV 5, prefab 1), each with the snapshot gate name and the fed assignment. Pure selectors ported exactly: `CalculateMaxTextureSize` (round-up-pow2 with wrapping zero plus pre-clamp signed cast), cubemap/NPOT/texture-type branches, and the `AddSprites` three-way plan.
- Three findings pinned rather than normalized: IHV and modern-prefab bundle copies skip the `Has_AssetBundleName_R` gate on a bare null check; the cubemap-array path passes `Width` twice as written; float-to-int casts truncate for sane values with NaN/overflow edges left open. A row-count slip in the new test (8 vs 9 for the 3D table) was fixed against the table itself.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (660 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Exporter base dispatch mechanics (2026-10-04)

- Added src/export/exporter_bases.rs: the `IAssetExporter` default routing (callback-less overloads forward with null callbacks; callback-taking twins throw), `BinaryAssetExporter` decisions (all shapes throw except single-with-callback running export-then-callback), `IsValidData` null/empty rules, always-true plain-collection creation, `YamlExporterBase` document-vs-stream framing with the exact multi-callback rejection message, and the `DummyAssetExporter.Get` four-way matrix with shared-empty vs fresh-skip collection shapes.
- Two transcription slips fixed along the way: a missing paren in a test assertion and a clippy constant-assertion finding resolved by modeling collection creation as a function instead of a bool constant.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (664 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Audio export decisions (2026-10-04)

- Added src/export/audio_export.rs: `AudioExportFormat` with the Default/PreferWav support gate, byte-level decoder dispatch (empty/FSB5/tracker magics with exact offsets and bounds, seven MOD words), both raw-extension tables by enum name, printable/hex diagnostic builders, the clip TryCreate refusal matrix with wav-for-ogg substitution, the export-time conversion trigger, native empty-data refusal with sound-type-first extension choice, and the YAML `.resS` staging discipline.
- Two findings fixed along the way: a test closure lifetime error (rewritten as an inner function) and two clippy lints (range-contains form, useless vec).
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (670 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Exporter stack order and winner resolution (2026-10-04)

- Added src/export/exporter_match.rs: `ObjectHandlerStack` LIFO try-order with the exact-type-or-inheriting filter and the clear-on-override cache rule, the shader winner chain (Simple first on decompiled text, then the mode exporter with Decompile falling to Dummy), the pure `has_decompiled_shader_text` gate, and the audio winner chains per format (converted-first under supported formats, native-first under Native, YAML everywhere for clips, nothing for non-clips).
- Removed a duplicated `AudioExportFormat` enum from audio_export.rs in favor of the existing settings type (identical variants); repaired the doc-comment split that move caused.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (674 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Shader export text paths (2026-10-04)

- Added src/export/shader_export.rs with vendored shader_data texts (fallback, two YAML patches, four template bodies): YAML `asset` extension, 2018.1/3.5 patch gates with verbatim texts and skip-if-exists destinations, CR-stripping loaders, the four-entry template catalog with count-based selection, dummy header-slice and parsed-form assembly, and property/flag/dimension keyword tables.
- Genuine original-runtime differential for the fallback literal: reflection dump from the isolated prior build gave 317 bytes; a first naive transcription missed the C# raw-string closing-delimiter dedent rule (39 bytes), and the corrected mechanical extraction now reproduces the runtime value byte-exactly (`fc: no differences`). Patch texts use the same verified rule (const evaluation is deterministic); template JSON stays a hardcoded table with bodies vendored verbatim.
- Two test-expectation slips fixed (fallback boundary lines) plus a const-fn tuple-comparison error (plain fn instead).
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (678 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. Vendored bytes: fallback_raw 337 (d7647fc4…), register patch 1154 (1a96c984…), file-locker patch 1511 (4974bb5c…), probed fallback 317 (8a89a3d9…). No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Vendored shader bytes line-ending fix (2026-10-04)

- Post-push audit caught a line-ending hazard: the workspace `.gitattributes` (`* text=auto eol=lf`) normalized the four vendored template bodies to LF in the committed blobs (692->655 bytes etc.), while the fallback and patch blobs kept their CRLF. Runtime behavior was unaffected either way (templates are CR-stripped at load like the source; patches need their CRLF and kept it), but committed bytes no longer matched the snapshot sources.
- Fix: `.gitattributes` now marks `src/export/shader_data/*.txt` and `templates/*.txt` as `-text` (no conversion either direction). A stale stat cache hid the change from plain `git add`; `git add --renormalize` forced re-evaluation and staged the CRLF template blobs.
- Proof: all four vendored templates are byte-identical to the snapshot originals with matching SHA-256 (Default 350a00a1…, SingleColor b6c0d27b…, SingleTexture adfc2606…, TransparentDiffuse a9244140…); fallback/patch blobs were already identical. No Rust source changed, so no cargo rerun was needed for this fixup. `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. Phase 1 remains open.

## Audio mixer export paths (2026-10-04)

- Added src/export/mixer_export.rs with vendored audio_mixer_patch.txt (3199 bytes, SHA-256 883fe386…, same verified mechanical extraction as the shader texts): TryCreate routing on the mixer-controller main with always-true creation, constructor gathering reads in source order, stride-two export-ID stepping, the base-table `mixer` extension, and unconditional patch application through the shared skip-if-exists mechanics.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (681 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Collection admission matrix (2026-10-04)

- Added src/export/collection_admission.rs: `TryCreateCollection` admission for all 25 concrete exporters as pure caller-fact decisions, plus scriptable-object guiskin/brush extensions, the font multi-export single-font rule, unknown multi-export AND-accumulation, `DeletedAssetsExporter` throwing type queries, and the dead `TerrainExportMode` setting (GUI-only, always resolves to YAML).
- Three modeling slips fixed before testing: a reused text-script fact for the lightmap arm (now a dedicated lighting-data fact with its own collection kind), a punted text arm (now admitted via facts), and a wrong Default-format audio expectation (ogg kept, wav only under PreferWav).
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (686 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Script exporter admission state (2026-10-04)

- Added src/export/script_admission.rs: the `ScriptExporter.TryCreateCollection` flag state machine (first script sets `HasDecompiled` and selects full/empty collections by assembly availability; later scripts redirect; non-scripts refuse), the export-set gate (decompile plus manager presence), a first-wins dedup registry, and the empty-overwrite versus full skip-existing file discipline split.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (690 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Path-id map post-exporter (2026-10-04)

- Added src/export/path_id_map.rs: the fifth post-exporter `PathIdMapExporter`, rendering `{AuxiliaryFilesPath}/path_id_map.json` with the measured `System.Text.Json` (`WriteIndented = true`, source-generated context) layout — per-`SerializedAssetCollection` files in source order, `Mesh`/`Texture`/`AudioClip`/`TextAsset` assets carrying `PathID`, `INamed.Name`, and `ClassName`, CRLF line breaks, uppercase `\uXXXX`, `null`/`[]` handling, and declaration-order properties.
- Genuine original-runtime differential: an isolated net10.0 probe (`Temp/opencode/stjprobe`) rendered the same model through `SerializedGameInfoSerializerContext`; its bytes (703 for the main case) are the Rust test's embedded expected value. Two further probes measured the exact default-encoder escape set for ASCII 0x20..0x7E, DEL, U+0080/U+00A0/U+2028/U+2029, an astral emoji, and all C0 controls, so no escaping rule was guessed.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (696 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## YAML scalar quoting reference-runtime differential (2026-10-04)

- Added `unity_yaml::tests::scalar_quoting_matches_the_reference_yaml_runtime`: a 48-case table of exact documents produced by the isolated net10.0 `AssetRipper.Yaml.dll`. The probe (`Temp/opencode/yamlemitprobe`) creates `YamlScalarNode.Create(value)` under a `k` key with `IsWriteDefaultTag = false`, `IsWriteVersion = false`, `IsFormatKeys = true`, and a `\n` newline, which is the same configuration as `write_unity_yaml_mapping`; its output is `Temp/opencode/yamlemitprobe/yaml_doc_probe.json`.
- The differential found and fixed two real divergences in the Rust emitter:
  - `DIFF-YAML-DASH-WHITESPACE-001`: `string_needs_quotes` used `starts_with("- ")`; the source regex `(^-\s)` also matches a `-` followed by non-control Unicode whitespace (U+00A0, U+2003), so the Rust predicate now checks `char::is_whitespace`.
  - `DIFF-YAML-EMPTY-WHITESPACE-001`: `write_str` unconditionally flushed deferred whitespace, while the source `Emitter.Write(ReadOnlySpan<char>)` skips `WriteDelayed` for an empty span; an empty plain value is now `k:\n`, not `k: \n`.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (697 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## YAML float formatting reference-runtime differential (2026-10-04)

- Added `format_dotnet_f32`/`format_dotnet_f64` in src/export/unity_yaml.rs and routed both the plain (`Float32`/`Float64`) and hex (`Float32Hex`/`Float64Hex`) scalar arms through them, replacing Rust's `Display` which never used exponent notation and spelled infinities `inf`/`-inf`.
- Genuine original-runtime differential: the isolated net10.0 probe `Temp/opencode/numericprobe` captured `Single`/`Double.ToString(CultureInfo.InvariantCulture)` for curated boundaries plus 60+60 deterministic pseudo-random finite bit patterns (`numeric_probe.json`, 203 cases); the thresholds are scientific when `e < -4` or `e >= 9` (f32) / `e >= 17` (f64), with signed two-digit exponents. The Rust test `unity_yaml::tests::float_formatting_matches_the_reference_runtime` asserts all 203 values and passes, so Rust's shortest-round-trip digits match .NET's and the notation switch is exact. Recorded as `DIFF-YAML-FLOAT-FORMAT-001`.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (698 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Typed-data reverse-hex reference-runtime differential (2026-10-04)

- Added `value_yaml::tests::typed_data_lists_match_the_reference_runtime`: for each supported element width it builds a `SerializedValue::Array`, converts it with the normal object walker, and asserts the emitted `m_Data` hex equals the reference runtime's `CreateHex` output. The expected values come from the isolated `AssetRipper.Yaml` probe `Temp/opencode/typedprobe` (`typed_probe.json`): byte `0012ff`, sbyte `ff7f80`, short `3412feff`, ushort `3412ffff`, char `4100de56`, int/uint `04030201ffffffff`, long/ulong `0807060504030201ffffffffffffffff`, bool `010001`.
- No divergence was found: the Rust `convert_integer_array` already writes little-endian bytes per element with lowercase hex for all ten widths, matching `ReverseHexString`. The differential upgrades that path from synthetic-only to runtime-verified.
- Cloud checkpoint verified: the earlier three local commits `aeadc03`, `8ac6e61`, `d4bc0e7` were pushed with `git push origin main:main` (exit 0); live `origin/main`, `HEAD`, and `FETCH_HEAD` all equalled `d4bc0e79e52ee3bffebf97ad17fab8f72d95dc8c`, and `git diff --exit-code HEAD FETCH_HEAD` exited 0.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (699 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## YAML mapping/sequence layout reference-runtime differential (2026-10-04)

- Added `unity_yaml::tests::layout_matches_the_reference_yaml_runtime`: 15 node-tree cases built both through the reference `YamlMappingNode`/`YamlSequenceNode` and through `UnityYamlNode`, emitted with `IsWriteDefaultTag = false`, `IsWriteVersion = false`, `IsFormatKeys = true`, and a `\n` newline, then compared byte-for-byte. Expected documents are in `Temp/opencode/layoutprobe/layout_probe.json`.
- Coverage: nested block mappings, block/BlockCurve/flow sequences, empty block/flow/BlockCurve sequences, sequences of mappings, flow mappings, empty block mappings, node-level tag (`!u!myTag`), anchor (`&a1`), `stripped`, an anchor on a multiline mapping (`a:\n  &m1\n  b: 1`), and a nested flow sequence inside a block sequence. No divergence was found, so scalar and layout YAML emission are now both runtime-verified; this upgrades the layout assertions from synthetic-only.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (700 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Packed integer bit-vector reference-runtime differential (2026-10-04)

- Added `src/resources/` and `src/resources/packed_bit_vector.rs`: the `PackedBitVector_Int32` pack/unpack codec from `PackedIntVectorExtensions`, the first resource-decoding primitive. Packing computes the per-element bit size as `ceil(log2(max + 1))` (with `0xFFFFFFFF -> 32`), then writes little-bit-order data; unpacking reverses it.
- Confirmed source quirk: `BitSize == 32` unpacks every element to zero, because the final mask is `(1 << BitSize) - 1` and C# masks shift counts to five bits, so `1 << 32 == 1` and the mask is zero. The reference probe (`Temp/opencode/packedprobe/packed_int_probe.json`) returned `[0, 0, 0, 0]` for all three 32-bit cases; the Rust code reproduces this with `wrapping_shl` instead of "fixing" it.
- Genuine original-runtime differential: 13 pack/unpack cases (uint/int/ushort) plus a 98-value bit-size boundary table (`bit_size_probe.json`) captured through the real `AssetRipper.SourceGenerated`/`.Extensions` assemblies; `resources::packed_bit_vector::tests::{packed_vectors_match_the_reference_runtime, bit_sizes_match_the_reference_runtime}` assert every value. Allocation is bounded by `PackedBitVectorLimits`, and malformed inputs return contextual errors.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (702 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Packed float bit-vector reference-runtime differential (2026-10-04)

- Added `src/resources/packed_float_vector.rs`: `PackedBitVector_Single` unpack/pack from `PackedFloatVectorExtensions`, including the chunked `PackedBitReader`. Values unpack as `x / (mask / Range) + Start`, and `Pack` defaults to a 24-bit size with the `adjustBitSize` (`ceil(log2(range))`) and 32-bit cap rules.
- Two source quirks reproduced and pinned by the probe (`Temp/opencode/floatprobe`):
  - `BitSize == 32` (and `BitSize == 0`) makes `halfMaxValue` zero, so every element unpacks as `0 / 0 == NaN` (`0x7FC00000`), confirmed for the 32-bit and 0-bit cases.
  - `Pack` builds its scale constant with `BitConverter.UInt32BitsToSingle((1u << BitSize) - 1u)`, reinterpreting the integer mask as float bits, so packing is only approximately a round trip. The Rust code mirrors this exactly.
- Genuine original-runtime differential: 7 unpack cases (sizes 0/4/8/16/32, chunked strides, `start` offsets) and 6 pack cases (default 24-bit, explicit 5-bit, `adjustBitSize`, negative values, empty input, zero range) captured through the real `AssetRipper.SourceGenerated(.Extensions)` assemblies; both tests assert every float's exact bit pattern. Malformed chunk parameters return `InvalidChunkStride`.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (704 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Packed quaternion bit-vector reference-runtime differential (2026-10-04)

- Added `src/resources/packed_quat_vector.rs`: `PackedBitVector_Quaternionf` pack/unpack from `PackedQuatVectorExtensions`, with the `QuaternionExtensions` index helpers (`0=X,1=Y,2=Z,3=W`). The 3-bit flag names the largest-magnitude component and its sign; the other three components are quantized to 9 or 10 bits and the named one is reconstructed as `sqrt(1 - sum)`.
- Confirmed source quirk: `Pack` scales with `BitConverter.Int32BitsToSingle((1 << bitSize) - 1)` — the integer mask reinterpreted as float bits — so an all-zero quaternion packs to the same bytes as the identity quaternion. The reference probe (`Temp/opencode/quatprobe`) captured this; the Rust code reproduces it exactly.
- Genuine original-runtime differential: 8 pack/unpack cases (identity, each axis as largest, a negative-largest tie, an arbitrary non-unit quaternion, and all zeros) captured through the real `AssetRipper.SourceGenerated(.Extensions)` assemblies; the test asserts the packed bytes and every unpacked component's exact bit pattern. A second test confirms each component can be selected as the largest.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (706 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Compressed mesh geometry reference-runtime differential (2026-10-04)

- Added `src/resources/compressed_mesh.rs`: the geometry channels of `CompressedMeshExtensions` — `GetVertices`/`SetVertices`, `GetNormals`/`SetNormals`, `GetTangents`/`SetTangents`, `GetTriangles`/`SetTriangles`, and `GetWeights`/`SetWeights` — plus `MeshHelper.FloatArrayToVector3` and `Vector3.Normalize`.
- Genuine original-runtime differential: the probe `Temp/opencode/meshprobe` created a real `CompressedMesh` via `CompressedMesh.Create(2019.4.3f1)`, drove the `Set*` methods, and read the `Get*` outputs; the Rust tests assert every vertex/normal/tangent/bone-weight float bit pattern and the triangle list. The `0xFFFFFFFF` triangle input collapses to all zeros in both, confirming the 32-bit mask quirk at the mesh level.
- The differential caught `DIFF-NORMALIZE-DIVISION-001`: `Vector3.Normalize` divides each component by the length rather than multiplying by a reciprocal; the reciprocal form differed by one ULP (`1065295263` vs `1065295264`). The port now divides.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (708 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Compressed mesh UV and bind-pose reference-runtime differential (2026-10-04)

- Extended `src/resources/compressed_mesh.rs` with the UV and bind-pose channels: `GetUV`/`SetUV` (both the two-channel fast path and the `UVInfo` path), `ReadChannel`, `UVInfo.GetChannelInfo`/`AddChannelInfo`, `MeshHelper.FloatArrayToVector2`, and `GetBindPoses`/`SetBindPoses` with the Unity/.NET layout transpose. New `UVInfo` validation returns `MismatchedUvLengths`, `InvalidUvChannel`, or `InvalidUvDimension` instead of misbehaving.
- Genuine original-runtime differential: the probe `Temp/opencode/uvprobe` created `CompressedMesh.Create(2019.4.3f1)` for UV and `Create(4.7.2f1)` for bind poses, drove `SetUV`/`SetBindPoses`, and read `GetUV`/`GetBindPoses`. The Rust test asserts the two-channel path, the three-channel `UVInfo` path (`UVInfo == 1365`), and two transposed bind poses, all bit-for-bit.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (709 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Compressed mesh color reference-runtime differential (2026-10-04)

- Extended `src/resources/compressed_mesh.rs` with the color channels: `GetFloatColors`/`SetFloatColors` and `MeshHelper.FloatArrayToColorFloat`. The modern `Has_FloatColors` path unpacks four floats per color; the legacy `Has_Colors` path mirrors the source's temporary `NumItems *= 4` / `BitSize /= 4` reinterpretation (done on a clone so the vector is not mutated) and converts each `Color32` to `ColorFloat`.
- Genuine original-runtime differential: the probe `Temp/opencode/colorprobe` drove `SetFloatColors`/`GetFloatColors` through `CompressedMesh.Create(2019.4.3f1)` (`Has_FloatColors` branch) and `Create(4.3.0f1)` (`Has_Colors` branch). The Rust test asserts both result sets bit-for-bit, including the byte quantization `0.5 -> 128 -> 0.50196...` on the legacy path.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (710 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Vertex-data blob reference-runtime differential (2026-10-04)

- Added `src/resources/vertex_data_blob.rs` mirroring `VertexDataBlob`, the vertex-data parts of `VertexDataExtensions`, and the `MeshHelper` format conversions: `Create(MeshData)`, `ReadData`/`ToMeshData`, `ConvertChannelsToStreams`/`ConvertStreamsToChannels`, `GetChannelDimension`, stream stride/size/offset, `MakeLittleEndian`, the `SupportsUV*`/`SupportsSkin` gates, `ToVertexFormat`, `GetFormatSize`, `IsIntFormat`, `BytesToFloatArray`, `BytesToIntArray`, and the float-array-to-vector/color helpers.
- Genuine original-runtime differential: the probe `Temp/opencode/vertexprobe` built the shared triangle `MeshData` and captured `VertexDataBlob.Create` for versions 3.5.0f1, 4.0.0f1, 5.0.0f1, 2017.1.0f1, 2018.1.0f1, and 2018.4.0f1 under both `LittleEndian` and `BigEndian`. The expected channels, streams, and raw bytes are committed as `src/resources/vertex_data_blob_fixture.txt`; the Rust test asserts them bit-for-bit and additionally round-trips each blob through `ReadData`, checking the `SupportsUV*`/`SupportsSkin` presence rules. A second test pins the `ConvertStreamsToChannels` interleaved offsets.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (712 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Shader channel and vertex format layer reference-runtime differential (2026-10-04)

- Added `src/resources/shader_channel.rs` mirroring the shader-channel enums/extensions (`ShaderChannel`, `ShaderChannel4`, `ShaderChannel5`, `ShaderChannel2018`) and the shader vertex-format enums/extensions (`VertexChannelFormat`, `VertexFormat2017`, `VertexFormat2019`, and the five-value `VertexFormat`), plus `ChannelInfoExtensions.GetVertexFormat`/`GetStride`.
- Genuine original-runtime differential: the probe `Temp/opencode/shaderprobe` enumerated every shader channel across six versions and every shader format across the same versions, and constructed `ChannelInfo` instances to exercise `GetVertexFormat`/`GetStride` across formats 0-12 and dimensions 1-4. The 426 expected rows are committed as `src/resources/shader_channel_fixture.txt` and matched row-for-row by the Rust test.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (713 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## VertexDataExtensions reference-runtime differential (2026-10-04)

- Added `src/resources/vertex_data_extensions.rs` mirroring `VertexDataExtensions`: stream gathering, current-channel access, `GetChannel` for both the explicit-channel and fixed-stream layouts, stream stride/size/offset, `GenerateVertices`, and `GenerateSkin`.
- Genuine original-runtime differential: the probe `Temp/opencode/vdprobe` instantiated real `VertexData.Create(version)` subclasses (adding channels through `AssetList.AddNew`), filled a 14-channel interleaved layout with crafted bytes, and drove every extension method for six versions. The Unity 3.5 fixed-stream path was exercised with a crafted stream mask. The 140 expected rows are committed as `src/resources/vertex_data_extensions_fixture.txt` and matched row-for-row, including the error branches. `GetCurrentChannels`/`SetCurrentChannels` have a source-derived unit test.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (715 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Sub-mesh data reference-runtime differential (2026-10-04)

- Added `src/resources/sub_mesh_data.rs` mirroring `SubMeshData`, `SubMeshExtensions`, `IndexFormatExtensions`, `Bounds`, and `MeshData.IndexFormat`: the `FirstByte`/`FirstIndex` translation, the `IsTriStrip`-versus-`Topology` substitution, `CopyTo` write-back, the `MeshTopology` enum, and `Bounds.CalculateFromVertexArray`.
- Genuine original-runtime differential: the probe `Temp/opencode/smprobe` instantiated real `SubMesh.Create(version)` subclasses for six versions, set the fields, and drove `SubMeshData.Create` and `CopyTo` for both index formats, plus `SetFirstIndex` and four `Bounds` cases. The 28 expected rows are committed as `src/resources/sub_mesh_data_fixture.txt` and matched row-for-row, including the pre-2017.3 zero `BaseVertex`/`TriangleCount` and the `IsTriStrip` fallback.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (717 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Tangent mode reference-runtime differential (2026-10-04)

- Added `src/resources/tangent_mode.rs` mirroring the `TangentMode`/`TangentMode2`/`TangentMode5` enums and extensions: the pre-5.5 high-constant bit shift and the 5.5+ identity mapping.
- Genuine original-runtime differential: the probe `Temp/opencode/animprobe` evaluated `ToTangentMode2`, `ToTangentMode5`, and `ToTangent` for the 5.0/5.4/5.5 branches across every `TangentMode` value `0..256`. The 256 expected rows are committed as `src/resources/tangent_mode_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (719 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Keyframe defaults reference-runtime differential (2026-10-04)

- Added `src/resources/keyframe.rs` mirroring `KeyframeExtensions.Float`: the weight and slope `SetValues` overloads, the `DefaultFloatWeight` constant, and the 2018.1 `InWeight`/`OutWeight`/`WeightedMode` presence gate.
- Genuine original-runtime differential: the probe `Temp/opencode/kfprobe` instantiated real `Keyframe_Single.Create(version)` subclasses for eleven versions and captured the reflected `Has_*` gates and both `SetValues` overloads. The 11 expected rows are committed as `src/resources/keyframe_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (720 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Animation curve construction reference-runtime differential (2026-10-04)

- Added `src/resources/animation_curve.rs` mirroring the `AnimationCurveExtensions` single-float overloads: the default loop/rotation envelope and the default-value, two-value, and slope two-keyframe builders, plus the `CurveLoopTypes`/`RotationOrder` enum values.
- Genuine original-runtime differential: the probe `Temp/opencode/curveprobe` instantiated real `AnimationCurve_Single.Create(version)` subclasses for eight versions and captured the `Has_RotationOrder` gate (5.4) and all three overloads' envelopes and keyframe fields. The 24 curve rows and 11 enum rows are committed as `src/resources/animation_curve_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (721 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Vector3f keyframe and vector extensions differential (2026-10-04)

- Added `src/resources/keyframe_vector3.rs` (the `KeyframeExtensions.Vector3f` overloads and the 2018.1 weight gate) and `src/resources/vector_extensions.rs` (the `Vector3f`/`Quaternionf` scale/copy/set/cast helpers).
- Genuine original-runtime differential: the probe `Temp/opencode/v3probe` instantiated real `Keyframe_Vector3f.Create(version)` subclasses for eight versions, captured the reflected `Has_InWeight`/`Has_OutWeight`/`Has_WeightedMode` gates (2018.1) and both `SetValues` overloads, and evaluated four `Vector3f` operations. The 8 keyframe rows and 4 vector rows are committed as `src/resources/keyframe_vector3_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (723 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Quaternionf keyframe differential (2026-10-04)

- Added `src/resources/keyframe_quaternion.rs`: the six `KeyframeExtensions.Quaternionf` overloads (quaternion/scalar/component weights, with and without slopes), the 2018.1 weight gate, and the quaternion slope/tangent handling.
- Genuine original-runtime differential: the probe `Temp/opencode/qprobe` instantiated real `Keyframe_Quaternionf.Create(version)` subclasses for eight versions, captured the reflected weight/`WeightedMode` gates, and drove all six overloads. The 48 rows are committed as `src/resources/keyframe_quaternion_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (724 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Compressed animation curve unpack differential (2026-10-04)

- Added `src/resources/compressed_animation_curve.rs`: `CompressedAnimationCurveExtensions.Unpack` for the quaternion destination, composing the packed int/float/quaternion vectors with the version-aware quaternion keyframe slopes overload.
- Genuine original-runtime differential: the probe `Temp/opencode/cacprobe` packed a real `CompressedAnimationCurve` (times `0/50/100/150`, four raw quaternions, twenty slopes) and called `Unpack` for four versions, capturing the path, loop settings, and every keyframe field. The 4 rows are committed as `src/resources/compressed_animation_curve_fixture.txt` and matched row-for-row, including the source's slope-stride overlap (out-slope of one keyframe equals in-slope of the next) and the pre-2018 absent weights.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (725 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## MuscleClipInfo initialization differential (2026-10-04)

- Added `src/resources/muscle_clip_info.rs`: both `MuscleClipInfoExtensions.Initialize` overloads (neutral defaults and the fourteen-field copy from `MuscleClipConstant`).
- Genuine original-runtime differential: the probe `Temp/opencode/mcprobe` instantiated real `MuscleClipInfo.Create`/`MuscleClipConstant.Create` subclasses for five versions, set distinctive constant values, and drove both overloads, capturing all seventeen info fields. The 5 rows are committed as `src/resources/muscle_clip_info_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (726 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Blend tree node accessor differential (2026-10-04)

- Added `src/resources/blend_tree_node.rs`: the `BlendTreeNodeConstantExtensions` accessors (`GetThreshold`/`GetPosition`/`GetMinThreshold`/`GetMaxThreshold`/`TryGetDirectBlendParameter`/`IsBlendTree`) with the version- and blend-type dispatch.
- Genuine original-runtime differential: the probe `Temp/opencode/btprobe` instantiated real `BlendTreeNodeConstant.Create(version)` subclasses for five versions, populated the 1D thresholds, 2D positions, direct ids, and child indices, and drove every accessor for three blend types. The 15 rows are committed as `src/resources/blend_tree_node_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (727 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## TransitionConstant accessor differential (2026-10-04)

- Added `src/resources/transition_constant.rs`: the `TransitionConstantExtensions` accessors (`GetExitTime`/`GetHasExitTime`/`GetInterruptionSource`/`GetHasFixedDuration`) with the old-versus-new layout fallbacks.
- Genuine original-runtime differential: the probe `Temp/opencode/trprobe` instantiated real `TransitionConstant.Create(version)` subclasses for six versions spanning the old (`Atomic`, condition-based exit time) and new (explicit fields) layouts, added an `ExitTime` condition, and drove all four accessors for three cases. The 18 rows are committed as `src/resources/transition_constant_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (728 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## StateConstant differential (2026-10-04)

- Added `src/resources/state_constant.rs`: the `StateConstantExtensions` id/name/write-default/blend-tree resolution with the version-gated fields and the parent-prefix substring rule.
- Genuine original-runtime differential: the probe `Temp/opencode/scprobe` instantiated real `StateConstant.Create(version)` subclasses for seven versions, populated the blend-tree array and a string dictionary, and drove the id, write-default, blend-tree, and four name cases. The 7 rows are committed as `src/resources/state_constant_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (729 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## ControllerConstant layer differential (2026-10-04)

- Added `src/resources/controller_constant.rs`: the `ControllerConstantExtensions` layer lookups (`GetLayerByStateMachineIndex`, `GetLayerIndexByStateMachineIndex`, `GetLayerIndex`) with the synchronized-layer skip and the error branches.
- Genuine original-runtime differential: the probe `Temp/opencode/ccprobe` instantiated real `ControllerConstant.Create(version)` subclasses for two versions, populated four layers including a synchronized one, and drove all three lookups plus the not-found and not-present error branches. The 2 rows are committed as `src/resources/controller_constant_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (730 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Blend-tree node, state-machine count, and animator enum-cast differentials (2026-10-04)

- Added `src/resources/blend_tree_node_constant.rs`: every `BlendTreeNodeConstantExtensions` accessor with the blend-type-dependent sub-structure selection and fallbacks, spanning the legacy `ChildThresholdArray` (3.5) and modern `Blend1dData`/`Blend2dData`/`BlendDirectData` layouts.
- Added `src/resources/state_machine_constant.rs`: `StateMachineConstantExtensions.StateMachineCount` with the selector transition-count semantics.
- Added `src/resources/animator_enums.rs`: the `LayerConstant`/`ValueConstant`/`AnimatorControllerParameter` enum casts as raw-discriminant newtypes.
- Genuine original-runtime differentials: probes `Temp/opencode/bt2probe` and `Temp/opencode/anim2probe` instantiated real generated subclasses across seven and six versions respectively and drove every accessor and cast. The rows are committed as `src/resources/blend_tree_node_constant_fixture.txt` (7), `src/resources/state_machine_constant_fixture.txt` (6), and `src/resources/animator_enums_fixture.txt` (6), and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (733 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Avatar constant and bone-path differentials (2026-10-04)

- Added `src/resources/avatar_constant.rs`: `AvatarConstantExtensions.GetDefaultPose`/`GetSkeletonNameIDArray` with the 4.3 version gate.
- Added `src/resources/avatar.rs`: `AvatarExtensions.FindBonePath`.
- Genuine original-runtime differentials: probe `Temp/opencode/avatarprobe` instantiated real `AvatarConstant.Create(version)` subclasses and real generated `Avatar` objects (via `AssetCreator.CreateCollection` + `AssetInfo`) for seven versions and confirmed the selected pose/name source by reference identity and the present/absent bone lookups. The rows are committed as `src/resources/avatar_constant_fixture.txt` and `src/resources/avatar_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (735 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Clip extensions differential (2026-10-04)

- Added `src/resources/streamed_clip.rs`, `src/resources/dense_clip.rs`, `src/resources/constant_clip.rs`, `src/resources/clip.rs`, and `src/resources/animation_clip.rs`.
- Genuine original-runtime differentials: probe `Temp/opencode/clip2probe` instantiated real `StreamedClip`/`DenseClip`/`ConstantClip`/`Clip` subclasses and real generated `AnimationClip` objects across nine versions (2.5–6000 where applicable) and drove every accessor, the set combinations, the value-array-to-generic-binding conversion, the legacy flag, and the 2022 negative-infinity-slope boundary. Rows are committed as the five fixture files and matched row-for-row.
- Confirmed source finding recorded in `docs/differential-findings.md`: `ValueConstant_5_5` has no `m_TypeID` backing field, so its `TypeID` accessor always returns 0; `ClipExtensions.ConvertValueArrayToGenericBinding` therefore degenerates to Animator bindings on 5.5+.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (740 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Animation-curve copy-overload differential (2026-10-04)

- Extended `src/resources/animation_curve.rs` with the `SetValues(keyframe)` / `SetValues(keyframe1, keyframe2)` / `SetValues(list)` / `SetValues(list, pre, post)` overloads, including the `CopyValues` target-presence semantics.
- Genuine original-runtime differential: probe `Temp/opencode/curve2probe` created real `AnimationCurve_Single` and `Keyframe_Single` subclasses across 2017/2019 target and source combinations, drove all four overloads, and captured the copied keyframes and envelope. The 16 rows are committed as `src/resources/animation_curve_copy_fixture.txt` and matched row-for-row.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (741 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Curve-wrapper SetValues differential (2026-10-04)

- Added `src/resources/curve_wrappers.rs`: the `QuaternionCurveExtensions`/`Vector3CurveExtensions`/`FloatCurveExtensions`/`PPtrCurveExtensions` `SetValues` overloads with the default envelope and the script pointer.
- Genuine original-runtime differential: probe `Temp/opencode/curve3probe` created real curve subclasses for four versions plus a collection-registered `MonoScript`, drove every overload, and confirmed path/attribute/class-id/script-resolution/envelope. The 20 rows are committed as `src/resources/curve_wrappers_fixture.txt` and matched row-for-row. The probe also established that `ClassIDType` lives in the `AssetRipper.SourceGenerated` root namespace.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (742 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## PPtr and animator-controller contains-clip differential (2026-10-04)

- Added `src/resources/pptr.rs`: `PPtrExtensions.IsAsset`/`IsNull`, including the dependency-table indexing for non-zero file ids.
- Added `src/resources/animator_controller.rs`: the `ContainsAnimationClip` overloads for animator controllers, override controllers, and the runtime dispatch.
- Genuine original-runtime differential: probe `Temp/opencode/animctrlprobe` created collection-registered `AnimationClip`/`AnimatorController`/`AnimatorOverrideController` objects for three versions (using `ProcessedAssetCollection.CreateAsset` so pointers resolve) and confirmed the controller, override, base-controller, and runtime-dispatch results. The 3 rows are committed as `src/resources/animator_controller_fixture.txt` and matched row-for-row. The `IsAsset` dependency branch is asserted directly because the snapshot exposes `AssetCollection.Dependencies` as read-only.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (744 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## GameObject active-state and transform hierarchy differential (2026-10-04)

- Added `src/resources/game_object.rs`: `GameObjectExtensions.GetIsActive`/`SetIsActive` with the boolean/byte combination and the version-gated byte field.
- Added `src/resources/transform.rs`: `TransformExtensions.GetRootPath`/`FindChild`/`CalculateRootOrder` over an arena model, with a cycle guard replacing the reference's unbounded recursion.
- Genuine original-runtime differentials: probe `Temp/opencode/goprobe2` created real `GameObject`/`Transform` objects for three versions, drove the active-state combinations and a registered three-level hierarchy, and captured the results; the rows are committed as `src/resources/game_object_fixture.txt` and `src/resources/transform_fixture.txt` and matched row-for-row. The probe also established that sampled versions serialize only `IsActive_Boolean` and that `GetStateBehaviours` cannot be probed because generated dictionary keys are not publicly mutable.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (746 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## UnityGuid.ToString differential and GUID formatting fix (2026-10-04)

- Confirmed via the external `UnityGuid` (probe `Temp/opencode/guidprobe2`) that `ToString` emits each 32-bit word's nibbles least-significant-first, the canonical Unity GUID layout (`unity default resources` = `0000000000000000e000000000000000`, matching the snapshot's `EGUID` word tuple `(0, 0, 0xE, 0)`).
- Fixed `src/export/unity_yaml.rs::format_unity_guid`, which previously emitted the reversed string, and corrected ten self-authored expectations in `meta.rs`, `project_output.rs`, `unity_yaml.rs`, `value_yaml.rs`, and `workspace_yaml.rs` that encoded the reversed convention.
- Added `src/export/guid_fixture.txt` and `export::unity_yaml::tests::unity_guid_string_matches_the_reference_runtime`.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (747 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## FileIdentifier Meta GUID fix (2026-10-04)

- Found the second instance of the GUID reversal: `serialized_file::metadata::FileIdentifier::get_file_path` formatted Meta GUIDs most-significant nibble first, whereas the reference `FileIdentifier.GetFilePath` returns `Guid.ToString()`.
- Fixed it to the nibble-reversed form and updated the `io::dependency` Meta-identity test's registered name and expected string to the corrected value (`76543210fedcba9800000000ffffffff` for the words `(0x01234567, 0x89abcdef, 0, 0xffffffff)`).
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (747 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Texture/Graphics format differentials (2026-10-04)

- Added `src/resources/texture_format.rs` (`IsCrunched`/`IsDxt`/`IsRgb`) and `src/resources/graphics_format.rs` (`ToTextureFormat` with the `NotSupported`/`NotImplemented`/`ArgumentOutOfRange` errors).
- Genuine original-runtime differential: probe `Temp/opencode/texprobe` enumerated all 91 `TextureFormat` and all 156 `GraphicsFormat` members and captured every classifier and mapping result (or thrown exception type). The rows are committed as `src/resources/texture_format_fixture.txt` (91) and `src/resources/graphics_format_fixture.txt` (156) and matched row-for-row; four aliased GraphicsFormat values collapse to one match arm.
- Final verification on Windows with Rust/Cargo 1.98.1: cargo fmt --all -- --check exit 0; cargo test --workspace --locked exit 0 (749 library, 23 CLI, 0 doc tests); cargo clippy --workspace --all-targets --locked -- -D warnings exit 0; tools/verify-baseline.ps1 exit 0 (1,449/1,449 reference files unchanged); git diff --check exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Byte-array, bounds, color, matrix, and rect differentials; GitHub Actions CI (2026-10-04)

- Added `src/resources/byte_array.rs` (`SwapBytes`), `src/resources/bounds.rs` (`Bounds`/`AABB` vertex-array bounds), `src/resources/color.rs` (`ColorRGBA32`/`ColorRGBAf` packing and black/white), `src/resources/matrix4x4.rs` (transpose copy/cast), and `src/resources/rectf.rs` (`CopyValues` and derived accessors).
- Genuine original-runtime differentials: probe `Temp/opencode/miscprobe` captured every behavior; the rows are committed as `byte_array_fixture.txt` (3), `bounds_fixture.txt` (4), `color_fixture.txt` (3), `matrix4x4_fixture.txt` (32), and `rectf_fixture.txt` (4) and matched row-for-row.
- Added `.github/workflows/ci.yml`: `rustfmt`, warnings-denied `clippy`, and `test` on Ubuntu and Windows, so compilation and the test suite run on GitHub Actions. The baseline verifier is not run in CI because it requires the read-only sibling reference snapshot.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## GitHub Actions CI and cross-platform fixes (2026-10-04)

- Added `.github/workflows/ci.yml`: `rustfmt`, warnings-denied `clippy`, and `cargo test` on `ubuntu-latest` and `windows-latest`, plus a build step, triggered on push/PR/manual dispatch. The `tools/verify-baseline.ps1` verifier still runs only locally because it needs the read-only sibling reference snapshot.
- The first CI run (`37247118771`) failed: clippy flagged `byte_array.rs` `% size != 0` (should be `is_multiple_of`), and the Linux test run exposed `DIFF-PACKEDFLOAT-NAN-001` (a platform-dependent NaN sign in packed-float unpack that passed on Windows).
- Fixed both; the second run (`37247323096`) is green on both operating systems across all five jobs. This is the first independent, non-Windows confirmation of the test suite.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Vector/quaternion/transform and Hash128 differentials (2026-10-04)

- Added `src/resources/vector.rs`, `src/resources/quaternion.rs`, and `TransformLocalView::initialize_default` in `src/resources/transform.rs`, each with a committed original-runtime differential fixture.
- Added `src/export/unity_yaml.rs::format_hash128` and wired it into `value_yaml.rs` (new `Hash128Overflow` error), mirroring the baseline's checked-overflow quirk (`DIFF-HASH128-OVERFLOW-001`) with a seven-row differential fixture including the exact 0x7F/0x80 boundary.
- GitHub Actions run `37248303815` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## StreamingInfo/GLTextureSettings/MinMaxGradient differentials (2026-10-04)

- Added `src/resources/streaming_info.rs`, `src/resources/gl_texture_settings.rs`, and `src/resources/min_max_gradient.rs`, each with a committed original-runtime differential fixture (probe `Temp/opencode/resprobe`).
- The differential captured two real version branches: the sampler `WrapMode` vs `WrapU`/`WrapV`/`WrapW` split, and the gradient packed-color vs float-color split.
- GitHub Actions run `37248866979` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Texture decoder (DXT1/3/5 and packed formats) differential (2026-10-04)

- Added `src/resources/texture_decode.rs`: DXT1/DXT3/DXT5 block decoding plus `RGBA32`, `RGB24`, `BGRA32`, `ARGB32`, `Alpha8`, `R8`, `RG16`, `RGB565`, `ARGB4444`, and `RGBA4444` packed decoders, ported from the `AssetRipper.TextureDecoder` dependency used by `TextureConverter`.
- Genuine original-runtime differential: probe `Temp/opencode/texdecprobe` decoded the shipped `AssetRipper.TextureDecoder.dll`; 22 rows are committed as `src/resources/texture_decode_fixture.txt` and matched byte-for-byte.
- Recorded `DIFF-TEXDEC-EXPANSION-001`: the DXT endpoint uses bit replication while the standalone `RGB565` converter uses a bare shift, and the three-color DXT index 3 is opaque black.
- GitHub Actions run `37249584027` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## BC4/BC5 texture decoders (2026-10-04)

- Added `decode_bc4`/`decode_bc5` to `src/resources/texture_decode.rs` and recorded `DIFF-BC45-ROUNDING-001`: the `Bc` decoders use `+1`-rounded channel interpolation while the `Dxt` decoder truncates.
- The fixture `src/resources/texture_decode_fixture.txt` now has 33 rows, including six direct interpolation-table probes that assert every table entry.
- GitHub Actions run `37250084317` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## ETC1/ETC2/EAC texture decoders (2026-10-04)

- Added `src/resources/etc_decode.rs`: ETC1 (individual/differential), ETC2 (individual/differential/T/H/planar), ETC2A1 punch-through, ETC2A8, and EAC R/RG unsigned/signed, ported from the `AssetRipper.TextureDecoder` dependency used by `TextureConverter`.
- Genuine original-runtime differential: probe `Temp/opencode/etcprobe` decoded blocks crafted to hit every mode against the shipped `AssetRipper.TextureDecoder.dll`; 23 rows are committed as `src/resources/etc_decode_fixture.txt` and matched byte-for-byte.
- Recorded `DIFF-ETC-ALPHAORDER-001`: the ETC2A8 alpha channel uses the reverse block permutation of the color channel.
- GitHub Actions run `37250701750` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## ATC, YUY2, and wide texture formats (2026-10-04)

- Added `src/resources/atc_yuy2.rs` (ATC RGB4/RGBA8, YUY2) and the 16-bit/half/float/signed/RGB9e5 converters in `src/resources/texture_decode.rs`, ported from `AssetRipper.TextureDecoder`'s `AtcDecoder`, `Yuy2Decoder`, and `NumericConversion`.
- Genuine original-runtime differential: probe `Temp/opencode/texprobe2` decoded 24 blocks/images against the shipped DLL; 5 rows are committed as `src/resources/atc_yuy2_fixture.txt` and 19 as `src/resources/wide_format_fixture.txt`, all matched byte-for-byte.
- Recorded `DIFF-ATC-COLOR-001`: ATC uses bit-replicating `Extend` and `5/8`/`3/8` interpolation, with a distinct explicit-alpha branch.
- GitHub Actions run `37251358484` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## BC7 texture decoder (2026-10-04)

- Added `src/resources/bc7.rs` and `src/resources/bc7_tables.rs`, ported from `AssetRipper.TextureDecoder`'s `BcHelpers.DecompressBc7`/`Bc7Tables`/`BitStream`. The large partition tables were mechanically extracted from the package source (`Bc7Tables.cs`) rather than transcribed, and committed as Rust data.
- Genuine original-runtime differential: probe `Temp/opencode/bc7probe` decoded one block per mode plus invalid and payload variants; 16 rows are committed as `src/resources/bc7_fixture.txt` and matched byte-for-byte.
- Texture format coverage now additionally includes BC7. Still unported: BC6H, ASTC, PVRTC, the `TextureConverter` mip/layer/`GraphicsFormat` dispatch, Crunch, and the image writers.
- GitHub Actions run `37251920277` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## BC6H texture decoder (2026-10-04)

- Added `src/resources/bc6h.rs` and `src/resources/bc6h_tables.rs`, ported from `AssetRipper.TextureDecoder`'s `BcHelpers.DecompressBc6h`/`Bc6hTables`/`Bc6h`. The tables were mechanically extracted from the package source.
- Genuine original-runtime differential: probe `Temp/opencode/bc6hprobe` decoded one block per mode plus a reserved mode and payload variants; 19 rows are committed as `src/resources/bc6h_fixture.txt` and matched byte-for-byte.
- Recorded `DIFF-BC6H-HALF-001`: the decoded 16-bit values are reinterpreted as IEEE-754 binary16 before conversion to the output channel, which a first integer-conversion port got wrong.
- Texture coverage now: DXT1/3/5, BC4/BC5, BC6H, BC7, ETC1/ETC2/ETC2A1/ETC2A8, EAC R/RG (u/s), ATC RGB4/RGBA8, YUY2, and the packed/wide formats. Remaining decoders: ASTC and PVRTC.
- GitHub Actions run `37252617246` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## PVRTC texture decoder (2026-10-04)

- Added `src/resources/pvrtc.rs`, ported from `AssetRipper.TextureDecoder`'s `PvrtcDecoder` (PVRTC2/PVRTC4).
- Genuine original-runtime differential: probe `Temp/opencode/pvrtcprobe` decoded 8×8 images with two payload patterns; four rows are committed as `src/resources/pvrtc_fixture.txt` and matched byte-for-byte.
- Recorded `DIFF-PVRTC-BLUE-001`: the reference expands the second colour's blue into colour A's slot, which the port reproduces exactly.
- Texture coverage now: DXT1/3/5, BC4/BC5, BC6H, BC7, ETC1/ETC2/ETC2A1/ETC2A8, EAC R/RG (u/s), ATC RGB4/RGBA8, PVRTC2/4, YUY2, and the packed/wide formats. Only ASTC remains among the block decoders.
- GitHub Actions run `37252919176` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## ASTC texture decoder (2026-10-05)

- Added `src/resources/astc.rs` and `src/resources/astc_tables.rs`, ported from `AssetRipper.TextureDecoder`'s `AstcDecoder` (4×4 through 12×12 footprints, void-extent blocks, dual-plane weights, and partition selection). The lookup tables are mechanically extracted from the package source.
- Genuine original-runtime differential: probe `Temp/opencode/astcprobe` decoded 4×4, 6×6, and 8×8 footprints plus a void-extent block; eight rows are committed as `src/resources/astc_fixture.txt` and matched byte-for-byte.
- Recorded `DIFF-ASTC-BITTRANSFER-001`: the reference's `BitTransferSigned(ref v[1], ref v[0])` argument order, which a first port mirrored incorrectly.
- Recorded `DIFF-ASTC-SHIFT-001`: C# masks `ulong` shift counts, so the Rust port uses wrapping shifts in `GetBits64` and promotes the bit-reverse table byte to `i32`.
- Added `TextureDecodeError::InvalidBlockDimension` instead of overloading `InputTooShort` for the reference's 4..=12 `ArgumentException`.
- Texture coverage now: DXT1/3/5, BC4/BC5, BC6H, BC7, ETC1/ETC2/ETC2A1/ETC2A8, EAC R/RG (u/s), ATC RGB4/RGBA8, PVRTC2/4, ASTC, YUY2, and the packed/wide formats. All block decoders dispatched by `TextureConverter.DecodeTexture` are now ported; the remaining texture work is the `RgbConverter`/`DirectBitmap` plumbing, the mip/layer/`GraphicsFormat` dispatch, Crunch, and the image writers.
- GitHub Actions run `37254780389` is green across all five jobs. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Texture converter dispatch (2026-10-05)

- Added `src/resources/texture_converter.rs`, porting `TextureConverter.TryConvertToBitmap`/`DecodeTexture`: the `TextureFormat` and `GraphicsFormat` decoder switches, the per-layer loop over `ImageSize` (with the `Texture3D` stride fallback), `FlipY`, and `UnpackNormal`.
- Added the missing RGB decoders `TextureFormat.BGR24` and `TextureFormat.ARGBFloat`, plus the `GraphicsFormat`-only decoders (`R32*_UInt`, `R32*_SInt`, `R16G16B16_SFloat`, `R32G32B32_SFloat`, `B8G8R8_SNorm`, `B8G8R8A8_SNorm`, `A16_UNorm`).
- Genuine original-runtime differentials: probe `Temp/opencode/texprobe3` appended four `BGR24`/`ARGBFloat` rows to `wide_format_fixture.txt` (now 23), and probe `Temp/opencode/texprobe4` produced the 13-row `graphics_format_decode_fixture.txt`. Both matched byte-for-byte.
- `UnpackNormal`'s truncating `Convert<double, byte>` (the reference gives `254`, not `255`, for a `(128,128)` normal) is covered by a synthetic test.
- Crunch decompression is not ported; crunched formats return `TextureConvertError::CrunchUnsupported` rather than decoding wrong data.
- `cargo test --workspace --locked`: 780 library and 23 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Texture image writers (2026-10-05)

- Added `src/resources/image_export.rs`: the `ImageExportFormat` enum and `Bitmap::save` for PNG, BMP, and TGA. The BMP writer reproduces `BmpWriter.WriteBmp` byte-for-byte; the PNG writer emits filter-0 scanlines with an RGB or RGBA colour type; the TGA writer emits uncompressed top-down BGRA/BGR.
- EXR, HDR, and JPEG return `ImageWriteError::Unsupported` until their codecs are ported.
- `cargo test --workspace --locked`: 786 library and 23 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Texture2D view and export (2026-10-05)

- Added `src/resources/texture.rs`: `Texture2DView::from_value` reads the class-28 export fields (`m_Width`, `m_Height`, `m_TextureFormat`, `m_CompleteImageSize`, `m_ImageCount`, `m_MipCount`, `m_LightmapFormat`, `image data`, `m_StreamData`), computes `ActualImageSize` (complete size, else per-layer data length), prefers inline over streamed bytes, and `export_texture2d_image` runs the decode → `FlipY` → `UnpackNormal` → image-writer pipeline.
- Aliases are read with `unique_field_in_base_chain`; duplicate aliases, missing required fields, negative dimensions, wrong value kinds, and unavailable image data all produce explicit errors rather than zero-filled output.
- 9 new focused tests cover field reads, the `ActualImageSize` fallback and per-layer division, missing/negative/ambiguous fields, PNG export, inline-over-stream precedence, streamed-content requirement, and the normal-map unpack.
- `cargo test --workspace --locked`: 795 library and 23 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## CLI texture export (2026-10-05)

- Added the `export-texture <serialized-file-index> <path-id> <new-output-image> <input-file> ...` CLI command. It mounts the selected inputs, loads the SerializedFile workspace, decodes the selected object, builds a `Texture2DView`, resolves inline or streamed image data through `resolve_and_read_streamed_resource_info`, and writes a PNG/BMP/TGA image via `export_texture2d_image`. The output extension selects the format (default PNG); an existing output file is never replaced.
- Two generated generation-10 SerializedFile CLI-handler tests verify a PNG signature and status line, refusal to overwrite, and a non-texture object failing without output.
- `cargo test --workspace --locked`: 795 library and 25 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Terrain heatmap exporter (2026-10-05)

- Added `src/resources/terrain_heatmap.rs`: `TerrainHeightmap::from_value` reads `m_Heightmap.m_Width`/`m_Height`/`m_Resolution`/`m_Heights`, `rgba32_data` applies the five-stop colour ramp (normalizing by `short.MaxValue`), and `bitmap` builds the `max(dimension, resolution)` RGBA32 bitmap and flips it.
- The `(Color32)ColorFloat` conversion matches the reference's `ConvertFloatToByte` (`NaN`/`<=0` → 0, `>=255` → 255, else truncate); alpha is always 255.
- Six synthetic tests; `cargo test --workspace --locked`: 801 library and 25 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Processed mesh data (2026-10-05)

- Added `compressed_mesh::decompress` (the `CompressedMesh.Decompress` channel orchestration) and `src/resources/mesh_data.rs` (the `MeshData` merge of the vertex-data-blob and compressed-mesh paths with the source's `??=` fallbacks, plus `UVCount`/`IndexFormat` and the `TryGet*AtIndex` post-processing: UV `Y` flip, tangent `W` snap, skin defaulting/renormalization).
- Two `decompress` tests and seven `MeshData` tests; `cargo test --workspace --locked`: 810 library and 25 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.


## Mesh class-43 field reader (2026-10-05)

- Added `src/resources/mesh.rs`: `MeshView::from_value` reads `m_VertexData`, `m_CompressedMesh`, `m_IndexBuffer`, `m_IndexFormat`, `m_SubMeshes`, `m_BindPose`, and `m_Skin`; `processed_index_buffer` decodes 16/32-bit indices with file endianness; `read_mesh_data` runs the blob/compressed decoders and merges them into `MeshData`.
- Five synthetic tests pass; `cargo test --workspace --locked`: 815 library and 25 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used.
- The serialized `SubMesh`, `BoneWeights4` lane, and old-stream spellings remain candidate mappings; no Unity-produced or C# differential has run, so the ledger row is `已实现但未验证`. Phase 1 remains open.


## Mesh generated-API field evidence (2026-10-05)

- Probe `Temp/opencode/meshprops` dumped the shipped `AssetRipper.SourceGenerated.dll` property lists for `IMesh`, `ISubMesh`, `IBoneWeights4`, `IStreamInfo`, `IChannelInfo`, and `ICompressedMesh`; the 105-line output is committed as `src/resources/mesh_generated_api_fixture.txt`.
- Combined with the `Pass002_RenameSubnodes` naming rule, the property names imply the `boneIndex[0]`/`weight[0]` lane names and the `channelMask`/`stride`/`dividerOp`/`frequency` stream names used by `src/resources/mesh.rs`. A regression test pins every relied-upon property.
- Fixed the stream stride to read the `stride` field (the `m_Stride` alias was wrong) with the storage width selected by the decoded value kind, and corrected the bind-pose fields to the `E00..E33` spelling used by `matrix4x4.rs`.
- `cargo test --workspace --locked`: 816 library and 25 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Recorded as `DIFF-MESH-GENERATED-API-001` (source-derived, not a serialization differential). Phase 1 remains open.


## GLB mesh export (2026-10-05)

- Added `src/resources/glb.rs`: `get_mesh_type`, the `GlbCoordinateConversion` scales, `build_primitive` (topology rewrite), and `build_glb` (glTF 2.0 JSON plus binary container). `GlbMeshExport` returns the file bytes, the per-sub-mesh emitted source-index sequence for auditing, and any warnings.
- Added the `export-mesh` CLI command in `src/main.rs`, guarded to class 43, refusing to overwrite, and reporting vertex/sub-mesh/emitted-index counts. It uses `select_unique_object`, `file_unity_version`, and `file_endian_type`; an absent or unparseable Unity version string is an explicit error rather than a default.
- 19 module tests and 3 CLI tests. The CLI test builds a synthetic class-43 `Mesh` SerializedFile with a Float32 position channel, a 16-bit index buffer, and one triangle sub-mesh, then asserts the GLB magic, total length, mesh name, and `SubMesh_0` node.
- `cargo test --workspace --locked`: 836 library and 28 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. SharpGLTF is an external package outside the frozen snapshot, so byte-level output equality is not claimed. Phase 1 remains open.

## Material object model and CRC28 differential (2026-10-05)

- Added `src/resources/material.rs`: `MaterialView` reads class-21 `Material` fields; `MaterialPropertySheetView` reads `m_TexEnvs`/`m_Floats`/`m_Colors`/`m_Ints`/`m_FlowMappedInYaml`; `MaterialTexEnvKeyShape::classify` reproduces the reference's three-branch tex-env dictionary selection and reports which `Has_*` gate a payload satisfied.
- Probed the shipped `AssetRipper.SourceGenerated.dll` (`Temp/opencode/matprops`); the 142-line property dump is committed as `src/resources/material_generated_api_fixture.txt`, and a regression test pins every relied-upon `IMaterial`, `UnityPropertySheet`, and `UnityTexEnv` property.
- Corrected the CRC understanding against a real differential: probe `Temp/opencode/crcprobe` read `Crc28Algorithm.Mask` (`0x0FFFFFFF`) and confirmed every Crc28 word is the `Crc32Algorithm` word masked to 28 bits. Twelve probe rows are committed as `src/resources/crc28_fixture.txt` and asserted, including a non-ASCII name hashed as UTF-8 bytes and `MatchUTF8` accepting the exact word while rejecting a perturbed one.
- Also corrected `find_property_name_by_crc28` to the reference's real search order (tex-env names with `_HDR`, `_ST`, `_TexelSize` postfixes, then floats, then ints, then colors) and added `m_Ints`.
- Split the coarse `RES-MATERIAL-001` placeholder into `RES-MATERIAL-VIEW-001` (已实现但未验证), `RES-MATERIAL-CRC28-001` (已验证, reference-runtime differential), and `RES-MATERIAL-EXPORT-001` (已调查, no `.mat`/`.shader` exporter yet).
- `cargo test --workspace --locked`: 852 library and 28 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Mesh read-path reference-runtime differential (2026-10-05)

- Probe `Temp/opencode/meshread` (SDK 10.0.401) built real generated `Mesh_5`/`Mesh_2017`/`Mesh_2019`/`Mesh_2022` objects through `AssetInfo` + `ProcessedAssetCollection` + `GameBundle`, filled each with the reference `MeshData.CreateTriangleMesh` values via `MeshExtensions.FillWithCompressedMeshData`, and called `ReadData` and `TryMakeFromMesh`.
- All four versions produced identical rows; the 53 kept rows are committed as `src/resources/mesh_read_fixture.txt`. The Rust test reconstructs the same inputs with its own compressed-mesh packers and asserts every reference value, so `RES-MESH-DATA-001` moves from `已实现但未验证` to `已验证`.
- Observed reference values now pinned: `UVCount=2` from a `UVInfo`-zeroed two-channel pack, `UV7 == null`, empty `Skin`/`BindPose` arrays, `[0,1,2]` indices, and `IndexFormat=UInt16`/`Is16=True`.
- An unexpected probe result: setting `IndexFormatE` to `UInt32` on a `Mesh_2019` still reported `Is16=True`, so `Has_IndexFormat()` appears false there. The 32-bit index path therefore remains covered only by the Rust unit test; recorded in the ledger row.
- `cargo test --workspace --locked`: 853 library and 28 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Mesh TypeTree gates from the reference Tpk (2026-10-05)

- Probe `Temp/opencode/tpkprobe` (SDK 10.0.401, AssetRipper.Tpk 2.0.0.0) walked the reference `SourceTpk` recursion and dumped the class-43 release TypeTrees for 3.5.0a0, 5.0.0a0, 2019.0.0a0, and 6000.6.0a5; the 909-line dump (185/223/238/255 nodes) is committed as `src/resources/mesh_typetree_fixture.txt`.
- The trees exposed three real reader defects, all fixed in `src/resources/mesh.rs`: (1) `m_BindPoses`/`m_FloatColors`/`m_Colors` were required unconditionally, but 3.5-4.x carry only the first two and 5.0+ only the latter two (with `m_UVInfo`); they are now optional with presence-derived gates. (2) `m_Streams` was read only as a list, but Unity 3.5 uses four fixed `m_Streams[0..3]` structs (`channelMask`/`offset`/`stride`/`align`, all `UInt32`, no `dividerOp`/`frequency`); the reader now falls back to them. (3) `m_IndexFormat` exists only on 2019+; older files stay on the 16-bit default. The 3.5 `SubMesh` shape (`isTriStrip` + `triangleCount`, no `topology`/`baseVertex`) was already readable and is now pinned, as are the 2019+-only `m_MeshMetrics[0/1]` fields.
- Recorded as `DIFF-MESH-TYPETREE-GATES-001` (source-derived from the reference's own TypeTree database, not a serialization differential). `RES-MESH-VIEW-001` stays `已实现但未验证`: no Unity-produced payload or C# round-trip has run.
- `cargo test --workspace --locked`: 874 library and 28 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Mesh UV-distribution metric with a runtime differential (2026-10-05)

- Added `src/resources/mesh_metric.rs`: `triangle_triples` (`TriangleEnumerable` winding, degenerate removal, quad split), `triangle_area_v2`/`triangle_area_v3`, and `calculate_mesh_metric` with the source defaults (`1.0`) and operation order. Reference throws become explicit `MeshMetricError` values.
- Probe `Temp/opencode/meshread` invoked the private static through an expression-tree delegate: square `1`, scaled `0.25`, flat `1`, empty `1`, strip `0.5`, committed as `src/resources/mesh_metric_fixture.txt` and matched exactly. A second probe section resolved the `Fill` stamp mystery: `Mesh_2019` (`Has_MeshMetrics_0_/1_` true) stamps `1`/`1`, `Mesh_5` (both gates false) discards the stamp, matching the TypeTree fields which exist only on 2019+.
- New ledger row `RES-MESH-METRIC-001` at `已验证`; recorded as `DIFF-MESH-METRIC-001`.
- `cargo test --workspace --locked`: 874 library and 28 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Scene-GLB material texture selection (2026-10-05)

- Added `MaterialView::glb_material_textures` in `src/resources/material.rs`, reproducing `GlbLevelBuilder.GetTextures`: `_MainTex`/`texture`/`Texture`/`_Texture` select the main texture (first wins), `_Normal`/`Normal`/`normal` select the normal texture (first wins), and the first other property is the main-texture replacement. PPtr resolution and the `as ITexture2D` check stay with the caller because they need the asset collection.
- Six tests; split `RES-MODEL-GLB-MATERIAL-001` (`已实现但未验证`) off the coarse `RES-MODEL-001` placeholder without touching it.
- `cargo test --workspace --locked`: 874 library and 28 CLI tests; warnings-denied Clippy and format check pass. Source-read only; no runtime differential has run. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Shader text export with a runtime differential (2026-10-05)

- Probe `Temp/opencode/shaderexport` (SDK 10.0.401) created real versioned shaders via `Shader.Create`, set `Script`, and called the shipped `DummyShaderTextExporter.ExportShader` plus both exporters' `TryCreateCollection`: `Shader_3_5`/`Shader_5` carry `Script` without `ParsedForm`, 2019+/2022+ the reverse; `Simple` admits exactly decompiled text and wins under every mode; `Decompile` mode falls back to the dummy exporter. Six unparsed-path rows are committed as `src/export/shader_dummy_fixture.txt`.
- Added `src/resources/shader.rs` (`ShaderView` with byte-oriented reads, presence-driven gates) and `export_shader_text` in `src/export/shader_export.rs` (stack order with explicit `YamlModeWritesAsset`/`ParsedFormUnsupported`/`MissingSubShader` errors). New `export-shader` CLI (`--shader-export-mode`, class-48 guard, overwrite refusal) and a synthetic class-21 `.mat` YAML emission test through the generic object-YAML path.
- `RES-MATERIAL-EXPORT-001` moves from `已调查` to `已实现但未验证`; recorded as `DIFF-SHADER-DUMMY-001`. The parsed-form dummy path stays open (explicit error, not a silent fallback).
- `cargo test --workspace --locked`: 881 library and 33 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Custom-curve attribute tables with a regex differential (2026-10-05)

- Added `src/resources/custom_curve.rs`: `BindingCustomType` (all 22 discriminants) and `TransformType`/`GetDimension` from the snapshot enums, the six constraint property tables (low nibble plus `attribute >> 8` indices), the `Renderer`/`RendererMaterial`/`SpriteRenderer` branches, the six converter prefixes, and a regex-free `is_unknown_path` matcher.
- Probe `Temp/opencode/rpathprobe` (SDK 10.0.401) evaluated the real .NET `UnknownPathRegex` on 21 paths; all rows committed as `src/resources/custom_curve_rpath_fixture.txt` and asserted exactly. Scene/checksum-dependent branches (blend-shape and material CRC lookups, `TryGetPath` tables, `ReverseAscii` fallbacks) stay caller-side; unknown attributes and unsupported types are explicit errors like the source throws.
- New ledger row `PROC-ANIM-CUSTOM-CURVE-001` at `已实现但未验证`; recorded as `DIFF-CUSTOM-CURVE-REGEX-001`.
- `cargo test --workspace --locked`: 889 library and 33 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Streamed-curve slopes and frame decoding with a runtime differential (2026-10-05)

- Probe `Temp/opencode/sclipprobe` (SDK 10.0.401) drove real `StreamedCurveKey` instances through `CalculateSlopes` (regular, negative-infinity, chained inexact-limit, equal/step limits, fractional times) and real `StreamedFrame.Read` over crafted LE/BE buffers (duplicate collapse, alignment). Eleven rows committed as `src/resources/streamed_curve_fixture.txt` with exact bit words.
- Added `src/resources/streamed_curve.rs`: key/frame reads with bounded buffers, bit-exact double-precision slope math (including the double-promoted limit comparison and the `5e-7f`-as-double threshold), and explicit truncation/negative-count errors. A test-replay bug on the chained row (wrong middle-key coefficients) was caught by the differential and fixed before committing.
- New ledger row `PROC-ANIM-STREAMED-001` at `已实现但未验证`; recorded as `DIFF-STREAMED-SLOPES-001`. Converter orchestration (buffer assembly, `ProcessStreams`, bindings cache) stays open.
- `cargo test --workspace --locked`: 893 library and 33 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Converter orchestration math with search and rounding differentials (2026-10-05)

- Probe `Temp/opencode/sclipprobe` also invoked the private `TryGetNextFrame` via reflection (six search rows) and `float.Round` on nine inputs (confirming to-even rounding with `-0.5` → `-0.0`); rows appended to `src/resources/streamed_curve_fixture.txt`.
- Added `try_get_next_frame` to `src/resources/streamed_curve.rs` and new module `src/resources/clip_convert.rs`: `IsTransform` precedence (Transform always; RectTransform only with valid type), bindings-cache expansion, `CalculateLastConstantTime` with all branches, the first-frame fix, dense frame times, the constant one-or-two rule, and `DefaultFloatWeight`. Missing bindings and invalid transform types are explicit errors.
- New ledger row `PROC-ANIM-CONVERT-001` at `已实现但未验证`; recorded as `DIFF-CLIP-CONVERT-001`. Curve emission on clip objects stays open.
- `cargo test --workspace --locked`: 899 library and 33 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## AudioClip view and export-audio CLI with decoder differentials (2026-10-05)

- Probe `Temp/opencode/audioprops` (SDK 10.0.401) pinned `IAudioClip` properties, per-version gates (3.5 inline `m_AudioData` + `m_Type`/`m_Format`; 5.0+ `m_Resource`), and the `FmodSoundType`/`FmodSoundFormat` discriminants (`src/resources/audio_generated_api_fixture.txt`). Probe `Temp/opencode/audioprobe` drove real 3.5 clips through the shipped `TryDecode`: the empty message, typed `wav`/`ogg` passthrough, and the unmapped default (`src/export/audio_decode_fixture.txt`).
- Added `src/resources/audio.rs` (`AudioClipView` with source-order resolution and lazy integrity, numeric `FmodSoundType` table), `try_decode_audio` in `src/export/audio_export.rs` (reference order with exact messages; `FSB5`/unknown are explicit errors), and the `export-audio` CLI (class-83 guard, inline plus streamed `.resS` resolution, overwrite refusal).
- Corrected a real deviation the trace exposed: the unknown-byte diagnostic fills with U+00D8, not ASCII `'O'` (source `FillCharacter` is explicit); code and test fixed, recorded in `DIFF-AUDIO-DECODE-001`. `OggToWav` could not load in the probe sandbox and stays source-read with an explicit error.
- `RES-AUDIO-001` moves from `未调查` to `已实现但未验证`; recorded as `DIFF-AUDIO-DECODE-001`.
- `cargo test --workspace --locked`: 908 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Parsed-form shader dummy with a runtime differential (2026-10-05)

- Probe `Temp/opencode/shaderexport` built real 2019.4 `Shader.Create` instances with a `ParsedForm` through the generated API (three cases: lit color+texture, flag/attribute/cube, single color) and called the shipped `DummyShaderTextExporter.ExportShader` (SDK 10.0.401); rows committed as `src/export/shader_parsed_fixture.txt` (`SPARSED|tag|ok|meta|prop|text`, floats as hex bits, text escaped).
- Added `ParsedShaderProperty`/`ParsedShaderForm` decoding to `src/resources/shader.rs` (`m_ParsedForm` with `m_Name`, `m_PropInfo`/`m_Props`, `m_FallbackName`, `m_CustomEditorName`; per-property `m_Attributes`/`m_Flags`/`m_Name`/`m_Description`/`m_Type`/`m_DefValue_0..3`/`m_DefTexture` with `m_TexDim`/`m_DefaultName`; serialized names follow the class-48 convention without a local class-48 TypeTree dump) and `render_parsed_property`/`render_parsed_properties`/`parsed_dummy_body`/`export_parsed_dummy` to `src/export/shader_export.rs` (stored attributes plus flag keywords in source order, `Vector` for color/vector, `Range(min, max)` with the reference space, texture dimension keywords, invariant float rendering via `format_dotnet_f32`, best-template selection by name since catalog and body orders differ).
- Fixed a real deviation the differential exposed: `Range` renders as `Range(min, max)` with a space (`SerializedPropertyExtensions.GetTypeString`), not `Range(min,max)`; the shipped-DLL templates agree byte-for-byte with the frozen snapshot sources (spaced `Default`, unspaced others), so no template normalization applies.
- Added `src/export/float_tostring_fixture.txt` (28-row `float.ToString(InvariantCulture)` probe `Temp/opencode/floatprobe`) pinning the exact formatter behind property rendering.
- `cargo test --workspace --locked`: 910 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Blend-shape name resolution with a checksum differential (2026-10-05)

- Probe `Temp/opencode/blendprops` (SDK 10.0.401) confirmed the generated surface: `IBlendShapeData` (`Channels`, `FullWeights`, `Shapes`, `Vertices`), `IMeshBlendShapeChannel` (`FrameCount`, `FrameIndex`, `NameHash`, `Name_R`), `IMeshBlendShape` (`AabbMaxDelta/MinDelta` gated, `FirstVertex`, `HasNormals/Tangents`, `Name_R` gated, `VertexCount`), plus the `IMesh` `Shapes`/`ShapesList`/`ShapeVertices` properties from the earlier mesh fixture.
- Added `src/resources/blend_shape.rs`: `crc32_ieee` (full reflected CRC-32, the unmasked core of `material::crc28`), `is_crc_match` (`IsCRCMatch`: absent names never match), `find_channel_name_by_crc` (`BlendShapeDataExtensions`: first stored `nameHash` match), and `BlendShapeLookupView` decoding `m_Shapes`/`channels` (`name`, `nameHash`) and `m_ShapesList` (`name`) with the `MeshExtensions` precedence (channels decide when present, never falling through to the legacy list). Serialized names come from the committed 5.0 class-43 TypeTree dump; vertex deltas, frame ranges, and weights stay with the mesh-geometry reader, and the mutating `SetValues` stays caller-side with the other generated setters.
- `DIFF-BLENDSHAPE-001` recorded. New ledger row `RES-BLENDSHAPE-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 918 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Script identity and assembly names with a runtime differential (2026-10-05)

- Probe `Temp/opencode/scriptidprobe` (SDK 10.0.401) drove the shipped `AssetRipper.IO.Files.dll` (`SpecialFileNames`: 16 fix rows, 5 remove/5 add extension rows, 9 project-assembly rows, pinning the case-sensitive edges) and the shipped `AssetRipper.Import.dll` (`ScriptingBackend` discriminants, three `ScriptIdentifier` renderings, both `ToUniqueName` overloads, default `IsDefault`/`ToString`, three constructor rejections); all 49 rows committed as `src/io/script_identity_fixture.txt`.
- Extended `src/io/special_names.rs` (`fix_assembly_name`, `remove/add_assembly_file_extension`, `is_project_assembly`, `ASSEMBLY_EXTENSION`) and added `src/processing/script_identity.rs` (`ScriptIdentifier` with validated construction, unique names, and display; `ScriptingBackend` with discriminants; `get_full_name`/`get_valid_assembly_name` kernels). The null-namespace rejection has no Rust counterpart and is documented as a gap.
- `DIFF-SCRIPT-IDENTITY-001` recorded. New ledger row `TYPE-SCRIPT-IDENTITY-001` (first audited slice of `TYPE-SCRIPT-001`) at `已实现但未验证`.
- `cargo test --workspace --locked`: 924 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Export-settings discriminants and defaults with a runtime differential (2026-10-05)

- Probe `Temp/opencode/settingprobe` (SDK 10.0.401) dumped every member discriminant of the 12 shipped configuration enums (`AssetRipper.Export.dll`, `AssetRipper.Import.dll`, `AssetRipper.Processing.dll`) plus `new ExportSettings()`/`ImportSettings()`/`ProcessingSettings()` default dumps; all 83 rows committed as `src/export/settings_fixture.txt`.
- Extended `src/export/settings.rs`: `discriminant()` consts for all 12 enums and the full 21-member `ScriptLanguageVersion` (replacing the two-variant `ScriptLanguageSelection`, including `Latest = 2147483647` and the `CSharp7_1 = 701` step). The automatic language selection still needs external version ordering and stays out; default Unity versions (`0.0.0a0`) are pinned as probe literals since the Rust version type models no record defaults.
- `DIFF-EXPORT-SETTINGS-001` recorded. New ledger row `EXP-EXPORT-SETTINGS-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 925 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Project-export lifecycle events and the unhandled-asset abort (2026-10-05)

- Extended `src/export/project_order.rs` (the `ProjectExporter.Export` orchestration module): the four lifecycle events (`PreparationStarted/Finished`, `ExportStarted/Finished`) with source order, a `run_project_export_loop` driver reproducing the per-collection gate/warning/progress shape, and a fallible collection creator so `CreateCollection`'s no-handler throw aborts planning with `UnhandledAsset` and no partial plan. No runtime differential applies (event wiring over caller-supplied asset identities cannot execute without a full managed bundle); all behavior is source-read and directly asserted.
- New ledger row `EXP-PROJECT-EXPORT-LOOP-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 928 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## TypeTreeResolver match predicate with a runtime differential (2026-10-05)

- Extended probe `Temp/opencode/scriptidprobe` (SDK 10.0.401) to drive the internal shipped `TypeTreeResolver` through reflection: an empty reference list reports `found=0` with the not-found reason, and a matching reference with an empty `OldType` reports `found=0` with the node-build reason (proving the three-way match fired first). Both `RESOLVE` rows appended to `src/io/script_identity_fixture.txt`.
- Extended `src/processing/script_identity.rs`: `type_reference_matches_script` (ordinal three-way match with assembly-extension stripping; absent fields read as empty; default identifiers never match) and `find_type_reference_index` (first-match scan). Node building from the matched `OldType` stays with the TypeTree reader; both failure literals are pinned.
- `DIFF-SCRIPT-RESOLVER-001` recorded. New ledger row `TYPE-SCRIPT-RESOLVER-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 930 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Export file-name planning with a filesystem differential (2026-10-05)

- Probe `Temp/opencode/filenameprobe` (SDK 10.0.401) drove the shipped `AssetRipper.IO.Files.dll`: static name helpers, the internal UTF-8 truncation over ASCII/CJK/emoji edges, `Path.GetExtension`/`GetFileNameWithoutExtension` edges (`.bashrc`, `a.`, `..`), and `LocalFileSystem.GetUniqueName` against real temporary directories (missing dir, repeated collisions sharing planner state, reserved stems, overlong unicode/extensionless names); 86 rows committed as `src/io/file_names_fixture.txt` (pure ASCII via probe `\u` escapes).
- Added `src/io/file_names.rs`: suffix cleanup, host-aware invalid-character fixing (Windows set vs portable set; reserved names Windows-only, like the source), UTF-8 truncation, both path-splitting rules, a `UniqueNamePlanner` over caller-supplied existence facts with the remembered-counter map, and the `GetUniqueFileName` composition. Fixed two real bugs the differential exposed along the way: the `.`/bare-dot extension rule and pipe-bearing inputs in fixture parsing.
- Also fixed a probe/output hygiene issue found mid-increment: PowerShell `>` redirection had decoded dotnet UTF-8 output as OEM cp437; the probe now emits pure ASCII and the fixture carries `\u` escapes.
- `DIFF-FILE-NAME-001` recorded. New ledger row `EXP-FILE-NAME-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 935 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

### Ubuntu follow-up: reserved-gate test miss (2026-10-05)

- CI run `37294766135` failed only `test (ubuntu-latest)`: `unique_name_planning_matches_the_reference_runtime` asserted the probed `con_0.txt` unconditionally, but `IsReservedName` is Windows-only in the source, so the correct Linux answer is the free `con.txt`. Fixed the test to degrade exactly like the source off Windows (same pattern as the `RESERVED` rows); no production code changed. The downloaded job log confirmed 934 passed with this single failure.

## Managed type classification with a runtime differential (2026-10-05)

- Probe `Temp/opencode/monoutilsprobe` (SDK 10.0.401) drove the shipped `AssetRipper.SerializationLogic.dll` (13 `MonoUtils` predicates over a 469-cell namespace/name matrix, `ToPrimitiveType` over 268 cells, all 17 `PrimitiveType` discriminants) and `AssetRipper.Import.dll` (`GameAssetFactory.CreateEngineAsset` at 2019.4.0f1: 21 concrete versioned subclasses plus 2 rejections); all 777 rows committed as `src/processing/mono_utils_fixture.txt`.
- Added `src/processing/mono_utils.rs`: namespace consts, all 13 predicates, and `PrimitiveType` with discriminants and name parsing. Assembly loading and object construction stay outside (need live managed assemblies). The 23 `ENGINE` rows were committed as fixture evidence; the name table itself is ported next in `asset_factory.rs`.
- `DIFF-MONO-UTILS-001` recorded. New ledger row `TYPE-MONO-UTILS-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 938 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Asset-factory dispatch with an engine-table differential (2026-10-06)

- Added `src/processing/asset_factory.rs` (first audited slice of `SER-TT-ABSENT-001`): the `ReadAsset` top dispatch (pre-3.5 unreadable, class-114 MonoBehaviour, normal), the `ReadNormalObject` fallback chain with its info/warning/error log levels, the `TryReadNormalObject` tail rules (exact, zero padding, 24-byte Chinese texture tail), the Patch build-bump retry arithmetic, the `CreateAsset` three outcomes, and the 21-arm `CreateEngineAsset` table (version-taking arms plus probed 2019.4 concrete classes). Byte reading, node building, and assembly-backed structures stay outside; version facts arrive precomputed (no parsed version model exists yet).
- The 23 `ENGINE` rows of `mono_utils_fixture.txt` (probe `Temp/opencode/monoutilsprobe`, SDK 10.0.401) are re-asserted through the new table; dispatch rules are source-read over caller facts. Also corrected the previous entry's over-claim: `mono_utils.rs` never contained the engine table.
- `DIFF-ASSET-FACTORY-001` recorded. New ledger row `SER-ASSET-FACTORY-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 944 library and 40 CLI tests; warnings-denied Clippy and format check pass. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Asset-plus-meta pipeline composition (2026-10-06)

- Extended `src/export/project_output.rs` (first pipeline slice of the `.meta` recovery item): `export_asset_with_meta` mirrors `AssetExportCollection.Export` over caller facts — failed exports write nothing, asset bytes land before the `Meta(GUID, importer)` record as `{file}.meta`, both files add-only (the source overwrites; refused by design). The confined directory walk is now shared between asset and sidecar writes with no behavior change (existing sidecar tests still pass). GUIDs stay caller-supplied nonzero (source mints per-collection random GUIDs; determinism requirement wins, documented).
- New ledger row `EXP-META-PIPELINE-001` at `已实现但未验证`; targeted module tests pass (8/8), full validation via Actions after push.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Script and assembly GUIDs with a runtime differential (2026-10-06)

- Extended `src/export/script_hashing.rs`: from-scratch RFC 1321 MD5 (section A.5 suite), `UnityGuid` words/string/`MissingReference`, `ConvertSystemOrUnityBytes`, `Md5Hash` one- and three-part overloads, `CalculateScriptGuid` bytes/`str`, `CalculateAssemblyGuid` with the ordinal `.dll` strip, and `RemoveAssemblyFileExtension`/`FixAssemblyName` (case-sensitive suffix, legacy-identifier prefix). The byte layout follows the external `AssetRipper.Primitives` `UnityGuid.cs` and is pinned by 16 committed probe rows in `src/export/script_guid_fixture.txt` (shipped DLLs, SDK 10.0.401; the probe writes the file itself in UTF-8 so no console roundtrip corrupts it). The probe also proved the `.dll`-strip rule (`MyLib.dll` equals `MyLib`) and killed a hand-derived nibble-only description of the conversion.
- `src/export/guid_policy.rs` now reports the deterministic sources closed: only `RandomPerCollection` stays open.
- `DIFF-SCRIPT-GUID-001` recorded. New ledger row `EXP-SCRIPT-GUID-001` at `已实现但未验证`; targeted export tests pass (303/303); CI run `37303561833` green (all five jobs) for remote `main` = `e41e28e`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Generator audit (2026-10-06)

- Audited all six `*.SourceGenerator` projects (three parallel source reads, csproj kinds spot-verified): 4 Roslyn incremental generators — `GUI.Licensing` (license `.md` to `Licenses.g.cs`), `GUI.Localizations` (locale `.json` to `LanguageCodes.g.cs`/`Localization.g.cs`), `Processing` (hardcoded polyfills to a compiled `EmbeddedAssembly.cs` DLL blob), `SourceGenerated.Extensions` (hardcoded ClassID list to per-type `AssetCreator.g.cs` factories) — and 2 console tools — `IO.Files` (`FileSystem`/`LocalFileSystem`/`VirtualFileSystem` partial scaffolds) and `GUI` (settings-page code plus `en_US.json` normalization).
- Rust dispositions recorded in `GENERATORS-001` (now `已调查`): no C# generator runs in the Rust build; VFS behavior is hand-ported; the `AssetCreator` factory overlaps the ported engine table; license/localization/settings derivations stay with their product rows.
- No Rust source changed, so no cargo runs were needed for this increment. CI run `37304229509` green (all five jobs) for remote `main` = `704b8f0`; local matches. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Tool audit (2026-10-06)

- Audited all fifteen tool-family projects (two parallel source reads, `OutputType` and `generate.bat` spot-verified): 9 dev/diagnostic consoles (cab-map, dependence-graph, file-extract, JSON-dump, MonoBehaviour-test, raw-texture-extract, system-test harness, TypeTree-extract, API-history generator) and the 7-project `AssemblyDumper` family (manual dump, TPK download, native-enum extraction, ILSpy recompile, NuGet fix, shared utils, doc-extraction library).
- Rust dispositions recorded in `TOOLS-001` (now `已调查`): no CLI equivalent owed as product surface; never port the emitter, only output semantics per class; no doc-pipeline, downloader, or packaging equivalents.
- No Rust source changed, so no cargo runs were needed for this increment. CI run `37305025403` green (all five jobs) for remote `main` = `db0f5d1`; local matches. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Configuration framework audit (2026-10-06)

- Audited `AssetRipper.Configuration` (16 files: entry/instance/set/storage plus singleton/list stores and the string/parsable/JSON serializer selection rule) and the app wiring (`SerializedSettings` single `AssetRipper.Settings.json`, `FullConfiguration` load/save/delete, `SaveSettingsToDisk` gate in `SettingsPage`), call sites spot-verified. Records/enums/defaults are already ported in `src/export/settings.rs`; the generic keyed store needs no Rust equivalent.
- `PRODUCT-CONFIG-001` moves to `已调查`; the concrete remainder is the JSON persistence round-trip plus the save/delete gate. Also corrected the row's Rust-module column (`src/export/settings.rs`, not `src/config/`).
- No Rust source changed, so no cargo runs were needed for this increment. CI run `37305637087` green (all five jobs) for remote `main` = `f788871`; local matches. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Localization audit (2026-10-06)

- Inventoried `AssetRipper/Localizations/`: 23 locale JSON files (`en_US.json` canonical with 272 keys via JSON parse; `id.json` effectively empty, exercising the fallback). Rules read directly: default `en-US`, `LoadLanguage` validation with hyphen normalization, per-key en-US fallback with a switch-default arm (`LocalizationGenerator.cs:46-73`).
- `PRODUCT-I18N-001` moves to `已调查`; no Rust localization module exists yet — the full table port stays open.
- No Rust source changed, so no cargo runs were needed for this increment. CI run `37306221817` green (all five jobs) for remote `main` = `b1f8126`; local matches. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## GUI surface audit (2026-10-06)

- Inventoried `GUI.Free` (3-line AOT host over `GUI.Web`) and `GUI.Web` (page/route inventory, `GameFileLoader` load/export call chains, Ookii `--Port/--Log/--LogPath/--local-web-file/--Headless` flag table). Key facts: no CLI export verb exists (export is HTTP POST only), output paths reject Desktop/Documents/Downloads, exports use timestamped subfolders.
- `PRODUCT-GUI-001` moves to `已调查`. Also corrected wrong symbol names in `PRODUCT-CLI-INSPECT-001` (`LoadGame`/`ExportGame` do not exist; actual names are `LoadAndProcess`/`ExportUnityProject`/`ExportPrimaryContent`).
- No Rust source changed, so no cargo runs were needed for this increment. CI run `37306891363` green (all five jobs) for remote `main` = `6a8712d`; local matches. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Ledger duplicate repair (2026-10-06)

- The parity gate (`tools/parity-report.ps1`: 356 rows) surfaced a real defect: two rows shared ID `EXP-EXPORT-SETTINGS-001` (a records row from the settings-records increment and a discriminant-differential row from the later probe increment). Repaired by splitting per ledger practice: the records row keeps the ID with combined evidence (status honestly normalized to implemented-but-unverified — its `已验证` claim rested on 552-test-era counts), and the differential row becomes `EXP-EXPORT-SETTINGS-DIFF-001` (matching the `EXP-SCRIPT-HASHING-DIFF-001` precedent). One botched merge attempt was reverted via `git checkout` before the final split; `git diff --check` passes.
- Re-run: 0 duplicates, 0 malformed, 105 non-verified, gate still OPEN. No Rust source changed, so no cargo runs were needed for this increment. CI run `37307865982` green (all five jobs) for remote `main` = `6c2b17c`; local matches. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Settings-file round-trip with a runtime differential (2026-10-06)

- Added `src/export/settings_file.rs`: `SerializedSettings`-exact JSON writer (two-space CRLF, declaration order, no BOM/trailing newline) and a schema-scoped reader (unknown-ignored, missing-defaulted, duplicates last-wins, LF/CRLF, bounded, path-carrying errors), plus opaque `VersionText` and `from_discriminant` maps for all eleven file enums in `settings.rs`. Byte shape pinned by a shipped-DLL probe (`Temp/opencode/settingprobe2`, SDK 10.0.401) with byte-level BOM/CRLF/trailing-newline verification; both outputs committed as fixtures.
- Three test failures fixed during development (LanguageCode emitted outside the object, missing array-skipping for unknown keys, one wrong expectation path) plus two Clippy denials (`push_str` single char, single-pattern match) and a missing object depth check.
- `DIFF-SETTINGS-FILE-001` recorded. New ledger row `EXP-SETTINGS-FILE-001` at implemented-but-unverified; targeted settings tests pass (11/11); CI run `37309863852` green (all five jobs) for remote `main` = `fc93892`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Settings persistence gate (2026-10-06)

- Extended `src/export/settings_file.rs` with the pure persistence rules: `decide_settings_persistence` (form post: checked saves, unchecked deletes), `decide_settings_auto_save` (background: checked saves, unchecked leaves the file alone), and `default_settings_file_path` over the ported `join_path` (exe directory arrives as a caller fact). One new gate test covers both decisions plus Windows/Unix/empty-root paths.
- Only actual file I/O, the process directory query, and startup application stay outside now.
- Targeted settings-file tests pass (7/7); CI run `37310873342` green (all five jobs) for remote `main` = `836f203`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Settings application in export-texture (2026-10-06)

- Bridged the duplicated `ImageExportFormat` enums (`resources::image_export::from_settings_format`, all six variants pinned with extension/discriminant agreement) and threaded the configured settings format through `export-texture` as the output-extension fallback. With default settings the behavior is byte-identical to the hardcoded `Png` fallback (both `export-texture` CLI tests still pass); a settings file load (`--settings` or default path) stays outside.
- New ledger row `EXP-SETTINGS-APPLY-001` at implemented-but-unverified; targeted tests pass; CI run `37312018667` green (all five jobs) for remote `main` = `7cedac2`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## CLI settings-file loading (2026-10-06)

- `export-texture` and `export-shader` accept a leading `--settings <path>` flag (`load_cli_export_settings`: bounded read, UTF-8 check, schema parse, path-carrying errors). The configured image format backs the texture fallback and the configured shader mode backs the mode flag, with explicit flags beating the file on any order. Usage lines updated.
- Three test failures fixed during development (binary-crate paths, a `?`-type mismatch, an `ExportDefaults`-vs-file type confusion) plus one Clippy denial (while-let loop) and one wrong test expectation (the `Simple` exporter admits the test script under every mode, so the shader test now uses compiled text where mode discriminates).
- `EXP-SETTINGS-APPLY-001` broadened to cover CLI loading; targeted lib (7/7 image-export), bin (43/43), and workspace Clippy checks pass locally; CI run `37314261710` green (all five jobs) for remote `main` = `e462613`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## GLB vertex-encoding differential (2026-10-06)

- Audited `AssetRipper.Export.Modules.Models` (10 files) and probed the shipped SharpGLTF 1.0.2 writer plus its upstream sources for exact accessor encodings and skin-lane rules. Found and fixed three deviations in `src/resources/glb.rs`: `COLOR_0` FLOAT→normalized UBYTE (truncation proved at 0.5→127; non-finite poisons the vector), `JOINTS_0` always-USHORT→value-width, indices always-UINT→width-switched at the reserved 65535. Skin lanes now merge/sum, drop zeros, renormalize, sort, and pad like `SparseWeight8`/`VertexJoints4` (all-zero weights throw in the source; out-of-range joints fail explicitly here).
- Two stale test expectations updated with the differential as authority; three probe detours recorded (degenerate test triangle, misread lane widths from interleaving, UBYTE-vs-USHORT confusion on joint 300 — all resolved by the probe itself, not worked around).
- `DIFF-GLB-ENCODING-001` recorded. New rows `RES-GLB-ENCODING-001` (implemented-but-unverified) and `RES-MODEL-001` moved to implementing with corrected module mapping (no `model.rs` exists). Targeted glb tests pass (23/23), full lib suite 965/965 locally; CI run `37319285613` green (all five jobs) for remote `main` = `d7b403a`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## GLB writer size guard (2026-10-06)

- Ported `GlbWriter.TryWrite`'s too-large refusal as `GlbBuildLimits::max_output_bytes` (default 2GiB, mirroring the source 2Gb buffer ceiling): every buffer view checks the running total and the final binary-plus-JSON total is checked, yielding an explicit `OutputTooLarge` error instead of an allocation failure. `build_glb` keeps its signature over default limits via `build_glb_with_limits`. Also refreshed the stale `EXP-GLB-001` remainder (SparseWeight ordering now verified; `VertexColor1Texture8` encoding covered).
- Targeted glb tests pass (24/24), full lib suite 966/966 and bin 43/43 locally, workspace Clippy clean; CI run `37321528059` green (all five jobs) for remote `main` = `f92976c`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## CLI settings loading in export-audio (2026-10-06)

- `export-audio` accepts a leading `--settings <path>` flag (usage updated), bridging `ExportSettings.AudioExportFormat` through `plan_audio_clip_collection`: `Default` is behavior-identical, non-`ogg` decodes under `PreferWav` write through untouched, and a `PreferWav`-meets-`ogg` decode fails explicitly naming the unavailable Vorbis conversion (no output file). Missing/malformed settings files are path-carrying input errors, mirroring texture/shader.
- Three new CLI tests (Vorbis refusal, wav passthrough, missing/malformed errors) plus the eight existing audio tests pass with an explicit `Default` format; full bin suite 46/46 and lib 966/966 locally, workspace Clippy clean; CI run `37323075310` green (all five jobs) for remote `main` = `e6f616d`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Default-path settings startup load (2026-10-06)

- `SerializedSettings.TryLoadFromDefaultPath` + `FullConfiguration.LoadFromDefaultPath` are now applied at CLI startup: `export-texture`/`export-shader`/`export-audio` fall back to the exe-adjacent `AssetRipper.Settings.json` when no `--settings` flag is given (explicit flag wins; a present-but-broken default file fails loudly with a path-carrying error, mirroring the source throw). The directory-dependent core (`load_default_export_settings_from_dir`) is hermetic; only the `current_exe` query lives in the thin wrapper.
- Three new CLI tests pin absent-file-None, the full export record load (PreferWav/Bmp/Decompile discriminants), and the broken-file path error. Full bin suite 49/49 and lib 966/966 locally, workspace Clippy clean; CI run `37324101483` green (all five jobs) for remote `main` = `c4a824f`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## EXPORT-UNITY-001 orchestration audit (2026-10-06)

- Audited all 124 files of `AssetRipper.Export.UnityProjects` (24 root files plus 14 subdirectory families) via three source reads: root dispatch/collection/pointer interfaces, every family (animators, audio, mixers, scripts, textures, terrains, project YAML/scenes/prefabs/managers, path-ID map, engine/deleted/raw/misc/shader assets, extensions), the `Overrides` exporter table, and the `ExportHandler` load/process/export pipeline. Mapped each family to its Rust module and corrected the row (it pointed at a nonexistent `src/export/unity_project.rs`).
- Two findings recorded as N/A with source evidence: `Texture2DExtensions.GetKTXBaseInternalFormat` has no baseline caller; folder `.meta` sidecars are dead (no `isFolder:true` `Meta` caller).
- Ported the two remaining pure `ExportHandler` pieces into `project_order.rs`: `GetListOfVersions` (first-seen distinct versions, space-joined) and `ThrowIfSettingsDontMatch` (explicit `SettingsMismatch` error; reference identity is caller-observed). Two new tests; lib suite 968/968 locally, workspace Clippy clean; CI run `37325748243` green (all five jobs) for remote `main` = `e28249b`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## TYPE-SCRIPT-001 manager/type-resolver audit (2026-10-06)

- Audited all 21 files of `AssetRipper.Import/Structure/Assembly/` (`Managers/`, `Serializable/`, `TypeTrees/`, backend/identifier/extensions): pure metadata paths mapped to their Rust modules (`script_identity.rs`, `script_hashing.rs`, `script_export_flow.rs`, `script_admission.rs`, backend detection + `.dll` collection in `io/`); live assembly loading (AsmResolver/Cpp2Il/Cecil/ILSpy) recorded as external-tooling-blocked. Corrected the row (it pointed at nonexistent `src/types/` with `???` status).
- Ported `MonoManager.IsMonoAssembly` as a named ordinal-`.dll` predicate in `script_identity.rs`, wired into both existing call sites with no behavior change, plus a dedicated test. Lib suite 969/969 locally, workspace Clippy clean; CI run `37327106695` green (all five jobs) for remote `main` = `e9aa2bb`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## PreferOriginalTextureExtension bridge (2026-10-06)

- Ported `GetTextureExportFormat` (`ImageExportFormat::texture_export_format`: recognized original extensions win when preferred, else configured) with unit tests, and threaded `ExportSettings.PreferOriginalTextureExtension` (default true, plus explicit-file and default-path values) through `export-texture`'s unrecognized-extension fallback. Bundle-decoded assets carry no original path (`None`), so the fallback reduces to the configured format today; the rule is pinned for future original-path tracking. New CLI test proves the prefer-original settings load falls back to PNG.
- Recorded with source evidence that `export-mesh` takes no settings (neither mesh exporter reads `ExportSettings`) and YAML exporter-selection settings apply only inside full project-export dispatch (no CLI surface).
- Full bin suite 50/50 and lib 970/970 locally, workspace Clippy clean; CI run `37328509913` green (all five jobs) for remote `main` = `0b05263`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Animation umbrella audit and PathChecksumCache port (2026-10-06)

- Inventoried all 13 animation processing files plus the 2 animator-controller export files. `PROC-ANIM-001` and `RES-ANIM-001` were the last two `未调查` rows; both move to `已调查` with per-file mappings (ported vs open vs external-blocked). No stale module paths remain in the ledger (verified by search).
- Ported the game-data-free `PathChecksumCache` kernels as `src/resources/path_checksum.rs` (seeded two-way cache, hash-and-store `Add`, reverse lookup, clearing `Reset`, depth-10 dotted field recursion, caller-side population documented, CRC via the pinned `crc32_ieee`). New row `PROC-ANIM-PATH-CHECKSUM-001`; six tests; lib suite 976/976 locally, workspace Clippy clean; CI run `37329970095` green (all five jobs) for remote `main` = `cf1ef8f`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Managed-registry framing and named-vector fix (2026-10-06)

- Verified every `ManagedReferenceTypes` literal against the decoder's inline strings and rewired `decode_root` detection to the named schema constants (`managed_types::managed_reference`). Found and fixed a real deviation: the named-vector element check matched `managedReferenceArrayItem` by name alone, while the source `IsNamedVector` applies the full `IsManagedReference` predicate (either type name plus one `rid`/`id` child) — now via `is_managed_reference_element` with a six-case test.
- Ported the resolver-free `ManagedReferenceRegistryReader` framing as `src/serialized_file/managed_registry.rs` (format 1/2 dispatch, terminus triple, -1/-2 sentinel classification, counted capacity guard; negative counts refused explicitly where the source allocation throws). New row `SER-MANAGED-REGISTRY-001`; four tests. Lib suite 981/981 locally, workspace Clippy clean; CI run `37331921643` green (all five jobs) for remote `main` = `4e7f397`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Export-failure warning text and meta-pipeline dependencies (2026-10-06)

- Ported the `Failed to export '{Name}' ({TypeName})` loop warning as `format_export_failure` in `project_order.rs` with a dedicated test; recorded precise source-evidenced dependencies for the three remaining meta-pipeline items (CLI exposure needs collection dispatch; GUID freshness needs per-collection minting inside it; folder sidecars are source-dead).
- Local validation incident: cargo's freshness cache skipped rebuilds after file edits (clock skew between source mtimes and the test binary), hiding the new test until `cargo clean -p assetripper-rs` forced a rebuild. Fresh-build results: lib 982/982, bin 50/50, workspace Clippy clean; CI run `37333787247` green (all five jobs) for remote `main` = `f733870`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Localization code rules (2026-10-06)

- Ported the locale-independent `GUI.Localizations` rules as `src/localization.rs`: default `en-US`, the 23 filename-derived hyphenated codes, underscore-to-hyphen normalization, existence, and the switch-only-to-known-different `LoadLanguage` decision (null/identical/unknown are silent no-ops, so no error half). Four tests; corrected the stale `src/localization/` (nonexistent, now created) and `src/ui/` (actually `src/product.rs`) ledger mappings. String tables stay open pending a UI message consumer.
- Lib suite 986/986 locally, workspace Clippy clean; CI run `37390365686` green (all five jobs) for remote `main` = `02ca514`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Fallback hash and synthetic names (2026-10-06)

- Ported `RawDataObject.RawDataHash` (CRC32 via the pinned `crc32_ieee`), `UnknownObject.Name` (always synthetic), and `UnreadableObject.Name` (stored unless null-or-empty) into `processing/asset_factory.rs` with a dedicated test (hash word, uppercase-hex shape, stored/empty/null arms). `SER-TT-ABSENT-001` evidence updated. Lib suite 987/987 locally, workspace Clippy clean; CI run `37391118909` green (all five jobs) for remote `main` = `739822e`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Xbox360 swap rule (2026-10-06)

- Ported `Texture2DExtensions.IsSwapBytes` (Xbox360 + 7 layouts) plus the adjacent-pair swap loop into `resources/texture_format.rs` with two tests (rule matrix, pair exchange + odd-count refusal). Odd counts are explicit errors where the source loop would index out of range; the copy-before-swap stays caller-side. Lib suite 989/989 locally, workspace Clippy clean; CI run `37392170107` green (all five jobs) for remote `main` = `38a5670`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Switch GOB unswizzle (2026-10-06)

- Ported `SwitchSwizzle.Unswizzle` as `src/resources/switch_swizzle.rs` (GOB tables, block dims/sizes, padded dims, blob texel height, corrected formats, RGBA-to-RGB, bounded copies with explicit errors for the source assert/throw paths). Eight tests including byte-for-byte reproduction of 8 vectors from an independent C# model written directly from the source (`switch_swizzle_fixture.txt`, probe `Temp/opencode/swizzleprobe`, SDK 10.0.401). New row `RES-TEXTURE-SWIZZLE-001`. Lib suite 997/997 locally, workspace Clippy clean; CI run `37394130404` green (all five jobs) for remote `main` = `2ca4eda`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## ImageTexture base view (2026-10-06)

- Ported the class-189 `ImageTexture` base field view (`ImageTextureView`: width/height/image-bytes/stream-info with the `_C189` aliases) plus the inline-wins `GetImageData` selection; verified no format field, blob, or swap applies to the base (concrete-subtype `GraphicsFormat` dispatch stays open). Two tests. Lib suite 999/999 locally, workspace Clippy clean; CI run `37395035692` green (all five jobs) for remote `main` = `8111b9d`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Radiance HDR writer (2026-10-06)
- Ported StbImageWriteSharp `WriteHdr` as `Bitmap::save_hdr` in `src/resources/image_export.rs` (fixed header, integer-`frexp` RGBE conversion, per-scanline new-style RLE with the 3-run/127-chunk/128-literal rules, flat outside `8 <= width < 0x8000`), wired into `save()` so `export-texture` `.hdr` outputs now encode. Pinned byte-for-byte by 22 shipped-DLL probe vectors (`hdr_fixture.txt`, probe `Temp/opencode/hdrprobe`, SDK 10.0.401) plus targeted 32767-RLE/32768-flat boundary tests and alpha indifference. New finding `DIFF-HDR-001`; `EXP-TEXTURE-IMAGE-WRITERS-001` evidence updated (EXR/JPEG stay `Unsupported`). Lib suite 1003/1003 and bin 50/50 locally, workspace Clippy clean; CI run `37396898585` green (all five jobs) for remote `main` = `3c4e811`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## GetImageData platform orchestration (2026-10-06)

- Ported `Texture2DExtensions.GetImageData` as `Texture2DView::resolved_image_data` (inline-wins with copy-before-swap semantics, streamed swap, Switch gate on blob length + exact `Switch` equality) with `m_PlatformBlob` on the view and `SWITCH_BUILD_TARGET`/`SWITCH2_BUILD_TARGET` consts; `export-texture` threads the file prefix platform/endianness. Pinned byte-for-byte by 12 live-object probe rows (`texture_platform_fixture.txt`, probe `Temp/opencode/texplatprobe`, 2022.3 assets, SDK 10.0.401), including the Switch2 exact-equality gate, the odd-count `IndexOutOfRangeException` mapping, and both Switch vectors end-to-end. Supporting probe facts: blob field version-gated (2020.2+; absent ≤2020.1) and the `m_PlatformBlob` spelling from the shipped DLL literals. New row `RES-TEXTURE-GETIMAGEDATA-001`, new finding `DIFF-TEXTURE-GETIMAGEDATA-001`. Lib suite 1008/1008 and bin 50/50 locally, workspace Clippy clean; CI run `37398996545` green (all five jobs) for remote `main` = `e0b16aa`; local matches.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Localization conversion tables (2026-10-06)

- Made `as_hyphenated_language_code` table-driven over the four real file stems (matching the generated switch: unknown underscored codes are identity, not blanket-replaced) and ported `as_underscored_language_code` with round-trip coverage; five tests. Recorded the `LanguageNameDictionary` non-port (build-time `CultureInfo` data, UI-only consumer). Lib suite 1009/1009 and bin 50/50 locally, `cargo fmt --check` clean, workspace Clippy clean.
- CI note: runs `37399516633` (3 attempts), `37400550877` (fresh push), and manual dispatch `37400914855` all fail in 4-5s with no steps, no runner, and no logs; GitHub status shows no incident and repo Actions are enabled. Local gates (lib 1009, bin 50, fmt, Clippy) all pass. Actions loop is blocked repo-side for now; work continues with full local validation until it recovers.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Cubemap export route (2026-10-06)

- Ported the class-89 cubemap route: `CUBEMAP_CLASS_ID` plus the `texture is not ICubemap` no-flip gate threaded through `export_texture2d_image`/`export_texture2d_value` into `export-texture` (28/89 guard). Generated-API audit of all 27 versioned classes committed as `src/resources/cubemap_generated_api_fixture.txt` (probe `Temp/opencode/cubeprobe`, SDK 10.0.401): oldest and newest expose every `_C28` view field. Three tests (fixture surface, flip gate on BMP tails, end-to-end class-89 CLI export). New row `RES-TEXTURE-CUBEMAP-001`. Debugging note: the new CLI test caught a real harness bug during development (a duplicated payload block shadowed the parameterized dims, silently building 2x1 files; exact-tail assertions exposed it). Lib suite 1011/1011 and bin 51/51 locally, `cargo fmt --check` clean, workspace Clippy clean. Actions validation pending: run `37402542981` for this increment failed in 4s with no steps (loop still blocked since `37399516633`).
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Array/3D texture views and rules (2026-10-06)

- Ported the class-117/187/188 field views plus selection, size, height/depth, and Texture3D gate rules as `src/resources/texture_array.rs`, pinned by 6 live-asset probe rows (`texture_array_fixture.txt`, probe `Temp/opencode/arrayprobe`, 2022.3 assets, SDK 10.0.401) with six further unit tests. New row `RES-TEXTURE-ARRAY-001`, new finding `DIFF-TEXTURE-ARRAY-001`. Lib suite 1018/1018 locally, `cargo fmt --check` clean, workspace Clippy clean; Actions validation pending (loop still blocked).
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Array/3D export wiring (2026-10-06)

- Composed `export_texture3d_image`/`export_texture2d_array_image`/`export_cubemap_array_image` in `src/resources/texture.rs` (GraphicsFormat route, per-type flip/unpack, `TextureExportError::Array`) and dispatched classes 117/187/188 in `export-texture` with export-level PNG tests plus end-to-end CLI exports for all three. Debugging note: the tests caught two authoring errors (an incoherent 4-byte cubemap vector for six faces, and a hardcoded TypeTree node count that broke the synthetic files). Lib suite 1019/1019 and bin 52/52 locally, `cargo fmt --check` clean, workspace Clippy clean; Actions validation pending (loop still blocked).
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Sprite conversion gates (2026-10-06)

- Added `src/resources/sprite.rs` (`SPRITE_CLASS_ID`, `sprite_supported`, `sprite_try_convert_to_bitmap`) mirroring `SpriteConverter.Supported`/`TryConvertToBitmap` over caller-resolved facts: a resolved texture reports its integrity/conversion outcome, while atlas-backed and texture-less sprites fail with an empty bitmap. Three tests pin the class ID and both truth tables. Targeted `cargo test -p assetripper-rs --lib resources::sprite`: 3/3 pass locally; `cargo fmt --check` clean; full workspace and CI pending (Actions blocked repo-side by billing: jobs fail in ~2s with no runner and the `spending limit` annotation). No runtime differential applies without a managed collection. New row `RES-SPRITE-CONVERT-001`.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Sprite atlas coordinates (2026-10-06)

- Extended `src/resources/sprite.rs` with `get_sprite_coordinates_in_atlas` (`SpriteAtlasCoordinates`) mirroring `SpriteExtensions.GetSpriteCoordinatesInAtlas` over caller facts: atlas-hit rect/offset selection, size-delta crop corners, stored-or-derived pivot, and zero-preserving border shifts in source order (f32). Three exact-binary tests pin the RD-rect path, the atlas-hit override with zero-border preservation, and the derived-pivot/no-border path. Targeted `cargo test -p assetripper-rs --lib resources::sprite`: 6/6 pass locally; `cargo fmt --check` clean; targeted `cargo clippy -p assetripper-rs --lib --locked -- -D warnings` clean; full workspace and CI pending (Actions still blocked repo-side by billing). No runtime differential applies without generated sprite objects. New row `RES-SPRITE-ATLAS-001`.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Web preview image dispatch (2026-10-06)

- Added `src/resources/preview_image.rs` (`PreviewAssetKind`, `has_preview_image_data`, `preview_bitmap_source`, `preview_raw_source`) mirroring the `AssetAPI` image-region dispatch over caller facts, composing the ported `sprite_supported` gate for the sprite presence arm. Three tests pin the presence table, the bitmap switch, and the raw switch with the `ITexture2D` narrowing (including the texture-less-sprite empty case caught during authoring). Targeted `cargo test -p assetripper-rs --lib resources::preview_image`: 3/3 pass locally; `cargo fmt --check` clean; targeted lib Clippy clean; full workspace and CI pending (Actions still blocked repo-side by billing). No runtime differential applies without managed objects. New row `RES-PREVIEW-IMAGE-001`.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Full-workspace retrovalidation with a workspace-local temp dir (2026-10-06)

- The three sprite/preview increments above had only targeted-test evidence because CI was billing-blocked. A local full run (`cargo test --workspace --locked --quiet`, Rust/Cargo 1.98.1) initially reported 1008 passed / 20 failed, but every failure was `Os { code: 5, PermissionDenied }` inside `std::env::temp_dir()` creation (`project_output.rs`, `stream.rs`, `scheme.rs` fixtures) — an environment restriction on the session temp directory, not a code regression.
- Re-ran with `TEMP`/`TMP` redirected to the gitignored `target/test-tmp`: **1028 library + 52 CLI + 0 doc tests, all pass**. This retroactively supplies the missing full-workspace evidence for `RES-SPRITE-CONVERT-001`, `RES-SPRITE-ATLAS-001`, and `RES-PREVIEW-IMAGE-001`. CI (Actions) remains billing-blocked.
- No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Crunch decompressor gate (2026-10-06)

- Audited the reference Crunch path: `CrunchHandler.DecompressCrunch` dispatches solely on `IsUseUnityCrunch` (Unity >= 2017.3, else the `ETC_RGB4Crunched`/`ETC2_RGBA8Crunched` pair) to two external NuGet packages (`AssetRipper.Conversions.UnityCrunch` 1.0.3, `AssetRipper.Conversions.Crunch` 1.0.3) with no implementation in the frozen snapshot.
- Added `is_use_unity_crunch` to `src/resources/texture_converter.rs` mirroring that gate over caller version/format facts. Both decompressors stay explicit `CrunchUnsupported` errors; the gate is not yet wired into `convert_to_bitmap`, which takes no version. New row `RES-TEXTURE-CRUNCH-001`. No runtime differential applies without the external packages.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1029 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## OpenEXR writer (2026-10-06)

- Ported `ExrWriter.Write<ColorRGBA<byte>, byte>` (shipped `AssetRipper.TextureDecoder.dll` 2.6.3, probe `Temp/opencode/exrprobe`, SDK 10.0.401) as `Bitmap::save_exr` in `src/resources/image_export.rs`, wired into `save()` so `ImageExportFormat.Exr` now writes: fixed 331-byte header (A/B/G/R half-float chlist with terminator, no compression, matching data/display windows, increasing-Y order, unit aspect, centered screen window of width one), per-scanline offset table, and premultiplied half-float pixels planar in chlist order (alpha linear, RGB through the sRGB transfer on the premultiplied product). Added `f32_to_half_bits` (round-to-nearest-even) plus `rgba_byte_to_half`/`srgb_decode` helpers.
- Findings pinned by the probes: alpha is linear for all 256 values; RGB is the sRGB transfer of `byte/255 * alpha/255` (premultiplied, transfer applied to the product — verified against naive decode-then-multiply, which misses by 16-59 ULP); scanlines are channel-planar, not interleaved. Authoring notes: two phantom "anomalies" (a fake non-monotonic LUT and a fake size-dependent divergence) were both eyeball mis-splits of undelimited probe hex — the delimited re-probes and the committed fixture agree everywhere; and the whiteblack fixture row was truncated by a hand copy (340 vs 379 bytes), so it was regenerated machine-to-machine (probe file to fixture splice, LF endings preserved). New fixture `src/resources/exr_fixture.txt` (256+256 conversion halves, four complete files) and new row `RES-TEXTURE-EXR-001`; `EXP-TEXTURE-IMAGE-WRITERS-001` now records JPEG as the only unsupported writer.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1033 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## JPEG writer (2026-10-06)

- Ported StbImageWriteSharp `ImageWriter.WriteJpg` (shipped `StbImageWriteSharp.dll`, probe `Temp/opencode/jpgprobe`, SDK 10.0.401) as `save_jpg` in the new `src/resources/jpeg.rs`, wired into `save()` so `ImageExportFormat.Jpeg` now writes: fixed JFIF APP0, measured quality-90 DQT (zero quality selects it), SOF0 4:2:0, standard Annex K DHT, fixed SOS, float JFIF RGB-to-YCbCr, 2x2 box-averaged chroma with edge replication, the textbook DCT with round-half-away-from-zero quantization, zigzag scan, canonical Huffman coding with `0xFF` stuffing, and EOI. New fixture `src/resources/jpg_fixture.txt` (ten complete files, machine-copied from probe output with LF endings) and new row `RES-TEXTURE-JPEG-001`; `EXP-TEXTURE-IMAGE-WRITERS-001` now records all six formats as written.
- Calibration notes: the first implementation used a misremembered AAN DCT and passed all six flat vectors (exact-zero AC hides transform errors); the single-pixel impulse differential showed AC responses at ~2/3 of the textbook values, and coefficient-by-coefficient decoding (temporary test-only scan decoder, since removed) confirmed the reference matches the textbook `(1/4)C(u)C(v)` DCT quantized with the emitted DQT — so the direct precomputed-basis DCT replaced the AAN and all ten files match byte-for-byte, including the odd-size edge replication and the odd chroma boundary (box-average confirmed). Two more phantom "anomalies" (green-DC and red-chroma values) were hand-decoding slips (luma/chroma code-length confusion, bit misalignment), resolved the same way: trust the byte-exact tests over hand arithmetic. The luma DQT tail does not match IJG-standard q90 scaling, so both measured tables are embedded directly (provenance in comments).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1038 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Sprite importer decisions (2026-10-06)

- Ported `TextureExportCollection.AddSprites` (`AssetRipper.Export.UnityProjects/Textures/TextureExportCollection.cs`) as `decide_sprite_importer` in `src/export/importer_fields.rs`: the empty/single/multiple sprite branches over caller facts (Single-vs-Multiple via rect equality plus name match, `TextureTypeE` forcing, sprite-supplied extrude/mesh-type/pivot/border/pixels-per-unit, and the `m_exportSprites` sheet-build gate), extending row `EXP-IMPORTER-FIELDS-001`. The shipped enum discriminants were read off `AssetRipper.SourceGenerated.dll` 1.4.0 with a reflection probe (`Temp/opencode/enumprobe`, SDK 10.0.401): `SpriteImportMode` Single 1 / Multiple 2, `SpriteAlignment` Center 0 / Custom 9, `SpriteMeshType.FullRect` 0 (note: FullRect is 0, Tight is 1 — opposite to the naive guess), `TextureImporterType.Sprite` 8. `ImporterFactory.GenerateTextureImporter` defaults confirm the empty path (Single, extrude 1, pivot 0.5, 100 PPU).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1039 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round: importer data, sprite metadata, texture exporter (2026-10-06)

- Three teammates worked disjoint scopes under the lead, all verified and integrated in one increment. `texture-meta` closed the `TextureImporterData` gaps in `src/export/importer_reads.rs` without duplicating covered rows: `TextureUsageMode` + `is_normalmap_usage_mode`, shared NormalMap/Default selection, strict `MipCount > 1` and the Texture3D `(gate && >1) || (gate && flag)` gate, per-branch texture shapes (cubemap-array keeps `Texture2DArray` as written), no-streaming constants, and fixed-shape rows for the 2D-array/cubemap-array/3D tables (5 new tests; module suite 10/10).
- `sprite-fill` ported `FillSpriteMetaData` as `fill_sprite_metadata` in `src/resources/sprite.rs` (name/rect/`Custom` alignment/pivot/border copies, tessellation 0, outline/physics/bones gates, PPU bone scaling with root-bone offset, caller-supplied sprite ID; 8 new tests; module suite 14/14). New row `RES-SPRITE-METADATA-001`; mesh-vertex extraction internals stay open by baseline size.
- `texture-export` ported the `TextureAssetExporter` gates and `Export()` flow in `src/export/export_extension.rs` (`texture_exporter_exports_sprites`, admission gate, `decide_texture_asset_export`; 7 new tests; module suite 17/17). New row `EXP-TEXTURE-EXPORTER-001`. Notable correction: the frozen baseline's format selection lives in `ImageExportFormat.cs`, not `Texture2DExtensions.cs` — the port reuses the existing selection with zero duplication.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1060 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 2: asset factory, file flags, registration audit (2026-10-06)

- `texture-meta` ported the `ReadMonoBehaviour` structure-source decision (`decide_monobehaviour_structure`: thrown reads keep prior structure and win over everything; parsed TypeTree assigned on clean reads, cleared otherwise; both fallback routes to the lazy remainder), exact diagnostic texts, and the 24-byte Chinese-texture tail layout in `src/processing/asset_factory.rs` (3 new tests; module suite 10/10; extends `SER-ASSET-FACTORY-001`). Generator-emitted `CreateSerialized` stays caller-side by baseline fact.
- `sprite-fill` ported the `SerializedFile.Flags` derivation (`serialized_file_object_read_flags` in `src/serialized_file/object.rs`: `.unity`-suffix editor scene vs game release, engine/builtin-extra bits with the generation-10 cutoff, endian composition, ordinal name predicates; 5 new tests; module suite 34/34; extends `SER-TRANSFER-FLAGS-001`). `IsDestroyed` is read but never consumed by the baseline, so no gate was invented.
- `texture-export` re-audited exporter registration call-by-call (`EXP-EXPORT-OVERRIDES-AUDIT-001`): no genuine gaps — the 55-row table matches in order, gates, inheritance flags, and dummy args — pinned by a source-order test and a 30-exporter census test (module suite 8/8).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1070 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 3: audio, animator, meta emission (2026-10-06)

- `texture-meta` covered the remaining `AudioClipExtensions` surface in `src/resources/audio.rs`: the `m_Format` view field with `sound_type_discriminant`/`sound_format_discriminant` presence-as-gate accessors, pinned by upgrading the fixture test to a full 32-row `FmodSoundType` + 18-row `FmodSoundFormat` differential (module suite 8/8; extends `RES-AUDIO-001`). The decoder/exporter chain stays untouched in `export/audio_export.rs` (no duplication); FSB decode stays with its external owner.
- `sprite-fill` ported `GetStateBehaviours` slot resolution and the child-list routing kernels in `src/resources/animator_controller.rs` (checked arithmetic, exact-OOB errors, absent-list handling; 3 new tests; module suite 4/4). New row `ANIM-STATE-BEHAVIOUR-001`; `FetchEditorHierarchy` and the context rebuild stay caller-side.
- `texture-export` ported the two unported sidecar site-flow gates in `src/export/meta_emission.rs` (`decide_asset_collection_export`: sidecar iff `ExportInner` succeeds; `decide_scene_export`: warn-and-skip only when duplicate and the file exists; 2 new tests; module suite 6/6; extends `EXP-META-EMISSION-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1076 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 4: material, shader, texture (2026-10-06)

- `texture-meta` ported the two remaining Material decision units in `src/resources/material.rs`: `create_engine_material` (null shader pointer vs unresolvable vs resolved) and renderer-order `find_material_property_name_by_crc28` (2 new tests; module suite 24/24; extends `RES-MATERIAL-EXPORT-001`). Collection-side PPtr resolution stays caller-side.
- `sprite-fill` ported the serialized-shader render-state predicates in `src/resources/shader.rs` (name/float/vector/stencil/state defaults with ordinal and boundary semantics; 5 new tests; module suite 10/10). New row `RES-SHADER-STATE-001`; `Name` backing and enum-cast machinery stay caller-side.
- `texture-export` fixed a genuine bug in `src/resources/texture.rs`: `has_mips` never read `m_MipMap` and understated flag-only textures — now `MipMap_C28 || MipCount_C28 > 1` per `Texture2DExtensions.Mips`, which feeds `EnableMipMap` (2 new tests; module suite 45/45; extends `RES-TEXTURE-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1085 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 5: mesh, compressed mesh, blendshape (2026-10-06)

- `texture-meta` ported the `MeshExtensions` status + streamed-data predicates in `src/resources/mesh.rs`: `m_StreamData` view support, `is_combined_mesh` (regex-free), `has_any_vertices`, `check_asset_integrity`, channel/vertex byte-source kernels (6 new tests; module suite 35/35; extends `RES-MODEL-001`). Mutating fill/set paths stay caller-side.
- `sprite-fill` factored the shared `reconstruct_signed_z` kernel (sqrt vs renormalize fallback with divide-NaN behavior) behind both normals and tangents and named the `vertex_count` (`NumItems/3`) rule in `src/resources/compressed_mesh.rs` (3 new tests; module suite 9/9; extends `RES-COMPRESSED-MESH-001`). No compressed-mesh baseline unit remains unported.
- `texture-export` ported the `CustomCurveResolver` BlendShape branch composition (`blend_shape_attribute_name`: unknown-passthrough, first-hit-wins, caller fallback) in `src/resources/blend_shape.rs` (1 new test; module suite 9/9; extends `RES-BLENDSHAPE-001`). Frame-range selection has zero baseline readers, so no kernel was asserted.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1095 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 6: scene, prefab, scene export (2026-10-06)

- `texture-meta` ported the `SceneHierarchyObject.Create` asset-sort switch (`classify_scene_hierarchy_asset`: 8 arms incl. the attached/detached MonoBehaviour gate) and the `FetchDependencies` edge order in `src/processing/scene.rs` (2 new tests; module suite 97/97; extends `EXP-SCENE-MEMBERS-001`). Live interface checks and pointer resolution stay caller-side.
- `sprite-fill` ported the `PrefabProcessor` scene-pass `AssetBundleName` selection (`scene_hierarchy_asset_bundle_name`: unset vs bundle-name vs asset-name; 3 new tests; module suite 18/18). New row `PROC-PREFAB-SCENE-BUNDLE-001`; traversal order and `GetAssetBundleName` resolution stay caller-side.
- `texture-export` ported the scene-path `ExportableAssets` hidden exclusion (`select_scene_exportable_assets`: order-preserving, limits + allocation errors; 2 new tests; module suite 9/9; extends `EXP-SCENE-MEMBERS-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1102 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 7: serialized file, Unity YAML, workspace YAML (2026-10-06)

- `texture-meta` ported the `SerializedFileHeader.IsSerializedFileHeader` sniff (`is_serialized_file`: big-endian prefix read, defined-generation gate, extended size re-read, metadata checks; 2 new tests; module suite 13/13; extends `SER-FILE-ENTRY-001`). Short gen-22 inputs return false instead of throwing (documented hardening difference); stream-backed sniffing stays caller-side.
- `sprite-fill` ported the `YamlSerializedVersion` add/force/prepend gates and `YamlMappingNode.InsertBegin` prepend in `src/export/unity_yaml.rs` (3 new tests incl. exact emitted text; module suite 16/16; extends `EXP-YAML-DOCUMENT-001`). Typeless-data mapping assembly stays with the value converter.
- `texture-export` pinned the `IsStripped` decision shape (`is_stripped_document`: all four fact combos; 1 new test; module suite 19/19; extends `EXP-YAML-STRIPPED-FIELDS-001`). No behavior change — the workspace flow still passes unstripped until hierarchy facts are wired.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1108 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 8: UnityFS, WebFile, AssetBundle (2026-10-06)

- `texture-meta` pinned the UnityFS named enum/flag surface in `src/container/unityfs.rs` (`is_serialized_file` node flag; `compression_type_name` table; 2 new tests; module suite 13/13; extends `IO-BUNDLE-UNITYFS-ENTRY-001`). Bundle write paths stay open by design (the crate has no bundle writer).
- `sprite-fill` named the WebFile 4-byte `AlignStream` step (`entry_padding`) and covered the sniff strict-terminator rule plus zero-entry/zero-size/multi-entry round trips in `src/container/web_file.rs` (4 new tests; module suite 9/9; extends `IO-BUNDLE-WEBDATA-001`). `ResourceFile` mounting stays with the scheme dispatcher.
- `texture-export` exhaustively re-surveyed the 3.2k-line `asset_bundle.rs` (no unported behavioral decision remains), adding a module-doc coverage ledger and two pinning tests for the `EnsureStartsWithAssets` casing/prepend and trailing-separator arms (module suite 13/13; extends `PROC-ASSETBUNDLE-PATH-METADATA-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1116 library, 52 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 9: CLI, product, project output (2026-10-06)

- `texture-meta` named the CLI top-level dispatch (`CliCommand` + `classify_cli_command`: exact case-sensitive spellings for all 8 commands plus help/version) and rewired `run()` behavior-identically in `src/main.rs` (3 new tests; bin suite 55/55; extends `PRODUCT-CLI-INSPECT-001`). The web-servant surface stays out of scope (no server in this binary).
- `sprite-fill` ported the `RotateLogs` rotation decisions in `src/product.rs` (automatic-path gate, exact deletion bound with the reserved incoming-file slot, oldest-first selection; 3 new tests; extends `PRODUCT-WEB-SURFACE-001`). Directory lookup, globbing, and per-file deletion stay caller-side.
- `texture-export` exhaustively re-surveyed `project_output.rs` (no unported behavioral decision remains), adding a module-doc coverage ledger and three pinning tests for the output-confinement arms (module suite 11/11; extends `EXP-META-EMISSION-001`). Note: one targeted run without the `TEMP`/`TMP` redirect failed on system-temp writes (environmental, pre-existing); all gates use the standard redirect.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1122 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 10: filesystem, LZ4, discovery (2026-10-06)

- `texture-meta` ported the VFS text + temp surface in `src/io/filesystem.rs` (UTF-8 `read_all_text`/`write_all_text` with explicit non-UTF-8 errors; `TemporaryDirectory` getter/setter; 2 new tests; module suite 10/10; extends `IO-FS-VIRTUAL-001`). `Encoding` overloads, GUID-named temp creation, and recursive directory delete stay open.
- `sprite-fill` added the fast-level LZ4 block encoder (`compress_block`: 12-bit hash table, `L00_FAST` write-path role with exact-bytes fixtures plus round trips; 3 new tests; module suite 5/5; extends `IO-COMP-LZ4-001`). Frame container, dictionaries, and the HC parser stay open; no K4os/Unity differential has run.
- `texture-export` verified the discovery scanner's split-folding/order/contiguity placement and gave the file its first six tests in `src/io/discovery.rs` (regex shape, order/totals, split folding, bad roots, limits, logical collision; module suite 6/6; extends `IO-FS-IMPORT-DIRECTORY-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1133 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 11: animation curve, compressed curve, prefab export (2026-10-06)

- `texture-meta` ported the `Vector3f`/`Quaternionf` envelope overloads in `src/resources/animation_curve.rs` over a shared helper with the `Single` fn rewired behavior-identically (fixture differentials unchanged; 1 new test; module suite 3/3; extends `ANIM-CURVE-SETVALUES-001`). Those flavors have no `SetValues` builders in the source.
- `sprite-fill` named the slope-stride `slope_window` kernel and covered the truncation paths in `src/resources/compressed_animation_curve.rs` (3 new tests; module suite 4/4; extends `ANIM-COMPRESSED-CURVE-001`).
- `texture-export` exhaustively re-surveyed `prefab_export.rs` (no unported behavioral decision remains), adding a module-doc coverage ledger and a pinning test for the unprefixed-class-ID arm (module suite 4/4; extends `EXP-PREFAB-EXPORT-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1138 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 12: avatar, streamed curve, script export (2026-10-06)

- `texture-meta` ported the humanoid binding-name kernel in `src/resources/avatar.rs` (versioned `update_humanoid_muscle` shifts; `humanoid_muscle_attribute` for all six bands; boundary consts; 3 new tests; module suite 5/5). New row `ANIM-HUMANOID-MUSCLE-001`; out-of-range IDs return `None` where the source throws (documented).
- `sprite-fill` fixed the double-to-float narrowing to source-cast semantics (`narrow_double_to_single`: overflow yields infinities, not saturation) and pinned the buffer-absolute frame-offset contract in `src/resources/streamed_curve.rs` (2 new tests; module suite 7/7; extends `PROC-ANIM-STREAMED-001`). No finite-value behavior change.
- `texture-export` ported the script collection pointer's ordered rejections (`script_collection_pointer_target` const kernel with Display pins; 1 new test; module suite 6/6; extends `EXP-SCRIPT-EXPORT-FLOW-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1144 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 13: clip convert, mixer, audio export (2026-10-06)

- `texture-meta` ported the forward curve scan (`try_get_next_frame`) and the humanoid muscle-curve name decision in `src/resources/clip_convert.rs` (2 new tests; module suite 7/7; extends `PROC-ANIM-CONVERT-001`). Frame-buffer mechanics and emission stay caller-side.
- `sprite-fill` ported the mixer processor's pure kernels in `src/export/mixer_export.rs` (effect classification, dummy parameter names, NUL-split plugin-name parse with documented hardenings; 3 new tests; module suite 6/6; extends `EXP-MIXER-EXPORT-001`). Non-negative effect names need FMOD numerics absent from the snapshot, so they stay unported rather than guessed.
- `texture-export` pinned the two exporter warning texts as exact-text format kernels in `src/export/audio_export.rs` (1 new test; module suite 9/9; extends `RES-AUDIO-001`). OggToWav/Fmod codecs remain external-codec opens.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1150 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 14: project settings, settings file, localization (2026-10-06)

- `texture-meta` ported the `EditorFormatProcessor` PlayerSettings arm in `src/processing/project_settings.rs` (unconditional Wasm target + unsafe-code; mscorlib-driven scripting pair; 1 new test; module suite 5/5; extends `PROC-PROJECT-EDITOR-SETTINGS-DEFAULTS-001`). Enum values come from the stable UnityEditor API (external-API dependency, documented).
- `sprite-fill` ported the settings `Log()` row builders in `src/export/settings_file.rs` (exact 20-row source-order sequence for both fixtures; 2 new tests; module suite 9/9; extends `EXP-SETTINGS-FILE-001`). Logger dispatch stays caller-side.
- `texture-export` closed the localization coverage gap in `src/localization.rs` (fourth regional arm pin + 23-code existence sweep with negatives; module suite 5/5; extends `PRODUCT-I18N-001`). String tables, native names, and UI wiring stay open with cause.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1153 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 15: terrain, sprite, texture decode (2026-10-06)

- `texture-meta` ported the terrain mesh-export height grid (`mesh_heights`: row-major normalized grid over padded dims; 1 new test; module suite 7/7; extends `RES-TERRAIN-HEATMAP-001`). Vertex/UV/triangle emission, splat resolution, and Terrain shadow decisions stay caller-side.
- `sprite-fill` ported the `SpriteProcessor` tail offset kernels (`sprite_pivot_offset`, `sprite_texture_rect_offset`; 1 new test; module suite 15/15; extends `RES-SPRITE-CONVERT-001`). Atlas detachment stays caller-side; packing-settings bits stay open pending generated enum names.
- `texture-export` completed a full `TextureConverter` dispatch audit (every arm covered; SNORM-BC parity verified as deliberate, not a bug) and pinned the decoder error arms (1 new test; module suite 4/4; extends `EXP-TEXDEC-BC45-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1156 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 16: blend tree, exporter base, value YAML (2026-10-06)

- `texture-meta` ported the blend-type representation choice (`blend_tree_blend_type`: Int32 vs wrapping UInt32 with caller-side presence gate; 1 new test; companion suite 3/3; extends `ANIM-BLENDTREE-NODE-001`). Controller assembly stays in its own module.
- `sprite-fill` ported the `FailExportCollection` decisions in `src/export/exporter_bases.rs` (always-false result, exact warning text, identity/ID/pointer matrices; 1 new test; module suite 5/5; extends `EXP-EXPORTER-BASES-001`). Collection lifecycles need the object model and stay outside.
- `texture-export` fixed a genuine parity bug in `src/export/value_yaml.rs` (float/double dictionary keys now use the exact .NET formatters instead of Rust Display; 1 new test; module suite 37/37; extends `EXP-YAML-VALUE-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1159 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 17: mesh data, shader export, streamed clip (2026-10-06)

- `texture-meta` ported the `MeshData.Empty` constructor and per-channel `HasUV0`–`HasUV7` predicates in `src/resources/mesh_data.rs` (1 new test; module suite 9/9; extends `RES-MESH-DATA-001`). Field reading and mesh emission stay caller-side.
- `sprite-fill` ported the template manifest resource paths and the requirement-constructor guard in `src/export/shader_export.rs` (1 new test; module suite 9/9; extends `EXP-SHADER-EXPORT-001`). JSON loading and importer copies stay outside.
- `texture-export` pinned the `StreamedClipExtensions` edge semantics in `src/resources/streamed_clip.rs` (unchecked wrap, overflow-free fallback, default view; 1 new test; module suite 2/2; extends `PROC-ANIM-STREAMED-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1162 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 18: animator state, state machine, transition (2026-10-06)

- `texture-meta` ported the `AnimatorStateContext` grouping math in `src/resources/state_constant.rs` (default-state index, ordered index selection, path stripping; 1 new test; module suite 2/2; extends `ANIM-STATE-CONSTANT-001`). Asset creation and the path registry stay caller-side.
- `sprite-fill` pinned the transition-count edges in `src/resources/state_machine_constant.rs` (extracted `count_selector_transitions` kernel; 1 new test; module suite 2/2; extends `ANIM-STATEMACHINE-CONSTANT-001`). Context reconstruction stays outside.
- `texture-export` pinned the transition fallback edges in `src/resources/transition_constant.rs` (absent-Atomic destination, first-exit-condition wins, bare-view defaults; 1 new test; module suite 2/2; extends `ANIM-TRANSITION-CONSTANT-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1165 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 19: controller constant, muscle clip, clip (2026-10-06)

- `texture-meta` ported the presence-gated `AnimatorExtensions.GetUpdateMode` default in `src/resources/controller_constant.rs` (stored discriminant when present, else `Normal`; 1 new test; module suite 2/2; extends `ANIM-CONTROLLER-CONSTANT-001`). Clip-containment and hierarchy walks stay caller-side.
- `sprite-fill` added junk-seeded branch-coverage tests for both `MuscleClipInfo` overloads in `src/resources/muscle_clip_info.rs` (exact-change and full-clear semantics; 2 new tests; module suite 3/3; extends `ANIM-MUSCLE-CLIP-INFO-001`). Clip lists stay open elsewhere.
- `texture-export` pinned the truncated-tail stride tolerance and present-but-empty value arrays in `src/resources/clip.rs` (1 new test; module suite 2/2; extends `ANIM-CLIP-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1169 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 20: avatar constant, dense clip, constant clip (2026-10-06)

- `texture-meta` ported the `CreateMotion` leaf-index math in `src/resources/avatar_constant.rs` (first-match `IndexOffset + position`, -1 on miss; 1 new test; module suite 2/2; extends `EXP-AVATAR-CONSTANT-001`). Asset creation and clip resolution stay caller-side.
- `sprite-fill` ported the `ProcessDenses` sample-walk kernels in `src/resources/dense_clip.rs` (active gate, checked sample positions, zero-slope defaults; 1 new test; module suite 2/2; extends `ANIM-DENSE-CLIP-001`). Binding classification and curve emission stay with the converter modules.
- `texture-export` pinned the `IsSet` count extremes in `src/resources/constant_clip.rs` (default/1/`usize::MAX`; 1 new test; module suite 2/2; extends `ANIM-CONSTANT-CLIP-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1172 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 21: animator enums, curve wrappers, keyframe (2026-10-06)

- `texture-meta` ported the `InitializeLayer` synced-layer decision in `src/resources/animator_enums.rs` (`synced_layer_index`: -1 unsynchronized, else wrapping index; 1 new test; module suite 2/2; extends `PROC-ANIM-001`). Parameter-default dispatch stays out (external enum values undeclared in the baseline).
- `sprite-fill` added junk-seeded wrapper coverage in `src/resources/curve_wrappers.rs` (stale-value overwrite, envelope reset, identity-field overwrite; 1 new test; module suite 2/2; extends `ANIM-CURVE-SETVALUES-001`). Script collection resolution stays caller-side.
- `texture-export` pinned the `DefaultFloatWeight` bits and overload shapes in `src/resources/keyframe.rs` (weight-only preserves slopes, slope overload writes them; 1 new test; module suite 2/2; extends `ANIM-KEYFRAME-DEFAULT-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1175 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 22: vector, keyframe v3/q, bounds (2026-10-06)

- `texture-meta` ported the `VectorParameterExtensions` in `src/resources/vector.rs` (`SetValues` + `GetType_` with baseline-pinned `ShaderParamType` discriminants; 1 new test; module suite 3/3; extends `EXP-VECTOR-001`). Shader parameter consumption stays caller-side.
- `sprite-fill` added junk-seeded exact-write tests for both keyframe partials (`keyframe_vector3.rs`, `keyframe_quaternion.rs`: weight-only resets with slope preservation, scalar broadcast, slopes-quat path; 1 new test per file; suites 2/2 each; extend `ANIM-KEYFRAME-V3-001` / `ANIM-KEYFRAME-Q-001`).
- `texture-export` ported the implicit `Bounds(AABB)` conversion and `CopyTo` in `src/resources/bounds.rs` (bit-exact round-trip test; 1 new test; module suite 2/2; extends `EXP-BOUNDS-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1179 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 23: quaternion, matrix, color (2026-10-06)

- `texture-meta` ported the editor euler-hint application in `src/resources/quaternion.rs` (`euler_hint_from_rotation` via the degrees `ToEulerAngle` path; 1 new test; module suite 2/2; extends `NUM-ROTATION-001`). Hint-field presence and transform assembly stay caller-side.
- `sprite-fill` ported the `MatrixParameterExtensions` in `src/resources/matrix4x4.rs` (`SetValues` + `GetType_` mirroring the vector-parameter shape; 1 new test; module suite 2/2; extends `EXP-MATRIX-001`). `ColumnCount` stays with the generated subclass default.
- `texture-export` ported the `ToColorFloat` composition kernel in `src/resources/color.rs` (reusing the file's own getters; 1 new test; module suite 2/2; closes the `ToColorFloat` open in `EXP-COLOR-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1182 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 24: rect, tangent mode, gradient (2026-10-06)

- `texture-meta` ported the sprite physics-shape point math in `src/resources/rectf.rs` (PPU scale-then-shift with pivot shift; 1 new test; module suite 2/2; extends `EXP-RECTF-001`). Shape assembly and the rotation fixup stay caller-side.
- `sprite-fill` pinned the full named mix table in `src/resources/tangent_mode.rs` (high parts + ten broken-bit mixes with derivation docs; 1 new test; module suite 3/3; extends `ANIM-TANGENT-MODE-001`). The full `TangentMode` surface is now pinned.
- `texture-export` fixed a genuine omission in `src/resources/min_max_gradient.rs` (missing fifth variant `RandomColor = 4` with decoder arm and round-trip coverage; 1 new test; module suite 2/2; extends `EXP-MINMAXGRADIENT-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1185 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 25: game object, transform, submesh (2026-10-06)

- `texture-meta` ported the `ShouldBeActive` rule and the `GetRootDepth` father walk in `src/resources/game_object.rs` (cycle/out-of-table errors instead of the source's unbounded loop; 2 new tests; module suite 3/3; extends `EXP-GAMEOBJECT-ACTIVE-001`). Component resolution and asset creation stay caller-side.
- `sprite-fill` ported `ToTransformation`/`ToInverseTransformation` in `src/resources/transform.rs` (composing the verified `Transformation::create`/`create_inverse`; 1 new test; module suite 3/3; extends `EXP-TRANSFORM-HIERARCHY-001`). `InitializeDefault` remains unported.
- `texture-export` pinned the submesh edge semantics in `src/resources/sub_mesh_data.rs` (unknown-name rejection, first-index truncation, unchecked-multiplication wrap parity; 1 new test; module suite 3/3; extends `RES-SUBMESH-DATA-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1189 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 26: packed float, shader channel, GL texture (2026-10-06)

- `texture-meta` ported the generic `Pack<T>` word path in `src/resources/packed_float_vector.rs` (`pack_words` delegating to the exact float pack; baseline-mirroring sweep over bit sizes 1–32; 1 new test; module suite 3/3; extends `RES-PACKED-FLOAT-001`). Byte ownership stays caller-side.
- `sprite-fill` ported the `ChannelInfo` dimension nibble rules in `src/resources/shader_channel.rs` (`IsSet`, low-nibble read, high-nibble-preserving write; existing differential rewired through the kernel; 1 new test; module suite 2/2; extends `RES-SHADER-CHANNEL-001`). The full `ChannelInfo` surface is now pinned.
- `texture-export` pinned the cross-layout copy absence rules in `src/resources/gl_texture_settings.rs` (absent-dest stays absent, missing-source reads zero, scalar unconditionality; 1 new test; module suite 2/2; extends `EXP-GLTEXTURESETTINGS-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1192 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 27: packed bit, packed quat, vertex blob (2026-10-06)

- `texture-meta` added the baseline suite's own guarantees in `src/resources/packed_bit_vector.rs` (full 1–31 bit-width round trips, signed/unsigned consistency, ushort boundaries, non-negative round trips; 1 new test; module suite 3/3; extends `RES-PACKED-VECTOR-001`). The packed-int surface is now covered member-for-member plus test guarantees.
- `sprite-fill` pinned every error arm and the index fallthrough in `src/resources/packed_quat_vector.rs` (empty round trip, limit gates, exact truncation points, out-of-range W addressing; 1 new test; module suite 3/3; extends `RES-PACKED-QUAT-001`). The full packed-quat surface is now pinned.
- `texture-export` pinned the counter-intuitive shared-read quirks in `src/resources/vertex_data_blob.rs` (SInt8 zero-extends, UInt16 sign-extends, SNorm clamp extremes; 1 new test; module suite 3/3; extends `RES-VERTEX-DATA-BLOB-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1195 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 28: vertex extensions, PPtr, streaming info (2026-10-06)

- `texture-meta` ported the missing `IsSet` predicate in `src/resources/vertex_data_extensions.rs` (exact predicate over caller facts with the streaming-info boundary documented; 1 new test; module suite 3/3; extends `RES-VERTEX-DATA-BLOB-001`). The extension surface is now covered member-for-member.
- `sprite-fill` pinned the residual `is_null`/`is_asset` edges in `src/resources/pptr.rs` (file-id-ignoring nullness, negative-file-id hardening; extended existing test; module suite 1/1; extends `EXP-PPTR-001`). `TryGetAsset` stays caller-side.
- `texture-export` pinned the view-level `set_values`/`get_values` round-trips and truncation parity in `src/resources/streaming_info.rs` (old-layout `u64::MAX`→`u32::MAX`, cross-layout narrowing; 1 new test; module suite 2/2; extends `IO-RESOURCE-STREAMINGINFO-FIELDS-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1197 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 29: mesh metric, shader, material (2026-10-06)

- Batch note: the original `mesh_view.rs`/`index_format.rs`/`bone_influence.rs` files do not exist (all three teammates stopped and asked instead of guessing; the IndexFormat and BoneWeight domains are confirmed covered in `sub_mesh_data.rs`/`numerics.rs`). All three were retargeted to verified-existing files before any write.
- `texture-meta` ported the processed-index lane/byte codec in `src/resources/mesh_metric.rs` (guarded lane conversions mirroring the source range exception, explicit-endian byte assembly; 2 new tests; module suite 13/13; extends `RES-MESH-METRIC-001`). Fill assembly and collection writes stay caller-side.
- `sprite-fill` ported the serialized-pass program selection in `src/resources/shader.rs` (`ShaderProgramType` discriminants/masks/strings, ordered selection with the ray-tracing gate, `SerializedPassType` parsing; 1 new test; extends `RES-SHADER-STATE-001`). `MaxShaderModelVersion` stays open.
- `texture-export` pinned the scalar contracts in `src/resources/material.rs` (negative rejection, U8-bool coercion, U32 reinterpretation, invalid-UTF8 errors, absent-shape arms; 1 new test; module suite 25/25; extends `RES-MATERIAL-VIEW-001`). Also flagged unported renderer getters for a future renderer module (tracked, not added).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1201 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 30: texture format, preview image, audio (2026-10-06)

- `texture-meta` ported the `TextureSettingsExtensions` version-union readers/writers in `src/resources/texture_format.rs` (`TextureSettingsView` with 7 presence-gated getters and 6 dual-spelling setters; 1 new test; module suite 4/4; extends `EXP-TEXTURE-FORMAT-001`). Enum-valued members stay caller-side.
- `sprite-fill` ported the endpoint URL facts and `GetImageData` response routing in `src/resources/preview_image.rs` (URL/query consts, extension suffix, bitmap/raw/404 matrix; 1 new test; module suite 4/4; extends `RES-PREVIEW-IMAGE-001`). Path encoding, bitmap encoding, and HTTP plumbing stay caller-side.
- `texture-export` pinned the uncovered branch combos and reader arms in `src/resources/audio.rs` (empty-inline-plus-resource fallthrough for both selection and integrity, U8 coercion, U32 wrap, malformed-resource errors, `WrongType` display; 2 new tests; module suite 10/10; extends `RES-AUDIO-001`). Review note: one new test-only `panic!` follows the file's existing test idiom (assertion guard inside `mod tests`, not a product error path).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1205 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 31: ETC decode, texture converter, texture array (2026-10-06)

- `texture-meta` closed the partial-block grid and buffer-bound hole in `src/resources/etc_decode.rs` (hand-derived uniform-block vectors: 5x5/1x1/6x3 ceiling-grid accounting, short-buffer errors; 1 new test; module suite 2/2; extends `EXP-TEXDEC-ETC-001`). No new kernels were needed; the decoder surface was already complete.
- `sprite-fill` closed the residual converter gap in `src/resources/texture_converter.rs` (zero-dimension guard, `i32::MAX` size guard, Texture3D `-1`-stride path, wrapper flag wiring; 1 new test; module suite 12/12; extends `EXP-TEXTCONV-DISPATCH-001`). Crunch stays documented.
- `texture-export` pinned the uncovered union branches in `src/resources/texture_array.rs` (both-present precedence, absent-format `RGBA32` fallback, lightmap/mip passthrough, unrepresentable-depth retention; 1 new test; module suite 8/8; extends `RES-TEXTURE-ARRAY-001`). The `Format` setter stays justifiably unported (no baseline callers).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1208 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 32: ASTC, PVRTC, sprite (2026-10-06)

- `texture-meta` closed the partial-grid/footprint-gate/bounds hole in `src/resources/astc.rs` (hand-derived void-extent vectors: 6x6-on-4x4, single-block large footprints, five rejected dimensions, short-buffer errors; 1 new test; module suite 2/2; extends `EXP-TEXDEC-ASTC-001`). No new kernels were needed; the decoder surface was already complete.
- `sprite-fill` pinned the buffer-guard arms and sub-block floor in `src/resources/pvrtc.rs` (exact `InputTooShort`/`OutputTooSmall`, 32-byte 2x2-block floor for 4x4-4bpp and 8x4-2bpp; 1 new test; module suite 2/2; extends `EXP-TEXDEC-PVRTC-001`). The PVRTC surface is fully pinned.
- `texture-export` closed the file's own documented `SettingsRaw` open in `src/resources/sprite.rs` (4 bit-decode kernels as raw discriminants: packed flag, mode, rotation, mesh type; 1 new test over crafted words; module suite 16/16; extends `RES-SPRITE-ATLAS-001`). Only packing-enum member names stay open.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1211 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 33: BC6H, BC7, ATC/YUY2 (2026-10-06)

- `texture-meta` covered the signed path in `src/resources/bc6h.rs` (hand-traced mode-10 block: signed clamps to opaque black vs unsigned-saturated red; all-zero 5x5 grid; short-buffer errors both paths; 1 new test; module suite 2/2; extends `EXP-TEXDEC-BC6H-001`). No new kernels were needed; the decoder surface was already complete.
- `sprite-fill` pinned the reserved mode, partial grids, and buffer guards in `src/resources/bc7.rs` (all-zero stream → opaque blocks, 6x5 → 64 bytes, short-buffer errors, empty-image `Ok(0)`; 1 new test; module suite 2/2; extends `EXP-TEXDEC-BC7-001`). The BC7 surface is fully pinned.
- `texture-export` pinned the ragged-block geometry, odd-width drop, and error arms in `src/resources/atc_yuy2.rs` (5x3 copy geometry against the validated block path, final-column drop, exact bounds per entry point; 1 new test; module suite 2/2; extends `EXP-TEXDEC-ATC-001`). Lead fixed one `clippy::identity_op` in the new test (`5 * 1 * 4` → `5 * 4`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1214 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 34: Switch swizzle, graphics format, texture (2026-10-06)

- `texture-meta` pinned the full block tables row-for-row in `src/resources/switch_swizzle.rs` (all 26 flat formats, six 4x4 blocks, 18 ASTC footprints, 32 byte-size rows, back-out set, five RGB→RGBA corrections; 1 new test; module suite 9/9; extends `RES-TEXTURE-SWIZZLE-001`). No new kernels were needed; this round pins the table literals.
- `sprite-fill` pinned the negative-input fallthrough and error displays in `src/resources/graphics_format.rs` (`-1`/`i32::MIN` → `ArgumentOutOfRange`, three `Display` strings verbatim; 1 new test; module suite 2/2; extends `EXP-GRAPHICS-FORMAT-001`). The GraphicsFormat surface is fully pinned.
- `texture-export` closed the last untested `ActualImageSize` arm in `src/resources/texture.rs` (ragged layers keep the whole length; extended existing test; extends `RES-TEXTURE-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1216 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 35: blend shape, terrain heatmap, custom curve (2026-10-06)

- `texture-meta` ported the portable `SetValues` hash half in `src/resources/blend_shape.rs` (`blend_shape_channel_name_hash` with writer/reader composition; 1 new test; module suite 10/10; extends `RES-BLENDSHAPE-001`). Generated-object writes stay caller-side.
- `sprite-fill` pinned every field-error arm and error display in `src/resources/terrain_heatmap.rs` (negative dims, `WrongType` shapes, `U32` acceptance, duplicate-name lookup, overflow on both consumers, six `Display` strings; 2 new tests; module suite 9/9; extends `RES-TERRAIN-HEATMAP-001`). The heatmap surface is fully pinned.
- `texture-export` pinned the unpinned contracts in `src/resources/custom_curve.rs` (bit-31-wins precedence, 31–36 constraint range, three `Display` spellings; 1 new test; module suite 9/9; extends `PROC-ANIM-CUSTOM-CURVE-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1220 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 36: mesh, avatar, clip (2026-10-06)

- `texture-meta` ported the `SetIndexFormat` write gate and `Is16BitIndices` predicate in `src/resources/mesh.rs` (field-exists write, silent 16-bit no-op, `UnsupportedIndexFormat` mirroring the source `NotSupportedException`; 2 new tests; module suite 40/40; extends `RES-MESH-VIEW-001`). Flag/compression setters and `Fill` stay caller-side.
- `sprite-fill` pinned the out-of-range muscle update/read divergence in `src/resources/avatar.rs` (update arithmetic asserted past `Last`, then reads `None` where the source throws; 1 new test; module suite 7/7; extends `EXP-AVATAR-BONEPATH-001`). `BoneType.Update` stays deliberately unported (no baseline callers).
- `texture-export` pinned the positional stride-skip semantics in `src/resources/clip.rs` (mid-stride CRC entries swallowed, never reclassified; 1 new test; module suite 3/3; extends `ANIM-CLIP-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1224 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 37: compressed mesh, streamed curve, keyframe (2026-10-06)

- `texture-meta` pinned the set-side gates and error branches in `src/resources/compressed_mesh.rs` (channel mismatch, gateless fast-path zeroes, empty-skin reset, gateless bind-pose/color reads, sign-less normal failure; 1 new test; module suite 10/10; extends `RES-COMPRESSED-MESH-001`). No new kernels were needed; the member surface was already fully ported.
- `sprite-fill` pinned the `HasDifferentLimits` estimated/exact arms and the `SetNegInfSlope` rewrite arm in `src/resources/streamed_curve.rs` (far-estimate rewrite, near-threshold continuity, exact-limit discontinuity, negative-infinity flag quirk; 1 new test; module suite 8/8; extends `PROC-ANIM-STREAMED-001`). The streamed-curve surface is fully pinned.
- `texture-export` pinned the 2018.1 presence boundary and the weighted-mode literal in `src/resources/keyframe.rs` (2017.4-absent vs 2018.1-present, `WeightedMode.None == 0`; 1 new test; module suite 3/3; extends `ANIM-KEYFRAME-DEFAULT-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1227 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 38: animation curve, compressed curve, curve wrappers (2026-10-06)

- `texture-meta` closed the cross-gate copy hole in `src/resources/animation_curve.rs` (pre-5.4 rotation slot, pre-2018.1 weights, both version directions; 1 new test; module suite 4/4; extends `ANIM-CURVE-SETVALUES-001`). No new kernels were needed; the member surface was already complete.
- `sprite-fill` pinned the error reporting in `src/resources/compressed_animation_curve.rs` (all five `Display` strings verbatim, `source()` contract; 1 new test; module suite 5/5; extends `ANIM-COMPRESSED-CURVE-001`). The compressed-curve surface is fully pinned.
- `texture-export` pinned the last unpinned reset direction in `src/resources/curve_wrappers.rs` (plain-Vector3 junk overwrite; extended existing test; module suite 2/2; extends `ANIM-CURVE-WRAPPERS-001`).
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1229 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 39: gate-first round (animator, GLB, x-file refs, scene, clip emission) (2026-10-06)

- Lead redirected the round mid-flight under the new gate-first policy: tasks 115–119 (all on verified rows) were superseded before delivery with zero code landed; replaced by tasks 120–124, all on incomplete rows.
- `texture-meta` ported the enum-free state-context decisions in `src/resources/animator_enums.rs` (`default_state_index`, `has_transition_destination`, Unity-5-only `is_state_machine_destination`; 1 new test; module suite 3/3; advances `PROC-ANIM-001`).
- `sprite-fill` ported the `GlbLevelBuilder` hierarchy-export dispatch in `src/resources/glb.rs` (size guard already ported; `references_dynamic_mesh` precedence, `subset_indices` selection; 1 new test; module suite 25/25; advances `RES-MODEL-001`).
- `texture-export` closed the engine-resource alias gap in `src/serialized_file/reference_graph.rs` (`engine_resource_alias`: exact wins, 0→missing, 1→resolve, N→ambiguous; 1 new test; module suite 12/12; advances `OBJ-REF-001`). GUID matching deliberately excluded (no baseline evidence).
- `mesh-deform` ported the export-time root filter in `src/processing/scene.rs` (`is_scene_hierarchy_root`, order-preserving `scene_hierarchy_root_indexes`; 2 new tests; module suite 99/99; advances `PROC-SCENE-001`).
- `anim-graph` ported the default-curve emission routing in `src/resources/clip_convert.rs` (`default_curve_target`, `keyframe_kind`; 2 new tests; module suite 9/9; advances `PROC-ANIM-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1236 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 40: gate-first round (UnityFS, raw/web, export loop, prefab, animator) (2026-10-06)

- `texture-meta` ported the metadata-placement branches in `src/container/unityfs.rs` (`BUNDLE_VERSION_LARGE_FILES_SUPPORT = 7`, directory-less metadata, pre-alignment v6 end placement; 1 new test; module suite 14/14; advances `IO-BUNDLE-UNITYFS-META-001`).
- `sprite-fill` pinned the corrupt-header guards in `src/container/raw_web.rs` (negative version/size, consumed-vs-declared mismatch with context strings; 1 new test; module suite 7/7; advances `IO-BUNDLE-RAW-WEB-HEADER-001`).
- `texture-export` pinned the `FixedPathError` diagnostics in `src/export/project_plan.rs` (empty-scene-path + allocation; 1 new test; module suite 5/5; advances `EXP-PROJECT-EXPORT-LOOP-001`). Adjacent loop/collection behaviors verified covered elsewhere, nothing duplicated.
- `mesh-deform` ported the main-asset/duplicate refusal gate in `src/export/prefab_export.rs` (`prefab_gather_admits`, refused members consume no ID seeds; 2 new tests; module suite 6/6; advances `PROC-PREFAB-SCENE-BUNDLE-001`).
- `anim-graph` ported the full `TryGetDestinationState` decision table in `src/resources/animator_controller.rs` (`decode_transition_destination` with sentinel/state/table-less/OOB/port arms; 1 new test; module suite 5/5; advances `RES-ANIM-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1242 library, 55 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 41: gate-first round (LZMA, zstd, scheme, file entry, CLI) (2026-10-06)

- `texture-meta` pinned the output-size contract in `src/compression/lzma.rs` (short stream hard-errors, long stream truncates like the source loop, decoder contract probed empirically; 1 new test; module suite 7/7; advances `IO-COMP-LZMA-UNITYFS-001`).
- `sprite-fill` pinned the boundary guards in `src/compression/zstd.rs` (short-signature no-throw reads, unreservable-allocation fail-fast; 1 new test; module suite 6/6; advances `IO-COMP-ZSTD-UNITYFS-001`).
- `texture-export` pinned the detection edges in `src/scheme.rs` (empty/short/wrapped inputs, wrapped-vs-bare readability; 1 new test; module suite 41/41; advances `IO-SCHEME-001`).
- `mesh-deform` wired the file-level Flags entry point in `src/serialized_file/file.rs` (`object_read_flags` forwarding parsed facts into the tested kernel; 1 new test; module suite 14/14; advances `SER-FILE-ENTRY-001`).
- `anim-graph` ported and wired the `LoadAndProcess` process gate in `src/main.rs` (`plan_load_process`, both inspect emptiness gates behavior-identical; 1 new test; bin suite 56/56; advances `PRODUCT-CLI-INSPECT-001`). The two intermediate dead-code warnings were resolved by the wiring itself.
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1246 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 42: gate-first round (VFS, mixed structure, meta objects, dep list, meta prefix) (2026-10-06)

- `texture-meta` ported the `GetUniqueName` length stage in `src/io/filesystem.rs` (extension-preserving UTF-8 truncation, all 12 baseline expectations; 1 new test; module suite 11/11; advances `IO-FS-VIRTUAL-001`).
- `sprite-fill` pinned the `CollectWebFiles` extension switch in `src/io/game_structure.rs` (`.datagz`/`.data.unityweb`/bare-`.unityweb` arms; 1 new test; module suite 5/5; advances `IO-STRUCTURE-MIXED-001`).
- `texture-export` pinned the modern population and rejection arms in `src/serialized_file/metadata.rs` (gen-19 fields, negative index, ambiguous scan, negative offset/size; 1 new test; module suite 17/17; advances `SER-META-OBJECTS-001`). ClassID backfill deliberately stays absent (writer invariant).
- `mesh-deform` ported the collection dependency-list slots in `src/serialized_file/collection_workspace.rs` (`initialize_collection_dependency_slots` with already-initialized guard; 2 new tests; module suite 2/2; advances `SER-REF-GRAPH-001`).
- `anim-graph` ported the prefix emission plan in `src/serialized_file/metadata_prefix.rs` (`MetadataPrefixEmission` over generation presence gates; 2 new tests; module suite 6/6; advances `SER-META-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1253 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 43: gate-first round (LZ4, resource, desktop, meta types, meta pipeline) (2026-10-06)

- `texture-meta` built the K4os reference differential in `src/compression/lz4_block.rs` (9 byte-for-byte vectors from `K4os.Compression.LZ4` 1.3.8 via a throwaway offline probe; 1 new test; module suite 6/6; advances `IO-COMP-LZ4-001`, closing its decode-direction verification gap).
- `sprite-fill` ported the single unported member in `src/resource.rs` (`is_default_resource_file` over `.resource`/`.ress` extensions; 1 new test; module suite 15/15; advances `IO-RESOURCE-001`).
- `texture-export` pinned the untested baseline-faithful arms in `src/io/platform_structure.rs` (Windows fallback naming, direct Linux selection, named-pair wins, empty-shape rejections; 1 new test; module suite 11/11; advances `IO-STRUCTURE-DESKTOP-001`).
- `mesh-deform` named the `readScriptID` presence gate in `src/serialized_file/metadata.rs` (`should_read_type_script_id`, rewired with zero behavior change; 1 new test; module suite 18/18; advances `SER-META-TYPES-001`).
- `anim-graph` ported the single-asset reference trio in `src/export/project_output.rs` (`single_asset_contains`/`single_asset_export_id`/`single_asset_export_pointer`; 1 new test; module suite 12/12; advances `EXP-META-PIPELINE-001`). Dead `ExportAsset` helper deliberately not ported (zero baseline callers).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1258 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 44: gate-first round (partial stream, Android, script identity, mono utils, platform files) (2026-10-06)

- `texture-meta` ported the write path in `src/io/stream.rs` (`Write` impl with full-count bound check, past-end failure, `SetLength` unsupported per source; 1 new test; module suite 18/18; advances `IO-STREAM-PARTIAL-001`, closing its read-only gap).
- `sprite-fill` ported the constructor directory layout in `src/io/platform_structure.rs` (`android_data_layout` joins; 1 new test; module suite 12/12; advances `IO-PLATFORM-ANDROID-001`). Dead `GetAndroidUnityAssemblyPath` deliberately not ported (zero baseline callers).
- `texture-export` ported the `GetPropertiesHash` fallback in `src/processing/script_identity.rs` (`properties_hash_fallback_bytes`; 1 new test; module suite 9/9; advances `TYPE-SCRIPT-001`). `HasAssemblyName` deliberately excluded (gate semantics unverifiable without generated sources).
- `mesh-deform` ported the enum-unwrap branch in `src/processing/mono_utils.rs` (`enum_backing_type_for_primitive` over `value__` facts; 1 new test; module suite 4/4; advances `TYPE-MONO-UTILS-001`).
- `anim-graph` ported the `CollectMainAssemblies` dispatch in `src/io/platform_file_inventory.rs` (`plan_mono_assembly_collection`, rewired behavior-identically; 1 new test; module suite 13/13; advances `IO-PLATFORM-FILES-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1263 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 45: gate-first round (smart stream, iOS, managed refs, asset factory, web data) (2026-10-06)

- `texture-meta` pinned the null-member matrix in `src/io/stream.rs` (reference `SmartStreamTests` disposed/null contract; 1 new test; module suite 19/19; advances `IO-STREAM-SMART-001`).
- `sprite-fill` ported the constructor directory layout in `src/io/platform_structure.rs` (`ios_data_layout` joins plus the app-binary-is-assembly rule; 1 new test; module suite 13/13; advances `IO-PLATFORM-IOS-001`).
- `texture-export` ported the `IsIndexedManagedReference` distinguisher in `src/serialized_file/object.rs` (legacy-`id` vs modern-`rid` field shape; 1 new test; module suite 35/35; advances `SER-MANAGED-REFS-001`). Integration Clippy caught the kernel as test-only dead code; follow-up task-155 wired it into the `decode_root` production path (flavor recorded on the local diagnostic, no behavior change, no `allow(dead_code)`).
- `mesh-deform` ported the tolerance gate in `src/processing/asset_factory.rs` (`is_engine_collection_name` combined port with end-to-end composition; 1 new test; module suite 11/11; advances `SER-ASSET-FACTORY-001`).
- `anim-graph` ported the entry display normalization in `src/container/web_file.rs` (`Display for WebFileEntry` via `fix_file_identifier`; 1 new test; module suite 10/10; advances `IO-BUNDLE-WEBDATA-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1268 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 46: gate-first round (multifile stream, Switch, registry, stream input, settings) (2026-10-06)

- `texture-meta` hardened the part-name trust boundary in `src/io/stream.rs` (`split_part_index` strict `{base}.split<digits>` decision with decoy rejection, rewired into `open_split_file`; 2 new tests incl. a disk-level decoy test; module suite 21/21; advances `IO-STREAM-MULTIFILE-001`).
- `sprite-fill` ported the constructor directory layout in `src/io/platform_structure.rs` (`switch_data_layout` with streaming-under-gamedata and `exefs/main`-assembly rules, wired into `detect_switch` behavior-identically; 1 new test; module suite 14/14; advances `IO-PLATFORM-SWITCH-001`).
- `texture-export` ported the registry content gate in `src/serialized_file/managed_registry.rs` (`referenced_object_has_content` with `frame_registry_entry` production framing; 1 new test; module suite 5/5; advances `SER-MANAGED-REFS-001`).
- `mesh-deform` named the container/leaf partition in `src/scheme.rs` (`is_container_kind`, wired at all three production branch sites; 2 new tests; module suite 43/43; advances `IO-STREAM-INPUT-001`).
- `anim-graph` ported the `IgnoreStreamingAssets` get/set shorthand in `src/export/settings.rs` (`skip_streaming_assets`/`ignore_streaming_assets`; 1 new test; module suite 16/16; advances `PRODUCT-CONFIG-001`). Consumer wiring stays open (outside scope, recorded in code docs).
- Standing rule applied from the round-45 fix-up: every new kernel ships with a real production caller (except `pub` exported API following the file pattern); the round-46 `panic!` hit is a `#[test]`-only assertion guard (batch-30 precedent).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1275 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 47: gate-first round (import directory, PS4, absent typetree, references) (2026-10-06)

- `texture-meta` ported the mixed import expansion in `src/io/discovery.rs` (`expand_import_paths`: files pass through, directories reuse bounded discovery, missing paths skip-counted; 2 new tests; module suite 8/8; advances `IO-FS-IMPORT-DIRECTORY-001`).
- `sprite-fill` ported the constructor directory layout in `src/io/platform_structure.rs` (`ps4_data_layout`: `Modules`/PRX plus `Data/Metadata`-direct rules, wired into `detect_ps4` behavior-identically; 1 new test; module suite 15/15; advances `IO-PLATFORM-PS4-001`).
- `texture-export` ported the reader-side decode fact in `src/serialized_file/object.rs` (`TypeTreeUsability` classification, wired as the prelude decision; 1 new test; module suite 36/36; advances `SER-TT-ABSENT-001`, reader lane).
- `mesh-deform` ported the factory-side composition in `src/processing/asset_factory.rs` (`decide_try_read_object` over creation/throw/tail facts; 1 new test; module suite 12/12; advances `SER-TT-ABSENT-001`, factory lane, no duplication with the reader lane).
- `anim-graph` ported the write-side construction choice in `src/serialized_file/references.rs` (`plan_pptr_construction`: strict vs force branches; 1 new test; module suite 5/5; advances `SER-REF-001`). Baseline survey notes: no `UnityObjectRef` exists in the frozen baseline (UABEA term); PPtr/IPPtr/AssetCollection surveyed instead.
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1281 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 48: gate-first round (WebGL, path list, ref graph, registry schema, assemblies) (2026-10-06)

- `sprite-fill` named the build-flavor table in `src/io/platform_structure.rs` (`WEBGL_BUILD_FLAVORS`, wired into `detect_webgl` behavior-identically, with previously uncovered Release-only coverage; 1 new test; module suite 16/16; advances `IO-PLATFORM-WEBGL-001`).
- `texture-meta` named the split-suffix decision in `src/io/filesystem.rs` (`split_sibling_part_index`, rewired into `find_contiguous_split_parts` with dead prefix removed; 2 new tests; module suite 13/13; advances `IO-FS-IMPORT-PATH-LIST-001`).
- `mesh-deform` named the file-index gate in `src/serialized_file/reference_graph.rs` (`external_table_index`, wired at all three resolution sites; 1 new test; module suite 13/13; advances `SER-REF-GRAPH-001`).
- `texture-export` ported the assembly-side shape selection in `src/serialized_file/managed_types.rs` (`managed_reference_shape_for_stable_ids`, the tree-side twin's assembly counterpart over a different selector fact; 1 new test; module suite 7/7; advances `SER-MANAGED-REFS-001`).
- `anim-graph` ported the backend selection in `src/io/platform_file_inventory.rs` (`select_scripting_backend` from `GetScriptingBackend` incl. the disable-script-import flag, wired into `detect_backend_and_collect_assemblies`; 2 new tests; module suite 15/15; advances `IO-PLATFORM-ASSEMBLIES-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1288 library, 56 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 49: gate-first round (WebPlayer, resource, settings file, CLI, script identity) (2026-10-06)

- `sprite-fill` named the pairing rule in `src/io/platform_structure.rs` (`web_player_bundle_path` for the `.html`→`.unity3d` rule, wired into `detect_web_player` behavior-identically; 1 new test; module suite 17/17; advances `IO-PLATFORM-WEBPLAYER-001`).
- `texture-meta` ported the export-time value operations in `src/resource.rs` (`StreamedResourceInfo::set_values`/`clear_values` mirroring the exporter save/clear/restore flow; 1 new test; broad `resource` filter 436/436; advances `IO-RESOURCE-001`).
- `anim-graph` ported the BOM tolerance in `src/export/settings_file.rs` (`strip_leading_bom`, wired into the production reader; extended load-tolerance test; module suite 9/9; advances `EXP-SETTINGS-FILE-001`).
- `mesh-deform` ported the input dedup in `src/main.rs` (`deduplicate_logical_input_paths` mirroring `SelectUniquePaths`, wired into both production mount entries; 1 new test; bin suite 57/57; advances `PRODUCT-CLI-INSPECT-001`).
- `texture-export` ported the mscorlib discovery in `src/processing/script_identity.rs` (`find_mscorlib_path`: exact-first then first case-insensitive; 1 new test; module suite 10/10; advances `TYPE-SCRIPT-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1291 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 50: gate-first round (WiiU, registry decode, mono utils, meta pipeline, archive) (2026-10-06)

- `sprite-fill` ported the constructor directory layout in `src/io/platform_structure.rs` (`wiiu_data_layout`, wired into `detect_wiiu` behavior-identically, absent IL2Cpp paths mirroring the backend rule; 1 new test; module suite 18/18; advances `IO-PLATFORM-WIIU-001`).
- `texture-export` ported the terminated-loop control in `src/serialized_file/managed_registry.rs` (`step_terminated_loop` with the `UnterminatedRegistry` error, wiring `is_terminus` its first production caller; 1 new test; module suite 6/6; advances `SER-MANAGED-REFS-001`).
- `mesh-deform` ported the CorLib signature fast path in `src/processing/mono_utils.rs` (`element_type_to_primitive` over the 13-arm `MonoElementType` table, completing the `ToPrimitiveType` trilogy; 1 new test; module suite 5/5; advances `TYPE-MONO-UTILS-001`).
- `anim-graph` extracted the sidecar naming atom in `src/export/project_output.rs` (`meta_sidecar_file_name`, wired into `write_unity_meta_sidecar` with the shared `META_EXTENSION` const; 1 new test; module suite 13/13; advances `EXP-META-PIPELINE-001`).
- `texture-meta` fixed the prefix-probe length basis in `src/scheme.rs` (`detect_primary_file_kind_with_length`: 0x20 minimum on total length, wired through `probe_raw_kind`, dead wrapper removed; 1 new test; module suite 44/44; advances `IO-BUNDLE-ARCHIVE-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1296 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 51: gate-first round (desktop, LZ4, registry schema, mono utils, settings) (2026-10-06)

- `sprite-fill` named the shared fallback kernel in `src/io/platform_structure.rs` (`desktop_data_fallback` resolving the round-43 Linux-leniency item as documented deliberate leniency, wired into both desktop detectors behavior-identically; 1 new test; module suite 19/19; advances `IO-STRUCTURE-DESKTOP-001`).
- `texture-meta` fixed the empty-input divergence in `src/compression/lz4_block.rs` (zero bytes like K4os `Encode`, existing test updated, no new test fn; module suite stays 6/6; advances `IO-COMP-LZ4-001`). Frame container, preset dicts, and HC parser judged correctly out of scope (no baseline usage — porting them would be dead code).
- `texture-export` ported the container-depth decision in `src/serialized_file/managed_types.rs` (`managed_reference_array_depth` for `GetManagedReferenceArrayDepth`; 1 new test; module suite 8/8; advances `SER-MANAGED-REFS-001`).
- `mesh-deform` ported the size/primitive pair in `src/processing/mono_utils.rs` (`PrimitiveType::byte_size` + `is_c_sharp_primitive` with in-file production caller; 1 new test; module suite 6/6; advances `TYPE-MONO-UTILS-001`).
- `anim-graph` ported the automatic language selection in `src/export/settings.rs` (`automatic_script_language_version` over the full version-threshold table; 1 new test; module suite 17/17; advances `PRODUCT-CONFIG-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1300 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 52: gate-first round (smart stream, asset factory, absent typetree, workspace resource, mixed union) (2026-10-06)

- `texture-meta` ported the backend-divergent `SetLength` rule in `src/io/stream.rs` (memory clamps, file preserves, correcting a stale expectation with source justification; 1 new test; module suite 22/22; advances `IO-STREAM-SMART-001`).
- `mesh-deform` ported the structure-resolver selection in `src/processing/asset_factory.rs` (`select_structure_resolver`: TPK engine objects take Null, else reference types; 1 new test; module suite 13/13; advances `SER-ASSET-FACTORY-001`). Also recorded here: the round-47 `decide_try_read_object` composition (module suite 12/12) was logged under `SER-TT-ABSENT-001` but lives in this file — matrix row now cites both.
- `texture-export` ported the `ReadAsset` version gate in `src/serialized_file/object.rs` (`version_is_unsupported` wired first in the decode prelude, refused payloads flow to raw retention, unknown versions fail open; 1 new test; module suite 37/37; advances `SER-TT-ABSENT-001`).
- `anim-graph` extracted the collection fallback in `src/serialized_file/workspace.rs` (`collection_lookup_candidates` from `ResolveCollection`, wired into `SourceBundleCollectionLookup::resolve`; 1 new test; module suite 28/28; advances `IO-RESOURCE-001`).
- `sprite-fill` ported the consumed-path skip in `src/io/game_structure.rs` (`from_vfs_paths_with_consumed_paths` mirroring the pre-`CheckMixed` removal, `from_vfs_paths` delegates with empty list; 1 new test; module suite 6/6; advances `PRODUCT-CLI-INSPECT-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1305 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 53: gate-first round (partial stream, ref graph, settings, import directory, web data) (2026-10-06)

- `texture-meta` locked the End-seek contract in `src/io/stream.rs` (framed discrepancy disproved for `PartialStream`, sibling inversion recorded as deliberate non-follow on the shared kernel, 1 new test guarding against future "alignment"; module suite 23/23; advances `IO-STREAM-PARTIAL-001`).
- `mesh-deform` ported the once-per-identifier report in `src/serialized_file/reference_graph.rs` (`missing_dependency_indexes` matching the init loop order; 1 new test; module suite 14/14; advances `SER-REF-GRAPH-001`).
- `anim-graph` ported the `DisableScriptImport` mapping in `src/export/settings.rs` (`disables_script_import` + `disable_script_import` read-path mirror; 1 new test; module suite 18/18; advances `PRODUCT-CONFIG-001`).
- `sprite-fill` named the explicit-file rule in `src/io/discovery.rs` (`explicit_file_logical_path`: split parts fold to base, wired into the `expand_import_paths` file arm; 1 new test; module suite 9/9; advances `IO-FS-IMPORT-DIRECTORY-001`).
- `texture-export` named the payload range math in `src/container/web_file.rs` (`entry_byte_range`, overflow-then-bounds, wired into `parse`; 1 new test; module suite 11/11; advances `IO-BUNDLE-WEBDATA-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1310 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 54: gate-first round (multifile, stream input, assemblies, path-list, settings file) (2026-10-06)

- `texture-meta` locked the multi-file End-seek contract in `src/io/stream.rs` (framed inversion confirmed real in the baseline, recorded as deliberate non-follow alongside the sibling; 1 new test; module suite 24/24; advances `IO-STREAM-MULTIFILE-001`).
- `mesh-deform` surveyed ten scheme files against `src/scheme.rs` and found every unit already ported (NO-GAP verdict, zero edits, suite re-verified 44/44; advances `IO-STREAM-INPUT-001` with survey evidence, row stays unflipped per rule). Routed an unverified suspicion for another owner: wrapper-compressed inputs probe as Resource in mixed/platform inventory while the baseline loads them via GZip/Brotli schemes — round-55 candidate.
- `anim-graph` ported the duplicate rule in `src/io/platform_file_inventory.rs` (`register_assembly`: first path wins like `TryAdd`, fixing a real last-wins divergence, wired into `collect_assemblies`; 1 new test; module suite 16/16; advances `IO-PLATFORM-ASSEMBLIES-001`). The matrix notes already claimed first-wins; the code now matches.
- `sprite-fill` named the base resolution in `src/io/filesystem.rs` (`logical_file_path`, split parts fold to the group base, rewired into both logical-file probes; 1 new test; module suite 14/14; advances `IO-FS-IMPORT-PATH-LIST-001`).
- `texture-export` named the depth bound in `src/export/settings_file.rs` (`enter_nesting` single decision point for both containers; 1 new test; module suite 10/10; advances `EXP-SETTINGS-FILE-001`).
- Gate question answered YES: four landed units plus one evidence-grade survey all target incomplete rows and reduce (or document) the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1314 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 55: gate-first round (wrapped-input verdict, UnityFS boundary, managed refs, factory) (2026-10-06)

- `sprite-fill` exonerated the wrapped-input suspicion on the mixed side in `src/io/game_structure.rs` (MISREAD: directory wrappers unregistered in the baseline too, explicit selections registered in both, downstream unwrap in both, baseline cites; docs + 1 pinning test; module suite 7/7; advances `PRODUCT-CLI-INSPECT-001` with evidence).
- `anim-graph` exonerated the same suspicion on the inventory side in `src/io/platform_file_inventory.rs` (MISREAD: probe parity, name-based registrations, downstream `read_stream` unwrap; docs-only, zero behavior change; suite stays 16/16; advances `IO-PLATFORM-FILES-001` with evidence).
- `texture-meta` pinned the slack boundary in `src/container/unityfs.rs` (implemented-then-refuted consumed-bytes continuation with a corrupting fixture, kept declared-end per the writer's exact-size invariant — confirmed source misread, deliberate non-follow; 1 new test; module suite 15/15; advances `IO-BUNDLE-UNITYFS-HEADER-001`).
- `texture-export` ported the Registry-field placement in `src/serialized_file/managed_types.rs` (three const pub kernels + full append truth table; 1 new test; module suite 9/9; advances `SER-MANAGED-REFS-001`).
- `mesh-deform` ported the player-settings predicate in `src/processing/asset_factory.rs` (`is_player_settings_class` for ClassID 129, downstream exporter key; 1 new test; module suite 14/14; advances `SER-ASSET-FACTORY-001`).
- Gate question answered YES: three landed units plus two independent exonerations all target incomplete rows and shrink (or evidence-close) the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1318 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 56: gate-first round (UnityFS entries, raw-web dispatch, import directory, settings, absent typetree) (2026-10-06)

- `texture-export` named the copy window in `src/container/unityfs.rs` (`entry_block_window`, half-open overlap, wired into `read_entry`; 1 new test; module suite 16/16; advances `IO-BUNDLE-UNITYFS-ENTRY-001`).
- `mesh-deform` surveyed raw-web dispatch files against `src/container/raw_web.rs` and found every unit already ported (NO-GAP verdict, zero edits, suite re-verified 7/7; advances `IO-BUNDLE-DISPATCH-001` with survey evidence, row stays unflipped per rule). Routed note: whether inventory consumes bundle version strings is the platform lane's business — round-57 candidate.
- `sprite-fill` named the representative pick in `src/io/discovery.rs` (`split_group_representative`: lexical-first handle, read order stays numeric; 1 new test; module suite 10/10; advances `IO-FS-IMPORT-DIRECTORY-001`).
- `anim-graph` closed the last in-file gap in `src/export/settings.rs` (`TerrainExportMode::from_discriminant`, asserts folded into the existing combined suite, zero new fns; combined settings/settings-file filter 19/19; advances `PRODUCT-CONFIG-001`).
- `texture-meta` ported the `ClassName` producer in `src/serialized_file/object.rs` (full 354-member `ClassIDType` table + name-or-decimal, feeding the recovery names; 1 new test; module suite 38/38; advances `SER-TT-ABSENT-001`).
- Gate question answered YES: four landed units plus one evidence-grade survey all target incomplete rows and reduce (or document) the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1321 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 57: gate-first round (version strings, UnityFS meta, managed registry, path-list, workspace) (2026-10-06)

- `anim-graph` exonerated the version-string note in `src/io/platform_file_inventory.rs` (sole baseline reader is the unported `IL2CppManager`; parsing would be readerless dead code; docs-only, zero behavior change; suite stays 16/16; advances `IO-PLATFORM-FILES-001` with evidence).
- `texture-export` named the size rule in `src/container/unityfs.rs` (`metadata_size_accepts`: exact or #1470 aligned slack, wired into `parse_metadata`; 1 new test; module suite 17/17; advances `IO-BUNDLE-UNITYFS-META-001`).
- `mesh-deform` ported the failure contract in `src/serialized_file/managed_registry.rs` (`decide_referenced_type_resolution`: names the type and reason, never silent null; 1 new test; module suite 7/7; advances `SER-MANAGED-REFS-001`).
- `sprite-fill` named the rooted check in `src/io/filesystem.rs` (`is_path_rooted` naming `IsPathRooted`, wired into `set_temporary_directory`; 1 new test; module suite 15/15; advances `IO-FS-IMPORT-PATH-LIST-001`).
- `texture-meta` cached the name fix in `src/serialized_file/workspace.rs` (`name_fixed` computed once at mount registration, consumed by lookups, 8 fixtures corrected; 1 new test; module suite 29/29; advances `IO-RESOURCE-001`).
- Gate question answered YES: four landed units plus one evidence-grade exoneration all target incomplete rows and reduce (or evidence-close) the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1325 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 58: gate-first round (LZMA evidence, typetree factory+reader, meta pipeline, mixed) (2026-10-06)

- `texture-meta` closed the independent-encoder gap in `src/compression/lzma.rs` (three SharpCompress-0.50.4 vectors round-tripped through its own decoder, hex with provenance; 1 new test; module suite 8/8; advances `IO-COMP-LZMA-UNITYFS-001`).
- `mesh-deform` ported the start index in `src/processing/asset_factory.rs` (`monobehaviour_structure_start_index` skipping generated-consumed base fields; 1 new test; module suite 15/15; advances `SER-TT-ABSENT-001` factory-side).
- `anim-graph` wired the orphaned kernel in `src/export/project_output.rs` (`decide_asset_collection_export` into `export_asset_with_meta`, behavior-identical dedup; zero new tests, suite stays 13/13; advances `EXP-META-PIPELINE-001`).
- `sprite-fill` fixed the assembly rule on the mixed side in `src/io/game_structure.rs` (`insert_assembly`: name-keyed first-wins, duplicates counted, wired into the directory-scan arm; 1 new test; module suite 8/8; advances `PRODUCT-CLI-INSPECT-001`).
- `texture-export` named the registry predicates in `src/serialized_file/object.rs` (`is_managed_references_registry` + `is_referenced_object_data`, fixing a real over-refusal of same-named nodes with children; 1 new test; module suite 39/39; advances `SER-TT-ABSENT-001` reader-side).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1329 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 59: gate-first round (LZMA size, mono utils, Android, platform files, web data) (2026-10-06)

- `texture-meta` closed the size-lane encoder gap in `src/compression/lzma.rs` (two SharpCompress-0.50.4 size-prefixed vectors, decoder round-tripped, byte-identical cross-probe check; 1 new test; module suite 9/9; advances `IO-COMP-LZMA-SIZE-001`).
- `mesh-deform` ported the last open table in `src/processing/mono_utils.rs` (`PrimitiveType::system_type_name` for `ToSystemTypeName`, 13 names + 4 absences; 1 new test; module suite 7/7; advances `TYPE-MONO-UTILS-001`).
- `sprite-fill` named the role split in `src/io/platform_structure.rs` (`AndroidRootRole` + `android_root_role`, wired into `detect_android`, plus the two previously unpinned OBB gates; 2 new tests; module suite 21/21; advances `IO-PLATFORM-ANDROID-001`).
- `anim-graph` fixed a real silent skip in `src/io/platform_file_inventory.rs` (`split_data_bundle_name`: split-only data bundles register under their logical name, wired into `collect_data_path`; 1 new test; module suite 17/17; advances `IO-PLATFORM-FILES-001`).
- `texture-export` named the narrowing in `src/container/web_file.rs` (`write_i32_length` single decision point at all three write sites, overflow arms pinned; 1 new test; module suite 12/12; advances `IO-BUNDLE-WEBDATA-001`).
- Gate question answered YES: all five units land on incomplete rows and reduce the Phase 1 incomplete count. No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1335 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 60: gate-first round (iOS, resolution order, script identity, managed survey, settings file) (2026-10-06)

- `sprite-fill` named the candidate step in `src/io/platform_structure.rs` (`ios_app_data`, wired into `detect_ios`, plus the previously unpinned dataless end-to-end; 1 new test; module suite 22/22; advances `IO-PLATFORM-IOS-001`).
- `texture-meta` fixed the duplicate order with runtime proof in `src/resource.rs` (offline probe against real `AssetRipper.Assets`: catalog stores file-stack pop order via the shared builder; provider-caching difference recorded as deliberate; 1 new test; broad `resource` filter 438/438; advances `IO-RESOURCE-RESOLUTION-001`, which stays verified with strengthened evidence). Lead note: this row was mislabeled 实现中 at dispatch — the bug-evidence exception applies, recorded honestly here.
- `anim-graph` ported the presence gate in `src/processing/script_identity.rs` (`has_assembly_name` for `HasAssemblyName`, release×field truth table; 1 new test; module suite 11/11; advances `TYPE-SCRIPT-001`).
- `mesh-deform` surveyed all managed-types sources against `src/serialized_file/managed_types.rs` and found every unit ported (NO-GAP verdict, zero edits, suite re-verified 9/9; advances `SER-MANAGED-REFS-001` with survey evidence, row stays unflipped per rule).
- `texture-export` named the narrowing in `src/export/settings_file.rs` (`enum_discriminant` single decision point for all enum properties, overflow arms pinned; 1 new test; module suite 11/11; advances `EXP-SETTINGS-FILE-001`).
- Gate question answered YES with one correction: four units land on incomplete rows; the fifth (task-227) touched a verified row but under the bug-evidence exception (runtime differential proving a real order divergence, now fixed). No other verified row touched.
- Integration regression caught and fixed by the lead: the task-227 pop-order change flipped root-resource enumeration, failing CLI test `explicit_file_inspection_registers_only_selected_resource_inputs_as_roots` (it encoded the disproved input order). Expectation updated to the proven pop order with a citing comment in `src/main.rs` — not a masked defect: the old order was disproved by the runtime differential. Full suite re-run green.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1339 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 61: gate-first round (Switch, streaming scan, provider, collections, workspace) (2026-10-06)

- `sprite-fill` named the constants in `src/io/platform_structure.rs` (`SWITCH_EXEFS_DIR`/`SWITCH_ROMFS_DIR`/`SWITCH_MAIN_BINARY`, wired into `switch_data_layout`, plus the previously unasserted root-name derivation; 1 new test; module suite 23/23; advances `IO-PLATFORM-SWITCH-001`).
- `texture-meta` pinned the integrity cells in `src/resource.rs` (direct C# runtime differential over real `StreamingInfo` classes: overflow-checked narrowing throws, reject-loudly matches the port's `InvalidRange`; 1 new test, no behavior change; broad `resource` filter 440/440; `IO-RESOURCE-STREAMINGINFO-FIELDS-001` stays verified with strengthened evidence). Also flagged: the round-49 truncation-parity premise in `resources/streaming_info.rs` contradicts the observed throw — follow-up candidate for that file's lane.
- `anim-graph` fixed the selection dedup in `src/io/game_structure.rs` (`select_unique_logical_path` for `SelectUniquePaths`: file+split count as one selection, fixing a real double-registration divergence; 1 new test; module suite 9/9; `IO-RESOURCE-PLATFORM-PROVIDER-001` stays verified with strengthened evidence).
- `mesh-deform` surveyed collection sources against `src/serialized_file/collection_workspace.rs` and found every unit ported (NO-GAP verdict, zero edits, suite re-verified 2/2; advances `SER-REF-GRAPH-001` with survey evidence, row stays unflipped per rule). Proposed next unit: `ReadData` TypeID<0→114 fallback belongs to the factory file — queued for a future factory round.
- `texture-export` named the walk bound in `src/serialized_file/workspace.rs` (`bundle_walk_step_limit`, guarding the parent-cycle case the source never survives; 1 new test; module suite 30/30; advances `IO-RESOURCE-001`).
- Gate question answered YES with corrections: two dispatches mislabeled verified rows as 实现中 (the LEN probe cannot tell 已验证 from 实现中 — both 3 chars; dispatch now reads STATUS text). Task-233 fixed a real divergence (bug-evidence exception applies); task-232 added differential-backed pins with no behavior change (evidence-strengthening, does not reduce the incomplete count). No other verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1343 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 62: gate-first round (factory fallback, platform files, meta pipeline, settings file, streaming premise) (2026-10-06)

- `mesh-deform` ported the ReadData fallback in `src/processing/asset_factory.rs` (`resolve_read_class_id`: negative → 114, wired via `classify_read_data_path` ahead of the version gate; 1 new test; module suite 16/16; advances `SER-TT-ABSENT-001`).
- `sprite-fill` named the literal in `src/io/platform_file_inventory.rs` (`is_compressed_data_bundle_name`, wired into `collect_data_path`; 1 new test; module suite 18/18; advances `IO-PLATFORM-FILES-001`).
- `anim-graph` fixed the tree-shape order in `src/export/project_output.rs` (output directory ensured before the outcome gate, fixing a real empty-subdirectory divergence; `failed_exports_write_neither_asset_nor_meta` re-aimed, suite stays 13/13; advances `EXP-META-PIPELINE-001`).
- `texture-export` named the bound in `src/export/settings_file.rs` (`check_string_bound` at both `parse_string` sites; 1 new test; module suite 12/12; advances `EXP-SETTINGS-FILE-001`).
- `texture-meta` reconciled the false premise in `src/resources/streaming_info.rs` (the truncation-parity claim replaced with the runtime truth: checked narrowing throws, the view wraps — recorded as a deliberate difference, suite 2/2; `IO-RESOURCE-STREAMINGINFO-FIELDS-001` stays verified, no row re-verification).
- Gate question answered YES with one note: four units land on incomplete rows; the fifth (task-240) fixes a false test premise on a verified row under the bug-evidence exception (no behavior change, removes a false parity claim). No other verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1346 library, 57 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.

## Team round 63: gate-first round (assemblies, CLI inspect, factory, script identity, meta pipeline) (2026-10-06)

- `sprite-fill` fixed the backend rule in `src/io/platform_file_inventory.rs` (`platform_supports_mono_backend`: Switch never Mono, fixing a real divergence with non-vacuity proof; 1 new test; module suite 19/19; advances `IO-PLATFORM-ASSEMBLIES-001`).
- `anim-graph` named the tri-state in `src/main.rs` (`plan_explicit_inspect`: workspace-then-roots-then-failure, wired into `inspect_explicit_files_to`; 3 new tests; bin suite 60/60; advances `PRODUCT-CLI-INSPECT-001`).
- `mesh-deform` ported the read routing in `src/processing/asset_factory.rs` (`select_typetree_read_plan`: mode-matching tree consumed, conversion into the other; 1 new test; module suite 17/17; advances `SER-TT-ABSENT-001`).
- `texture-export` ported the lookup trio in `src/processing/script_identity.rs` (`find_module_type_index` + `script_is_present`/`script_is_valid`; 1 new test; module suite 12/12; advances `TYPE-SCRIPT-001`).
- `texture-meta` extracted the write atom in `src/export/project_output.rs` (`write_asset_bytes_to_new_file`, pinning the throwing-exporter partial-asset path; 1 new test; module suite 14/14; advances `EXP-META-PIPELINE-001`).
- Gate question answered YES, cleanly: all five units land on incomplete rows (all five dispatch rows STATUS-checked as text). No verified row touched.
- Final verification on Windows with Rust/Cargo 1.98.1: `cargo fmt --all -- --check` exit 0; `cargo test --workspace --locked` exit 0 (1350 library, 60 CLI, 0 doc tests, `TEMP`/`TMP` redirected to `target/test-tmp`); `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; `tools/verify-baseline.ps1` exit 0 (1,449/1,449 reference files unchanged); `git diff --check` exit 0. No Unity-produced file, user AssetBundle, or UABEA content was used. Phase 1 remains open.
