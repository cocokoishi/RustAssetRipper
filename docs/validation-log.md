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
