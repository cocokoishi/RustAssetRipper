# Continuation status

## Current phase

Phase 1: full Rust port of the local AssetRipper source baseline. The Phase 1 gate has not passed. Phase 2 remains locked: no user AssetBundle contents have been opened, hashed, identified, or parsed, and UABEA has not been read or run.

## Worktree and baseline

- Active Git worktree: `C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper`.
- Only this RustAssetRipper repository is a write target. Its `main` branch tracks the configured `origin/main`, and validated increments are pushed there. No other repository has been changed.
- Safety note: the user warns that running the original export path against a non-empty output directory may delete existing contents. No original exporter has been run; any later reference export must use a newly created, verified empty isolated output directory.
- The local reference snapshot is `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper`, not a Git checkout. Its 1,449-file checksum manifest is in `docs/baseline-files.sha256`.
- The workspace inventory uses the harmless Chinese aliases 青岚雅苑 and 松风竹庭; exact input names are omitted. Later validation read and replayed sample contents, and the inventory remains incomplete and unfrozen. UABEA remains unread; the GOAL §25 gate remains OPEN.
- Reference identity recorded in `docs/baseline.md`: AssetRipper 2.0.0, `net10.0`, 51 solution projects, including 8 test projects and 6 source generators. Full feature/dependency/platform/option audit remains incomplete.

## Implemented in the Rust worktree

- Bounded endian-aware binary reader with checked slices, absolute offsets, alignment, strings, and contextual errors.
- SerializedFile format generation predicates and header parser, including the generation-9 endian header branch and generation-22 extended fields.
- Metadata prefix parsing, including pre-generation-9 metadata-at-end/endian handling, version text, target platform, and TypeTree flag.
- Metadata tables: serialized types, object records and checked payload ranges, local script identifiers, external file identifiers, reference types, dependencies, and user information.
- Legacy TypeTree records and the generation-10 / generation-12+ string-buffer form, including reference hashes and the audited common-string lookup table.
- TypeTree-driven object payload decoding for primitive fields, strings, arrays, nested structs, recognized PPtr fileID/pathID pairs, unknown fixed-size leaves, field/array alignment, and explicit allocation limits. Missing-TypeTree and managed-reference-registry inputs report explicit errors rather than being treated as decoded data.
- `ObjectWorkspace` decodes every object from the loaded workspace within object, file-size, and aggregate decoded-allocation limits. It preserves serialized/resolved type metadata and source payload ranges; decode failures remain per-object errors with raw source ranges. CLI inspection reports aggregate counts and up to 128 contextual decode errors. This remains a TypeTree value view, not generated class-specific AssetRipper object materialization.
- `WorkspaceAssetCollections` groups each SerializedFile with a source-order collection index, fixed name, bundle membership, external-identifier count, and object-workspace range. CLI reports this grouping. These records do not yet construct the source Bundle/AssetCollection hierarchy or generated Unity asset objects.
- `inspect` reports the joined TypeTree PPtr graph across loaded files, separating null, unique, and unresolved targets from per-file scan issues and limiting detailed diagnostics to 128. Workspace external references now follow source `Bundle.ResolveCollection`: current collections, direct child bundles in source order, then ancestors while excluding the branch just climbed; root collections precede root bundles. Name fixing and the default-resource/builtin-extra aliases are applied, and same-name collisions use the source's first-match rule. Missing inputs and references hidden in undecodable/raw fields remain unresolved/issues.
- PPtr file-scope classification: null PathIDs, unique/missing/ambiguous local targets, one-based external table entries, and invalid FileIDs. ObjectReferenceGraph indexes PathIDs per SerializedFile, scans decoded TypeTree values into field-addressed PPtr edges, and reports undecoded objects, raw leaves, trailing bytes, and limit failures instead of claiming a complete graph. The workspace loads uniquely resolved SerializedFile candidates from mounted VFS leaves; platform detection, source registries/DataPaths, and GUID-derived Meta identity have synthetic coverage, while full object-processing integration remains open.
- Stream and filesystem groundwork: PartialStream provides bounded seekable views; FileRangeStream provides nested file ranges with independent logical positions over a shared handle; MultiFileStream composes ordered seekable parts; SchemeReader::read_stream and read_path dispatch complete inputs with a 2 GiB default size bound. Local regular-file and .splitN path opening, numeric ordering, and missing-part checks are implemented. SmartStream null/memory/file/temp and generic read-only seekable-reader backends, Arc references, and partial views are implemented. VirtualFileSystem now provides a bounded in-memory directory tree, SmartStream-backed files, file operations, directory creation, and deterministic recursive enumeration. Synthetic tests cover these behaviors; OS-backed positional reads and the broader local FileSystem adapter remain outstanding. Direct entry mounting and recursive tree mounting are implemented for WebData, UnityFS, UnityRaw, and UnityWeb paths with synthetic tests, including nested containers and GZip-wrapped containers. Directory import supports both recursive expansion and a file-backed mode that preserves source container identities; a broader FileSystem abstraction remains outstanding.
- Bundle container work: UnityFS has partial header and block/directory metadata for start/end placement, version-7 alignment and padding flags, uncompressed/raw LZ4/raw LZMA payloads, Zstandard storage blocks, and directory-entry extraction across blocks. UnityRaw and UnityWeb have bounded versioned header, directory, and entry parsing; UnityWeb decodes the final size-prefixed LZMA scene chunk. Unity WebData 1.0 has bounded directory parsing, borrowed entry ranges, and a bounded writer. Synthetic fixtures exercise these paths only. Source scheme ordering, top-level dispatch, nested GZip/Brotli decoding, and WebData, UnityFS, UnityRaw, and UnityWeb entry dispatch are implemented with synthetic fixtures; external-file integration and real-file differentials remain incomplete. UnityArchive parsing and writing are explicitly unsupported by the source baseline; LZHAM is also explicitly unsupported.
- Workspace collection traversal now follows source stack reversal and recursive container collection order. Scene inspection selects the last encountered BuildSettings object and walks streamed-scene AssetBundles by that collection order; root input order remains deterministic in Rust because the source root registration uses a runtime-dependent HashSet order.
- Scene plans are now materialized as bounded `WorkspaceSceneDefinition` models with ordered workspace collection membership and unique collection ownership. The models still reference SerializedFile indexes; actual typed AssetCollection objects and concrete GUID generation remain open.
- `WorkspaceSceneHierarchies` now inventories source `SceneHierarchyObject.Create` categories by file/object/PathID for GameObjects, LevelGameManager classes, PrefabInstances, and SceneRoots. Transform IDs, resolved GameObject component edges, and non-null inherited `m_GameObject` PPtrs are recorded as evidence-labeled component candidates; generated component classes are not yet materialized.
- `WorkspaceSceneCollectionHierarchy` records source-derived missing-Transform plans for SceneDefinition GameObjects classified with `NoTransformComponent`, including zero position, identity rotation, unit scale, root order zero, and a conditional zero Euler hint. These plans preserve source identity but do not create a generated Unity object or output PathID; non-scene GameObjects remain outside this scene-owned model.
- `WorkspaceGeneratedTransformPlanBuilder` now collects missing-Transform GameObjects across every loaded file with GameObjects and routes them only after streamed-scene ownership is final: SceneDefinition-owned candidates stay with their scene; non-scene candidates form the bounded `Missing Prefab Transforms` plan list. The plan still does not allocate a Unity Transform, PathID, or generated collection.
- `WorkspaceSceneCollectionHierarchy` now stores a source-ordered per-GameObject index into its `m_Component` relationships. The index preserves each array slot, including null, and exposes validated ordered iteration without duplicating relationship payloads; it does not yet construct component objects or recursively traverse Transform children.
- The inspect pipeline now retains bounded hierarchy snapshots for collections with GameObject/Transform or PrefabInstance data. After streamed-scene ownership is final, scene-owned graphs stay with their SceneDefinition and unassigned graphs are exposed as non-scene collections for Prefab traversal; non-scene missing-Transform plans remain in their separate workspace result.
- `WorkspaceSceneHierarchies::fetch_game_object_hierarchy` now models the ordered depth-first identity walk from `GameObjectExtensions.FetchHierarchy`, including the last Transform/RectTransform selection, cross-file child Transform resolution, explicit missing-reference states, active-path cycle detection, and entry/depth/pending-action limits. It does not materialize generated classes or get called by the CLI yet.
- `SceneGraph` now decodes class ID 1001 `m_RootGameObject` PPtrs and validates them against the joined reference graph. `WorkspaceSceneHierarchies` exposes PrefabInstance identities in workspace file/object order and resolves roots with distinct null, field-unavailable, unresolved, identity-mismatch, and non-GameObject outcomes; inspect reports bounded diagnostics. No root is treated as null merely because its field or target is unavailable.
- Next: derive the exact `GameBundle.FetchAssets()` sequence from bundle/collection traversal, seed the processed set from SceneDefinition GameObjects, then implement the ordered existing-PrefabInstance pass and remaining-GameObject `GetRoot` pass with global identity deduplication. Construct the hierarchy asset model only after those source order and ownership rules are represented.
- The selected BuildSettings list is copied into a bounded EditorBuildSettings workspace model, preserving path order and duplicates and enabling every scene entry. The Unity `Generated Settings` collection and asset serializer remain unimplemented.
- `EditorSettingsExtensions.SetToDefaults` values now have a semantic Rust defaults model with explicit capability switches for optional Collab and AssetPipeline fields; it is not yet wired to target-version schema selection or generated-asset construction.
- Compression wrappers: GZip magic detection, bounded concatenated-member decoding, and bounded GZip writing are implemented. Brotli signature parsing follows RFC 7932 metadata length semantics and bounded Brotli decoding. An RFC-constructed marked stream exposed an off-by-one in the local source detector; the Rust detector has a regression fix, recorded in `docs/differential-findings.md`. Fixtures cover GZip member/checksum behavior, limits, Brotli marker detection and decoding for WBITS 10–24, and nested dispatch. No C# or Unity-produced wrapper differential has run.
- Complete-file `SerializedFile::parse` facade that reads a real encoded header, metadata, and referenced object payloads from the same borrowed byte slice; a generation-10 synthetic end-to-end fixture covers this path.
- Rust package uses `lzma-rs`, `ruzstd`, `brotli-decompressor`, and pure-Rust `flate2` for codec paths; versions, feature flags, licenses, and validation limits are recorded in `docs/dependencies-and-licenses.md`, and `Cargo.lock` is checked in. The declared minimum Rust version is 1.87 due to `ruzstd`; only Rust 1.98.1 has been run. UnityFS, Raw/Web, wrapper decoder, and codec paths remain partial. A narrow directory-inspection CLI exists; full CLI options/configuration, full object construction, complete object/reference-graph integration, remaining bundle codecs, no-TypeTree recovery, resource handlers, exporters, and UI have not been implemented.

## Single-file path import and recursive parsed-container mounting

- Added bounded VFS insertion of owned readable SmartStreams with automatic parent creation and duplicate-path rejection.
- Added direct mounting plus a recursive tree-mount API for parsed UnityFS, UnityRaw/UnityWeb, and WebData containers. Nested recognized containers and wrappers are traversed beneath their parent entry path.
- Direct mount fixtures cover WebData, UnityFS, UnityRaw, and UnityWeb. Recursive fixtures cover nested WebData, GZip-wrapped nested WebData, and nested UnityFS; global visited-entry, nesting-depth, active nested-container memory, aggregate-output, and partial-progress behavior are covered. All evidence is synthetic.
- cargo test --workspace --locked: 140 passed, 0 failed; cargo clippy --workspace --all-targets --locked -- -D warnings: passed.
- Next: add directory/batch input discovery and a broader local FileSystem adapter around mount_path, then continue object construction and cross-file resource resolution. Resource handlers, exporters, CLI, and UI remain outstanding.

## Latest verification

- `cargo fmt --all -- --check`: exit 0.
- `cargo test --workspace --locked --quiet`: exit 0; 279 library tests, 10 binary tests, 0 doc tests passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `cargo run --locked --quiet -- --help` and `--version`: exit 0.
- `cargo tree --locked`: exit 0; the dependency graph is recorded in `docs/dependencies-and-licenses.md`.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest.
- `dotnet --info`: exit 1 because PowerShell cannot find `dotnet`; AssetRipper build and tests remain unrun.
- Rust 1.87 is declared as the minimum but has not been tested. Rust 1.98.1 is the only verified toolchain.
- Tests use self-constructed byte fixtures and source-derived expected fields. No real Unity SerializedFile differential run has occurred.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest. `git diff --check`: exit 0.
- Latest tested code uses synthetic fixtures only. No C# or Unity-produced differential has run.
- Exact commands and transient fixture/clippy failures are recorded in `docs/validation-log.md`.

## Next work

1. Connect platform and mixed registry results to complete object construction and processing; preserve distinct identities for all imported container leaves.
2. Add independent codec and real-file fixtures, and resolve source differentials such as the Brotli marker issue with executable C# evidence when a .NET environment is available.
3. Expand `docs/feature-parity.md` into per-version/per-platform behavior rows across all 51 projects, registration tables, options/defaults, generators, GUI workflows, and exporters.
4. Add independent generic fixtures and source-derived regression coverage for untested SerializedFile generations and malformed layouts; obtain a usable .NET environment before claiming C# differential validation.
5. Complete CLI options/configuration, resource handlers, export formats, UI workflows, and required integration/end-to-end verification.

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
- Platform file registration, source backend detection, and Mono assembly inventory are implemented for the audited layouts; product application-flow integration remains unfinished. Phase 1 remains open.


## Platform game-file registration

- Added `PlatformStructure::collect_game_files` in `src/io/platform_file_inventory.rs`. It visits DataPaths in source order, registers `data.unity3d`, `datapack.unity3d`, and the source primary SerializedFile name patterns, and uses logical base paths for `.split0` parts.
- WebGL data files and the matched WebPlayer bundle follow their constructor registration rules. Streaming bundle scans use the source platform paths; Android scans the APK `assets` root recursively while skipping its direct `bin` subtree, even when `skip_streaming_assets` is true, matching the source Android override. Bundle candidates are accepted only when the raw UnityFS/UnityRaw/UnityWeb header is recognized.
- The scanner bounds probed file count/size/aggregate bytes, directory count/depth, and all enumerated path lengths. Seven synthetic tests cover primary names, compressed game files, streaming bundles and skip mode, Android traversal, Web registrations, explicit missing DataPaths, and limits.
- `cargo test --workspace --locked`: exit 0; 190 tests passed, 0 failed, 0 doc tests. Warnings-denied workspace Clippy and `cargo fmt --all -- --check` passed. `tools/verify-baseline.ps1` confirmed 1,449 reference files unchanged; `git diff --check` passed.
- At this stage, platform inventory did not make unreferenced top-level bundles/WebData initial inputs. The following section records the initial-expansion implementation that superseded this limitation.


## Platform registered containers in the SerializedFile workspace

- Platform inventories expose source-named primary inputs separately from all registered game files. The loader now dispatches every registered non-primary file, adds direct SerializedFiles, records Resource candidates, and recursively expands registered UnityFS, UnityRaw, UnityWeb, and WebData containers before external dependency resolution.
- SerializedFile and Resource leaves are registered under their relative container paths. The workspace receives the expanded-container identities up front, so an external reference to an already-expanded source container is reported as `ContainerAlreadyExpanded` instead of mounting it again.
- Aggregate input bytes, visited entries, mounted bytes, source-container count, SerializedFile count, and dependency-registry size remain bounded. The synthetic WebData case verifies one leaf load and both `AlreadyLoaded`/`ContainerAlreadyExpanded` outcomes; a second case verifies the initial SerializedFile limit.
- Validation at this increment: `cargo test --workspace --locked --quiet` passed 198 tests; warnings-denied workspace Clippy, `cargo fmt --all -- --check`, reference-baseline verification, and `git diff --check` passed.
- Arbitrary unregistered input paths, complete product UI flow, .NET/real-platform differential, and the Phase 1 gate remain incomplete. Do not inspect user AssetBundles or UABEA.


## Platform scripting backend and Mono assembly inventory

- Platform inventory now reports `Unknown`, `Mono`, or `Il2Cpp`. IL2CPP detection requires the source platform's game-assembly and global-metadata marker pair; Mono is selected only when that pair is absent and the platform ManagedPath has direct `.dll` files. Wii U remains Mono-only, while WebGL/WebPlayer remain Unknown.
- The source ManagedPath rules are implemented for Windows/Linux/macOS, Android, iOS, Switch, PS4, Wii U, and Windows Phone. Mono assembly registration reads only direct `.dll` files from ManagedPath; duplicate basenames are reported and deterministic first-path registration is retained. Android's `libil2cpp.so` search is recursive and uses the configured directory bounds.
- Four synthetic tests cover Mono assembly collection and its limit, the required IL2CPP marker pair, eight platform-specific IL2CPP marker pairs taking precedence over Mono files, and Wii U's Mono-only behavior. The full workspace suite passes 196 tests.
- Warnings-denied Clippy, formatting, baseline verification (1,449 reference files unchanged), and diff check passed. No .NET or real platform-build differential has run.
- A narrow inspect CLI now connects local directories to platform or Mixed registry/workspace loading; the source GUI/export workflow and full Phase 1 gate remain open.


## Source-preserving local directory workflow

- `SmartStream::from_reader` now retains any owned seekable reader as read-only shared VFS storage. `VirtualFileSystem::insert_file_stream_with_parents` is public for this import boundary.
- Added `SchemeReader::mount_directory_preserving_containers`: it imports every discovered regular file by its relative path, combines `.splitN` parts into one logical file-backed stream, and leaves containers unexpanded for source platform detection and registration.
- The inspect CLI first uses this source-preserving mode. A detected platform uses `collect_game_files` and `load_primary_serialized_workspace`, which expands registered initial containers. Mixed inputs are re-imported through the recursive directory mount and then loaded through `MixedGameStructure`.
- Synthetic split-WebData coverage verifies that the exact container remains a VFS file and dispatches after import. CLI tests cover both the mixed fallback and a Windows platform directory whose `data.unity3d` is expanded with a matching external leaf. Current verification passed 200 library and 4 binary tests; warnings-denied Clippy, formatting, CLI help/version, baseline verification, and diff checks passed.
- Full local FileSystem parity, the rest of CLI/product workflows, export/UI, and the Phase 1 gate remain incomplete. User AssetBundles and UABEA remain uninspected.


## File-scoped object graph navigation

- `ObjectReferenceGraph::object_indices_for_path_id` returns every matching object-table index, so duplicate PathIDs stay distinct. Per-file and multi-file graph APIs now expose outgoing edges by source object; the multi-file graph also exposes normalized file-scoped object identities and incoming uniquely resolved edges.
- `SerializedValue` now exposes ordered struct fields and array elements, PPtr identity, and exact-name unique-field lookup. Duplicate field names and non-struct lookups return explicit errors so later resource and scene processors cannot silently choose an arbitrary field.
- Unique lookup also follows nested fields named `Base`, the TypeTree representation used for inherited object fields; duplicate names across that chain remain errors. `SceneGraph::from_decoded_roots` checks that every scene-class object is supplied exactly once before reporting completeness.
- Caller file indexes disambiguate duplicate normalized file names, and object-table indexes disambiguate duplicate PathIDs. Incoming-edge lookup scans the already bounded reference list rather than allocating a second graph-sized index.
- Synthetic tests cover duplicate identity lookup, normalized file identity, invalid object indexes, and incoming/outgoing edges. Validation results are recorded in `docs/validation-log.md`.
- Next: build runtime object classes and field-to-object relationships from source-generated schemas, then add explicit no-TypeTree and managed-reference paths. Full Unity object construction, resource handlers, exporters, product UI, and the Phase 1 gate remain incomplete; user AssetBundles and UABEA remain uninspected.


## Initial scene relationship model

- `processing::scene::SceneGraph` records every object-table identity, then decodes GameObject (class ID 1), Transform (4), and RectTransform (224) roots, including IDs derived from resolved modern serialized types. It follows nested `Base` structs for inherited fields and retains GameObject name bytes and active state, ordered `m_Component` links, Transform ownership, father, and children links.
- Pointer results preserve null, uniquely resolved local or workspace object, missing/ambiguous local/external target, missing/ambiguous dependency name, and invalid FileID as separate states. `resolve_with_multi_file_graph` verifies the source file and every object-table identity, matches each relationship to its TypeTree field path, then applies all target updates atomically. Decode and shape errors remain per-object issues; scene record, relation, decoded-byte, name-byte, field-path, and issue counts are bounded.
- Synthetic values verify ordering, target classification, raw invalid UTF-8 name preservation, inherited fields, duplicate/missing field diagnostics, malformed arrays, modern resolved-type class IDs, incomplete root sets, and bounds. Generated generation-10 source/target SerializedFiles exercise complete GameObject decoding and local/cross-file component resolution. A complete generated 2019.4 Transform SerializedFile exercises decoded local values; synthetic TypeTree roots exercise RectTransform layout values and a layout without optional root-order/Euler fields.
- `SceneGraph::audit_local_transform_links` checks local father/children reciprocity, duplicate links, and target Transform class. The local baseline calculates root order from the father's ordered child array and throws if a non-root Transform is absent there. Rust reports the inconsistency without mutating input values. This graph-local audit cannot verify reciprocal links in a different file yet.
- `SceneGraph::discover_local_roots` finds file-local candidates when a GameObject has no Transform or its first resolved Transform has a null father, matching `SceneHierarchyObject.GetRoots`. It preserves serialized component order, rejects root claims when the component list is incomplete, and reports unresolved component/father states. It does not assign these candidates to source SceneDefinitions or synthesize missing Transform objects.
- `processing::scene_naming` ports the source `maindata`/`levelN` boundary at Unity 5.3, inverse naming, BuildSettings path recovery, and duplicate scene path checks. It is a tested pure mapping layer; it is not connected to parsed BuildSettings assets, AssetCollections, SceneGUIDs, or streamed-scene bundle metadata.
- `processing::scene_metadata` extracts ordered paths from BuildSettings `m_Scenes` and source-normalized legacy `m_Levels`; `decode_build_settings_scene_paths_from_parts` now connects that parser to original SerializedFile bytes, parsed metadata, and bounded object decoding. `inspect` lazily reads each file only when it finds a class ID 141 object and prints the decoded paths.
- A generated generation-10 complete BuildSettings SerializedFile exercises the TypeTree-to-path pipeline. Latest tests: `cargo test --workspace --locked --quiet` passed 231 library tests and 4 binary tests; warnings-denied workspace Clippy passed. Formatting check, baseline verification, and final diff check are recorded after documentation changes. There is no original Unity file differential.
- Next: identify source scene collections using the baseline's LevelGameManager classes, attach BuildSettings paths and OcclusionCullingSettings SceneGUIDs, and parse streamed-scene `SceneHashes`/container relationships; then continue into `PrefabProcessor`'s hierarchy ownership/recursion, keeping cross-file relations explicit. No Unity project export is implemented here; Phase 1 remains open. User AssetBundles and UABEA remain uninspected.

## CLI scene graph and root summary

- inspect now discovers class IDs 1/4/224 in loaded workspace metadata, builds a bounded multi-file PPtr graph once, and for each matching file reads original VFS bytes, reparses the SerializedFile, resolves scene edges, discovers workspace-aware root candidates including external Transforms, and audits local father/child links.
- The summary prints GameObject/Transform counts, relationship count, decode completeness, and counts of root candidates and hierarchy issues. It prints candidate PathIDs and reasons. Errors remain command errors; malformed decoded object data is represented by SceneGraph issues.
- Latest verification: cargo test --workspace --locked --quiet passed 231 library and 4 binary tests; warnings-denied workspace Clippy, format check, baseline verification (1,449 files unchanged), and git diff --check passed. The CLI smoke fixture has class ID 1 but a generic Root TypeTree, so the branch executes without validating GameObject fields or asserting printed scene output; component APIs have synthetic coverage.
- Next: create a CLI-level scene fixture and assert the reported candidate and diagnostics, then continue identifying source scene collections from LevelGameManager classes and attach BuildSettings paths/SceneGUIDs and streamed-scene SceneHashes. Phase 1 remains open; no user AssetBundles or UABEA were inspected.

## Scene collection manager classification

- The inspected baseline extension enumerates four classes inheriting LevelGameManager: OcclusionCullingSettings (29), RenderSettings (104), LightmapSettings (157), and NavMeshSettings (196). SceneDefinitionProcessor uses any ILevelGameManager in a collection to mark it as a scene collection.
- Added this source-derived ClassID mapping. inspect reports candidate files plus each manager class and PathID. The mapping unit test covers all four IDs and representative non-manager values.
- Latest verification: 232 library and 4 binary tests passed; warnings-denied Clippy, formatting, baseline verification, and diff check passed. No original Unity scene file was used.
- Next: associate SceneGUID values and scene candidates with BuildSettings paths using source collection identity rules. Continue with streamed-scene AssetBundle SceneHashes/container mapping and SceneDefinition aggregation. Phase 1 remains open; user AssetBundles and UABEA remain uninspected.

## OcclusionCullingSettings SceneGUID extraction

- Added strict optional extraction of SceneGUID/m_SceneGUID from decoded class ID 29 objects. The GUID struct must contain exactly one spelling for each of the four source u32 data words; absent TypeTree fields remain absent and malformed/ambiguous values stay errors.
- inspect lazily reads and parses a file when it finds OcclusionCullingSettings, then reports the source word values. A generated generation-10 SerializedFile fixture exercises this TypeTree and CLI path; the test asserts the decoded four words and that inspect completes. There is no Unity-produced file differential.
- Latest verification: 234 library and 5 binary tests passed; warnings-denied Clippy, format check, 1,449-file baseline verification, and diff check passed.
- Next: map scene manager files to collection identities, associate BuildSettings paths and the decoded SceneGUIDs, then handle streamed-scene AssetBundle SceneHashes/container entries and SceneDefinition aggregation. Phase 1 remains open; user AssetBundles and UABEA remain uninspected.

## BuildSettings paths on scene collection candidates

- Source collection identity is SerializedAssetCollection.Name = file.NameFixed; FileBase.NameFixed applies FixFileIdentifier normalization. The collection OriginalVersion drives SceneHelpers.TryGetScenePath.
- inspect now resolves each manager candidate name through the unique decoded BuildSettings path list, applies the pre/post-5.3 filename rule, recovers the Unity path, and marks duplicate entries. Missing, conflicting, stale, malformed-version, and unusable-path states stay distinct.
- Distinct BuildSettings path arrays are reported as ambiguous. The source selects the last BuildSettings encountered in AssetCollection order; workspace ordering is not yet proven equivalent. A missing serialized Unity version is not replaced with a guessed default.
- Latest verification: 236 library and 6 binary tests passed; a generated two-file summary test asserts the SceneGUID words and resolved BuildSettings path; warnings-denied Clippy, formatting, 1,449-file baseline verification, and diff check passed.
- Next: recover the exact BuildSettings selection order/default-version behavior, inspect the source AssetBundle SceneHashes/container mapping path, and then aggregate scene collection identity, path, and GUID into SceneDefinitions. Phase 1 remains open; user AssetBundles and UABEA remain uninspected.

## Streamed-scene bundle path mapping

- Added bounded pure mapping helpers for decoded AssetBundle SceneHashes and Container entries. SceneHashes paths select the first matching normalized bundle collection. Container fallback assigns each ordered path key to the next collection flagged as a scene and advances the source cursor.
- The helper removes extensions, applies the source rooted-path rule, and ensures an Assets/ prefix. Tests cover duplicate names, relative/rooted paths, ordered fallback, missing candidates, and limits.
- Latest verification: 239 library and 6 binary tests passed; warnings-denied Clippy, formatting, 1,449-file baseline verification, and diff check passed.
- Next: recover the class ID 142 TypeTree shapes for streamed-scene flags, SceneHashes, and Container, then preserve serialized bundle boundaries so the mappings can be applied to the correct scene collection files. Do not inspect user AssetBundles or UABEA before the Phase 1 gate.

## Streamed-scene AssetBundle field extraction

- Added `extract_streamed_scene_bundle_metadata` in `src/processing/scene_collections.rs`. It reads the optional streamed-scene flag, `SceneHashes` pair strings, and ordered `Container` path keys from generic TypeTree-decoded values; both `m_` serialized names and generated property aliases are accepted. Entry counts and string bytes are bounded across both arrays.
- A generated minimal class ID 142 SerializedFile fixture now exercises TypeTree decoding through field extraction and verifies the null `Container.second.asset` PPtr. Unity Technologies UnityDataTools documents the corresponding scene-bundle field and pair shapes: https://github.com/Unity-Technologies/UnityDataTools/blob/main/Documentation/assetbundle-format.md. The fixture is partial and synthetic, not a Unity-produced file.
- The frozen local AssetRipper tree references generated `IAssetBundle` accessors but omits the emitted class ID 142 schema; the expected field names and pair layout are therefore not yet verified against a class 142 TypeTree. No actual class 142 SerializedFile object was used. No bundle grouping or path override application exists yet, and Phase 1 remains open.
- Next: carry source container/bundle identity through the workspace and apply scene overrides to the right collections; extend validation across additional class 142 schema generations. User AssetBundles and UABEA remain uninspected.

## Recursive container scope identities

- Recursive container mounting now reports each source container's virtual root, including nested/wrapped containers, in traversal order. This recovers boundaries that would otherwise be lost when SerializedFiles are flattened into a VFS.
- Per-traversal scope strings are bounded at 64 MiB; report reservations are fallible. Synthetic tests cover root/nested order and limit failures.
- `LoadedSerializedFile` now retains the deepest source container scope, source entry index, and direct collection index for both initial and dependency expansions. `ordered_collection_file_indices(scope)` returns each scope's direct collections in source order and rejects gaps. Next: connect this per-scope index to class ID 142 SceneHashes/Container extraction and scene mapping, then represent the full parent/child `SerializedBundle` tree. No bundle or user sample content was inspected.

## Streamed-scene mapping branch selection

- `map_streamed_scene_bundle_paths` now applies the source precedence rule: nonempty SceneHashes first, otherwise ordered Container paths against current scene flags. Non-streamed, missing-path, and mismatched-collection states remain explicit.
- Synthetic tests cover precedence, first duplicate-name selection, Container fallback, missing path data, and count mismatches. Next: connect TypeTree-decoded class ID 142 objects from `inspect` to this mapper using `ordered_collection_file_indices(scope)`, then record effective scene-path overrides. Full SceneDefinition and bundle-tree processing remain open; user AssetBundles and UABEA remain uninspected.

## CLI streamed-scene path mapping

- `inspect` now decodes class ID 142 objects, uses their owning container scope to build direct collection names in source order, maps SceneHashes or Container paths, and reports the final streamed-scene override. Same-scope AssetBundle objects are processed by owner collection and object order so later source assignments take precedence.
- The synthetic WebData end-to-end test confirms the TypeTree decode, class ID 142 mapping, and effective path output. Aggregate inspect limits bound bundle objects, metadata entries/string bytes, and path mappings. Next: construct source SceneDefinition/AssetCollection models and continue into Prefab/scene export. No Unity-produced class ID 142 differential or user sample inspection has occurred; Phase 1 remains open.

## SceneDefinition planning

- Current phase: Phase 1, independent Rust port; gate not passed. Frozen reference tree remains `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper`; only the Rust repository is a write target. Latest pushed revision before this increment was `ca63852daddd6644b78195195e73930a6c4f92a6`.
- Added `plan_scene_definition` and connected it to CLI inspect. It reports the source collection file index, normalized collection name, final name/path, path source, and whether SceneGUID words are preserved or a random GUID must be generated at export. Streamed-scene paths override resolved BuildSettings paths; unresolved paths use the source `FromName` fallback. Generation is deferred; no fixed or fabricated GUID is produced.
- Synthetic verification: 250 library tests and 7 CLI tests passed, along with warnings-denied Clippy, format check, the 1,449-file baseline verifier, and `git diff --check`. An initial boundary-test failure was caused by a test collection name exceeding its earlier configured bound; the input was corrected and the full suite then passed.
- No Unity-produced files, user AssetBundles, or UABEA content were inspected. No actual `AssetCollection`/`SceneDefinition` objects, full `SerializedBundle` tree, EditorBuildSettings asset, Unity project export, or Unity runtime behavior are implemented or verified.
- Next: materialize and test the complete parent/child `SerializedBundle` representation with direct collection source order, then build source-compatible collection grouping and aggregate the current scene plans into SceneDefinition and EditorBuildSettings models. Keep the source BuildSettings last-encountered behavior unresolved until the source traversal order is represented exactly. After that, continue the remaining baseline processors and product entry points; do not unlock Phase 2 before the complete Phase 1 gate.

## Workspace container tree and collection links

- Current phase: Phase 1; gate not passed. Only `C:\Users\koishi\Desktop\RustAssetripper\RustAssetripper` is writable; the local AssetRipper baseline remains read-only. Latest pushed revision before this increment was `110ba5a68e07f3ef017f18e75dc7ef64d38649d9`.
- Mount reports retain each ResourceFile leaf's path, owning container, source entry, and direct order; initial and dependency imports remap those links into workspace-global indexes. Streamed-scene AssetBundle mapping now reads the owning bundle node's direct collection indexes rather than looking up collections by scope path, so pathless root nodes are supported.
- Verification is synthetic: 255 library tests and 8 CLI tests passed; warnings-denied Clippy, format, 1,449-file baseline verification, and diff check passed. No user AssetBundle or UABEA content was inspected.
- FailedFiles and actual AssetCollection/SceneDefinition objects remain unmodeled. Resource bytes remain in the VFS; resource decoding/export and Unity runtime behavior are not implemented or verified.
- Next: add an explicit serialized collection/object model over the parsed records, then materialize scene associations from decoded scene-manager objects. Continue with generated EditorBuildSettings, scene export, remaining baseline processors, and product entry points; Phase 1 remains open.


## Workspace-aware cross-file scene root discovery

- Current phase: Phase 1; gate not passed. Latest pushed revision before this increment: 184bb3bf12d61f74555943b503a1f1e6f39d8a4c. Only the RustAssetRipper repository is writable.
- SceneGraph root discovery now uses the joined workspace reference graph to follow GameObject component pointers into other loaded SerializedFiles, identify external Transform/RectTransform classes, and inspect the selected Transform m_Father edge in its owning file. The CLI summary reports workspace-aware root candidates while reciprocal transform auditing remains file-local.
- Synthetic generation-10 SerializedFiles verify that a GameObject with an external Transform whose father is null is a root, that a missing father target remains an issue instead of becoming a root, and that a same-named source graph with a different serialized PPtr pair is rejected. MultiFileObjectReference retains the serialized FileID/PathID pair for this identity check. No Unity-produced file, user AssetBundle, or UABEA was used.
- Verification: cargo test --workspace --locked --quiet passed 270 library and 10 binary tests; warnings-denied Clippy, formatting, 1,449-file baseline verification, and git diff --check passed.
- Remaining: root candidates are not yet assembled into scene exports; reciprocal parent/child auditing across files, scene object ownership, Prefab processing, and Unity project export remain open. Phase 1 remains open.
- Next: extend reciprocal Transform hierarchy auditing across the workspace and connect scene object records to source SceneDefinition and export processing, keeping aggregate limits explicit.
## Workspace reciprocal Transform hierarchy audit

- Current phase: Phase 1; gate not passed. The last pushed revision before this increment was `5ce5654c946ec817fb055221f1c772e1f0d529bc`. Only the `RustAssetRipper` repository is a write target; user bundles and UABEA remain untouched.
- `SceneGraph::audit_workspace_transform_links` now checks Transform/RectTransform `m_Father` and `m_Children` reciprocity through the ordered multi-file PPtr graph. It preserves unresolved targets and reports missing, duplicate, or mismatched counterparts. It reports incomplete source or target scans instead of claiming a reciprocal edge is absent when a TypeTree scan was partial. CLI inspection now prints the number of checked links, issue count, and audit-complete state.
- Synthetic two-file generation-10 tests cover valid child/parent reciprocity in both files, a missing reverse edge, incomplete source and counterpart scans, strict array-index matching, and configured relationship/issue limits. `cargo test --workspace --locked --quiet` passed 275 library tests and 10 CLI tests; warnings-denied Clippy, format check, 1,449-file baseline verification, and `git diff --check` passed.
- No Unity-produced asset, user VRChat AssetBundle, UABEA source/executable, or C# runtime differential was used. The full Phase 1 port remains incomplete; SceneDefinition ownership, Prefab construction, Unity project export, full Transform/RectTransform version coverage, and a dedicated CLI output assertion remain open. Keep the Phase 2 gate closed.
- Next: connect scene records to the source SceneDefinition and export path, retaining the workspace audit's incomplete-scan semantics and aggregate limits.
## SceneDefinition-owned workspace hierarchy model

- Current phase: Phase 1; gate not passed. The last pushed revision before this increment was `7b94254f0c8ad7abf94019a25d7be272267c6104`. Only the `RustAssetRipper` repository is a write target; no user bundle or UABEA data was accessed.
- The inspect pipeline now binds compact GameObject/Transform/RectTransform graph records, PPtr relationships, roots, decode issues, and reciprocal audit results to each matching `WorkspaceSceneDefinition`. Manager-only scene collections receive an explicit empty graph; streamed-scene-only collections are modeled after their AssetBundle path assignment is known.
- The model verifies file/PathID identities against the joined graph, preserves external root Transform identity, and makes overall completeness depend on graph resolution, per-object reference-scan completeness, root discovery, and hierarchy audit. Aggregate budgets are transactional; the zero-object test confirms a rejected insertion leaves no partial graph.
- Verification: `cargo test --workspace --locked --quiet` passed 276 library and 10 binary tests. A focused API test and streamed-scene CLI test passed. After the test initializer cleanup, warnings-denied Clippy, format check, the 1,449-file baseline verifier, and `git diff --check` all passed.
- No Unity-produced scene, user AssetBundle, UABEA, or C# differential was used. This remains a graph view, not concrete generated component assets, SceneHierarchyObject serialization, Prefab processing, or Unity project export. Phase 1 remains open.
- Next: extend parsed scene records to the source SceneHierarchyObject asset inventory and generated component type system, then carry that into Prefab and Unity-project export processing.

## Source-order SceneDefinition and Prefab root progress

- Current phase: Phase 1; gate not passed. The latest pushed revision before this increment was `03af590f1ea0c77a5dd2f0bbd9786c441ec07a21`. Only `C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper` is writable; the local AssetRipper source is read-only. UABEA and user AssetBundles remain untouched.
- SceneDefinition discovery and PrefabInstance root diagnostics now follow the reconstructed `GameBundle.FetchAssets()` collection order, preserving object-table order within each collection.
- Transform snapshots now index `m_Father` alongside `m_Children` and `m_GameObject`. `WorkspaceSceneHierarchies::get_root_game_object` follows the source first-Transform rule, resolves parent and owner PPtrs across files, and preserves explicit issues for unavailable or malformed edges. The source no-Transform case resolves to the input GameObject after the preceding missing-Transform synthesis stage.
- Verification: after adding the explicit parent-with-null-GameObject termination assertion, `cargo test --workspace --locked --quiet` passed 293 library and 10 CLI tests; warnings-denied Clippy, format check, `git diff --check`, and the 1,449-file baseline verifier passed. No Unity-produced input, user AssetBundle, UABEA, or original runtime was used.
- Remaining: this is root identity planning, not the source-wide `PrefabProcessor.Process` execution. Scene GameObject processed membership, ordered existing-PrefabInstance handling, remaining-GameObject deduplication, `SetPrefabInternal`, generated PrefabHierarchy/PrefabInstance assets, CLI integration, and project export are open. Phase 1 remains open.
- Next: rerun the updated focused and full validation, then implement an ordered identity-level PrefabProcessor plan: seed processed GameObjects from SceneDefinitions, apply existing PrefabInstances in source asset order, then resolve remaining GameObjects through `get_root_game_object` and the existing hierarchy walk. Preserve already-processed, explicit-null, unresolved, and incomplete-walk outcomes separately; do not claim asset generation or export.

## Ordered PrefabProcessor identity planning

- Current phase: Phase 1; gate not passed. Latest pushed revision before this increment: `e7c5cb6c7028bd85a041bb3141319fcb5ec63bc8`. Only `C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper` is writable. The local AssetRipper reference is read-only. No user AssetBundle or UABEA data has been accessed.
- Added `plan_workspace_prefab_processing` and wired it into CLI `inspect`. It seeds SceneDefinition GameObjects, processes existing PrefabInstances in source collection/object order, then applies GetRoot and global dedup to remaining GameObjects. The plan preserves root-null, root-unavailable, already-processed, completed, and incomplete-hierarchy outcomes. Incomplete hierarchy processing stops the plan.
- Hierarchy plans add virtual Transform entries with source default values where `AddMissingTransforms` applies. They have no generated PathID and do not construct or mutate Unity objects. CLI output labels the result an identity plan and reports incomplete graph conditions.
- Verification: 298 library and 10 CLI tests passed; warnings-denied Clippy, format check, `git diff --check`, and the baseline verifier (1,449 files unchanged) passed on this worktree.
- Remaining: SceneAsset/PrefabHierarchy/PrefabInstance generated asset materialization and serialization, `SetPrefabInternal` effects, project export, all other baseline feature areas, and the Phase 1 completeness gate. No Phase 2 work has begun.
- Next: finish formatting/baseline/diff validation, commit/push this increment, then inspect the reference schemas/creation code for generated SceneAsset and PrefabHierarchy object construction and implement corresponding typed Rust models using the identity plans as their inputs.

## Processed prefab asset PathID allocation plan

- Current phase: Phase 1; the gate has not passed. Before this increment, local main and our configured origin/main both pointed to e094f00435c15b71300e5d98473ec762212c7479. Only the RustAssetRipper repository is a write target; no other remote repository is accessed or modified.
- Traced the baseline allocation rules: processed collections start with independent PathID counters at zero; each generated asset increments its collection counter before insertion. The PrefabProcessor creates Prefab Hierarchies, Generated Prefabs, and Missing Prefab Transforms collections first; missing Transforms are added in FetchAssets() order; SceneAsset objects are then allocated by SceneDefinition order; existing and generated PrefabHierarchy objects share a separate collection sequence.
- Added a bounded ordered PathID allocation plan and wired it into inspect. It retains collection kind/name, generated asset class ID and local collection PathID, source GameObject/root identities, and the generated PrefabInstance identity linked from its PrefabHierarchy. It returns no PathIDs if Prefab processing or any selected hierarchy graph is incomplete.
- Synthetic tests cover the allocation plan, independent collection counters, generated Transform defaults, generated Prefab linkage, and incomplete-plan behavior. Full verification: 301 library and 10 CLI tests passed; warnings-denied Clippy, format check, baseline verification (1,449 files unchanged), and git diff --check passed.
- A first full test run found an inspect fixture whose selected collection had no recovered hierarchy graph. This is now surfaced as an incomplete allocation plan with no assigned IDs; the final full suite passes.
- This is still an identity and PathID plan. Full source-generated object field initialization, component mutation, SetPrefabInternal, dependency/PPtr construction, serialization, and Unity project export remain unimplemented. No user AssetBundle or UABEA data has been inspected; the Phase 2 gate stays closed.
- Next: inspect source-generated class schemas and SetPrefabInternal/component-addition effects, then implement actual typed generated object values behind the validated allocation identities. Do not claim this plan is object materialization or export.

## Generated Transform component append and export-path source audit

- Current phase: Phase 1; the gate remains open. This increment follows pushed commit 09c0be04b71b0ac7a17ec5b13b7296c9072d7eef. Only the RustAssetRipper repository is writable; the AssetRipper source is read-only, and no other remote repository or user sample was accessed.
- Source audit: GameObjectExtensions.AddComponent appends a ComponentPair, sets its ClassID, then stores a PPtr to the generated Transform relative to the GameObject collection. Added a typed append record containing the verified GameObject identity, existing component slot count, Transform class ID and allocated Transform identity. Null component PPtrs contribute a slot and are not compacted.
- SceneAssetExportCollection maps the generated SceneAsset to Meta and its target Scene GUID; the scene YAML itself is emitted from SceneHierarchyObject by SceneExportCollection. PrefabExportCollection groups Hierarchy.ExportableAssets and chooses legacy Prefab asset output versus PrefabImporter according to IPrefabMarker. SetPrefabInternal walks the hierarchy and assigns PrefabInternal_C18P only when the prefab implements that marker.
- Verification: full suite passed 301 library and 10 CLI tests; warnings-denied Clippy, format check, 1,449-file baseline verification, and git diff check passed. No Unity or reference-tool differential was run.
- Still missing: mutation/materialization of source component arrays, generated asset serialized fields, dependency-table PPtr indexes, versioned IPrefabMarker behavior, AssetGroup MainAsset/hidden membership, and scene/prefab YAML plus meta export. The component append record is not a completed mutation or export.
- Next: model source component arrays as a checked overlay keyed by GameObject identity, preserving every source slot and appending generated Transform pointers; then resolve serialized PPtr file indexes from ProcessedBundle collection dependencies.

## Generated Transform component-array overlay

- Current phase: Phase 1; gate not passed. This increment follows pushed commit b0045c8495f7c6d4ed4f033150f074bae50abbe5. Only RustAssetRipper is a write target.
- Added WorkspaceSceneCollectionHierarchy::game_object_component_relationships_for_identity, which verifies the source GameObject identity and returns its component slots in serialized array order. The processed plan now retains an overlay for each missing-Transform GameObject: original zero-based slot order, original fileID/PathID, null or resolved target state, and a separate generated Transform append linked to its processed collection identity.
- A null source PPtr remains a slot. The synthetic fixture verifies source slot 0 is preserved and the generated component is appended at slot 1. Full verification passed: 301 library and 10 CLI tests, warnings-denied Clippy, format check, baseline verifier (1,449 files unchanged), and git diff check.
- This is still a non-mutating overlay. It does not write back GameObject m_Component values or decide the generated Transform's serialized PPtr fileID. Existing source PPtrs retain their original file scope in the overlay.
- Next: port AssetCollection.AddDependency/ForceCreatePPtr for processed collections so generated Transform references receive correct output collection file indexes while source PPtrs keep their source collection scope. Then materialize this overlay as a version-aware serialized GameObject value.

## Processed collection constructor PPtr dependency indexes

- Current phase: Phase 1; gate not passed. This increment follows pushed commit 41ce8ea4a97cc9dac241235053d8fe9fe77aa3db. The authorized cloud target remains only cocokoishi/RustAssetRipper.
- Source AssetCollection starts with itself at dependency index 0. AddDependency uses first existing identity or appends a new compatible collection; ForceCreatePPtr stores that dependency index with the target PathID.
- Added per-processed-collection first-touch dependency tables for the non-null constructor assignments used by generated Transform.m_GameObject and generated PrefabInstance.m_RootGameObject. The generated pointer plan names the semantic field, assigned fileID, PathID, and target source identity. Duplicate references to one source collection reuse its first fileID.
- Generated Transform, Generated Prefab, and scene-generated collection dependency indexes are complete for the constructor-assigned pointers modeled. Prefab Hierarchy dependency indexes are explicitly incomplete while AssetGroup lists, Root, and Prefab references remain unbuilt.
- Verification: 303 library and 10 CLI tests passed; warnings-denied Clippy, format check, baseline verification (1,449 reference files unchanged), and git diff check passed.
- Still open: serialized-file source dependency list updates for the appended GameObject component PPtr, PrefabHierarchy AssetGroup references and MainAsset mapping, generated PrefabInstance flags and metadata, version-specific pointer/class layouts, and actual serialization/export. This dependency plan is not a complete Unity object graph.
- Next: audit and replay source collection dependency changes made by processors before PrefabProcessor, then materialize the checked component overlay as typed SerializedValue with correctly scoped source and processed collection PPtrs.

## Source component PPtr dependency append planning

- Current phase: Phase 1; gate not passed. The last pushed revision before this increment was `e7d96bb5af4824e454b28eeca55e682b49aa02a8`. Only `cocokoishi/RustAssetRipper` is authorized for cloud changes; the local AssetRipper baseline remains read-only. No UABEA or user AssetBundle content was accessed.
- `ExportHandler.GetProcessors()` places SceneDefinition, OriginalPath, MainAsset, AnimatorController, AudioMixer, EditorFormat, and LightingData processors before PrefabProcessor. Those earlier passes can affect a source collection's dependency table, so the generated component PPtr FileID cannot safely be computed from the initial external-identifier count alone.
- The processed-prefab plan now records one source-collection dependency-append request per `(source file, generated collection)`, deduplicates repeated GameObjects while retaining the first owner, and deliberately leaves `file_id` unset until earlier processor dependency effects are modeled. CLI inspection exposes the request count and deferred status.
- Verification: the focused source-dependency test passed; full `cargo test --workspace --locked --quiet` passed 304 library and 10 CLI tests. Warnings-denied Clippy, format check, the 1,449-file baseline verifier, and `git diff --check` passed.
- Next: model or replay pre-PrefabProcessor mutations to each source collection dependency list and assign the append FileID only from that resulting ordered list; then create the typed serialized GameObject overlay. Keep Phase 2 locked until the complete Phase 1 gate passes.

## Ordered source dependency FileIDs across pre-Prefab processing

- Current phase: Phase 1; gate not passed. The last pushed revision before this increment is 39e33272e7b7fa8d30474e8a5921196b6fc731c3. Only cocokoishi/RustAssetRipper is authorized for cloud changes; the local AssetRipper baseline remains read-only. No UABEA or user AssetBundle content was accessed.
- AnimatorControllerProcessor writes generated StateMachine PPtrs into the source AnimatorController layer arrays before LightingDataProcessor. The layer-array dependency effect is not decoded yet. Source files containing AnimatorController class ID 91 are therefore marked uncertain, and all dependency FileIDs in those files stay deferred, including files whose controllers might have zero layers.
- For source files without that barrier, LightingDataProcessor's generated LightingDataAsset PPtr is planned only when the first LightmapSettings in source order exposes m_LightingDataAsset and the SceneDefinition contains RenderSettings. Those appends precede Prefab's generated Transform component PPtrs. FileIDs start after self and the original external identifiers, then follow deduplicated first-touch append order. With three external identifiers and no AnimatorController barrier, the synthetic LightingData and Transform FileIDs are 4 and 5.
- CLI inspection reports assigned and deferred FileIDs plus the number of source files held by the AnimatorController barrier. Verification: 309 library and 10 CLI tests passed; warnings-denied Clippy, format check, 1,449-file baseline verification, and git diff check passed.
- Still open: decode versioned AnimatorController LayerArray and StateMachineArray semantics to remove the safe deferral; write ordered dependency-table changes and typed GameObject component values; construct the remaining generated Prefab assets and serialize/export them. Phase 1 remains incomplete.
- Next: implement the version-aware AnimatorController layer/state-machine inventory, then materialize source dependency updates and the GameObject component overlay as typed SerializedValue while retaining all original PPtr slots and target identities.

## AnimatorController source dependency inventory

- Current phase: Phase 1; gate not passed. This increment follows pushed revision `e36036c5240d2d360eaa54d272f596d6f46456f9`. Only `cocokoishi/RustAssetRipper` is an authorized cloud write target; the local AssetRipper baseline remains read-only. No UABEA or user AssetBundle content was accessed.
- Replaced the all-controller-file deferral with a bounded TypeTree inventory. It accepts `m_LayerArray` and the baseline's historical `m_HumanLayerArray`, unwraps the generated `Data` layer value when present, and validates each `m_StateMachineIndex` against `m_StateMachineArray`. One source-collection dependency append is modeled for any file with at least one valid non-empty layer array. This append precedes LightingData and Prefab additions.
- A missing/ambiguous field, failed object decode, malformed index, or index outside the state-machine array defers only the affected source file's pending dependency FileIDs. Empty layer arrays add no dependency. Synthetic FileID checks show three original external identifiers followed by AnimatorController, LightingData, and Prefab entries at IDs 4, 5, and 6; without the AnimatorController append, the latter entries remain 4 and 5.
- Verification passed: 312 library and 10 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check`.
- Still incomplete: serialize the AnimatorController layer PPtrs and ordered source dependency-table edits; write the typed GameObject component overlay; finish generated Prefab assets and export. No end-to-end source TypeTree sample was used for this synthetic inventory, and Phase 1 remains open.
- Next: represent the AnimatorController source append as a first-class ordered dependency operation tied to its first generated StateMachine target, then materialize source SerializedValue PPtrs and dependency tables without dropping existing entries.
