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
version-routing blocker exists at the TypeTree level. Class-to-file count
gaps are expected on both sides through engine-asset redirects; see
`DIFF-ENGINE-REDIRECT-001` for the mechanism and the future diff rule
(files plus per-asset redirect sets, never count equality).

## Per-sample acceptance state

| ID | Rust replay | Inventory | Reference export | Semantic diff | Verdict |
|----|-------------|-----------|------------------|---------------|---------|
| VRC-AV01 | probe done | done: 2 nodes, 1 file (2022.3.22f1, tree), 2753 objs, 1.5MB resource node; decode 2753/2753 OK | done: 2120 files, class-mirroring counts | pending | parse+decode-OK; export pending |
| VRC-AV02 | probe done | decode 642/642 OK | done: 473 files; 156/156 anim, 28/28 png, 9/9 controller, 15/15 cs, 8/8 mat, 1 ogg; 6 shader vs 8 class-48 (observation) | pending | parse+decode-OK; export pending |
| VRC-AV03 | probe done | done: 3 nodes, 1 file (inner 2018.4.20f1, tree), 260 objs, 16MB resource node; decode 260/260 OK | done: 144 files; 25/25 png, 9/9 anim, 4/4 cs, 1 ogg; 13 mat vs 14 class-21 and 8 shader vs 10 class-48 (observations) | pending | parse+decode-OK; export pending |
| VRC-AV04 | probe done | decode 1952/1952 OK | done: 1191 files; 370/370 anim, 67/67 png, 40/40 mat, 9/9 controller, 14/14 cs, 4/4 ogg; 13 shader vs 19 class-48, plus 4 renderTexture and paired raw types 1818360608/9 (observations) | pending | parse+decode-OK; export pending |
| VRC-AV05 | probe done | decode 216/216 OK | done: 281 files; 69/69 anim, 28/28 ogg, 6/6 png, 8/8 mat, 7/7 controller, 11/11 cs, inner 2022.3.22f1; 2 shader vs 3 class-48 (observation) | pending | parse+decode-OK; export pending |
| VRC-AV06 | probe done | decode 3394/3394 OK | done: 2067 files; 623/623 anim, 26/26 mat, 9/9 controller, 18/18 cs, 3/3 ogg, inner 2022.3.22f1; 248 png vs 249 class-28, 8 shader vs 12 class-48 (observations); 992 class-114 noted | pending | parse+decode-OK; export pending |
| VRC-AV07 | probe done | decode 1373/1373 OK | done: 1205 files; 499/499 anim, 41/41 png, 9/9 mat, 8/8 controller, 11/11 cs, inner 2022.3.22f1; 7 shader vs 8 class-48 (observation); 4 blendtree + 1 state sub-docs noted | pending | parse+decode-OK; export pending |
| VRC-AV08 | probe done | decode 1364/1364 OK | done: 712 files; 179/179 anim, 52/52 png, 9/9 controller, 13/13 cs, 9/9 ogg; 27 mat vs 28 class-21, 8 shader vs 12 class-48 (observations); 40 meshes and Parent/Rotation constraint families noted | pending | parse+decode-OK; export pending |
| VRC-AV09 | probe done | decode 3827/3827 OK | done: 2269 files; 645/645 anim, 44/44 mat, 9/9 controller, 17/17 cs, 36/36 ogg, inner 2022.3.22f1; 242 png vs 243 class-28, 10 shader vs 14 class-48 (observations) | pending | parse+decode-OK; export pending |
| VRC-AV10 | probe done | decode 6529/6529 OK | done: 3313 files; 848/848 anim, 231/231 png, 97/97 mat, 9/9 controller, 23/23 cs, 12/12 ogg, inner 2022.3.22f1; 16 shader vs 24 class-48 (observation); 98 state + 72 blendtree sub-docs noted | pending | parse+decode-OK; export pending |
| VRC-AV11 | probe done | decode 6461/6461 OK | done: 3211 files; 705/705 anim, 20/20 cs, 9/9 controller, 24/24 ogg, inner 2022.3.22f1; 439 png vs 438 class-28, 128 mat vs 129 class-21, 27 shader vs 37 class-48 (observations); 41 blendtree + 13 state sub-docs noted | pending | parse+decode-OK; export pending |
| VRC-W01 | probe done | done: 3 nodes, 2 files (2022.3.22f1, trees), 1008 objs, 243KB resource node; decode 1008/1008 OK | done: 478 files incl 1 unity, 1 ogg, 102 png vs 104 class-28 (observation, not a verdict) | pending | parse+decode-OK; export pending |
| VRC-W02 | probe done | done: fmt-7 bundle, 3 nodes, 2 files (2019.4.31f1, trees), 46 objs, 12MB resource node; decode 46/46 OK | done: 37 files incl 1 unity, 1 ogg, 2/2 png, 5/5 cs | pending | parse+decode-OK; export pending |
| VRC-W03 | probe done | decode 2274/2274 OK | done: 1080 files; 7/7 anim, 44/44 cs, 3/3 controller, 4/4 ogg, 1 unity, 37 prefab; 43 mat vs 46 class-21, 43 shader vs 63 class-48, 169 texture files vs 173 class-28 (observations); class-213 x91 noted | pending | parse+decode-OK; export pending |
| VRC-W04 | probe done | decode 15871/15871 OK | done: 787 files; 30/30 anim, 6/6 controller, 38/38 cs, 1 unity; 107 mat vs 111 class-21, 101 texture files vs 108 class-28, 9 shader vs 16 class-48 (observations); big class-222/224 families and engine class LightingSettings noted | pending | parse+decode-OK; export pending |
| VRC-W05 | probe done | decode 1441/1441 OK | done: 637 files; 2/2 anim, 1/1 controller, 27/27 cs, 3/3 ogg, 1 unity, 1 renderTexture; 27 mat vs 29 class-21, 104 texture files vs 103 class-28, 36 shader vs 43 class-48 (observations); recurring engine classes (Lighting/Parent/Position/Rotation constraints) identified | pending | parse+decode-OK; export pending |
| VRC-W06 | probe done | decode 426664/426664 OK | done: 9679 files; 169/169 anim, 708/708 png, 60/60 controller, 53/53 cs, 28/28 ogg, 1 unity, 1 overrideController; 888 mat vs 890 class-21, 139 shader vs 169 class-48 (observations); 103K GameObjects, one class-29 Crunch texture, SpriteAtlas/Aim/LookAt engine classes identified | pending | parse+decode-OK; export pending |
| VRC-W07 | probe done | decode 42222/42222 OK | done: 3284 files; 79/79 anim, 27/27 controller, 55/55 cs, 27/27 ogg, 1 unity, 9 ttf; 332 mat vs 343 class-21, 383 texture files vs 386 class-28, 72 shader vs 86 class-48 (observations; largest material gap so far) | pending | parse+decode-OK; export pending |
| VRC-W08 | probe done | decode 1993/1993 OK | done: 526 files; 4/4 anim, 30/30 mat, 4/4 controller, 20/20 cs, 2/2 ogg, 1 unity; 131 texture files vs 121 class-28 (overage: cubemap/sprite-derived PNGs suspected amid 10 class-89 and 34 sprite pairs), 34 shader vs 40 class-48 (observations) | pending | parse+decode-OK; export pending |
| VRC-W09 | probe done | decode 10587/10587 OK | done: 2099 files; 58/58 anim, 13/13 controller, 67/67 cs, 26/26 ogg, 1 unity, 1 terrainlayer, 1 overrideController; 205 mat vs 214 class-21, 291 texture files vs 300 class-28, 72 shader vs 103 class-48 (observations; largest shader gap) | pending | parse+decode-OK; export pending |
| VRC-W10 | probe done (alias cross-walk confirmed by size+version, see notes) | decode 72365/72365 OK | done: 2182 files; 1/1 anim, 1/1 controller, 29/29 cs, 1/1 ogg, 1 unity, 1 playable, inner 2022.3.6f1; 86 mat vs 91 class-21, 247 texture files vs 249 class-28, 44 shader vs 52 class-48 (observations) | pending | parse+decode-OK; export pending |
| VRC-W11 | probe done | decode 33974/33974 OK | done: 2207 files; 93/93 anim, 8/8 controller, 39/39 cs, 1/1 ogg, 1 unity, 1 flare, 1 playable; 141 mat vs 145 class-21, 260 png vs 257 class-28, 53 shader vs 59 class-48 (observations) | pending | parse+decode-OK; export pending |

Acceptance order: smallest first (VRC-AV01, VRC-W01), then by size.
Reference outputs and Rust outputs stay in gitignored local scratch only.

## Comparison discipline (established 2026-10-14)

Reference exports regenerate random GUIDs and fresh timestamps on every run
(verified: two exports of one avatar share the file tree exactly but differ
in all 1057 sidecars' guid/timeCreated lines, plus 32 YAML assets carrying
those GUIDs; binary assets are byte-identical). Consequences: never
byte-compare exports across runs, not even reference-vs-reference; Rust-vs-ref
diffs must pair assets by stable identity (relative path plus local fileID)
and compare resolved reference targets rather than raw GUIDs; timeCreated is
always normalized. The Rust implementation's deterministic per-identity GUIDs
stay an asset for reproducibility, not a mismatch.

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
  engine class later identified as ParentConstraint. Zero entry/parse
  errors.
- VRC-W02: oldest container (fmt 7) parses cleanly; small world graph with
  scene-level classes (20/104/108/150/157/196 family). Zero entry/parse
  errors.

No container/metadata blocker surfaced in the first four. Next: object
decode triage on VRC-AV01 (class-74/114 heavy), then reference-side runs.

## Decode triage (22 samples, 637216/637216 OK)

Extended the probe with per-object `decode_object_payload` and swept the
whole corpus: all 11 avatars (28771 objects) and all 11 worlds (608445
objects) decode with zero failures, covering dense class-74/114 spreads,
fmt-7 containers, and 2018/2019/2022/6000 inner versions. The recurring
large raw-type IDs are identified engine classes (LightingSettings_2020 =
850595691, ParentConstraint_2018 = 1773428102, PositionConstraint_2018 =
1818360608, RotationConstraint_2018 = 1818360609) via the dispatch fixture,
so no unknown-type observation remains. Scale note: VRC-W06 carries 421913
SerializedFile (export-scale awareness, not a failure). Caveats:
stale-rlib success paths match HEAD for these APIs, but decode-OK is not
semantic-OK (values unvalidated); factory/workspace/export untouched.
Next: reference-side runs, then export replay once a binary strategy exists.

## Factory triage (22 samples, 637216/637216 OK)

Extended the probe again with per-object `materialize_object` through the
embedded reference layouts (2022.3 PC platform facts): every decoded object
also constructs, 637216/637216 with zero failures, matching decode counts
exactly. Construction-OK is not export-OK (workspace/collections/export
untouched) and values remain unvalidated. Next: reference-side runs.

## Open tooling gaps

- Runnable Rust binary from HEAD: prior sessions ran local release builds
  on this machine repeatedly (Oct 11-13 replays) and disk has since
  recovered, so a background `cargo build --release --locked --bin
  assetripper-rs` was started in-session; gate replays unblock when it
  lands. The CI-artifact alternative stays available if local builds prove
  too heavy (needs owner approval for the workflow change).
- Reference export driver: done (scratch headless driver, 22/22 baselines).

## Corpus reconciliation notes (anonymous)

- One ID is confirmed against the prior program's alias set by exact byte
  size plus inner Unity version: VRC-W10 matches the world alias whose
  input is 73,242,523 bytes at Unity 2022.3.6f1. All other cross-walks
  stay unconfirmed: prior object counts differ from probe counts by small
  deltas on near candidates (e.g. two cases differ by exactly 176 objects),
  which may reflect counting conventions, sample updates, or different
  files. No ID is asserted to an alias without owner reconciliation; the
  truncated hashes in this ledger bridge any future mapping.
- Pre-existing repo docs (Oct 11-13 replay entries) contain probable
  original asset/object names inside evidence text. Adjudication against
  the private mapping plus any redaction needs owner decision; shared
  history is never rewritten unilaterally. No new exposures were added
  from the current acceptance rounds.
