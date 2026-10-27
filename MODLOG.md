# MODLOG

Newest first. Milestones only; findings and addresses live in
`docs/formats/`, current state in `STATUS-handoff.md`, the work queue in
`docs/open-questions.md`.

## 2026-10-27 - The one client launches the practice range

- `ow-client` and `ow-scene` are workspace members; `ow-sim` stays parked.
- `ow-tank::mapresolve::find_practice_range` enumerates all type-`0x09F`
  headers, finds the one whose `m_mapName` is
  `Maps\TrainingRoom\Training Room - Practice`, and takes the first paired
  variation whose `STUGameMode.displayName` is exactly `Practice Range`.
- The default launch draws the resolver's 34-chunk set: 1,358 placeables,
  229 distinct models, 2,449 model-group placements, 872 draws. No first-N
  chunk list, proximity cluster or sandbox default.
- Materials/textures open through `GameData::open_asset`; the viewer reports
  textured, flat-fill and placeholder draws instead of hiding them.
- `tests/practice_range_real.rs` pins the selection, chunk set, counts and
  bit-exact bounds. Open: the variation-selector caller, the bundle directory
  producer and the streaming policy.

## 2026-10-27 - Map resolver and bundle path

- The map chain is read and implemented: `STUMapHeader` -> paired mode and
  variation -> variation's type-`0x002` scene map -> stored chunk references
  plus the 31 chunks synthesised by `FUN_141b32e20` (`0x141b32e20`).
- `FUN_141ff5c80` (`0x141ff5c80`) reads four stored references from the map
  asset (`+0x1B8`, the `+0x1B0`/`+0x1B4` list, `+0x1C0`, `+0x1C8`) before the
  table slots; the former "second chunk path" is this stored set.
- The practice header `0x0790000000000688` resolves mode
  `0x0230000000000018` to variation 0's scene `0x080000000f69002e`: 34/34
  chunks, 1,358 placeables, bounds pinned.
- `ow-tank::bundle` and `GameData::open_asset` implement the client's
  store-or-bundle flag and entry lookup (`FUN_141fd4a50`, `0x141fd4a50`;
  `FUN_141fd5820`, `0x141fd5820`). The practice header and its map asset open
  from bundle `0x0d90968e484f8c33`.

## 2026-10-26 - Build-154504 manifest

- `CmfKey154504` (`0x142e48a20`) and `CmfIv154504` (`0x142e48700`) derive the
  AES-256-CBC key/IV from the installed client's table at `0x1436100c0`.
  No key bytes are vendored.
- The encrypted `Win_SPWin_RDEV_EExt.cmf` decrypts to 1,301,598 GUID-sorted
  records. The records previously called `STUMapHeader` on the `0x01e0...`
  prefix are type `0x079` (`VoiceCategory`); the real map headers are type
  `0x09F`.

## 2026-10-26 - CASC oracle

- Added `python dev/verify_harness/cli.py casc`, a dedicated suite for the
  shipped `ow-casc` functions.
- Verified `Salsa20Xor` (`0x1424e32c0`), `Salsa20StateInit`
  (`0x1424e3230`), `DynamicIndexHash` (`0x142512c60`),
  `DynamicIndexKeyHash` (`0x142513230`) and `EncodingHeaderParse`
  (`0x142498920`). Coverage and mutation gates pass; the ledger binds each
  verdict to its source and revision.
- The oracle found and fixed the index hash's zero-length and trailing
  twelve-byte-group defects.

## 2026-10-26 - Runtime reset and rebuild gate

- The runtime crates were parked and the active workspace reduced to the data
  layer (`ow-casc`, `ow-stu`, `ow-tank`, `xtask`).
- The practice-range chunk seed and the synthetic default map claim were
  removed. The rebuild order in `docs/rebuild-plan.md` made the CASC oracle,
  manifest, map resolver and client sequential gates.
- `cargo xtask verify` is the one gate; `--fast` omits install-reading tests.
