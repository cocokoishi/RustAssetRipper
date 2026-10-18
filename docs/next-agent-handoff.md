# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest code revision: `bcf8f839317fd4dd6a5e5ac37bf8cb806592e068`.
- GitHub Actions run [39142272661](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39142272661) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Stored Material EngineResourceData match and miss now drive source-matched Web project redirects and output trees; see `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-MATERIAL-001`.
- The ledger has 409 rows: 381 verified and 28 non-verified. Phase 1 remains open. `PRODUCT-CONFIG-001` stays `已实现但未验证`; other stored resource kinds, generic DataSet serializers, and file-dialog input remain open.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Implement stored Shader matching in EngineResourceData. Frozen `PredefinedAssetCache.ShaderKey` is `(Name, PropertyNamesHash)`; cache insertion hashes ordered `Shader.PropertyNames`, while lookup hashes parsed property names or zero when no parsed form exists. Rust currently rejects stored Shader entries. Trace the source Shader JSON shape and hash/match semantics, then add a public source-generated fixture and compare Web redirect and full output against the frozen source. Keep `PRODUCT-CONFIG-001` open.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. Other external blockers remain individually marked in the ledger.
