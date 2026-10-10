# Rust AssetRipper hand-off — 2026-10-09

## Stop point

This hand-off was refreshed after the user requested a source-file size audit and refactor. The current tree passes Rust formatting and workspace compilation, and the isolated managed probe builds. Test suites were not run for the structural split. The outstanding LightingDataAsset source-family matrix last failed at Unity 5.6 on `LightingData.asset.meta` as described below. Phase 1 remains open.

- Repository: `C:\Users\koishi\Desktop\RustAssetripper\RustAssetripper`
- Base HEAD before the source-size refactor commit: `e1ce91927a5818ea9c2e356a8bca06ac65c9e9f3` (tree `ff59484e4490aa4f635c5121008e4d2c25b68712`). The last fetched `origin/main` matched that base. Use live `git status` and `git log` for the current repository state. Do not make a hand-off-only commit.

## Mission and phase gate

Continue the complete Rust migration described in `AGENTS.md`. Phase 1 is full AssetRipper feature parity and independent verification. Phase 2 is the exhaustive VRChat bundle audit and remains locked until Phase 1 passes. Until then, do not inspect, hash, identify, scan, parse, or export user AssetBundles, and do not read or run UABEA. Keep the original AssetRipper source frozen and read-only.

The latest header-aware parity report found 386 rows: 362 verified, 12 implemented but unverified, 8 in progress, 3 investigated, and 1 externally blocked. There are 24 non-verified rows, no malformed or duplicate IDs, Phase 1 is OPEN, and Phase 2 is LOCKED. The current unresolved IDs are:

`RES-MATERIAL-EXPORT-001`, `IO-FS-IMPORT-PRESERVE-DIRECTORY-001`, `IO-STREAM-001`, `SER-TT-ABSENT-001`, `TYPE-SCRIPT-001`, `OBJ-REF-001`, `PROC-SCENE-001`, `PROC-ANIM-001`, `PROC-ANIM-STREAMED-001`, `PROC-ANIM-CONVERT-001`, `PROC-ANIM-CUSTOM-CURVE-001`, `RES-TEXTURE-001`, `RES-MODEL-001`, `RES-AUDIO-001`, `RES-ANIM-001`, `EXPORT-UNITY-001`, `EXP-SETTINGS-APPLY-001`, `PRODUCT-GUI-001`, `PRODUCT-I18N-001`, `RES-MESH-VIEW-001`, `EXP-GLB-001`, `RES-BLENDSHAPE-001`, `SER-ASSET-FACTORY-001`, and `EXP-FILE-NAME-001`.

Use `docs/phase1-closure-inventory.md` as the unique remaining queue. Keep the parity report's `Status` column interpretation header-based. Every coherent batch should reduce a Phase 1 blocker or settle a material source behavior with independent evidence. Do not count green tests alone as progress or mark a broad parity row verified from this bounded fixture work.

## Workspace and frozen baseline

- Rust repository: `C:\Users\koishi\Desktop\RustAssetripper\RustAssetripper`
- Frozen AssetRipper source: `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper`
- Baseline: AssetRipper 2.0.0 local source snapshot without Git metadata. `docs/baseline-files.sha256` lists 1,449 files; manifest SHA-256 is `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`.
- Isolated source build/probe copy: `%LOCALAPPDATA%\Temp\opencode\ar-ref`; .NET SDK 10.0.401. Do not build in the frozen source tree.
- Unity Editor 2022.3.22f1 is installed, but no valid Editor license is available. Unity import/runtime checks remain externally blocked.

## Current worktree

The path list below records the earlier hand-off state. The source-size refactor added Rust include fragments, Rust test shards, five managed-probe partial-class files, and `tools/check-source-file-lengths.ps1`; use the live `git status --short` output for the complete current path list. `docs/source-file-size-audit.md` records the full pre-refactor inventory and decisions.

Modified paths:

- `docs/differential-findings.md`, `docs/feature-parity.md`, `docs/next-agent-handoff.md`, `docs/phase1-closure-inventory.md`, `docs/validation-log.md`
- `src/export/collection_interfaces_fixture.txt`, `src/export/project_workflow.rs`, `src/export/scene_wiring.rs`, `src/export/texture_project.rs`, `src/export/value_yaml.rs`, `src/export/workspace_yaml.rs`, `src/main.rs`
- `src/processing/scene_hierarchy.rs`, `src/processing/lightmap_settings_capability_fixture.txt`
- `src/serialized_file/object.rs`, `src/serialized_file/object/writer.rs`
- `tools/probes/managed-project/Program.cs`

Untracked source-generated fixtures:

- `src/export/lightmap_project_source_5_0_fixture.txt` — SHA-256 `DDFA81853C92D2F8A703897576EBABB0507A3D7AE59DA79766815A7B92898D92`
- `src/export/lightmap_project_source_5_3_fixture.txt` — `1744CCB401A8BEAC658107B2CE42D51CA5300EA4BE9BE580C6B60B6765ED49E2`
- `src/export/lightmap_project_source_5_4_fixture.txt` — `335FA42CDA126413753A74225504F69596B857F9DC03CC24E91FCB4A5E2B6476`
- `src/export/lightmap_project_source_5_6_fixture.txt` — `6A5A1948B36C383C477E3BAE92FBA551D9E13AE2776F233062C467CC6524D246`
- `src/export/lightmap_project_source_2017_4_fixture.txt` — `A3C46E93383C974C117A6F3F2A32700415CDBB9EEFFC42F00EF154BFDC3C0913`
- `src/export/lightmap_project_source_2018_2_fixture.txt` — `EFB563BAA4D9FCF0148819C755C4CDFD7F312D87C3AF27A18C10B940F04681B0`
- `src/export/lightmap_project_source_2018_3_fixture.txt` — `FC1571941BECE12578DE584151FD6080AAE96C5A19986A0CA3930AF50845B013`
- `src/export/lightmap_project_source_2019_4_fixture.txt` — `93951F8E3B73362F0154C5BBB34FB00291074D49BE010BA578779C8B53F462B4`
- `src/export/lightmap_project_source_2022_3_fixture.txt` — `054D4A11819A9FB56C3DBD07A4D6FD624537A93FAE995CD88077C0483C37D570`
- `src/processing/lighting_data_asset_capability_fixture.txt` — `E10AE6FC467D99F83C421BC4209AD6D4E2039C146A5FF3F1BFB8F7E654F33FBF`

`src/export/collection_interfaces_fixture.txt` has a Git status anomaly: the earlier audit found its HEAD, index, and worktree blob hashes all equal (`308ddfcee3138d17a770d048d2efb448a7e2c1bf`) despite `git status` reporting it modified. Preserve it; do not stage, rewrite, or reset it until the anomaly is understood.

## Active increment: LightingDataAsset version families

The earlier source-matched Unity 2019.4.40f1 route handles an empty scene with one image lightmap and compares generated Scene/LDA YAML, metadata, BMP bytes, GUIDs, and pointer relations. Its focused test passed before this version-family expansion. That result does not establish support for scenes with object hierarchies or other LightingDataAsset versions.

This increment uses isolated probes against the frozen AssetRipper source and adds source-generated project fixtures for Unity 5.0, 5.3, 5.4, 5.6, 2017.4, 2018.2, 2018.3, 2019.4, and 2022.3. The managed-project probe emits capability rows for 32 Unity versions. Source facts captured include:

- Class 1120 `LightingDataAsset` is enabled from Unity 5.0 onward.
- The source LDA schema has no scene link at 5.0, uses `m_SceneGUID` at 5.3, has neither `m_Scene` nor `m_SceneGUID` across 5.4–5.6, and uses `m_Scene` from 2017.1 onward.
- `m_EnlightenDataVersion` is absent in the Unity 5 and 5.3 LDA schemas, and present from 5.4 onward.
- Inherited emitted YAML fields change at 2018.2 and 2018.3: legacy prefab fields before 2018.2; `m_CorrespondingSourceObject` plus `m_PrefabInternal` in 2018.2; and `m_CorrespondingSourceObject`, `m_PrefabInstance`, `m_PrefabAsset` from 2018.3.
- Unity 5.0 source Scene YAML calls the LDA pointer `m_LightmapSnapshot`; later source output uses `m_LightingDataAsset`.

Rust changes currently remove the 2018.3 planner guard, generate version-dependent LDA fields, and add versioned `TextureImporter` metadata layouts in `src/export/texture_project.rs`. The test `export_project_matches_source_lightmap_schema_families` in `src/main.rs` compares source fixtures across the 5.0, 5.3, 5.4, 5.6, 2017.4, 2018.2, 2018.3, and 2022.3 families. Source fixture data and the probe are uncommitted.

### Current failure and immediate debugging point

The last observed focused matrix run passed the 5.0, 5.3, and 5.4 source-output comparisons, then failed on Unity 5.6 at `LightingData.asset.meta`. Rust emits a `NativeFormatImporter` with a blank `name` and the standard fields; the frozen 5.6 output also has `mainObjectFileID: 112000000` after `name`. Inspect the source fixture versions to establish the exact layout boundary, then make the native importer version-aware. The 5.6 Scene YAML and LightingData YAML comparisons had passed before the sidecar mismatch.

The current tree has since passed `cargo check --workspace --locked` and the managed-probe build after the file splits. The source-family test has not been rerun after those structural changes; its last result remains the 5.6 importer metadata mismatch above. Resume with the focused source-family test after fixing that schema difference:

```powershell
cargo test --workspace --locked export_project_matches_source_lightmap_schema_families -- --nocapture
```

Inspect these locations first:

```powershell
rg -n 'apply_generated_lighting_data_pointer_overlay|generated_lighting_data_field_path' src/export/workspace_yaml.rs
```

Then review the call path around the overlay and the alias branch inside `apply_generated_lighting_data_pointer_overlay`. The resolver matches the generated field path exactly, so ensure the non-null legacy pointer is still present after editor-tree materialization and reaches the visitor.

After the 5.0 path is fixed, resolve any source-output differences from the new TextureImporter layouts. Re-run the source-family matrix and the original 2019.4 focused route. Keep unsupported layouts fail-closed. This slice covers only empty-scene, one-lightmap Image/BMP behavior; it still rejects GameObject/Transform/component candidates and non-null RenderSettings references.

## Validation record at stop

The full test and lint sets below were run before the LightingDataAsset version-family edits and remain stale. Current structural-refactor checks are recorded separately:

- `cargo fmt --all -- --check` — passed after the structural refactor.
- `cargo check --workspace --locked` — passed after the structural refactor.
- `cargo check --workspace --locked --tests` — passed; test targets compiled, but no test cases were executed.
- `dotnet build tools/probes/managed-project/ManagedProjectProbe.csproj --no-restore -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref"` — passed with zero warnings and zero errors.
- `tools/check-source-file-lengths.ps1` — passed; only the vendored `dotnetdll` file remains over 2,000 lines.
- `git diff --check` — passed; Git reported existing CRLF-to-LF normalization notices for documentation and the probe source.
- `cargo test --workspace --locked --quiet` — passed before current edits: 1,585 library tests, 1 ignored; 119 CLI tests; 1 doctest ignored.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed before current edits.
- `tools/verify-baseline.ps1` — passed before current edits; all 1,449 frozen reference files matched.
- Header-indexed parity report — 386 rows, 362 verified, 24 non-verified; Phase 1 OPEN. This increment closes no parity row; blocker count remains 24 before → 24 after.
- Latest source-family focused test before the structural refactor — failed at Unity 5.6 importer metadata as described above. No test suite was run for the structural split.

In this PowerShell environment, run the parity report as:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\parity-report.ps1
```

It returns exit code 1 while Phase 1 is open. Do not describe earlier green validations as current results after the code changes.

## Next actions

1. Compare the 5.4 and 5.6 `NativeFormatImporter` sidecars in the source fixtures and implement the evidenced `mainObjectFileID` version boundary.
2. Run `export_project_matches_source_lightmap_schema_families`; address any remaining source-output mismatches without broadening beyond evidenced layouts.
3. Run the existing Unity 2019.4 focused route, then the relevant module/full validation set. Record actual results in `docs/validation-log.md`.
4. Review `docs/differential-findings.md`, `docs/feature-parity.md`, and `docs/phase1-closure-inventory.md`. Do not close broad parity rows from this bounded source behavior; report the blocker count accurately.
5. For a completed coherent implementation increment, review the entire diff, check live `origin/main`, then make a normal implementation commit and push if still appropriate. Do not create a documentation-only hand-off commit.
6. Continue from the highest gate-impact item in `docs/phase1-closure-inventory.md`.

No user VRChat bundle content or UABEA was accessed in this increment. Do not reset, clean, force-push, or modify the frozen AssetRipper tree. Unity import/runtime validation remains unavailable without a valid Editor license.

## Stop status

Phase 1 remains OPEN and Phase 2 remains LOCKED. The existing 2019.4 bounded route had a passing source comparison before this expansion, but the current version-family work is unverified and the last observed matrix run failed at Unity 5.6 importer metadata. The source-size refactor compiles but its test suites were not run. This is a continuation note, not a completion report.
