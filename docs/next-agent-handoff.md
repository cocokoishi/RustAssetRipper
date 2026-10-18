# RustAssetRipper hand-off - 2026-10-17

## Latest verified checkpoint

- HEAD: 95be06c3f3d0b08e14b12ff21b0a170b1e6df54b on main. GitHub Actions run 39102098259 passed all eight jobs.
- Ubuntu workspace tests passed (1,941 library and 215 CLI); Windows passed (1,943 library and 215 CLI); Ubuntu release-mode tests passed (1,941 library and 215 CLI). Both Clippy jobs, formatting, ledger audit, and public repository hygiene passed.
- Closed slice: object_workspace_replays_source_opaque_object_raw_fallbacks replays the frozen unknown_class, unknown_no_tree, unreadable_version, and unreadable_no_tree factory rows through ObjectWorkspace. It checks source fallback kind, class/name, CRC-derived name, silent diagnostics, incomplete graph state, and retained raw ranges of 8, 8, 20, and 8 bytes.
- Parity report: 409 rows, 380 verified, 29 non-verified, no duplicate IDs or malformed rows. Phase 1 remains open; SER-TT-ABSENT-001 remains in progress.
- No private input or output was used. Full tests ran on Actions, not locally.

## Primary next gap

Source-compare the supported-version textasset_bad_tail UnreadableObject fallback through ObjectWorkspace against the frozen SF_READ row and GameAssetFactory.ReadAsset. Check the error classification, source name, retained 25-byte raw payload, and diagnostic semantics; account for the path-context portion of the message before asserting equality.

## Operating constraints

Stay on main. Use Actions for full Cargo validation. Before each push, run tools/check_public_repository.py and inspect git diff --name-only origin/main...HEAD. Keep private inputs local.
