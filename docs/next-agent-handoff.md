# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest product-code revision: 640c3975a8a1e2d3338a3cc9f1eba3a93b3e4c74.
- GitHub Actions run [`39153515515`](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39153515515) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Unity 5.6 and 2019.4 class-28 Texture2D Web YAML each match the frozen Release GUI byte-for-byte on one public inline-RGBA32 fixture. For the 2019.4 fixture, both output trees contain the same nine relative paths; the YAML is 980 bytes and matches the source golden without normalization.
- The ledger remains 409 rows: 381 verified and 28 non-verified. RES-TEXTURE-001 remains unverified for broader texture coverage; RES-MODEL-001 remains open for broader Mesh/model behavior.
- Preserve the existing untracked tools/__pycache__/ directory.

## Primary next gap

Verify the complete class-28 Texture2D YAML path for Unity 2022.3 through the Web project-export workflow, using src/export/lightmap_project_source_2022_3_fixture.txt. Compare a fresh frozen Release GUI export and CI-built Rust artifact against the full .texture2D document and output path set. Add a fix only if the comparison shows a reproducible mismatch; leave RES-TEXTURE-001 unverified until the broader required scope is covered.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. The UABEA checkout has no prebuilt executable, so UABEA runtime comparison was unavailable for this slice. Other external blockers remain individually marked in the ledger.
