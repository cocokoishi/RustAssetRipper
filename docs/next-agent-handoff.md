# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest product-code revision: `746655073e141ddc1197a274987e1d53a07f63be`.
- GitHub Actions run [39151747117](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39151747117) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Unity 5.6 class-28 Texture2D Web YAML now matches the frozen Release GUI byte-for-byte on one public inline-RGBA32 fixture. Both output trees contain the same 12 relative paths; the Texture2D YAML is 729 bytes. The source, Rust CI artifact, and checked-in golden agree without normalization.
- The ledger remains 409 rows: 381 verified and 28 non-verified. `RES-TEXTURE-001` remains `已实现但未验证` for wider texture coverage; `RES-MODEL-001` remains open for broader Mesh/model behavior.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Verify the complete class-28 Texture2D YAML path for Unity 2019.4 through the Web project-export workflow, using `src/export/texture_project_source_2019_4_fixture.txt`. Existing `DIFF-TEXTURE-PROJECT-001` evidence compares image bytes and importer output, not the full `.texture2D` YAML document. Load the public fixture into the frozen Release GUI and a CI-built Rust artifact with fresh roots, compare the path set and full serialized asset, then add a narrowly scoped editor-TypeTree fix only if the comparison shows a real mismatch. This is the next `RES-TEXTURE-001` verification slice; do not infer a mismatch from the unverified status.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. Other external blockers remain individually marked in the ledger.
