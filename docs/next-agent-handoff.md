# RustAssetRipper hand-off — 2026-10-10

## Active stage (2026-10-11)

Product Convergence mode is active under the user's current task attachment. Local 用户资源格式/VRCA acceptance is authorized. Frozen AssetRipper source remains the behavioral reference. Unity Editor runtime validation is unavailable in this environment.

## Current acceptance and blocker

- Primary acceptance sample: 青岚雅苑` (Unity 2022.3.22f1). Rust export: `target/real-acceptance-20261011-avtr青岚古境-final`; 8,548 objects, 1,537 asset files, 705 AnimationClips, and 9 AnimatorControllers. Frozen source export: `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`; comparison report: `target/real-acceptance-20261011-avtr青岚古境-semantic-compare.json`.
- The first comparison uncovered a tooling gap: the YAML inventory does not scan `.state` or `.blendtree`. The source project has 13 standalone `.state` and 41 `.blendtree` assets that Rust embeds in controller documents. Including those documents makes AnimatorState/BlendTree counts align, so the initial count deltas were false positives.
- CURRENT_BLOCKER: a graph-level difference in `FX_Milfy 1.controller` remains unclassified. Source leaves the `AutoDresser_Comp` and `AutoDresser_Decomp` state-machine names blank, exports no child states on either machine, and keeps their states in standalone `.state` files with empty `m_Transitions`. Rust recovers six states in each machine and emits 24 state transitions from the packed state constants. AssetRipper's `AssignStateMachineChildStates` skips machines with blank names, which explains the source output omission, but the recovered Rust graph still needs independent semantic validation.
- Next action: build a graph comparison that resolves source external `.state`/`.blendtree` GUIDs into the matching Rust controller subassets by layer, state path, and ordered motion edges. Compare state fields, transition targets/conditions, behavior ownership, and clip/tree data. Trace the 24 transitions to raw packed data and a source-runtime projection; correct Rust only if that evidence shows a defect. Unity Editor runtime validation is unavailable.
- `青岚山河卷` (SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`) exports to 1,087 assets. Its 12 default-YAML lightmaps are class-28 YAML and were byte-identical before and after the false-positive notice fix. Latest replay: `target/real-acceptance-20261011-青岚山河卷-yaml-diagnostics-r2`.

The dated checkpoint below is historical. Its Phase 2 lock is superseded by the current Product Convergence authorization. Preserve the untracked local `cleanup_manifest.txt` and `tools/probes/anim-convert/packages/`.

## Historical checkpoint state

- Repository: C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper
- Branch: main; starting HEAD: 4bcaba2399eb3e3f72741aebd0c40e6d06ee34aa, matched origin/main before this checkpoint.
- Phase 1 full Rust port remains OPEN. Phase 2 remains LOCKED. Do not open, hash, identify, parse, or export user VRChat bundles, and do not read or run UABEA until the Phase 1 gate passes.
- Reference baseline: local AssetRipper 2.0.0 snapshot, 1,449 files; manifest is docs/baseline-files.sha256 (SHA-256 29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9).
- Feature ledger: 386 rows, 363 verified and 23 non-verified. This work does not close a broad parity row.

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
