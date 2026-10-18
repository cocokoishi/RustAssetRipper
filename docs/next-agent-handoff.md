# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest code revision: `1cc8107f8ff476565795f2d31cbb798a938522a6`.
- GitHub Actions run [39145881118](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39145881118) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Stored Material, Shader, and Mesh EngineResourceData entries now drive bounded source-matched Web project redirects; see `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-MATERIAL-001`, `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-SHADER-001`, and `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-MESH-001`.
- The ledger has 409 rows: 381 verified and 28 non-verified. Phase 1 remains open. `PRODUCT-CONFIG-001` stays `已实现但未验证`; other stored resource kinds, generic DataSet serializers, and file-dialog input remain open.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Implement stored Sprite matching in EngineResourceData. Frozen `EngineAssetsExporter.TryCreateCollection` checks the SpriteInformationObject's texture cache entry before checking its `(Name, TextureName)` Sprite key; `EngineAssetsFactory.Create(ISprite)` declines to create an entry if a non-null texture pointer is unresolved. Rust already resolves the embedded Sprite texture key. Add stored Sprite parsing/routing and a public source-generated Sprite plus resolvable Texture fixture, then compare match/miss Web exports with the frozen source. Keep `PRODUCT-CONFIG-001` open. The stored Mesh redirect is verified, while normal Mesh/Texture2D YAML field differences remain open under `RES-MODEL-001` and `RES-TEXTURE-001`.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. Other external blockers remain individually marked in the ledger.
