# `ow-casc`: what is verified and what is not

The CASC companion to [`ow-tank-1to1.md`](ow-tank-1to1.md). It records what is
proven against the client's bytes, what is read and tested against the
install, what is ported from a public reference, and what is still open.

## Ground truth

The source is the current build's code. The harness oracle is the installed
`Overwatch.exe` PE with its `.text` decrypted by `dev/offline_page_recover.py`
into `module_offline.bin`: the PE headers and timestamp are the installed
file's own, `.rdata`, `.data`, `.pdata`, `.rsrc` and `.reloc` are byte-identical
to it, and `.text` is the decrypted code. No running process is involved. The
recovery report is `dev/data/offline_recover/REPORT.md` (local, git-ignored);
its held-out check agrees 2,284/2,284.

The unit descriptors carry the recovery image's `v2b` frame. The runner maps it
to the current-image address with
`v2b - 0x7ff7c4d20000 == exe - 0x140000000`; every citation in this page is the
`0x140000000 + RVA` form.

**Gate.** `python dev/verify_harness/cli.py casc` runs the five CASC units
(about 128 s with warm mutant build slots). `cargo xtask verify` does **not**
run the differential harness, so a CASC regression can pass the nominal gate.
The ledger binds a verdict to the candidate source and the git revision;
re-run `casc` after a commit if you need a fresh binding.

## A. Verified by the differential harness

| function | address | unit | result | honest caveat |
|---|---|---|---|---|
| `Salsa20StateInit` | `0x1424e3230` | `fn/Salsa20StateInit` | verified: 100%, 24/24 | proves the 64-byte state; the return pointer is not compared |
| `Salsa20Xor` | `0x1424e32c0` | `fn/Salsa20Xor` | verified: 100%, 7/7 scored + 3 declared equivalent | the probe seeks to the counter and advances the state itself; canonical states and counters 0..3 only, high-word carry untested |
| `DynamicIndexHash` | `0x142512c60` | `fn/DynamicIndexHash` | verified: 100%, 12/12 | three post-loop edges declared dead; the client's alignment-dependent loads are oracle-side, the Rust is byte-wise |
| `DynamicIndexKeyHash` | `0x142513230` | `fn/DynamicIndexKeyHash` | verified: 100%, 24/24 | production callers: `DynamicIndexSegmentParse` validates each page record and `DynamicIndexPageInsert` stores the same hash; the page-table read path is implemented |
| `EncodingHeaderParse` | `0x142498920` | `fn/EncodingHeaderParse` | verified: 100%, 39/39 | proves the raw 40-byte struct; the null-data guard is untestable; the production wrapper zeroes the buffer before the call |

These are the only CASC functions for which the five-stage word "verified" is
earned. The rest of the container is not.

## B. Read and install-tested, not harness-verified

| area | client address | what is proven today |
|---|---|---|
| `.build.info` row/bool select | `0x14248f2a0`, `0x1424aec30`, `0x1424d8800` | the rules are read-cited and pinned on synthetic text; no install test |
| keyring parse/byte order | `0x1424cebd0`, `0x14248f040`, `0x14248efd0` | read-cited; synthetic byte-order test; the build keyring is read from the install at runtime |
| bucket hash and segment/archive selection | `0x142501b40`, `0x14250e100`, `0x14250e720`, `0x14250e400`, `0x14250c8f0` | the fold is matched on 1,147,132 real keys (salt 0) and 1,488 archive-header placeholders (salt 1); the scan keeps the highest segment per bucket; the archive is `(packed + 0x1e) >> 30`; install-tested, no harness unit (stateful/I/O) |
| index entry search and duplicate choice | `0x1425111a0`, `0x142511540`, `0x1425115d0`, `0x1425209a0` | the client's lower-bound walk and equal-range expansion are implemented over file-order entries; the first equal entry is chosen; every equal group of all 1,148,620 install entries was range-checked and the 806 duplicate pairs resolve to the header entry |
| page table layout, hash and lookup | `0x14250fcf0`, `0x1425111a0`, `0x1425209a0`, `0x142513230` | parsed at the client's offset with 21 records per 0x200 page; a record is a u32 hash + key9 + location5 + size4 + type1; the stored hash is `DynamicIndexKeyHash(payload, 0x13, 0)` OR `0x80000000`; the client scans pages and records linearly and stops at the first zero hash. Synthetic non-zero tables cover first/middle/last, type 1/2/3/6, a zero sentinel, a same-key collision and page counts one/many; all 2,560 install pages have zero hashes, so the branch is dormant there |
| bucket index header/entries | `0x14250fcf0` | header/section/entry/page-table layout and both widths; synthetic tests plus real hash/page counts |
| encoding header/real values | `0x142498920` | header verified; installed 16/16 keys, 4 KiB pages, 16025 + 10519 pages and the 1,484,507-byte espec block are pinned |
| BLTE N/Z/E blocks | `0x1424e3230`, `0x1424e32c0` | Salsa20 verified; the N/Z/E container walk is synthetic and self-consistent, not client-compared |
| `.cmf` header/record/flags | `0x141fda1f0`, `0x141feff80`, `0x141fef610`, `0x141fd4a50` | the flag chain and the store/bundle branch open 305/305 map headers |
| bundle entry lookup | `0x141fd5820` (ow-tank) | `fn/BundleLookup`: verified, 100%, 12/12 |
| bundle directory lookup | `0x141fd5160` (ow-tank) | `fn/BundleDirectoryLookup`: verified, 93.3% with the loaded-bundle false edge as a named gap |
| build-154504 key/IV | `0x142e48a20`, `0x142e48700` | differential + mutation (364 cases each); coverage gate 0, acknowledged gap; not a five-stage verdict |

## C. Ported or reference-derived, not client-read

| area | source | status |
|---|---|---|
| CDN routes/fetch/cache | TACTLib `HttpCDNClient.Fetch` / `ICDNClient` | a port; 12 offline mock tests, no client address, no real endpoint test |
| CDN index-page formula | reference `CDNIndexHandler` + measured footers | reference/measured; its four tests build the fixture with the same formula |
| CMF builds 154088/150851 | TACTLib `ProCMF_154088`; no test for 150851 | ports; only 154504 has differential units |
| config text parse | no client address | format-derived; no test against the install's real config files |

## D. Open

| area | client address | state |
|---|---|---|
| `DynamicIndexSegmentParse` | `0x14250fcf0` | header/section/entry/page-table layout parsed by `ow-casc`; the asynchronous segment load and the page-table **writer** remain open |
| `DynamicKeyStateLookup` residency | `0x142520c60`, `0x142520700`, `0x1425208d0` | the chosen location and size are read; the interval merge and the `DynamicIndexRangeIsResident` resident-range check are not |
| `DynamicIndexDecodeEntries` | `0x1425209a0` | the 18-byte entry decode and the page-record decode are implemented, including the page type's effect on the chosen entry; the interval-buffer/type side effects are not |
| `BlteFrameHeaderParse` | `0x1424a90e0` | accepts only type `0x0F`; does not check `12 + 24*count`; skips the frame and chunk MD5s |
| `BlteBlockRead` | `0x1424d61b0` | N/Z/E only; no LZ4 (`4`, `BlteBlockReadLz4` `0x1424d6e50`), no ARC4 `A`, no frame `F`, no type `0x10` |
| `DynamicContainerRead` | `0x1424f48d0` | reads the 30-byte header and a span; no page cache or page-reader path |
| `EncodingSectionsRead` | `0x1424d1c60` | page reader is format-derived; no client function and no real-file test |
| CDN index-page reader | none | reference/measured; no client handler read |
| content key = MD5(decoded) | none | measured 300/300; no MD5 code and no test in the tree |
| LZ4 / ARC4 / frame modes | `0x1424d6e50` etc. | not implemented; no asset on this install exercises them |

## E. What the docs got wrong (corrected in this pass)

- `docs/formats/casc.md` said LZ4 mode `4` was **implemented**. The code
  rejects every mode except `N`/`Z`/`E`, and there is no LZ4 dependency. It is
  now marked not implemented.
- `docs/formats/casc.md` called `CmfKey154504`/`CmfIv154504` a "Harness proof"
  although their coverage gate is `0`; they are differential + mutation only.
- `docs/formats/casc.md` and `manifest.rs` called the 154088/150851
  derivations verified; 154088 is a TACTLib port and 150851 has no test.
- The Oracle paragraph said the oracle is `Overwatch.exe`; it is the installed
  PE's decrypted `.text`, which is the same build with all plaintext sections
  byte-identical.
- Three stale numbers: `103 MB` (now 109,882,544 stored / 111,058,223 decoded),
  `1,467,099` (now 1,484,507), and the orphan "357 entry records" sentence.
- `docs/systems-coverage.md` summarised the container as verified while six
  functions are open; the row now separates the verified leaves from the open
  path.
- The bucket section said the client opens "the segment named by the container
  header". There is no such field: `DynamicIndexOpen` (`0x14250e100`) scans
  the directory for `<bucket><segment>.idx`, keeps the highest segment per
  bucket, and caches the sixteen numbers at `+0x110`. The old all-16 merge is
  deleted.
- The page table was described as "a `u32` hash, a 19-byte key and a pad". The
  19-byte payload is key (9) + location (5) + size (4) + type (1); the pad
  makes the record 24 bytes. `DynamicIndexFindEntry` stops at a zero stored
  hash, which is every record on this install; the positive-record path is
  implemented and synthetic-tested, not exercised by the install.
- `docs/casc-1to1.md` said `DynamicIndexKeyHash` had no production caller. It
  has six: `DynamicIndexSegmentParse` validates every page record; the page
  writer (`DynamicIndexPageInsert`) stores the same hash; four other index
  families call it too. The page-table read path is now implemented.

## Next order

1. The page-table writer (`DynamicIndexPageInsert`, `0x142511d20`) and the
   key-state interval merge/resident-range check (`0x142520c60`,
   `0x142520700`, `0x1425208d0`): the page-table read path is in place and
   synthetic-tested, but the insert range and the interval side effects are
   not.
2. `BlteFrameHeaderParse` (`0x1424a90e0`) and `BlteBlockRead`
   (`0x1424d61b0`): type `0x10`, size equality and the MD5s; then the missing
   modes.
3. `DynamicIndexSegmentParse` (`0x14250fcf0`) asynchronous load beyond the
   already-parsed header/entry/page-table layout.
4. `EncodingSectionsRead` (`0x1424d1c60`) and a real encoding page walk.
5. The CDN index-page formula from the client's handler, then LZ4.

## Closing a row

A row moves to verified only when a unit names the current-image address, uses
a `candidateCrate: "ow-casc"` probe, and passes all five harness stages. A row
may instead be recorded as a port or an open question; it may not simply
disappear.
