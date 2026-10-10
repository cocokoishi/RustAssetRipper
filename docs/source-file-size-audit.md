# Source file size audit

Date: 2026-10-09

## Method and result

The scan used `rg --files` over Rust, C#, PowerShell, Python, JavaScript/TypeScript, shell, C/C++, Java, Go, SQL, Vue, HTML, and CSS source files. It counted physical lines with `.NET` `File.ReadAllLines` and treated a file as oversized only when it had more than 2,000 lines. The scan included `vendor/` so third-party exceptions remain visible.

Before this refactor, the scan found 28 oversized files:

| Lines | File |
|---:|---|
| 7,670 | `src/processing/scene_hierarchy.rs` |
| 7,074 | `src/export/workspace_yaml.rs` |
| 6,983 | `src/export/value_yaml.rs` |
| 6,808 | `src/scheme.rs` |
| 6,761 | `src/export/project_workflow.rs` |
| 5,899 | `src/processing/scene.rs` |
| 4,784 | `src/serialized_file/object.rs` |
| 4,682 | `src/processing/prefab_processing.rs` |
| 4,159 | `src/resources/custom_curve_tables.rs` |
| 3,836 | `src/serialized_file/reference_graph.rs` |
| 3,414 | `src/processing/asset_factory.rs` |
| 3,360 | `tools/probes/managed-project/Program.cs` |
| 3,319 | `src/processing/asset_bundle.rs` |
| 3,270 | `src/container/unityfs.rs` |
| 3,253 | `vendor/dotnetdll-0.3.0/src/resolution/read.rs` |
| 3,178 | `src/serialized_file/metadata.rs` |
| 2,879 | `src/io/platform_file_inventory.rs` |
| 2,866 | `src/export/scene_wiring.rs` |
| 2,824 | `src/resources/glb.rs` |
| 2,819 | `src/processing/scene_collections.rs` |
| 2,796 | `src/io/local_filesystem.rs` |
| 2,754 | `src/numerics.rs` |
| 2,693 | `src/io/stream.rs` |
| 2,629 | `src/resource.rs` |
| 2,506 | `src/processing/managed_assembly.rs` |
| 2,353 | `src/io/platform_structure.rs` |
| 2,294 | `src/serialized_file/object/writer.rs` |
| 2,286 | `src/serialized_file/file/writer.rs` |

The current scan leaves one over-limit file: the 3,253-line vendored `dotnetdll` source above. It remains untouched to preserve third-party provenance. No first-party source file exceeds 2,000 lines.

Run the repeatable check with:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\tools\check-source-file-lengths.ps1
```

The check reports every over-limit source file, fails for first-party files, and reports vendored files as explicit exceptions.

## Refactor decisions

- Split the production declarations in `scene_hierarchy`, `workspace_yaml`, `value_yaml`, `scheme`, `project_workflow`, `scene`, `prefab_processing`, `asset_bundle`, `object`, `reference_graph`, `asset_factory`, and the SerializedFile rewrite writer into ordered same-module source fragments. Keeping them in the same Rust module preserves private visibility and behavior while making each source file reviewable in smaller sections.
- Moved embedded Rust tests out of 25 oversized files into adjacent test shards. Test shards remain included inside the original test module, retaining access to the same private items. Existing large CLI and workspace test splits were already present when this scan began.
- Split `ManagedProjectProbe` into its top-level dispatcher and five partial-class source files grouped by probe domain. The command-line behavior stays in `Program.cs`.
- Split the generated ParticleSystem path table into two ordered data chunks and a small compile-time join. All 1,792 `(hash, path)` entries were compared in order with the pre-split table from `HEAD`; they matched exactly.
- Left the vendored `dotnetdll` implementation intact.

## Checks

- `cargo fmt --all -- --check` — passed.
- `cargo check --workspace --locked` — passed.
- `cargo check --workspace --locked --tests` — passed; this compiled test targets without executing test cases.
- `dotnet build tools/probes/managed-project/ManagedProjectProbe.csproj --no-restore -p:ReferenceRoot="$env:LOCALAPPDATA\Temp\opencode\ar-ref"` — passed with zero warnings and zero errors.
- ParticleSystem table comparison — passed for all 1,792 ordered entries.
- `tools/check-source-file-lengths.ps1` — passed; no first-party file exceeds 2,000 lines, and the vendored 3,253-line file is reported as an exception.
- Test cases were not executed for this structural refactor.

The source fragments use `include!` to retain one module scope; this avoids visibility churn, but diagnostics for included declarations can point to the fragment file. Future changes should keep the fragment names and include order aligned with the declaration groups.
