# Roadmap to a rendered practice range

The end goal, stated properly: a **first-person 3D practice range**, playable,
with Overwatch's geometry, bots, HUD and combat — so the systems can be tested
the way the game tests them. Not a terminal harness.

This document exists because the project drifted. What was built is a data layer
and a terminal simulator; the target is a rendered game client. The gap is large
and this is the honest accounting of it.

## What exists

| | state |
|---|---|
| CASC container, manifests, STU | working and verified |
| hero health / armour / shields | 124 of 132 heroes |
| weapon ballistics (rates, speeds, lifetimes, ranges) | 56 graphs, 889 named parameters |
| combat simulation (hitscan, projectiles, spread, falloff, pools, armour) | working, unit-tested |
| per-shot damage | **not in the files** — see `docs/formats/weapons.md` |

## What a rendered range needs, and where each piece stands

| layer | state | notes |
|---|---|---|
| **map geometry** | available, not read | **45,302 map-chunk assets**, type `0x0BC`. TankLib documents this as "Tank MapChunk, file type 0BC" — which also settles the `.0bc` mystery in the community research doc: it is the map chunk, not an unknown format. |
| **models** (bots, props, weapon view models) | available, not read | 30,402 assets of type `0x00C`, a chunked container (magic `0xF123456F`) with Model / RenderMesh / Skeleton / Cloth / Hardpoint chunks |
| **textures** | available, not read | 191,599 assets of type `0x004`, Overwatch's TXTR |
| **materials and shaders** | **not reproducible** | the game's shader system is proprietary. Geometry and textures can be brought in faithfully; a *pixel-identical* look cannot. Expect a faithful layout with approximated materials. |
| **renderer** | not started | a `wgpu` window, first-person camera, depth buffer, texture sampling |
| **character controller** | not started | movement, gravity, acceleration, collision against map geometry |
| **combat numbers** | blocked | not in the files; see the Ghidra section |
| **HUD** | not started | health, ammo, crosshair, damage numbers, the wall panel from the range |

The asset side is the bulk of the work and **none of it needs reverse
engineering**. The formats are documented in the reference tooling and the
assets are on disk.

## Ghidra: what it can and cannot give

The expectation was that Ghidra supplies the missing reference point for
behaviour. Half true, and the half that is false matters.

**`Overwatch.exe` is encrypted at rest.** Measured on the retail binary:

```
.text    48,602,668 bytes   entropy 8.000   XR
.rdata   10,104,740 bytes   entropy 6.488   R
.data    13,202,108 bytes   entropy 4.558   WR
```

Entropy 8.000 is the maximum for byte data: the code section is not code on
disk. A normal compiled `.text` sits near 6.0–6.5. Consistent with that, the
whole 65 MB binary contains only **496 KB of printable strings**, and none of the
identifiers a combat implementation would want — `numShotsPerSecond`,
`projectileSpeed`, `HeroSetting` and `m_maxHealth` all occur zero times.

So loading the on-disk exe into Ghidra gives the loader and the decryptor, not
the simulation. It has been imported anyway (117,223 functions) so the
environment is ready, but **static analysis of the file as shipped will not
produce the combat constants.**

Getting at the decrypted code means capturing the image from a running process.
That is a different kind of step, and two things need saying plainly:

1. It is circumventing a protection layer rather than reading files. Whether
   that is acceptable is the operator's call, not a decision to make quietly.
2. **The client runs Warden.** Attaching a debugger or a memory dumper to a live
   session risks the account. That is a concrete cost to the person asking, and
   it should be weighed before, not after.

Recommended order, therefore: build the range from data first, because it does
not depend on any of this, and revisit the executable only for the specific
numbers that turn out to be unreachable any other way — with the risk known.

## Staged plan

Each stage is meant to be visibly closer to the range.

1. **Chunked-data reader.** `ow-tank` gains a reader for the chunk container
   (header magic `0xF123456F`, chunk table, 4-character XOR-reversed chunk tags).
   This is the prerequisite for every asset.
2. **Map geometry.** Read a map-chunk asset, get its vertices and indices, and
   dump a mesh to a file. Verify by geometry statistics and by a rendered still.
3. **A window.** `wgpu` + `winit`, a depth buffer, a free camera, the map mesh.
   This is the first point at which the project *looks* like the target.
4. **Textures and materials.** TXTR decode to RGBA, sampled with approximated
   material parameters.
5. **Models.** Skeleton, RenderMesh and Hardpoint chunks, so the training bots
   and the weapon view model appear.
6. **Character controller.** Movement, gravity, collision against map geometry.
7. **Combat and HUD.** The simulation already exists; it needs wiring to the
   rendered world plus health, ammo, crosshair and damage numbers.
8. **Bots.** The practice-range bots as entities with the health pools already
   extracted.

Stages 1–3 are the critical path and retire the largest risks: whether the
geometry can be read at all, and whether it renders. Everything after that is
incremental.

## What "1 to 1" can and cannot mean

Achievable: the same map, the same layout, the same bots, the same hero
statistics, the same weapon behaviour, a playable first-person camera, and a HUD
carrying the same information.

Not achievable: pixel-identical rendering. The shaders are proprietary and are
not shipped as data in a form that can be reimplemented from the install alone.

Worth stating once so it is not discovered late.
