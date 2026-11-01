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

**Gate.** `python dev/verify_harness/cli.py casc` runs the eight CASC units
(about 150 s with warm mutant build slots). `cargo xtask verify` does **not**
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
| `EncodingFindCKey` | `0x142498540` | `fn/EncodingFindCKey` | verified: 100%, 7/7 | proves the out record address and hit/miss; the page search `0x142498880` is a callee outside the coverage window (its results are compared); RAX's upper bits are not compared because callers use AL |
| `EncodingFindEKey` | `0x142498610` | `fn/EncodingFindEKey` | verified: 100%, 10/10 scored + 1 declared equivalent | same search-callee caveat; pins the raw page-search length against the min record-scan length, and the `0xFFFFFFFF` terminator |
| `CdnIndexFooterParse` | `0x1424d9af0` | `fn/CdnIndexFooterParse` | verified: 100%, 11/11 scored + 1 declared equivalent | proves the 0x30-byte footer struct, the six derived dwords and every reject that changes the compared memory; the client writes only AL, so the final length-gate status is unobservable in the void/memory channel (declared gap, synthetic tests cover it); `allowedImageReads` are the CRT cookie, memcpy flag, a compiler dispatch table and error-format strings |

These are the only CASC functions for which the five-stage word "verified" is
earned. The rest of the container is not.

## B. Read and install-tested, not harness-verified

| area | client address | what is proven today |
|---|---|---|
| `.build.info` row/bool select | `0x14248f2a0`, `0x1424aec30`, `0x1424d8800` | the rules are read-cited and pinned on synthetic text; no install test |
| keyring parse/byte order | `0x1424cebd0`, `0x14248f040`, `0x14248efd0` | read-cited; synthetic byte-order test; the build keyring is read from the install at runtime |
| bucket hash and segment/archive selection | `0x142501b40`, `0x14250e100`, `0x14250e720`, `0x14250e400`, `0x14250c8f0` | the fold is matched on 1,147,132 real keys (salt 0) and 1,488 archive-header placeholders (salt 1); the scan keeps the highest segment per bucket; the archive is `(packed + 0x1e) >> 30`; install-tested, no harness unit (stateful/I/O) |
| index entry search, key-state intervals and resolve | `0x1425111a0`, `0x142511540`, `0x1425115d0`, `0x142520590`, `0x142520700`, `0x1425208d0`, `0x142520c60`, `0x1425209a0` | the client's lower-bound walk, equal-range expansion, sink decode order, interval merge and resident check are implemented; every equal group of all 1,148,620 install entries was range-checked, every salt-0 payload request `(0x1e, size - 0x1e)` resolves status 0 to `packed + 0x1e`, the 1,488 placeholders return status 2, and the 806 duplicate pairs are covered by the second entry's type-6 interval; synthetic tests cover types 1-7, gaps, i32 wrap and all statuses |
| page table layout, hash and lookup | `0x14250fcf0`, `0x1425111a0`, `0x1425209a0`, `0x142513230` | parsed at the client's offset with 21 records per 0x200 page; a record is a u32 hash + key9 + location5 + size4 + type1; the stored hash is `DynamicIndexKeyHash(payload, 0x13, 0)` OR `0x80000000`; the client scans pages and records linearly and stops at the first zero hash. Synthetic non-zero tables cover first/middle/last, type 1/2/3/6, a zero sentinel, a same-key collision and page counts one/many; all 2,560 install pages have zero hashes, so the branch is dormant there |
| archive page cache and `DynamicContainerRead` | `0x1424f48d0`, `0x1424fcd20`, `0x1424fbf00`, `0x1424f8790`, `0x1424faa20`, `0x1424fb1c0`, `0x1424fa440`, `0x1424fa540`, `0x1424fadf0`, `0x1424f9bf0` | read operation for operation: 16 slots of 0x10000 bytes from the `.cmf` open params (`+0x78 = 0x10`, `+0x7c = 0` -> default 0x10000), key `(archive ordinal, page-aligned archive offset)`, no size threshold (the choice is offset/length alignment only), head/middle/tail plan, one direct whole-page run (non-blocking flag 1) and page-served head/tail, whole-page loads, a hit requires the stored length to cover the fragment, the least-recently-used usable slot is reloaded with the lowest index on a stamp tie, a short page zero-fills and reports the short count, an I/O error is status 8 and resets the slot, and status 10 is an archive above 0x3fe or a range crossing a 2^30 boundary. `Store::read_payload` is the client's `(packed + 0x1e, size - 0x1e)` range and `open_ekey` decodes it; `read_stored` is the full-record wrapper for the tests and adds the 30-byte data header with one un-cached read that the client never performs. Install test: small 33,885/33,855 B (stored/payload) is one page read then a cache hit, large 248,182/248,152 B is exactly head page + direct whole-page run + tail page, and the 16 slots still hold 16 after 17 distinct pages with the first evicted. Synthetic tests cover aligned bypass, hit, eviction/reuse, short page, zero length, I/O error and a closed table. No harness unit: the pure plan leaf passed 364 differential cases but its refcount-handle block leaves coverage at 3/6, and the cache itself is lock/I/O state |
| bucket index segment parse/load | `0x14250fcf0`, `0x14250fc60` | read operation for operation: section/file header, section hash, bucket and version/width checks, entry-block bound and rolling hash, page-table offset/minimum and per-record hash, with the client's 5/9 status split. The entry count is `block_size / 18` (floor), so a trailing partial entry is accepted and ignored. The read-only mode-2 path is a synchronous allocator buffer plus one `ReadFile`; the other modes map a read-only view. `ow-casc` reads the same bytes and parses them synchronously. Install-tested on all sixteen files; synthetic tests cover every rejection. Not modelled: writable empty-file creation, the >4 GiB and mode-2 zero-size edges, and the short-read continuation |
| encoding header/real values | `0x142498920` | header verified; installed 16/16 keys, 4 KiB pages, 16025 + 10519 pages and the 1,484,507-byte espec block are pinned |
| encoding section read | `0x1424d1c60` | read operation for operation: the five sections (espec at `0x16`, then ck index `(ckey_width + 0x10) * ck_count`, ck bodies `ck_page_bytes * ck_count`, ek index `(ekey_width + 0x10) * ek_count`, ek bodies `ek_page_bytes * ek_count`), all offsets and sizes in the client's 32-bit wrapping arithmetic and passed as `u32`, allocator alignment `0x10`, and the range rule `offset > len || (offset == len && size != 0)` with a clamped short read reported as status 14 and a rejected request completing through the no-result lookup as status 3 (the first failing section in layout order; the client merges completion order). The espec tail check (`FUN_142498a70`, status 8) and the NUL count are implemented. Install-tested on the 111,058,223-byte blob: 16,025 + 10,519 pages, 1,714,571 ckey entries / 1,714,573 ekeys, 1,714,573 espec entries, 29,694 espec strings, all 26,544 page hashes equal MD5 of their page body. Synthetic tests cover every section name in both range branches and the 32-bit wrap. **No harness unit:** the function allocates through a vtable at `*(info[0]) + 0x18`, submits five reads into the container's I/O engine, and takes the container lock; the Unicorn oracle takes plain-buffer arguments and cannot build the container object, its allocator vtable, the TLS/lock state, or the five queued requests. The pure part is the five-offset arithmetic and no client function exposes it separately. The page lookup is no longer format-derived: `EncodingPageIndexSearch` (`0x142498880`), `EncodingFindCKey` (`0x142498540`) and `EncodingFindEKey` (`0x142498610`) are in section A, `EncodingResolve` (`0x142498700`) is read in section B, and the read path performs no page-hash check. |
| encoding resolve | `0x142498700` | read operation for operation: ready-byte status 1, ckey record's `u32` decoded size, first ekey copy, ekey lookup, ekey `u40` low 32 bits as stored size at `+0x24`, espec pointer through `info+0x58`; the `info+8` multi-ekey path copies the last ekey and returns 7 and is not reached on this install; no harness unit, the two finds prove its parts |
| CDN index file layout and page reader | `0x1424d9af0`, `0x1424d9c90`, `0x1424d9f20`, `0x1424dabb0`, `0x1424dad20`, `0x1424da4c0` | read operation for operation: the 20-byte footer (12-byte header + MD5(header + 8 zeros)[..8]), `pageSize = footer[3] << 10`, `entrySize = footer[4] + footer[5] + footer[6]`, `entriesPerPage = pageSize / entrySize`, `pageCount = (elements - 1 + entriesPerPage) / entriesPerPage` in 32-bit wrapping, `lastKeyBytes = pageCount * keyBytes`, `checksumBytes = pageCount * checksumSize`, and the exact total `checksumSize + 20 + (checksumSize + keyBytes + pageSize) * pageCount`. Tail layout is page bodies, last keys, per-page MD5s, terminal MD5(last keys + checksums)[..checksumSize], footer; page p is `pageSize` bytes at `p * pageSize`; record order is key, size, ordinal (`offsetBytes - 4`), offset, all big-endian. The full-file validator also checks MD5(terminal + footer) against the index's own 16-byte key. Install-tested on all four config-named indexes: archive group 260,173,908 B / 63,149 pages / 9,914,328 entries, patch group 48,616,028 B / 11,800 / 1,923,358, file index 576,828 B / 140 / 28,374, patch file index 90,668 B / 22 / 4,426, every page MD5 matching. The CDN client range-reads the 20-byte footer and the tail and computes full paths via `CdnPathFormat`; no decompiled call site for a per-page HTTP request was found, so `fetch_index_page` is the layout's page span, not an observed request shape. **Not found:** a last-key page-selection search (section D) |
| BLTE frame header and N/Z/E/4 blocks | `0x1424a90e0`, `0x1424d61b0`, `0x1424d6e50` | the client's parse order, `0x0F`/`0x10` records, `12 + record_size*count`, the optional header-MD5 check, the chunk table, the `E` header and Salsa20, and the `N`/`Z`/`4` dispatch are implemented. Synthetic tests cover every reject, both frame types, LZ4, encrypted LZ4 and multi-chunk walking. A one-off full install scan found 1,146,326 unique salt-0 keys: 1,146,325 type-`0x0F` containers, one known non-BLTE orphan, 1,714,597 chunks split `N`/`Z`/`E` = 284,878/1,386,658/43,061, and no `0x10`, direct container or outer `4` block. The install's 63-key keyring lacks the `E` keys (six distinct ids in the 32nd-key sample), so their inner modes are unreadable and an inner `4` cannot be ruled out. The committed test samples every 32nd key and decodes 34 records. Salsa20 itself is harness-verified; the container walk is not |
| `.cmf` header/record/flags | `0x141fda1f0`, `0x141feff80`, `0x141fef610`, `0x141fd4a50` | the flag chain and the store/bundle branch open 305/305 map headers |
| bundle entry lookup | `0x141fd5820` (ow-tank) | `fn/BundleLookup`: verified, 100%, 12/12 |
| bundle directory lookup | `0x141fd5160` (ow-tank) | `fn/BundleDirectoryLookup`: verified, 93.3% with the loaded-bundle false edge as a named gap |
| build-154504 key/IV | `0x142e48a20`, `0x142e48700` | differential + mutation (364 cases each); coverage gate 0, acknowledged gap; not a five-stage verdict |

## C. Ported or reference-derived, not client-read

| area | source | status |
|---|---|---|
| CDN routes/fetch/cache | TACTLib `HttpCDNClient.Fetch` / `ICDNClient` | a port; 12 offline mock tests, no client address, no real endpoint test |
| CMF builds 154088/150851 | TACTLib `ProCMF_154088`; no test for 150851 | ports; only 154504 has differential units |
| config text parse | no client address | format-derived; no test against the install's real config files |

## D. Open

| area | client address | state |
|---|---|---|
| CDN index page selection | none found | **Open read item.** The handler reads `indices/<hash>.index`, validates it (`0x1424d9af0`, `0x1424d9c90`, `0x1424d9f20`) and caches it, but no function in the image was found that chooses a page from the `pageCount * keyBytes` last-key array for a lookup key. Searched: every `memcmp(...,16)` caller (317 functions, 25 with a loop), every function containing the footer fields `0x14/0x1c/0x20/0x24` (the parsers only), every `SHL ...,4` function with a `0x24` reference, the index handler classes at `0x1424e7cb0`/`0x1424e8db0`/`0x1424d1f40` and the index builder at `0x1424f04f0`/`0x1424f0920`. The downloaded index is passed to a callback (`0x1424b0850` → `0x1424bbe60`) whose consumer was not found; the page-selection search may live in the launcher/agent rather than `Overwatch.exe`. The Rust selection is therefore still reference-derived and must not be called read. |
| page-table writer | `0x142511d20`, `0x142511c90`, `0x142520cc0`, `0x1426a8ce0` | read operation for operation: 24-byte record at `page * 0x200 + record * 0x18`, first zero hash, payload-then-hash with a full fence, the type/range status machine (0/1/2/4/`0xa`/`0xc`/`0xf`), the eight-page flush at record 20 with `page & 7 == 7`, and the one-retry segment creation above it. **Not implemented:** all five type-3 call sites pass a null range and write location/size from uninitialised stack slots, the writer is allocator and I/O state, and the read path never calls it; all 2,560 install pages are zero, so the positive path is synthetic-only |
| content key = MD5(decoded) | none | measured 300/300; `blte` now has MD5 for the frame header, but the content-key relation has no code or test |

## E. What the docs got wrong (corrected in this pass)

- `docs/formats/casc.md` once said LZ4 mode `4` was implemented when it was
  not. The client's `BlteBlockReadLz4` (`0x1424d6e50`) is now implemented and
  synthetic-tested; no asset on this install uses it.
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
- The page-table writer was listed as the first open read item. It is now read
  operation for operation and documented in section D. It stays unimplemented
  for two reasons: all five type-3 call sites pass a null range and write
  location/size from uninitialised stack slots, and the writer is allocator and
  I/O state that the read path never calls. "Read" is not "implemented", and
  the row says so.
- The segment load was called asynchronous. It is synchronous: mode 2 opens
  the file with `GENERIC_READ` and does one `ReadFile` into an allocator
  buffer; every other mode maps a read-only view. Both paths share the parse
  body. `ow-casc` is a read-only reader, so it follows the mode-2 path with
  `fs::read`; there is no async API to add.
- The entry block's count is `block_size / 18` with no remainder check. The
  old parser rejected a non-multiple block size, which the client accepts; the
  check is removed and a synthetic partial-entry test pins it.
- The encoding page reader was called format-derived. The section *layout* is
  now read operation for operation from `EncodingSectionsRead`
  (`0x1424d1c60`) and install-tested over the whole blob, including the
  client's 32-bit wrapping arithmetic and the espec tail check. The *lookup*
  (binary search, record scan, page-hash check) is now read too: the search is
  `EncodingPageIndexSearch` (`0x142498880`), the two record scans are
  `EncodingFindCKey`/`EncodingFindEKey`, and the resolve is `EncodingResolve`
  (`0x142498700`). The read path performs **no page-hash check**; the stored
  16-byte hash is only the search stride's padding. The first sampled record
  also corrected the two sizes: the ckey record's `u32` is the decoded size
  (524,304 here) and the ekey record's `u40` low 32 bits are the stored size
  (3,530 here), not the other way round. The section reader itself still has
  no harness unit because it is allocator and I/O state: there is no
  plain-buffer signature for the Unicorn oracle to call.
- `DynamicContainerRead` was said to have no page cache. The page reader and
  its 16-slot/0x10000-byte cache are now read operation for operation  (`0x1424fcd20`, `0x1424fbf00`, `0x1424f8790`, `0x1424faa20`,
  `0x1424fb1c0`, `0x1424fa440`, `0x1424fa540`, `0x1424fadf0`,
  `0x1424f9bf0`), implemented in `crates/ow-casc/src/pagecache.rs`, and
  install-tested. The geometry comes from the `.cmf` database open params, not
  the index header. `Store::read_payload` now performs the client's exact
  `(packed + 0x1e, size - 0x1e)` range through the cache; `Store::read_stored`
  keeps its full-record contract by adding the 30-byte data header with one
  un-cached read, which the client never does. The old cross-archive loop was
  replaced by the client's status-10 rejection for a range whose first and last
  bytes are not in the same archive.

- The CDN index footer was called a 12-byte footer with the checksum size
  searched in 8..=16 and `pageCount` derived from the footer's position. The
  client's `0x1424d9af0` reads a fixed **20-byte** footer, takes the checksum
  size from `footer[7]`, and derives `pageCount` from the element count:
  `ceil(elements / (pageSize / entrySize))`. The footer also carries
  `MD5(header[0..12] + 8 zero bytes)[..8]`; the full-file validator
  `0x1424d9c90` checks the terminal `MD5(last keys + checksums)`, the total
  length, and `MD5(terminal + footer)` against the index's 16-byte key. The
  reference's position-based formula happened to equal the client's on these
  four files; it is not the client's expression and is gone.

## Next order

The segment parse/load and the page-table writer are no longer read items;
section B records the parse/load, and section D records the writer with the
reason it stays unimplemented. The encoding section read and the archive page
cache (`DynamicContainerRead`'s `0x1424f48d0`/`0x1424fcd20` path) are now in
section B; the encoding page lookup and the CDN index footer are in section A.
The remaining read order is:

1. **The CDN index page-selection search.** The footer, tail, page offset,
   entry stride and every validator are read. No function in the image was
   found that takes a key and walks the `pageCount * keyBytes` last-key array;
   see section D for the searches. Until it is found, the Rust selection stays
   reference-derived and this row stays open.

What is still not tested is listed where it belongs: the page-table writer is
read but not implemented (section D: null ranges and uninitialised stack
fields at every type-3 call site, and no read-path caller); the CDN index
page selection is open as above; the CDN HTTP transport is a TACTLib port with
no real endpoint test (section C); BLTE `0x10` is absent from this install, no
outer `4` block was observed, and the `E` inner modes are unreadable because
the 63-key keyring lacks the keys; the CDN index `fn/CdnIndexFooterParse` unit
declares the final length-gate status as an unobservable gap because the
client writes only AL; and install measurements that are not test-pinned stay
measurements.

The page-cache plan (`PageReadPlan`, `0x1424faa20`) is a pure leaf, but its
unit cannot earn the five-stage word: the same function publishes the
refcounted archive handle, and with the null handle the oracle can construct
the three refcount branches never execute (coverage 3/6; a branch that never
ran cannot be declared dead). The unit was removed rather than shipped with a
weakened gate.

## Closing a row

A row moves to verified only when a unit names the current-image address, uses
a `candidateCrate: "ow-casc"` probe, and passes all five harness stages. A row
may instead be recorded as a port or an open question; it may not simply
disappear.
