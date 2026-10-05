# RustAssetRipper handoff — 2026-10-03

This is the current continuation record. Read it before changing code. `docs/continuation.md` contains useful history, but some of its older “next” bullets and cloud hashes are stale; this file supersedes those instructions for the present state. Use `docs/feature-parity.md` for the evolving source-to-Rust ledger and `docs/validation-log.md` for per-increment evidence.

## 1. Goal prompt and actual project objective

The Goal tool currently records the startup objective as “pasted text file … Read this file before continuing.” The project objective supplied by the user in the long `AGENTS.md` instructions is the governing task behind that prompt:

1. Port the complete feature set of the local AssetRipper source snapshot to an independently working Rust product.
2. Only after the complete Phase 1 parity gate passes, inspect, compare, validate, and repair **all** user-provided VRChat AssetBundles in Phase 2, using Rust AssetRipper, the original AssetRipper, and UABEA at the levels each actually supports.
3. Prove object/reference, scene/Prefab, animation, asset, and Unity project behavior; close or explicitly leave unresolved every material difference. A CLI demo, a handful of supported formats, or successful file creation is not completion.

This is intentionally a full migration goal, not an MVP. Do not narrow the target because it is large. Do not announce completion while any required gate item remains open. The user’s latest explicit pause request applies to this session; the project goal itself remains unfinished and should resume from this handoff when the user asks.

## 2. Hard operating rules

- **Write scope:** only `C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper` and its authorized remote `https://github.com/cocokoishi/RustAssetRipper.git` are project write targets. Do not edit, push to, open a PR against, or otherwise interfere with anyone else’s repository. The sibling reference `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper` is read-only.
- **Cloud checkpoints:** the user explicitly asked for ongoing pushes. After each coherent, locally validated implementation increment, commit and non-force push it to this project’s `main`; verify the live `origin/main` ref and fetched tree match the pushed local tree. Check the live ref before every push. If remote history has moved, inspect and reconcile safely; never force-push, reset, clean, or overwrite user work.
- **Stage gate:** the current stage is Phase 1. Until its complete gate passes, do not read, hash, identify, scan, parse, or open user bundle contents, and do not read or run UABEA. Root sample names had previously been recorded for location awareness only; they are not an input manifest and must not be treated as inspected samples. The UABEA directory was identified but its contents were not read.
- **Protect inputs:** never modify or delete original AssetBundles. The user also warned that the original AssetRipper export path can delete files in a non-empty output directory. Do not run it against user data; any later reference export must use a newly created, verified empty isolated directory.
- **Evidence:** distinguish synthetic-only verification from Unity-produced input, original-runtime differential, and dynamic Unity validation. “Not run” is not “passed.” Keep matrix states truthful and do not mark a feature verified merely because code exists.
- **Secrets:** a prior chat attachment included a plaintext access token. Its value is deliberately not reproduced here. Do not retrieve, quote, reuse, commit, or log it. The user was advised to revoke/rotate it; rely only on the repo’s already configured authentication, without exposing credentials.
- No subagents were requested. Continue directly unless the user later asks for delegation.

## 3. Frozen reference and current repository

| Item | Recorded identity / status |
|---|---|
| Rust repository | `C:\Users\koishi\Desktop\RustAssetripper\RustAssetRipper` |
| Branch / only authorized remote | `main` / `origin` → `https://github.com/cocokoishi/RustAssetRipper.git` |
| Reference source | `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper` (read-only, no Git metadata in this snapshot) |
| Frozen baseline | AssetRipper source version prefix `2.0.0`, target `net10.0`; 51 solution projects, including 8 test projects and 6 source-generator projects; 1,449 files in the frozen checksum manifest |
| Baseline manifest | `docs/baseline-files.sha256`; manifest SHA-256 `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`; latest verification found 1,449 unchanged files |
| Upstream version claim | README says Unity 3.5 through 6000.4.X; this is the source’s claim, not independent compatibility evidence |
| Reference build status | `dotnet` was unavailable on PATH when checked; original AssetRipper build/tests and runtime differential are therefore still unrun. Do not imply otherwise. |
| Rust toolchain | Rust/Cargo 1.98.1 on Windows (stable MSVC); binaries are under `C:\Users\koishi\.cargo\bin`, not on PATH. Manifest declares Rust 1.87; 1.87 has not been tested. |
| Rust package | `assetripper-rs` 0.1.0, edition 2021, `rust-version = 1.87`; `Cargo.lock` is checked in. |
| Generated source dependency | Baseline project metadata references `AssetRipper.SourceGenerated` 1.4.0. Its generated class sources were not present in the frozen source snapshot or local NuGet cache during the earlier investigation; exact package/source-generator behavior still needs tracing. |

The source identity cannot be tied to an upstream Git commit, branch, tag, or submodule revision because the local snapshot has no `.git`. The reproducible identity is the local path plus the complete checksum manifest. Keep verifying it with `tools/verify-baseline.ps1`.

## 4. User’s full goal versus `AGENTS.md` plan

The long `AGENTS.md` is substantially more specific than a generic migration ticket. It makes full parity and all-sample validation explicit gates. The following comparison is the honest state as of this handoff.

| Goal / AGENTS plan | Required evidence | Current state | Difference / next direction |
|---|---|---|---|
| Complete independent Rust product | Every baseline behavior, version/platform branch, resource path, export format, config, and user workflow implemented without delegating core execution to C#/.NET or another full parser | **Not met.** Rust has substantial binary, container, SerializedFile, TypeTree, workspace, reference, processing, and YAML groundwork. It remains a partial inspection/export implementation. | Complete the source/registration/option/exporter/UI audit and implement all required branches; do not call partial parsers “full support.” |
| Phase 1 source freeze and traceability | Reproducible baseline identity; source symbol → behavior → Rust mapping → independent fixture → result | Baseline identity and 209 feature-ledger IDs are recorded. Audit coverage is not exhaustive; reference .NET build/tests have not run. | Continue splitting ledger rows by real source branches, registrations, options, asset classes, and UI workflows. Preserve “unverified” where evidence is missing. |
| Phase 1 input, format, and type support | All baseline input forms, containers/codecs, versions/platform layouts, TypeTrees, missing-TypeTree paths, managed references, generated classes | Several common readers and synthetic container/SerializedFile/TypeTree paths are implemented. This does not cover all source paths; missing-TypeTree recovery, managed-reference decoding, broad generated class materialization, and some codecs/version branches remain absent or unverified. | Compare every parser/dispatch registration and source generator. Build fixtures independent of the Rust decoder and obtain original-runtime comparisons when the .NET environment is available. |
| Phase 1 object graph and processing | Fully constructed typed objects, files/bundles/dependencies, resolved references, scene and Prefab reconstruction, generated object rules | TypeTree value decoding, PPtr graphs, workspace indexes, and several bounded Scene/Prefab planning/YAML paths exist. These are not complete generated AssetRipper object graphs or complete hierarchy/export processing. | Finish source-order and dependency semantics, generated asset identities, per-class construction, unresolved reference classification, and all scene/Prefab paths. |
| Phase 1 resources and project output | Every baseline asset processor/exporter, correct formats and mappings, `.meta`/GUID/importer/path behavior, usable Unity project | Generic YAML primitives and partial Scene/Prefab output exist. Meta envelope and a newly added standalone add-only sidecar writer exist. There is no complete exporter registry/pipeline, no full resource exporter set, no importer factory/GUID routing/folder metadata integration, and no complete project output. | Wire output as a complete, auditable pipeline; implement all baseline importers/exporters and per-resource semantics. Verify against source behavior and Unity import where possible. |
| Phase 1 product layer | All original CLI arguments/configuration/defaults plus GUI/web/service workflows, settings, localization, progress/cancel/error paths, packaging | A narrow inspection CLI exists. Full CLI parity, persistent settings/config, GUI/web interaction, batch UX, cancellation/progress, and original deployment workflows are not implemented. | Audit every entry point in the 51-project baseline and provide equivalent Rust workflows; record platform-specific branches. |
| Phase 1 correctness and testing | Binary/layout, generated controlled Unity projects, independent expected results, original baseline differential, errors/fuzz/limits, real entry-point tests | Current suite is synthetic and source-derived. The entire Rust workspace passes current tests and static checks. No Unity-produced file or AssetRipper runtime differential has run. | Add independent fixtures and cross-tool/runtime differential. Passing self-generated fixtures does not close source parity. Build/install .NET in an isolated environment or record the specific external blocker without changing the reference tree. |
| Phase 1 gate | All required parity entries verified, no placeholder/silent drop, independent build/run/docs, integrity report | **Not passed; Phase 2 is locked.** | Do not begin Stage 2 early. Produce the required completeness audit and evidence report only after every required item is closed. |
| Phase 2 all user samples | Immutable complete sample manifest and Rust/original/UABEA comparisons after Phase 1; full references, assets, scenes, animation, and runtime validation | **Not started by design.** No sample bytes were opened, identified, or hashed; UABEA was not read or run. | Only unlock after explicit Phase 1 gate evidence. Then inventory every provided input, freeze hashes, compare tools at their genuine common capability level, and report every unresolved difference. |
| Final goal | Both phases complete; every sample passes and no material unresolved differences | **Not met. The project is unfinished.** | Resume Phase 1 only. User asked to pause the goal now, not to change its completion criteria. |

### Relevant `AGENTS.md` sections

- Sections 1, 3, 10, and 22 define the full-product scope and hard two-phase gate. They are **not** satisfied by the current code or test count.
- Sections 2 and 4 protect inputs and freeze the reference. The baseline manifest exists and its verifier passes. Keep all writes in this repo; reference files remain read-only. Full baseline scope (registrations, options, generators, tests, licenses, GUI) remains under audit.
- Section 5 requires a feature matrix with traceable, fine-grained entries and truthful statuses. There are currently 209 stable IDs before this handoff’s sidecar row (210 after update), but that count is not a completeness claim. The matrix itself explicitly says its rows are initial scopes that must be split further.
- Sections 6–8 demand all baseline input/types, graph/processing, resource exporters and product workflows; no wrappers, placeholder success, or silent loss. The implemented primitives are partial. A full sweep for every prohibited pattern and every unimplemented baseline branch remains outstanding.
- Sections 9 and 18 require independent evidence and regression discipline. Current proof is synthetic/source-derived; no original-runtime differential or user-sample result exists.
- Sections 11–19 define Stage 2 inventory, three-way comparisons, semantic/reference/animation/resource checks, and reproducible per-sample reporting. None may run until the Phase 1 gate passes.
- Sections 20–21 require measured stability/performance and a current handoff. This file plus the updated continuation pointer is the present handoff; broad performance and resilience measurements remain open.

## 5. What exists in Rust now (partial, not parity)

The worktree has significant incremental work. Use the source ledger for exact IDs, limits, and evidence. High-level inventory:

- Checked endian-aware binary readers, bounded stream/range composition, virtual file storage, file import/mounting groundwork, and synthetic dispatch/mount tests for several WebData/UnityFS/UnityRaw/UnityWeb paths.
- Partial UnityFS/Raw/Web/WebData container decoding and selected GZip/Brotli/LZ4/LZMA/Zstandard paths. UnityArchive and LZHAM are identified as unsupported by the frozen source baseline; do not invent features the source itself lacks.
- SerializedFile header/metadata/object-range parsing, common legacy TypeTree records, TypeTree-driven value decoding with bounds, and explicit per-object decode diagnostics. This is not full no-TypeTree recovery or generated Unity class materialization.
- Workspace collection bookkeeping and TypeTree PPtr indexing/resolution with explicit unresolved states; processing planners for AssetBundle and ResourceManager original paths; selected Scene/Prefab hierarchy planning and YAML writers. End-to-end completeness is still limited.
- Unity YAML and `.meta` envelope serializers, plus the current standalone filesystem sidecar primitive described below.
- A narrow CLI can inspect selected inputs and report inventory/decoded information. It does not provide the complete original application experience.

### Current uncommitted implementation increment (must be cloud-synced before declaring this handoff done)

`src/export/project_output.rs` and one export module registration were added. `write_unity_meta_sidecar`:

1. Accepts an already-created `UnityMetaFile` and a portable relative asset path.
2. Enforces 32 KiB default relative-path bytes and 256 default components; rejects empty, traversal, rooted/drive, device-name, invalid-character, and duplicate-separator paths.
3. Creates the output root, confines normal directory creation to a canonical root, rejects symlink/non-directory ancestors, then opens exactly the `.meta` file with `create_new` so existing files are never overwritten.
4. Writes the existing bounded `.meta` YAML bytes. It does **not** write the asset, choose/create an importer, allocate or persist GUIDs, create folder sidecars, or connect to a project export/CLI workflow.

Four tests cover successful add-only output and preservation of a sibling file, existing-sidecar collision without overwrite, unsafe-path rejection before output, and path-size/component limits. The first Windows run found only a test expectation comparing an extended canonical path with a non-canonical path; the assertion now canonicalizes its expected root and all four focused tests pass. A first Clippy run also found a needless explicit lifetime; it was removed and the final warnings-denied Clippy run passes.

This helper is not a complete exporter and has not been compared with AssetRipper’s runtime output. Current tests run on Windows only. It is not crash-atomic: an I/O failure after creating a new sidecar could leave that newly created file partial, and ancestor checking is not designed to defeat a concurrent malicious filesystem race. Consider those limits when later wiring it into export tasks; never replace an existing file to “repair” a collision.

## 6. Verification evidence at pause

Commands were run in this repo with binaries under `C:\Users\koishi\.cargo\bin`:

| Command | Exit / result |
|---|---|
| `cargo fmt --all -- --check` | 0 |
| `cargo test --workspace --locked --quiet` | 0; 491 library tests, 23 CLI tests, 0 doc tests; all passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 0 |
| `tools/verify-baseline.ps1` | 0; 1,449 baseline entries unchanged, missing, or unlisted: 0 |
| `git diff --check` | 0 before the final handoff document was written; rerun it after documentation changes and before push |
| AssetRipper `.NET` build/tests or runtime comparison | Not run; `dotnet` was unavailable on PATH in the earlier environment check |
| Unity-produced fixture / Unity Editor import and runtime checks | Not run |
| User VRChat bundle / UABEA inspection | Not run and prohibited before the gate |

The complete Rust test suite is evidence for this implementation snapshot, not evidence that AssetRipper parity is complete. Current implementation test counts also do not test the source reference runtime.

## 7. Git / cloud state and required final sync

Before this increment, local `main` was at `5b95e616d36bbf4020576e697fa59f89a2d6b82f` and remote `main` at `1ff88beb73b4b634cdc6a957999cf844bbaa7467`; both had tree `4984d90072e592c786dba30e4bbe07987eef3bfc`, but their histories had diverged. `git merge-tree --write-tree HEAD origin/main` previewed the exact same tree with no conflicts. A normal local merge commit `090d35a71b140ccaba0df974a3e19c1d714641f3` reconciled ancestry without changing that tree.

The implementation and first version of this handoff were committed as `b786f331b5947929386b11314bd09fefe5eb2d35`, tree `4ec2b2011e43de6bd2940dca35e01b82240ba0de`. Before pushing, `ls-remote`/`fetch` confirmed remote `1ff88beb...` was an ancestor. A normal fast-forward `git push origin main:main` succeeded. Afterward, `ls-remote` returned `b786f331...`; fetch returned the same commit and tree `4ec2b201...`; `git diff --exit-code HEAD FETCH_HEAD` exited 0. This code+handoff checkpoint is live on the authorized remote.

The cloud evidence above was recorded in a documentation-only follow-up to this handoff and `docs/validation-log.md`; that follow-up is pushed and verified as part of the current remote `main`. Since a commit cannot contain its own ID, use `git log -1` plus a live ref/fetched-tree check to identify the exact documentation revision.

For future increments: check `git status --short`, `git diff --check`, and the exact diff; stage only the intended source/docs (never `target/`, sample data, reference sources, credentials, or unrelated files); check the live ref using `git ls-remote origin refs/heads/main` and `git fetch origin main`; reconcile unexpected ancestry movement safely; then commit, normal-push, fetch, and compare local/fetched trees. Never say “pushed” unless the live ref and fetched tree match. A commit cannot store its own hash in its tree; identify the final handoff revision from `git log` in the user-facing update.

## 8. Concrete resume plan for the next AI

Resume Phase 1 only, in this order. Do not restart the whole project plan or jump to user samples.

1. **Re-open state safely:** read this file, `docs/continuation.md`, the last sections of `docs/validation-log.md`, and the latest rows of `docs/feature-parity.md`. Check the current branch, `git status`, and the live remote tree. If the final cloud checkpoint recorded below is present and the tree matches, start cleanly from it.
2. **Finish `.meta` source tracing and connect output:** continue from `EXP-META-ENVELOPE-001` and `EXP-META-SIDECAR-FS-001`. Trace all actual source `CreateImporter` implementations and class-specific importer settings; the baseline reaches generated-source/package behavior that is not directly in the snapshot. Do not guess importer defaults. Identify source GUID allocation/persistence, export paths, collision rules, folder metadata, and write order. Then create an export-owned metadata/output plan and connect safe sidecar creation to the project pipeline without overwrites. Add source-derived and independent tests; mark only the exact tested slice verified.
3. **Audit whole exporter registration and resource matrix:** inventory every baseline exporter/processor and all type/version/platform dispatch paths from the frozen source. Split ledger rows by real behavior. Implement and validate general mechanisms, then the per-resource exporters; do not stop at Scene, Prefab, or YAML. Preserve raw/unknown data without claiming a structured exporter exists.
4. **Continue object model and product workflows:** finish generated type/object creation, dependency and reference semantics, all remaining Scene/Prefab behaviors, project layout, original CLI options/configuration, GUI/web workflows, settings, progress/cancel/error paths, and build/deployment entry points as source audit reveals. Keep Rust core independent of AssetRipper/UABEA/.NET at runtime.
5. **Close the Phase 1 evidence gap:** independently generated or otherwise independently specified fixtures, source/runtime differential when a safe .NET environment is available, malformed-input/resource-limit/error tests, all entry points, and a machine-readable completeness report. Keep .NET and Unity runtime checks marked unrun or blocked until actually performed. Never convert inability to run a required check into a pass.
6. **Only after every Phase 1 criterion is evidenced** may the owner unlock Phase 2. Then build the immutable complete sample manifest before parsing, compare Rust/original/UABEA only where each genuinely supports it, and complete the reference, scene, resource, animation, import/runtime, and per-sample reports described in AGENTS.md. If Phase 1 is incomplete, Phase 2 remains closed.
7. **For each coherent increment:** update the feature ledger and validation log, run the relevant required checks, protect user changes, commit, push, and verify the remote tree. Keep this handoff/continuation current at each pause.

## 9. Honest status

**The project is not complete. Phase 1 is active and its gate has not passed. Phase 2 has not started.** The next AI should treat this file as a detailed checkpoint, not as approval to weaken the goal or inspect user bundles early.


## 10. Checkpoint: mesh decode path (2026-10-04)

- The `CompressedMesh` decoder is complete and runtime-verified against `AssetRipper.SourceGenerated.Extensions` in `src/resources/compressed_mesh.rs`: geometry channels (`dd...`/`659bbf3`), UV channels and bind poses (`d4c6527`), and color channels (`ae6ed43`).
- Added `src/resources/vertex_data_blob.rs` (`ae...`; the next commit): the full `VertexDataBlob` create/parse path plus the `MeshHelper` format conversions. A committed fixture (`src/resources/vertex_data_blob_fixture.txt`) captured from the reference runtime pins channels, streams, and bytes for six versions under both endians; the Rust test matches them bit-for-bit and round-trips `ReadData`.
- Not yet ported on this path: skin-stream writing in `Create` (source `//todo`), `VertexDataExtensions.GenerateVertices`/`GenerateSkin`/`GetChannel`, and bind-pose/`MeshData` index-buffer handling.
- State at this checkpoint: 712 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the Phase 1 gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 11. Checkpoint: shader channel/format layer (2026-10-04)

- Added `src/resources/shader_channel.rs`: the `ShaderChannel`/`ShaderChannel4`/`ShaderChannel5`/`ShaderChannel2018` mappings and the `VertexChannelFormat`/`VertexFormat2017`/`VertexFormat2019`/shader-`VertexFormat` encodings, with `ChannelInfoExtensions.GetVertexFormat`/`GetStride`. `src/resources/shader_channel_fixture.txt` captured 426 rows from the reference runtime across six versions and is matched row-for-row.
- This is the prerequisite for `VertexDataExtensions.GetChannel`/`GenerateVertices`/`GenerateSkin`, which are the next mesh-path pieces and remain unported. `GenerateVertices`/`GenerateSkin` need an `IVertexData`-like view and a submesh view; the generated `IVertexData` classes are not in the snapshot, so those will be unit-tested from constructed channel/stream/data layouts rather than a direct class probe.
- State: 713 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 12. Checkpoint: VertexDataExtensions (2026-10-04)

- Added `src/resources/vertex_data_extensions.rs`: `GetStreams`/`GetStreamsInvariant`, `GetCurrentChannels`/`SetCurrentChannels`, `GetChannel` (both the explicit-channel and old fixed-stream layouts), `GetStreamStride`/`Size`/`Offset`, `GenerateVertices`, and `GenerateSkin`. A real `VertexData.Create` differential captured 140 rows across six versions as `src/resources/vertex_data_extensions_fixture.txt`.
- This completes the vertex-data read path used by `MeshData.TryMakeFromMesh` and `SpriteMetaDataExtensions`. Still unported on this path: `SubMeshData`/mesh index buffer, bind-pose plumbing, and the actual mesh/sprite exporter wiring.
- State: 715 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 13. Checkpoint: sub-mesh data (2026-10-04)

- Added `src/resources/sub_mesh_data.rs`: `SubMeshData.Create`/`CopyTo`, `SubMeshExtensions` topology and first-index helpers, `IndexFormatExtensions.Size`, `MeshData.IndexFormat`, the `MeshTopology` enum, and `Bounds.CalculateFromVertexArray`. A real `SubMesh.Create` differential captured 28 rows across six versions as `src/resources/sub_mesh_data_fixture.txt`.
- Still unported on the mesh path: `MeshExtensions.GetProcessedIndexBuffer`/`SetProcessedIndexBuffer` (index-buffer byte/endian handling), `CalculateMeshMetric`, `GetChannelsData`, and the mesh exporter wiring. These can reuse `SubMeshData` and `VertexDataBlob`.
- State: 717 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 14. Checkpoint: animation tangent modes (2026-10-04)

- Added `src/resources/tangent_mode.rs`: the `TangentMode`/`TangentMode2`/`TangentMode5` mapping with the 5.5 branch. A real runtime differential captured all 256 values as `src/resources/tangent_mode_fixture.txt`.
- Next on the animation path: `KeyframeExtensions.Float.SetValues` and the `AnimationCurveExtensions` `SetValues` overloads (the probe `Temp/opencode/animprobe` already captured keyframe/curve results; the `InWeight`/`OutWeight` fields are version-gated and need a precise presence gate before porting), then `FloatCurve`/`AnimationClip` curve decoding.
- State: 719 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 15. Checkpoint: animation keyframe defaults (2026-10-04)

- Added `src/resources/keyframe.rs`: `KeyframeExtensions.Float` `SetValues` (weight and slope overloads), `DefaultFloatWeight`, and the 2018.1 `InWeight`/`OutWeight`/`WeightedMode` presence gate. A real `Keyframe_Single.Create` differential captured eleven versions as `src/resources/keyframe_fixture.txt`.
- Next on the animation path: the `AnimationCurveExtensions` `SetValues` overloads (the `animprobe` capture shows `PreInfinity`/`PostInfinity` always present and `RotationOrder` gated around 5.4), then `FloatCurve`/`AnimationClip` curve decoding and the `QuaternionCurve`/`Vector3Curve` variants.
- State: 720 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 16. Checkpoint: animation curve construction (2026-10-04)

- Added `src/resources/animation_curve.rs`: the `AnimationCurveExtensions` single-float overloads (default loop/rotation envelope, default-value, two-value, slope builders) and the `CurveLoopTypes`/`RotationOrder` enum values. A real `AnimationCurve_Single.Create` differential captured eight versions as `src/resources/animation_curve_fixture.txt`.
- Next on the animation path: the `Vector3f`/`Quaternionf` curve variants, the keyframe-list overloads, then the `FloatCurve`/`Vector3Curve`/`QuaternionCurve`/`PPtrCurve` bindings and `AnimationClip` clip decoding (`DenseClip`/`StreamedClip`/`ConstantClip`/compressed curves) and `MuscleClipInfo`.
- State: 721 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 17. Checkpoint: Vector3f keyframes and vector extensions (2026-10-04)

- Added `src/resources/keyframe_vector3.rs` and `src/resources/vector_extensions.rs`: the `KeyframeExtensions.Vector3f` weight/slope overloads, the 2018.1 field gate, and the `Vector3f`/`Quaternionf` scale/copy/set/cast helpers. A real `Keyframe_Vector3f.Create` differential captured eight versions as `src/resources/keyframe_vector3_fixture.txt`.
- Next on the animation path: the `Quaternionf` keyframe variant (including the scalar-weight fill and the quaternion `SetValues`), the `Vector3f`/`Quaternionf` curve builders, then the `FloatCurve`/`Vector3Curve`/`QuaternionCurve`/`PPtrCurve` bindings and `AnimationClip` clip decoding.
- State: 723 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 18. Checkpoint: Quaternionf keyframes (2026-10-04)

- Added `src/resources/keyframe_quaternion.rs`: all six `KeyframeExtensions.Quaternionf` overloads and the 2018.1 weight gate. A real `Keyframe_Quaternionf.Create` differential captured eight versions as `src/resources/keyframe_quaternion_fixture.txt`.
- Next on the animation path: the `Vector3f`/`Quaternionf` curve builders, then the `FloatCurve`/`Vector3Curve`/`QuaternionCurve`/`PPtrCurve` binding paths and `AnimationClip` clip decoding (`DenseClip`/`StreamedClip`/`ConstantClip`/compressed curves) plus `MuscleClipInfo`/`ControllerConstant` if the exporter needs it.
- State: 724 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 19. Checkpoint: compressed animation curve unpack (2026-10-04)

- Added `src/resources/compressed_animation_curve.rs`: `CompressedAnimationCurveExtensions.Unpack` composing the packed vectors with the quaternion keyframe slopes overload. A real `CompressedAnimationCurve` differential captured four versions as `src/resources/compressed_animation_curve_fixture.txt`.
- Next on the animation path: the compressed `Vector3f`/single curve unpackers; `DenseClip`/`StreamedClip`/`ConstantClip` decoding; then the `FloatCurve`/`Vector3Curve`/`QuaternionCurve`/`PPtrCurve` binding list and `AnimationClip`/`Avatar`/`AnimatorController` assembly.
- State: 725 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 20. Checkpoint: MuscleClipInfo initialization (2026-10-04)

- Added `src/resources/muscle_clip_info.rs`: both `MuscleClipInfoExtensions.Initialize` overloads. A real `MuscleClipInfo`/`MuscleClipConstant` differential captured five versions as `src/resources/muscle_clip_info_fixture.txt`.
- Next on the animation path: the `AnimationClip` clip lists (`DenseClip`/`StreamedClip`/`ConstantClip` predicates and `SampleArray`/`Data` decoding), the binding curve list (`FloatCurve`/`Vector3Curve`/`QuaternionCurve`/`PPtrCurve`), then `Avatar`/`AnimatorController` assembly.
- State: 726 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 21. Checkpoint: blend tree node accessors (2026-10-04)

- Added `src/resources/blend_tree_node.rs`: the `BlendTreeNodeConstantExtensions` accessors with version- and blend-type dispatch. A real `BlendTreeNodeConstant.Create` differential captured five versions and three blend types as `src/resources/blend_tree_node_fixture.txt`.
- Next on the animator path: `TransitionConstantExtensions` (exit time, has-exit-time, interruption source, fixed duration) and `StateConstantExtensions` (id/name resolution, blend-tree detection), then the `ControllerConstant` layer lookups and the `Avatar`/`AnimatorController` assembly.
- State: 727 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 22. Checkpoint: transition constants (2026-10-04)

- Added `src/resources/transition_constant.rs`: the `TransitionConstantExtensions` accessors with the old-versus-new layout fallbacks. A real `TransitionConstant.Create` differential captured six versions as `src/resources/transition_constant_fixture.txt`.
- Next on the animator path: `StateConstantExtensions` (id/name resolution, blend-tree detection) and the `ControllerConstant` layer lookups, then the `Avatar` assembly.
- State: 728 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 23. Checkpoint: state constants (2026-10-04)

- Added `src/resources/state_constant.rs`: the `StateConstantExtensions` id/name/write-default/blend-tree resolution. A real `StateConstant.Create` differential captured seven versions as `src/resources/state_constant_fixture.txt`.
- Next on the animator path: the `ControllerConstant` layer lookups and the `Avatar`/`AvatarConstant` assembly, then the `AnimationClip` binding curve list.
- State: 729 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 24. Checkpoint: controller constants (2026-10-04)

- Added `src/resources/controller_constant.rs`: the `ControllerConstantExtensions` layer lookups. A real `ControllerConstant.Create` differential captured two versions as `src/resources/controller_constant_fixture.txt`.
- Next on the animator path: the `Avatar`/`AvatarConstant` assembly and the `AnimationClip` binding curve list, then the `AnimatorController` export path.
- State: 730 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 25. Checkpoint: blend-tree nodes, state-machine counts, enum casts (2026-10-04)

- Added `src/resources/blend_tree_node_constant.rs`, `src/resources/state_machine_constant.rs`, and `src/resources/animator_enums.rs`, each with a committed original-runtime differential fixture.
- Next on the animator path: the `Avatar`/`AvatarConstant` assembly and `AvatarExtensions.FindBonePath`, then the `AnimationClip` binding curve list and the `AnimatorController` state-machine/state assembly.
- State: 733 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 26. Checkpoint: avatar constants and bone paths (2026-10-04)

- Added `src/resources/avatar_constant.rs` and `src/resources/avatar.rs`, each with a committed original-runtime differential fixture. The probe also established that generated class assets can be built with `AssetCreator.CreateCollection(version)` + `AssetInfo`, which unblocks future class-level differentials.
- Next on the animator path: the `AnimationClip` binding curve list and the `AnimatorController` state-machine/state assembly.
- State: 735 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 27. Checkpoint: clip extensions (2026-10-04)

- Added the five clip modules (`streamed_clip`, `dense_clip`, `constant_clip`, `clip`, `animation_clip`) with committed original-runtime differentials, including the value-array-to-generic-binding CRC conversion and the 2022 negative-infinity-slope boundary.
- Confirmed a source quirk: `ValueConstant_5_5.TypeID` is inert (no backing field), so the legacy binding conversion degenerates to Animator bindings on 5.5+.
- Next on the animator path: the `AnimatorController` state-machine/state assembly and the `AnimationClip` export path.
- State: 740 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 28. Checkpoint: animation-curve copy overloads (2026-10-04)

- Extended `src/resources/animation_curve.rs` with the keyframe-copy `SetValues` overloads and their target-presence `CopyValues` semantics, differentially verified across mixed versions.
- Next on the animator path: the `QuaternionCurve`/`Vector3Curve`/`FloatCurve`/`PPtrCurve` `SetValues` wrappers, then the `AnimatorController` state-machine/state assembly and the `AnimationClip` export path.
- State: 741 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 29. Checkpoint: curve wrappers (2026-10-04)

- Added `src/resources/curve_wrappers.rs` covering the four curve-wrapper `SetValues` extensions with the envelope reset and script pointer, differentially verified across four versions.
- Next on the animator path: the `AnimatorController` state-machine/state assembly, then the `AnimationClip` export path.
- State: 742 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 30. Checkpoint: PPtr and controller contains-clip (2026-10-04)

- Added `src/resources/pptr.rs` and `src/resources/animator_controller.rs`, covering the `IsAsset`/`IsNull` pointer rule and the three `ContainsAnimationClip` extensions; the collection-registered `ProcessedAssetCollection.CreateAsset` route is now established for pointer-resolving differentials.
- Next on the animator path: the `AnimatorController` state-machine/state assembly and `GetStateBehaviours`, then the `AnimationClip` export path.
- State: 744 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 31. Checkpoint: GameObject active state and transform hierarchy (2026-10-04)

- Added `src/resources/game_object.rs` and `src/resources/transform.rs`, differentially verified for three versions. Confirmed that sampled versions serialize only `IsActive_Boolean`, and that `GetStateBehaviours` is not probeable because `AssetDictionary` keys (`StateKey`) are not publicly mutable.
- Next on the animator path: the `AnimatorController` state-machine/state assembly; the `GetStateBehaviours` "found" branch will need either an internal-collection construction route or a modeled dictionary.
- State: 746 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 32. Checkpoint: UnityGuid.ToString differential and GUID fix (2026-10-04)

- Discovered and fixed a real GUID-formatting defect: `format_unity_guid` emitted reversed strings; the reference `UnityGuid.ToString` nibble-reverses each 32-bit word. Added `guid_fixture.txt` and a runtime differential, and corrected ten affine test expectations.
- This closes the `.meta` GUID-string half of the resume plan; importer-body tracing (`CreateImporter`, class-specific settings, GUID allocation/persistence integration) remains open and has no runtime-differential path yet.
- State: 747 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 33. Checkpoint: FileIdentifier Meta GUID fix (2026-10-04)

- Found the second instance of the GUID reversal in `serialized_file::metadata::FileIdentifier::get_file_path` (the reference `FileIdentifier.GetFilePath` returns `Guid.ToString()`), fixed it to the nibble-reversed form, and corrected the `io::dependency` Meta-identity test.
- Also verified that the sibling `Hash128` format (`HashHelper.ToString`, big-endian per little-endian word) is already correct in `serialized_file::metadata`; it uses the opposite nibble convention from GUID.
- State: 747 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 34. Checkpoint: texture and graphics format tables (2026-10-04)

- Added `src/resources/texture_format.rs` and `src/resources/graphics_format.rs`, each with a full-enum original-runtime differential (91 and 156 rows).
- These feed the texture exporter, which is still unported; the mapping tables are now pinned exactly.
- State: 749 library + 23 CLI tests, warnings-denied Clippy, format check, 1,449-file baseline verifier, and `git diff --check` all pass. Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 35. Checkpoint: pure-logic extensions and GitHub Actions CI (2026-10-04)

- Added the byte-array/bounds/color/matrix/rect modules, each with a committed original-runtime differential fixture.
- Added `.github/workflows/ci.yml` (rustfmt + clippy -D warnings + test on Ubuntu and Windows); compilation and tests now run on GitHub Actions rather than the local machine. The `tools/verify-baseline.ps1` verifier still runs only locally because it needs the read-only sibling reference snapshot.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 36. Checkpoint: vectors, quaternions, transform init, Hash128 (2026-10-04)

- Added the vector/quaternion/transform-init modules and the `Hash128` formatter with its checked-overflow quirk, all CI-verified on Ubuntu and Windows.
- CI is the compile/test gate now; each push runs `rustfmt`, `clippy -D warnings`, and `test` on both operating systems.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 37. Checkpoint: StreamingInfo, GLTextureSettings, MinMaxGradient (2026-10-04)

- Added the streamed-resource, sampler-settings, and particle-gradient modules with committed original-runtime differentials.
- Captured two version branches directly: `WrapMode` (old) vs `WrapU`/`WrapV`/`WrapW` (new), and packed vs float gradient color.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 38. Checkpoint: texture decoders (2026-10-04)

- Added the DXT1/DXT3/DXT5 and packed-format decoders with a committed `AssetRipper.TextureDecoder` differential fixture; this is the first real texture-pixel capability in the Rust port.
- Remaining texture work: BC4/BC5/BC6H/BC7, ATC, ETC/EAC, ASTC, PVRTC, YUY2, half/float/signed/16-bit formats, the `TextureConverter` mip/layer/`GraphicsFormat` dispatch, Crunch, and the PNG/EXR/BMP writers.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 39. Checkpoint: ETC/EAC, ATC/YUY2, and wide texture formats (2026-10-04)

- Added `src/resources/etc_decode.rs` (ETC1 individual/differential, ETC2 individual/differential/T/H/planar, ETC2A1 punch-through, ETC2A8, EAC R/RG unsigned/signed), `src/resources/atc_yuy2.rs` (ATC RGB4/RGBA8, YUY2), and the 16-bit/half/float/signed/RGB9e5 converters in `src/resources/texture_decode.rs`.
- Every module carries a committed original-runtime differential (`AssetRipper.TextureDecoder` 2.6.3 decoded through the shipped DLL): `etc_decode_fixture.txt` (23 rows), `atc_yuy2_fixture.txt` (5 rows), `wide_format_fixture.txt` (19 rows), plus the earlier `texture_decode_fixture.txt` (33 rows).
- Recorded `DIFF-ETC-ALPHAORDER-001` (ETC2A8 alpha uses the reverse block permutation) and `DIFF-ATC-COLOR-001` (ATC `Extend`/`5-3` weights and explicit-alpha branch).
- Texture format coverage now: DXT1/3/5, BC4/BC5, ETC1/ETC2/ETC2A1/ETC2A8, EAC R/RG (u/s), ATC RGB4/RGBA8, YUY2, RGBA32/RGB24/BGRA32/ARGB32/Alpha8/R8/RG16/RGB565/ARGB4444/RGBA4444, and R16/RG32/RGB48/RGBA64/half/float/signed/RGB9e5. Still unported: BC6H, BC7, ASTC, PVRTC, and the `TextureConverter` mip/layer/`GraphicsFormat` dispatch, Crunch, and the image writers.
- Verified the `.meta` envelope (`src/export/meta.rs`) matches the reference `Meta.cs`: `fileFormatVersion: 2`, `guid`, optional `folderAsset`, optional `timeCreated`/`licenseType: Free` with the `SOURCE_DATE_EPOCH` fallback, then the importer mapping. `format_unity_guid_words` delegates to the corrected `format_unity_guid`. The remaining `.meta` work is the FS sidecar integration and the per-class `CreateImporter` tracing.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 40. Checkpoint: BC7 decoder (2026-10-04)

- Added `src/resources/bc7.rs` and `src/resources/bc7_tables.rs` (BC7), with a 16-row committed original-runtime differential (`bc7_fixture.txt`) covering every mode plus the invalid-mode transparent-black path. The large partition tables were mechanically extracted from `Bc7Tables.cs` rather than transcribed.
- Texture coverage now: DXT1/3/5, BC4/BC5, BC7, ETC1/ETC2/ETC2A1/ETC2A8, EAC R/RG (u/s), ATC RGB4/RGBA8, YUY2, and the packed/wide formats. Remaining decoders: BC6H, ASTC, PVRTC.
- Still open beyond decoders: the `TextureConverter` mip/layer/`GraphicsFormat` dispatch, Crunch, and the image writers (PNG/EXR/BMP); the `.meta` FS sidecar integration and per-class `CreateImporter` tracing; the full exporter registration/resource matrix audit; and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 41. Checkpoint: BC6H and PVRTC decoders (2026-10-04)

- Added `src/resources/bc6h.rs`/`bc6h_tables.rs` (BC6H, 19-row differential, tables mechanically extracted) and `src/resources/pvrtc.rs` (PVRTC2/PVRTC4, 4-row differential).
- Recorded `DIFF-BC6H-HALF-001` (the decoded 16-bit values are reinterpreted as IEEE-754 binary16 before output conversion) and `DIFF-PVRTC-BLUE-001` (the reference expands the second colour's blue into colour A's slot).
- Texture decoder coverage is now complete except ASTC: DXT1/3/5, BC4/BC5, BC6H, BC7, ETC1/ETC2/ETC2A1/ETC2A8, EAC R/RG (u/s), ATC RGB4/RGBA8, PVRTC2/4, YUY2, and the packed/wide formats.
- Still open beyond the decoders: ASTC, the `TextureConverter` mip/layer/`GraphicsFormat` dispatch, Crunch, and the image writers; the `.meta` FS sidecar integration and per-class `CreateImporter` tracing; the full exporter registration/resource matrix audit; and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 42. Checkpoint: ASTC decoder (2026-10-05)

- Added `src/resources/astc.rs` and `src/resources/astc_tables.rs` (ASTC 4×4–12×12, void-extent, dual-plane, partitions), with an 8-row committed original-runtime differential (`astc_fixture.txt`). The lookup tables were mechanically extracted from `AstcDecoder.cs`.
- Recorded `DIFF-ASTC-BITTRANSFER-001` (the reference calls `BitTransferSigned(ref v[1], ref v[0])`) and `DIFF-ASTC-SHIFT-001` (C# masks `ulong` shift counts; the Rust port uses wrapping shifts and promotes the bit-reverse table byte to `i32`). Added `TextureDecodeError::InvalidBlockDimension`.
- All block decoders dispatched by `TextureConverter.DecodeTexture` are now ported: DXT1/3/5, BC4/BC5, BC6H, BC7, ETC1/ETC2/ETC2A1/ETC2A8, EAC R/RG (u/s), ATC RGB4/RGBA8, PVRTC2/4, ASTC, YUY2, and the packed/wide formats.
- Next: the `RgbConverter`/`DirectBitmap` plumbing (the generic `TColor`/`TChannelValue` conversion used by `TryConvertToBitmap`), the `TextureConverter` mip/layer/`GraphicsFormat` dispatch, Crunch, and the image writers (PNG/EXR/BMP/TGA); then the `.meta` FS sidecar integration and per-class `CreateImporter` tracing; the full exporter registration/resource matrix audit; and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 43. Checkpoint: TextureConverter dispatch (2026-10-05)

- Added `src/resources/texture_converter.rs` (`TryConvertToBitmap`/`DecodeTexture`: the `TextureFormat` and `GraphicsFormat` switches, the per-layer loop, `FlipY`, and `UnpackNormal`), plus the missing `BGR24`/`ARGBFloat` and `GraphicsFormat`-only decoders in `texture_decode.rs`.
- Differential coverage: `wide_format_fixture.txt` grew to 23 rows (`texprobe3`), and `graphics_format_decode_fixture.txt` has 13 rows (`texprobe4`).
- Crunch is still not ported; crunched formats return `TextureConvertError::CrunchUnsupported`.
- `cargo test --workspace --locked`: 780 library and 23 CLI tests; warnings-denied Clippy passes.
- Next: the `RgbConverter`/`DirectBitmap` pixel-type plumbing and the image writers (PNG/EXR/BMP/TGA/JPEG), then Crunch; then the `.meta` FS sidecar integration and per-class `CreateImporter` tracing; the full exporter registration/resource matrix audit; and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 44. Checkpoint: Texture image writers (2026-10-05)

- Added `src/resources/image_export.rs`: `ImageExportFormat` and `Bitmap::save` for PNG, BMP (byte-exact `BmpWriter` layout), and TGA. EXR/HDR/JPEG return `ImageWriteError::Unsupported`.
- `cargo test --workspace --locked`: 786 library and 23 CLI tests; warnings-denied Clippy passes.
- Next: the `RgbConverter`/`DirectBitmap` pixel-type plumbing, Crunch, and the EXR/HDR/JPEG writers; then the `.meta` FS sidecar integration and per-class `CreateImporter` tracing; the full exporter registration/resource matrix audit; and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 45. Checkpoint: Texture2D view and export (2026-10-05)

- Added `src/resources/texture.rs`: the class-28 `Texture2DView` field reader (`m_Width`/`m_Height`/`m_TextureFormat`/`m_CompleteImageSize`/`m_ImageCount`/`m_MipCount`/`m_LightmapFormat`/`image data`/`m_StreamData`), `ActualImageSize`, inline-over-stream selection, and `export_texture2d_image`/`export_texture2d_value` wiring the decode → flip → normal-unpack → writer pipeline.
- 9 focused tests; `cargo test --workspace --locked`: 795 library and 23 CLI tests; warnings-denied Clippy passes.
- Next: the class-189 `ImageTexture` `GraphicsFormat` route, Xbox360/Switch platform transforms, Crunch, EXR/HDR/JPEG writers, sprite/atlas export, and wiring the texture exporter into a project export; then the `.meta` asset-copied importer fields, the exporter registration/resource matrix audit, and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 46. Checkpoint: CLI texture export (2026-10-05)

- Added the `export-texture` CLI command (`export_texture_to`): mount, workspace load, object decode, `Texture2DView`, inline/streamed data resolution via `resolve_and_read_streamed_resource_info`, and `export_texture2d_image`; the output extension selects PNG/BMP/TGA and an existing output is never replaced.
- Two generated generation-10 SerializedFile CLI-handler tests; `cargo test --workspace --locked`: 795 library and 25 CLI tests; warnings-denied Clippy passes.
- Next: the class-189/3D/array texture routes, Xbox360/Switch platform transforms, Crunch, EXR/HDR/JPEG writers, sprite/atlas export, `.meta` sidecars, and wiring into a project export; then the exporter registration/resource matrix audit and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.
