# Source behavior and differential findings

## DIFF-CONTROLLER-SUBDOC-IDS-001 — controller sub-docs use allocator IDs, source uses hierarchy-order pseudo-random (open)

玉京烟霞 r14 all 8 `.controller` docs differ: Rust emits `[91, 1102-state(7000000001), 1107-machine(7000000000)]` while source emits `[91, 1107-machine(1107709254077376921), 1102-state(1102730242686860875)]` — same values otherwise. `AnimatorControllerExportCollection` inherits default `GenerateExportID` (`GetPseudoRandomExportId(asset, m_exportIDs.Count)`); `AddAsset` skips the controller itself, so seeds count non-controller assets in `FetchEditorHierarchy` order (machine 0, states/transitions/blendtrees inline after). Proven by `pseudo_random_export_ids_match_frozen_玉京烟霞_controller_subdocs` (ahoge machine/state IDs reproduce exactly). Remaining: replace the `7000000000` allocator for kept docs with hierarchy-order pseudo-random assignment and emit docs in hierarchy order (stress cases: moon_doll 100+ mixed docs, TriggerHoldDetect inline 1101 transitions).

## DIFF-SPRITE-IMPORTER-FACTS-001 — sprite facts wired (fixed)

玉京烟霞 `menu.png` (+4 VRC UI textures) `.meta` differed: `textureType` 8 vs 0, `spriteMeshType` 1 vs 0, `alignment` 9 vs 0 (both sides agreed `spriteMode` 1). Production never called `decide_sprite_importer` (test-only) and discovered no per-texture sprites. Fixed by workspace/graph discovery via `m_RD.m_Texture` plus `decide_sprite_importer` field application as a planning post-pass on the plain-Texture2D route, with facts carrying the processed coords values and `f32` rendering. All 5 metas match source; cubemap/3D/lightmap/atlas-packed stay open with prior behavior.

## DIFF-PATH-ID-MAP-FILE-ORDER-001 — multi-CAB file order fixed via source collection order

玉京烟霞 (2 SerializedFiles) `AuxiliaryFiles/path_id_map.json`: per-file asset sets and within-file PathID sequences matched, but file order differed (source lists `sharedassets` first; Rust listed the main file first). UnityFS directory order (`unityfs_inventory`: sharedassets index 0, main index 1) is the source rule, and the workspace already tracks it as `source_collection_file_indices` (used by scene/prefab processing). `build_path_id_map` now iterates that canonical order instead of workspace insertion order. 玉京烟霞 r10 map is byte-exact (66,977/66,977); 松月竹庭 r17 zero-diff regression; full suite green. Single-file samples are unaffected by construction.

## DIFF-EDITOR-RESOURCE-86-121-001 — editor layout omitted

New frozen Web outputs show 烟雨长亭 CustomRenderTexture(class86) missing four editor base fields and m_ImageContentsHash, and 玉京烟霞 Flare(class121) missing four editor base fields. Release bytes decode using the release TPK; export was retaining that layout instead of the frozen editor projection. Generic2022.3 class selection now includes86/121, with frozen-layout/export-Hash128 regressions. Full lib1701pass/1ignored,Clippy/release0. Final 烟雨长亭 r5 CustomRenderTexture is whole-file byte-exact with frozen source. First 玉京烟霞 three-way audit (r8) pairs class121 and matches class124 exactly; corrections committed as `ff865c2` with full monotonicity replays.

## DIFF-NATIVE-YAML-CLASS-NAME-001 — shared abstract root mislabeled

玉京烟霞 FlareLayer(class124) release/editor TypeTrees root at Behaviour, whereas source generated asset.ClassName is FlareLayer. HaloLayer125 has the same rule at3.5/2022.3. Source TypeTreeObject deliberately derives its name from editor schema; a global class-ID relabel would violate that fallback. New optional native_class_name hint is supplied only for Generated objects and only substitutes abstract roots in project mode. Regression47/47 focused passes include known-ID fallback, unknown/script/explicit/importer preservation. Corrected 玉京烟霞 r8 verified (class124 exact). No comparator exclusions added.

## DIFF-烟雨长亭-SOFT-PREDEFINED-CACHE-001 — pixel equality with sampler differences

All44 烟雨长亭 finalr5 materials compared using exact parsed fields and GUID owners:6 equivalent;38 differ only at _IgnoreCookieTexture, frozen source builtin10001 versus Rust emittedSoft.png. Actual Unity2022.3.22f1 builtin PNG is byte/pixel-exact with Rust; bundle Texture2D PathID1213616326460374869 Alpha8 base16384 bytes equals builtin base mip. However input mipcount/colorSpace/streaming/filter/aniso are1/0/1/2/1, versus builtin8/1/0/1/0. Frozen cache matches(name,width,height), ignoring these settings; Rust preserves input/frozenImporterFactory settings in all12 inspected checks. **Rust-faithful representation difference**, source cache discards recoverable sampling metadata. Do not claim full builtin binding or Unity runtime equivalence. Reports `target/completion-audit-烟雨长亭-r5-material-before.json`, `target/completion-audit-烟雨长亭-soft-fidelity-r5.json`; no product patch.

## DIFF-PREFAB-IMPORTER-VERSION-BUNDLE-001 — required metadata defect, fixed

Expanded 烟雨长亭 finalr5 whole-project audit reveals source PrefabImporter variantParentGUID0 plus actual root assetBundleName; Rust instead hardcodes unableToImportOnPreviousDomainReload0, omits variantParentGUID, and emits empty assetBundleName. Source PrefabExportCollection.CreateImporter selects the full export-version class and copies RootGameObject.AssetBundleName. Frozen TPK class468431735 at2022.3.22f1 has m_VariantParentGUID, confirmed by source runtime dump. Fixed in `dc959fe`: version-gated fields (`unableToImport` only <2020.1, `variantParentGUID` only >=2022.2, TPK-surveyed) plus Container-stamp bundle-name mirror (oracle PRE-empty/POST-stamped proof; 142 resolved via decoded workspace). 烟雨长亭 r7 + 松月竹庭 r16 prefab metas match source; all prefab-bearing samples replayed monotonically. Packages manifest is byte-exact since the CRLF fix; auxiliary path-map mismatch remains open separately.

## DIFF-REFERENCE-WEB-EXPORT-001 — missing CLI locally bypassed

Frozen GUI.Web Commands routes accept `/LoadFile` and `/Export/UnityProject` forms. The existing isolated GUI.Free build produced fresh 玉京烟霞/烟雨长亭 projects via loopback without changing the baseline. `tools/export-reference-web.ps1` captures hashes/settings/logs, refuses existing destinations and stops its own server. Both exit0; comparison reveals new class86/121/124 gaps. “No CLI, blocked” is superseded. See newest validation-log evidence.

## DIFF-VALIDATION-REFERENCE-SCOPE-001 — local IDs and failure exits

Old checker matched only full GUID PPtrs. Missing local777 falsely reported zero dangling; dangling GUIDs still exited0. New checker validates local scope, YAML IDs, duplicates, owner existence and malformed syntax; JSON preserves issues. Thirteen contracts pass. Builtins/imported IDs remain PARTIAL; `-RequireComplete` exits2. Actual 烟雨长亭/云栖仙府/玉京烟霞 existence results have zero issues, with all incomplete target categories retained. No complete reference acceptance inferred.

## DIFF-VALIDATION-PROJECT-SCOPE-001 — importer/project omission

Old auditor ignored meta/non-Assets files; a different importer mainObjectFileID and EditorSettings falsely produced no differences. Supplementary results now include all project files; unpaired YAML contributes to totals; duplicate local IDs fail; `-RequireEquivalent` exits1 on unresolved raw differences. Nine contracts pass. Meta normalization uses proven per-project GUID ownership and root numeric timeCreated (frozen Meta.cs export-time CurrentTick), preserving timestamp presence, importer data, IDs, array order and opaque bytes. Diagnostic exit0 is not a completion gate.

This log records behavior differences found between the Rust port and the frozen local source. A source-code analysis is identified as such; it is not described as an executed C# differential.

## DIFF-MANAGED-DLL-SAVE-001 — manager-backed assembly save output

| Field | Finding |
|---|---|
| Status | The source manager-backed save behavior is isolated, and Rust emits semantically matching CLR metadata models for three source-generated Unity 2019.4.40f1 assemblies. Forwarder helper generation and PluginImporter layout selection are source-matched for the tested Unity version cases in `DIFF-MANAGED-FORWARDER-HELPERS-001`. PE layout and bytes differ; `ProjectSettings/EditorSettings.asset` remains absent. `TYPE-SCRIPT-001` and `EXPORT-UNITY-001` remain open. |
| Source location | `Scripts/ScriptExporter.cs` (`GetExportType`, `CreateExportPointer`); `Scripts/ScriptExportCollection.cs` (`Export`, `OnAssemblyExported`); `Import/Structure/Assembly/Managers/BaseManager.cs` (`Read`, `GetStreamForAssembly`); `Import/Structure/Assembly/ForwardingAssemblyGenerator.cs`; Rust: `src/processing/managed_assembly.rs`, `src/export/project_workflow.rs`, `vendor/dotnetdll-0.3.0/src/resolution/`, and `src/main.rs`. |
| Input and source-saved artifacts | Three independent source-authored fixtures: `FixtureScripts.dll`, 4,608 bytes, input SHA-256 `52117F6D87359D5F82D9985A6582ED59E55FA4C8193DD7456D7DC78DE5E946F1`, source-saved SHA-256 `713CD36E68542FB6567B1780BD566A8434FE5F80C1538622809EE0FD6C25E5D2`; `ManagedReferenceFixtureScripts.dll`, 5,120 bytes, input SHA-256 `1B20CEFF8CE7DE3E996D6497F3F7892B6472D53261DCED73F12D623FFDE902BB`, source-saved SHA-256 `1E8A6019535D7DF90091471C9C9734575BEA84EE2925F5B3C67BB88931780886`; `ComplexFixtureScripts.dll`, 5,120 bytes, input SHA-256 `D6E4155E931CFBC44ED92A44CC2436873D71DFBE293E9C568304DD118F2D6389`, source-saved SHA-256 `7DF4C790CF645C4AD14D156C7261082A29D7225B63C2D39EFD746BB1AFFBEB4D`. The source-saved DLLs are retained as `src/processing/managed_assembly_saved_source_saved_fixture.dll`, `managed_reference_schema_source_saved_fixture.dll`, and `managed_assembly_schema_complex_source_saved_fixture.dll`. |
| Source behavior | In the tested `DllExportWithoutRenaming` path, custom assemblies select `Save`; MonoScript pointers use `CalculateAssemblyGuid` plus the MonoScript file ID. `ForwardingAssemblyGenerator.Process` clears manager stream caches when Mscorlib is present. `BaseManager.GetStreamForAssembly` serializes the loaded module after that cache clear; the project collection writes the resulting DLL beneath `Assets/Plugins` with `PluginImporter` metadata. The isolated source exports also emit `System.Runtime.CompilerServices.Unsafe.dll` and `EditorSettings.asset`. |
| Rust behavior and regression | `ManagedAssembly::serialize_for_export` checks file bounds and PE architecture, parses and rewrites the managed assembly in Rust, and fails explicitly on malformed or unsupported machine headers. The production `export-project` save pass now uses this writer rather than copying source bytes. Tests cover 32-bit PE preservation, fields, signatures, all regular methods/property accessors/event methods, method implementation flags, and decoded IL bodies, plus malformed and unsupported-machine inputs. The project-workflow and CLI regressions preserve all input bytes, compare the exported type model with source-saved fixtures, and retain the already source-matched MonoBehaviour YAML and PluginImporter metadata. `dotnetdll 0.3.0` is locally vendored with fixes for ECMA-335 `ManagedMask` decoding, TypeDef method-list starts that account for property/event methods, and AssemblyRef ExportedType implementations that must preserve caller flags. See `vendor/dotnetdll-0.3.0/UPSTREAM-PATCHES.md`. |
| Independent PEReader comparison | The .NET 10 `System.Reflection.Metadata` probe compares assembly references, TypeDefs, base types, fields, methods, signatures and flags, property/event/accessor associations, method implementation flags, IL body lengths, local signatures, and exception regions. Source/Rust row counts match for all three cases: 34/34 (`ComplexFixtureScripts`), 19/19 (`FixtureScripts`), and 24/24 (`ManagedReferenceFixtureScripts`). Rust's decoded instruction-model regression additionally normalizes method/field table indices to identities and matches complete method-body models. |
| Output representation and remaining differences | The Rust writer produces 3,072 bytes for the complex DLL, 2,560 bytes for `FixtureScripts.dll`, and 2,560 bytes for `ManagedReferenceFixtureScripts.dll`; corresponding source outputs are 5,120, 4,608, and 5,120 bytes. These outputs are not byte-identical, and the fixture comparisons do not prove preservation of every possible PE resource or signing form. The manager-generated forwarder helper outputs are source-matched in the bounded cases under `DIFF-MANAGED-FORWARDER-HELPERS-001`; `ProjectSettings/EditorSettings.asset`, other script modes, PE architectures, assembly/resource variants, source-manager dependency closure, and complete project output remain open. Manager-backed reference-map selection remains tracked separately under `DIFF-MONOSCRIPT-REFERENCE-MSCORLIB2-001`. |

## DIFF-MANAGED-SCHEMA-NOTREE-001 — complex assembly-backed schema and project output

| Field | Finding |
|---|---|
| Status | Frozen-source runtime matched for one synthetic Unity 2019.4.40f1 no-TypeTree MonoBehaviour schema and workspace project-export route. `SER-TT-ABSENT-001`, `TYPE-SCRIPT-001`, and `EXPORT-UNITY-001` remain open. |
| Source behavior and input | `tools/probes/managed-project/Program.cs` authors a 488-byte generation-21 SerializedFile with no file TypeTree and three objects: MonoBehaviour PathID 42, MonoScript PathID 7, and TextAsset PathID 9. Input SHA-256 is `253BBFF419210012360717A1A3F3B19A607EDD7AADBAD889DA6718769CA071F3`. The managed fixture DLL is 5,120 bytes, SHA-256 `D6E4155E931CFBC44ED92A44CC2436873D71DFBE293E9C568304DD118F2D6389`. Its source `FieldSerializer` TypeTree is frozen as 33 nodes in `src/processing/managed_assembly_schema_complex_fixture.txt` (SHA-256 `B8F270AB72F077F985CB8D509CA86F74B1F7F80D52F14D52D3F46D095CD92CC5`). `Game.RichController` contains inherited scalars, an `int[]`, `List<float>`, a nested serializable struct/string, a Unity object pointer, a serialized private bool, and an enum, while excluding static, readonly, and nonserialized fields. |
| Source-runtime evidence | Two fresh isolated `export-managed-schema` runs, `target/managed-schema-source-replay-20261009-d123ae51c042491a96dac63285cd3628/a` and `/b`, both exited 0. Each produced the same normalized rows as `src/processing/managed_assembly_schema_project_fixture.txt` (4,799 bytes, SHA-256 `092DE172696C62AB76FF0507D78D6C7A821B2145746DFDADD2EA9F50FA78249C`). The separately frozen raw source capture `src/export/managed_schema_project_fixture.txt` was cross-checked against those normalized rows: replacing only the captured TextAsset GUID made its MonoBehaviour YAML identical; the TextAsset bytes and normalized TextScriptImporter and PluginImporter sidecars also matched. |
| Rust behavior and regression | `builds_source_schema_for_nested_and_collection_fields` matches all 33 source `FieldSerializer` nodes, including the nested string Array version. `project_export_matches_source_for_a_complex_no_type_tree_managed_schema` runs managed assembly discovery, `ObjectWorkspace::recover_mono_behaviour_structures`, graph rebuild, and `export_workspace_yaml_project_with_assembly_catalog`; it verifies all three objects decode, the reference graph is complete, and the output contains two serialized object exports plus the saved DLL. `export_project_matches_complex_missing_typetree_script_fields` independently verifies the product CLI path. Both project tests compare the full RichRoot YAML after substituting only the generated TextAsset GUID, plus exact TextAsset bytes and normalized importer sidecars. The output pointer uses source `AssetType.Meta` / PPtr type 3. |
| Compared artifacts | The captured raw source MonoBehaviour YAML is 645 bytes (SHA-256 `5C388802B87E059FC7B3FA5E64D45E585302FE119474094761F8147CEA5D14B7`). The source probe's GUID-normalized YAML is 631 bytes (SHA-256 `2B522F68B0EE2900C04E41ECBA46BCC3DDF8501528464D691264470CBC9E2D4A`), matching the Rust output after the one allowed GUID substitution. TextAsset bytes are 21 bytes (SHA-256 `435D25BED7D6170C5A49CF6F1433B370953607E0CB9A7CA36AA4C72841401272`). Normalized TextScriptImporter metadata is 134 bytes (SHA-256 `0808A2EE58BFA8092BCD8054ABA1664C3496525154590AC445A14183AFA33252`); normalized PluginImporter metadata is 557 bytes (SHA-256 `4125102E52A39385D375040FBFACED429B1BBFA33F2291505FE30A7F97133402`). |
| Remaining differences | This is not full project parity. The source reserializes the saved fixture DLL to SHA-256 `7DF4C790CF645C4AD14D156C7261082A29D7225B63C2D39EFD746BB1AFFBEB4D`, while Rust preserves the 5,120-byte input DLL; see `DIFF-MANAGED-DLL-SAVE-001`. The source also emits `System.Runtime.CompilerServices.Unsafe.dll` and `ProjectSettings/EditorSettings.asset`, which this Rust route does not. No Unity Editor import/runtime result is available; the installed editor has no valid license. No user AssetBundle or UABEA content was used. |

## DIFF-MANAGED-REF-ASSEMBLY-SCHEMA-001 — no-TypeTree assembly-resolved SerializeReference schemas across registry formats

| Field | Finding |
|---|---|
| Status | Frozen-source runtime and Rust production CLI match two synthetic assembly-resolved managed-reference cases: Unity 2019.4.40f1 uses terminated/index registry format 1; Unity 2021.2.0f1 uses counted/stable-ID format 2. This closes those fixture behaviors only; the broad parity rows remain open. |
| Source location | `AssetRipper.SerializationLogic/FieldSerializer.Logic.cs` (`HasStableReferenceIds` boundary at Unity 2021.2.0a19) and `FieldSerializer.cs` (`IndexedManagedReference` versus `ManagedReference` field shape); `AssetRipper.Import/Structure/Assembly/Serializable/ManagedReferenceRegistryReader.cs` (`Read`, `ReadTerminatedObjects`, `ReadCountedObjects`, `ReadReferencedObjectData`); `TypeTreeResolver.cs`; `BaseManager.TryGetSerializableType`; `SerializableTreeType.FromReferencedObjectNode`. Rust: `src/serialized_file/managed_registry.rs`, `src/serialized_file/object.rs`, `src/serialized_file/object_workspace.rs`, `src/processing/managed_assembly_schema.rs`, `src/export/project_workflow.rs`, and `src/main.rs`. |
| Input evidence | `tools/probes/managed-project/Program.cs` authors both two-file SerializedFile sets with no embedded TypeTrees. Unity 2019.4.40f1: `managed_reference_schema.assets` is 600 bytes (SHA-256 `8B50CEDEEBAC3B47033AA6F6C9C9E6CE5737BCD60D475AD74040957211C18DBC`); `target.assets` is 168 bytes (SHA-256 `8BAA072E745240C4F7BA495AFE2AC17DC75281CC2BC8DD3B273311469028B21A`). Unity 2021.2.0f1: the root is 592 bytes (SHA-256 `FF2A7FF93F3F428D02A7F8E9BD406531B38BFE1FDFF66ECFE7EF443369D2B6B3`) and the target is 168 bytes (SHA-256 `9A81772071EC4AF1B3B295A656741A2F5E6363EA74B2F0EC209EEB995BE2F5D7`). Both use the same 5,120-byte `ManagedReferenceFixtureScripts.dll` (SHA-256 `1B20CEFF8CE7DE3E996D6497F3F7892B6472D53261DCED73F12D623FFDE902BB`). The frozen source rows are 7,818 bytes / SHA-256 `FE4218C5EE97B4763B92299DBD4BB0FAAEA4A75EFCE931BB6DC5A6C46BF6B4E4` for the legacy case and 7,828 bytes / SHA-256 `0BA0CED711E65EB9FAC8C0443A23E63AFD41A13633C2120F085EB2ABA8375DE0` for the counted case. |
| Source-runtime evidence | Fresh .NET SDK 10.0.401 runs of `export-managed-reference-schema` reproduced both committed row files byte-for-byte. The source captured TypeTrees for `GraphHost`, `NodeA`, and `NodeB` through the initialized `gameData.AssemblyManager`. The 2019.4 root and nested references use the `id` field with sequential indices 0/1, registry version 1 and a `Terminus` triple; the 2021.2 path uses `rid`, stable 64-bit IDs 101/202 and registry version 2 with a count. Both source YAMLs contain marker 77, values 13/27, the A↔B cycle, and the external TextAsset pointer/content. |
| Rust behavior and regression | `ObjectWorkspace` passes a `ManagedAssemblyCatalog` resolver into registry payload decoding while retaining file `referenceTypes` as fallback. Direct workspace tests assert both SerializedFiles contain no file TypeTrees, recover both objects and all three managed edges, and resolve the PPtr to target PathID 9. Production CLI tests `export_project_recovers_legacy_assembly_managed_references_without_file_typetrees` and `export_project_recovers_assembly_resolved_managed_references_without_file_typetrees` run the full `export-project` route for both versions. The 886-byte legacy YAML (source fixture SHA-256 `A594CA950DBB4E068B69F4F2EDD873422F7ED6AE6862296027F5C2727A9CE430`) and 899-byte counted YAML (SHA-256 `E12E312E2961D0A26517606FDA1A3A44C7F233F8AF63E2C9D65769C003735D25`) match after replacing only the generated target/script GUIDs. In both cases, target bytes and normalized 134-byte TextScriptImporter and 557-byte PluginImporter sidecars match. |
| Remaining differences | For both versions, source post-processing rewrites the 5,120-byte DLL to SHA-256 `1E8A6019535D7DF90091471C9C9734575BEA84EE2925F5B3C67BB88931780886`, while Rust preserves the input. The source also emits `System.Runtime.CompilerServices.Unsafe.dll` and `ProjectSettings/EditorSettings.asset`; the Rust project route does not. These remain open under `DIFF-MANAGED-DLL-SAVE-001` and `EXPORT-UNITY-001`. Unity Editor import/runtime validation is unavailable because the installed editor has no valid license. No user AssetBundle or UABEA content was used. |

## DIFF-TEXTASSET-PROJECT-001 — TextAsset collection output and TextExportMode

| Field | Finding |
|---|---|
| Status | Source-runtime matched and fixed for a source-authored Unity 2019.4.40f1 no-TypeTree TextAsset under `Parse` and `Bytes`. |
| Source location | `TextAssetExporter.TryCreateCollection` and `Export`; `TextAssetExportCollection.GetExportExtension` and `CreateImporter`; `ProjectExporter.Overrides.cs`. Rust: `src/export/project_exporter.rs`, `src/export/project_workflow.rs`, `src/export/resource_extensions.rs`. |
| Input evidence | `src/processing/asset_factory_fixture.txt` contains the frozen-source `textasset_notree` SerializedFile: 120 bytes, SHA-256 `BF7601B1D6A9E99330B62AD36035389A038B5A0C0171A08BE738012799A6CCEC`. It is Unity 2019.4.40f1, class 49, PathID 42, name `Hero`, with `m_Script` equal to UTF-8 `print(1)` and no embedded file TypeTree. |
| Source-runtime evidence | `tools/probes/managed-project/Program.cs` runs the frozen `ExportHandler.LoadAndProcess` and `ExportHandler.Export` path. `export-textasset ... Parse` and `export-textasset ... Bytes` both exited 0. Committed outputs in `src/export/textasset_project_fixture.txt` record `Assets/TextAsset/Hero.txt` and `Assets/TextAsset/Hero.bytes`, each containing the exact eight input script bytes, plus the normalized `TextScriptImporter` sidecar. |
| Source behavior | `TextAssetExporter` claims only a non-empty `Script_C49`; its writer emits the original script bytes. With no best extension, `Parse` classifies `print(1)` as `txt`, while `Bytes` selects `bytes`. `TextAssetExportCollection` creates a `TextScriptImporter` whose 2019.4 sidecar contains `externalObjects`, `userData`, `assetBundleName`, and `assetBundleVariant`. |
| Rust behavior and regression | Project dispatch now supplies TextAsset script validity to collection selection, retains the serialized string bytes, applies `TextExportMode` to the output extension, writes those bytes directly, and emits `TextScriptImporter` metadata. `text_asset_exporter_requires_nonempty_script_data` checks the claim boundary. `export_project_routes_textasset_bytes_mode_and_importer` runs the product CLI for default `Parse` and settings-file `Bytes`, compares both normalized sidecars with the frozen-source fixture, checks the output paths and bytes, and verifies the input remains unchanged. |
| Scope | This closes the one source behavior for the tested 2019.4.40f1 input and two modes, not `EXP-SETTINGS-APPLY-001` or `EXPORT-UNITY-001`. `Txt`, best-extension overrides, AssetBundle-derived importer values, other Unity versions, empty-script SerializedFile dispatch, and Unity Editor import remain unverified. |

## DIFF-LOCAL-FS-ENUMERATION-TIMING-001 — directory enumeration validates paths at source-compatible stages

| Field | Finding |
|---|---|
| Status | Source-runtime matched and fixed for missing roots, invalid patterns, file paths, roots removed after query creation, and entries added before iteration |
| Source location | `LocalFileSystem.g.cs` delegates `EnumerateFiles`/`EnumerateDirectories` to `System.IO.Directory`; `GetFiles`/`GetDirectories` delegate to their eager counterparts |
| Input evidence | `tools/probes/local-filesystem/Program.cs` runs against the isolated frozen `AssetRipper.IO.Files.dll` under .NET SDK 10.0.401. Its normalized outcomes are committed in `src/io/enumeration_timing_fixture.txt`; the probe uses only scratch paths under `target/`. |
| Source behavior | Missing roots fail with `DirectoryNotFoundException` when the enumeration method is called; malformed patterns fail with `ArgumentException` before root lookup; an existing file path is accepted by the call and fails with `IOException` on first iteration. If a valid root is deleted after query creation, first iteration returns no entries. Files/directories created after query creation are visible when iteration begins. |
| Rust behavior and repair | `DirectoryEnumeration::new` now checks patterns first, rejects empty paths, and performs a metadata existence check at call time without requiring a directory. It still opens/reads the directory lazily. A root that disappears after the successful call ends with an empty iterator; a non-directory path still errors on iteration. |
| Regression evidence | `directory_enumeration_error_timing_matches_reference_runtime_fixture` compares the source fixture with Rust call and first-iteration outcomes for files and directories. The existing `local_directory_enumeration_matches_search_options_and_patterns` retains the search and recursion checks. |
| Scope | This settles the deferred-enumeration timing branch only. Entry ordering, which the baseline does not promise and Rust sorts for deterministic output, is unchanged. Other stream/container integration and platform checks remain open under `IO-STREAM-001`. |

## DIFF-BROTLI-METADATA-001 — metadata length off by one in source detection

| Field | Finding |
|---|---|
| Status | Frozen-source runtime confirms the scheme detector false-negative; Rust follows RFC 7932 and the direct source decoder confirms the payload |
| Source location | `AssetRipper/Source/AssetRipper.IO.Files/CompressedFiles/Brotli/BrotliFile.cs`, `BrotliFile.IsBrotliFile` |
| Input evidence | `tools/probes/io-differential/Program.cs` creates an actual temporary file containing a valid RFC 7932 Brotli stream with Unity's 36-byte metadata marker and a .NET BrotliStream-compressed UnityFS payload. The source `SchemeReader.LoadFile` result and direct `BrotliFile.Read` result are frozen in `src/compression/wrapper_scheme_runtime_fixture.txt`; a direct Rust regression checks both plus file-backed wrapper import and mounted UnityFS entry bytes. |
| Source-runtime behavior | The source `SchemeReader.LoadFile` selects `ResourceFile` and leaves the marked bytes untouched. Calling the source `BrotliFile.Read` directly on the same bytes produces the original UnityFS bytes. The detector reads the encoded `MSKIPLEN - 1` value and compares it directly with `BrotliSignature.Length`; for the valid 36-byte marker, the encoded value is 35, so the signature comparison fails. |
| Root cause | RFC 7932 §9.2 stores metadata length minus one. The detector omitted the `+1` needed to obtain the number of metadata bytes before comparing the marker length. |
| Rust change | `has_brotli_file_signature` now parses the Brotli window header, validates the empty metadata block fields and zero alignment bits, adds one to the encoded length, bounds the marker read, and compares the metadata bytes. The recursive scheme reader then decodes the same input under the configured output limit. |
| Regression evidence | The bit-level marker tests recognize and decode WBITS 10–24. The frozen-source runtime probe confirms the scheme-level false-negative, while the direct source decoder and Rust's file-backed path both recover the UnityFS payload. `docs/validation-log.md` records exact commands and test results. |
| Remaining comparison | No Unity-produced Brotli wrapper was available in Phase 1. The runtime fixture is a generated standards-valid file and does not establish the prevalence of this source detector bug in shipped inputs. |

The format reference is [RFC 7932 §9.2](https://www.rfc-editor.org/rfc/rfc7932.html#section-9.2). Its metadata length field stores `MSKIPLEN - 1`, and the metadata bytes are excluded from decompressed output.
## DIFF-MULTIFILE-SEEK-END-001 — source uses the opposite End offset sign

| Field | Finding |
|---|---|
| Status | Resolved: Rust now reproduces the isolated C# source behavior and has a direct regression |
| Source location | AssetRipper/Source/AssetRipper.IO.Files/Streams/MultiFileStream.cs, MultiFileStream.Seek |
| Input evidence | `tools/probes/multifile/MultiFileProbe.cs` runs against the isolated frozen `AssetRipper.IO.Files.dll` and records `SEEK_END|10|10|12|8` in `src/io/multifile_path_fixture.txt`. A fresh 16-row probe run on 2026-10-08 matched the committed fixture exactly. Rust test `multi_file_stream_seeks_from_end_with_source_offsets` checks the same offsets. |
| Source-runtime behavior | For length 10, the source returns 10 for End(0), 12 for End(-2), and 8 for End(+2), confirming `Position = Length - offset`. |
| Rust behavior | `MultiFileStream` now computes `Length - offset`, giving 10, 12, and 8 for End(0), End(-2), and End(+2). It also rejects negative and out-of-range positions rather than wrapping them into `u64`. |
| Resolution | The earlier deliberate hardening divergence was removed to restore source parity. `source_seek_target` applies this rule to both `MultiFileStream` and the file-backed `RandomAccessStream` view; `PartialStream` retains the source's separate standard-sign rule. |

## DIFF-MULTIFILE-PATH-001 — canonical split names and path-backed mounting

| Field | Finding |
|---|---|
| Status | Canonical path behavior now matches the frozen source's acceptance/rejection outcomes; source-runtime differential and mount regression pass |
| Source location | `AssetRipper.IO.Files/Streams/MultiFileStream.cs` (`IsMultiFile`, `OpenRead`); `SplitNameComparer.cs`; `AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`) |
| Input evidence | `tools/probes/multifile/MultiFileProbe.cs` runs only synthetic files under the Rust repository's ignored `target` directory against the isolated reference copy at `%LOCALAPPDATA%\Temp\opencode\ar-ref`. Its committed fixture records regular and split base/part names, `IsMultiFile` cases, part ordering through split10, boundary reads, missing-part rejection, and zero-padded-name rejection. |
| Source-runtime behavior | `SchemeReader.LoadFile` exposes the same logical base name for a regular path, split base, or supplied split part. Parts are numerically ordered; gaps and `.split00` plus `.split1` are rejected because the source requires exact `.split0` through `.splitN` names. |
| Rust change | `open_split_file` now checks the exact canonical names across the enumerated `.split*` candidate count before sorting/opening. This rejects `.split00` as a substitute for `.split0`; `SchemeReader::mount_path` carries the logical name into the VFS. |
| Regression evidence | `multi_file_paths_match_reference_runtime_fixture` compares path classification, bytes, names, numeric order, boundary reads, gap rejection, and padded-name rejection. `mount_path_assembles_split_inputs_and_uses_the_logical_base_name` and `mount_path_rejects_zero_padded_parts_the_reference_cannot_open` exercise the integrated mount path. |
| Remaining difference | Rust reports `NotFound` with the expected canonical path; the C# fixture records a rejected `Exception`. Both reject the malformed family. End-relative seek behavior now matches and is closed in `DIFF-MULTIFILE-SEEK-END-001`. |

## DIFF-LOCAL-FS-SHARING-001 — Rust's default Windows file sharing allowed input mutation

| Field | Finding |
|---|---|
| Status | Windows behavior is runtime-pinned. Unix read sharing is implemented from .NET 10 source and cross-target checked; non-Windows runtime validation is externally blocked. |
| Source location | `AssetRipper.IO.Files/LocalFileSystem.g.cs` (`LocalFileImplementation.OpenRead`); `LocalFileSystem.cs`; `Streams/MultiFileStream.cs`; `SchemeReader.LoadFile`; [.NET 10 SafeFileHandle.Unix.cs](https://github.com/dotnet/runtime/blob/release/10.0/src/libraries/System.Private.CoreLib/src/Microsoft/Win32/SafeHandles/SafeFileHandle.Unix.cs); [.NET 10 File.cs](https://github.com/dotnet/runtime/blob/release/10.0/src/libraries/System.Private.CoreLib/src/System/IO/File.cs); [.NET 10 FileSystem.Unix.cs](https://github.com/dotnet/runtime/blob/release/10.0/src/libraries/System.Private.CoreLib/src/System/IO/FileSystem.Unix.cs) |
| Input evidence | `tools/probes/local-filesystem/Program.cs` ran against the isolated frozen AssetRipper IO.Files project using synthetic files under the Rust repository's ignored `target` directory. `src/io/local_filesystem_fixture.txt` records that writes and deletes are rejected while ordinary and split input streams are open, and that their original bytes remain readable. |
| Source-runtime behavior | On Windows, `LocalFileSystem.File.OpenRead` uses .NET `File.OpenRead` (`FileShare.Read`), and writes/deletes fail with `IOException` while the handle is retained. In .NET 10 Unix source, `FileShare.Read` maps to a nonblocking shared `flock`; only `EWOULDBLOCK` rejects the open and other lock errors are best-effort. `File.WriteAllBytes` also requests `FileShare.Read`, so compatible shared locks allow it to write on supported local filesystems. Unix delete uses `unlink`, which the advisory lock does not prevent. [.NET documents `Directory.GetFiles` order as unspecified](https://learn.microsoft.com/en-us/dotnet/api/system.io.directory.getfiles?view=net-10.0). |
| Root cause | Rust initially matched Windows by setting a narrow share mode but left Unix at plain `File::open`; the earlier source note incorrectly stated that .NET Unix did not lock `FileShare.Read`. |
| Rust change | `open_os_read` uses Windows `FILE_SHARE_READ`; on Rustix-supported Unix targets it takes a nonblocking shared `flock` and rejects only `WOULDBLOCK`, matching .NET's best-effort treatment of other lock errors. Other targets retain native read-only opens. Discovery sorts results for deterministic output; this is compatible with the source's unspecified order contract. |
| Regression evidence | The fresh frozen-source probe exited 0 with 76 lines. After filtering four supplemental `SCHEME` rows and one direct `BROTLI` row covered by their own fixtures, the remaining 71 lines match `src/io/io_differential_fixture.txt` exactly; all three `DIR|*` rows match. Windows test `physical_file_read_sharing_matches_local_filesystem_runtime_fixture` and `mount_directory_preserving_containers_keeps_split_container_identity` cover retained inputs. Linux and Windows test targets pass `cargo check --workspace --tests`; the Unix lock regression was cross-compiled, not run. |
| Remaining comparison | No Linux runtime, WSL distribution, or container runtime is available here, so the non-Windows .NET and Rust retained-reader cases and their integrated directory-import replay have not run. This row remains blocked and cannot be marked verified until a supported non-Windows runtime is available. |

## DIFF-LOCAL-FS-ADAPTER-001 — local filesystem wrappers and temporary storage

| Field | Finding |
|---|---|
| Status | The Windows reference probe matches the implemented LocalFileSystem file, directory, text, enumeration, path, and temporary-storage operations. Remaining text-encoding and cross-platform runtime gaps keep the aggregate adapter open. |
| Source location | `AssetRipper.IO.Files/FileSystem.cs`; `LocalFileSystem.cs`; generated `LocalFileSystem.g.cs`; `Streams/Smart/SmartStream.cs` |
| Input evidence | `tools/probes/local-filesystem/LocalFileSystemProbe.csproj` ran against the shipped frozen `AssetRipper.IO.Files.dll` with scratch files beneath `target/`. Its 37 normalized rows are committed in `src/io/local_filesystem_fixture.txt`; random GUIDs and absolute paths are reduced to shape and existence facts. |
| Source-runtime behavior | The default temp path first probes `<AppContext.BaseDirectory>/temp/<4-hex>` and falls back to `<Path.GetTempPath()>/AssetRipper/<4-hex>` on IO/authorization failure. `File.CreateTemporary` creates an empty full-GUID file; `Directory.CreateTemporary` creates an 8-hex child. The setter ignores blank values and stores a full path. File and directory Get/Enumerate forms are exercised for top-level and recursive wildcard searches, including modern *.* matching of extensionless files. Default text writes are UTF-8 without a preamble; reads detect UTF-8/16 BOMs; explicit UTF-16 writes include its preamble. Path Join, name, extension, directory, rooted, full-path, and relative-path results are source-probed. `File.OpenWrite` preserves existing tail bytes and starts at position zero; `WriteAllBytes` is blocked while a Windows `File.OpenRead` is retained. `DeleteTemporaryDirectory` recursively removes its tree and is a no-op once absent. |
| Rust change | New `src/io/local_filesystem.rs` centralizes source-style shared/exclusive file opens, byte and text I/O, create/delete/exists, lazy directory enumeration, path helpers, temp file/directory methods, and local-first/system-fallback temp selection. `SmartStream::create_temp` now uses this adapter and deletes the temporary file after its last shared reference drops. |
| Regression evidence | Windows test `local_filesystem_operations_match_the_frozen_runtime_fixture` compares all 37 normalized source rows. Additional tests cover local temp selection, fallback when the executable root is not a directory, blank setters, file sharing, and GUID shape. `default_temporary_smart_stream_uses_local_filesystem_policy_and_lifetime` checks the production stream connection and reference lifetime. |
| Remaining comparison | TextEncoding currently covers UTF-8, UTF-16LE/BE, and UTF-32LE/BE; arbitrary .NET Encoding instances and custom fallback policies remain open. Enumerate* result sets are source-probed, but deferred error timing is not compared. The Unix FileShare runtime probe is separately blocked by the lack of a non-Windows runtime. Aggregate status stays `IO-STREAM-001` in progress. |

## DIFF-REFERENCE-TEST-NEWLINE-001 — 20 reference test failures are a checkout artifact, and they confirm LF is the intended YAML newline

| Field | Finding |
|---|---|
| Status | Root cause identified from source plus executed reference test output. Not a product defect and not a Rust defect. |
| Source location | `AssetRipper/Source/AssetRipper.Tests/Traversal/DefaultYamlWalkerTests.cs`; `AssetRipper/Source/AssetRipper.Yaml/YAMLWriter.cs` |
| Input evidence | The reference suite was executed in an isolated copy. `AssetRipper.Tests` reported `Failed: 20, Passed: 151, Total: 171`. Every failure message has the same shape, for example `Expected: "%YAML 1.1\r\n%TAG !u!..."` versus `But was: "%YAML 1.1\n%TAG !u!..."` with `Strings differ at index 9`, which is the first newline. The seven other test projects passed completely: AssemblyDumper 9/9, Assets 57/57, GUI.Web 6/6, IO.Files 137/137, Numerics 65/65, SerializationLogic 54/54, Yaml 11/11. Reference total: 339 passed, 20 failed. |
| Source-derived behavior | `DefaultYamlWalkerTests.GenerateYaml` constructs `StringWriter { NewLine = "\n" }`, so the generated document uses LF. The expected values are C# raw string literals, whose line endings come from the `.cs` file itself. |
| Root cause | The frozen checkout is CRLF. `DefaultYamlWalkerTests.cs` was measured as 354 LF of which 354 are CRLF and 0 are lone LF, so every raw string literal contains `\r\n` while the writer deliberately emits `\n`. A CRLF assertion cannot match an LF writer. |
| Reference build status | `dotnet 10.0.401` is available on this workstation, so `dotnet restore AssetRipper.slnx` and `dotnet build AssetRipper.slnx -c Release` both exit 0 across all 51 projects. The earlier handoff statement that `dotnet` was unavailable no longer holds. |
| Rust behavior | `src/export/unity_yaml.rs` already emits `\n` for the YAML head, node line breaks, and the document separator, which matches the source writer's configured newline and not the CRLF checkout. No Rust change was required. |
| Remaining comparison | This finding documents a test-fixture/checkout mismatch, not a behavior difference. A LF checkout of the same commit would make the 20 assertions pass. Unity Editor import checks and the reference GUI/web runtime remain unrun. |


## DIFF-YAML-DASH-WHITESPACE-001 — `-` followed by non-space Unicode whitespace

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper/Source/AssetRipper.Yaml/YamlScalarNode.StringNode.cs`, `GetStringStyle` / `IllegalStringsRegex` (`(^-\s)`) |
| Input evidence | Isolated probe (`Temp/opencode/yamlemitprobe`) referencing the built net10.0 `AssetRipper.Yaml.dll`: `YamlScalarNode.Create("-\u00A0foo")` and `"-\u2003foo"` emit `'-\u00A0foo'` and `'-\u2003foo'` (single-quoted), while `"-\u200Bfoo"` stays plain. |
| Source-derived behavior | The regex alternative `(^-\s)` uses .NET `\s` = `[ \f\n\r\t\v\x85\p{Z}]`, so a `-` followed by any whitespace starts a quoted scalar. Control whitespace is already DoubleQuoted earlier, so only non-control Unicode whitespace (for example U+00A0, U+2003) reaches this branch. |
| Root cause | The Rust predicate used `value.starts_with("- ")`, which only recognizes an ASCII space after the dash. |
| Rust change | `string_needs_quotes` now tests `value.starts_with('-')` plus `char::is_whitespace` on the following character. |
| Regression evidence | `unity_yaml::tests::scalar_quoting_matches_the_reference_yaml_runtime` embeds the probe's exact document bytes for `-\u00A0foo`, `-\u2003foo`, and `-\u200Bfoo`. |
| Remaining comparison | Only the probe's 48 cases were executed; a broader value sweep and Unity-produced YAML remain unrun. |

## DIFF-YAML-EMPTY-WHITESPACE-001 — empty plain value drops the pending space

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper/Source/AssetRipper.Yaml/Emitter.cs`, `Write(ReadOnlySpan<char>)` |
| Input evidence | The same isolated probe wrote `{k: <empty string>}`; the reference runtime emitted `k:\n` (no space after the colon), while the Rust emitter produced `k: \n`. |
| Source-derived behavior | `Emitter.Write(ReadOnlySpan<char>)` calls `WriteDelayed()` only when the span is non-empty. An empty plain scalar therefore lets the newline from `EndTransition` cancel the deferred whitespace instead of flushing it. |
| Root cause | The Rust `write_str` unconditionally called `write_delayed()` before appending, flushing the deferred space. |
| Rust change | `write_str` now returns early for an empty value, matching `Write(ReadOnlySpan<char>)`. |
| Regression evidence | The differential table's first case (`""` -> `k:\n`) plus the empty-string cases in `EXP-YAML-QUOTING-001`. |
| Remaining comparison | The rule is pinned for scalar values; whether an empty mapping/sequence node produces the same cancellation in every layout was not separately probed. |


## DIFF-YAML-FLOAT-FORMAT-001 — float scalars must follow .NET shortest-round-trip formatting

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper/Source/AssetRipper.Yaml/YamlScalarNode.NumericNode.cs`; `YamlScalarNode.FloatingPointHexNode.cs`; `Emitter.cs` `Write(float)`/`Write(double)`; `AssetRipper/Source/AssetRipper.IO.Files/InvariantStreamWriter.cs` |
| Input evidence | Isolated net10.0 probe (`Temp/opencode/numericprobe/numeric_probe.json`, 203 cases) capturing `Single`/`Double.ToString(CultureInfo.InvariantCulture)` for curated boundaries plus 60+60 deterministic pseudo-random finite bit patterns. |
| Source-derived behavior | Floats pass through `TextWriter.Write` under the baseline's invariant culture, so Unity YAML floats use .NET's default shortest-round-trip "G" format: `NaN` -> `NaN`; `+Inf` -> `Infinity`; `-Inf` -> `-Infinity`; zero keeps its sign (`0`/`-0`). Scientific notation is used when the decimal exponent `e` is below -4 or at least 9 (`Single`) / 17 (`Double`); the exponent is always signed and zero-padded to two digits. `FloatingPointHexNode` wraps the same text as `0x{bits:x8}({value})` / `0x{bits:x16}({value})`. |
| Root cause | Rust emitted `f32`/`f64` through `Display`, which never uses exponent notation and spells infinities `inf`/`-inf`; for example `1E-05` became `0.00001` and `3.4028235E+38` became a 39-digit integer. |
| Rust change | Added `format_dotnet_f32`/`format_dotnet_f64`, which parse Rust's shortest-round-trip `LowerExp` digits and apply the measured notation thresholds; both the plain and hex scalar arms now use them. |
| Regression evidence | `unity_yaml::tests::float_formatting_matches_the_reference_runtime` asserts all 203 captured values; the quoting differential next to it also exercises `\u0000` and control characters. |
| Remaining comparison | The probe covers finite values and the special forms; no Unity-produced YAML float comparison has run, and the earlier `EXP-YAML-NUMERIC-001` row previously relied on Rust formatting. |


## DIFF-NORMALIZE-DIVISION-001 — `Vector3.Normalize` divides instead of using a reciprocal

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `System.Numerics.Vector3.Normalize` used by `CompressedMeshExtensions.GetNormals`/`GetTangents`/`MakeFloatAndSignArrays` |
| Input evidence | Isolated probe (`Temp/opencode/meshprobe`) called `Vector3.Normalize` on `(1.2, 0.1, 0)`, `(-3, 4, 0)`, and `(1, 1, 1)` and recorded the exact bit patterns; the Rust port using a reciprocal gave `1065295263` for the first component of the first vector where .NET gives `1065295264`. |
| Source-derived behavior | `Vector3.Normalize` computes `value.X / length`, `value.Y / length`, `value.Z / length` — a per-component division, not a `1.0 / length` reciprocal multiply. |
| Root cause | The first Rust implementation multiplied by `1.0f32 / length`, which rounds differently in the last ULP. |
| Rust change | `resources::compressed_mesh::normalize` now divides each component by the length. |
| Regression evidence | `resources::compressed_mesh::tests::normalize_matches_the_reference_runtime` plus the normals/tangents differential in `geometry_matches_the_reference_runtime`. |
| Remaining comparison | Only three normalize inputs were probed; the fix is the source's actual formula, so broader inputs follow from the same expression. |


## DIFF-VALUECONSTANT-TYPEID-001 — `ValueConstant_5_5`'s `TypeID` is inert

| Field | Finding |
|---|---|
| Status | Confirmed source behavior recorded; the Rust port models an explicit `type_id` and notes the constraint |
| Source location | `AssetRipper.SourceGenerated.Subclasses.ValueConstant.ValueConstant_5_5` (generated); consumed by `AssetRipper.SourceGenerated.Extensions/ClipExtensions.cs` `ConvertValueArrayToGenericBinding` |
| Input evidence | Isolated probe (`Temp/opencode/vcread`, `vcfields`, `vctype`) created `ValueConstant.Create(version)` for 4.3–2017. `ValueConstant_4` has a private `m_TypeID` field and round-trips `TypeID`; `ValueConstant_5_5` has only `m_ID`, `m_Type`, `m_Index`, its `TypeID` getter always returns 0, and setting `TypeID` or `Type` leaves it 0. |
| Source-derived behavior | `ClipExtensions.ConvertValueArrayToGenericBinding` reads `values.Data.ValueArray[i].TypeID`. On the `_5_5` layout (Unity 5.5+) that value is always 0, so every entry takes the `else` branch: an Animator binding with `Attribute = ID` and stride 1, instead of the Transform position/rotation/scale classification. |
| Root cause | The generated `_5_5` subtype dropped the `m_TypeID` serialized field (the CRC now lives in `Type`), but the extension was not updated to read `Type`. This is a baseline quirk, not a Rust defect. |
| Rust change | `resources::clip::ClipView::convert_value_array_to_generic_binding` reproduces the source algorithm over an explicit `ValueArrayEntry::type_id`. The serialization layer must mirror the generated class: for the `_5_5` layout leave `type_id` at 0 (do not substitute `Type`), so 5.5+ conversion degenerates identically. |
| Regression evidence | `resources::clip::tests::clip_matches_the_reference_runtime` asserts the four-binding result for the `_4` layout (4.3/5.0/5.4) and the empty result when a clip has no binding; the fixture is committed as `src/resources/clip_binding_fixture.txt`. |
| Remaining comparison | No real 5.5+ asset with a legacy `Binding` array has been converted through both tools, and the Unity re-import effect of the degenerated bindings has not been observed. |


## DIFF-GUID-TOSTRING-001 — `UnityGuid.ToString` nibble order

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | External `AssetRipper.Primitives.UnityGuid.ToString`; the generated `GUID` subclass override in `AssetRipper.AssemblyDumper/Passes/Pass502_FixGuidAndHashYaml.cs`; consumed by `Meta.ExportYamlDocument` (`root.Add(GuidName, GUID.ToString())`) and every project-pointer YAML GUID |
| Input evidence | Isolated probe (`Temp/opencode/guidprobe2`) constructed `UnityGuid` from word tuples and byte spans and captured `ToString`/`ToByteArray`. For `Data0=0x01020304, Data1=0x11121314, Data2=0x21222324, Data3=0x31323334` the reference yields `40302010413121114232221243332313`, the per-word reversal of the big-endian hex; `ToByteArray` round-trips the four little-endian words. |
| Source-derived behavior | `UnityGuid.ToString` emits each 32-bit word's nibbles from least significant to most significant, which is the canonical Unity layout: the well-known `unity default resources` GUID is `0000000000000000e000000000000000` for the word tuple `(0, 0, 0xE, 0)`. |
| Root cause | The Rust `format_unity_guid` read the four little-endian words but emitted each word's nibbles most-significant first, producing the reversed string (`00000000000000000000000e00000000` for the same tuple). Every `.meta` GUID and every YAML GUID reference was therefore reversed relative to the baseline. |
| Rust change | `format_unity_guid` now emits nibbles least-significant-first. Ten self-authored test expectations that encoded the reversed convention were corrected to the reference-derived values (GUID map keys, `m_Guid`/`m_Guids`, `Meta`/pointer YAML, sidecar bytes, workspace pointer GUIDs). |
| Regression evidence | `export::unity_yaml::tests::unity_guid_string_matches_the_reference_runtime` asserts the four committed probe rows (`src/export/guid_fixture.txt`) plus the canonical engine GUID; the previously failing Meta/pointer/value/workspace tests now pass unchanged in behavior. |
| Remaining comparison | The `Hash128.ToString` helper's exact byte layout is in the external package and was not probed; no Unity-import run has confirmed the exported GUIDs. |


## DIFF-GUID-TOSTRING-002 — `FileIdentifier.GetFilePath` Meta GUID

| Field | Finding |
|---|---|
| Status | Fixed in Rust; shares the `DIFF-GUID-TOSTRING-001` reference rule |
| Source location | `AssetRipper.IO.Files/SerializedFiles/Parser/FileIdentifier.cs` `GetFilePath` (and `ToString`): for `AssetType.Meta`, `return Guid.ToString();` |
| Input evidence | The same `Temp/opencode/guidprobe2` probe established the `UnityGuid.ToString` nibble order. The Rust `serialized_file::metadata::FileIdentifier::get_file_path` was formatting the four words with `{data0:08x}{data1:08x}{data2:08x}{data3:08x}` (most-significant nibble first). |
| Source-derived behavior | Meta external identifiers resolve to the canonical Unity GUID string, e.g. the words `(0x01234567, 0x89abcdef, 0, 0xffffffff)` become `76543210fedcba9800000000ffffffff`. |
| Root cause | The same reversal as `format_unity_guid`: the four little-endian words were emitted most-significant nibble first, so the dependency identity string disagreed with every GUID the exporter writes. |
| Rust change | `FileIdentifier::get_file_path` now emits each word's nibbles least-significant first, matching the fixed `export::unity_yaml::format_unity_guid`. The `io::dependency` Meta-identity test's registered name and expected string were updated to the corrected value. |
| Regression evidence | `io::dependency::tests::meta_bundle_identity_uses_guid_but_platform_dependency_uses_path_name` asserts the corrected GUID string; the non-Meta path identity and missing-GUID branches are unchanged. |
| Remaining comparison | No real external-dependency resolution through a Meta identifier has been exercised against a Unity project. |


## DIFF-PACKEDFLOAT-NAN-001 — cross-platform NaN sign in packed float unpack

| Field | Finding |
|---|---|
| Status | Fixed in Rust; found by GitHub Actions on Linux |
| Source location | `AssetRipper.SourceGenerated.Extensions/PackedFloatVectorExtensions.cs` reader: `return (x / halfMaxValue) + Start;`; Rust `resources::packed_float_vector::PackedBitReader::read` |
| Input evidence | The `float_vectors_match_the_reference_runtime` case `start=10.0, range=2.0, bitSize=32, data=0102030405060708` unpacks via the source quirk `mask = 0` so `halfMaxValue = 0`, giving `0.0 / 0.0`. The Windows reference (`Temp/opencode/floatprobe`) and the Windows Rust build both produced the positive quiet NaN `0x7FC00000`; the Linux x86-64 CI build produced the negative quiet NaN `0xFFC00000` (left `[4290772992, 4290772992]` vs right `[2143289344, 2143289344]`). |
| Source-derived behavior | The baseline targets Windows .NET, whose `0.0 / 0.0` yields `0x7FC00000`. The raw hardware default NaN sign is platform-dependent. |
| Root cause | Rust left the division result as the hardware default NaN, whose sign differs between the Windows and Linux x86-64 code paths. |
| Rust change | `PackedBitReader::read` canonicalizes a NaN result to `f32::from_bits(0x7FC00000)`, matching the Windows baseline deterministically. |
| Regression evidence | The same differential test now passes on both `ubuntu-latest` and `windows-latest` in GitHub Actions run `37247323096`. |
| Remaining comparison | Only the `mask = 0` `0/0` NaN path is canonicalized; a NaN produced from a NaN input payload is not separately probed. |


## DIFF-HASH128-OVERFLOW-001 — `HashHelper.ToString` checked-overflow quirk

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | Generated `AssetRipper.SourceGenerated.Helpers.HashHelper.ToString`, called by the `Hash128` `ToString` override injected in `Pass502_FixGuidAndHashYaml.cs` |
| Input evidence | Probe `Temp/opencode/hashprobe2` set the generated `Hash128` byte fields and captured `ToString` for seven patterns. Bytes `00..0f` yields `03020100070605040b0a09080f0e0d0c`; `0000007f...` yields `7f000000000000000000000000000000`; any word whose most significant byte is `>= 0x80` (`abababab...`, `...ff`, `00000080...`) throws `OverflowException`. |
| Source-derived behavior | The shipped helper builds each 32-bit word with checked `int` arithmetic, so the top byte's high bit overflows. The success format is each little-endian word's big-endian hex, the opposite nibble order from `UnityGuid.ToString`. |
| Root cause | The injected helper source casts to `uint` (which cannot overflow), but the shipped helper evidently uses checked `int`; the Rust port had no overflow path. |
| Rust change | Added `export::unity_yaml::format_hash128`, which returns `Hash128Error::Overflow` when any word's most significant byte is `>= 0x80` and otherwise emits the big-endian-per-word hex; `value_yaml`'s Hash128 conversion now propagates `UnityYamlObjectConversionError::Hash128Overflow` instead of always emitting a string. |
| Regression evidence | `export::unity_yaml::tests::hash128_string_matches_the_reference_runtime` asserts all seven committed rows (`src/export/hash128_fixture.txt`), including the 0x7F/0x80 boundary. |
| Remaining comparison | No real asset with a high-bit-top-byte hash has been exported through both tools; the per-asset failure handling of the baseline was not observed. |


## DIFF-TEXDEC-EXPANSION-001 — DXT bit-replication vs RGB565 bare-shift expansion, and the three-color DXT opaque-black index

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `DxtDecoder` and `RgbConverter`, dispatched by `AssetRipper.Export.Modules.Textures/TextureConverter.cs` |
| Input evidence | Probe `Temp/opencode/texdecprobe` decoded the shipped package. The DXT endpoint `0xF800` becomes `255` (`(31 << 3) \| (31 >> 2)`), while the standalone `RGB565` converter maps the same word to `248` (`31 << 3`). A DXT1/3/5 block with `color0 = 0x0000 <= color1 = 0xFFFF` and all index 3 yields opaque black `(0,0,0,255)`, not transparent black. |
| Source-derived behavior | The two code paths intentionally use different channel expansion. The three-color DXT index 3 is decoded as black with the default alpha `255` because the color routine does not carry transparency. |
| Root cause | A naive port would share one RGB565 expansion and would emit transparent black for the three-color DXT index. |
| Rust change | Added `resources::texture_decode` with a DXT-specific `dxt_rgb565_to_rgba` (bit replication) and a separate `rgb565_to_rgba` (bare shift), and a shared `write_color_block` whose three-color table ends in `[0, 0, 0, 255]`. |
| Regression evidence | `resources::texture_decode::tests::texture_decoders_match_the_reference_runtime` asserts all 22 committed rows (`src/resources/texture_decode_fixture.txt`), including the non-extreme `0x1860` endpoint and the `color0 <= color1` DXT3/DXT5 blocks. |
| Remaining comparison | Only 4×4 and 2×2 single-block inputs were probed; multi-block clipping is asserted structurally but not differentially. |


## DIFF-BC45-ROUNDING-001 — `Bc` decoders round the alpha interpolation; the `Dxt` decoder truncates

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Bc/BcHelpers.SmoothAlphaBlock` (used by `Bc4`/`Bc5`) versus `Dxt/DxtDecoder.DecompressDXT5` |
| Input evidence | Probe `Temp/opencode/texdecprobe` decoded the same alpha endpoints `a0 = 16, a1 = 32` through both paths. `Bc4` yields the table `16, 32, 19, 22, 25, 29, 0, 255`; `DxtDecoder.DecompressDXT5` yields `16, 32, 19, 22, 25, 28, 0, 255`. The `Bc` values match `(weight0 * a0 + weight1 * a1 + 1) / divisor`; the `Dxt` values match the same expression without the `+ 1`. |
| Source-derived behavior | The two decoder families are separate forks (bcdec vs uTinyRipper) with different rounding. `Bc4`/`Bc5` round up; `DxtDecoder` truncates. |
| Root cause | A shared alpha table would make one family disagree with the reference. |
| Rust change | Added `resources::texture_decode::bc4_alpha_table` (with `+ 1`) and kept `dxt5_alpha_table` (without), and pointed `decode_bc4`/`decode_bc5` at the rounded table. |
| Regression evidence | The committed fixture `src/resources/texture_decode_fixture.txt` includes six direct table-probe rows (`BC4T`) whose `pixel i` index is `i % 8`, so every table entry is asserted for six endpoint pairs. |
| Remaining comparison | The DXT decoder source confirms the truncating formula; only the sampled endpoints were probed. |


## DIFF-ETC-ALPHAORDER-001 — ETC2A8 alpha and color use opposite block permutations

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Etc/EtcDecoder.cs`: `DecodeEtc2Block` writes `output[WriteOrderTable[i]]`; `DecodeEac11Block` writes `output[WriteOrderTableRev[i] * 4]` |
| Input evidence | Probe `Temp/opencode/etcprobe` decoded an ETC2A8 block whose alpha indices ramp and whose color block is fixed; the resulting alpha channel is the reverse permutation of the color channel, which a naive shared loop would not reproduce. |
| Source-derived behavior | `WriteOrderTableRev[i] = WriteOrderTable[15 - i]`, so the alpha channel of pixel `p` is filled from index `15 - p`. |
| Root cause | A single shared write order would swap the alpha of symmetric-looking blocks. |
| Rust change | `resources::etc_decode` keeps `WRITE_ORDER_TABLE` for color and `WRITE_ORDER_TABLE_REV` for the EAC/alpha channel, and `decode_eac11_constant`/`decode_eac11_table` iterate the reversed table. |
| Regression evidence | `resources::etc_decode::tests::etc_decoders_match_the_reference_runtime` asserts all 23 committed rows, including the ETC2A8 rows and the EAC R/RG rows that exercise the reversed order. |
| Remaining comparison | Only 4×4 single-block inputs were probed; multi-block clipping is asserted structurally but not differentially. |


## DIFF-ATC-COLOR-001 — ATC endpoint interpolation and explicit-alpha branch differ from DXT

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Atc/AtcDecoder.cs` `DecodeColors`/`Extend` |
| Input evidence | Probe `Temp/opencode/texprobe2` decoded `c0 = 0x1CE7, c1 = 0x1CE7` (opaque) and `c0 = 0x9CE7, c1 = 0x001F` (explicit alpha). The opaque endpoints expand through `Extend(v,5,8) = (v<<3)|(v>>2)` and interpolate with `5/8` and `3/8` weights, not the DXT `1/3`/`2/3` weights. |
| Source-derived behavior | ATC's explicit-alpha branch sets color 0 to zero and derives color 1 as `max(0, c2 - c3/4)`; the alpha interpolation reuses the truncating DXT5-style table. |
| Root cause | Reusing the DXT color routine or the `+1`-rounded `Bc` alpha table would disagree with the reference. |
| Rust change | Added `resources::atc_yuy2` with ATC-specific `decode_colors`/`extend` and the truncating `dxt5_alpha_table` for alpha. |
| Regression evidence | `resources::atc_yuy2::tests::atc_and_yuy2_match_the_reference_runtime` asserts all five committed rows. |
| Remaining comparison | Only two ATC blocks were probed. |


## DIFF-BC6H-HALF-001 — BC6H 16-bit values are reinterpreted as half-floats for output conversion

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Bc/Bc6h.cs`: the decoded buffer is typed `ColorRGB<Half>` and `CopyBufferToOutput<ColorRGB<Half>, Half, TOutputColor, TOutputChannelValue>` is used |
| Input evidence | Probe `Temp/opencode/bc6hprobe` decoded the mode-0 block `0030557a...`. The reference output `1700ffff...` (R=23, G=0, B=255) is impossible if the 16-bit values are converted as integers (`(v*255+32895)>>16`), because `FinishUnquantize` caps at `31743` (which maps to `124`). Interpreting the same 16-bit values as binary16 floats reproduces the output. |
| Source-derived behavior | `FinishUnquantize` returns a `ushort` that is stored into a `ColorRGB<Half>` buffer, so each channel is a half-float bit pattern. The conversion to `byte` is `clamp((float)half * 255)`. |
| Root cause | A first port converted the 16-bit values as integers, which disagreed on every channel. |
| Rust change | `resources::bc6h` converts through `half_bits_to_f32` + `float_to_byte`, and the half/float helpers in `texture_decode` were made crate-visible. |
| Regression evidence | `resources::bc6h::tests::bc6h_matches_the_reference_runtime` asserts all 19 committed rows across the fourteen modes. |
| Remaining comparison | Only the unsigned path (`isSigned = false`) is exercised, matching the `TextureConverter` call site. |


## DIFF-PVRTC-BLUE-001 — PVRTC second-colour blue expands into the wrong slot

| Field | Finding |
|---|---|
| Status | Mirrored in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Pvrtc/PvrtcDecoder.cs` `Unpack5554Colour`, non-opaque branch |
| Input evidence | Probe `Temp/opencode/pvrtcprobe` decoded a PVRTC4 8×8 image whose colours exercise the translucent branch. The reference writes `abColors[i * 4 + 2]` but then expands `abColors[0 * 4 + 2]` for both colours, so the second colour's blue (slot 6) is left un-replicated. A "corrected" port that expands slot 6 disagrees on the affected pixels. |
| Source-derived behavior | Colour A's blue is expanded twice (once with `>> 3` and once with `>> 4`); colour B's blue is never expanded. |
| Root cause | A literal translation of the reference is required; "fixing" the index changes the output. |
| Rust change | `resources::pvrtc::unpack_5554_colour` reproduces the slot-2 write for both branches. |
| Regression evidence | `resources::pvrtc::tests::pvrtc_matches_the_reference_runtime` asserts all four committed rows for PVRTC2 and PVRTC4. |
| Remaining comparison | Only 8×8 power-of-two images were probed. |

## DIFF-ASTC-BITTRANSFER-001 — `BitTransferSigned` argument order

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by a reference-runtime differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Astc/AstcDecoder.cs`, `DecodeEndpoints` cases 5, 9, and 13 |
| Input evidence | Probe `Temp/opencode/astcprobe` decoded 4×4 and 8×8 blocks. A port that treated the first `BitTransferSigned` argument as `v[0]` disagreed on every multi-partition block, for example block 0 of the first row: reference `136,178,42,128` versus the swapped port `33,25,0,50`. |
| Source-derived behavior | The reference calls `BitTransferSigned(ref v[1], ref v[0])`, so the first parameter `a` is `v[1]` and `b` is `v[0]`. Inside the helper `b = (b >> 1) | (a & 0x80)` and `a = (a >> 1) & 0x3f` (signed). |
| Root cause | The initial Rust port named the locals `a0 = v[0]`, `b0 = v[1]` and passed them in that order, mirroring the reference's register roles instead of its argument order. |
| Rust change | `resources::astc::decode_endpoints` now passes `(&mut v1, &mut v0)`, `(&mut v3, &mut v2)`, `(&mut v5, &mut v4)`, and `(&mut v7, &mut v6)` for cases 5, 9, and 13. |
| Regression evidence | `resources::astc::tests::astc_matches_the_reference_runtime` asserts all eight committed rows. |
| Remaining comparison | Only 4×4, 6×6, and 8×8 block footprints were probed. |

## DIFF-ASTC-SHIFT-001 — C# masks `ulong` shift counts

| Field | Finding |
|---|---|
| Status | Fixed in Rust and regression-covered by the same differential |
| Source location | `AssetRipper.TextureDecoder` 2.6.3 `Astc/AstcDecoder.cs`, `GetBits64` and `BitReverseU8` |
| Input evidence | The first Rust run panicked with "attempt to shift left with overflow" at `GetBits64`'s `bit <= 0` branch and at `BitReverseU8`'s `>> (8 - bits)` when `bits == 0`. |
| Source-derived behavior | C# shifts a `ulong` by `count & 0x3f` and a promoted `int` (from `byte`) by `count & 0x1f`; `BitReverseTable[c] >> 8` is `0`, not an error. |
| Root cause | Rust's `<<`/`>>` on fixed-width integers panic (debug) when the shift count is not less than the width; the source relies on C#'s masking. |
| Rust change | `resources::astc::get_bits64` uses `wrapping_shr`/`wrapping_shl` for every branch, and `bit_reverse_u8` promotes the table byte to `i32` before shifting. |
| Regression evidence | The committed `astc_fixture.txt` rows include the reversed-intseq blocks that reach the masked shifts. |
| Remaining comparison | The masking behavior is asserted through decoded output, not by a direct helper probe. |

## DIFF-MESH-GENERATED-API-001 — serialized mesh field names from the generated API

| Field | Finding |
|---|---|
| Status | Source-derived evidence recorded; not an executed serialization differential |
| Source location | Shipped `AssetRipper.SourceGenerated.dll` (`IMesh`, `ISubMesh`, `IBoneWeights4`, `IStreamInfo`, `IChannelInfo`, `ICompressedMesh`); `AssetRipper/Source/AssetRipper.AssemblyDumper/Passes/Pass002_RenameSubnodes.cs` (`data[0]` becomes `data_0_`); `ValidNameGenerator.GetValidFieldName` |
| Input evidence | Probe `Temp/opencode/meshprops` dumped the generated property lists, committed as `src/resources/mesh_generated_api_fixture.txt` (105 lines). `ISubMesh` exposes `BaseVertex`/`FirstByte`/`FirstVertex`/`IndexCount`/`IsTriStrip`/`LocalAABB`/`Topology`/`TriangleCount`/`VertexCount`; `IBoneWeights4` exposes `BoneIndex_0_`..`BoneIndex_3_` and `Weight_0_`..`Weight_3_`; `IStreamInfo` exposes `ChannelMask`/`Offset`/`Stride_Byte`/`Stride_UInt32`/`DividerOp`/`Frequency`. |
| Source-derived behavior | The naming rule replaces invalid characters (`[`/`]`) with underscores, prefixes `m_`, and capitalizes, so `BoneIndex_0_` inverts to the TypeTree name `boneIndex[0]` and `Stride_Byte`/`Stride_UInt32` invert to a `stride` field with two storage widths. |
| Root cause | The frozen snapshot does not contain the generated `SubMesh`/`BoneWeights4` classes, so the TypeTree spellings cannot be read directly from it. |
| Rust change | `resources::mesh::read_sub_meshes`/`read_skin`/`read_streams` use the inverted names; a regression test pins the fixture property sets the mapping relies on. |
| Regression evidence | `resources::mesh::tests::generated_api_fixture_supports_the_field_name_mapping` asserts all relied-upon properties. |
| Remaining comparison | A residual `m_`-prefix ambiguity remains for sub-struct children, and no serialized `Mesh` payload or C# serialization round-trip has been executed. |

## DIFF-MESH-TYPETREE-GATES-001 - real class-43 TypeTrees correct the reader's version gates

| Field | Finding |
|---|---|
| Status | Source-derived evidence recorded; not an executed serialization differential |
| Source location | Reference `SourceTpk` (`AssetRipper.SourceGenerated.SourceTpk`) via `AssetRipper.Tpk` 2.0.0.0: class-43 release TypeTrees for 3.5.0a0, 5.0.0a0, 2019.0.0a0, 6000.6.0a5 |
| Input evidence | Probe `Temp/opencode/tpkprobe` walked the reference Tpk recursion and dumped every release node; the 909-line dump (185/223/238/255 nodes) is committed as `src/resources/mesh_typetree_fixture.txt`. |
| Source-derived behavior | Unity 3.5 `VertexData` has four fixed `m_Streams[0..3]` (`channelMask`/`offset`/`stride`/`align`, all `UInt32`) and no `m_Channels`; Unity 5.0+ uses `m_Channels` (`stream`/`offset`/`format`/`dimension`, all `UInt8`); `m_IndexFormat` exists only on 2019+. Unity 3.5 `CompressedMesh` carries `m_BindPoses`/`m_Colors` but no `m_FloatColors`/`m_UVInfo`; Unity 5.0+ is the reverse. Unity 3.5 `SubMesh` carries `isTriStrip` (`UInt32`) + `triangleCount` instead of `topology`, and has no `baseVertex`. `m_MeshMetrics[0/1]` exist only on 2019+. |
| Root cause | The reader previously required `m_BindPoses`/`m_FloatColors`/`m_Colors` unconditionally, read `m_Streams` only as a list, and asserted `m_IndexFormat` on all versions, so a real Unity 3.5 or 5.0 payload would fail or misread. |
| Rust change | `resources::mesh::read_compressed_mesh` treats the four gated fields as optional with presence-derived gates; `read_vertex_data` falls back to the four fixed `m_Streams[N]` structs; `m_IndexFormat` absence keeps the 16-bit default; the `SubMesh` `topology`/`baseVertex`/`triangleCount`/`isTriStrip` branches were already optional and are now pinned per version. |
| Regression evidence | `resources::mesh::tests::real_class43_typetrees_match_the_reader_field_names` asserts the exact child lists, gates, and element types for all four versions; `unity_3_5_fixed_streams_are_read_without_channels`, `a_unity_5_compressed_mesh_reads_with_the_pre_five_gates_off`, and `a_pre_unity_5_compressed_mesh_reads_with_the_unity_5_gates_off` pin the reader behavior. |
| Remaining comparison | No serialized `Mesh` payload from Unity and no C# serialization round-trip has been executed; the fixture is the reference's own TypeTree database, so a Unity/AssetRipper divergence inside the Tpk itself would not be caught. `m_CurrentChannels` (3.5/5.0) is not captured because the read path never consumes it. |

## DIFF-MESH-METRIC-001 - `CalculateMeshMetric` value and error behavior

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.SourceGenerated.Extensions/MeshExtensions.cs` (`CalculateMeshMetric`, `TriangleArea`); `TriangleEnumerable.cs` |
| Input evidence | Probe `Temp/opencode/meshread` invoked the private static directly through an expression-tree delegate (five value cases), committed as `src/resources/mesh_metric_fixture.txt`: square `1`, scaled `0.25`, flat `1`, empty `1`, strip `0.5`. A second probe section filled versioned meshes and read back `MeshMetrics_0_/1_`: `1`/`1` on `Mesh_2019` (`Has_MeshMetrics_0_/1_` true), `0`/`0` on `Mesh_5` (both gates false), matching the `m_MeshMetrics` TypeTree fields which exist only on 2019+. |
| Source-derived behavior | The metric is left-associative float division (`vertexAreaSum / n / uvAreaSum`); a single unit triangle yields `1`, not `0.5`. Short strips yield nothing without error; partial triangle/quad groups and out-of-range indexes throw in the reference. The `FillWithCompressedMeshData` stamp persists only when the generated class carries the fields. |
| Root cause | N/A (new port; the hand computation in the first draft said `0.5` for one triangle and the probe corrected it before the test was written). |
| Rust change | `resources::mesh_metric::{calculate_mesh_metric, triangle_triples, triangle_area_v2, triangle_area_v3}` with the source operation order; throws become `MeshMetricError` values. |
| Regression evidence | `resources::mesh_metric::tests::metric_matches_the_reference_runtime` asserts all five probed words exactly, `metric_stamp_follows_the_generated_class_gates` pins the version-gated stamp, plus winding/degenerate/partial-group/unsupported/out-of-range tests. |
| Remaining comparison | Only five value cases were probed. The stamp write path itself is not ported; any future port must gate it on field presence. |

## DIFF-SHADER-DUMMY-001 - shader script/parsed-form version gates and unparsed dummy text

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.SourceGenerated` class-48 `Shader_*`/`IShader` (`Script`, `ParsedForm`, `Has_Script`, `Has_ParsedForm`); `AssetRipper.Export.UnityProjects/Shaders/DummyShaderTextExporter.cs` (`ExportShader`); `SimpleShaderExporter.cs` (`TryCreateCollection`); `ProjectExporter.Overrides.cs` (shader registration order) |
| Input evidence | Probe `Temp/opencode/shaderexport` created real versioned shaders (`Shader.Create`, SDK 10.0.401), set `Script`, and called the shipped `DummyShaderTextExporter.ExportShader` plus both exporters' `TryCreateCollection`. Gates: `Shader_3_5`/`Shader_5` report `Has_Script` true / `Has_ParsedForm` false; `Shader_2019_3_0_b0`/`Shader_2022_2_0_b10` report the reverse. Six unparsed-path rows are committed as `src/export/shader_dummy_fixture.txt` (escaped `SEXPORT` rows: script with/without `SubShader`, with `Program`, on both script-carrying versions). |
| Source-derived behavior | The unparsed dummy output is the script header up to (not including) the first `SubShader`, then `\t//DummyShaderTextExporter\n`, one tab, the fallback body, and `}`; a missing `SubShader` returns false with no output. The header slice retains the script's own whitespace before `SubShader` (hence the doubled tab in the probed rows). `Simple` admits exactly scripts with decompiled text and is tried first under every mode, so a decompiled script never reaches the dummy exporter through the stack; `Decompile` mode falls back to the dummy exporter (no decompiler exporter exists in this snapshot). |
| Root cause | N/A (new port; the stacked-vs-direct distinction was caught while writing the test: the first draft asserted the dummy text through the stack for an admitting script, which the reference would route to `Simple` instead). |
| Rust change | `resources::shader::{ShaderView, script_has_decompiled_text}` (byte-oriented; presence-driven gates) and `export::shader_export::{export_shader_text, render_unparsed_dummy_bytes}` reusing `select_shader_exporter`; `Yaml` and parsed-form dummy are explicit errors, never silent fallbacks. |
| Regression evidence | `export::shader_export::tests::dummy_text_matches_the_reference_runtime` asserts all six probed rows byte-for-byte, separating direct-dummy assembly from stack dispatch; `byte_and_str_dummy_assembly_agree_on_valid_utf8` pins the byte/str equivalence; four `export-shader` CLI tests cover a synthetic class-48 SerializedFile end to end. |
| Remaining comparison | Only three script shapes on two versions were probed. The parsed-form dummy path (property block plus `TemplateList` selection over generated objects) is not ported; 2019+ shaders with a parsed form fail explicitly. No Unity-produced shader payload has been exported. |

## DIFF-CUSTOM-CURVE-REGEX-001 - `UnknownPathRegex` without a regex dependency

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs` (`UnknownPathRegex`: `^path_0x[0-9A-F]{1,10}_[H-Wh-w]{6}[HJLN]$`) |
| Input evidence | Probe `Temp/opencode/rpathprobe` evaluated the real .NET regex (same pattern literal, `RegexOptions.Compiled`, SDK 10.0.401) on 21 paths; rows committed as `src/resources/custom_curve_rpath_fixture.txt`. The four manufactured shapes match; lowercase hex, wrong prefixes, short/long tails, out-of-range tail letters (`G`, `Q`, `X`), and embedded separators do not. |
| Source-derived behavior | The matcher is a pure classifier over ASCII: literal `path_0x`, one to ten uppercase hex digits, one underscore, six `[H-W]`/`[h-w]` characters, one `[HJLN]` character. No regex crate is added; the manual matcher implements exactly this shape. |
| Root cause | N/A (new port). |
| Rust change | `resources::custom_curve::is_unknown_path` plus the surrounding data-driven `ToAttributeName` branches (constraint tables, renderer/material/sprite rules, converter prefixes). |
| Regression evidence | `resources::custom_curve::tests::unknown_path_matches_the_dotnet_regex` asserts all 21 probed rows; the shape test pins the boundary literals independently of the fixture. |
| Remaining comparison | The regex itself has no version branches. Scene- and checksum-dependent `ToAttributeName` branches are not ported. |

## DIFF-STREAMED-SLOPES-001 - streamed-curve slopes and frame decoding

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.Processing/AnimationClips/Editor/StreamedCurveKey.cs` (`Read`, `CalculateSlopes`, `SetRegularSlopes`, `SetNonNegSpecialSlopes`, `CalculateSpecialSlopes`, `SetNegInfSlope`, `SetPosInfSlope`, `HasDifferentLimits`); `StreamedFrame.cs` (`Read`, duplicate collapse, 2017.1+ `Align`) |
| Input evidence | Probe `Temp/opencode/sclipprobe` drove real `StreamedCurveKey` instances through `CalculateSlopes` (eight cases: regular, negative-infinity with arithmetic next-limit, a chained inexact-limit second calculation, equal/step limits with and without negative-infinity support, fractional times) and real `StreamedFrame.Read` over crafted little/big-endian buffers (duplicate collapse, pre/post-2017.1 alignment). Eleven rows committed as `src/resources/streamed_curve_fixture.txt`, with slopes, values, and next-key limits as exact bit words. |
| Source-derived behavior | Regular slopes use double-precision deltas with single-precision casts at the source positions; a coefficient-free next key under negative-infinity support gets an arithmetic left limit marked inexact. Zero-coefficient keys yield zero, positive, or negative infinity by the limit comparison; discontinuous estimates use the `5e-7f`-as-double threshold against `max(|LSL|, |RSL|)`. Frames collapse consecutive same-index curves keeping the last and align to four bytes on 2017.1+. The first comparison in `HasDifferentLimits` promotes the right limit to double (not a float truncation), which the port reproduces. |
| Root cause | N/A (new port; one test-replay bug on the chained row was caught before committing: the replay middle key initially carried the wrong coefficients and produced negative instead of positive-zero-third-slope output, fixed by reconstructing the exact probe sequence). |
| Rust change | `resources::streamed_curve::{StreamedCurveKey, StreamedFrame}` with bounded reads; truncation, negative counts, and oversized counts are explicit `StreamedCurveError` values. |
| Regression evidence | `resources::streamed_curve::tests::slope_calculation_matches_the_reference_runtime` asserts all eight probed slope rows bit-for-bit (replaying the chained arithmetic limit through the same two-call sequence); `frame_read_matches_the_reference_runtime` asserts all three frame rows; `frame_alignment_uses_the_absolute_buffer_offset` additionally checks the exact 2017.0/2017.1 boundary and absolute input offset; truncation/negative-count tests cover the error paths. |
| Remaining comparison | Eight slope shapes, three frame layouts, and six next-frame searches were compared with the source runtime. The exact 2017.1 alignment predicate is also covered at 2017.0 and 2017.1. The converter and `GenerateFramesFromStreamedClip` are now implemented and replayed against one source-generated Unity 2022.3 clip under `DIFF-ANIMATION-CLIP-PROCESS-001`; no Unity-produced clip or dynamic Unity sample has been tested, and project export wiring remains open under the broader animation/export rows. |

## DIFF-CLIP-CONVERT-001 - next-frame search and banker's rounding

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values |
| Source location | `AssetRipper.Processing/AnimationClips/AnimationClipConverter.cs` (`TryGetNextFrame`, `CalculateLastConstantTime` frame math via `float.Round`) |
| Input evidence | Probe `Temp/opencode/sclipprobe` invoked the private static `TryGetNextFrame` via reflection over frames with curve indexes `[3,7]`, `[7,3]`, `[7]` (six search rows) and evaluated `float.Round` on nine inputs; rows appended to `src/resources/streamed_curve_fixture.txt` (`SNEXT`, `ROUND`). The rounding rows confirm to-even behavior with sign preservation (`-0.5` rounds to `-0.0`). |
| Source-derived behavior | The search scans later frames in order and returns the first frame/position holding the curve ID, or misses. Frame-count math uses banker's rounding before truncation. |
| Root cause | N/A (new port). |
| Rust change | `resources::streamed_curve::try_get_next_frame` and `resources::clip_convert::{bankers_round, truncate_frame_count, calculate_last_constant_time}` plus the bindings cache, first-frame fix, dense/constant rules, and default weight. |
| Regression evidence | `next_frame_search_matches_the_reference_runtime` asserts all six search rows; `bankers_round_matches_the_reference_runtime` asserts all nine rounding words including the negative-zero case; branch tests cover every `CalculateLastConstantTime` path with synthetic frames. |
| Remaining comparison | The rounding probe covers nine inputs; the stop-time branches are synthetic-only. Curve emission stays open. |

## DIFF-ANIMATION-CLIP-PROCESS-001 - full generated AnimationClip conversion

| Field | Finding |
|---|---|
| Status | Full converter output matches one source-generated Unity 2022.3.0f1 clip and scene graph. Other version branches and product wiring remain open. |
| Source location | `AssetRipper.Processing/AnimationClips/AnimationClipConverter.cs` (`Process`, streamed/dense/constant processing, curve emission); `PathChecksumCache.cs`; `CustomCurveResolver.cs`; `AssetRipper.SourceGenerated.Extensions/AnimationClipExtensions.cs` (`FindRoots`) |
| Input evidence | The isolated `tools/probes/anim-convert` `convert` mode creates real generated GameObject, Transform, Renderer, Mesh, Material, AnimatorController, Animator, and AnimationClip objects at Unity 2022.3.0f1, populates 23 GenericBindings, streamed/dense/constant data, and one AnimationEvent, then invokes the shipped converter. A fresh run exited 0 and wrote 5,137 bytes with SHA-256 `A4C1BA2733ABAB473E46C924CEF3D343E4E1EB69293E04195BC9009EB714CAD5`. |
| Source-derived behavior | The converter's complete mutations cover streamed frame generation and slope calculation, transform and Euler expansion, default engine/GameObject curves, custom-curve fallback naming with hierarchy facts, dense and constant samples, a PPtr curve, event preservation, and MuscleClipInfo initialization. The generated null `Script` PPtr remains a zero-valued pointer (`PathID=0`). |
| Rust change | `resources::animation::converter_fixture::source_probe_input` reconstructs the deterministic source input in the Rust `AnimationClipObject`/`SceneFactsTable` model; `process_clip` runs the full conversion and the formatter compares all emitted curve and event fields against the source output fixture. |
| Regression evidence | `complete_clip_process_matches_the_source_generated_asset` matches every output row and float bit pattern in `src/resources/animation/clip_convert_fixture.txt`; the source probe's fresh output and committed fixture hashes match exactly. Full workspace validation passed with 1,532 library tests (1 ignored), 98 CLI tests, and 1 ignored doc test; warnings-denied Clippy, formatting, diff check, and all 1,449 frozen baseline files passed. |
| Remaining comparison | This source-probe fixture covers one 2022.3 conversion, not all version/platform branches. The production `ObjectWorkspace` to `export-project` caller and TypeTree curve writer are now covered separately by `DIFF-ANIMATION-PROJECT-EXPORT-001`. External script FileID identity, remaining processor/AnimatorController paths, Unity Editor import, and dynamic sampling remain open. Keep `PROC-ANIM-001`, `PROC-ANIM-CONVERT-001`, `PROC-ANIM-CUSTOM-CURVE-001`, and `RES-ANIM-001` open; `PROC-ANIM-STREAMED-001` is verified for its bounded streamed decode and next-frame lookup contract. |

## DIFF-AUDIO-DECODE-001 - decoder dispatch, diagnostic fill, and clip version gates

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on probed values, plus one source-derived correction |
| Source location | `AssetRipper.Export.Modules.Audio/AudioClipDecoder.cs` (`TryDecode`, `CheckMagic`, `CopyPrintable`, `CopyHex`); `AssetRipper.SourceGenerated.Extensions/AudioClipExtensions.cs` (`GetAudioData`, `CheckAssetIntegrity`); `FmodSoundTypeExtensions.cs` (`ToRawExtension`); generated class-83 `AudioClip_*` gates |
| Input evidence | Probe `Temp/opencode/audioprobe` drove real `AudioClip.Create` instances with `AudioData`/`Type` set through the shipped `TryDecode` (SDK 10.0.401): the empty early message, typed `wav`/`ogg` passthrough with raw bytes, and the unmapped-type `audioClip` default — four rows committed as `src/export/audio_decode_fixture.txt`. Probe `Temp/opencode/audioprops` pinned the `IAudioClip` properties, the per-version gates (3.5 inline+typed, 5.0+ resource), and the `FmodSoundType`/`FmodSoundFormat` discriminants (`src/resources/audio_generated_api_fixture.txt`). The magic/diagnostic branches are reachable only through resource-backed 5.0+ clips, so they stay source-read. |
| Source-derived behavior | On 3.5 layouts `Has_Type` is version-true, so `TryDecode` always takes the typed path there; the magic checks apply to untyped (resource-backed) data. The unknown-byte diagnostic fills with U+00D8 (`'Ø'`), not ASCII `'O'`: the prior Rust port used the wrong fill and its test encoded it. Fixed to the source literal with the test corrected. |
| Root cause | The fill-character deviation predates this increment (wrong by one codepoint in `diagnostic_printable`); caught while tracing the diagnostic path for the CLI error. |
| Rust change | `resources::audio::{AudioClipView, fmod_sound_type_raw_extension}` (presence-driven gates, source-order resolution, lazy integrity check); `export::audio_export::try_decode_audio` composing the reference order with exact messages; `export-audio` CLI with inline and streamed-resource resolution. `FSB5`/Vorbis stay explicit errors. |
| Regression evidence | `export::audio_export::tests::try_decode_matches_the_reference_runtime` asserts all four probed rows; the corrected fill test pins U+00D8; seven `export-audio` CLI tests cover typed/tracker passthrough, streamed resolution, empty/unknown/FSB5 failures, the class-83 guard, and overwrite refusal. |
| Remaining comparison | Only four decode shapes were probed; `OggToWav` could not load in the probe sandbox (NAudio closure mismatch) and stays source-read. No Unity-produced clip has been exported. |

## DIFF-SHADER-PARSED-001 - parsed-form dummy property rendering and template selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows after one spacing fix |
| Source location | `AssetRipper.Export.UnityProjects/Shaders/DummyShaderTextExporter.cs` (`Export(IShader)`, `Export(ISerializedProperties)`, `Export(ISerializedProperty)`); `AssetRipper.SourceGenerated.Extensions/SerializedPropertyExtensions.cs` (`GetTypeString`, `GetDefaultValue`, `GetAttributes`); `TemplateList.cs`/`TemplateShader.cs`/`RequiredProperty.cs`/`PropertyType.cs`; `SerializedPropertyType.cs`/`SerializedPropertyFlag.cs` |
| Input evidence | Probe `Temp/opencode/shaderexport` built real 2019.4 `Shader.Create` instances with a `ParsedForm` through the generated API (lit, flags, single) and called the shipped `ExportShader` (SDK 10.0.401); rows committed as `src/export/shader_parsed_fixture.txt`. Probe `Temp/opencode/floatprobe` pinned the 28 `float.ToString(InvariantCulture)` values behind default rendering (`src/export/float_tostring_fixture.txt`). |
| Source-derived behavior | Property lines emit stored attributes then flag keywords in fixed order, `Vector` for color/vector, `Range(min, max)` with a space, texture keywords by `TexDim`, vector defaults `(v0,v1,v2,v3)` without spaces, and scalar defaults from `DefValue_0_`; the header/properties/marker/template/fallback/custom-editor assembly matches `render_parsed_dummy`. The shipped-DLL templates agree with the frozen snapshot sources (spaced `Default`, unspaced others). |
| Root cause | The first Rust port rendered `Range(min,max)` without the reference space; the differential caught it. |
| Rust change | `resources::shader::{ParsedShaderProperty, ParsedShaderForm, ShaderView::from_value}` decoding; `export::shader_export::{render_parsed_property, render_parsed_properties, parsed_dummy_body, export_parsed_dummy}` wired into the `Dummy`/`Decompile` stack. Serialized `m_*` names follow the class-48 convention without a local class-48 TypeTree dump. |
| Regression evidence | `export::shader_export::tests::parsed_dummy_text_matches_the_reference_runtime` matches all three probed outputs byte-for-byte through direct assembly and the mode stack; `float_tostring_matches_the_reference_runtime` pins all 28 formatter rows. |
| Remaining comparison | Only three property shapes were probed; `any`/`3D`/`2DArray`/`CubeArray` dimensions and `NoScaleOffset`/`Normal` flags are asserted directly. No Unity-produced shader has been exported. |

## DIFF-BLENDSHAPE-001 - blend-shape CRC lookup and versioned wire layout

| Field | Finding |
|---|---|
| Status | Source-runtime and Rust production-workspace comparison closes the bounded lookup behavior for source-generated Unity 4.1 and 5.0 Mesh inputs. `RES-BLENDSHAPE-001` remains open for geometry and animation integration. |
| Source location | `AssetRipper.SourceGenerated.Extensions/MeshBlendShapeExtensions.cs` (`IsCRCMatch`); `BlendShapeDataExtensions.cs` (`FindShapeNameByCRC`); `MeshExtensions.cs` (`FindBlendShapeNameByCRC`); `MeshBlendShapeChannelExtensions.cs` (`SetValues`); generated `IMesh` 4.1/5.0 implementations; `AssetRipper.Checksum.Crc32Algorithm` (external package). |
| Input evidence | A fresh isolated .NET 10 probe generated no-TypeTree Mesh SerializedFiles through the source `AssetFactory`, `WriteAsset`, and `SerializedFileBuilder`: modern 5.0 input, 440 bytes, SHA-256 `4C8CF84EB293B1469BC273EE2F29F89FECF0748F5544E89E8B52122C73C1BDB1`; legacy 4.1 input, 472 bytes, SHA-256 `FDAF0F4CB3578738E89BBB2DE0ED6210E1A6018ABC9D1496D04A1F4F0E4C59BF`. Source output rows are committed in `src/resources/blend_shape_source_fixture.txt`, SHA-256 `3D666D96AAEA7E0D36D5F5A46E90C6124720000286B09B306A0E4135B7D35EC2`. |
| Source-derived behavior | `FindBlendShapeNameByCRC` checks `Has_Shapes()` first and returns the first `BlendShapeData.Channels` entry whose stored `NameHash` matches; a miss returns null and never falls through. In generated 4.1 Mesh, `Has_Shapes()` is false and `Has_ShapesList()` is true, while that legacy list is serialized as an array in the raw `m_Shapes` field; `m_ShapesList` is an API property name, not the wire field name. Legacy `IsCRCMatch` rejects a missing name, accepts a present empty name for CRC 0, and scans in order. `SetValues` writes name, UTF-8 CRC, frame index, and frame count. |
| Root cause | The earlier Rust decoder expected a separate serialized `m_ShapesList` field. Source-generated no-TypeTree 4.1 input showed that this wire assumption was wrong. |
| Rust change | `BlendShapeLookupView::from_value` now discriminates the versioned `m_Shapes` value: a struct decodes modern channels; an array decodes legacy `MeshBlendShape_4_1` records; other present kinds return an explicit error. The source-generated fixture loads through `SerializedFileWorkspace` and `ObjectWorkspace`; the resolver compares first-match and miss behavior, while decoded channel fields preserve the source `SetValues` frame index/count. |
| Regression evidence | Fresh `blend-shape` source probe exited 0 and reproduced all non-hex rows in the committed fixture. `source_generated_legacy_and_modern_meshes_resolve_blend_shape_bindings` passes against both no-TypeTree SerializedFiles and verifies decoded names, duplicate hashes, frame indices/counts, first-match, misses, legacy absent/empty/named values, and CRC behavior. The 12 shipped `Crc32Algorithm.HashUTF8` vectors and `MatchUTF8` accept/reject rows remain covered by `src/resources/crc28_fixture.txt`. Full workspace and static checks are recorded in `docs/validation-log.md`. |
| Remaining comparison | This does not decode blend-shape vertex deltas, normals/tangents, frame ranges, or weights into `MeshData`, nor does it connect real Mesh objects to animation binding/export. Other legacy/channel version branches and Unity-produced meshes remain unverified. No full `RES-BLENDSHAPE-001` closure is claimed. |
| Evidence | `src/resources/blend_shape_source_fixture.txt`; `tools/probes/scene-processor/Program.cs`; `src/resources/blend_shape.rs`; `AssetRipper.SourceGenerated.Extensions/{MeshExtensions.cs,MeshBlendShapeExtensions.cs,BlendShapeDataExtensions.cs,MeshBlendShapeChannelExtensions.cs}`; `docs/validation-log.md`. |

## DIFF-SCRIPT-IDENTITY-001 - assembly names and script identity

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.IO.Files/SpecialFileNames.cs` (`FixAssemblyName`, `RemoveAssemblyFileExtension`, `AddAssemblyFileExtension`, `IsProjectAssembly`); `AssetRipper.Import.Structure.Assembly/ScriptIdentifier.cs`; `ScriptingBackend.cs`; `MonoScriptExtensions.cs` (`GetFullName`, `GetValidAssemblyName`) |
| Input evidence | Probe `Temp/opencode/scriptidprobe` (SDK 10.0.401) drove the shipped `AssetRipper.IO.Files.dll` and `AssetRipper.Import.dll`; all 49 rows committed as `src/io/script_identity_fixture.txt`. The probe pinned the case-sensitive edges (`csharp`/`CSHARP.DLL`/`A.DLL` untouched), the prefix-before-strip order (`Boo.dll` → `Boo`, unprefixed), the `Assembly-`/`Assembly - ` prefix pair, the `[assembly]name` unique-name shapes, the default `ToString` type-name fallback, and the `ArgumentException`/`ArgumentNullException` rejections. |
| Source-derived behavior | Legacy module names gain `Assembly - ` before `.dll` stripping; `GetValidAssemblyName` falls back to `Assembly-CSharp` on empty; `GetFullName` qualifies only with a non-empty namespace. |
| Root cause | No deviation found; this increment ports previously uncovered identity behavior. |
| Rust change | `io::special_names::{fix_assembly_name, remove/add_assembly_file_extension, is_project_assembly}`; `processing::script_identity::{ScriptIdentifier, ScriptingBackend, get_full_name, get_valid_assembly_name}`. |
| Regression evidence | `assembly_name_helpers_match_the_reference_runtime` matches all 30 name rows; four identity tests match all 19 remaining rows. The null-namespace rejection is documented as unrepresentable in Rust. |
| Remaining comparison | The `IMonoScript`/assembly-manager resolutions (`GetBehaviourType`, `GetScriptID`, `IsScriptPresents`, `GetPropertiesHash`) need live managed assemblies and stay unported. |

## DIFF-EXPORT-SETTINGS-001 - configuration enum discriminants and record defaults

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.Export/Configuration/*.cs` (all 14 files); `AssetRipper.Import/Configuration/ImportSettings.cs`; `AssetRipper.Processing/Configuration/ProcessingSettings.cs` |
| Input evidence | Probe `Temp/opencode/settingprobe` (SDK 10.0.401) dumped all 59 enum discriminants plus 24 record defaults from the shipped assemblies; rows committed as `src/export/settings_fixture.txt`. The probe closed a real gap (the full 21-member `ScriptLanguageVersion`, including `Latest = 2147483647`) and confirmed every existing Rust default. |
| Source-derived behavior | Enum orders/discriminants are declaration order except `ScriptLanguageVersion` (explicit values); record defaults are the documented initializers with `DefaultVersion`/`TargetVersion` at `0.0.0a0`. |
| Root cause | No deviation found; this increment ports previously unverified discriminants and one missing enum. |
| Rust change | `export::settings::{discriminant consts, ScriptLanguageVersion (full), ExportDefaults/ImportDefaults/ProcessingDefaults}` asserted against the fixture, including the capitalized .NET boolean rendering. |
| Regression evidence | `enum_discriminants_and_record_defaults_match_the_reference_runtime` matches all 59 discriminants and 24 defaults. |
| Remaining comparison | Automatic language selection and version-record defaults need external version ordering and stay unported. |

## DIFF-SCRIPT-RESOLVER-001 - TypeTreeResolver match and failure reasons

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.Import.Structure.Assembly.Serializable/TypeTreeResolver.cs` |
| Input evidence | Extended probe `Temp/opencode/scriptidprobe` (SDK 10.0.401) invoked the internal shipped resolver through reflection: `RESOLVE|empty|0|<not-found>` and `RESOLVE|broken-tree|0|<node-build-failed>` appended to `src/io/script_identity_fixture.txt`. The broken-tree row proves the three-way match fired (a miss would report not-found). |
| Source-derived behavior | Ordinal class/namespace match plus extension-stripped assembly match, first-match scan order, and the two verbatim failure reasons. A default (null-name) identifier can never match. |
| Root cause | No deviation found; this increment ports previously uncovered resolver behavior. |
| Rust change | `processing::script_identity::{type_reference_matches_script, find_type_reference_index, TYPE_REFERENCE_NOT_FOUND, TYPE_TREE_NODE_BUILD_FAILED}` over the existing `SerializedTypeReference` metadata model. |
| Regression evidence | `type_reference_scan_matches_the_reference_runtime` replays the probed match and pins both literals; `type_reference_match_covers_assembly_stripping_case_and_absence` pins stripping, ordinality, absence, the default-identifier rule, and first-match order. |
| Remaining comparison | Building nodes from the matched `OldType` needs the TypeTree reader and stays unported. |

## DIFF-FILE-NAME-001 - export file-name planning and splitter edges

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows after two fixes |
| Source location | `AssetRipper.IO.Files/FileSystem.cs` (`RemoveCloneSuffixes`, `RemoveInstanceSuffixes`, `FixInvalidFileNameCharacters`, `FixInvalidPathCharacters`, `IsReservedName`, `GetUniqueName`, `MaxFileNameLength`); `Utf8Truncation.cs`; `ExportCollection.cs` (`GetUniqueFileName`) |
| Input evidence | Probe `Temp/opencode/filenameprobe` (SDK 10.0.401) drove the shipped `AssetRipper.IO.Files.dll`, including `LocalFileSystem.GetUniqueName` against real temporary directories; 86 rows committed as `src/io/file_names_fixture.txt` (pure ASCII via probe `\u` escapes after a cp437 pipeline mojibake was found and fixed). |
| Source-derived behavior | Host-dependent invalid sets and the Windows-only reserved gate (mirrored, not normalized); the `.`/bare-dot extension asymmetry (`a.` stems to `a` with no extension); truncation backs off split sequences; the remembered counter only skips checked-taken names. |
| Root cause | The first Rust port used one symmetric splitter (wrong on `a.`/`.`) and naive pipe splitting (wrong on pipe-bearing names); both fixed and regression-covered. |
| Rust change | `io::file_names::{UniqueNamePlanner, unique_export_file_name, ExportFileNameFacts, ...}` over caller-supplied existence facts. |
| Regression evidence | Five fixture-driven tests match all 86 probed rows plus the composition steps; OS-dependent rows degrade exactly like the source on non-Windows hosts. |
| Remaining comparison | Writing the asset bytes and the `{file}.meta` sidecar stays with the export pipeline; the add-only sidecar refusal (vs source overwrite) is the documented deviation. |

## DIFF-PROJECT-PATH-HOST-001 - preserve host path semantics through staged project output

| Field | Finding |
|---|---|
| Source behavior | `AssetExportCollection.Export` fixes the best directory, joins it below the project root with `FileSystem.Path.Join`, plans a unique name against the destination directory, then writes the asset and `{file}.meta`. `FileSystem.FixInvalidPathCharacters` removes the host's directory separators from the invalid-character set before applying its path sanitizer. The source's `System.IO.Path` recognizes the backslash character as a Windows separator; POSIX uses forward slash only, so a POSIX backslash remains part of one path component. File existence and case collision behavior follow the host filesystem. |
| Source evidence | A fresh .NET SDK 10.0.401 run of `tools/probes/local-filesystem/LocalFileSystemProbe.csproj` against the isolated AssetRipper `AssetRipper.IO.Files.dll` exited 0. Its 10 `PATH_HELPERS` rows matched `src/io/local_filesystem_fixture.txt`; Windows outputs include a backslash-separated join, `filename=beta.ext`, and a backslash-separated relative path. The same fixture's 86-row filename probe covers `FixInvalidPathCharacters`, reserved names, truncation, extension handling, and unique-name state. |
| Rust change | `asset_path` and `collection_paths` normalize backslashes only for Windows relative-path serialization. `ProjectExportPlan` validates with host separator and collision rules. `project_output` splits and writes using host separators and applies Windows-only filename restrictions on Windows. |
| Regression evidence | `path_planning_keeps_posix_backslashes_as_literal_characters`, `planned_file_collision_checks_follow_host_case_sensitivity`, `output_path_components_follow_host_backslash_semantics`, and `project_asset_and_meta_writers_follow_host_backslash_semantics` pass. The last test writes both asset bytes and the sidecar to the host-resolved path. Full Windows validation passed 1,572 library tests, 112 CLI tests, one ignored doctest, warnings-denied Clippy, rustfmt, `git diff --check`, and the 1,449-file baseline verifier. |
| Remaining difference | POSIX source-runtime execution is still missing. WSL is not installed; live GitHub Actions run `37951663657` failed all five jobs before starting any steps (zero steps), so it supplied no Linux test evidence. The Windows collision key currently uses Rust `to_lowercase`; equivalence to every Windows filesystem comparison mode is not established. `EXP-FILE-NAME-001` remains implemented but unverified; this finding closes no broad feature row. Phase 1 remains OPEN. |

## DIFF-MONO-UTILS-001 - managed type classification and engine-asset names

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; behavior matches on all probed rows |
| Source location | `AssetRipper.SerializationLogic/MonoUtils.cs`; `AssetRipper.Import.AssetCreation/GameAssetFactory.cs` (`CreateEngineAsset`) |
| Input evidence | Probe `Temp/opencode/monoutilsprobe` (SDK 10.0.401) drove the shipped assemblies: 469 predicate cells, 268 `ToPrimitiveType` cells, 17 `PrimitiveType` discriminants, and 23 `CreateEngineAsset` outcomes at 2019.4.0f1; rows committed as `src/processing/mono_utils_fixture.txt`. |
| Source-derived behavior | Namespace-gated tables with exact const spellings; only long `System` names map to primitives (aliases fall to `Complex`); 21 engine names resolve to versioned subclasses (6 version-taking), anything else throws `NotSupportedException`. |
| Root cause | No deviation found; this increment ports previously uncovered classification behavior. |
| Rust change | `processing::mono_utils::{predicates, PrimitiveType, to_primitive_type, EngineAssetKind, engine_asset_kind}`. |
| Regression evidence | Three fixture-driven tests match all 777 probed rows. |
| Remaining comparison | Assembly loading, type resolution, and object construction need live managed assemblies and stay unported. |

## DIFF-ASSET-FACTORY-001 - asset-creation dispatch and engine-asset names

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded for the engine table; dispatch rules source-read and directly asserted |
| Source location | `AssetRipper.Import.AssetCreation/GameAssetFactory.cs` (`ReadAsset`, `ReadNormalObject`, `TryReadNormalObject`, `CreateAsset`, `CreateEngineAsset`) |
| Input evidence | The 23 `ENGINE` rows of `src/processing/mono_utils_fixture.txt` (probe `Temp/opencode/monoutilsprobe` at 2019.4.0f1, SDK 10.0.401) are re-asserted through `engine_asset_kind`; the version gate, fallback chain, tail rules, and bump arithmetic follow the inspected dispatch exactly. |
| Source-derived behavior | Pre-3.5 versions are unreadable before any read; MonoBehaviour takes the script path; engine collections tolerate failures (info for shaders); Patch versions retry once with a wrapping build bump; exact/zero-pad reads pass while the 24-byte Texture2D tail takes the Chinese-fields path; generated classes win over TPK trees over unknown objects. |
| Root cause | No deviation found; this increment ports previously uncovered factory behavior. |
| Rust change | `processing::asset_factory::{classify_read_path, classify_object_tail, decide_normal_object, bump_patch_build, classify_created_asset, engine_asset_kind}`. |
| Regression evidence | Six tests pin the dispatch precedence, tail branches, fallback order with log levels, bump wraparound, creation order, and all 23 engine arms. |
| Remaining comparison | Byte reading, node building, and assembly-backed MonoBehaviour structures need generated objects and live assemblies and stay unported. |

## DIFF-SCRIPT-GUID-001 - script and assembly GUID byte layout

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; layout ported and pinned row-for-row |
| Source location | `AssetRipper.Export.UnityProjects/Scripts/ScriptHashing.cs` (`CalculateScriptGuid`, `CalculateAssemblyGuid`) via `AssetRipper.Primitives/UnityGuid.cs` (`Md5Hash`, `ConvertSystemOrUnityBytes`, `ToString`) |
| Input evidence | 16 committed rows in `src/export/script_guid_fixture.txt` (probe `Temp/opencode/guidprobe`, shipped `AssetRipper.Export.UnityProjects.dll` plus `AssetRipper.Primitives.dll`, SDK 10.0.401): 4 `GUID3` string rows, 4 `GUID1` string rows (including the `MyLib.dll` extension-strip proof), and 8 `WORDS` word rows. |
| Source-derived behavior | `Md5Hash` MD5s the concatenated UTF-8 bytes; `ConvertSystemOrUnityBytes` reverses the first four bytes, swaps each of the next two pairs, leaves the tail, then swaps nibbles per byte; words are the converted buffer read little-endian; `ToString` renders each word least-significant nibble first. A prior hand-derived "nibble swap only" description was wrong about the field flips; the machine-checked `MATCH concat` rows plus the upstream source settle it. |
| Root cause | No deviation found; this increment ports the previously open GUID companions of the FileID hash. |
| Rust change | `export::script_hashing::{UnityGuid, unity_guid_md5_hash, unity_guid_md5_hash_three, calculate_script_guid, calculate_script_guid_str, calculate_assembly_guid, remove_assembly_file_extension, fix_assembly_name}` plus a from-scratch RFC 1321 MD5; `export::guid_policy::byte_layout_in_snapshot` now reports the deterministic sources closed. |
| Regression evidence | The RFC 1321 section A.5 suite pins MD5; the fixture test asserts all 4 script strings, all 4 assembly strings, and all 8 word rows, plus `str`/bytes equivalence, `MissingReference`/zero formatting, and GUID input bounds. |
| Remaining comparison | The `IMonoScript` overloads (fixed assembly names, namespace/class bytes) need live generated script objects and stay unported. |

## DIFF-SETTINGS-FILE-001 - settings-file JSON shape

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; shape ported and pinned byte-for-byte |
| Source location | `AssetRipper.Export/Configuration/SerializedSettings.cs` (`Save`, `DefaultFileName`); `SerializedSettingsContext.cs` (`WriteIndented`); `AssetRipper.Import/Configuration/UnityVersionJsonConverter.cs` |
| Input evidence | Two committed probe outputs (`src/export/settings_file_defaults.json`, `src/export/settings_file_modified.json`; probe `Temp/opencode/settingprobe2`, shipped `AssetRipper.Export.dll`, SDK 10.0.401): fresh-record defaults plus an all-fields-non-default record. Byte-level facts verified outside the text reader: no BOM, CRLF newlines, no trailing newline. |
| Source-derived behavior | Root `Import`/`Processing`/`Export` in declaration order; enums as numbers (including `-1`); versions as `ToString()` renderings (`0.0.0a0` default); `LanguageCode` string-or-null; `IgnoreStreamingAssets` absent via `[JsonIgnore]`; two-space indentation. |
| Root cause | No deviation found; this increment ports the previously open persistence round-trip. |
| Rust change | `export::settings_file::{write_settings_file, parse_settings_file, SerializedSettingsFile, VersionText}` plus a schema-scoped JSON reader; `from_discriminant` const maps for all eleven file enums in `export::settings`. |
| Regression evidence | Both writer outputs asserted byte-for-byte against CRLF literals; both fixtures round-trip; tolerance (unknown/missing/duplicate/escapes/surrogates) and rejection (paths, bounds) tests pin the load behavior. |
| Remaining comparison | Exe-adjacent default paths, the save/delete gate, and startup application need product wiring and stay unported. |

## DIFF-GLB-ENCODING-001 - GLB accessor encodings and skin-lane rules

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; encodings ported and pinned |
| Source location | `AssetRipper.Export.Modules.Models` (`GlbSubMeshBuilder`, `MeshDataExtensions`, `VertexColor1Texture8`) via SharpGLTF 1.0.2 (`VertexJoints4`, `SparseWeight8`, `IndexWeight`) |
| Input evidence | Probe `Temp/opencode/gltfprobe` (shipped `SharpGLTF.Core/Runtime/Toolkit.dll` 1.0.2, SDK 10.0.401) dumping accessor tables and buffer lanes: `POSITION/NORMAL/TANGENT/TEXCOORD 5126`, `COLOR_0 5121 normalized`, `JOINTS_0 5121` (all joints ≤ 255) vs whole-accessor `5123` with a 300 joint, `WEIGHTS_0 5126`, small-mesh indices `5123`, 65535-vertex indices `5123`, 65538-vertex indices `5125`; color lanes `FF0000FF,00FF007F,0000FF3F` (0.5→127 truncation); skin lanes proving dupe-sum, zero-drop, renormalization (0.3→0.5), and an `ArgumentOutOfRangeException` on all-zero weights. Upstream sources read for the exact rules: `VertexSkinning.cs` (`VertexJoints4` ctor, `GetEncodingAttributes`), `SparseWeight8.cs` (`Create`, `OrderedByWeight`, sum-0/1 renormalization skip), `IndexWeight.cs` (`InsertUnsorted` merge, `IsGreaterThan`, `BubbleSortByWeight`). |
| Source-derived behavior | Colors quantize per channel with truncation, but any non-finite channel zeroes the whole color (probed with infinite/NaN lanes). Skin: negatives are rejected by the port (unrepresentable); zero-weight lanes drop; duplicate indices merge by summation; sums scale to one except exact 0/1; lanes sort by magnitude desc with smaller indices on ties; missing lanes pad joint zero; joints wider than a byte widen the whole accessor; indices wider than 65534 widen the whole accessor (65535 is reserved). |
| Root cause | Three deviations found and fixed: `COLOR_0` was FLOAT (now normalized UBYTE), `JOINTS_0` was always USHORT (now value-width), indices were always UINT (now width-switched). Two prior test expectations encoded the old assumptions and were updated with the differential as authority. |
| Rust change | `resources::glb::{quantize_color, quantize_color_channel, process_skin_bindings, push_color_accessor, push_ubyte_view}`; width-switching `push_joints_accessor`/`push_index_accessor`; new `GlbError::BoneIndexTooLarge`. |
| Regression evidence | New tests pin truncation (incl. 0.999→254), vector poisoning, merge/drop/renormalize/pad rules, both error variants, and all four accessor width switches; full lib suite passes 965/965 locally. |
| Remaining comparison | `GlbLevelBuilder` hierarchy export and `GlbTerrainBuilder` have no CLI path and stay unported; morph targets are absent on both sides (confirmed in `GlbSubMeshBuilder`). |

## DIFF-HDR-001 - Radiance HDR writer rules

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; writer ported and pinned byte-for-byte |
| Source location | StbImageWriteSharp 1.16.7 `ImageWriter.WriteHdr` (shipped `StbImageWriteSharp.dll`), called by `AssetRipper.Export.Modules.Textures/DirectBitmap\`1.cs` (`SaveAsHdr`) with `Width`, `Height * Depth`, and the `GetDataAndComponentsForSaving` bytes |
| Input evidence | Probe `Temp/opencode/hdrprobe` (SDK 10.0.401, RGBA input): 22 committed `VEC` rows in `src/resources/hdr_fixture.txt` plus two over-size boundary facts (32767-wide solid is RLE at 2191 bytes with marker `02 02 7F FF`; 32768-wide solid is flat at 131187 bytes with an `C8643280` tail) |
| Source-derived behavior | Fixed 101-byte header (`#?RADIANCE`, `# Written by stb_image_write.h`, `FORMAT=32-bit_rle_rgbe`, `EXPOSURE=` with 10 spaces plus `1.0000000000000`, blank line) then `-Y {h} +X {w}`; bytes scale by `1/255` into a shared-exponent RGBE pixel (zero maps to all-zero, alpha ignored); scanlines encode independently with new-style RLE iff `8 <= width < 0x8000`, else flat; a match of 3+ encodes as 127-capped runs where an already-classified run emits even a 1-2 tail chunk, while shorter matches accumulate 128-capped literals that stop at the next run |
| Root cause | No deviation found; this increment ports the previously open HDR writer. The run-tail rule (e.g. a 128-run emits `R127 R1`, not `R127` plus a 3-literal) and the `0x8000` flat boundary were established purely from probe bytes, not from library sources |
| Rust change | `resources::image_export::{save_hdr, rgb8_to_rgbe, encode_hdr_scanline_rle}` with integer-`frexp` conversion and bounded chunk emission; `save()` dispatches `Hdr` to the new writer |
| Regression evidence | All 22 fixture vectors reproduce byte-for-byte (including all 256 gray conversions, every probed run length 1-130 plus 300, the 300-literal chunking, the mixed literal/run row, and the two-row marker repetition); targeted tests pin both width boundaries and alpha indifference; full lib suite passes 1003/1003 locally with warnings-denied Clippy |
| Remaining comparison | EXR (`ExrWriter`) and JPEG (stb `WriteJpg`) stay `Unsupported`; the zero-width degenerate case writes header only by construction and was not probed |

## DIFF-TEXTURE-ARRAY-001 - array/3D texture selection and size rules

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; views and rules ported and pinned |
| Source location | `AssetRipper.SourceGenerated.Extensions/Texture2DArrayExtensions.cs`, `Texture3DExtensions.cs`, `CubemapArrayExtensions.cs`; `AssetRipper.Export.Modules.Textures/TextureConverter.cs` (the three `GraphicsFormat` overloads) |
| Input evidence | Probe `Temp/opencode/arrayprobe` driving live 2022.3 assets over versioned `ProcessedAssetCollection`s (SDK 10.0.401): 6 committed rows in `src/resources/texture_array_fixture.txt`, plus `Has_*` gate outcomes on 5.6/2019.4/2022.3 and the newest-class property dumps |
| Source-derived behavior | Inline bytes win with no swap or unswizzle on any of the three classes; `Texture2DArray` size divides by depth with depths below two keeping the whole size; cubemap height equals width and depth is `CubemapCount * 6` (kept verbatim with the source `Not sure about this`); cubemap size is zero with no faces; the two `Texture3D` unions share one field each (setting `Format_UInt32` reads back through `Format`); the lightmap gate is off everywhere probed so the usage mode backs it; missing mip counts derive `round(log2(maxSide))`; unconvertible graphics formats fall back to `RGBA32` |
| Root cause | No deviation found; this increment ports the previously open array/3D field layer. The probe corrected one design assumption (separate `Texture3D` format spellings are one shared field, so the view keeps both read spellings but one value) |
| Rust change | `resources::texture_array::{Texture3DView, Texture2DArrayView, CubemapArrayView, resolve_array_image_data, texture2d_array_complete_image_size, cubemap_array_height/depth/complete_image_size, Texture3DView::{format_value, texture_format_or_default, lightmap_value, complete_image_size_value, mip_count_value}}` with `wrapping_div` matching the source `(int)` casts and an explicit error for the unreachable face-count overflow |
| Regression evidence | All 6 fixture rows reproduce byte-for-byte with their derived sizes; union precedence, rounding fallback (including the zero-side refusal where the source conversion throws), size branches, overflow refusal, and view readers pinned; full lib suite passes 1018/1018 locally with warnings-denied Clippy |
| Remaining comparison | Bitmap export wiring and CLI classes 117/187/188 stay open |

## DIFF-TEXTURE-GETIMAGEDATA-001 - GetImageData platform orchestration

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; orchestration ported and pinned |
| Source location | `AssetRipper.SourceGenerated.Extensions/Texture2DExtensions.cs` (`GetImageData` lines 79-113, `IsSwapBytes` lines 135-152); `AssetRipper.IO.Files/BuildTarget.cs` (Xbox360 = 11, Switch = 38, Switch2 = 48) |
| Input evidence | Probe `Temp/opencode/texplatprobe` driving live 2022.3 `Texture2D` assets over versioned `ProcessedAssetCollection`s (SDK 10.0.401): 12 committed rows in `src/resources/texture_platform_fixture.txt`. Supporting facts from the same probe: `Has_PlatformBlob_C28` is false on 2020.1 and older, true on 2020.2+; `m_PlatformBlob` confirmed as the single UTF-16 serialized-name literal in the shipped `AssetRipper.SourceGenerated.dll` |
| Source-derived behavior | Inline bytes win and are copied before a swap (input never mutated, output is a new array); non-swap formats return the input array by reference; streamed bytes swap in place on the fresh buffer; the unswizzle gate is blob length >= 12 AND exact `Switch` equality (Switch2 passes through); odd swap counts throw `IndexOutOfRangeException`; empty input returns empty |
| Root cause | Two probe-design corrections, no source deviations: RGBA32/RGB24 are not in the swap list (first probe vectors were no-op passthroughs by mistake), and pre-2020.2 assets have no blob store (the setter silently drops, so Switch vectors need 2020.2+ assets). The Rust port maps the odd-count throw to `SwapBytesError::OddByteCount` and borrows instead of aliasing (observably identical) |
| Rust change | `resources::texture::{Texture2DView::platform_blob, resolved_image_data}` with `Cow` aliasing; `SWITCH_BUILD_TARGET`/`SWITCH2_BUILD_TARGET` consts; `TextureExportError::Swap`/`Unswizzle`; CLI threads `metadata.prefix.target_platform`/`body_endian` through `export_texture2d_image`/`export_texture2d_value` |
| Regression evidence | All 12 fixture rows reproduce byte-for-byte, including both Switch vectors through the full orchestration; view-immutability, borrow-vs-own, streamed-swap, and blob-spelling tests; full lib suite passes 1008/1008 locally with warnings-denied Clippy |
| Remaining comparison | Streamed fetching stays caller-side; unknown platforms (discriminant 0) transform nothing by construction |

## DIFF-TYPETREE-READER-001 - TypeTree old/new reader parity

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; both rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/TypeTrees/TypeTreeNode.cs (IsFormat5, HasRefTypeHash, Read, Write); TypeTree.cs (Read, SetNamesFromBuffer, GetTypeName, ReadTreeNode); CommonString.cs (113-entry StringBuffer) |
| Input evidence | Probe Temp/opencode/typetreeprobe (shipped AssetRipper.IO.Files.dll plus AssetRipper.IO.Endian.dll, SDK 10.0.401) dumping 21 generation predicates, all 113 common entries, plus C#-constructed nested payloads: legacy BE gen8 and LE gen11 (GameObject/int/Vector3f/float, 4 nodes, levels 0,1,1,2); modern LE gen10 (custom MyCustomType plus common int, no hash), BE gen19 (same with hashes), LE gen22 (common Vector3f/float with hashes); negative-count refusal. Committed as src/serialized_file/typetree_reader_fixture.txt (156 lines). |
| Source-derived behavior | Legacy form is null-terminated type/name plus size/index/flags/version/meta plus child count with depth-assigned levels; modern form is count plus buffer size plus fixed 24-byte nodes (19+ adds 8-byte hash for 32 bytes) plus string buffer with custom offsets versus 0x80000000-masked common offsets; negative counts throw InvalidDataException. |
| Root cause | No deviation found; this increment ports the previously open nested and full-table coverage. Rust names IsFormat5 as is_format5 and HasRefTypeHash as has_ref_type_hash and wires all three call sites behavior-identically. |
| Rust change | src/serialized_file/metadata.rs (is_format5, has_ref_type_hash, read_type_tree, read_modern_type_tree_node, read_legacy_type_tree_node) plus test typetree_reader_matches_reference_runtime; fixture src/serialized_file/typetree_reader_fixture.txt. |
| Regression evidence | All 21 predicates, all 113 common offsets plus gap refusals, all 5 payloads (nodes, buffer bytes, offsets, levels, hashes, trailing checks) and the fail-loud negative-count case reproduce the reference dumps; full lib suite 1351 plus 60 CLI pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced SerializedFile TypeTrees and managed-reference payload decoding stay outside these two reader rows (tracked by SER-TT-ABSENT-001, SER-MANAGED-REFS-001 and related object rows). |

## DIFF-BUNDLE-HEADER-001 - UnityFS header fields/alignment plus Archive unsupported parity

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; both rows ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/BundleHeader.cs (IsBundleHeader 0x20 minimum); BundleVersion.cs (BF_LargeFilesSupport 7); FileStreamBundleHeader.cs plus FileStreamBundleFile.cs (header fields, v7+ 16-byte alignment); Archive/ArchiveBundleHeader.cs plus ArchiveBundle.cs plus ArchiveBundleScheme.cs (detection plus NotSupported Read/Write) |
| Input evidence | Probe Temp/opencode/bundleheaderprobe (shipped IO.Files plus IO.Endian plus Primitives DLLs, SDK 10.0.401) dumping the BundleVersion enum, Archive/UnityFS detection vectors (full/short/other-magic/empty) with total-length semantics, Archive Read/Write exception types, plus reference-written minimal empty-table uncompressed combined bundles for v5/v6/v7/v8 with read-back dumps. Committed as src/container/bundle_header_fixture.txt (27 lines). |
| Source-derived behavior | Bundle detection requires total stream length 0x20 or more; Archive detection is UnityArchive magic while its Read/Write always throw NotSupportedException; UnityFS header carries signature, version, two engine strings, Size, Comp, Uncomp and Flags with no per-version field branch beyond the v7+ post-header 16-byte alignment; v5 parses with shorter engine strings; v8 behaves like v7. |
| Root cause | No deviation found; this increment ports the previously open older-version and reference-generated coverage. Rust keeps the 0x20 total-length rule, the ArchiveReadUnsupported fail-loud error, the BUNDLE_VERSION_LARGE_FILES_SUPPORT 7 constant and the v7+ alignment. |
| Rust change | src/container/unityfs.rs test bundle_headers_match_reference_generated_bundles; src/scheme.rs test archive_detection_and_unsupported_read_match_reference; fixture src/container/bundle_header_fixture.txt. |
| Regression evidence | All detection vectors, both Archive throw types, all four reference bundles (fields, Total/HeadLen/Pad, zero blocks/nodes/hash, metadata/data offsets, v7+ alignment, v5 older proof) reproduce; full lib suite 1353 plus 60 CLI pass with warnings-denied Clippy. |
| Remaining comparison | UnityFS metadata compression and directory/entry lanes stay with IO-BUNDLE-UNITYFS-META-001 and IO-BUNDLE-UNITYFS-ENTRY-001; no Unity-produced bundle file has been compared. |

## DIFF-FILE-HEADER-001 - SerializedFile header generation branches

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; row ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedFileHeader.cs (Read, Write, HasEndianess, HasLargeFilesSupport, IsSerializedFileHeader); FormatVersion.cs |
| Input evidence | Probe Temp/opencode/fileheaderprobe (shipped IO.Files plus IO.Endian DLLs, SDK 10.0.401) dumping the FormatVersion enum, 21 generation predicates, plus reference-written headers for gens 5,8,9,21,22 with read-back dumps. Committed as src/serialized_file/file_header_fixture.txt (56 lines). |
| Source-derived behavior | Pre-9 headers are 16-byte legacy with no endian marker; 9+ adds the marker plus 4-byte alignment; 22+ writes zeros at legacy positions and real values at the 0x14 extended block; MetadataSize 0 or below throws; undefined generations throw. |
| Root cause | No deviation found; this increment ports the previously open pre-9 plus reference-generated coverage. Rust keeps the same branches with stricter fail-closed bounds (documented hardening over Debug-only asserts). |
| Rust change | src/serialized_file/header.rs test headers_match_reference_written_fixtures; fixture src/serialized_file/file_header_fixture.txt. |
| Regression evidence | All 21 predicates, all five reference headers (version, sizes, offsets, marker, reserved, header size) reproduce; full suite 1355 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Full SerializedFile metadata/object integration and container extraction stay with SER-META and SER-FILE-ENTRY rows; no Unity-produced file compared. |

## DIFF-GZIP-001 - GZip wrapper decode parity with .NET

| Field | Finding |
|---|---|
| Status | .NET interop differential recorded; decode direction pinned |
| Source location | AssetRipper.IO.Files/CompressedFiles/GZip/GZipFile.cs; GZipFileScheme.CanRead |
| Input evidence | Probe Temp/opencode/gzipprobe (System.IO.Compression.GZipStream Optimal, SDK 10.0.401) emitting hello/sequence/concat vectors plus .NET round-trip proof; empty is degenerate with zero bytes. Committed as src/compression/gzip_dotnet_fixture.txt (7 lines, independently encoded). |
| Source-derived behavior | Any valid GZip member with 1F 8B is accepted; concatenated members decode as one stream; encoder settings affect bytes but not validity. |
| Root cause | No deviation found in decode; encoder bytes intentionally differ by settings (documented valid-member difference, round-trip proven on both sides for the decode direction). |
| Rust change | src/compression/wrappers.rs test dotnet_encoded_members_decode_to_the_same_bytes; fixture src/compression/gzip_dotnet_fixture.txt. |
| Regression evidence | All three .NET vectors plus concat reproduce byte-for-byte through Rust decompress_gzip with magic checks; full suite 1355 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Real wrapped Unity file comparison and recursive dispatch over user content stay open; encoder byte-identity is explicitly not owed. |

## DIFF-LZMA-001 - LZMA unity and size-prefix lanes

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; both rows ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/LzmaCompression.cs (DecompressLzmaStream, DecompressLzmaSizeStream, CompressLzmaStream, CompressLzmaSizeStream); FileStreamBundleFile.cs (LZMA metadata write throws NotImplementedException) |
| Input evidence | Probe Temp/opencode/lzmaprobe (shipped IO.Files DLL, SDK 10.0.401) decoding existing SharpCompress vectors through the reference entry points, generating new unity-gen/size-gen vectors via the reference Compress methods, proving the write refusal and the -1 silent-empty. Committed as src/compression/lzma_reference_fixture.txt (13 lines). |
| Source-derived behavior | Unity lane is five property bytes plus raw stream; size lane adds an eight-byte LE decoded length; short streams are left short by the source loop for downstream rejection while Rust fails closed; long streams truncate to the requested count on both sides; LZMA metadata cannot be written by the baseline. |
| Root cause | No deviation found in decode; short/negative hardening and the missing write unit are documented deliberate dispositions (fail-closed, no success, no silent drop). |
| Rust change | src/compression/lzma.rs test reference_runtime_outputs_match_the_rust_decoder; fixture src/compression/lzma_reference_fixture.txt. |
| Regression evidence | All reference outputs match Rust byte-for-byte; write refusal and negative fail-closed pinned; full suite 1356 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | UnityFS metadata/entry integration over LZMA and real UnityWeb file comparison stay open as noted; encoder byte-identity is not owed. |

## DIFF-UNITYFS-META-001 - UnityFS metadata all lanes

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; row ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/FileStream/FileStreamBundleFile.cs (ReadFileStreamMetadata, ReadMetadata); BlocksInfo.cs; DirectoryInfo.cs; StorageBlock.cs; FileStreamNode.cs |
| Input evidence | Probe Temp/opencode/unityfsmetaprobe (shipped IO.Files DLL, SDK 10.0.401) reading eight C#-constructed full bundles with 1-block/1-node tables (None/LZ4/LZMA, combined/separate, start/end, v6/v7) plus LZHAM and truncated refusals. Committed as src/container/unityfs_meta_fixture.txt (20 lines). |
| Source-derived behavior | Metadata compression None/LZ4/LZMA decode via the same payload path; LZHAM throws UnsupportedBundleDecompression before reading; placement at end seeks to Size minus compressed length with v7 16-byte alignment; separate directories leave nodes empty; truncated metadata throws loudly. |
| Root cause | No deviation found; this increment ports the previously synthetic-only lanes. LZHAM is unsupported-by-the-baseline (proven refusal). Truncated input fails loud on both sides with different messages (no silent success). |
| Rust change | src/container/unityfs.rs test metadata_tables_match_reference_constructed_bundles; fixture src/container/unityfs_meta_fixture.txt. |
| Regression evidence | All six decodable lanes reproduce blocks/nodes/header/offsets/table values; LZHAM and truncated pinned fail-loud; full suite 1357 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Entry extraction over storage blocks (Zstd Huffman/FSE, non-combined variants) stays with IO-BUNDLE-UNITYFS-ENTRY-001; real-file comparison stays open as noted. |

## DIFF-UNITYFS-ENTRY-001 - UnityFS entry all lanes

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; row ported and pinned |
| Source location | AssetRipper.IO.Files/BundleFiles/BundleFileBlockReader.cs (block dispatch with ZstdCompression.IsZstd fallback); FileStreamBundleFile.cs (ReadFileStreamData with AddFailedFile); ZstdCompression.cs; ResourceFile.cs |
| Input evidence | Probe Temp/opencode/unityfsentryprobe (shipped IO.Files DLL, SDK 10.0.401) reading seven C#-constructed bundles with entry payloads across None/LZ4/LZMA/Zstd-Huffman-FSE blocks, two-block reassembly, non-combined empty, and LZHAM refusal. Committed as src/container/unityfs_entry_fixture.txt (18 lines). |
| Source-derived behavior | Entry bytes reassemble across storage blocks with per-block compression; Zstd blocks decode by frame-signature fallback; non-combined directories yield zero entries; LZHAM entries record FailedFile loudly instead of succeeding. |
| Root cause | No deviation found; this increment ports the previously synthetic-only lanes including Zstd Huffman/FSE (previously RFC-built only). LZHAM is unsupported-by-the-baseline with loud proof on both sides. |
| Rust change | src/container/unityfs.rs test entry_bytes_match_reference_constructed_bundles; fixture src/container/unityfs_entry_fixture.txt. |
| Regression evidence | All five decodable entries reproduce byte-for-byte; non-combined empty and LZHAM loud pinned; full suite 1358 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Real-file comparison stays open as noted; the UnityFS header/meta/entry vertical is now fully reference-differentially covered. |

## DIFF-SER-SCALAR-001 - SerializedFile metadata scalar fields

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedFileMetadata.cs (HasSignature, HasPlatform, HasLongFileID, HasUserInformation, stripped-version fallback); SerializedFileHeader.cs (pre-9 metadata-at-end) |
| Input evidence | Probe Temp/opencode/sermetascalarprobe (shipped IO.Files DLL, SDK 10.0.401) reading three C#-constructed files with empty tables across gen8 parseable/unparseable and gen14. Committed as src/serialized_file/metadata_scalar_fixture.txt (10 lines). |
| Source-derived behavior | Version text gates the layout branch with stripped fallback; LongFileID exists only for generations 7-13; user information is a trailing zero-term string; pre-9 metadata sits at the end with DataOffset 0 while gen14 data follows metadata. |
| Root cause | No deviation found except representation: unparseable versions normalize to 0.0.0a0 on the reference while Rust preserves the raw text (both signal stripped, documented). |
| Rust change | src/serialized_file/metadata.rs test scalar_fields_match_reference_constructed_files; fixture src/serialized_file/metadata_scalar_fixture.txt. |
| Regression evidence | All three files reproduce version/platform/LongFileID/user/tables on both sides; bounds for both layouts pinned; full suite 1359 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Type/object/external/script tables plus TypeTree payloads stay with SER-META-TYPES/OBJECTS/EXTERNALS rows; full UnityVersion grammar stays out by design. |

## DIFF-SER-TABLE-001 - SerializedFile object/external/script tables

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/ObjectInfo.cs (IsLongID, HasClassID, HasScriptTypeIndex, HasIsDestroyed, HasSerializedTypeIndex); FileIdentifier.cs; LocalSerializedObjectIdentifier.cs |
| Input evidence | Probe Temp/opencode/sermetatableprobe (shipped IO.Files DLL, SDK 10.0.401) reading three C#-constructed files with one object, one external, and zero/one script records across gen8/12/20. Committed as src/serialized_file/metadata_table_fixture.txt (10 lines). |
| Source-derived behavior | PathID is 32-bit before generation 14 and 64-bit with 4-byte alignment after; pre-17 files carry raw type IDs with ClassID/ScriptTypeIndex/IsDestroyed branches while gen20 carries a type-table index; script identifiers mirror the PathID width; externals carry asset path, GUID, type, and origin. |
| Root cause | No deviation found; this increment ports the previously synthetic-only table reads. Zero types keep type matching out of scope by construction on both sides. |
| Rust change | src/serialized_file/metadata.rs test table_records_match_reference_constructed_files; fixture src/serialized_file/metadata_table_fixture.txt. |
| Regression evidence | All records reproduce FileID/TypeID/ClassID/scripts/externals on both sides; full suite 1360 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Type records with TypeTree payloads stay with SER-META-TYPES-001; identifier-to-object integration and script recovery stay downstream. |

## DIFF-LEGACY-001 - Legacy Raw/Web/WebData containers

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned; one real Rust bug fixed |
| Source location | AssetRipper.IO.Files/BundleFiles/BundleHeader.cs; RawWeb/RawWebBundleHeader.cs (HasHash/HasCompleteFileSize/HasUncompressedBlocksInfoSize, Write throws); RawWeb/RawWebBundleFile.cs (header-size check, last-chunk LZMA, metadata-size check); WebFiles/WebFile.cs; WebFiles/WebFileEntry.cs (Offset/Size plus four-byte LE name) |
| Input evidence | Probe Temp/opencode/legacybundleprobe (shipped IO.Files DLL, SDK 10.0.401) over C#-constructed Raw/Web bundles plus reference-written WebFiles with align on/off and a 200-char name, plus corrupt-header and bad-offset refusals. Committed as src/container/legacy_bundle_fixture.txt (30 lines). |
| Source-derived behavior | Version gates 1-8 select hash/CRC, complete size, and uncompressed size with 4-byte alignment; Raw reads post-header bytes while Web decompresses the last LZMA size-prefixed scene; entry names are four-byte LE (not 7-bit); corrupt sizes and bad ranges fail loud. |
| Root cause | One real deviation found and fixed: Rust used 7-bit name lengths while the reference uses four-byte LE (bytes proven from reference-written files). All other lanes match; malformed inputs fail loud on both sides with different messages (no silent success). Raw/Web Write is unsupported-by-the-baseline (proven refusal). |
| Rust change | src/container/raw_web.rs test headers_and_entries_match_reference_constructed_bundles; src/container/web_file.rs test directory_and_entries_match_reference_written_files plus the 7-bit to four-byte fix across read/write/tests/fixtures; fixture src/container/legacy_bundle_fixture.txt. |
| Regression evidence | All version gates, both entry lanes, both WebFile alignments, and both refusals reproduce; the name-codec fix is regression-covered by D1/D2; full suite 1362 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Real-file comparison stays open as noted; legacy container lanes are now fully reference-differentially covered. |

## DIFF-STREAM-001 - Stream wrappers Smart/MultiFile/Partial/RandomAccess

| Field | Finding |
|---|---|
| Status | Source-runtime wrapper differences reconciled; wrapper regressions pass |
| Source location | AssetRipper.IO.Files/Streams/Smart/SmartStream.cs (null fails loud); MultiFileStream.cs (splitN OpenRead); PartialStream.cs (no construction positioning, Seek adds base offset); RandomAccessStream.cs (internal, bounded file range) |
| Input evidence | Frozen reference probes `tools/probes/io-differential` and `tools/probes/multifile` execute lengths, reads, seeks, positions, EOF, and `ReadByte` over memory, temp, null, split parts, ranges, and file ranges. Fresh 2026-10-08 runs produced six `STREAM` rows and 16 MultiFile path rows matching their committed fixtures exactly. |
| Source-derived behavior | `PartialStream` begins at the wrapped cursor, clamps `Read` by `Length - Position`, delegates `ReadByte` without clamping, and maps seek offsets through its base. `RandomAccessStream` and `MultiFileStream` use `Length - offset` for End seeks. Shared range reads leave the parent cursor unchanged; split reads advance part-by-part; null SmartStream operations fail loudly. |
| Root cause | Rust had normalized three source quirks to conventional behavior: first `PartialStream.Read` was repositioned, `ReadByte` was range-bounded, and RandomAccess/MultiFile End offsets were standard-signed. Compatibility required preserving the source operations. |
| Rust change | `src/io/stream.rs` now retains the source parent cursor for PartialStream reads, exposes the source's delegated `read_byte`, and uses source-sign End seeks for `FileRangeStream` and `MultiFileStream`. Unix file ranges use `read_at`; Windows uses `seek_read` and restores the cursor while the shared handle remains locked. |
| Regression evidence | `stream_observables_match_the_reference_runtime_fixture`, `partial_stream_matches_source_cursor_and_bounded_read_behavior`, `partial_stream_read_byte_matches_source_unbounded_delegation_and_borrowed_lifetime`, `partial_and_random_access_views_keep_their_distinct_end_seek_rules`, and `multi_file_stream_seeks_from_end_with_source_offsets`; all 29 `io::stream::tests` passed. |
| Remaining comparison | No difference remains in the tested wrapper operations. The full LocalFileSystem adapter surface and the bounded contiguous-buffer requirement of downstream container parsers remain open in aggregate `IO-STREAM-001`; Unix retained-reader runtime validation remains under the externally blocked directory-import row. |

## DIFF-LOCAL-FS-TEXT-ENCODING-001 — local and virtual filesystem text encodings

| Field | Finding |
|---|---|
| Status | Stock runtime encodings and fallback behavior are source-runtime pinned; aggregate `IO-STREAM-001` remains open |
| Source location | `Source/AssetRipper.IO.Files/LocalFileSystem.g.cs` delegates to `System.IO.File.ReadAllText/WriteAllText`; `VirtualFileSystem.cs` calls the supplied `Encoding.GetString/GetBytes`; `FileSystem.g.cs` declares both encoding overloads |
| Input evidence | The isolated `tools/probes/local-filesystem/LocalFileSystemProbe.csproj` ran against the frozen source on .NET SDK 10.0.401. `Encoding.GetEncodings()` reported code pages 1200, 1201, 12000, 12001, 20127, 28591, and 65001; `Encoding.UTF7` separately reported code page 65000. Runtime output is committed as `src/io/text_encoding_fixture.txt` and the malformed UTF-8 subset as `src/io/vfs_text_fixture.txt`. |
| Source-runtime behavior | Local reads detect and strip UTF-8/16/32 BOMs; a matching BOM preserves the supplied encoding fallback, while a BOM that changes encoding uses the detected encoding's standard fallback. Explicit local writes emit the selected encoding preamble and open/truncate before encoding errors. Virtual filesystem reads do not detect BOMs and preserve U+FEFF; writes use `GetBytes` with no preamble and leave existing bytes unchanged when encoding throws. ASCII defaults to `?` for invalid decode and encode; Latin-1 maps each byte directly. The UTF-7 static encoding has its own direct/shift rules and byte fallback. Encoder replacement fallback is applied per UTF-16 code unit, so a non-BMP character makes two ASCII/Latin-1 replacements. |
| Rust change | `TextEncoding` now covers the seven encodings registered by the baseline runtime plus UTF-7. Local and virtual text routes share bounded, fallible UTF codecs while retaining their distinct BOM/preamble and write-failure behavior. `TextEncodingFallbacks` supports source-matched replacement and exception policies. |
| Regression evidence | `local_and_virtual_text_encodings_match_the_source_runtime_fixture`, `utf7_codec_matches_the_source_runtime_fixture`, and `virtual_file_utf8_replacement_matches_source_runtime_corpus` compare the source fixtures, including malformed UTF-8/16/32, UTF-7 sequences, custom fallbacks, preambles, and exception side effects. The source command exited 0; focused Rust regressions pass. Full validation is recorded in `docs/validation-log.md`. |
| Remaining comparison | The Rust enum models the baseline runtime's stock encodings, not arbitrary user-defined `Encoding` subclasses. The frozen product call-site search found only default text overloads. Deferred enumeration error timing, broader stream/container integration, and non-Windows file-sharing validation remain open in `IO-STREAM-001` / `IO-FS-IMPORT-PRESERVE-DIRECTORY-001`. |

## DIFF-SER-TYPE-001 - SerializedFile type and managed-reference records

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; two rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedTypeBase.cs (RefactoredClassId 16, RefactorTypeData 17, HasTypeTreeHashes 13, StoresTypeDependencies 21); SerializedType.cs; SerializedTypeReference.cs (gen21-only name strings); TypeTree.cs (legacy recursive versus modern count-plus-buffer) |
| Input evidence | Probe Temp/opencode/sermetatypeprobe (shipped IO.Files DLL, SDK 10.0.401) reading five C#-constructed files spanning gen8/12/20/21 with one type or one managed ref and zero objects. Committed as src/serialized_file/metadata_type_fixture.txt (15 lines). |
| Source-derived behavior | RawTypeID always; stripped from 16; script index from 17; ScriptID only for -1/114/positive-index scripts with OldTypeHash from 13; TypeTree legacy 1-node versus modern empty; dependencies from 21; managed names only from 21. All reproduce. |
| Root cause | No deviation found; this increment ports the previously synthetic-only type lanes. Hash values pin to construction facts with reference alignment proof. |
| Rust change | src/serialized_file/metadata.rs test type_records_match_reference_constructed_files; fixture src/serialized_file/metadata_type_fixture.txt. |
| Regression evidence | All five records reproduce IDs, flags, indices, hashes, trees, dependencies, and managed names on both sides; full suite 1364 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Type-to-object matching and script recovery stay with their downstream rows; managed field resolution stays downstream. |

## DIFF-COMP-ENC-001 - Independently-encoded Brotli/LZ4/Zstd

| Field | Finding |
|---|---|
| Status | Independent-encoder differential recorded; three rows ported and pinned |
| Source location | AssetRipper.IO.Files/CompressedFiles/Brotli/BrotliFile.cs (BrotliStream decode, Write NotImplemented); BundleFileBlockReader.cs with K4os raw blocks; BundleFiles/ZstdCompression.cs (PartialStream-wrapped DecompressStream, pre-sized output) |
| Input evidence | Probe Temp/opencode/compencodeprobe (SDK 10.0.401) with .NET/K4os/ZstdSharp encoders and reference round-trip oracles. Committed as src/compression/reference_encoded_fixture.txt (25 lines). |
| Source-derived behavior | Brotli text/binary/empty, LZ4 match-heavy and literal blocks, and Zstd entropy-coded frames all decode byte-exact in Rust. |
| Root cause | No deviation found; this increment replaces hand-only evidence with independently encoded vectors (Zstd is the first Huffman/FSE evidence). |
| Rust change | Three per-codec tests in wrappers.rs, lz4_block.rs, zstd.rs; shared fixture src/compression/reference_encoded_fixture.txt. |
| Regression evidence | All seven vectors decode byte-exact on both sides; full suite 1367 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Broad fuzz stays future work; Unity Zstd uses standard framing covered here. |

## DIFF-COMP-WRITE-INTEROP-001 — Rust LZMA, LZ4, and GZip outputs decoded by the source runtime

| Field | Finding |
|---|---|
| Status | Both codec directions are source-runtime verified for the baseline's supported write helpers |
| Source location | `BundleFiles/LzmaCompression.cs`; `CompressedFiles/GZip/GZipFile.cs`; `FileStream/FileStreamBundleFile.cs` and its K4os LZ4 call |
| Input evidence | Rust-authored raw LZMA, size-prefixed LZMA, GZip, and raw LZ4 blocks are frozen in `src/compression/rust_encoder_reference_fixture.txt`. `tools/probes/io-differential/Program.cs` reads those files through the source `LzmaCompression` methods, .NET `GZipStream`, and K4os `LZ4Codec.Decode`. Each source output is byte-identical to the input payload. |
| Source-derived behavior | The baseline offers raw and size-prefixed LZMA compression helpers, GZip file write, and LZ4 block encoding used by the UnityFS metadata writer. Brotli has no writer, Zstandard exposes a decoder only, UnityFS metadata writing rejects LZMA, and non-empty UnityFS entry data writing throws `NotImplementedException`. |
| Rust change | `src/compression/lzma.rs` now exposes bounded raw and size-prefixed LZMA writers. Existing bounded LZ4 and GZip writers are checked against the source decoders. No full UnityFS bundle writer is added: the only product callsite found for `FileStreamBundleFile.Write` creates an empty uncompressed lighting bundle; non-empty entry data is not implemented in the frozen source. |
| Regression evidence | `rust_lzma_encoder_outputs_match_reference_runtime_decoding`, `rust_lz4_encoder_output_decodes_in_k4os_reference_runtime`, and `rust_gzip_encoder_output_decodes_in_the_reference_runtime` compare the product outputs to the frozen fixture and also decode them with the Rust readers. Existing independent source-encoded vectors cover the opposite direction. |
| Remaining comparison | Unity-produced compression outputs and broad fuzz coverage remain unrun. This finding covers codec formats and the source-supported codec write helpers; it does not claim an AssetBundle creation workflow. |

## DIFF-IO-STREAM-INPUT-001 - bounded seekable SchemeReader path input

| Field | Finding |
|---|---|
| Status | Closed for 47 source-probed `LoadFile` lanes, 47 `ReadFile(SmartStream)` lanes, numbered split input, and configured wrapper/input bounds; the known Brotli detector difference is tracked separately |
| Source location | `AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`, `ReadFile`); `AssetRipper.IO.Files/Streams/MultiFileStream.cs` (`OpenReadMulti`) |
| Input evidence | `tools/probes/io-differential/Program.cs` creates synthetic inputs under ignored `target/` and records 47 frozen `SchemeReader.LoadFile` plus 47 `SchemeReader.ReadFile(SmartStream)` outcomes in `src/io/io_differential_stream_fixture.txt`; two fresh isolated runs reproduce the stream rows byte-for-byte. The split-path source observation is recorded in `src/io/multifile_path_fixture.txt`. The standalone SerializedFile row is recorded in src/io/serialized_file_scheme_fixture.txt. No user AssetBundle bytes are involved. |
| Source behavior | `LoadFile` opens a seekable `SmartStream` over a regular file or numbered split parts and passes it through the registered `IScheme` order. `ReadFile(SmartStream)` applies that scheme order to the caller stream; path and direct-stream source outputs match for all 47 captured lanes. |
| Rust behavior | `read_path_streaming` and generic `read_stream` share bounded prefix detection and stream outer GZip/Brotli decoding under per-layer and aggregate limits. VFS, platform-inventory, and dependency dispatch callers use the generic stream route. The final decoded container/leaf remains materialized under `max_input_bytes` because current parsers require contiguous bytes. |
| Regression evidence | `seekable_stream_dispatch_matches_both_source_scheme_reader_routes` compares 47 frozen path and `SmartStream` source rows with Rust stream and in-memory dispatch. `seekable_stream_wrapper_decode_does_not_read_into_a_whole_input_buffer` confirms the compressed source is read in bounded chunks. Existing file-backed recursive-inventory, mount, failure-continuation, split-input, standalone SerializedFile, and wrapper-limit regressions remain covered. |
| Remaining comparison | One difference is intentional and source-confirmed: the baseline `SchemeReader` returns an RFC-valid marked Brotli stream as a raw `ResourceFile`, while Rust recognizes and decodes the marker; `DIFF-BROTLI-METADATA-001` records the detector bug and direct decoder evidence. Final parser layers still materialize bounded contiguous bytes, and non-Windows sharing remains under `IO-FS-IMPORT-PRESERVE-DIRECTORY-001`. No Unity-produced input is claimed. |

## DIFF-SER-ENV-001 - SerializedFile envelopes and file entry

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; two rows ported and pinned |
| Source location | AssetRipper.IO.Files/SerializedFiles/Parser/SerializedFileMetadata.cs (HasEnableTypeTree 13, SupportsRefObject 20, scripts 11); SerializedFileHeader.cs (HasEndianess 9-plus, LargeFilesSupport 22) |
| Input evidence | Probe Temp/opencode/sermetaenvprobe (shipped IO.Files DLL, SDK 10.0.401) reading eleven minimal envelopes plus one gen10 full file. Committed as src/serialized_file/metadata_envelope_fixture.txt (28 lines). |
| Source-derived behavior | All per-generation presence gates reproduce with empty tables; gen10 entry carries identity plus payload bytes. |
| Root cause | No deviation found; one probe-construction correction made (gen22 needs its endian marker plus 48-byte large header). |
| Rust change | src/serialized_file/metadata.rs test metadata_envelopes_match_reference_constructed_files; fixture src/serialized_file/metadata_envelope_fixture.txt. |
| Regression evidence | All twelve files reproduce envelope plus payload on both sides; full suite 1368 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Object field semantics stay with the object-value rows; container extraction stays with its row. |

## DIFF-SER-OBJF-001 - Object field reading (values and arrays)

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; two rows ported and pinned |
| Source location | AssetRipper.Import/Structure/Assembly/Serializable/SerializableStructure.cs; TypeTrees/SerializableTreeType.cs (ToPrimititeType, IsString, AlignBytes); SerializableValue.cs (ReadUtf8StringAligned, per-field Align) |
| Input evidence | Probe Temp/opencode/objfieldprobe (shipped Import DLL, SDK 10.0.401) reading one VALUES file plus one ARRAYS file. Committed as src/serialized_file/object_field_fixture.txt (9 lines). |
| Source-derived behavior | All scalar, string, nested, and array values reproduce; string nodes need Array children; strings self-align; byte vectors align with AlignBytes plus pad. |
| Root cause | Source evidence pins ordinary byte-vector fields as Array<U8>. Using the compact Bytes form for those in-limit arrays changes the decoded value shape. Large direct byte vectors also need bounded compact storage to avoid per-byte enum allocation. |
| Rust change | src/serialized_file/object/codec.rs preserves Array<U8> for arrays within max_array_elements and uses Bytes only for direct char/UInt8 arrays above that element limit; the byte count is still charged to the decoded-allocation budget and checked against remaining payload. The object-field source fixture remains the semantic oracle. |
| Regression evidence | object_scalar_string_nested_fields_match_reference and object_string_nested_empty_arrays_match_reference reproduce source field shapes; compact_u8_vectors_fit_a_byte_sized_decode_budget_and_roundtrip verifies over-limit compact representation and exact payload round-trip. Full workspace suite passed with 1,598 library tests and 124 CLI tests (one library test ignored). |
| Remaining comparison | Managed-reference payloads, enum/generated semantics, fixed buffers, and unusual engine layouts stay with their rows; the full version/flag alignment matrix stays open. |

## DIFF-SER-OBJW-001 - Object encode and file rewrite

| Field | Finding |
|---|---|
| Status | Reference-constructed-file evidence recorded; two rows ported and pinned |
| Source location | AssetRipper.Import/Structure/Assembly/Serializable/SerializableValue.cs; AssetRipper.IO.Files/SerializedFiles/Parser/ObjectInfo.cs (gen22 i64 byteStart); SerializedTypeBase.cs (21-plus post-tree deps); SerializedFileMetadata.cs (pre-9 end-minus-size metadata) |
| Input evidence | Probe Temp/opencode/objrewriteprobe (shipped IO.Files plus Import DLLs, SDK 10.0.401) reading four decodeable files R8/R12/R20/R22; F1 payload from DIFF-SER-OBJF-001 reused as the encode oracle. Committed as src/serialized_file/object_rewrite_fixture.txt (12 lines). |
| Source-derived behavior | Encode reproduces reference-authored bytes exactly; rewrite with growth reparses on all four generations with correct sizes, values, alignment, and retention. |
| Root cause | No Rust value deviation found. One layout mismatch corrected on the probe side (gen8 must be header-data-metadata, not metadata-middle). |
| Rust change | src/serialized_file/object.rs test object_encode_reproduces_reference_authored_payload; src/serialized_file/file.rs test rewrites_reference_constructed_files_across_generations; fixture src/serialized_file/object_rewrite_fixture.txt. No production change. |
| Regression evidence | Both new tests pass; full suite 1372 plus 60 pass with warnings-denied Clippy. |
| Remaining comparison | Managed-reference payloads stay explicit errors; add/remove objects and table changes stay out of scope by design; exact pad bytes are Rust policy with no baseline counterpart. |

## DIFF-SET-SAVEDEL-001 - Settings save/delete product surface

| Field | Finding |
|---|---|
| Status | Source-behavior parity recorded; settings-file row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export/Configuration/SerializedSettings.cs (SaveToDefaultPath, DeleteDefaultPath, TryLoadFromDefaultPath) |
| Input evidence | Source read (no runtime probe needed: semantics are file overwrite and existence-checked delete on the executable-adjacent path); hermetic temp-directory round-trip tests in src/main.rs. |
| Source-derived behavior | Save overwrites unconditionally; delete is a no-op success when absent; load keeps built-in defaults when absent and fails loudly on corrupt content. |
| Root cause | No deviation; the CLI commands are the product equivalent of the GUI save/delete triggers. |
| Rust change | src/main.rs (save_serialized_settings_to_dir, delete_serialized_settings_in_dir, save/delete_default_settings_from_arguments, CLI dispatch, usage, four tests plus classifier pin). |
| Regression evidence | Four new CLI tests pass; full suite 1373 plus 64 pass with warnings-denied Clippy. |
| Remaining comparison | Unknown enum discriminants stay explicit errors per the row disposition. |

## DIFF-ANIM-CACHE-001 - PathChecksumCache add/get/reset

| Field | Finding |
|---|---|
| Status | Reference-method evidence recorded; cache row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Processing/AnimationClips/PathChecksumCache.cs |
| Input evidence | Probe Temp/opencode/animcacheprobe (shipped Processing/Checksum DLLs, SDK 10.0.401) calling the real Add/TryGetPath/Reset via reflection. Committed as src/resources/anim_cache_fixture.txt (24 lines). |
| Source-derived behavior | Seed present; eight Add words, idempotent re-adds, all reverse lookups, miss, and reset-clears-seed plus re-add all reproduce exactly. |
| Root cause | No Rust deviation found. Two probe-side construction facts: parameterless construction leaves field-initializer dictionaries null, and Reset touches processedAssets (both restored via reflection). |
| Rust change | src/resources/path_checksum.rs test cache_reproduces_the_reference_add_get_reset_sequence; fixture src/resources/anim_cache_fixture.txt. No production change. |
| Regression evidence | New test passes (module 7/7); full suite 1374 plus 64 pass with warnings-denied Clippy. |
| Remaining comparison | BuildPathsCache population and Add(IMonoScript) stay caller-side per the row disposition. |

## DIFF-IO-DISCOVERY-001 - Bundle discovery end to end

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; three discovery rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Structure/GameStructure.cs; AssetRipper/Source/AssetRipper.Assets/Bundles/GameBundle.FromPaths.cs |
| Input evidence | Probe Temp/opencode/bundleprobe2 (shipped Import/Assets/IO DLLs, SDK 10.0.401) running GameStructure.Load over two synthesized UnityFS bundles. Committed as src/io/bundle_discovery_fixture.txt (bundle hexes plus reference dump). |
| Source-derived behavior | Both entry namings (extension and CAB-) discover, parse, and materialize without assemblies; collections are entry-named; unknown ClassIDs yield zero collections; engine collections are injected. |
| Root cause | No Rust deviation on the shared inventory (files, entries, collections, ClassIDs, PathIDs, decode counts). Object models differ by design (generated vs TypeTree). |
| Rust change | src/main.rs test directory_inspection_reproduces_the_reference_bundle_inventory; inspect_directories_to refactor (dead printer removed); fixture src/io/bundle_discovery_fixture.txt. |
| Regression evidence | New test passes; full suite 1374 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Engine injection and Unity-produced files stay out per the row dispositions. |

## DIFF-IO-PLATFORM-001 - Platform detection and registration

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; twelve platform rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Platforms/; AssetRipper/Source/AssetRipper.Import/Structure/Platforms/ |
| Input evidence | Probe Temp/opencode/platformprobe (shipped Import/IO DLLs, SDK 10.0.401) running CheckPlatform plus CollectFiles over 21 constructed trees. Committed as src/io/platform_probe_fixture.txt (specs plus reference dump plus readable version/container bytes). |
| Source-derived behavior | All 21 cases reproduce on kind, name, backend, DataPaths, files, and assemblies, including OBB ordering, dual Mac DataPaths, PS4 null version, and WinPhone shadowing. |
| Root cause | No Rust deviation found. Presence-only decisions plus content reads limited to version files and bundle/SerializedFile header checks (documented per platform). |
| Rust change | src/io/platform_structure.rs test detection_and_registration_reproduce_the_reference_platform_dump; fixture src/io/platform_probe_fixture.txt. No production change. |
| Regression evidence | New test passes (21/21 cases); full suite 1375 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced builds and version-field scope stay out per the row dispositions. |

## DIFF-IO-MIXED-SELECTION-001 - Mixed-input selection

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; three file-selection rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Structure/Platforms/; AssetRipper/Source/AssetRipper.Import/Structure/GameStructure.cs |
| Input evidence | Probe Temp/opencode/mixedprobe (shipped Import/IO DLLs, SDK 10.0.401) running GameStructure.Load over a mixed directory, an explicit file list, and an explicit split-part selection. Committed as src/io/mixed_probe_fixture.txt. |
| Source-derived behavior | Directory, explicit, and split-part selections reproduce on names and paths, including split folding, short-versus-full key conventions, nested traversal, and the unknown-.resS negative. |
| Root cause | Two Rust deviations found and fixed: directory walk dropped split groups (now folded via scan_folded_kind); explicit selections kept the part path (now the logical base path). |
| Rust change | src/io/game_structure.rs production (split folding plus base-path registration); test mixed_selection_reproduces_the_reference_mixed_dump; fixture src/io/mixed_probe_fixture.txt. |
| Regression evidence | New test passes (3/3 selections); full suite 1376 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced inputs and external-file dependency loading stay out per the row dispositions. |

## DIFF-SER-PPTR-001 - PPtr values, external table, and join

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; two file-reference rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/Parser/; AssetRipper/Source/AssetRipper.Import/Structure/Assembly/Serializable/; AssetRipper/Source/AssetRipper.SerializationLogic/SerializablePointerType.cs |
| Input evidence | Probe Temp/opencode/pptrprobe (shipped Import/IO DLLs, SDK 10.0.401) building two gen12 SerializedFiles with PPtr payloads and one external entry. Committed as src/serialized_file/pptr_probe_fixture.txt. |
| Source-derived behavior | Per-file graphs expose the same edges, the external table matches on GUID words/type/path, and the multi-file join resolves every edge with a complete graph. |
| Root cause | No Rust deviation found. Four source facts (expanded PPtr nodes, CommonString typenames, zero-terminated FileIdentifier strings, gen12 LongFileID) proven mid-batch. |
| Rust change | Test pptr_values_external_table_and_join_reproduce_the_reference_dump; fixture src/serialized_file/pptr_probe_fixture.txt. No production change. |
| Regression evidence | New test passes; full suite 1377 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Meta-type GUID-name matching and Unity-produced inputs stay out per the row dispositions. |

## DIFF-EXP-SETTINGS-VERSION-001 - automatic C# language selection

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export/Configuration/ScriptLanguageVersion.cs; AssetRipper.Primitives dependency rank order |
| Input evidence | Probe Temp/opencode/versionprobe (shipped Export + Primitives DLLs, SDK 10.0.401) resolving both automatic modes over 17 boundary Unity versions. Committed as src/export/settings_version_fixture.txt. |
| Source-derived behavior | Both automatic modes match the reference on every version; the HasCSharp9Support gate lands exactly on 2021.2.0b6 and 2022.1.0a3. |
| Root cause | No Rust deviation found. Release-channel rank order plus the source threshold table ported and pinned mid-batch. |
| Rust change | UnityReleaseVersion/UnityReleaseType plus language_version_facts in src/export/settings.rs; fixture src/export/settings_version_fixture.txt; test automatic_language_selection_reproduces_the_reference_version_dump. |
| Regression evidence | New test passes; full suite 1378 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Unity-produced inputs stay out per the row disposition. |

## DIFF-IO-VFS-001 - VirtualFileSystem operations and enumeration

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.IO.Files/VirtualFileSystem.cs; AssetRipper/Source/AssetRipper.IO.Files.Tests/VirtualFileSystemTests.cs |
| Input evidence | Probe Temp/opencode/vfsprobe (shipped IO.Files DLL, SDK 10.0.401) replaying 26 scenarios. Committed as src/io/vfs_probe_fixture.txt. |
| Source-derived behavior | Every operation outcome matches; enumeration membership matches on all shapes while raw order differs (insertion vs lexical). |
| Root cause | No Rust deviation beyond the documented order hardening. Insertion order proven by reverse-creation and nested-shape discriminators. |
| Rust change | Test virtual_file_system_reproduces_the_reference_probe_dump; fixture src/io/vfs_probe_fixture.txt. No production change. |
| Regression evidence | New test passes; full suite 1379 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | GetUniqueName counter loop, local adapter (IO-STREAM-001), and Unity-produced inputs stay out per the row disposition. |

## DIFF-ANIM-MUSCLE-001 - humanoid muscle binding table

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/Enums/AnimationClip/HumanoidMuscleType.cs |
| Input evidence | Probe Temp/opencode/muscleprobe (shipped SourceGenerated.Extensions DLL, SDK 10.0.401) dumping all ids plus versioned updates. Committed as src/resources/muscle_probe_fixture.txt. |
| Source-derived behavior | Every attribute string and every versioned update matches; the throw boundary lands exactly at Last=200. |
| Root cause | One real deviation found and fixed: out-of-range None replaced by HumanoidMuscleError (source throws ArgumentException). No silent drop remains on this path. |
| Rust change | HumanoidMuscleError plus Result return in avatar.rs and clip_convert.rs; fixture src/resources/muscle_probe_fixture.txt; test humanoid_muscles_reproduce_the_reference_probe_dump. |
| Regression evidence | New test passes; full suite 1380 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Binding-to-muscle cast, curve emission, and Unity-produced inputs stay out per the row disposition. |

## DIFF-SCRIPT-RESOLVER-001 - resolver node building

| Field | Finding |
|---|---|
| Status | Reference-product-path evidence recorded; one file-reference row ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Import/Structure/Assembly/Serializable/TypeTreeResolver.cs; .../TypeTrees/TypeTreeNodeStruct.cs; .../TypeTrees/SerializableTreeType.cs; AssetRipper.SerializationLogic (PrimitiveType, ManagedReferenceTypes, SerializablePointerType) |
| Input evidence | Extended probe Temp/opencode/scriptidprobe (shipped Import/IO.Files DLLs, SDK 10.0.401) with six hand-built OldType node lists. Committed as NODESRC/NODETREE/NTTYPE/NTFIELD rows in src/io/script_identity_fixture.txt. |
| Source-derived behavior | Every built type and field matches in exact order. Malformed level steps fail the build where the source throws (explicit None, no silent tree). |
| Root cause | One real deviation found and fixed: ReferencedManagedType flow mapping (true, per its shared constructor). No silent drop remains on this path. |
| Rust change | build_type_nodes, unwrap_primitive, SerializableTreeType model with shared substitutions, and resolve_serializable_tree in processing/script_identity.rs; test serializable_trees_reproduce_the_reference_node_dump. |
| Regression evidence | New test passes; full suite 1381 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Live assembly loading and Cecil type resolution stay TYPE-SCRIPT-001; MonoBehaviour-structure root mode and Tpk trees stay outside this row. |

## DIFF-CUBEMAP-CONVERT-001 - live-cubemap conversion (flip gate and UnpackNormal)

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Textures/TextureConverter.cs (`TryConvertToBitmap(ITexture2D, ...)`, `UnpackNormal`); AssetRipper/Source/AssetRipper.SourceGenerated/Classes/ClassID_89 (27 versioned `Cubemap_*` classes) |
| Input evidence | Probe Temp/opencode/cubeconvprobe (shipped Export.Modules.Textures + SourceGenerated DLLs, SDK 10.0.401) converting a real class-89 Cubemap (6 DXT5 faces, hand-crafted flip-asymmetric blocks) with and without the `NormalmapDXT5nm` lightmap plus a class-28 Texture2D over the same face bytes. Committed as src/resources/cubemap_convert_fixture.txt (15 rows). |
| Source-derived behavior | Cubemap output stacks six 8x8 faces with no FlipY; the plain texture flips; `UnpackNormal` swaps alpha and red (the `GetChannels` out-parameter order is `(r, g, b, a)`, so the caller locals named `(a, g, _, r)` receive input red in `a` and input alpha in `r`) and recomputes blue with truncating conversion. |
| Root cause | One real deviation found and fixed: `Bitmap::unpack_normal` kept red/alpha in place and only recomputed blue. The old synthetic-only tests encoded the bug (e.g. `(128,128,0,255)` expected `(128,128,254,255)`); the reference produces `(255,128,127,128)`. Two stale expectations corrected alongside the fix. |
| Rust change | Swapped red/alpha lanes in `Bitmap::unpack_normal` (src/resources/texture_converter.rs); fixture src/resources/cubemap_convert_fixture.txt; test live_cubemap_conversion_matches_the_reference_runtime. |
| Regression evidence | All three probed conversions reproduce every output pixel plus dimensions; full suite 1384 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Live-sprite/atlas conversion and Unity-produced textures stay with their own rows; external decoders (Crunch) stay unsupported like the 2D path. |

## DIFF-SPRITE-ATLAS-001 - live-sprite atlas coordinates

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; six rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/SpriteExtensions.cs (`GetSpriteCoordinatesInAtlas`); AssetRipper/Source/AssetRipper.Assets/Generics/AssetDictionary.cs + AssetPair.cs (map mechanics) |
| Input evidence | Probe Temp/opencode/spriteprobe (shipped SourceGenerated.Extensions + SourceGenerated DLLs, SDK 10.0.401) driving real Sprite objects (2022.3, 5.6, 4.5) with `m_` backing fields and a real SpriteAtlas with an empty or reference-hit RenderDataMap entry. Committed as src/resources/sprite_atlas_fixture.txt (3 gate + 30 case rows). |
| Source-derived behavior | RD path, stored and stored-zero pivots, atlas miss with zero-border preservation, atlas hit with border shift, key-less 5.6 sprite, and derived-pivot 4.5 sprite all reproduce bit-for-bit; `Has_Pivot` is false on 4.5 (true on 5.6/2022.3), `Has_RenderDataKey` false before 2022, `Has_Border` true throughout. |
| Root cause | No Rust deviation found; this increment replaces synthetic-only atlas evidence with live-object vectors. Map population bypasses the pair property setters (reference types rejected) via backing fields, like file readers do. |
| Rust change | Test atlas_coordinates_match_the_reference_runtime in src/resources/sprite.rs; fixture src/resources/sprite_atlas_fixture.txt. No production change. |
| Regression evidence | All six probed cases reproduce rect, pivot, and border; full suite 1385 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | `RD.Texture` pointer resolution and exporter wiring stay with RES-SPRITE-CONVERT-001; Unity-produced sprites stay out per the row disposition. |

## DIFF-SPRITE-CONVERT-001 - live-sprite conversion (pointer resolution and delegation)

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Textures/SpriteConverter.cs (`Supported`, `TryConvertToBitmap`); AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/PPtrExtensions.cs (FileID 0 = same collection) |
| Input evidence | Probe Temp/opencode/spriteprobe (shipped Export.Modules.Textures + Assets DLLs, SDK 10.0.401) with a real 4x4 RGBA32 texture registered via protected `AddAsset` plus real sprites whose `RD.Texture` m_ fields link (FileID 0, PathID 44), null (PathID 0), or dangle (PathID 999). Committed as src/resources/sprite_convert_fixture.txt (13 rows). |
| Source-derived behavior | Linked sprite reports Supported with a flipped 4x4 bitmap; nulled and dangling sprites report false with empty output. FileID 0 resolves inside the sprite's own collection; only the registered PathID is present. |
| Root cause | No Rust deviation found; this increment proves cross-object pointer resolution plus gated delegation end-to-end. |
| Rust change | Test sprite_conversion_matches_the_reference_runtime in src/resources/sprite.rs; fixture src/resources/sprite_convert_fixture.txt. No production change. |
| Regression evidence | All three probed conversions reproduce both decisions plus every pixel; full suite 1386 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Exporter wiring stays with the exporter rows; Unity-produced sprites stay out per the row disposition. |

## DIFF-TEXTURE-ARRAY-CONVERT-001 - array/3D conversion pixels and the 117 unpack gate

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; five rows ported and pinned; one real bug fixed |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Textures/TextureConverter.cs (ITexture2DArray lines 86-113, ITexture3D lines 51-84 with the FlipY plus stored-lightmap UnpackNormal, ICubemapArray lines 115-142 with the FlipY kept despite `Maybe not needed?`); AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/Texture3DExtensions.cs (`GetLightmapFormat` lines 88-98, usage-backed) |
| Input evidence | Probe Temp/opencode/arrayconvprobe (shipped Export.Modules.Textures + Assets DLLs, SDK 10.0.401) driving real class-187 (2x2 depth 2), class-117 (2x2x2 plain and UsageMode-5), and class-188 (width 2 count 1) assets with full inline R8G8B8A8_UNorm bytes. Committed as src/resources/texture_array_convert_fixture.txt (15 rows). |
| Source-derived behavior | All three overloads FlipY per layer (array rows land [r1,r0,r3,r2], cubemap faces flip pairwise); empty buffers refuse; UsageMode 5 with stored LightmapFormat 0 does NOT unpack because the converter reads the stored `LightmapFormatE`, not `GetLightmapFormat`. |
| Root cause | One real Rust deviation found and fixed: `export_texture3d_image` unpacked on the derived (usage-backed) lightmap value, so a normal-mapped usage with a default stored field unpacked where the reference does not. The gate now reads the stored field only. An early probe scare (ArgumentOutOfRange on every conversion) traced to short probe buffers (one pixel per stacked row); full buffers convert. |
| Rust change | `resources::texture::export_texture3d_image` unpack gate plus doc comment; test `array_conversions_match_the_reference_runtime` in `resources::texture`; fixture `src/resources/texture_array_convert_fixture.txt`. No other production change (`flip_y` already flips per layer). |
| Regression evidence | All five probed conversions reproduce ok/size/pixels through the export functions with PNG wiring asserted; the `t3-normal` row fails under the old derived gate. Full suite 1387 plus 65 pass with warnings-denied Clippy. |
| Remaining comparison | Crunch stays unsupported on these paths like the 2D one; streamed fetching stays caller-side; Unity-produced array textures stay out per the row disposition. |

## DIFF-TEXTURE-EXPORTER-001 - exporter admission and format selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; fourteen rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/TextureAssetExporter.cs (`TryCreateCollection` lines 26-38, `Export` lines 40-60); AssetRipper/Source/AssetRipper.Export/Configuration/ImageExportFormat.cs (`GetTextureExportFormat` lines 93-100, `TryGetFromExtension` lines 61-75); AssetRipper/Source/AssetRipper.Processing/AssetGroup.cs (`SetMainAsset`); AssetRipper/Source/AssetRipper.Assets/IUnityObjectBase.cs (`GetBestExtension`, Override ?? Original) |
| Input evidence | Probe Temp/opencode/texexpprobe (shipped UnityProjects/Processing/Export/Modules DLLs, SDK 10.0.401) with a real texture plus sprite grouped in a SpriteInformationObject (both MainAssets verified), a lone texture, and a GameObject, driving shipped `TryCreateCollection` per sprite mode, `CheckAssetIntegrity`, `TryConvertToBitmap`, and the full selection matrix. Committed as src/export/texture_exporter_fixture.txt (18 rows). |
| Source-derived behavior | Grouped texture admits under all three modes (non-sprite bypasses the Yaml gate); grouped sprite admits everywhere except Yaml; lone texture and GameObject decline; integrity and conversion pass on the live texture; `jpg` selects Jpeg, `??` lets an unrecognized override shadow a recognized original, uppercase `JPEG` falls back to configured. |
| Root cause | No Rust deviation found; this increment proves exporter branch selection plus format selection end-to-end through real objects. |
| Rust change | Test texture_exporter_matches_the_reference_runtime in export/export_extension.rs; fixture src/export/texture_exporter_fixture.txt. No production change. |
| Regression evidence | All 8 admissions and all 6 selections reproduce through the decision kernels with the probed integrity/conversion facts; export suite 18/18. |
| Remaining comparison | Bitmap encode and file IO stay caller-side; Unity-produced projects stay out per the row disposition. |

## DIFF-SHADER-STATE-001 - render-state predicates and pass selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; 43 behavior rows ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/SerializedShaderFloatValueExtensions.cs, SerializedShaderVectorValueExtensions.cs, SerializedShaderStateExtensions.cs, SerializedStencilOpExtensions.cs, SerializedShaderRTBlendStateExtensions.cs, SerializedPassExtensions.cs; Enums/Shader/ShaderType.cs, Enums/Shader/SerializedShader/StencilOp.cs, StencilComp.cs |
| Input evidence | Probe Temp/opencode/shstateprobe (shipped SourceGenerated.Extensions + SourceGenerated DLLs, SDK 10.0.401) with real SerializedShaderFloatValue_5_5, SerializedShaderVectorValue_5_5, SerializedShaderState_2017_2, SerializedStencilOp_5_5, SerializedPass_2017, and SerializedPass_2019_3_0_a7 instances plus `m_` backing-field writes, calling the shipped predicates. Committed as src/resources/shader_state_fixture.txt (49 rows). |
| Source-derived behavior | Exact float equality (-0.0 is zero, denormals are not), 255-max, `<noninit>`/empty namelessness, postfix-free search order, Keep/Keep/Keep/Always stencil default, alpha-nonzero/conservative-present-nonzero/On-Off words, the six-way stencil conjunction, mask-bit program membership with source order, the 2019 ray-tracing gate, live ShaderType/SerializedPassType tables, and C# `(int)`-cast agreement with Rust on NaN/out-of-range magnitudes. |
| Root cause | No Rust deviation found; this increment replaces source-derived expectations with live-object vectors and upgrades the `IsDefault` truncation scope note from assumed to probe-backed. A whole-tree caller audit over the frozen snapshot finds zero external call sites for any of these members, so the kernels pin decision shapes with no observable product behavior behind them. |
| Rust change | Test render_state_predicates_match_the_reference_runtime in resources/shader.rs; fixture src/resources/shader_state_fixture.txt; `IsDefault` doc now cites probe evidence. No production change. |
| Regression evidence | All 43 behavior rows replay through the kernels; shader suite 13/13. |
| Remaining comparison | Name backing selection, generic enum-cast machinery, name-or-value emission strings, remaining typed casts, and MaxShaderModelVersion stay caller-side; Unity-produced shaders stay out per the row disposition. |

## DIFF-SPRITE-METADATA-001 - sprite sheet entry fill

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; three fill cases ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/SpriteMetaDataExtensions.cs (`FillSpriteMetaData` lines 26-71); AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/TextureExportCollection.cs (`AddSpriteSheet` lines 140-165, `SpriteMetaData.Create` per version) |
| Input evidence | Probe Temp/opencode/smetaprobe (shipped SourceGenerated DLL, SDK 10.0.401) with real 2022.3 sprites (plain, boned, atlas-hit, atlas-miss) plus `SpriteMetaData.Create` instances through the shipped fill. Committed as src/resources/sprite_metadata_fixture.txt (19 rows). |
| Source-derived behavior | Verbatim name carry, atlas-hit rect/pivot/border (with the border shift), miss-path RD rect/pivot/border, `Custom` alignment, zero tessellation, gate decisions, PPU bone scaling with the sprite-Rect (not atlas) root offset, and SpriteID assignment on version gates alone (even with an empty bone list). The vertex-less reference leaves outline/physics/vertices/indices/weights empty. |
| Root cause | No Rust deviation found; this increment replaces synthetic-only gate coverage with live-object vectors over the exact exporter-called factory and entry point. |
| Rust change | Test sprite_metadata_fill_matches_the_reference_runtime in resources/sprite.rs; fixture src/resources/sprite_metadata_fixture.txt. No production change. |
| Regression evidence | All three fill cases replay field-for-field through the kernel; sprite suite 19/19. |
| Remaining comparison | Mesh-vertex extraction bodies (`SetBoneGeometry`, `VertexDataBlob` reads, `GenerateOutline`/`GeneratePhysicsShape`, `MeshOutlineGenerator`) stay open by baseline size; real skinned-sprite vertex payloads stay out per the row disposition. |

## DIFF-MATERIAL-VIEW-001 - material property surface

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; five material cases ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/MaterialExtensions.cs (`FindPropertyNameByCRC28`, `TryGetTextureProperty`, `GetTextureProperties`); UnityPropertySheetExtensions.cs; AssetRipper/Source/AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs (line 107 live caller); AssetRipper/Source/AssetRipper.Export.Modules.Models/GlbLevelBuilder.cs (line 244 live caller) |
| Input evidence | Probe Temp/opencode/matprobe (shipped SourceGenerated/Checksum DLLs, SDK 10.0.401) with real `Material.Create` sheets on 3.5, 5.6, 2019.4, and 2022.3 (empty and populated) through the shipped property surface with shipped CRC28 words. Committed as src/resources/material_view_fixture.txt (62 rows). |
| Source-derived behavior | Exactly one tex-env/float/color gate per version (`Has_Ints` off on 3.5 and 5.6, on from 2019.4); serialized texture order; hit/miss lookups; CRC searches (postfix hits, plain float hit, misses); empty-but-gated sheets return null without throwing on all four versions. |
| Root cause | No Rust deviation found; this increment replaces source-derived expectations with live-object vectors. The `NotSupportedException` arm is bounded: every shipped version matches exactly one tex-env gate and the unversioned base sheet is abstract, so it fires only for file-reader-produced unknown layouts. |
| Rust change | Test material_view_matches_the_reference_runtime in resources/material.rs; fixture src/resources/material_view_fixture.txt. No production change. |
| Regression evidence | All five material cases replay through the view; material suite 26/26. |
| Remaining comparison | FastPropertyName shapes stay synthetic (cross-version dict writes are invisible to the version-gated extension); Material `.mat` YAML export and the shader exporters remain open. |

## DIFF-ANIM-HIERARCHY-001 - animator editor-hierarchy walk

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; 27 yields plus slot order ported and pinned |
| Source location | AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/AnimatorControllerExtensions.cs (`GetStateBehaviours` lines 44-72, `FetchEditorHierarchy` lines 74-299); live callers AssetRipper/Source/AssetRipper.Processing/AnimatorControllers/AnimatorControllerProcessor.cs and AssetRipper/Source/AssetRipper.Export.UnityProjects/AnimatorControllers/AnimatorControllerExportCollection.cs |
| Input evidence | Probe Temp/opencode/animhierprobe (shipped SourceGenerated DLL, SDK 10.0.401) with a real versioned controller graph (2022.3 layers/pairs/machines plus 5.0 and 4.0 machines/states, FileID-0 PPtrs) through the shipped walk and behaviour lookup. Committed as src/resources/animator_hierarchy_fixture.txt (194 rows). |
| Source-derived behavior | Exact 27-yield claim order: layer behaviour/motion pairs, nested blend-tree recursion, the modern machine order, and the 4.0 legacy else-branch order (child machines, local, ordered, states); animation clips never yield; no deduplication on double-claimed states; slot order follows the index array ([1,0]); missing keys yield empty; resolved behaviours get `HideInHierarchy` (0->1). |
| Root cause | No Rust deviation found; this increment adds the fetch-order kernels and replaces synthetic-only slot/gate coverage with live-object vectors. Probe mechanics of note: same-version PPtrs resolve through the composite `PathID` while the _4 family needs the `m_PathID_` int field (the split slot silently mis-resolves); PPtrAccessList views reject AddNew over null backings, so file-reader-style backing fills are required; the 5.0 machine is modern-layout, only 4.0 takes the legacy branch. |
| Rust change | Fetch-order kernels (`fetch_editor_hierarchy` plus `FetchController/Layer/Pair/StateMachine/State/BlendTree` facts) and test editor_hierarchy_matches_the_reference_runtime in resources/animator_controller.rs; fixture src/resources/animator_hierarchy_fixture.txt. |
| Regression evidence | All 27 yields plus the live slot vector replay through the kernels; animator suite 6/6. |
| Remaining comparison | Pointer resolution and the flag mutation stay caller-side per the row disposition; transition-destination context rebuilds remain with their own coverage. |

## DIFF-GLB-MATERIAL-001 - GLB main/normal texture selection

| Field | Finding |
|---|---|
| Status | Reference-runtime differential recorded; five selection cases ported and pinned, one production gap closed |
| Source location | AssetRipper/Source/AssetRipper.Export.Modules.Models/GlbLevelBuilder.cs (`GetTextures` lines 239-261, `IsMainTexture`, `IsNormalTexture`, private static on the nested `BuildParameters`); `MakeMaterialBuilder` lines 224-237 |
| Input evidence | Probe Temp/opencode/glbmatprobe (shipped Export.Modules.Models DLL, SDK 10.0.401) with real 2022.3 materials plus registered RGBA32 textures through the shipped selection. Committed as src/resources/glb_material_fixture.txt (25 rows). |
| Source-derived behavior | First main/normal spelling wins; any other property is the replacement that fills an absent main; the `??=` applies after resolution, so a null-texture main entry keeps looking (null-skips to `_Texture`). |
| Root cause | Real deviation found and fixed: the name-only kernel blocked later main spellings behind a null-texture first pick, while the reference skips unresolvable picks. Fixed with the resolution-aware `glb_material_textures_resolved` taking a caller predicate; the name-first kernel is unchanged. |
| Rust change | New `glb_material_textures_resolved` in resources/material.rs plus test glb_material_textures_match_the_reference_runtime; fixture src/resources/glb_material_fixture.txt. |
| Regression evidence | All five selection cases replay at resolved-path level; material suite 27/27; existing name-first tests unchanged. |
| Remaining comparison | Image decoding and SharpGLTF material building remain open. |

## DIFF-ANIM-CUSTOM-CURVE-001 — custom-curve resolver behavior and ReverseAscii salt

| Field | Finding |
|---|---|
| Status | Resolver kernels match the captured reference-runtime vectors; production animation integration remains open |
| Source location | `AssetRipper.Processing/AnimationClips/CustomCurveResolver.cs` (`ToAttributeName`); `AssetRipper.Checksum` `Crc32Algorithm.ReverseAscii`; generated component `TryGetPath` dictionaries |
| Input evidence | Isolated `Temp/opencode/curveresolve` probe using the shipped .NET 10.0.401 reference assemblies and generated 2022.3 objects for an Animation, hierarchy, skinned mesh, and renderer/material. `Temp/opencode/ildump` decoded the shipped checksum method. Captured values are committed in `src/resources/custom_curve_resolve_fixture.txt`; the 11 path tables are committed in `src/resources/custom_curve_tables.rs`. These are live managed-object probes and source-derived table dumps, not Unity-produced files. |
| Source behavior | The fixture pins 39 actual `ToAttributeName` cases, including table hits/misses, scene-dependent blend-shape/material outcomes, manufactured fallbacks, and exact unsupported-enum exceptions. `ReverseAscii` passes `HashAscii(prefix, "HHHHHHH")`, so the seven-H marker is appended to the prefix before the CRC is computed. The 11 component dictionaries contain 2,551 entries. |
| Root cause | The initial Rust port hashed only the prefix before reversing the CRC, omitting the source's seven-H marker. The first fixture expectations also had incorrect CRC words and did not reproduce the resolver's real scene predicate. The live probe exposed these mismatches; Rust now streams the marker into the same CRC state and the fixture records the correct predicates and values. |
| Rust change | `src/resources/blend_shape.rs` now exposes a crate-private incremental IEEE CRC update; `src/resources/custom_curve.rs` ports the `ReverseAscii` transform, resolver dispatch, table hits, and source-compatible errors for unsupported binding enum values. |
| Regression evidence | `cargo test --workspace --locked custom_curve::tests` passes 12/12. The fixture includes 14 `ReverseAscii` outputs and 2 direct `HashAscii` marker hashes. The full Rust workspace passes 1,397 library tests and 65 CLI tests; formatting, warnings-denied Clippy, and the 1,449-file baseline verifier pass. |
| Remaining comparison | Rust `converter::process_clip` calls `to_attribute_name`; `DIFF-ANIMATION-CLIP-PROCESS-001` source-replays one complete 2022.3 clip, and `DIFF-ANIMATION-PROJECT-EXPORT-001` now supplies workspace hierarchy plus Mesh blend-shape and Renderer/Material property facts in the production caller. External script facts, remaining version branches, and dynamic Unity animation sampling remain open. Keep `PROC-ANIM-CUSTOM-CURVE-001` open. |

## DIFF-MANAGED-REFS-002 - nested managed-reference registries

| Field | Finding |
|---|---|
| Status | Closed for the source reader paths exercised by independent modern and legacy nested-registry fixtures; no Unity-produced or dynamic Unity result is claimed |
| Source location | `AssetRipper.Import/Structure/Assembly/Serializable/ManagedReferenceRegistryReader.cs` (`Read`, `ReadManagedType`, `ReadReferencedObject`, `ReadReferencedObjectData`); `SerializableStructure.cs` (`Read`); `TypeTreeResolver.cs`; `TypeTrees/SerializableTreeType.cs` (`FromReferencedObjectNode`, `AddNode`); `AssetRipper.SerializationLogic/ManagedReferenceTypes.cs` |
| Source evidence | The frozen source's `FromReferencedObjectNode` excludes only a direct root Registry marker. `AddNode` retains and constructs a Registry node below a nested structure, and `SerializableStructure.Read` dispatches that field through `ManagedReferenceRegistryReader.Read`. The checked-in probe builds those TypeTrees and runs the shipped reader on the same fixture bytes used by Rust. No UABEA or user AssetBundle data was used. |
| Rust change | `decode_typetree_payload` now recursively decodes nested registries with inherited object-stream offsets, allocation limits, and a configurable recursion bound. The writer uses the same recursive TypeTree path. The reference graph assigns stable preorder scopes to structs owning a Registry field; entries and ID edges resolve by `(scope, id)`, so duplicate IDs in different scopes remain distinct. Scope count, object count, edge count, and path bounds produce explicit errors. |
| Regression evidence | `tools/probes/managed-nested/Program.cs` consumes counted modern `rid` and terminated legacy indexed `id` fixtures to exactly 152/152 and 200/200 bytes. Rust's `nested_registry_decodes_writes_and_resolves_equal_ids_in_distinct_scopes` decodes and re-encodes both fixtures byte-for-byte, resolves equal IDs to the proper local entries, and checks depth/scope bounds. Existing root tests retain shared/cyclic IDs, nested PPtrs, YAML, and framing errors. |
| Remaining difference | No discrepancy remains in the source paths exercised. The source reader does not publish a separate managed-ID resolution graph; Rust's graph scope mapping follows the nested `SerializableStructure` ownership and is checked with equal IDs on both sides of the scope boundary. Unity-produced fixture and dynamic Unity validation remain unrun. `SER-MANAGED-REFS-001` is verified; the Phase 1 gate remains open. |

## DIFF-PREFAB-SCENE-BUNDLE-001 - scene hierarchy bundle-name assignment is ignored

| Field | Finding |
|---|---|
| Status | Closed for candidate selection and effective property behavior in the isolated AssetRipper runtime; no Unity-produced or Editor import/runtime result is claimed |
| Source location | `AssetRipper/Source/AssetRipper.Processing/Prefabs/PrefabProcessor.cs` (`Process` scene pass, lines 28-48); `AssetRipper/Source/AssetRipper.Processing/GeneratedObjectBase.cs` (`AssetBundleName` no-op setter); `Bundle.FetchAssets`; `AssetBundleExtensions.GetAssetBundleName`; `SerializedBundle.Name` |
| Source behavior | In `GameBundle.Scenes` order, the first scene collection whose owner is a `SerializedBundle` supplies the candidate. The first `IAssetBundle` in `FetchAssets()` order contributes `GetAssetBundleName()` (nonempty serialized `AssetBundleName_R`, else owning bundle name); absent an asset, the candidate is `bundle.Name`; absent a `SerializedBundle`, there is no candidate. `PrefabProcessor` assigns that candidate to `SceneHierarchyObject.AssetBundleName`, but the generated hierarchy inherits `GeneratedObjectBase.AssetBundleName`, whose setter does nothing and whose getter returns null. Thus the effective property is always null. The streamed-scene `Debug.Assert` has no release behavior. |
| Rust change | The bundle planner now keeps the selected candidate distinct from `effective_asset_bundle_name`, which remains `None`. Materialized scene hierarchies retain the candidate for diagnostics but expose an unset effective property; `export-project` writes blank DefaultImporter bundle-name fields, and `inspect` reports both values instead of presenting the candidate as the stored property. |
| Regression evidence | `tools/probes/scene-processor/Program.cs` places full 2019.4 class-ID-141 and class-ID-142 fixtures inside a `SerializedBundle` and runs the shipped processors. The runtime reports `BUNDLE-COMPUTED|streamed-scene-bundle` and `HIER-SET|<null>|...`; the C# source base confirms the no-op setter. Rust source-output replay checks `bundle=null`, and the project export test verifies blank `assetBundleName`/`assetBundleVariant` with a `DefaultImporter`. `tools/probes/scene-importer` records `DefaultImporter.Create` default fields in `src/export/default_importer_probe_fixture.txt`. Focused tests pass: 21 PrefabProcessor tests, the streamed-scene export integration, and four scene-summary CLI tests. |
| Remaining difference | Generated importer defaults and Rust sidecar fields are covered, but a complete source `SceneExportCollection` write and Unity Editor import remain unrun. `Has_IsStreamedSceneAssetBundle` version gates remain caller facts because the frozen snapshot does not expose their generated thresholds. Phase 1 remains open. |

## DIFF-SINGLEFILE-IMPORT-001 - single-path import against reference-written containers

| Field | Finding |
|---|---|
| Status | Closed for the one-path mount contract exercised with reference-written WebData, split paths, truncation, and gap inputs; no Unity-produced or dynamic Unity result is claimed |
| Source location | `AssetRipper/Source/AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`); `AssetRipper/Source/AssetRipper.IO.Files/WebFiles/WebFile.cs` (`Write`, `Read`); `AssetRipper/Source/AssetRipper.IO.Files/Streams/MultiFileStream.cs`; `AssetRipper/Source/AssetRipper.IO.Files/FileContainer.cs` |
| Source behavior | `WebFile.Write` with entry alignment on and off produces distinct bytes that both read back with identical entries (`alpha.bin` 13 bytes, `beta.bin` 5 bytes). An 8-byte truncation does not throw: dispatch falls through signature recognition and the input loads as a raw `ResourceFile` leaf. Split base and split-part paths resolve to the same entries. A missing middle part (`split0` + `split2` without `split1`) is rejected with an `Exception`. |
| Rust change | No production change was needed: `SchemeReader::mount_path` already implements this contract. The new test pins it against the reference bytes. |
| Regression evidence | Isolated probe `tools/probes/singlefile/Program.cs` (frozen `AssetRipper.IO.Files`, SDK 10.0.401) wrote both containers and recorded every outcome in `src/scheme/singlefile_probe_fixture.txt`. Rust test `mount_path_matches_reference_written_webdata_and_fails_loud_on_gap` replays all rows through production `mount_path`: both reference variants mount 2 entries with byte-identical payloads, base and part paths agree, the gap fails naming the missing part with no partial mount left behind, and the truncation mounts verbatim as a raw leaf matching the source fallback. Full workspace: 1416 library and 68 CLI tests; warnings-denied Clippy, format check, and `git diff --check` passed; baseline verifier still exits 1 on the 188 pre-existing unlisted build outputs with all 1,449 listed files unchanged. |
| Remaining difference | No discrepancy remains in the source paths exercised. Mounted VFS entries feed the same workspace ingestion and dependency-report pipeline as directory imports; missing/ambiguous/resource dependency outcomes stay explicit in that shared layer. Unity-produced single-file inputs and dynamic Unity import checks remain unrun. `IO-FS-IMPORT-PATH-001` is verified; the Phase 1 gate remains open with 35 non-verified rows. |

## DIFF-BUILDSETTINGS-REAL-LAYOUT-001 - BuildSettings real TPK layouts and reference materialization

| Field | Finding |
|---|---|
| Status | Closed for the class-141 scene-path layouts exercised across the dumped versions; the struct-entry branch is newly bounded as defensive-only. |
| Source location | `AssetRipper/Source/AssetRipper.Import/Structure/Assembly/TypeTrees/TypeTreeNodeStruct.cs` (`TryMakeFromTpk`); `AssetRipper/Source/AssetRipper.AssemblyDumper/Passes/Pass002_RenameSubnodes.cs` (line 473: `m_Levels` to `m_Scenes` earlier than 5.2); `AssetRipper/Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs` (fixed-layout `ReadNormalObject`); `AssetRipper/Source/AssetRipper.Tools.DependenceGrapher/Program.cs` (bundle loading pattern) |
| Source behavior | The real class-141 release trees use plain string vectors for scenes on every dumped version (4.7.2f1 `levels`, 5.0.0f1 `levels`, 5.2.0f1 through 6000.0.23f1 `scenes`): `vector` to `Array` to `int size` plus `string data` to `Array` to `int size` plus `char data`. No struct-with-`path` entry shape exists in any dumped layout. The rename rule proves real serialized trees carry `m_`-prefixed names. The last `bool` before the strings carries `AlignBytes` (0x4000) so the version string starts on a four-byte boundary. `GameAssetFactory` reads the fixed generated layout and yields `UnreadableObject` unless the payload matches byte-exactly. |
| Rust change | No production change was needed: `decode_build_settings_scene_paths` already handles both `m_Scenes` and `m_Levels` string vectors, and the object decoder already honors the `AlignBytes` flag (0x4000). The full-layout test fixtures now mirror the dumped TPK order, shapes, and bool-run alignment. The struct-entry branch stays as synthetic-tested defensive code with no version provenance in class 141 (likely confusion with `EditorBuildSettings` scenes, a different settings object). |
| Regression evidence | Isolated probe `tools/probes/buildsettings/Program.cs` (frozen runtime, SDK 10.0.401) dumped the real TPK node lists for 4.7.2f1, 5.0.0f1, 5.2.0f1, 2019.4.40f1, 2021.3.0f1, 2022.3.0f1, and 6000.0.23f1 (recorded in `src/processing/buildsettings_tpk_fixture.txt`). Rust-built full-layout payloads for 2019.4.40f1 (`m_Scenes`) and 5.0.0f1 (`m_Levels`) were parsed by the reference and materialized as REAL generated objects (`BuildSettings_2018_3_0_b4`, `BuildSettings_5`) with agreeing scene paths, version strings, GUID words, and flags (recorded in `src/processing/buildsettings_probe_fixture.txt`). Rust test `full_layout_build_settings_match_the_reference_materialization` replays every row through production `decode_build_settings_scene_paths` plus field reads. Full workspace: 1419 library and 69 CLI tests; warnings-denied Clippy, format check, and `git diff --check` passed; baseline verifier still exits 1 on the 188 pre-existing unlisted build outputs with all 1,449 listed files unchanged. |
| Remaining difference | No discrepancy remains in the source paths exercised. A Unity-produced `globalgamemanagers` file and dynamic Unity import checks remain unrun. `PROC-SCENE-BUILDSETTINGS-001` is verified; the Phase 1 gate remains open with 32 non-verified rows.

## DIFF-EXPORT-ROOT-GUARD-001 - Export root guard, deletion consent, and the consent-declined default

| Field | Finding |
|---|---|
| Status | Closed for the guard contract exercised by the reference-runtime probe, with one intentional and documented deviation in how a refused export is surfaced. |
| Source location | `AssetRipper/Source/AssetRipper.GUI.Web/GameFileLoader.cs` (`ExportUnityProject` 59-76, `ExportPrimaryContent` 78-99, `IsValidExportDirectory` 108-122, `IsNonEmptyDirectory` 124-127, `UserConsentsToDeletion` 129-142) |
| Source behavior | Both export entry points run the same guard. An empty path, or a path whose `Path.GetFileName` is exactly `Desktop`, `Documents`, or `Downloads`, is invalid and the method returns without exporting. When loaded and valid: if `Directory.Exists(path)` and `EnumerateFileSystemEntries(path).Any()` then consent is requested; on refusal the source logs `User declined to delete existing export directory. Aborting export.` and again returns without exporting. Otherwise it runs `Directory.Delete(path, true)`, `Directory.CreateDirectory(path)`, then exports. `UserConsentsToDeletion` returns `true` whenever `Headless` is set, and otherwise a dialog result that defaults to `false`. The reflection probe recorded 22 directory-gate rows, 9 non-empty-directory rows (2 symlink rows emitted `skip`), 48 `Path.GetFileName` rows, and 1 headless-consent row (`true`). |
| Rust change | New `src/export/export_root.rs` ports `source_file_name`, the protected-name gate, `is_non_empty_directory`, and `prepare_export_root(path, delete_existing)` (symlink-aware delete of a non-empty root, then create). `export_workspace_yaml_project` now calls it as its first statement, replacing the older `ensure_fresh_output_root`, so the guard runs before any project write and an existing non-empty root can never be written over. `export-project` accepts `--delete-existing` before or after the inputs and never consumes it as the output root; the flag selects the source's delete-and-recreate arm. Without the flag the product keeps the source's consent-declined arm, so a default export stays non-destructive. |
| Regression evidence | Replay test `reference_export_root_guard_rows_match_the_rust_port` in `src/export/export_root.rs` substitutes the probe work root into `src/export/project_guard_fixture.txt` and asserts every row through production code, pinning Windows counts `[22, 7, 48, 1]` (the 2 symlink rows are recorded `skip`) and the consented delete; `validate_export_directory_rejects_empty_and_system_directory_names`, `is_non_empty_directory_requires_a_readable_directory_with_entries`, and 4 `prepare_export_root_*` tests cover the arms directly. CLI: `export_project_refuses_a_nonempty_output_directory_without_touching_it`, `export_project_replaces_a_nonempty_output_directory_when_delete_existing_leads`, `export_project_accepts_delete_existing_after_the_inputs`, `export_project_treats_delete_existing_as_a_flag_not_as_the_output_directory`, and the end-to-end `export_project_writes_a_cross_file_yaml_workspace` are green. Full workspace: 1432 library and 72 CLI tests passed; warnings-denied Clippy on Windows and on `--target x86_64-unknown-linux-gnu`, format check, and `git diff --check` passed; `tools/verify-baseline.ps1` exits 0 (1,449 files, no missing, changed, or unlisted). |
| Remaining difference | The source silently returns (no export, no error) on an invalid path or refused consent; the Rust port returns a typed `ExportRootError` and the CLI exits non-zero with a message containing the source's refusal sentence. That surfacing difference is intentional: a skipped export must not be reported as a successful one. The probe emitted `skip` for the two symlink rows, so the symlink-aware delete is implemented and reviewed but not demonstrated at runtime. The headless auto-consent arm is deliberately not the product default. |

## DIFF-EXPORT-DIR-GATE-UNIX-001 - Export directory gate and `Path.GetFileName` extraction on Unix

| Field | Finding |
|---|---|
| Status | Closed for the Windows-runtime rows and for the divergence found in the verified product path; the Unix arm is source-derived only. |
| Source location | `AssetRipper/Source/AssetRipper.GUI.Web/GameFileLoader.cs` (`IsValidExportDirectory`); authoritative .NET sources under `target/ref-src/`: `Path.cs` (`GetFileName(ReadOnlySpan<char>)`, 236-248), `Path.Windows.cs`, `Path.Unix.cs`, `PathInternal.Windows.cs` (`GetRootLength`, device/UNC/extended prefixes, drive-letter check) |
| Source behavior | `Path.GetFileName` starts at `max(root length, last separator index + 1)`. Windows root rules produce `""` for `C:`, `C:\`, `\\server\share`, `//root`; `file` for `\\?\C:\x\y`, `\\?\UNC\server\share\file`, and `\\\\.\C:\x`; `a` for `C:a`; `b:c` for `a\b:c`. Unix treats only `/` as a separator with a single-slash root, so `//root` yields `root` (Windows `""`), `C:` yields `C:` (Windows `""`), and `C:\Users\me\Desktop` yields `Desktop` rather than `""`. The gate itself only compares that basename against `Desktop`/`Documents`/`Downloads`, so the platform difference is observable on Unix for Windows-style absolute paths. |
| Rust change | `export_root::source_file_name` implements the shared rule over raw platform units (`cfg`-split `encode_wide`/`from_wide` and `as_bytes`/`from_vec`) with a `#[cfg(windows)]` device/UNC/drive-letter root length and a `#[cfg(unix)]` single-slash root. `product::is_valid_export_directory` now delegates to it, and `product.rs` re-exports the single `PROTECTED_EXPORT_DIRECTORY_NAMES` constant instead of keeping a duplicate. Previously the product gate used `path.rsplit(['/','\\']).next()`, which treated `\` as a separator on every host, so on Unix it rejected `C:\Users\me\Desktop` where the source accepts it. |
| Regression evidence | `source_file_name` is pinned by a 12-case cross-platform table plus a `#[cfg(windows)]` 23-case table and a `#[cfg(unix)]` 9-case table derived from the authoritative .NET sources; `product.rs` gained `export_directory_gate_agrees_with_the_shared_export_root_guard`, which asserts over 13 paths that `is_valid_export_directory(path)` equals `validate_export_directory(Path::new(path)).is_ok()`, so the product gate and the shared guard cannot drift again, alongside a `cfg`-split test that keeps `C:\Users\me\Desktop` rejected on Windows and accepted on Unix. The 48 `Path.GetFileName` fixture rows replay the Windows runtime. Full workspace: 1432 library and 72 CLI tests passed; warnings-denied Clippy on Windows and on `--target x86_64-unknown-linux-gnu`, format check, and `git diff --check` passed. |
| Remaining difference | No Unix runtime is available on this host, so the Unix root-length table, the Unix arm of the gate, and the Unix invalid-character set of `src/io/file_names.rs` (`DIFF-FILE-NAME-001`) rest on the authoritative .NET sources rather than a runtime differential, and the non-portable fixture rows are skipped by design. Cross-target Clippy proves the Unix arm compiles warning-free, not that it behaves correctly at runtime. | |

## DIFF-PREFAB-IMPORTER-DEFAULTS-001 - PrefabImporter factory defaults

| Field | Finding |
|---|---|
| Status | Closed for the default object returned by PrefabImporter.Create with a null AssetCollection on the probed Unity version. RootGameObject AssetBundleName application and the full source collection export remain open under EXPORT-UNITY-001. |
| Source location | Source/AssetRipper.Export.UnityProjects/Project/PrefabExportCollection.cs (CreateImporter); AssetRipper.SourceGenerated ClassID_468431735.PrefabImporter.Create |
| Source behavior | The isolated runtime probe on Unity 2022.3.22f1 returned AssetBundleName=null, blank AssetBundleName_R, AssetBundleVariant, Name, and UserData, with ExternalObjects.Count=0. PrefabExportCollection only assigns AssetBundleName_R when RootGameObject.AssetBundleName is non-null. |
| Rust result | The modern Prefab project route emits PrefabImporter with empty externalObjects, userData, assetBundleName, and assetBundleVariant for the synthetic root with no bundle metadata. |
| Regression evidence | tools/probes/scene-importer/Program.cs and src/export/prefab_importer_probe_fixture.txt record the reference-runtime result. project_export_writes_a_processed_prefab_collection_with_generated_transform verifies the resulting .prefab and .meta through export-project. The probe ran against the isolated ar-ref build, not the read-only reference source directory. |
| Remaining difference | The workspace route does not yet derive RootGameObject.AssetBundleName from the source AssetBundle Container. No full source export comparison or Unity Editor import was run; generated legacy Prefab and m_PrefabInternal behavior remain unsupported. |

## DIFF-PREFAB-INSTANCE-ROUTE-001 - Existing modern PrefabInstance processing and export

| Field | Finding |
|---|---|
| Status | Closed for the synthetic Unity 2019.4 modern `PrefabInstance` branch through AssetRipper source processing and the Rust `export-project` route. This does not close full collection-export parity or `EXPORT-UNITY-001`. |
| Source location | `AssetRipper.Processing.Prefabs.PrefabProcessor.Process` existing-PrefabInstance loop; `PrefabHierarchyObject.Create`; `GameObjectHierarchyObject.ExportableAssets`; `AssetRipper.Export.UnityProjects.Project.PrefabExportCollection` |
| Input evidence | Rust test helper `make_existing_prefab_instance_file` builds generation-10 bytes for Unity 2019.4.40f1 with `BuildTarget.NoTarget`, GameObject PathID 10, Transform PathID 11, and PrefabInstance PathID 12 whose `m_RootGameObject` points to 10. The identical file was passed to the reference runtime under a `.unity` path so its collection flags selected the Editor TypeTree layout. SHA-256: `AFF86571CB3C94DDB2130F2B718BB5DC9F286A0971531D247535E504928E984D`. |
| Reference behavior | The isolated `SceneProcessorProbe` run materializes all three input classes as generated objects and the real `PrefabProcessor` creates one `PrefabHierarchyObject`. Root GameObject 10 and Transform 11 are members and exportable; PrefabInstance 12 is retained as the hierarchy source and hidden from YAML output. Exact rows are in `src/processing/prefab_instance_runtime_fixture.txt`. |
| Rust result | `project_export_routes_an_existing_modern_prefab_instance_collection` routes the same bytes through the Rust workspace and project exporter. It writes one `Root.prefab` and one `PrefabImporter` sidecar with exactly the GameObject and Transform documents, reciprocal mapped PPtrs, and no visible PrefabInstance document or duplicate generic `.asset` output. |
| Regression evidence | Focused command `cargo test --workspace --locked project_export_routes_an_existing_modern_prefab_instance_collection -- --nocapture` passed (1 library test; 73 CLI tests filtered). Reference command `dotnet run --project tools/probes/scene-processor/SceneProcessorProbe.csproj --configuration Release -- process target/prefab-instance-source-runtime.txt target/prefab-instance-2019.4.unity` exited 0 against the isolated `ar-ref` build. Probe output confirms `GameObject_2018_3`, `Transform_2018_3`, `PrefabInstance_2018_3`, one Prefab Hierarchies collection, and the expected exportable/hidden sets. |
| Remaining difference | This is a source processor differential plus a Rust product-route fixture, not a comparison against AssetRipper's full Unity project exporter. Unity Editor import was not run because the installed Editor is unlicensed. Generated legacy Prefabs, legacy `m_PrefabInternal` overlays, Container-derived importer bundle metadata, and broader export parity remain open. |

## DIFF-YAML-STREAMED-IMAGE-CLASS-ROUTE-001 - Streamed image-texture YAML routing

| Field | Finding |
|---|---|
| Status | Closed for the streamed YAML preparation branch and generated `IImageTexture` class dispatch; texture decoding and full project-export parity remain open. |
| Source location | `AssetRipper.Export.UnityProjects.ProjectExporter` registrations; `YamlStreamedAssetExporter.TryCreateCollection`; `YamlStreamedAssetExportCollection.ExportTexture`; `IImageTexture`; `StreamingInfoExtensions`. |
| Source behavior | The source registers the YAML streamed exporter for the `IImageTexture` interface. Reflection of the frozen generated assembly reports class IDs 28, 89, 117, 187, 188, and 189. For class 28, the isolated source project exporter resolved `texture.resS` offset 2 size 3, emitted bytes `32 33 34` as Texture2D TypelessData, cleared the serialized stream fields for YAML, then restored the original object values after export. Current TPK layouts for 28/89/117/187/188 serialize `m_StreamData` and `image data`; class 189 uses its generated class fields. |
| Rust result | `workspace_yaml_export_handles_every_generated_image_texture_interface_class` sends six generated TypeTree-shaped SerializedFiles through the Rust collection YAML route, checking bytes, cleared stream fields, and unchanged source objects. `export_project_materializes_streamed_texture2d_and_fails_closed_when_resource_is_missing` checks the class-28 root-resource project path, `.texture2D.meta`, input preservation, and no output on a missing declared resource. |
| Regression evidence | Source probe: `dotnet run --project tools/probes/streamed-image-project/StreamedImageProjectProbe.csproj --configuration Release -p:ReferenceRoot=C:\Users\koishi\AppData\Local\Temp\opencode\ar-ref -- target/source-streamed-image-runtime` exited 0 and asserted the emitted YAML payload/cleared stream plus restored object values. Rust full workspace: 1,485 library passed, 1 ignored; 74 CLI passed; 0 doc tests. |
| Remaining difference | The direct source exporter probe constructs a generated class-28 object and ResourceFile; the compact Rust SerializedFile fixture does not encode the full generated class-28 release payload, so this is a source-behavior differential, not byte-identical source parser replay. The Rust six-class integration is based on the frozen interface union and TPK field probe, not per-version generated dispatch. Source declines empty/unavailable streamed data; the Rust project route reports a missing declared resource as an error instead of silently omitting it. Unity Editor import is unavailable on this host. |
## DIFF-TT-ABSENT-GENERATED-TEXTASSET-001 - Bounded generated TextAsset and MonoScript recovery

| Field | Finding |
|---|---|
| Status | Closed for source-probed Unity 2019.4.40f1 generated TextAsset and MonoScript cases through Rust object workspace and reference graph; the TextAsset also reaches export-project. SER-TT-ABSENT-001 and SER-ASSET-FACTORY-001 remain open. |
| Source location | AssetRipper.Import.AssetCreation.GameAssetFactory.ReadAsset / CreateAsset; generated TextAsset_2018_3; TypeTreeNodeStruct.TryMakeFromTpk; SerializedFile.ObjectReadFlags. |
| Input evidence | The source probe authors TextAsset class ID 49 and MonoScript class ID 115, each at PathID 42, Unity 2019.4.40f1, platform 19 (StandaloneWin64Player), with an empty serialized file TypeTree. Their SF_HEX and SF_READ rows are frozen as textasset_notree and monoscript_notree in src/processing/asset_factory_fixture.txt; a fresh isolated probe replay matched all four rows for each case byte-for-byte. |
| Reference behavior | The shipped factory creates TextAsset_2018_3 named Hero and MonoScript_2018_3 named HeroController from their source TPK layouts. The MonoScript rows also pin m_ClassName=HeroController, m_Namespace=VRC.SDK, m_AssemblyName=Assembly-CSharp, and m_ExecutionOrder=200. The isolated SerFactoryProbe author command exited 0 and wrote only to the ar-ref copy and Rust target scratch/output; its exact invocation and location are recorded in docs/validation-log.md. |
| Rust result | ObjectWorkspace recovers both payloads and retains each recovered TypeTree; the reference graph reports a complete scan for both. The MonoScript workspace regression checks its class, name, namespace, assembly, and execution-order fields. export-project emits one class-49 YAML document for the TextAsset with m_Name: Hero and m_Script: print(1). Both inputs remain unchanged. An unsupported unknown class remains unresolved and fails project export before creating the output root. |
| Regression evidence | Focused command cargo test --workspace --locked tree -- --nocapture passed (41 library tests and 2 CLI tests matched); cargo test --workspace --locked monoscript -- --nocapture passed the MonoScript case. Full suite and build gates are in docs/validation-log.md. |
| Remaining difference | Rust recovery is restricted to generated classes with source-probed 2019.4.40f1 TPK layouts and selects release/editor data using file object-read flags. The project integration exercised the player/release layout only; editor-file fallback selection remains unverified. Other generated classes or versions, TPK TypeTreeObject, MonoBehaviour assembly types, and all GameAssetFactory retry/tail/engine-collection branches also remain open. The project-export route is only checked end to end for TextAsset. Unity Editor import was not run. |

## DIFF-IO-BUNDLE-SCHEME-001 - Bundle dispatch, layouts, entries, and nested reads

| Field | Finding |
|---|---|
| Status | Closed for the source-probed UnityFS entry-failure reporting and continuation behavior across direct, recursive, batch, workspace, inspection, and export-refusal paths. External resource/dependency resolution remains open under `IO-RESOURCE-001`; full Phase 1 parity remains incomplete. |
| Source location | `AssetRipper.IO.Files.SchemeReader.LoadFile` / `ReadFile`; `FileStreamBundleFile.ReadFileStreamMetadata` / `ReadFileStreamData`; `BundleFileBlockReader.ReadEntry`; `RawWebBundleHeader.Read`; `RawWebBundleFile.Read`; `WebFile.Read`; `ArchiveBundleFile.Read`. |
| Fixture evidence | `tools/probes/io-differential/Program.cs` independently builds container bytes and calls the frozen source runtime's `SchemeReader.LoadFile` plus `ReadContentsRecursively`. `src/io/io_differential_fixture.txt` now contains 41 source `SCHEME` cases (the other 30 stream rows were byte-identical to the prior fixture). The source run writes only to the isolated Rust `target/` directory and references the isolated build at `%LOCALAPPDATA%\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release`. |
| Reference behavior | Cases cover UnityFS versions 1/6/7/8; metadata compression None/LZMA/LZ4/LZ4HC plus rejected LZHAM and unknown discriminants; storage None/LZMA/LZ4/LZ4HC, source-rejected LZHAM, and Zstandard frames on the default branch; entries crossing mixed-compression blocks; combined and uncombined directory metadata; front/end metadata placement; v7+ header alignment and padding-at-start; UnityRaw and UnityWeb header versions 1–8; Web's final scene record; nested UnityFS/Raw/Web/WebData and GZip paths; WebData entries; plain resources; and UnityArchive's `NotSupportedException`. A UnityFS LZHAM block becomes one `FailedFile` while a valid sibling remains in the reference container. |
| Rust result | Production `SchemeReader` matches every reference scheme and recursive leaf inventory. For the 36 source-readable container cases without failed entries, the test calls `mount_container_tree_with_source_name`, checks resource counts/names, reads each VFS-mounted file, and compares its bytes with the reference inventory. For the LZHAM entry case, direct and recursive product mounts now retain one typed failed-entry record and continue to mount the later `after.bin` sibling with the reference bytes. Batch aggregation preserves the originating input path; ObjectWorkspace preserves the container owner. CLI inspection reports the failed entry, and project export rejects the incomplete batch/workspace before creating output. Archive remains explicit `ArchiveReadUnsupported`; LZHAM/unknown metadata remain explicit parse errors. |
| Regression evidence | Isolated source command (exit 0): `dotnet run --project tools/probes/io-differential/IoDifferentialProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- target/io-differential-final.txt`; it wrote 71 total lines, including 41 `SCHEME` rows. The fixture includes a valid later sibling in a separate uncompressed block after the LZHAM failure. Rust focused tests passed for the full scheme fixture, workspace failure ownership, and `inspection_reports_failed_bundle_entries_and_project_export_refuses_partial_mounts`; the full suite and static gates are recorded in `docs/validation-log.md`. The source probe emits an existing CA2022 warning in the unrelated `SmartStream.Read` probe. |
| Remaining difference | No difference remains in the source-probed bundle dispatch, recursive mount, and per-entry UnityFS failure-reporting behavior. Failure count and diagnostic metadata limits are explicit fail-closed bounds. External resource-name/dependency resolution remains open under `IO-RESOURCE-001`; no Unity Editor import/runtime result is claimed. |

## DIFF-PRODUCT-CLI-EMPTY-DIRECTORY-001 - LoadAndProcess on an empty input directory

| Field | Finding |
|---|---|
| Status | Closed for the empty-directory load/process gate. The broader directory-inspection workflow remains open under `PRODUCT-CLI-INSPECT-001`. |
| Source location | `GameStructure.Load`; `GameStructure` constructor; `ExportHandler.LoadAndProcess`; `GameFileLoader.LoadAndProcess`. |
| Input evidence | `tools/probes/cli-inspect/CliInspectProbe.csproj` calls the frozen `ExportHandler.LoadAndProcess` with one empty directory using the isolated reference build. The probe input is created under Rust `target/`; no source or user data is read. |
| Reference behavior | Source returns a GameData whose GameBundle has zero asset collections. The LoadAndProcess collection gate skips `Process`, so the overridden processor-sequence factory is never requested. The probe emits `EMPTY_DIRECTORY|collections=0|processors_requested=0`. `GameStructure` logs its existing no-valid-assets warning. |
| Rust result | `mount_input_directories` now creates the selected VFS root even when no files were discovered. `inspect_directories` returns successfully with a no-valid-assets message and skips workspace loading/processing, matching the source empty-bundle outcome. |
| Regression evidence | Isolated source command (exit 0): `dotnet run --project tools/probes/cli-inspect/CliInspectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- target/cli-inspect-empty-probe-input`. Rust test `empty_directory_inspection_reports_unprocessed_empty_bundle` replays the frozen output row in `src/io/cli_inspect_fixture.txt` and asserts the CLI warning. Product command `cargo run --locked -- inspect target/cli-inspect-empty-probe-input` exits 0 with the expected message. |
| Remaining difference | This closes only the empty-directory outcome. Platform and mixed directory inventory, dependency loading, and all baseline GUI/export/settings workflows retain their own open or completed rows; full source product parity is not claimed. |

## DIFF-PRODUCT-CLI-PLATFORM-MIXED-001 - Platform and mixed directory union in CLI inspection

| Field | Finding |
|---|---|
| Status | Closed for preserving selected mixed-directory files alongside a detected platform structure and resolving a cross-file reference in their common workspace. Remaining CLI error/summary behavior and full product workflows stay open under `PRODUCT-CLI-INSPECT-001`. |
| Source location | `AssetRipper/Source/AssetRipper.Import/Platforms/PlatformChecker.cs`; `AssetRipper/Source/AssetRipper.Import/Structure/GameStructure.cs` (`InitializeGameCollection`); `AssetRipper/Source/AssetRipper.Export.UnityProjects/ExportHandler.cs` (`LoadAndProcess`). |
| Input evidence | `tools/probes/cli-inspect/CliInspectProbe.csproj` builds a synthetic Windows platform root containing `game.exe` and `game_Data/globalgamemanagers`, plus a separately selected mixed directory containing `main.assets`. Both SerializedFiles use the source-probed `SFHEX` bytes from `src/io/platform_probe_fixture.txt`; scratch inputs are created beneath Rust `target/`. |
| Reference behavior | The isolated source runtime retains `globalgamemanagers` and `main.assets` in `GameBundle.FetchAssetCollections()` and requests the processor sequence once. Its result is frozen as `PLATFORM_MIXED|input_names=globalgamemanagers,main.assets|processors_requested=1`. |
| Rust result | `inspect_directories_to` now scans selected paths left unconsumed by platform detection. `PlatformStructure::load_primary_serialized_workspace_with_additional_inputs` merges mixed SerializedFile inputs, registrations, and DataPaths with platform initial files and pre-expanded containers before one dependency walk. The regression gives the platform primary file an external reference to mixed `main.assets`; the report resolves that request as `AlreadyLoaded` in the same workspace. `inspect` reports both registration sets and all three collections (platform primary, platform container child, and mixed file). |
| Regression evidence | Isolated source command (exit 0): `dotnet run --project tools/probes/cli-inspect/CliInspectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- --platform-mixed src/io/platform_probe_fixture.txt target/cli-inspect-platform-mixed-replay`. Rust command `cargo test --workspace --locked platform_directory_inspection_unions_mixed_inputs_and_resolves_their_references -- --nocapture` passes; it checks the common-workspace dependency outcome and CLI summary against the frozen source row in `src/io/cli_inspect_fixture.txt`. Product command `cargo run --locked -- inspect target/cli-inspect-platform-mixed-replay/platform target/cli-inspect-platform-mixed-replay/mixed` exits 0 and reports both registrations and two SerializedFiles. |
| Remaining difference | This closes one Windows platform root plus one mixed directory and a resolved external reference. Other platform layouts, multiple platform selections, and malformed mixed inputs remain under their platform and directory-import rows; malformed container behavior remains under IO-BUNDLE-001/IO-STREAM-INPUT-001. Full export/settings/cancellation and GUI workflows remain under their product rows. No Unity Editor import/runtime behavior is claimed. |

## DIFF-PRODUCT-CLI-INPUT-GATES-001 - directory inspection input errors and empty-load behavior

| Field | Finding |
|---|---|
| Status | Closed for the Rust directory-inspection input gate, no-asset result, platform/mixed union, dependency summary, and structural inventory route. This does not close baseline processor mutations, project export, settings, or GUI behavior. |
| Source location | AssetRipper.GUI.Web.GameFileLoader.LoadAndProcess; AssetRipper.Export.UnityProjects.ExportHandler.LoadAndProcess; AssetRipper.Import.Structure.GameStructure.Load; AssetRipper.Import.Platforms.PlatformChecker.CheckPlatform; MixedGameStructure; GameBundle.FromPaths. |
| Input evidence | The isolated frozen-source probe tools/probes/cli-inspect generated only scratch directories under Rust target. Its --input-cases mode checks an empty path list, a missing directory, an empty directory, and a resource-only directory; its --platform-mixed mode creates a Windows platform root plus a selected mixed directory from src/io/platform_probe_fixture.txt. The stable LOAD and PLATFORM_MIXED rows are in src/io/cli_inspect_fixture.txt. No user bundle or reference source file was used as input. |
| Source behavior | ExportHandler.LoadAndProcess throws ArgumentException for no paths and fails on a missing directory. Empty and resource-only directories return a GameBundle with zero asset collections and never request the processor sequence. A Windows platform directory plus a mixed directory retains globalgamemanagers and main.assets and requests one processor pass. |
| Rust behavior | The inspect CLI rejects no arguments and nonexistent directories with nonzero diagnostics. Empty and resource-only directories return successfully with the no-valid-assets message and skip workspace processing. For valid directories, inspection mounts bundle inventory, discovers SerializedFiles, resolves reachable external references, merges selected mixed files with a recognized platform workspace, and prints registrations, collection, scene, and dependency summaries. Failed bundle entries are reported by the existing mount/inspection path. |
| Regression evidence | Fresh isolated source command: dotnet run --project tools/probes/cli-inspect/CliInspectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref\Source\0Bins\AssetRipper.GUI.Free\Release" -- --input-cases target/cli-inspect-input-cases-replay. The four selected LOAD rows matched src/io/cli_inspect_fixture.txt byte-for-byte. Rust tests: inspect_cli_rejects_empty_and_missing_directory_inputs; resource_only_directory_matches_the_source_no_asset_process_gate; empty_directory_inspection_reports_unprocessed_empty_bundle; directory_inspection_reproduces_the_reference_bundle_inventory; platform_directory_inspection_preserves_and_expands_registered_containers; platform_directory_inspection_unions_mixed_inputs_and_resolves_their_references. Actual cargo run --locked --quiet -- inspect invocations exit 1 for no arguments and missing directories, and exit 0 with the expected no-assets message for empty/resource-only directories. A fresh source platform/mixed probe and Rust product command both report both input registrations and two SerializedFiles. |
| Remaining comparison | The Rust CLI summary is a diagnostic surface with no baseline stdout equivalent; its structural values are checked against frozen source collections, input inventories, and dependency outcomes. The source processor sequence is probed only for its gate, not for processor mutations; those mutations remain under PROC-SCENE-001 and PROC-ANIM-001. Full exporter/settings/cancellation and browser workflows remain under EXPORT-UNITY-001, EXP-SETTINGS-APPLY-001, and PRODUCT-GUI-001. |

## DIFF-IO-RESOURCE-EXPORT-001 - Streamed resource resolution through project export

| Field | Finding |
|---|---|
| Status | Closed for the generic streamed-resource lookup and bounded-range path exercised through project export, including selected root ResourceFiles and an external provider/DataPath. Texture decoding and complete project-export parity remain open in their asset-family and product rows. |
| Source location | AssetRipper.Export.UnityProjects ProjectExporter and YamlStreamedAssetExporter; StreamingInfoExtensions; GameBundle.ResolveExternalResource; IResourceProvider; Rust src/export/workspace_yaml.rs, src/export/project_workflow.rs, src/resource.rs, and src/io/platform_structure.rs. |
| Input evidence | Synthetic Unity 2022.3.22f1 Texture2D class 28 with stream path texture.resS, offset 2, size 3; the resource bytes are 30 31 32 33 34 35. Rust product input uses a generated Windows player layout with the resource under game_Data. The isolated reference probe tests both a GameBundle root ResourceFile and a custom IResourceProvider backed by a generated scratch file. |
| Source behavior | The frozen source project exporter reads bytes 32 33 34 into Texture2D YAML, clears the path/offset/size fields in the export view, and restores the original generated object after serialization. In external mode, GameBundle.ResourceProvider receives exactly one request for texture.resS and returns the resource file; the output YAML has empty path and zero offset/size. |
| Rust behavior | Production export-project builds the platform resource provider and passes it to the workspace exporter. The streamed YAML route resolves class-28 bytes at offset 2/size 3, emits data 323334, clears stream metadata in the export-owned view, writes the Texture2D asset, NativeFormatImporter sidecar, and detected Unity project version, and leaves both input files unchanged. The same YAML preparation route covers Mesh and generated IImageTexture class IDs 28, 89, 117, 187, 188, and 189. The root-catalog project-export test and missing-resource fail-closed test remain passing. |
| Regression evidence | Source commands exited 0 using the isolated reference copy: streamed-image-project probe in default root-resource mode and with --external target/.../source-export target/.../input/texture.resS. External output rows are frozen in src/export/streamed_image_resource_fixture.txt. Rust command cargo test --workspace --locked export_project_resolves_streamed_texture2d_from_platform_data_path -- --nocapture passed; existing export_project_materializes_streamed_texture2d_and_fails_closed_when_resource_is_missing covers root input and missing-resource behavior. Full gates are recorded in docs/validation-log.md. |
| Remaining difference | No difference remains in the tested root/provider lookup, byte range, YAML stream clearing, and source-object restoration route. Other class-specific decoders/exporters, complete settings and product workflows, and Unity Editor import/runtime validation remain open under their dedicated rows; the installed Editor has no license. |

## DIFF-SHADER-PROJECT-001 - Shader and Material project collection integration

| Field | Finding |
|---|---|
| Status | Open. The covered source collection paths now have a Rust `export-project` route and source-runtime importer comparisons; complete Material fields and Unity import remain outstanding. |
| Source location | `ProjectExporter.Overrides` registration order; `SimpleShaderExporter`, `DummyShaderTextExporter`, `YamlShaderExporter`, `YamlShaderExportCollection`; `ShaderExportCollection.CreateImporter`; base `AssetExportCollection.GetExportID`; `ExportCollection.GetUniqueFileName`. |
| Source behavior | An isolated AssetRipper 2.0.0 runtime probe exported generated Shader objects for Dummy and YAML modes at Unity 5.0, 2017.1, 2017.2, 2017.4, 2018.1/2/3, 2019.4, and 2022.3. Simple decompiled text is written verbatim to `.shader` and uses `ShaderImporter`; YAML mode writes `.asset`, uses `NativeFormatImporter`, and writes `AvoidSavingYamlShaders.cs` from Unity 3.5 plus `YamlShaderPostprocessor.cs` from 2018.1. `name` is present through 2018.2; `mainObjectFileID` is present by 2017.1; `externalObjects` appears from 2017.2; and `nonModifiableTextures` appears from 2018.1. Base asset IDs are class-prefixed (`Shader` 4,800,000; `Material` 2,100,000); Windows-invalid slash and backslash in `Test/Simple` become underscores. Unset string importer fields serialize as empty YAML values rather than the token `null`. The Rust comparison fixture contains 18 normalized importer rows (nine Unity versions for each of Dummy and YAML). |
| Rust behavior | `project_workflow` passes decoded Shader state through the source registration stack and routes Simple/Dummy text to `.shader` with versioned ShaderImporter metadata, and YAML mode to `.asset` with versioned NativeFormatImporter metadata and deduplicated editor patch files. Parsed-form dummy rendering is wired into the project route. Asset and Shader YAML anchors, pointers, and importer IDs use class-prefixed IDs. `m_NonModifiableTextures` map entries resolve their PPtrs through the workspace graph and project export pointer map; null entries remain null, and unresolved or unmatched edges fail closed. Material YAML participates in an integrated Material→Shader GUID/fileID regression. |
| Regression evidence | `tools/probes/shader-project/Program.cs` ran against the isolated read-only-source copy at `%LOCALAPPDATA%\Temp\opencode\ar-ref`; build exited 0 with 0 warnings and 0 errors. Eighteen normalized `.meta` rows and source asset/support-file output rows are recorded in `src/export/shader_project_fixture.txt`. The 2018.2.21f1 class-48 TPK confirms the `map` / `pair(first:string, second:PPtr<Texture>)` serialized shape; the isolated source runtime emitted the null `_MainTex` importer row committed in `src/export/shader_nonmodifiable_fixture.txt`. `shader_and_yaml_shader_importers_match_reference_project_probe` compares every base importer sidecar row; `export_project_shader_importer_matches_the_reference_nonmodifiable_texture_map` compares the non-modifiable null-map row; `export_project_shader_importer_maps_nonmodifiable_texture_pointers` checks a resolved cross-file texture PPtr against its emitted GUID/fileID. The project probe build and focused tests are recorded in `docs/validation-log.md`. |
| Remaining difference | Material serialization is not complete for all versioned fields; source-runtime output was generated rather than Unity-produced; Unity Editor import/runtime was not run because the installed editor is unlicensed. Serialized class-48 TypeTree coverage remains partial. `RES-MATERIAL-EXPORT-001` and `EXPORT-UNITY-001` remain open. |

## DIFF-MATERIAL-PROJECT-2018-001 - Unity 2018.2 Material project YAML

| Field | Finding |
|---|---|
| Status | Matched for the 2018.2.21f1 Material release layout with both empty saved-property maps and a populated case containing one entry in each texture/float/color map. This bounded comparison does not close `RES-MATERIAL-EXPORT-001` or the broader Shader/Material project row. |
| Source location | `AssetRipper.Export.UnityProjects/Project/DefaultYamlExporter.cs`; the generated class-21 Material and `UnityPropertySheet` TypeTrees; project pointer conversion for the Material `m_Shader` field. |
| Input evidence | `SerFactoryProbe.dll tpk 21 2018.2.21f1` produced release and editor TypeTrees committed in `src/export/material_tpk_2018_fixture.txt`. Rust regressions build synthetic SerializedFiles from the release tree: one has empty maps, and the populated case has `_MainTex` with non-default scale/offset, `_Glossiness: 0.375`, and `_Color` RGBA `(0.125, 0.25, 0.5, 0.75)`, plus the same cross-file Shader PPtr and string tag. No user data is used. |
| Source behavior | The isolated AssetRipper runtime probe creates generated `Material_2018_2` and `Shader_2018_2` objects, exports into a fresh target directory, and emits two normalized `.mat` fixtures. The populated output in `src/export/material_project_properties_fixture.yaml` confirms dictionary-key rendering and all non-default values; the base output remains in `src/export/material_project_fixture.yaml`. Both contain root `serializedVersion: 6`, the three editor defaults, all release fields, nested `m_SavedProperties.serializedVersion: 3`, and a Shader pointer with class-prefixed fileID and `type: 3`. |
| Rust behavior | The TypeTree writer preserves node versions; Unity YAML conversion emits versioned nested-struct `serializedVersion`; the 2018.2 project Material body adds the source-runtime editor defaults; and the Material `m_Shader` export pointer uses Meta pointer type 3 while resolving its fileID/GUID from the Shader collection. The populated regression matches texture scale/offset, float and RGBA values, and all three string-keyed dictionaries. |
| Regression evidence | Fresh source runtime exports exited 0 and both normalized YAML fixtures matched exactly; the fresh class-21 TPK probe matched its committed TypeTree fixture after newline normalization. `export_project_material_yaml_matches_the_reference_2018_2_project_probe` and `export_project_material_saved_property_maps_match_the_reference_2018_2_project_probe` compare the emitted `.mat` against the source fixtures after normalizing only the generated Shader GUID. `export_project_dispatches_collections_and_writes_a_complete_unity_project` also passes with source-confirmed `type: 3`. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | The populated case has one entry per saved-property map. Multiple-entry ordering/duplicate-key behavior, other Material version layouts and fields, Unity-produced SerializedFiles, C# serialization round-trip, and Unity Editor import/runtime remain unverified. The Material row stays `已实现但未验证`. |

## DIFF-MATERIAL-PROJECT-2018-3-001 - Unity 2018.3 Material project YAML boundary

| Field | Finding |
|---|---|
| Status | Matched for the 2018.3.0f1 release layout with one populated entry in each saved-property map. This differential records the editor-field boundary from 2018.2 and does not close `RES-MATERIAL-EXPORT-001`. |
| Source location | `AssetRipper.Export.UnityProjects/Project/DefaultYamlExporter.cs`; generated class-21 `Material_2018_3`, class-48 `Shader_2018_3`, and their release TypeTrees; project Material editor-default serialization. |
| Input evidence | `SerFactoryProbe.dll tpk 21 2018.3.0f1` produced the release/editor trees committed in `src/export/material_tpk_2018_3_fixture.txt`. The Rust regression constructs a synthetic SerializedFile from the release tree with the same scalar/tag fields and one `_MainTex`, `_Glossiness`, and `_Color` entry used by the 2018.3 source runtime. The Material Shader PPtr resolves across files. No user data is used. |
| Source behavior | An isolated source runtime probe creates generated 2018.3 Material/Shader objects and exports `src/export/material_project_2018_3_fixture.yaml`. Relative to the 2018.2 output, the source editor block includes `m_PrefabInstance` and `m_PrefabAsset` instead of `m_PrefabInternal`; the serialized release fields and populated saved-property values match the 2018.2 case. |
| Rust behavior | Project Material export selects the source-observed 2018.3 editor fields by Unity version and emits their null PPtrs in source order. It preserves the 2018.2 branch separately. The TypeTree decoder and saved-property YAML conversion produce the same scalar, string-tag, texture-environment, float, and RGBA values as the source runtime. |
| Regression evidence | `export_project_material_yaml_matches_the_reference_2018_3_project_probe` passes against the source fixture after normalizing only the generated Shader GUID. Fresh source runtime runs matched all three Material fixtures (2018.2 empty, 2018.2 populated, 2018.3 populated), and a fresh TPK probe matched the committed 2018.3 TypeTree fixture. The isolated probe project build succeeded with 0 warnings and 0 errors. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | This is one 2018.3 layout with one entry per map. Other Unity-version editor fields/layouts, multiple-entry ordering/duplicate behavior, Unity-produced SerializedFiles, C# serialization round-trip, and Unity Editor import/runtime remain unverified. `RES-MATERIAL-EXPORT-001` remains `已实现但未验证`. |

## DIFF-MATERIAL-PROJECT-2017-4-001 - Unity 2017.4 Material project YAML boundary

| Field | Finding |
|---|---|
| Status | Matched for the 2017.4.40f1 release layout with one populated entry in each saved-property map. This adds the pre-2018 editor-default shape; it does not close `RES-MATERIAL-EXPORT-001`. |
| Source location | Generated class-21 `Material_2017_1_0_b9`, class-48 `Shader_2017_3`, their release TypeTrees, and `AssetRipper.Export.UnityProjects.Project.DefaultYamlExporter`. |
| Input evidence | `SerFactoryProbe.dll dispatch 2017.4.40f1 21 48` selects `Material_2017_1_0_b9` and `Shader_2017_3`; both have TPK layouts. `SerFactoryProbe.dll tpk 21 2017.4.40f1` is committed in `src/export/material_tpk_2017_4_fixture.txt`. Rust constructs a synthetic SerializedFile with the release fields, one cross-file Shader PPtr, string tag, and one entry each for texture, float, and color maps. |
| Source behavior | The isolated source project probe uses the factory-selected generated classes and emits `src/export/material_project_2017_4_fixture.yaml`. Its editor block contains `m_ObjectHideFlags`, `m_PrefabParentObject`, and `m_PrefabInternal`; `m_DoubleSidedGI` is retained along with the distinctive saved-property values. |
| Rust behavior | Project Material export selects the observed 2017.4 editor fields by Unity version, preceding the existing 2018.2 and 2018.3 field sets. It retains the TPK `m_DoubleSidedGI` value and serializes all three populated maps and the mapped Shader pointer. |
| Regression evidence | `export_project_material_yaml_matches_the_reference_2017_4_project_probe` passes against the source fixture after normalizing only the generated Shader GUID. Fresh source runtime and TPK invocations matched their committed fixtures; factory dispatch confirmed the generated class names. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | This is one 2017.4 layout with one entry per map. Multiple-entry ordering/duplicate behavior, other Unity-version layouts and fields, Unity-produced SerializedFiles, C# serialization round-trip, and Unity Editor import/runtime remain unverified. `RES-MATERIAL-EXPORT-001` remains `已实现但未验证`. |

## DIFF-MATERIAL-PROJECT-2018-MAPS-001 - Material saved-property order and duplicate keys

| Field | Finding |
|---|---|
| Status | Matched for the 2018.2.21f1 string-keyed texture, float, and color maps with two entries each, both for distinct keys and repeated keys. This closes only these source-YAML cases. |
| Source location | Generated class-21 release TypeTree `m_SavedProperties.m_TexEnvs`, `m_Floats`, and `m_Colors`; `DefaultYamlExporter` and `YamlWalker` dictionary/pair traversal. |
| Input evidence | The Rust tests construct class-21 SerializedFile payloads from the committed 2018.2.21f1 release TPK. The distinct-key case uses deliberately non-alphabetical insertion order and different values; the duplicate case repeats each map's first key with a different value. All keys, counts, PPtrs, and floats are serialized in the source TypeTree order. |
| Source behavior | Isolated source-runtime probes add two pairs to each generated `AssetDictionary`. The unique-key fixture (`src/export/material_project_multi_fixture.yaml`) retains insertion order rather than sorting keys. The duplicate fixture (`src/export/material_project_duplicate_fixture.yaml`) writes both rows with the same key and retains both distinct values; it does not deduplicate or silently overwrite either pair. |
| Rust behavior | The workspace TypeTree decoder retains map pair order and duplicate keys through project YAML construction. The emitted `.mat` maps match the reference outputs byte-for-byte after normalizing only the generated Shader GUID. |
| Regression evidence | Fresh source runtime exports matched both committed fixtures. `export_project_material_multiple_saved_property_entries_preserve_source_order` checks two distinct entries in all three maps; `export_project_material_duplicate_saved_property_keys_match_reference_yaml` checks repeated keys and both values. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | These are two-entry 2018.2 maps with float/color/string values and null texture pointers. Larger maps, non-null texture PPtrs within saved properties, other key/value TypeTree shapes and Unity versions, and Unity Editor import/runtime remain unverified. `RES-MATERIAL-EXPORT-001` remains `已实现但未验证`. |

## DIFF-MATERIAL-PROJECT-2019-4-001 - Material project export at Unity 2019.4

| Field | Finding |
|---|---|
| Status | Matched for the factory-selected 2019.4.40f1 Material/Shader classes and the serialized Material fields in the captured source YAML. |
| Source location | `GameAssetFactory` class dispatch and class-21/class-48 TPK TypeTrees; `DefaultYamlExporter`/`YamlWalker`; project Material and Shader collections. |
| Input evidence | `dispatch 2019.4.40f1 21 48` selects `Material_2018_3` and `Shader_2019_3_0_b0`, with TPK for both. The committed class-21 release/editor TypeTree is `src/export/material_tpk_2019_4_fixture.txt`; a fresh class-48 TPK replay carries `m_ParsedForm`, and the generated 2019 shader properties are captured in `src/export/shader_parsed_fixture.txt`. |
| Source behavior | An isolated source-runtime project probe constructs the selected generated classes, sets non-default Material fields and one populated entry per texture/float/color map, and exports `.mat`. Its normalized YAML is `src/export/material_project_2019_4_fixture.yaml`, byte-identical to the 2018.3 YAML fixture. The editor block contains `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset`. |
| Rust behavior | The first exact comparison failed because `value_yaml` selected editor fields only for the exact 2018.3 tuple. The source TPK confirmed the same field group at 2019.4; Rust now selects it for `(2019, 4)` and the exact `.mat` comparison passes after normalizing only the generated Shader GUID. |
| Regression evidence | `export_project_material_yaml_matches_the_reference_2019_4_project_probe` constructs a class-21 payload from the committed source TPK and checks the full project YAML and mapped cross-file Shader PPtr. Fresh source dispatch, TPK, and project-runtime replay matched their fixtures. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | This verifies the 2019.4 layout only. 2020+ Material TypeTrees/field branches, larger maps, non-null saved-property texture PPtrs, Unity-produced SerializedFiles, and Unity Editor import/runtime remain open. `RES-MATERIAL-EXPORT-001` remains `已实现但未验证`. |
## DIFF-MATERIAL-PROJECT-2019-TEXTURE-001 - Non-null Material saved texture PPtr across files

| Field | Finding |
|---|---|
| Status | Matched for the 2019.4.40f1 `_MainTex.m_Texture` pointer to a separate generated Texture2D collection and its Unity project export identity. |
| Source location | `UnityTexEnv_5.Texture`/`IPPtr.SetAsset`; `GameBundle` processed collections; Material YAML and Texture2D project collections/export IDs. |
| Input evidence | Source factory dispatch selects `Texture2D_2019_4_9`. The isolated source probe places Material/Shader and a 1x1 RGBA32 Texture2D in separate processed collections, then assigns the Texture2D through the saved-property PPtr. |
| Source behavior | The source `.mat` emits `m_Texture: {fileID: 2800000, guid: <TEXTURE_GUID>, type: 2}` and exports the referenced Texture2D sidecar. `src/export/material_project_2019_4_texture_ref_fixture.yaml` normalizes only the generated Shader and Texture2D GUIDs. |
| Rust behavior | A 2019.4 Material SerializedFile built from the source release TPK references Shader PathID 42 through external fileID 1 and Texture2D PathID 88 through external fileID 2. Rust resolves both targets and emits the saved texture PPtr with the same generated fileID/type and the GUID from `TextureFixture.texture2D.meta`. |
| Regression evidence | `export_project_material_resolves_saved_texture_pointer_to_exported_texture_guid` checks the full source `.mat` after GUID normalization and verifies the Texture2D asset/sidecar exist. The fresh source replay matched the committed fixture. Full gates and counts are in `docs/validation-log.md`. |
| Remaining difference | One generated 2019.4 Texture2D target and one string-keyed map entry are covered. Other Unity versions/key layouts, multiple non-null texture PPtrs, Unity-produced SerializedFiles, and Unity Editor import/runtime remain open. `RES-MATERIAL-EXPORT-001` remains `已实现但未验证`. |

## DIFF-MATERIAL-PROJECT-2022-UNITY6-001 — late Material editor TypeTrees and project YAML

| Field | Finding |
|---|---|
| Status | Matched for Unity 2022.3.22f1 and Unity 6 6000.4.0f1 using the factory-selected Material and Shader classes; the broader Material/export feature remains open. |
| Source location | `GameAssetFactory` version dispatch; generated class-21/class-48 release and editor TPK TypeTrees; `DefaultYamlExporter` Material serialization and `YamlWalker` collection traversal. |
| Input evidence | Fresh source dispatch selects `Material_2022_1_0_b14`/`Shader_2022_3_20` at 2022.3.22f1 and `Material_2023_2_8`/`Shader_6000_3_0_b6` at 6000.4.0f1. Full class-21 and class-48 TPK output is committed in `src/export/material_tpk_2022_3_fixture.txt` and `src/export/material_tpk_6000_4_fixture.txt`. The source project probe exports a split valid/invalid keyword set and one saved entry for texture, float, and color, plus the source-preserved empty integer map. |
| Source behavior | Both release TypeTrees use Material `serializedVersion: 8`, `m_ValidKeywords`, `m_InvalidKeywords`, `m_Ints`, and `m_BuildTextureStacks`. The editor TypeTrees add `m_Parent`, `m_ModifiedSerializedProperties`, and `m_LockedProperties` among the shared Prefab fields. `m_AllowLocking` is absent at 2022.3 and present at 6000.4. The normalized `.mat` fixtures are `src/export/material_project_2022_3_fixture.yaml` and `src/export/material_project_6000_4_fixture.yaml`; only the generated Shader GUID is normalized. |
| Rust behavior | `value_yaml` now adds the observed editor-only Material fields at their TypeTree positions for the exact 2022.3 and 6000.4 version tuples, with null PPtr, zero unsigned, empty string, and false defaults. The source-TPK test payload writer follows each release tree's split keyword vectors, integer map, and texture-stack vector. |
| Regression evidence | `export_project_material_yaml_matches_the_reference_2022_3_project_probe` and `export_project_material_yaml_matches_the_reference_unity6_project_probe` compare the complete project `.mat` text after normalizing only the generated Shader GUID. Fresh dispatch, class-21/class-48 TPK, and `.mat` replays matched both committed fixtures. Focused Material comparisons pass 10/10; full workspace tests, warnings-denied Clippy, formatting, diff check, and the 1,449-file frozen-baseline verifier pass as recorded in `docs/validation-log.md`. The Rust inputs are synthetic SerializedFiles built from source TypeTrees, not Unity-produced files. |
| Remaining difference | This evidence covers exactly 2022.3.22f1 and 6000.4.0f1. Other late-version minor branches, larger maps, additional non-null texture PPtrs, Unity-produced payloads, and Unity Editor import/runtime remain open. `RES-MATERIAL-EXPORT-001` and `EXPORT-UNITY-001` remain open. |

## DIFF-TT-TPK-EDITOR-MODE-001 — TPK TypeTreeObject read mode in an editor scene

| Field | Finding |
|---|---|
| Status | Matched for a source-authored Unity 2019.4.40f1 no-TypeTree class-194 object in a `NoTarget` `.unity` file. The broader TypeTree recovery and asset-factory rows remain open. |
| Source location | `GameAssetFactory.ReadAsset` / `ReadNormalObject` / `CreateAsset`; `TypeTreeObject.ReadRelease` / `ReadEditor` and `ConvertFields`; `SerializedFile.Flags`; `TransferInstructionFlagsExtensions.IsRelease`; `TypeTreeNodeStruct.TryMakeFromTpk`. |
| Input evidence | The isolated factory probe selects class ID 194 as `GEN|194|MISSING`, `TPK|194|yes` at 2019.4.40f1. `tpk_navmesh_editor` has target platform `NoTarget` (`0xFFFFFFFE`), file path `navmesh.unity`, and no serialized file TypeTree. The source release tree has 5 nodes; the editor tree has 12. |
| Source behavior | `NoTarget` plus a `.unity` path sets editor-minimal-scene flags, so the fallback `DoubleTypeTreeObject` reads the editor tree. The source materializes `m_ObjectHideFlags=3`, null prefab pointers, and `m_Name=SceneNavMesh`. Its `WriteEditor` bytes equal the authored object bytes; `WriteRelease` converts to the release layout and emits only the name string. Ordinary game files continue to use the release layout. |
| Rust behavior | `class_layout` now selects the source TPK tree from the serialized file's actual read flags for both generated classes and TPK-only `TypeTreeObject`s, including the MonoBehaviour header. Missing-file-TypeTree recovery is shared by generated and TPK-only non-MonoBehaviour classes, and both `ObjectWorkspace` and the reference graph use it. Unsupported class/version cases and MonoBehaviour assembly-backed structures remain explicit decode gaps. |
| Regression evidence | The refreshed `src/processing/asset_factory_fixture.txt` retains every previous source row and adds the NoTarget scene plus source `EDITOR_HEX` / `RELEASE_HEX`. `factory_differential_replays_every_reference_case` and `object_workspace_recovers_editor_tpk_tree_object_without_file_typetree` pass. Full tests, warnings-denied Clippy, format, diff check, and the frozen-baseline verifier pass in `docs/validation-log.md`. A Rust `export-project` CLI replay completed and correctly reported this input class as non-exportable with zero asset files; that CLI run was not a source exporter differential. |
| Remaining difference | Only the 2019.4 class-194 branch is covered here. Generated-class/TypeTreeObject layouts across all baseline versions, MonoBehaviour and assembly/type recovery, unreadable/unknown/raw paths, and full project exporter parity remain open. `SER-TT-ABSENT-001` and `SER-ASSET-FACTORY-001` stay open. |

## DIFF-TT-TPK-COMPLETE-001 — Complete source TPK database and version selection

| Field | Finding |
|---|---|
| Status | The embedded Rust projection matches the frozen source TPK tables and `TryMakeFromTpk` output signatures for every captured class/version query. This does not close generated-class materialization or the broader no-TypeTree factory rows. |
| Source location | `SourceTpk.GetStream`; `TpkTypeTreeBlob`; `TpkClassInformation.Classes`; `TypeTreeNodeStruct.TryMakeFromTpk`; `GetItemForVersion`. |
| Input evidence | The isolated source probe recorded the raw `SourceTpk.GetStream()` bytes (207,864 bytes; SHA-256 `B5BD478FD590B76362D09349EBC9BA28DC126A6A8460BD19B4CC193CCE1CB894`) and projected 403 class IDs, 1,420 source versions, 5,098 class/version boundaries, 98 null entries, 26,516 nodes, and 7,363 strings. The normalized data is committed in `src/processing/source_tpk_fixture.txt`; the independent source selection matrix is `src/processing/source_tpk_selection_fixture.txt`. |
| Source behavior | Selection queries cover the before-first version, each class boundary, the prior source version at each boundary, and the latest source version. The captured matrix contains 7,304 cases: 7,240 successful release/editor tree pairs and 64 source exceptions. Successful trees are compared by flattened node count and FNV64 signature over depth, type/name, node version, and metadata flags. |
| Rust behavior | `src/processing/source_tpk.rs` parses and validates the complete offline projection, checks packed Unity version ordering and table references, reconstructs release/editor TypeTrees lazily, and caches them. The source version selector preserves the observed first-entry and null-entry fallback behavior; missing roots and invalid data return contextual lookup errors. Factory and workspace recovery use this source-backed TPK lookup without a runtime AssetRipper or .NET dependency. |
| Regression evidence | `cargo test --locked --lib source_tpk -- --nocapture` exercises all 7,304 source selection cases plus projection integrity, version fallback, and factory integration. Focused factory and ObjectWorkspace source regressions also pass. Full-workspace results and static checks are recorded in `docs/validation-log.md`. |
| Remaining difference | The complete TPK layout database is now available, but generated class dispatch/materialization, MonoBehaviour assembly/type recovery, unknown/unreadable/raw behavior, and end-to-end export remain incomplete. `SER-TT-ABSENT-001` and `SER-ASSET-FACTORY-001` remain open; the Phase 1 gate is not passed. |

## DIFF-SER-FACTORY-DISPATCH-MATRIX-001 — Versioned generated-class dispatch

| Field | Finding |
|---|---|
| Status | Rust matches the frozen `AssetFactory.CreateSerialized` outcome over the expanded class/version query domain, including generated type names, null results, and abstract-class exceptions. The object-reader and no-TypeTree rows remain open. |
| Source location | `AssetRipper.AssemblyDumper/Passes/Pass940_MakeAssetFactory.cs`; `GameAssetFactory.CreateAsset`; each `TpkClassInformation.Classes` version boundary. |
| Input evidence | The isolated `ser-factory dispatch-matrix` probe queried 404 class IDs at 1,838 versions: the before-first `0.0.0f0`, all 1,420 package-wide TPK versions, and 417 distinct per-class boundaries omitted by that global list. This is 742,552 source calls. The run-length matrix has 3,508 transitions, 37 `AbstractClassException` outcomes, and two `MISSING` transitions. Fixture SHA-256: `CA4E67C5853F9E5F436B808E80819EC69647792002172F1420FD2201640A6035`. |
| Source behavior | `Pass940_MakeAssetFactory` emits version-floor checks from each generated class group's own version range. The old probe queried only the package-wide version list and missed class 1 changing to `GameObject_4` at `4.0.0a0`; class 194 becomes `MISSING` at `5.0.0a0`. The source matrix now samples those exact per-class boundaries as well as the shared catalog. |
| Rust behavior | `src/processing/source_factory_dispatch.rs` validates the expanded matrix and resolves each class by source version floor. `ReferenceClassLayouts` uses this table before the complete TPK provider; source throws remain explicit diagnostics, while source null results continue through `CreateAsset`'s TPK/unknown fallback. The product has no .NET runtime dependency. |
| Regression evidence | `source_generated_factory_matrix_is_complete_and_retains_non_success_outcomes` validates the matrix dimensions and outcome counts. `every_source_generated_factory_query_matches_the_rle_source_matrix` replays all 742,552 queries. `factory_differential_replays_every_reference_case` and `object_workspace_uses_generated_class_at_the_alpha_tpk_boundary` exercise the corrected choice through the production factory and ObjectWorkspace. Full checks are recorded in `docs/validation-log.md`. |
| Remaining difference | No class-selection difference remains in the captured source query domain. Per-class non-default field semantics, assembly-backed MonoBehaviour recovery, and complete export remain open under `SER-ASSET-FACTORY-001`, `SER-TT-ABSENT-001`, and the export rows. |

## DIFF-SER-FACTORY-GENERATED-PAYLOADS-001 — Generated release/editor payload layouts

| Field | Finding |
|---|---|
| Status | Rust's TypeTree decoder fully consumes each reference-generated release/editor default payload at every captured non-null TPK class/version boundary. This is broad layout evidence, not full generated-object parity. |
| Source location | Generated class `WriteRelease`/`WriteEditor`; `SourceTpk` class histories; `GameAssetFactory` class construction. |
| Input evidence | The isolated `ser-factory payload-matrix` probe used the frozen AssetRipper runtime (.NET SDK 10.0.401) to write both modes at 4,461 class/version boundaries: 8,922 payload records, 1,482,552 total serialized bytes, 539 source-missing/exception skips, and zero writer failures. The committed source bytes are `src/processing/source_factory_payload_fixture.txt`. |
| Source behavior | The generated serializers write version-selected release and editor field layouts; the TPK history records those class/version layout boundaries. The source class factory can miss or throw for some class/version pairs, which the existing dispatch matrix records independently. |
| Rust behavior | `source_generated_payloads_decode_against_all_selected_tpk_boundaries` checks source factory class names, selects the corresponding release/editor TPK tree, and decodes every byte with no trailing payload. `ObjectWorkspace` also replays a source-authored no-TypeTree GameObject at the previously missed `4.0.0a0` boundary and selects `GameObject_4`. |
| Regression evidence | The 8,922-row fixture is checked in under `src/processing/source_factory_payload_fixture.txt`; the source-authored alpha-boundary SerializedFile is included in `src/processing/asset_factory_fixture.txt`. Focused test commands and full workspace gates are recorded in `docs/validation-log.md`. |
| Remaining difference | The payloads are generated default objects. They do not prove arbitrary non-default field values, specialized per-class read side effects, all TypeTreeObject release/editor conversion behavior, assembly-backed MonoBehaviour structures, or Unity import/runtime behavior. `SER-ASSET-FACTORY-001` and `SER-TT-ABSENT-001` remain open. |

## DIFF-UNITYFS-METADATA-ALIGNMENT-SLACK-001 — Honor the declared UnityFS metadata extent

| Field | Finding |
|---|---|
| Status | The source-runtime probe confirms a baseline read-position bug in the accepted v7 four-byte metadata-alignment-slack case. Rust starts entry reading after the full declared metadata extent and returns the intact entry. |
| Source location | `FileStreamBundleFile.ReadFileStreamMetadata`; `ReadMetadata` alignment-slack acceptance; `ReadFileStreamData`; `BundleFileBlockReader.ReadEntry` (source comment references issue `#1470`). |
| Input evidence | The isolated `io-differential --unityfs-alignment-slack` probe reads a C#-built 155-byte UnityFS v7 file through the shipped source `SchemeReader`. Fixture `src/container/unityfs_alignment_slack_fixture.txt` records input SHA-256 `5EA1339936493444E074FDD4C2129C063109D274E2F8B444BF06AB0BAB46CB65`, the complete input, source tree, and both resource byte sequences. |
| Source behavior | Metadata begins at offset 64. Its combined structures consume 69 bytes while both header metadata sizes declare 72. The source accepts this because rounding 69 up to four bytes yields 72, but does not advance over the three padding bytes. Since metadata is at the start and no later data alignment flag is set, the block reader starts at offset 133: the resource becomes three zero bytes followed by the first 16 payload bytes, losing the final three bytes. |
| Rust behavior | `UnityFsBundle::parse` starts the block stream at offset 136, the end of the declared metadata region. The production `SchemeReader` dispatch returns all 19 original entry bytes. |
| Classification | This is a source bug, not the desired serialization semantics: the accepted slack is part of the declared metadata extent, and treating it as entry bytes shifts the directory's block-relative ranges and truncates a valid entry. Rust keeps the header's declared range and is covered by a source-built fixture regression. |
| Regression evidence | `production_dispatch_skips_reference_metadata_slack_before_entry_data` replays the exact reference-probed bytes through `SchemeReader`, compares the captured source output to the Rust output, and pins the full payload. The focused UnityFS suite and full workspace results are recorded in `docs/validation-log.md`. |
| Remaining difference | No unresolved difference remains for this accepted alignment-slack case. The broader Phase 1 gate and Unity-produced bundle/Unity Editor validation remain open. |

## DIFF-PREFAB-GENERATED-LEGACY-001 — Generated pre-2018.3 Prefab project export

| Field | Finding |
|---|---|
| Status | No YAML or importer-semantic difference remains for the synthetic Unity 2017.4.40f1 generated-Prefab hierarchy. The source `.prefab` and Rust `.prefab` are byte-identical. The source and Rust `.meta` files differ only in their newly generated GUID and `timeCreated`; after excluding those two values, the importer metadata is identical and retains `mainObjectFileID: 100100000`. |
| Source location | `Source/AssetRipper.Export.UnityProjects/Project/PrefabExportCollection.cs`; `Source/AssetRipper.Processing/Prefabs/PrefabProcessor.cs`; `Source/AssetRipper.Export.UnityProjects/YamlWalker.cs`; `Source/AssetRipper.Export.UnityProjects/Project/NativeFormatImporter` creation. |
| Input evidence | The isolated source `legacy-export` probe used a synthetic 336-byte 2017.4.40f1 SerializedFile with a GameObject and Transform. Input SHA-256: `5AE9CF939995BDE16A4933207C8EA02B66D7B40602820B0014E1ABF892E58F01`. Probe output is `src/export/legacy_prefab_source_2017_4_probe_fixture.txt`; the source YAML is `src/export/legacy_prefab_source_2017_4_fixture.yaml`. |
| Source behavior | `ExportHandler.LoadAndProcess` creates a generated legacy `Prefab` asset and a hierarchy rooted at the generated GameObject. The source emits `Assets/GameObject/LegacyRoot.prefab` with GameObject and Transform `m_PrefabInternal` references to the Prefab main document and a `Prefab` class-ID-1001 document whose `m_RootGameObject` points back to the root. The sidecar uses `NativeFormatImporter` and main file ID `100100000`. |
| Rust behavior | `export-project` selects the legacy marker before Unity 2018.3, constructs class ID 1001 from the frozen editor TPK TypeTree, encodes the generated value against that schema, maps the root PPtr through the project pointer map, and overlays the generated Prefab pointer on hierarchy members. Generated-class field names and root serialized-version emission follow the source YAML walker. The CLI writes the same asset path and importer main file ID. |
| Regression evidence | `project_export_matches_source_for_a_generated_legacy_prefab` compares the full output to the source fixture. Both YAML hashes are `407CD0C7AD68018E0188D8C96820DE475CD19B89B0E4C2EAB6BEAAC893C53873`. Adjacent project-workflow tests pass 6/6, scene-wiring tests 6/6 with one probe test ignored, workspace-YAML tests 21/21, and value-YAML tests 38/38. Commands and results are in `docs/validation-log.md`. |
| Remaining difference | Evidence covers this synthetic hierarchy and Unity version only. Other pre-2018.3 versions, nested or overridden Prefabs, AssetBundle-derived importer names, Unity-produced inputs, and Unity Editor import remain open. This does not close `EXPORT-UNITY-001`. |

## DIFF-SER-FACTORY-TEXTURE-TAIL-ENDIAN-001 — Texture2D extra fields use serialized body endian

| Field | Finding |
|---|---|
| Status | Fixed and source-matched for both serialized body endians in the synthetic Unity 2019.4.40f1 fixture. Rust previously decoded these words as little-endian even when the SerializedFile body was big-endian. |
| Source location | `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs`: `TryReadNormalObject` recognizes exactly 24 remaining bytes for Texture2D and calls `ReadExtraTextureFields`; the `EndianSpanReader` uses the owning collection's endian. |
| Input evidence | `src/processing/asset_factory_fixture.txt` includes `texture_chinese_tail_le` (224 bytes, SHA-256 `AB45DC0B722570B07B286A811A893F8F42711F1026F9AFCD240EEC1C9E32C2B8`) and `texture_chinese_tail_be` (224 bytes, SHA-256 `26DB551986715EF460AC10221505FB104028BE5AA393E5A803A0E77E85F7C5B9`). Each base payload is source-generated class-28 release serialization with a manually appended 24-byte tail encoding width 2048, height 1024, and GUID words `11223344,55667788,90ABCDEF,01234567` in the matching endian. |
| Source behavior | The isolated AssetRipper factory reads both byte orders, retains the same dimensions/GUID words, and logs `Texture Logo had an extra 24 bytes, which were assumed to be non-standard Chinese fields.` |
| Rust behavior | `parse_chinese_texture_fields` now decodes each 32-bit word using `SerializedFile.metadata.prefix.body_endian`; workspace factory state retains the fields and considers the recognized suffix complete. |
| Regression evidence | `factory_differential_replays_every_reference_case` compares both source-authored fixture rows, warning text, fields, trailing-byte classification, and workspace materialization. The generated source probe output matches all 256 committed fixture lines. Focused Rust test passed 1/1. |
| Remaining difference | The two source-authored endianness fixtures validate this fixed 24-byte branch, not real-world Chinese Unity bundles or all Texture2D class/version layouts. `SER-ASSET-FACTORY-001` and full Texture2D exporter coverage remain open. |

## DIFF-SER-FACTORY-ENGINE-SHADER-001 — Engine-collection Shader failure handling

| Field | Finding |
|---|---|
| Status | Source-matched for the four frozen default-resource and builtin-extra collection names using a synthetic 2019.4.40f1 Shader with a non-zero invalid tail. The generated Shader is retained and the source emits an Info-level engine-type-tree message. |
| Source location | `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs`: `ReadNormalObject`; `Source/AssetRipper.IO.Files/SpecialFileNames.cs`: `IsDefaultResourceOrBuiltinExtra`. |
| Input evidence | `src/processing/asset_factory_fixture.txt` includes four 168-byte SerializedFiles, one for each of `unity default resources`, `unity_default_resources`, `unity builtin extra`, and `unity_builtin_extra`. All share SHA-256 `BE57A1047909BB58A9AD066B29176A1E36BDDE8EECA6670C3FBAEC735FB49BCC`; the differing collection names are supplied by the synthetic file paths. Each carries a generated class-48 release Shader payload followed by non-zero bytes. |
| Source behavior | For all four aliases, AssetRipper returns generated type `Shader_2019_3_0_b0` and logs `Shader with PathID 42 in <collection> uses engine type trees.` at Info level instead of converting the object to `UnreadableObject`. |
| Rust behavior | `is_engine_collection_name` recognizes the four exact aliases. `read_normal_object` retains the generated Shader, classifies the invalid tail, and records `FactoryLogLevel::Info` without an error message. `ObjectWorkspace` preserves the object as incomplete because the malformed tail is still present. |
| Regression evidence | `factory_differential_replays_every_reference_case` compares all four source Info messages, generated type/class/name, tail classification, and incomplete workspace state. The source probe output matches all 272 committed fixture lines. |
| Remaining difference | This validates the synthetic failed-read branch only. Valid engine-resource Shader field loading and actual built-in TypeTree hydration/export remain open under `SER-ASSET-FACTORY-001` and `RES-SHADER-PARSED-001`. |

## DIFF-SER-FACTORY-PATCH-RETRY-001 — Patch-version generated-object retry

| Field | Finding |
|---|---|
| Status | Source-matched for a synthetic class-108 Light payload whose old Patch-version layout fails but whose single incremented-build release retry succeeds. |
| Source location | `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs`: `ReadNormalObject` retries only after the initial read error when the collection version type is Patch; `TryReadNormalObject` creates and reads the selected class layout. |
| Input evidence | `src/processing/asset_factory_fixture.txt` contains a 248-byte SerializedFile tagged `2017.1.0p0`, SHA-256 `F3F8E6CDDA84B1300B98C7CB7480A67C91376B0236686F1756AD973C7B6B4480`. Its class-108 Light object was written from the generated `2017.1.1f0` layout. |
| Source behavior | Direct source dispatch selects `Light_2017` for `2017.1.0p0` and `Light_2017_1_0_p4` for the bumped `2017.1.1f0`. The old release TypeTree expects the 13-float `m_FalloffTable`; the bumped release layout omits that field. The factory returns `Light_2017_1_0_p4` with no error log, showing the first read failed and the retry succeeded. |
| Rust behavior | The factory first attempts the input version, retries with `FactoryUnityVersion::with_bumped_build`, selects the bumped TPK layout, returns generated type `Light_2017_1_0_p4`, and materializes the object fully through `ObjectWorkspace` with an exact tail. |
| Regression evidence | `factory_differential_replays_every_reference_case` compares the source-authored bytes and resulting type; the case-specific regression checks the original Patch signature, retry type, absent `m_FalloffTable`, exact payload tail, and complete ObjectWorkspace state. The fresh source probe output matches all 275 fixture lines. |
| Remaining difference | This is one synthetic StandaloneWin64Player release-mode class/version boundary. Other Patch retry boundaries, editor-mode behavior, Unity-produced assets, and broad generated-object semantics remain open under `SER-ASSET-FACTORY-001`. |

## DIFF-SER-FACTORY-PATCH-RETRY-FAILURE-001 — Preserve initial failure after Patch retry

| Field | Finding |
|---|---|
| Status | The source-matched both-attempts-failed fallback exposed and fixed a Rust diagnostic-source mismatch. The unreadable outcome, generated fallback name, raw bytes, and original Patch context now agree. |
| Source location | `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs`: after the build-bump retry also fails, `ReadNormalObject` logs the first attempt's `error`, constructs `UnreadableObject` from the original asset identity and bytes, and copies the original `INamed.Name` if present. |
| Input evidence | `src/processing/asset_factory_fixture.txt` contains a 248-byte class-108 Light SerializedFile tagged `2017.1.0p0`, SHA-256 `3019C7CA4E0882EC0363758658FDF0B0E63AAE252F7CE1A9E91D3E711214CC56`. Its `2017.1.1f0` generated payload is truncated by one byte so both the old and bumped layouts fail. |
| Source behavior | AssetRipper returns `UnreadableObject`, name `UnreadableLight_C446EAB8`, preserves the 151-byte raw payload, and logs the initial `2017.1.0p0` error. Its diagnostic includes `System.ArgumentOutOfRangeException` from the old `FalloffTable` reader. |
| Rust behavior | Rust previously used the retry attempt's `2017.1.1f0` diagnostic. `read_normal_object` now passes the first failed `NormalAttempt` to `unreadable_from_failed` when retry also fails. Rust returns the same unreadable name/type and raw bytes and preserves the original version/platform/collection/path context. |
| Regression evidence | `factory_differential_replays_every_reference_case` asserts the original version context in both logs, source exception type/message, Rust truncated-input diagnostic, unreadable name/type/raw length, and incomplete ObjectWorkspace state. Source stack frames are omitted from the text fixture as runtime-specific; exception type/message and input context remain. The focused regression passes 1/1. |
| Remaining difference | The .NET reference reports an `ArgumentOutOfRangeException` and Rust reports its structured truncated-input decode error. These exception details differ by implementation; both identify the same initial Patch read and retain the same input context. Other Patch retry failures and broad factory behavior remain open under `SER-ASSET-FACTORY-001`. |

## DIFF-SER-FACTORY-PATCH-RETRY-EDITOR-001 — Patch retry with editor TypeTrees

| Field | Finding |
|---|---|
| Status | Source-matched for a NoTarget `.unity` class-108 Light input that fails its Patch-version editor read and succeeds through the editor-layout retry. |
| Source location | `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs`: `ReadNormalObject` performs the Patch build bump; `TryReadNormalObject` calls `asset.Read`, which uses editor serialization when the collection's read flags select it. |
| Input evidence | `src/processing/asset_factory_fixture.txt` contains a 272-byte SerializedFile tagged `2017.1.0p0`, named `patch_retry_light_editor.unity`, SHA-256 `58516E6E22B33AFC0127CD60CFB64043C9B32A322D788A95B4BEF51AE7626166`. Its NoTarget class-108 payload is written with `WriteEditor` from the generated `2017.1.1f0` layout. |
| Source behavior | The source factory returns generated type `Light_2017_1_0_p4` with no error log. Direct class dispatch identifies `Light_2017` at `2017.1.0p0` and `Light_2017_1_0_p4` at `2017.1.1f0`, so the returned object demonstrates that the editor-layout retry succeeded. |
| Rust behavior | `object_read_flags` selects editor mode for the NoTarget `.unity` file. The factory retries using the bumped version, selects the editor TPK tree, returns `Light_2017_1_0_p4` with an exact payload tail, and fully materializes it through ObjectWorkspace. |
| Regression evidence | `factory_differential_replays_every_reference_case` checks the NoTarget editor flag, input Patch version, retry type, exact tail, and complete workspace state. The fresh source probe output matches all 283 committed fixture lines. |
| Remaining difference | This is one synthetic editor-mode class/version case. Other editor Patch retry classes and versions, Unity-produced assets, and broad generated-object semantics remain open under `SER-ASSET-FACTORY-001`. |

## DIFF-SER-FACTORY-PATCH-RETRY-EDITOR-001 — Patch retry with editor TypeTrees

| Field | Finding |
|---|---|
| Status | Source-matched for a NoTarget `.unity` class-108 Light input that fails its Patch-version editor read and succeeds through the editor-layout retry. |
| Source location | `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs`: `ReadNormalObject` performs the Patch build bump; `TryReadNormalObject` calls `asset.Read`, which uses editor serialization when the collection's read flags select it. |
| Input evidence | `src/processing/asset_factory_fixture.txt` contains a 272-byte SerializedFile tagged `2017.1.0p0`, named `patch_retry_light_editor.unity`, SHA-256 `58516E6E22B33AFC0127CD60CFB64043C9B32A322D788A95B4BEF51AE7626166`. Its NoTarget class-108 payload is written with `WriteEditor` from the generated `2017.1.1f0` layout. |
| Source behavior | The source factory returns generated type `Light_2017_1_0_p4` with no error log. Direct class dispatch identifies `Light_2017` at `2017.1.0p0` and `Light_2017_1_0_p4` at `2017.1.1f0`, so the returned object demonstrates that the editor-layout retry succeeded. |
| Rust behavior | `object_read_flags` selects editor mode for the NoTarget `.unity` file. The factory retries using the bumped version, selects the editor TPK tree, returns `Light_2017_1_0_p4` with an exact payload tail, and fully materializes it through ObjectWorkspace. |
| Regression evidence | `factory_differential_replays_every_reference_case` checks the NoTarget editor flag, input Patch version, retry type, exact tail, and complete workspace state. The fresh source probe output matches all 283 committed fixture lines. |
| Remaining difference | This is one synthetic editor-mode class/version case. Other editor Patch retry classes and versions, Unity-produced assets, and broad generated-object semantics remain open under `SER-ASSET-FACTORY-001`. |

## DIFF-RAW-FALLBACK-PROJECT-001 - Unknown and unreadable raw project export

| Field | Finding |
|---|---|
| Status | Source-matched for the bounded no-TypeTree class-9999 UnknownObject and UnreadableObject cases. Raw output bytes and paths agree; this does not close general unknown/unreadable factory coverage or project-export parity. |
| Source location | `GameAssetFactory.CreateAsset`; `UnknownObject`; `UnreadableObject`; `RawDataObject`; `UnknownExportCollection.Export`; `UnreadableExportCollection.Export`; `ProjectExporter.Overrides`. |
| Input evidence | `src/processing/asset_factory_fixture.txt` contains source-authored 104-byte files. `unknown_no_tree` is Unity 2019.4.40f1, input SHA-256 `066A7BCA44E4007F120CDDD4CF2BA294590617404B0D3385C1EE669082EA4170`; `unreadable_no_tree` is Unity 3.4.0f1, input SHA-256 `8BF988C398F9D99E3007CD571B84099679B731C80109D0BB21ED8484122B3AA8`. Both use class ID 9999 and payload `0102030405060708`. The fresh factory author replay matched all 289 fixture lines; generated output SHA-256 is `F90A1A7DD1E9498A31D6B7BDE891ED0E1116907C5D41637D1A4F7A0E89EBFEDD`. |
| Source behavior | With `ExportUnreadableAssets=true`, AssetRipper preserves UnknownObject at `ExportedProject/AssetRipper/UnknownAssets/9999/Unknown9999_3FCA88C5.unknown` and UnreadableObject at `ExportedProject/AssetRipper/UnreadableAssets/9999/Unreadable9999_3FCA88C5.unreadable`. Each file contains the exact eight payload bytes, SHA-256 `66840DDA154E8A113C31DD0AD32F7F3A366A80E8136979D8F5A101D3D29D6F72`, and no `.meta` sidecar. The source also emits auxiliary project files outside this bounded comparison. |
| Rust behavior | `export-project` produces the corresponding output-root-relative `AssetRipper/...` path and exact payload bytes, writes no sidecar, and reports that decoded fields and embedded references remain opaque. Default-off regressions confirm both raw files are skipped. Raw objects are omitted from GUID pointer registration; Rust does not invent a serialized export identity for them. |
| Regression evidence | Fresh isolated source `raw-fallback-export` probes ran on both source-authored fixture inputs and checked one raw file/no `.meta`; `export_project_preserves_unknown_raw_bytes_without_meta_when_enabled`, `export_project_preserves_unreadable_raw_bytes_without_meta_when_enabled`, `export_project_skips_unknown_object_when_raw_fallback_setting_is_disabled`, and `export_project_skips_unreadable_object_when_raw_fallback_setting_is_disabled` pass. Exact output identities and CLI status are recorded in `docs/validation-log.md`. |
| Remaining difference | The tested raw payloads are opaque: no fields or embedded references can be scanned. This fixture set has no cross-object reference to a raw object, so output-pointer behavior for such references is not source-differentially verified; the Rust pointer path uses explicit missing-reference handling instead of silently assigning null or minting a GUID. The source's auxiliary project files were not compared. Broader factory coverage remains open under `SER-TT-ABSENT-001` and `SER-ASSET-FACTORY-001`; full collection and project output remain open under `EXPORT-UNITY-001`. |

## DIFF-OBJ-REF-MONO-SCRIPT-LOCAL-001 — Unloaded MonoBehaviour local script reference

| Field | Finding |
|---|---|
| Status | Source-matched bounded integration: a no-TypeTree MonoBehaviour retains its generated base header, and its local `m_Script` pointer resolves to the generated MonoScript object. The unreadable script-specific remainder remains explicit and prevents a complete scan. |
| Source location | `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs` and generated MonoBehaviour/MonoScript asset readers; `SerializedAssetCollection` object construction and the reference-graph traversal. |
| Input evidence | The isolated source probe authors `mono_unloaded.assets` as a generation-10 no-TypeTree SerializedFile tagged Unity 2019.4.40f1. PathID 42 is class 114 MonoBehaviour with null `m_GameObject`, `m_Script`=`PPtr(fileID=0, pathID=7)`, name `Behaviour`, and trailing bytes `DE AD BE EF`; PathID 7 is class 115 MonoScript with name `HeroController`, namespace `VRC.SDK`, assembly `Assembly-CSharp`, and execution order 200. The source-authored fixture `src/processing/asset_factory_fixture.txt` has SHA-256 `13225E7216E078B0EE7FE31F85A050E1AAFEBF813DFAD18251FF7CCF57155746`. Fresh `author` probe output reproduced the exact fixture hash. |
| Source behavior | Source factory inspection identifies the MonoBehaviour as `MonoBehaviour_2019_1_0_b4` with `UnloadedStructure`, and the MonoScript as `MonoScript_2018_3` with the recorded non-default fields. The remaining four bytes are not consumed as a valid script layout. |
| Rust behavior | `SerializedFileWorkspace` and `ObjectWorkspace` load both typed objects. Rust retains the MonoBehaviour header and `m_Script` pointer, resolves it to local file index 0/object index 1 (PathID 7, class 115), preserves the null `m_GameObject` reference as null, and materializes the MonoScript's non-default `HeroController`, `VRC.SDK`, `Assembly-CSharp`, and execution-order 200 fields. The reference graph reports `UnparsedTrailingPayload { byte_count: 4 }`, so the object and joined scan remain incomplete. |
| Regression evidence | `object_workspace_resolves_unloaded_monobehaviour_script_to_local_monoscript` checks both factory identities, the MonoBehaviour pointer field, all four MonoScript fields, the exact local target, null pointer, class lookup, incomplete status, trailing-payload issue, and unchanged input bytes. The focused test and full workspace suite pass. |
| Remaining difference | This demonstrates one local no-TypeTree reference and preserves rather than interprets the unreadable MonoBehaviour script body. It does not implement arbitrary MonoBehaviour field reconstruction, assembly/type recovery, external or cross-bundle references, or export identity mapping. Keep `TYPE-SCRIPT-001`, `SER-TT-ABSENT-001`, `SER-ASSET-FACTORY-001`, and `OBJ-REF-001` open. |

## DIFF-SER-FACTORY-WORKSPACE-MATRIX-001 — Source-generated defaults through ObjectWorkspace

| Field | Finding |
|---|---|
| Status | Source-authored default objects now match through Rust file loading, object materialization, and reference scanning for every successfully generated class/version/mode boundary in this bounded corpus. Two generic mismatches found by the matrix were corrected and have regressions. |
| Source location | `Source/AssetRipper.Assets/Collections/SerializedAssetCollection.cs` passes `ObjectInfo.TypeID` to `AssetInfo`; `Source/AssetRipper.Import/AssetCreation/GameAssetFactory.cs` selects the generated class and reads release/editor fields. |
| Input evidence | The isolated probe writes 4,461 successful TPK class/version boundaries as 8,922 release/editor payloads, groups them into 1,168 synthetic no-TypeTree `FormatVersion.Unknown_10` SerializedFiles (the Unity-version signature varies by TPK boundary), and reloads those files through the frozen source reader/factory before recording source object type, name, MonoBehaviour Structure kind, and payload length. The committed compressed fixture is `src/processing/source_factory_workspace_fixture.bin.gz` (209,974 bytes; SHA-256 `56D81E5F406EA94891CE5482B32082E22461E53A40645E99D51FAD34A8E632D3`). The isolated command `& "$env:LOCALAPPDATA\Microsoft\dotnet\dotnet.exe" run --project tools/probes/ser-factory/SerFactoryProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref" -- workspace-matrix target/source_factory_workspace_fixture_recheck2.gz` exited 0 and reproduced the same hash. It reported 5,000 TPK boundaries, 8,922 payloads, 1,168 groups, 1,861,808 serialized bytes, 1,482,552 payload bytes, 1,078 release/editor skips (539 source `MISSING` or `AbstractClassException` boundaries), and zero writer failures. The probe records all 1,078 skip keys and reasons in its generated `.skips.txt` sidecar. The generated skip sidecar has 792 `MISSING` and 286 `THROW:AbstractClassException` mode-specific records; the existing payload fixture separately distinguishes 396 and 143 unique class/version boundaries, respectively. The probe now fails with class/version/mode context if any writer fails. |
| Rust behavior | `object_workspace_replays_source_factory_defaults_at_every_tpk_boundary` parses and loads every generated file through `SerializedFileWorkspace` and `ObjectWorkspace`; it checks file/version/platform/type-tree metadata, all object identities and types, source names and MonoBehaviour structure outcomes, exact payload bytes against all 8,922 rows of `source_factory_payload_fixture.txt`, decoded byte accounting, recovered TPK roots, source fallback states, and reference-scan issue ownership. Its exact source-payload key check accounts for all 539 existing skip rows. |
| Difference and repair | The first matrix run found that Rust preferred the legacy short `ObjectInfo.class_id` field when deriving the factory class ID. For `factory_0110_release.assets` (`5.6.0b5`), source `TypeID` is `1480428607` (`LowerResBlitTexture_5_6_0_b5`) while the short field is zero; Rust incorrectly dispatched class zero. `object_read_class_id` now uses the resolved/raw `TypeID`, applying the source's negative-to-MonoBehaviour fallback afterward. `read_class_id_uses_type_id_instead_of_legacy_class_id_field` and the full matrix cover the fix. |
| Regression evidence | `cargo test --workspace --locked object_workspace_replays_source_factory_defaults_at_every_tpk_boundary -- --nocapture` passed 1/1 after verifying the exact source payload bytes. The source probe command and fixture SHA-256 above are repeatable. The final full suite and warnings-denied checks are recorded in `docs/validation-log.md`. |
| Remaining difference | This matrix exercises generated default values and no-TypeTree file loading; it does not validate non-default values, all generated serializer field semantics, every Patch retry, valid engine-asset exceptions, assembly-backed MonoBehaviour recovery, cross-file references in generated data, or exporters. Keep `SER-TT-ABSENT-001`, `SER-ASSET-FACTORY-001`, and `OBJ-REF-001` open. |

## DIFF-REFGRAPH-EMPTY-RAW-001 — Empty opaque values carry no unscanned bytes

| Field | Finding |
|---|---|
| Status | Rust no longer reports an empty opaque TypeTree leaf as an incomplete reference scan. Non-empty opaque leaves remain explicit issues. |
| Source location | `src/serialized_file/reference_graph.rs`, `collect_references`, handles `SerializedValue::Raw` leaves. |
| Input evidence | The source-generated workspace fixture includes `GraphicsSettings_3_5` at `3.5.0a0` with a zero-byte payload. Rust decoded that object completely but previously emitted `UnparsedRawValue { byte_count: 0 }` during scanning. |
| Difference and repair | An empty raw leaf contains no serialized bytes in which a pointer could be encoded, so the scanner now emits no unparsed-byte issue for that case. Non-empty raw leaves still produce `UnparsedRawValue`, retaining the existing explicit diagnostic behavior. |
| Regression evidence | `cargo test --workspace --locked raw_type_tree -- --nocapture` passed both tests: non-empty raw bytes still produce an issue and empty raw values produce none. The full source-factory workspace matrix also asserts that every graph issue belongs to a source-unknown, unreadable, or unloaded-structure object. |
| Scope | This is a structural scanner rule based on the actual empty byte span; it does not claim that non-empty opaque bytes have been reference-scanned. |

## DIFF-UNITYFS-ATEND-COMPRESSED-METADATA-001 — Version-7 end-located compressed metadata

| Field | Finding |
|---|---|
| Status | Source-matched for two source-authored UnityFS v7 bundles with `BlocksInfoAtTheEnd`: LZ4-compressed metadata with padded mixed-compression multi-block data, and LZMA-compressed metadata with an LZ4 storage block. Rust recovers the same recursive entry names and bytes as frozen AssetRipper. |
| Source location | `FileStreamBundleHeader.Read`; `FileStreamBundleFile.ReadFileStreamMetadata`; `FileStreamBundleFile.ReadFileStreamData`; `BundleFileBlockReader.ReadEntry`. The baseline aligns the large-file header, seeks to `Header.Size - CompressedBlocksInfoSize`, decodes metadata at the end, then returns to aligned front block data and assembles each directory entry across storage blocks. |
| Input evidence | The committed source-authored cases are in `src/io/io_differential_fixture.txt`. `ufs_v7_atend_lz4meta_multiblock_padded` is 142 bytes (SHA-256 `FE98166F84A2313AEC8ADC304867C1B60C73D2356B18F8D0A931D30BBB991207`); `ufs_v7_atend_lzma_meta_lz4block` is 116 bytes (SHA-256 `62DCBB6917590A36296A4EB90255321D8C04D1904B355F01EA6DF5F4D6A5E863`). Both were generated by the committed C# `BuildUnityFs` probe helper and read by the frozen source runtime. The expanded fixture SHA-256 is `77F8D271C8AECA0E4ECBC34090D1C8145F17FDB9B27208C0D4055051ABDDCE2E`. |
| Source behavior | For the padded LZ4-metadata case, the source returns `cross.bin` = `ABCDEFGHIJ` and `tail.bin` = `FGHIJ` from an LZ4-compressed first block plus a raw second block. For the LZMA-metadata case, it returns `data/file.bin` = `0123456789` from an LZ4 block. Both bundles report one parsed container and zero failed entries. |
| Rust behavior | The production `SchemeReader` and `UnityFsBundle::read_entry` path parses both inputs, uses the end-located compressed metadata extent, accounts for v7 alignment/padding, and yields the exact same recursive inventories and entry bytes. `read_path_streaming` returns the same results for each committed row. |
| Regression evidence | The fresh isolated `IoDifferentialProbe` run exited 0 and emitted both expected source rows. `scheme_dispatch_matches_the_reference_runtime_fixture` now replays 43 source-authored baseline container cases and passed focused validation after the two rows were added. The reference build also succeeded from the isolated copy; the probe emitted its existing CA2022 analyzer warning but completed successfully. |
| Scope | This evidence settles the v7 at-end/compressed-metadata interaction combinations above. It does not expand UnityFS output writing: the frozen source itself throws for non-empty entry-data writes. The existing source/Rust supported read lanes and explicit write limitations remain recorded under `IO-BUNDLE-001` and `IO-COMP-001`. No user AssetBundle or UABEA content was used. |

## DIFF-TT-ABSENT-GENERATED-PREFAB-PROJECT-001 — No-TypeTree generated GameObject/Transform project export

| Field | Finding |
|---|---|
| Status | Source-matched for one synthetic Unity 2019.4.40f1 no-TypeTree file containing a generated GameObject and Transform with non-default serialized fields. The Rust product resolves their reciprocal pointers and emits the same Prefab document and normalized importer metadata as AssetRipper. This does not close any broad feature row. |
| Source location | `AssetFactory.CreateSerialized`; `GameAssetFactory`; the source `SceneDefinitionProcessor` / `PrefabProcessor` path; `PrefabExportCollection`; `PrefabImporter.Create`. The isolated probe calls `ExportHandler.LoadAndProcess` and `Export`. |
| Input evidence | `src/export/no_tree_prefab_source_2019_4_input.hex` is the 360-byte source-authored SerializedFile, SHA-256 `294C2F9D7A0660432E309CAB8C9AD14BF3260990235D6257899FAD78CED3BCE5`. It has no serialized TypeTrees. The generated GameObject (PathID 10) is named `NoTreeRoot`, layer 12, active; its Transform (PathID 11) has position `(1.25,-2.5,3.75)`, quaternion `(0,0.70710677,0,0.70710677)`, and scale `(2,0.5,1.25)`. Both component references point to each other. |
| Source behavior | The frozen source factory resolves both generated classes, creates one Prefab collection, and writes `ExportedProject/Assets/GameObject/NoTreeRoot.prefab`. The 880-byte YAML has SHA-256 `73FF234EA712F9AB78D3AB03C93E7C52A7815072E22ED2D9D2304FBD981444AF`. Its `.meta` has randomized `guid` and `timeCreated`; after excluding only those two lines, every remaining field agrees with Rust, including the empty `addedObjectFileIDs` and zero `isPrefabVariant` / `unableToImportOnPreviousDomainReload` defaults. |
| Rust behavior | The workspace path uses the recovered TPK schemas and decoded payloads, resolves GameObject→Transform and Transform→GameObject in the reference graph, and emits `Assets/GameObject/NoTreeRoot.prefab` with byte-identical YAML. The exporter uses the root GameObject for the collection name and keeps the source TypeTree field names (`m_ObjectHideFlags`, `m_Component`) for generated GameObject/Transform documents. |
| Regression evidence | `project_export_matches_source_for_a_no_tree_generated_prefab_hierarchy` verifies absent/empty input TypeTrees, generated class outcomes, payload decoding, complete reference scan, reciprocal pointer targets, exact Prefab YAML, normalized `.meta`, one collection, two exported objects, and zero warnings. Fresh isolated source probe and `cargo run --locked -- export-project target/notree-prefab-rust-final-20261008 target/notree-prefab-source-final-20261008/NoTreeRoot.unity` both exited 0; both Prefab YAML files hash to the value above. |
| Remaining differences | This bounded comparison does not compare `AuxiliaryFiles/path_id_map.json`, `Packages/manifest.json`, `ProjectSettings/EditorSettings.asset`, or `ProjectSettings/ProjectVersion.txt` emitted by the source. It does not validate other Unity versions, hierarchy shapes, arbitrary generated fields, or Unity Editor import/runtime. Keep `SER-TT-ABSENT-001`, `SER-ASSET-FACTORY-001`, `OBJ-REF-001`, `PROC-SCENE-001`, and `EXPORT-UNITY-001` open. No user AssetBundle or UABEA content was used. |

## DIFF-ANIMATION-WORKSPACE-TPK-001 — Non-default generated AnimationClip workspace

| Field | Finding |
|---|---|
| Status | Source-generated synthetic evidence for Unity 2022.3.0f1: a no-TypeTree SerializedFile contains an AnimationClip with non-default binding, streamed, dense, constant, event, and PPtr data alongside its scene assets. Rust materializes and exports all 13 objects through the production workspace and file-YAML paths. This does not close a broad feature row. |
| Source location | `AssetRipper.SourceGenerated.Classes.ClassID_74.AnimationClip`; `AssetFactory.CreateSerialized`; `TpkFile`; `AnimationClipConverter.Process`; Rust `ObjectWorkspace` and `export-file-yaml`. |
| Input evidence | `src/resources/animation/animation_workspace_fixture.assets` is a source-generated 4,168-byte `FormatVersion.Unknown_10` SerializedFile with no file TypeTrees, Unity 2022.3.0f1, and 13 objects. SHA-256 `08E67AE11A865D1645E6CDA6E0D0C92B6A4B57EA5A1E04BB5E1E6B19A2EBD069`. It includes 23 generic bindings, one local plus one null PPtr mapping, streamed frames, two dense frames with six values, two constant values, and one non-default event. The source `workspace` probe also reproduces the existing full converter output fixture at SHA-256 `A4C1BA2733ABAB473E46C924CEF3D343E4E1EB69293E04195BC9009EB714CAD5`. |
| Source behavior | The frozen generated writer emits the no-TypeTree SerializedFile from the same real generated object graph used by the source `AnimationClipConverter` probe. A fresh source `tpk 91 2022.3.0f1` query confirms the actual `ValueArray` child shape: `vector.m_PositionValues` / `vector.m_ScaleValues` contain `Array.Array`, then `float3` or `float4` children named `data`. |
| Rust behavior | The `ObjectWorkspace` regression recovers all 13 generated payloads, verifies the non-default scalar/array/event fields and PPtr targets, and confirms the per-file reference graph is complete with 58 PPtrs (21 uniquely resolved and 37 null). The CLI's `export-file-yaml` path writes 13 ordered documents and preserves the tested fields and local PPtr mapping. |
| Difference and repair | The YAML hint builder previously accepted only a custom `m_Array` / `m_Data` shape and generated `Vector3Float` / `Vector4Float` labels. It now accepts the frozen source's `Array` / `data` structure and `float3` / `float4` labels while retaining the earlier forms. This fixed an actual `export-file-yaml` failure on the source-generated AnimatorController in the workspace. |
| Regression evidence | `source_generated_nondefault_animation_clip_materializes_through_object_workspace`, `export_file_yaml_handles_source_generated_animation_workspace_without_typetrees`, `clip_muscle_and_xform_aliases_follow_vector_child_type`, and `value_array_aliases_follow_nested_vector_element_type`. Full workspace validation on 2026-10-08 passed 1,533 library tests (1 ignored) and 99 CLI tests; warnings-denied Clippy, formatting, diff check, and the frozen-baseline verifier passed. |
| Remaining differences | The fixture exercises one synthetic 2022.3.0f1 graph. `process_clip` is still not constructed from the `ObjectWorkspace` object by the product processing sequence, and project export still lacks the complete `.anim`/AnimatorController collection pipeline and Unity dynamic sampling. Keep `SER-TT-ABSENT-001`, `SER-ASSET-FACTORY-001`, `PROC-ANIM-*`, `RES-ANIM-001`, and `EXPORT-UNITY-001` open. No user AssetBundle or UABEA content was used. |

## DIFF-MANAGED-REF-PROJECT-001 — Managed-reference cycles and cross-file PPtr in project export

| Field | Finding |
|---|---|
| Status | A synthetic Unity 2019.4.40f1 / `FormatVersion.StoresTypeDependencies` integration now passes through the frozen AssetRipper project path and the Rust `export-project` path. Rust resolves a two-node managed-reference cycle and a cross-file PPtr. The complete root YAML matches after normalizing only the randomized target GUID; the referenced object's complete YAML matches byte-for-byte. This closes the previously recorded document-level difference for this fixture, not a broad feature row. |
| Source location | The isolated probe calls frozen `ExportHandler.LoadAndProcess` and `Export`. The Rust route uses `SerializedFileWorkspace`, `ObjectWorkspace`, `MultiFileReferenceGraph`, and `export_workspace_yaml_project_with_settings` (`src/export/project_workflow.rs`, `src/export/workspace_yaml.rs`). |
| Input evidence | `tools/probes/managed-project/Program.cs` authors `managed.assets` (2,216 bytes, SHA-256 `641B803EB567DD433833A86F31D300B5CA8CF65356A25FAF273873CD04C2E976`) and `target.assets` (824 bytes, SHA-256 `38B8AC7D07DEBB265099D7D11B757DA72F91B5DAD00130E29BFEE4CF81D586DC`). The committed inputs are under `src/export/managed_reference_project_fixture/`; a fresh source probe reproduced both hashes. The root MonoBehaviour at PathID 42 contains counted managed-reference entries RID 100 (`NodeA`) and RID 200 (`NodeB`), with values 123/456 and links forming a cycle. `NodeA.m_Asset` points to target fileID 1 / PathID 900 in `target.assets`. Serialized file TypeTrees and the serialized reference-type metadata are present. |
| Source behavior | The isolated frozen-source `ExportHandler` loads both MonoBehaviours and writes `ManagedRoot.asset` plus `ReferencedRoot.asset` and their sidecars. Its root YAML includes the managed registry, both typed nodes, their cyclic RID links, and the target asset reference. Fresh run: `dotnet run --project tools/probes/managed-project/ManagedProjectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref" -- target/managed-project/run5-input target/managed-project/run5-source-output target/managed-project/run5-source-rows.txt` exited 0. |
| Rust behavior | The Rust workspace fully materializes both objects as class 114. Its joined reference graph reports the managed RID links as resolved and resolves `references.RefIds[0].data.m_Asset` from source PathID 42 to `target.assets` (workspace file index 1; serialized fileID 1 / PathID 900). Project export emits two `.asset` files and sidecars; the PPtr uses the target `.meta` GUID with Unity fileID 11400000 and `type: 2`. Fresh `cargo run --locked --quiet -- export-project target/managed-project/run5-rust-output target/managed-project/run5-input` exited 0. The full root MonoBehaviour document matches the source after replacing only the generated target GUID; the referenced MonoBehaviour document matches exactly. |
| Difference and repair | The first Rust export attempt rejected PathID 42 because generation 21 omits the legacy object-table `class_id`. The resolved class is available through the source factory result. Project document dispatch and ShaderImporter target identity now use `object.factory.class_id`; YAML identity validation also checks the resolved raw TypeID. The project YAML path supplies the six source default header fields for this MonoBehaviour, so the earlier report that these fields were absent is superseded by the full-document comparison. |
| Regression evidence | `project_export_keeps_source_managed_registry_cycles_and_cross_file_pointer` checks both resolved object classes, the exact cross-file pointer destination, two managed objects and all three resolved RID edges, full root YAML after replacing only the randomized target GUID, byte-for-byte target YAML, output counts, and absence of processing warnings. Fresh source input hashes matched the checked-in fixtures. The focused command `cargo test --locked project_export_keeps_source_managed_registry_cycles_and_cross_file_pointer -- --nocapture` passed 1/1 on 2026-10-09. |
| Remaining differences | This fixture has embedded TypeTrees and does not cover missing-TypeTree MonoBehaviour recovery, assembly-backed MonoBehaviour recovery, the full nested/cross-container bundle dependency matrix, or Unity import/runtime. Keep `OBJ-REF-001` and `EXPORT-UNITY-001` open; this does not close their broader reference and export paths. No user AssetBundle or UABEA content was used. |

## DIFF-OBJ-REF-NESTED-BUNDLE-001 — Cross-file PPtr through nested UnityFS bundles

| Field | Finding |
|---|---|
| Status | Frozen AssetRipper and Rust `export-project` both load a source file from an outer UnityFS and its referenced SerializedFile from a nested UnityFS. The managed-reference cycle and cross-file PPtr survive export. Root/target YAML and importer metadata match after normalizing the generated target GUID, generated sidecar `guid`/`timeCreated`, and sidecar trailing blank-line formatting. This closes the tested nested-bundle lookup behavior; the broader reference feature row remains open. |
| Source location | Frozen `ExportHandler.LoadAndProcess`, `Bundle.ResolveCollection`, SerializedFile container expansion, and the source collection lookup in `src/serialized_file/workspace.rs`. |
| Input evidence | `tools/probes/managed-project/Program.cs` creates `managed.assets` (2,216 bytes, SHA-256 `641B803EB567DD433833A86F31D300B5CA8CF65356A25FAF273873CD04C2E976`) in `workspace.bundle`; a nested `nested.bundle` contains `target.assets` (824 bytes, SHA-256 `38B8AC7D07DEBB265099D7D11B757DA72F91B5DAD00130E29BFEE4CF81D586DC`). The outer source-authored UnityFS is 3,323 bytes, SHA-256 `8A2896F4E7EE24D3628827CA2CB316DD530E74A9C5CB04A97D5841501A71B573`; its nested bundle is 948 bytes, SHA-256 `561BBE967E4B79C8D1FDE7F6FED266E4806161194D310744D4EC33B206D1081A`. `src/export/managed_reference_nested_bundle_fixture.txt` records the reproducible source input and normalized source output. |
| Source behavior | The isolated source probe reports `Asset bundle 'workspace' has been found`, then loads `ManagedRoot` at PathID 42 and `ReferencedRoot` at PathID 900. The source root contains RID 100/200 cyclic links and an `m_Asset` PPtr to the object in the nested target file. |
| Rust behavior | The production CLI imports the outer bundle, recursively mounts the nested bundle, loads two SerializedFiles/two objects, and exports both managed objects. The PPtr maps to fileID 11400000 and the target `.meta` GUID. |
| Comparison | The root YAML matches source output after normalizing only the target GUID; referenced YAML matches byte-for-byte. Both `NativeFormatImporter` sidecars match after removing generated `guid` and `timeCreated` fields and canonicalizing trailing blank lines to one final newline. Normalized source hashes: root YAML 792 bytes / `CDB365D2DC23B0303D3977BC0F5693246C86D4015455AA246575A7F65FA2255B`; target YAML 362 bytes / `66D18DC5B18B71A6A367A3DD773921E3EB4CB3711472742DC55A6E3852C4BEE4`; each normalized sidecar 165 bytes / `F3709C26DD3870ADF6B3E1DB5CC98738517FCD9FE06760CFD5DFD712832BAD64`. |
| Regression evidence | The isolated source command `& "$env:LOCALAPPDATA\Microsoft\dotnet\dotnet.exe" run --project tools/probes/managed-project/ManagedProjectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref" -- export-nested-reference-bundle target/nested-reference-run4/input target/nested-reference-run4/source-output target/nested-reference-run4/source-rows.txt` exited 0. Rust `cargo run --locked --quiet -- export-project target/nested-reference-run4/rust-output target/nested-reference-run4/input/workspace.bundle` exited 0; all four normalized output files match the source rows byte-for-byte. `cargo test --locked export_project_resolves_cross_file_reference_through_nested_unityfs -- --nocapture` passed 1/1. |
| Remaining scope | This proves one root-to-nested cross-file lookup path with managed cycles and mapped export pointers. It does not close the full bundle hierarchy matrix, built-in resource identities, missing/ambiguous dependencies, all collection order cases, or Unity import/runtime. Keep `OBJ-REF-001` and `EXPORT-UNITY-001` open. No user AssetBundle or UABEA content was accessed. |

## DIFF-MANAGED-ASSEMBLY-001 — Mono identity, FieldSerializer schema, and structure export

| Item | Evidence |
|---|---|
| Source behavior | `BaseManager.Read` parses the managed assembly, indexes the path-derived filename stem, and registers the manifest assembly name. `FindType` uses exact assembly, namespace, and class names. `IsPresent` requires a set manager and matching TypeDef; `IsValid` additionally rejects abstract types. `FieldSerializer` walks serializable base fields, applies source field/attribute filters, and builds a Unity TypeTree using the source class IDs and metadata flags. `MonoManager.TryLoad` skips invalid/native PE files before managed metadata parsing and fails its initialization path for a CLR image that cannot be read. |
| Source evidence | The isolated source probe reads a generated 4,608-byte `FixtureScripts.dll` as `Assembly-CSharp.dll`; its SHA-256 is `52117F6D87359D5F82D9985A6582ED59E55FA4C8193DD7456D7DC78DE5E946F1`. The committed fixture pins six identity/validity queries, five ordered field metadata rows, and the exact schema nodes: `HeroController`/Base (class ID 1, meta 32768), inherited `SInt64`, `SInt32` health, and float speed fields (meta 16384). The independent source `ExportHandler` probe writes the recovered `MonoBehaviour` document in `src/processing/managed_assembly_project_fixture.txt` (SHA-256 `4BB4FA4667DB461A6929A26414E6B35A3841AA2D35747D7F5643D56845B1DDCB`), its generated script text stored as hex in `managed_assembly_script_fixture.hex` (decoded source bytes SHA-256 `F0E0162BFBEB05C5F22B8760B3392BE54A9C750E0EEF418ED3393F3D64295061`), and its normalized MonoImporter sidecar in `managed_assembly_script_meta_fixture.txt` (SHA-256 `B2A0819852E42EF6DA57BE5F36D8E241FFE092AFC28C95C1DCA679E6AE48EDC4`). |
| Rust behavior | `src/processing/managed_assembly.rs` bounds the candidate count, aggregate bytes, per-file bytes, TypeDef count, and field count; checks the PE COM descriptor before invoking the Rust CLR metadata reader; reproduces path and manifest aliases; and resolves decoded class-115 payload identities. `managed_assembly_schema.rs` constructs a source-shaped tree for the covered inheritance/attribute case. `ObjectWorkspace` matches the selected local MonoScript and assembly, decodes the entire `UnloadedStructure` suffix, and replaces the file graph with typed recovered fields. Production `export-project` flattens those fields into MonoBehaviour YAML and supplies the source default header fields. |
| Regression evidence | The source schema fixture is replayed exactly. Production `export-project` emits the deterministic `.cs` path, dummy source, MonoImporter metadata, and matching `m_Script` Meta pointer for the covered predefined Assembly-CSharp class. The full MonoBehaviour YAML equals the frozen source document; script bytes equal the frozen source script fixture; sidecar YAML matches after excluding only randomized `timeCreated`. `export_project_recovers_assembly_backed_monobehaviour_structure` passed. Full validation: `cargo test --workspace --locked --quiet` passed 1,548 library tests (1 ignored), 100 CLI tests, and 1 ignored doc test. Warnings-denied all-target/all-feature Clippy, format check, diff check, and the 1,449-file frozen-baseline verifier passed. |
| Remaining difference | The pinned `.cs`/MonoImporter integration covers one synthetic Unity 2019.4.40f1 predefined assembly. No-manager reference maps and a reference pointer are covered separately in `DIFF-MONOSCRIPT-REFERENCE-001`; one manager-backed mscorlib2 `Mono.Cecil` reference pointer now matches the source under `DIFF-MONOSCRIPT-REFERENCE-MSCORLIB2-001`. Broader manager-backed project routes for non-predefined assemblies and `Save`/`Skip` modes remain incomplete. Saved DLL export, script collection deduplication/redirects, generic class path behavior, and version-specific MonoImporter layouts remain open. The schema remains proven only for one inherited class; nested/collection/managed-reference fields, referenced assemblies, Unity-version gates, broad filtering/layout conversion, and malformed cases remain open. IL2CPP/Cpp2IL, content levels, decompile/save, and complete script-export behavior remain open under `TYPE-SCRIPT-001`; Unity import/runtime behavior is not established. Phase 1 remains open. |
| Input protection | Synthetic compiler-generated fixture only. No user AssetBundle, UABEA, or user sample name/hash/content was accessed. Phase 2 remains locked. |

## DIFF-MONOSCRIPT-REFERENCE-001 — Reference-assembly script pointers without a manager

| Field | Finding |
|---|---|
| Status | A source-authored Unity 2019.4.40f1 pair of SerializedFiles contains a `UnityEngine.Object` MonoScript and a MonoBehaviour in another file that points to it. AssetRipper and Rust now agree on the exported dummy script, MonoImporter sidecar (excluding `timeCreated` and normalizing line endings), empty-reference asmdef, and complete MonoBehaviour YAML. The pointer uses the reference assembly GUID and MD4 script fileID; the script sidecar keeps the separately calculated script GUID. This is a bounded no-manager path under `TYPE-SCRIPT-001`, not a feature-row closure. |
| Source location | `ReferenceAssemblies.GetReferenceAssemblies` builds a version-selected dictionary and always inserts `UnityEngine`; with `BaseManager.HasMscorlib2 == false`, the selected Mono list prefers Mono4. `ScriptExporter.CreateExportPointer` uses the reference GUID and `ScriptHashing.CalculateScriptFileID` for `Skip`. `EmptyScriptExportCollection` still emits its empty dummy script and MonoImporter metadata. Rust implements the captured no-manager result in `src/export/reference_assemblies.rs` and separates pointer-map GUID/fileID from the script `.meta` GUID in `src/export/project_workflow.rs`. |
| Reference-map evidence | `src/processing/reference_assembly_map_fixture.txt` is the 1,392,937-byte source-runtime capture of all 111 version boundaries and 83 unique `GetReferenceAssemblies` maps; SHA-256 `C123AA2306CCF583B1DAF0C17ECBD17892A454F5857E0324B1F01BE2854DAAB3`. The reference test checks the UnityEngine GUID, a version-dependent `UnityEngine.CoreModule` entry across the 2017.1/2017.2 boundary, the UnityEngine.UI GUID nibble order, and lower-bound clamping. |
| Input evidence | `src/processing/reference_script_project_fixture.txt` stores the two synthetic inputs and source outputs; 10,356 bytes, SHA-256 `B02E5C1DBD8CD8B3BD618FFBD7650AC42ED2A8D1E9C8DA19869083229F803471`. The script file is 208 bytes (SHA-256 `60E96B8D4713CB9C713F5EAA564387E4ACE2CB896091A4C2142B573F618AD21B`); the behavior file is 888 bytes (SHA-256 `192FB12B2CECA2A9B7BFF9A5DF283E13985C792A8DB7B04A9BAE16BD4724EFBB`). Both files were authored by the source probe; no user bundle was involved. |
| Source output | The captured project contains `Assets/Scripts/UnityEngine/UnityEngine/Object.cs` (2,774 bytes, SHA-256 `7C3CEFEE48FFF865121D45D2053D538F9E1D4B4C666E80DFD02A1C6A7C9BB174`), its normalized MonoImporter sidecar (254 bytes, SHA-256 `8B64A11A8C71065AE1F4E4465D5264B162D0E311E3479DC4180BB3100751782D`), and `UnityEngine.asmdef` (78 bytes, SHA-256 `49C994693D0219748E8827CC5096F868232FADBE48541050772D86B36B4D7210`). The referencing `ReferenceRoot.asset` (418 bytes, SHA-256 `729428A80780C05858C5B4E0313B7804DBA194ABEBB5121E38D2F238A81BF6C3`) contains `m_Script: {fileID: 350709172, guid: f70555f144d8491a825f0804e09c671c, type: 3}`. |
| Validation | The isolated source command `dotnet run --project tools/probes/managed-project/ManagedProjectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref" -- export-reference-script target/reference-script-source-20261008e/input target/reference-script-source-20261008e/output target/reference-script-source-20261008e/rows.txt` exited 0. Rust test `export_project_matches_source_reference_monoscript_pointer_without_a_manager` passed and compares the complete source outputs while verifying both input files remain unchanged. Focused map tests passed 2/2. Full validation passed: 1,550 library tests (1 ignored), 102 CLI tests, 1 ignored doc test; warnings-denied Clippy, format check, diff check, and all 1,449 baseline-file checks exited 0. The parity report recognizes all 386 rows and keeps Phase 1 OPEN. |
| Remaining differences | This reference pointer export is the no-manager source path. The manager-backed `HasMscorlib2` map-selection data, production selection, and one mscorlib2 `Mono.Cecil` pointer export are source-pinned under `DIFF-MONOSCRIPT-REFERENCE-MSCORLIB2-001`. Broader manager-backed `Save`/`Skip`/decompile behavior, duplicate identities, redirect collections, saved DLL reserialization/dependency closure, other importer versions, and IL2CPP/Cpp2IL remain open. The no-manager pointer case is one synthetic 2019.4.40f1 pair and the manager-backed pointer case is one synthetic 2019.1.0f1 pair; neither is Unity Editor import/runtime validation. Keep `TYPE-SCRIPT-001` and `EXPORT-UNITY-001` open. No user AssetBundle or UABEA content was accessed; Phase 2 remains locked. |

## DIFF-MONOSCRIPT-REFERENCE-MSCORLIB2-001 — manager-backed reference map selection

| Field | Finding |
|---|---|
| Status | The frozen source's `HasMscorlib2 == true` reference-assembly map is captured across all 111 source version boundaries, its selection is wired to the parsed `mscorlib` manifest major, and a manager-backed `Mono.Cecil` pointer export matches the frozen source byte for byte. This closes the tested map-selection and pointer-output subcases only; `TYPE-SCRIPT-001` remains in progress. |
| Source location | `IAssemblyManager.HasMscorlib2` is `Mscorlib?.Version.Major == 2`. `ReferenceAssemblies.GetReferenceAssemblies` selects `Mono2` when that property is true and the per-version list is non-empty, otherwise it falls back to `Mono4`; it then adds the remaining Unity assembly lists and the `UnityEngine` default. Rust: `src/export/reference_assemblies.rs`, `src/export/project_workflow.rs`, and `src/processing/managed_assembly.rs`. |
| Source-runtime evidence | `tools/probes/managed-project/Program.cs` loads the installed .NET Framework 2.0 `mscorlib.dll` into the frozen source `BaseManager`, verifies the manifest identity and major version, and calls `ReferenceAssemblies.GetReferenceAssemblies` at every source version boundary. The 895,780-byte fixture `src/processing/reference_assembly_map_mscorlib2_fixture.txt` contains 111 ordered boundaries, 69 deduplicated maps, and 13,359 entries (SHA-256 `81A5A4380C555B35260058EE68AEE1E23B4952FA76F1CE7E97AEDAC638222626`). A fresh probe replay produced the same SHA-256. At Unity 2019.1.0f1, `Mono.Cecil` is present in the manager-backed map and absent from the no-manager (`HasMscorlib2 == false`) map. |
| Rust behavior and regression | `ReferenceAssemblyMaps::from_source_fixture_with_mscorlib2` parses both source captures. The production project workflow selects the `true` table when the managed catalog reports `mscorlib` major 2; absent `mscorlib` and the .NET runtime fallback select the `false` table. The 111-boundary regression compares Rust selection against every captured assembly membership/GUID cell. A generated 4,608-byte `mscorlib.dll` identity fixture (SHA-256 `53A2E6C134A91E88849DC7BD4D53E2EA27223FA609CE2CA70872A7DC523A0115`) verifies catalog parsing and map selection. A source-authored Unity 2019.1.0f1 pair plus that `mscorlib.dll` fixture then runs through the production Rust `export-project` path; the resulting 418-byte `ReferenceRoot.asset` is byte-identical to the frozen source output (SHA-256 `729428A80780C05858C5B4E0313B7804DBA194ABEBB5121E38D2F238A81BF6C3`). Its `m_Script` pointer is fileID `350709172`, GUID `f70555f144d8491a825f0804e09c671c`, type 3. Rust reports the unavailable `Mono.Cecil` managed type explicitly while preserving this reference pointer. |
| Input and source output evidence | `src/processing/reference_script_mscorlib2_project_fixture.txt` is the 12,850-byte deterministic source probe record (SHA-256 `B2AF9D550BC8D027813D6D2D47176C9B26F2E3A0D5E39F3B6240164BA2ED6112`); a fresh source replay reproduced it exactly. The probe confirmed `Mono|IsSet=true|HasMscorlib2=true`. AssetRipper also emitted `System.Runtime.CompilerServices.Unsafe` and `System.Runtime.InteropServices` plugin outputs from the generated mscorlib fixture; those auxiliary files were not compared, so this is a pointer/YAML match rather than whole-project parity. |
| Validation | `export-manager-reference-script` ran twice through the isolated frozen source and produced identical fixture rows; the Rust product CLI and `export_project_matches_source_manager_backed_mscorlib2_reference_pointer` matched the source MonoBehaviour YAML. Full workspace validation is recorded in `docs/validation-log.md`. |
| Remaining differences | The map selector and one manager-backed reference pointer output are covered. Script `Save`/`Skip`/decompile modes, DLL reserialization and dependency closure, collection deduplication/redirects, version-specific importer layouts, and IL2CPP/Cpp2IL remain open. The managed `Mono.Cecil` type was absent from the synthetic input and Rust reports it as unresolved; type reconstruction is not established by this pointer test. Unity Editor import/runtime was not run. Phase 1 remains open and Phase 2 remains locked. No user AssetBundle or UABEA content was accessed. |

## DIFF-MONOSCRIPT-MISSING-SAVED-ASSEMBLY-001 — preserve a saved-assembly script pointer when the DLL is absent

| Field | Finding |
|---|---|
| Source behavior | The isolated source project has `HasMscorlib2 == true` and targets Unity `2023.3.0a16`. The manager-backed reference table selects map 63, where `Mono.Cecil` is absent. With the default `Hybrid` setting, `ScriptExporter.GetExportType` therefore selects `Save`; its pointer uses the deterministic assembly GUID and script fileID even though the imported assemblies contain no `Mono.Cecil.dll`. Frozen-source `ExportHandler` completes and writes the referring MonoBehaviour with `m_Script: {fileID: 350709172, guid: 9c0efa53083fa5c2e288a6f7d5241625, type: 3}`. It does not fabricate a `Mono.Cecil.dll`. |
| Input and source evidence | `src/processing/managed_reference_missing_assembly_project_fixture.txt` captures the two source-authored SerializedFiles, the 4,608-byte major-2 `mscorlib.dll` test assembly, and source project YAML. The 12,850-byte fixture SHA-256 is `5E3671E18FB006AD50376454FD52662575E3D01A70E228BC180545D0A74B71B2`. Fresh source probes at `2023.3.0a16` reproduced identical fixture hashes. The source MonoBehaviour YAML is 418 bytes with SHA-256 `550CDF6CBA073557F0B6B324E9E656F8EC08F56E5BE5C23907EF5217649CA86E`. The test assembly also causes the source to emit `System.Runtime.CompilerServices.Unsafe.dll`; this auxiliary dependency output is not part of this bounded comparison. |
| Rust behavior and regression | Rust now retains the source assembly GUID/fileID pointer when `ScriptExportMode::Hybrid` selects `Save` but the catalog lacks that assembly. It emits no fabricated DLL and reports the missing content through `WorkspaceProjectExportReport::unresolved_script_content` and the CLI status. The MonoScript type stays explicitly unresolved as `AssemblyNotLoaded`. `export_project_preserves_manager_backed_pointer_when_saved_assembly_is_absent` verifies the exact source YAML, content warning, absence of DLL output, and input preservation. |
| Validation | The isolated source command `export-manager-reference-script ... Mono.Cecil 2023.3.0a16` exited 0 twice and reproduced the fixture hash. Rust `cargo run --locked --quiet -- export-project target/manager-reference-script-2023a16-rust target/manager-reference-script-2023a16-source/input` exited 0; source and Rust MonoBehaviour YAML hashes match exactly. Focused manager-backed regressions passed 2/2. Full workspace validation is recorded in `docs/validation-log.md`. |
| Remaining differences | This covers one Unity version, one missing saved assembly, and one MonoScript identity. The source-generated auxiliary `System.Runtime.CompilerServices.Unsafe.dll` is not reconstructed by Rust's current managed-assembly catalog; broader manager dependency discovery, assembly export, script modes, and Unity Editor import/runtime remain open under `TYPE-SCRIPT-001`. No feature row closes. Phase 1 remains open and Phase 2 remains locked. No user AssetBundle or UABEA content was accessed. |

## DIFF-MONOSCRIPT-DECOMPILE-MISSING-TYPE-001 — retain the decompile pointer when its type is absent

| Field | Finding |
|---|---|
| Source behavior | At Unity `2023.3.0a16`, manager-backed map 63 has no `Mono.Cecil` entry. Under `ScriptExportMode::Decompiled`, `ScriptExporter.GetExportType` selects `Decompile`; `ScriptExportCollection.ShouldExport` then skips a `.cs` file because the loaded manager cannot find the serialized `UnityEngine.Object` type in `Mono.Cecil`. The source still writes the MonoBehaviour with `m_Script: {fileID: 11500000, guid: 99973287c380a5bd750521a6c6da4425, type: 3}`. |
| Input and source evidence | `src/processing/managed_reference_missing_assembly_decompiled_project_fixture.txt` contains the source-authored inputs and output (12,864 bytes; SHA-256 `A8882079CEBE72F08C71BF0AE59D503110A42F1ADAE413EB734929BEC58E24E7`). The 417-byte source MonoBehaviour YAML hash is `15FE8C626FDE95613FB8A327397DEC8FD8BF6C841AF79EA04BB6DCCCB9964701`. Fresh source runs with `ScriptExportMode::Decompiled` reproduced the fixture. AssetRipper separately decompiles `System.Runtime.CompilerServices.Unsafe` from the generated mscorlib fixture; that auxiliary output is not compared. |
| Rust behavior and regression | Rust checks the manager catalog's exact script type presence. When absent in the manager-backed decompile route, it retains the script GUID/main fileID pointer, emits no placeholder `.cs` for `Mono.Cecil`, and reports the missing script content. `export_project_decompiled_mode_keeps_pointer_without_absent_script_type_file` compares the full MonoBehaviour YAML to the source fixture and checks the warning and input preservation. |
| Validation | The isolated source `export-manager-reference-script ... Mono.Cecil 2023.3.0a16 Decompiled` probe exited 0 twice. Rust `cargo run --locked --quiet -- export-project --settings target/manager-reference-script-2023a16-decompiled-settings.json target/manager-reference-script-2023a16-decompiled-rust target/manager-reference-script-2023a16-decompiled-source/input` exited 0; source and Rust YAML hashes match. Focused test passed 1/1. Full workspace validation is recorded in `docs/validation-log.md`. |
| Remaining differences | This covers one absent assembly/type identity, one Unity version, and one decompile-mode pointer path. Decompilation when the manager does contain the type, source manager dependency closure, the auxiliary `Unsafe` output, other script modes, and Unity Editor import/runtime remain open under `TYPE-SCRIPT-001`. No feature row closes. Phase 1 remains open and Phase 2 remains locked. No user AssetBundle or UABEA content was accessed. |

## DIFF-MONOSCRIPT-EMPTY-GENERIC-001 — Generic script export without a managed assembly

| Field | Finding |
|---|---|
| Status | A source-authored Unity 2019.4.40f1 / `FormatVersion.StoresTypeDependencies` SerializedFile with one no-TypeTree MonoScript (`Game.Widget`1`, assembly `GameScripts`, execution order 17) now passes through AssetRipper's `EmptyScriptExportCollection` and Rust `export-project`. Rust matches the source dummy script, MonoImporter sidecar (after excluding `timeCreated` and normalizing line endings), and generated assembly definition byte-for-byte. This adds a bounded branch under `TYPE-SCRIPT-001`; it closes no broad feature row. |
| Source location | `EmptyScriptExportCollection` collects unique `MonoScriptInfo` identities, expands generic names through `EmptyScript.GetContent`, writes a MonoImporter sidecar, and emits assembly definitions for non-predefined assemblies at Unity 2017.3 and later. `AssemblyDefinitionExporter` serializes an empty-reference `GameScripts.asmdef` when no module is available. |
| Input evidence | `src/processing/generic_script_project_fixture.txt` pins the 200-byte input at SHA-256 `D8F315556693542ACE504497EDE39A50E822979ABA704D48EA5CF8AF932001F0`. It has an empty file TypeTree, generated MonoScript class 115, and the serialized identity fields. The source probe runs `managed-project export-empty-script`; a fresh run at `target/generic-script-source-2d741c405b2c43abae9d7bde49f68d90` exited 0. |
| Source output | AssetRipper writes `Assets/Scripts/GameScripts/Game/Widget`1.cs` (2,771 bytes, SHA-256 `2C2BB6FD5CE5CF622C4CD2BDECF6CE3D5938D922096D81F58B0283445AE8CAE7`), `Widget`1.cs.meta` (255 normalized bytes, SHA-256 `5602D982707A32472C10AE9D32BA7ABEC274158B0CE5DBF8794F1F65C393B69E`), and `Assets/Scripts/GameScripts/GameScripts.asmdef` (78 bytes, SHA-256 `4734ADE669AAABE5A25B3D15BD51992945959C43D09CAF8F497AA50AC43D09A9`). The sidecar comparison removes only its nondeterministic creation-time field and normalizes line endings. |
| Rust behavior | With no assembly manager, the production route now accepts a non-predefined user assembly, emits the generated dummy class `Widget<T1>` at the source path, retains the source script GUID/fileID and execution order, and stages the empty-reference `.asmdef` on Unity 2017.3+. The product regression `export_project_matches_source_generic_empty_script_without_a_manager` checks all three outputs and preserves the source input. |
| Remaining differences | The whole source project also emits Packages and EditorSettings files; those remain outside this script-only comparison. Reference-assembly pointer behavior is covered separately in `DIFF-MONOSCRIPT-REFERENCE-001`; duplicate identities/redirect collections, manager-backed custom assembly modes, DLL save/renaming, decompiler output, ScriptLanguageVersion, IL2CPP/Cpp2IL, or importer layouts beyond 2019.4.40f1. Keep `TYPE-SCRIPT-001` and `EXPORT-UNITY-001` open. No user AssetBundle or UABEA content was used. |

## DIFF-PROC-SCENE-UNRESOLVED-COMPONENT-001 — Missing external GameObject component

| Field | Finding |
|---|---|
| Status | The Rust workspace scene path now uses materialized source-generated TypeTrees for GameObject relationships. It preserves an unresolved external component PPtr as an issue and refuses to infer that the GameObject has no Transform. This intentionally diverges from the frozen source processor, which generates a replacement Transform for the same input. |
| Source location | `Source/AssetRipper.SourceGenerated.Extensions/GameObjectExtensions.cs` (`GetTransform`, `GetComponentAccessList`); `Source/AssetRipper.Processing/Prefabs/PrefabProcessor.cs` (`HasNoTransform`, `AddMissingTransforms`); Rust `src/processing/scene.rs::SceneGraph::from_materialized_workspace`; `src/main.rs` workspace summary processing. |
| Input evidence | `src/processing/scene_unresolved_source_fixture.txt` contains a 280-byte Unity 2019.4.40f1, generation-10, no-TypeTree SerializedFile with SHA-256 `198E7537EFA7CC54CD3EDF3BB35976FD13082D927EDDD4C01EC4FD211FEADC3B`. Its sole GameObject (PathID 10) has one `m_Component` pair whose serialized class ID is Transform (4) and whose PPtr is `fileID=1, PathID=99`; the file dependency names `missing_component.assets`, which is not supplied. The frozen-source processor rows fixture is 1,454 bytes, SHA-256 `358020D681D893E6FDAE5CD0C10F4DC400D4B59F06E28EA0B510AAFE7F5E8529`. |
| Source behavior | `TryGetComponent<ITransform>` cannot resolve the external PPtr, so `HasNoTransform` treats it as absent. `PrefabProcessor.AddMissingTransforms` then adds a generated Transform and the source Prefab hierarchy contains that generated asset. The original serialized pair still points to the missing external target. |
| Rust behavior | `ObjectWorkspace` decodes the no-TypeTree GameObject with its recovered TPK tree; scene processing now consumes that same payload instead of reparsing only the file TypeTree. The graph retains `m_Component[0].component` as `MissingDependency { external_index: 0, path_id: 99 }`, reports an unresolved component issue, creates no root candidate, and plans zero generated Transforms. Project processing remains incomplete because the dependency is absent. |
| Difference and repair | The raw component pair identifies class ID 4 and a non-null PPtr to a missing external object. Treating failed resolution as “no Transform” loses the distinction between a null component and an unavailable dependency and can add a duplicate Transform. Rust retains the unresolved identity and blocks completion rather than replacing it with null or synthesizing a potentially duplicate component. This is recorded as a correctness divergence from the source behavior; no baseline feature row is closed. |
| Regression evidence | Fresh isolated source replay via `SceneProcessorProbe ... notree-unresolved-component` exited 0 and reproduced the committed rows and input hash. Rust `workspace_scene_processing_keeps_unresolved_no_type_tree_components_indeterminate` passes; `inspect-files` reports one relationship, one root issue, zero generated Transform plans, and the unresolved dependency. See `docs/validation-log.md`. |
| Remaining scope | This case covers one generated no-TypeTree GameObject layout and a missing external Transform. Other Unity versions, RectTransform pairs, multiple ordered components, loaded cross-file targets, scene-owned generation, project export, and dynamic Unity behavior remain under `OBJ-REF-001`, `SER-TT-ABSENT-001`, and `PROC-SCENE-001`. No user AssetBundle or UABEA content was accessed. |

## DIFF-OBJ-REF-NESTED-SIBLING-ORDER-001 — Duplicate collection names in sibling nested bundles

| Field | Finding |
|---|---|
| Status | A source-authored UnityFS project contains two sibling nested bundles with a SerializedFile named `target.assets`. Frozen AssetRipper resolves the root cross-file PPtr to `FirstTarget` in the first outer-bundle child; Rust now retains that same entry-order choice through project export. The later `SecondTarget` remains a distinct loaded and exported object. This closes only the tested ordering difference, not `OBJ-REF-001`. |
| Source location | Frozen source `AssetRipper/Source/AssetRipper.Assets/Bundles/Bundle.cs`, `Bundle.ResolveCollection`, searches child bundles in source order and returns the first matching collection. Rust `src/scheme.rs` retains each top-level mounted container tree in `BatchMountReport`; `src/serialized_file/workspace.rs` consumes that topology as a `PreExpandedContainer`; `src/main.rs` passes the mount report through workspace loading into the production `export-project` path. |
| Input evidence | `src/export/managed_reference_sibling_bundle_fixture.txt` records the source-authored input and output rows. The 4,288-byte outer UnityFS is SHA-256 `BFB304218F87B69214BCF817FC47157E55B7D018C2765A45E2B6D069A3D1E420`. Entry order is `managed.assets`, `first.bundle`, `second.bundle`; both child bundles contain a `target.assets` SerializedFile. The 816-byte first target has SHA-256 `CAD7A806C71F2C34ADEE3B0CB56589046F428F42CDA6D55E03FB28EB6F6BCF15`; the second has SHA-256 `D00F33BDDA415ABF3223DF15A8C0E494F9303FA203B6574DC1E63EC400979D76`. |
| Source behavior | The isolated source `ExportHandler` loads the root plus both targets and selects `FirstTarget` for the duplicate-name dependency. The captured source rows report `SELECTED|target.assets|FirstTarget|...`; the source target documents distinguish `FirstTarget` (health 789) from `SecondTarget` (health 456). |
| Difference and repair | Before the fix, generic batch import flattened the nested entries before constructing `SerializedFileWorkspace`, discarding `MountedContainerBundle` parent and entry-order metadata. The source-backed regression then selected `SecondTarget` in Rust. Initial root-container trees now travel from batch mount reports into workspace initialization, preserving the bundle hierarchy and the same first-child lookup behavior. Pathless root mounts receive an internal per-input identity so retaining the tree does not reject a previously valid mount with no recorded scope. |
| Rust behavior | The regression `export_project_uses_source_entry_order_for_duplicate_nested_bundle_dependencies` checks the production project-export path and joined graph. The graph resolves to `first.bundle/target.assets`; the root YAML maps only to `FirstTarget`'s generated GUID, both target YAML documents match their frozen source fixtures, and all three `.meta` files match after excluding only generated `guid` and `timeCreated` and canonicalizing the final newline. Rust reports three SerializedFiles, three objects, and three asset files; the original input bytes remain unchanged. |
| Regression evidence | Fresh source command `& "$env:LOCALAPPDATA\Microsoft\dotnet\dotnet.exe" run --project tools/probes/managed-project/ManagedProjectProbe.csproj --configuration Release -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref" -- export-sibling-bundle-reference-order target/sibling-order-run2/input target/sibling-order-run2/source-output target/sibling-order-run2/source-rows.txt` exited 0 and selected `FirstTarget`. `cargo run --locked --quiet -- export-project target/sibling-order-run2/rust-output target/sibling-order-run2/input/workspace.bundle` exited 0 with three SerializedFiles and three objects. The focused sibling-order regression and full locked workspace suite passed; details are in `docs/validation-log.md`. |
| Remaining scope | This fixture covers one source-ordered pair of sibling nested UnityFS bundles, one duplicate collection name, one cross-file PPtr, and generic MonoBehaviour export. Other container formats and nesting shapes, repeated names at other hierarchy levels, cross-bundle dependency discovery, ambiguous/missing/built-in references, complete reference identity mapping, all exporter families, and Unity Editor import/runtime remain open under `OBJ-REF-001` and `EXPORT-UNITY-001`. No user AssetBundle or UABEA content was accessed; Phase 2 remains locked. |



## DIFF-MANAGED-FORWARDER-HELPERS-001 — manager-generated helper assemblies and PluginImporter layouts

| Field | Finding |
|---|---|
| Status | The helper-generation and importer-layout behavior is matched for the source-probed mscorlib-v2 cases. This closes the tested helper-output behavior only; TYPE-SCRIPT-001 and EXPORT-UNITY-001 remain open. |
| Source location | Frozen Import/Structure/Assembly/ForwardingAssemblyGenerator.cs generates System.Runtime.CompilerServices.Unsafe and selected System.Runtime.InteropServices exported types from loaded mscorlib; Scripts/ScriptExportCollection.cs::OnAssemblyExported creates the PluginImporter. The source factory dispatch for class 1050 transitions to PluginImporter_2023_3_0_a15 at Unity 2023.3.0a15. Rust: src/processing/managed_assembly.rs, src/export/project_workflow.rs, src/export/script_hashing.rs, and the vendored dotnetdll writer. |
| Input and source fixtures | The mscorlib-v2 fixture is 4,608 bytes, SHA-256 53A2E6C134A91E88849DC7BD4D53E2EA27223FA609CE2CA70872A7DC523A0115. Frozen helper DLL fixtures are 1,536 bytes each: Unsafe SHA-256 E1776934471A4D0C64C99814A743848EF8FECFA02C94667C81287E68D738F8C3; InteropServices SHA-256 ED913BF776E22A32800ECCAD3D73223054120EF273BE1B1FB6139AAD030CF1A9. PluginImporter source fixtures cover Unity 2019.1.0f1 (v2 sequence), 2023.3.0a14 (v2 sequence), 2023.3.0a15 (v3 named map), and 2023.3.0a16 (v3 named map). |
| Source behavior | Isolated source exports route both helper DLLs for Unity 2019.1.0f1; Unity 2023.3.0a16 routes Unsafe only because the manager reference map does not select InteropServices. Fresh source probe exports at 2023.3.0a14 and 2023.3.0a15 confirm the PluginImporter layout change at the source factory boundary: serializedVersion 2 through a14 and serializedVersion 3 from a15, with the corresponding sequence-to-named-map shape. |
| Rust behavior and regressions | Production export synthesizes both helper types from loaded mscorlib and applies the reference-map Save routing. The Unity 2019.1 integration regression verifies both helper assemblies, timestamp-normalized Rust writer output, source MonoBehaviour YAML, and both normalized v2 PluginImporter sidecars. The Unity 2023.3.0a16 integration regression verifies the Unsafe-only output and its v3 sidecar. plugin_importer_layout_tracks_the_source_class_version_transition compares the complete importer body with four source metadata fixtures. |
| Independent PEReader evidence | The .NET 10 System.Reflection.Metadata comparison matches the source/Rust Unsafe helper model in 6/6 rows and the InteropServices helper model in 5/5 rows. Compared fields include module name/MVID, PE machine, assembly identity, assembly reference identity/culture/key token, exported type names/namespaces, attributes, and AssemblyRef implementation. |
| Remaining differences | These helper cases do not establish every managed assembly version, signing/resource form, script mode, or complete project output. ProjectSettings/EditorSettings.asset remains absent, and broad manager dependency discovery, other importer branches, Unity Editor import/runtime, and the parent product rows remain open. No user AssetBundle or UABEA content was accessed. |

## DIFF-TEXTURE-PROJECT-001 - Standalone generated Texture2D project export

| Field | Finding |
|---|---|
| Status | The no-Sprite generated class-28 project route now matches the frozen source image and `TextureImporter` output for inline and streamed source-authored fixtures at Unity 2019.4.40f1 and 2022.3.22f1. This closes that bounded source behavior only; `RES-TEXTURE-001`, `EXP-SETTINGS-APPLY-001`, and `EXPORT-UNITY-001` remain open. |
| Source locations | `Source/AssetRipper.Processing/Textures/SpriteProcessor.cs` creates a main-asset wrapper for a texture without a Sprite. `Source/AssetRipper.Export.UnityProjects/Textures/TextureAssetExporter.cs` registers image conversion, `TextureExportCollection.cs` writes the image and importer, and `ImporterFactory.cs` selects the versioned importer schema. Rust uses `src/export/texture_project.rs` and the normal collection-selection and project-output path in `src/export/project_workflow.rs`. |
| Input and source evidence | `ManagedProjectProbe export-texture-project` invokes frozen-source `ExportHandler.LoadAndProcess` and `Export`. Fresh isolated runs reproduced all non-comment rows of the four checked-in source fixtures: 2019.4 inline (256-byte SerializedFile, SHA-256 `FEBA34D41FD197985F568DBAF160B96862A0DB407812686F0C79D9C5DCD01A7E`); 2022.3 inline (280 bytes, `0BB1CD6279D159251DBE465A782F22C5F22988FF3B3FEF34F8BD10903306AC71`); 2019.4 streamed (264-byte SerializedFile, `025B8003387ADD7B2760EF27EDC3B4BA845402A70AADD23CFCBE7DB761B463CB`); and 2022.3 streamed (280 bytes, `C550E1643882E86B238158061B95ABCDE6E52850C77CB28C364EC85071C31F33`). Each streamed case uses a 16-byte `TextureFixture.resS` with SHA-256 `A8E82E241E77A364DA8B4C18A4C545BB1A1F9605242599FF6619B9F50C52C2D6`. The probe reports six source project files for each case. |
| Source output | With `ImageExportFormat=Bmp` and `PreferOriginalTextureExtension=false`, all four source outputs contain the same 138-byte `TextureFixture.bmp`, SHA-256 `2F42FAD6E338F9BB3C717BCDF1FDC9764275D3EF9905A16AE96C7EC2A8A7D4FC`. Both versioned `TextureImporter` sidecars are captured completely; only generated `guid` and `timeCreated` values are normalized for comparison. The source package manifest and ProjectVersion are also captured. |
| Rust behavior and regression | The production route simulates the source-created main-asset fact only for generated class-28 textures with no embedded file TypeTree or incoming Sprite reference and a recovered source TPK tree, selects the registered `TextureAssetExporter` through the normal registration walk, uses the configured image format, resolves declared streamed data through the bounded resource resolver, and emits the versioned importer sidecar. Minimal embedded-TypeTree class-28 objects remain on their prior YAML route. `export_project_matches_source_for_standalone_generated_texture_versions` passes all four source fixtures. It compares exact BMP bytes, complete normalized importer bodies, ProjectVersion, the package manifest after line-ending and terminal-newline normalization, and input preservation. Separate missing-resource and truncated-range cases fail before writing any project file. |
| Rust validation | `cargo test --workspace --locked export_project_matches_source_for_standalone_generated_texture_versions -- --nocapture` exited 0: one CLI test passed; the 1,573 library and 112 other CLI tests were filtered. `cargo fmt --all` completed before the focused run. Full workspace, Clippy, final format check, baseline verifier, and final diff check are recorded separately in `docs/validation-log.md` when run. |
| Remaining differences | The source path map now has a production route and source comparisons for the generated Texture2D cases; `ProjectSettings/EditorSettings.asset` remains absent, so this is not a full project-output match. The Rust importer layout is currently source-pinned only at the two listed Unity versions. Sprite-backed textures, Sprite atlases, lightmaps, other image-texture classes, other image formats and non-default importer field combinations remain unverified. Unity Editor import/runtime is unavailable without a valid license. No user AssetBundle or UABEA content was accessed. |
| Fixtures | `src/export/texture_project_source_2019_4_fixture.txt`; `src/export/texture_project_source_2019_4_streamed_fixture.txt`; `src/export/texture_project_source_2022_3_fixture.txt`; `src/export/texture_project_source_2022_3_streamed_fixture.txt`; `tools/probes/managed-project/Program.cs`; `src/export/texture_project.rs`; `src/export/project_workflow.rs`; `src/main.rs`. |
## DIFF-PATH-ID-MAP-PROJECT-001 — PathIdMap post-export integration

| Field | Finding |
|---|---|
| Status | The production `export-project` path writes `AuxiliaryFiles/path_id_map.json` and now matches all four source asset interface categories: Mesh, Texture2D, AudioClip, and TextAsset. The full generated interface/version matrix is source-derived and checked against every factory-generated row in those class families. This verifies `EXP-PATH-ID-MAP-001`; the broader `EXPORT-UNITY-001` product row remains in progress. |
| Source locations | Source/AssetRipper.Export.UnityProjects/PathIdMapping/PathIdMapExporter.cs; SerializedGameInfo source-generated JSON context; SerializedAssetCollection.Name; SpecialFileNames.FixFileIdentifier; GameBundle.FetchAssetCollections(). Rust wiring is in src/export/project_workflow.rs, src/export/project_exporter.rs, src/export/path_id_map.rs, and src/export/post_export.rs. |
| Selection evidence | tools/probes/export-collections/Program.cs --path-id-map-types reflects the isolated frozen AssetRipper.SourceGenerated assembly. src/export/path_id_map_generated_type_interfaces_fixture.txt pins the concrete generated (class ID, type name) membership for IMesh, ITexture, IAudioClip, ITextAsset, and INamed. The regression cross-checks the table against every non-throw generated type selected by source_factory_dispatch_fixture.txt in the relevant class families. Exact version boundaries include Shader_5 versus Shader_5_5 and class-109 CGProgram/ShaderInclude variants. |
| Source/runtime comparison | A fresh .NET 10.0.401 `ManagedProjectProbe export-path-id-map-project` run called frozen `ExportHandler.LoadAndProcess` and `Export` on a source-authored 832-byte Unity 2019.4.40f1 SerializedFile (SHA-256 `821FEB2CB7CE45575D9BDD1690D918742C39FB96063070C70DC9456041102D36`). Source emitted a 559-byte map (SHA-256 `A72BFB37C562ABA832F0F6E427993013A3138D7ACDE14527B60EF7B65D2ACCBA`) with Mesh/Texture2D/AudioClip/TextAsset at PathIDs 101/202/303/404 in object-table order. The Rust production CLI output matched exact bytes. A fresh `--path-id-map-types` reflection run reproduced all 331 committed type rows across 22 class IDs; Rust table tests check all 309 matching factory type/version rows. Existing source fixtures cover four class-28 inline/streamed cases at Unity 2019.4 and 2022.3, legacy collection output, and three nested sibling collections in source order. |
| Rust integration and validation | `build_path_id_map` consumes `SerializedFileWorkspace` and `ObjectWorkspace` order, applies `fix_file_identifier`, reads the name only for `INamed`, and fails closed on a missing source-generated type row. `cargo test --workspace --locked path_id_map -- --nocapture` exits 0 with 9 library tests and the new four-interface production CLI regression. The regression runs `export-project` on the source fixture and compares all 559 PathIdMap bytes, including each PathID, Name, Type, and ordering. The existing texture, legacy Prefab, and nested sibling-bundle source comparisons also pass. |
| Remaining differences | The dedicated PathIdMap row is verified for all four interface categories and the frozen generated type/version matrix. Windows production output matches the frozen source bytes; explicit serializer tests cover LF output, but no Unix runtime is available. Conditional StreamingAssets and GameAssemblies, `ProjectSettings/EditorSettings.asset`, full project output layout, Unity import/runtime, and the broader exporter pipeline remain open. No user AssetBundle or UABEA content was accessed. |
| Evidence | src/export/path_id_map_generated_type_interfaces_fixture.txt; src/export/path_id_map_source_2019_4_fixture.txt; src/export/managed_reference_sibling_bundle_fixture.txt; four src/export/texture_project_source_*_fixture.txt files; src/export/legacy_prefab_source_2017_4_probe_fixture.txt; tools/probes/export-collections/Program.cs; tools/probes/managed-project/Program.cs; docs/validation-log.md. |

## DIFF-AUDIO-YAML-EMPTY-FALLBACK-001 — Empty AudioClip YAML project fallback

| Field | Finding |
|---|---|
| Status | Frozen AssetRipper and the Rust production `export-project` path now match the bounded Unity 2019.4.40f1 empty AudioClip YAML fallback and normalized `NativeFormatImporter` metadata. This closes only the empty fallback behavior. `RES-AUDIO-001` and `EXPORT-UNITY-001` remain open. |
| Source locations | `Source/AssetRipper.Export.UnityProjects/Audio/YamlAudioExporter.cs` (`TryCreateCollection`); `YamlAudioExportCollection.cs` (`GetExportExtension`, `ExportInner`); source-generated `ClassID_83.AudioClip_2018_3`; Rust `src/export/project_workflow.rs` (`AudioYaml` route and generated default fields). |
| Source fixture and runtime | The source-authored 832-byte Unity 2019.4.40f1 `PathMapFixture.assets` is pinned in `src/export/path_id_map_source_2019_4_fixture.txt` (SHA-256 `821FEB2CB7CE45575D9BDD1690D918742C39FB96063070C70DC9456041102D36`). The isolated .NET 10 probe `export-path-id-map-project` emits `ExportedProject/Assets/AudioClip/AudioFixture.audioclip` (608 bytes, SHA-256 `D095CD9DF9BB19BB49E841079E94F1B4BE652E0C846B456FE0238786EACD018F`) and a 227-byte `.meta`. The fixture records normalized metadata SHA-256 `FC05A0DB7820CDFB9931C858D850CEF12CA902F73EE1A4D012AB13C16A5FA7D3`; normalization replaces only `guid` and `timeCreated` and canonicalizes line endings. |
| Rust route and regression | Production `export-project` routes a selected `YamlAudioExporter` to `.audioclip`, adds missing base PPtr/object fields, and emits `m_EditorResource` plus `m_EditorCompressionFormat` only for the generated `AudioClip_2018_3` class. The generated editor resource uses the source blank `m_Source` YAML scalar. `export_project_matches_source_for_empty_audio_yaml_fallback` compares complete YAML and normalized `.meta`, asserts no generic `.asset` was emitted, and confirms source input bytes are unchanged. |
| Validation | `cargo fmt --all` completed. `cargo test --workspace --locked audio -- --nocapture` exited 0: 24 library tests and 11 CLI tests passed, including the production source comparison. `cargo test --workspace --locked decoded_streaming_info_resolves_explicit_root_and_checks_its_range -- --nocapture` exited 0: one resource-range library test passed, including the full-`u64` materialization-limit case. The .NET 10 isolated source probe exited 0 and reproduced the fixture YAML hash. Full workspace verification passed after the resource-range descriptor refactor: `cargo test --workspace --locked --quiet` exited 0 (1,575 library tests passed, 1 ignored; 115 CLI tests passed; 1 doctest ignored); `cargo clippy --workspace --all-targets --locked -- -D warnings` exited 0; `cargo fmt --all -- --check` exited 0; `tools/verify-baseline.ps1` exited 0 and confirmed all 1,449 frozen-source files unchanged; `git diff --check` exited 0. The header-aware parity report exited 1 as expected with 386 valid rows, 24 non-verified rows, no malformed rows or duplicate IDs, and Phase 1 OPEN. |
| Remaining differences | This does not implement or verify nonempty decoded audio binary output, FSB5/Vorbis reconstruction, Ogg-to-WAV conversion, streamed `.resS` project routing, NativeAudio importer behavior, broader AudioClip version families, or Unity Editor import. Missing declared audio content must remain an explicit error in Rust; do not silently clear it to mimic the source collection's failure fallback. No user AssetBundle or UABEA content was accessed. |
| Evidence | `src/export/audio_project_fallback_source_2019_4_fixture.txt`; `src/export/path_id_map_source_2019_4_fixture.txt`; `src/resources/audio.rs`; `src/resource.rs`; `src/export/project_workflow.rs`; `src/main.rs`; `docs/validation-log.md`. |

## DIFF-AUDIO-YAML-STREAM-OFFSET-001 — Streamed AudioClip YAML sidecar range

| Field | Finding |
|---|---|
| Status | The frozen source's streamed YAML exporter writes the selected bytes but retains an offset relative to the original resource file. Rust writes the same bytes and rebases the range to zero so the emitted sidecar and YAML refer to the same content. The bounded behavior is covered end to end; `RES-AUDIO-001` and `EXPORT-UNITY-001` remain open. |
| Source locations | `Source/AssetRipper.Export.UnityProjects/Audio/YamlAudioExporter.cs::TryCreateCollection`; `YamlAudioExportCollection.cs::ExportInner`; `Source/AssetRipper.SourceGenerated.Extensions/StreamedResourceExtensions.cs::TryGetContent`, `GetContent`, and `CheckIntegrity`. Rust uses `src/resources/audio.rs`, `src/resource.rs`, and `src/export/project_workflow.rs`. |
| Input and source output | The source-authored Unity 2019.4.40f1 SerializedFile is 224 bytes (SHA-256 `E24212D6CB7E2E04F1C79D4756DF86F566C32DC69018C339F417514B5A1BF025`). Its `AudioFixture.resS` input is 15 bytes (SHA-256 `3E6F80EC051165D61F5285791860162D78CE6A56F769D516A042A90BFC36626D`); the AudioClip requests offset 6, size 3, which selects `ABC`. Frozen `ExportHandler.LoadAndProcess` plus `Export` emits a 653-byte `.audioclip` (SHA-256 `4BCAA827F2EFEC5F0026CF81375C584B3F0DBFADD903E9FBE63E2760222F6B4F`) with `m_Offset: 6` and a three-byte `.resS` containing `ABC` (SHA-256 `B5D4045C3F466FA91FE2CC6ABE79232A1A57CDF104F7A26E716E0A1E2789DF78`). The normalized 197-byte NativeFormatImporter `.meta` hash is `FC05A0DB7820CDFB9931C858D850CEF12CA902F73EE1A4D012AB13C16A5FA7D3`. The complete source output rows are in `src/export/audio_project_stream_source_2019_4_fixture.txt`. |
| Source discrepancy | `ExportInner` writes `resource.TryGetContent(...)` bytes to `filePath + ".resS"`, changes only `Source` for serialization, then restores the original `Source`, `Offset`, and `Size`. The emitted sidecar contains only the selected three bytes, while YAML retains offset 6 and size 3. The frozen `CheckIntegrity` requires `stream.Length >= offset + size`; this emitted range therefore fails its own range condition (`3 < 6 + 3`). When `TryGetContent` cannot produce data, the source collection clears the resource fields and serializes successfully, dropping declared data. Rust deliberately errors for missing or invalid nonzero ranges, matching the project's fail-closed requirements. |
| Rust behavior and regression | Production `export-project` resolves the 64-bit StreamedResource range through bundle, root, and platform-resource lookup; stages the range bytes at the planned `.resS` path; and writes the export-owned YAML resource with offset 0 and size equal to the sidecar bytes. The source-authored test compares the full YAML after changing only the offset from 6 to 0, exact sidecar bytes, normalized metadata, and both input files. A second regression verifies missing and truncated nonzero ranges fail before any asset, sidecar, or metadata file is created. |
| Validation and remaining scope | The fresh isolated .NET 10 probe exited 0. `cargo test --workspace --locked audio -- --nocapture` passed the 24 library audio tests and 13 matching CLI tests. The source and Rust fixtures establish one generated Unity 2019.4.40f1 YAML branch only. Audio decoding/conversion, FSB5/Vorbis, Ogg-to-WAV, binary/native project exporters, other AudioClip schema/version families, and Unity Editor import remain open. Unity import/runtime was not run because this workstation has no valid Editor license; no user AssetBundle or UABEA content was accessed. See `docs/validation-log.md`. |
| Evidence | `src/export/audio_project_stream_source_2019_4_fixture.txt`; `tools/probes/managed-project/Program.cs`; `src/export/project_workflow.rs`; `src/resource.rs`; `src/main.rs`; `docs/validation-log.md`. |

## DIFF-UNITYFS-STREAM-ENTRY-001 — file-backed UnityFS entry ranges

| Field | Finding |
|---|---|
| Status | The direct UnityFS mount path now matches the source's file-backed range behavior for single-block uncompressed entries and its materialized output behavior for empty, LZ4-compressed, and multiblock entries. This closes these bounded entry-stream behaviors only; `IO-STREAM-001` remains in progress. |
| Source locations | `Source/AssetRipper.IO.Files/BundleFiles/FileStream/BundleFileBlockReader.cs::ReadEntry`; `FileStreamBundleFile.cs::ReadFileStreamData`; `Streams/Smart/SmartStream.cs::CreatePartial` and `StreamType`. Rust uses `src/container/unityfs.rs`, `src/io/stream.rs`, and `src/scheme.rs`. |
| Input evidence | The isolated `.NET 10.0.401` `io-differential --unityfs-entry-streams` probe independently builds four UnityFS v6 files and reads them through frozen `SchemeReader.LoadFile`. `src/container/unityfs_stream_entry_source_fixture.txt` records all input and output bytes and runtime backing types: uncompressed single block, 147 bytes (SHA-256 `B698B2DDD42812F443D1A1434BD583C4C64989070C735B7F5DF196FD83095202`); empty entry, 122 bytes (`B5324DA2052450BC51279D3FE8BB2ECD577CADCA8012F85D676442DEDEC577B6`); one LZ4 block, 149 bytes (`1C5BD0218978290F673CA7409C52032E2837C955C565CBAE546BDB3D9C24F44C`); and an entry spanning two LZ4 blocks, 160 bytes (`DB3D6546CBD522198D08DC8DCF902DB33A78353D5D252E074CDD7F8F87F14B60`). |
| Source behavior and discrepancy | `BundleFileBlockReader.ReadEntry` returns `m_stream.CreatePartial(...)` whenever the directory has one uncompressed block, including a zero-length entry. Reflection shows that the resulting backing stream is `RandomAccessStream`; for LZ4 and multiblock entries, the returned resource uses `MemoryStream`. The source `SmartStream.StreamType` getter throws `InvalidOperationException` for `RandomAccessStream`, because its type switch omits that backing type. Rust retains the intended file-backed range and reports `SmartStreamType::File`; it does not reproduce this source diagnostic-property bug. All four source entry byte sequences match. |
| Rust integration and regressions | `UnityFsStreamBundle` parses header/metadata through the shared slice/stream validation path. `SchemeReader::mount_path` and batch mounting recursively retain direct SerializedFile/Resource entry streams; one uncompressed block remains source-backed, while compressed or multiblock output is materialized within bounds. The production regressions cover direct and batch leaf mounts, empty entries, LZ4 and multiblock bytes, nested-container byte limits, and nested source-generated collection topology. The source nested fixture compares the new stream route's scopes, bundle parent/entry links, serialized-file order, resource order, failures, and mounted byte counts against the established slice-backed route. |
| Validation and remaining scope | The frozen-source probe exited 0. `cargo test --workspace --locked --quiet` passed 1,581 library tests (1 ignored), 117 CLI tests, and 1 ignored doctest after fixing the direct stream probe to recognize SerializedFile entries. The direct UnityFS route is covered here; compressed outer wrappers and other container formats still use their existing materialization paths, downstream SerializedFile/object parsers still receive bounded contiguous bytes, and Unix retained-reader runtime evidence remains externally blocked under `IO-FS-IMPORT-PRESERVE-DIRECTORY-001`. No broad parity row closes. |
| Evidence | `src/container/unityfs_stream_entry_source_fixture.txt`; `tools/probes/io-differential/Program.cs`; `src/container/unityfs.rs`; `src/io/stream.rs`; `src/scheme.rs`; `docs/phase1-closure-inventory.md`; `docs/validation-log.md`. |
## DIFF-ENGINE-RESOURCE-POINTER-001 — explicit engine-collection pointer redirects

| Field | Finding |
|---|---|
| Status | Closed for explicit default-resource and builtin-extra collection names. The broader OBJ-REF-001 row remains in progress. |
| Frozen source behavior | AssetRipper.Export.UnityProjects/EngineAssets/EngineAssetsExporter.cs::TryCreateCollection calls IsEngineFile; that method recognizes SpecialFileNames.IsDefaultResource and IsBuiltinExtra, then creates SingleRedirectExportCollection(asset, asset.PathID, EGUID/FGUID, AssetType.Internal). AssetRipper.IO.Files/SpecialFileNames.cs defines two exact spellings for each family. |
| Source inputs and results | The isolated .NET 10.0.401 export-engine-resource-pointer probe creates Unity 2019.4.40f1 SerializedFile pairs and a root MonoBehaviour PPtr to engine PathID 900. Four fresh source runs returned exit 0. unity default resources and unity_default_resources emitted 0000000000000000e000000000000000:900:0; unity builtin extra and unity_builtin_extra emitted 0000000000000000f000000000000000:900:0. Each source project contains the ManagedRoot YAML and its metadata, with no standalone EngineResourceTarget asset. Fixtures preserve the complete input bytes, YAML, and normalized metadata. |
| Rust production route and evidence | src/export/project_workflow.rs::engine_resource_collection_guid selects EGUID/FGUID from the source-compatible exact-name predicates. The production route requires EngineAssetsExporter plus Redirect, records the original PathID as an Internal pointer, reserves the predefined GUID, and filters the redirect from standalone output. export_project_redirects_engine_resource_references_to_predefined_guid_and_path_id runs all four fixture cases through export_project_to; root YAML bytes match source exactly, normalized NativeFormatImporter metadata matches, no engine target asset is emitted, and input files remain unchanged. |
| Fixture SHA-256 | engine_resource_reference_project_fixture.txt: 6BD07A5F329AD206648439DA745364901287089C168CFD8A877EE1D04DDE78A9; engine_builtin_extra_reference_project_fixture.txt: 8B4BB078C8C9A64EA72C30D87C3F081059F4EDB96B5C12274CE437427A0D332C; engine_resource_underscore_reference_project_fixture.txt: CFA5AB242379C9B6310A08CC0D856DC9A72561718BD0EB57667108EA28788CC1; engine_builtin_extra_underscore_reference_project_fixture.txt: E3B7DDAC6124C2482609CB3F102CF2F412313EA5E5A4F9E0AFD4EE58F93ABA15. |
| Remaining scope | GameInitializer.EngineResourceInjector injection of TPK default/extra resource collections and PredefinedAssetCache lookup of embedded engine resources remain outside this work unit. Other engine-resource version branches and the full reference graph/export mapping remain open under OBJ-REF-001. No user AssetBundle or UABEA data was used. |
| Evidence | The four src/export engine-resource fixtures; tools/probes/managed-project/Program.cs; src/export/project_workflow.rs; src/main.rs; docs/feature-parity.md; docs/phase1-closure-inventory.md; docs/validation-log.md. |

## DIFF-LIGHTMAP-TEXTURE-ROUTE-001 — source-owned lightmap texture project route

| Field | Finding |
|---|---|
| Status | The frozen-source version capability, one Unity 2019.4 no-TypeTree lightmap route, and ten source-generated lightmap schema fixtures now match Rust output. This settles those bounded processor/export behaviors only; `RES-TEXTURE-001`, `EXP-SETTINGS-APPLY-001`, `PROC-SCENE-001`, and `EXPORT-UNITY-001` remain open. |
| Frozen source behavior | `EditorFormatProcessor` runs before `LightingDataProcessor`. `LightmapSettingsExtensions.ConvertToEditorFormat` sets editor defaults and `m_GIWorkflowMode`; `LightingDataProcessor` selects the first `ILightmapSettings`, requires `IRenderSettings`, creates a `LightingDataAsset` when the generated type supports `Has_LightingDataAsset`, links it from the settings object, copies Enlighten mapping/lightmaps/probes, and assigns ownership/path information. `ProjectYamlWalker` serializes the editor-visible generated class fields; the source fixture places copied lightmap entries in `LightingData.asset` and emits the generated manager fields and pointer in the Scene. `LightmapTextureAssetExporter.TryCreateCollection` selects a texture whose `MainAsset` is an `ILightingDataAsset`; its collection uses `ImporterFactory.GenerateTextureImporter`. That factory leaves `SpriteMeshType.Tight` intact on the specialized Lightmap collection, while ordinary `TextureExportCollection.AddSprites` sets `FullRect` when there are no sprites. |
| Version capability evidence | A fresh isolated .NET 10.0.401 probe exercised 32 generated `LightmapSettings` version selections. `Has_LightingDataAsset` is false at 3.5.0f1, 4.0.0f1, and 4.7.2f1, then true from 5.0.0f1 through 6000.0.0f1. The committed rows are in `src/processing/lightmap_settings_capability_fixture.txt` (SHA-256 `8D68E27BBB5217816290EF1B8046C9FB6541E8403E3026A965E4C8ACEBFE01F4`). |
| Source fixture and output | The isolated `export-lightmap-project` probe generated an 808-byte Unity 2019.4.40f1 no-TypeTree SerializedFile with class 157 LightmapSettings, class 104 RenderSettings, and class 28 Texture2D. Input SHA-256: `9F894E799A77E48EF8B85C47304091D1FD9792C9FF840DD2DC1CA34F9EBA9766`. With `LightmapTextureExportFormat=Image`, `ImageExportFormat=Bmp`, and original-extension preference disabled, AssetRipper emits `Assets/Scenes/lightmapfixture.assets/LightmapFixture.bmp`, its importer, a generated `LightingData.asset`, and a Scene `m_LightingDataAsset` pointer. The BMP SHA-256 is `2F42FAD6E338F9BB3C717BCDF1FDC9764275D3EF9905A16AE96C7EC2A8A7D4FC`; normalized importer SHA-256 is `735DF2086D1C6A58FB82AA7070FF5BC5852D19E8C22F7D452763CFAB9CDF7F19`. Two fresh source runs produced byte-identical normalized rows with SHA-256 `93951F8E3B73362F0154C5BBB34FB00291074D49BE010BA578779C8B53F462B4`, committed as `src/export/lightmap_project_source_2019_4_fixture.txt`. |
| Rust route and regression | `scene_wiring.rs` passes the parsed Unity version into the Scene hierarchy capability classifier. The classifier records the first LightmapSettings as owner even when the release payload omits `m_LightingDataAsset`. `project_workflow.rs` builds the generated class-1120 root, copies source lightmap fields, maps the Scene/LDA/texture pointers, writes the LDA and metadata, and routes the configured image beside the Scene. `workspace_yaml.rs` uses the reference editor TypeTree for manager output, retains the original release TypeTree for conversion-hint fallback, and merges only source values whose fields are editor-visible. `export_project_routes_lightmap_textures_beside_their_scene` compares complete normalized Scene and LightingData YAML, exact BMP bytes, complete normalized texture/LDA/Scene metadata, unique GUIDs and pointer targets, main file ID 112000000, and unchanged input bytes. |
| Versioned source fixture regression | `export_project_matches_source_lightmap_schema_families` compares normalized complete output for Unity 5.0, 5.3, 5.4, 5.5, 5.6, 2017.2, 2017.4, 2018.2, 2018.3, and 2022.3. Source evidence pins the LightingDataAsset `mainObjectFileID` absence at 5.5 and presence at 5.6, Scene `externalObjects` and `m_Scene` pointer at 2017.2, the distinct 2017.2 versus 2017.4 TextureImporter fields/order, and 2018 TextureImporter fields (`spriteSheet.internalID` is absent; `pSDShowRemoveMatteOption` is present in 2018.3). Generated GUID and timestamp normalization remains the only output normalization. |
| Remaining differences | The generated route is still fail-closed for scenes with GameObject/Transform/component candidates and for non-null RenderSettings object references; one Unity 5.6 class-108 Light route now matches both the complete Scene YAML and generated LightingDataAsset under `DIFF-LIGHTINGDATA-LIGHT-5_6-001`, while other Light versions remain fail-closed and renderer, terrain, reflection, and populated mapping paths need source-backed processing and output coverage. The current class-1120 route has a Unity 2018.3 guard although the class-157 capability probe finds `m_LightingDataAsset` from Unity 5.0; reconcile the base-field and `m_Scene`/`m_SceneGUID` layout boundaries before broadening. Other lightmap entries, EXR/YAML settings, additional importer/version/platform branches, and Unity Editor import remain open. No parity row closes from this bounded scenario. |
| Evidence | `src/export/lightmap_project_source_2019_4_fixture.txt`; `src/export/lightmap_project_source_5_0_fixture.txt`; `src/export/lightmap_project_source_5_3_fixture.txt`; `src/export/lightmap_project_source_5_4_fixture.txt`; `src/export/lightmap_project_source_5_5_fixture.txt`; `src/export/lightmap_project_source_5_6_fixture.txt`; `src/export/lightmap_project_source_2017_2_fixture.txt`; `src/export/lightmap_project_source_2017_4_fixture.txt`; `src/export/lightmap_project_source_2018_2_fixture.txt`; `src/export/lightmap_project_source_2018_3_fixture.txt`; `src/export/lightmap_project_source_2022_3_fixture.txt`; `src/processing/lightmap_settings_capability_fixture.txt`; `tools/probes/managed-project/Program.cs`; `src/processing/scene_hierarchy.rs`; `src/export/scene_wiring.rs`; `src/export/texture_project.rs`; `src/export/project_workflow.rs`; `src/main.rs`; `docs/validation-log.md`. |

## DIFF-LIGHTINGDATA-LIGHT-5_6-001 — Unity 5.6 Light records

| Field | Finding |
|---|---|
| Status | The source-generated Unity 5.6.0f1 class-108 Light route now matches complete Scene YAML and generated LightingDataAsset output for a default Light, a Light with nonzero BakingOutput, two Light components on one GameObject, and one Light on each of two root GameObjects. This adds independent evidence under open `PROC-SCENE-001` and `EXPORT-UNITY-001`; neither row closes. |
| Frozen source behavior | `LightingDataProcessor` visits scene `ILight` assets and `AddLight` appends their `SceneObjectIdentifier` to `LightingDataAsset.Lights`, targeting the Scene-exported Light fileID. When the generated Light exposes `BakingOutput`, the processor appends that value to `LightBakingOutputs`. The frozen editor TypeTree and generated YAML walk preserve editor field names and omit the release-only `m_BakingOutput` field from the Scene document. |
| Source fixture | The isolated frozen-source probe authored no-TypeTree Unity 5.6.0f1 SerializedFiles with LightmapSettings, RenderSettings, GameObject, Transform, and Light. Two fresh source runs of the zero-output case produced identical normalized rows (SHA-256 `42A34E05DBD7D9F990CB3F3B63799592423E8E2699927F0872DC64D498BA0E65`); its input SHA-256 is `33362E317309B192B197534DD3A308309DF1581ED5149583C112BB4033C34A7A`. A second fixture sets `Light.BakingOutput` to `ProbeOcclusionLightIndex=7`, `OcclusionMaskChannel=3`, and `LightmappingMask=258`; two fresh source runs produced identical rows (SHA-256 `535FB1598B9BA318DBA71A5B5D2B0341B2A7BC31BA68F3162CD47C22D6341BF1`), with input SHA-256 `206C192BDA4479D73C8C55DFDBD3DEAD0A3AFD2F8A52AD2B172CE99046C4599F`. A third fixture contains two class-108 Light components on one GameObject, with distinct baking outputs `(7,3,258)` and `(4,2,513)`; two fresh source runs produced identical rows (SHA-256 `35C410956DD89823D9B7E4C71F9555E3839BB1E27CEB13300E4DAC78167A14F9`) and input SHA-256 `AE7F03C17A4DFA2DCEEBB6545D2E56E7D8EB829F8AF3165E028CD23D7C9A56E1`. A fourth fixture puts one Light on each of two root GameObjects, with separate Transform components and BakingOutputs `(7,3,258)` and `(4,2,513)`; two fresh source runs produced identical rows (SHA-256 `8A68FBF60FDE379E80021233E843A184A8D87B67E735E1DE398F72236F111D34`), with input SHA-256 `7C492C3CEB832074FB44218AAC36A6ABC44BEA1ABD6C1D9E9837F9B48427AA81`. Source `LightingData.asset` pairs targets 105/109 with the corresponding ordered records. |
| Rust route and regression | The production project route validates class-108 Light identity and decoded payload, maps each Light through the Scene assignment fileID, and writes source-TypeTree-shaped Light and BakingOutput records. `export_project_maps_5_6_light_to_lighting_data` compares complete normalized Scene and `LightingData.asset` YAML plus metadata for all four fixture shapes, checks Scene-to-LDA GUID links, asserts ordered target IDs and distinct BakingOutput records for both same-GameObject and separate-GameObject cases, and verifies unchanged inputs. The initial full-Scene comparison found `m_ObjectHideFlags` aliased to `m_HideFlags`; Rust now uses the source editor TypeTree and preserves its generated field names for class 108. Type/shape mismatches remain errors. The route is bounded to Unity 5.6; other versions remain fail-closed without source evidence. |
| Remaining scope | Default/nonzero single Lights, two Lights on one GameObject, and one Light each on two root GameObjects are covered. Other Light versions and hierarchy arrangements, other LightingDataAsset arrays, Renderer/Terrain/Reflection routes, broader Scene processing, and Unity Editor import/runtime behavior remain open. No parity row closes. No user AssetBundle or UABEA content was accessed. |
| Evidence | `src/export/lightmap_project_source_5_6_light_fixture.txt`; `src/export/lightmap_project_source_5_6_light_baking_fixture.txt`; `src/export/lightmap_project_source_5_6_multiple_lights_fixture.txt`; `src/export/lightmap_project_source_5_6_multiple_gameobjects_fixture.txt`; `tools/probes/managed-project/ManagedProjectProbe_TextureLighting.cs`; `tools/probes/managed-project/Program.cs`; `src/export/project_workflow/workspace_identity.rs`; `src/export/project_workflow/lighting_asset.rs`; `src/main_tests_05.rs`; `docs/feature-parity.md`; `docs/phase1-closure-inventory.md`; `docs/validation-log.md`. |

## DIFF-GUID-TYPETREE-FIELDS-001 — GUID field names in serialized TypeTrees

| Field | Finding |
|---|---|
| Status | Unity YAML GUID conversion accepts both generated-member names (`m_Data_0_` through `m_Data_3_`) and serialized array-style names (`data[0]` through `data[3]`) and emits the same canonical GUID string. This fixes the production Scene manager export regression; broader type/version coverage remains open and no feature row closes. |
| Source location and representation | Frozen `AssetRipper.SourceGenerated.Extensions/GuidExtensions.cs::CopyValues` copies the four Unity GUID words through generated `Data_0_` to `Data_3_` members. The serialized `GUID` TypeTree form exercised by the Scene `OcclusionCullingSettings.m_SceneGUID` fixture uses four unsigned words named `data[0]` to `data[3]`; the existing GUID converter previously recognized only `m_Data_*_`. |
| Rust change and evidence | `src/export/value_yaml.rs` now records the actual matched field spelling and accepts one alias for each of the four word positions. `guid_type_tree_indexed_data_names_emit_unity_guid_strings` pins the indexed form. The production `export_project_writes_processed_scene_collections_and_skips_duplicate_build_paths` regression now successfully exports the GUID-bearing Scene manager. The indexed-tree regression input is a constructed fixture; this finding does not claim Unity-produced or Unity Editor validation. |
| Remaining scope | Additional GUID-containing generated object families and version-specific field layouts remain part of `EXPORT-UNITY-001`; Unity-produced and Editor import/runtime checks remain unrun. |
| Evidence | `Source/AssetRipper.SourceGenerated.Extensions/GuidExtensions.cs`; `src/export/scene_wiring.rs`; `src/export/value_yaml.rs`; `src/main.rs`; `docs/validation-log.md`. |

## DIFF-LIGHTMAP-MESHRENDERER-5_6-001 — Unity 5.6 lightmapped MeshRenderer data

| Field | Finding |
|---|---|
| Status | Open. The generated `LightingData.asset` projection, renderer identity, Mesh metadata, and complete normalized Scene YAML now match frozen source for the bounded Unity 5.6 MeshRenderer fixture with a MeshFilter-backed UvMesh pointer. Other Scene/editor-format and LightingData routes remain unresolved under `PROC-SCENE-001`. |
| Frozen source behavior | `LightingDataProcessor.AddRenderer` emits paired renderer-ID and RendererData entries when `GetLightmapIndex()` is not `ushort.MaxValue`; the target object is resolved through its Scene export pointer, both lightmap ST vectors are copied, and dynamic index is fixed at `ushort.MaxValue`. `EditorFormatProcessor` also applies editor defaults to Renderers and converts GameObject tags before project YAML export. |
| Source fixture and runtime | A fresh frozen-source probe authored a no-TypeTree 5.6.0f1 SerializedFile with one GameObject, Transform, and class-23 MeshRenderer (no MeshFilter), plus LightmapSettings, RenderSettings, and a 2x2 Texture2D. The MeshRenderer uses lightmap index 0 and `m_LightmapTilingOffset=(1,1,0,0)`. Two fresh source runs produced identical normalized rows. A second fixture adds a class-33 MeshFilter and class-43 Mesh; two further runs produced identical normalized rows. Input SHA-256: `ED168DD114B50120B4C68FEB69F56BA87B99FE36FC23BF8F741769A4A6BCCD6E`; fixture SHA-256: `F8B0467BA575DD86DD9DE46D99EB83CE0FB4DC9B9785F05209F946CD5E2D71F8`. The no-MeshFilter input/fixture hashes are `E7ACBBF93D61EF491DDD49AC82F37850BE5BB3A75656CA806E1CA95393F44681` and `2032D1E6D129ADA8BF25424C2CBCC0C9D75D99A07D6A3832A60B74EBDFEC72ED`. |
| Rust route and regression | `collect_lighting_data_renderer_inputs` reads class-23 objects in the Scene ID plan, resolves their GameObject and optional MeshFilter through exact workspace identities and reference-graph edges, then resolves the Mesh target through the project pointer map. `append_lighting_data_renderer_records` creates the source TypeTree-shaped arrays, uses the Scene assignment fileID, copies both ST vectors, and writes dynamic index 65535. The no-MeshFilter regression matches complete normalized `LightingData.asset` YAML/metadata; `export_project_maps_5_6_lightmapped_renderer_uv_mesh` matches the complete normalized MeshFilter-backed `LightingData.asset` YAML, verifies its Mesh fileID/GUID against the exported Mesh meta and Scene MeshFilter pointer, and confirms input bytes are unchanged. The regression also matches the source Mesh `.meta`, including `mainObjectFileID: 4300000`. |
| Observed Scene difference and root cause | The original full Scene differential showed release-format GameObject/Transform/Renderer YAML where AssetRipper emits editor-format YAML. Rust now uses the frozen Unity 5.6 editor TypeTrees for GameObject, Transform, MeshFilter, and MeshRenderer; applies the source tag conversion, Transform root-order/Euler-hint calculation, and Renderer editor defaults; and matches the complete normalized Scene YAML from this fixture. Regression: `export_project_maps_5_6_lightmapped_renderer_uv_mesh` compares the complete Scene output to the committed source row. |
| Remaining scope | The product route supports this bounded class-23 MeshRenderer LightingData path with no MeshFilter or one unambiguous MeshFilter/Mesh target. Its Mesh `.meta` matches source, including `mainObjectFileID: 4300000`. The other 17 source-generated renderer class IDs, static-batch GameObject flag propagation, custom TagManager-backed tags, other Light versions and mixed baking-output cases, Terrain arrays, Prefab-owned lighting data, and other Scene editor-format/version paths remain open under `PROC-SCENE-001`; this bounded source evidence does not close that row. No Unity Editor import/runtime validation was run. |
| Evidence | `tools/probes/managed-project/ManagedProjectProbe_TextureLighting.cs`; `tools/probes/managed-project/Program.cs`; `src/export/lightmap_project_source_5_6_renderer_fixture.txt`; `src/export/project_workflow/collection_planning.rs`; `src/export/project_workflow/lighting_asset.rs`; `src/export/project_workflow/workspace_identity.rs`; `src/main_tests_05.rs`; `docs/feature-parity.md`; `docs/phase1-closure-inventory.md`; `docs/validation-log.md`. |

## DIFF-ANIMATION-WORKSPACE-CONVERTER-001 — serialized clip conversion input

| Field | Finding |
|---|---|
| Status | The source-generated Unity 2022.3 no-TypeTree AnimationClip passes from `ObjectWorkspace` values through the Rust converter and matches the frozen source-runtime conversion output row-for-row, including float bit patterns. This verifies the SerializedValue-to-converter input path under `PROC-ANIM-CONVERT-001`; the streamed-key/frame and next-frame contract is separately verified under `PROC-ANIM-STREAMED-001`. It does not close the broader conversion or export path. |
| Source behavior | The frozen `AnimationClipConverter.Process` consumes `ClipBindingConstant`, streamed frames, dense and constant samples, scene facts, and `MuscleClipConstant`, then emits curves and initializes `MuscleClipInfo`. The source-runtime output is `src/resources/animation/clip_convert_fixture.txt` (5,137 bytes; SHA-256 `A4C1BA2733ABAB473E46E924CEF3D343E4E1EB69293E04195BC9009EB714CAD5`). |
| Rust implementation | `src/resources/animation/serialized_clip.rs::read_animation_clip_for_conversion` reads typed converter inputs from a decoded AnimationClip payload with duplicate, type, integer-range, and unsupported-shape errors. The `source_generated_nondefault_animation_clip_materializes_through_object_workspace` regression applies `process_clip` to the loaded payload and compares the complete normalized converter record. A second source-generated fixture now verifies existing curve arrays are preserved. |
| Remaining scope | This source-probe fixture continues to use its test scene table; production workspace scene facts and YAML serialization are covered separately by `DIFF-ANIMATION-PROJECT-EXPORT-001`. Unity dynamic sampling is unrun. |
| Evidence | `src/resources/animation/animation_workspace_fixture.assets`; `src/resources/animation/clip_convert_fixture.txt`; `src/resources/animation/serialized_clip.rs`; `src/resources/animation/converter_fixture.rs`; `tools/probes/anim-convert/Program.cs`; `docs/feature-parity.md`; `docs/validation-log.md`. |

## DIFF-ANIMATION-WORKSPACE-EXISTING-CURVES-001 — existing curve preservation

| Field | Finding |
|---|---|
| Status | A source-generated Unity 2022.3 no-TypeTree AnimationClip with pre-existing Vector3, Quaternion, float, and PPtr curves now loads through `ObjectWorkspace`, passes through the Rust converter, and matches the frozen AssetRipper output rows exactly. This adds independent verification under `PROC-ANIM-CONVERT-001`; the feature row remains non-verified. |
| Source input and oracle | `tools/probes/anim-convert/Program.cs::SeedExistingCurves` creates one curve/key per listed family with distinct values, envelopes, flags, weights, and a local PPtr. The source SerializedFile is `src/resources/animation/animation_workspace_existing_curves_fixture.assets` (4,592 bytes; SHA-256 `E9D19E70E0E6EC95F50A3A0499DEFC2D2625689DC92B613A699648A6D025630D`). The source-runtime `AnimationClipConverter.Process` output is `src/resources/animation/clip_convert_existing_curves_fixture.txt` (5,745 bytes; SHA-256 `75AB4F7650C2D4B9C47D588C2092809B4BE4F222A4C2CADFD6E3796637F5EA40`). The serialized keyframe layout omits `TangentMode`; the fixture uses the generated default value zero. |
| Rust implementation | `read_animation_clip_for_conversion` parses existing position, rotation, scale, Euler, float, and PPtr curve lists, including keyframe values/slopes/weights, loop envelopes, paths, flags, and pointer values. Version-gated curve rotation order and keyframe weight fields are checked explicitly. Script pointers preserve both FileID and PathID; workspace conversion resolves them against matching reference-graph edges and rejects missing, ambiguous, null, or non-MonoScript targets. |
| Regression evidence | `source_generated_existing_curves_materialize_and_survive_conversion` checks representative decoded fields, executes production `process_clip`, and compares the complete normalized output with the source-runtime fixture. The output check covers curve order, all float bit patterns, event preservation, envelopes, and PPtr mapping. |
| Remaining scope | This specific converter fixture still uses its source-probe scene table; the production graph-to-project path and Unity YAML curve writer are covered separately by `DIFF-ANIMATION-PROJECT-EXPORT-001`, and one external MonoScript binding is source-compared under `DIFF-ANIMATION-EXTERNAL-SCRIPT-001`. Broader pointer/file-index cases and Unity dynamic sampling remain open. |
| Evidence | `tools/probes/anim-convert/Program.cs`; `src/resources/animation/animation_workspace_existing_curves_fixture.assets`; `src/resources/animation/clip_convert_existing_curves_fixture.txt`; `src/resources/animation/serialized_clip.rs`; `src/resources/animation/converter_fixture.rs`; `docs/feature-parity.md`; `docs/validation-log.md`. |

## DIFF-ANIMATION-PROJECT-EXPORT-001 — processed AnimationClip reaches Unity project YAML

| Field | Finding |
|---|---|
| Status | The production workspace project-export path now invokes the AnimationClip editor-format pass, serializes converted values through the clip's TypeTree, and emits them through the `.anim` YAML route. The Unity 2022.3 source-generated 13-object regression traverses this path, matches the production workspace-derived converter result against every frozen source row and float bit pattern, round-trips the converted TypeTree value, and confirms converted curve data reaches the exported document. This adds independent verification under `PROC-ANIM-CONVERT-001`; the row remains unverified. |
| Source behavior | The frozen `EditorFormatProcessor` runs `AnimationClipConverter.Process` for generated AnimationClip assets before project export. The source runtime fixture `src/resources/animation/clip_convert_existing_curves_fixture.txt` pins existing curve preservation and converter-emitted fields for its Unity 2022.3 input. |
| Rust implementation | `src/export/anim_wiring.rs::process_workspace_animation_clips` builds workspace path roots and scene facts, gathers custom-curve lookup CRCs, resolves requested blend-shape names from Mesh values and material-property names from ordered Renderer material references, runs `process_clip`, and writes the modified value tree with `write_animation_clip_conversion`. Project YAML conversion receives this root for class 74. PPtrs added at converter-generated field paths map only through matching serialized `(fileID, PathID)` edges from the same source object, requiring all candidates to agree. |
| Regression evidence | `source_generated_workspace_builds_animation_roots_and_hierarchy_paths` calls full `export_workspace_yaml_project_with_settings` on `animation_workspace_existing_curves_fixture.assets`, uses workspace-derived hierarchy/avatar/TOS and Mesh/Material facts, compares the complete converter output with the frozen AssetRipper rows, finds the emitted `.anim`, checks the converted `Arm` curve plus Euler/PPtr curve arrays, and round-trips the converted clip value tree. |
| Remaining scope | This source-generated Unity 2022.3 fixture now compares the complete source and Rust `.anim` after normalizing output GUIDs through each project's `.meta` paths; the files match line-for-line (905 lines). The independent external Script identity projection remains bounded to one assembly and one input edge. Unity import/dynamic sampling, broader managed Script and pointer cases, remaining AnimatorController/state-machine processing, and other version branches remain open. No feature row is closed by this finding. |
| Evidence | `src/export/anim_wiring.rs`; `src/resources/animation/serialized_clip.rs`; `src/export/project_workflow/export.rs`; `src/export/project_workflow/planning.rs`; `src/export/workspace_yaml/object_conversion.rs`; `src/resources/animation/animation_workspace_existing_curves_fixture.assets`; `src/resources/animation/clip_convert_existing_curves_fixture.txt`; `docs/feature-parity.md`; `docs/validation-log.md`. |

## DIFF-ANIMATION-MANAGED-SCRIPT-PATHS-001 — Unity 2022.3 MonoScript checksum fields

| Field | Finding |
|---|---|
| Status | The bounded `PathChecksumCache.Add(IMonoScript)` field-path behavior is independently source-verified for Unity 2022.3.22f1 and exercised through the Rust workspace adapter. `PROC-ANIM-CUSTOM-CURVE-001` remains open. |
| Frozen source behavior | `PathChecksumCache.Add(IMonoScript)` resolves `SerializableType.Fields`, recursively adds only paths whose field type is primitive, descends through nested fields, and stops after depth 10. Each path uses AssetRipper's Unity CRC32 implementation. |
| Source input and oracle | The isolated source probe generated a no-TypeTree Unity 2022.3.22f1 SerializedFile containing a MonoBehaviour, its local MonoScript, and a TextAsset. Input length: 488 bytes; SHA-256: `F6895443304BC2A0C9FFBDFCAA43641C12A9E0EDFC6548DC71923D29329A15C7`. The source managed assembly is 5,120 bytes with SHA-256 `D6E4155E931CFBC44ED92A44CC2436873D71DFBE293E9C568304DD118F2D6389`. The eight `SCRIPT_PATH` rows and three source-cache lookups for `target`, `target.m_FileID`, and `target.m_PathID` (`<null>` for each) are stored in `src/processing/managed_assembly_path_checksum_2022_3_fixture.txt`. |
| Rust route and regression | `populate_animation_script_facts` reads the class-115 MonoScript from `ObjectWorkspace`, resolves its script identity against the loaded `ManagedAssemblyCatalog`, builds the schema with the SerializedFile Unity version, and inserts it under the workspace-global identity. `source_managed_monoscript_populates_animation_checksum_facts` loads the source-generated input and checks the eight positive source CRC/path lookups and three source `<null>` results for PPtr field paths through the production cache. This source comparison caught and removed Rust fabricated `m_FileID`/`m_PathID` child paths for `EnginePointer` schemas. |
| Remaining scope | A source-authored script-bound AnimationClip with external `fileID=1, PathID=7` is now exercised and its emitted curve identity is compared under `DIFF-ANIMATION-EXTERNAL-SCRIPT-001`. Wider assembly/script coverage, other pointer/file-index cases, other source version branches, and Unity dynamic sampling remain open; this finding does not close the feature row. |
| Evidence | `src/export/anim_wiring.rs`; `src/processing/managed_assembly_schema.rs`; `src/processing/managed_assembly_path_checksum_2022_3_fixture.txt`; `tools/probes/managed-project/ManagedProjectProbe_Schema.cs`; `tools/probes/managed-project/Program.cs`; `docs/feature-parity.md`; `docs/validation-log.md`. |

## DIFF-ANIMATION-EXTERNAL-SCRIPT-001 — external MonoScript binding in Unity 2022.3 project export

| Field | Finding |
|---|---|
| Status | A source-authored `AnimationClip` with `GenericBinding.Script` at input `fileID=1, PathID=7` resolves to a class-115 MonoScript in `scripts.assets`. Rust project export and the frozen AssetRipper project export emit the same health-curve `attribute`, `path`, `classID`, and Script pointer after normalizing the plugin GUID token. This verifies that bounded external Script identity path; full clip/project parity remains open. |
| Source inputs | Unity 2022.3.22f1 `animation_workspace.assets`: 4,624 bytes, SHA-256 `BDA77CF1738A2FA48822B9CBAB188A34716B332FCDD127AEC67EC4F14580D232`; `scripts.assets`: 488 bytes, SHA-256 `F6895443304BC2A0C9FFBDFCAA43641C12A9E0EDFC6548DC71923D29329A15C7`; `ComplexFixtureScripts.dll`: 5,120 bytes, SHA-256 `D6E4155E931CFBC44ED92A44CC2436873D71DFBE293E9C568304DD118F2D6389`. The source project uses plugin GUID `b19bcd3f8dffca40198d81b88e4464bb`. |
| Rust route | `read_animation_clip_for_conversion` retains both Script PPtr components. `materialize_script_references` matches the original `(fileID, PathID)` against the clip's reference-graph edges, requires a unique resolved class-115 target and workspace identity for conversion, then restores the source PPtr before writing the converted clip. The project export also writes the assembly `.meta` consumed by the curve. |
| Compared output | The test `source_external_monoscript_binding_matches_project_yaml_identity` loads both SerializedFiles and the assembly through production workspaces, verifies the external edge and class ID, performs project export, and compares the emitted health curve identity and keyframes to the frozen source projection. The source and Rust output fields match: `attribute: health`; `path: path_0xB6C65665_WvpMuNH`; `classID: 114`; `script: {fileID: -915535769, guid: b19bcd3f8dffca40198d81b88e4464bb, type: 3}`; both outputs also contain two health-key `tangentMode: 1` values. |
| Remaining scope | The full source and Rust `.anim` outputs each have 905 lines and match after normalizing each emitted GUID to the asset path whose `.meta` declares it; their byte hashes differ because export-generated GUIDs differ. The source projection pins the external Prefab member PPtr as fileID `1709254077376921`, type `2`; Rust now matches for both PPtr curve values and `pptrCurveMapping`. Unity import/dynamic sampling, wider assembly/script handling, other pointer/file-index cases, source version branches, and AnimatorController/state-machine behavior remain open. `PROC-ANIM-CONVERT-001` and `PROC-ANIM-CUSTOM-CURVE-001` stay non-verified; no feature row is closed. |
| Evidence | `src/export/anim_wiring.rs::source_external_monoscript_binding_matches_project_yaml_identity`; `src/resources/animation/serialized_clip.rs`; `src/resources/curve_wrappers.rs`; `src/resources/animation/animation_workspace_external_script_2022_3_fixture.assets`; `src/resources/animation/animation_external_script_target_2022_3_fixture.assets`; `src/resources/animation/animation_external_script_project_2022_3_fixture.txt`; `AssetRipper.SourceGenerated.Extensions/KeyframeExtensions.Float.cs`; `tools/probes/anim-convert/Program.cs`; `tools/probes/managed-project/ManagedProjectProbe_Animation.cs`. |


## DIFF-IO-STREAM-WORKSPACE-001 — file-backed SerializedFile workspace loading

| Field | Finding |
|---|---|
| Status | Direct SerializedFiles now keep a seekable input stream through metadata parsing and object-workspace materialization. Header and metadata are compared to the existing slice parser on both an independent source-runtime fixture and a large-gap read-count fixture; object payloads are read by their checked file ranges. |
| Rust change | `StreamSerializedFile` reads the bounded header and metadata region, then supplies one object range at a time to the existing asset factory and reference-graph walker. The no-TypeTree TPK recovery and MonoBehaviour file-TypeTree route use the same object payload bytes and source layout rules. Non-SerializedFile inputs keep the existing SchemeReader path. |
| Regression evidence | `source_runtime_serialized_file_fixture_matches_file_backed_metadata_and_ranges` compares metadata and every object range on the frozen source-authored standalone SerializedFile fixture. `file_backed_workspace_reader_matches_slice_parse_without_reading_the_data_gap` compares header, metadata, and decoded object value and asserts reads stay below half of a 1 MiB unused data gap. The 11 `object_workspace` tests and three `workspace_reference_graph` tests pass. |
| Remaining scope | The generic SchemeReader still materializes its final parsed layer; compressed wrappers and non-direct container routes retain their existing materialization paths. Unity-produced inputs and the full IO-STREAM row remain open; no feature status is closed. |

## DIFF-IO-STREAM-WORKSPACE-002 - wrapped SerializedFile workspace loading

| Field | Finding |
|---|---|
| Status | GZip and Brotli wrapper decoding can write bounded chunks to a temporary SmartStream, avoiding a complete decoded-layer Vec for SerializedFile workspace loading. |
| Input evidence | The standalone SerializedFile bytes are the frozen AssetRipper SchemeReader fixture in `src/io/serialized_file_scheme_fixture.txt`. The Brotli wrapper and its direct decoded bytes are the frozen source-runtime fixture in `src/compression/wrapper_scheme_runtime_fixture.txt`. |
| Rust change | `SchemeReader::unwrap_serialized_file_stream` recursively detects wrappers, enforces the existing input, depth, per-layer, and aggregate limits, and spools decoded output. `SerializedFileWorkspace` and `ObjectWorkspace` then parse metadata and read checked object ranges from that stream. |
| Regression evidence | `compressed_source_serialized_file_matches_both_workspace_loaders` compares header, metadata, reference graph, and all materialized objects for the same source SerializedFile with and without a GZip wrapper. `file_backed_wrapper_spooling_enforces_*` checks input, depth, per-layer, and aggregate bounds. `brotli_stream_sink_matches_the_frozen_source_decode` matches the source runtime's direct Brotli output byte-for-byte. The complete workspace suite passes. |
| Remaining scope | General SchemeReader dispatch still materializes its final layer; wrapped non-SerializedFile leaves and compressed/non-direct container entries remain on existing materialized routes. `IO-STREAM-001` remains in progress. No Unity user bundle or feature status was changed. |

## DIFF-IO-STREAM-UNITYFS-WRAPPER-001 — UnityFS entry wrapper decoding

| Field | Finding |
|---|---|
| Source location | `AssetRipper.IO.Files/FileContainer.cs` (`ReadContents`, `ReadContentsRecursively`); `CompressedFiles/CompressedFile.cs` (`ReadContentsRecursively`) |
| Input evidence | `src/container/unityfs_nested_wrapper_source_fixture.txt`, generated by the isolated IO.Files runtime probe from a C#-written UnityFS bundle with single- and double-GZip entries. |
| Source-derived behavior | The source output contains three ResourceFiles. It decodes one outer GZip layer for each entry; a double-GZip entry remains GZip-wrapped after that one layer. The source keeps decoded SerializedFile and UnityFS bytes as ResourceFiles instead of recursively parsing them. |
| Root cause | Rust's UnityFS entry fallback applied full SchemeReader dispatch to each resource payload, recursively parsing wrapper contents beyond the source container path. |
| Rust change | `mount_unityfs_stream_tree_at` decodes one wrapper layer into a bounded temporary SmartStream and mounts that output as a ResourceFile, preserving the source entry name and order. Wrapper, input, decompressed-size, and output-size failures remain errors. |
| Regression evidence | `source_unityfs_wrapped_entries_decode_one_layer_and_stay_resource_files` mounts the frozen bundle through `mount_path`, checks all three file-backed ResourceFile results against the source inventory, and confirms there are no parsed SerializedFiles. The focused regression passes. |
| Remaining comparison | Other UnityFS entry kinds, other container families, and generic final-layer materialization remain open under `IO-STREAM-001`. |

## DIFF-IO-STREAM-TOP-LEVEL-LEAF-001 — top-level wrapped ResourceFile mounting

| Field | Finding |
|---|---|
| Source location | `AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`, recursive source behavior); `CompressedFiles/GZipFile.cs`; `ResourceFiles/ResourceFile.cs` |
| Input evidence | `src/compression/gzip_resource_source_fixture.txt`, replayed from the isolated .NET 10 IO.Files runtime probe against the frozen AssetRipper assembly. The source identifies `gzip_plain_resource.bin` as a `GZipFile` containing a plain ResourceFile with bytes `677A69702077726170706564207265736F75726365206279746573`. |
| Source-derived behavior | A top-level GZip wrapper is decoded to one ResourceFile while preserving the leaf name and exact decoded bytes. The committed probe row was reproduced byte-for-byte from the isolated source runtime. |
| Rust change | `mount_path` and `mount_paths` now pass file-backed `SerializedFile` and `Resource` dispatch results to `mount_container_stream_at_path` instead of calling `into_dispatched` and materializing the final payload. Direct input streams are copied in bounded chunks to a temporary backing stream so the VFS does not retain a lock on the caller's source file; decoded wrapper streams already use temporary backing and are mounted directly. Existing output bounds and path validation remain active. |
| Regression evidence | `source_gzip_wrapped_resource_mounts_file_backed_through_both_path_apis` writes the source input to the source leaf name, exercises both production path APIs, checks one entry and the decoded bytes, and confirms each mounted file remains backed by `SmartStreamType::File`. The focused regression passed. |
| Remaining comparison | Other final leaf/container kinds, compressed/non-direct container entries, and Unix retained-reader runtime validation remain open. `IO-STREAM-001` remains in progress; this bounded case closes no parity row. |

## DIFF-IO-STREAM-WEBDATA-PATH-001 — top-level WebData streaming mount

| Field | Finding |
|---|---|
| Source location | `AssetRipper.IO.Files/SchemeReader.cs` (`LoadFile`, recursive source behavior); `AssetRipper.IO.Files/WebFiles/WebFile.cs`; `ResourceFile.cs` |
| Input evidence | Frozen source rows `SCHEME|webdata` in `src/io/io_differential_fixture.txt` and `SCHEME|webdata_gzip_entry` in `src/io/io_differential_stream_fixture.txt`, with exact source-mounted inventories and payload bytes. |
| Source-derived behavior | Top-level UnityWebData entries are recursively dispatched in directory order; the GZip entry becomes a ResourceFile with the decoded payload and original entry name. |
| Rust change | `mount_path` and `mount_paths` route direct UnityWebData streams into the bounded directory reader. It reads only the header after a 20-byte prefix, then copies entry ranges with a fixed 64 KiB buffer into temporary SmartStreams before dispatch. Resource and SerializedFile results mount from their backing streams; other kinds retain the existing recursive dispatch path. Header, input, entry, nesting, path, and output limits remain enforced. |
| Regression evidence | `source_webdata_entries_mount_file_backed_through_both_path_apis` replays both frozen source rows through both production APIs, checks exact paths and bytes, requires file-backed mounted entries, and removes each input while the VFS outputs remain live. |
| Remaining comparison | This covers direct top-level WebData inputs. The source-probed UnityFS-nested case is recorded in `DIFF-IO-STREAM-WEBDATA-UNITYFS-001`; other Raw/Web container entry paths, generic final-layer parsing, and Unix retained-reader runtime validation remain open. `IO-STREAM-001` remains in progress. |

## DIFF-IO-STREAM-WEBDATA-UNITYFS-001 — UnityFS nested WebData stream mounting

| Field | Finding |
|---|---|
| Source location | `AssetRipper.IO.Files/BundleFiles/FileStream/FileStreamBundleFile.cs`; `AssetRipper.IO.Files/WebFiles/WebFile.cs`; recursive `SchemeReader` dispatch |
| Input evidence | Frozen source row `SCHEME|ufs_nested_webdata` in `src/io/io_differential_fixture.txt`; it contains one UnityFS `inner.webdata` entry and the source recursive inventory for two ResourceFiles. |
| Source-derived behavior | A UnityFS entry recognized as WebData is recursively mounted as a child container; its parent bundle index, parent entry index, nested scope, direct ResourceFile order, names, and bytes are retained. |
| Rust change | The file-backed UnityFS tree now sends a direct UnityWebData entry stream to `mount_webdata_stream_tree_at`, preserving the shared mount report and nested depth/active-byte limits. It no longer converts that complete child container to an owned `Vec` before reading the WebData directory. |
| Regression evidence | `source_unityfs_nested_webdata_mounts_file_backed_through_both_path_apis` replays the frozen row through `mount_path` and `mount_paths`, compares parent/scope metadata, resource order and paths, exact payload bytes, and file-backed output, then removes the source file while mounted VFSes remain live. |
| Remaining comparison | This covers the source-probed UnityFS direct-entry route. UnityWeb/UnityRaw nested WebData and other container entries, generic final-layer parsing, and Unix retained-reader runtime validation remain open; `IO-STREAM-001` remains in progress. |

## DIFF-IO-STREAM-WEBDATA-UNITYWEB-001 — UnityWeb nested WebData stream mounting

| Field | Finding |
|---|---|
| Source location | `AssetRipper.IO.Files/BundleFiles/RawWeb/RawWebBundleFile.cs`; `AssetRipper.IO.Files/WebFiles/WebFile.cs`; recursive `SchemeReader` dispatch |
| Input evidence | Frozen source row `SCHEME|web_nested_webdata` in `src/io/io_differential_fixture.txt`; the row records the UnityWeb header, compressed scene, nested WebData entry, and two exact ResourceFile payloads. |
| Source-derived behavior | The UnityWeb scene directory identifies `nested.webdata`; recursive scheme dispatch mounts that child as a WebData container and preserves its two ordered ResourceFiles and exact payload bytes. |
| Rust change | `mount_path` and `mount_paths` read a bounded UnityWeb header prefix and the declared compressed scene, decode it under the existing chunk limit, then copy each directory entry in bounded chunks to temporary `SmartStream` backing before dispatch. A direct WebData child recurses through the shared mount report and parent/scope metadata. Header, input, decompression, path, entry, nesting, and output checks remain enforced. |
| Regression evidence | `source_unityweb_nested_webdata_mounts_file_backed_through_both_path_apis` replays the frozen row through both production APIs, checks container parent indexes and scope paths, ResourceFile order and paths, exact payload bytes, file-backed output, and source-file release while mounted VFSes remain live. |
| Remaining comparison | This covers the frozen UnityWeb-to-WebData route. UnityRaw and other Raw/Web container entry paths, generic final-layer parsing, and Unix retained-reader runtime validation remain open; `IO-STREAM-001` remains in progress. |

## DIFF-IO-STREAM-RAW-UNITYFS-001 — UnityRaw nested UnityFS stream mounting

| Field | Finding |
|---|---|
| Source location | `AssetRipper.IO.Files/BundleFiles/Raw/RawBundleFile.cs`; `RawWebBundleFile.cs`; recursive `SchemeReader` dispatch |
| Input evidence | Frozen source row `SCHEME|raw_nested_unityfs` in `src/io/io_differential_stream_fixture.txt`; its inventory records a UnityRaw bundle containing one UnityFS bundle and the exact `data/file.bin` payload. |
| Source-derived behavior | The UnityRaw directory identifies `nested.bundle`; recursive scheme dispatch mounts that entry as a UnityFS container and retains its child ResourceFile path and bytes. |
| Rust change | The stream mount reads a bounded UnityRaw header prefix and metadata range, validates directory entries against the post-header data size, and copies entry ranges to temporary SmartStreams in 64 KiB chunks. A direct UnityFS child enters the file-backed UnityFS tree route. UnityFS ranges over temporary-file backing now remain file-backed temporary ranges; original in-memory stream behavior is unchanged. |
| Regression evidence | `source_unityraw_nested_unityfs_mounts_file_backed_through_both_path_apis` replays the frozen row through `mount_path` and `mount_paths`, checks bundle parent/scope metadata, ResourceFile path and bytes, file-backed output, and source-file release while the VFSes remain live. |
| Remaining comparison | This covers the frozen UnityRaw-to-UnityFS route. UnityRaw-to-WebData and other Raw/Web nested-container paths, generic final-layer parsing, and Unix retained-reader runtime validation remain open; `IO-STREAM-001` remains in progress. |

## DIFF-IO-STREAM-WEB-SCENE-001 — bounded UnityWeb scene decoding

| Field | Finding |
|---|---|
| Source location | `AssetRipper.IO.Files/BundleFiles/RawWeb/WebBundleFile.cs`; recursive `SchemeReader` dispatch |
| Input evidence | Frozen source row `SCHEME|web_nested_webdata` in `src/io/io_differential_stream_fixture.txt`; it contains a UnityWeb scene with a nested WebData bundle and two ResourceFiles. |
| Source-derived behavior | The last UnityWeb scene is decompressed, its directory is parsed, and nested entries are dispatched with the same ordered ResourceFile names and payload bytes. |
| Rust change | The compressed scene range is read as a bounded stream, the size-prefixed LZMA decoder writes directly to temporary-file backing under the declared output limit, only the bounded directory prefix is buffered, and entry ranges are copied in fixed-size chunks before dispatch. |
| Regression evidence | `source_unityweb_nested_webdata_mounts_file_backed_through_both_path_apis` replays the frozen row through both production APIs and compares parent/scope metadata, entry order and paths, exact bytes, file-backed output, and source-file release. `decodes_sharpcompress_encoded_size_prefixed_payloads` exercises the writer decoder against independent AssetRipper SharpCompress vectors and checks the output cap. |
| Remaining comparison | This removes the full compressed-scene and decompressed-scene `Vec` allocations from the file-backed mount route. Other UnityRaw/UnityWeb nested branches, parser paths that need contiguous bytes, and Unix retained-reader runtime validation remain open under `IO-STREAM-001` / `IO-FS-IMPORT-PRESERVE-DIRECTORY-001`. |

## DIFF-ANIMATOR-BLENDTREE-1D-2022-001 — Unity 2022.3 simple 1D BlendTree projection

| Field | Finding |
|---|---|
| Source location | `Source/AssetRipper.Processing/AnimatorControllers/VirtualAnimationFactory.cs` (`CreateBlendTree`, `AddAndInitializeNewChild`) and generated `BlendTree_2018_3` / `ChildMotion_5` APIs |
| Input evidence | A reference-runtime probe constructs a Unity 2022.3.0f1 AnimatorController, two AnimationClips, and a two-child Simple1D `BlendTreeNodeConstant`. The frozen output projection is committed as `src/resources/animator_blendtree_source_2022_3_fixture.txt` (SHA-256 `956F2050901BB6EE100B6A973BEC85D6FF2376616A64F9B87FF6B032F7B35DF2`). |
| Source-derived behavior | AssetRipper creates a class-ID-206 BlendTree with serialized version 1 and `HideInHierarchy`, resolves the blend event through TOS, emits min/max thresholds, and creates ordered version-2 ChildMotion records. Child time scales are reciprocals of clip durations; cycle offsets and mirror flags come from the corresponding leaf nodes. |
| Rust change | The 2022.3 project route reconstructs BlendTree and ChildMotion YAML subassets from the packed state constants, maps leaf clip pointers from the controller clip list, and preserves BlendTree references from AnimatorState. Cyclic or invalid node indexes fail with errors. BlendTree reconstruction outside Unity 2022.3 remains explicitly unsupported pending version-specific evidence. |
| Regression evidence | `simple_1d_blend_tree_matches_frozen_source_runtime_projection` compares the reconstructed field projection to the frozen runtime fixture and independently checks that each child motion points to the corresponding supplied clip YAML identity. |
| Remaining comparison | This verifies one Unity 2022.3 Simple1D BlendTree projection, not a full `.controller` project export. Nested state-machine hierarchy, 2D/direct BlendTrees, AnimatorController-wide source YAML comparison, and Unity Editor dynamic sampling remain open under `RES-ANIM-001`. |

## DIFF-ANIMATOR-NESTED-BLENDTREE-1D-2022-001 — Unity 2022.3 nested Simple1D BlendTree projection

| Field | Finding |
|---|---|
| Source location | `Source/AssetRipper.Processing/AnimatorControllers/VirtualAnimationFactory.cs` (`CreateMotion`, `CreateBlendTree`, `AddAndInitializeNewChild`) and generated `BlendTree_2018_3` / `ChildMotion_5` APIs |
| Input evidence | A reference-runtime probe constructs a Unity 2022.3.0f1 parent Simple1D tree, one nested Simple1D child, distinct blend parameters, and three clip leaves. The parent and nested projections are committed as `src/resources/animator_nested_blendtree_source_2022_3_fixture.txt` (SHA-256 `B24EA9D3A3B9966E33576F38019BCC79479C84A443406CBB13F3B9187A661235`). |
| Source-derived behavior | The parent ChildMotion points to a recursively generated BlendTree, uses time scale 1 and mirror false, and retains the nested node cycle offset. The nested tree retains its own blend parameter, thresholds, ordered clip motions, reciprocal duration time scales, cycle offsets, and mirror flags. |
| Rust change | Recursive controller motion export emits each BlendTree as a separate class-ID-206 document and uses its generated local fileID from the parent ChildMotion. Leaf motions continue to point to their original AnimationClip document identities. |
| Regression evidence | `nested_simple_1d_blend_tree_matches_frozen_source_runtime_projection` matches all 26 source projection rows and checks that the parent targets the nested document while nested children target the second and third supplied clip identities in order. |
| Remaining comparison | This covers one Unity 2022.3 nested Simple1D graph, not complete controller export. Nested state-machine hierarchy, 2D/direct BlendTrees, other version branches, full `.controller` source YAML comparison, and Unity Editor dynamic sampling remain open under `RES-ANIM-001`. |

## DIFF-ANIMATOR-NESTED-STATE-MACHINE-2022-001 — packed controller hierarchy to Unity project

| Field | Finding |
|---|---|
| Source location | Frozen `AnimatorControllerProcessor.Process`, `AnimatorStateContext`, `AnimatorStateMachineContext`, `VirtualAnimationFactory`, and `YamlWalker` |
| Input evidence | `animator_nested_state_machine_source_2022_3_fixture.txt` contains source-authored Unity 2022.3.0f1 release SerializedFile bytes without TypeTree, projections, object identities and all 24 source YAML documents; SHA-256 `E2EE06FF89D32C85DC25B8DE75A840DE88665A9992A8B27F062C3369B64BDB02`. Source-authored 24-object NoTarget editor input is `animator_nested_state_machine_editor_source_2022_3_input.hex`, SHA-256 `C170EE11FB131C9AE304BEFD84DD908A66FC9CB65FDE5A517EC48EEA8D810A16`. |
| Rust change and evidence | Release project export reconstructs four nested machines and four globally indexed states, entry/default transitions, parent-owned child exits, suppressed default exits, root AnyState and editor positions. All 24 exported YAML objects match source after mapping local identities and raw source PPtr rendering. Regressions also verify synced-layer ownership, reserved root fileID, invalid destinations, and preservation of the source-authored editor hierarchy. |
| Remaining comparison | Controller behaviours, shared-controller claims/cloning, editor hierarchy collection grouping, legacy and other version branches, other BlendTree types and Unity runtime sampling remain open under `PROC-ANIM-001` / `RES-ANIM-001`. |

## DIFF-ANIMATOR-EMPTY-STATE-MACHINE-2022-001 — empty path and parent recovery

| Field | Finding |
|---|---|
| Source location | `AnimatorStateMachineContext`: definitive exit parents, parent restrictions, state/selector proposals, linked-parent sharing, fallback, `GenerateFullPathsForUnknownStateMachines`; `Crc32Algorithm.ReverseUTF8` |
| Input evidence | The same frozen runtime probe adds seven unknown empty selector groups without their path strings in TOS, a Chinese root, and discriminating transition connections. `animator_empty_state_machine_source_2022_3_fixture.txt` SHA-256 `0E727E28ADCEA9D60A735B0376BF07D0682CAD9475FA8A6E19D9F641576D119A` contains independent no-TypeTree input, hierarchy projections and all 41 source YAML documents. |
| Rust change and evidence | Product export now infers definitive/possible parents in source order, enforces direct/subtree restrictions, shares deepest known parents, generates CRC-preserving UTF-8 `EMPTY_` paths, and reproduces source Extra StateMachine/New State fallback. All 41 project YAML documents and projected graph fields match source. Cyclic definitive parents fail closed; the generated default state does not expand valid packed destination indices. |
| Difference found and fixed | The first focused execution exposed missing Extra StateMachine layout: source `SetChildrenPositions` visits it explicitly. Its generated default state now participates in the child count and grid; source expectations were unchanged. |
| Remaining comparison | Controllers without any states but with nested selectors remain explicit errors; wider controller/version coverage and the unresolved domains above are not claimed complete. |

## DIFF-ANIMATOR-BEHAVIOUR-COLLECTIONS-2022-001 — behaviour bindings and controller ownership

| Field | Finding |
|---|---|
| Source | `AnimatorControllerExtensions.GetStateBehaviours` / `FetchEditorHierarchy`, `VirtualAnimationFactory`, `AnimatorControllerExportCollection`, `GameAssetFactory.ReadMonoBehaviour`, `UnloadedStructure.LoadStructure`, and `Pass108_WalkMethods.GetName`. |
| Independent input | `AnimConvertProbe.dll controller-behaviour-state-machine` produces no-TypeTree release/editor inputs and complete source YAML; `controller-shared-editor-state-machine` authors a second controller sharing the same editor graph. Behaviour fixture SHA-256: `9417A557B01B8FEAE29A3F54860EC5533A3288B18825286D19FEBF12A79728A2`; shared editor input: `6CEAC47D83C738B200A7E808E138D1CAD36C1222A8ABF62C839FF42DD01EE238`. Regenerating the source projection retains the same hash. |
| Implementation | Compiled layer/state ranges select ordered behaviour slots; states retain explicit null slots and machines omit them. Selected behaviours join the controller collection. Editor controllers retain their complete existing hierarchy. Shared members have collection-local copies and canonical external identities. Preparation and pointer registration remain bounded; rejected copy batches do not partially change mappings. Generated animation members retain their original serialized YAML names. |
| Recovery correction | Initial materialization still preserves source `UnloadedStructure`, as required by the frozen generated-default matrix. An explicit later step resolves only a null-script header with no unread bytes, and rebuilds affected reference scans from bounded decoded object values without rereading a complete file. Only exact repaired factory failures are removed; earlier scan limits and unrelated failures remain explicit. |
| Evidence | Full YAML comparison covers the 26-document behaviour graph, all 24 existing editor documents, and both complete 26-document shared editor outputs after local identity mapping. CLI export covers release, editor and shared inputs, checks all local targets, distinct metadata GUIDs and unchanged input. Tests retain overflow, non-null-script, unread-byte, earlier-scan-limit and allocation-limit rejection. |
| Remaining | This does not establish the complete native ExportHandler shared-release cloning behavior, state-free nested selector reconstruction, other version/controller branches, or Unity runtime sampling. `PROC-ANIM-001` / `RES-ANIM-001` remain open. |

## DIFF-GENERATED-EDITOR-SETTINGS-PROJECT-001 — generated project configuration output

| Field | Finding |
|---|---|
| Source | `SceneDefinitionProcessor.Process` always creates EditorSettings; `EditorSettingsExtensions.SetToDefaults` sets only available generated properties; `ManagerExportCollection` writes anchor 1 without an importer sidecar. |
| Existing evidence | Reuses `audio_project_stream_source_2019_4_fixture.txt` and `texture_project_source_2022_3_fixture.txt`; no existing source output was changed. Source-runtime enum inspection confirms Generic=1, AssetPipeline Version2=1, CacheServer AsPreferences=0 and the two play-mode flags 1/2. |
| Implementation and validation | Project export constructs the editor layout from frozen TPK data, applies the existing settings model, writes `ProjectSettings/EditorSettings.asset`, and replaces imported settings at that destination without duplicate paths. Complete bytes match source for Unity 2019.4.40f1 and 2022.3.22f1. The existing inline/streamed texture CLI cases now compare EditorSettings and the entire source output file inventory as well as all previously checked artifacts. |
| Remaining | Conditional StreamingAssets/GameAssemblies, other generated settings, complete settings application and Unity Editor import remain open under `EXPORT-UNITY-001`. Phase 1 remains OPEN; no user VRChat bundle or UABEA input was accessed. |

## DIFF-PROJECT-CONDITIONAL-OUTPUTS-001 — original StreamingAssets and manager assemblies

- Source: frozen `Project/StreamingAssetsPostExporter.cs`, `Scripts/DllPostExporter.cs`, `BaseManager.GetStreamForAssembly` / `ClearStreamCache`, and the platform StreamingAssets paths. The previous product path omitted both conditional output families.
- Implementation: the planner retains bounded original stream ranges and directories; the writer copies in 64 KiB chunks with add-only, confined paths. The CLI keeps an original physical tree separately from the expanded processing VFS, including wrapper bytes, physical split parts and empty directories. Auxiliary GameAssemblies saves all catalog assemblies independently of MonoScript references or script export mode, with no `.meta`; Plugins output is reused when available. PE serialization still requires bounded contiguous buffers.
- Settings: the project CLI retains serialized import/export records rather than dropping Import. StreamingAssetsMode controls post-export copying and platform inventory; ScriptContentLevel.Level0 disables the manager. Other import/processing integration, including filtering before recursive mounting, remains open.
- Independent evidence: `tools/probes/managed-project/ManagedProjectProbe_PostExport.cs` calls the frozen post-exporters on generic inputs and an existing managed assembly. `src/export/post_export_source_2022_3_fixture.txt` captures original bytes, directories, all conditional skips, and the source-saved assembly (SHA-256 `7F433BEF9948E2B5DAE3FC4C686B591C2ABF6DCA184B1A7CD2B5BFA823C0E949`). Rust compares copied bytes/directories and the saved assembly identity/type/field model. A CLI project regression exercises Extract, persisted Ignore, and disabled scripts while preserving every original input.
- Fail-closed checks: entry/aggregate-byte limits do not read copied payloads; path escapes, reserved Windows names, sidecar collisions and file-as-directory conflicts are rejected before consented output-root deletion. This closes the bounded conditional-output gap; source-manager dependency closure, all assembly processors, broader exporters and Unity runtime acceptance remain unclosed. Phase 1 is OPEN; no user AssetBundle was accessed.

## DIFF-REAL-AVTR青岚古境-CONTROLLER-GRAPH-001 — unnamed AutoDresser machines

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, Unity 2022.3.22f1, SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`. Frozen source output: `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`; current Rust output: `target/real-acceptance-20261011-avtr青岚古境-blendtree-serialization-r1`. |
| Source behavior | `AnimatorStateContext.Process` creates states from `StateConstantArray`. `AnimatorStateMachineContext.AssignStateMachineChildStates` skips a state-machine context with an empty `Name`; otherwise `AddStateAndTransitionsToStateMachine` emits each state's `TransitionConstantArray`. |
| State pairing | In `FX_Milfy 1.controller`, source's `AutoDresser_Comp` and `AutoDresser_Decomp` machines have blank names and empty `m_ChildStates`; each has six ordered entry transitions to external `.state` assets. Pairing those ordered destinations with Rust's ordered child states yields 12/12 matching state names, scalar state fields, canonical motion targets (`Assets/AnimationClip/Empty_0.anim#7400000`), and byte-equal behavior YAML payloads. The behavior script in all pairs maps to `VRCAvatarParameterDriver.cs`. |
| Input-level transition evidence | The original `AnimatorController` object 3272 (PathID `207285344989274544`, `FX_Milfy 1`) was decoded independently through the frozen source parser using `controller-bundle-constants` in `tools/probes/anim-convert/Program.cs`; JSON: `target/real-acceptance-20261011-avtr青岚古境-controller-source-raw-r1.json`. Layer bindings resolve to `AutoDresser_Comp` (machine 22) and `AutoDresser_Decomp` (machine 23). Each machine has six states, and both selector tables contain index 0 (`m_IsEntry=true`) and index 1 (`m_IsEntry=false`), both with `m_FullPathID=0`. All 24 state `TransitionConstantArray` entries have `m_DestinationState=30001`; the Rust decoder's `30,000` selector flag maps this to selector index 1, the exit selector. Comp has one condition per state: `AutoDresser_Local`, mode 7, thresholds 0 through 5. Decomp has three zero-threshold conditions per state over `AutoDresser_Bool0/1/2`, with modes `[1,1,1]`, `[2,1,1]`, `[1,2,1]`, `[2,2,1]`, `[1,1,2]`, `[2,1,2]` in state order. The source `m_TOS` table resolves the event IDs and state names. |
| Frozen source processing | The same probe invokes the frozen source `AnimatorControllerProcessor.Process(GameData)` on the unchanged bundle; projection: `target/real-acceptance-20261011-avtr青岚古境-controller-source-process-r1.json`. Both named layers resolve to blank-named root machines with zero child states and six entry transitions. The entry destinations are the six `AutoDresserToN` states, whose processed outgoing transition lists are empty. Comp entry conditions use mode 6 for `AutoDresser_Local`, thresholds 0 through 5; Decomp entry conditions use Bool0/Bool1/Bool2 modes `[2,2,2]`, `[1,2,2]`, `[2,1,2]`, `[1,1,2]`, `[2,2,1]`, `[1,2,1]`. This is source processing evidence, not Unity runtime sampling. |
| Transition difference | The 13 standalone source `.state` files have empty `m_Transitions`. Rust attaches six states to each machine and emits the 24 packed-state transitions; every emitted transition targets the state-machine exit (`m_IsExit: 1`, null `m_DstState`). The independently source-decoded constants agree with Rust's emitted destinations and conditions. Frozen source `AnimatorStateMachineContext.AssignStateMachineChildStates` skips both unnamed machine contexts, explaining why source project export emits no children or those transitions. |
| Count normalization | Source puts 13 states and 41 BlendTrees in standalone `.state`/`.blendtree` files; Rust stores the corresponding counts as controller subassets. Including those files yields the same total AnimatorState and BlendTree counts (639 and 174 respectively). The current comparison harness omits these file extensions, so its raw class-count and path deltas are not semantic evidence. |
| BlendTree comparison | `tools/compare-controller-blendtrees.ps1` recursively compares every class-ID-206 document and ordered child motion. The run at `target/real-acceptance-20261011-avtr青岚古境-blendtree-graph-r1.json` reports 174/174 documents, 156/156 unique semantic graphs, 631/631 child records, and zero differences. It maps external object pointers by asset path and normalizes omitted runtime defaults (`serializedVersion=1`, `IsAnim=false`) using the frozen source-runtime projection. |
| Classification | The source export omission is explained by its empty-name guard. Frozen source parsing independently supports the packed constants, and frozen source processing independently confirms the blank-machine projection and entry-transition conditions. Unity Editor sampling is unavailable, so runtime equivalence and the behavior of the recovered unnamed hierarchy remain unverified. |
| Next evidence | Sample the two layers in Unity Editor at discriminating parameter values and verify state selection, transition timing, motion output, and exit behavior. Until then, keep runtime equivalence and full sample acceptance open. |

## DIFF-ENGINE-PREDEFINED-MATERIAL-2022-3-001 — Unity 2022.3 predefined Material cache identity

| Field | Finding |
|---|---|
| Frozen source | `EngineAssetsExporter.TryCreateCollection` first checks engine collection names, then calls `PredefinedAssetCache.Contains(asset)`. For `IMaterial`, the cache key is the exact material name and resolved shader name; `TryAdd(Material)` stores the engine TPK PathID/GUID. The source `EngineAssetsTpk` entry `2022.3.11f1` has `ExtraResources[10303] = Material("Default-Material", "Standard")`, with FGUID. |
| Input and before-fix evidence | 青岚雅苑` (SHA-256 `d923d39e1a67d9cfc2d67baacd985f756400b639b2489efa4b290241708d70b5`) contains a class-21 `Default-Material` whose `m_Shader` resolves to `Standard`. The first Rust project export wrote a standalone `Assets/Material/Default-Material.mat`; the generated Cube MeshRenderer pointed to its generated GUID/fileID. The frozen source project omitted that asset and pointed to builtin-extra PathID 10303. |
| Rust change | `predefined_materials_2022_3_11.tsv` stores the Material identity keys and resource targets. `object_routing.rs` resolves `m_Shader` through the multi-file reference graph and redirects only an exact `(name, shader name)` match for Unity 2022.3 patch 11 or later. The redirect retains the TPK PathID and EGUID/FGUID; no sample path, hash, or object PathID is used to select the rule. |
| Verification | The focused catalog test passes. A fresh Rust export of the unchanged 松月竹庭 input omits `Default-Material.mat`; the Cube renderer PPtr is exactly `{fileID: 10303, guid: 0000000000000000f000000000000000, type: 0}`, matching frozen source. Whole-project replay reports 114/114 matching input object rows and 1788/1788 YAML documents. The dedicated BlendTree comparison reports 3/3 documents, 37/37 children, and zero differences. |
| Remaining scope | This verifies only the 15 Material identity rows in the source 2022.3.11f1 table. Other TPK versions and other predefined asset classes remain open. The whole-project report still has 4/5 path-count differences and caps both reference-difference lists at 100 entries; Unity Editor import/runtime checks are unavailable. The separate unnamed AutoDresser AnimatorController runtime blocker remains unresolved, so the sample and overall product are not accepted. No UABEA inspection was run. |

## DIFF-REAL-AVTR青岚古境-SCENE-EDITOR-FORMAT-001 — Unity 2022.3 prefab scene-object YAML

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, Unity 2022.3.22f1, SHA-256 `cb65e179f0bee693f53fcebdb7836fa12e8ee854082c648388e26b87b152f2bb`. Frozen source project: `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`; Rust replay: `target/real-acceptance-20261011-avtr青岚古境-editor-format-r1`. |
| Observed before fix | The Rust prefab serialized release layouts for GameObject and Transform. For example, source emitted `m_TagString: Untagged`, `m_RootOrder`, and `m_LocalEulerAnglesHint`; Rust emitted the release `m_Tag: 0` and omitted the editor-only Transform fields. This disagreed with the real source project output. |
| Frozen source behavior | `EditorFormatProcessor.Process` visits release collections. `GameObjectExtensions.ConvertToEditorFormat` maps the numeric tag through TagManager; `EditorFormatConverterAsync.Convert(ITransform)` computes root order and Euler hints; `EditorFormatConverter.Convert(IRenderer)` applies the editor renderer defaults. `ProjectYamlWalker` serializes editor-visible fields. |
| Rust change | `should_use_scene_editor_type_tree` now selects versioned editor TypeTrees for generated class IDs 1, 4, 23, 33, and 108 for Unity 2022.3, while retaining the existing 5.6 route. The existing tag, Transform, and Renderer conversions run on the editor-format root. The selection is version/class based and does not inspect sample names, hashes, or object IDs. |
| Verification | The focused version-gate test passes 1/1, the workspace YAML module passes 23/23, the Unity 5.6 MeshFilter/Renderer regression passes 1/1, and the release build exits 0. Fresh real-sample export exits 0. The selected prefab has 4,491 source/Rust documents with common class/local-fileID keys; after canonicalizing each project's GUIDs through its own `.meta` asset path, all 1,828 GameObjects, 1,828 Transforms, 100 MeshRenderers, 100 MeshFilters, and 5 Lights match source YAML exactly. Evidence: `target/real-acceptance-20261011-avtr青岚古境-editor-format-r1-scene-objects.json`. |
| Remaining scope | This proves only these five object classes in this Unity 2022.3.22f1 avatar prefab. Custom TagManager tags, static-batch GameObject flags, other component classes/versions, and Unity Editor import/runtime are not closed. The broad comparator still pairs some documents by asset path; a separately checked source `MonoBehaviour_11` and Rust `MonoBehaviour_10` menu normalize to identical YAML, demonstrating one false same-path pair but not proving complete MonoBehaviour parity. The AutoDresser controller runtime behavior remains unverified, so sample acceptance stays open. |

## DIFF-REAL-AVTR云栖仙府-PROJECT-LAYOUT-001 — Animator subasset and generated-prefab path differences

| Field | Finding |
|---|---|
| Input | 青岚雅苑`, 58,150,845 bytes, SHA-256 `eb0113f5b26d2649dbacd9766dadd66db804e713b1970f7a0707c370ab6785c6`. Inspect reports version text `5.x.x` and revision `6000.0.60f1-DWR`; source and Rust exported projects both target `2022.3.22f1`. |
| Streaming inspection | 6,529/6,529 object payloads decode. StreamingInfo scan: 349 candidates, 226 Bundle ranges, zero scan errors, 1,839,476 structural values, `complete=true`. The 22 unresolved object-graph entries are all `PredefinedEngineAsset` references in `DefaultResources`: Mesh IDs 10210 (10), 10202 (6), 10207 (3), and Shader ID 10101 (3). They are not missing external Bundle files. |
| Project comparison | Rust export `target/real-acceptance-20261011-avtr云栖仙府-stream-scan-r1` has 10,489 objects in 1,406 asset files and 197,732,909 asset bytes. Frozen source project is `target/real-acceptance-20261011-avtr云栖仙府-source/ExportedProject/ExportedProject`. The broad comparison reports 386/386 serialized asset rows; 3,312/2,821 source/Rust files; 2,814 same relative paths; 498 source-only and 7 Rust-only paths; 9,956/10,204 YAML documents; 2,846 matched documents; 1,370 mapped references; and 1,112 raw reference matches. Reference and null-pointer difference samples are capped at 100. |
| Path classification | The 498 source-only paths consist of 98 `.state`, 72 `.blendtree`, 76 `.asset`, 2 `.mask`, 1 `.prefab`, and 249 `.meta` files. Rust-only paths include three AvatarMask output files, a generated GameObject prefab and its `.meta`, and `AuxiliaryFiles/path_id_map.json`. This is an output-layout classification, not proof that the source-only object data is absent from Rust. |
| BlendTree slice | `tools/compare-controller-blendtrees.ps1` reports 717/717 documents, 339/339 unique recursive graphs, 1,807/1,807 ordered ChildMotion records, and zero semantic differences. Report: `target/real-acceptance-20261011-avtr云栖仙府-stream-scan-r1-blendtrees.json`. |
| Classification and next work | The BlendTree layout difference and the eight unnamed AnimatorState machine graphs covered by `DIFF-REAL-AVTR云栖仙府-ANIMATOR-STATE-001` are reconciled at the checked serialized-structure level. Other AnimatorState, MonoBehaviour, AvatarMask, and prefab differences still need cross-path matching against Rust controller subassets and generated prefab output. Unity Editor runtime checks are unavailable, so 云栖仙府 acceptance remains open. |

## DIFF-REAL-AVTR云栖仙府-ANIMATOR-STATE-001 -- unnamed machine states and transitions recovered from packed constants

| Field | Finding |
|---|---|
| Input and controller identity | 青岚雅苑`, unchanged SHA-256 `eb0113f5b26d2649dbacd9766dadd66db804e713b1970f7a0707c370ab6785c6`. The paired controllers are `VRCFury FX` (PathID `5877301275973446833`) and `VRCFury FX_1` (PathID `-2113325549591894014`). |
| Raw state-machine evidence | Frozen-source `controller-bundle-constants` probes decoded eight affected layer bindings in each controller: four one-state ClosetPart layers, `MapleCloset_Encoder` (19 states, 32 transitions), `MapleCloset_Decoder` (16, 1), `MapleCloset_Toggle_Encoder` (5, 6), and `MapleCloset_Toggle_Decoder` (5, 0). Each controller has 49 packed states and 39 transitions across these machines. The raw constants JSON resolves state names and transition `UserID` names through TOS; the probe now emits both numeric IDs and resolved names. |
| Source-to-Rust graph comparison | Independent audit reports `target/real-acceptance-20261011-avtr云栖仙府-unnamed-state-transition-audit-r1.json` and `target/real-acceptance-20261011-avtr云栖仙府-unnamed-state-transition-FX1-audit-r1.json` each cover 8 machines, 49 ordered states, and 39 transitions with zero mismatches. Checked fields include child-state order/count and default mapping; transition destination state or selector, destination machine, exit status, TOS-resolved transition name, timing and exit-time values, ordered conditions, and exact IEEE-754 f32 bits. |
| Frozen-source output | The source project contains 98 corresponding standalone `.state` files. Their exact name multiset matches the 98 packed state names in the two controllers (26 unique names), but all 98 have empty `m_Transitions`. Their eight blank-name machine documents have no emitted child states. Frozen source `AssetRipper/Source/AssetRipper.Processing/AnimatorControllers/AnimatorStateMachineContext.cs` skips machine contexts with empty recovered `Name` in `AssignStateMachineChildStates` (lines 563-575), explaining this projection. The first machine's default pointer remains external to `Assets/AnimatorState/ClosetPartRoot_3.state#110200000`. |
| Representative state content | For the `ClosetPartRoot` root state, Rust document `Assets/AnimatorController/VRCFury FX.controller#7000000126` matches frozen-source `ClosetPartRoot_3.state#110200000` after replacing PPtr literals with one common placeholder; both normalized bodies hash to `D2237E47B1CE70E102AB1C10FFA04444D2DB97C14513CE41FD5442200CBC3E61`. Its motion pointer resolves into the BlendTree graphs covered by the 717-document comparison above. The broader audit compares graph structure and transition fields; it is not a bytewise comparison of every state body. |
| Classification | This is a reconciled serialized-layout difference for these eight unnamed machines in the two controllers. Rust reconstructs children and packed transitions that the frozen source project projection omits because of its empty-name guard; the input-level source constants independently support the reconstructed graph. This does not establish Unity Editor import or runtime behavior. |
| Remaining scope | Other 云栖仙府 AnimatorState graphs, MonoBehaviour, AvatarMask, and prefab path differences remain open. Unity Editor runtime validation is unavailable; 云栖仙府 is not accepted. |

## DIFF-REAL-AVTR青岚古境-SMR-EDITOR-FIELDNAME-001 — Unity 2022.3 SkinnedMeshRenderer editor base fields

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, SHA-256 `cb65e179f0bee693f53fcebdb7836fa12e8ee854082c648388e26b87b152f2bb`; inner SerializedFile version `2022.3.22f1`; frozen source project `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`. |
| Before-fix output | The source root prefab and Rust output each contain 97 class-137 documents with common local fileIDs. Rust had the editor base fields but wrote `m_HideFlags`; the editor TypeTree expects `m_ObjectHideFlags`. |
| Root cause and fix | The Unity 2022.3 class-137 editor TypeTree was selected and renderer conversion ran, but the later field-name-preservation predicate omitted class 137. The source-release alias therefore renamed the editor field. `should_preserve_type_tree_field_names` now preserves names for class 137 only when its editor TypeTree is active. |
| Verification | `cargo test --lib --locked skinned_mesh_renderer -- --nocapture` passed 2/2; the release build exited 0. Fresh project export `target/real-acceptance-20261011-avtr青岚古境-fieldnames-r1` has 97/97 class-137 documents paired by fileID, all four editor base fields in each, and zero `m_HideFlags` occurrences. Check: `target/real-acceptance-20261011-avtr青岚古境-fieldnames-r1-renderer-check.json`. |
| Limits | The targeted check does not compare every SkinnedMeshRenderer value or Unity import/runtime behavior. The broad project comparator reports 7,957/8,035 YAML documents and does not compare non-reference field values; overall source parity and avatar acceptance remain open. Unity Editor and UABEA were not run for this entry. |

## DIFF-REAL-AVTR青岚古境-EDITOR-ARRAY-FIELDS-001 - Unity 2022.3 component editor TypeTrees inside arrays

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, Unity 2022.3.22f1, SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`. Frozen source project: `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`; Rust replay: `target/real-acceptance-20261011-avtr青岚古境-editor-components-r2`. |
| Observed before fix | The source prefab included `tangentMode` in serialized AnimationCurve keyframes. Rust selected the correct 2022.3 editor TypeTrees for AudioSource (class 82) and ParticleSystem (class 198), but the TypeTree merge replaced each default array with source array values. Editor-only Keyframe fields were therefore dropped. |
| Root cause and fix | `merge_editor_default_value` now follows editor TypeTree child nodes for structs and arrays. For each source array element it builds an editor-schema default, then overlays source values recursively. Editor-only fields retain their schema defaults; source values and array ordering remain intact. The 2022.3 editor route now covers generated classes 20, 82, 135, 137, 198, 199, 895512359, and 1773428102; the renderer editor-format defaults also apply to class 199. |
| Verification | `cargo test --lib --locked export::workspace_yaml::object_conversion::tests -- --nocapture` passed 6/6, including a regression for an editor-only Keyframe field inside a source array; `cargo build --release --locked --bin assetripper-rs` exited 0. Fresh production exports r2 and r3 exited 0 with 8,547 objects and 1,536 asset files. The root prefab has 4,491/4,491 documents matched by class ID and local fileID. AudioSource 27/27 and ParticleSystem 90/90 documents match exactly; all 135 AudioSource and 2,184 ParticleSystem `tangentMode` values match. The r3 audit also verifies exact editor-format YAML for 2 Animators and 6 Constraints. Reports: `target/real-acceptance-20261011-avtr青岚古境-editor-components-r2-prefab-document-audit-replay.json` and `target/real-acceptance-20261011-avtr青岚古境-editor-components-r3-prefab-document-audit.json`. |
| Remaining differences | One root-prefab class-114 `expressionsMenu` pointer names different exported assets (`MonoBehaviour_56` in source vs `MonoBehaviour_46` in Rust). Its output-path difference is reconciled by the recursive semantic graph evidence in `DIFF-REAL-AVTR青岚古境-EXPRESSION-MENU-GRAPH-001`. This entry does not verify Unity import/runtime behavior or close overall sample/product acceptance. UABEA was not run. |

## DIFF-REAL-AVTR青岚古境-EXPRESSION-MENU-GRAPH-001 - MonoBehaviour path difference with equal recursive menu graph

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`; frozen source project and Rust r3 export are those recorded in `DIFF-REAL-AVTR青岚古境-EDITOR-ARRAY-FIELDS-001`. |
| Pointer evidence | A Rust ObjectWorkspace probe resolved AvatarDescriptor owner PathID `8557772095086357067` to `expressionsMenu` fileID 0 / PathID `3742894584187122398` in `CAB-2dab0a6fbd16267852718a98beb84761`. In the exported root prefab the source pointer names `Assets/MonoBehaviour/MonoBehaviour_56.asset`; Rust names `Assets/MonoBehaviour/MonoBehaviour_46.asset`. The source and Rust asset names are not stable object identities. |
| Recursive comparison | `MonoBehaviour_56.asset` and `MonoBehaviour_46.asset` have identical YAML after replacing project GUID values. Their dependency walk pairs 65 MonoBehaviour YAML assets, all 65 exact after the same normalization. The graph contains 219 PNG assets whose decoded 32-bit ARGB pixel hashes match exactly, plus one script asset whose bytes match exactly. Summary: `target/real-acceptance-20261011-avtr青岚古境-expression-menu-semantic-summary.json`; details: `target/real-acceptance-20261011-avtr青岚古境-expression-menu-pixel-audit.json`. |
| Classification | The prefab comparator reports one GUID-to-path mismatch because the source and Rust projects assigned different uniquified asset filenames. The root pointer's recursive output data and all texture pixels match; this is classified as an output naming/GUID difference with equivalent menu graph content, not missing menu data. The full recursive byte-oriented diagnostic report is `target/real-acceptance-20261011-avtr青岚古境-expression-menu-graph-r1.json`; its PNG byte differences are covered by the pixel-level report above. |
| Limits | The selected prefab has no remaining identified component-data loss after the r3 editor-TypeTree fixes and menu graph mapping. Unity Editor import/runtime behavior, other project assets, remaining user bundles, and full product acceptance remain open. UABEA was not run. |

## DIFF-REAL-AVTR青岚古境-AVATARMASK-EDITOR-TREE-001 - Unity 2022.3 AvatarMask editor base fields

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, unchanged SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`, inner Unity version `2022.3.22f1`. The selected root prefab references the class-319 `vrc_HandsOnly(Clone)` AvatarMask. |
| Before-fix output | Frozen source `Assets/AvatarMask/vrc_HandsOnly.mask` contains `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset`. Rust r3 `Assets/AvatarMask_319/vrc_HandsOnly.mask` omitted all four fields. `m_Name`, `m_Mask`, and `m_Elements` were equal. |
| Root cause and fix | Class 319 was not selected for the Unity 2022.3 editor TypeTree merge in `should_use_scene_editor_type_tree`; project export therefore used the release object schema. Class 319 is now included in the 2022.3 route, which emits its editor base fields while retaining the source mask payload. |
| Verification | The focused 2022.3 editor-TypeTree group passed 7/7 and the release build exited 0. Fresh Rust export `target/real-acceptance-20261011-avtr青岚古境-avatar-mask-editor-r1` exited 0 with 8,547 objects and 1,536 asset files. Its mask has all four source editor base fields and matches `m_Name`, `m_Mask`, and `m_Elements`; the root prefab's fileID-31900000 GUID resolves to that mask's `.meta`. Root-prefab audit pairs all 4,491 documents; report: `target/real-acceptance-20261011-avtr青岚古境-avatar-mask-editor-r1-prefab-audit.json`. |
| Limits | Source uses `Assets/AvatarMask/` and Rust uses `Assets/AvatarMask_319/`; the generated `.meta` reference resolves correctly. This does not establish Unity Editor import/runtime behavior or complete avatar acceptance. The existing expression-menu output-name difference remains recursively reconciled separately; UABEA was not run. |

## DIFF-REAL-AVTR青岚古境-MESH-EDITOR-TREE-001 - Unity 2022.3 Mesh editor schema and TypelessData YAML

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, unchanged SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`, inner Unity version `2022.3.22f1`; frozen source project `target/real-acceptance-20261011-avtr青岚古境-source/ExportedProject`. |
| Source behavior | Frozen `EditorFormatProcessor` sets `MeshOptimizationFlags.Everything` (`-1`). The class-43 editor TPK layout supplies the four editor base fields and `m_MeshOptimizationFlags`; `VertexData.m_DataSize` is a `TypelessData` node. Unity project YAML expands it as a byte count plus `_typelessdata`. |
| Rust fix | Unity 2022.3 class 43 now selects the editor TypeTree, applies optimization flags `-1`, and converts the decoded `VertexData.m_DataSize` byte value into Unity's `m_DataSize` length and `_typelessdata` payload fields. The original bytes are moved without rewriting. Malformed or duplicate schema fields are explicit errors. |
| Focused check | `cargo test --release --lib --locked unity_2022_3_mesh_editor_tree_has_optimization_field -- --nocapture` passed 1/1. The regression checks the editor TypeTree node shape, optimization value, emitted byte count, and retained payload bytes. The release CLI build and fresh real-sample export both exited 0. |
| Real project evidence | Rust replay `target/real-acceptance-20261011-avtr青岚古境-mesh-editor-r2` contains 112 Mesh assets at the same paths as the frozen source. `Halo_Kokona.asset` is byte-identical at 3,542 bytes (SHA-256 `84179675F9DF37C156D225C055B4DAC671CEECE309B4644EA7D92F10D1CCB806`). Across the full Mesh directory, 109 assets are byte-identical; the other three differ only in 27 decimal spellings of BlendShape vertex floats, with zero parsed Float32 bit mismatches. |
| Reference mapping | The fresh root-prefab audit paired 4,491/4,491 documents by class/local fileID. All 100 MeshRenderer and 100 MeshFilter documents are exact after project GUID-to-path normalization, preserving references through each project's Mesh `.meta`. Audit: `target/real-acceptance-20261011-avtr青岚古境-mesh-editor-r2-prefab-audit.json`. |
| Classification and limits | The class-43 editor field and TypelessData serialization mismatch is fixed for Unity 2022.3. The three lexical float differences are equivalent at the serialized Float32 bit level. The previously recorded expression-menu path difference remains reconciled by its recursive semantic graph audit. Unity Editor import/runtime validation is unavailable, UABEA was not run, and this does not accept the avatar or complete product acceptance. |

## DIFF-REAL-AVTR青岚古境-AVATAR-EDITOR-TREE-001 - Unity 2022.3 Avatar editor base fields

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, unchanged SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`, inner Unity version `2022.3.22f1`; source asset `Assets/Avatar/Avatar0Avatar.asset`. |
| Before-fix difference | The source Avatar YAML had `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset`; Rust omitted all four. The source carried three null base PPtrs while Rust carried none. |
| Root cause and fix | Class 90 was not selected for the Unity 2022.3 editor TypeTree route. Both TPK layouts expose the same root payload members (`m_Name`, `m_AvatarSize`, `m_Avatar`, `m_TOS`, and `m_HumanDescription`); the editor TypeTree also adds the four base fields. Class 90 now uses that editor schema. |
| Field and payload verification | Fresh Rust output has all four fields and three null PPtrs. The source and Rust Avatar documents have the same 10,568 ordered numeric scalar tokens with zero semantic numeric/Float32 bit differences, identical hashes and lengths for all 8 long hexadecimal payload fields, and exactly matching `m_TOS` (21,633 characters) and `m_HumanDescription` (76,881 characters) sections. Both retain 554 skeleton-pose transforms and 304 human-skeleton nodes. |
| Project references | The refreshed whole-project report has no Avatar null-pointer-count or reference-difference row. The root-prefab audit pairs all 4,491 documents; Animator (class 95) is exact for 2/2 documents after GUID-to-path normalization. Reports: `target/real-acceptance-20261011-avtr青岚古境-avatar-editor-r1-compare.json` and `target/real-acceptance-20261011-avtr青岚古境-avatar-editor-r1-prefab-audit.json`. |
| Limits | This closes the checked Avatar editor-base-field and serialized-data mismatch for this Unity 2022.3 sample. Numeric/hex/text comparisons verify serialized output, not Unity import or runtime behavior. Unity Editor import/runtime validation is unavailable, UABEA was not run, and the avatar and product remain unaccepted. |

## DIFF-REAL-AVTR青岚古境-PREDEFINED-TEXTURE-CACHE-001 - Default particle texture reference

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑`, unchanged SHA-256 `CB65E179F0BEE693F53FCEBDB7836FA12E8EE854082C648388E26B87B152F2BB`; comparison `target/real-acceptance-20261011-avtr青岚古境-avatar-editor-r1-compare.json`. |
| Observed difference | In `Assets/Material/Minigun Particle Colision.mat`, the frozen source project points `_MainTex` to builtin GUID `0000000000000000f000000000000000`, fileID `10307`. Rust points to `Assets/Texture2D/Default-ParticleSystem.png`, fileID `2800000`, GUID `a38ebd49c44ce850b2b32c21420f8396`. The other two material references are preserved after path/GUID normalization. |
| Source cache evidence | Both source and Rust `AuxiliaryFiles/path_id_map.json` identify input PathID `4988150354445090395` as `Default-ParticleSystem`, type `Texture2D`. Frozen `PredefinedAssetCache.Contains(ITexture2D)` matches `(name, width, height)`; `EngineAssetsFactory.Create(ITexture2D)` stores the name and dimensions in its predefined metadata. The source material's fileID `10307` is consistent with that cache route. |
| Rust and builtin payload evidence | The input Texture2D exports as a 64×64 PNG of 3,256 bytes with SHA-256 `368CA2F9059FC43CB327EB7215C512F23E685890B044EACD37629FC5DB587262`. Unity 2022.3.22f1 `unity_builtin_extra` (SHA-256 `D05BA3DF03C56287D720CEFC0DDBD68D4942F2A20D41736E01E786571180644B`) contains builtin Texture2D fileID 10307; exporting it also produces a 3,256-byte PNG with the same SHA-256. The material's builtin reference and Rust's project Texture2D therefore have identical decoded PNG content for this sample. |
| Classification | Serialized-content difference resolved as semantically equivalent; no routing change was made. This comparison does not verify Unity import/runtime behavior. Unity Editor import remains blocked by the recorded missing Editor license, and UABEA was not run. |

## DIFF-REAL-青岚山河卷-LIGHTPROBES-EDITOR-TREE-001 - Unity 2022.3 LightProbes editor base fields

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷`, 73,242,523 bytes, SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`; source Unity version `2022.3.6f1`; frozen source project `target/real-acceptance-current-青岚山河卷-source/ExportedProject/ExportedProject`. |
| Before-fix difference | The source `Assets/Scenes/Lost Kitten/LightProbes.asset` class-258 document contains `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset`. Rust r1 `Assets/LightProbes_258/LightProbes_258.asset` omitted those fields. The remaining serialized LightProbes payload was present. |
| Root cause and fix | Class 258 was generated from the release layout because `should_use_scene_editor_type_tree` omitted LightProbes from the Unity 2022.3 class set. The route now selects its reference editor TypeTree for 2022.3, preserving the editor schema and merging the source payload. The editor tree's four base fields are covered by the existing class-layout regression. Commit `f4423d2` is pushed to `main`. |
| Verification | `cargo test unity_2022_3_ -- --nocapture` passed 8/8; `cargo build --release --bin assetripper-rs` exited 0; targeted `rustfmt --check --edition 2021 src/export/workspace_yaml/object_conversion.rs` and `git diff --check` exited 0. Fresh release export `target/real-acceptance-current-青岚山河卷-r2` exited 0 with 2 SerializedFiles, 72,350 objects, 1,087 asset files, and 242,386,474 asset bytes. Its class-258 `LightProbes_258.asset` is byte-identical to the frozen source document at 439 bytes (SHA-256 `057D8B1F1C4384DE25702C326B2431B75422B359E23A865D0C43FC96FE4971FA`), including all four base fields and the remaining YAML payload. |
| Limits | Source and Rust export the document under different asset paths; the corresponding class and local fileID are 258 and 25800000. This targeted document check does not repeat the full project comparison or establish Unity import/runtime behavior. The two baseline non-exportable empty collections (classes 150 and 142) remain unchanged. UABEA was not run; the world and overall product remain unaccepted. |

## DIFF-REAL-青岚山河卷-COLLIDER-EDITOR-TREES-001 - Unity 2022.3 BoxCollider and MeshCollider editor TypeTrees

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷`, SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`, Unity 2022.3.6f1; frozen source project `target/real-acceptance-current-青岚山河卷-source/ExportedProject/ExportedProject`. |
| Before-fix difference | The r2 scene audit paired all 71,237 documents. All 3,997 class-65 BoxCollider documents and all 629 class-64 MeshCollider documents lacked the source editor base fields `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset`. |
| Root cause and fix | The Unity 2022.3 editor-TypeTree route omitted class IDs 65 and 64. Both classes now select their versioned reference editor TypeTrees; the class-selection and editor-base-field tests include both IDs. |
| Verification | `cargo test --release --lib --locked export::workspace_yaml::object_conversion::tests -- --nocapture` passed 7/7; `cargo build --release --locked --bin assetripper-rs`, targeted `rustfmt --check --edition 2021 src/export/workspace_yaml/object_conversion.rs`, and `git diff --check` exited 0. Fresh full export `target/real-acceptance-current-青岚山河卷-r4` exited 0: 2 SerializedFiles, 72,350 objects, 1,087 asset files, 242,973,976 asset bytes, 6 support files, and 2 post-exporter files. BoxCollider documents match source exactly, 3,997/3,997. MeshCollider documents pair by class/local fileID, 629/629; 628 raw YAML differences are generated GUIDs. Each side's GUIDs resolve through `.meta` files to the same 11 relative `Assets/Mesh/*.asset` paths, and all 11 corresponding mesh assets are byte-identical by SHA-256. After GUID-to-relative-path normalization, all 629 MeshCollider documents match exactly. |
| Limits | Unity Editor import/runtime validation was not run for this replay; the installed Editor is blocked by the previously recorded missing license. UABEA was not run. The scene audit still has other class differences, including 86 CapsuleCollider documents (class 136); the two baseline non-exportable empty collections (classes 150 and 142) remain. The world and overall product are not accepted. |

## DIFF-REAL-青岚山河卷-CAPSULECOLLIDER-EDITOR-TREE-001 - Unity 2022.3 CapsuleCollider editor base fields

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷`, SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`, Unity 2022.3.6f1; frozen source scene `target/real-acceptance-current-青岚山河卷-source/ExportedProject/ExportedProject/Assets/Scenes/Lost Kitten.unity`. |
| Before-fix difference | The source has 86 class-136 `CapsuleCollider` documents. Rust r4 paired all 86 by local fileID but omitted `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset` in every document. |
| Root cause and fix | Class 136 was omitted from the Unity 2022.3 editor-TypeTree route. It now uses the reference editor TypeTree, and both class-selection and editor-base-field tests cover it. |
| Verification | Selector test `cargo test --release --lib --locked unity_2022_3_scene_classes_use_editor_type_trees -- --nocapture` passed 1/1; editor-tree field test `cargo test --release --lib --locked unity_2022_3_editor_tree_classes_have_editor_base_fields -- --nocapture` passed 1/1; `cargo test --release --lib --locked export::workspace_yaml::object_conversion::tests -- --nocapture` passed 7/7. `cargo build --release --locked --bin assetripper-rs`, targeted `rustfmt --check --edition 2021 src/export/workspace_yaml/object_conversion.rs`, and `git diff --check` exited 0. Fresh full export `target/real-acceptance-current-青岚山河卷-r5` exited 0: 2 SerializedFiles, 72,350 objects, 1,087 asset files, and 242,984,898 asset bytes. Class 136 matches exactly for all 86/86 documents, including the four editor fields. Classes 65 and 64 remain exact for 3,997/3,997 and 629/629 documents respectively after path-based GUID normalization for class 64. |
| Limits | Unity Editor import/runtime validation was not run; the installed Editor is blocked by the previously recorded missing license. UABEA was not run. Class 205 LODGroup and other scene differences remain open; the world and overall product are not accepted. |

## DIFF-REAL-青岚山河卷-LODGroup-EDITOR-TREE-001 - Unity 2022.3 LODGroup editor base fields

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷`, SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`, Unity 2022.3.6f1; frozen source scene `target/real-acceptance-current-青岚山河卷-source/ExportedProject/ExportedProject/Assets/Scenes/Lost Kitten.unity`. |
| Before-fix difference | The source has 2,739 class-205 `LODGroup` documents. Rust r5 paired all 2,739 by local fileID but omitted `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset` in every document; the nested `m_LODs` payload comparison was still pending. |
| Root cause and fix | Class 205 was omitted from the Unity 2022.3 editor-TypeTree route. It now uses the reference editor TypeTree, and both class-selection and editor-base-field tests cover it. No renderer-style value conversion applies: the frozen `EditorFormatProcessor`/`EditorFormatConverter` handle GameObject, Renderer, Transform, Mesh, and other listed classes, but LODGroup documents carry only the four base fields plus their release payload. |
| Verification | `cargo test --release --lib --locked unity_2022_3_ -- --nocapture` passed 8/8 at commit time; `cargo build --release --locked --bin assetripper-rs` exited 0. Fresh full export `target/real-acceptance-current-青岚山河卷-r6` exited 0: 2 SerializedFiles, 72,350 objects, 1,087 asset files, 243,332,751 asset bytes, 6 support files, and 2 post-exporter files. All 2,739/2,739 class-205 documents pair by local fileID and are byte-exact after isolating each document body, including the four editor fields and the complete nested `m_LODs` payload (screen heights, fade widths, ordered renderer fileIDs) plus `m_Enabled`. Rechecked slices in r6: BoxCollider 3,997/3,997 exact, CapsuleCollider 86/86 exact, MeshCollider 629/629 paired, LightProbes asset byte-identical (439 bytes, SHA-256 `057D8B1F1C4384DE25702C326B2431B75422B359E23A865D0C43FC96FE4971FA`). |
| Limits | Unity Editor import/runtime validation was not run; the installed Editor is blocked by the previously recorded missing license. UABEA was not run. Class-222 CanvasRenderer documents (12/12 missing the same four base fields) and other scene differences remain open; the world and overall product are not accepted. |

## DIFF-REAL-青岚山河卷-CANVASRENDERER-EDITOR-TREE-001 - Unity 2022.3 CanvasRenderer editor base fields

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷`, SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`, Unity 2022.3.6f1; frozen source scene `target/real-acceptance-current-青岚山河卷-source/ExportedProject/ExportedProject/Assets/Scenes/Lost Kitten.unity`. |
| Before-fix difference | The source has 12 class-222 `CanvasRenderer` documents. Rust r6 paired all 12 by local fileID but omitted `m_ObjectHideFlags`, `m_CorrespondingSourceObject`, `m_PrefabInstance`, and `m_PrefabAsset` in every document; the `m_GameObject`/`m_CullTransparentMesh` payload already matched. |
| Root cause and fix | Class 222 was omitted from the Unity 2022.3 editor-TypeTree route. It now uses the reference editor TypeTree, and both class-selection and editor-base-field tests cover it. No renderer-style value conversion applies: the source CanvasRenderer documents carry only the four base fields plus `m_GameObject` and `m_CullTransparentMesh`. |
| Verification | `cargo test --release --lib --locked export::workspace_yaml::object_conversion::tests -- --nocapture` passed 7/7; `cargo test --release --lib --locked export::` passed 500/500 (1 ignored); `cargo build --release --locked --bin assetripper-rs` exited 0. Fresh full export `target/real-acceptance-current-青岚山河卷-r7` exited 0: 2 SerializedFiles, 72,350 objects, 1,087 asset files, 243,334,402 asset bytes, 6 support files, and 2 post-exporter files. All 12/12 class-222 documents pair by local fileID and are byte-exact after isolating each document body. Rechecked slices in r7: class 205 2,739/2,739, class 65 3,997/3,997, and class 136 86/86 exact. |
| Limits | Unity Editor import/runtime validation was not run; the installed Editor is blocked by the previously recorded missing license. UABEA was not run. Remaining scene differences and the world and overall product acceptance stay open. |

## DIFF-REAL-青岚山河卷-SCENE-EDITOR-ROUTE-002 - Unity 2022.3 AudioListener/Tree/NavMeshSettings/Canvas/RectTransform editor fields

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷`, SHA-256 `100338C82E934B51357CE143106DE8EC43C4389DD50727E0674BFD3CC137267A`, Unity 2022.3.6f1; frozen source scene `target/real-acceptance-current-青岚山河卷-source/ExportedProject/ExportedProject/Assets/Scenes/Lost Kitten.unity`. |
| Before-fix difference | Rust r7 omitted the four editor base fields in class 81 AudioListener (1 doc), class 193 Tree (1,456 docs, also missing null `m_TreeData`), class 223 Canvas (2 docs), class 224 RectTransform (19 docs, also missing `m_ConstrainProportionsScale`/`m_LocalEulerAnglesHint`), class 196 NavMeshSettings (1 doc, missing `m_ObjectHideFlags` and the whole `m_BuildSettings` subtree), and the base-field part of class 320 PlayableDirector (1 doc). |
| Root cause and fix | All six classes were omitted from the Unity 2022.3 editor-TypeTree route. Classes 81/193/223/224/320 use the reference editor TypeTree with the standard merge; class 224 additionally enters the Transform editor-format arm so `m_LocalEulerAnglesHint`/`m_RootOrder` are computed from rotation/hierarchy exactly as frozen `EditorFormatConverterAsync.Convert(ITransform)` does (field-presence guarded, so the absent `m_RootOrder` on RectTransform is skipped). Class 196 is a manager class, not an editor-base class: its frozen TPK editor tree holds only `m_ObjectHideFlags` plus payload, so it joins the 104/157 `ensure_scene_manager_hide_flags` set, and `apply_nav_mesh_settings_editor_defaults` ports `NavMeshSettingsExtensions.ConvertToEditorFormat` + `NavMeshBuildSettingsExtensions.SetToDefault` for null `m_NavMeshData`. A first attempt failed loudly on the TPK shape fact that `maxJobWorkers` is UInt32; the default table carries the TPK shape (`U32` variant) instead of coercing. Non-null `m_NavMeshData` is an explicit conversion error, not silent defaults. |
| Verification | `cargo test --release --lib --locked export::workspace_yaml::object_conversion::tests -- --nocapture` passed 8/8, including the new manager-family regression (104/157/196 have `m_ObjectHideFlags`, lack the three PPtr base fields); `cargo test --release --lib --locked export::` passed 501/501 (1 ignored); `cargo build --release --locked --bin assetripper-rs` exited 0. Fresh full export `target/real-acceptance-current-青岚山河卷-r8` exited 0: 2 SerializedFiles, 72,350 objects, 1,087 asset files, 243,562,238 asset bytes, 6 support files, 2 post-exporter files. Newly byte-exact: 81 (1/1), 193 (1,456/1,456), 196 (1/1), 223 (2/2), 224 (19/19). All prior exact and GUID-semantic slices recheck unchanged. |
| Limits | Class 320 bindings, class-4 float prints, Default-Material direction, and the lighting path remain open (see validation log). Unity Editor import/runtime and UABEA were not run. |

## DIFF-YAML-FLOAT-F64-001 - f64 E-notation quirk stays latent

| Field | Finding |
|---|---|
| Input and reference | 3,675-pattern oracle sweep (net10.0, matching frozen TFM); 青岚山河卷 r23 full-output scan. |
| Before-fix difference | 4 E-notation cases where .NET prints a shorter non-round-tripping string (Grisu-interval quirk, e.g. `2^-25`); true ties already match. |
| Rust change | None (test-only): oracle-pinned agreement test; oracle committed as permanent probe. |
| Verification | 3,671/3,675 agree; zero F64 values in real-sample YAML; `export::` 514/514. |
| Limits | Revisit if F64 ever reaches real output. Unity Editor import/runtime and UABEA were not run. |

## DIFF-云栖仙府-BREADTH-001 - First 云栖仙府 audit with four editor routes

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑` (Unity 2022.3.22f1); 20261011 source export; TPK editor-base probes for 54/84/96/153. |
| Before-fix difference | Four classes exported release-shaped (missing base fields, zeroed renderer defaults). |
| Rust change | 54/84/96/153 join the 2022.3 editor selector plus both regression lists. |
| Verification | r3 audit: 54/84/153 exact, 96 semGUID; 231/231 PNGs pixel-identical; zero rust-only files; `export::` 513/513. |
| Limits | Controller placement families, 246 source-only files, 243 opaque files stay open. Unity Editor import/runtime and UABEA were not run. |

## DIFF-玉京烟霞-BREADTH-001 - First 玉京烟霞/烟雨长亭 replays with renderer and Flare routes

| Field | Finding |
|---|---|
| Input and reference | `松月云庭图` (2022.3.22f1, 34,150 objects), 青岚雅苑` (5,829 objects); TPK editor-base probes for 120/212/123/124. No frozen source exports exist (no CLI path — blocked lane). |
| Before-fix difference | 120/212/123/124 exported release-shaped (missing base; zeroed renderer defaults on 120/212). |
| Rust change | 120/212 join the editor selector, both regression lists, scene-object dispatch, and renderer arm; 123/124 join the selector and both lists. |
| Verification | r3 docs carry base fields + Unity defaults; `export::` 514/514; 烟雨长亭 census clean (159 = EditorSettings). |
| Limits | Differential vs source blocked on frozen exports. Unity Editor import/runtime and UABEA were not run. |

## DIFF-YAML-RENDERER-RULE-001 - Universal IRenderer editor defaults

| Field | Finding |
|---|---|
| Input and reference | 云栖仙府 TrailRenderer GI fields (1/3/4/0.5/89 vs zeros); frozen `EditorFormatConverter.Convert(IRenderer)` + `EditorFormatProcessor` interface dispatch. |
| Before-fix difference | Rust scoped the overwrite to 23/137/199; every other renderer kept zeros. |
| Rust change | 96 joins the renderer arm and scene-object dispatch; direct test pins all six defaults. |
| Verification | 96 at semGUID in r3; 120/212/123/124 structurally verified in 玉京烟霞 r3; unit test green; full suite green. |
| Limits | Frozen 玉京烟霞/烟雨长亭 source exports blocked (no CLI path in frozen app). Unity Editor import/runtime and UABEA were not run. |

## DIFF-青岚古境-MATERIAL-BUILTIN-001 - Bundle texture preserved vs builtin remap

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑` `Minigun Particle Colision.mat`; bundle Texture2D `Default-ParticleSystem` (PathID 4988150354445090395, 64×64 RGBA32, 21,844 bytes). |
| Before-fix difference | Source points the slot at builtin 10307 and drops the object; Rust exports the decoded bytes and references them (r3's only rust-only file). |
| Rust change | None: the material PPtr provably resolves locally (export refuses unresolved; zero dangling texture PPtrs), so Rust is bundle-faithful. |
| Verification | Object dump shows full image data; generated PNG decodes it (64×64 soft dot, no fabrication); sibling slot proven same-target both sides. |
| Limits | Byte-equality against Unity's engine builtin unchecked. KNOWN ACCEPTABLE DIFFERENCE (Rust preserves). Unity Editor import/runtime and UABEA were not run. |

## DIFF-REFS-INTEGRITY-001 - Project-wide reference resolution

| Field | Finding |
|---|---|
| Input and reference | Latest exports of all six samples; `.meta` guid maps; frozen scene-ID convention. |
| Before-fix difference | No product change — new verification lane. |
| Rust change | Committed `tools/check-project-references.ps1`. |
| Verification | 61,223 PPtrs resolve with zero dangling across 青岚山河卷/青岚古境/云栖仙府/玉京烟霞/烟雨长亭/松月竹庭. |
| Limits | Static evidence only; Unity import behavior unobserved. Unity Editor import/runtime and UABEA were not run. |

## DIFF-青岚古境-CONTROLLER-001 - AnimatorController shared-asset claiming and cloning

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑` controllers; frozen `AnimatorControllerProcessor` (reconstruction from `ControllerConstant`, `FetchEditorHierarchy` claiming, processed-collection cloning, `MainAsset` assignment). |
| Before-fix difference | Source splits 41 shared BlendTrees, 13 shared states, and 12 shared behaviours (e.g. `VRCAvatarParameterDriver`) into separate `.blendtree`/`.state`/`.asset` files; Rust embeds raw hierarchies once per object, with export-order ordinals permuted by the missing interleaved clones. |
| Rust change | None this round (triage only). |
| Verification | Exact bundle-count reconciliation (174 trees, 639 states); all 66 r2 MonoBehaviour files content-twin to source; `subMenu` reference-equivalence proof across ordinals; duplicate-export suspicion exonerated (both sides duplicate identically). The 59 青岚古境 114 diffs are guid/path artifacts with zero masked-content residual. Split objects proven orphans (zero embedded twins); split localized to FX_Milfy 1 (381 vs 394 states); raw-space sharing invisible, so the trigger needs processed-hierarchy coverage. Export rule mapped: `ProjectExporter` visits every asset, `AnimatorControllerExporter` claims only `MainAsset`-is-controller, remainder falls to `DefaultYamlExporter`; replaced originals keep null MainAsset (processor assigns only processed assets; `MainAssetProcessor` covers just Font/TerrainData). Oracle `controller-oracle-青岚古境2.tsv`: pre CAB has zero sub-asset objects; 2,074 created fresh; null sets (13 states PathIDs 645-650/658-663+602, 41 trees, 78 CAB behaviours) reconcile exactly with 132 split files; embedded trees = hierarchy sums (35+24+73+1=133). Round-16 counts 66/246 superseded. States+trees split IMPLEMENTED and validated (青岚古境 r5: 41 trees semGUID, 13 states paired; 云栖仙府 r4: 72 trees semGUID, 98 states paired, onlySource 246→76). |
| Limits | Controller split COMPLETE on 青岚古境 + 云栖仙府 (onlySource=0 both; all remaining diffs guid/ID-family/accepted). 青岚古境 opaque fully classified (52 + 439 px + 24 packets). Unity Editor import/runtime and UABEA were not run. |

## DIFF-OPAQUE-SHADER-PROPS-001 - Shader DefValue static-array field names

| Field | Finding |
|---|---|
| Input and reference | 20 青岚山河卷 dummy shaders (ASE/poiyomi/Hidden); TypeTree probes (5.6/2019.4/2022.3) all showing `float m_DefValue[0..3]`; frozen dummy property rendering. |
| Before-fix difference | Rust read fictional `m_DefValue_0_` fields, kept zeros, and rendered `Range(0, 0) = 0`. |
| Rust change | Read the literal bracket names; documented the serialization-vs-generated sanitization rule; new `_Wind` regression test. |
| Verification | `AE_Leaves.shader` SHA-256-identical; all 20 shaders byte-identical in r23; all 8 松月竹庭 lilToon shaders byte-identical in r12; `resources::shader` 16/16; `export::` 514/514. |
| Limits | None known. Unity Editor import/runtime and UABEA were not run. |

## DIFF-OPAQUE-OGG-FRAMING-001 - Ogg page framing with identical packet streams

| Field | Finding |
|---|---|
| Input and reference | 青岚山河卷 `Nijika Lost Kitten (Bocchi the Rock).ogg` (FSB5 Vorbis remux); Ogg page/packet splitter over both outputs. |
| Before-fix difference | 1.6M/1.9M bytes differ from byte 22 (page checksums onward). |
| Analysis | Both files carry 6,729 packets with identical headers (identification, comment, setup) and bit-identical audio-packet streams — Rust remuxes original packets verbatim; only page grouping differs. |
| Verification | Packet-stream SHA-256 equality; `AE_Leaves`-class byte proof does not apply here — classified content-EXACT, framing differs. |
| Limits | Page-granule detail unverified; exact page-split parity deferred as zero-gain. Unity Editor import/runtime and UABEA were not run. |

## DIFF-OPAQUE-SCRIPT-001 - Global dummy-script blank-line whitespace

| Field | Finding |
|---|---|
| Input and reference | 青岚山河卷 `Door.cs`; frozen `EmptyScript.cs` global raw-string template. |
| Before-fix difference | One blank comment line: source `\t\t\t`, Rust empty (2,698 vs 2,695 bytes, the only diff). |
| Rust change | Global template line restored to `\t\t\t`; test expectation corrected to source truth. |
| Verification | `Door.cs` byte-identical in r22; template unit tests green. |
| Limits | None known. Unity Editor import/runtime and UABEA were not run. |

## DIFF-OPAQUE-NORMALMAP-001 - DXT5nm unpack trigger value

| Field | Finding |
|---|---|
| Input and reference | 33 青岚山河卷 normal-map PNGs; frozen `TextureConverter` (`LightmapFormatE == NormalmapDXT5nm` → `UnpackNormal`); bundle `m_LightmapFormat: 3` on `1001_normal` (PathID 152). |
| Before-fix difference | Rust compared against transcribed value 5, skipped the unpack, and wrote packed DXT5nm pixels. |
| Rust change | Constant corrected to 3 with behavioral rationale; test pins the literal through `from_value` on an I32 field. |
| Verification | GDI+ pixel-hash over all 235 PNG pairs: 235/235 identical (was 202/235); `resources::texture` 54/54; `export::` 512/512. |
| Limits | PNG bytes still differ (encoder-only). 20 dummy-shader property gaps and 1 OGG re-encode gap stay open. Unity Editor import/runtime and UABEA were not run. |

## DIFF-REAL-青岚山河卷-SPRITE-001 - SpriteProcessor geometry recomputation

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷` packed sprites (Cookie, Video Player, bed, mirror, music); frozen `SpriteProcessor.ProcessSprite` and `SpriteExtensions.GetSpriteCoordinatesInAtlas`. |
| Before-fix difference | Source rederives Rect/Pivot/Border from the render-data rectangle and recomputes Offset/TextureRectOffset (1-ulp and derived differences); Rust emitted raw bundle values. |
| Rust change | `apply_sprite_geometry` ports the unconditional recompute in source order with `f32` semantics for class 213; non-null `m_SpriteAtlas` is a loud error (atlas-copy branch needs cross-object reads); `Has_*` follows bundle field presence. |
| Verification | Bit-exact regression test on Cookie vectors; 青岚山河卷 r21 class 213 at 10 semGUID with zero diffs; `export::` 512/512; Clippy clean. A casing bug (`TextureRect` vs real `textureRect`) failed the first replay loudly and was fixed with real node names in both code and fixture. |
| Limits | Atlas-packed sprites (non-null atlas) are an explicit error, unvalidated. Unity Editor import/runtime and UABEA were not run. |

## DIFF-松月竹庭-PATHS-001 - Export ClassName overrides and bundle-container original directories

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑` (Unity 2022.3.11f1); frozen `Pass110` overrides, `IUnityObjectBase.GetBestDirectory`, `OriginalPathProcessor`, `CreatePrefabForRoot`, `PrefabExportCollection`; isolated reflection probe. |
| Differences | `AvatarMask_319/` vs `AvatarMask/` (enum spelling vs `OriginalName`); root prefab under `GameObject/` vs `Assets/` root (unwired container mapping). |
| Rust change | `class_original_name` override table (238/258/319/329, reflection ground truth); `bundle_container_original_directories` (DirectExport `FileID == 0` container mapping, source processor order) wired into routing and prefab planning. |
| Verification | Unit tests for both rules; 松月竹庭 r9 replay (all 356 files pair, zero path residuals); 青岚山河卷 r14+ scene parity held; full suite green. |
| Limits | ResourceManager-container mapping, GroupByBundleName mode, shader override paths, lowercasing restoration, and original name/extension plumbing stay open. Unity Editor import/runtime and UABEA were not run. |

## DIFF-YAML-FLOAT-TIE-002 - Exact decimal tie-break for shortest-float printing

| Field | Finding |
|---|---|
| Input and reference | Frozen `.NET Single.ToString` shortest-repr tie-to-even rule; `Face (merged).baked.asset` mesh vertices (`2^-12` printing `...463` instead of `...462`). |
| Before-fix difference | Round-10 fix compared neighbors by `f64` distance, whose rounding noise flips exact ties. |
| Rust change | Exact comparison of `2*value` against the neighbor digit sum as terminating decimals (bounded string arithmetic, no bigints); off-by-one caught by the existing probe fixture during development. |
| Verification | 28-row fixture, both tie tests, new power-of-two vectors; `export::` 511/511; 青岚山河卷 r20 mesh 93/93 exact; 松月竹庭 r11 monotonicity. |
| Limits | `f64` tie behavior remains unprobed. Unity Editor import/runtime and UABEA were not run. |

## DIFF-YAML-POINTER-TYPE-001 - Binary-exporter pointer type for shader references

| Field | Finding |
|---|---|
| Input and reference | `PostProcessResources.asset` shader fields; frozen `BinaryAssetExporter.ToExportType` (`Meta`) vs `YamlExporterBase` (`Serialized`). |
| Before-fix difference | Generic pointer map reported `Serialized` (type 2) for `ShaderText`-routed shaders; source reports `Meta` (type 3). |
| Rust change | `ObjectRoute::ShaderText` joins the `Meta` arm; YAML-mode shaders keep `Serialized`. |
| Verification | 青岚山河卷 r16 class 114 zero diffs; materials unaffected (own path already emitted type 3). |
| Limits | Unity Editor import/runtime and UABEA were not run. |

## DIFF-YAML-PAIR-RENDERING-001 - Keyed rendering for pairs with valid dictionary keys

| Field | Finding |
|---|---|
| Input and reference | ComputeShader variant maps (`- : 0` vs `- first:`/`second:`, 35 occurrences vs zero source-wide), Sprite `m_RenderDataKey`; frozen `YamlWalker.EnterPair`/`EnterDictionaryPair` + `IsValidDictionaryKey` (string/primitive/`GUID`). |
| Before-fix difference | Rust rendered every non-`map` pair as a `first`/`second` struct. |
| Rust change | `keyed_pair_element_paths` hint (any-position `pair` with valid-key first; `map`-owned pairs stay on the dictionary branch) plus struct-branch keyed rendering with loud shape/key errors. |
| Verification | Vector-element and direct-field tests incl. empty keys; 青岚山河卷 r19/r20 compute shaders 12/12 exact; sprite pairs fixed (residual is geometry values only). |
| Limits | Vector/direct pairs with invalid-key first keep struct rendering (unprobed). Unity Editor import/runtime and UABEA were not run. |

## DIFF-YAML-TYPELESS-001 - Schema-driven TypelessData count-plus-payload expansion

| Field | Finding |
|---|---|
| Input and reference | Texture2D `image data` and SpriteRD `m_DataSize` rendering inline hex vs source count+`_typelessdata` (third and fourth instances of the Mesh `m_DataSize` rule); TPK `TypelessData` node type; compute-shader `code:` bytecode confirmed inline in both (non-TypelessData, unaffected). |
| Before-fix difference | Only class-scoped helpers expanded; Sprite render-data blobs rendered inline. |
| Rust change | `typeless_data_expansion_paths` records `TypelessData`-typed nodes; struct writer expands `Bytes` values (other shapes keep generic rendering, so mesh/texture helpers and streamed tests are unaffected). |
| Verification | New conversion test; full suite green; 青岚山河卷 r20 sprite family halved (10 diff → 5 semGUID + 5 diff). |
| Limits | Unity Editor import/runtime and UABEA were not run. |

## DIFF-松月竹庭-BREADTH-001 - Stable-identity breadth audit: export-ID family, avatar style proof, path and opaque triage

| Field | Finding |
|---|---|
| Input and reference | 青岚雅苑` (Unity 2022.3.11f1); committed `tools/audit-project-documents.ps1` (stable `classID|localFileID` pairing, per-project `.meta` GUID maps); report `target/real-acceptance-20261012-avtr松月竹庭-r8-audit.json`. |
| Stable-identity results | Exact: classes 1, 4, 43 (40/40 Mesh), plus material/texture GUID-semantic slices. Open: controller/animation member export IDs (1101/1102/1107/206 unmatched; 91 roots all Different; 114 sub-objects 72 unmatched pairs), one Avatar style diff, two collection path divergences, 69 opaque content diffs. Prior weak-pairing lists are superseded as pairing artifacts where they relied on display names. |
| Export-ID family | Frozen `AssetsExportCollection.GenerateExportID` assigns `GetPseudoRandomExportId(asset, m_exportIDs.Count)`; Rust uses sequential `get_main_export_id` modifiers plus `GENERATED_FILE_ID_BASE` allocator anchors. Same root cause as the timeline fix, but the controller re-synthesis needs seed-order alignment with `FetchEditorHierarchy` and anchor/pointer replumbing — deferred to a dedicated round. Internally consistent on both sides; no corruption. |
| Export-ID family (reclassified 2026-10-12) | KNOWN ACCEPTABLE DIFFERENCE: all 455 member references in the frozen controller output are file-local (`{fileID: ...}` with no guid), and Rust's re-synthesis is internally consistent (anchors and pointers agree) and deterministic. File-local IDs carry no Unity import/runtime semantics, so cloning the pseudo-random scheme would be zero-functional-gain surgery on a 3,000-line subsystem. Per product priorities (data correctness first), this stays accepted until all functional gaps close; revisit only on contrary Unity evidence. |
| Avatar style proof | Ordered scalar-token sequences from both `云栖仙府Avatar.asset` files are identical (21,688 tokens, zero non-semantic diffs); only flow-vs-block nesting style differs. The exact source rule for `float3` sequence-nested values is unpinned; no writer change made. |
| Path and opaque triage | `AvatarMask` vs `AvatarMask_319` directories; root-level vs `GameObject/` prefab placement — both open in the collection-directory family. Opaque: 52 PNG, 9 OGG, 8 shader diffs untriaged; 17 identical. |
| Limits | Unity Editor import/runtime and UABEA were not run. Product acceptance remains open. |

## DIFF-REAL-青岚山河卷-TIMELINE-IDS-001 - ScriptableObject group child export IDs use pseudo-random class-prefixed hashing

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷` (Unity 2022.3.6f1), `Assets/MonoBehaviour/Lost Kitten.playable` child documents; frozen `AssetsExportCollection.GenerateExportID` (`AssetRipper.Export.UnityProjects/Project/AssetsExportCollection.cs` line 53-56) and `ExportIdHandler.GetPseudoRandomExportId` (class-114 prefix plus `XxHash64(seed) mod 10^15`, seed = `m_exportIDs.Count`). |
| Before-fix difference | Source timeline children carry `114709254077376921`, `114730242686860875`, ... while Rust planned sequential `11400001`, `11400002`, ... via `get_main_export_id(114, n+1)`. Scene binding keys/values/order/targets otherwise matched, so only the key fileIDs (and the playable asset's own document IDs) diverged. |
| Rust change | `prepare_scriptable_object_groups` plans each child with `get_pseudo_random_export_id(114, zero_based_insertion_seed, child_collection_version)`, threading the workspace through the export context with explicit version errors. Covers TimelineAsset and PostProcessProfile groups generically. |
| Verification | Scratch reproduction of the first four frozen IDs from the repo's own hash routine before editing; new 4-vector contract test; `export::ids::` 5/5; `export::` 505/505; warnings-denied Clippy; release build; r13 replay (timeline IDs exact, timeline file GUID-normalized identical, scene 49,862 exact + 21,375 semGUID with zero residuals) and the 松月竹庭 r8 regression replay (identical to r7 on every headline metric), all recorded in the validation-log entry above. |
| Limits | Unity Editor import/runtime and UABEA were not run. The world scene surface is at parity; product acceptance remains open. |

## DIFF-REAL-青岚山河卷-LIGHTING-001 - LightingSettings editor defaults and scene-subfolder paths

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷` (Unity 2022.3.6f1); frozen source `Assets/Scenes/Lost Kitten/Lost KittenSettings.lighting` (1,807 bytes); frozen `LightingDataProcessor.SetPathsAndMainAsset` and `LightmapEditorSettingsExtensions.ConvertToEditorFormat`; isolated `ser-factory` TPK probe for class 850595691. |
| Before-fix difference | Rust r10 emitted `Assets/LightingSettings/Lost KittenSettings.lighting` (344 bytes, truncated after `m_UsingShadowmask`, no base fields) and `Assets/LightProbes_258/LightProbes_258.asset`. Root causes: (1) the recovered release TypeTree (14 nodes, proven by probe) was used as the export schema while the full payload decoded from the embedded tree was silently narrowed to it; (2) no equivalent of `SetPathsAndMainAsset` assigned scene-subfolder directories to unshared LightProbes/LightingSettings targets. |
| Rust change | Class 850595691 uses the reference editor TypeTree; `apply_lighting_settings_editor_defaults` ports `ConvertToEditorFormat` unconditionally per the probed node types; `scene_lighting_subasset_directories` ports the scene-path rule with shared-target unsetting, applied as an `original_directory` fallback isolated from texture lightmap flags. |
| Verification | `export::` 505/505, warnings-denied Clippy, release build, and the r12 replay are recorded in the validation-log entry above: the `.lighting` asset is byte-identical at the source path, LightProbes moved to the scene subfolder, and the scene audit totals 49,862 exact + 21,374 semGUID with only class 320 left. |
| Limits | The `LightProbes_258.asset` file-stem suffix (source: `LightProbes.asset`) remains open. Class 320 bindings remain open. Unity Editor import/runtime and UABEA were not run. |

## DIFF-YAML-FLOAT-TIE-001 - Shortest-float tie-break vs .NET Single.ToString

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷` (Unity 2022.3.6f1), class-4 `m_LocalPosition.z` (e.g. local fileID 34284); frozen reference YAML spellings `-280.89062`/`-477.89062` vs Rust `-280.89063`/`-477.89063`. |
| Root cause and proof | Both spellings parse to identical `f32` bits (`0xC38C7200`, `0xC3EEF200`; exact midpoint values `-280.890625`, `-477.890625`), so decoding is exonerated. Rust's `LowerExp` picks the odd shortest neighbor where .NET resolves the exact tie toward the even last digit. All 34 residuals are this single pattern (one field, two value pairs). |
| Rust change | `round_shortest_tie_to_even_f32` adjusts only the shared `format_dotnet_f32` path: same-length round-tripping neighbor runs are enumerated (capped, rollover-safe) and the closest-to-exact spelling wins with exact `f64` ties broken toward even; all other values return Rust's string untouched. `f64` formatting is unchanged (no observed divergence; remains open). |
| Verification | Reference-derived regression vectors plus non-tie controls; full `unity_yaml`/`export` suites, warnings-denied Clippy, release build, and the r10 replay (class 4: 20,547/20,547 exact; scene totals 49,862 exact + 21,373 semGUID with 2 residuals) are recorded in the validation-log entry above. The r9-vs-r10 whole-tree guard (232 changed non-`.meta` files, zero structural/line-count diffs, 28/28 unique number pairs bit-identical as `f32`) proves the formatter change is spelling-only. |
| Limits | `f64` tie behavior is unprobed. Class 157 and class 320 scene residuals remain open. Unity Editor import/runtime and UABEA were not run. |

## DIFF-ENGINE-PREDEFINED-BANDS-001 - Engine-assets TPK version-band selection for predefined Material/Shader identity

| Field | Finding |
|---|---|
| Input and reference | `青岚山河卷` (Unity 2022.3.6f1) and 青岚雅苑` (Unity 2022.3.11f1); frozen `EngineAssetsExporter.CreateFromTpkBlob` (`AssetRipper.Export.UnityProjects/EngineAssets/EngineAssetsExporter.cs` lines 34-61); isolated probe `tools/probes/engine-assets/EngineAssetsProbe.csproj` (`bands`, `bandjson`, `tsv` modes) run against the isolated reference build with .NET SDK 10.0.401, all exit 0. |
| Before-fix difference | Rust r8 exported a stray `Material/Default-Material.mat` and pointed one class-23 and one class-114 document at it (fileID 2100000, generated GUID, type 2); the frozen source points both at builtin-extra fileID 10303/FGUID and emits no such asset. Cause: `predefined_engine_material_target` applied its table only for `2022.3 patch >= 11`, while the shader table applied unconditionally to every version. |
| Source behavior (probed) | The embedded blob carries 114 ordered band keys. `bandjson 2022.3.6f1` selects `2022.2.15f1`; `bandjson 2022.3.22f1` selects `2022.3.11f1` — the same nearest-key-at-or-below rule as the frozen selector, verified live. Projecting both bands to the committed TSV row format reproduces the committed material rows exactly from the 2022.2.15f1 band (15/15) and the committed shader rows exactly from the 2022.3.11f1 band (202/202). The two bands' material rows are mutually byte-identical; shader rows differ in 7 instances (fileID 111 properties; fileIDs 112/114/15313/15314/15315 only in 2022.3.11f1). DefaultResources pathID-to-type rows are identical, so the class-ID validation table needs no band split. |
| Rust change | `select_engine_assets_band` ports the exact source band selection over embedded `engine_asset_bands.tsv` (ordered with `FactoryUnityVersion::source_order_bits`, the frozen `GetBits` order). Material/Shader lookups read band-keyed tables and return `None` for unprobed bands. Old version-named TSVs are replaced by band-keyed fixtures; the probe project is committed (its `bin`/`obj` outputs stay ignored). The shader reverse-name path stays band-independent with justification. |
| Verification | Routing tests, `export::` suite, warnings-denied Clippy, release build, and the r9 replay with per-class byte-exact and GUID-normalized audits are recorded in the validation-log entry above, together with the 松月竹庭 r7 regression replay (Cube pointer preserved, Cat_Ear byte-identical, source-vs-r7 comparison identical-or-better than r6 on every metric). |
| Limits | Only the 2022.2.15f1 and 2022.3.11f1 bands have embedded tables; versions selecting other bands get no predefined redirect (documented debt, fail-open to normal export). Non-null `m_NavMeshData`, float last-digit prints, the lighting path, and PlayableDirector bindings remain open. Unity Editor import/runtime and UABEA were not run. |
