# Coverage: every client system

One line per system: what is read, what is open, and where the findings live.
"Read" means a path or value with an address in `Overwatch.exe`, or a value read
from the install — not a full reimplementation. The work queue is
[`open-questions.md`](open-questions.md).

## Gameplay

| # | system | state | where |
|---|---|---|---|
| 1 | Character mover (data) | per-hero slots, jump struct and component schema read from the install | `formats/character-controller.md` |
| 2 | Mover update | component, advance, invoker, state-modifier evaluator and physics command read; the caller that computes the wish/scales is **open** | `formats/character-controller.md` |
| 3 | Domino physics | tick driver, substep, constraint dispatch and integration functions located; runtime schema and arithmetic must be re-read in the current image | `formats/domino.md` |
| 4 | Weapons (data) | loadout chain, weapon graphs, volley literals and projectile fields read from the install | `formats/weapons.md` |
| 5 | Statescript | graph data, node records, executor drive and dispatch read; the node implementation behind the type index is **open** | `formats/weapons.md` |
| 6 | Damage and health | health/armour/shield pools read; damage application, armour mitigation and shield regeneration are **open** | `formats/weapons.md` |
| 7 | Camera | config surface, FOV, near/far, projection and FP pose path read; the per-frame look/interpolation consumer is **open** | `formats/camera.md` |
| 8 | Input | mouse scale, aim assist and force-input mode read; the per-frame handler handoff is **open** | `formats/camera.md` |
| 9 | Animation format | clip header, packed tracks, runtime record and sampler read | `formats/animation.md` |
| 10 | Animation runtime | blend tree, FP items and hardpoint attachment read; the Pose2d VM and reference retarget are **open** | `formats/animation.md` |
| 11 | View model | FP component and consumer located; the per-frame offset/scalar application is **open** | `formats/animation.md`, `formats/camera.md` |
| 12 | Collision | map geometry is visual only; the character step/ledge/slope path is **open** | `formats/maps.md`, `formats/character-controller.md` |
| 13 | Bots and abilities | test geometry only; no client AI read | `formats/weapons.md` |

**The client is built.** `ow-client` and `ow-scene` are workspace members and
the default launch draws the resolved practice range; `ow-sim` is parked
reference code. The viewer makes no gameplay claim.

## Data and content

| # | system | state | where |
|---|---|---|---|
| 14 | CASC container | build info, configs, keyring, bucket index (`DynamicIndexHash` and `DynamicIndexKeyHash` verified), encoding header (`EncodingHeaderParse` verified), BLTE (`Salsa20StateInit` and `Salsa20Xor` verified) and the manifests, including the build-154504 encrypted derivation (`CmfKey154504`, `CmfIv154504`) | `formats/casc.md` |
| 15 | STU | v1/v2 layouts, fields, strings, asset refs and the typed layer read | `formats/stu.md` |
| 16 | Entities and health | hero → gameplay entity → schema entries read; pool values read from the install, not client-verified (`ow-tank-1to1.md`) | `formats/stu.md` |
| 17 | Models, skeletons, chunks | `mskl`, `MRNM`, `MSTU`, `MDLC` and hardpoints read | `formats/stu.md`, `formats/animation.md` |
| 18 | Maps | chunk kinds, placeables, model/look joins, the map-to-chunk link, the bundle asset path and the map asset's stored chunk fields read (type-`0x09F` `STUMapHeader` → variation → type-`0x002` scene map → 31 synthesised + stored chunks; all 305 headers open; the practice range resolves 34 chunks and the default client draws them, including 2,449 model-group entries); the directory lookup is verified (`fn/BundleDirectoryLookup`) but its producer is not; the header has **no client-side variation selector** (the map arrives as type-`0x002` GUIDs in the level-load message), so the open parts are the level-load message's producer, the bundle directory's producer and the streaming policy | `formats/maps.md` |
| 19 | Materials and textures | model -> look -> material -> data -> texture chain read; bundle-aware `open_asset`; BC1-BC7/BC6H decode; client material application and the 55 practice-range placeholder draws are open | `formats/materials.md` |
| 20 | Lighting | manifest table and chunk GUIDs read | `formats/lighting.md` |
| 21 | Shaders | group and code containers read; bytecode semantics out of scope | `formats/shaders.md` |
| 22 | Sound | Wwise metadata, sound/voice manifests and WEM headers read; audio decode not started | `formats/sound.md` |
| 23 | Replay | `phl`/`prp` container and verified prefix read; frame payload not decoded | `formats/replay.md` |
| 24 | Content surfaces | unlocks, skin themes, loot boxes, voice conversations and catalogs read where shipped | `formats/content.md` |

## Not in scope

- **Network protocol and matchmaking** — not read; no doc retained.
- **UI implementation** — the client uses imgui; not a gameplay target.
- **Renderer behaviour** — the viewer exists; the client's rendering pipeline
  is not a gameplay claim.
- **Anti-cheat and loader internals** — not part of gameplay behaviour.
