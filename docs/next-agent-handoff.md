# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Latest code revision: `1004581fc6a4fef85b8676e3e2e3e7ab342308c4`; Actions run `39113047642` passed all eight jobs and uploaded the Linux/Windows release binaries.
- Sprite project slice: the source-generated Unity 2019.4.40f1 graph matches Yaml, Native, and Texture2D export for inventory, normalized text/meta/settings outputs, SpriteAtlas editor data, and exact BMP bytes. The follow-up CI fix also makes empty inline Sprite bone data ignore streamed resource bytes, matching `SetBoneGeometry`/`VertexDataBlob.ReadData`.
- Tests: Ubuntu debug 1,941 library + 216 CLI; Windows 1,943 + 216; Ubuntu release 1,941 + 216. Formatting, both Clippy jobs, ledger audit, and public-repository hygiene passed.
- Parity report: 409 rows, 381 verified, 28 non-verified (7 in progress, 2 externally blocked, 19 implemented but unverified). Phase 1 remains open.
- No private input or output was used.

## Primary next gap

Extend the source-generated 2019.4 SpriteAtlas project fixture to multiple Sprite subassets on one Texture2D. Compare ordered SpriteSheet entries and class-ID-keyed `internalIDToNameTable` values in Yaml, Native, and Texture2D modes against the frozen source, preserving deterministic order and normalizing only source-proven random identifiers.

## External limitation

Unity Editor import/runtime validation remains unavailable because this workstation has no valid Unity Editor license; record it as unrun, not passed.
