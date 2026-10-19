# Completion audit — 2026-10-12

## Current follow-up — 2026-10-13 (GOAL §25 remains NOT COMPLETE)

- At the start of this audit update, branch main was at a25c2dd, pushed to origin; that commit contains only the documentation audit/handoff update. This follow-up also changes documentation only. The Ogg-to-WAV implementation remains uncommitted and unverified end to end. GOAL.TXT, cleanup_manifest.txt, tools/probes/*/packages/, and the historical private-world-006 completion audit remain protected from staging.
- The former PreferWav Ogg rejection was replaced by a bounded Ogg/Vorbis-to-32-bit-float-WAV converter wired to export-audio and the Unity 2022.3 project binary route. Two focused library tests and the full locked library suite (1,710 passed, 1 ignored) passed; warnings-denied Clippy passed. The corrected CLI integration test did not execute because linking failed with LNK1318: PDB LIMIT (12). Release build, fresh project replay, semantic audit, reference scan, and monotonicity replay remain NOT RUN. The compatible NAudio reference probe does not prove the frozen app ran successfully: its frozen NAudio package combination raised TypeLoadException.
- The latest machine parity report remains 389/389 recognized, 366 verified, 23 non-verified, zero malformed rows or duplicate IDs, and Phase 1 OPEN. The report is target/ogg-wav-parity-report-20261013.json. No parity row was closed by the Ogg change.
- A bounded GOAL §25 source sweep found no executable todo! or unimplemented! macro in src. The only TODO match is the sprite module's quotation of the baseline Edges warning; Sprite metadata mesh-vertex extraction remains explicitly open. The one ignored Rust test only emits Scene reference-probe inputs. The runtime process search found no child-process call sites in src; this does not replace a complete runtime dependency audit.
- The Texture3D unknown-format RGBA32 fallback found during the sweep matches the frozen Texture3DExtensions.GetTextureFormat catch/fallback. It is not a newly discovered Rust/reference discrepancy; Unity behavior for unknown serialized formats remains unverified.
- A separate source-to-export trace found the non-Yaml SpriteExportMode gap: frozen TextureExportCollection adds SpriteMetaData and ID/name mapping for Native/Texture2D, while Rust's object route passes false for export_sprites, its decision applier writes only scalar fields, and every version template leaves spriteSheet.sprites empty. Default-Yaml private-world-006 evidence does not cover this mode. The finding is DIFF-SPRITE-NON-YAML-IMPORTER-001; no Rust export/test was run, so the emitted difference still needs a controlled differential.
- A further static GOAL §25 trace found a possible mesh merge loss: frozen VertexDataBlob.ReadData can return false for absent positions while still filling other channel outputs; MeshExtensions.ReadData ignores that boolean and merges compressed channels before MeshData.TryMakeFromMesh checks vertices. Rust returns None for the entire blob unless channel 0 is set, so a compressed-position plus non-position-vertex-data case may lose those channels. DIFF-MESH-PARTIAL-CHANNELS-001 records this as a candidate only; no serialized fixture, source run, Rust regression, or export was executed.
- The required browser UI is not implemented. The frozen WebApplicationLauncher registers 46 actual GET/POST routes, while `src/product.rs` records route/default/command facts and some pure helpers but provides no handlers; `src/main.rs` has no server command, and no Rust listener primitive or front-end assets were found. PRODUCT-GUI-001 remains `已调查` with all browser workflows open. This was a static route/source search only; no browser or runtime test ran.
- Latest free-space check: 17,997,824 bytes (17.16 MiB); only C: is available as a filesystem drive. The user requested temporary-file cleanup. Preflight identified only generated Ogg probe build outputs and the configured test scratch tree (29,986,363 bytes total), all inside target with no reparse points or audit-named JSON and no active Cargo, Rustc, or dotnet process. The recursive deletion command was rejected by environment policy before PowerShell execution; no files were removed and no alternate deletion route was attempted.
- Next work: resume the corrected CLI test and required release/replay/audit checks if storage and policy permit; then begin the actual Rust browser runtime required by PRODUCT-GUI-001, followed by the Native/Texture2D Sprite route and remaining non-verified rows. Prove the Mesh partial-channel candidate before changing it. Unity Editor and UABEA remain NOT RUN; no Unity runtime claim is made.

Starting HEAD: `6c45a40136cb0a0e1274cbf69c9fd6c50cf6f2de`. GOAL §25 conclusion: **NOT COMPLETE**.

## Latest checkpoint — PrefabImporter fix pushed

Tooling changes are committed/pushed as387e107. Product class86/121 editor-layout and generated-only native-name corrections are committed/pushed as `ff865c2` with monotonicity replays (private-name-015 r15, private-name-017 r8, private-world-005 r7, private-name-018 r26) plus first private-world-006 three-way audit (r8: class121 paired, class124 exact). The PrefabImporter version/bundle-name defect is fixed and pushed as `dc959fe` (private-name-016 r7 + private-name-015 r16 prefab metas match source; all prefab-bearing samples replayed). `Packages/manifest.json` is byte-exact since the CRLF fix pushed as `39b5367` (private-name-016 r8). Goal was resumed by the user after the pause; no product task remains running. Detailed commands/results/resume order are in latest validation-log and next-agent-handoff. Tables below retain their earlier bounded audit scopes, not a final acceptance claim.

## Findings affecting the next action

- The frozen Web API successfully exported private-world-006/private-name-016. Missing CLI was a local tooling limitation; `tools/export-reference-web.ps1` resolves it without changing source or adding Rust runtime dependencies.
- Old reference validation missed every local `{fileID: N}`, overwrote duplicate GUIDs, and returned 0 on dangling references. Reproduced missing local ID 777 as `checked=0 dangling=0`, exit 0. New checker validates scope/IDs/owners, saves all issues, and distinguishes imported/builtin targets. Thirteen contracts pass; `-RequireComplete` exits 2 on partial validation.
- Old document audit omitted `.meta`, ProjectSettings and Packages. Different importer mainObjectFileID and EditorSettings falsely produced no differences. New audit includes these, counts unpaired YAML, rejects duplicate fileIDs and supplies `-RequireEquivalent` (exit 1 on differences). Nine contracts pass. Only GUID-owner paths and numeric root `timeCreated` are normalized; frozen `Meta.cs::CurrentTick` proves the latter is an export timestamp. No importer fields, field presence, ordered arrays or opaque bytes are ignored.
- Fresh oracle comparison reveals functional gaps: private-name-016 class86 CustomRenderTexture editor fields/hash, private-world-006 class121 Flare editor fields, private-world-006 class124 root name Behaviour instead of FlareLayer. TPK and source YamlWalker explain these generically. Fixes are in progress, invalidating the earlier blanket claim that all residuals were acceptable.
- `parity-report.ps1` exits 1: 389 rows, 366 verified within stated scopes, 12 implemented/unverified, 9 in progress, 1 investigated, 1 blocked; full gate OPEN.
- Original baseline-directory strict check exits 1: 643 unlisted build outputs, zero changed/missing manifest files. Preserved them; no destructive cleanup. Isolated reference copy matches all 1449 manifest source files. These are different checks; neither result substitutes for the other.

## Broad sweep

| Required audit area | Evidence / outstanding work |
|---|---|
| TODO/placeholders | No executable todo!/unimplemented! in src; resources/sprite.rs contains a source TODO quotation. Absence of these macros is not implementation coverage. |
| Unsupported families | workspace_yaml/object_conversion.rs rejects non-null SpriteAtlas; project_workflow/workspace_identity.rs rejects Terrain lighting; planning.rs rejects PreferWav Ogg-to-WAV. Explicit errors remain product debt. |
| Fallbacks/silent loss | Raw/unreadable/script fallback routes still need complete registration/source tracing. Loud errors above do not establish an absence of other loss paths. |
| Version/platform branches | Named editor-layout whitelist limited to Unity 5.6/2022.3; predefined engine Material/Shader tables cover two bands. Other versions/classes remain open. |
| References/resource paths | New local/external YAML existence checks pass inspected outputs. Imported/builtin IDs, SpriteAtlas resources, missing externals and broader processed-asset mappings require additional proof. |
| Tests | Baseline lib 1698 passed / 1 ignored. Ignored scene probe writes fixture inputs (scene_wiring_tests_01.rs:1203). New tool suites 13+9 pass. Product corrections require fresh whole suite and replays. |
| Formatting/lints | Initial Clippy exit 0 included three vendored lifetime warnings; explicit return lifetimes remove those warnings. Full format check exits 1 on existing drift, preserved. |
| Comparison exclusions | ID-family mismatches remain visible. Metadata/project files newly covered; opaque pixel/packet equivalence must be established for new inputs, not inherited from another sample. GUID owner mapping is not arbitrary subasset ID proof. |
| Samples/families | Nested sample directory has 16 common-extension bundle files plus root inputs; no all-input frozen manifest/acceptance exists. Signature discovery also remains necessary. Humanoid/Root Motion/runtime, atlas sprites, Terrain, script/settings and broader platform/version families remain unclosed. |
| Non-Rust dependencies | Reviewed shipping entry/core paths contain no reference/.NET subprocess calls. Web export/C# probes are development oracles; broader dependency/license audit remains in ledger. |
| External lanes | Unity import/runtime and UABEA not run this increment. Historical Unity license refusal remains separate from static evidence. |

## Acceptance A–O

| Requirement | Current conclusion |
|---|---|
| A Rust build | Locked baseline release succeeds; each code increment must rebuild before replay. |
| B no runtime delegation | Inspected paths independent; full route/dependency audit still open. |
| C real workflows | Incomplete: selected inputs and explicit unsupported families. |
| D no silent loss | Not proven broadly; fallback/source tracing remains. |
| E references | Partial: inspected YAML existence checks clear; imported/builtin identities unresolved. |
| F hierarchy | Bounded prior proof; new controller identity families/runtime require validation. |
| G resource semantics | New class86/121/124 gaps and 38 private-name-016 material pointers need closure. |
| H importer/meta | Newly covered; complete importer/subasset/Unity import proof pending. |
| I differences resolved | False: new oracle results expose functional differences. |
| J required suites | Baseline/tool suites pass; new code suites pending. Format/baseline-directory checks fail honestly. |
| K monotonicity | Prior binary evidence exists; current changes need fresh checks. |
| L limits stated | Unity and static/reference evidence distinguished. |
| M truthful docs | Current audit corrects stale blockers; historical sections do not describe current acceptance. |
| N no sample patches | Fixes follow pointer scope, TPK version/class layouts and native class names. |
| O architecture | Existing generic conversion infrastructure used; broad audit remains open. |

Exact commands, hashes, exits and artifact paths are in the newest validation-log section. Goal remains active.
