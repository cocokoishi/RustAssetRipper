# Completion audit — 2026-10-12

Starting HEAD: `6c45a40136cb0a0e1274cbf69c9fd6c50cf6f2de`. GOAL §25 conclusion: **NOT COMPLETE**.

## Findings affecting the next action

- The frozen Web API successfully exported 玉京烟霞/烟雨长亭. Missing CLI was a local tooling limitation; `tools/export-reference-web.ps1` resolves it without changing source or adding Rust runtime dependencies.
- Old reference validation missed every local `{fileID: N}`, overwrote duplicate GUIDs, and returned 0 on dangling references. Reproduced missing local ID 777 as `checked=0 dangling=0`, exit 0. New checker validates scope/IDs/owners, saves all issues, and distinguishes imported/builtin targets. Thirteen contracts pass; `-RequireComplete` exits 2 on partial validation.
- Old document audit omitted `.meta`, ProjectSettings and Packages. Different importer mainObjectFileID and EditorSettings falsely produced no differences. New audit includes these, counts unpaired YAML, rejects duplicate fileIDs and supplies `-RequireEquivalent` (exit 1 on differences). Nine contracts pass. Only GUID-owner paths and numeric root `timeCreated` are normalized; frozen `Meta.cs::CurrentTick` proves the latter is an export timestamp. No importer fields, field presence, ordered arrays or opaque bytes are ignored.
- Fresh oracle comparison reveals functional gaps: 烟雨长亭 class86 CustomRenderTexture editor fields/hash, 玉京烟霞 class121 Flare editor fields, 玉京烟霞 class124 root name Behaviour instead of FlareLayer. TPK and source YamlWalker explain these generically. Fixes are in progress, invalidating the earlier blanket claim that all residuals were acceptable.
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
| G resource semantics | New class86/121/124 gaps and 38 烟雨长亭 material pointers need closure. |
| H importer/meta | Newly covered; complete importer/subasset/Unity import proof pending. |
| I differences resolved | False: new oracle results expose functional differences. |
| J required suites | Baseline/tool suites pass; new code suites pending. Format/baseline-directory checks fail honestly. |
| K monotonicity | Prior binary evidence exists; current changes need fresh checks. |
| L limits stated | Unity and static/reference evidence distinguished. |
| M truthful docs | Current audit corrects stale blockers; historical sections do not describe current acceptance. |
| N no sample patches | Fixes follow pointer scope, TPK version/class layouts and native class names. |
| O architecture | Existing generic conversion infrastructure used; broad audit remains open. |

Exact commands, hashes, exits and artifact paths are in the newest validation-log section. Goal remains active.
