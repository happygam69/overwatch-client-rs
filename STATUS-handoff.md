# STATUS

The living state: what works, what is open, and what comes next.
`MODLOG.md` is the milestone history; `docs/formats/` holds findings and
addresses; `docs/open-questions.md` is the work queue.

## The gate

```sh
cargo xtask verify            # fmt, clippy, tests, tool index, deviation check
cargo xtask verify --fast     # without the install-reading tests
```

## Current state

**The one client launches the real practice range.** `ow-client` and
`ow-scene` are workspace members. The default launch resolves a
`STUMapHeader` by `m_mapName` (`Maps\TrainingRoom\Training Room - Practice`),
takes the first paired variation whose `STUGameMode.displayName` is exactly
`Practice Range`, and loads the 34 chunks the client's map loader requests for
scene map `0x080000000f69002e`. No chunk seed, capture or synthetic course is
the default.

The client is a **viewer**. It makes no movement, combat or camera-behaviour
claim. `ow-sim` is parked reference code.

## Exact today

- **CASC oracle:** five verified units, `python dev/verify_harness/cli.py casc`
  passes 5/5: `Salsa20Xor`, `Salsa20StateInit`, `DynamicIndexHash`,
  `DynamicIndexKeyHash` and `EncodingHeaderParse`. The oracle found and fixed
  the zero-length and trailing-twelve-byte index-hash defects.
- **Build-154504 manifest:** `CmfKey154504` (`0x142e48a20`) and
  `CmfIv154504` (`0x142e48700`) use the table at `0x1436100c0`, read from the
  installed `Overwatch.exe` at runtime. The encrypted manifest decrypts to
  1,301,598 GUID-sorted records.
- **Bundle assets:** the CMF record byte's bit 7 selects the encoded store or
  a type-`0x09C` bundle; the entry lookup is read from `FUN_141fd5820`
  (`0x141fd5820`) and implemented in `ow-tank::bundle`. All 305 type-`0x09F`
  map headers open: 42 from the store, 263 from bundles.
- **Map resolver:** `ow-tank::mapresolve` follows a header's paired mode and
  variation arrays, the variation's type-`0x002` scene map, the map asset's
  stored chunk references and the 31 synthesised chunks. The practice range
  requests and opens 34/34 chunks: 1,358 placeables (229 model-group
  references, 550 entity references), bounds pinned bit-exact.
- **Client:** 229 distinct models, 2,449 model-group placements, 872 draws.
  Materials report 289 looks, 594 entries, 120 decoded textures, 817 textured
  submesh draws, 55 placeholders, 148 flat fills and 3 no-sRGB entries.
- **Other verified client functions:** `AnimTimeWrap`,
  `BuildPerspectiveMatrix`, `InitProjectionDescriptor`,
  `OrthonormalizeCameraBasis` (stateful) and `MapChunkGuid` (`fn/ChunkGuid`,
  the first `ow-tank` tier-2 unit). `CurveKeySampleDual` is inconclusive at
  75% coverage; the two `ow-sim` units cannot run while `ow-sim` is parked.

## Open

- **Map variation selector:** the client function that chooses a variation for
  a requested game mode is not read. The practice header has two modes named
  `Practice Range` (`0x18` and `0x5a`); the launch takes the first in header
  order. This is an affinity, not the client's selector.
- **Bundle directory producer:** our bundle index scans the manifest's
  type-`0x09C` records; the client builds a directory blob in its content
  database (`FUN_141fd53a0`, `0x141fd53a0`). One non-1:1 stage remains.
- **Streaming policy:** which chunks load when is not read.
- **Gameplay:** movement, combat, camera application, view model, retarget,
  Pose2d VM, fire loop/damage and collision are open. The old approximations
  are deleted.
- **Materials:** the viewer is not the client's renderer; 55 placeholder draws
  remain diagnostics, not a finished material path.
- **Remaining CASC units:** `DynamicIndexSegmentParse`, `DynamicIndexFindEntry`,
  `DynamicIndexDecodeEntries`, `BlteFrameHeaderParse`, `BlteBlockRead` and
  `DynamicContainerRead`.

## Next

1. Read the variation-selector caller (`docs/open-questions.md` item 11).
2. Rebuild movement from the `MoverInvoker` caller through `MoverAdvance` and
   Domino.
3. Then camera, animation retarget, combat and collision in that order.

## Key files

- `docs/rebuild-plan.md` - stage order and current gates.
- `docs/open-questions.md` - the work queue.
- `docs/formats/character-controller.md` - mover component, current-image path.
- `docs/formats/domino.md` - physics tick and integration.
- `docs/formats/weapons.md` - combat data and statescript join.
- `docs/formats/camera.md` - camera surface, FOV, near/far, input.
- `docs/formats/animation.md` - clips, sampler, blend tree, view model.
- `docs/formats/maps.md` - map header, bundle, resolver and practice range.
- `docs/deviations.md` - legacy non-1:1 code.
- `docs/systems-coverage.md` - every system and its next anchor.
- `dev/INDEX.md` - analysis tools.
