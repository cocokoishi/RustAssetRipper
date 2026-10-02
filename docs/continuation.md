# Continuation status

## Current phase

Phase 1: full Rust port of the local AssetRipper source baseline. The Phase 1 gate has not passed. Phase 2 remains locked: no user AssetBundle contents have been opened, hashed, identified, or parsed, and UABEA has not been read or run.

## Worktree and baseline

- Active Git worktree: `C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper`.
- Only this RustAssetRipper repository is a write target. Its `main` branch tracks the configured `origin/main`, and validated increments are pushed there. No other repository has been changed.
- Safety note: the user warns that running the original export path against a non-empty output directory may delete existing contents. No original exporter has been run; any later reference export must use a newly created, verified empty isolated output directory.
- The local reference snapshot is `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper`, not a Git checkout. Its 1,449-file checksum manifest is in `docs/baseline-files.sha256`.
- Reference identity recorded in `docs/baseline.md`: AssetRipper 2.0.0, `net10.0`, 51 solution projects, including 8 test projects and 6 source generators. Full feature/dependency/platform/option audit remains incomplete.

## Implemented in the Rust worktree

- Bounded endian-aware binary reader with checked slices, absolute offsets, alignment, strings, and contextual errors.
- SerializedFile format generation predicates and header parser, including the generation-9 endian header branch and generation-22 extended fields.
- Metadata prefix parsing, including pre-generation-9 metadata-at-end/endian handling, version text, target platform, and TypeTree flag.
- Metadata tables: serialized types, object records and checked payload ranges, local script identifiers, external file identifiers, reference types, dependencies, and user information.
- Legacy TypeTree records and the generation-10 / generation-12+ string-buffer form, including reference hashes and the audited common-string lookup table.
- TypeTree-driven object payload decoding for primitive fields, strings, arrays, nested structs, recognized PPtr fileID/pathID pairs, unknown fixed-size leaves, field/array alignment, and explicit allocation limits. Missing-TypeTree and managed-reference-registry inputs report explicit errors rather than being treated as decoded data.
- PPtr file-scope classification: null PathIDs, unique/missing/ambiguous local targets, one-based external table entries, and invalid FileIDs. ObjectReferenceGraph indexes PathIDs per SerializedFile, scans decoded TypeTree values into field-addressed PPtr edges, and reports undecoded objects, raw leaves, trailing bytes, and limit failures instead of claiming a complete graph. External bundles are identified; the workspace now loads uniquely resolved SerializedFile candidates from mounted VFS leaves, while platform DataPath discovery and GUID-derived Meta lookup remain open.
- Stream and filesystem groundwork: PartialStream provides bounded seekable views; FileRangeStream provides nested file ranges with independent logical positions over a shared handle; MultiFileStream composes ordered seekable parts; SchemeReader::read_stream and read_path dispatch complete inputs with a 2 GiB default size bound. Local regular-file and .splitN path opening, numeric ordering, and missing-part checks are implemented. SmartStream null/memory/file/temp modes, Arc references, and partial views are implemented. VirtualFileSystem now provides a bounded in-memory directory tree, SmartStream-backed files, file operations, directory creation, and deterministic recursive enumeration. Synthetic tests cover these behaviors; OS-backed positional reads and the local FileSystem adapter remain outstanding. Direct entry mounting and recursive tree mounting are implemented for WebData, UnityFS, UnityRaw, and UnityWeb paths with synthetic tests, including nested containers and GZip-wrapped containers. SchemeReader::mount_path connects one local input to this recursive tree API; bounded directory/batch discovery is implemented, while a broader local FileSystem adapter remains outstanding.
- Bundle container work: UnityFS has partial header and block/directory metadata for start/end placement, version-7 alignment and padding flags, uncompressed/raw LZ4/raw LZMA payloads, Zstandard storage blocks, and directory-entry extraction across blocks. UnityRaw and UnityWeb have bounded versioned header, directory, and entry parsing; UnityWeb decodes the final size-prefixed LZMA scene chunk. Unity WebData 1.0 has bounded directory parsing, borrowed entry ranges, and a bounded writer. Synthetic fixtures exercise these paths only. Source scheme ordering, top-level dispatch, nested GZip/Brotli decoding, and WebData, UnityFS, UnityRaw, and UnityWeb entry dispatch are implemented with synthetic fixtures; external-file integration and real-file differentials remain incomplete. UnityArchive parsing and writing are explicitly unsupported by the source baseline; LZHAM is also explicitly unsupported.
- Compression wrappers: GZip magic detection, bounded concatenated-member decoding, and bounded GZip writing are implemented. Brotli signature parsing follows RFC 7932 metadata length semantics and bounded Brotli decoding. An RFC-constructed marked stream exposed an off-by-one in the local source detector; the Rust detector has a regression fix, recorded in `docs/differential-findings.md`. Fixtures cover GZip member/checksum behavior, limits, Brotli marker detection and decoding for WBITS 10–24, and nested dispatch. No C# or Unity-produced wrapper differential has run.
- Complete-file `SerializedFile::parse` facade that reads a real encoded header, metadata, and referenced object payloads from the same borrowed byte slice; a generation-10 synthetic end-to-end fixture covers this path.
- Rust package uses `lzma-rs`, `ruzstd`, `brotli-decompressor`, and pure-Rust `flate2` for codec paths; versions, feature flags, licenses, and validation limits are recorded in `docs/dependencies-and-licenses.md`, and `Cargo.lock` is checked in. The declared minimum Rust version is 1.87 due to `ruzstd`; only Rust 1.98.1 has been run. UnityFS, Raw/Web, wrapper decoder, and codec paths remain partial. Product-level directory/batch orchestration, full object construction and complete dependency orchestration (platform registry/DataPath generation and GUID-derived Meta lookup), CLI, remaining bundle codecs, full Unity object models and no-TypeTree recovery, remaining object construction and reference-graph integration, resource handlers, exporters, and UI have not been implemented.

## Single-file path import and recursive parsed-container mounting

- Added bounded VFS insertion of owned readable SmartStreams with automatic parent creation and duplicate-path rejection.
- Added direct mounting plus a recursive tree-mount API for parsed UnityFS, UnityRaw/UnityWeb, and WebData containers. Nested recognized containers and wrappers are traversed beneath their parent entry path.
- Direct mount fixtures cover WebData, UnityFS, UnityRaw, and UnityWeb. Recursive fixtures cover nested WebData, GZip-wrapped nested WebData, and nested UnityFS; global visited-entry, nesting-depth, active nested-container memory, aggregate-output, and partial-progress behavior are covered. All evidence is synthetic.
- cargo test --workspace --locked: 140 passed, 0 failed; cargo clippy --workspace --all-targets --locked -- -D warnings: passed.
- Next: add directory/batch input discovery and a broader local FileSystem adapter around mount_path, then continue object construction and cross-file resource resolution. Resource handlers, exporters, CLI, and UI remain outstanding.

## Latest verification

- `cargo fmt --all -- --check`: exit 0.
- `cargo test --workspace --locked`: exit 0; 168 unit tests passed, 0 failed, 0 doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `cargo tree --locked`: exit 0; the dependency graph is recorded in `docs/dependencies-and-licenses.md`.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest.
- `dotnet --info`: exit 1 because PowerShell cannot find `dotnet`; AssetRipper build and tests remain unrun.
- Rust 1.87 is declared as the minimum but has not been tested. Rust 1.98.1 is the only verified toolchain.
- Tests use self-constructed byte fixtures and source-derived expected fields. No real Unity SerializedFile differential run has occurred.
- Exact commands and transient fixture/clippy failures are recorded in `docs/validation-log.md`.

## Next work

1. Build platform-specific registries/DataPaths and port GUID-derived `AssetType.Meta` lookup; then connect the importer to the full object-processing pipeline.
2. Add independent codec and real-file fixtures, and resolve source differentials such as the Brotli marker issue with executable C# evidence when a .NET environment is available.
3. Expand `docs/feature-parity.md` into per-version/per-platform behavior rows across all 51 projects, registration tables, options/defaults, generators, GUI workflows, and exporters.
4. Add independent generic fixtures and source-derived regression coverage for untested SerializedFile generations and malformed layouts; obtain a usable .NET environment before claiming C# differential validation.
5. Continue into object/reference construction, all resource handlers, export formats, configuration, CLI, UI, and complete integration/end-to-end verification.

Do not read UABEA or any user AssetBundle until the first-phase gate has actual evidence for every required capability.

## Single-file path to VFS import

- Added SchemeReader::mount_path for a local regular file or .splitN logical input.
- Standalone Resource and SerializedFile inputs are mounted under their logical filename; recognized container inputs feed the recursive container-tree mount.
- Temporary-file tests cover ordinary Resource input, loading via .split1 while preserving the base name, and mounting a root WebData container and its leaf.
- cargo test --workspace --locked: 140 passed, 0 failed; cargo clippy --workspace --all-targets --locked -- -D warnings: passed.
- Directory/batch discovery, a broader local filesystem adapter, external dependency loading, and the rest of the product pipeline remain outstanding.

## Directory batch import

- `SchemeReader::mount_directory` recursively discovers regular inputs, sorts logical paths deterministically, preserves standalone file paths, namespaces root containers by logical input path, and combines `.splitN` parts as one input.
- Directory entries, depth, logical input count, aggregate source bytes, and aggregate mounted leaf bytes have configurable limits. Input bytes are preflighted before mount mutation; later mount failures retain completed-input and partial-output counts.
- Synthetic tests cover standalone files, nested directories, a root WebData container, split parts, aggregate input/output bounds, partial progress, colliding internal paths from separate bundles, and ambiguous regular/split names. No user samples were scanned.
- `cargo test --workspace --locked`: 145 tests passed; formatting and Clippy are clean, and the baseline verifier matched all 1,449 reference files.
- Remaining input work: broader local FileSystem adapter and external dependency discovery/loading. The next major implementation area is full object construction and cross-file reference resolution.

## Explicit path-list batch import

- `SchemeReader::mount_paths` accepts an ordered slice of local path/destination pairs and shares the directory importer’s input, output, per-file, and per-container limits.
- The full logical input length is preflighted before any VFS mutation. Directories and symbolic links are rejected; repeated logical split paths targeting the same virtual root are rejected before mounting.
- Synthetic tests cover a standalone Resource plus a WebData container with independent destinations, aggregate input-size preflight, and duplicate split-group rejection.
- `cargo test --workspace --locked`: 147 tests passed; warnings-denied Clippy and `cargo fmt --all -- --check` passed.
- Callers must provide one path per logical split group and choose distinct destination roots where source names may collide. Real-file and C# differential validation remains outstanding.

## Serialized external dependency-name normalization

- Ported `SpecialFileNames.FixFileIdentifier`, `FixDependencyName`, and `FixResourcePath` to `src/io/special_names.rs` and exposed the normalized path through `FileIdentifier::fixed_path_name`; the original metadata string remains intact.
- Tests cover `library/`, `resources/`, `archive:/`, lowercase rules, fallback paths, and original-name preservation.
- `cargo test --workspace --locked`: 150 tests passed; Clippy with `-D warnings`, formatting, and the 1,449-file baseline verification passed.
- Next: port the actual dependency lookup and loading path from `PlatformGameStructure.RequestDependency`, then connect loaded SerializedFiles to the cross-file PPtr graph. GUID-derived `AssetType.Meta` paths remain outstanding.

## Cross-file PPtr graph

- Added `MultiFileReferenceGraph` to join already-loaded per-file PPtr scans by normalized serialized-file identity, using the source `FileBase.Name` / `FileIdentifier.PathName` normalization rules.
- Unique external file matches resolve PathIDs against the target file index. Nulls, invalid FileIDs, missing dependencies, duplicate file identities, and missing or duplicate target PathIDs remain separate outcomes; `is_complete` only returns true when every input scan is complete and every edge resolves uniquely or is explicitly null.
- File, reference, per-name, and aggregate-name bounds are applied before constructing the cross-file result. The caller supplies the loaded file set; this increment does not discover or load dependencies from VFS or host paths.
- Synthetic tests cover unique cross-file resolution, missing and ambiguous names, missing target PathID, source-name aliases, candidate enumeration, and limits.
- `cargo fmt --all -- --check`: exit 0. `cargo test --workspace --locked`: 153 tests passed, 0 failed. `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. Baseline verifier found all 1,449 reference files unchanged; `git diff --check`: exit 0.
- No real-file or C# differential has run. GUID-derived `AssetType.Meta` matching and actual dependency discovery/loading remain outstanding.

## Mounted VFS dependency lookup

- Added `DependencyIndex`, built from caller-supplied source file identities/virtual paths and ordered DataPaths. It matches the source `RequestDependency` stages: registered name first, exact `DataPath / dependency` checks, then default-resource/builtin-extra aliases under the DataPath root, `Resources`, and legacy `unity` directories.
- A unique result can be opened as a readable `SmartStream`. Missing and multiple viable paths remain explicit; lookup rejects absolute/traversing dependency names and enforces registered-file, DataPath, name/path byte, aggregate index, and candidate limits.
- Synthetic VFS tests cover registered-name precedence, nested DataPath lookup, missing and ambiguous results, engine alias ordering, reading an opened result, path traversal rejection, and configured bounds.
- `cargo test --workspace --locked --quiet`: 159 passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. `cargo fmt --all -- --check`: exit 0; the baseline verifier found all 1,449 reference files unchanged.
- The caller must still build the source structure's registry/DataPaths and recursively parse discovered SerializedFiles into the bundle graph. No host directory or user sample was scanned; no real-file/C# differential has run.

## VFS SerializedFile workspace load

- Added `SerializedFileWorkspace::load_from_vfs` for explicitly named leaf SerializedFiles already mounted in VFS. It preflights file count, per-file and aggregate input bytes, and aggregate names before reading any contents; then it scheme-dispatches, parses headers/metadata, scans each file's object references, and checks aggregate object/edge bounds.
- The workspace retains each source name/path, SerializedFile header, owned metadata, and per-file reference index. Original bytes remain in VFS rather than being duplicated. `reference_graph()` joins loaded files on demand and preserves all missing/ambiguous outcomes.
- Three synthetic cases cover a complete generation-10 file flowing from VFS through SchemeReader and the parser, a local PPtr resolving through the workspace graph, aggregate input preflight, non-SerializedFile rejection, and aggregate object limits.
- `cargo test --workspace --locked --quiet`: 162 passed, 0 failed, 0 doc tests. `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0. Formatting and baseline checks are recorded in the validation log.
- This increment still requires the caller to identify every leaf SerializedFile. DependencyIndex is not yet driving recursive external loading, and no real-file/C# differential has run.

## Recursive mounted SerializedFile dependencies

- Added `SerializedFileWorkspace::load_from_vfs_with_dependencies`. It builds a bounded dependency registry from all mounted VFS file paths plus caller-supplied source identities, keeps caller DataPath ordering, and walks every loaded SerializedFile's external table until no newly resolved SerializedFiles remain.
- Unique SerializedFile candidates are parsed and scanned into the same workspace. Cycles and repeated references stop at an already-loaded identity; ambiguous loaded names, ambiguous VFS candidates, missing names, Resource candidates, and containers needing expansion remain distinct report outcomes.
- The workspace report now accounts for dynamically loaded files, bytes, normalized names, objects, and reference edges. The dependency walk bounds request count, total normalized request-name bytes, and reported ambiguous candidate-path bytes.
- Synthetic fixtures cover a three-file external chain with a cycle, file-scoped PPtr resolution through the cross-file graph, missing/ambiguous/Resource/container outcomes, and request/aggregate-workspace bounds. All files are test-generated.
- Latest verification: `cargo test --workspace --locked` passed 168 tests; warnings-denied Clippy, `cargo fmt --all -- --check`, the 1,449-file reference verifier, and `git diff --check` passed. No real-file or C# differential has run.
- Next: port GUID-derived `AssetType.Meta` lookup and construct platform-specific registries/DataPaths. Phase 1 still has major parser, object, resource, export, configuration, and product-interface work remaining; do not inspect user AssetBundles or UABEA.

## Dependency container expansion

- The dependency loader now takes a mutable VFS, expands supported UnityFS, UnityRaw, UnityWeb, and WebData candidates with the existing recursive mount API, and rebuilds the bounded registry after each expansion.
- Container leaves retain their path relative to an isolated virtual mount root as their serialized identity. Resource leaves are registered for later lookups; unique SerializedFile leaves are parsed, appended to the workspace, and added to the external-table queue. Repeated container paths are not mounted twice.
- Aggregate dependency-container input, mounted-entry, and output-byte limits are enforced in addition to each container's limits and the VFS entry bound. A leaf that remains a container is an explicit error. UnityArchive remains unsupported by the source baseline parser.
- A synthetic WebData container fixture verifies mounting one nested SerializedFile, loading it into the workspace, matching the external identity, and resolving its PPtr through the cross-file graph. Empty-container reporting and aggregate input/entry/output limits are also covered. No real-file/C# differential has run.
- `cargo test --workspace --locked`: 168 tests passed, 0 failed, 0 doc tests. Warnings-denied Clippy, `cargo fmt --all -- --check`, the 1,449-file baseline verifier, and `git diff --check` all passed.
- Next: port GUID-derived `AssetType.Meta` identity and derive platform file registries/DataPaths from mounted inputs. The full Phase 1 gate remains open; do not inspect user AssetBundles or UABEA.


## GUID-derived Meta dependency identity

- FileIdentifier::get_file_path preserves the source bundle-collection identity: AssetType.Meta uses four lowercase, zero-padded GUID words with no separators. Platform external requests still use normalized PathName, matching StructureDependencyProvider. A malformed Meta record without a GUID has no bundle identity.
- Synthetic tests cover source GUID word formatting for game-bundle identity, external Meta requests still resolving by PathName, ordinary path lookup, and malformed missing-GUID handling. The full workspace suite passed 171 tests; warnings-denied Clippy, format check, 1,449-file baseline verification, and diff check passed. No real-file or C# differential has run.
- Next: derive platform registries and ordered DataPaths from mounted inputs, then connect the remaining platform-specific lookup rules. Phase 1 is still open; do not inspect user AssetBundles or UABEA.


## Mixed mounted input structure

- Added MixedGameStructure::from_vfs_paths for explicitly selected mounted files and directories. It returns source-style registered identities, recognized SerializedFile inputs, .dll assembly names, deterministic DataPaths, and scan counters. Directly selected files remain registered as selected; directory inventory registers SerializedFiles, supported bundle headers, and WebGL data names while keeping assemblies separate.
- Directory classification probes at most the first 32 bytes and uses the VFS-declared file length. The default bounds are 100,000 selections, 1,000,000 scanned files, 2 GiB per file, and 16 GiB aggregate declared bytes. GZip/Brotli-wrapped headers are not decoded during inventory.
- Four synthetic tests cover file/directory inventory, file kinds and identity names, DataPaths, and selection/file/byte limits. Full workspace tests passed 176 tests; warnings-denied Clippy, formatting, the 1,449-file baseline verifier, and diff check passed. No real-file or C# differential has run.
- Next: implement the source platform-root detectors and their ordered DataPaths, then wire game-structure inventory into the application input flow. Phase 1 remains open; do not inspect user AssetBundles or UABEA.


## Desktop platform root detection

- Added source-priority detection for Windows, Linux, and macOS selected paths. Windows/Linux executable and _Data inputs normalize to their application root; Windows named/fallback Data directories, Linux named/fallback Data directories, and macOS Contents/Resources/Data plus Contents/Resources DataPath order are preserved.
- Three synthetic tests cover executable and Data-folder selection, platform priority, fallback paths, ordered macOS paths, and missing/invalid selections. Full workspace tests passed 179 tests; warnings-denied Clippy, formatting, the 1,449-file baseline verifier, and diff check passed. No real-file or C# differential has run.
- Next: port Android/OBB and iOS layout detection, then console and Web platform layouts; keep platform file collection and product input-flow integration visible as separate unfinished scopes.


## Mobile, console, and Web platform roots

- detect_platform_structure now follows local PlatformChecker priority across Windows, Linux, macOS, Android, iOS, Switch, PS4, WebGL, WebPlayer, Wii U, and Windows Phone. Android accepts one APK and optional OBB root, rejects duplicate roots, and emits APK then OBB DataPaths. Other added layouts cover iOS Payload apps, PS4 Media, Switch ExeFS/RomFS, WebGL Build/Development/Release, WebPlayer HTML/bundle pairs, Wii U content/Data, and the source Windows Phone predicate.
- Seven synthetic platform tests pass. Full workspace tests passed 183 tests; warnings-denied Clippy, formatting, the 1,449-file baseline verifier, and diff check passed. The Windows Phone overlap test confirms Windows wins earlier in the source priority order. No real-file or C# differential has run.
- Platform file registration is now implemented for the source game-file and streaming-bundle paths; Mono assembly collection, backend identification, and application-flow integration remain unfinished. Phase 1 remains open.


## Platform game-file registration

- Added `PlatformStructure::collect_game_files` in `src/io/platform_file_inventory.rs`. It visits DataPaths in source order, registers `data.unity3d`, `datapack.unity3d`, and the source primary SerializedFile name patterns, and uses logical base paths for `.split0` parts.
- WebGL data files and the matched WebPlayer bundle follow their constructor registration rules. Streaming bundle scans use the source platform paths; Android scans the APK `assets` root recursively while skipping its direct `bin` subtree, even when `skip_streaming_assets` is true, matching the source Android override. Bundle candidates are accepted only when the raw UnityFS/UnityRaw/UnityWeb header is recognized.
- The scanner bounds probed file count/size/aggregate bytes, directory count/depth, and all enumerated path lengths. Seven synthetic tests cover primary names, compressed game files, streaming bundles and skip mode, Android traversal, Web registrations, explicit missing DataPaths, and limits.
- `cargo test --workspace --locked`: exit 0; 190 tests passed, 0 failed, 0 doc tests. Warnings-denied workspace Clippy and `cargo fmt --all -- --check` passed. `tools/verify-baseline.ps1` confirmed 1,449 reference files unchanged; `git diff --check` passed.
- This API does not yet collect Mono assemblies, detect platform scripting backends, or connect the platform structure to the product input flow. No real-file or C# differential has run; Phase 1 remains open.
