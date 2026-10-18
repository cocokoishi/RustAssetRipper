# RustAssetRipper hand-off - 2026-10-17

## Latest verified checkpoint

- Code HEAD: 21e3a06e4c46b8f9b0b01a374e1951003972d9fb on main. GitHub Actions run 39103622329 passed all eight jobs.
- Ubuntu workspace tests passed (1,941 library and 215 CLI); Windows passed (1,943 library and 215 CLI); Ubuntu release-mode tests passed (1,941 library and 215 CLI). Both Clippy jobs, formatting, ledger audit, and public repository hygiene passed.
- Closed slice: object_workspace_replays_source_opaque_object_raw_fallbacks exercises all six frozen UnknownObject/UnreadableObject fixture rows through ObjectWorkspace. It checks kind, class/name, CRC-derived names, silent versus Error logs, diagnostic context, and byte-exact raw ranges of 8, 8, 20, 8, 25, and 151 bytes. The Rust truncated-input detail for the Patch retry still differs from the source .NET exception text and is documented.
- Parity report: 409 rows, 380 verified, 29 non-verified, no duplicate IDs or malformed rows. Phase 1 remains open; SER-TT-ABSENT-001 remains in progress.
- No private input or output was used. Full tests ran on Actions, not locally.

## Primary next gap

Broaden Patch retry failure coverage beyond class-108 Light. Inspect the frozen factory dispatch data for another patched class/version where both the original and build-bumped generated reads fail; author a source-pinned case and replay it through ObjectWorkspace, checking the original error context and raw retention. Keep the known Rust/.NET exception-detail difference explicit.

## Operating constraints

Stay on main. Use Actions for full Cargo validation. Before each push, run tools/check_public_repository.py and inspect git diff --name-only origin/main...HEAD. Keep private inputs local.
