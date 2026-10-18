# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest code revision: `538d383566cb73808598c9f08f32ce0206a34126`.
- GitHub Actions run [39143737507](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39143737507) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Stored Material and Shader EngineResourceData entries now drive source-matched Web project redirects; see `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-MATERIAL-001` and `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-SHADER-001`.
- The ledger has 409 rows: 381 verified and 28 non-verified. Phase 1 remains open. `PRODUCT-CONFIG-001` stays `已实现但未验证`; other stored resource kinds, generic DataSet serializers, and file-dialog input remain open.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Implement stored Sprite matching in EngineResourceData. Frozen `PredefinedAssetCache.SpriteKey` is `(Name, TextureName)`; `EngineAssetsFactory.Create(ISprite)` resolves the texture pointer and declines to create a cache entry if a non-null pointer is unresolved. Rust already has a Sprite view and embedded-cache pointer resolution. Add stored Sprite parsing/routing and a public source-generated Sprite plus resolvable Texture fixture, then compare match/miss Web exports with the frozen source. Keep `PRODUCT-CONFIG-001` open.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. Other external blockers remain individually marked in the ledger.
