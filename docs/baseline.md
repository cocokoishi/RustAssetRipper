# AssetRipper source baseline

## Identity

- Reference path: `C:\Users\koishi\Desktop\RustAssetripper\AssetRipper`.
- The reference directory has no `.git` metadata. Commit, branch, tag, and submodule revisions therefore cannot be recovered from this copy.
- `Source/Directory.Build.props` declares version prefix `2.0.0`, target framework `net10.0`, nullable analysis, latest C# language version, AOT compatibility, and checked integer arithmetic.
- The upstream README claims Unity support from `3.5.0` through `6000.4.X`; it also cautions that support quality varies by version. This is a source claim, not independent compatibility evidence.
- `AssetRipper.slnx` lists 51 projects: 8 test projects and 6 source-generator projects, alongside core libraries, GUI/web components, export modules, developer tools, and assembly processing tools.
- `baseline-projects.csv` records every solution project, project references, directly declared NuGet packages, project kind, and output type as found in the local snapshot. Transitive dependency and license analysis remain open.
- The local snapshot contains 1,449 files in 188 directories, totaling 23,247,459 bytes at the initial inventory.
- The root license is GNU GPL version 3; an identical copy is retained as `LICENSE.md` in the Rust repository (SHA-256 `81cbae84a29ce7e770bf2bc7b178e50bda0ce8de6067aba661b0bc7b05b562f8`). Third-party notices are present under `Source/Licenses`; dependency-by-dependency license and migration analysis remains open.

## Reproducible identity for this non-Git snapshot

`baseline-files.sha256` contains a SHA-256 checksum and relative path for every file under the reference directory at the initial freeze. It has 1,449 entries. Its own SHA-256 is `29d1fd27f66b974c43750f30aa99117eb423cb318f3062daf05e1489f10f08f9`.

The reference source was read only. No build, export, or test output was written into it. The checksum manifest is stored in this Rust repository under `docs/`.

## Build and test availability

- `dotnet --info` was attempted and could not run because `dotnet` was not found on `PATH`.
- `cargo`, `rustc`, and `rustup` are not on `PATH`, but were found under `%USERPROFILE%\.cargo\bin` and used directly.
- The active toolchain is `rustc 1.98.1`, `cargo 1.98.1`, `stable-aarch64-pc-windows-msvc`. The Rust package now declares 1.87 as its minimum because `ruzstd` 0.9.0 requires it; that minimum version has not been tested.
- The reference solution includes 8 test projects, but its tests remain unrun because `dotnet` is unavailable.
- Current Rust verification is recorded in `validation-log.md`.
- The Rust repository had no refs at initial identification. The active `main` branch now tracks the configured `origin/main`; validated Rust increments are committed and pushed there. No other repository is a write target.

## Scope findings still open

The first source inventory identifies project boundaries, generated-code projects, file/container modules, import, processing, export, configuration, localization, web UI, and developer tools. Full dependency graphs, every version/platform branch, registration path, option/default, GUI workflow, test mapping, and third-party license obligations have not yet been audited. The feature matrix marks those areas as not investigated; the first phase gate is not passed.
