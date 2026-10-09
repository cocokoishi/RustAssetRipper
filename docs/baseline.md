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

- `dotnet` **is** available on this workstation: SDK `10.0.401`. Earlier handoff records stated that
  `dotnet` could not be found on `PATH`; that is no longer true and the reference baseline can now be
  built, tested, and executed for differential validation.
- The reference must never be built in place: an in-tree build writes `bin/`/`obj/` and would break
  the 1,449-file manifest. All reference builds use an isolated copy outside the read-only tree, for
  example `%LOCALAPPDATA%\Temp\opencode\ar-ref`. `tools/verify-baseline.ps1` was re-run after the
  build and reported all 1,449 files unchanged.
- `AssetRipper.slnx` restore succeeds from the `nuget.config` feed `https://nuget.samboy.dev/v3/index.json`
  (reachable, HTTP 200). `AssetRipper.SourceGenerated` 1.4.0 resolves from that feed, so the generated
  class package referenced by the snapshot is available instead of missing. It contains 18,299 types,
  of which 3,439 are concrete generated Unity asset classes such as `ClassID_28.Texture2D_2017`, and
  its 106 MiB XML document carries the per-class, per-version member metadata.
- `dotnet build AssetRipper.slnx -c Release` exits 0 with 32 warnings and 0 errors.
- `dotnet test` over the 8 test projects reports 339 passed and 20 failed. All 20 failures are the CRLF
  checkout artifact recorded as `DIFF-REFERENCE-TEST-NEWLINE-001`; they are not product defects.
- Unity Editor 2022.3.22f1 is installed at `C:\Program Files\Unity 2022.3.22f1`. A fresh batch-mode
  launch against an empty synthetic project exited 1 with `No valid Unity Editor license found`;
  therefore no Unity project import/runtime evidence is currently available. The probe output is
  recorded in `validation-log.md`, and its generated project/log remain under the ignored `target/`.
- `cargo`, `rustc`, and `rustup` are still not on `PATH` and are used from `%USERPROFILE%\.cargo\bin`.
- The active Rust toolchain is `rustc 1.98.1`, `cargo 1.98.1`, `stable-aarch64-pc-windows-msvc`. The Rust
  package declares 1.87 as its minimum because `ruzstd` 0.9.0 requires it; that minimum is untested.
- Current Rust verification is recorded in `validation-log.md`.
- The only cloud write target is the user-authorized cocokoishi/RustAssetRipper main branch. Verify the
  live remote ref through the authorized GitHub connection before each write because the workstation
  origin/main tracking ref can lag. Do not use or touch any other repository.

## Scope findings still open

The initial source inventory identified project boundaries, generated-code projects, file/container modules, import, processing, export, configuration, localization, web UI, and developer tools. The current `docs/feature-parity.md` contains 386 behavior rows: 362 verified, 12 implemented but unverified, 8 in progress, 3 investigated, and 1 externally blocked. The report leaves 24 non-verified rows and keeps the Phase 1 gate open. Full dependency graphs, every version/platform branch, registration path, option/default, GUI workflow, test mapping, and third-party license obligations remain under audit; see the row-level ledger and closure inventory for current status.
