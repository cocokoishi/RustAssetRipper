# RustAssetRipper hand-off - 2026-10-18

## Latest validated checkpoint

- Latest validated product-code commit: `096f1a40c49f9105a9ad3aadc450eb0cd5092cc6`. Actions run [39181385497](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39181385497) passed all eight jobs, including Windows/Linux workspace tests and release builds, Ubuntu release-mode tests, Clippy, formatting, ledger, and repository hygiene.
- `EXP-ENGINE-PREDEFINED-GAMEOBJECT-001` records one source-matched case: frozen AssetRipper fails on duplicate collection registration when a stored GameObject cache match is already a processed Prefab member. Rust returns the same failure category; Scene overlap and successful redirection remain open.
- Ledger: 410 rows, 381 verified, 29 non-verified, duplicates=0, malformed=0, gate=OPEN. Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Continue `OBJ-REF-001`. The public differential covers ordered nested managed-reference arrays resolving external file IDs 1 and 2 into separate SerializedFiles. Find the next highest-impact uncovered PPtr/dependency branch with a frozen-runtime fixture; keep the broader row open until external, bundle, built-in, cycle, and export-identity behavior is covered.

## External limitation

Unity Editor import/runtime validation remains unavailable because this workstation has no valid Editor license. No prebuilt UABEA executable is available; its source/classdata remain read-only references.
