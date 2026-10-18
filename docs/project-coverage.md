# Project coverage audit

Status vocabulary follows the feature matrix: `未调查`, `已调查`, `实现中`, `已实现但未验证`,
`已验证`, `受外部条件阻塞`. A per-project status is coarser than the behavior ledger: it reports
the project as a whole, while `feature-parity.md` rows remain the authoritative per-behavior truth.
`已验证` on a test project means its tests were executed as reference evidence, not ported.

Reference identity: `AssetRipper/` snapshot, version prefix `2.0.0`, `net10.0`, 51 solution projects
(22 libraries, 15 tools, 8 tests, 6 source generators). Baseline manifest `docs/baseline-files.sha256`.

## Libraries (22)

| Project | Capability | Rust counterpart | Status | Notes |
|---|---|---|---|---|
| AssetRipper.Assets | Core object model: AssetInfo, IUnityObjectBase, bundles, collections, SceneDefinition | `serialized_file/collection_workspace.rs`, `processing/scene_collections.rs`, `processing/project_plan.rs` (GUID rule) | 实现中 | Identity/collection/GUID rules ported and tested; generated object materialization open |
| AssetRipper.Configuration | DataStorage, DataEntry, DataInstance, DataSet and typed configuration-file serializers | `src/configuration.rs`, `src/export/settings_file.rs`, `src/web/state.rs`, `src/web/routes.rs` | 实现中 | Insertion-ordered singleton/list stores and the three typed settings records match the bounded configuration-files HTTP workflow. EngineResourceData null/invalid handling and empty-dictionary JSON text also match; non-empty payload/export behavior, generic Parsable/Json DataSet types, and native file-dialog inputs remain open. The stores are session-only. |
| AssetRipper.DocExtraction | Documentation XML extraction for the dumper chain | none | 未调查 | File list only |
| AssetRipper.Export.Modules.Audio | FMOD/NAudio decode, AudioClipDecoder/Converter | none | 未调查 | File list only; needed for audio export parity |
| AssetRipper.Export.Modules.Models | SharpGLTF GLB model/mesh/navmesh/terrain export | `src/resources/glb.rs`, `src/resources/glb_level.rs`, `src/export/scene_glb.rs`, `src/main_cli_primary_content.rs` | 实现中 | Mesh, terrain and navmesh routes are exercised; one Unity 2019.4 cross-file Prefab triangle is compared through primary content. Wider model version and topology coverage remains open |
| AssetRipper.Export.Modules.Textures | Crunch/FastPng/UnityCrunch/TextureDecoder conversion | none | 未调查 | File list only; needed for texture export parity |
| AssetRipper.Export.PrimaryContent | Second export pipeline and content extractors | `export/primary_content.rs` (handlers, order, paths) | 实现中 | Planning verified; extractor internals open |
| AssetRipper.Export.UnityProjects | Main export pipeline, collections, YAML walkers, Meta | `export/*` planners, tables, `meta.rs`, `unity_yaml.rs` | 实现中 | Planning/routing verified; byte-writing pipeline and generated schemas open |
| AssetRipper.Export | Exporter interfaces, script/decompile infrastructure, dummy exporters | `export/registration.rs` (Dummy semantics) | 已调查 | Interfaces and dummy rules audited; decompile chain not ported |
| AssetRipper.GUI.Free | One-file launcher delegating to the web app | `product.rs` (launch defaults) | 已调查 | Single delegation line; covered by surface audit |
| AssetRipper.GUI.Licensing | Premium licensing UI and generator | none | 未调查 | File list only |
| AssetRipper.GUI.Localizations | Internationalization resources and generator | none (`ExportDefaults.language_code` is a stub) | 未调查 | File list only |
| AssetRipper.GUI.Web | The web application: routes, commands, loader, pages | `product.rs` (routes, commands, loader) | 实现中 | Surface and workflow verified; pages/handlers/serve open |
| AssetRipper.Import | Game structures, bundle loading, assembly managers, config | `io/platform_structure.rs`, settings values | 实现中 | Platform/mixed structures verified in part; managers and loading open |
| AssetRipper.IO.Files | Containers, streams, VFS, FileSystem, SerializedFile IO | `container/*`, `io/*`, `scheme.rs`, `serialized_file/*` | 实现中 | Broad synthetic coverage; differentials and adapters open |
| AssetRipper.Numerics | Math primitives (vectors, quaternions, transforms, ranges) | none | 未调查 | File list only; reference tests executed 65/65 as evidence of source health |
| AssetRipper.Processing | Asset processors and processing configuration | `processing/pipeline.rs`, scene/prefab planners | 实现中 | Order verified; processor internals partial |
| AssetRipper.SerializationLogic | Managed references, field/mono-type serialization | `serialized_file/managed_types.rs` (tables and schema) | 实现中 | Tables and registry schema verified; Cecil resolution and payload decoding open |
| AssetRipper.SourceGenerated.Extensions | Handwritten generated-class extensions | traced across many increments | 已调查 | Individual rules ported where snapshot-determined; `Has_*` gates are generated and open |
| AssetRipper.Web | HTML rendering helpers and YAML web extensions | none | 未调查 | File list only |
| AssetRipper.Yaml | YAML DOM, scalars, writer, escaping | `export/unity_yaml.rs` | 实现中 | Writer verified against source order/scalars; emitter edge cases open |
| UnityEngine | Test-stub Unity API types | none | 未调查 | Test support only |

## Tools (15)

All tools are inventoried by entry point only; none is ported. Every row is `未调查` for behavior.

| Project | Capability |
|---|---|
| AssetRipper.AssemblyDumper | Unity assembly dumping driver (Exe) |
| AssetRipper.AssemblyDumper.Utils | Dumper shared utilities |
| AssetRipper.AssemblyDumper.Downloader | Dumper asset downloader (Exe) |
| AssetRipper.AssemblyDumper.NativeEnumExtractor | Native enum extraction (Exe) |
| AssetRipper.AssemblyDumper.NuGetFixer | NuGet package repair (Exe) |
| AssetRipper.AssemblyDumper.Recompiler | Assembly recompilation (Exe) |
| AssetRipper.DocExtraction.ConsoleApp | Documentation extraction driver (Exe) |
| AssetRipper.Tools.CabMapGenerator | CAB map generation (Exe) |
| AssetRipper.Tools.DependenceGrapher | Dependency graphing (Exe) |
| AssetRipper.Tools.FileExtractor | File extraction (Exe) |
| AssetRipper.Tools.JsonSerializer | JSON serialization probe (Exe) |
| AssetRipper.Tools.MonoBehaviourTester | MonoBehaviour testing (Exe) |
| AssetRipper.Tools.RawTextureExtractor | Raw texture extraction (Exe) |
| AssetRipper.Tools.SystemTester | System testing (Exe) |
| AssetRipper.Tools.TypeTreeExtractor | TypeTree extraction (Exe) |

## Tests (8)

Executed independently from the isolated copy as reference evidence (490 passed, 20 failed on the
CRLF checkout artifact `DIFF-REFERENCE-TEST-NEWLINE-001`, 0 other failures). Not ported.

| Project | Result |
|---|---|
| AssetRipper.AssemblyDumper.Tests | 9/9 passed |
| AssetRipper.Assets.Tests | 57/57 passed |
| AssetRipper.GUI.Web.Tests | 6/6 passed |
| AssetRipper.IO.Files.Tests | 137/137 passed |
| AssetRipper.Numerics.Tests | 65/65 passed |
| AssetRipper.SerializationLogic.Tests | 54/54 passed |
| AssetRipper.Tests | 151 passed, 20 artifact failures |
| AssetRipper.Yaml.Tests | 11/11 passed |

## Source generators (6)

Rust has no C# source generators; what matters is their output. The generated class package
(`AssetRipper.SourceGenerated` 1.4.0) resolves from the snapshot feed and its member metadata is
available for audit. All rows are `已调查` with outputs as the ongoing audit target.

| Project | Generates |
|---|---|
| AssetRipper.GUI.Licensing.SourceGenerator | Licensing keys |
| AssetRipper.GUI.Localizations.SourceGenerator | Localization resources |
| AssetRipper.GUI.SourceGenerator | GUI glue |
| AssetRipper.IO.Files.SourceGenerator | Serialized-file IO helpers |
| AssetRipper.Processing.SourceGenerator | Processing helpers |
| AssetRipper.SourceGenerated.Extensions.SourceGenerator | Generated-class extension members |

## Summary counts

- Libraries: 0 已验证, 3 已调查, 10 实现中, 9 未调查.
- Tools: 0 ported, 15 未调查.
- Tests: 8 executed as evidence.
- Generators: 6 已调查 (outputs pending).

This document is the backbone for the Phase 1 completeness gate, not the gate itself: every
`未调查` row above must shrink to a behavior-ledger entry with evidence before the gate can pass.
