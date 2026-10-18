# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest product-code revision: `e5917a4b09958c5d432665cb81975c4505f490db`.
- GitHub Actions run [`39159501038`](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39159501038) passed all eight jobs; Windows/Linux release artifacts were uploaded.
- `OBJ-REF-001` now has a nested-owner differential: a requester inside `source.bundle` resolves duplicate `target.assets` siblings to the first remaining outer-entry match. Rust's PPtr graph, collection dependency table, and project YAML match frozen AssetRipper. See `DIFF-OBJ-REF-NESTED-OWNER-SCOPE-001`.
- The ledger remains 409 rows: 381 verified and 28 non-verified. `OBJ-REF-001` remains in progress; this case does not close the row.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Probe a missing external dependency requested by a SerializedFile inside a nested UnityFS bundle. Compare frozen `Bundle.ResolveCollection`/PPtr/export behavior with Rust's dependency report, unresolved graph edge, and CLI/export result. Pin the requester bundle identity and exact missing name; do not infer parity from output counts.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. The UABEA checkout has no prebuilt executable. Other external blockers remain individually marked in the ledger.
