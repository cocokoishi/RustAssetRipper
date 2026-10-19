# RustAssetRipper hand-off — 2026-10-18

## Latest validated checkpoint
- HEAD 24e8f29 is pushed to main; GitHub Actions run 39191177807 passed all eight jobs.
- Current parity report: 410 total, 381 verified, 29 non-verified, 0 duplicates, 0 malformed, gate OPEN.
- Preserve the pre-existing untracked tools/__pycache__/ directory.

## Gap just closed
- Added frozen-source Unity 2022.3 no-TypeTree TPK fallback cases for class 194 NavMesh and class 129 PlayerSettings.
- factory_differential_replays_every_reference_case verifies source-authored fallback rows, consumed lengths, selected values, and empty tails. The feature ledger remains in progress.

## Primary next gap
- EXP-ENGINE-PREDEFINED-GAMEOBJECT-001: source-differentiate a successful stored GameObject cache redirect when the object is not already claimed by a Scene or Prefab collection. Existing evidence covers the Prefab collision branch only. Start from the public synthetic Unity 2017.4 fixture, vary its input route to keep the GameObject outside Scene/Prefab membership, and compare exported collection and pointer semantics in frozen AssetRipper and Rust.

## Environment limitation
- Unity Editor import/runtime still lacks a valid local license; no prebuilt UABEA executable is available. Neither blocks the next source differential.