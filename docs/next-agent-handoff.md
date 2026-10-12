# RustAssetRipper hand-off — 2026-10-10

## Active stage (2026-10-11)

Product Convergence mode is active under the user's current task attachment. Local 用户资源格式/VRCA acceptance is authorized. Frozen AssetRipper source remains the behavioral reference. Unity Editor runtime validation is unavailable in this environment.

## Current acceptance and blocker

- Primary acceptance sample: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`). Latest Rust export: `target/real-acceptance-20261011-avtr青岚古境-avatar-mask-editor-r1`; 8,547 objects, 1,536 asset files, 601,755,682 asset bytes. Frozen source export: `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`.
- Latest root-prefab audit pairs all 4,491 documents by class ID/local fileID. GameObject (1,828), Transform (1,828), MeshRenderer (100), MeshFilter (100), AudioSource (27), ParticleSystem (90), and the other compared root classes match after project GUID/path normalization; 135 and 2,184 curve tangentMode values match. The class-319 `vrc_HandsOnly(Clone)` mask now includes the four editor base fields and its root-prefab pointer resolves to the emitted `.meta`. One AvatarDescriptor pointer still uses different menu output paths (`MonoBehaviour_56` vs `MonoBehaviour_46`), but its recursive graph matches across 65 MonoBehaviour YAML assets, 219 pixel-exact PNGs, and one byte-exact script asset. Reports: `target/real-acceptance-20261011-avtr青岚古境-avatar-mask-editor-r1-prefab-audit.json` and `target/real-acceptance-20261011-avtr青岚古境-expression-menu-semantic-summary.json`.
- The broad YAML inventory still does not scan `.state` or `.blendtree`. Source has 13 standalone `.state` and 41 `.blendtree` assets which Rust embeds in controller documents. The new `tools/compare-controller-blendtrees.ps1` compares every recursive tree graph: 174/174 documents, 156/156 unique semantic graphs, 631/631 child records, zero differences. Report: `target/real-acceptance-20261011-avtr青岚古境-blendtree-graph-r1.json`.
- A large Mesh index buffer previously exhausted the StreamingInfo scanner's node limit by scheduling scalar leaves. The scanner now budgets only structs and arrays; focused resource tests pass 20/20. Latest real-sample inspection reports 552 candidates, 436 Bundle ranges, zero scan errors, and `complete=true` after 5,020,123 structural values. Fresh export and comparison results are recorded in the newest validation-log entry; 19 inspect-mode refs are Unity builtin meshes whose exported source/Rust GUID and fileID pointers match.
- Latest additional real-sample comparison: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `d923d39e1a67d9cfc2d67baacd985f756400b639b2489efa4b290241708d70b5`). Rust r6 is `target/real-acceptance-20261012-avtr松月竹庭-r6`; frozen source export is `target/real-acceptance-20261012-avtr松月竹庭-source/ExportedProject`. A predefined `Default-Material` cache miss was fixed using the source 2022.3.11f1 `(Material.Name, Shader.Name)` identity. The Cube renderer pointer now matches source at builtin-extra PathID 10303/FGUID, and the standalone Rust `.mat` is gone. Reports record 114/114 matching input object rows and 1788/1788 YAML documents; the whole-project comparison still has 4/5 path-count differences and capped reference/null-pointer difference lists. The focused controller report has 3/3 BlendTrees, 37/37 child records, zero semantic differences.
- Additional avatar sample: 青岚雅苑` (58,150,845 bytes; SHA-256 `eb0113f5b26d2649dbacd9766dadd66db804e713b1970f7a0707c370ab6785c6`). Fresh Rust export is `target/real-acceptance-20261011-avtr云栖仙府-stream-scan-r1`; frozen source project is `target/real-acceptance-20261011-avtr云栖仙府-source/ExportedProject/ExportedProject`. StreamingInfo scan is complete with zero scan errors, and 6,529/6,529 object payloads decoded. The 22 unresolved graph references all target `DefaultResources` engine assets.
- 云栖仙府 BlendTrees compare 717/717 documents, 339/339 unique recursive graphs, 1,807/1,807 ChildMotion records, with zero semantic differences. The broad project report records 3,312 source vs 2,821 Rust files, 498 source-only and 7 Rust-only paths, 9,956/10,204 YAML documents, and 2,846 matched documents; reference and null-pointer difference lists are capped. The source-only paths include 98 `.state`, 72 `.blendtree`, 76 `.asset`, 2 `.mask`, 1 `.prefab`, and 249 `.meta` files. See `DIFF-REAL-AVTR云栖仙府-PROJECT-LAYOUT-001`.
- The previous class-43 Mesh schema discrepancy is fixed and documented under `DIFF-REAL-AVTR青岚古境-MESH-EDITOR-TREE-001`; commit `29a7a78` is pushed. The class-319 AvatarMask and class-90 Avatar editor-base discrepancies are also fixed for their inspected sample paths.
- Resolved serialized-content difference: in 青岚雅苑`, source `_MainTex` points to builtin fileID 10307 while Rust points to the exported `Default-ParticleSystem` Texture2D. The builtin fileID 10307 and bundle PathID `4988150354445090395` export to identical 3,256-byte PNGs (SHA-256 `368CA2F9059FC43CB327EB7215C512F23E685890B044EACD37629FC5DB587262`). No code change was needed; Unity import/runtime remains unverified. See `DIFF-REAL-AVTR青岚古境-PREDEFINED-TEXTURE-CACHE-001`.
- CURRENT_BLOCKER: In `青岚山河卷`, class-205 LODGroup is the next scene audit. The r2 source/Rust report paired 2,739 class-205 documents and reported all as different; an example showed Rust omitting the four editor base fields, but the complete document and nested `m_LODs` payload have not yet been compared. Determine whether the editor TypeTree route resolves the class-level differences, add focused tests, and replay the full export before classifying it. Other world mismatches remain open; do not treat the sample as accepted.
- Separate runtime acceptance boundary: the unnamed `AutoDresser_Comp` and `AutoDresser_Decomp` layers in 青岚雅苑` remain unverified in Unity. Frozen-source parsing and processing evidence is recorded, but dynamic state sampling is unavailable. Keep that result open; do not claim runtime equivalence. UABEA was not run.
- `青岚山河卷` (SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`) latest full Rust export is `target/real-acceptance-current-青岚山河卷-r5`: 72,350 objects, 1,087 asset files, 242,984,898 asset bytes. Its 12 default-YAML lightmaps were byte-identical before and after the false-positive notice fix. LightProbes, all 3,997 BoxCollider, 629 MeshCollider, and 86 CapsuleCollider documents now match the frozen source (MeshCollider GUIDs normalized through identical `.meta` paths); see `DIFF-REAL-青岚山河卷-LIGHTPROBES-EDITOR-TREE-001`, `DIFF-REAL-青岚山河卷-COLLIDER-EDITOR-TREES-001`, and `DIFF-REAL-青岚山河卷-CAPSULECOLLIDER-EDITOR-TREE-001`. Class 205 LODGroup is next to audit. The whole project and sample remain unaccepted.

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
