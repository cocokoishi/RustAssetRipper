# RustAssetRipper hand-off — 2026-10-10

## Latest current checkpoint — 2026-10-14 (universal headers fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r9: real-diff 15 to 13; zero unpaired; binaries clean. Next: field-level extras in order: 82/198 value checks, 91 extra lines, renderer GI sets (23/137/199), mesh/animator/light/constraint extras (43/95/108/1773428102), stamps (1102/1107), 90 flow rule.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (prefab headers fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r8: classes 1+4 exact; zero unpaired; binaries clean. Next: remaining paired-content diffs (15 classes).
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (material headers fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r7: materials resolved; zero unpaired; binaries clean. Next: paired-content diffs, starting with the 83 transforms.
- Local checks: `git diff --check` exit 0; cargo check/clippy + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (cubemap route fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r6: zero unpaired files; all binaries packet/pixel-clean. Next: paired-content diffs, starting with the 13 materials (then transforms/cascades).
- Local checks: `git diff --check` exit 0; cargo check/clippy + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (audio body fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r5: OGG sidecar matches; audio packet-clean. Next: the cubemap image route, then paired-content diffs (materials/transforms/cascades).
- Local checks: `git diff --check` exit 0; cargo check/clippy + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (importer stamp fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r4: PNG sidecars match, images/audio packet-clean. Next: 2018.4 AudioImporter body deltas (OGG sidecar), then paired-content diffs and the cubemap route.
- Local checks: `git diff --check` exit 0; cargo check/clippy + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (2018 routes land, bodies open)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r3: images packet/pixel-clean, audio packet-identical, unpaired gone. Next: 2018.4 importer-body deltas (25 sidecars), then paired-content diffs and the cubemap route.
- Local checks: `git diff --check` exit 0; cargo check/clippy + 13/13 module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (Unity-6 program scoped)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Avatar VRC-AV03 audited: Unity-6 texture/audio routes unported (systematic fallbacks + cascades) plus missing 6000-band redirects (FGUID x7). Next: generate 6000-band predefined tables via the probe (bounded), then scope the importer transcription.
- Local checks: `git diff --check` exit 0. No local Cargo run (no product change). Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (second avatar clean)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Avatar VRC-AV02 audited: only the known flow-style Avatar doc differs (values verified identical); PNGs pixel-identical; no product change. Next: first replay plus audit of the following sample in acceptance order.
- Local checks: `git diff --check` exit 0. No local Cargo run (no product change). Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (smallest world fully clean)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- World VRC-W01 re-audit: zero real diffs in any class, zero unpaired docs, no regressions. Next: first replay plus audit of the following sample in acceptance order.
- Local checks: `git diff --check` exit 0; cargo check + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (sprite redirects fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Sprite/texture engine redirects implemented with probe-generated bands; world re-audit clean except 4 GameObject static-flags diffs. Next blocker: mirror the frozen editor-format static rule (non-default batch info or non-empty subset indices marks the GameObject all-static).
- Local checks: `git diff --check` exit 0; cargo check + 11/11 module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (semantic audit, 1 diff left)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (audit JSON stays quarantined in gitignored scratch; only counts/class IDs in docs).
- Smallest-avatar semantic audit: zero unpaired docs, thousands exact, PNGs pixel-identical, one auxiliary known-acceptable. Next blocker: the single class-90 Avatar document difference (key in quarantined audit JSON).
- Local checks: `git diff --check` exit 0. Product validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (first Rust replay: parity)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample input read-only; outputs quarantined in gitignored scratch.
- First HEAD replay (smallest avatar) exits 0 with per-extension histogram identical to reference in every row; semantic content diff next (pair by stable identity). Open: CLI object count vs file objects (9780 vs 2753).
- Local checks: `git diff --check` exit 0. Product validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (HEAD binary ready)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them or outputs.
- HEAD release binary built locally in ~10min with no machine impact; gate replays unblocked. Next: first Rust export replay (smallest avatar) plus semantic diff vs its reference baseline.
- Local checks: `git diff --check` exit 0. Product validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (prior program found, build on)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. One corpus ID confirmed against the prior alias set; other cross-walks need owner reconciliation (hashes bridge). Pre-existing probable-original-names flagged for owner adjudication; no new exposures added.
- Prior sessions' deep acceptance program discovered in current docs (alias replays, audit toolchain, per-class findings); HEAD release build started in background on that precedent. Next: collect binary, gate replays, Rust-vs-ref diffs.
- Local checks: `git diff --check` exit 0. No local Cargo test run; validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (comparison discipline set)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. GUID tokens only; nothing committed from outputs.
- Reference re-export proves per-run GUID/timestamp regeneration (trees identical, all sidecars differ); ledger now states the pairing discipline. Next: Rust-side export comparison once a binary strategy exists (authorization-blocked; see ledger).
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (texture attribution rule)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. No samples opened for this static finding.
- `DIFF-TEXTURE-ATTRIBUTION-001`: default image outputs attribute to class-28 plus cubemaps (verified on 3 baselines); sprites stay in metadata by default; raw-texture routing open. Next: Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (redirects quantified)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. GUID-token census only; nothing committed from outputs.
- Reference-side redirect census confirms the diff rule quantitatively (99 engine + 2 band dangling references, zero unexplained). Next: Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (corpus fully baselined 22/22)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- All 22 samples have reference baselines with class-level notes; output-shape catalogue covers playable/flare/terrainlayer/sub-docs/raw-textures/fonts/physicMaterial. Next: Rust-side export comparison once a binary strategy exists (the remaining open leg alongside semantic diffs).
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (twenty baselines, big shader gap)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Twenty reference baselines captured; largest shader gap so far (72 vs 103) plus a terrainlayer shape. Next: remaining giant-sample baselines (73MB, 287MB) plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (eighteen baselines, overage rule)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Eighteen reference baselines captured; texture outputs can exceed class-28 counts (cubemap/sprite attribution needed in future diffs). Next: remaining large-sample baselines plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (sixteen baselines, mat gap)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Sixteen reference baselines captured; largest material gap so far (332 vs 343) reinforces per-asset (not count) diff assertions. Next: remaining large-sample baselines plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (fourteen baselines, scale OK)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Fourteen reference baselines captured; the 426K-object extreme exports cleanly with exact core counts; more engine classes identified statically. Next: remaining large-sample baselines plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (raw-type IDs identified)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. No samples opened for this static identification.
- Recurring large raw-type IDs are ordinary engine classes (Lighting/Parent/Position/Rotation constraints) per the dispatch fixture; observation closed. Next: remaining large-sample baselines plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (twelve baselines, recurring IDs)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Twelve reference baselines captured; unknown raw-type IDs recur across samples (static identification task queued); font/physicMaterial shapes noted. Next: remaining large-sample baselines plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (ten baselines, tag split)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Ten reference baselines captured; 6000 bundle tags systematically carry 2022 inner content; sub-document and raw-texture output shapes noted. Next: remaining large-sample baselines plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (redirect rule recorded)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. No samples opened for this static finding.
- `DIFF-ENGINE-REDIRECT-001`: engine-asset redirects explain file-count gaps on both sides; future diffs assert files plus redirect sets. Next: Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (seven baselines, skip pattern)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Seven reference baselines captured; shader shortfall recurs (content-dependent), materials near-exact. Next static task: read reference exporter skip rules; plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (five reference baselines)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Five reference baselines captured (structural counts; two material/shader count observations logged as non-verdicts). Next: more baselines plus Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (first reference baseline)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them (reference outputs quarantined in gitignored scratch).
- Reference driver builds clean; first baseline export (smallest avatar: 2120 files, class-mirroring counts) done, second (smallest world) running. Next: collect world baseline, then Rust-side export comparison once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (reference driver building)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them.
- Headless reference driver authored in gitignored scratch; restore green, Release build running in background. Next: collect build, first reference export, quarantine outputs locally.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (factory triage 637216/637216)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them.
- Factory construction clean corpus-wide (637216/637216, zero failures; caveats recorded). Next: reference-side runs, then export replay once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (full-corpus decode sweep)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them.
- Decode sweep clean corpus-wide (637216/637216, zero failures; caveats recorded). Next: reference-side runs, then export replay once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (decode triage 4067/4067)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them.
- Per-object decode triage clean on all four inventoried samples (4067/4067, zero failures; caveats recorded). Next: reference-side runs, then export replay once a binary strategy exists.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (first real triage, zero errors)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public. Sample inputs read-only; nothing committed from them.
- Inventoried VRC-AV01/W01/AV03/W02 via rlib-linked probe (triage-grade): containers, versions, class histograms, resource nodes all parse with zero errors; one unknown raw type noted as an observation. Next: object-decode triage on VRC-AV01, then reference-side runs.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (acceptance corpus registered)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are now the top acceptance corpus per user direction; synthetic only for minimization/regression. Strict anonymity in all public surfaces (IDs + truncated hashes only; samples never committed).
- Registered `docs/acceptance-corpus.md` (22 samples, container facts, TPK coverage OK) and recorded the pending FSB5 CI success. Next: Rust-side structural triage from smallest sample upward; binary procurement still open.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (FSB5 export-project integration)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Added an `export-project` integration test for the 2022.3-gated audio binary route (version-parameterized fixture helper, silent FSB5 bytes, Ogg + sidecar asserts). Single-file rustfmt clean in the new regions.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (CRN export-project integration)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Added an `export-project` integration test for the CRN fixture with layout-agnostic output-tree asserts (PNG envelope + importer sidecar). Single-file rustfmt clean in the new region.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (parity accounting refresh)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Updated four feature-parity rows whose evidence went stale after the texture/audio integration arc (no status inflation; open items narrowed precisely).
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (FSB5 export-audio integration)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Added an FSB5 `export-audio` integration test reusing `make_audio_clip_file` (no new fixture): Default asserts a 5-packet Ogg remux, PreferWav asserts the 128-frame silent WAV. Single-file rustfmt clean in the new region.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (CRN export-texture integration)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Shipped the scoped integration at `export-texture` level: committed hand-built fixture (`tests/fixtures/crn-texture-512.assets`, recipe in the test comment) plus an in-process test asserting PNG envelope/dims/color-type. Fixture shape pre-verified locally via rlib-linked probe (parse/decode/oracle hash); export half goes through CI.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (CRN integration scoped)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Scoped (not started): CRN texture through `export-project` to PNG using the `main_tests_04` in-process harness with a generated class-28 payload carrying the committed CRN bytes; asserts PNG dims/pixels/importer. Open questions on generated field set and admission recorded in the validation log. Estimated multi-turn.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (meta timestamp edges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Pinned `.meta` timeCreated acceptance at both DateTimeOffset edges (inclusive) plus one-second-outside rejection; bounds verified by arithmetic.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (CRN guard sweep complete)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Extended the guard test with level-count surgery: all six production guard arms now pinned. Remaining CRN reality gaps unchanged (multi-face/mip fixtures, writers/CLI, Unity).
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (robustness sweep consolidation)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Verified the remaining pre-decode surfaces clean (swap, swizzle, dispatch reachability, decompression caps); no change needed. Robustness arc state: CRN/Vorbis/Ogg/decoders/allocation/bombs all verified or fixed; reality-dependent remainder is multi-face/mips, writers/CLI, real audio, codecs, Unity.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (sprite docs corrected)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- `src/resources/sprite.rs` docs claimed physics-shape/bone-geometry open; verified the capability exists (kernel + route extraction + importer emission, all tested) and corrected the doc block. Edges TODO is the source's own, retained as empty list.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (CRN bombs on both decoders)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Extended both CRN bombs to the Unity unpack path (same unchecked slicing, shared containment); classic + Unity fixtures asserted to the same loud errors.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (fallible bitmap allocation)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Fixed the one unguarded production bitmap allocation (non-Crunch path) to fail loudly via `try_reserve_exact` like its Crunch neighbor; sibling sites are test-only. OOM arm untestable by construction; success paths covered by existing tests + CI.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (decoder short-input sweep)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- New test asserts all 94 decoder routes reject 0/1/7-byte inputs loudly at valid dimensions. If CI reports a panic or `Ok`, the offending decoder needs a length guard next.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (hostile Ogg regression)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- New test pins the Ogg converter against truncations and page corruptions: no panic allowed; only loud errors, documented empty output, or well-formed WAV. If CI finds a panic or malformed WAV, the reader path needs containment or a header fix next.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (hostile Vorbis packets, no panic)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Probed lewton with hostile packets across three setups/layouts: zero panics in 55 decodes, so no containment needed (unlike CRN); production maps errors loudly. New end-to-end regression pins two hostile shapes; packet builder parameterized.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (CRN panic containment + bombs)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Static audit found two decoder panic sites the truncation green run cannot cover (header level loop, section offset slices). Contained both decoder call sites with `catch_unwind` mapping to loud errors (no vendored globals, verified); new level-count and table-offset bomb regressions assert loud failure instead of panic.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (truncated-CRN regression)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- New test asserts truncated CRN input fails loudly at eight cut points through the production wrapper (no panic, no short-Ok bitmap). If CI finds a panic instead, the wrapper needs a length pre-check next.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (Crunch guard-arm regression)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Added a four-case regression for the CRN wrapper's mismatch arms (faces, dimensions, codec, header) using only the committed fixture plus garbage bytes. Still open per the log: writers, project/CLI output, mips past level 0, multiple faces.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (stale-claim sweep, no code change)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Corrected stale living claims without touching statuses or dated history: CRUNCH entry (fixtures committed, production wrapper verified in-tree; remainder is writers/CLI/mips/faces), FSB5 DIFF entry (four-layout table proof plus rejection/rebuild/checksum regressions), RES-AUDIO-001 row (production-table lookup covered; real-audio/multi-stream/codecs/versions still open).
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (sprite triage narrowing, no code change)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Stale-claim check on `DIFF-SPRITE-NON-YAML-IMPORTER-001`: the Oct-13 "unwired" triage was accurate then but later wiring (`2363613` mode-driven decision, full collect/decide/apply/sheet/table route in `object_routing.rs`) superseded it. Narrowed the DIFF entry and the parity parenthetical to "wired with verification open"; the controlled export comparison still never ran. Dated history left untouched.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (YAML quoting predicate verified)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Audited the YAML quoting predicate against the frozen regex arm-for-arm (including Unicode property equivalence): exact match, shared edge holes included. New test pins 22 predicate cases plus bare-empty emission against drift in either direction.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (export traversal audit + parity pin)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Traced hostile asset names through the export planners: file names neutralize separators, but unknown/unreadable planners pass `../../` shapes through exactly like the frozen source (pinned by a new parity test); script namespaces cannot form `..`. Exploit needs a malicious bundle plus (for unknown) a non-default setting. No product change: a write-time guard would deviate from the reference.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (surround WAV conversion test)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Extended WAV coverage past stereo: the 5.1 silent container now asserts multichannel header fields and exact-silence data through the full decode path.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (FSB5 rebuild-side limit test)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Pinned the previously uncovered rebuilt-Ogg limit arm: exact-fit succeeds, one byte less fails with `DecodedDataLimitExceeded`. WAV-side limits were already covered.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (Ogg writer structural coverage)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Added direct tests for the previously uncovered Ogg writer paths: lacing table, oversize-packet fragmentation, and page grouping, all backed by a checksum-validating page parser. Source page-split parity stays deferred per the earlier zero-gain decision.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (Ogg writer audit + checksum proof)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Audited the Ogg page writer against RFC 3533: all arms clean (flags, caps, lacing, CRC shape, overflow handling). Added `ogg_page_checksum_matches_real_encoder_pages`, which proves the CRC function against every page of the repository Ogg fixture — the one writer property round-trips cannot show. Source page-split parity stays deferred per the earlier zero-gain decision.
- Local checks: `git diff --check` exit 0; single-file `rustfmt --check` clean. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (FSB5 surround layouts and header rejection)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Extended the table-backed FSB5 proof to 5.1/7.1 containers (silent packets, exact-silence WAV) and pinned loud failures for bad signatures, unsupported versions, and truncated headers. Probe evidence under gitignored `target/probe-fsb5-table/`, not committed. Real-audio table-backed decode still awaits Phase-2 inputs.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (privacy remediation in current sources)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Scanned tracked sources for identity leaks: original bundle filenames already absent; remediated two export-ID tests, one controller-root test module/fixture, and four provenance comments to generic descriptors with technical substance intact. Re-scan clean. Shared history untouched per the no-rewrite rule. See the validation-log entry.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (I18N decision-logic audit, no code change)

- Worktree `../main-work` (detached at `45d2778`); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Localization decision logic verified against frozen source and generated tables: default `en-US`, 23-code table/order, 4+4 hyphenation arms with null passthrough, ordinal existence, `LoadLanguage` null/identical/unknown semantics; locale dir holds the 23 stems, `en_US.json` has 272 keys, `id.json` is empty. Rust tests already pinned all of it, so no code changed. PRODUCT-I18N-001 stays open for the UI surface only. See the validation-log entry.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint); validation delegated to GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (Crunch/Texture3D-array proven non-issue)

- Worktree `../main-work` (detached at `b99597e`); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- Crunch decoder proven unreachable for Texture3D/array shapes: frozen `ToTextureFormat` maps no GraphicsFormat to a Crunch TextureFormat, so the no-version-API note becomes a proven non-issue rather than open debt. New regression `graphics_format_conversion_never_yields_crunched_texture_format` exhausts domain 0-151. See the validation-log entry.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint). Actions run `38491835469` is fully green on both platforms (lib 1,749 passed/1 ignored, CLI 128 passed, Clippy, release builds, formatting, hygiene).

## Latest current checkpoint — 2026-10-13 (FSB5 table decode validation in progress)

- Worktree `../main-work` (detached at `a32add3`); the Desktop checkout's conflicts and local changes are untouched. Phase 1 open, Phase 2 locked; no user input, Unity, or UABEA used.
- FSB5 production setup-table validation: scratch probes (gitignored `target/probe-fsb5-table/`, not committed) prove all 161 `VORBIS_LOOKUP` keys equal their setup CRC32 and every setup parses under lewton with production identification parameters (97 stereo-coupled parse only with two channels; 64 coupling-free parse with any layout). A micro-packet search found two-packet silent sequences decoding to exactly 128 zero frames under every setup/channel combination (225/225). New tests in `src/export/audio_export.rs` pin table integrity, ident-builder round-trip, table-backed end-to-end silent decode (stereo + mono), and loud unknown-CRC rejection. Real-audio table-backed decode still awaits Phase-2 inputs; see `DIFF-AUDIO-FSB5-RECONSTRUCTION-001`.
- Local checks: `git diff --check` exit 0. No local Cargo test/build (machine performance constraint). Actions run `38489124673` is fully green on both platforms (lib 1,748 passed/1 ignored including the four new tests, CLI 128 passed, Clippy, release builds, formatting, hygiene); an earlier run caught three over-width test lines, fixed in a follow-up commit first.

## Latest current checkpoint — 2026-10-13

- Phase 1 remains open. Commit 0cd4edb adds streamed Sprite bone geometry: the importer now reuses the already-resolved external vertex byte range and errors if a set stream is unresolved. Synthetic coverage verifies streamed and inline geometry equality.
- GitHub Actions run 38463426666 passed Linux and Windows workspace tests, warnings-denied Clippy, release builds, changed-line formatting, and public-repository hygiene. Linux: 1,730 library tests and 127 CLI tests passed (1 library test ignored); Windows: 1,732 library tests and 127 CLI tests passed (1 library test ignored). Local Cargo tests/builds were not run.
- The source SetBoneGeometry path reads inline VertexData directly, so the streamed behavior is a generic Rust extension and source-output/Unity validation remains open. Source Edges still has a TODO. No user input or project export was run.
- The completion inventory report is `target/feature-parity-report-20261013.json`: 391/391 rows recognized, 24 non-verified, zero duplicates/malformed rows, `phase1_gate=OPEN`. The source scan found no `todo!()` or `unimplemented!()` macros; ordinary fallbacks and full feature coverage still need audit.
- Work remains in isolated branch `codex/mesh-partial-channel-20261013`. Continue the §25 silent-loss/fallback audit by tracing candidate `continue` and default paths back through source and inputs; change code only after reproducing a concrete loss. Use GitHub Actions for Cargo validation. Unity Editor import/runtime was not run.


## Current checkpoint — 2026-10-13 active

### Latest validated increments

- Branch `main`, pushed HEAD `438e1e8` (`Match source Ogg sample clipping`). The only uncommitted tracked user change is `GOAL.TXT`; keep it unstaged. Preserve `cleanup_manifest.txt`, `docs/completion-audit-玉京烟霞-20261012.md`, and `tools/probes/anim-convert/packages/`.
- Ogg root cause fixed generically in `src/export/audio_export.rs`: NVorbis v0.10.4 sets `StreamDecoder.ClipSamples=true`; Rust now clamps decoded f32 samples to [-1,1]. Full lib 1,711 passed/1 ignored, CLI test 1/1, Clippy and release build pass; touched file formatted. Whole `cargo fmt --check` still fails only on documented pre-existing unrelated drift. Fresh 青岚山河卷 PreferWav r3 matches the compatible NAudio reference across 8,831,232 samples (max error 2.98e-7); source/r3 and r2/r3 audits plus partial reference results are in validation-log.
- Frozen GUI.Web exports were re-created without a CLI. 玉京烟霞 source/Rust r1: `target/real-acceptance-20261013-松月云庭图-source-web-r1/ExportedProject` and `target/real-acceptance-20261013-松月云庭图-r1`; source/Rust document report `target/real-acceptance-20261013-松月云庭图-r1-documents.json`. 260 PNGs pixel-equal, 1 OGG's 9,076 packets identical, all controller docs match, and the one Avatar text-style difference is structurally equal. Both reference scans are PARTIAL with zero issues and 1,806 imported owner-only IDs.
- 烟雨长亭 source/Rust r1: `target/real-acceptance-20261013-avtrm烟雨长亭-source-web-r1/ExportedProject` and `target/real-acceptance-20261013-avtrm烟雨长亭-r1`; report `target/real-acceptance-20261013-avtrm烟雨长亭-r1-documents.json`. 242 PNGs pixel-equal, all 36 OGG packet sequences identical, three Avatar documents structurally equal, class 86 byte-exact. Visible residuals: the 38 known class-21 builtin-vs-Soft material targets, Rust-only 月照琉璃.png/meta, 3 Avatar formatting differences, and the auxiliary path-map scope artifact. Reference scans are PARTIAL with zero issues; source/Rust counts differ by 38 target classifications and remain unresolved for full identity acceptance.
- Use retained short roots `C:\kjW13P`, `C:\kjPW2`, `C:\kjPW3`, `C:\kjMS13`, `C:\kjMR13`, `C:\kjAS13`, `C:\kjAR13` and older user roots. Do not repoint/delete junctions. C: had 43.9 GB free at last check. Keep audit JSON and fresh replay trees; no cleanup or deletion was performed.
- Unity Editor import/runtime and UABEA have NOT RUN. The source/Rust results above are static evidence and do not mean the samples or product are accepted.

### Immediate next work under the current gate

- The header-indexed parity report remains open (389 rows, 365 verified, 24 non-verified; `phase1_gate=OPEN`), so Phase 2/UABEA stays gated. The six-vector CRN production-wrapper regression passes against the independent full-RGBA hashes; full library tests (1712 passed, 1 ignored), Clippy, and release build pass. Global fmt still reports only the seven documented pre-existing files; `texture_converter.rs` is clean. Next: continue the broad GOAL §25 audit and close Phase 1 gaps. No sample replay, Unity check, or UABEA run was done for this test-only increment.
- After that, continue the broad GOAL §25 audit: malformed feature rows, unsupported class/version branches, silent-loss fallbacks, user entry completeness, and runtime dependency sweep. Keep the goal open; Unity licensing remains unavailable, and no static comparison substitutes for Editor runtime.

### 2026-10-13 Crunch reconciliation

- RES-TEXTURE-CRUNCH-001 now reflects the code: pure-Rust classic/Unity Crunch decode is wired from the serialized Unity version into Texture2D/Cubemap export. Six scratch CRN vectors now match independent native C++ oracle hashes through the low-level decoder; the status remains 已实现但未验证 because AssetRipper format/version routing and export have not been exercised with valid CRN inputs.
- The old statements that Crunch is wholly unported were corrected in the parity row and the current cubemap/array finding summaries. Texture3D/array helpers still use the no-version converter; no CRN behavior is claimed for those shapes. The missing direct texture2ddecoder dependency/license record was added to docs/dependencies-and-licenses.md; vendor/COPYRIGHT remains retained.
- Machine-readable parity audits: target/crunch-reconciliation-parity-report-20261013.json and target/crunch-fixture-decoder-parity-report-20261013.json. The post-update report recognizes 389/389 rows, lists 365 verified and 24 non-verified, and finds zero duplicate or malformed rows. Exit 1 reflects phase1_gate=OPEN, not a parser error.
- No production source or in-repository test code changed. No Cargo test/build, project export, or user sample input was run. A direct rustc harness did run against the prebuilt vendored decoder artifact; six externally sourced CRNs and its executable/log are under target/crunch-reference-fixtures-20261013. C: free space after that probe was 13,549,568 bytes; latest check is 13,185,024 bytes. Next CRN step: add the six vectors with their attribution and test the production converter wrapper, then run the required validation and fresh replay when storage allows.

### Prior validated controller-ID checkpoint (historical)

- Repository: C:/Users/koishi/Desktop/RustAssetripper/RustAssetRipper, branch main. Implementation commit 9f6ecf2 fixes AnimatorController release sub-document IDs and emission order. Check git log -5 for the immediately following evidence-document commit and push status.
- Root cause: frozen FetchEditorHierarchy traversal order determines the child asset count passed to GetPseudoRandomExportId; the controller root is omitted from that count. The old export allocator IDs/order differed. The generic post-assembly walker now follows the emitted YAML hierarchy, assigns/remaps IDs and local PPtrs/anchors, and errors on unreachable documents/collisions. Regression release_controller_documents_follow_source_hierarchy_ids_and_order pins the source TriggerHoldDetect machine/state/transition IDs and order.
- Validation after the final code edit: cargo test --locked --lib exit0 (1708 passed, 1 ignored); cargo clippy --locked --all-targets -- -D warnings exit0; cargo build --release --locked exit0. cargo fmt --all -- --check exit1 on established unrelated drift in target_dependency_probe.rs, project_workflow.rs other import blocks, texture_project.rs, collection_preparation.rs, prefab_roots.rs, texture.rs, texture_array.rs. Do not format those unrelated regions. Logs: target/controller-id-final-{lib-tests,clippy,format,release-build}.log.
- Fresh replays r15/r9/r19/r9/r10/r28 for 玉京烟霞/云栖仙府/松月竹庭/烟雨长亭/青岚古境/青岚山河卷 all exited0; six exact-decimal/BigInt static structure reports show every Avatar/controller structure equivalent with no unmapped controller docs. Class-91/1101/1102/1107 IDs/order now pair; complete counts and raw outputs are in docs/validation-log.md and target/controller-id-*-source-audit.json.
- Whole-project strict audits still exit1. Avatar class-90 style/representation diffs remain, 烟雨长亭/青岚古境 material/cache differences remain, opaque byte differences remain despite PNG pixel and OGG packet equality, and path_id_map audit-root scope artifacts remain. Strict reference scans are PARTIAL with zero reported dangling issues (counts: 玉京烟霞111812, 云栖仙府35601, 松月竹庭9006, 烟雨长亭17137, 青岚古境25453, 青岚山河卷207388). These are not full acceptance passes.
- Unity Editor import/runtime sampling and UABEA were not run. No static result is to be described as Unity behavior validation. All six replay exports explicitly report baseline EmptyExportCollection object(s) as non-exportable. Product remains unaccepted.
- Current short roots added: C:/rS10 -> 青岚古境 r10; C:/rW28 -> 青岚山河卷 r28. Existing short roots C:/rarM15, C:/kjR9, C:/rK19, C:/rM9 remain for the other fresh outputs. Existing source roots are C:/rarMS, C:/kjKS, C:/rKS, C:/rarAS, C:/rSS, C:/rWS/ExportedProject. Recheck junction targets before use.
- Disk at the last check was about 4.3GB free. No replay/comparator/test process remained running. Do not delete target audit JSON. Keep using fresh output paths, never --delete-existing. Original bundles and frozen outputs were not modified.
- Protected paths remain unstaged: GOAL.TXT, cleanup_manifest.txt, tools/probes/*/packages/. Also leave the untracked historical docs/completion-audit-玉京烟霞-20261012.md untouched unless a concrete need is established.

### Superseded Ogg continuation — completed and pushed

The Ogg-to-WAV clipping fix and its source/Rust evidence are complete in commit `438e1e8`; see the active checkpoint and 2026-10-13 validation-log entry. Continue with the controlled CRN wrapper regression above, then the broad completion-gate audit.
### Current checkpoint before the previous user-requested pause (historical)

**Historical status only.** At that checkpoint the user had said “ok now stop and hand-off write,” and the goal was marked paused. A later user instruction resumed work; the active status above supersedes this historical paragraph. The sole working subagent was interrupted at that earlier checkpoint.

### Repository and saved work

- Repository: `C:/Users/koishi/Desktop/RustAssetripper/RustAssetRipper`, branch main. Last implementation/tooling commit before this documentation checkpoint: **387e10757ee91a7dfdb3bf60696a01b4122f42eb**, pushed normally and remote hash verified. It fixes audit/reference scope, adds frozen Web export automation, and removes three vendored lifetime warnings. Read `git log -5` for the subsequent documentation commit.
- Four source files contain **uncommitted corrections**: `src/export/workspace_yaml/object_conversion.rs`, `src/export/value_yaml/model.rs`, `src/export/value_yaml/object_document.rs`, `src/export/value_yaml/tests/value_conversion.rs`. Preserve these changes; they are not disposable scratch. They add Unity 2022.3 class86/class121 editor layouts and a generated-only native class-name hint for shared abstract TypeTree roots. TypeTreeObject fallbacks, unknown/script/explicit/importer names retain their prior behavior. No sample name/hash/PathID dispatch was added.
- Final working-tree binary: `target/release/assetripper-rs.exe`, SHA256 **SHA256_REDACTED**. Full lib **1701 passed / 1 ignored**, Clippy exit0 with no warning/error matches, full locked release build exit0. Logs: `target/resource-editor-final-{lib-tests,clippy,release-all-build}.log`.
- `cargo fmt --all -- --check` exits1 on pre-existing drift in the same eight files as the initial check; no newly changed file is in that drift set. Do not format unrelated code. `git diff --check` previously exit0. New value-YAML tests47/47 pass; class86/121 TPK regression passes. The initial Hash128 test wrongly equated TPK node version with exported version; frozen probe proves node v1 versus export serializedVersion2, and the corrected assertion tests the actual export contract.
- Protected unstaged files remain `GOAL.TXT`, `cleanup_manifest.txt`, `tools/probes/*/packages/`. Never stage them. Documentation alone is committed at this pause; source corrections await the remaining replay/monotonicity checks.

### Oracle, replays, and exact current gaps

- Frozen projects exist: `target/real-acceptance-20261012-松月云庭图-source-web-r1/ExportedProject` and `target/real-acceptance-20261012-avtrm烟雨长亭-source-web-r1/ExportedProject`. `tools/export-reference-web.ps1` POSTs to frozen GUI.Free `/LoadFile` and `/Export/UnityProject`; both exports exit0. “No CLI, blocked” is obsolete. Input hashes/source-copy hashes and settings/logs are recorded in validation-log and `docs/completion-audit-玉京烟霞-20261012.md`.
- **Sprite importer facts FIXED and pushed (`b244def`):** production now discovers per-texture sprites via `m_RD.m_Texture` + graph resolution and applies `decide_sprite_importer` as a routing post-pass with processed coords values (stale bundle `m_Rect` replaced via the existing `get_sprite_coordinates_in_atlas` kernel) and `f32` rendering. All 5 玉京烟霞 sprite metas match source (incl. bit-exact derived pivot `0.49999997`); r14 audit profile identical to r10 with additional diffs down to the path_id_map scope artifact; 松月竹庭 r18 zero-diff regression; suite 1,706/1,706. Atlas-packed/cubemap/3D/lightmap stay open. Monotonicity replays current: 玉京烟霞 r14, 松月竹庭 r18 (superseded r10/r11/r13, 松月竹庭 r16/r17 deleted; disk 9.2GB).
- **Next scoped blocker: 玉京烟霞 r14 class-90/91 doc diffs** (`Assets/青岚资源路径`, 8 animator `.controller` docs incl. LowerLegRibbon/PB_CatEar) plus the 261 opaque-different pairs. Class-90 triaged: pure flow-vs-block style in `m_AvatarSkeletonPose` (`- t: {…}` vs expanded; values incl. `-0` identical) — check the `TransferUsingFlowMappingStyle` flag table for Avatar. Class-91 triaged: machine doc exists but ordered after states with allocator IDs (`7000000000`-scheme) instead of hierarchy-order pseudo-random IDs. **Seed rule proven by new test** (`pseudo_random_export_ids_match_frozen_玉京烟霞_controller_subdocs`): `AddAsset` skips the controller, machine takes seed 0, states follow in `FetchEditorHierarchy` order. Remaining work: hierarchy-order pseudo-random assignment + doc emission order (stress: moon_doll 100+ docs, TriggerHoldDetect inline 1101s).
- **Final 烟雨长亭 r5 succeeds:** `target/real-acceptance-current-avtrm烟雨长亭-r5`; 5829 objects,1131 Assets/青岚资源路径,343427284 asset bytes, support5/post2. Class86 `EnvLightColor.renderTexture` is **byte-exact** with frozen source, including editor base fields and Hash128. Source/Rust SHA256 `SHA256_REDACTED`.
- Final 烟雨长亭 whole-project strict audit exits **1**, not accepted: `target/resource-editor-final-烟雨长亭-r5-source-all-files.json/.log`,1135 paired assets,source-only0,Rust-only1 (`月照琉璃.png`),310 opaque pairs/32 exact/278 different. Supplemental1135 files had four reported residuals, now resolved except Soft: PrefabImporter metadata defect FIXED (`dc959fe`, 烟雨长亭 r7 meta matches source); AuxiliaryFiles/path_id_map.json exonerated (byte-exact 49,002; audit-scope artifact); Packages/manifest.json FIXED byte-exact (`39b5367`, 烟雨长亭 r8). Do not hide these raw results.
- **PrefabImporter defect history (fixed, see bullet above):** source prefab meta has `variantParentGUID: 000...000`, no `unableToImportOnPreviousDomainReload`, and the actual prefab `.unity3d` assetBundleName. Root source `PrefabExportCollection.cs::CreateImporter` selects PrefabImporter using the full export version and copies RootGameObject.AssetBundleName. Frozen class468431735 TPK at2022.3.22f1 confirms `m_VariantParentGUID`; probe exit0, `target/prefab-importer-source-tpk-2022-3.txt`. Trace release bytes → bundle-name processing metadata → routed prefab → versioned importer before editing.
- **玉京烟霞 r6 and r7 both FAILED exit1, Windows os112 disk full.** r6 used intermediate binary and failed at 云岫藏珍.asset; r7 used final binary and failed at `Assets/青岚资源路径` despite NTFS-compressed fresh output/temp. Logs: `target/resource-editor-玉京烟霞-r6-export.log`, `target/resource-editor-final-玉京烟霞-r7-export.log`. Both partial directories must remain classified failed. Do not reuse them or pass --delete-existing. Superseded by successful r8 (class121 paired, class124 exact) and r10 (path_id_map byte-exact).
- Final r5 references: `target/resource-editor-final-烟雨长亭-r5-references.json/.log`, strict exit **2/PARTIAL**,17137 checked(local14826/external2311),issues0,builtin6/imported-owner-only1239. 玉京烟霞 source/r5 each111812 checked,issues0,builtin263/imported-owner-only1806,strict exit2. Zero dangling issues is bounded existence evidence, not complete imported/builtin identity acceptance.
- 玉京烟霞 strict independent structure proof now uses **BigInt integer/header/PPtr IDs and exact decimal lexical rational comparison**, preserving ordered mappings/sequences and reference bijections: all1 Avatar+8 controllers/197 documents equal, `target/completion-audit-玉京烟霞-r5-avatar-controller-structure-r3.json/.log`. This supersedes r2's JavaScript floating-value limit. Final 烟雨长亭 r5 all3 Avatars+9 controllers/2378 docs equal, `target/completion-audit-烟雨长亭-r5-avatar-controller-structure.json/.log`. No Unity runtime claim.
- **Soft source-cache fidelity triage completed on final 烟雨长亭 r5:** all44 materials inspected;6 GUID-owner-equivalent, remaining38 differ only at `_IgnoreCookieTexture` source builtin10001 versus Rust 月照琉璃.png. Actual Unity2022.3.22f1 default resource10001 PNG is byte/pixel-exact with Rust, but input mip/color-space/streaming/filter/aniso settings differ from builtin. Input PathID1213616326460374869 Alpha8 base16384 bytes matches builtin base mip; all12 inspected importer fields preserve input or frozen ImporterFactory behavior. Frozen predefined cache keys only(name,width,height) and substitutes the builtin despite those settings. Classified **Rust-faithful representation difference**, not full binding/runtime equivalence. Reports `target/completion-audit-烟雨长亭-r5-material-before.json`, `target/completion-audit-烟雨长亭-soft-fidelity-r5.json`.
- 玉京烟霞 old r5 opaque proof completed:260/260 differing PNGs pixel/dimension/format-equal;1 OGG9076/9076 packets bit-equal;106 other opaque pairs byte-exact. **Final 烟雨长亭 r5 per-file278 opaque residual proofs were not run before the pause.** Do not inherit 玉京烟霞 proof for these inputs.
- Older-sample monotonicity for the current binary: DONE. Current replays: 松月竹庭 r17, 青岚古境 r9, 云栖仙府 r8, 青岚山河卷 r27, 烟雨长亭 r8, 玉京烟霞 r10 — all audited with profiles identical to (or better than) predecessors.

### Short roots and storage

- Existing short roots: `C:/rarMS` 玉京烟霞 source; `C:/rarMR` complete 玉京烟霞 r5; `C:/rarAS` 烟雨长亭 source; `C:/rarAR` 烟雨长亭 r3; `C:/rarAR4` 烟雨长亭 intermediate r4; **C:/rarAR5` final 烟雨长亭 r5**. Existing `C:/kjS`, `C:/kjKS`, `C:/kjR6`, `C:/kjR3` remain. Deleted kjR4/kjR5 must not be used. Audit through short junctions because PowerShell5.1 lacks reliable long-path support.
- Automatic policy review rejected the attempted removal of proven superseded 玉京烟霞 r4; no deletion executed and no bypass attempted. Replacement proof is `target/superseded-玉京烟霞-r4-cleanup-proof.json`; old r4/r5 remain. Both were losslessly NTFS-compressed:2054081395 logical bytes each stored951684747. Generated 玉京烟霞 source was compressed:2117389851 logical bytes stored1012598587. Original bundles/source were not modified.
- A generated-directory compression batch was **cancelled at the user's pause**, process/session exit-1. Only target/debug was partly processed; its compact log has no completion summary. The later 青岚山河卷/云栖仙府/松月竹庭 directories in the batch were **not reached**. `target/compression-debug.log` remains; `target/resource-editor-generated-compression.json` does **not** exist. No compact process remains. Free C: approximately **4.59GB** at pause, fluctuating with OS paging; do not treat this as enough for 玉京烟霞.
- That checkpoint recorded C: at 13 MB free; it is superseded. Current free space was about 43.9 GB. Preserve all existing target JSON and replay trees; use fresh output names and never pass `--delete-existing`.

### Resume order (not executed after pause)

1. Read this section, latest validation-log/differential-findings/completion audits, GOAL, `git status` and `git log`. Keep at most one subagent.
2. Free enough space; run final binary into **fresh 玉京烟霞 r8**. Create a new short junction only after successful export. Run whole-project strict audit and strict reference scan against C:/rarMS; confirm class121/124 corrections and investigate newly visible importer/project residuals.
3. Run fresh affected older samples, semantic audits and reference checks to prove monotonicity. Refresh/split relevant feature-ledger scopes. Only then commit and push the four pending correction files with their evidence; no protected files.
4. Implement the reproduced generic PrefabImporter/version/bundle-name correction with independent frozen fixtures and regression, then repeat the required suite/build/fresh sample checks. 烟雨长亭 r5 opaque proofs and auxiliary-output setting mismatch also remain open.
5. Continue broad GOAL §25 closure: latest ledger report389/366 bounded verified/23 open, SpriteAtlas rejection, Terrain lighting, PreferWav Ogg-to-WAV, narrow version/platform/editor/cache branches, silent-loss/fallback tracing, nested16 sample files/unfrozen complete manifest, full product entry/settings/features, imported references and Unity runtime. Original strict baseline check exits1 on643 unlisted build files (zero changed/missing manifest files); isolated1449/1449 source hashes match. These are different checks.

Core is independent; C#/Web/Node are development oracles only. UABEA and Unity import/dynamic animation were **not run**; historical missing Unity license remains an external runtime blocker. GOAL §25 and whole-product/sample acceptance remain **NOT COMPLETE**.

## Active stage (2026-10-11)

Product Convergence mode is active under the user's current task attachment. Local 用户资源格式/VRCA acceptance is authorized. Frozen AssetRipper source remains the behavioral reference. Unity Editor runtime validation is unavailable in this environment.

## Current acceptance and blocker

- Primary acceptance sample: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `SHA256_REDACTED`). Latest Rust export: `target/real-acceptance-20261011-avtr青岚古境-avatar-mask-editor-r1`; 8,547 objects, 1,536 asset files, 601,755,682 asset bytes. Frozen source export: `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`.
- Latest root-prefab audit pairs all 4,491 documents by class ID/local fileID. GameObject (1,828), Transform (1,828), MeshRenderer (100), MeshFilter (100), AudioSource (27), ParticleSystem (90), and the other compared root classes match after project GUID/path normalization; 135 and 2,184 curve tangentMode values match. The class-319 `vrc_HandsOnly(Clone)` mask now includes the four editor base fields and its root-prefab pointer resolves to the emitted `.meta`. One AvatarDescriptor pointer still uses different menu output paths (`MonoBehaviour_56` vs `MonoBehaviour_46`), but its recursive graph matches across 65 MonoBehaviour YAML assets, 219 pixel-exact PNGs, and one byte-exact script asset. Reports: `target/real-acceptance-20261011-avtr青岚古境-avatar-mask-editor-r1-prefab-audit.json` and `target/real-acceptance-20261011-avtr青岚古境-expression-menu-semantic-summary.json`.
- The broad YAML inventory still does not scan `.state` or `.blendtree`. Source has 13 standalone `.state` and 41 `.blendtree` assets which Rust embeds in controller documents. The new `tools/compare-controller-blendtrees.ps1` compares every recursive tree graph: 174/174 documents, 156/156 unique semantic graphs, 631/631 child records, zero differences. Report: `target/real-acceptance-20261011-avtr青岚古境-blendtree-graph-r1.json`.
- A large Mesh index buffer previously exhausted the StreamingInfo scanner's node limit by scheduling scalar leaves. The scanner now budgets only structs and arrays; focused resource tests pass 20/20. Latest real-sample inspection reports 552 candidates, 436 Bundle ranges, zero scan errors, and `complete=true` after 5,020,123 structural values. Fresh export and comparison results are recorded in the newest validation-log entry; 19 inspect-mode refs are Unity builtin meshes whose exported source/Rust GUID and fileID pointers match.
- Latest additional real-sample comparison: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `SHA256_REDACTED`). Rust r6 is `target/real-acceptance-20261012-avtr松月竹庭-r6`; frozen source export is `target/real-acceptance-20261012-avtr松月竹庭-source/ExportedProject`. A predefined `Default-Material` cache miss was fixed using the source 2022.3.11f1 `(Material.Name, Shader.Name)` identity. The Cube renderer pointer now matches source at builtin-extra PathID 10303/FGUID, and the standalone Rust `.mat` is gone. Reports record 114/114 matching input object rows and 1788/1788 YAML documents; the whole-project comparison still has 4/5 path-count differences and capped reference/null-pointer difference lists. The focused controller report has 3/3 BlendTrees, 37/37 child records, zero semantic differences.
- Additional avatar sample: 青岚雅苑` (58,150,845 bytes; SHA-256 `SHA256_REDACTED`). Fresh Rust export is `target/real-acceptance-20261011-avtr云栖仙府-stream-scan-r1`; frozen source project is `target/real-acceptance-20261011-avtr云栖仙府-source/ExportedProject/ExportedProject`. StreamingInfo scan is complete with zero scan errors, and 6,529/6,529 object payloads decoded. The 22 unresolved graph references all target `DefaultResources` engine assets.
- 云栖仙府 BlendTrees compare 717/717 documents, 339/339 unique recursive graphs, 1,807/1,807 ChildMotion records, with zero semantic differences. The broad project report records 3,312 source vs 2,821 Rust files, 498 source-only and 7 Rust-only paths, 9,956/10,204 YAML documents, and 2,846 matched documents; reference and null-pointer difference lists are capped. The source-only paths include 98 `.state`, 72 `.blendtree`, 76 `.asset`, 2 `.mask`, 1 `.prefab`, and 249 `.meta` files. See `DIFF-REAL-AVTR云栖仙府-PROJECT-LAYOUT-001`.
- The previous class-43 Mesh schema discrepancy is fixed and documented under `DIFF-REAL-AVTR青岚古境-MESH-EDITOR-TREE-001`; commit `29a7a78` is pushed. The class-319 AvatarMask and class-90 Avatar editor-base discrepancies are also fixed for their inspected sample paths.
- Resolved serialized-content difference: in 青岚雅苑`, source `_MainTex` points to builtin fileID 10307 while Rust points to the exported `Default-ParticleSystem` Texture2D. The builtin fileID 10307 and bundle PathID `4988150354445090395` export to identical 3,256-byte PNGs (SHA-256 `SHA256_REDACTED`). No code change was needed; Unity import/runtime remains unverified. See `DIFF-REAL-AVTR青岚古境-PREDEFINED-TEXTURE-CACHE-001`.
- CURRENT_BLOCKER: content-first breadth. Controller split COMPLETE and validated on two samples (states+trees+behaviours; code in `src/export/project_workflow/animator_controller.rs`, `animator_collections.rs`, `planning.rs`, `export.rs`; 青岚古境 r7 + 云栖仙府 r6 prove placement/content parity with onlySource=0; 4 regression samples byte-stable). Next by value: (1) frozen 玉京烟霞/烟雨长亭 source exports (blocked: no CLI path); (2) Ogg page-split parity (zero-gain, deferred); (3) PNG encoder settings (cosmetic); (4) Minigun material + ID-family + opaque-encoder residuals are classified acceptable — revisit only with new evidence; (5) broad completion-gate audit per GOAL §25 when lanes clear. Proof tools committed (`tools/compare-png-pixels.ps1`, `tools/compare-ogg-packets.ps1`, `tools/check-project-references.ps1`, `tools/probes/controller-oracle/`). Audit technique note: samples with 260+ char paths must be audited through short directory junctions (`mklink /J`, retained: `C:\kjS`/`C:\kjR4`/`C:\kjR5`/`C:\kjR6`/`C:\kjKS`), since the audit runs under PowerShell 5.1 without long-path support. Unity import/runtime validation remains blocked on licensing. Keep the goal open until breadth evidence exists.
- Separate runtime acceptance boundary: the unnamed `AutoDresser_Comp` and `AutoDresser_Decomp` layers in 青岚雅苑` remain unverified in Unity. Frozen-source parsing and processing evidence is recorded, but dynamic state sampling is unavailable. Keep that result open; do not claim runtime equivalence. UABEA was not run.
- `青岚山河卷` (SHA-256 `SHA256_REDACTED`) latest full Rust export is `target/real-acceptance-current-青岚山河卷-r23`: 72,349 objects, 1,086 asset files (meta count equals source), 244,669,089 asset bytes. Full-project audit (1,092 paired files, zero path residuals): byte-exact GameObject 20,567, Transform 20,547/20,547, BoxCollider 3,997, Mesh 93/93, Texture2D 12/12, ComputeShader 12/12, the scene-subfolder `.lighting` asset, and the pseudo-random timeline child IDs; GUID-semantic slices include MeshRenderer 10,239+1, MeshFilter 10,230+10, SkinnedMeshRenderer 189, MeshCollider 628+1, and all 10 sprites. Zero FAIL-class doc diffs; accepted: avatar style (proven equivalent) and file-local controller IDs. Opaque: 83 byte-identical (incl. `Door.cs` + all 20 shaders); all 235 PNGs pixel-identical (encoder-only bytes); OGG packet streams bit-identical (page framing differs). See the r23 validation-log entry. 松月竹庭 r11 regression: stable audit identical in structure to r9 with zero path residuals (mask exact, prefab at root, 114 at 77 matched with zero diffs). The whole project and sample remain unaccepted.


## 2026-10-13 - GitHub Actions checkpoint

- Branch `codex/mesh-partial-channel-20261013`, validated HEAD `b0a828fe7c492b46e88983f39cab4350d9193b48`; the final CI run is 38424252512 and all jobs passed.
- Linux and Windows workspace tests, warnings-denied Clippy, changed-line formatting, public repository hygiene, and release builds passed. Exact commands, counts, and scope are in the appended validation-log entry.
- The texture YAML assertion correction is test-only: it now checks the serialized byte count and `_typelessdata` separately, with the expected payload associated with each ordered object. The audio path assertion uses the existing host-separator normalization.
- Local Cargo tests and builds were not run; heavy validation was delegated to Actions. No user asset input or UABEA content was used in this increment.
- Phase 1 remains open and Phase 2 remains locked by the repository gate. Do not inspect user bundles or UABEA until a documented Phase 1 pass exists. Unity Editor import/runtime remains externally blocked by licensing.
- Next high-value audit item: Sprite importer metadata. `src/resources/sprite.rs` currently exposes decision gates and bone scaling; source-backed outline, physics-shape, and vertex/index/weight extraction are still open and are not wired through the importer path. Trace the frozen source and current project-export route, then implement and test the generic behavior.
- Pending worktree edits remain outside this commit. Preserve them; inspect `git status` before staging. In particular, a texture-field alias conversion change in `src/export/workspace_yaml/object_conversion.rs` is unvalidated; validate its full source-field to editor-TypeTree path before committing. This file also carries an unrelated local privacy comment edit that must stay out of public patch context.
- Keep routine tests/builds on Actions because local machine performance is limited. No target cleanup or temporary artifact deletion was performed in this checkpoint.
The dated checkpoint below is historical. The current user-supplied AGENTS.md requires the first-stage completion gate before Phase 2; UABEA/sample analysis remains gated until `phase1_gate=PASS`. Preserve the untracked local `cleanup_manifest.txt` and `tools/probes/anim-convert/packages/`.

## Historical checkpoint state

- Repository: 本地路径已隐去
- Branch: main; starting HEAD: 4bcaba2399eb3e3f72741aebd0c40e6d06ee34aa, matched origin/main before this checkpoint.
- Phase 1 full Rust port remains OPEN. Phase 2 remains LOCKED. Do not open, hash, identify, parse, or export user VRChat bundles, and do not read or run UABEA until the Phase 1 gate passes.
- Reference baseline: local AssetRipper 2.0.0 snapshot, 1,449 files; manifest is docs/baseline-files.sha256 (SHA-256 SHA256_REDACTED).
- Feature ledger: 388 rows, 365 verified and 23 non-verified. The latest increment closes one narrow 2022.3 scene-object editor-format slice; it does not close a broad Phase 1 gate.

## Latest implementation checkpoint

- The product stream mount path now uses one recursive SmartStream child router for UnityFS, UnityRaw, UnityWeb, and UnityWebData, with SerializedFile and Resource leaves. Source-backed ranges are distinguished from decoded/materialized children for active nested-byte accounting; UnityFS one-layer wrapper and failed-entry semantics are retained. Remaining slice/Cow dispatch belongs to the legacy in-memory APIs. Some bounded entry ranges still need temporary backing when they cannot safely retain a source range.
- Unity 2022.3 AnimatorController work covers source-projected simple and nested 1D BlendTrees, state-machine hierarchy, ordered behaviour/editor collection ownership, and scoped pointers. Broader controller export, other blend modes/version branches, and Unity runtime sampling remain open.
- Generated EditorSettings and conditional project outputs now cover original StreamingAssets copying, catalog-wide auxiliary GameAssemblies, and persisted script/StreamingAssets options. Complete manager dependency closure and remaining import/export settings are still open.
- The latest evidence and exact commands are in docs/validation-log.md; source differences are in docs/differential-findings.md. No expected fixture or frozen baseline was modified.

## Validation at stop

Latest focused runs passed:
- cargo test --lib export:: --locked --quiet: 479 passed, 1 ignored.
- cargo test --lib io::platform_ --locked --quiet: 43 passed.
- cargo test --bin assetripper-rs export_project_ --locked --quiet: 53 passed.
- cargo check --workspace --locked.
- cargo fmt --all -- --check; git diff --check; tools/verify-baseline.ps1 (1,449 files unchanged).
- The managed-project source probe build and post-export mode also passed; its fixture hash and command are recorded in the validation log.

Full workspace tests, full Clippy, and Unity Editor import/runtime checks were not run for the latest increment. Unity runtime is unavailable without a valid Editor license. Do not upgrade focused evidence into a full Phase 1 pass.

## Next work

Continue from docs/phase1-closure-inventory.md. The nearest product gaps are shared-release AnimatorController ownership and remaining assembly processing/dependency closure, followed by the other open resource and project-export paths. Keep Phase 1 as the gate; the smallest 用户资源格式/VRCA acceptance is not authorized until it passes.

## Preserved local scratch

The checkpoint intentionally excludes and leaves untouched: src/bin/ (three local probe programs), tools/compare-real-unity-projects.ps1, and tools/probes/anim-convert/packages/ (22 restored package files). Review these separately before any future staging.
The Unity 5.6 RenderSettings regression in src/main_tests_05.rs and its source fixture are also preserved locally but excluded from this checkpoint, following the earlier limit on adding Unity 5.6-specific synthetic cases.

## 2026-10-13 — Sprite outline continuation (45f4609)

- Phase 1 is still open and Phase 2 remains locked. This increment read only the frozen source code; it did not open user bundles or UABEA.
- Commit `45f4609` was pushed normally to `codex/mesh-partial-channel-20261013`. It adds a source-compatible standalone mesh-outline walk and reference-output regression cases in `src/resources/sprite.rs`.
- The run for that commit is GitHub Actions `38429434143` (<https://github.com/cocokoishi/RustAssetRipper/actions/runs/38429434143>) and completed successfully. Linux: 1717 library and 127 CLI tests passed (one ignored library test); Clippy with warnings denied and release build passed. Windows: 1719 library and 127 CLI tests passed (one ignored library test); Clippy with warnings denied and release build passed. Changed-line formatting and public-repository hygiene passed.
- Local full Cargo checks/builds were not run. The temporary frozen-source probe command and outputs are in `docs/validation-log.md`.
- The new mesh walk is not yet connected to texture importer output. Next: finish Actions, record the final result, then implement versioned render-data vertex/index extraction and the caller-side PPU/pivot/packing transforms needed to feed `GenerateOutline`. Continue with the source path, not bundle-specific patches.
- The prior checkpoint's pending no-push notice is superseded by the ordinary push of `45f4609`. Preserve all other staged and unstaged work. Keep `GOAL.TXT`, `cleanup_manifest.txt`, and probe package directories out of commits; do not stage entire docs or source directories.

## 2026-10-13 continuation — Sprite render-data outline extraction (`9bf96b5`)

- Phase 1 remains open; Phase 2 is still locked. The increment read frozen source and synthetic decoded fields only. No user bundle, UABEA, or Unity runtime was used.
- `generate_render_data_sprite_outlines` now reads version-aware vertex slices and the source-defined 16-bit little-endian submesh triangles; `generate_sprite_outlines` already applies PPU scale, pivot shift, and packed rotation. Commit `9bf96b5` was pushed normally.
- GitHub Actions `38432913994` passed: Ubuntu 1,720 library + 127 CLI tests and Windows 1,722 library + 127 CLI tests (one ignored library test on each platform), warnings-denied Clippy and release build on both platforms, changed-line formatting, and public-repository hygiene. Local Cargo validation was not run.
- Remaining Sprite work: decode the render-data fields from TypeTree values; resolve external stream bytes before calling the kernel; implement the legacy `Vertices`/`Indices` branch; connect the result to importer output and `FillSpriteMetaData`. `GeneratePhysicsShape` and `SetBoneGeometry` remain open. Unity import/runtime remains unverified.
- Next: trace the generic SpriteRenderData-to-importer facts path in the frozen source and Rust routing, add synthetic regressions for the mapping, then wire the kernel without sample-specific dispatch. Continue Phase 1 completion audit afterward.
- Keep all existing staged and unstaged work intact. Do not stage `GOAL.TXT`, `cleanup_manifest.txt`, or probe package directories. No subagents were used.

## 2026-10-13 continuation — Decoded SpriteRenderData outline inputs (`a879d4f`)

- Phase 1 remains open; Phase 2 remains locked. This increment used frozen source and synthetic decoded values only. No user resource, UABEA, or Unity runtime was used.
- The Rust path now maps a decoded render-data value into version-aware vertex slices and source-specific 16-bit little-endian submesh triangles. A set external stream must be resolved by the caller; unresolved bytes are an explicit error. The no-modern-data result signals the legacy path and does not claim that legacy geometry was implemented.
- Commit `a879d4f` was pushed normally. Actions `38434778368` passed: Ubuntu 1,722 library + 127 CLI tests and Windows 1,724 library + 127 CLI tests (one ignored library test per platform), warnings-denied Clippy and release builds on both platforms, changed-line formatting, and public-repository hygiene. Local Cargo validation was not run.
- Remaining Sprite work: connect TypeTree object payloads and reference-graph ownership to the decoded-value kernel; resolve streamed bytes using the owning collection; implement legacy `Vertices`/`Indices`; serialize outlines into SpriteMetaData and the texture importer. `GeneratePhysicsShape` and `SetBoneGeometry` also remain open.
- Next: trace the general SpriteRenderData ownership and stream-resolution path, add regression fixtures for that mapping, then wire the already-validated kernel to importer output. Continue the Phase 1 completion audit before any Phase 2 input inspection.
- Preserve staged and unstaged work. Keep `GOAL.TXT`, `cleanup_manifest.txt`, and probe package directories out of commits. No subagents were used.

## 2026-10-13 continuation - Legacy Sprite geometry (51ea73c)

- Phase 1 remains open; Phase 2 remains locked. Commit `51ea73c` is pushed normally. No user-provided bundle, UABEA, or Unity runtime was used.
- Sprite outline generation now reads both decoded modern submesh geometry and the frozen-source legacy `m_Vertices`/`m_Indices` geometry. Modern branch precedence, `pos` Vector3 decoding, 16-bit index triples, trailing-index behavior, and malformed-value errors have synthetic regressions.
- GitHub Actions run 38437678839 passed Linux and Windows workspace tests, warnings-denied Clippy, release builds, changed-line formatting, and public-repository hygiene. Exact counts and commands are in `docs/validation-log.md`. Local Cargo validation was not run.
- Superseded next step: the SpriteAtlas ownership and TextureImporter association path is now implemented under `DIFF-SPRITE-ATLAS-PROJECT-001`; see the current checkpoint below. The remaining project-level and full Phase 1 gaps are still open.
- Preserve all staged and unstaged work. Keep `GOAL.TXT`, `cleanup_manifest.txt`, and `tools/probes/*/packages/` out of commits. Routine Cargo work stays on GitHub Actions because local performance is limited.

## Checkpoint (2026-10-13) - SpriteAtlas reconstruction and importer association

- Current product phase: Phase 1 parity remains open. Code commits `1ec529b`, `ee28395`, and `a80bac5` were pushed through `codex/mesh-partial-channel-20261013`; no sample-specific behavior was added.
- Project YAML reconstruction resolves SpriteAtlas data by exact render key, copies the traced render-data members, recalculates geometry using Atlas crop data, clears the Atlas link/tags, and maps copied PPtrs through the multi-file graph and export pointer map.
- TextureImporter collection now follows workspace object encounter order for Sprites and `PackedSprites` order for SpriteAtlas entries. It resolves AtlasData.texture in the Atlas file scope, preserves first insertion while merging a later Atlas association, caches each Atlas key index once, and errors on unresolved non-null or wrong-type references. Explicit null and valid predefined engine references have separate paths.
- GitHub Actions run `38475037053` passed: Ubuntu 1,739 library tests passed / 1 ignored and 127 CLI tests passed; Windows 1,741 library tests passed / 1 ignored and 127 CLI tests passed. Warnings-denied Clippy and release builds passed on both platforms; changed-line formatting and public-repository hygiene passed. The initial CI compile/lint issues and a mistaken miss-key fixture are documented in `docs/validation-log.md`.
- Local checks were limited to `rustfmt` and `git diff --check`; Cargo was not run locally due to the machine performance constraint.
- Privacy correction: tracked Markdown was normalized with the local alias registry. Registered raw input names, absolute home paths, long hashes/GUID-like values, and Unity asset paths/file names are removed from current versions; the private mapping file stays outside Git and out of staging.
- Evidence is synthetic decoded-value coverage only. No Unity project replay was produced, so `tools/audit-project-documents.ps1` and `tools/check-project-references.ps1` were not run. Unity Editor runtime/import and UABEA remain unrun. Keep `RES-SPRITE-ATLAS-PROJECT-001` at `已实现但未验证` until project-level evidence exists.
- Next: continue the Phase 1 completion-gate audit from `docs/feature-parity.md`, trace the next high-value non-verified source path, and add one generic implementation plus CI-backed regressions at a time. Preserve the privacy and staging rules; use GitHub Actions for Cargo validation and do not spawn subagents.

## 2026-10-13 checkpoint - CI-backed container and mesh contract regressions

- Current branch phase: Phase 1 parity remains open; Phase 2 input inspection remains locked. Work used synthetic fixtures and frozen-source analysis only; no user-provided input, UABEA, or Unity Editor was used.
- Latest code commits 7e04cef, 28a7d4c, and c1bd12f are pushed normally to codex/mesh-partial-channel-20261013. The increments add a partial mesh-channel merge regression, nested UnityRaw-to-WebData mounting through both path APIs, and retained-reader assertions for directory-preserving split-container import.
- GitHub Actions run 38478285253 passed on Ubuntu and Windows. Workspace tests, warnings-denied Clippy, release builds, changed-Rust formatting, and public-repository hygiene passed. Exact test counts and commands are in docs/validation-log.md.
- The mesh finding DIFF-MESH-PARTIAL-CHANNELS-001 is now classified as a stale static candidate for the narrow partial-channel merge contract: the generic merge reader already preserves decoded channels. The new test covers generated decoded values; serialized TypeTree-to-project export remains unverified.
- The generated nested-container test confirms file-backed recursive WebData dispatch through both path APIs. The Unix Rust retained-reader behavior now runs in Actions. The non-Windows frozen-source runtime comparison remains unrun, so IO-FS-IMPORT-PRESERVE-DIRECTORY-001 remains blocked for that comparison.
- No project export was produced. tools/audit-project-documents.ps1 and tools/check-project-references.ps1, Unity Editor import/runtime, and UABEA were not run. No Unity license status was tested.
- Next: use the parity inventory to identify the next high-value non-verified Phase 1 route, trace source-to-export behavior before code changes, add one generic regression-backed increment, and run heavy validation in GitHub Actions. Preserve the Phase 1 gate and public-repository privacy constraints. Do not stage GOAL.TXT, cleanup_manifest.txt, or tools/probes/*/packages/.

## 2026-10-13 checkpoint - Mixed nested-container recursion audit

- Current gate: Phase 1 remains open and Phase 2 input inspection remains locked. No user-provided input, UABEA, or Unity Editor was used.
- Code commit d9d0e7e was pushed normally to codex/mesh-partial-channel-20261013. It adds a generated three-level UnityRaw-to-UnityRaw-to-WebData regression through both path APIs, checking parent scopes, identity, order, file-backed output, and payload bytes. Source-to-mount tracing found the implementation already recursively dispatches recognized containers; this increment closes a coverage gap and changes no production behavior.
- GitHub Actions run 38480063415 passed on Ubuntu and Windows: workspace tests, warnings-denied Clippy, release builds, changed-Rust formatting, and public-repository hygiene. Counts and exact commands are in docs/validation-log.md.
- The formal feature-parity report is target/feature-parity-report-20261013-container-audit.json. It exits 1 because the gate is open: 393 rows, 368 verified, 25 non-verified, zero duplicate IDs, zero malformed rows.
- No project export was produced. tools/audit-project-documents.ps1 and tools/check-project-references.ps1, Unity Editor import/runtime, and UABEA were not run. No Unity license status was tested. The non-Windows frozen-source runtime comparison remains unrun.
- Next: investigate RES-MATERIAL-EXPORT-001's documented coverage gap for multiple non-null saved texture pointers. Trace bytes through TypeTree decoding, cross-file reference resolution, pointer mapping, and emitted Material YAML before changing code; add a source-backed generic regression if the path exposes a coverage or implementation gap. Keep heavy Cargo validation on GitHub Actions and preserve public-repository privacy and staging restrictions.

## 2026-10-13 checkpoint - Material saved-property pointer coverage

- Phase 1 remains open; Phase 2 input inspection is still locked. No user-provided input, UABEA, or Unity Editor was used.
- Code commit 20c798c adds a generated class-21 project-export regression for two non-null saved texture PPtrs in separate external files; f56c80f fixes one changed-line formatting issue. Both were pushed normally to codex/mesh-partial-channel-20261013.
- GitHub Actions run 38481770880 passed on Ubuntu and Windows: workspace tests (Ubuntu 1,742 library + 128 CLI; Windows 1,744 library + 128 CLI; one ignored library test per platform), warnings-denied Clippy, release builds, changed-Rust formatting, and public-repository hygiene.
- The test confirms distinct source PPtrs map to their own exported Texture2D GUIDs and preserves map order. The exact source-runtime comparison still covers only one non-null pointer; RES-MATERIAL-EXPORT-001 remains implemented but unverified.
- The latest formal parity report is target/feature-parity-report-20261013-container-audit.json: 393 rows, 368 verified, 25 non-verified, no duplicate IDs or malformed rows. Its exit 1 is expected while the Phase 1 gate is open.
- No persistent export output was produced. Project-document/reference audits, Unity Editor import/runtime, and UABEA remain unrun; no Unity license status was tested.
- Next: continue the Phase 1 material audit by obtaining an independent source-runtime result for multiple saved texture PPtrs or broaden version/map coverage. Trace the serialized value, graph edge, pointer identity, and YAML output before any production edit. Keep full Cargo checks on GitHub Actions and preserve privacy/staging rules.


## 2026-10-13 checkpoint - FSB5 decoder scope and generated fixtures

- Phase 1 remains open; Phase 2 remains locked. No user input, UABEA, or Unity Editor was used.
- `RES-AUDIO-001` records a bounded Rust FSB5/Vorbis route: versions 0/1, one stream, format 15, and setup CRCs in the vendored table. Historical packet-identity evidence covers one input. The generated version-1 packet regression exercises parser, packet decode, and Ogg reconstruction with an injected setup header; production setup-table lookup remains untested.
- Code commit `5347638` added parser regression `fsb5_vorbis_parser_reads_versioned_headers_metadata_and_aligned_packets` and packet regression `fsb5_vorbis_reconstruction_preserves_audio_packets_and_source_samples`. Actions run `38484792282` passed workspace tests, warnings-denied Clippy, release builds, and public-repository hygiene on Ubuntu and Windows; it failed changed-line formatting because the `packets` array was kept on one line. The array has been expanded in the worktree; that correction is not yet committed or validated by Actions.
- Latest parity audit: `target/feature-parity-report-20261013-fsb5-decoder-audit.json`, 393 rows, 368 verified, 25 non-verified, zero duplicate IDs, zero malformed rows; expected exit 1 and Phase 1 OPEN.
- Actions run `38483926755` passed on Ubuntu and Windows for `e6bfbc0`, including the parser-only test. Exact `38484792282` outcomes and test counts are in `docs/validation-log.md`. Keep Cargo tests, Clippy, and release builds on GitHub Actions due host performance limits; no local Cargo command was run.
- Next: commit and push the formatting correction, confirm all jobs pass on Actions, then add a table-backed fixture that exercises a production setup CRC through the public decoder path. Do not promote broad audio parity based on the injected-setup fixture.
- Unity Editor import/runtime, UABEA, project-output audits, and the successful decoder path were not run. Preserve privacy and staging rules; do not stage `GOAL.TXT`, `cleanup_manifest.txt`, or `tools/probes/*/packages/`.

## 2026-10-13 handoff - Main fast-forward and next parity work

- The current code line was fast-forwarded to `main` at `b991f74`; the working tree used for this handoff is detached at that main commit. No force push was used. Continue on `main`; do not create a feature branch for the next increment.
- GitHub Actions run `38485280724` passed all jobs on Ubuntu and Windows for `b991f74`: workspace tests (Ubuntu 1,744 library + 128 CLI; Windows 1,746 library + 128 CLI; one ignored library test per platform), warnings-denied Clippy, release builds, changed-line formatting, and public-repository hygiene. The earlier `38484792282` run failed only formatting; `b991f74` corrected it. No Cargo command ran locally.
- Phase 1 remains open and Phase 2 remains locked. The latest parity inventory still has 393 rows: 368 verified, 14 implemented-but-unverified, 9 in progress, 1 investigated, and 1 externally blocked. Its exit 1 is expected while required features remain open; see the retained report under ignored `target/` and `docs/validation-log.md`.
- FSB5 now has parser and packet-reconstruction regressions. The packet test injects a generic setup header and does not validate a successful decode using the production setup table. `RES-AUDIO-001` remains implemented-but-unverified; the next concrete task is to establish a successful production-table decoder fixture, then broaden supported formats and version/layout coverage.
- No user-provided inputs, UABEA, Unity Editor import/runtime, or project export were used in this increment. Do not inspect user bundles before the Phase 1 gate passes.
- The separate Desktop checkout was left untouched because it already contains unresolved merge conflicts and local changes. Use a fresh checkout of `main` for subsequent work; do not reset or clean that existing checkout.
- Preserve the privacy and staging rules: do not stage `GOAL.TXT`, `cleanup_manifest.txt`, or `tools/probes/*/packages/`. Keep Cargo tests, Clippy, and release builds on GitHub Actions due the local performance constraint.
