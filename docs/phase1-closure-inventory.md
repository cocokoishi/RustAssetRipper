# Phase 1 closure inventory

Current inventory basis: local `main` and fetched `origin/main` are both `a63437d501c721c24948eff66b3b982dd7a052e4` (tree `e4358b42aa1344751059e267d0f0f67d37916851`), pushed and verified. This commit contains the nested-registry implementation, initial workspace YAML export, multi-file path differential, and Windows local-file sharing fix recorded below. The working tree may retain untracked probe `bin`/`obj` build outputs; they were not committed. Reference source: `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper`, frozen by `docs/baseline-files.sha256` (1,449 entries; manifest SHA-256 `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`).

The status counts below come from the `Status` cell in each stable-ID row of `docs/feature-parity.md`, matched against the required status vocabulary; prose mentions elsewhere do not affect the count.

| Ledger status | Rows |
|---|---:|
| `已验证` | 342 |
| `已实现但未验证` | 17 |
| `实现中` | 14 |
| `已调查` | 4 |
| `未调查` | 0 |
| `受外部条件阻塞` | 0 |
| **Open rows** | **35** |

All 35 queued rows currently block the Phase 1 gate: each maps to baseline shipping behavior, and no exact verified disposition removes it from the product scope. `TYPE-SCRIPT-001` still needs a source-backed runtime-versus-tooling disposition. Unity-only evidence is a real gap where listed; it is not counted as passed. The inventory is the Phase 1 work queue and should be refreshed only after closing a complete behavior or finding a new source-backed disposition.

## Closed behavior units

| Stable ID | Closure evidence |
|---|---|
| `SER-MANAGED-REFS-001` | The frozen source reader consumes independent modern counted and legacy terminated nested-Registry fixtures fully. Rust decode/write byte round trips match both fixtures, and duplicate IDs resolve in their nearest registry-owning struct scope. See `DIFF-MANAGED-REFS-002` and the source probe and regression test named in `docs/feature-parity.md`. |
| `IO-STREAM-PATH-001` | An isolated AssetRipper IO.Files runtime probe compared regular paths, split base/part paths, logical names, recognition rules, numeric ordering through `.split10`, boundary reads, missing parts, and `.split00` rejection. Rust's path fixture test matches the observable outcomes, and `SchemeReader::mount_path` integration tests verify logical-name mounting and padded-part rejection. The source's End-seek sign difference is recorded separately under `DIFF-MULTIFILE-SEEK-END-001`. |
| `PROC-PREFAB-SCENE-BUNDLE-001` | The integrated `plan_workspace_scene_hierarchy_bundle_names` planner composes source-order scene/collection traversal, per-bundle `FetchAssets()`-ordered asset facts, real `get_asset_bundle_name` resolution, and the streamed-scene `Debug.Assert` as an explicit release-continue warning. CLI `print_workspace_summary_to` resolves live class-142 payloads and reports per-scene hierarchy bundle names. See `DIFF-PREFAB-SCENE-BUNDLE-001` and the planner plus CLI regression tests named in `docs/feature-parity.md`. |
| `IO-FS-IMPORT-PATH-001` | An isolated AssetRipper IO.Files runtime probe wrote aligned and unaligned WebData containers and pinned entry inventory, truncation fallback to a raw leaf, split base/part resolution, and missing-part rejection. Rust replays every row through production `mount_path` with byte-identical mounts and fail-closed gaps. See `DIFF-SINGLEFILE-IMPORT-001` and the regression test named in `docs/feature-parity.md`. |

## Dependency order

1. Close the import, container, serialization, generated-object, and reference path together. Do not treat file inventory or decoded TypeTree values as a complete object model.
2. Close workspace scene and Prefab materialization, then animation processing on those objects.
3. Finish asset decoders and exporter families, including their resource streams and importer metadata.
4. Connect the complete collection set to one end-to-end Unity project export path, then close configuration and user workflows around it.
5. Run the source differential, independent fixture, error-path, build, and Unity checks required for each closed row and the final gate.

The row-level work units below are the smallest complete closures justified by distinct source behavior. A passing unit test or a detached kernel does not close a row whose source caller, object construction, or product route remains absent.

## Queue

### Import, container, and resource loading

| Stable ID | Status | Baseline source symbols/files | Missing work type and shortest complete closure unit | Gate |
|---|---|---|---|---|
| `PRODUCT-CLI-INSPECT-001` | `实现中` | `GameFileLoader.LoadAndProcess`; `GameStructure.CheckStructure`; `GameFileLoader.ExportUnityProject` and `ExportPrimaryContent` | **Integration/product:** make the Rust directory workflow discover platform and mixed inputs, mount dependencies, materialize the workspace, and report the same observable summary through the supported CLI entry point; exercise errors and batch behavior against reference output. | Yes |
| `IO-FS-IMPORT-PRESERVE-DIRECTORY-001` | `已实现但未验证` | `LocalFileSystem.File.OpenRead`; generated `LocalFileSystem.FileImplementation`; `MultiFileStream.OpenRead`; `SchemeReader.LoadFile`; platform `CollectFiles` | **Differential/platform:** Windows `FileShare.Read` lock behavior is now runtime-pinned and implemented for regular and split inputs; compare directory enumeration and retained-reader behavior on supported non-Windows hosts, then rerun the integrated directory import fixture. | Yes |
| `IO-STREAM-INPUT-001` | `已实现但未验证` | `SchemeReader.LoadFile`; `SchemeReader.ReadFile` | **Implementation/integration:** replace full-input materialization on supported paths with bounded seekable streaming, then validate nested wrapper/container parsing and resource limits on independently produced inputs. | Yes |
| `IO-STREAM-001` | `实现中` | `RandomAccessStream`; `PartialStream`; `MultiFileStream`; `SmartStream` | **Implementation/integration:** complete the source stream semantics through local file imports and resource/container readers, including partial and split reads, ownership/lifetime, seek behavior, and bounded I/O; run reference differential cases. | Yes |
| `IO-BUNDLE-001` | `实现中` | `SchemeReader`; `BundleHeader`; `FileStream`; `Archive`; `RawWeb` | **Implementation/integration:** support every baseline-supported bundle form, entry layout, placement, recursive mount, and dependency path. Keep UnityArchive explicitly rejected if the frozen source also throws; verify that disposition from source and runtime. | Yes |
| `IO-COMP-001` | `实现中` | `CompressionType`; `CompressedFiles/Brotli`; `CompressedFiles/GZip`; bundle codec dispatch | **Implementation/differential:** close each source-supported codec and every wrapper/bundle dispatch lane with independent codec fixtures and real reference-generated outputs; make unsupported source cases explicit. | Yes |
| `IO-RESOURCE-001` | `实现中` | `ResourceFile`; `Bundle.ResolveResource`; `GameBundle.FromPaths`; `GameBundle.ResolveExternalResource`; `StreamedResourceExtensions`; `StreamingInfoExtensions` | **Integration:** connect root and external resource lookup plus validated offset/size reads to generated object decoding and exporters, including version-specific `StreamingInfo` field access and missing-resource diagnostics. | Yes |

### Serialization, generated objects, and reference graph

| Stable ID | Status | Baseline source symbols/files | Missing work type and shortest complete closure unit | Gate |
|---|---|---|---|---|
| `SER-TT-ABSENT-001` | `实现中` | `GameAssetFactory`; `TypeTreeObject`; `UnknownObject`; `UnreadableObject`; `RawDataObject` | **Implementation:** build the source fallback objects and recover objects when TypeTrees or generated types are absent; include TPK and unknown-class paths, raw preservation, and reference-runtime fixtures. | Yes |
| `TYPE-SCRIPT-001` | `已调查` | `AssetRipper.Import/Structure/Assembly/`; `MonoManager`; `IL2CppManager`; `TypeTreeResolver`; `SerializableType`; `ScriptIdentifier`; MonoScript/MonoBehaviour extensions | **Disposition/implementation:** establish which assembly and IL2CPP paths are shipping runtime behavior versus developer tooling; port all required metadata/type recovery into Rust without a .NET runtime parser dependency and verify managed/stripped cases. | Yes |
| `OBJ-REF-001` | `实现中` | Asset collections, traversal, bundles, and `ObjectInfo` | **Integration:** join file, bundle, local, external, and built-in identities into a complete graph with stable export mappings; classify null, missing, ambiguous, invalid, and unresolved references and preserve cycles. | Yes |
| `SER-ASSET-FACTORY-001` | `已实现但未验证` | `GameAssetFactory.ReadAsset`; `ReadNormalObject`; `TryReadNormalObject`; `CreateAsset`; `CreateEngineAsset` | **Implementation/integration:** materialize source-generated classes and all source factory fallback/retry branches from real SerializedFile objects, including engine assets and assembly-backed objects; compare object type and fields with the reference. | Yes |

### Scene, Prefab, and workspace processing

| Stable ID | Status | Baseline source symbols/files | Missing work type and shortest complete closure unit | Gate |
|---|---|---|---|---|
| `PROC-SCENE-001` | `实现中` | `Processing/Scenes`; `Prefabs`; `SceneHierarchyObject.GetRoots`; `TraversalHelperMethods`; `TransformExtensions`; `EditorFormatConverterAsync` | **Integration:** create typed GameObject/component/Transform graphs from the workspace, preserve cross-file relationships and ordered components, then run Scene and Prefab processors including generated/missing objects and diagnostics. | Yes |
| `PROC-SCENE-NAMING-001` | `实现中` | `SceneHelpers`; `SceneDefinitionProcessor` | **Integration:** materialize the source scene collections and definitions, resolve BuildSettings indexes, paths, GUIDs, duplicate entries, and streamed-scene hashes/container mappings from serialized input. | Yes |
| `PROC-SCENE-BUILDSETTINGS-001` | `实现中` | BuildSettings class ID 141; `Pass002_RenameSubnodes` | **Implementation/integration:** decode real legacy and modern BuildSettings objects, preserve ordered paths, and apply the source rename/association pass to project objects; compare an independently created SerializedFile to the reference. | Yes |
| `PROC-SCENE-INSPECT-001` | `已实现但未验证` | `SceneHierarchyObject.GetRoots`; `TransformExtensions.CalculateRootOrder`; workspace inspect | **Integration:** make inspection operate on the same typed SceneHierarchy objects used by processing/export, with root order, ownership, cross-file edges, and unresolved-reference outcomes; retain a CLI end-to-end fixture. | Yes |

### Animation processing and export

| Stable ID | Status | Baseline source symbols/files | Missing work type and shortest complete closure unit | Gate |
|---|---|---|---|---|
| `PROC-ANIM-001` | `已调查` | `AnimationClipConverter`; `CustomCurveResolver`; `VirtualAnimationFactory`; Animator controller state/machine contexts and processor | **Implementation/integration:** complete the animation processing sequence on materialized assets: hierarchy/avatar/TOS caches, script bindings, virtual hierarchy, state and transition contexts, controller release rebuild, and curve emission. Validate ordering and mutations against the source. | Yes |
| `PROC-ANIM-STREAMED-001` | `已实现但未验证` | `StreamedCurveKey.Read` and `CalculateSlopes`; `StreamedFrame.Read`; `AnimationClipConverter.TryGetNextFrame` | **Integration:** assemble streamed buffers and connect key/frame parsing, duplicate collapse, slope rules, bindings, and next-frame search to the complete streamed/dense/constant clip conversion; compare complete clips, not isolated key structs. | Yes |
| `PROC-ANIM-CONVERT-001` | `已实现但未验证` | `AnimationClipConverter.InitializeBindingsCache`; constant/dense timing; `GenericBindingExtensions.IsTransform` | **Integration:** emit transform, default, and custom curves into mutable generated AnimationClip objects with correct keyframes, tangents, bindings, events, and source ordering; compare serialized output and sampled behavior. | Yes |
| `PROC-ANIM-CUSTOM-CURVE-001` | `已实现但未验证` | `CustomCurveResolver.ToAttributeName`; `BindingCustomType`; `TransformType`; `AnimationClipConverter` fallback prefixes | **Integration:** focused `custom_curve::tests` passes 12/12. The live 2022.3 fixture has 39 resolver rows, 14 `ReverseAscii` outputs, and 2 `HashAscii` marker hashes; the 11 component dictionaries contain 2,551 entries. The source's `HHHHHHH` marker and exception behavior are pinned. The resolver still accepts caller-supplied scene facts; wire root/child, renderer, material, and blend-shape lookups into actual clip conversion before closing the full behavior. | Yes |
| `RES-ANIM-001` | `已调查` | `AnimatorControllerExporter`; `AnimatorControllerExportCollection`; animation and controller processors | **Implementation/integration/Unity evidence:** emit clips/controllers with editor hierarchy, state contexts, bindings, events, and behavior references; compare static YAML and run Unity dynamic sampling for curves, transitions, blend trees, and root motion. | Yes |

### Resource decoding and exporter families

| Stable ID | Status | Baseline source symbols/files | Missing work type and shortest complete closure unit | Gate |
|---|---|---|---|---|
| `RES-MATERIAL-EXPORT-001` | `已实现但未验证` | `ShaderExportCollection`; YAML/dummy/simple shader exporters; `ProjectExporter` material emission | **Implementation/integration:** render parsed-form shader properties and emit complete versioned Material/Shader assets through exporter selection, including all serialized material fields; test reference output and a Unity-produced fixture. | Yes |
| `RES-TEXTURE-001` | `实现中` | `Export.Modules.Textures`; `Export.UnityProjects/Textures` | **Implementation/integration:** support every baseline texture/sprite/lightmap type and platform branch, resolve streamed data, apply format/importer settings, and route the results through project export; validate pixels and metadata. | Yes |
| `RES-MODEL-001` | `实现中` | `Export.Modules.Models`; `Export.UnityProjects` | **Implementation/integration:** complete model/scene/terrain export with vertex/index/topology, skinning, bind pose, blend-shape, material, hierarchy, and resource-stream handling through the collection pipeline. | Yes |
| `RES-AUDIO-001` | `已实现但未验证` | `AudioClipDecoder`; `AudioConverter`; UnityProjects Audio; AudioClip/FMOD extensions | **Implementation/integration:** port required FSB5 and Vorbis conversion behavior in Rust, finish generated class-field routing and resource streams, and exercise inline and external audio on independent/reference fixtures. | Yes |
| `RES-MESH-VIEW-001` | `已实现但未验证` | `MeshExtensions.ReadData`; `VertexDataBlob.Create`; `MeshData.TryMakeFromMesh` | **Implementation/integration:** settle versioned field naming from reference runtime, complete metric/blend-shape/streamed-data reads, and build `MeshData` from real serialized class-43 objects through its export callers. | Yes |
| `EXP-GLB-001` | `已实现但未验证` | `GlbMeshBuilder`; `GlbSubMeshBuilder`; `GlbMeshType`; coordinate conversion; `GlbWriter` | **Implementation/integration:** finish scene-level and terrain GLB export and blend-shape output, connect processed assets and resolved materials to the writer, and validate full output semantics in an independent viewer/Unity import. | Yes |
| `RES-SHADER-PARSED-001` | `已实现但未验证` | `DummyShaderTextExporter`; serialized property extensions; template and property metadata | **Implementation/integration:** consume parsed-form properties from generated shader objects and emit the full dummy shader output through shader collection dispatch; validate with serialized Unity-produced shader data. | Yes |
| `RES-BLENDSHAPE-001` | `已实现但未验证` | `MeshBlendShapeExtensions`; `BlendShapeDataExtensions`; `MeshExtensions.FindBlendShapeNameByCRC`; `SetValues` | **Implementation/integration:** connect name/hash resolution to geometry frame/delta/weight decoding and animation binding; cover legacy list and channel layouts with independent versioned inputs. | Yes |
| `EXP-YAML-STREAMED-ASSET-PREP-001` | `实现中` | `YamlStreamedAssetExportCollection.ExportMesh` and `ExportTexture` | **Integration:** feed resolved streamed resources and version-specific inline fields into actual YAML collection exporters, preserve the input object, and validate complete mesh/texture YAML plus sidecar resources from an external fixture. | Yes |

### Project export, configuration, and product surface

| Stable ID | Status | Baseline source symbols/files | Missing work type and shortest complete closure unit | Gate |
|---|---|---|---|---|
| `EXPORT-UNITY-001` | `实现中` | `Export.UnityProjects` (124 files); `ExportHandler`; `Meta` | **Remaining integration:** connect source processors and collection selection to the project path, materialize generated Scene/Prefab assets, add class-specific binary/exporter families and importer variants, apply complete settings, and cover progress, cancellation, and errors. The initial `export-project` CLI now mounts mixed files/directories, resolves cross-file references, stages YAML, and writes one `.asset` plus `.meta` per SerializedFile with ProjectVersion and package files; synthetic CLI tests cover successful cross-file export and fail-closed unresolved pointers. Keep the row open until the complete baseline collection and exporter pipeline is integrated and validated. | Yes |
| `EXP-SETTINGS-APPLY-001` | `已实现但未验证` | Image, shader, and audio `ExportSettings` fallbacks | **Integration:** apply all settings to every exporter family and project workflow, including explicit CLI overrides, defaults, YAML and mesh exporters, and persisted config; verify output semantics and error behavior. | Yes |
| `EXP-META-PIPELINE-001` | `已实现但未验证` | `AssetExportCollection.Export`; `ExportCollection.ExportMeta` and `ExportAsset` | **Integration/differential:** create importer records and GUIDs inside collection dispatch, write asset and metadata as a coherent export transaction, and resolve the documented overwrite-versus-add-only difference under the required safe output policy. | Yes |
| `EXP-FILE-NAME-001` | `已实现但未验证` | `FileSystem` filename helpers; `Utf8Truncation`; `ExportCollection.GetUniqueFileName` | **Integration/differential:** use filename planning in all collection exports and verify collision, host-specific invalid characters, UTF-8 truncation, extension, and Unix backslash behavior against the source. | Yes |
| `PRODUCT-GUI-001` | `已调查` | `GUI.Free`; `GUI.Web` pages, arguments, launcher, loader, commands | **Implementation/disposition:** provide equivalent browse/search/settings/import/export/diagnostic workflows with progress, cancellation, errors, localization, and headless mode; document any source feature proven dev-only. | Yes |
| `PRODUCT-I18N-001` | `已实现但未验证` | `GUI.Localizations`; `Localizations` tables | **Integration:** connect all 272 message keys and en-US fallbacks to the user interface, preserve startup language validation and settings behavior, and verify representative translated workflows. | Yes |

## Current verification state

- `cargo fmt --all -- --check` passes.
- The focused resolver suite passes 12/12. Its fixture assertions cover 39 actual `ToAttributeName` cases, 14 manufactured-name results, and 2 marker-hash results; the generated component tables contain 2,551 entries.
- Full workspace tests pass: 1,409 library tests and 68 CLI tests; there are 0 doc tests. Warnings-denied all-target/all-feature Clippy passes.
- `tools/verify-baseline.ps1` exits 1 because of 188 unlisted build-output files created by the earlier source probe; the previous read-only checksum pass found all 1,449 manifest-listed reference files unchanged. Cleanup remains blocked by automatic review, so the verifier is not passed. `git diff --check` passes after the current documentation edits.
- This evidence validates the local kernel and source-derived vectors. It does not close the row because no production `AnimationClipConverter` caller constructs the scene facts or routes real clip bindings through the resolver.
- No user AssetBundle content or UABEA content has been read. Phase 2 remains locked.
