# RustAssetRipper hand-off — 2026-10-10

## Active stage (2026-10-11)

Product Convergence mode is active under the user's current task attachment. Local 用户资源格式/VRCA acceptance is authorized. Frozen AssetRipper source remains the behavioral reference. Unity Editor runtime validation is unavailable in this environment.

## Current acceptance and blocker

- Primary acceptance sample: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`). Latest Rust export: `target/real-acceptance-20261011-avtr青岚古境-editor-components-r3`; 8,547 objects, 1,536 asset files, 601,755,555 asset bytes. Frozen source export: `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`.
- The latest root-prefab audit pairs all 4,491 documents by class ID/local fileID. Animator (2), PositionConstraint (2), RotationConstraint (4), AudioSource (27), and ParticleSystem (90) documents now match the frozen source after project GUID/path normalization; 135 and 2,184 curve tangentMode values match. One AvatarDescriptor pointer has different output paths (`MonoBehaviour_56` vs `MonoBehaviour_46`), but its recursive graph matches across 65 MonoBehaviour YAML assets, 219 pixel-exact PNGs, and one byte-exact script asset. This closes the selected root-prefab component/menu graph slice, not full avatar acceptance. Reports: `target/real-acceptance-20261011-avtr青岚古境-editor-components-r3-prefab-document-audit.json` and `target/real-acceptance-20261011-avtr青岚古境-expression-menu-semantic-summary.json`.
- The broad YAML inventory still does not scan `.state` or `.blendtree`. Source has 13 standalone `.state` and 41 `.blendtree` assets which Rust embeds in controller documents. The new `tools/compare-controller-blendtrees.ps1` compares every recursive tree graph: 174/174 documents, 156/156 unique semantic graphs, 631/631 child records, zero differences. Report: `target/real-acceptance-20261011-avtr青岚古境-blendtree-graph-r1.json`.
- A large Mesh index buffer previously exhausted the StreamingInfo scanner's node limit by scheduling scalar leaves. The scanner now budgets only structs and arrays; focused resource tests pass 20/20. Latest real-sample inspection reports 552 candidates, 436 Bundle ranges, zero scan errors, and `complete=true` after 5,020,123 structural values. Fresh export and comparison results are recorded in the newest validation-log entry; 19 inspect-mode refs are Unity builtin meshes whose exported source/Rust GUID and fileID pointers match.
- Latest additional real-sample comparison: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `d923d39e1a67d9cfc2d67baacd985f756400b639b2489efa4b290241708d70b5`). Rust r6 is `target/real-acceptance-20261012-avtr松月竹庭-r6`; frozen source export is `target/real-acceptance-20261012-avtr松月竹庭-source/ExportedProject`. A predefined `Default-Material` cache miss was fixed using the source 2022.3.11f1 `(Material.Name, Shader.Name)` identity. The Cube renderer pointer now matches source at builtin-extra PathID 10303/FGUID, and the standalone Rust `.mat` is gone. Reports record 114/114 matching input object rows and 1788/1788 YAML documents; the whole-project comparison still has 4/5 path-count differences and capped reference/null-pointer difference lists. The focused controller report has 3/3 BlendTrees, 37/37 child records, zero semantic differences.
- Additional avatar sample: 青岚雅苑` (58,150,845 bytes; SHA-256 `eb0113f5b26d2649dbacd9766dadd66db804e713b1970f7a0707c370ab6785c6`). Fresh Rust export is `target/real-acceptance-20261011-avtr云栖仙府-stream-scan-r1`; frozen source project is `target/real-acceptance-20261011-avtr云栖仙府-source/ExportedProject/ExportedProject`. StreamingInfo scan is complete with zero scan errors, and 6,529/6,529 object payloads decoded. The 22 unresolved graph references all target `DefaultResources` engine assets.
- 云栖仙府 BlendTrees compare 717/717 documents, 339/339 unique recursive graphs, 1,807/1,807 ChildMotion records, with zero semantic differences. The broad project report records 3,312 source vs 2,821 Rust files, 498 source-only and 7 Rust-only paths, 9,956/10,204 YAML documents, and 2,846 matched documents; reference and null-pointer difference lists are capped. The source-only paths include 98 `.state`, 72 `.blendtree`, 76 `.asset`, 2 `.mask`, 1 `.prefab`, and 249 `.meta` files. See `DIFF-REAL-AVTR云栖仙府-PROJECT-LAYOUT-001`.
- CURRENT_BLOCKER: 青岚雅苑` still has cross-path AnimatorState, MonoBehaviour, AvatarMask, and generated-prefab output-layout differences that are not semantically matched. The BlendTree graph is reconciled; next action is to build class/fileID/content matching for AnimatorState assets and their transition/reference graph, then continue to the other object families. 云栖仙府 remains unaccepted. The avtr青岚古境 root-prefab component/menu graph slice is reconciled, while its project-wide and Unity runtime checks remain open.
- Separate runtime acceptance boundary: the unnamed `AutoDresser_Comp` and `AutoDresser_Decomp` layers in 青岚雅苑` remain unverified in Unity. Frozen-source parsing and processing evidence is recorded, but dynamic state sampling is unavailable. Keep that result open; do not claim runtime equivalence. UABEA was not run.
- `青岚山河卷` (SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`) exports to 1,087 assets. Its 12 default-YAML lightmaps are class-28 YAML and were byte-identical before and after the false-positive notice fix. Latest replay: `target/real-acceptance-20261011-青岚山河卷-yaml-diagnostics-r2`.

The dated checkpoint below is historical. Its Phase 2 lock is superseded by the current Product Convergence authorization. Preserve the untracked local `cleanup_manifest.txt` and `tools/probes/anim-convert/packages/`.

## Historical checkpoint state

- Repository: C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper
- Branch: main; starting HEAD: 4bcaba2399eb3e3f72741aebd0c40e6d06ee34aa, matched origin/main before this checkpoint.
- Phase 1 full Rust port remains OPEN. Phase 2 remains LOCKED. Do not open, hash, identify, parse, or export user VRChat bundles, and do not read or run UABEA until the Phase 1 gate passes.
- Reference baseline: local AssetRipper 2.0.0 snapshot, 1,449 files; manifest is docs/baseline-files.sha256 (SHA-256 29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9).
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
