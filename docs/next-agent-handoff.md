# RustAssetRipper hand-off - 2026-10-18

## Latest verified checkpoint

- Code revision: 85fa01b10199591b2faa6e62ac97ae7605a53be2.
- GitHub Actions run [39147814501](https://github.com/cocokoishi/RustAssetRipper/actions/runs/39147814501) passed all eight jobs and uploaded Windows/Linux release artifacts.
- Stored EngineResourceData redirects now cover TextAsset, Texture2D, AudioClip, Cubemap, Material, Shader, Mesh, and Sprite. Sprite routing first checks its texture-group cache key; a texture hit suppresses unmatched Sprite members.
- The ledger remains 409 rows: 381 verified and 28 non-verified. Phase 1 remains open. PRODUCT-CONFIG-001 stays unverified because generic DataSet serializers, other stored resource kinds, and native file-dialog input remain open.
- Preserve the existing untracked tools/__pycache__/ directory.

## Primary next gap

Fix the normal class-43 Mesh YAML omissions already observed on the public Unity 5.6 fixture: m_ObjectHideFlags, m_PrefabParentObject, m_PrefabInternal, and m_MeshOptimized. Trace each field through the frozen Mesh exporter and generated interfaces, then implement the general source rule and compare the complete normalized Mesh YAML. Keep this separate from the stored Mesh cache redirect.

## External limitation

Unity Editor import/runtime validation remains unrun because this workstation has no valid Editor license. Other external blockers remain individually marked in the ledger.
