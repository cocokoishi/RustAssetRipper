# Continuation status

## Current phase

Phase 1: full Rust port of the local AssetRipper source baseline. The Phase 1 gate has not passed. Phase 2 remains locked: no user AssetBundle contents have been opened, hashed, identified, or parsed, and UABEA has not been read or run.

## Worktree and baseline

- Active Git worktree: `C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper`.
- Only this RustAssetRipper repository has been modified. No other Git repository was contacted or changed; no remote write or push has occurred.
- The local reference snapshot is `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper`, not a Git checkout. Its 1,449-file checksum manifest is in `docs/baseline-files.sha256`.
- Reference identity recorded in `docs/baseline.md`: AssetRipper 2.0.0, `net10.0`, 51 solution projects, including 8 test projects and 6 source generators. Full feature/dependency/platform/option audit remains incomplete.

## Implemented in the Rust worktree

- Bounded endian-aware binary reader with checked slices, absolute offsets, alignment, strings, and contextual errors.
- SerializedFile format generation predicates and header parser, including the generation-9 endian header branch and generation-22 extended fields.
- Metadata prefix parsing, including pre-generation-9 metadata-at-end/endian handling, version text, target platform, and TypeTree flag.
- Metadata tables: serialized types, object records and checked payload ranges, local script identifiers, external file identifiers, reference types, dependencies, and user information.
- Legacy TypeTree records and the generation-10 / generation-12+ string-buffer form, including reference hashes and the audited common-string lookup table.
- TypeTree-driven object payload decoding for primitive fields, strings, arrays, nested structs, unknown fixed-size leaves, field/array alignment, and explicit allocation limits. Missing-TypeTree and managed-reference-registry inputs report explicit errors rather than being treated as decoded data.
- Complete-file `SerializedFile::parse` facade that reads a real encoded header, metadata, and referenced object payloads from the same borrowed byte slice; a generation-10 synthetic end-to-end fixture covers this path.
- Rust package has no third-party dependencies; `Cargo.lock` is checked in when checkpointed. Product CLI, bundle/container handling, decompression, full Unity object models and no-TypeTree recovery, reference graph, resource handlers, exporters, and UI have not been implemented.

## Latest verification

- `cargo fmt -- --check`: exit 0.
- `cargo test --workspace`: exit 0; 48 unit tests passed, 0 failed, 0 doc tests.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `tools/verify-baseline.ps1`: exit 0; all 1,449 reference files match the manifest.
- `dotnet --info`: exit 1 because PowerShell cannot find `dotnet`; AssetRipper build and tests remain unrun.
- Rust 1.74 is declared as the minimum but has not been tested. Rust 1.98.1 is the only verified toolchain.
- Tests use self-constructed byte fixtures and source-derived expected fields. No real Unity SerializedFile differential run has occurred.
- Exact commands and transient fixture/clippy failures are recorded in `docs/validation-log.md`.

## Next work

1. Continue the SerializedFile path beyond metadata: parse object payloads from TypeTrees into structured values, with alignment, arrays, references, and unknown-field retention tied to source behavior.
2. Audit and implement stream abstractions, bundle variants, block directories, and compression paths from the current source snapshot.
3. Expand `docs/feature-parity.md` into per-version/per-platform behavior rows across all 51 projects, registration tables, options/defaults, generators, GUI workflows, and exporters.
4. Add independent generic fixtures and source-derived regression coverage for untested SerializedFile generations and malformed layouts; obtain a usable .NET environment before claiming C# differential validation.
5. Continue into object/reference construction, all resource handlers, export formats, configuration, CLI, UI, and complete integration/end-to-end verification.

Do not read UABEA or any user AssetBundle until the first-phase gate has actual evidence for every required capability.
