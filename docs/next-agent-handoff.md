# RustAssetRipper hand-off - 2026-10-18

## Latest validated checkpoint

- Product-code commit `160121b1f46d64504f20b0051f160972f9fa3004`; Actions run [39183782973](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39183782973) passed all eight jobs. The unresolved external PPtr regression source-matches three cases: negative/out-of-range external file IDs and a missing PathID in a present dependency.
- Ledger: 410 rows, 381 verified, 29 non-verified, duplicates=0, malformed=0, gate=OPEN. `OBJ-REF-001` remains in progress. Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Source-test first-match ordering for duplicate same-named SerializedFiles within one bundle. Build a small public synthetic UnityFS fixture with a requester and two `target.assets` entries in a single bundle; run frozen `ExportHandler`, then compare Rust's chosen target and exported reference. The source scans a bundle's direct collections in stored order, but no runtime differential currently pins that same-bundle case.

## External limitation

Unity Editor import/runtime validation remains unavailable because this workstation has no valid Editor license. No prebuilt UABEA executable is available; its source/classdata remain read-only references.
