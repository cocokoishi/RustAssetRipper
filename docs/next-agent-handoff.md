# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest code revision: `ac14e3e38ea53aa274111327472c4cf19bbfa96c`.
- GitHub Actions run [39140537593](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39140537593) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Stored AudioClip and Cubemap EngineResourceData matches now drive source-matched Web project redirects. Their CI-built Windows artifacts produced the same output trees and byte-identical files as the frozen Release GUI; see `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-AUDIOCLIP-001` and `DIFF-PRODUCT-CONFIG-ENGINE-RESOURCE-CUBEMAP-001`.
- The live parity ledger has 409 rows, 381 verified, and 28 non-verified. Phase 1 remains open. `PRODUCT-CONFIG-001` stays `已实现但未验证` because other resource kinds, generic DataSet serializers, and file-dialog input remain open.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Implement stored Material matching in EngineResourceData. Frozen `PredefinedAssetCache.MaterialKey` is `(Name, Shader?.Name)`. Rust currently rejects stored Material entries; the existing Material view and shader resolver are used only by embedded engine-cache matching. Build a public source-generated Material input with a resolvable Shader pointer, then compare the Web cache redirect, path-map identity, and full output tree with the frozen source. Keep `PRODUCT-CONFIG-001` open.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. Other external blockers remain individually marked in the ledger.
