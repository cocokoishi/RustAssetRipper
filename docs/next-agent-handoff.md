# RustAssetRipper hand-off - 2026-10-18

## Latest validated checkpoint
- HEAD `87e02c8` is pushed to `main`; GitHub Actions run `39188787066` passed all jobs on Windows and Linux.
- Current parity report: 410 total, 381 verified, 29 non-verified, 0 duplicates, 0 malformed, gate OPEN.
- Preserve the pre-existing untracked `tools/__pycache__/` directory.

## Gap just closed
- Added frozen-source Unity 2022.3 no-TypeTree TextAsset coverage with class-49 release/editor TPK rows and exact decoded `m_Name` / `m_Script` assertions.
- The focused factory differential passed locally and on Actions. `SER-ASSET-FACTORY-001` remains implemented but not verified.

## Primary next gap
- `SER-TT-ABSENT-001`: add a frozen 2022.3 differential for an unresolved no-TypeTree class and compare the source fallback outcome with Rust.

## External limitation
- Unity Editor import/runtime validation still lacks a valid local license, and no prebuilt UABEA executable is available. Neither blocks the next synthetic differential; private samples remain local.
