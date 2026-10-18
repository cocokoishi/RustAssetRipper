# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest code-bearing revision: `4843b9540a06e1a0e0ae99c9219c6387020e42a9`; Actions run `39105588611` passed all eight jobs. The current evidence sync is documentation-only.
- Slice closed: source-probed Patch retry failures now cover Light (class 108), GraphicsSettings (class 30), and Font (class 128) through the factory differential and ObjectWorkspace. Rust preserves each initial context and exact raw payload; its truncated-input error differs from the source .NET exception details.
- Actions tests: Ubuntu 1,941 library + 215 CLI; Windows 1,943 + 215; Ubuntu release 1,941 + 215. Clippy, formatting, ledger audit, and privacy jobs passed.
- Parity report: 409 rows, 380 verified, 29 non-verified, no duplicate IDs or malformed rows. Phase 1 remains open; `SER-TT-ABSENT-001` remains in progress.
- No private input or output was used.

## Primary next gap

Build a source-authored class-ID-28 Texture2D fixture with non-default serialized fields and streamed resource data; compare decoded field values and exact resource bytes through Rust ObjectWorkspace against AssetRipper, with UABEA as a structural cross-check where applicable. Keep `SER-TT-ABSENT-001` and `RES-TEXTURE-001` open until broader behavior is verified.

## Operating constraints

Stay on main. Use Actions for full Cargo validation. Before each push, run `tools/check_public_repository.py` and inspect `git diff --name-only origin/main...HEAD`. Keep private inputs local.
