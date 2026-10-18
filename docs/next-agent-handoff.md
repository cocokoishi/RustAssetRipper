# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest product-code revision: f4b246d9d411916e581cbaefa2daaf218dc2b6cf.
- GitHub Actions run [`39155137580`](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39155137580) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Unity 5.6, 2019.4, and 2022.3 class-28 Texture2D Web YAML each match the frozen Release GUI byte-for-byte on a public inline-RGBA32 fixture. For the 2022.3 fixture, both output trees contain the same nine relative paths; the YAML is 1106 bytes and matches the source golden without normalization.
- The ledger remains 409 rows: 381 verified and 28 non-verified. RES-TEXTURE-001 and RES-TEXTURE-CRUNCH-001 remain unverified for their broader format/variant scope; RES-MODEL-001 also remains open.
- Preserve the existing untracked tools/__pycache__/ directory.

## Primary next gap

Advance RES-TEXTURE-CRUNCH-001 by checking a public multi-face or multi-mip CRN case against the frozen AssetRipper runtime decoders and the independent pixel oracle. The current six attributed CRN fixtures exercise decoder dispatch and branch failures, but do not cover multi-face/multi-mip payload semantics. Add a focused fixture/regression only after reproducing a concrete input and compare every decoded face/mip; keep the row unverified until its remaining scope is covered.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. The UABEA checkout has no prebuilt executable, so UABEA runtime comparison was unavailable for these YAML slices. Other external blockers remain individually marked in the ledger.
