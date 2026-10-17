# RustAssetRipper hand-off — 2026-10-16

## Latest current checkpoint - 2026-10-16 (the JSON extractor's pointer override)

## Previous checkpoint - 2026-10-16 (primary-content extractor decisions ported)

- **The primary-content pipeline's missing half is started**: the handler table, dispatch, assembly,
  loop and membership were already ported, but nothing decided *what each extractor does*. New
  `src/export/primary_extractors.rs` ports `BinaryAssetContentExtractor` and records the two
  unconditional extractors. New ledger row `PRODUCT-PRIMARY-CONTENT-EXTRACTORS-001`.
- **Every binary arm requires a non-empty payload before admitting**, so an empty payload falls
  through to the next handler instead of exporting an empty file. `VideoClip` is gated on
  `CheckIntegrity()` rather than a length, and `MovieTexture` keeps `Has_MovieData` and
  non-emptiness as **two** separate conditions. `EmptyContentExtractor` **claims** into the shared
  empty collection - it does not decline - which is why an asset reaching one of those handlers is
  never reconsidered.
- **`Path.GetExtension` is host-dependent and the port had assumed `/`.** The video-clip path
  extension produced `d\x` for `C:\dir.d\x` on Windows where the reference produces nothing. Fixed
  by delegating to the fixture-verified `asset_path::get_extension` with a new
  `HostPathPlatform::host()`; `src/export/path_extension_fixture.txt` (18 runtime rows) now pins the
  helper, and the divergence is `DIFF-VIDEO-PATH-SEPARATOR-001`.
- **Duplicate port check, second hit in two rounds.** My first version reimplemented
  `GetFontExtension` and `GetExtensionFromPath`; `resource_extensions` already ported both. Replaced
  with imports. **Rule: before porting a reference behaviour, grep the tree for the reference symbol
  name.** Both duplicates were caught by exactly that search.
- **The four GLB extractors' gates are ported too.** `GlbMeshExporter` follows `MeshExtensions.IsSet()`
  (not "is a mesh"), `GlbNavMeshExporter` tests `HeightMeshes` for **null** so a present-but-empty
  list is admitted, `GlbTerrainExporter` has no condition, `GlbModelExporter` admits only a scene /
  prefab hierarchy main asset. `mesh::is_set` was the only genuinely missing predicate and it
  **composes** the already-ported `CompressedMesh::is_set` and `vertex_data_extensions::is_set`.
- **Remaining for `/Export/PrimaryContent` (still 501):** the audio, texture, script and
  deleted-asset extractors, plus a production caller for `binary_asset_claim`, `glb_asset_claim` and
  `json_walker::write_json_body`.
- **Next steps, in order:** finish the extractor layer (texture + script + GLB are the largest
  remaining), then wire the route, then the 9 `实现中` rows (`RES-TEXTURE-001`, `RES-MODEL-001`,
  `EXPORT-UNITY-001` first).
- **Corpus note:** `docs/acceptance-corpus.md` marks all 22 samples CONVERGED but those verdicts are
  dated 2026-10-15 and predate 60+ commits. They must be re-replayed locally before they count.
- **Never format `src/resources/glb_tests_01.rs` via the repo check without knowing it differs even
  when wrapped**; the CI script now wraps every `src/**/*_tests_NN.rs` fragment, which is verified for
  four of them.
- **A GLB is binary: decode the JSON chunk before asserting on text.**
- **Run `tools/parity-report.ps1` after every ledger edit**; a literal `||` inside a cell splits the
  row.
- **`JsonContentExtractor` overrides `VisitPPtr`**: a file id naming a dependency renders `{ "m_Collection": "<name>", "m_PathID": <id> }`, not the base `{ "m_FileID": n, "m_PathID": m }`. Both forms are ported; the base form is replayed from 5 runtime rows (`json_walker_pptr_fixture.txt`), the override is source-derived (private nested class needing a live `AssetCollection`). See `DIFF-JSON-PPTR-FORM-001`. **All extractor decisions are ported, and the stack order is measured.** `GetHandlerStack` yields the **reverse** of registration order (`OverrideHandler` appends, then pushes onto a `Stack<T>`), so `DeletedAssetsExporter` is tried first and `JsonContentExtractor` is the final fallback; `handler_stack_from`/`handler_stack_for` compute it and 9 runtime rows replay it. The port had this documented backwards - see `DIFF-PRIMARY-HANDLER-ORDER-001`. **Next: give `create_primary_content_collection` a production caller and wire the route**, which is what unlocks `/Export/PrimaryContent`.
- **A baseline capability that is NOT in the baseline source:** the primary-content script export decompiles C# through `AssetRipper.ICSharpCode.Decompiler` 11.0.0.9495, a forked ILSpy package reference. The AssetRipper-side code is ~85 lines around it. crates.io has no mature Rust equivalent (`backtrip` 0.1.4, `dotscope` 0.9.1 does not emit C#). New ledger row `TYPE-SCRIPT-DECOMPILE-001` = 受外部条件阻塞. **Closing it needs a user decision** (Rust decompiler vs accepted dependency), so the primary-content scripts directory cannot be finished by porting alone. See `SCOPE-SCRIPT-DECOMPILER-001`.
- **RES-MODEL-001 re-statused 实现中 to 已实现但未验证** after auditing it against the code: the conversion is implemented (vertex and skin lanes with the reference`'`s quantization, probed), and two of its cells were false. It claimed weights and bind poses were still required (they are implemented) and that model export settings were pending (the baseline has **no** model export setting at all). What is genuinely open is that neither the mesh nor the scene GLB builder has a production caller, so no geometry lane has been compared end-to-end. **Do not "fix" the absent glTF `skins` array** - the baseline builds every sub-mesh with `AddRigidMesh` and emits no inverse bind matrices either; see `SCOPE-GLB-RIGID-MESH-001`.
- **A background `gh run watch` job exiting 0 does not mean the run passed.** The job's exit code is its own last command's, not the run's conclusion. Read the per-job conclusions (`gh run view <id> --json jobs`) before claiming green, and re-check the *newest* run after a burst of pushes - `cancel-in-progress` marks superseded runs `cancelled`, which is not green. This has now cost two false claims; both were caught by checking conclusions instead of an exit code.
- **Inside a code span, write `OR`/`AND`, not an escaped pipe.** `parity-report.ps1`'s splitter correctly treats `\|` as an escaped pipe, but markdown does not process escapes inside backticks, so `\|\|` inside a code span renders as literal backslashes. Do not "fix" the checker for this - it is right. A naive `Split('|')` is not a markdown parser; I misdiagnosed this row as malformed with 15 cells when it had 13 and MALFORMED=0 was correct.
- **Ledger counts rotate in evidence cells.** RES-MODEL-001 quoted "1216 library and 55 CLI tests" long after the suite passed 1800; a stale count is the cheapest signal that a row has not been re-read. Record the count from the run that justified the row.
- **RES-TEXTURE-001 re-statused 实现中 to 已实现但未验证** (2026-10-16): its cell claimed "platform format branches pending" and the branches are implemented - 126 dispatch arms covering every reference family including the Crunch modes, and port code for all nine exporters in the reference `Textures` directory. What is open is **comparison breadth** (generated fixtures only, two Unity versions, BMP), not code.
- **Primary-content route, step 3 done:** `claim_for_extractor` (registration + `PrimaryContentObjectFacts` -> `Declined`/`Empty`/`Claimed`) and `json_content_document` exist, so the route supplies **facts** rather than matching extractor names in the HTTP layer. An audio clip is deliberately **claimed** while its producer reports `Unimplemented` - declining would send it to the JSON fallback, which the reference never does. `json_content_document` uses the **host** line break (`StreamWriter`), unlike the web tab`'`s explicit `"\n"`. **Next: `ObjectWorkspace` -> `PrimaryContentObject` adapter, `WebState::export_primary_content`, route registration (replace the 501 and its test).**
- **`project_exporter::object_class_facts` is now the one place** that maps a `FactoryAssetKind` to `ObjectClassFacts` (lifted out of the project walk`'`s object loop). Use it for the primary-content object adapter instead of writing a second copy. The `Texture3D` exception matters: through the TypeTree branch it is **not** dynamic, or it loses its class-specific handlers.
- **Primary-content route, step 2 done:** `src/export/primary_export.rs` connects the ported loop + destination planner + extractor decisions, so a collection can reach a file. `payload_kind` maps extractor to payload; payload and filesystem are caller closures (asset model / real disk); an extractor with no producer reports `Unimplemented` and fails its collection rather than being skipped. **Two probe contracts:** `get_unique_name` skips `File.Exists` entirely when `Directory.Exists` is false (the reference resolves the name before creating the directory), and the probe must reflect the run`'`s own writes or two assets collide. **Next: `WebState::export_primary_content` + the route registration + a producer per extractor.**
- **Primary-content route, step 1 done:** `handler_stack_for_object` derives an object's handler stack from its class facts via the existing `class_satisfies_target` (no new type rule was needed - it already answers both disjuncts plus the TypeTree/Unknown/Unreadable fallbacks), and `create_primary_content_collections_for_objects` runs the assembly rule over those stacks with `PrimaryContentClaim::{Declined, Empty, Claimed}` keeping "claimed" apart from "claimed into the empty collection". Stack order is asserted against the real interface table for classes 43/1/115/28. **Next: the payload producers and `WebState::export_primary_content` + the route registration.**
- **The primary-content pipeline's observable contract is complete; three parts must NOT be ported.** `ExportCollectionBase.ExportAsset` has no callers anywhere in the module (dead). `MultipleExportCollection`'s secondary `HashSet` only feeds another set, so its dedup and enumeration order are unobservable (and .NET hash order is not reproducible). `DeletedAssetsExportCollection.Export` throws but its `Exportable` is false, so the loop never calls it. **What remains is the route plus per-extractor payload production against the asset model** - the shared blocker, plus the decompiler decision for the script half.
- **Do NOT wire the scene GLB into the Unity-project export.** `GlbModelExporter` is instantiated in exactly one place (`PrimaryContentExporter.cs:99`) and `GlbLevelBuilder.Build` is called in exactly one place (`GlbModelExporter.cs:34`), so the scene/prefab GLB belongs to the **primary-content** pipeline only. Wiring `export_scene_glb` into the project export - where a materialized scene collection and a post-export stage already exist, so it looks natural - would emit `.glb` files the reference never writes there and pollute every project comparison. `EXP-GLB-SCENE-001` is blocked on the primary-content route, **not** on conversion code: the mesh gate, material/texture resolution, dynamic-vs-static dispatch, subset indices and transforms are all ported.
- **The remaining six 实现中 rows are accurate** (PROC-SCENE-001, PROC-ANIM-001, IO-STREAM-001, SER-TT-ABSENT-001, TYPE-SCRIPT-001, OBJ-REF-001): their gaps are missing **implementation**, not missing comparison. Only the two priority rows were stale.
- **Two of the three priority rows left 实现中 for the same reason:** the status was set when the row was opened and never revisited. Both audits found a cell that was false, not merely stale. Audit the remaining 实现中 rows the same way - measure the claim against the code before rewording anything.
- Parity ledger: 409 rows, 380 verified, 29 non-verified (7 实现中, 20 已实现但未验证, 2 受外部条件阻塞), DUPES=0, MALFORMED=0, gate OPEN.
- Verified: **1917** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (duplicate .NET float formatter removed)

- **The .NET-semantics sweep found its first hit in code committed one round earlier.** The JSON
  walker's float formatter was a *second* implementation of .NET `ToString`; `unity_yaml::
  format_dotnet_f32`/`_f64` already existed and additionally implements the equal-length tie rule.
  The copy printed `-280.89063` where .NET prints `-280.89062`. **The copy is deleted; the walker
  delegates.** Fixture grew to 82 rows with runtime-captured tie rows.
- **Rule for the rest of the sweep: before writing a second port of a reference behaviour, look for
  the first.** A duplicate is where the hard half of a rule gets omitted, and a fixture authored
  beside the copy will not cover it.
- **The sweep is now measurable, not guesswork.** ~190 runtime fixtures already exist under `src/`
  (floats, GUID, Hash128, vectors, bounds, matrix, color, textures, meshes, animation, containers,
  metadata). The JSON walker was an outlier. **Next sweep targets are the surfaces with no fixture
  at all** - enumerate them by listing formatter/encoding functions and checking for a fixture
  sibling, rather than by reading modules at random.
- **Two cheap edit traps, both hit this round:** `WriteAllLines`/`WriteAllText` whole-file rewrites
  reintroduce CRLF (fix with `git diff --check` then normalise), and a line-range replacement whose
  filter matches on text rather than position can delete an unrelated definition that shares the
  pattern. The compiler caught both; **review the diff, not just the test result.**
- **Remaining Phase-1 work, in rough priority order:** finish the sweep for unfixtured .NET
  surfaces; the primary-content content extractors (biggest missing layer - dispatch/assembly/loop/
  membership and the JSON/GLB pieces are done); `PRODUCT-GUI-001`'s pending pages incl.
  `/Export/PrimaryContent` and `/Scenes/View` (both 501); `RES-TEXTURE-001` / `RES-MODEL-001` /
  `EXPORT-UNITY-001` (`实现中`); the `PROC-ANIM-*` family; `PRODUCT-CONFIG-001`'s data-set subsystem.
- **Corpus note:** `docs/acceptance-corpus.md` marks all 22 samples CONVERGED but those verdicts are
  dated 2026-10-15 and predate 61+ commits. **They must be re-replayed locally** before they count as
  current evidence.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text.**
- **Run `tools/parity-report.ps1` after every ledger edit**; a literal `||` inside a cell splits the row.
- **CI formatting check fixed:** `tools/check_changed_rust_format.py` now wraps every `src/**/*_tests_NN.rs` fragment, not just `main_tests_NN.rs`. Before this, **any edit to one of the other 13 fragments failed CI**; four of them are verified byte-identical to their wrapped rustfmt output.
- **GUID rendering on the metadata path has its own runtime fixture** (`unity_guid_render_fixture.txt` + `tools/probes/guid-render`), with edge rows for the sign bit. The sweep is complete; its inventory is in `validation-log.md`.
- Parity ledger: 407 rows, 380 verified, 27 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- Verified: **1878** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.
- **CI on `2fced0e` is green across all eight jobs** (hygiene, ledger audit, changed-line rustfmt, clippy on both OSes, tests + release build on both OSes, release-mode tests). **Note the workflow runs with `cancel-in-progress`:** a burst of pushes cancels earlier runs, so after several pushes check the *newest* run rather than the one bearing the commit you pushed second-to-last. Reading a cancelled run as green was a mistake made once already.

## Previous checkpoint - 2026-10-16 (JSON surface renders the reference's indented text)

- **`JsonWalker` had no production caller.** `render_asset_json` served compact `write_json_value`
  output, so the JSON tab and `/Assets/Json` differed from the reference's tab-indented text in
  every line. Added `write_json_body` (the reference's traversal order) and wired the route to it.
  Recorded as `DIFF-JSON-INDENT-001`.
- **The line break is the writer's, not a constant.** The export's `StreamWriter` takes
  `Environment.NewLine` (`\r\n` on Windows); the browser route sets `NewLine = "\n"` explicitly
  (`AssetAPI.cs`). `JsonWalker::new` uses the host default, `JsonWalker::with_line_break` takes an
  explicit one, and a test pins both. **I first made the module hard-code the host newline from the
  probe's `\r\n` - the probe's own writer configuration is part of the evidence, not just its output.**
- **The shape rules are now runtime-verified.** `tools/probes/json-walker` drives the reference's own
  `DefaultJsonWalker` over a `StringWriter` (no asset model needed) and 13 scenarios are replayed by
  `walker_shape_matches_the_reference_runtime_rows` from `json_walker_shape_fixture.txt`. **Use this
  probe for any further walker work; it is the cheapest ground truth in this area.**
- **What is still open on `PRODUCT-ASSET-JSON-TAB-001`:** the traversal *order* - the port walks its
  decoded `Struct`, the reference walks `WalkStandard`, so a document replay against original
  AssetRipper is the missing evidence. The PPtr form is unprobed (reflection could not build a
  `PPtr<TAsset>` argument).
- **Remaining Phase-1 work, in rough priority order:** the primary-content content extractors
  (biggest missing layer; dispatch/assembly/loop/membership are done), `PRODUCT-GUI-001`'s pending
  pages, `RES-TEXTURE-001` / `RES-MODEL-001` / `EXPORT-UNITY-001` (`实现中`), the `PROC-ANIM-*`
  family, and `PRODUCT-CONFIG-001`'s data-set subsystem.
- **Corpus note:** `docs/acceptance-corpus.md` marks all 22 samples CONVERGED, but those verdicts are
  dated 2026-10-15 and predate roughly a day of commits. **They are stale relative to HEAD** and must
  be re-replayed locally before they count as current evidence.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **Run `tools/parity-report.ps1` after every ledger edit**; a literal `||` inside a cell splits the
  row. Also check `git diff --check` for a blank line at EOF after a whole-file rewrite, and
  normalize CRLF back to LF (`WriteAllLines` reintroduces CRLF).
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 380 verified, 27 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Verified: **1877** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (JSON walker float formatting corrected)

- **The JSON walker formatted floats with Rust's `Display`, which is not .NET's `ToString`.**
  Fixed in `src/export/json_walker.rs`; recorded as `DIFF-JSON-FLOAT-FORMAT-001`. The reference's
  real behaviour is captured in `src/export/float_format_fixture.txt` (79 rows from
  `tools/probes/float-format`, which calls the real
  `value.ToString(CultureInfo.InvariantCulture)`):
  - **Exponent threshold**: .NET switches to exponent form at exponent `<= -5`, or `>= 9` (float) /
    `>= 17` (double). `Display` **never** uses exponent form. So `1e-5f` → `1E-05` but `1e-4f` →
    `0.0001`; `1e9f` → `1E+09` but `1e8f` → `100000000`.
  - **Exponent spelling**: uppercase `E`, explicit `+` for positives, at least two digits.
  - **Non-finite**: bare `NaN` / `Infinity` / `-Infinity` (not valid JSON, but that is what the
    reference writes). `Display` gives `inf` / `-inf`.
- **The precision trap is the part to remember.** The formatter takes the value widened to `f64`,
  so the shortest digit string must come from the **originating** type: formatting widened `1e-5f`
  at `f64` precision yields `0.000009999999747378752`, because widening is exact. Digits and
  threshold are carried by one `kind` value so they cannot drift apart.
- **Replaying the captured rows caught two of my own errors** (the widened digits, then the lower
  bound being `<= -5` rather than `< -5`). Neither was visible from reading the code. This is the
  third time in this series that a runtime-derived expectation beat a reasoned one — **probe the
  runtime for encoders and formatters; do not reason from the source.**
- **Both JSON-walker defects are now closed** (`DIFF-JSON-ESCAPE-ENCODER-001`,
  `DIFF-JSON-FLOAT-FORMAT-001`). The walker's remaining unverified surface is the traversal order and
  the empty-container shapes, which are already documented in the module and not yet replayed
  against a real export.
- **Next steps, in order:** the primary-content **content extractors** are the only missing layer
  (`JsonContentExtractor` glue over the now-corrected walker, `BinaryAssetContentExtractor`,
  audio/texture/script extractors, GLB model/mesh/terrain/navmesh collections) — without them
  `/Export/PrimaryContent` cannot be implemented even though dispatch, assembly, loop and membership
  are all ported. Then `RES-TEXTURE-001` (`实现中`) and `PROC-ANIM-*`.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **Run `tools/parity-report.ps1` after every ledger edit**; a literal `||` inside a cell splits the
  row and raises `MALFORMED`.
- **Do not reuse `glb::escape_json` for the JSON walker** — the two encoders are genuinely different.
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1874** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (JSON walker encoder corrected)

- **The JSON walker was escaping like JSON, not like `HttpUtility.JavaScriptStringEncode`.**
  Fixed in `src/export/json_walker.rs`; recorded as `DIFF-JSON-ESCAPE-ENCODER-001`. The reference's
  real behaviour is captured in `src/export/json_escape_fixture.txt` (34 rows from
  `tools/probes/json-escape`, which calls the actual encoder):
  - **control characters are `\uXXXX` with uppercase hex** - there is no `\n`/`\r`/`\t`/`\b`/`\f`
    shorthand anywhere;
  - `< > & '` become `\u003c \u003e \u0026 \u0027`;
  - `U+2028`/`U+2029` are escaped;
  - **non-ASCII is left as-is** (`é`, `中` pass through).
- **The old test asserted the same bug the code had** (it expected `\n` and `\t`). Second occurrence
  of the "test written from the implementation" failure mode - after the glTF quaternion sign
  error. **When adding an encoder/decoder test, derive the expectation from a runtime probe, never
  from the function under test.** The new fixture-replay test makes the expectation independent.
- **Do not reuse `glb::escape_json` here.** It implements ordinary JSON escaping; the two encoders
  are genuinely different, and de-duplicating them would silently re-introduce this defect.
- **Next steps, in order:** the primary-content **content extractors** are now the only major
  missing layer (`JsonContentExtractor` glue over the corrected walker, `BinaryAssetContentExtractor`,
  audio/texture/script extractors, and the GLB model/mesh/terrain/navmesh collections) - without
  them `/Export/PrimaryContent` cannot be implemented even though dispatch, assembly, loop and
  membership are all ported. Then `RES-TEXTURE-001` (`实现中`) and `PROC-ANIM-*`.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **Run `tools/parity-report.ps1` after every ledger edit** and check `MALFORMED` is empty; a
  literal `||` inside a cell splits the row.
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1873** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (`isScene` is dead in the baseline)

- **`GlbLevelBuilder.Build`'s `isScene` flag is never true in the frozen source.** Recorded as
  `DIFF-GLB-ISSCENE-DEAD-001`. The proof is exhaustive: `Build` has one caller
  (`GlbModelExporter.ExportModel`), `ExportModel` has one caller (the **multi-asset**
  `IContentExtractor.Export` overload), and that hard-codes `false`. Neither
  `GlbSceneModelExportCollection` nor `GlbPrefabModelExportCollection` overrides `ExportInner`, so
  a scene hierarchy gets `false` too. **The class name `GlbSceneModelExportCollection` is what
  misleads** - it is not the place that would set the flag.
- Consequence: in the shipped product a scene GLB and a prefab GLB come from the same call with the
  same flag, and **no root node ever carries a local transform**.
- **Do not hard-code `false` in the port.** `is_scene` stays a parameter because the
  reference-verified `Build` semantics must remain testable on both arms, and
  `level_plan_matches_the_reference_runtime_rows` does so against reference-runtime fixtures. The
  honest statement is in the doc comment: production callers pass `false`.
- **This is the fourth time reading the source corrected a gap I had recorded** (after
  `EXP-GLB-001` twice and `RES-MODEL-001`). Same shape every time: an *implemented but unreachable*
  branch read as a missing feature. The settling check is small - list every call site of the entry
  point and read the arguments.
- **Ledger hygiene:** a literal `||` inside a table cell splits the row and makes the parity report
  raise `MALFORMED`. Use `A OR B` in prose. **Run `tools/parity-report.ps1` after every ledger edit**
  and check `MALFORMED` is empty.
- **Next steps, in order:** wire the primary-content export path to the web surface
  (`/Export/PrimaryContent`, still 501) now that dispatch/assembly/loop/membership are all ported;
  then `RES-TEXTURE-001` (`实现中`) and `PROC-ANIM-*`.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1872** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (collection membership rules ported)

- **Ported the membership half of `SingleExportCollection`/`MultipleExportCollection`**:
  `PrimaryContentCollection::contains` and `add_asset`.
- **Membership is by asset identity, never by name.** `Contains` compares `AssetInfo`, which is
  `readonly record struct AssetInfo(AssetCollection Collection, long PathID, int ClassID)` -
  structural equality over that triple. Pinned directly: two assets with the *same* display name
  stay in separate collections and neither claims the other.
- **`AddAsset` refuses to append the primary** (`!ReferenceEquals(asset, Asset)`), because `Assets`
  already yields the primary first; without the guard it would be written twice.
- **A second asymmetry in the same file is benign - documented so nobody "fixes" it later.**
  `SingleExportCollection.Name` returns `GetBestName()` **raw** while the *filename* is sanitized.
  Unlike `DIFF-PRIMARY-CONTENT-CLASSNAME-001`, this cannot cause harm: the only consumers of `Name`
  are the progress/warning **log** lines, so a raw name never reaches a path. **I checked the
  consumers rather than pattern-matching on the earlier finding** - the two look identical and are
  not. Do not change it.
- **General lesson from the last two rounds:** the same-looking asymmetry can be a defect in one
  place and harmless in another. Check where the value goes before deciding.
- **Next concrete step for the scene GLB:** give `export_scene_glb` a caller reachable from the
  primary-content path, and resolve per-node geometry (the reference reaches a mesh through
  `BuildParameters.TryGetOrMakeMeshData`). `EXP-GLB-001` and `EXP-GLB-SCENE-001` share this work.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1872** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (primary-content path escape fixed)

- **Found and fixed a real path escape in the frozen source.**
  `ExportCollectionBase.GetUniqueFileName` sanitizes the asset-derived name on **one** branch only:
  when the trimmed best name is empty it falls back to `asset.ClassName` **verbatim**, skipping
  `FixInvalidFileNameCharacters`. The port reproduced that faithfully; it now sanitizes both.
  Recorded as `DIFF-PRIMARY-CONTENT-CLASSNAME-001`.
- **Why it matters:** `TypeTreeObject.ClassName` is `EditorFields.Type.Name`, i.e. the TypeTree's
  own `m_Type` string - **file-controlled data, not a C# identifier**. The baseline corroborates
  that such names need sanitizing: `Pass002_RenameSubnodes` runs `ValidNameGenerator.GetValidTypeName`
  over every `TypeName`, and that generator's regex is `[<>\[\]\s&\(\):\.-]`. Four of those (`[`,
  `]`, `:`, whitespace) are also invalid **file-name** characters, so an unsanitized type name can
  inject a path component.
- **Reachability, stated precisely because the first version of this claim was too broad:** the
  branch is *not* "the asset has no name". `GetBestName` itself falls back through `OriginalName`
  to `ClassName`, so an ordinary unnamed asset already takes the **sanitizing** branch. The
  unsanitized branch needs a name that is present but **whitespace-only** (`OverrideName` /
  `OriginalName` that trims to nothing). Real, but narrower than it first looked - check
  reachability before writing a claim down.
- **When a test expectation and the implementation disagree, check which is wrong before editing.**
  Two of mine were wrong: the sanitizer yields `Foo_Bar__Baz` (one `_` per invalid character, not
  a collapsed run), and the class-name fallback lands in `Export/Assets/` via `GetBestDirectory`,
  not directly under `Export`. The implementation was right in both cases.
- **Next concrete step for the scene GLB:** give `export_scene_glb` a caller reachable from the
  primary-content path, and resolve per-node geometry (the reference reaches a mesh through
  `BuildParameters.TryGetOrMakeMeshData`). `EXP-GLB-001` and `EXP-GLB-SCENE-001` share this work.
  The scene GLB belongs to the **primary-content** path (`GameFileLoader.cs:96`,
  `/Export/PrimaryContent`), **not** the Unity project export.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1871** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (primary-content pipeline complete)

- **Ported `PrimaryContentExporter.CreateCollection`.** The primary-content pipeline is now complete
  end to end: handler table -> `create_primary_content_collection` (dispatch) ->
  `create_primary_content_collections` (assembly) -> `run_primary_content_export_loop` (export).
- **An exhausted handler stack is an error, not an `Option`.** The source throws `There is no
  exporter that can handle '<asset>'`; through the default registration that branch is unreachable
  (`JsonContentExtractor` claims `IUnityObjectBase` first). It is kept as a loud error anyway,
  because a fallback collection would silently drop a resource the moment a caller narrows the
  stack. The `IUnityObjectBase` stack is asserted to be exactly `JsonContentExtractor` then
  `DeletedAssetsExporter`.
- **A duplicate is skipped before dispatch**, so a stack that would decline is never consulted
  twice for the same asset. That is why resolution moved *inside* the assembly loop.
- **Lesson worth keeping:** my first draft of the dispatch compiled while ignoring its arguments
  and always returning the error - a placeholder that a test could have "passed" against. If a
  function takes inputs it does not use, that is a bug, not a simplification.
- **Next concrete step for the scene GLB:** give `export_scene_glb` a caller reachable from the
  primary-content path, and resolve per-node geometry (the reference reaches a mesh through
  `BuildParameters.TryGetOrMakeMeshData`, so the caller resolves each GameObject's
  `MeshFilter`/`Renderer` and passes per-node `MeshData`). `EXP-GLB-001` and `EXP-GLB-SCENE-001`
  share this work. The scene GLB belongs to the **primary-content** path
  (`GameFileLoader.cs:96`, `/Export/PrimaryContent`), **not** the Unity project export.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1870** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (primary-content export loop ported)

- **Ported `PrimaryContentExporter.Export`** as `run_primary_content_export_loop`. The
  primary-content control flow is now complete end to end: handler table -> collection assembly ->
  export loop.
- **The counter is the subtle part:** progress is emitted **only** for exportable collections but
  indexed over **all** of them, so a run whose first two collections are not exportable opens at
  `(3/4)`. The project pipeline counts only what it exports. A test builds exactly that shape and
  asserts the emitted lines, so this cannot silently drift.
- The loop takes an `export_one` callback and returns an ordered `Progress`/`Failed` event list, so
  its logging is testable without a file system. A failed write emits progress **then** failure; a
  non-exportable collection emits nothing, not even a skip notice.
- `ExportCollectionBase.Name` is abstract in the source with a different derivation per collection
  type, so the name is a **caller-supplied field** rather than a rule this module invents.
- **Next concrete step for the scene GLB:** give `export_scene_glb` a caller reachable from the
  primary-content path, and resolve per-node geometry (the reference reaches a mesh through
  `BuildParameters.TryGetOrMakeMeshData`, so the caller resolves each GameObject's
  `MeshFilter`/`Renderer` and passes per-node `MeshData`). `EXP-GLB-001` and `EXP-GLB-SCENE-001`
  share this work. Remember the scene GLB belongs to the **primary-content** path
  (`GameFileLoader.cs:96`, `/Export/PrimaryContent`), **not** the Unity project export.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1868** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (primary-content assembly ported; scene GLB's home corrected)

- **Ported `PrimaryContentExporter.CreateCollections`** into
  `create_primary_content_collections` (`src/export/primary_content.rs`). The port already had the
  handler table and the single-asset path planner; the algorithm *between* them was missing. New
  ledger row `PRODUCT-PRIMARY-CONTENT-001` (406 -> 407 rows).
- **Two ordering rules are pinned by tests because both are easy to reverse** (the module used to
  document them without implementing them):
  - `queued.Add(asset)` runs **before** the empty-collection check, so an asset whose collection
    turned out empty is still marked and never reconsidered.
  - A non-empty collection queues **all** its members, so an asset claimed earlier is never
    exported twice.
- **I corrected a wrong note from the previous round.** I had written "give `export_scene_glb` a
  caller in the project export pipeline". Wrong pipeline: `GlbModelExporter` is registered in the
  **primary-content** handler stack, and `PrimaryContentExporter` has exactly one caller in the
  frozen source - `GameFileLoader.cs:96`, the GUI's *Export Primary Content* action, i.e. the
  port's `/Export/PrimaryContent` (still 501). **The scene GLB belongs to the primary-content path,
  not the Unity project export.** No code was built on the wrong assumption.
- **Next concrete step for the scene GLB:** give `export_scene_glb` a caller reachable from the
  primary-content path, and resolve per-node geometry (the reference reaches a mesh through
  `BuildParameters.TryGetOrMakeMeshData`, so the caller resolves each GameObject's
  `MeshFilter`/`Renderer` and passes per-node `MeshData`). `EXP-GLB-001` and `EXP-GLB-SCENE-001`
  share this work.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- **When a feature a gap cell calls unported lands, re-check that cell in the same round** - four
  cells have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`).
- Parity ledger: 407 rows, 381 verified, 26 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` and `/Export/PrimaryContent` both still return 501.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1865** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (all 25 rows audited; one stale cell left)

- **I audited every non-verified row's gap cell in one pass**, after three stale cells had been
  found one round at a time. Result: **24 of 25 are accurate**, and the one remaining stale cell -
  `EXP-GLB-001` - is corrected. The ledger's counts are unchanged (406 rows, 381 verified, 25
  non-verified).
- **`EXP-GLB-001` claimed `GlbLevelBuilder.Build` "remains unported", which stopped being true
  several rounds ago.** Every part now exists: `build_level_plan` (traversal, reference-verified),
  `build_scene_document` + `SceneDocument::into_glb` (document), `attach_plan_geometry`,
  `build_scene_plan` (collection adapter), `export_scene_glb` (entry), and
  `ResourceExportFamily::HierarchyGlb` (selection). What is genuinely left is **the wiring, not the
  builder**: `export_scene_glb` has no caller outside its own module and tests.
- **The two rows that looked most suspicious were fine**, which is why the audit was worth doing
  properly: `PRODUCT-GUI-001` enumerates each pending page and each deliberate substitution, and
  `PRODUCT-CONFIG-001`'s "data-set subsystem" is a real upstream type family (`DataSet`,
  `JsonDataSet`, `ParsableDataSet`, `StringDataSet`, `ListDataStorage`) that `ConfigurationFilesPage`
  genuinely uses. **Do not "correct" those.**
- **Rule for future rounds:** when a feature a gap cell names as unported actually lands, re-check
  that cell in the same round. The check is one command - look for the symbol in `src/`. Four cells
  have been wrong this way (`EXP-GLB-001` twice, `RES-MODEL-001`, `RES-BLENDSHAPE-001`), and a
  stale cell sends the next reader to redo work that exists.
- **Next candidates, in order:** the scene export wiring (`EXP-GLB-SCENE-001` + `EXP-GLB-001` share
  it: give `export_scene_glb` a caller and resolve per-node geometry), then `RES-TEXTURE-001`
  (`实现中`) and `PROC-ANIM-*`.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1862** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (blend-shape frames are a baseline boundary)

- `RES-BLENDSHAPE-001` listed "MeshData geometry and frame/delta/weight decoding" as remaining
  work. **The baseline never decodes that data**, so it is a baseline boundary rather than a port
  gap, and the cell is corrected. The row keeps 已实现但未验证 for its real gaps (real animation
  binding/export over a real sample, other versioned layouts, Unity-produced mesh validation).
- **Evidence is exhaustive:** the frozen `Source` tree references blend-shape data in exactly two
  places, both name resolution. Nothing reads `m_Frames`, `m_DeltaVertices`, `m_DeltaNormals` or
  `m_FullWeights`, and the GLB module has no morph-target code - matching the independent finding
  already recorded on `EXP-GLB-001`.
- **A methodological note that cost me a false start:** `blend_shape_attribute_name` looked like it
  had no production caller, i.e. a missing integration. It is a *lower-level helper*; the real path
  runs `converter.rs` -> `BindingCustomType::BLEND_SHAPE` -> `AnimationSceneFacts::blend_shape_name`
  -> a table populated by the real resolver. **A helper with no direct caller is not evidence of a
  missing path.** Follow the caller chain before writing down a finding.
- **This is the third stale-claim correction on these rows** (after `EXP-GLB-001` and
  `RES-MODEL-001`). The recurring shape: a gap cell mixing an *unimplemented upstream feature* with
  genuine port gaps. **When a row says something is unported, first ask whether the baseline
  implements it at all.**
- **Next candidates, in order:** `RES-TEXTURE-001` (`实现中`), the scene export's real-file
  verification (`EXP-GLB-SCENE-001`), `PROC-ANIM-*`.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- Parity ledger: 406 rows, 381 verified, 25 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1862** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (Crunch dispatch proven load-bearing)

- **The Crunch dispatch is now cross-checked against the reference runtime**, not taken on faith.
  `tools/probes/crunch-faces` loads the shipped `AssetRipper.Conversions.Crunch.dll` and
  `AssetRipper.Conversions.UnityCrunch.dll` and runs **both** decoders over every committed CRN
  fixture; the rows are committed as `src/resources/crunch_dispatch_fixture.txt`. Both branches
  matter:
  - The ETC payloads **cannot** go through the classic decoder - `unity_etc1.crn` makes it
    **throw**, `unity_etc2a.crn` makes it **fail**. The format branch prevents a crash.
  - The DXT payloads decode through both decoders to **different bytes**. The version branch
    prevents a silently wrong image, which is the worse failure mode.
- `RES-TEXTURE-CRUNCH-001` **keeps 已实现但未验证**: CRN variants beyond the six fixtures, real
  multi-face/multi-mip payloads, real-sample coverage and Unity import behaviour are still open.
  Do not re-litigate the *gate* - that part is settled.
- **Probe trap:** the shipped entry point takes `ReadOnlySpan<byte>`, and a span **cannot be boxed**
  for `MethodBase.Invoke` - it throws `ArgumentException: Object of type 'System.Byte[]' cannot be
  converted to type 'System.ReadOnlySpan\`1[System.Byte]'`. Invoke through a delegate whose
  signature matches instead.
- **Next candidates, in order:** `RES-BLENDSHAPE-001` (MeshData geometry and frame/delta/weight
  decoding is the named gap), `RES-TEXTURE-001` (`实现中`), then the scene export's real-file
  verification (`EXP-GLB-SCENE-001`).
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- Parity ledger: 406 rows, 381 verified, 25 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1862** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (FSB5 Vorbis setup table verified byte-exact)

- **`RES-AUDIO-001`'s "real-audio table decode stays open" gap was investigated and the table is
  complete and byte-identical to the reference's.** 161 entries in both, matching keys, matching
  payload lengths, and a matching SHA-256 digest for the sampled header. The row stays
  已实现但未验证 for the *other* named gaps (multi-stream banks, other codecs, end-to-end clip
  decode), but table coverage is no longer an open question - do not re-investigate it.
- **The probe recipe, since two wrong turns cost time:** `tools/probes/fmod-setup` reflects the
  table out of the reference's `Fmod5Sharp.dll`. The table is **not a field** - it is a generated
  singleton **property**, `Fmod5Sharp.CodecRebuilders.FmodVorbisRebuilder.headers`, and the
  containing type is compiler-mangled. Search `PropertyInfo` (not only `FieldInfo`) and select by
  member name containing `Vorbis`, or the probe reports "no-table-found" while still finding the
  unrelated frequency table - which reads as "upstream removed it".
  Run: `dotnet run --project tools/probes/fmod-setup/FmodSetupProbe.csproj -c Release -p:ReferenceRoot="$LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- <same path>`.
- **Compare values, not just keys.** The probe prints a SHA-256 prefix per entry because matching
  keys with corrupted payloads would decode silence while looking correct.
- `RES-AUDIO-001` also notes the port does not call `Fmod5Sharp` at runtime, which is intended: the
  table is vendored from `fsbex` 0.3.0 with its MIT notice kept in
  `vendor/fsbex-vorbis-table/LICENSE-MIT`.
- **Remaining work for `EXP-GLB-SCENE-001` (still 已实现但未验证), in order:** run a real-file scene
  export and compare against reference output; wire geometry selection (the reference reaches a
  mesh through `BuildParameters.TryGetOrMakeMeshData`, so the caller resolves each GameObject's
  `MeshFilter`/`Renderer` and passes per-node `MeshData`); route the family through the project
  export pipeline.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
- Parity ledger: 406 rows, 381 verified, 25 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1861** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (hierarchy GLB joins the exporter tables)

- The hierarchy GLB export is now **selectable**: `ResourceExportFamily::HierarchyGlb`,
  `hierarchy_glb_family`, and per-family `export_extension`/`is_glb` in
  `src/export/res_wiring.rs`. Three tests.
- **`GlbModelExporter` is selected by object kind, not class ID.** The reference registers it for
  `GameObjectHierarchyObject`, `IGameObject`, `IComponent` and `ILevelGameManager`;
  `resource_export_family` is class-keyed and cannot express that, so `hierarchy_glb_family(true)`
  is a separate entry point. Classes 29 and 1001 deliberately stay unowned by the class-keyed
  classifier, and a test pins that so the two paths cannot be conflated.
- **`plan_resource_export` returns `None` for the hierarchy family on purpose.** A whole-hierarchy
  GLB needs the scene graph plus per-node meshes; a single-asset planner has neither, and a
  one-object GLB would be a silent degradation that looks like a hierarchy export.
- **No new path code was needed.** `SingleExportCollection`'s naming (best directory, best name
  with clone/instance suffixes removed, empty -> class name, then unique) is exactly what
  `plan_single_export_paths` already does; only the family and its `"glb"` extension were missing.
- **Remaining work for `EXP-GLB-SCENE-001` (still 已实现但未验证), in order:**
  1. Run a real-file scene export and compare against reference output.
  2. Wire geometry selection: the reference reaches a mesh through
     `BuildParameters.TryGetOrMakeMeshData` (an `IMesh` -> `MeshData` build plus cache), so the
     caller resolves each GameObject's `MeshFilter`/`Renderer` fields and passes per-node
     `MeshData`; `SceneCollectionGraph::mesh` is where the selection belongs.
  3. Route the family through the project export pipeline so a scene actually writes its `.glb`.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
  `glb.rs`, `glb_level.rs`, `scene_glb.rs`, `res_wiring.rs` and `export/mod.rs` are rustfmt-clean.
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header).
- Parity ledger: 406 rows, 381 verified, 25 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1861** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (scene GLB export entry; root-selection bug fixed)

- `src/export/scene_glb.rs` is the port of `GlbModelExporter`, the scene-level GLB export entry,
  with the `MaterializedSceneCollection` -> `GlbLevelPlan` adapter. Nine tests. New parity row
  `EXP-GLB-SCENE-001`, **已实现但未验证** (no real-file export has run).
- **I had a real bug in root selection and the reference corrected me.** I treated a Transform's
  father as an ordinary hierarchy link. `GameObjectExtensions.GetRoot` actually climbs while the
  parent **has a GameObject**:
  ```csharp
  ITransform? parent = root.Father_C4P;
  if (parent is null || parent.GameObject_C4P is null) { break; }
  ```
  So a Transform whose father has no GameObject **is itself a root**. This matters: the
  GameObject-less father emits nothing (`AddGameObjectToScene` returns before its child loop), so
  without the rule its entire subtree is unreachable. `build_scene_plan` now does the climb,
  bounded by the collection length so a father cycle cannot hang the walk. **Twice now the walk's
  entry condition, not its recursion, was where I was wrong - read both.**
- A Transform with no resolved values reads back as the **identity** (`ToTransformation` reads
  generated properties whose unset values are zero/identity/unit), not as "no transform".
- **Fixture trap: a Transform's `m_Father` names the parent TRANSFORM, not its GameObject.** My
  first fixture passed GameObject indexes, every edge failed to resolve, and the export silently
  produced four root nodes. If a scene export emits all-roots with no transforms, check this
  first.
- **Next steps for the scene export, in order:**
  1. Run a real-file scene export and compare against reference output - this is what would move
     `EXP-GLB-SCENE-001` to 已验证.
  2. Wire geometry selection. The reference gets a mesh through
     `BuildParameters.TryGetOrMakeMeshData` (an `IMesh` -> `MeshData` build plus cache), so the
     caller must resolve each GameObject's `MeshFilter`/`Renderer` fields and pass per-node
     `MeshData`; `SceneCollectionGraph::mesh` is where the selection belongs.
  3. Register the export in the project export pipeline alongside the per-asset GLB exporters.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
  `glb.rs`, `glb_level.rs`, `scene_glb.rs` and `export/mod.rs` are rustfmt-clean.
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header); both `glb_level.rs` and `scene_glb/tests.rs` have a `json_chunk` helper.
- Parity ledger: 406 rows, 381 verified, 25 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1858** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

- `build_scene_plan` in `src/resources/glb_level.rs` adapts a materialized scene collection to a
  `GlbLevelPlan`, reading through a small `SceneCollectionGraph` trait so the walk is testable
  against the committed reference rows without building a whole scene. Eleven tests pass.
- **A rule worth knowing: a Transform with no GameObject PRUNES ITS SUBTREE.** The source's
  `AddGameObjectToScene` does `if (gameObject is null) { return; }` *before* its child loop, so
  neither the node nor its descendants are emitted. My first test asserted the opposite
  (skip the node, keep the children) and failed; the implementation was right. If you touch this
  walk, keep the early return - descending anyway emits a subtree the reference never visits.
- **The remaining work for the scene GLB export is now only the export entry.** All the pieces
  exist: `build_scene_plan` (input), `build_scene_document` (node graph), `attach_plan_geometry`
  (geometry). What is missing is the port of `GlbModelExporter.ExportModel`, which takes a
  `SceneHierarchyObject`/`PrefabHierarchyObject`, calls `GlbLevelBuilder.Build(assets, isScene)`
  and writes through `GlbWriter.TryWrite`. The port's counterparts are
  `WorkspaceSceneHierarchies` (`scenes()`, `scene_asset_identities_in_collection_order`) and
  `MaterializedSceneCollection` (`transforms`, `game_objects`, `transform_of`), whose
  `MaterializedTransform` already carries `game_object`, `father`, `children` and
  `values: Option<SceneTransformValues>` with local position/rotation/scale. **Wire those into
  `SceneCollectionGraph` and register the export.**
- `EXP-GLB-001` and `RES-MODEL-001` keep their statuses until that entry exists; the scene path
  carries no parity row yet.
- **Never format `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`).
  `glb.rs` and `glb_level.rs` are rustfmt-clean and safe.
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header); `glb_level.rs` has a `json_chunk` test helper.
- Parity ledger: 405 rows, 381 verified, 24 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1849** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (scene geometry attaches; ledger claim corrected)

- `attach_plan_geometry` in `src/resources/glb.rs` closed the seam between the two halves of the
  scene builder: the plan decides which sub-meshes a node wants, and this fills them with real
  primitives (positions, normals, tangents, UVs, colours, skin lanes, index accessor). Two tests
  pin it, including the two behaviours a naive version gets wrong - missing mesh data warns and
  skips rather than failing the build, and an out-of-range sub-mesh index is reported rather than
  panicking.
- **The scene GLB export still needs its integration, and the reference shows exactly what it
  is.** `AssetRipper.Export.PrimaryContent/Models/GlbModelExporter.cs` drives
  `GlbLevelBuilder.Build(assets, isScene)` from `SceneHierarchyObject` and `PrefabHierarchyObject`
  and writes through `GlbWriter.TryWrite`. **That is the next unit of work**; the port has the
  counterparts (`WorkspaceSceneGameObjectHierarchy`, `WorkspaceSceneTransformHierarchy` in
  `src/processing/scene_hierarchy.rs`). Until it lands, `EXP-GLB-001` and `RES-MODEL-001` stay
  non-verified and the scene path carries no parity row.
- **`RES-MODEL-001`'s gap list was stale and is corrected.** It claimed `GlbLevelBuilder`,
  `GlbTerrainBuilder` and the `GlbWriter` size guard were all unported; all three are implemented
  and I verified each before editing. Its own evidence column already mentioned the
  `GlbLevelBuilder` kernels, so the cell contradicted its own row.
- **That was one bad cell, not a pattern.** Every other row containing "unported" was read in
  context and is accurate and scoped. Before "correcting" another one, read the cell in full - the
  wording is usually about a named sub-feature, not the whole capability.
- Both `glb.rs` and `glb_level.rs` are rustfmt-clean and safe to format. **Never format
  `src/resources/glb_tests_01.rs`** (nor `main_tests_01.rs`/`main_tests_03.rs`); it is an indented
  fragment.
- **A GLB is binary: decode the JSON chunk before asserting on text** (12-byte header, then the
  chunk header). `glb_level.rs` has a `json_chunk` test helper.
- Parity ledger: 405 rows, 381 verified, 24 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1846** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (scene plan emits a multi-node glTF document)

- Both halves of the scene builder now exist and are reference-checked:
  `build_scene_document` in `src/resources/glb_level.rs` emits a multi-node glTF container from a
  `GlbLevelPlan`, and the emitted root transform
  (`"translation":[-1.0,2.0,3.0]`, `"rotation":[0.0,-0.70710677,-0.0,0.70710677]`) matches the
  reference rows in `glb_level_fixture.txt` exactly, so `DIFF-GLB-QUATERNION-SIGN-001` is now
  visible in emitted output.
- **A GLB is binary: decode the JSON chunk before asserting on text.** 12-byte header, then the
  chunk header; the whole buffer is not UTF-8, so `from_utf8_lossy` over the file compares
  mojibake. `glb_level.rs` has a `json_chunk` test helper for this.
- The document writer lives in `glb.rs` (`SceneDocument::into_glb`, `node_transform_json`,
  `json_float`, `escape_json` now public) so the accessor/buffer-view encodings have one
  implementation. `glb_level.rs` composes nodes only; geometry stays with the caller that holds
  the decoded `MeshData`.
- **Still open, and the next real step:** `glb_level.rs` is **not wired into an export path** and
  carries no parity row, so `EXP-GLB-001` stays 已实现但未验证. Wiring it means feeding decoded
  meshes through the plan's `subset_indices` and appending real primitives to `SceneDocument`.
- **Geometry lanes for a scene build are uncompared.** I tried attaching a class-43
  `MeshFilter`/`Renderer` to the probe hierarchy so the reference would emit `GMESH`/`GPOS`/`GIDX`
  rows, and reverted the attempt: the generated 2019 API differs from the modern one (2019
  `VertexData` uses `Channels`/`Data`/`VertexCount`; `ChannelInfo` fields are capitalised `Byte`;
  the channel list starts empty and must be filled), and it ended in a `NullReferenceException`
  with **no AssetRipper frame**, i.e. a fault in the probe fixture, not in the reference. If you
  retry, start from the working `TerrainCase` geometry path in the same probe rather than from a
  hand-built class-43 Mesh. Until then, do not claim scene geometry is verified.
- **Never run rustfmt on `src/resources/glb_tests_01.rs`**, `src/main_tests_01.rs`, or
  `src/main_tests_03.rs`; all three are indented fragments. `src/resources/glb.rs` and
  `src/resources/glb_level.rs` *are* rustfmt-clean and safe to format.
- Parity ledger: 405 rows, 381 verified, 24 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501 (`load_game` does not run `process_workspace_scenes_and_prefabs`).
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1844** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (scene walk verified against the reference runtime)

- `src/resources/glb_level.rs` matched the shipped `GlbLevelBuilder.Build` **node for node**,
  verified against the reference runtime and not against a reading of its source. The new probe
  mode `dotnet run --project tools/probes/res-glb/ResGlbProbe.csproj -c Release -- level` builds a
  two-root hierarchy, drives the real `Build`, and dumps every node; the rows are committed as
  `src/resources/glb_level_fixture.txt`.
- **Three rules confirmed, not inferred:** depth-first source order
  (`Root, Child, Grandchild, Second`); the `parentNode is not null || IsScene` rule (an **asset**
  build leaves roots at identity, a **scene** build converts them); and scale unchanged by the
  conversion.
- **`DIFF-GLB-QUATERNION-SIGN-001` is now corroborated end-to-end.** The scene rows show a
  +90-degree Y rotation arriving as `-0.70710677` about Y, and +90 about Z as `-0.70710677` about
  Z, while +60 about X keeps its sign - the `(1, -1, -1)` product seen through a real scene build.
- **Reference behaviour worth knowing:** a scene build validates the local rotation, so an
  all-zero quaternion makes SharpGLTF throw `(Parameter 'rotation')`. Use valid rotations in probe
  fixtures; this is third-party validation of the reference's input, not a port gap.
- **The `level` probe fixture took several attempts; these are the traps.**
  `ProcessedAssetCollection` does not register factory-created assets, and a PPtr resolves
  *through the collection*, so links fail with "Can't find transform component" until they are
  registered. `AddAsset` is protected and its dictionary lives on the base `AssetCollection`, so
  walk `BaseType`. Hand-built `ComponentPair` PPtrs are untyped; the generated `AddComponent`
  extension's `SetAsset` call is what types them. `Children_C4P` views `Children_C4`, so a child
  must be added to the parent's list as well as pointed at its father.
- **Next step for `EXP-GLB-001`:** the traversal is reference-verified, so the remaining work is to
  feed the plan into the GLB document writer and emit a real multi-node glTF scene, then compare
  that document against the same probe output (which already dumps meshes and accessors).
- Parity ledger: 405 rows, 381 verified, 24 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
  `glb_level.rs` is **not** wired into an export path and carries no parity row yet.
- `/Scenes/View` still returns 501; the obstacle remains that `load_game` does not run
  `process_workspace_scenes_and_prefabs` (only the export workflow does).
- **Never run rustfmt on `src/resources/glb_tests_01.rs`**, `src/main_tests_01.rs`, or
  `src/main_tests_03.rs` - all three are indented fragments and a whole-file run rewrites them.
- Corpus replays are local-only and have not run in these rounds.
- Verified: **1842** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Previous checkpoint - 2026-10-16 (glTF quaternion sign bug fixed; level plan ported)

- **A real correctness bug was found and fixed: `DIFF-GLB-QUATERNION-SIGN-001`.** The glTF
  quaternion conversion had every axis sign inverted. The reference multiplies the axis by
  `axisFlipScale * CoordinateSpaceConversionScale` = `-1 * (-1, 1, 1)` = `(1, -1, -1)`; the port
  applied the coordinate scale alone, `(-1, 1, 1)`. Fixed in `src/resources/glb.rs`.
- **The lesson worth carrying: a test derived from the code it tests proves nothing.** The helper
  had no callers and no test, and the single test that mentioned it asserted the *same wrong
  value* - the expectation had been written from the implementation. It was found only because
  the scene-level builder needed local rotations. When adding an expectation, derive it from the
  reference or from the format, never from the implementation.
- **How the reference evidence was obtained**, since the recipe matters: the temp reference build
  that probes point at gets cleaned up, so rebuild it with
  `dotnet build AssetRipper\Source\AssetRipper.GUI.Free\AssetRipper.GUI.Free.csproj -c Release -o %LOCALAPPDATA%\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release`.
  Then `dotnet run --project tools/probes/res-glb/ResGlbProbe.csproj -c Release -- quat` prints
  reference-runtime rows for the coordinate conversions (bind by reflection; the type is
  internal). Two traps: `System.Numerics` vectors/quaternions expose **fields, not properties**,
  and an internal type must be loaded explicitly before it appears in the assembly set.
- **`src/resources/glb_tests_01.rs` is NOT rustfmt-clean at HEAD and must never be formatted
  wholesale.** It is written as an indented fragment, like `main_tests_01.rs`. Running rustfmt on
  it reformatted all 976 lines and turned a 136-line addition into a 1008/874 rewrite; that had
  to be undone by splicing in the restored file with `git show HEAD~1:<path>`. **Add to it by
  hand, at the same 4-space indentation, and never run rustfmt on it.**
- **New this round:** `src/resources/glb_level.rs` ports `GlbLevelBuilder.Build`'s traversal as an
  explicit plan (one node per Transform in source order, parent links, the "local transform only
  for a child or a scene root" rule, dynamic-vs-static mesh choice, node bound). Five tests pass.
  It is **not wired into an export path and not marked verified** - `EXP-GLB-001` keeps its status.
  **The next step for that row** is to feed this plan into the GLB document writer so a scene
  build emits real multi-node glTF, then verify it against the reference probe.
- Parity ledger: 405 rows, **381 verified, 24 non-verified**, DUPES=0, MALFORMED=0, gate OPEN.
- `/Scenes/View` still returns 501; the obstacle remains that `load_game` does not run
  `process_workspace_scenes_and_prefabs` (only the export workflow does).
- Corpus replays are local-only and have not run in these rounds, so `acceptance-corpus.md`
  per-sample verdicts predate recent changes.
- Verified: **1841** library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (`EXP-GLB-001` gap list corrected; one real gap left)

- `EXP-GLB-001` stays 已实现但未验证, but its 已知差异 column was **wrong about two of four named
  gaps** and is now accurate:
  - **`GlbTerrainBuilder` is ported, wired and tested** - `build_terrain_glb_with_limits` is
    reached from `src/export/res_wiring.rs` and covered by
    `terrain_plan_reads_the_heightmap_and_scale_through_the_source_builder`;
    `build_navmesh_glb_with_limits` is covered by
    `navmesh_plan_reads_height_meshes_and_emits_position_only`. Do not re-implement these.
  - **Morph targets / blend-shape emission are absent from the reference GLB module itself**
    (re-verified: `AssetRipper.Export.Modules.Models` has no such code). That is a baseline
    boundary, not a port gap.
  - **The one real gap is `GlbLevelBuilder.Build`** - the 301-line scene-level builder that walks
    Transform hierarchies over many assets and assembles the glTF scene graph. Its supporting
    kernels are already ported (`references_dynamic_mesh`, `subset_indices`, and
    `GlbLevelBuilder.GetTextures` in `src/resources/material.rs`), so **start from those**.
  - **A stale "unported" list is a doc defect.** Before implementing a named gap, check whether it
    exists; and after implementing one, remove it from the list.
- `/Scenes/View` still returns 501. The frozen view needs a scene's name, path, GUID and
  collection links; `SceneDefinitionPlan` carries name/path/guid and
  `WorkspaceSceneDefinition::collection_file_indices()` carries the membership. The obstacle is
  that `load_game` never runs `process_workspace_scenes_and_prefabs` - only the export workflow
  does - so this needs that processing added to the load path, not just a route.
- Corpus: the anonymous ledger (`docs/acceptance-corpus.md`) is intact and privacy-clean - no
  original names, no absolute user paths, no content. Corpus replays are local-only and have not
  been re-run in these web-surface rounds, so per-sample verdicts there predate recent changes.
- Parity ledger: 404 rows, 380 verified, 24 non-verified, DUPES=0, MALFORMED=0, gate OPEN.
- **Process reminders:** commit a passing unit before cleanup; PowerShell file writers introduce
  CRLF into LF-normalised files (Git warns, and `git diff --check` catches trailing blank lines).
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (mesh view closed; non-verified queue at 24)

- `RES-MESH-VIEW-001` is 已验证: its recorded gap was "no version branch is asserted beyond the
  caller-supplied layout inputs", and the production-layout mesh fixture plus the
  `/Assets/Model.glb` e2e now supply exactly that assertion for 2019.4. Other versions stay open
  and are still listed in the row.
- `EXP-GLB-001` deliberately **stays** 已实现但未验证: its gaps are unported features
  (`GlbTerrainBuilder`, `GlbLevelBuilder`, morph targets, blend-shape emission), not a missing
  test, so a stronger test does not close it.
- Parity ledger: 404 rows, **380 verified, 24 non-verified**, DUPES=0, MALFORMED=0, gate OPEN.
- **When closing a 已实现但未验证 row, read its own 已知差异 and verification columns first.** The
  gate is usually a specific named assertion; closing anything else does not count, and closing
  the wrong thing is worse than leaving it open.
- **Next, in priority order:**
  1. `EXP-GLB-001`'s named gaps: `GlbTerrainBuilder`, `GlbLevelBuilder` (scene-level GLB), morph
     targets, blend-shape emission. These are real implementation work.
  2. `PROC-ANIM-*` and `RES-ANIM-001`, then `RES-TEXTURE-001`, `RES-MODEL-001`,
     `EXPORT-UNITY-001` - all `实现中` or 已实现但未验证 and needing implementation.
  3. `/Scenes/View` still returns 501. The frozen view needs a scene's name, path, GUID and
     collection links; `SceneDefinitionPlan` (`src/processing/scene_collections.rs`) already
     carries name/path/guid and `WorkspaceSceneDefinition::collection_file_indices()` carries the
     membership. The work is that `load_game` does not run `process_workspace_scenes_and_prefabs`
     (only the export workflow does), so the scene view needs that processing added to the load
     path - a larger change than a single route.
- **Process reminders:** commit a passing unit before any cleanup; and editing a LF-normalised
  file with a PowerShell writer introduces CRLF, which Git's warning catches - normalize before
  committing.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (failed-file pages, and a load-path divergence fixed)

- `PRODUCT-FAILEDFILE-TAB-001` is 已验证. `/FailedFiles/View` and `/FailedFiles/StackTrace` are
  implemented and exercised end to end against the committed `ufs_v6_lzham_block` fixture.
- **DIFF-FAILEDFILE-LOAD-REFUSAL-001 is closed, and it was a real divergence, not a missing
  page.** The routes had been unreachable because `load_game` applied the *export* refusal on the
  *load* path: a game with one bad storage block could not be opened at all. `LoadFailurePolicy`
  now separates the two - browsing tolerates a retained failure (as the reference's
  `LoadAndProcess` does), exporting refuses. **When adding a load-time check, ask which operation
  the reference gates, not just whether the check is defensible.**
- The failed entry attaches to the bundle that **owns** the failed container, not the root; the
  e2e asks the tree for that bundle's own path. Bundle paths are chains of child indexes from the
  root, not flat workspace indexes.
- Parity ledger: 404 rows, 379 verified, 25 non-verified, gate OPEN.
- **Next candidates:**
  1. `/Scenes/View` extras and `/Export/PrimaryContent`.
  2. The 25-row non-verified queue. Most rows are `实现中` (RES-TEXTURE-001, RES-MODEL-001,
     EXPORT-UNITY-001, PROC-ANIM-001, PROC-SCENE-001, OBJ-REF-001, SER-TT-ABSENT-001,
     TYPE-SCRIPT-001, IO-STREAM-001) and need **implementation**, not verification - that is where
     the remaining product risk sits.
  3. `PRODUCT-GUI-001` can move toward 已验证 now that every frozen tab and the failed-file pages
     are covered; audit which entry points remain.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (tab order aligned and pinned)

- The asset tab order now matches the frozen `ViewPage` exactly: Information, Audio, Image,
  Model, Text, Font, Video, Yaml, Json, Hex, Dependencies, Development. The port had a different
  sequence before this round.
- `assert_frozen_asset_tab_order` (in the routes e2e) pins it. Two things it encodes: an asset
  renders a *subsequence* because every tab is conditional, and the frozen display names are
  **`Yaml`/`Json`** from `Localization.Yaml`/`Localization.Json`, not `YAML`/`JSON`.
- **Next candidates, in the order I would take them:**
  1. `/FailedFiles/Stack` - the frozen page links a per-failure stack trace; the view route
     exists, the stack route does not.
  2. `/Scenes/View` extras and `/Export/PrimaryContent`.
  3. The 25-row non-verified queue. Many rows are `实现中` (RES-TEXTURE-001, RES-MODEL-001,
     EXPORT-UNITY-001, PROC-ANIM-001, PROC-SCENE-001, OBJ-REF-001, SER-TT-ABSENT-001,
     TYPE-SCRIPT-001, IO-STREAM-001) - those need real implementation, not verification, and are
     where the remaining product risk actually sits.
  4. `PRODUCT-GUI-001` can move toward 已验证 once its remaining entry points are covered.
- **Process reminder from last round:** commit a passing unit before any cleanup. I lost an
  uncommitted fixture to a careless `git checkout --` and had to rewrite it.
- Parity ledger: 403 rows, 378 verified, 25 non-verified, gate OPEN.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (model tab verified; all frozen tabs complete)

- `PRODUCT-ASSET-MODEL-TAB-001` is 已验证. **Every tab in the frozen `ViewPage` list is now
  implemented**, and all but the ordering detail below are verified.
- The mesh blocker is gone: `make_production_mesh_file` writes the full production class-43
  field list. Two payload facts to remember if this fixture is ever touched: the ten
  `PackedBitVector`s are ordered so that `m_FloatColors` is **eighth** (only `m_Vertices`,
  `m_UV`, `m_Normals`, `m_Tangents`, `m_FloatColors` carry `m_Range`/`m_Start`), and the reader
  aligns the next 4-byte field after each trailing `m_BitSize` byte.
- E2e model assertions are in place: the page links the route, and the response is a GLB with
  magic `glTF`, version 2, and a declared length equal to the body.
- **Next:** align the port's asset tab order with the frozen `ViewPage` order (Information,
  Audio, Image, Model, Text, Font, Video, Yaml, Json, Hex, Dependencies, Development) - the port
  currently emits them in a different sequence. Then `/Scenes/View` extras,
  `/FailedFiles/Stack`, and `/Export/PrimaryContent`.
- **Process warning.** I lost an uncommitted fixture this round by running `git checkout --` on
  `src/main_tests_03.rs` to drop formatting noise, and had to rewrite it. Commit a passing unit
  before doing any cleanup, and never revert a path whose uncommitted contents are not already
  safe somewhere.
- The e2e fixture set is nine inputs with root collections ordering
  `z-audio(0), z-font(1), z-mesh(2), z-textasset(3), z-texture(4), z-unknown(5), z-video(6)`.
- Parity ledger: 403 rows, 378 verified, 25 non-verified, gate OPEN.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 199 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (model tab implemented; blocked on a mesh fixture)

- **Start here.** `render_asset_model`, `/Assets/Model.glb` and the model tab are implemented and
  build cleanly, but are **not verified** and have no 已验证 parity row. The blocker is a
  *fixture*, not product code: the committed `make_mesh_file` is a ten-node class-43 sketch that
  decodes only under the synthetic layout source. Under production layouts it fails with
  `Mesh.m_Shapes.vertices array count 6 requires at least 240 bytes (40 per element), but only 80
  remain`, falls back to `UnreadableObject`, and the route answers 404.
- **Concrete next step:** write a production-layout class-43 fixture (dump the tree from
  `reference_class_layouts().tpk_trees(43, &version)` first). It needs `m_Name`, `m_SubMeshes`,
  `m_Shapes`, `m_IndexBuffer`, `m_IndexFormat`, `m_VertexData` with `m_VertexCount`, `m_Channels`
  and `m_Streams`, and `m_DataSize`. Then restore the route assertions: `model/gltf-binary`, and
  a GLB header whose magic is `glTF`, version is 2, and declared length equals the body length.
  Those assertions are written out in this round's git history if useful.
- Implementation notes kept: `MeshView::from_value` + `read_mesh_data` feed `build_glb`;
  `EndianType` is in `resources::vertex_data_blob` while the header endian is
  `io::binary::Endian`; `UnityVersion` has no `Default`, so a missing version string needs an
  explicit fallback. `has_model` is the type test alone, unlike the font and video gates.
- **Fixture trap, fifth time** (texture, audio, font, video, mesh): always dump the production
  tree and mirror it. The e2e fixture set is now nine inputs with root collections ordering
  `z-audio(0), z-font(1), z-mesh(2), z-textasset(3), z-texture(4), z-unknown(5), z-video(6)`.
- Parity ledger: 402 rows, 377 verified, 25 non-verified, gate OPEN.
- **After the model tab:** align the port's asset tab order with the frozen `ViewPage` order
  (Information, Audio, Image, Model, Text, Font, Video, Yaml, Json, Hex, Dependencies,
  Development), then `/Scenes/View` extras, `/FailedFiles/Stack`, `/Export/PrimaryContent`.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 198 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (video tab + route verified)

- `PRODUCT-ASSET-VIDEO-TAB-001` is 已验证. `/Assets/Video` applies both frozen gates
  (`TryGetExtensionFromPath` from `m_OriginalPath`, `TryGetContent` over
  `m_ExternalResources`) and the e2e asserts `video/mp4` plus the exact range bytes.
- **Correction to the previous hand-off:** its "open item" about `has_video` disagreeing with
  the unit-tested view was my own diagnostic error, not a product gap. I had printed
  `state.objects.get(0, 0)` (a MonoBehaviour) instead of the clip at `(file 9, object 0)`.
  Enumerating the workspace for class-329 objects showed the view and the tree agree. When
  debugging a gate, print the object the code under test actually resolves.
- Class-329 facts worth keeping: `m_ExternalResources` is a `StreamedResource` spelled
  `m_Source`/`m_Offset`/`m_Size` (not `StreamingInfo`'s `path`/`offset`/`size`), and its
  `m_Size` is `UInt64` while `StreamingInfo.size` is `UInt32`. The production preamble is
  thirteen fields deep before that descriptor, with a trailing `m_HasSplitAlpha`.
- Tag tabs now complete except the model tab. The frozen `ViewPage` also orders tabs as
  Information, Audio, Image, Model, Text, Font, Video, Yaml, Json, Hex, Dependencies,
  Development - the port's current order still differs, worth aligning in a later pass.
- The e2e fixture set is now eight inputs; root collections order
  `z-audio(0), z-font(1), z-textasset(2), z-texture(3), z-unknown(4), z-video(5)`.
- Parity ledger: 402 rows, 377 verified, 25 non-verified, gate OPEN.
- **Remaining:** model tab (`/Assets/Model.glb`, gate `asset is IMesh`, needs `GlbMeshBuilder` +
  `GlbWriter` on the web path); then `/Scenes/View` extras, `/FailedFiles/Stack`,
  `/Export/PrimaryContent`.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 198 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (VideoClip decode layer; route/tab not yet verified)

- **Open item first.** `VideoClipView` (`src/resources/video_clip.rs`, 4 unit tests),
  `/Assets/Video`, `read_asset_video` and the video tab are implemented, but the route/tab are
  **not verified end to end** and correctly have no 已验证 parity row. Symptom: the e2e mounts a
  real class-329 asset that decodes as `TestVideo`, yet `TreeAsset.has_video` is false there while
  `VideoClipView::from_value(...).check_integrity()` is true in unit tests on the same shape.
  The e2e video route assertions were removed rather than written to pass. **Start here:** compare
  the payload the tree sees against the one the unit test builds - most likely the tree reads a
  different object than the collection row the test resolves, or `has_video` is computed before
  the layout the route uses is applied.
- Two general findings landed from this work, worth keeping:
  - Class 329 spells `m_ExternalResources` as a `StreamedResource` with
    `m_Source`/`m_Offset`/`m_Size`, not the `StreamingInfo` spelling `path`/`offset`/`size`.
    `VideoClipView` accepts both.
  - `StreamedResource.m_Size` is `UInt64` while `StreamingInfo.size` is `UInt32`; the normalizer
    narrows only when the value fits and otherwise keeps the wide value so the error surfaces.
- **The recurring fixture trap, now four times (texture, audio, font, video).** Always dump the
  production tree from `reference_class_layouts().tpk_trees(class_id, &version)` and mirror it
  exactly, including trailing fields - the reader walks the whole node list, so a missing tail is
  a hard "truncated input" error. The class-329 preamble is 13 fields deep before
  `m_ExternalResources`.
- The e2e fixture set is now eight inputs; root collections order
  `z-audio(0), z-font(1), z-textasset(2), z-texture(3), z-unknown(4), z-video(5)`.
- Parity ledger: 401 rows, 376 verified, 25 non-verified, gate OPEN.
- **Remaining:** model tab (`/Assets/Model.glb`, gate `asset is IMesh`, needs `GlbMeshBuilder` +
  `GlbWriter` on the web path); then `/Scenes/View` extras, `/FailedFiles/Stack`,
  `/Export/PrimaryContent`.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1834 library + 198 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (font tab + route)

- `PRODUCT-ASSET-FONT-TAB-001` is 已验证. Route `/Assets/Font` ports the frozen magic table
  exactly (`OTTO`, `00 01 00 00`, `ttcf`, else `dat`/octet-stream); data under four bytes counts
  as no font data. `read_asset_font` returns a named `FontData` struct.
- **The recurring fixture trap, now seen three times.** A hand-written fixture that only the
  *synthetic* test layout accepts will decode as `Unreadable*` once production layouts apply.
  The production class-128 layout puts **13 fields before `m_FontData` and 4 after it**; a tree
  that stops at `m_FontData` fails at `m_FontSize` with "truncated input (requested 4 bytes,
  0 remain)". Always dump the production tree from
  `reference_class_layouts().tpk_trees(class_id, &version)` and mirror it, then pin it with a
  focused test under `SourceFactoryLayoutGuard::use_source_factory()`.
- The e2e fixture set is now seven inputs; root collections order
  `z-audio(0), z-font(1), z-textasset(2), z-texture(3), z-unknown(4)`. Update those indexes when
  adding inputs.
- Parity ledger: 401 rows, 376 verified, 25 non-verified, gate OPEN.
- **Remaining tabs:** model (`/Assets/Model.glb`, gate `asset is IMesh`; needs `GlbMeshBuilder`
  + `GlbWriter` on the web path) and video (`/Assets/Video`, gate `asset is IVideoClip` with
  `CheckIntegrity`; needs `TryGetExtensionFromPath` + `TryGetContent`). Those two complete the
  frozen `ViewPage` tab list. After that: `/Scenes/View` extras, `/FailedFiles/Stack`,
  `/Export/PrimaryContent`.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs`.
- Verified: 1830 library + 197 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (audio tab + route, Development tab)

- `PRODUCT-ASSET-AUDIO-TAB-001` is 已验证 and the Development tab is added. Route
  `/Assets/Audio` reuses `try_decode_audio` + `convert_ogg_to_wav_with_limit`; streamed clips
  read through `resolve_and_read_streamed_resource_range` (64-bit offset/size).
- **Fixture trap to remember:** the production class-83 layout at 3.5.0f1 is
  `m_Name, m_Format, m_Type, m_3D, m_UseHardware, m_Stream, m_AudioData` - `m_AudioData` is NOT
  directly after `m_Name`. `make_audio_clip_file` writes the short shape and only decodes under
  the synthetic layout source. Use `make_generated_audio_clip_file` for anything that runs under
  production layouts, and remember the reader aligns an `int` after a bool run.
- The e2e fixture set is now six inputs; root collections order as
  `z-audio, z-textasset, z-texture, z-unknown`, so collection indexes are 0/1/2/3. Keep the
  unknown-class view at `"I":3` and the texture view at `"I":2` when adding more.
- Parity ledger: 400 rows, 375 verified, 25 non-verified, gate OPEN.
- **Remaining tabs:** model (`/Assets/Model.glb`, gate `asset is IMesh`), font (`/Assets/Font`,
  gate `asset is IFont` with readable data), video (`/Assets/Video`, gate `asset is IVideoClip`).
  Then the frozen `ViewPage` tab list is complete. After that: `/Scenes/View` extras,
  `/FailedFiles/Stack`, and `/Export/PrimaryContent`.
- Do not run rustfmt on `src/main_tests_01.rs` or `src/main_tests_03.rs` (neither is clean at
  HEAD; whole-file runs reformat hundreds of unrelated lines).
- Verified: 1830 library + 196 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (YAML tab + route)

- `PRODUCT-ASSET-YAML-TAB-001` is 已验证. `render_asset_yaml` in `src/web/state.rs` reuses the
  project export's object-document path, so the tab and `/Assets/Yaml` serve the same text the
  exporter writes. Anchor is `GetMainExportID(class_id, 0)` = `class_id * 100000`.
- Deliberate readings to preserve: PPtr fields render null (the frozen standalone walker has no
  export map either), and the download name is `<best name>.asset` per the frozen tab.
- Parity ledger: 399 rows, 374 verified, 25 non-verified, gate OPEN.
- **Next in the same style:** the remaining *conditional* tabs, which need `has_*` flags on
  `TreeAsset` plus a route each. The frozen URL constants are `Base + "/Audio"` (plus
  `?Extension=wav` for the playback source), `"/Model.glb"`, `"/Font"`, `"/Video"`; their gates
  are `asset is IAudioClip`, `asset is IMesh`, `asset is IFont` with readable data, and
  `asset is IVideoClip` with integrity. The Development tab needs no route: it is a table of
  `Asset.GetType().Name`.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD).
- Verified: 1830 library + 195 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (JSON tab + DefaultJsonWalker port)

- `PRODUCT-ASSET-JSON-TAB-001` is 已验证. New `src/export/json_walker.rs` ports
  `AssetRipper.Export.PrimaryContent.DefaultJsonWalker`, and `GET /Assets/Json` plus the asset
  page's JSON tab are wired. Eight unit tests pin the reference's shape rules; the routes e2e
  checks both the inlined tab and the route body.
- Watch out for the two easy-to-miss rules if you extend the walker:
  - `SerializedValue::Char(u16)` is a UTF-16 code unit and renders as a character, not a number.
  - A value following an explicit prefix uses `visit_value_inline`, not `visit_value`; the latter
    re-positions and corrupts the output. `visit_value`/`visit_string`/`visit_bool` position,
    `visit_value_inline` does not.
- Parity ledger is 398 rows, 373 verified, 25 non-verified, gate OPEN.
- **Next, in the same style:** the `/Assets/Yaml` route and YAML tab (the frozen `YamlTab` is
  also `Enabled => true`, and the port already has Unity YAML machinery in
  `src/export/value_yaml`), then the remaining conditional tabs — audio, model, font, video,
  development — which need `HasAudioData`/`HasModelData`/`HasFontData`/`HasVideoData`
  equivalents, and the frozen URL constants are `Base + "/Audio"`, `"/Model.glb"`, `"/Font"`,
  `"/Video"`, `"/Yaml"`.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD).
- Verified: 1830 library + 195 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (image route landed and pixel-verified)

- `PRODUCT-ASSET-IMAGE-ROUTE-001` is 已验证. `GET /Assets/Image` serves a decoded class-28
  texture, the asset page shows the image tab, and the routes e2e test **inflates the served
  PNG and compares pixels** against the source RGBA bytes rather than only checking the magic
  header. Touched: `src/web/{tree,state,routes,pages}.rs`, `src/web_paths.rs`.
- The routes e2e now mounts six inputs; adding the texture collection shifted the root
  collection indexes, so the unknown-class view is `"I":2` (was `"I":1`) and the root
  collection assertion lists three names. Keep those in sync if more root fixtures are added.
- Remaining web work: the other asset tabs (audio, model, font, video, YAML, JSON,
  development), scene/OriginalPath/OverridePath/AssetBundleName/custom rows,
  `/FailedFiles/View` + `/StackTrace`, `/Scenes/View`, `/Export/PrimaryContent`. The
  `/Assets/Image` handler is a good template: resolve the tree path, gate on a `has_*` flag,
  delegate to a `WebState` method.
- Non-verified queue is 25 of 397; `PRODUCT-GUI-001` stays 已实现但未验证 until its remaining
  tabs and entry points are covered.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD).
- Verified: 1822 library + 195 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (texture fixture SOLVED; image route unblocked)

- The texture fixture blocker is **closed**. It decodes through both the file TypeTree route
  and the full `load_game` production route, and the production leg is asserted
  (`the texture must decode under production layouts too`).
- Root cause: the fixture builder wrote every node with the six-argument `put_node`, which
  hard-codes `meta_flags = 0`, so the dump's `metaFlag` column was silently dropped.
  `field_aligns` ORs `ALIGN_BYTES_FLAG` (0x4000) over a field's node chain and `decode_field`
  pads after it; this record marks `m_DownscaleFallback`, `m_StreamingMipmaps`,
  `m_StreamingMipmapsPriority` and `image data`. Losing the column removed three alignment
  pads and shifted everything after them. The builder now carries `meta_flag` into
  `put_node_with_meta`.
- Debug recipe that found it: a temporary trace in `codec.rs::decode_field` printing each
  field's end offset and `align_after`. All-false on a tree whose dump says otherwise points
  at the builder, not the payload. The payload's `image data` length word is pinned at 80 by
  `debug_assert_eq!`.
- **Next task, now unblocked:** re-land the `/Assets/Image` web route (the arc from rounds
  15-17): `src/web/tree.rs` `has_image`, `src/web/state.rs` `read_asset_image_bytes` +
  `render_texture_image`, `src/web/routes.rs` `/Assets/Image`, `src/web/pages.rs` image
  section, `src/web_paths.rs` `asset_image_url`. It was reverted only because this fixture
  could not be trusted; re-apply and verify the routes e2e.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD).
- Verified: 1822 library + 195 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (texture payload: exact one-byte gap, three leads)

- No code change; tree green at `d9e83e7` with the corrected fixture. The remaining gap is now
  **exactly one byte**, not four:
  - Walking the corrected tree's `metaFlag` column (`ALIGN_BYTES_FLAG = 0x4000`) predicts the
    `image data` length word at offset **80**.
  - The decoder reports `image data requires 134217728` with `23 remain at byte offset 81`.
    `134217728` is `0x08000000`, i.e. the fixture's own length word `8` read one byte late.
    So the reader is one byte **ahead** of the align-derived offset.
  - `StreamingInfo` in this record is `unsigned int offset` / `unsigned int size` /
    `string path` - all 32-bit (the 8-byte offset came from the wrong record).
- Try next, in order:
  1. Empty `m_Name`: writer emits only its 4-byte length word; the reader seems to consume a
     terminator byte. Try a trailing byte **without** re-aligning after it.
  2. Use `make_texture_file` in `main_tests_01.rs` as the oracle - it is the long-working
     builder and writes `m_LightmapFormat` as a single byte with no align before `image data`.
     Compare it field-by-field with the new writer.
  3. Rule out an endianness read before changing any field width.
- Recipe: `TEXFIX_DEBUG=1` prints the production-layout decode result in one run.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD).

## Latest current checkpoint - 2026-10-16 (fixture was wrong; probe bug fixed)

- The "payload 4 bytes short" mystery is solved and it was **the fixture, not the payload**.
  `tools/probes/tpk-trees/Program.cs` kept the *last* entry of a class record's version map
  instead of the latest record at or before the target, so it dumped release root **23374**
  for `2019.4.40f1` when the correct record is root **18204**. The fixed probe now emits
  18204, matching `SourceTpkDatabase::embedded()` exactly.
- `src/resources/texture_typetree_fixture.txt` is **replaced** with the correct 38-node
  record (was 46 nodes from the wrong one). `m_PlatformBlob` / `m_IsAlphaChannelOptional` /
  `m_MipmapLimitGroupName` / `m_MipsStripped` are not part of the 2019.4 class-28 layout;
  all earlier payload work against them was chasing an unused tree.
- Assertions updated to 38 nodes / 20 direct children. The texture payload tail still does
  not satisfy the file-tree leg and the production-layout leg at the same time, and the
  production leg is deliberately not asserted; that is the remaining open item.
- Rule for any future probe that feeds a checked-in fixture: use the *same* selection
  semantics as the implementation under test.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD).
- Verified: 1822 library + 195 bin tests, 0 failed; warnings-denied Clippy exit 0.

## Latest current checkpoint - 2026-10-16 (texture blocker: two 2019.4 trees disagree)

- The "payload 4 bytes short" is **solved as a diagnosis**: it is not a payload bug. Read
  `materialize.rs::class_layout` - the decode layout is `source.tpk_trees(class_id, version)`
  first, and the file TypeTree is only a fallback. Production sets
  `source_tpk = SourceTpkDatabase::embedded()`; the test-only layout source returns `None`
  from `tpk_trees`, which is why only the synthetic-layout runs exercise the file tree.
- The embedded database's 2019.4.40f1 class-28 record is a **different shape** from the
  committed file dump (release root 23374):
  - embedded: `m_ForcedFallbackFormat`, `m_DownscaleFallback`, `m_IgnoreMasterTextureLimit`;
    no `m_PlatformBlob`, no `m_IsAlphaChannelOptional`, no `m_MipmapLimitGroupName`.
  - committed dump: has those three, lacks the first three.
  One payload cannot satisfy both, so every layout permutation moved the error without
  going green.
- **The real task now** (not fixture busywork): decide which 2019.4 Texture2D shape is
  correct. Both the probe and the embedded database claim to answer for `2019.4.40f1`, so
  one lookup is picking the wrong record - a genuine correctness question for real 2019.4
  texture decoding. Recipe: dump both with `TEXFIX_DEBUG=1` (prints a `TPKNODE|` list of the
  production tree) and compare against `src/resources/texture_typetree_fixture.txt`.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD).

## Latest current checkpoint - 2026-10-16 (texture payload: hypotheses ruled out, next lead given)

- No code change this round; the tree is at `306e81e` and green. The value is the narrowed
  search, recorded in full in `docs/validation-log.md`:
  - The **fixture TypeTree is verified correct** — the reader's own parsed view matches the
    dump exactly (`m_PlatformBlob`→`image data`→`m_StreamData` with right parenting).
  - The writer's payload offsets are self-consistent with that order.
  - The decoder still lands on `m_StreamData.path` at **byte offset 92**, and that number did
    **not move across four different payload layouts**. Do not keep re-arranging the tail:
    an offset that is invariant to the preceding bytes means the divergence is elsewhere.
  - **Next lead to check:** the `m_CompleteImageSize` / `m_ImageCount` /
    `m_TextureDimension` gate - the decoder may derive an image byte count from those and
    skip/over-read `image data` before ever reaching `m_StreamData`.
- Debug recipe: run the fixture test with `TEXFIX_DEBUG=1`; the `PRODUCTION|` line prints the
  decode error and offset in one run.
- Do not run rustfmt on `src/main_tests_01.rs` (not clean at HEAD; a whole-file run reformats
  1500+ unrelated lines).

## Latest current checkpoint - 2026-10-16 (texture tree fixed; payload tail open)

- `make_reference_texture_file` now emits a **correctly parented** TypeTree. The dump rows
  were previously written with row-order indices, which re-parented `m_StreamData`'s children;
  the builder now derives sub-node counts from the `level` column, and asserts the release
  root's 22 direct children.
- **Open item, with exact numbers:** the hand-written payload is 4 bytes short at the tail
  under production layouts. `m_PlatformBlob` ends at 88, the decoder wants
  `m_StreamData.path` at 92, and the symptom is
  `field Texture2D.m_StreamData.path has negative array/string length -14806006`
  (`-14806006` = `0xFF1E140A` = the fixture's own pixels `0A 14 1E FF` read as a length).
  Fix the `m_PlatformBlob`/`image data`/`m_StreamData` tail, then the image arc can be
  re-landed.
- Why earlier increments looked green: the test binary's **synthetic layout masked this**.
  Any texture fixture work must be checked under
  `SourceFactoryLayoutGuard::use_source_factory()`, which makes the file TypeTree
  authoritative. The test now records that leg as a measured open item instead of asserting.
- Do **not** run rustfmt on `src/main_tests_01.rs`: it is not rustfmt-clean at HEAD and a
  whole-file run reformats 1500+ unrelated lines (happened twice this round; both reverted).
- Verified: `cargo test --locked --workspace` 1822 lib + 195 bin, 0 failed; warnings-denied
  Clippy exit 0. No product behavior changed; the image implementation stays reverted.

## Latest current checkpoint - 2026-10-16 (reference Texture2D fixture decodes; image arc ready)

- The fixture is **verified working**, not just generated.
  `reference_texture_typetree_decodes_through_the_file_route` builds a class-28 file from
  the committed 46-node dump, loads it through the workspace/object-workspace route the web
  uses, and asserts `(2, 1)`, format 4 and the exact pixel bytes. Bin suite now **195/195**.
- Builder entry point: `main_tests_01.rs::make_reference_texture_file(width, height, data)`
  (plus `TEXTURE_TYPETREE_FIXTURE`), both `pub(super)` so the web tests can use them.
- Two gotchas the next author must not re-hit:
  1. Fixture header comments start with `# NODE|`, so skip `#` lines when parsing the dump.
  2. After stripping `NODE|`, the columns are `[version, level, type, name, byteSize, ...]`
     - type is index 2, byte size index 4.
  3. Payload alignment (from `writer/encode.rs`): strings are `i32` length + bytes +
     align-to-4; `vector`/`TypelessData` arrays are an aligned `i32` length then element
     bytes, aligned again. Missing that trailing alignment shows up as
     `field Texture2D.image data requires <huge> bytes, but only N remain`.
- Next step: re-apply the `/Assets/Image` implementation (row image facts, the route serving
  raw pixels plus `Extension`-encoded formats, and the image section) and switch the web
  end-to-end to `make_reference_texture_file`, then re-run the full battery.
- Note: `make_texture_file` (the 10-node sketch) is still used by the texture export,
  cubemap and settings tests and must not be modified; append a new builder instead.

## Latest current checkpoint - 2026-10-16 (Texture2D TypeTree fixture generated; image arc unblocked)

- The blocker recorded in the entry below is **solved at the source**. New committed probe
  `tools/probes/tpk-trees/` dumps any class's release TypeTree from the reference TPK
  database; its class-28 dump is committed as
  `src/resources/texture_typetree_fixture.txt` (46 nodes, `releaseRoot=23374`,
  2019.4.40f1).
- The real tree puts `m_IsReadable` at node 213, byte size 1, between `m_MipCount` and
  `m_IsPreProcessed` — the hand-written sketch that failed was wrong about placement, which
  is why it kept dying at "byte offset 36 while parsing Texture2D.m_IsReadable".
- Probe API notes (reflection-only, recorded so the next dump is quick): `SourceTpk` lives
  in `AssetRipper.SourceGenerated`; `SourceTpk.GetStream()` → `TpkFile.FromStream(...)
  .GetDataBlob()` → `TpkTypeTreeBlob`; `ClassInformation[].ID` + `.Classes` (version map)
  → `TpkUnityClass.ReleaseRootNode`; nodes via `NodeBuffer[index]` with `TypeName`/`Name`
  as `StringBuffer` indices and `SubNodes` as `ushort[]`. Texture2D's record `Name` is
  `190`.
- The `/Assets/Image` product code is still reverted, so no behavior changed this round.
  Rebuild it against the new fixture (turn the 46 nodes into the synthetic file's TypeTree
  with the exact byte sizes from the fixture).
- Verified: probe exit 0 with 46 NODE rows; `cargo test --locked --bin assetripper-rs`
  194/194 bin tests pass.

## Latest current checkpoint - 2026-10-16 (image tab blocked on fixture, not product)

- **No code change this round.** The `/Assets/Image` arc (image facts on rows, route,
  image section) was written, compiled, and driven to a fully green 193/194 state, then
  deliberately reverted: the last failure is a *fixture* limitation, not a product one.
- Concrete blocker, with evidence, for whoever picks this up:
  1. `GET /Assets/Image` must run under production layouts, which take the file-TypeTree
     route, and that route reads `Texture2D.m_IsReadable`.
  2. The shared synthetic texture builder (`main_tests_01.rs::make_texture_file`) writes a
     10-node TypeTree without that field, so the object decodes as `Unreadable`:
     `truncated input (requested 1 bytes, 0 remain) at byte offset 36 while parsing
     Texture2D.m_IsReadable`. The texture *export* tests pass only because
     `export_texture_to` resolves through the class-fallback (TPK) route, which never
     reads the flag — so their success is not evidence the fixture is web-usable.
  3. Adding the node by hand does not work: the real 2019.4 layout places `m_IsReadable`
     at TypeTree node index **2664** (`src/resources/mesh_typetree_fixture.txt`), i.e. a
     full release-ordered class layout, not a 10-node sketch. Three hand placements were
     tried (before `image data`, after `image data`, node 7 vs 10) and all three kept the
     reader at byte offset 36.
  4. Therefore: do not retry this arc with a hand-written node. Either (a) author a
     complete Texture2D TypeTree fixture from the frozen class database the way other
     fixtures were generated, or (b) pick a different remaining tab whose data the
     existing fixtures already carry (audio/video/YAML/JSON/text-adjacent rows, or
     `/FailedFiles/*`).
  5. Note the shared-builder hazard: `make_texture_file` is used by the texture export,
     cubemap, and settings tests, and appending a byte to its payload broke 5 of them
     (payload tail/padding is asserted). Any change there must be a new builder, not an
     edit.
- Current verified state is unchanged and green: HEAD == origin/main == `cf5f753`,
  working tree clean, bin suite 194/194.

## Latest current checkpoint - 2026-10-16 (asset raw bytes served, hex section)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. Snapshot rows
  carry a binary flag for undecodable payloads with a raw range; `GET /Assets/Binary`
  serves the exact bytes as octet-stream (empty 200 otherwise) and the asset page shows
  the frozen download-only hex section.
- Verification evidence: fixture end-to-end adds the synthetic unknown-class file
  (class 7, `Unknown7_3FCA88C5`, raw `01..08`) plus decoded-asset negatives; full
  `cargo test --locked --workspace` **1822 lib + 194 bin, 0 failed**, 1 ignored; Clippy
  `-D warnings` exit 0; per-file rustfmt and `git diff --check` clean. Full evidence in
  `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: shader/MonoScript texts, eight remaining asset tabs plus the other
  `/Assets/*` data routes, `/FailedFiles/*`, `/Scenes/View`, `/Export/PrimaryContent`;
  then continue down the ledger.

## Latest current checkpoint - 2026-10-16 (asset text served from TextAssets)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. Snapshot rows
  carry file/object indexes and a TextAsset text flag; `GET /Assets/Text` serves script
  bytes (empty 200 otherwise) and the asset page inlines the text with a
  settings-driven download name. Shader/MonoScript texts stay open.
- Verification evidence: fixture end-to-end adds the synthetic TextAsset (`Hero`,
  `print(1)`, `Hero.txt`) plus MonoBehaviour negatives; full `cargo test --locked
  --workspace` **1822 lib + 194 bin, 0 failed**, 1 ignored; Clippy `-D warnings` exit 0;
  per-file rustfmt and `git diff --check` clean. Full evidence in `docs/validation-log.md`.
- Gotcha recorded: the test binary substitutes synthetic TypeTrees unless the
  `SourceFactoryLayoutGuard` is held — the end-to-end holds it (production layouts).
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: shader/MonoScript texts, nine remaining asset tabs plus the other
  `/Assets/*` data routes, `/FailedFiles/*`, `/Scenes/View`, `/Export/PrimaryContent`;
  then continue down the ledger.

## Latest current checkpoint - 2026-10-16 (`PRODUCT-I18N-001` closed, gate 26 to 25)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. Full re-audit of
  the localization row: 23 vendored tables byte-identical to frozen, `language_names.json`
  identical 23/23 to the frozen build's baked dictionary (new `tools/probes/lang-names/`
  reflection dump), new exhaustive 272x23 fallback test; row promoted to verified with
  both old caveats resolved (names pinned as build-time data; `LanguageCode` parse-opaque
  matches the source). Ledger gate now 25 non-verified (9 `实现中` + 1 blocked + 15
  `已实现但未验证`).
- Verification evidence: full `cargo test --locked --workspace` **1822 lib + 194 bin,
  0 failed**, 1 ignored; Clippy `-D warnings` exit 0; per-file rustfmt and
  `git diff --check` clean; parity report 396 rows, 0 malformed. Full evidence in
  `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: the ten remaining asset tabs plus `/Assets/*` data routes,
  `/FailedFiles/View` + `/StackTrace`, `/Scenes/View`, `/Export/PrimaryContent`; then
  continue down the ledger (next candidates among the 15 unverified).

## Latest current checkpoint - 2026-10-16 (asset pages list reference edges)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. The snapshot
  resolves edges through the workspace's own joined graph (source-order first-match, same
  as export); the asset page renders a dependencies section with field paths and target
  links, `Missing` only for unresolvable targets. New committed probe
  `tools/probes/asset-deps/` pins the frozen ground truth on the fixture (five nulls per
  object, no section without assemblies); the port surfaces the managed edge the export
  proves and links it. Note: the isolated `ar-ref` tree lost its prebuilt GUI assemblies,
  so probes currently need `-p:ReferenceRoot='<workspace>/AssetRipper'`.
- Verification evidence: fixture end-to-end pins the exact nested link, the negative
  case, and the 404; full `cargo test --locked --workspace` **1822 lib + 193 bin,
  0 failed**, 1 ignored; Clippy `-D warnings` exit 0; per-file rustfmt and
  `git diff --check` clean. Full evidence in `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: the ten remaining asset tabs plus `/Assets/*` data routes,
  `/FailedFiles/View` + `/StackTrace`, `/Scenes/View`, `/Export/PrimaryContent`. Then
  re-run the Phase-1 gap rules (9 `实现中` + 1 blocked + 16 `已实现但未验证`).

## Latest current checkpoint - 2026-10-16 (asset pages open with information)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. Snapshot rows
  carry the numeric class ID; `GET /Assets/View` renders the display name with the
  collection link and the path/class-number/class-name rows. The scene link, path rows,
  per-type custom rows and the other eleven tabs are recorded open.
- Verification evidence: fixture end-to-end follows a real asset link (class 114 page,
  per-kind 404); full `cargo test --locked --workspace` **1822 lib + 193 bin, 0 failed**,
  1 ignored; Clippy `-D warnings` exit 0; per-file rustfmt and `git diff --check` clean.
  Full evidence in `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: the eleven remaining asset tabs plus `/Assets/*` data routes,
  `/FailedFiles/View` + `/StackTrace`, `/Scenes/View`, `/Export/PrimaryContent`. Then
  re-run the Phase-1 gap rules (9 `实现中` + 1 blocked + 16 `已实现但未验证`).

## Latest current checkpoint - 2026-10-16 (resource pages serve names, sizes, bytes)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. The snapshot
  carries resource virtual paths; `GET /Resources/View` renders the fixed name, bundle
  link, byte size and download button, and `GET /Resources/Data` serves the exact
  filesystem bytes as octet-stream. Raw/fixed naming split pinned by end-to-end (bundle
  link raw, title/download fixed).
- Verification evidence: fixture end-to-end loads a synthetic 29-byte `.resS` (root
  listing, view page, byte-identical download, 404); full `cargo test --locked
  --workspace` **1822 lib + 193 bin, 0 failed**, 1 ignored; Clippy `-D warnings` exit 0;
  per-file rustfmt and `git diff --check` clean. Full evidence in `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: `/Assets/*` routes and page, `/FailedFiles/View` + `/StackTrace`,
  `/Scenes/View`, `/Export/PrimaryContent`. Failed-entry test data still needs crafted
  inputs. Then re-run the Phase-1 gap rules (9 `实现中` + 1 blocked + 16 `已实现但未验证`).

## Latest current checkpoint - 2026-10-16 (search pages query every asset)

- Worktree: `main-work` detached; HEAD == origin/main before this slice.
  `BundleTree::search` (display/class containment, collection order) plus `bundle_path`
  feed `GET /Search/View`, which renders the frozen form, prompt/empty/results states
  and the four-column results table. The class filter and client script are recorded
  open (server renders every row, no client code).
- Verification evidence: fixture end-to-end (class, name, case-insensitive, empty and
  hopeless queries; unloaded 404); every-language sweep renders all three search states;
  full `cargo test --locked --workspace` **1822 lib + 193 bin, 0 failed**, 1 ignored;
  `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0; per-file
  rustfmt and `git diff --check` clean. Full evidence in `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: `/Assets/*` data routes, `/Resources/View` + `/Data`,
  `/FailedFiles/View` + `/StackTrace`, `/Scenes/View`, `/Export/PrimaryContent`.
  Resource/failed test data still needs crafted inputs. Then re-run the Phase-1 gap
  rules (9 `实现中` + 1 blocked + 16 `已实现但未验证`).

## Latest current checkpoint - 2026-10-16 (collection pages list every asset)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. The snapshot now
  carries `TreeAsset` rows (path ID, source class name, export-identity display name from
  the decoded object workspace); `GET /Collections/View` renders the frozen columns with
  the bundle link. The class filter, page control, client script, scene link and
  dependencies sections are recorded open, not silently dropped. `object_asset_name` is
  re-exported from `project_workflow` (bin/lib are different crates, so same-crate
  visibility cannot cross; the public re-export style is used).
- Verification evidence: the fixture end-to-end asserts both `MonoBehaviour` rows with
  asset links; full `cargo test --locked --workspace` **1822 lib + 191 bin, 0 failed**,
  1 ignored; `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0;
  per-file rustfmt and `git diff --check` clean. Full evidence in `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: `/Assets/*` data routes, `/Resources/Data` (+ view), `/FailedFiles/StackTrace`
  (+ view), `/Scenes/View`, `/Search/View`, `/Export/PrimaryContent`. Failed/resource test
  data needs crafted inputs (no bundle writer exists); asset names already flow. Then
  re-run the Phase-1 gap rules (9 `实现中` + 1 blocked + 16 `已实现但未验证`).

## Latest current checkpoint - 2026-10-16 (bundle pages render from a live snapshot)

- Worktree: `main-work` detached; HEAD == origin/main before this slice. New binary module
  `src/web/tree.rs` (`BundleTree` snapshot mirroring `GameBundle.FromPaths`, built once per
  load from the workspace inventory plus the root resource catalog, dropped on reset);
  `GET /Bundles/View` renders the resolved bundle and `GET /Collections/Count` answers the
  census as JSON; the other tree routes resolve first (baseline per-kind 404s, with the C#
  record-`ToString` diagnostic forms added to `web_paths`) and pend only past that.
- Verification evidence: fixture end-to-end
  (`web::routes::tests::a_loaded_bundle_renders_its_tree_pages`, committed 3,323-byte
  synthetic nested UnityFS bundle, no layout guard needed) plus the every-language sweep now
  rendering the bundle page; full `cargo test --locked --workspace` **1822 lib + 191 bin,
  0 failed**, 1 ignored; `cargo clippy --workspace --all-targets --locked -- -D warnings`
  exit 0; per-file rustfmt (never bare `src/lib.rs`) and `git diff --check` clean. Full
  evidence in `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text. Mapping table
  stays outside the repo; history untouched.
- Next move: the collection-content pages (`/Collections/View` asset lists need decoded
  asset names; `/Assets/*` data routes; `/Resources/Data`; `/FailedFiles/StackTrace`;
  `/Scenes/View`; `/Search/View`; `/Export/PrimaryContent`). Then re-run the Phase-1 gap
  rules (9 `实现中` + 1 blocked + 16 `已实现但未验证`).

## Latest current checkpoint - 2026-10-16 (bundle-tree path values landed; tree projection is next)

- Worktree: `main-work` detached; HEAD == origin/main before this slice, working tree holds only
  the five intended paths (`src/web_paths.rs` new; `src/lib.rs`, `src/web/{mod,pages,routes}.rs`
  edited). No bundles, reference outputs or user data touched. Formatted per-file with
  `rustfmt --edition 2021 --check` (never bare-rustfmt `src/lib.rs` — it follows the module
  tree and rewrites unrelated drift files; one such accident was reverted).
- **New deliverable — `src/web_paths.rs` library module** (beside `product.rs`/`json.rs`, the
  precedent for product-surface code in the lib): all six frozen `Paths/` structs with exact
  `ToJson` shapes parsed through the shared `crate::json` reader (exact key sets any order,
  integral-only numbers, `i32` range checks, `"P":null` ≡ root), the tree operations
  (`parent`/`child`/`collection`/`failed_file`/`resource`/`asset`), all six `*API.GetViewUrl`
  builders, and the shared `TryGet*FromQuery` gates as generic `view_gate`. Six routes
  (`/Bundles|/Collections|/Assets|/Resources|/FailedFiles|/Scenes` + `/View`) run the frozen
  missing-path/malformed-JSON/not-loaded 404 gates and keep the explicit pending page only for
  a well-formed loaded path. No dead code: the lib placement keeps every builder/operation
  alive for the coming pages (a bin-side module tripped `-D warnings` dead-code and was
  removed in favor of the lib module).
- Verification evidence: `cargo test --locked --lib web_paths` 6/6; `--bin assetripper-rs
  web::` 55/55; full `cargo test --locked --workspace` **1821 lib + 188 bin, 0 failed**,
  1 ignored; `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0;
  per-file rustfmt clean; `git diff --check` exit 0. Full evidence in `docs/validation-log.md`.
- Privacy: no sample content/paths/names in code, tests, docs or commit text; only class IDs,
  counts and field names. Mapping table stays outside the repo; history untouched.
- Next move: build the bundle-tree projection (`Bundle`/`Collection`/`Asset`/`Resource`/
  `FailedFile`/`Scene` model over `WebState::loaded`, following frozen `PathExtensions`
  resolve + per-page `ViewPage` rendering with `PathLinking` up/down links) to turn `Ready`
  paths into real pages; then `/Collections/Count`, `/FailedFiles/StackTrace`,
  `/Resources/Data`, `/Assets/*` data routes, `/Search/View`, `/Export/PrimaryContent`.
  Then re-run the Phase-1 gap rules (9 `实现中` + 1 blocked + 16 `已实现但未验证`).

## Latest current checkpoint - 2026-10-16 (pure-Rust web surface landed as `src/web/`)

- Worktree: `main-work` detached, clean except uncommitted working tree `src/web/` and the launcher edits; HEAD `5ea8de13f655aa25aea17d0f495ff601a2f8552a` == origin/main. No original bundles, reference outputs or user data were touched. Honest state: derived surface is verified at the seam level plus one end-to-end smoke test; the remaining work lands the bundle-tree/asset pages and PrimaryContent export driver.
- **New deliverable — `src/web/` binary-crate module** (`arguments`, `http`, `server`, `state`, `pages`, `routes`, `settings_form`, `localization`) over self-contained HTML/CSS (`site.css` from `src/web/site.css`) with zero online dependencies plus 23 vendored locale files and generated `language_names.json`. 46 `WEB_ROUTES` registered. No-argument launch starts the browser; `serve` selects the web entry; `--help`/`--version`/`-?` keep CLI behavior. Launch parser verified byte-for-byte against the shipped `AssetRipper.GUI.Free.exe`: attached-value-only bools, case sensitivity, duplicate/single quote rules, missing-value and unknown-argument diagnostics, repeatable `--local-web-file`, short-circuit help/version.
- **Bugs left by coding found and fixed this slice**: (a) `Response::redirect` was 303, must be 302; (b) embedded `DROPDOWNS` was missing the ScriptExportMode dropdown (9→10) and its key in `selected_value`; (c) document builder `text()` escaped nested markup (settings pages now use `raw()`); (d) dropdowns that do not localize their option names (Image/Lightmap) showed `⟦missing:Bmp⟧` markers — fixed by making `display_key` optional with fallback to the enum member name; (e) disabled free-edition controls rendered unchecked even when the record said true; (f) enum parse fallback in `apply_form` was required to be the zero member (per `TryParseEnum` default(T), e.g. Yaml for Audio) rather than be left untouched.
- Verification evidence:
  - `cargo fmt --all -- --check`: clean on changed files.
  - `cargo check --locked --all-targets`: zero warnings/errors (last checked).
  - `cargo test --locked --workspace`: **1815 library + 187 bin tests, 0 failed**, 1 ignored.
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- End-to-end smoke test (`target/debug/assetripper-rs.exe serve --headless --port 0 --log`): bind `http://127.0.0.1:50128`, `GET /` 200 (5975B), `/Settings/Edit` 200 (14836B), `/Commands` 200 (5408B), `/css/site.css` 200 (4568B `text/css`), `/Licenses` 200 (111666B), `/openapi.json` 200 (4648B), `/IO/Directory/Empty?Path=...` 200, `POST /Settings/Update` 302 Location `/Settings/Edit`, unknown path 404, HEAD 405 (matches baseline, which does not register HEAD). Log rotated; welcome banner matched.
- Product ledger corrections landed (`PRODUCT-GUI-001` now `已实现但未验证` with verification evidence; `PRODUCT-WEB-SURFACE-001` records the landed server and remaining unported routes; `PRODUCT-CONFIG-001` had its false `DataEntry/DataInstance/DataSet/DataStorage` claim corrected to "not ported / open path"). Update `docs/validation-log.md` next with this increment's command hashes and output paths.
- Still-open for this workstream (no "next action identified" rest; these are tracked, not blocking):
  - Web surface: `/Assets/*`, `/Bundles/View`, `/Collections/View|Count`, `/FailedFiles/View|StackTrace`, `/Resources/View|Data`, `/Scenes/View`, `/Search/View`, `/Export/PrimaryContent` return 501 pending a bundle-tree (`BundlePath`/`CollectionPath`/`AssetPath`/`ResourcePath`/`FailedFilePath`) port; shader-platform + combined-mesh to close `/PremiumFeatures`; `AssetRipper.Configuration` data-set subsystem for `/ConfigurationFiles`. Rendering technology is an accepted substitution (no Bootstrap/Vue/Babylon, no online deps) and confirmed steering.
  - Phase 1 product list: 9 cells `实现中` (`IO-STREAM-001`, `SER-TT-ABSENT-001`, `TYPE-SCRIPT-001`, `OBJ-REF-001`, `PROC-SCENE-001`, `PROC-ANIM-001`, `RES-TEXTURE-001`, `RES-MODEL-001`, `EXPORT-UNITY-001`), 1 blocked (`IO-FS-IMPORT-PRESERVE-DIRECTORY-001`), 14 `已实现但未验证` — turn into verified with independent evidence; `parity-report.ps1` schema-to-phase1 closure still the outer gate.
- Disk/free-space: workspace tests finished under existing 2.8 GB free target; reused superseded build info only.
- Next move for the next agent: build the bundle-tree/path model in Rust and implement the 501 pending asset pages, then the PrimaryContent export driver; then re-run the Phase 1 gap rules (no sample-specific exceptions).

## Latest current checkpoint - 2026-10-16 (eleventh world converges; next blocker is the repository-wide product gap audit)

- Worktree `main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined under the ignored `target/` tree).
- Eleventh world CONVERGED with the unchanged HEAD binary (`dd9f005`) and therefore with no regression risk for the earlier worlds. Reference oracle `REF_EXIT=0` (2203 files under `Assets/`, 2117319914 bytes, fresh `target/ra-w11/ev/report.json`, exe sha256 `FB62A778…`). Rust replay `export-project --delete-existing target\ra-w11\rust-r1 target\ra-w11\W11.vrcw` exit 0 (34150 objects, 1096 asset files, 2053092425 bytes). Audit r1 `pairedFiles=1107 onlySource=0 onlyRust=0`, every class `diff=0` and the single differing document (one class-90 Avatar) proven `STYLE_ONLY` (22792 tokens, `realDiff=0`); 260/260 PNG pixel-identical, 1/1 OGG packet-identical (9076/9076); the only additional difference is the by-design `AuxiliaryFiles/path_id_map.json`.
- **Corpus status: all 22 inputs are now accepted** (AV01-AV11, W01-W11; W11 was the last one). Recorded per-sample evidence lives in `docs/acceptance-corpus.md`; the tool commands are `tools/audit-project-documents.ps1`, `tools/compare-document-styles.ps1`, `tools/compare-png-pixels.ps1`, `tools/compare-ogg-packets.ps1` driven from a per-sample directory under `target/ra-*`.
- **Next blocker: the repository-wide product gap audit.** `tools/parity-report.ps1` (ledger `docs/feature-parity.md`) still reports `phase1_gate=OPEN` with 396 rows and 25 non-verified: 9 `实现中` (`IO-STREAM-001`, `SER-TT-ABSENT-001`, `TYPE-SCRIPT-001`, `OBJ-REF-001`, `PROC-SCENE-001`, `PROC-ANIM-001`, `RES-TEXTURE-001`, `RES-MODEL-001`, `EXPORT-UNITY-001`), 1 `受外部条件阻塞` (`IO-FS-IMPORT-PRESERVE-DIRECTORY-001`), 1 `已调查` (`PRODUCT-GUI-001`) and 14 `已实现但未验证`. The sharpest verified reality gap found this round: the Rust product ships **no web/HTTP surface at all** — `src/product.rs` ports only the frozen route table, launch defaults, command rules and log rotation, and the binary's subcommands are `export-object-yaml`, `export-file-yaml`, `export-texture`, `export-mesh`, `export-shader`, `export-audio`, `export-project`, `inspect`, `inspect-files`, `save-default-settings`, `delete-default-settings` (no server; no `TcpListener`/HTTP dependency in `Cargo.toml`). The frozen baseline's user surface is `AssetRipper.GUI.Free` → `WebApplicationLauncher.Launch` with 47 routes (`src/product.rs` `WEB_ROUTES`), the five POST commands, the native dialogs, the settings pages and 23 locale JSON files (`PRODUCT-I18N-001` records that the string tables await a UI consumer). Implementing that surface in pure Rust (no .NET) is the next workstream, together with closing the nine `实现中` rows and turning the 14 `已实现但未验证` rows into verified ones with independent evidence.
- CI: `Rust CI` run `38846743676` for `dd9f005` is success in all seven jobs. Pending CI is never completion and no repository-caused failure is open.
- Not run: Unity Editor import/runtime (Editor license), UABEA, and the Python public-repository checker (`python` unavailable locally; the PowerShell equivalent scan ran).

## Latest current checkpoint - 2026-10-15 (ninth world converges; next blocker is the eleventh world replay)

- Worktree `main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Ninth world CONVERGED. The terrain splat-alpha collection (`DIFF-TERRAIN-SPLAT-ALPHA-COLLECTION-001`) is implemented generically and verified on real input: replay r3 exit 0 with 1043 asset files (one fewer than r2, the collapsed texture asset and its `.meta`), the terrain asset byte-identical to the reference except two minted cross-file GUIDs, audit r3 `pairedFiles=1052 onlySource=0 onlyRust=0` with class 156 `ExactAfterGUIDPathMap=1 diff=0` and class 28 `exact=3 diff=0`, `additionalDifferences=1` (the by-design `AuxiliaryFiles/path_id_map.json`), both class-90 Avatar documents STYLE_ONLY with zero real differences, 289/289 PNG pixel-identical, 26/26 OGG packet-identical. The previously fixed slices stay green in the same run (class 164 28/28, class 259 5/5, class 1953259897 semGUID 1/1, all `diff=0`).
- Closed set: sixth, seventh, eighth, ninth and tenth worlds plus AV01-AV11 and W01-W05. The only other audited sample with class-156 objects is the sixth world and all four of its terrain-data documents carry `m_AlphaTextures: []`, so the new collection is provably inert there.
- **Next blocker: VRC-W11 (the largest input, ~274 MB) has never been replayed end-to-end.** Its recorded state is probe-only (`decode 33974/33974 OK`, `done: 2207 files`, export pending). Next step: run `tools/export-reference-web.ps1` for it, replay the Rust export into a fresh directory with `--delete-existing`, run `tools/audit-project-documents.ps1` plus `tools/compare-document-styles.ps1` on every differing pair and `tools/compare-png-pixels.ps1` / `tools/compare-ogg-packets.ps1` over the opaque pairs, then record the verdict in `docs/acceptance-corpus.md`.
- New tool: `tools/compare-document-styles.ps1` classifies each differing paired document as exact / GUID-path-equivalent / style-only / real difference (real differences print their first differing tokens). It confirms every Avatar document on the audited worlds is style-only.
- Local checks for the ninth-world fix: `cargo build --release --locked` exit 0; `cargo test --locked --lib` 1810 passed, 0 failed, 1 ignored; `cargo clippy --workspace --all-targets --locked -- -D warnings` exit 0. Whole-tree `cargo fmt --all -- --check` still reports only the five documented pre-existing drift files, none of them touched by this change; every added or edited file is rustfmt-clean.
- Privacy: the current docs remain free of private bundle-name prefixes, raw input paths and raw object names; the mapping table stays outside the repository and history was not rewritten.
- Not run: Unity Editor import/runtime (Editor license), UABEA, and the Python public-repository checker (`python` unavailable locally).

## Latest current checkpoint - 2026-10-15 (seventh world: two generic fixes verified on real input; re-verify converged set)

- Worktree `main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Fourth world r7 replay with the mesh-redirect binary exited 0 and exported 4924 mesh asset files on both sides with zero only-reference and zero only-port names; DIFF-ENGINE-MESH-CACHE-001 is closed.
- Seventh world: the first audit pass used a wrong reference root (the frozen exporter nested the project one level deeper, `.../ref/ExportedProject/ExportedProject`), which mis-keyed only the `.meta`/project-file comparison; the corrected pass pairs 1645 files with zero unpaired and shows nearly every earlier "difference" is GUID-path equivalent. Real residue was the LightingData name-order swap (class 157/258/1120 plus two `LightingData*.asset.meta` sidecars) and class 167 (`AudioReverbZone`) exported release-shaped; both got generic fixes (`plan_lighting_data_paths` reserving the generated name before routed assets; class 167 joined the 2022.3 editor route with a TPK-backed test) that the fixed-binary replay confirmed: `LightingData.asset` now holds class 1120 and `LightingData_0.asset` class 258, and the re-audit resolves class 157/258/1120 plus the two sidecars while class 167 is `exact=2 diff=0`. All 409 opaque pairs are proven equivalent (382/382 PNG pixel-identical; 27/27 OGG packet-identical with `ALL_IDENTICAL=True`) and the two class-90 Avatar documents are strictly token-equal after stripping YAML layout characters (style-only).
- Local checks: `cargo check --locked` exit 0; `cargo clippy --locked --all-targets -- -D warnings` exit 0; the two new `planning.rs` reservation tests, `fourth_world_editor_route_additions_carry_the_full_editor_base` and `unity_2022_3_scene_classes_use_editor_type_trees` all pass. Next: re-verify the converged set (AV01-AV11, W01-W05) after the routing change, then continue with the eighth through eleventh worlds; Unity Editor import/runtime remains blocked by the Editor license and UABEA is not run.
- Converged set unchanged until re-verified: AV01-AV11, W01-W05.

## Latest current checkpoint - 2026-10-15 (predefined Mesh redirect verified; document audit in progress)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- DIFF-ENGINE-MESH-CACHE-001 fix implemented: the `tools/probes/engine-assets` probe gained a `meshtsv` mode and four `predefined_meshes_band_*.tsv` tables (2018.3.0b6, 2022.2.15f1, 2022.3.11f1, 6000.0.29f1) are embedded; the routing decision derives the frozen `MeshKey` (name, `m_VertexData.m_VertexCount`, `m_SubMeshes` count, `m_LocalAABB` center/extent) from a decoded class-43 payload and redirects when the band table matches, declining whenever a key component is unreadable.
- Local checks: 1807 library + 133 CLI tests pass, warnings-denied Clippy clean, `git diff --check` clean, edited files rustfmt-clean (whole-tree fmt still shows only the five documented pre-existing drift files). **Verified on real input:** the affected world replays with exit 0 (4835 asset files, 379582640 bytes) and now exports 4924 mesh asset files on both sides with zero only-reference and zero only-port mesh names - the two extra Cube assets are gone and every later suffix realigns, closing DIFF-ENGINE-MESH-CACHE-001. **Next step: run the semantic document audit of this replay and triage the remaining class-90 (Avatar), class-114 (state) and opaque PNG/OGG differences.**
- Converged set unchanged until re-verified: AV01-AV11, W01-W05.

## Latest current checkpoint — 2026-10-15 (fourth world down to one real cluster; next blocker is the predefined Mesh redirect)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- The fourth world's r6 replay pairs 4840 files with **zero unmatched** (onlySource fell from 1 to 0), writes the previously missing sprite asset, pairs all 69 class-213 documents, matches every class added to the 2022.3 editor route exactly, and cuts the state class-114 diffs from 1076 to 52. Real differences are now 206 documents in five classes.
- **Next blocker: the predefined Mesh redirect (DIFF-ENGINE-MESH-CACHE-001).** Two mesh assets the reference never writes are exported, and because they consume names in the shared `Cube` sequence every later mesh suffix shifts by two — the same source mesh is `Cube_27` in the reference and `Cube_29` in the port — which breaks 109 mesh-filter, 30 mesh and 8 mesh-collider documents. The frozen rule is `PredefinedAssetCache`'s `MeshKey(Name, VertexCount, SubMeshCount, LocalAABB)` redirect via `EngineAssetsExporter.TryCreateCollection`; the builtin `Cube` mesh is path ID 10202 with 24 vertices, one sub-mesh and a `(0,0,0)`/`(0.5,0.5,0.5)` box. The port ships tables for materials, shaders, sprites and textures but none for meshes. The recorded fix: extend `tools/probes/engine-assets` with a mesh TSV mode, add the four band tables, and consult them with the decoded vertex count, sub-mesh count and `m_LocalAABB`.
- Remaining after that: seven Avatar documents whose style proof is pending, 52 state class-114 documents, and the 944 opaque pairs that still need the PNG/OGG content comparison (the reference proven equivalent elsewhere by pixel and packet identity).
- The first world's r5 regression replay after the routing change stays clean: zero diff classes, zero unpaired, 102/102 PNG pixel-identical, 1/1 OGG packet-identical. The other converged samples still need the same replay before their verdicts are renewed.
- Local checks: 1805 library + 133 CLI tests pass, warnings-denied Clippy clean, rustfmt and `git diff --check` clean. Disk was recovered to ~3 GB by deleting superseded replay outputs only.
- CI runs five jobs on Actions (privacy, ledger/repository audit, changed-line rustfmt, Clippy and tests on Ubuntu+Windows, release-mode tests). **No repository-tracked job ever reads VRC inputs; the real samples stay local by design.**

## Latest current checkpoint — 2026-10-15 (engine-sprite gate and twelve more editor classes)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Two generic fixes landed after the fourth world's NavMesh blocker cleared (that replay now audits class 196 as an exact document match).
- **Engine-sprite redirect gate.** A world-authored Sprite named like a builtin, with a same-named texture, was redirected to the builtin-extra identity instead of being exported, costing 1024 reference sites across 1076 state documents with no notice. The frozen exporter decides a `SpriteInformationObject` group through `Cache.Contains(sio.Texture)` — the texture, keyed by name *and* dimensions — before any sprite row is consulted, because the group's main asset is assigned to the texture and every member sprite. The port keyed the decision on the sprite's own `(Name, TextureName)` pair, which cannot see that the builtin texture is 32x32 while the authored one is 20x20. The redirect is now gated on the resolved texture's predefined identity; the replay writes the previously missing sprite asset and drops the builtin fallback pointers from 1084 to 60 in the scene.
- **Twelve more 2022.3 editor classes.** A TPK probe found OcclusionCullingSettings (29), Projector (119), Font (128), WheelCollider (146), AudioLowPassFilter (169), WindZone (182), NavMeshAgent (195), AnimatorOverrideController (221), NavMeshData (238), OcclusionCullingData (363), LookAtConstraint (1183024399) and ScaleConstraint (1818360610) carrying editor base fields the release trees omit, with none routed. They now use the editor route, covered by a probe-backed regression test (class 29 is the one whose editor tree carries only `m_ObjectHideFlags`, matching the frozen document).
- The fourth world still has real differences: a mesh-identity cluster (class 33/43/64; two Rust-only mesh assets), the curated Avatar documents, the opaque PNG/OGG byte encodings, and whatever the pending audit reports. Its r6 replay exports with exit 0.
- Local checks: 1805 library + 133 CLI tests pass, warnings-denied Clippy clean, rustfmt and `git diff --check` clean. The r6 semantic audit is running; its result and the remaining class counts are the next recorded step. Converged set unchanged until re-verified: AV01-AV11, W01-W05 — the engine-sprite gate changed routing, so those samples need a regression replay before their verdicts stand.
- CI runs five jobs on Actions (privacy, ledger/repository audit, changed-line rustfmt, Clippy and tests on Ubuntu+Windows, release-mode tests). **No repository-tracked job ever reads VRC inputs; the real samples stay local by design.**

## Latest current checkpoint — 2026-10-15 (fourth world passes its NavMesh blocker)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- The fourth world previously failed loudly at class-196 `NavMeshSettings` because a non-null `m_NavMeshData` had no source-sourcing path. The frozen three-branch `ConvertToEditorFormat` is now fully implemented generically: null/dangling data keeps the standalone defaults, `m_NavMeshParams` maps five fields over those defaults with the source's truncating `(int)` cast, and `m_NavMeshBuildSettings` is copied field-for-field with nested-struct recursion, editor-only fields keeping their merged value and source-only fields getting no assignment (exactly the generated `CopyValues` behavior). A frozen-TPK probe across 5.0 to 6000.0.23 pinned the class-238/196 version boundaries the branches depend on.
- Local checks: `cargo check` and `clippy -D warnings` clean; 1803 workspace tests pass (6 new focused NavMesh tests); `git diff --check` exit 0; rustfmt clean on edited files. The fourth world's `export-project` replay now exits 0 (previously exit 1); its semantic audit against the frozen reference is the current open step. Converged set unchanged: AV01-AV11, W01-W05.
- CI gained two jobs so heavier validation runs on Actions instead of locally: a ledger/repository audit (`tools/parity-report.ps1` structure plus the oversized-source report) and release-mode workspace tests. **No repository-tracked job ever reads VRC inputs; the real samples stay local by design.**

## Latest current checkpoint — 2026-10-14 (third world converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Third world r1: CONVERGED on the current binary (proven-equivalent Avatar; PNG/OGG content proven; one auxiliary map). Converged set: AV01-AV11, W01-W05. Next: W06-W11 worlds pending.
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (second world converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Second world r3: CONVERGED (395 paired, zero unpaired; zero doc diffs; PNG content proven; one auxiliary map). Converged set: AV01-AV11, W01-W04. Next: W05-W11 worlds pending.
- Local checks: `git diff --check` exit 0; full workspace suite green; cargo check/clippy (-D warnings locally) clean. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (world converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-W03 r9: CONVERGED (541 paired, zero unpaired; docs exact-or-mapped except a proven known-acceptable dangling-GUID pair; PNG/OGG content proven; one auxiliary map). Converged set: AV01-AV11, W01, W02, W03. Next: W04-W11 worlds pending.
- Local checks: `git diff --check` exit 0; full workspace suite green; cargo check/clippy (-D warnings locally) clean. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (atlas sprites plus Hash128; world r5 exits 0)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Atlas-sprite overlay threading plus null-overlay fix plus Hash128-uint correction implemented and unit-tested. Reporting world r5 exports exit 0 (541 paired, zero unpaired); broad paired diffs under triage (DIFF-W03-AUDIT-001). Converged set unchanged (AV01-AV11, W01, W02).
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (terrain lighting implemented)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- Terrain lighting records implemented and unit-tested; W03 r2 passes LightingData planning (phase order proves execution) and now fails loudly on atlas-packed sprites (DIFF-ATLAS-SPRITE-001, next blocker). Converged set unchanged (AV01-AV11, W01, W02).
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (AV01 converges; W03 blocked)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV01 r2: CONVERGED (proven-equivalent Avatar; zero unpaired). Converged set: AV01-AV11, W01, W02 (all avatars plus two worlds). VRC-W03 r1: export fails loudly (class-218 Terrain lighting unimplemented; fail-closed, no partials). Next blocker: DIFF-TERRAIN-LIGHTING-001.
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (AV11 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV11 r1: CONVERGED (two proven-equivalent Avatars; zero unpaired, 1606 paired). Converged set: AV02-AV11, W01, W02 (+AV01 with only known Avatar style). Next: advance the corpus (W03-W11 worlds pending).
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (AV10 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV10 r1: CONVERGED (only proven-equivalent Avatar; zero unpaired, 1657 paired). Converged set: AV02-AV10, W01, W02 (+AV01 with only known Avatar style). Audit tooling hits the Windows path limit on very long repository file names; map a drive letter to the outputs root and run the audit through it. Next: advance the corpus.
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (AV09 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV09 r1: CONVERGED (three proven-equivalent Avatars; zero unpaired, 1135 paired). Converged set: AV02-AV09, W01, W02 (+AV01 with only known Avatar style). Next: advance the corpus.
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (AV08 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV08 r1: CONVERGED (only proven-equivalent Avatar; zero unpaired, 356 paired). Converged set: AV02-AV08, W01, W02 (+AV01 with only known Avatar style). Six large inputs (AV08-11, W10-11) live outside the small-sample directory; locate future ones by byte size under the Desktop tree (never record their paths/names). Next: advance the corpus.
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (AV07 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV07 r1: CONVERGED (only proven-equivalent Avatar; zero unpaired, 602 paired; inner 2022.3 despite 6000 bundle tag). Converged set: AV02-AV07, W01, W02 (+AV01 with only known Avatar style). Next: advance the corpus.
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (AV06 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV06 r2: CONVERGED (only proven-equivalent Avatar; zero unpaired, 1034 paired). Converged set: AV02, AV03, AV04, AV05, AV06, W01, W02 (+AV01 with only known Avatar style). Next: advance the corpus.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (AV04 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV04 r1: CONVERGED (only proven-equivalent Avatar; zero unpaired, 596 paired). Converged set: AV02, AV03, AV04, AV05, W01, W02 (+AV01 with only known Avatar style). Next: advance the corpus.
- No product code changed this round (docs only).

## Latest current checkpoint — 2026-10-14 (AV05 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV05 r2: CONVERGED (only proven-equivalent Avatar; zero unpaired). Converged set: AV02, AV03, AV05, W01, W02 (+AV01 with only known Avatar style). Emitter change proven blast-free by exhaustion (no empty items in any converged YAML). Next: advance the corpus.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + full workspace suite green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (AV02 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV02 r2: CONVERGED (only proven-equivalent Avatar; zero unpaired). Converged set: AV02, AV03, W01, W02 (+AV01 with only known Avatar style). Next: advance the corpus (samples with probe-done/export-pending need reference baselines first).
- No product code changed this round (docs only). Disk at ~8GB free; superseded replays cleaned.

## Latest current checkpoint — 2026-10-14 (W02 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-W02 r4: CONVERGED (zero real-diff classes; zero unpaired). Regression replays same binary: AV03 r22 (only proven-equivalent Avatar), AV01 r2 (only known Avatar style), W01 r4 (still converged). Next: advance the corpus to the next sample.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (W02 nearly converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-W02 r2: real-diff 6 to 1; only NavMeshSettings (196) open — needs a struct-default insert for m_BuildSettings (values recorded in DIFF-NAVMESH-DEFAULTS-2019-001; member types still to probe from the 2019.4 TPK editor tree). Cookie pointer byte-identical; zero unpaired.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (AV03 converges)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r21: CONVERGED (all content exact except proven-equivalent Avatar; zero unpaired; binaries clean). Next: advance the acceptance corpus to the next sample (per-sample replays with the strengthened binary; 2018.4/2019.4 backfill versions now cover many classes).
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (mesh fields fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r19: real-diff 3 to 2; zero unpaired; binaries clean. Next: final two — 90 Avatar flow-vs-block, 91 extra lines.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (class deltas fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r17: real-diff 6 to 3; zero unpaired; binaries clean. Next: final three — 43 m_MeshOptimized, 90 Avatar flow-vs-block, 91 extra lines.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (renderer GI fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r16: real-diff 9 to 6; zero unpaired; binaries clean. Next: 91 extra lines, class extras (43 m_MeshOptimized, 95 m_WarningMessage, 108 shadow fields, 1773428102 m_IsLocked), 90 flow rule.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (CustomData labels fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r14: real-diff 10 to 9; zero unpaired; binaries clean. Next: renderer GI editor sets (23/137/199 share 10 missing fields; reuse the existing renderer editor-format conversion), then 91 extras, class extras (43/95/108/1773428102), 90 flow rule.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (tangentMode fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r13: real-diff 11 to 10; zero unpaired; binaries clean. Next: particle MinMaxGradient/colorLabel fields plus 198 L1626 light PPtr (values unchecked), then 91 extras, renderer GI sets (23/137/199), class extras (43/95/108/1773428102), 90 flow rule.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

## Latest current checkpoint — 2026-10-14 (document stamps fixed)

- Worktree `../main-work` (detached); the Desktop checkout's conflicts and local changes are untouched. Real samples are the top corpus; strict anonymity everywhere public (only class IDs/counts/field names in docs; audit outputs quarantined).
- VRC-AV03 r11: real-diff 13 to 11; zero unpaired; binaries clean. Next: curve tangentMode decode gap (82/198 share it; TPK 2018.4 tree proven lacking tangentMode while reference output has it), then 91 extras, renderer GI, class extras, 90 flow.
- Local checks: `git diff --check` exit 0; cargo check/clippy (-D warnings locally) + module tests green. Full validation via GitHub Actions after push.

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
- 烟雨长亭 source/Rust r1: `target/real-acceptance-20261013-烟雨长亭-source-web-r1/ExportedProject` and `target/real-acceptance-20261013-烟雨长亭-r1`; report `target/real-acceptance-20261013-烟雨长亭-r1-documents.json`. 242 PNGs pixel-equal, all 36 OGG packet sequences identical, three Avatar documents structurally equal, class 86 byte-exact. Visible residuals: the 38 known class-21 builtin-vs-Soft material targets, Rust-only 月照琉璃.png/meta, 3 Avatar formatting differences, and the auxiliary path-map scope artifact. Reference scans are PARTIAL with zero issues; source/Rust counts differ by 38 target classifications and remain unresolved for full identity acceptance.
- Use retained short roots `C:\REDACTED_INPUT`, `C:\kjPW2`, `C:\kjPW3`, `C:\kjMS13`, `C:\kjMR13`, `C:\kjAS13`, `C:\kjAR13` and older user roots. Do not repoint/delete junctions. C: had 43.9 GB free at last check. Keep audit JSON and fresh replay trees; no cleanup or deletion was performed.
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

- Frozen projects exist: `target/real-acceptance-20261012-松月云庭图-source-web-r1/ExportedProject` and `target/real-acceptance-20261012-烟雨长亭-source-web-r1/ExportedProject`. `tools/export-reference-web.ps1` POSTs to frozen GUI.Free `/LoadFile` and `/Export/UnityProject`; both exports exit0. “No CLI, blocked” is obsolete. Input hashes/source-copy hashes and settings/logs are recorded in validation-log and `docs/completion-audit-玉京烟霞-20261012.md`.
- **Sprite importer facts FIXED and pushed (`b244def`):** production now discovers per-texture sprites via `m_RD.m_Texture` + graph resolution and applies `decide_sprite_importer` as a routing post-pass with processed coords values (stale bundle `m_Rect` replaced via the existing `get_sprite_coordinates_in_atlas` kernel) and `f32` rendering. All 5 玉京烟霞 sprite metas match source (incl. bit-exact derived pivot `0.49999997`); r14 audit profile identical to r10 with additional diffs down to the path_id_map scope artifact; 松月竹庭 r18 zero-diff regression; suite 1,706/1,706. Atlas-packed/cubemap/3D/lightmap stay open. Monotonicity replays current: 玉京烟霞 r14, 松月竹庭 r18 (superseded r10/r11/r13, 松月竹庭 r16/r17 deleted; disk 9.2GB).
- **Next scoped blocker: 玉京烟霞 r14 class-90/91 doc diffs** (`Assets/青岚资源路径`, 8 animator `.controller` docs incl. LowerLegRibbon/PB_CatEar) plus the 261 opaque-different pairs. Class-90 triaged: pure flow-vs-block style in `m_AvatarSkeletonPose` (`- t: {…}` vs expanded; values incl. `-0` identical) — check the `TransferUsingFlowMappingStyle` flag table for Avatar. Class-91 triaged: machine doc exists but ordered after states with allocator IDs (`7000000000`-scheme) instead of hierarchy-order pseudo-random IDs. **Seed rule proven by new test** (`pseudo_random_export_ids_match_frozen_玉京烟霞_controller_subdocs`): `AddAsset` skips the controller, machine takes seed 0, states follow in `FetchEditorHierarchy` order. Remaining work: hierarchy-order pseudo-random assignment + doc emission order (stress: moon_doll 100+ docs, TriggerHoldDetect inline 1101s).
- **Final 烟雨长亭 r5 succeeds:** `target/real-acceptance-current-烟雨长亭-r5`; 5829 objects,1131 Assets/青岚资源路径,343427284 asset bytes, support5/post2. Class86 `EnvLightColor.renderTexture` is **byte-exact** with frozen source, including editor base fields and Hash128. Source/Rust SHA256 `SHA256_REDACTED`.
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

- Primary acceptance sample: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `SHA256_REDACTED`). Latest Rust export: `target/real-acceptance-20261011-青岚古境-avatar-mask-editor-r1`; 8,547 objects, 1,536 asset files, 601,755,682 asset bytes. Frozen source export: `target/real-acceptance-20261011-青岚古境-source/ExportedProject`.
- Latest root-prefab audit pairs all 4,491 documents by class ID/local fileID. GameObject (1,828), Transform (1,828), MeshRenderer (100), MeshFilter (100), AudioSource (27), ParticleSystem (90), and the other compared root classes match after project GUID/path normalization; 135 and 2,184 curve tangentMode values match. The class-319 `vrc_HandsOnly(Clone)` mask now includes the four editor base fields and its root-prefab pointer resolves to the emitted `.meta`. One AvatarDescriptor pointer still uses different menu output paths (`MonoBehaviour_56` vs `MonoBehaviour_46`), but its recursive graph matches across 65 MonoBehaviour YAML assets, 219 pixel-exact PNGs, and one byte-exact script asset. Reports: `target/real-acceptance-20261011-青岚古境-avatar-mask-editor-r1-prefab-audit.json` and `target/real-acceptance-20261011-青岚古境-expression-menu-semantic-summary.json`.
- The broad YAML inventory still does not scan `.state` or `.blendtree`. Source has 13 standalone `.state` and 41 `.blendtree` assets which Rust embeds in controller documents. The new `tools/compare-controller-blendtrees.ps1` compares every recursive tree graph: 174/174 documents, 156/156 unique semantic graphs, 631/631 child records, zero differences. Report: `target/real-acceptance-20261011-青岚古境-blendtree-graph-r1.json`.
- A large Mesh index buffer previously exhausted the StreamingInfo scanner's node limit by scheduling scalar leaves. The scanner now budgets only structs and arrays; focused resource tests pass 20/20. Latest real-sample inspection reports 552 candidates, 436 Bundle ranges, zero scan errors, and `complete=true` after 5,020,123 structural values. Fresh export and comparison results are recorded in the newest validation-log entry; 19 inspect-mode refs are Unity builtin meshes whose exported source/Rust GUID and fileID pointers match.
- Latest additional real-sample comparison: 青岚雅苑` (Unity 2022.3.22f1, SHA-256 `SHA256_REDACTED`). Rust r6 is `target/real-acceptance-20261012-松月竹庭-r6`; frozen source export is `target/real-acceptance-20261012-松月竹庭-source/ExportedProject`. A predefined `Default-Material` cache miss was fixed using the source 2022.3.11f1 `(Material.Name, Shader.Name)` identity. The Cube renderer pointer now matches source at builtin-extra PathID 10303/FGUID, and the standalone Rust `.mat` is gone. Reports record 114/114 matching input object rows and 1788/1788 YAML documents; the whole-project comparison still has 4/5 path-count differences and capped reference/null-pointer difference lists. The focused controller report has 3/3 BlendTrees, 37/37 child records, zero semantic differences.
- Additional avatar sample: 青岚雅苑` (58,150,845 bytes; SHA-256 `SHA256_REDACTED`). Fresh Rust export is `target/real-acceptance-20261011-云栖仙府-stream-scan-r1`; frozen source project is `target/real-acceptance-20261011-云栖仙府-source/ExportedProject/ExportedProject`. StreamingInfo scan is complete with zero scan errors, and 6,529/6,529 object payloads decoded. The 22 unresolved graph references all target `DefaultResources` engine assets.
- 云栖仙府 BlendTrees compare 717/717 documents, 339/339 unique recursive graphs, 1,807/1,807 ChildMotion records, with zero semantic differences. The broad project report records 3,312 source vs 2,821 Rust files, 498 source-only and 7 Rust-only paths, 9,956/10,204 YAML documents, and 2,846 matched documents; reference and null-pointer difference lists are capped. The source-only paths include 98 `.state`, 72 `.blendtree`, 76 `.asset`, 2 `.mask`, 1 `.prefab`, and 249 `.meta` files. See `DIFF-REAL-云栖仙府-PROJECT-LAYOUT-001`.
- The previous class-43 Mesh schema discrepancy is fixed and documented under `DIFF-REAL-青岚古境-MESH-EDITOR-TREE-001`; commit `29a7a78` is pushed. The class-319 AvatarMask and class-90 Avatar editor-base discrepancies are also fixed for their inspected sample paths.
- Resolved serialized-content difference: in 青岚雅苑`, source `_MainTex` points to builtin fileID 10307 while Rust points to the exported `Default-ParticleSystem` Texture2D. The builtin fileID 10307 and bundle PathID `4988150354445090395` export to identical 3,256-byte PNGs (SHA-256 `SHA256_REDACTED`). No code change was needed; Unity import/runtime remains unverified. See `DIFF-REAL-青岚古境-PREDEFINED-TEXTURE-CACHE-001`.
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
