# MODLOG

Newest first. Milestones only; findings and addresses live in
`docs/formats/`, current state in `STATUS-handoff.md`, the work queue in
`docs/open-questions.md`.

## 2026-10-29 - Texture upload: exact 16-bit and HDR values

- `Texture::rgba16` and `SceneTextureLevel::rgba16` now carry exact half words
  for R16G16B16A16_FLOAT (DXGI 10) and R16_FLOAT (54) as well as BC6H (95/96);
  R16_FLOAT is padded `(r, 0, 0, 1.0)`. `SceneTexture` also carries the stored
  format byte, so the uploader no longer needs the display string.
- `ow-client` uploads those levels as `Rgba16Float` (8 B/px, 256-byte row
  pitch), a documented **viewer substitution** for the client's DXGI ids; the
  format switch is `TextureFormat::sample_type`-checked against the existing
  `Float { filterable: true }` layout. R16_UNORM, UINT, the BC CPU decode and
  the sampler remain open.
- Format ids re-read from the current image: stored `0x0A`→PM `0x32`→10,
  `0x37`→`0x0B`→54, `0x39`→`0x07`→56, `0x60`→`0x44`→95, `0x61`→`0x43`→96
  (`0x143356f50` → `0x14348f500`); `0x1426545d0` writes the id and
  `0x141f0a000` copies the blocks with no CPU conversion.

## 2026-10-29 - Texture parser leaves: descriptor, size-walk and format units

- Corrected BC2/BC3 to the four-colour colour block (BC1 keeps three-colour)
  and applied the texture docs audit, including the 25-unit / twelve-`ow-tank`
  ledger line.
- Added three tier-2 units against the current image: `fn/TextureDescriptor`
  (`0x142e2d900`, 364 cases, 100% branch coverage, 12/12),
  `fn/TextureDescriptorSizeWalk` (`0x142e2dbb0` over `0x142e37070`, 364 cases,
  100% branch coverage, 11/11 with one declared equivalent mask widening) and
  `fn/TextureFormatFamilyBpp` (`0x142e372b0`, 374 cases, 100% branch coverage,
  12/12).
- The format switch has no `.pdata` entry, so its unit declares an exact
  `coverageWindow` of 285 bytes (`0x142e372b0..0x142e373cc`). This stops the
  branch sweep before the dispatch table at `0x142e373d0..0x142e37491`; the
  size walk's dispatch block is `0x142e37164..0x142e37189`.
- The descriptor unit compares the 16-byte descriptor/flags prefix; fields past
  `+0x10` are builder bookkeeping and stay outside the unit (documented gap).

## 2026-10-29 - Texture chain: header/payload, material records, colour space, mip/array layout

- The type-`004` header, its descriptor and its chunk/payload chain are read:
  the embedded chunk and each payload's own header name their level range
  (`0x142e38730`/`40`/`50`, `0x142255e10`), the payload GUID is
  `0x142e37520`, and `0x141e29210` selects the raw run or the chunk/payload
  runs.
- The material record's payload shape and input-name hash are read
  (`material.rs`); the generic material-data → shader join and the base-colour
  rule remain open (`docs/formats/materials.md`).
- Upload colour space is the asset's DXGI id through `0x143356f50` →
  `0x14348f500`; the resource (`0x1426545d0`) and SRV format
  (`0x142653b90`) use the same id, and the viewer carries it as
  `SceneTexture::srgb`.
- The mip/array layout is read from `0x141e296a0` and the `0x141f0a000` copy
  loop: outer array element/face, inner mip ascending, data pointer advancing
  by each level's size; record index `(array*faces + face)*mip_count + mip`
  (`0x142e36e80`). `tests/texture_levels_real.rs` pins the cubemap, the two
  multi-mip assets and one raw six-surface asset at independent offsets, and
  scans 400 assets. 1D/cube SRV views, the sampler, volume depth and the
  alternate `0x141f11f90` copy path remain open.

## 2026-10-28 - CASC page-table writer read: record layout, status machine, flush

- `DynamicIndexPageInsert` (`0x142511d20`) is read operation for operation:
  the first zero stored hash, the 24-byte record at
  `page * 0x200 + record * 0x18`, the 19-byte payload hashed with
  `DynamicIndexKeyHash(payload, 0x13, 0) | 0x80000000` after a full fence, and
  the status machine 0/1/2/4/`0xa`/`0xc`/`0xf` in `FUN_142520cc0`
  (`0x142520cc0`). A record index of 20 with `page & 7 == 7` flushes the
  preceding eight pages through `FUN_1426a8ce0` (`0x1426a8ce0`).
- `DynamicIndexKeyInsert` (`0x1425032d0`) creates the bucket's next segment on
  `0xf` and retries once; twelve callers, fourteen call sites, and no
  read-path caller.
- Not implemented, and the docs say why: the writer is allocator and I/O
  state, and all five type-3 call sites pass a null range (for example
  `FUN_142500430` at `0x142500541`), so those records' location and size are
  uninitialised stack bytes. The writer leaves the CASC open list;
  asynchronous segment load is now first.

## 2026-10-28 - CASC BLTE frame header and block reader

- `BlteFrameHeaderParse` (`0x1424a90e0`) is implemented operation for
  operation: type `0x0F`/`0x10`, `header_size == 12 + record_size*count`, the
  `0x24` minimum, the optional header-MD5 check, the 24/40-byte records and
  the type-`0x10` extras.
- `BlteBlockRead` (`0x1424d61b0`) dispatches `N`/`Z`/`E`/`4`; `E` accepts an
  IV of 0..8 bytes, and `BlteBlockReadLz4` (`0x1424d6e50`) is implemented with
  the client's 11-byte header, `5..=16` exponent and stateless LZ4 call. There
  is no `A` mode; `F`/`O` are status 5 at `FUN_1424eb510`.
- A one-off full scan of 1,146,326 unique salt-0 keys found 1,146,325
  type-`0x0F` containers, one known non-BLTE orphan, 1,714,597 chunks
  (`N` 284,878, `Z` 1,386,658, `E` 43,061), and no `0x10`, direct container or
  outer `4` block. The committed install test samples every 32nd key and
  decodes 34 records; E inner modes are unreadable because the install keyring
  lacks their keys.
- The `Salsa20Xor` unit's three declared-equivalent mutant line numbers moved
  with the file; the unit is re-verified, and `cli.py casc` remains 5/5.

## 2026-10-28 - CASC key-state intervals and resident resolve

- `DynamicIndexEntrySink` (`0x142520590`), `DynamicIndexAppendInterval`
  (`0x142520700`), `DynamicIndexNormalizeIntervals` (`0x1425208d0`) and
  `DynamicIndexRangeIsResident` (`0x142520c60`) are implemented as the client's
  16-byte `{u64 start, i32 delta}` events: types 1-7, resident and non-resident
  deltas, unstable sort, i32-wrapping equal-start sums, crossing output, the
  inclusive end comparison and the flag-0 complement.
- `DynamicIndexResolveRange` (`0x1425115d0`) is implemented with the client's
  bounds and statuses: 0 found, 2 no entry, 3 not resident, 4 past end;
  `0xffffffff` becomes `size - start`; `out_range` is written before the
  resident check and `out_entry` only on success.
- `DynamicIndexDecodeEntries`' decode order is reproduced (type 1 for a single
  equal entry; type 2 then type 6 for duplicates; page records in order; one
  final normalize), and `Store::read_stored` now resolves `(0x1e, size - 0x1e)`
  before the full-record read. Status 2 is `KeyNotFound`; 3/4 is a parse error.
- The install test now resolves all 1,147,132 salt-0 payload requests to
  `packed + 0x1e` (status 0), all 1,488 salt-1 placeholders to status 2, and
  checks that the 806 duplicate requests are the second entry's type-6
  interval. Synthetic tests cover types 1-7, gaps, i32 wrap, all statuses and
  `0xffffffff`. Page types 3/4/5/7 are synthetic only; the install's pages are
  all zero.

## 2026-10-28 - CASC page table read: layout, hash, linear lookup

- The `.idx` page table starts at
  `(entry_block_size + entry_header + 0x1007) & ~0xfff`; 21 24-byte records per
  0x200 page, a `u32` stored hash plus key9 + location5 + size4 + type1 and a
  pad byte. `DynamicIndexFindEntry` scans pages and records in order and stops
  at the first zero stored hash; there is no hash-based page selection.
- `DynamicIndexKeyHash` has production callers after all:
  `DynamicIndexSegmentParse` validates every page record and the page writer
  `DynamicIndexPageInsert` stores the same `hash | 0x80000000`. The old "no production
  caller" line is corrected.
- `PageTable`/`BucketFile` in `index.rs` parse and retain the table; `find`
  applies the page type byte (1/2 replace the chosen entry, 3 clears it, 4-7
  only interval/metrics) after the sorted-entry range. Synthetic tables cover
  first/middle/last, type 1/2/3/6, zero sentinel and page counts one/many.
- The install has 2,560 pages and 53,760 record slots, all with zero stored
  hashes; the first zero is page 0 record 0 in every file, so no key resolves
  through the page table there. The install test reports those counts.

## 2026-10-28 - CASC index entry search and duplicate choice

- `DynamicIndexFindEntry` (`0x1425111a0`) is now the client's lower-bound walk
  with backward/forward equal-range expansion over file-order entries, not a
  HashMap. `DynamicIndexResolveRange` (`0x1425115d0`) and the
  `DynamicIndexResolveKeyRange` wrapper choose the **first** equal entry; the install's 806
  duplicate pairs therefore resolve to the 30-byte header entry, not the
  payload entry.
- The page table is 21 records per 0x200 page at 24 bytes: a `u32` stored hash
  plus a 19-byte payload (key 9 + location 5 + size 4 + type 1). The install's
  page count is non-zero but every stored hash is zero, so the page-table
  branch stops at the first record and contributes nothing; its positive-record
  path is still open. `docs/formats/casc.md` records the rule and proof.
- `tests/index_find_real.rs` walks all 1,148,620 entries: every key's range is
  its contiguous equal group, all 1,147,132 salt-0 keys resolve to the first
  entry, 1,488 salt-1 placeholders resolve to nothing, and all 806 duplicate
  pairs choose the header entry and read back BLTE.

## 2026-10-28 - CASC bucket hash and segment/archive selection are read and install-tested

- `DynamicKeyStateLookup` (`0x142501b40`) is `((xor9 ^ (xor9 >> 4)) + salt) & 0xf`;
  all three callers pass salt 0. `ow-casc` now has `index::bucket_of` and uses
  it instead of a merged all-16 map.
- `DynamicIndexOpen` (`0x14250e100`) / `DynamicIndexScanSegments`
  (`0x14250e720`) select the highest `<bucket><segment>.idx` per bucket and
  cache the sixteen numbers at `+0x110`; there is no container-header field.
  The old merge is deleted, and a missing bucket or segment is an error.
- The install test matches 1,147,132 real keys at salt 0 and 1,488 30-byte
  archive-header placeholders at salt 1, with 0 unmatched; 806 keys are listed
  twice (header and payload), which remains part of the open
  `DynamicIndexFindEntry` work. `docs/formats/casc.md` records the rule,
  addresses and proof.

## 2026-10-27 - CASC audit: five leaves verified, the container path open

- The five differential units are real: `Salsa20Xor`, `Salsa20StateInit`,
  `DynamicIndexHash`, `DynamicIndexKeyHash` and `EncodingHeaderParse`, all
  against the current build's decrypted `.text`. The installed PE's plaintext
  sections are byte-identical to the recovered image; no process capture is
  involved.
- The container path is not: six named units remain, LZ4 was falsely marked
  implemented, frame type `0x10` and the MD5s are skipped,
  `DynamicIndexFindEntry` is a HashMap substitute, the segment/bucket selection
  is ours, and the CDN page formula is reference-derived.
- `docs/casc-1to1.md` is the new register; `docs/open-questions.md` item 14 is
  the queue. The audit also corrected the LZ4, "verified derivations", CMF
  "harness proof" and stale-number claims.

## 2026-10-27 - The verification harness's mutation stage is parallel and bounded

- `fn/BundleLookup` went from 1310.6 s to 77.9 s (16.8x) and
  `fn/BundleDirectoryLookup` from ~25 min to 158.1 s, with the same verdicts.
  The time was 93% a 300 s wait on each of four mutants that never return, plus
  a full dependency rebuild per mutant.
- Mutant probes now build and run in a worker pool (`--jobs`, default
  `min(4, active)`), the mutation corpus's oracle results are read from the
  stage-3 cache in the main thread, and each mutant has a mutation-only call
  budget (`--mutation-timeout`, unit option `mutationCallTimeout`, default
  10 s; the baseline keeps 300 s). A timeout is still a kill, and is logged as
  one.
- Mutant builds share four warm cargo target slots per crate. `cargo clean -p`
  plus an artifact stamp check makes a build always dirty and a stale binary a
  hard error. `HARNESS_VERSION` is 5.
- No unit JSON, corpus, fixture, threshold or mutant declaration changed.
  Ledger comparison for the five proof units: status, coverage and
  killed/total identical. Numbers and the proof are in
  `dev/verify_harness/PERFORMANCE.md`.

## 2026-10-27 - The bundle directory lookup is harness-verified

- Added `fn/BundleDirectoryLookup` to `dev/verify_harness`: oracle
  `BundleDirectoryLookup` at `0x141fd5160`, candidate
  `ow_tank::bundle::directory_lookup` through `crates/ow-tank/src/bin/verify_probe.rs`
  (a `bundle_directory_lookup` entry whose state region holds the directory at
  its own image address). 364 synthetic differential cases on every channel,
  93.3% branch coverage (14/15 reachable branches of 18 sites; three sites
  declare one structurally dead edge each) and 35/35 mutants killed (one
  further type-count gate flip declared equivalent; one signature-line mutant
  does not compile and is counted killed).
- Read the blob the function walks: the type-entry array at `+0x18` and count
  at `+0x20`, the 0x20-byte type entry, the group's sorted/index arrays and
  running listed count, the handle-array slot arithmetic and the tagged handle
  status. The type key is the asset type id (the bit-reversed 12-bit
  field at GUID bits 48..59, plus one); a mask read as
  `0xFFF0...` agreed on every `0x0D90...` GUID and diverged on a random one.
  `FUN_141fd4040` disassembles as `SHR EAX,0xf` / `AND AL,1`, not the
  decompiler's `& 0xffffff01`; bits 23 and 31 are not status bits, and the
  first differential run caught that on a bit-31 object.
- The loaded-bundle guard's false path needs `FUN_141fcf330` and the live
  content-database object, so it is a named acknowledged gap (coverage gate
  93%), not a silent exclusion. The producer of the directory blob remains
  open in `ow-tank-1to1.md` section A; this unit verifies its consumer.

## 2026-10-27 - The map variation selector is not a client function

- Read the header's mode/variation path to its end. `m_D97BC44F`
  (`0xD97BC44F`) and `m_78715D57` (`0x78715D57`) occur in `.text` only at
  their registration sites (`0x14143ba49`, `0x14143bc95`); the class hash
  `0x02514B6A` occurs only at `RegisterStuMapHeader` and in the lazy
  asset-type descriptor init at `0x1403216a0`. The field descriptors and the
  class definition have no consumer, so the client never reads a header's
  variation arrays.
- The map reaches the client as type-`0x002` GUIDs in Jam message `0x501B`
  (dispatch `0x14063b3f0`, handler `0x140618b30`, preload `0x14077b7d0`;
  four GUID vectors at `+0x78`/`+0x90`/`+0xA8`/`+0xC0`). The replay header's
  map GUID is a header's `m_map` (base map), not a variation's scene map.
- `m_A9253C68` is a `STUIdentifier` (`0xD800000000000000 | u32`, class
  `0x9CADF2EC`), not an asset. 305 headers / 1,612 mode entries carry 30
  distinct identifiers; a `(mode, identifier)` pair can name two scene maps
  (header `0x07900000000000d4`, mode `0x0230000000000016`). The practice launch
  is therefore documented as a viewer default; the open question narrows to the
  producer of the level-load GUID lists.
- `mapresolve::Variation` now carries the read `variant` identifier, and three
  install tests pin the finding (shared practice identifier; two scene maps per
  mode+identifier; replay map is a base map).

## 2026-10-27 - The bundle entry lookup is harness-verified

- Added `fn/BundleLookup` to `dev/verify_harness`: oracle `BundleEntryLookup`
  at `0x141fd5820`, candidate `crates/ow-tank/src/bundle.rs` through
  `crates/ow-tank/src/bin/verify_probe.rs`. 364 synthetic differential cases on
  every channel, 100% branch coverage (29/29 reachable branches of 35 sites,
  six structurally dead edges declared) and 12/12 mutants killed.
- The read settled the search: bounds `0` and `count - 1` (not a half-open
  pair), midpoint `(lo + hi) >> 1`, a lower bound and then an exact GUID
  equality, and offset `0` as "absent". The old half-open walk agreed on every
  GUID-sorted install table but diverges on an unsorted one, so `offset_of` now
  takes the client's walk operation for operation.
- The unit also proves the client's argument contract: the caller passes a
  tagged handle, not a payload pointer. `bundle::entry_lookup` models the
  handle guard, the width gate and the null payload check; the probe returns
  `guest_base + offset` so both sides compare one guest `u64`. The fixture is
  synthetic; no install bytes are used. The directory blob's producer is still
  open, so `docs/ow-tank-1to1.md` keeps that producer row in section A.

## 2026-10-27 - Build-154504 key and IV are harness units

- Added `fn/CmfKey154504` and `fn/CmfIv154504` to `dev/verify_harness`:
  oracle `CmfKey154504` at `0x142e48a20` and `CmfIv154504` at `0x142e48700`,
  candidate `crates/ow-tank/src/manifest.rs` through
  `crates/ow-tank/src/bin/verify_probe.rs`. 364 synthetic differential cases
  each on every channel; the mutation stage kills 1/1 and 5/5.
- Split `key_iv_154504` into the shipped `key_154504` and `iv_154504` halves;
  the read showed the key seed is `table[0x120]` with step 3, and the IV
  cursor starts at `16 * buildVersion`, steps by
  `digest[dataCount % 20] + dataCount`, and XORs
  `digest[(154504 + i) % 20]`.
- The units use synthetic tables, headers, names and digests; no installed
  table or manifest bytes are in the repository. The coverage stage cannot
  measure either real body (the linear sweep desynchronises at the
  overlapping-instruction opaque-predicate prologue), so both units carry an
  acknowledged gap and set the coverage gate to 0; the row stays in
  `docs/ow-tank-1to1.md` section B.

## 2026-10-27 - The chunk-GUID formula is harness-verified

- `fn/ChunkGuid` is the first `ow-tank` unit in `dev/verify_harness`: oracle
  `MapChunkGuid` at `0x141b32e20`, candidate
  `crates/ow-tank/src/mapguid.rs` called by
  `crates/ow-tank/src/bin/verify_probe.rs`. 364 differential cases on every
  channel, 100% coverage, 3/3 viable mutants killed.
- The read showed the kind is the full `+8` qword of the stride-`0x20` table
  entry at `0x1439aa320`, shifted left 32 in a 64-bit register; the old
  `kind & 0xFFFF` mask dropped kind `0x1_0000`. The map argument is a `uint`,
  so only its low 32 bits are used.
- The probe performs the client's table-slot read and passes the identical
  fixture bytes to the oracle, so the unit proves the formula against the
  client's instructions rather than against the install alone.

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
