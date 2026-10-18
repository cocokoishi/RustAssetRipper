# RustAssetRipper Agent Rules

## Workspace

The current working directory `.` **is the RustAssetRipper repository root**. Do not look for or create `main-work/`, another checkout, or another Rust implementation.

Only this repository is a write target. Local reference directories such as `../AssetRipper` and `../UABEA` are **read-only development oracles**. Private VRChat/Unity inputs and their derived outputs stay local and outside Git.

At session start, inspect only current truth:

```text
git status
git branch --show-current
git rev-parse HEAD
git fetch origin
```

Then read `GOAL.TXT`, the live parity report, the relevant rows of `docs/feature-parity.md`, the latest relevant handoff/evidence, and the Actions state for the current pushed revision. Do not blindly follow stale handoff prose.

## Mission

Drive RustAssetRipper to the complete behavioral-replica goal in `GOAL.TXT`. Compilation, broad parsing, successful export, many passing tests, or support for current samples are **progress, not completion**.

The shipping implementation must be Rust-owned. AssetRipper, UABEA, .NET/C# helpers, and other rippers may guide development but must never be required at runtime.

## Oracle model

Use all available evidence instead of treating one tool as infallible:

- **Frozen AssetRipper** is the primary behavioral target.
- **UABEA** is an independent differential oracle and should be used whenever it can expose parsing, object, field, resource, or export discrepancies.
- **Original Unity bytes/format semantics** arbitrate disagreements between tools.

For useful cases, compare the same input through **RustAssetRipper vs AssetRipper vs UABEA**, not Rust vs AssetRipper only. A match between two tools is evidence, not proof; investigate meaningful disagreements before changing Rust.

Never change Rust merely to imitate a UABEA quirk that conflicts with the frozen AssetRipper baseline or the underlying format.

## Forward-only convergence

Every engineering cycle must close or materially narrow **one named, reproducible gap**.

Before editing, know four things:

1. what observable behavior is wrong or missing;
2. where the frozen reference behavior comes from;
3. where the Rust path is;
4. what evidence will prove the gap closed.

Choose work in this order:

1. CI failure on current HEAD;
2. blocker affecting multiple parity rows/workflows;
3. real missing baseline behavior;
4. implemented-but-unverified behavior that can now be proven;
5. isolated low-impact gaps.

Do not spend a session rewriting plans, repeating broad audits, polishing verified areas, adding duplicate tests, or refactoring without parity impact.

**One strong vertical slice beats ten partial edits.**

If two consecutive attempts on the same gap produce neither new evidence nor a narrower root cause, stop patching blindly. Retrace from original bytes / AssetRipper / UABEA / format rules, then make the next change.

## Implementation loop

For the selected gap:

```text
locate/reproduce mismatch
→ inspect AssetRipper + UABEA where useful
→ trace original data semantics
→ implement the smallest general Rust fix
→ add focused regression evidence
→ inspect diff
→ commit
→ run privacy guard / inspect outgoing paths
→ push
→ inspect GitHub Actions
→ fix CI if needed
→ record only useful evidence
→ immediately take the next live gap
```

Do not wait for user confirmation after normal commits. Continue until genuinely blocked or the full goal is reached.

Fix general Unity/version/serialization/export rules. Never special-case private filenames, hashes, PathIDs, object names, avatars, worlds, or one output directory.

Never fake parity by dropping fields/resources, substituting null/default/zero, returning success from an incomplete path, raw-dumping data that requires structured export, weakening assertions, widening tolerances after seeing a failure, deleting failing tests, or calling unrun validation passed.

## Actions-first

Use GitHub Actions for heavy build/test/release work by default. Locally prefer source inspection, editing, targeted lightweight checks, formatting, diff review, privacy checks, and private acceptance with CI-built binaries.

Do not repeatedly run full local `cargo build`, `cargo test`, release builds, Clippy, or large integration suites when Actions already covers them. If CI is red because of the current change, repair it before starting unrelated work.

Make coherent, bisectable commits frequently. Do not accumulate a huge rewrite.

Before commit/push, review explicit paths and run the repository privacy guard. Never force-push as normal workflow, never destructive-clean user work, and never stage unrelated files.

## Private VRChat / Unity data

Private `.vrca`, `.vrcw`, bundles, extracted assets, dumps, derived Unity projects, proprietary resources, private names, and reconstructive outputs must never enter Git, GitHub, Actions, artifacts, caches, issues, PRs, commit messages, public docs, or CI logs.

Private differential acceptance is local-only:

```text
CI-built Rust binary
+ local private input
+ local frozen AssetRipper
+ local UABEA
→ semantic comparison
→ sanitized conclusions only
```

Use anonymous labels in public evidence. Do not publish private filenames, absolute paths, content hashes, object names, or raw payloads.

A private sample may reveal a bug; the fix must become a general implementation rule and, when feasible, a public/synthetic regression.

## Evidence

Compare semantics, not appearance or counts. Depending on the gap, compare containers, SerializedFiles, TypeTrees, ClassID/PathID identity, PPtrs, dependencies, resources, scenes/Prefabs, animation, meshes, textures/materials/shaders/audio/video, exported fields, `.meta`/GUID/local-fileID relations, project config, CLI, Web/API, and error behavior.

Normalize only values proven nondeterministic or semantically irrelevant.

Keep documentation short and useful. `docs/feature-parity.md` is the authoritative status ledger; validation/differential docs store evidence; `docs/next-agent-handoff.md` stores only current continuation truth. Do not create new roadmaps or session diaries when existing structures suffice.

A session is progress only if a parity row advances, required behavior lands, a real mismatch disappears, a product flow becomes executable, CI/privacy/release blocking is removed, or a root cause becomes materially narrower.

## Done

Do not call the project complete until `GOAL.TXT` is satisfied at one revision: no required parity gaps remain, strict closure checks pass, required Actions are green, release artifacts exist, public differential coverage passes, and authorized private acceptance is clean locally without private data entering GitHub.

Until then: **take the highest-leverage live gap, close it, commit, push, verify, and move forward.**