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
the default. This selection is a **viewer default**: the current image has no
client-side mode→variation selector (see "Open").

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
  1,301,598 GUID-sorted records. The two routines now have differential units
  (`fn/CmfKey154504`, `fn/CmfIv154504`): 364 synthetic cases each, every
  channel, all key and IV bytes; the coverage stage cannot measure the body
  (acknowledged gap, gate 0), so the row stays in `ow-tank-1to1.md` section B.
- **Bundle assets:** the CMF record byte's bit 7 selects the encoded store or
  a type-`0x09C` bundle; the entry lookup is read from `FUN_141fd5820`
  (`0x141fd5820`), implemented in `ow-tank::bundle`, and is a verified unit
  (`fn/BundleLookup`: 364 synthetic cases, 100% branch coverage, 12/12
  mutants). All 305 type-`0x09F` map headers open: 42 from the store, 263 from
  bundles.
- **Bundle directory:** the per-type directory **lookup** is read from
  `BundleDirectoryLookup` (`0x141fd5160`), implemented as
  `ow_tank::bundle::{Directory, directory_lookup, guid_type_key}` and verified
  by `fn/BundleDirectoryLookup` (364 synthetic cases, 93.3% coverage with one
  named guard gap). The directory's **producer** is still open; the install
  scan in `BundleIndex` remains our own.
- **Map resolver:** `ow-tank::mapresolve` follows a header's paired mode and
  variation arrays, the variation's type-`0x002` scene map, the map asset's
  stored chunk references and the 31 synthesised chunks. The practice range
  requests and opens 34/34 chunks: 1,358 placeables (229 model-group
  references, 550 entity references), bounds pinned bit-exact. The mode
  entry's `m_A9253C68` is read as a `STUIdentifier` (30 identifiers across the
  install), and the replay header's map GUID is proven to be a base map, not a
  scene map.
- **Client:** 229 distinct models, 2,449 model-group placements, 872 draws.
  Materials report 289 looks, 594 entries, 120 decoded textures, 817 textured
  submesh draws, 55 placeholders, 148 flat fills and 3 no-sRGB entries.
- **Other verified client functions:** `AnimTimeWrap`,
  `BuildPerspectiveMatrix`, `InitProjectionDescriptor`,
  `OrthonormalizeCameraBasis` (stateful), `MapChunkGuid` (`fn/ChunkGuid`,
  the first `ow-tank` tier-2 unit), `BundleEntryLookup`
  (`fn/BundleLookup`, the second `ow-tank` tier-2 unit: 100% coverage,
  12/12 mutants) and `BundleDirectoryLookup` (`fn/BundleDirectoryLookup`,
  the third: 93.3% coverage with one named guard gap, 35/35 mutants killed).
  The build-154504 key/IV units
  (`fn/CmfKey154504`, `fn/CmfIv154504`) pass differential and mutation but
  keep an acknowledged coverage gap. `CurveKeySampleDual` is inconclusive at
  75% coverage; the two `ow-sim` units cannot run while `ow-sim` is parked.

## Open

- **Map variation choice:** the client has no mode→variation selector. The
  field hashes `0xD97BC44F`/`0x78715D57` occur only in the registration
  functions and the header class hash only in the asset-type descriptor init;
  the map arrives as type-`0x002` GUIDs in the level-load message (Jam
  `0x501B`, `0x14063b3f0` → `0x140618b30` → `0x14077b7d0`). What is open is
  the producer of that message, including which input it uses. A
  `(mode, identifier)` pair can name two scene maps, so the practice-range
  launch affinity is a viewer default, not a missing client rule.
- **Bundle directory producer:** our bundle index scans the manifest's
  type-`0x09C` records; the client builds a directory blob in its content
  database (`FUN_141fd53a0`, `0x141fd53a0`). One non-1:1 stage remains.
- **Streaming policy:** which chunks load when is not read.
- **Gameplay:** movement, combat, camera application, view model, retarget,
  Pose2d VM, fire loop/damage and collision are open. The old approximations
  are deleted.
- **Materials:** the viewer is not the client's renderer; 55 placeholder draws
  remain diagnostics, not a finished material path.
- **Remaining CASC units:** `DynamicIndexSegmentParse`,
  `DynamicIndexFindEntry`, `DynamicIndexDecodeEntries`, `BlteFrameHeaderParse`,
  `BlteBlockRead`, `DynamicContainerRead`, `EncodingSectionsRead` and the
  key-state resolve (`0x142511540`/`0x1425115d0`); the BLTE `4`/`A`/`F` and
  `0x10` modes, the frame/chunk MD5 checks and the CDN index-page formula are
  also open. The bucket fold and the segment/archive selection are now read and
  install-tested (1,147,132 salt-0 keys and 1,488 salt-1 header placeholders);
  `docs/casc-1to1.md` is the register.

## Next

1. Read the producer of the level-load GUID lists (`docs/open-questions.md`
   item 11.1): who resolves a map request to Jam `0x501B`'s four vectors, and
   whether the local practice-range session builds it in-process.
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
