# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest product-code revision: `af9943ad08ce861ea1587627269adec2b6560e43`.
- GitHub Actions run [`39162781437`](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39162781437) passed all eight jobs and uploaded Windows/Linux release artifacts.
- `OBJ-REF-001` now has a source-backed nested missing-dependency case: the graph retains `MissingDependency { external_index: 0, path_id: 900 }`, and CI-artifact `export-project` output matches the frozen root YAML byte-for-byte. See `DIFF-OBJ-REF-NESTED-MISSING-DEPENDENCY-001`.
- The ledger remains 409 rows: 381 verified and 28 non-verified. `OBJ-REF-001` remains in progress; this case does not close the row.
- Preserve the existing untracked `tools/__pycache__/` directory.

## Primary next gap

Exercise a `PPtr` inside an array-valued managed-reference entry. The exporter now maps concrete `SerializedValue` paths such as `[0]` to TypeTree wildcard paths such as `[*]`, but the current missing-reference fixture only exercises a scalar `m_Asset`. Build a public synthetic input, compare the frozen source graph and full project YAML with the CI-built Rust `export-project` artifact, and keep the parent row open until broader identity coverage is complete.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. The UABEA checkout has no prebuilt executable. Other external blockers remain individually marked in the ledger.
