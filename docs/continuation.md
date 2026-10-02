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
- PPtr file-scope classification: null PathIDs, unique/missing/ambiguous local targets, one-based external table entries, and invalid FileIDs. ObjectReferenceGraph indexes PathIDs per SerializedFile, scans decoded TypeTree values into field-addressed PPtr edges, and reports undecoded objects, raw leaves, trailing bytes, and limit failures instead of claiming a complete graph. External bundles are identified but are not loaded or traversed yet.
- Stream and filesystem groundwork: PartialStream provides bounded seekable views; FileRangeStream provides nested file ranges with independent logical positions over a shared handle; MultiFileStream composes ordered seekable parts; SchemeReader::read_stream and read_path dispatch complete inputs with a 2 GiB default size bound. Local regular-file and .splitN path opening, numeric ordering, and missing-part checks are implemented. SmartStream null/memory/file/temp modes, Arc references, and partial views are implemented. VirtualFileSystem now provides a bounded in-memory directory tree, SmartStream-backed files, file operations, directory creation, and deterministic recursive enumeration. Synthetic tests cover these behaviors; OS-backed positional reads, the local FileSystem adapter, and mounting parsed container entries into the virtual tree remain outstanding.
- Bundle container work: UnityFS has partial header and block/directory metadata for start/end placement, version-7 alignment and padding flags, uncompressed/raw LZ4/raw LZMA payloads, Zstandard storage blocks, and directory-entry extraction across blocks. UnityRaw and UnityWeb have bounded versioned header, directory, and entry parsing; UnityWeb decodes the final size-prefixed LZMA scene chunk. Unity WebData 1.0 has bounded directory parsing, borrowed entry ranges, and a bounded writer. Synthetic fixtures exercise these paths only. Source scheme ordering, top-level dispatch, nested GZip/Brotli decoding, and WebData, UnityFS, UnityRaw, and UnityWeb entry dispatch are implemented with synthetic fixtures; external-file integration and real-file differentials remain incomplete. UnityArchive parsing and writing are explicitly unsupported by the source baseline; LZHAM is also explicitly unsupported.
- Compression wrappers: GZip magic detection, bounded concatenated-member decoding, and bounded GZip writing are implemented. Brotli signature parsing follows RFC 7932 metadata length semantics and bounded Brotli decoding. An RFC-constructed marked stream exposed an off-by-one in the local source detector; the Rust detector has a regression fix, recorded in `docs/differential-findings.md`. Fixtures cover GZip member/checksum behavior, limits, Brotli marker detection and decoding for WBITS 10–24, and nested dispatch. No C# or Unity-produced wrapper differential has run.
- Complete-file `SerializedFile::parse` facade that reads a real encoded header, metadata, and referenced object payloads from the same borrowed byte slice; a generation-10 synthetic end-to-end fixture covers this path.
- Rust package uses `lzma-rs`, `ruzstd`, `brotli-decompressor`, and pure-Rust `flate2` for codec paths; versions, feature flags, licenses, and validation limits are recorded in `docs/dependencies-and-licenses.md`, and `Cargo.lock` is checked in. The declared minimum Rust version is 1.87 due to `ruzstd`; only Rust 1.98.1 has been run. UnityFS, Raw/Web, wrapper decoder, and codec paths remain partial. The complete product input pipeline, virtual filesystem integration, CLI, remaining bundle codecs, full Unity object models and no-TypeTree recovery, multi-file reference graph, resource handlers, exporters, and UI have not been implemented.

## Latest verification

- `cargo fmt -- --check`: exit 0.
- `cargo test --workspace --locked`: exit 0; 126 unit tests passed, 0 failed, 0 doc tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `cargo tree --locked`: exit 0; the dependency graph is recorded in `docs/dependencies-and-licenses.md`.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest.
- `dotnet --info`: exit 1 because PowerShell cannot find `dotnet`; AssetRipper build and tests remain unrun.
- Rust 1.87 is declared as the minimum but has not been tested. Rust 1.98.1 is the only verified toolchain.
- Tests use self-constructed byte fixtures and source-derived expected fields. No real Unity SerializedFile differential run has occurred.
- Exact commands and transient fixture/clippy failures are recorded in `docs/validation-log.md`.

## Next work

1. Implement the local FileSystem adapter and mount parsed container entries into VirtualFileSystem; then connect the input pipeline to complete object construction and cross-file external resource resolution.
2. Add independent codec and real-file fixtures, and resolve source differentials such as the Brotli marker issue with executable C# evidence when a .NET environment is available.
3. Expand `docs/feature-parity.md` into per-version/per-platform behavior rows across all 51 projects, registration tables, options/defaults, generators, GUI workflows, and exporters.
4. Add independent generic fixtures and source-derived regression coverage for untested SerializedFile generations and malformed layouts; obtain a usable .NET environment before claiming C# differential validation.
5. Continue into object/reference construction, all resource handlers, export formats, configuration, CLI, UI, and complete integration/end-to-end verification.

Do not read UABEA or any user AssetBundle until the first-phase gate has actual evidence for every required capability.
