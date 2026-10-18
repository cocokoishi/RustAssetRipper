# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest product-code revision: `296ff2bc1b5690278a275b1a330364573fc4a949`.
- GitHub Actions run [`39157239238`](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39157239238) passed all eight jobs and uploaded Windows/Linux release artifacts.
- The CRN header probe now reports the six original fixtures as 512x512, 10 levels, one face. A generated public 8x8 DXT1 CRN cubemap adds a bounded three-level, six-face conversion case. The frozen classic decoder returns 288 DXT bytes and UnityCrunch throws on this pre-2017.3 input; Rust matches all six top-level face RGBA hashes against the Crunch `-split` output. Details: `DIFF-RES-TEXTURE-CRUNCH-MULTIFACE-001`.
- The ledger remains 409 rows: 381 verified and 28 non-verified. `RES-TEXTURE-CRUNCH-001` remains unverified for broader codec/version/product coverage.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Advance `OBJ-REF-001` by reproducing one unresolved or ambiguous external dependency case in a nested bundle. The live ledger still lists unresolved/ambiguous dependencies and the full bundle hierarchy matrix as required. Compare AssetRipper and Rust by exact collection/file identity and resolved PPtr target, then exercise the corresponding export mapping if the fixture supports it. Do not infer parity from output counts or filenames.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. The UABEA checkout has no prebuilt executable, so UABEA runtime comparison was unavailable for the recent Texture2D YAML slices. Other external blockers remain individually marked in the ledger.
