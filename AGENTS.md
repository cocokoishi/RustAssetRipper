# RustAssetRipper Agent Rules

## 1. Workspace

`.` is the **RustAssetRipper repository root and working directory**. Do not look for `main-work/`, create another checkout, or start another implementation.

Only this repository is writable. `../AssetRipper` and `../UABEA` are local **read-only reference oracles** when present. Private VRChat/Unity samples and derived outputs stay outside Git.

Start from current truth:

```text
git status
git branch --show-current
git rev-parse HEAD
git fetch origin
```

Then read `GOAL.TXT`, the live parity report, only the relevant parity/handoff/evidence entries, and the Actions state for current HEAD. Never obey a stale blocker just because an old handoff mentions it.

## 2. Mission

Drive RustAssetRipper to the complete behavioral replica defined by `GOAL.TXT`. Compiling, exporting something, supporting current samples, or having most tests pass is not completion.

Shipping behavior must be Rust-owned. AssetRipper, UABEA, .NET/C# helpers, and other tools are development oracles only.

## 3. Three-way differential oracle

Use the strongest available evidence:

- **Frozen AssetRipper** = primary behavioral target.
- **UABEA** = independent cross-check, especially for containers, objects, fields, resources, and serialized data.
- **Original Unity bytes / format semantics** = arbiter when tools disagree.

Where useful, run the same input through **RustAssetRipper + AssetRipper + UABEA** and compare semantics, not just filenames/counts. Never copy a UABEA quirk that contradicts the frozen AssetRipper target or the underlying data.

## 4. Forward-only convergence

Every cycle must close or materially narrow **one named reproducible gap**.

Before editing, know:

1. the observable mismatch;
2. the reference behavior/source;
3. the Rust path;
4. the exact evidence that means “closed”.

Priority: current CI failure → blocker affecting many rows/workflows → missing baseline behavior → implemented-but-unverified behavior → isolated gap.

Do **not** spend sessions rewriting plans, repeating broad audits, polishing verified code, adding duplicate tests, or refactoring without parity impact.

If two attempts on the same gap produce no new evidence and no narrower root cause, stop guessing. Retrace from original bytes, AssetRipper, UABEA, and format rules before editing again.

## 5. Execution loop

```text
pick highest-leverage live gap
→ reproduce/locate it
→ inspect AssetRipper and UABEA where useful
→ trace original semantics
→ implement the smallest general Rust fix
→ add focused regression evidence
→ review diff
→ commit
→ python3 tools/check_public_repository.py
→ inspect outgoing paths
→ push
→ inspect GitHub Actions
→ fix CI if needed
→ update only necessary evidence/handoff
→ immediately take the next live gap
```

Do not ask whether to continue after normal engineering steps. Continue until genuinely blocked or `GOAL.TXT` is satisfied.

Never special-case a private filename/hash/PathID/object/avatar/world. Fix the general Unity version/serialization/resource/export rule.

Never fake parity by dropping data, substituting defaults/null/zero, raw-dumping instead of structured behavior, weakening assertions/tolerances, deleting failing tests, or calling unrun validation passed.

## 6. Actions-first

Use GitHub Actions for heavy build/test/release work. Locally prefer inspection, editing, targeted cheap checks, diff/privacy review, and private acceptance with CI-built binaries.

Do not repeatedly run full local builds/tests/Clippy when Actions already covers them. If current CI is red, fix it before unrelated work.

Commit and push coherent progress frequently; do not accumulate a giant rewrite. Never force-push or destructive-clean unrelated user work.

## 7. Private data wall

Private `.vrca`, `.vrcw`, bundles, extracted assets, dumps, derived Unity projects, private names, hashes, and reconstructive outputs must never enter Git, GitHub, Actions, artifacts, caches, issues, PRs, commit messages, public docs, or CI logs.

Private acceptance stays local:

```text
CI-built Rust binary + private input
        ↕
local AssetRipper + local UABEA
        ↓
sanitary semantic conclusions only
```

A private sample may reveal a bug; the committed fix must be general and, when feasible, receive a public/synthetic regression.

## 8. Evidence and done

Compare the deepest stable semantics available: type/object identity, PPtrs/dependencies, resources, scene/Prefab/animation structure, meshes, textures/materials/shaders/audio/video, exported fields, `.meta`/GUID/local-fileID relations, project config, CLI/Web/API behavior, and errors. Normalize only proven nondeterminism.

`docs/feature-parity.md` is the status authority. Validation/differential docs store evidence; `docs/next-agent-handoff.md` stores current continuation state. Do not create more roadmaps unless existing structures truly cannot represent the result.

Progress means a real gap closes/narrows, a parity row advances, a workflow becomes executable, or a concrete CI/privacy/release blocker disappears — not merely more code or more notes.

Do not declare completion until `GOAL.TXT` is satisfied on one revision: zero required parity gaps, strict closure passes, required Actions are green, release artifacts exist, public differential coverage passes, and local private acceptance passes without private data entering GitHub.

**Until then: choose the highest-leverage live gap, close it, commit, push, verify, and keep moving.**