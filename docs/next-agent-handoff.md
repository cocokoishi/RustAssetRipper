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


## 47. Checkpoint: Terrain heatmap exporter (2026-10-05)

- Added `src/resources/terrain_heatmap.rs`: the `TerrainData` heightmap reader, the five-stop colour ramp, and the `max(dimension, resolution)` flipped RGBA32 bitmap. The `(Color32)ColorFloat` conversion matches the reference's truncating `ConvertFloatToByte`.
- Six synthetic tests; `cargo test --workspace --locked`: 801 library and 25 CLI tests; warnings-denied Clippy passes.
- Next: the class-189/3D/array texture routes, platform transforms, Crunch, EXR/HDR/JPEG writers, sprite/atlas export, `.meta` sidecars, and wiring into a project export; then the exporter registration/resource matrix audit and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 48. Checkpoint: Processed mesh data (2026-10-05)

- Added `compressed_mesh::decompress` (the `CompressedMesh.Decompress` orchestration) and `src/resources/mesh_data.rs` (the processed `MeshData` merging the vertex-data-blob and compressed paths with the source's `??=` fallbacks, plus `UVCount`/`IndexFormat` and the `TryGet*AtIndex` post-processing).
- Nine tests; `cargo test --workspace --locked`: 810 library and 25 CLI tests; warnings-denied Clippy passes.
- Next: the `Mesh` field reader producing the blob and compressed mesh, `BindPose`/`m_Skin` parsing, and the GLB/OBJ mesh exporters; the class-189/3D/array texture routes, platform transforms, Crunch, EXR/HDR/JPEG writers, sprite/atlas export, and `.meta` sidecars; then the exporter registration/resource matrix audit and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 49. Checkpoint: Mesh class-43 field reader (2026-10-05)

- Added `src/resources/mesh.rs`: class-43 `MeshView` field reading, endian-aware `m_IndexBuffer` decoding, and `read_mesh_data` assembling processed `MeshData` from the blob/compressed paths.
- Five synthetic tests; `cargo test --workspace --locked`: 815 library and 25 CLI tests; warnings-denied Clippy passes. The ledger row is `已实现但未验证` because serialized sub-mesh/skin/stream spellings still need a Unity-produced or C# differential.
- Next: validate the mesh field spellings against generated/Unity-produced evidence, then the GLB/OBJ mesh exporters; the class-189/3D/array texture routes, platform transforms, Crunch, EXR/HDR/JPEG writers, sprite/atlas export, and `.meta` sidecars; then the exporter registration/resource matrix audit and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 50. Checkpoint: Mesh generated-API field evidence (2026-10-05)

- Probe `Temp/opencode/meshprops` dumped the shipped `AssetRipper.SourceGenerated.dll` property lists; the 105-line output is committed as `src/resources/mesh_generated_api_fixture.txt` with a regression test pinning every relied-upon property.
- Fixed the stream stride to read `stride` (the `m_Stride` alias was wrong) with storage width from the value kind, and corrected bind-pose fields to `E00..E33`.
- `cargo test --workspace --locked`: 816 library and 25 CLI tests; warnings-denied Clippy passes. Recorded as `DIFF-MESH-GENERATED-API-001` (source-derived, not a serialization differential).
- Next: the GLB/OBJ mesh exporters; the class-189/3D/array texture routes, platform transforms, Crunch, EXR/HDR/JPEG writers, sprite/atlas export, and `.meta` sidecars; then the exporter registration/resource matrix audit and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.


## 51. Checkpoint: GLB mesh export and `export-mesh` CLI (2026-10-05)

- Added `src/resources/glb.rs` with the `GlbMeshType` selection, the coordinate-space and tangent conversions, the sub-mesh topology rewrite, and a self-contained glTF 2.0 GLB writer (JSON plus binary chunk, no external glTF library at runtime).
- Added the `export-mesh` CLI command with three tests, including a synthetic class-43 `Mesh` SerializedFile that produces a verified GLB; 19 module tests cover every topology rewrite, the container layout, and the error paths.
- `cargo test --workspace --locked`: 836 library and 28 CLI tests; warnings-denied Clippy passes. The ledger row is `EXP-GLB-001` at `已实现但未验证`: no Unity-produced input and no SharpGLTF output comparison has run.
- Next: blend shapes (`IBlendShapeData`), `CalculateMeshMetric`, the scene-level `GlbLevelBuilder`/`GlbTerrainBuilder`, streamed mesh bytes; the class-189/3D/array texture routes, platform transforms, Crunch, EXR/HDR/JPEG writers, sprite/atlas export, and `.meta` sidecars; then the exporter registration/resource matrix audit and the object-model/product workflows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 52. Checkpoint: Material object model and CRC28 differential (2026-10-05)

- Added `src/resources/material.rs` with the class-21 `Material` field model, the three `m_TexEnvs` dictionary shapes, and `find_property_name_by_crc28`.
- Two reference probes pinned the evidence: `Temp/opencode/matprops` for the generated property names (`src/resources/material_generated_api_fixture.txt`) and `Temp/opencode/crcprobe` for `Crc28Algorithm` (`src/resources/crc28_fixture.txt`). `RES-MATERIAL-CRC28-001` is now `已验证` on a real runtime differential; `RES-MATERIAL-VIEW-001` is `已实现但未验证`.
- `cargo test --workspace --locked`: 852 library and 28 CLI tests; warnings-denied Clippy passes.
- Next: `RES-MATERIAL-EXPORT-001` (the `.mat`/`.shader` asset exporter and its `ShaderExportMode` branches), then the other uninvestigated rows: `RES-ANIM-001`/`PROC-ANIM-001` (animation clips and controllers), `RES-AUDIO-001` (audio decode), `RES-MODEL-001` (blend shapes, `CalculateMeshMetric`, scene-level GLB), `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, `PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, and `TOOLS-001`.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 53. Checkpoint: Mesh read-path reference-runtime differential (2026-10-05)

- Probe `Temp/opencode/meshread` exercised the real generated `Mesh` + `MeshExtensions.ReadData` + `MeshData.TryMakeFromMesh` path for four Unity versions; the rows are committed as `src/resources/mesh_read_fixture.txt` and `RES-MESH-DATA-001` is now `已验证`.
- `cargo test --workspace --locked`: 853 library and 28 CLI tests; warnings-denied Clippy passes.
- Next: the same differential treatment for skin/bind-pose/blend-shape mesh paths and the class-43 field reader (`RES-MESH-VIEW-001` still `已实现但未验证`); then `RES-MATERIAL-EXPORT-001` (`.mat`/`.shader`), the animation rows (`RES-ANIM-001`/`PROC-ANIM-001`), `RES-AUDIO-001`, `RES-MODEL-001` (blend shapes, `CalculateMeshMetric`, scene-level GLB), `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 54. Checkpoint: Mesh TypeTree gates from the reference Tpk (2026-10-05)

- Committed `src/resources/mesh_typetree_fixture.txt` (real class-43 release TypeTrees for four Unity versions, dumped from the reference `SourceTpk`) and fixed three version-gate defects the trees exposed in `src/resources/mesh.rs`.
- 18 mesh tests pass, including the per-version TypeTree assertions and the Unity 3.5 fixed-stream reader test. `cargo test --workspace --locked`: 857 library and 28 CLI tests; warnings-denied Clippy and format check pass.
- Next: `RES-MATERIAL-EXPORT-001` (`.mat`/`.shader`), the animation rows (`RES-ANIM-001`/`PROC-ANIM-001`), `RES-AUDIO-001`, `RES-MODEL-001` (blend shapes, `CalculateMeshMetric`, scene-level GLB), `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 55. Checkpoint: Scene-GLB material texture selection (2026-10-05)

- Added `MaterialView::glb_material_textures` (`GlbLevelBuilder.GetTextures` name routing) with six tests; new ledger row `RES-MODEL-GLB-MATERIAL-001` at `已实现但未验证`.
- `cargo test --workspace --locked`: 863 library and 28 CLI tests; warnings-denied Clippy passes.
- Next: `RES-MATERIAL-EXPORT-001` (`.mat`/`.shader`), the animation rows, `RES-AUDIO-001`, blend shapes/`CalculateMeshMetric`/scene-level GLB assembly, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 56. Checkpoint: Mesh metric differential + doc-encoding repair (2026-10-05)

- Added `src/resources/mesh_metric.rs` (`TriangleEnumerable`/`CalculateMeshMetric`) with an exact-match runtime differential (`src/resources/mesh_metric_fixture.txt`); resolved the `Fill` stamp version gate (`Mesh_2019` stamps, `Mesh_5` discards). New row `RES-MESH-METRIC-001` at `已验证`.
- Repaired doc corruption from shell heredoc appends (PowerShell backtick escapes and transport mojibake ate backticks/CJK in four appended blocks): truncated and rewrote them via the edit tool; verified zero control bytes and intact CJK/backticks. Rule going forward: docs are written only through the edit/write tools, never shell heredocs.
- `cargo test --workspace --locked`: 874 library and 28 CLI tests; warnings-denied Clippy and format check pass.
- Next: `RES-MATERIAL-EXPORT-001` (`.mat`/`.shader`), the animation rows (`RES-ANIM-001`/`PROC-ANIM-001`), `RES-AUDIO-001`, blend shapes/scene-level GLB assembly, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 57. Checkpoint: Shader text export CLI + material YAML emission (2026-10-05)

- Added `src/resources/shader.rs` (`ShaderView`: `m_Script`/`m_Name` bytes, `m_ParsedForm` presence; byte-level decompiled-text gate agreeing with the `&str` gate) and `export_shader_text` in `src/export/shader_export.rs` (stack order: `Simple` raw bytes under every mode, else `Yaml` errors, else `Dummy`/`Decompile` render or fail explicitly; parsed-form dummy is an explicit `ParsedFormUnsupported`).
- Runtime differential `src/export/shader_dummy_fixture.txt` (probe `Temp/opencode/shaderexport`, real per-version `Shader.Create` + shipped `DummyShaderTextExporter.ExportShader`): six rows matched byte-for-byte, including the version gates (`Shader_3_5`/`Shader_5` script-only, 2019+/2022+ parsed-only) and `Simple`-first precedence. New `export-shader` CLI with `--shader-export-mode` plus a synthetic class-21 `.mat` YAML emission test (`--- !u!21 &1`).
- `RES-MATERIAL-EXPORT-001` moves to `已实现但未验证`; recorded as `DIFF-SHADER-DUMMY-001`.
- `cargo test --workspace --locked`: 881 library and 33 CLI tests; warnings-denied Clippy and format check pass.
- Next: the animation rows (`RES-ANIM-001`/`PROC-ANIM-001`), `RES-AUDIO-001`, parsed-form shader dummy, blend shapes/scene-level GLB assembly, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 58. Checkpoint: Custom-curve attribute tables + regex differential (2026-10-05)

- Added `src/resources/custom_curve.rs` (constraint tables, `Renderer`/`RendererMaterial`/`SpriteRenderer` branches, `BindingCustomType`/`TransformType`, prefixes, regex-free `is_unknown_path`) with a 21-row .NET-regex differential (`src/resources/custom_curve_rpath_fixture.txt`). New row `PROC-ANIM-CUSTOM-CURVE-001` at `已实现但未验证`; `DIFF-CUSTOM-CURVE-REGEX-001` recorded. Scene/checksum branches stay open with explicit errors.
- `cargo test --workspace --locked`: 889 library and 33 CLI tests; warnings-denied Clippy and format check pass.
- Next: `AnimationClipConverter` core (bindings cache, streamed/dense/constant processing), `RES-AUDIO-001`, parsed-form shader dummy, `RES-ANIM-001` remainder, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 59. Checkpoint: Streamed-curve slopes + frame decoding differential (2026-10-05)

- Added `src/resources/streamed_curve.rs` (`StreamedCurveKey` reads + bit-exact `CalculateSlopes`, `StreamedFrame` reads with duplicate collapse and the 2017.1+ alignment gate) with an 11-row runtime differential (`src/resources/streamed_curve_fixture.txt`). New row `PROC-ANIM-STREAMED-001` at `已实现但未验证`; `DIFF-STREAMED-SLOPES-001` recorded. Converter orchestration stays open.
- Survived a mid-turn server restart without loss: worktree state re-verified from evidence (uncommitted module + fixture present, docs untouched), full suite re-run green before documenting.
- `cargo test --workspace --locked`: 893 library and 33 CLI tests; warnings-denied Clippy and format check pass.
- Next: `AnimationClipConverter` orchestration (buffer assembly, `ProcessStreams`/`ProcessDenses`/`ProcessConstant`, bindings cache), `RES-AUDIO-001`, parsed-form shader dummy, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 60. Checkpoint: Converter orchestration math + search/rounding differentials (2026-10-05)

- Added `try_get_next_frame` (6-row reflection differential) to `streamed_curve.rs` and new `clip_convert.rs` (bindings cache, `IsTransform` precedence, `CalculateLastConstantTime`, first-frame fix, dense/constant rules, `DefaultFloatWeight`, 9-row rounding differential). New row `PROC-ANIM-CONVERT-001` at `已实现但未验证`; `DIFF-CLIP-CONVERT-001` recorded. Curve emission stays open.
- `cargo test --workspace --locked`: 899 library and 33 CLI tests; warnings-denied Clippy and format check pass.
- Next: `RES-AUDIO-001`, parsed-form shader dummy, blend shapes, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 61. Checkpoint: AudioClip view + export-audio CLI + decoder differentials (2026-10-05)

- Added `src/resources/audio.rs` (`AudioClipView`, source-order resolution, lazy integrity, numeric `FmodSoundType` table) with a generated-API fixture, `try_decode_audio` with a 4-row `TryDecode` differential, and the `export-audio` CLI (inline + streamed resolution, 7 CLI tests). Fixed the U+00D8 diagnostic-fill deviation. `RES-AUDIO-001` at `已实现但未验证`; `DIFF-AUDIO-DECODE-001` recorded. `FSB5`/Vorbis stay explicit errors.
- `cargo test --workspace --locked`: 908 library and 40 CLI tests; warnings-denied Clippy and format check pass.
- Next: parsed-form shader dummy, blend shapes, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 62. Checkpoint: parsed-form shader dummy with a runtime differential (2026-10-05)

- Added `ParsedShaderProperty`/`ParsedShaderForm` decoding to `src/resources/shader.rs` and `render_parsed_property`/`render_parsed_properties`/`parsed_dummy_body`/`export_parsed_dummy` to `src/export/shader_export.rs`, wired into the `Dummy`/`Decompile` stack. Committed `src/export/shader_parsed_fixture.txt` (three real 2019.4 parsed forms through the shipped exporter) and `src/export/float_tostring_fixture.txt` (28 float renderings). Fixed the `Range(min, max)` spacing the differential exposed. New row `RES-SHADER-PARSED-001`; `DIFF-SHADER-PARSED-001` recorded. Serialized `m_*` names follow the class-48 convention without a local class-48 TypeTree dump.
- `cargo test --workspace --locked`: 910 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37285289510` green (all five jobs) for remote `main` = `be12850`; local matches.
- Next: blend shapes, `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 63. Checkpoint: blend-shape name resolution with a checksum differential (2026-10-05)

- Added `src/resources/blend_shape.rs` (`crc32_ieee`, `is_crc_match`, `find_channel_name_by_crc`, `BlendShapeLookupView` decoding `m_Shapes`/`channels` and `m_ShapesList` with the source precedence). CRC core re-asserts the 12 shipped `Crc32Algorithm` words in `crc28_fixture.txt`; generated surface confirmed by probe `Temp/opencode/blendprops`; serialized names from the committed 5.0 class-43 TypeTree dump. New row `RES-BLENDSHAPE-001`; `DIFF-BLENDSHAPE-001` recorded. `SetValues` stays caller-side.
- `cargo test --workspace --locked`: 918 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37286752472` green (all five jobs) for remote `main` = `298dc13`; local matches.
- Next: `EXPORT-UNITY-001`, `TYPE-SCRIPT-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 64. Checkpoint: script identity and assembly names with a runtime differential (2026-10-05)

- Extended `src/io/special_names.rs` (assembly-name fix/add/remove/project helpers) and added `src/processing/script_identity.rs` (`ScriptIdentifier`, `ScriptingBackend`, full/valid-name kernels). All 49 shipped-DLL probe rows in `src/io/script_identity_fixture.txt` matched, including case-sensitive edges and constructor rejections. New row `TYPE-SCRIPT-IDENTITY-001` (first audited slice of `TYPE-SCRIPT-001`); `DIFF-SCRIPT-IDENTITY-001` recorded. Manager resolutions stay unported.
- `cargo test --workspace --locked`: 924 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37288176690` green (all five jobs) for remote `main` = `42f44c7`; local matches.
- Next: `EXPORT-UNITY-001` orchestration audit, `TYPE-SCRIPT-001` remainder, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 65. Checkpoint: export-settings discriminants and defaults with a runtime differential (2026-10-05)

- Added `discriminant()` consts for all 12 configuration enums in `src/export/settings.rs` and replaced two-variant `ScriptLanguageSelection` with the full 21-member `ScriptLanguageVersion` (including `Latest = 2147483647`). All 83 shipped-assembly rows in `src/export/settings_fixture.txt` (probe `Temp/opencode/settingprobe`) matched, including the 24 record defaults and the capitalized .NET boolean rendering. New row `EXP-EXPORT-SETTINGS-001`; `DIFF-EXPORT-SETTINGS-001` recorded.
- `cargo test --workspace --locked`: 925 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37289460672` green (all five jobs) for remote `main` = `5374d76`; local matches.
- Next: `EXPORT-UNITY-001` orchestration audit, `TYPE-SCRIPT-001` remainder (manager resolutions), and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 66. Checkpoint: project-export lifecycle events and the unhandled-asset abort (2026-10-05)

- Extended `src/export/project_order.rs` with the four `ProjectExporter.Export` lifecycle events, a loop driver reproducing the gate/warning/progress shape, and a fallible collection creator so the no-handler throw aborts planning with `UnhandledAsset`. Source-read over caller facts (no runtime differential applies); three new tests pin the abort, bracketing, and event sequence. New row `EXP-PROJECT-EXPORT-LOOP-001`.
- `cargo test --workspace --locked`: 928 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37290556514` green (all five jobs) for remote `main` = `dc4db32`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), `EXPORT-UNITY-001` remainder (file-writing pipeline), and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 67. Checkpoint: TypeTreeResolver match predicate with a runtime differential (2026-10-05)

- Extended probe `Temp/opencode/scriptidprobe` to drive the internal shipped `TypeTreeResolver` (empty list + matching-but-broken `OldType`), pinning both failure reasons; rows appended to `src/io/script_identity_fixture.txt`. Extended `src/processing/script_identity.rs` with the ordinal three-way match, first-match scan, and verbatim reason literals (default identifiers never match). New row `TYPE-SCRIPT-RESOLVER-001`; `DIFF-SCRIPT-RESOLVER-001` recorded. Node building stays with the TypeTree reader.
- `cargo test --workspace --locked`: 930 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37291680476` green (all five jobs) for remote `main` = `55b1e0e`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), `EXPORT-UNITY-001` remainder (asset/sidecar file writes), and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 68. Checkpoint: export file-name planning with a filesystem differential (2026-10-05)

- Added `src/io/file_names.rs` (suffix cleanup, host-aware invalid-character fixing, reserved names, UTF-8 truncation, both path-splitting rules, `UniqueNamePlanner`, `GetUniqueFileName` composition). All 86 shipped-DLL rows in `src/io/file_names_fixture.txt` (probe `Temp/opencode/filenameprobe`, real temp dirs) matched, fixing the `.`/bare-dot asymmetry and pipe-bearing fixture parsing along the way, plus a cp437 probe-output mojibake fixed via pure-ASCII probe escapes. New row `EXP-FILE-NAME-001`; `DIFF-FILE-NAME-001` recorded.
- `cargo test --workspace --locked`: 935 library and 40 CLI tests; warnings-denied Clippy and format check pass.
- CI: run `37294766135` (for `ec2d9fa`) failed only `test (ubuntu-latest)` on the then-unconditional `con_0.txt` expectation; fixed in `6459798`, whose run `37295745789` is green on all five jobs. Remote `main` = `6459798`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), `EXPORT-UNITY-001` remainder (asset/sidecar file writes), and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 69. Checkpoint: managed type classification with a runtime differential (2026-10-06)

- Added `src/processing/mono_utils.rs` (13 `MonoUtils` predicates, `PrimitiveType` + `to_primitive_type`). All 777 shipped-assembly rows in `src/processing/mono_utils_fixture.txt` (probe `Temp/opencode/monoutilsprobe`) matched, including the 23 `ENGINE` rows kept as fixture evidence for the next increment. New row `TYPE-MONO-UTILS-001`; `DIFF-MONO-UTILS-001` recorded. Assembly loading/construction stay out (need live managed assemblies).
- `cargo test --workspace --locked`: 938 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37297415096` green (all five jobs) for remote `main` = `cb90d30`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), `EXPORT-UNITY-001` remainder (asset/sidecar file writes), and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 70. Checkpoint: asset-factory dispatch with an engine-table differential (2026-10-06)

- Added `src/processing/asset_factory.rs` (first audited slice of `SER-TT-ABSENT-001`): `ReadAsset` dispatch, `ReadNormalObject` fallback chain with log levels, `TryReadNormalObject` tail rules, Patch build-bump retry, `CreateAsset` outcomes, and the 21-arm `CreateEngineAsset` table re-asserting the `ENGINE` fixture rows. New row `SER-ASSET-FACTORY-001`; `DIFF-ASSET-FACTORY-001` recorded. Byte reading/node building/assemblies stay out.
- `cargo test --workspace --locked`: 944 library and 40 CLI tests; warnings-denied Clippy and format check pass. CI run `37298704192` green (all five jobs) for remote `main` = `1afe5c4`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), `EXPORT-UNITY-001` remainder (asset/sidecar file writes), and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 71. Checkpoint: asset-plus-meta pipeline composition (2026-10-06)

- Extended `src/export/project_output.rs` with `export_asset_with_meta` mirroring `AssetExportCollection.Export` (skip-on-failure, asset bytes before `{file}.meta`, add-only both files, caller-supplied GUIDs, shared confined walk). New row `EXP-META-PIPELINE-001`. Targeted module tests pass (8/8); CI run `37300927885` green (all five jobs) for remote `main` = `18e5fea`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 72. Checkpoint: script and assembly GUIDs with a runtime differential (2026-10-06)

- Extended `src/export/script_hashing.rs` with from-scratch RFC 1321 MD5, `UnityGuid` (words, nibble-reversed string, `MissingReference`), `ConvertSystemOrUnityBytes`, `Md5Hash` overloads, `CalculateScriptGuid` bytes/`str`, `CalculateAssemblyGuid` with ordinal `.dll` strip, and `RemoveAssemblyFileExtension`/`FixAssemblyName`. Pinned by 16 committed probe rows in `src/export/script_guid_fixture.txt` (shipped DLLs, SDK 10.0.401; `MyLib.dll` == `MyLib` proved at runtime). `guid_policy` now reports only `RandomPerCollection` open. New row `EXP-SCRIPT-GUID-001`; `DIFF-SCRIPT-GUID-001` recorded. Targeted export tests pass (303/303); CI run `37303561833` green (all five jobs) for remote `main` = `e41e28e`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, `TOOLS-001`, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 73. Checkpoint: generator audit (2026-10-06)

- Audited all six `*.SourceGenerator` projects (4 Roslyn analyzers + 2 console tools) with per-generator input/output/consumer triples, csproj kinds spot-verified. `GENERATORS-001` moves to `已调查` with Rust dispositions (no C# generator in the Rust build; VFS hand-ported; `AssetCreator` overlaps the engine table; license/localization/settings stay with product rows). Docs-only increment; CI run `37304229509` green (all five jobs) for remote `main` = `704b8f0`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 74. Checkpoint: tool audit (2026-10-06)

- Audited all fifteen tool-family projects (9 dev/diagnostic consoles + 7-project `AssemblyDumper` family) with per-tool input/output/consumer triples, `OutputType` and `generate.bat` spot-verified. `TOOLS-001` moves to `已调查` with Rust dispositions (no CLI equivalent owed as product surface; never port the emitter; no doc-pipeline/downloader/packaging equivalents). Docs-only increment; CI run `37305025403` green (all five jobs) for remote `main` = `db0f5d1`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, settings persistence, and the product rows (`PRODUCT-GUI/I18N-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 75. Checkpoint: configuration framework audit (2026-10-06)

- Audited `AssetRipper.Configuration` plus the settings load/save/delete wiring (call sites spot-verified). `PRODUCT-CONFIG-001` moves to `已调查` (records already ported; generic store needs no equivalent; JSON persistence round-trip + save/delete gate stay open). Docs-only increment; CI run `37305637087` green (all five jobs) for remote `main` = `f788871`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, settings persistence, and `PRODUCT-GUI-001`.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 76. Checkpoint: localization audit (2026-10-06)

- Inventoried 23 locale files (272-key `en_US.json`; empty `id.json`) with fallback rules read from `Localization.cs`/`LanguageCodes.cs`/`LocalizationGenerator.cs`. `PRODUCT-I18N-001` moves to `已调查`. Docs-only increment; CI run `37306221817` green (all five jobs) for remote `main` = `b1f8126`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, settings persistence, and `PRODUCT-GUI-001`.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 77. Checkpoint: GUI surface audit (2026-10-06)

- Inventoried `GUI.Free` + `GUI.Web` pages/routes/flags with call chains read directly. `PRODUCT-GUI-001` moves to `已调查`; fixed wrong `GameFileLoader` symbols in `PRODUCT-CLI-INSPECT-001`. Docs-only increment; CI run `37306891363` green (all five jobs) for remote `main` = `6a8712d`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and settings persistence.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 78. Checkpoint: ledger duplicate repair (2026-10-06)

- Split the colliding `EXP-EXPORT-SETTINGS-001` rows into records + `EXP-EXPORT-SETTINGS-DIFF-001` (stale verified claim honestly normalized). Gate re-run: 356 rows, 0 duplicates, 0 malformed, 105 non-verified, OPEN. Docs-only increment; CI run `37307865982` green (all five jobs) for remote `main` = `6c2b17c`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and the save/delete gate plus startup application.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 79. Checkpoint: settings-file round-trip with a runtime differential (2026-10-06)

- Added `src/export/settings_file.rs` (STJ-exact writer + tolerant bounded reader + opaque versions + 11 reverse discriminant maps), pinned by two committed shipped-DLL probe fixtures with byte-level BOM/CRLF facts. New row `EXP-SETTINGS-FILE-001`; `DIFF-SETTINGS-FILE-001` recorded. Targeted settings tests pass (11/11); CI run `37309863852` green (all five jobs) for remote `main` = `fc93892`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and actual settings file I/O plus startup application.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 80. Checkpoint: settings persistence gate (2026-10-06)

- Added the pure persistence rules (`decide_settings_persistence`, `decide_settings_auto_save`, `default_settings_file_path`) with a gate test. Only file I/O, the process directory query, and startup application stay outside. Targeted settings-file tests pass (7/7); CI run `37310873342` green (all five jobs) for remote `main` = `836f203`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and settings-file loading into the CLI.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 81. Checkpoint: settings application in export-texture (2026-10-06)

- Bridged settings-to-writer image formats and threaded the configured default through `export-texture` (behavior-identical under default settings). New row `EXP-SETTINGS-APPLY-001`. Targeted lib + CLI tests pass; CI run `37312018667` green (all five jobs) for remote `main` = `7cedac2`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and remaining per-command settings plus default-path startup load.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 82. Checkpoint: CLI settings-file loading (2026-10-06)

- `export-texture`/`export-shader` accept `--settings <path>` with explicit-beats-file precedence and path-carrying errors; usage updated. `EXP-SETTINGS-APPLY-001` broadened. Targeted lib, bin (43/43), and workspace Clippy pass locally; CI run `37314261710` green (all five jobs) for remote `main` = `e462613`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and remaining per-command settings plus default-path startup load.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 83. Checkpoint: GLB vertex-encoding differential (2026-10-06)

- Fixed three GLB accessor deviations with a SharpGLTF differential (color quantization, joint/index widths, SparseWeight lane rules). New row `RES-GLB-ENCODING-001`; `RES-MODEL-001` to implementing; `DIFF-GLB-ENCODING-001` recorded. Full lib suite 965/965 locally; CI run `37319285613` green (all five jobs) for remote `main` = `d7b403a`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and remaining per-command settings plus default-path startup load.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 84. Checkpoint: GLB writer size guard (2026-10-06)

- Ported the `GlbWriter` too-large refusal as a configurable `GlbBuildLimits::max_output_bytes` cap (default 2GiB) with an explicit `OutputTooLarge` error checked per buffer view and on the final total; refreshed the stale `EXP-GLB-001` remainder. Targeted glb tests pass (24/24), full lib 966/966 and bin 43/43 locally, workspace Clippy clean; CI run `37321528059` green (all five jobs) for remote `main` = `f92976c`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 85. Checkpoint: CLI settings loading in export-audio (2026-10-06)

- `export-audio` accepts `--settings <path>`, bridging `AudioExportFormat` through `plan_audio_clip_collection` (Default identical; PreferWav-meets-ogg fails explicitly on the unavailable Vorbis conversion; non-ogg passes through). Three new CLI tests; full bin 46/46 and lib 966/966 locally, workspace Clippy clean; CI run `37323075310` green (all five jobs) for remote `main` = `e6f616d`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, `export-mesh`/YAML per-command settings plus default-path startup load, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 86. Checkpoint: default-path settings startup load (2026-10-06)

- The three settings-aware commands fall back to the exe-adjacent `AssetRipper.Settings.json` when no `--settings` flag is given (explicit flag wins; broken default file fails loudly). Three new CLI tests; full bin 49/49 and lib 966/966 locally, workspace Clippy clean; CI run `37324101483` green (all five jobs) for remote `main` = `c4a824f`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), `EXPORT-UNITY-001` orchestration audit, CLI exposure + GUID freshness + folder sidecars, `export-mesh`/YAML per-command settings, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 87. Checkpoint: EXPORT-UNITY-001 orchestration audit (2026-10-06)

- Audited all 124 UnityProjects files family-by-family, corrected the stale row (wrong module path, `???` status), recorded two dead-code N/A findings, and ported `GetListOfVersions` + `ThrowIfSettingsDontMatch`. Lib suite 968/968 locally, workspace Clippy clean; CI run `37325748243` green (all five jobs) for remote `main` = `e28249b`; local matches.
- Next: `TYPE-SCRIPT-001` remainder (manager resolutions), CLI exposure + GUID freshness + folder sidecars, `export-mesh`/YAML per-command settings, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 88. Checkpoint: TYPE-SCRIPT-001 audit and IsMonoAssembly port (2026-10-06)

- Audited all 21 Assembly files, corrected the stale row (nonexistent `src/types/`, `???` status), split ported metadata paths from external-tooling-blocked live loading, and ported `IsMonoAssembly` with a dedicated test. Lib suite 969/969 locally, workspace Clippy clean; CI run `37327106695` green (all five jobs) for remote `main` = `e9aa2bb`; local matches.
- Next: CLI exposure + GUID freshness + folder sidecars, `export-mesh`/YAML per-command settings, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 89. Checkpoint: PreferOriginalTextureExtension bridge (2026-10-06)

- Ported `GetTextureExportFormat` and threaded the preference through `export-texture`; recorded source-evidenced N/A for `export-mesh`/YAML per-command settings. Full bin 50/50 and lib 970/970 locally, workspace Clippy clean; CI run `37328509913` green (all five jobs) for remote `main` = `0b05263`; local matches.
- Next: CLI exposure + GUID freshness + folder sidecars, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 90. Checkpoint: animation umbrella audit and PathChecksumCache (2026-10-06)

- Last two `未调查` rows (`PROC-ANIM-001`, `RES-ANIM-001`) moved to `已调查` with per-file mappings; new `PROC-ANIM-PATH-CHECKSUM-001` row with the ported cache kernels (6 tests). Lib suite 976/976 locally, workspace Clippy clean; CI run `37329970095` green (all five jobs) for remote `main` = `cf1ef8f`; local matches.
- Next: CLI exposure + GUID freshness + folder sidecars, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 91. Checkpoint: managed-registry framing and named-vector fix (2026-10-06)

- Verified managed-reference literals against `ManagedReferenceTypes`, fixed the named-vector element deviation with the full predicate, and ported the resolver-free registry framing (new `SER-MANAGED-REGISTRY-001`, 4 tests + 6-case predicate test). Lib suite 981/981 locally, workspace Clippy clean; CI run `37331921643` green (all five jobs) for remote `main` = `4e7f397`; local matches.
- Next: CLI exposure + GUID freshness + folder sidecars, and the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 92. Checkpoint: export-failure warning text (2026-10-06)

- Ported `format_export_failure` with a test; recorded precise dependencies for the meta-pipeline remainder. Caught and fixed a stale-cargo-cache incident (`clean -p` before trusting local runs). Fresh-build lib 982/982, bin 50/50, workspace Clippy clean; CI run `37333787247` green (all five jobs) for remote `main` = `f733870`; local matches.
- Next: the product rows (`PRODUCT-CONFIG/GUI/I18N-001`, `GENERATORS-001`, `TOOLS-001`).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 93. Checkpoint: localization code rules (2026-10-06)

- New `src/localization.rs` with the locale-independent rules (4 tests); fixed two stale module mappings; string tables stay open. Lib 986/986 locally, workspace Clippy clean; CI run `37390365686` green (all five jobs) for remote `main` = `02ca514`; local matches.
- Next: fallback-object behaviors, then the remaining product rows (`PRODUCT-CONFIG-001`, `PRODUCT-GUI-001`, `GENERATORS-001`, `TOOLS-001` remainders).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 94. Checkpoint: fallback hash and synthetic names (2026-10-06)

- Ported `RawDataHash` + `Unknown`/`Unreadable` naming with a dedicated test; `SER-TT-ABSENT-001` evidence updated. Lib 987/987 locally, workspace Clippy clean; CI run `37391118909` green (all five jobs) for remote `main` = `739822e`; local matches.
- Next: texture platform transforms, then the remaining product rows (`PRODUCT-CONFIG-001`, `PRODUCT-GUI-001`, `GENERATORS-001`, `TOOLS-001` remainders).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 95. Checkpoint: Xbox360 swap rule (2026-10-06)

- Ported `IsSwapBytes` + pair loop with two tests. Lib 989/989 locally, workspace Clippy clean; CI run `37392170107` green (all five jobs) for remote `main` = `38a5670`; local matches.
- Next: Switch unswizzle, then the remaining product rows (`PRODUCT-CONFIG-001`, `PRODUCT-GUI-001`, `GENERATORS-001`, `TOOLS-001` remainders).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 96. Checkpoint: Switch GOB unswizzle (2026-10-06)

- Ported `Unswizzle` with 8 tests incl. byte-exact cross-model vectors; new row `RES-TEXTURE-SWIZZLE-001`. Lib 997/997 locally, workspace Clippy clean; CI run `37394130404` green (all five jobs) for remote `main` = `2ca4eda`; local matches.
- Next: the ImageTexture route, then the remaining product rows (`PRODUCT-CONFIG-001`, `PRODUCT-GUI-001`, `GENERATORS-001`, `TOOLS-001` remainders).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 97. Checkpoint: ImageTexture base view (2026-10-06)

- Ported `ImageTextureView` + selection with two tests. Lib 999/999 locally, workspace Clippy clean; CI run `37395035692` green (all five jobs) for remote `main` = `8111b9d`; local matches.
- Next: the HDR writer, then the remaining product rows (`PRODUCT-CONFIG-001`, `PRODUCT-GUI-001`, `GENERATORS-001`, `TOOLS-001` remainders).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 98. Checkpoint: Radiance HDR writer (2026-10-06)

- Ported `save_hdr` byte-for-byte (22-vector shipped-DLL differential + boundary tests); new finding `DIFF-HDR-001`. Lib 1003/1003 and bin 50/50 locally, workspace Clippy clean; CI run `37396898585` green (all five jobs) for remote `main` = `3c4e811`; local matches.
- Next: the `GetImageData` platform orchestration, then the remaining product rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 99. Checkpoint: GetImageData platform orchestration (2026-10-06)

- Ported `resolved_image_data` with a 12-row live-object differential; new row `RES-TEXTURE-GETIMAGEDATA-001`, new finding `DIFF-TEXTURE-GETIMAGEDATA-001`. Lib 1008/1008 and bin 50/50 locally, workspace Clippy clean; CI run `37398996545` green (all five jobs) for remote `main` = `e0b16aa`; local matches.
- Next: the localization conversion tables, then the remaining product rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 100. Checkpoint: localization conversion tables (2026-10-06)

- Table-driven stem conversions with round-trip tests (5 localization tests); recorded the name-dictionary non-port. Lib 1009/1009 and bin 50/50 locally, `cargo fmt --check` clean, workspace Clippy clean.
- CI note: runs `37399516633` (3 attempts), `37400550877` (fresh push), and manual dispatch `37400914855` all fail in 4-5s with no steps, no runner, and no logs; GitHub status shows no incident and repo Actions are enabled. Local gates (lib 1009, bin 50, fmt, Clippy) all pass. Actions loop is blocked repo-side for now; work continues with full local validation until it recovers.
- Next: the cubemap export route, then the remaining product rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 101. Checkpoint: cubemap export route (2026-10-06)

- Ported class-89 acceptance + no-flip gate with a generated-API audit and an end-to-end CLI test; new row `RES-TEXTURE-CUBEMAP-001`. Lib 1011/1011 and bin 51/51 locally, fmt + Clippy clean. Actions validation pending: run `37402542981` failed in 4s with no steps (loop still blocked).
- Next: the array/3D texture views, then export wiring plus remaining rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 102. Checkpoint: array/3D texture views and rules (2026-10-06)

- Ported 117/187/188 views + rules with a 6-row live-asset differential; new row `RES-TEXTURE-ARRAY-001`, new finding `DIFF-TEXTURE-ARRAY-001`. Lib 1018/1018 locally, fmt + Clippy clean; Actions validation pending (loop still blocked).
- Next: array/3D export wiring + CLI classes, then remaining rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 103. Checkpoint: array/3D export wiring (2026-10-06)

- Composed the three exports + CLI dispatch with PNG and end-to-end tests. Lib 1019/1019 and bin 52/52 locally, fmt + Clippy clean; Actions validation pending (loop still blocked).
- Next: remaining rows (sprite/atlas, Crunch/EXR/JPEG, meta fields, then core SER/OBJ/PROC rows).
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 104. Checkpoint: sprite conversion gates (2026-10-06)

- Added `src/resources/sprite.rs` (`SPRITE_CLASS_ID`, `sprite_supported`, `sprite_try_convert_to_bitmap`) mirroring `SpriteConverter.Supported`/`TryConvertToBitmap` over caller facts; new row `RES-SPRITE-CONVERT-001`. Targeted lib tests 3/3 locally, fmt clean; full workspace + CI pending (Actions still blocked: billing `spending limit` annotation, no runner).
- Next: sprite atlas rect math, then Crunch/EXR/JPEG, meta fields, then core SER/OBJ/PROC rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 105. Checkpoint: sprite atlas coordinates (2026-10-06)

- Extended `src/resources/sprite.rs` with `get_sprite_coordinates_in_atlas` in source f32 order; new row `RES-SPRITE-ATLAS-001`. Targeted lib tests 6/6 locally, fmt + targeted lib Clippy clean; full workspace + CI pending (Actions still blocked by billing).
- Next: preview image dispatch, then Crunch/EXR/JPEG, meta fields, then core SER/OBJ/PROC rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 106. Checkpoint: web preview image dispatch (2026-10-06)

- Added `src/resources/preview_image.rs` routing `HasImageData`/`GetImageBitmap`/`GetRawImageData` over caller facts; new row `RES-PREVIEW-IMAGE-001`. Targeted lib tests 3/3 locally, fmt + targeted lib Clippy clean; full workspace + CI pending (Actions still blocked by billing).
- Next: Crunch/EXR/JPEG, meta fields, then core SER/OBJ/PROC rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 107. Checkpoint: full-workspace retrovalidation + Crunch gate (2026-10-06)

- Retrovalidation: the sprite/atlas/preview increments (104-106) had only targeted-test evidence. A local full run first showed 1008 passed / 20 failed, all 20 being `temp_dir()` `PermissionDenied` environment failures; re-running with `TEMP`/`TMP` redirected to gitignored `target/test-tmp` gives **1028 lib + 52 CLI + 0 doc, all pass**. This backfills the missing full-workspace evidence for `RES-SPRITE-CONVERT-001`, `RES-SPRITE-ATLAS-001`, `RES-PREVIEW-IMAGE-001`. Lesson: keep `target/test-tmp` redirection for local full runs in this sandbox. CI (Actions) still billing-blocked.
- Ported `CrunchHandler.IsUseUnityCrunch` as `is_use_unity_crunch` in `src/resources/texture_converter.rs` (2017.3 version switch + ETC-crunched fallback pair) with a boundary/matrix test; both decompressors stay explicit `CrunchUnsupported` because they are external NuGet packages with no in-snapshot implementation. New row `RES-TEXTURE-CRUNCH-001`. Authoring note: the first test draft wrongly asserted `false` for an uncrunched format on 2017.3+; the source version gate dominates before the format is considered (the caller only invokes it for crunched formats).
- Gates: `cargo fmt --check` 0, full tests 1029 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Push blocked (2026-10-06): committed locally as `bb82986` on top of `bf30dba` (= live `origin/main`, verified before commit). Pushing with the desktop key failed: `Bearer` scheme returned `invalid credentials`; the `token` scheme hung on a credential prompt and was killed. The token value was never printed or logged. Do not hammer auth: the desktop token is likely revoked/rotated or lacks scope — ask the user for a fresh key before retrying, then verify live ref + fetched tree before pushing.
- Next: EXR/JPEG writers, sprite exporter wiring, meta fields, then core SER/OBJ/PROC rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 108. Checkpoint: OpenEXR writer (2026-10-06)

- Ported `ExrWriter.Write<ColorRGBA<byte>, byte>` as `Bitmap::save_exr` (331-byte header, offset table, premultiplied channel-planar half pixels; linear alpha, sRGB-of-product RGB) with `f32_to_half_bits` RN-even conversion; wired `Exr` into `save()`, JPEG stays `Unsupported`. Genuine runtime differential: 4 complete files byte-for-byte, full 256+256 conversion tables, premultiplied grid/threshold cells on both sRGB branch sides (`exr_fixture.txt`, probe `Temp/opencode/exrprobe`, shipped DLL, SDK 10.0.401). New row `RES-TEXTURE-EXR-001`; writers row now lists JPEG as the only unsupported format. Notably, Rust `f32::powf` matches the reference bit-exactly across the whole table — no libm divergence. Authoring lessons: never trust eyeball-split undelimited hex (two phantom anomalies, both mine); never hand-copy long fixture rows (whiteblack row was truncated 340/379, regenerated by file splice).
- Gates: `cargo fmt --check` 0, full tests 1033 lib + 52 CLI + 0 doc, warnings-denied Clippy 0 (after fixing 3 new lints), baseline verifier 1,449 unchanged, `git diff --check` 0.
- Push still blocked: local `2e63b66` unpushed; the refreshed desktop key is rejected under both `Bearer` and `token` schemes (`invalid credentials`, fast failure, value never logged). Stopping auth retries — the key needs `repo` scope (classic) or Contents read+write on `cocokoishi/RustAssetRipper` (fine-grained), or belongs to an account without write access. Next agent: do not retry without a new key; verify live ref + fetched tree before pushing.
- Next: JPEG writer disposition, sprite exporter wiring, meta fields, then core SER/OBJ/PROC rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 109. Checkpoint: JPEG writer + push unblocked (2026-10-06)

- CORRECTION to 108: push is no longer blocked. The user logged into `gh` directly (`cocokoishi`, `repo` scope); `2e63b66` + `118108b` pushed as `bf30dba..118108b` to `origin/main`, verified live. The desktop `ghtoken.txt` is deleted; future pushes ride the `gh` login (a repo-local `credential.helper` experiment printed a harmless `credential-gh` warning and was removed; auth flows from the manager keyring).
- Ported `WriteJpg` (zero quality) as `save_jpg` in new `src/resources/jpeg.rs` (JFIF/DQT/SOF0/DHT/SOS headers, float YCbCr, box-average 420 + edge replication, textbook DCT, half-away quantization, zigzag, canonical Huffman + stuffing, EOI), wired `Jpeg` into `save()`. Differential: 10 complete files byte-for-byte, measured Q90 DQT, canonical Huffman spot-checks, test-only scan-decoder round-trip (`jpg_fixture.txt`, probe `Temp/opencode/jpgprobe`, shipped DLL, SDK 10.0.401). New row `RES-TEXTURE-JPEG-001`; writers row now records all six formats written. Key finding: a misremembered AAN DCT passed all flats but gave ~2/3 AC on the impulse — replaced by the textbook transform. Phantom green/chroma anomalies were hand-decoding slips again. Luma DQT tail is nonstandard, so measured tables are embedded.
- Gates: `cargo fmt --check` 0, full tests 1038 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: sprite exporter wiring, meta fields, then core SER/OBJ/PROC rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 110. Checkpoint: sprite importer decisions (2026-10-06)

- Ported `TextureExportCollection.AddSprites` as `decide_sprite_importer` in `src/export/importer_fields.rs` (empty/single/multiple branches, rect+name Single condition, sprite-supplied geometry, pivot/border gating, sheet-build flag), extending row `EXP-IMPORTER-FIELDS-001`. Shipped enum discriminants via reflection probe `Temp/opencode/enumprobe` (`AssetRipper.SourceGenerated.dll` 1.4.0): Single 1, Multiple 2, Center 0, Custom 9, FullRect 0, Sprite 8. Standing rule reaffirmed: commit and push promptly after each validated increment.
- Gates: `cargo fmt --check` 0, full tests 1039 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: meta texture-importer asset-copied fields, `FillSpriteMetaData`, then core SER/OBJ/PROC rows.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 111. Checkpoint: first team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes in one increment, all lead-verified (targeted suites re-run: 10/10, 14/14, 17/17) and integrated: `texture-meta` closed the `TextureImporterData` gaps (normalmap predicate, per-branch shapes, mip gates, no-streaming constants; `EXP-IMPORTER-READS-001`); `sprite-fill` ported `FillSpriteMetaData` (`RES-SPRITE-METADATA-001`); `texture-export` ported the `TextureAssetExporter` gates/flow (`EXP-TEXTURE-EXPORTER-001`, correcting the format-selection location to `ImageExportFormat.cs`). No scope violations, no placeholders, no docs edits by teammates.
- Gates: `cargo fmt --check` 0, full tests 1060 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: core SER/OBJ/PROC rows (second team round), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 112. Checkpoint: second team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 10/10, 34/34, 8/8) and integrated: `texture-meta` ported the `ReadMonoBehaviour` structure decision + diagnostics + tail layout (`SER-ASSET-FACTORY-001`); `sprite-fill` ported the `SerializedFile.Flags` derivation (`SER-TRANSFER-FLAGS-001`); `texture-export` re-audited registration with no gaps found, adding two pinning tests (`EXP-EXPORT-OVERRIDES-AUDIT-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1070 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: third team round (mesh/animation/audio gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 113. Checkpoint: third team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 8/8, 4/4, 6/6) and integrated: `texture-meta` covered the `AudioClipExtensions` remainder with a full FST/FSF fixture differential (`RES-AUDIO-001`); `sprite-fill` ported `GetStateBehaviours` + list routing (`ANIM-STATE-BEHAVIOUR-001`); `texture-export` ported the two sidecar site-flow gates (`EXP-META-EMISSION-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1076 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: fourth team round (material/shader/texture gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 114. Checkpoint: fourth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 24/24, 10/10, 45/45) and integrated: `texture-meta` ported engine-material creation + renderer CRC lookup (`RES-MATERIAL-EXPORT-001`); `sprite-fill` ported shader render-state predicates (`RES-SHADER-STATE-001`); `texture-export` fixed the `has_mips` flag-only understatement (`RES-TEXTURE-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1085 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: fifth team round (mesh/font/video gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 115. Checkpoint: fifth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 35/35, 9/9, 9/9) and integrated: `texture-meta` ported mesh stream-data/integrity/source kernels (`RES-MODEL-001`); `sprite-fill` factored the shared normal/tangent z-reconstruction kernel (`RES-COMPRESSED-MESH-001`); `texture-export` ported the BlendShape curve-resolver branch (`RES-BLENDSHAPE-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1095 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: sixth team round (scene/prefab/export gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 116. Checkpoint: sixth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 97/97, 18/18, 9/9) and integrated: `texture-meta` ported the scene-hierarchy Create switch + dependency order (`EXP-SCENE-MEMBERS-001`); `sprite-fill` ported the scene-pass bundle-name selection (`PROC-PREFAB-SCENE-BUNDLE-001`); `texture-export` ported the scene ExportableAssets selection (`EXP-SCENE-MEMBERS-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1102 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: seventh team round (YAML writer/serialized-file gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 117. Checkpoint: seventh team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 13/13, 16/16, 19/19) and integrated: `texture-meta` ported the SerializedFile header sniff (`SER-FILE-ENTRY-001`); `sprite-fill` ported serializedVersion gates + node prepend (`EXP-YAML-DOCUMENT-001`); `texture-export` pinned the IsStripped decision shape (`EXP-YAML-STRIPPED-FIELDS-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1108 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: eighth team round (bundle/archive/container gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 118. Checkpoint: eighth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 13/13, 9/9, 13/13) and integrated: `texture-meta` pinned the UnityFS flag/name surface (`IO-BUNDLE-UNITYFS-ENTRY-001`); `sprite-fill` named the WebFile align step + boundary coverage (`IO-BUNDLE-WEBDATA-001`); `texture-export` closed the asset_bundle survey with a coverage ledger + pinning tests (`PROC-ASSETBUNDLE-PATH-METADATA-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1116 lib + 52 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: ninth team round (CLI/product-layer gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 119. Checkpoint: ninth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 55/55 bin, 13/13 lib-filter, 11/11) and integrated: `texture-meta` named the CLI dispatch kernel (`PRODUCT-CLI-INSPECT-001`); `sprite-fill` ported log-rotation decisions (`PRODUCT-WEB-SURFACE-001`); `texture-export` closed the project_output survey with a ledger + confinement tests (`EXP-META-EMISSION-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1122 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: tenth team round (IO/compression gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 120. Checkpoint: tenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 10/10, 5/5, 6/6) and integrated: `texture-meta` ported VFS text/temp surface (`IO-FS-VIRTUAL-001`); `sprite-fill` added the LZ4 block encoder (`IO-COMP-LZ4-001`); `texture-export` gave discovery its first tests (`IO-FS-IMPORT-DIRECTORY-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1133 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: eleventh team round (animation/texture-format gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 121. Checkpoint: eleventh team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 3/3, 4/4, 4/4) and integrated: `texture-meta` ported curve envelope overloads (`ANIM-CURVE-SETVALUES-001`); `sprite-fill` named the slope-stride kernel (`ANIM-COMPRESSED-CURVE-001`); `texture-export` closed the prefab_export survey with a ledger + pinning test (`EXP-PREFAB-EXPORT-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1138 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: twelfth team round (avatar/streamed-curve/script gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 122. Checkpoint: twelfth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 5/5, 7/7, 6/6) and integrated: `texture-meta` ported the humanoid muscle kernel (`ANIM-HUMANOID-MUSCLE-001`); `sprite-fill` fixed float-narrowing semantics (`PROC-ANIM-STREAMED-001`); `texture-export` ported the script pointer kernel (`EXP-SCRIPT-EXPORT-FLOW-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1144 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: thirteenth team round (clip-convert/mixer/audio-export gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 123. Checkpoint: thirteenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 7/7, 6/6, 9/9) and integrated: `texture-meta` ported clip scan + muscle-curve names (`PROC-ANIM-CONVERT-001`); `sprite-fill` ported mixer processor kernels (`EXP-MIXER-EXPORT-001`); `texture-export` pinned audio warning texts (`RES-AUDIO-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1150 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: fourteenth team round (terrain/settings/localization gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 124. Checkpoint: fourteenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 5/5, 9/9, 5/5) and integrated: `texture-meta` ported the PlayerSettings arm (`PROC-PROJECT-EDITOR-SETTINGS-DEFAULTS-001`); `sprite-fill` ported settings Log rows (`EXP-SETTINGS-FILE-001`); `texture-export` closed localization coverage (`PRODUCT-I18N-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1153 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: fifteenth team round (terrain/video/sprite-export gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 125. Checkpoint: fifteenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 7/7, 15/15, 4/4) and integrated: `texture-meta` ported the terrain height grid (`RES-TERRAIN-HEATMAP-001`); `sprite-fill` ported sprite offset kernels (`RES-SPRITE-CONVERT-001`); `texture-export` audited the converter dispatch + error arms (`EXP-TEXDEC-BC45-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1156 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: sixteenth team round (blend-tree/video-adjacent/exporter-base gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 126. Checkpoint: sixteenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 3/3, 5/5, 37/37) and integrated: `texture-meta` ported the blend-type kernel (`ANIM-BLENDTREE-NODE-001`); `sprite-fill` ported fail-collection decisions (`EXP-EXPORTER-BASES-001`); `texture-export` fixed float dict-key parity (`EXP-YAML-VALUE-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1159 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: seventeenth team round (shader/animator-controller/mesh-export gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 127. Checkpoint: seventeenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 9/9, 9/9, 2/2) and integrated: `texture-meta` ported UV predicates + Empty (`RES-MESH-DATA-001`); `sprite-fill` ported template paths + guard (`EXP-SHADER-EXPORT-001`); `texture-export` pinned clip edge semantics (`PROC-ANIM-STREAMED-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1162 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: eighteenth team round (animator-state/controller gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 128. Checkpoint: eighteenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 2/2, 2/2, 2/2) and integrated: `texture-meta` ported state-context grouping (`ANIM-STATE-CONSTANT-001`); `sprite-fill` pinned count edges (`ANIM-STATEMACHINE-CONSTANT-001`); `texture-export` pinned transition edges (`ANIM-TRANSITION-CONSTANT-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1165 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: nineteenth team round (controller-constant/layer/override gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 129. Checkpoint: nineteenth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 2/2, 3/3, 2/2) and integrated: `texture-meta` ported the update-mode default (`ANIM-CONTROLLER-CONSTANT-001`); `sprite-fill` added muscle-clip branch coverage (`ANIM-MUSCLE-CLIP-INFO-001`); `texture-export` pinned clip conversion edges (`ANIM-CLIP-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1169 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: twentieth team round (avatar/dense/constant-clip gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 130. Checkpoint: twentieth team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 2/2, 2/2, 2/2) and integrated: `texture-meta` ported the leaf-index math (`EXP-AVATAR-CONSTANT-001`); `sprite-fill` ported the dense sample walk (`ANIM-DENSE-CLIP-001`); `texture-export` pinned IsSet extremes (`ANIM-CONSTANT-CLIP-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1172 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: twenty-first team round (animator-enums/curve-wrapper/keyframe gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.

## 131. Checkpoint: twenty-first team round integrated (2026-10-06)

- Three teammates shipped disjoint scopes, all lead-verified (targeted suites re-run: 2/2, 2/2, 2/2) and integrated: `texture-meta` ported the synced-layer decision (`PROC-ANIM-001`); `sprite-fill` added wrapper branch coverage (`ANIM-CURVE-SETVALUES-001`); `texture-export` pinned keyframe weight semantics (`ANIM-KEYFRAME-DEFAULT-001`). No scope violations, no placeholders.
- Gates: `cargo fmt --check` 0, full tests 1175 lib + 55 CLI + 0 doc, warnings-denied Clippy 0, baseline verifier 1,449 unchanged, `git diff --check` 0.
- Next: twenty-second team round (vector/keyframe-v3-q/bounds gaps), then `SetBoneGeometry`/outline mesh internals.
- State: Phase 1 is still active; the gate has not passed and Phase 2 remains locked. No user sample or UABEA content has been read.
