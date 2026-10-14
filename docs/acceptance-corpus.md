# Acceptance corpus (anonymous ledger)

Real VRCA/VRCW acceptance inputs. Privacy rules: this file, commits, tests,
logs, fixtures, docs, and CI output must never contain original file names,
world names, author/user identity, absolute paths, server info, private
naming, or any content that reveals a sample's source. Samples are tracked by
anonymous IDs plus desensitized hashes (sha256 truncated to 16 hex) and sizes.
Real inputs are local authorized acceptance data only: never commit the input
files or recognizable content. Synthetic tests exist to minimize, reproduce,
and lock regressions for issues real samples expose; they never substitute
for real-sample acceptance.

## Corpus (22 files, all UnityFS containers)

| ID | Kind | Size (bytes) | sha256[:16] | Bundle fmt | Unity rev | TPK |
|----|------|--------------|-------------|------------|-----------|-----|
| VRC-AV01 | avatar | 2322676 | A11A5DADB89432FE | 8 | 2022.3.22f1 | yes |
| VRC-AV02 | avatar | 4801621 | 223EDD9568EBA068 | 8 | 2022.3.22f1 | yes |
| VRC-AV03 | avatar | 6244530 | A1640F0DE2A24CB5 | 8 | 6000.0.60f1-DWR | yes |
| VRC-AV04 | avatar | 6718453 | 4BB36B3FD7622630 | 8 | 2022.3.22f1 | yes |
| VRC-AV05 | avatar | 6996530 | FC9B10CE782BBBFA | 8 | 6000.0.60f1-DWR | yes |
| VRC-AV06 | avatar | 17009862 | AEA5FB732668E657 | 8 | 2022.3.22f1 | yes |
| VRC-AV07 | avatar | 17455683 | 7C918C89F8E5C308 | 8 | 6000.0.60f1-DWR | yes |
| VRC-AV08 | avatar | 39614078 | D923D39E1A67D9CF | 8 | 6000.0.60f1-DWR | yes |
| VRC-AV09 | avatar | 51539303 | 8A1EF30C62DC3D9C | 8 | 6000.0.60f1-DWR | yes |
| VRC-AV10 | avatar | 58150845 | EB0113F5B26D2649 | 8 | 6000.0.60f1-DWR | yes |
| VRC-AV11 | avatar | 98305350 | CB65E179F0BEE693 | 8 | 2022.3.22f1 | yes |
| VRC-W01 | world | 13127654 | 8F4A91DF6A9E4977 | 8 | 2022.3.22f1 | yes |
| VRC-W02 | world | 18457199 | 8BCADAB5F67A7E0B | 7 | 2019.4.31f1 | yes |
| VRC-W03 | world | 20727080 | FC73FFBDBAC14421 | 8 | 2022.3.22f1 | yes |
| VRC-W04 | world | 20946066 | 907776C74127D83B | 8 | 2022.3.22f1 | yes |
| VRC-W05 | world | 31949256 | C856441ABA99F65E | 8 | 2022.3.22f1 | yes |
| VRC-W06 | world | 33353771 | DE64397F93AF1692 | 8 | 2022.3.22f1 | yes |
| VRC-W07 | world | 34102275 | 0F893169C2CC2922 | 8 | 2022.3.22f1 | yes |
| VRC-W08 | world | 34294280 | 5187F8FECFB0AC27 | 8 | 2022.3.22f1 | yes |
| VRC-W09 | world | 34888505 | ABE4BE0991480484 | 8 | 2022.3.22f1 | yes |
| VRC-W10 | world | 73242523 | 100338C82E934B51 | 8 | 2022.3.6f1 | yes |
| VRC-W11 | world | 286877903 | C4407DC3963AE45C | 8 | 2022.3.22f1 | yes |

IDs are stable: `VRC-AV##`/`VRC-W##` by (kind, size). Container facts come
from header-only byte reads (magic, format version, version strings); no
sample content was opened for this ledger. All four Unity revisions
(2019.4.31f1, 2022.3.6f1, 2022.3.22f1, 6000.0.60f1) have TPK rows, so no
version-routing blocker exists at the TypeTree level.

## Per-sample acceptance state

| ID | Rust replay | Inventory | Reference export | Semantic diff | Verdict |
|----|-------------|-----------|------------------|---------------|---------|
| VRC-AV01 | probe done | done: 2 nodes, 1 file (2022.3.22f1, tree), 2753 objs, 1.5MB resource node; decode 2753/2753 OK | pending | pending | parse+decode-OK; export pending |
| VRC-AV02 | pending | pending | pending | pending | — |
| VRC-AV03 | probe done | done: 3 nodes, 1 file (inner 2018.4.20f1, tree), 260 objs, 16MB resource node; decode 260/260 OK | pending | pending | parse+decode-OK; export pending |
| VRC-AV04 | pending | pending | pending | pending | — |
| VRC-AV05 | pending | pending | pending | pending | — |
| VRC-AV06 | pending | pending | pending | pending | — |
| VRC-AV07 | pending | pending | pending | pending | — |
| VRC-AV08 | pending | pending | pending | pending | — |
| VRC-AV09 | pending | pending | pending | pending | — |
| VRC-AV10 | pending | pending | pending | pending | — |
| VRC-AV11 | pending | pending | pending | pending | — |
| VRC-W01 | probe done | done: 3 nodes, 2 files (2022.3.22f1, trees), 1008 objs, 243KB resource node; decode 1008/1008 OK | pending | pending | parse+decode-OK; export pending |
| VRC-W02 | probe done | done: fmt-7 bundle, 3 nodes, 2 files (2019.4.31f1, trees), 46 objs, 12MB resource node; decode 46/46 OK | pending | pending | parse+decode-OK; export pending |
| VRC-W03 | pending | pending | pending | pending | — |
| VRC-W04 | pending | pending | pending | pending | — |
| VRC-W05 | pending | pending | pending | pending | — |
| VRC-W06 | pending | pending | pending | pending | — |
| VRC-W07 | pending | pending | pending | pending | — |
| VRC-W08 | pending | pending | pending | pending | — |
| VRC-W09 | pending | pending | pending | pending | — |
| VRC-W10 | pending | pending | pending | pending | — |
| VRC-W11 | pending | pending | pending | pending | — |

Acceptance order: smallest first (VRC-AV01, VRC-W01), then by size.
Reference outputs and Rust outputs stay in gitignored local scratch only.

## First triage notes (anonymous, structural only)

Probed with an rlib-linked inventory driver (Oct-13 prebuilt rlib: parse,
entry reads, metadata histograms only; no object decode, no conversion).
Findings are triage-grade and must be rechecked against HEAD before any
become blockers.

- VRC-AV01: dense avatar file — 100 GameObjects/Transforms with 906
  class-74, 1520 class-114, 11 class-115, 104 class-28, single class-21/43.
  Zero entry/parse errors.
- VRC-W01: world file — 172 GameObjects across 2 SerializedFiles, broad
  renderer/collider/audio/lighting class spread including one class-83.
  Zero entry/parse errors.
- VRC-AV03: inner SerializedFile reports 2018.4.20f1 while the bundle tag
  reads 6000.0.60f1 (build Editor vs content runtime; routing uses inner
  versions). Carries a 16MB resource node, one class-89 Cubemap, and one
  unknown raw type 1773428102 (parsed without error; identity open).
  Zero entry/parse errors.
- VRC-W02: oldest container (fmt 7) parses cleanly; small world graph with
  scene-level classes (20/104/108/150/157/196 family). Zero entry/parse
  errors.

No container/metadata blocker surfaced in the first four. Next: object
decode triage on VRC-AV01 (class-74/114 heavy), then reference-side runs.

## Decode triage (4 samples, 4067/4067 OK)

Extended the probe with per-object `decode_object_payload`: VRC-AV01
2753/2753, VRC-W01 231+777=1008/1008, VRC-AV03 260/260 (including the
unknown raw type 1773428102), VRC-W02 15+31=46/46. Zero decode failures.
Caveats: stale-rlib success paths match HEAD for these APIs, but decode-OK
is not semantic-OK (values unvalidated); factory/workspace/export untouched.
Next: reference-side runs, then export replay once a binary strategy exists.

## Open tooling gaps

- Runnable Rust binary from HEAD: no CI artifact upload exists (workflow is
  read-only by design), local cargo builds are ruled out by the machine
  performance constraint, and no prebuilt binary exists. Triage can proceed
  through rlib-linked probes with a documented staleness caveat; gate-level
  replays need a binary strategy (deferred decision, no permission changes
  without authorization).
- Reference export driver: no headless frozen-source export driver exists
  yet; comparison runs need one (deferred build).
