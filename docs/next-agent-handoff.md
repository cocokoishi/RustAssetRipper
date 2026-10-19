# RustAssetRipper hand-off - 2026-10-19

## Latest verified checkpoint

- Latest validated product-code revision: `4dd889f0b67793e78c2037752c3929af53390990`. Actions run [39170870140](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39170870140) passed all eight jobs and uploaded Windows/Linux release artifacts.
- `DIFF-OBJ-REF-NESTED-SEPARATE-RESOLVED-ARRAY-001` verifies ordered managed-reference array PPtrs through external indices 1 and 2 to PathIDs 900 and 901 in separate nested SerializedFiles. The Windows regression and exact-run artifact replay passed; the root plus both target YAMLs matched the frozen-source goldens after only generated-GUID and line-ending/trailing-whitespace normalization.
- Privacy cleanup revision `8c765ea0230eaed90c3702ba3bebc80409e21ba8` anonymized the three tracked acceptance documents; the local public-repository guard passed and Actions run [39179092843](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39179092843) passed all eight jobs.
- Ledger: 409 rows, 381 verified, 28 non-verified (7 in progress, 2 externally blocked, 19 implemented but unverified). `OBJ-REF-001` remains in progress; no row was closed by this slice.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Add source-matched `GameObject` lookup from `EngineResourceData`. Frozen `PredefinedAssetCache.TryAdd/Contains(GameObject)` keys by name, layer, and component count. Rust `EngineResourceData::parse` has no `GameObject` entry/key and currently falls back when a stored resource uses that type. Build a public synthetic settings/object fixture, compare the redirect through the frozen exporter, then implement and verify the Rust `EGUID` target mapping. Relevant source: `Source/AssetRipper.Export.UnityProjects/EngineAssets/PredefinedAssetCache.cs`; Rust: `src/export/project_workflow/engine_resource_data.rs`, `src/export/project_workflow/object_routing.rs`. Keep `OBJ-REF-001` open.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. UABEA source and classdata are present, but no prebuilt executable is available. Other external blockers remain individually marked in the ledger.
