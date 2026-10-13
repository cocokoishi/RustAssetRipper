# CI validation record: legacy multi-sprite importer metadata

Date: 2026-10-13 UTC  
Commit: `74ef470`  
GitHub Actions run: [38445124918](https://github.com/cocokoishi/RustAssetRipper/actions/runs/38445124918)

## Commands and results

- `cargo test --workspace --locked`: exit 0 on Ubuntu and Windows. Ubuntu reported 1,728 passed and 1 ignored in the library suite, plus 127 CLI tests passed. Windows reported 1,730 passed and 1 ignored in the library suite, plus 127 CLI tests passed.
- `cargo build --workspace --release --locked`: exit 0 on Ubuntu and Windows.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0 on Ubuntu and Windows.
- Changed Rust line formatting check: exit 0.
- Public repository hygiene check: exit 0 for private-path metadata, oversized blobs, and credential scanning.
- Local Cargo tests and builds: not run; the hosted runners performed the resource-intensive checks.

## Input, output, and semantic result

The run tested the repository commit and synthetic importer unit fixtures; it did not read user bundles. The new regression covers two ordered Sprite metadata entries, per-sprite geometry fields, and the legacy `fileIDToRecycleName` mapping using source collection export IDs. No Actions artifact was produced; the run logs are the evidence output.

## Remaining checks

No source-runtime differential or Unity import was run. Multi-sprite layouts containing bone-geometry fields remain explicitly rejected, and atlas association remains outside this increment. The public history text-name cleanup is tracked separately; this run's metadata-only hygiene check does not scan document contents for resource names.
