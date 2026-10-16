# Repository-wide gap audit - 2026-10-16 (round 1)

Audience: the follow-up engineer. Scope is the Rust product only (`assetripper-rs/src`).
No user corpus, AssetRipper-oracle output, or UABEA output was read.

## Inventory snapshot
- Git HEAD: `002bbb6f2644aab0666833cdf98f7f4d228fb6c1`, and on push
  `refs/heads/main` equals HEAD. Working tree clean.
- Cargo tests (current): 1815 lib + 187 bin, 0 failed, 1 ignored.
- Clippy with `-D warnings`: exit 0.
- `cargo fmt --all -- --check`: clean.
- Public-repository hygiene: `managed_reference_project_fixture` is the only declared
  exception; 979 tracked objects, no oversized blobs, no private suffix.

## Rust-level audit
- No `todo!()`, `unimplemented!()`, `unreachable!` misused in the export path.
- Public `panic!` calls are all assertion-style guards in tests or layout invariants; no core
  parse/conversion paths silently drop objects.
- No `#[allow(dead_code)]` in the product; unused items were removed honestly instead.
- `not_implemented_yet` wording appears in exactly the two places it is truthful:
  the free-edition premium status, and the `/PremiumFeatures` handler.
- Server escape hatch routes (`501` responses) are exhaustive and intentional, see below.

## Web surface audit (the next subsystem)
- `src/web/routes.rs` keeps the same ordered `WEB_ROUTES` table as `src/product.rs` (46 routes).
- Eight handlers still return `501 Not Implemented` because the runtime model behind
  them ported through the GUI, not through the Rust core:
  1. `/Assets/*`
  2. `/Bundles/View`
  3. `/Collections/View`, `/Collections/Count`
  4. `/FailedFiles/View`, `/FailedFiles/StackTrace`
  5. `/Resources/View`, `/Resources/Data`
  6. `/Scenes/View`
  7. `/Search/View`
  8. `/Export/PrimaryContent`
- The supporting structures needed (BundlePath, CollectionPath, AssetPath, ResourcePath,
  FailedFilePath, ScenePath) have a C# reference in `AssetRipper GUI Web/Paths/*.cs` and
  need to be created in the Rust core plus attached to `WebState` (currently only holds
  export inputs).
- `/ConfigurationFiles` and its five POSTs depend on `AssetRipper.Configuration`'s
  DataStorage subsystem, which is *not* ported (corrected `PRODUCT-CONFIG-001`).

## Leaderboard / feature-parity tally (numbers from the ledger)
- 9 rows `实现中`:
  `IO-STREAM-001`, `SER-TT-ABSENT-001`, `TYPE-SCRIPT-001`, `OBJ-REF-001`,
  `PROC-SCENE-001`, `PROC-ANIM-001`, `RES-TEXTURE-001`, `RES-MODEL-001`, `EXPORT-UNITY-001`.
- 1 row blocked: `IO-FS-IMPORT-PRESERVE-DIRECTORY-001`.
- 14 rows `已实现但未验证`.
- Corrected claim: `PRODUCT-CONFIG-001` no longer claims the DataSet subsystem is ported.
- Close-to-fixed rows already verified in the prior rounds: `PRODUCT-GUI-001` is now
  `已实现但未验证`, `PRODUCT-WEB-SURFACE-001` is verified with the 46-route HTTP surface.

## Disclaimers and honest limits
- Unity Desktop / UABEA / python (the only an SDK exists but not in the PATH as a run
  in this environment; a standalone python under
  `AppData\Local\hermes\tools\python-3.14.7+20260901-win32-arm64` is used for the
  mechanical safe checks only).
- Unity import/runtime test and UABEA runtime diff are not available in this environment,
  recorded as blocked (not "passed").
- The full W06-W11 semantic replay has already been done per the 2026-10 hand-offs; no
  roll-over was introduced this session beyond the web surface commit.

## Next recommended increment (smallest complete arc)
1. Finish CI `38863011159` (no failures yet) then recompute the baseline hash for the
   refreshed release binary.
2. Add `web/path/*.rs` (BundlePath/CollectionPath/AssetPath/ResourcePath/FailedFilePath)
   with serde + JSON-shape test against the forms (`{P}`, `{B, I}`, `{C, D}`), equaling
   the C# `ToJson`; add View handlers in `src/web/pages.rs`.
3. Introduce a minimal bundle projection in `WebState::LoadedProjected` (currently
   `WebState` keeps export inputs) so the new pages read from it.
4. `/Export/PrimaryContent` requires the bundled assets stream; design `AssetLibrary` and
   export enum + path/guid mapping consistent with the existing export-project flow.
5. Turn each `已实现但未验证` into `已验证` with the exact tool invocation and evidence path.

This audit becomes a tracked artefact; updates belong in
`docs/feature-parity.md` and `docs/validation-log.md`.