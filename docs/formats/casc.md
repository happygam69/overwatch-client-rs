# The CASC container

How the install stores bytes, and how `ow-casc` gets them back. Every claim
here is checked against the retail install (`2.25.0.1.154504`) and, where
marked, against the client's own functions in `Overwatch.exe`.

## Layout

```text
<install>/.build.info
<install>/_retail_/.flavor.info
<install>/data/casc/config/<xx>/<yy>/<key>   build, CDN, patch and keyring configs
<install>/data/casc/data/<bucket:02x><segment:08x>.idx  bucket index (one segment per bucket)
<install>/data/casc/data/data.NNN             stored records
<install>/data/casc/indices/<hash>.index      CDN routes, not local locations
<install>/data/casc/ecache/*.idx              cached chunk manifests, not content
<install>/data/casc/pro/                      v8 `.idx` files; no data, not content
```

`config/` files are addressed by their own key, two directory levels deep:
`config/<key[0..2]>/<key[2..4]>/<key>`.

The current install has 93 `data.NNN` archives, 16 bucket index files and an
encoding file of 109,882,544 stored / 111,058,223 decoded bytes.

**Only `data/` is local content.** `ecache/` records are a 30-byte header plus
a BLTE frame header with no payload; `indices/` is the CDN's archive map (its
archive ordinals are not local pack numbers); `pro/` has no `data` archives.
Reading any of them as content produces a plausible failure.

## `.build.info`

A header row names the columns, then one row per branch. Column names carry a
`!TYPE:WIDTH` suffix to strip (`Branch!STRING:0`, `Build Key!HEX:16`, ...).
Relevant columns: `Branch`, `Active`, `Build Key`, `CDN Key`, `KeyRing`,
`Version`, `Product`.

Two directory shapes are supported:

- **install root:** `.build.info` and `data/casc` in the same folder;
- **game folder:** `_retail_` with `.flavor.info`; `.build.info` and `data/casc`
  are one level up.

`BuildInfoLoadFromInstall` (`0x14248f2a0`) builds `<install>/../.build.info`
when `.flavor.info` exists. `BuildInfoSelectRow` (`0x1424aec30`) picks the
**first active row whose Product equals the flavor**. With no flavor it
requires every row's Product to be equal and returns the last active row.
`Product` is at row offset `+0`, not `Branch` at `+8`; the flavor is `pro`.
`ParseBoolString` (`0x1424d8800`) accepts `1`, `true`, `yes`, `on`, `enable`,
`enabled` (case-insensitive) as true.

## Config files

`key = value value ...` lines; `#` starts a comment. Values are lists.

Build config:

```text
root = <content key>
encoding = <content key> <encoding key>
encoding-size = <decoded size> <stored size>
build-name = prometheus-...
build-number = ...
build-product = Prometheus
```

`encoding` carries two keys: the content key first, the encoding key second.
Only the encoding key names a stored artifact. `BuildConfigParse`
(`0x1424ccc00`) stores them at build-config `+0x1a0` and `+0x1b4`;
`ow-casc` tries the encoding key first and treats the other as a fallback.

CDN config carries `archives`, `file-index`, `archive-group` and the patch
variants. There is no `key-layout-*`, so the bucket index is the path.

### CDN index file

An `indices/<hash>.index` is read by the client's own handler, not only by the
reference:

| offset | field |
|---|---|
| last 20 B | footer, below |

The footer is a fixed **20 bytes** at `len - 20`:

| footer byte | meaning |
|---|---|
| `0` | version, 0 or 1 |
| `1`, `2` | zero |
| `3` | page size in KiB (`pageSize = byte3 << 10`) |
| `4` | offset-field width |
| `5` | size-field width |
| `6` | key width (16) |
| `7` | checksum width, 0..16 |
| `8..12` | element count, `u32` little-endian |
| `12..20` | `MD5(footer[0..12] + 8 zero bytes)[..8]` |

`CdnIndexFooterParse` (`0x1424d9af0`) copies the footer, saves the hash, zeroes
`out[0xc..0x14]`, hashes the 20 bytes and compares the first 8. On a match it
restores the hash, accepts version 0/1 and checksum width 0..16, then writes:

```text
out+0x14  pageSize        = footer[3] << 10
out+0x18  entrySize       = footer[4] + footer[5] + footer[6]
out+0x1c  entriesPerPage  = pageSize / entrySize
out+0x20  pageCount       = (elementCount - 1 + entriesPerPage) / entriesPerPage
out+0x24  lastKeyBytes    = pageCount * keyBytes
out+0x28  checksumBytes   = pageCount * checksumSize
```

and accepts only when the file length is exactly

```text
checksumSize + 20 + (checksumSize + keyBytes + pageSize) * pageCount
```

with every step in 32-bit arithmetic. The function returns 8 in `AL` for a
reject; callers test `AL` only (`fn/CdnIndexFooterParse`).

The file is laid out as:

```text
page bodies    page p at p * pageSize, pageSize bytes each
last keys      pageCount * keyBytes, the last key of each page
checksums      pageCount * checksumSize, MD5(page body)[..checksumSize]
terminal       checksumSize bytes, MD5(last keys + checksums)[..checksumSize]
footer         20 bytes above
```

`CdnIndexFileParse` (`0x1424d9c90`) checks the terminal checksum and then
`MD5(terminal + footer)` against the index's own 16-byte key (the file name).
`CdnIndexPageChecksums` (`0x1424d9f20`) compares every page's MD5 with its
stored checksum; a mismatch is ignored. Records are `[key keyBytes][size
sizeBytes BE][archive ordinal (offsetBytes - 4)][offset 4 BE]`; a loose index
has `offsetBytes = 0` and stops after the size. The main group has
`offsetBytes = 6` (u16 ordinal), the patch group 5 (u8).

The install's four config-named indexes, read with this formula:

| index | file bytes | offB | stride | entries/page | pages | entries | last-page entries |
|---|---|---|---|---|---|---|---|
| `archive-group` | 260,173,908 | 6 | 26 | 157 | 63,149 | 9,914,328 | 92 |
| `patch-archive-group` | 48,616,028 | 5 | 25 | 163 | 11,800 | 1,923,358 | 121 |
| `file-index` | 576,828 | 0 | 20 | 204 | 140 | 28,374 | 18 |
| `patch-file-index` | 90,668 | 0 | 20 | 204 | 22 | 4,426 | 142 |

All four match the length formula exactly, the footer hash validates, the
terminal hash matches, and every page MD5 matches its stored checksum
(`crates/ow-casc/tests/cdn_index_real.rs`).

### Page selection and the per-page read

The last-key array is walked by `CdnIndexPageSelect` (`0x1424da730`, 469 B),
a method of the index object whose footer fields sit at `index + 0x58`. The
selection is a lower bound with an early equal exit, and the record scan
follows in the same call:

```text
keyBytes  = *(u8 *)(index+0x5e)      pageCount = *(i32 *)(index+0x78)
lo = 0; hi = pageCount; page = 0
while (lo != hi) {
  mid = (hi + lo) / 2
  cmp = memcmp(*(u64 *)(index+0x88) + mid*keyBytes, key, min(keyLen,keyBytes))
  if (cmp < 0) lo = mid + 1
  else { hi = mid; if (cmp == 0) { page = mid; break; } }
  page = lo
}
if (page == pageCount) return 6                  // also pageCount == 0
status = CdnIndexPageResident(index, page); if (status) return status
entriesPerPage = *(i32 *)(index+0x74)            elementCount = *(i32 *)(index+0x60)
count = min(entriesPerPage, elementCount - entriesPerPage*page)
for i in 0..count:
  rec = (*(u64 *)(index+0x98)) + entrySize*i
  if memcmp(rec, key, min(keyLen,keyBytes)) == 0 -> found
size   = BE(rec + keyBytes,  *(u8 *)(index+0x5d))   accumulated in 32 bits
offset = BE(after size,     *(u8 *)(index+0x5c))   accumulated in 64 bits
if (out_key16) copy min(keyBytes,16), zero-fill to 16
return 0
```

The return is `AL` only: 0 found, 6 not found, otherwise the page-resident
status. The early equal exit can return a later page than a pure lower bound
when adjacent last keys are equal; on this install's two group indexes no
adjacent last keys are equal (74,949 checked by a one-off scan, not a committed
test), so the two agree there. The
first record whose `min(keyLen,keyBytes)` prefix matches wins; there is no
equal-range expansion.

`CdnIndexPageResident` (`0x1424da590`) makes one page current. In whole-file
mode (`index+0x38 == 0`) the page pointer is `*(u64 *)(index+0x50) +
page*pageSize`, with no MD5 check. In the footer-only/streamed mode it reads
the page by HTTP range (`0x142522870` builds the request; the WinHTTP read is
`0x142521d70`), requires the full `pageSize`, and rejects the page with status
8 when `MD5(page)[..checksumSize]` differs from the stored per-page checksum
at `index+0x88 + lastKeyBytes + page*checksumSize`. The scratch page is the
extra `pageSize` bytes after the tail. **This is the per-page HTTP request the
earlier note said was not found**; it is reached only in the footer-only mode.
`ow-casc` reads whole index files, so it takes the whole-file branch; the
streamed branch is read and documented but not implemented by the port.

`CdnResolveLocation` (`0x1424b0cc0`) is the caller: it tries the archive-group
index at `client+0x48` (patch `+0x118`), splits the 6-byte offset into
`ordinal = off >> (shift*8)` and `off & mask`, and copies the 16-byte archive
record from `client+0x108` (patch `+0x1d8`); a miss or a non-load status falls
back to the file index at `client+0x1e8` (patch `+0x290`) with the 16-byte
result key. The batch form `CdnIndexPageSelectBatch` (`0x1424da910`) advances
the page monotonically over sorted keys. The batch wrappers `0x1424b0e50` and
`0x1424b0ef0` are not Ghidra functions; they are read at the expression level
only. The slot-to-config-key mapping (`+0x48` archive group, `+0x118` patch
group, `+0x1e8` file index, `+0x290` patch file index) is a structural
inference, not a read of the config wiring.

Still not read: which function writes the archive shift/mask/count/array
(`client+0xf0..+0x110` and `+0x1c0..+0x1e0`) from the CDN config, and the batch
wrappers operation for operation. The CDN transport itself remains a TACTLib
port with no real endpoint test, but the request shape (range offset and
length, full-page requirement, checksum reject) is now read.

## Keyring

```ini
key-1b3e4e1ecfb25877 = 3de60d37c664723595f27c5cdbf08bfa
```

The name's last 16 hex digits are the key id. The client parses them with
`ParseHexKeyId` (`0x14248f040`) and the value with `ParseHexKey`
(`0x14248efd0`); the BLTE E-block lookup reads the 8 id bytes as a
little-endian `u64`. The byte swap is the address order, not a convention.

The community "support" keyring uses `<id> <key> [1|0]`, space separated,
with the id used as written and no swap. A file is told apart by whether a
line begins `key-`. A correct key registered under the wrong byte order simply
will not be found.

## Bucket index

Sixteen files named `<bucket:02x><segment:08x>.idx`; `0300000159.idx` is
bucket 3, segment 0x159. Header:

| offset | field |
|---|---|
| `0x00` | `i32` unknown (16 in the reference files) |
| `0x04` | `i32` header hash |
| `0x08` | `u16` version, 7 or newer |
| `0x0A` | `u8` bucket number |
| `0x0B` | `u8` extra bytes, 0 |
| `0x0C` | `u8` size field width, 4 |
| `0x0D` | `u8` offset field width, 5 |
| `0x0E` | `u8` key width, 9 |
| `0x0F` | `u8` file-offset bits, 30 |
| `0x10` | `u64` maximum addressable size |
| `0x18` | 8 bytes padding |

`DynamicIndexSegmentParse` (`0x14250fcf0`) rejects `version < 7` and widths
other than 4/5/9; it does not read the extra-bytes or offset-bits bytes. A v8
file with the same widths reads identically.

### Segment load and validation

`DynamicIndexSegmentParse` is reached only from `DynamicIndexSegmentLoad`
(`0x14250fc60`). It has two I/O paths, selected by the container mode that
`DynamicContainerOpenIndex` (`0x142503000`) passes from `DynamicIndexOpen`
(`0x14250e100`):

- mode 2 (the read-only container): the file is opened with `GENERIC_READ`,
  `OPEN_EXISTING`, share mode 7 and `FILE_ATTRIBUTE_NORMAL`. The parser asks
  for the file size, allocates that many bytes, and performs one `ReadFile` at
  offset 0. This is **synchronous**: there is no `FILE_FLAG_OVERLAPPED`, no
  completion port and no callback.
- every other mode: the file is opened read-write (access 6) and a read-only
  view is mapped; the parse body reads the mapped bytes.

Both paths join the same parse body. `ow-casc` is a read-only reader, so its
`fs::read` is the mode-2 path: the same bytes and the same parse. The only
difference is buffer ownership, an allocator buffer versus a mapped view.
There is no async API to add.

The validation order, with the client's status:

1. file size `< 8` -> 9;
2. file size `< section size + 8` (the add is 64-bit) -> 9;
3. `DynamicIndexHash(buffer + 8, section size)` word 0 differs from the stored
   dword at `+4` -> 9;
4. section size `< 0x10` -> 5;
5. header bucket byte at `+0x0A` differs from the expected bucket -> 9;
6. version `< 7`, or the width bytes at `+0x0C`/`+0x0D`/`+0x0E` are not
   4/5/9 -> 5;
7. store the `u64` at `+0x10` as the maximum addressable size (not checked);
8. entry header `(section size + 0x17) & ~0xf` is at or past the file size
   -> 9;
9. remaining `= file size - entry header` is `< 8` -> 9;
10. entry block size (the `u32` at the entry header) `+ 8` exceeds the
    remaining bytes -> 9;
11. entry count `= block size / 0x12` (floor, with **no remainder check**);
    rolling `DynamicIndexHash` over that many 18-byte entries, word 0 differs
    from the stored dword at `entry header + 4` -> 9;
12. page-table offset `= (block size + entry header + 0x1007) & ~0xfff` is at
    or past the file size -> 9;
13. remaining `= file size - page-table offset` is `< 0x7800` -> 9;
14. page count `= remaining >> 9`; for each page and each of its 21 records, a
    zero stored hash stops the scan successfully; otherwise
    `DynamicIndexKeyHash(record + 4, 0x13, 0) | 0x80000000` must equal the
    stored hash or -> 9;
15. status 0 keeps the parsed fields; a non-zero status frees/unmaps the
    buffer and zeroes them.

`ow-casc` returns one `Malformed` error rather than the client's 5/9 status
word; callers branch only on success. For a static file inside the client's
4 GiB bound, acceptance is the same, including the entry-count floor: a
declared block size that is not a multiple of 18 is accepted and its trailing
partial entry is never hashed or decoded.

Not modelled: the writable path creates an empty segment when the file size is
0 (mode 2 with size 0 returns 9); a file with size above `0xffffffff` makes
the client return 0 with zeroed fields before parsing. `fs::read` returns either all bytes
or an error, so the client's short-read continuation after a status-0 read
cannot occur here. A read failure is an `Io` error; the client maps the OS
status (5 -> `0x14`, `0x70` -> 8, otherwise 7) and aborts unless the mapped
status is 0.

The section header's second word is a 32-bit hash of the section body; the
entry block's second word is a rolling hash over the 18-byte entries. The
client verifies both (`DynamicIndexHash`, `0x142512c60`) before using the
segment. `ow-casc` implements the same hash; a one-off scan of the sixteen
`.idx` files matched, while the in-crate test pins one real header and two
rolling states.

### Key to bucket

`DynamicKeyStateLookup` (`0x142501b40`) XORs the nine key bytes into one byte
`b`, computes `((b ^ (b >> 4)) + salt) & 0xf`, locks the per-bucket lock at
`manager + 0xa58 + bucket * 8`, and looks the key up in that bucket's key
state. The function's third argument is the salt. Its three callers —
`DynamicContainerRead` (`0x1424f48d0`), `FUN_1424f2900` and `FUN_1424f4b30` —
all pass 0. `index::bucket_of` is that expression; the other key-state entry
points take the same salt argument and use it for their own inserts.

### Bucket to segment

There is **no container-header field** that names the segment. The earlier
note that the container header supplied it was wrong. On open,
`DynamicIndexOpen` (`0x14250e100`) first tries the sixteen cached segment
numbers in the object at index-manager `+0x68`, table `+0x110`. A cold cache
sends it to `DynamicIndexScanSegments` (`0x14250e720`), which enumerates the
data directory and accepts exactly ten hex digits, a dot and `idx`
(case-insensitive), optionally with the client's `._idx` temporary marker.
The first two digits are the bucket (the parsed byte must be below 16, so the
first digit is `0`), the last eight are the segment. The scan keeps the
**highest** segment number per bucket; in a read-write store it deletes a
lower or temporary file, and in a read-only store it ignores it.
`DynamicIndexSegmentOpen` (`0x14250e400`) opens
`<base>/<bucket:02x><segment:08x>.idx` for the bucket it is asked for and
`DynamicIndexSegmentLoad` (`0x14250fc60`) parses it synchronously. The parser
rejects a file whose header bucket at `+0x0A` disagrees with the bucket the
name selected, so a renamed file cannot silently load.

The `+0x110` table is a runtime cache of the open segment number per bucket,
not a header field: `DynamicIndexOpen` writes it after the scan.
`DynamicIndexSegmentOpen` itself returns 0 when the requested segment is
already open, 1 when the cached number is greater, and otherwise opens the
file. All 21 callers of `DynamicIndexSegmentOpen` pass `table[bucket]` as the
segment number; only the source of `bucket` differs (the key fold or an
explicit bucket argument). The container open loads all sixteen selected
segments (one per bucket); a key lookup lazily opens its own bucket's segment
when it is not already open. The client never opens two segments for one
bucket and never merges entries across files.

A read-only open requires all sixteen buckets. A missing file makes
`DynamicIndexOpen` return 9 (0x12 if all sixteen are missing); a lazy open for
a missing file returns 8 (the OS error mapping) and the key lookup fails. A
missing bucket or segment is an error, with no fallback to another file.

### Entry to archive

Each entry is `key[9] | location[5, big-endian] | size u32 LE`; the location is
`(archive << 30) | offset`. `DynamicContainerRead` adds the 30-byte data
header (`0x1e`) to the packed location; `DynamicContainerReadPage`
(`0x1424fcd20`) shifts it right by 30 to select the archive and masks the
offset with `2^30 - 1`. `DynamicContainerOpenArchive` (`0x14250c8f0`) opens
`<base>/data.%03d` through an archive-number cache and returns the file. A
missing archive is an error; there is no fallback. The shift and mask are set
by the container constructor (`0x1424f3ee0` passes `0x1e`), not read from the
index header's offset-bits byte; `index::Location::data_archive` mirrors the
client's addition. `DynamicContainerReadPage` rejects a range whose first and
last byte are not in the same archive with status 10; `Store::read_payload`
implements that rejection and does not split the range. No entry in this
install crosses a `2^30` boundary, so `(packed + 0x1e) >> 30` equals
`packed >> 30` for every key.

### Archive page cache

`DynamicContainerReadPage` (`0x1424fcd20`) opens the archive and hands the
archive-relative range to the reader object at `container+0xb38`. The reader is
built by `ContainerReaderInit` (`0x1424fbf00`); its page cache is at `reader+8`
and is built by `PageCacheInit` (`0x1424f8790`). This is the cache the old notes
called missing.

**Geometry.** The page cache does not read its geometry from the index header.
The `.cmf` database open params carry the slot count at `+0x78` and the page
size at `+0x7c` (`DatabaseOpenApplyParams`, `0x1424a6df0`, copies the 0xa0-byte
params to `reader+0x58`); the open path at `0x141fd20f0` sets `+0x78 = 0x10`
and `+0x7c = 0` ("use the default"), and `DatabaseOpenConfigureCache`
(`0x1424a7d70`) substitutes `0x10000` for a zero page size. This install
therefore runs **16 slots of 0x10000 bytes** (1 MiB). `PageCacheInit` allocates
one page buffer per slot; the slot record is `0x70` bytes and the page table is
a separate `0x38`-byte object holding the lock, the last-use clock at `+0x20`,
the slot array at `+0x28`, and the closed flag at `+0x30`.

**Key.** A page is keyed by `(archive number, page-aligned archive offset)`.
The archive number is the qword at `handle+0x18` that
`DynamicContainerOpenArchive` (`0x14250c8f0`) stores when it opens `data.NNN`;
it is the archive ordinal, not a new per-open id. The archive-relative offset
is `absolute & reader+0x1c8` (`2^30 - 1`, from `1 << shift` with the shift
`0x1e` at `reader+0x1d0`); the page key is that offset rounded down to the
`0x10000` page. The slot also stores the number of bytes actually read.

**Read plan.** `PageReadPlan` (`0x1424faa20`) splits the range into a head
fragment (offset to its page end), a run of whole pages, and a tail fragment.
Its 0xb0-byte output has the head at `+0x30` (dest, page, len, slot, in-page,
flag), the whole-page run at `+0x58` (flag, page `+0x60`, dest `+0x68`, len
`+0x70`) and the tail at `+0x78` (dest, page `+0x80`, len `+0x88`, slot, in-page
`+0x98`, flag). The cache reader (`PageCacheRead`, `0x1424fb1c0`) then:

1. looks the head page up (`PageSlotFind`, `0x1424fa440`) and copies the
   fragment on a hit;
2. on a head miss loads the whole page (`PageLoad`, `0x1424fa540`);
3. reads the whole-page run with **one direct read** because the cache's
   non-blocking flag (`cache+0x10`, set to 1 by the open path) is set;
4. looks the tail page up and loads it the same way, but only when the
   whole-page run was read completely.

The completion (`PageLoadComplete`, `0x1424fadf0`, and
`PageSlotPublish`, `0x1424f9bf0`) copies `min(fragment_len, stored - in_page)`
bytes and zero-fills the rest of the fragment, so a short page reports the short
count rather than inventing bytes. A range whose offset and length are both
page-aligned never enters the cache: `ArchiveRead` (`0x1424f9fe0`) sends it
straight to the archive handle. A zero length returns 0 before any I/O. The
middle is direct, not page-by-page, on this install: the non-blocking flag is 1,
so only the head and tail fragments are served from the cache.

**There is no read-size threshold.** The decision is purely the offset and
length alignment: a one-byte unaligned read loads its whole 0x10000-byte page
and copies one byte; a multi-megabyte unaligned read is one head page load, one
direct whole-page run and one tail page load. A page-aligned, page-multiple
read of any size is one direct read and never touches the cache.

**Hit and eviction.** `PageSlotFind` scans the fixed table. A usable slot whose
key matches *and* whose stored length covers `in_page + len` is a hit; its stamp
is set to the incremented table clock. A miss — including a key match whose
stored length is too short, e.g. the last page of a file — takes the usable slot
with the smallest stamp (the largest `clock - stamp`, ties to the lowest index),
bumps its stamp, and reloads it. Slots marked busy/failed (`+0x2e != 0`) are
never evicted. There is no free list and no "LRU" in the general sense: it is
this exact minimum-stamp scan over 16 fixed slots. The client also keeps a
per-table lock and a per-slot event and can wait for another thread's in-flight
page; a single-threaded reader cannot observe those states, and `ow-casc` keeps
the surviving state (key, buffer, stored length, stamp) synchronously.

**Maintenance.** `PageCacheClose` (`0x1424f9eb0`) sets the closed flag and
invalidates every populated slot; `PageCacheReopen` (`0x1424f9f90`) clears it;
`PageCacheInvalidateAll` (`0x1424fab50`) and `PageCacheInvalidateRange`
(`0x1424fabd0`) do the same without the flag. The read path calls
`FUN_1424fff70` (`0x1424fff70`) and these helpers only when the container mode
is not 2; this install's read-only mode 2 skips the whole block, so the cache is
never closed or flushed during a read here. The cache is also filtered by an
archive-number set (`0x1424fcc80`) only in non-mode-2 opens; mode 2 skips that
check too.

**Statuses.** A page or direct read reports 0 with the bytes actually read; a
short read at the archive's end is not an error. A read failure is status 8 at
`DynamicContainerReadPage` and the slot returns to the empty usable state.
`DynamicContainerReadPage` itself returns 10 for an archive above `0x3fe` or a
range whose first and last byte are not in the same archive; it does **not**
split a range at a 2^30 boundary. `DynamicContainerRead` returns the lookup
status (0/2/3/4/...) or the page status; on 0 with a short count it logs
`dynamic_container.read_truncated.count` and records the short range. No entry
in this install takes that path.

**Install proof.** `crates/ow-casc/tests/page_cache_real.rs` opens the real
store, resolves a small record and a record larger than two pages, and compares
`Store::read_payload` with a direct archive read:

- small record: stored 33,885 B, payload 33,855 B, first read is exactly one
  whole-page read at the page-aligned offset, second read issues no archive I/O;
- large record: stored 248,182 B, payload 248,152 B, exactly three reads — head
  page, one direct whole-page run, tail page — in that order;
- after 17 distinct pages the 16-slot table still holds 16, the first page is
  gone, and re-reading it is a miss that loads the page again.

The synthetic tests in `pagecache.rs` pin every branch: an unaligned read and a
hit, an aligned bypass, a head/middle/tail range, 17-page eviction and reuse, a
short page that is not a hit and is zero-filled, a zero-length read, an I/O
error that resets the slot, a closed table, and the plan fields at every
boundary.

**No harness unit.** The plan arithmetic is a pure leaf and a unit was built
(`PageReadPlan` at `0x1424faa20`): it passed determinism and the 364-case
differential, but it cannot earn the five-stage word. The same function also
publishes the refcounted archive handle; with a null handle (the only kind the
oracle can construct) the three refcount-block branches never execute, so the
coverage gate is 3/6 reachable branches, and a branch that never ran cannot be
declared unreachable. Reaching it needs a live C++ refcount object and, on
release, its vtable slot. The unit is therefore not shipped; the plan is pinned
by the Rust unit test that parses the same `plan_read_into` bytes, and the
cache itself is locking and I/O state with no plain-buffer oracle shape.

### Proof on this install

One pass over all 1,148,620 entries of the sixteen files:

| class | entries | rule |
|---|---:|---|
| real keys | 1,147,132 | `bucket_of(key, 0)` equals the file's header bucket |
| archive-header placeholders | 1,488 | `bucket_of(key, 1)` equals the file's header bucket |
| unmatched | 0 | — |

The 1,488 placeholders are 93 archives × 16 slots: a 30-byte record at
`offset = 30 * bucket`, `key[2..4] == 00 28`, and one fixed five-byte suffix.
They sit in the first sixteen 30-byte slots of each archive, not at
hash-selected buckets, and they match the formula with salt 1. The read path's salt
is 0, so it never looks them up; the two salt domains are not a reason to
change the lookup formula. Separately, 806 keys appear twice in one bucket —
one entry at the 30-byte header, one at the payload (`offset + 30`,
`size - 30`). The client chooses the first (header) entry; `SegmentEntries`
keeps the file order and returns it, and the second entry is the same stored
record seen at its payload.

The bucket hash and the segment/archive selection are inline and stateful (a
typed manager plus a directory scan), so they have no five-stage differential
unit. The proof is the install test
`crates/ow-casc/tests/bucket_hash_real.rs` (all keys, all archive numbers,
selected segment numbers) and the synthetic tests in `index.rs` and
`container.rs` (the fold, the filename parser, the highest-segment scan,
two-segment selection, no cross-bucket merge, and a missing bucket).

The entry search is also stateful: `DynamicIndexFindEntry` builds a decode
context with an allocator, reads an optional page table and calls
`DynamicIndexDecodeEntries`; it is not an isolated pure function. The
install test `crates/ow-casc/tests/index_find_real.rs` walks every entry in
the selected segments: `find_range` equals each key's contiguous equal group,
all 1,147,132 salt-0 entries resolve to the first entry of their group, the
1,488 salt-1 placeholders resolve to nothing, and each of the 806 duplicate
pairs resolves to the header entry whose stored record is the 30-byte header
followed by BLTE. The same pass asks `Store::resolve_range` for every key's
client request `(0x1e, size - 0x1e)`: all 1,147,132 salt-0 keys return status
0 with the absolute range `(packed + 0x1e, size - 0x1e)`, all 1,488 salt-1
placeholders return status 2, and each duplicate request starts at the second
entry's packed location and ends at the header entry's end — the second
entry's type-6 interval. It also parses all sixteen page tables: 2,560 pages,
53,760 record slots, 0 non-zero stored hashes, first zero at page 0 record 0,
and no page-table key resolves. `Store::entry_count` is 1,148,620;
`Store::key_count` is 1,147,814 because 806 keys are listed twice. The pure
search is checked against a linear first/last scan on a deterministic sorted
corpus in `find_range_agrees_with_a_linear_scan`; the page-table parser and
lookup are checked by `page_table_parses_and_selects_in_page_order`,
`a_zero_page_hash_stops_the_scan`, `a_page_record_hash_must_match_its_payload`,
`a_short_page_table_is_rejected`, `page_collision_uses_decode_order` and
`page_counts_one_and_many_share_the_lookup_order`.

### Entry search and duplicates

`DynamicIndexFindEntry` (`0x1425111a0`) lower-bound searches the segment's
18-byte entries, which are in non-decreasing 9-byte key order. It starts with
`lo = 0`, `hi = count`; each step uses `mid = (lo + hi) >> 1` and
`memcmp(key, entry + mid * 18, 9)`. A smaller key moves `hi` to `mid`, a
greater key moves `lo` to `mid + 1`, and equality records `mid`, expands
backward and forward over the equal keys, and stops. It then decodes
`[first, one-past-last)`; an absent key yields an empty range at its insertion
point. `DynamicIndexDecodeEntries` (`0x1425209a0`) decodes the 5-byte
big-endian location and 4-byte little-endian size; the first equal entry is the
chosen one, while later equal entries are only appended to the key state's
interval buffer. `DynamicIndexResolveRange` (`0x1425115d0`) reads the chosen
entry, adds the caller's requested offset and returns the location and size;
`DynamicIndexResolveKeyRange` (`0x142511540`) is the wrapper called by
`DynamicKeyStateLookup`. It returns 0 on success, 2 when no entry matches,
3 when the requested range is not resident and 4 when the range runs past the
entry. `DynamicIndexFindEntry`'s two optional output words are the page index
and the record index reached by the page scan; both are 0 when the scan stops
at the first record, and `DynamicIndexResolveRange` passes 0 for both, so the
resolve path does not consume them.

On this install's 806 duplicate pairs the first entry is at the 30-byte data
header and the second at `offset + 30` with `size - 30`. The client chooses the
header entry; `DynamicContainerRead` then adds `0x1e` to reach the payload, so
the resolved bytes are the same BLTE as the second entry. `SegmentEntries` in
`index.rs` keeps the file order and returns the first entry of the equal range;
it does not sort or de-duplicate.

The key-to-entry page table that follows the entry block starts at
`(entry_block_size + entry_header + 0x1007) & ~0xfff`. The client requires at
least `0x7800` bytes and derives `page_count = (file_size - offset) >> 9`.
Each 0x200-byte page holds 21 records at a 24-byte stride; the last 8 bytes are
pad. A record is a `u32` stored hash followed by a 19-byte payload — the
9-byte key, the 5-byte location, the 4-byte size and one type byte — then a pad
byte. `DynamicIndexKeyHash` hashes the 19-byte payload at `record + 4`, and the
stored dword is `hash | 0x80000000`. This is a production caller of the
verified `DynamicIndexKeyHash`, not a dead function; the page writer
`DynamicIndexPageInsert` (`0x142511d20`) computes the same hash when it
appends a record.

`DynamicIndexFindEntry` does **not** select a page from the key. It scans every
page and every record in file order and stops at the first record whose stored
hash is zero; the zero is the empty-slot sentinel, not a valid record and not a
per-page header. It collects matching record pointers and decodes them after
the 18-byte entry range. A page hit and a sorted-entry hit decode the same
5-byte location and 4-byte size; the difference is the page record's type byte
and the decode order. The type byte is passed to `DynamicIndexEntrySink`
(`0x142520590`); `ow-casc` models every type (see "Key-state intervals and
resolve" below): 1 and 2 replace the chosen entry, 3 clears it, 4 appends the
current chosen entry as a resident interval, 5 clears the intervals, and 6/7
append the record's interval resident/non-resident. The page-table writer is
`DynamicIndexPageInsert` (`0x142511d20`); see "Page-table writer" below. The
write wrapper at `0x1425033e0` passes type 6 or 7, and the read path never
supplies a page type on this install because every page table is empty.

On this install every page table is dormant: 2,560 pages, 53,760 record slots,
**0** non-zero stored hashes, and the first zero is page 0 record 0 in every
one of the sixteen files. The client enters the branch because the page count
is non-zero (144-176 pages per file; 152 for `0300000159.idx`) and stops at the
first record, so no key resolves through the page table here. The synthetic
tests build non-zero tables and exercise first/middle/last records, type
1/2/3/6, a zero sentinel and page counts one and many.

### Page-table writer

`DynamicIndexPageInsert` (`0x142511d20`) is the only writer for this table; the
wrapper at `0x142511c90` forwards its five arguments, returns the insert's
status, and then calls `FUN_142520f50` (`0x142520f50`) with zero and the
context's word at `+8`; that call is a no-op because the logger acts only on 1
or 2. The arguments are the find context, the 9-byte key, the record type, an
optional range `{u64 start, u32 len}` and an optional 16-byte out-entry. With a
type other than 3 the range is required; a null range with any other type
returns `0xa` before any state is touched. The function runs the same
`DynamicIndexFindEntry` walk as the read path into a fresh find context, and
the record index it returns is the first zero stored hash in page/record order,
or 21 when no slot is free. There is no free list and no per-page header, so
records fill the table sequentially.

If the table is full (record index 21, the find function's `0x15` sentinel),
the writer frees the find context and returns `0xf`; the out-entry is not
written. Otherwise it copies the chosen entry's 16 bytes to the out-entry
first, before the range check and even when the insert later fails. A null
out-entry is allowed. The chosen entry is the one the read path would use: the
entry-block copy for a key with equal entries, or the page record when no
entry-block copy exists.

A record sits at `page * 0x200 + record * 0x18`. Its fields are the stored
hash (u32, little endian), the 9-byte key, the 5-byte big-endian absolute
location, the 4-byte little-endian size, the type byte and one pad byte. The
absolute location is `chosen.location + range.start` and the size is
`range.len` truncated to 32 bits. The writer stores the payload first, hashes
exactly those 19 bytes with `DynamicIndexKeyHash(record + 4, 0x13, 0)`, sets
bit 31, executes a full fence and stores the hash last; the pad byte at
`record + 23` is neither written nor hashed. The fence orders the payload
before the hash that publishes it.

`FUN_142520cc0` (`0x142520cc0`) is the insert gate and is called with the find
context, the type, the absolute range and a one-byte no-write flag that starts
clear. It applies the read path's chosen-entry and interval state but answers
with a status instead of appending intervals: type 1/2 returns `0xc` when the
key is already chosen; type 3 returns 2 when it is not chosen; types 6/7 return
2 when it is not chosen, 4 when the absolute range leaves the chosen entry, and
set the no-write flag when the range is already fully resident (6) or fully
non-resident (7). Any other type returns 1. A non-zero status is returned
unchanged; a set no-write flag returns 0 without a record.

The status word is therefore: 0 written, or already in the requested state;
1 unknown type; 2 no chosen entry for type 3/6/7; 4 range outside the chosen
entry; `0xa` invalid arguments; `0xc` a type-1/2 key that is already present;
`0xf` table full. Every insert appends; there is no in-place update, so an
older record with the same key stays where it is, page-record order is append
order, and an equal entry-block range still wins the read path's choice.

After a successful write, a record index of 20 (the last slot of a page) with
`page & 7 == 7` flushes the preceding eight pages. The call is
`FUN_1426a8ce0` (`0x1426a8ce0`) with the mapped-view record at context
`+0x18`, a zero flag, the byte offset
`table_base + (page - 7) * 0x200 - segment_base` (context words `+0x68` and
`+0x48`) and length `0x1000`. The callee rejects a null view, a view shorter
than the length, or an offset past `size - length` with `0x57`
(`ERROR_INVALID_PARAMETER`); otherwise it flushes the 4 KiB span through one
indirect slot; when that succeeds and the caller's flag is zero it also flushes
the view's file handle through a second indirect slot, and a zero return from
either slot is reported as an error. It returns 0 on success or a non-zero
error code, and the writer formats a non-zero result with `FUN_142511f10`
(`0x142511f10`), which writes the message into a stack buffer and discards it.
The two import names are not readable in this image because the on-disk name
table is zeroed; the call shapes identify a mapped-view flush and a file-handle
flush, not their exact APIs.

`DynamicIndexKeyInsert` (`0x1425032d0`) is the layer above. It calls the
wrapper at `0x142511c90`; on `0xf` it asks `FUN_14250dc30` (`0x14250dc30`) to
create the bucket's next segment, stores the new segment number at
`(manager[0x68] + 0x110 + bucket * 4)` and retries the insert once. If the
retry still fails and the result is not the type-3 `2`, `FUN_14250d170`
(`0x14250d170`) passes the key to the error formatter at `0x14269acd0`, which
writes into a 24-byte local that is not used again. Twelve functions call this
layer, fourteen call sites in all; the read path never calls it.

`ow-casc` does not implement the writer, for two reasons that a test cannot
remove. The writer is allocator and I/O state rather than a pure function: its
inputs are the client's find context and its page-table/mapped-view state, and
its observable effects are the record, the out-entry and the flush. More
decisively, all five type-3 call sites pass a null range (for example
`FUN_142500430` at `0x142500541`), so the location and size written into those
records come from uninitialised stack slots. The type-3 sink ignores both, so
the calls are functionally deletes, but the record bytes are not a function of
the arguments and no bit-exact golden vector can exist. The install has no
positive page record either: all 2,560 pages are zero, so the positive write
path is synthetic-only. The writer is read and documented, not ported.

### Key-state intervals and resolve

`DynamicIndexDecodeEntries` (`0x1425209a0`) clears the interval count, decodes
the sorted equal range, then the matching page records in page/record order,
and calls `DynamicIndexNormalizeIntervals` (`0x1425208d0`) once at the end. A
single equal entry is sink type 1 (found, chosen, clear intervals, append its
resident interval); two or more are type 2 for the first (found, chosen, clear
intervals) and type 6 for each later entry (append resident). The chosen entry
is therefore the first equal entry, and on a duplicate pair the only interval
comes from the second entry.

`DynamicIndexAppendInterval` (`0x142520700`) writes two 16-byte
`{u64 start, i32 delta}` events: `(start, +1)` and `(start + (u32)len, -1)` for
a resident range, or `(start, -0x40000000)` and `(start + (u32)len,
+0x40000000)` for a non-resident one. Only the non-resident append normalizes
immediately; the decode pass normalizes once at the end. The end is a 64-bit
add of the 32-bit length.

`DynamicIndexNormalizeIntervals` sorts the events by start (the client's
unstable 16-byte sort), sums each equal-start group with 32-bit wrap, and keeps
only coverage crossings: `(start, +1)` when coverage goes from `<= 0` to `> 0`
and `(start, -1)` when it goes from `> 0` to `<= 0`. The group's start is the
first event's start, and non-crossing groups disappear.

`DynamicIndexRangeIsResident` (`0x142520c60`) walks the normalized events while
`event.start <= request.start`, adding deltas with 32-bit wrap. If the buffer
ends there, it answers `!resident` (an empty tail is outside every interval).
Otherwise it accepts only when the request's coverage matches the flag — `> 0`
for a resident request, `<= 0` for a non-resident one — and
`request.start + len <= next_event.start`. The end comparison is inclusive, so
a request that ends exactly at the next event is accepted. The client's
flag-mismatch return is non-zero with a zero low byte; every caller tests only
the low byte (`DynamicIndexResolveRange` uses `TEST AL,AL`), so `ow-casc`
models the answer as a bool.

`DynamicIndexResolveRange` (`0x1425115d0`) first requires a chosen entry
(status 2 otherwise). It rejects `start > size` (status 4); a request length of
`0xffffffff` becomes `size - start`; and it rejects `start + len > size`
(status 4). The output range is the absolute `packed + start` with that length
and is written before the resident check. A non-zero length is then checked
with the resident flag 1; failure is status 3 and leaves the entry unwritten.
A zero length skips the check and succeeds. Success writes the chosen entry's
packed location and stored size.

`DynamicContainerRead` (`0x1424f48d0`) asks for
`(caller_offset + 0x1e, caller_len)`. `Store::read_payload` is that request for
a full record: it resolves `(0x1e, size - 0x1e)`, reads exactly that payload
range through the archive page cache, and returns the bytes without the data
header. `Store::read_stored` is the full-record compatibility wrapper used by
the tests (`blte::decode` accepts either form): it calls `read_payload` and
prepends the 30-byte data header with one un-cached read, which the client's
read path never performs. Status 2
is `KeyNotFound`, and status 3/4 is a parse error. The install's 806 duplicate
keys only read because the second entry's type-6 interval is exactly that
payload range. The short-record and status-3/4 error paths of `read_payload` are
not exercised: every install record carries the 30-byte header and every
install payload request is resident. The page cache itself is tested for a
short read with a synthetic archive.

The interval arithmetic is synthetic-tested (merge, adjacency, overlap, gap,
i32 wrap, non-resident holes, all four statuses, `0xffffffff`, every sink type,
duplicate decode order, type-7 split). The install sweep exercises the type-1
and type-2+6 paths on every key. Page types 3/4/5/7 are synthetic only: this
install's page tables have no records.


## Encoding file

`EncodingHeaderParse` (`0x142498920`) reads the 22-byte header. It accepts
only when the bytes are `'EN'`, version 1, byte 17 zero, both key widths are
1..16, and both page sizes, both page counts and the espec block are non-zero.
It writes the client's 40-byte output struct **before** the value checks run:
ckey width at `0x11`, ekey width at `0x12`, ckey->ekey page count at `0x14`,
ckey->ekey page bytes at `0x18`, ekey->espec page count at `0x1c`,
ekey->espec page bytes at `0x20`, espec block size at `0x24`. A header with a
zero page count therefore still has every field written when the parser
returns 8. `ow-casc` keeps that order in `encoding::write_header_fields`, and
`EncodingHeader::from_bytes` is the safe wrapper that calls it.

The installed header is 16/16 keys, 4 KiB pages, 16025 + 10519 pages and a
1,484,507-byte espec block. The parser accepts every key width 1..16; the
section reader uses the client's `width + 0x10` index stride for any width, and
the pure `find_*` lookup helpers take the widths from the info struct (they are
harness-tested at 1/2/4/8/16). The public `EncodingFile` wrapper still refuses
a header that is not 16/16 (`Error::UnsupportedKeyWidth`) because the install's
file is 16/16; that limit belongs to the wrapper, not to the client or the
lookup.

`EncodingSectionsRead` (`0x1424d1c60`) is the section reader. Its caller is the
encoding state machine `FUN_1424d16c0` (state 3); it takes the read object in
`RCX` and a one-byte "started" out-parameter in `RDX`. The parsed header lives
in the 0x50-byte info struct that `reader+0x88` points at: the client's 40-byte
header at `+0x00..+0x27` and five section pointers at `+0x28` (espec), `+0x30`
(ckey index), `+0x38` (ckey bodies), `+0x40` (ekey index) and `+0x48` (ekey
bodies). The stream descriptor at `reader+0x48` carries the stream's `u32`
length at `+0x10`.

The client reads the sections in this order, with every offset and size
computed in a 32-bit signed register and handed to the range reader as a `u32`:

| section | offset | size |
|---|---|---|
| espec | `0x16` | header `+0x24` |
| ckey->ekey page index | `0x16 + espec` | `(ckey_width + 0x10) * ck_count` |
| ckey->ekey page bodies | previous + previous size | `ck_page_bytes * ck_count` |
| ekey->espec page index | previous + previous size | `(ekey_width + 0x10) * ek_count` |
| ekey->espec page bodies | previous + previous size | `ek_page_bytes * ek_count` |

Each allocation passes alignment `0x10`. The index entry is `first key[width]`
plus a 16-byte page hash; the section size arithmetic is why a header with a
different key width moves every later section. Because the arithmetic is
32-bit, a product or offset sum that overflows wraps before the range check.
The client allocates five separate buffers and reads into them; `ow-casc`
slices the already-decoded stream instead. The bytes are the same; the
client's allocation-failure path has no Rust equivalent here.

The five reads go through the generic range reader (`FUN_1424d14c0` builds a
request, `FUN_142495ee0` reaches `FUN_142494d10`, which copies the stream
descriptor and queues the request). The function then takes the container's
lock at `container+0x498`, drains the reader's pending list at `reader+0xb8`
until the head points to itself (`FUN_142496bd0` per request), releases the
lock, and returns 0. It sets `reader+0x40` to 4 and the out-parameter to 1.
The completion callback `FUN_1424d1a60` decrements `reader+0xc8` and merges
each read's status into `reader+0x168`; at zero it moves the state machine to
5, or to 6 on error.

The range rule is `offset > stream_len || (offset == stream_len && size != 0)`
for a rejection; otherwise a request that crosses the end is clamped to
`stream_len - offset` and flagged short. The read completion
(`FUN_142495470`) turns the short flag into status 14. The rejected arm marks
the request with state 3 (after formatting the stream key); the drain
(`FUN_142496bd0`) notifies the completion, the result lookup finds no result,
and the reader merges status 3. `EncodingHeaderRead` (`0x1424d1b40`) gives
status 14 to a header read that is not exactly 22 bytes, and also remaps a
header-parser reject (the parser's 8) to 14 before the reader sees it. Its only
path to 8 is a redundant check of the two page-byte fields, which cannot fire
after an accepted header. A section read clamped short is 14 through the
completion's short flag.

The client issues all five reads and merges their statuses in completion
order; `ow-casc` reads them synchronously and reports the first failing
section in layout order. The open fails either way; a corrupt file with two
different failures could report a different one of the two statuses.

The client's post-read step `FUN_142498a70` (`0x142498a70`) validates the
espec block: its last byte must be zero (else status 8), and the number of NUL
bytes is the number of strings it indexes. `ow-casc` performs the same check
and count.

The installed decoded stream is 111,058,223 bytes. The five sections end at
111,058,161; the last 62 bytes are the builder's debug string
`b:{22=n,1484507=z,512800=n,65638400=n,336608=n,43085824=n,*=z}`, whose five
numbers are exactly the section sizes, and are never read by the client. The
sections are espec 22 + 1,484,507, ck index 1,484,529 + 512,800, ck bodies
1,997,329 + 65,638,400, ek index 67,635,729 + 336,608 and ek bodies
67,972,337 + 43,085,824. Walking all 16,025 ckey pages gives 1,714,571 entries
and 1,714,573 ekeys; the two pages that are physically full (7664 and 13330,
both stopping at 4,082) carry the only two double-ekey entries. Their `u32`
decoded sizes sum to 193,863,820,123 bytes. Walking all 10,519 ekey pages gives
1,714,573 entries, only the last page has an `0xFFFFFFFF` terminator, and the
espec index range is 0..=29,693. Their `u40` stored sizes sum to
99,356,850,082 bytes; every fifth byte is zero, so the client's 32-bit read of
the tail is the same value as the full 40-bit value on this install. The espec
block has 29,694 NUL bytes and every index is used. All 26,544 page hashes
equal MD5 of their page body. The first ckey page starts at
`000029571a8ec20b5b56ba7719fbeb6f` and its page hash is
`eb565d89ce6518e251036b616aa007f4`; the first ekey page starts at
`000000c38d1651fc6b938f1310dedbd1`. This is measured on the install by
`crates/ow-casc/tests/encoding_real.rs` (about 3 s in a debug build).

### Page lookup

`EncodingPageIndexSearch` (`0x142498880`) is the page-index binary search. Its
arguments are the index base, the page count, the key width, the key and the
key length. An entry is `first key[width]` followed by the page's 16-byte MD5,
so the stride is `width + 0x10`. The client computes
`mid = ((hi - lo) >> 1) + lo` in 32-bit unsigned arithmetic, calls
`memcmp(key, entry, key_len)`, returns `mid` when the comparison is equal,
raises the low bound to `mid + 1` when it is positive (the key is above the
entry's first key), lowers the high bound to `mid` when it is negative, and
loops while `lo < hi`. The miss is `lo - 1` with 32-bit wrap, which is
`0xFFFFFFFF` when the key is below page 0. The chosen page is therefore the one
whose first key is the greatest one `<= key`; it is **not** the first page
whose first key is `>= key`. An exact first key returns its own page, and a key
between pages `N-1` and `N` returns `N-1`. The caller compares the return with
`-1`. The search is verified by `fn/EncodingPageIndexSearch`: its four
conditional branches (`pages != 0`, the equal compare, `lo = mid + 1` and
`lo < hi`) are all measured at 100%, the return is compared zero-extended, and
the index and key buffers prove it writes nothing. The entry offset is a
32-bit product, so a page count high enough to wrap it is outside what the
arena can represent and is declared open there.

`EncodingFindCKey` (`0x142498540`) uses that search with
`min(key_len, ckey_width)` as the comparison length, then starts the page body
at `page * ck_page_bytes` (`IMUL`, 32-bit) and walks to
`start + ck_page_bytes`. A ckey record is `u16` count, a big-endian `u32`
decoded size, `ckey[ckey_width]`, then `count * ekey_width` bytes of ekeys.
**Only the low byte of the count is read**: a zero low byte ends the page, and
a nonzero high byte is ignored. The scan compares the ckey prefix over the same
`min(key_len, ckey_width)` length. A hit writes the record's address to the
out slot and returns 0; a miss returns 6 and leaves the out slot zero.
`EncodingFindEKey` (`0x142498610`) is the ekey equivalent. Its page search uses
the caller's **full** key length (not the minimum), while its record scan uses
`min(key_len, ekey_width)`. An ekey record is `ekey[ekey_width]`, a big-endian
`u32` espec index and a big-endian `u40` stored size; an espec index of
`0xFFFFFFFF` ends the page. Both scans advance with the client's 32-bit
arithmetic.

`EncodingResolve` (`0x142498700`) is the caller-level resolve. It returns 1
when the info struct's `+0x10` ready byte is zero, copies the ckey, gets the
ckey record's `u32` decoded size for `+0x10`, copies the first ekey to `+0x14`
(or copies the last ekey and returns 7 when the info struct's `+8` field
selects the multi-ekey path; that path is not reached on this install), writes
the ekey width at `+0x30`, looks the ekey up, reads the **low four bytes** of
the ekey's `u40` into `+0x24` (the fifth byte at `+4` is never read), and
resolves the espec index to a string pointer through the table at `info+0x58`.
The first sampled install record proves both sizes: the ckey's `u32` is 524,304
and equals `Store::open_ekey`'s decoded length, while the ekey's tail is 3,530,
the smaller stored length. The resolve's ekey is the MD5 of the stored bytes
(`FUN_14248e250` hashes the read buffer of `+0x24` bytes and compares it with
the ekey at `+0x14`), which is why the two sizes differ.

**The read-path lookup never checks the page hash.**
`EncodingPageIndexSearch` uses the 16 hash bytes only as part of the
`width + 0x10` stride; `EncodingFindCKey`, `EncodingFindEKey` and
`EncodingResolve` never read them and contain no MD5 call. The measured
relation (`page hash == MD5(page body)` on all 26,544 pages) is a format fact,
not a read-path check. No other read-path function was found that verifies it;
whether a repair or write path does is not claimed.

The lookup's boundary rule is checked on every install page: all 16,025 ckey
first keys and all 10,519 ekey first keys resolve to their own page; all 16,024
and 10,518 keys strictly between consecutive first keys resolve to the earlier
page; and a key above the last first key resolves to the last page. The
synthetic tests cover the empty index, the exact/between/below/above cases, a
zero low count byte after a record, a nonzero high count byte, a record after
the ekey terminator, duplicate keys, compare lengths 0/1/2 and widths
1/2/4/8/16. The harness units are `fn/EncodingFindCKey` (verified 100%,
7/7 mutants) and `fn/EncodingFindEKey` (verified 100%, 10/10 mutants).

## BLTE

`BlteFrameHeaderParse` (`0x1424a90e0`) is implemented operation for operation.
It first requires at least `0x24` bytes, then checks the `BLTE` magic, a
non-zero big-endian header size at `+4`, a frame type at `+8` of `0x0F`
(24-byte records) or `0x10` (40-byte records), and a non-zero big-endian `u24`
count at `+9`. The header size must equal `12 + record_size*count` and the
buffer must contain that many bytes. It computes MD5 over exactly
`header_size` bytes; the caller's optional digest is compared only when the
check flag is non-zero. Status 7 means need-more (`0x24` or the declared
size); status 8 means invalid. `BlteFrameHeaderOpen` (`0x1424a9590`) and
`BlteFrameHeaderOpenEx` (`0x1424a9670`) are the wrappers.

Each record is `stored u32 BE`, `decoded u32 BE`, `md5[16]`; type `0x10` adds
`extra[16]`, held in the client's parallel array. The read path parses with
the check flag clear and never compares the per-chunk MD5s. The only read-path
use of the type-`0x10` extras is an existence check in `FUN_1424a9a20` when it
computes the on-disk size; no consumer of their contents was found. Chunk 0
starts at `header_size`; no hash sits between the table and the first chunk.

Block modes:

- `N` — stored (`BlteBlockReadStored`, `0x1424d7850`);
- `Z` — zlib (`BlteBlockReadZlib`, `0x1424d7ca0`);
- `E` — Salsa20-encrypted (`BlteChunkDecrypt`, `0x1424d68a0`), key from the
  caller's keyring;
- `4` — LZ4 (`BlteBlockReadLz4`, `0x1424d6e50`);
- anything else is status 8.

`BlteBlockRead` (`0x1424d61b0`) accepts an `E` IV of 0..8 bytes, not only 4/8.
`FUN_1424eb510`, the top-level direct reader, has the same four modes and
returns status 5 for `F`/`O`; there is no `A` case in either function. `A` and
`F` are not block modes of this build.

`E` chunks carry a one-byte key-name length (must be 8), the 8-byte key id, a
one-byte IV length (0..8), the IV, and the cipher byte `S`. A keyring miss is
status 13. Salsa20 starts at counter 0, the IV is zero-extended to 8 bytes,
and the chunk ordinal is XORed into its low four bytes. The plaintext's first
byte is the real block mode.

`BlteBlockReadLz4` reads an 11-byte block header: `4`, version `0x01`, a
big-endian `u64` declared size (checked only against `0xffffffff`), and a
block-size exponent in `5..=16`. The block size is `1 << exponent`; each slice
is fed to the stateless `Lz4DecompressSafe` (`0x1426201d0`) with a compressed
bound of `block_size + 0x10 + block_size/0xff`. `Lz4Decompress`
(`0x1426207f0`) is called with a zeroed 32-byte context from `Lz4ContextCreate`
(`0x1426201c0`), so it always takes the stateless path. A decode of zero or more
than `block_size` is status 8. A compressed slice shorter than the bound
returns status 0 without consuming input; `decode_lz4_chunk` treats that as an
error because it already holds the whole chunk.

MD5 is standard: `Md5Init` (`0x1426a5100`) writes the four RFC 1321 init
words, `Md5Update` (`0x1426a4f40`) updates the byte count and buffers input,
`Md5Final` (`0x1426a5010`) appends `0x80`, zeroes to 56 mod 64, writes the bit
length little-endian and emits the four state words little-endian, and
`Md5Transform` (`0x1426ab530`) is the 64-round block transform. The transform
loads the four state words, adds each round's message word from the 64-byte
block, and adds each accumulator back into the word it came from; the block is
read only, so the state is the whole output. Its Ghidra function listing stops
after the first 64 bytes and the rest of the straight-line body belongs to no
function, while the code continues at instruction boundaries to the return
(`0x1426ab530..0x1426abca9`, 1913 bytes, no conditional branch anywhere). It
is verified by `fn/Md5Transform`, which calls the shipped
`ow_casc::blte::md5_transform`.

Salsa20 is the standard 20-round cipher; `Salsa20StateInit`
(`0x1424e3230`), `Salsa20Xor` (`0x1424e32c0`) and the block helpers match the
Rust implementation.

A 30-byte record is a header-only empty file: `DynamicContainerRead`
(`0x1424f48d0`) adds `0x1e` and returns a zero-length range. A full one-off
scan of all 1,146,326 unique salt-0 keys found 1,146,325 type-`0x0F`
containers, no direct (zero header-size) container, no type `0x10`, no outer
`4` block, and one known orphan payload that is not BLTE. The 1,714,597 chunks
split `N`/`Z`/`E` = 284,878/1,386,658/43,061. The install's 63-key keyring
does not contain the keys used by the 43,061 `E` chunks (the 32nd-key sample
saw six distinct ids) and no support keyring is present, so their inner modes
are not measurable here; an inner `4` cannot be ruled out. The committed
install test samples every 32nd unique salt-0 key (35,822 containers, 54,490
chunks) and decodes 34 records in full. Type `0x10` and LZ4 therefore have
synthetic coverage only; no observed outer `4` block exercises LZ4 here.

The byte census (`cargo run --release -p ow-casc --bin census`, full stride-1
scan, read-only: frame headers and chunk mode bytes only, no payload decoded)
measures the share those chunk counts carry. Of the 1,146,326 records' index
sizes (84,463,293,804 bytes), 84,373,987,087 bytes are chunk bytes; the
remaining 89,306,717 bytes (0.11%) are the 30-byte data headers, the BLTE
frame headers and the one non-BLTE record. The modes split `N`/`Z`/`E` =
8,858,748,676/72,530,821,791/2,984,416,620 stored bytes, 10.50%/85.96%/3.54%,
and their declared decoded sizes are 8,858,463,798/163,035,566,718/
6,079,055,245 bytes. The 43,061 `E` chunks (2.51% of chunks, 3.54% of chunk
bytes, 3.42% of decoded bytes) carry 11 distinct key ids, none in the
keyring, so 22,506 records (1.96%) cannot be decoded at all until a key is
supplied. `--stride N` reproduces a sample; `cargo test --release -p ow-casc
--test blte_real` asserts that the bytes classified across modes equal the
frame headers' declared total.

## Oracle

Eleven CASC units run under `dev/verify_harness/`. The oracle is the current
build's code: the installed `Overwatch.exe` PE with its `.text` decrypted by
`dev/offline_page_recover.py` into `module_offline.bin` (same PE headers and
timestamp; `.rdata`, `.data`, `.pdata`, `.rsrc` and `.reloc` are byte-identical
to the installed file; no process capture). The unit descriptors carry that
recovery image's `v2b` frame; the runner maps it to the current-image address
`0x140000000 + RVA`. The candidate is the matching shipped function in
`ow-casc`, built by
`crates/ow-casc/src/bin/verify_probe.rs`. `python
dev/verify_harness/cli.py casc` runs exactly these units and exits non-zero if
any one of them is not verified.

| client function | address | shipped candidate | unit |
|---|---|---|---|
| `Salsa20StateInit` | `0x1424e3230` | `ow_casc::blte::salsa20_state_init` | `fn/Salsa20StateInit` |
| `Salsa20Xor` | `0x1424e32c0` | `ow_casc::blte::salsa20` | `fn/Salsa20Xor` |
| `DynamicIndexHash` | `0x142512c60` | `ow_casc::index::hash_index` | `fn/DynamicIndexHash` |
| `DynamicIndexKeyHash` | `0x142513230` | `ow_casc::index::key_hash` | `fn/DynamicIndexKeyHash` |
| `EncodingHeaderParse` | `0x142498920` | `ow_casc::encoding::write_header_fields` | `fn/EncodingHeaderParse` |
| `EncodingFindCKey` | `0x142498540` | `ow_casc::encoding::find_ckey_record` | `fn/EncodingFindCKey` |
| `EncodingFindEKey` | `0x142498610` | `ow_casc::encoding::find_ekey_record` | `fn/EncodingFindEKey` |
| `CdnIndexFooterParse` | `0x1424d9af0` | `ow_casc::container::parse_index_footer` | `fn/CdnIndexFooterParse` |
| `EncodingPageIndexSearch` | `0x142498880` | `ow_casc::encoding::page_index_search` | `fn/EncodingPageIndexSearch` |
| `Md5Transform` | `0x1426ab530` | `ow_casc::blte::md5_transform` | `fn/Md5Transform` |
| `CdnIndexPageSelect` | `0x1424da730` | `ow_casc::container::select_page` + `decode_page_record` | `fn/CdnIndexPageSelect` |
| `EncodingResolve` | `0x142498700` | `ow_casc::encoding::EncodingFile` (sizes + `ekeys_for_ckey`) | no standalone unit: it composes the two finds and the espec table; the find units prove the parts |

`Salsa20Xor`:

- Signature: `(u32 state[16], void *data, u32 len)`. The 64-byte state is the
  cipher state, the buffer is XORed in place, and the state's 64-bit counter
  advances by `ceil(len / 64)`.
- `Salsa20StateInit` (`0x1424e3230`) builds that state from a 16-byte key, an
  8-byte nonce and a 64-bit counter. It uses the `expand 16-byte k` constants
  and stores the 16 words in diagonal order: words 0..3 are the constants, the
  key words are at 5, 4, 9 and 12, the nonce words at 6 and 11, and the counter
  at 8 (low) and 13 (high).
- `Salsa20Xor` dispatches to `Salsa20XorBlocks256` (`0x1424e3370`) for 256
  bytes or more and `Salsa20XorBlocks64` (`0x1424e3e20`) for the remaining
  whole 64-byte blocks. A partial tail is copied through a full 64-byte block,
  so it still consumes one counter increment.
- 364 cases (pinned zero, all-ones, block-boundary and nonzero-counter vectors
  plus a deterministic sweep) agree bit-for-bit on the ciphertext and the
  state counter. All three conditional branches of `Salsa20Xor` are covered;
  the mutation stage kills 7 of 7 scored mutants (three allocation- or
  cast-only mutants are declared equivalent).
- Not tested: a counter whose high word carries. `ow_casc::blte::salsa20`
  always starts at block 0, and the unit uses counters 0..3.

`Salsa20StateInit`:

- Signature: `(u8 state[64], const u32 key[4], const u32 nonce[2], u64
  counter)`. It writes all 64 bytes and returns the output pointer; the return
  is declared void so the emulator's and the probe's addresses are not
  compared.
- The BLTE path builds every 64-byte block's state through
  `salsa20_state_init`, so this unit and `fn/Salsa20Xor` cover the same
  function the decryption runs.
- 364 cases (the pinned key `01..10`/nonce `10..17` vector, the counter's word
  split at `0xffff_ffff`, `0x1_0000_0000` and all-ones, plus a deterministic
  sweep) agree on all 64 bytes. The function has no conditional branches, so
  coverage is 100% of zero sites; 24 of 24 scored mutants are killed.

`DynamicIndexHash`:

- Signature: `(const u8 *data, size_t len, u32 *state0, u32 *state1)`. The
  two state words are in and out, and the function returns nothing. The same
  function checks the file-header hash and the rolling entry-block hash in
  `DynamicIndexSegmentParse` (`0x14250fcf0`).
- The block loop runs `floor((len - 1) / 12)` blocks: the last 1..=12 bytes
  are added to the running words, not mixed as a block. A trailing whole
  twelve-byte group is a tail, and a zero-length input stores the two running
  words before the final mix. The reference `.idx` files only hash 16- and
  18-byte spans, so these shapes are pinned by the oracle unit (empty, 12 and
  24 bytes) rather than by a file.
- The loop has three paths, selected by the data pointer's low two bits
  (16-bit aligned, odd, otherwise 16-bit). The fixture pins the pointer
  alignment so all three run and must agree.
- 364 cases (the real `0300000159.idx` header and its first two entries, every
  tail length 0..12 at each alignment, and a length/byte sweep) agree on both
  state words and the untouched buffer. 12 of 12 scored mutants are killed.
- Coverage: 8 of the 11 swept branch sites reach both edges. The other three
  are the compiler's post-loop `remaining > 12` tests, whose jump edge is dead
  because the loop leaves a remainder of 1..=12. The unit declares those three
  edges under `unreachableBranches`; the harness accepts a declaration only
  when the site executed, exactly one edge was observed, and the declared
  edge was never observed. Coverage is 100% of the reachable sites.

`DynamicIndexKeyHash`:

- Signature: `(const void *key, size_t len, u32 initval)` returning `u32` in
  EAX. It seeds three running words with `0xdeadbeef + (u32)len + initval`
  and runs lookup3 `hashlittle`: twelve-byte groups are mixed, the 1..=12
  byte tail is added, and the final mix returns `c`. A zero-length key is the
  client's `case 0` and returns `c` before the final mix. `initval` is read
  through `R8D`, so only its low 32 bits reach `c`.
- `DynamicIndexSegmentParse` (`0x14250fcf0`) calls it once per 24-byte page
  record as `key_hash(record + 4, 0x13, 0)` and compares
  `hash | 0x80000000` with the record's first dword. The page writer
  `DynamicIndexPageInsert` (`0x142511d20`) computes the same value. The other
  direct callers are `0x142504d20`, `0x142504e90`, `0x14251dcb0` and
  `0x14251f720` (other index-family writers); the "no production caller" note
  was wrong.
- Three load paths are selected by the key pointer's low two bits (dword,
  word and byte loads). They assemble the same twelve bytes; the fixture pins
  arena offsets 0, 1, 2 and 3 so every path executes and the shipped
  byte-wise `key_hash` must agree with each.
- 364 cases (the real 19-byte page-record payload, two real 19-byte entry-block
  spans of `0300000159.idx`, every tail length 1..12, all lengths 0..64 at
  four alignments, init `0`/`1`/`0xffffffff`/`0xdeadbeef` and a 64-bit init
  whose upper half must be ignored) agree on the return word and the
  untouched buffer. All 8 branch sites reach both edges; 24 of 24 scored
  mutants are killed.
- Not tested: every `.idx` page table on this install is zero-filled, so no
  stored page-record hash exists to agree with. The production caller
  `DynamicIndexSegmentParse` is now mirrored by `PageTable::parse`, and the
  synthetic page-table fixtures validate the hash; the function's contract is
  verified from the client's bytes. The page-table read path and the key-state
  interval/resolve path are implemented. The segment load/parse has no
  differential unit: it is allocator and I/O state, and the mode-2 path is
  `fs::read` plus the shared parse. Its acceptance is install-tested on all
  sixteen files and synthetic-tested on every rejection in the validation
  order above. The page-table write path (`DynamicIndexPageInsert`) is read
  and documented in "Page-table writer" above but not implemented: no
  read-path caller exists, and the type-3 calls write location and size from
  uninitialised stack slots.

`EncodingHeaderParse`:

- Signature: `(void *out, const u8 *data, u32 len)` returning 0 for an
  accepted header and 8 for a rejected one. The output is the 40-byte struct
  above; the file's integers are big-endian and the client assembles each one
  with a shift-and-or sequence, stores it host-order, and shifts a page size
  left by ten to turn its KiB count into bytes.
- The gates are `len >= 22`, `'E'`, `'N'`, version 1 and byte 17 zero. Only a
  header that passes them is written. The seven value checks (ckey 1..16,
  ekey 1..16 via `(u8)(ekey - 1) <= 15`, both page sizes, both page counts and
  the espec block non-zero) run after the writes. A value failure still leaves
  every field written.
- 364 cases: the install's real 22-byte header (16/16 keys, 4 KiB pages,
  16025 + 10519 pages, 1,484,507-byte espec), every length 0..22, longer
  lengths, every gate byte, each width/page size/page count/espec at its zero,
  one and all-ones edges, mixed headers and a deterministic sweep. The output
  buffer starts at `0xA5` and the input buffer past the header at `0x5A`, and
  both are compared, so untouched bytes and input mutation are visible.
- All 13 reachable branch sites reach both edges. The null-data guard's jump
  edge is not representable through the harness (a pointer argument always
  names an arena buffer) and is declared under `unreachableBranches`; the
  harness validates the declaration. 39 of 39 scored mutants are killed.
- Not tested: a null `data` pointer. Length 0 reaches the same reject block
  and is compared. The 16/16 limit is the page reader's, not the parser's:
  widths 1..16 are pinned as accepted.

## Manifest

A `.cmf` is the asset registry: GUID → content key. The first 48 bytes are
clear even when the body is encrypted:

| offset | field |
|---|---|
| `0x00` | `u32` build version |
| `0x1c` | `i32` data patch record count |
| `0x20` | `i32` data record count |
| `0x24` | `i32` entry patch record count |
| `0x28` | `i32` entry count |
| `0x2c` | `u32` magic |

The magic's bits 8..31 carry `cmf`; the top byte is the format version.
Body: `entry_count * 20` bytes, then `data_count * 29` bytes
(`u64 guid`, `u32 size`, `u8 unknown`, 16-byte content key). The record list
is **GUID-sorted**; that is the validity check.

### What the record's `unknown` byte means

The byte at `+12` is not padding. Bit 7 says the payload is **not** in the
encoded store: it is an entry of a type-`0x09C` bundle. The low seven bits are a
class byte the client carries through to its loader; `FUN_141fda1f0`
(`0x141fda1f0`) switches on it and picks a streaming priority (`0..3` → 0,
`4..6` → 1 or 2 by the manifest-kind bit), and this install only uses `0..6`.

The bit's journey is read from the image, and it is the reason a bundle-hosted
asset is never opened through its content key:

1. `FUN_141feff80` (`0x141feff80`) copies the byte into the low byte of the
   in-memory record's flags dword (`+0xc`) and moves bit 7 to **bit 16**
   (`(byte[12] & 0x80) << 9`).
2. `FUN_141fef610` (`0x141fef610`) answers a GUID query with
   `{size, class, flags, content key, manifest}` and puts in-memory bit 16 at
   **result bit 9** (`0x200`).
3. `FUN_141fd4a50` (`0x141fd4a50`) sends a result with bit 9 clear to the
   encoded store (`Store::open_ckey`) and one with bit 9 set to the bundle
   directory. There is no store fallback for the flagged case, even when the
   content key does have an encoding entry — the map asset
   `0x0800000000000688` is exactly that case.

`ow-tank`'s `bundle::RECORD_IN_BUNDLE` is this bit, and `GameData::open_asset`
is the two-way open.

### The bundle entry table

A bundle's payload (a type-`0x09C` record's decoded bytes) is read by
`FUN_141fd5820` (`0x141fd5820`): `i32 count`, `u8 width` (1, 2 or 4), then
`count` entries of `u64 GUID` + `width`-byte offset. The entries are GUID-sorted
and the client binary-searches them; offset `0` means "not here". The table
stores no length — an asset's length is its CMF record's `size`.
`docs/formats/maps.md` has the full account and the install counts. The search
is the verified unit `fn/BundleLookup` (oracle `BundleEntryLookup` at
`0x141fd5820`): 364 synthetic cases, 100% branch coverage and 12/12 mutants.

The key/IV derivation is per build and uses a 512-byte table. The implemented
CMF derivations are builds `150851`, `154088` and `154504`: AES-256, CBC, no
padding. Only `154504` has differential units (with the coverage gap below);
`154088` is a TACTLib port and `150851` has no test. A wrong key/IV yields an
unsorted record list; `Registry::open` enforces that check, which is how
AES-128 was ruled out.

### Build 154504: the table and the derivation

The current image's 512-byte table is a plain global at `0x1436100c0`
(`0x140000000 + RVA 0x36100c0`, in `.rdata`). The 512 bytes end at
`0x1436102c0`, where a separate string begins. The installed `Overwatch.exe`
maps `.rdata` at VA `0x2f2d000` from file offset `0x2f2b800`, so the table's
file offset is `0x360e8c0`; `ow-tank` reads exactly those bytes from the
operator's executable at runtime. The table is not vendored.

The two routines the client's CMF path calls:

- `CmfKey154504` (`0x142e48a20`) returns the 32-byte key. The seed is
  `table[0x100 + 32]` (`table[0x120]`); each byte is
  `table[SignedMod(cursor, 512)]` and the cursor advances by 3. The key reads
  no header field and no file name, so it is the same for every manifest of
  this build.
- `CmfIv154504` (`0x142e48700`) returns the 16-byte IV. The digest is SHA-1 of
  the ASCII manifest file name. The cursor starts at
  `16 * header.buildVersion`; each byte is `table[SignedMod(cursor, 512)]`,
  then the cursor advances by `digest[header.dataCount % 20] + dataCount` and
  the byte is XORed with `digest[(build + i) % 20]`. The client's routine holds
  `154504` as the digest-index constant and reads the header for the cursor
  seed.

`CmfCryptoInit` (`0x141feee80`) is the caller: it reads the 48-byte clear
header, accepts the version-27 `cmf` magic, calls the two routines (call sites
`0x141fef04c` and `0x141fef061`) and sets up AES-256-CBC with no padding.

Install measurement: the three encrypted regional content manifests and the
enUS text manifest decrypt to GUID-sorted records with this derivation: 1,301,598
(`Win_SPWin_RDEV_EExt.cmf`), 1,306,039 (`Win_SPWin_RCN_EExt.cmf`), 1,301,855
(`Win_SPWin_RKR_EExt.cmf`) and 124,196
(`Win_SPWin_RDEV_LenUS_text_EExt.cmf`). The 154088 formula on this table, the
table based at `0x1436101c0`, the 154088 table with this formula, and wrong
seeds or steps each produce hundreds of thousands of out-of-order adjacent
pairs. `Registry::open` rejects any list that is not GUID-sorted. The install
test opens `Win_SPWin_RDEV_EExt.cmf` and its enUS text manifest; the RCN and
RKR counts are measured, not test-pinned.

Differential and mutation proof (not a five-stage verdict):
`fn/CmfKey154504` and `fn/CmfIv154504` in
`dev/verify_harness/units/` pass the differential stage on 364 cases each,
every channel. The cases cover all 32 key bytes and all 16 IV bytes, table
bytes `0x00`/`0xFF`, build `0` and `0xFFFF_FFFF`, data counts `0`, `19`, `20`,
`21` and the 32-bit extremes, digest bytes `0x00`/`0xFF` at every digest index
`0..19`, the 32-bit cursor wrap below `2^32` and the negative-`i32` modulo
edge. The mutation stage kills 1/1 and 5/5 (the fifth IV mutant flips the
return arrow and does not compile; the harness's rule counts that as killed).
The coverage stage cannot measure either real body: the linear sweep
desynchronises at the overlapping-instruction opaque-predicate prologue and
reports the padding stubs (`0/15` and `5/33`). Both units carry that as an
acknowledged gap with the coverage gate at `0`; the differential and mutation
stages are the proof. The units use synthetic 512-byte tables, headers, names
and digests; neither the installed table nor any manifest bytes are in the
repository.

### The content key is the MD5 of the decoded file

Measured on 300 small records that resolve locally: `md5(decoded) == content
key` for 300/300, and the decoded length equals the record's declared `size`
for 300/300. The same keys do **not** hash the stored record (`0/300`), the
BLTE body without its 30-byte data header (`0/300`) or the data header's first
16 bytes (`0/300`). The rule is the content key at `record.content_key` =
MD5 of the decoded payload.

### Type-0x079 is not `STUMapHeader`

The 25 records on the `0x01e00000...` prefix are tool type `0x079`
(`VoiceCategory`). The client's `STUMapHeader` is class `0x02514B6A` and tool
type `0x09F`; see `formats/maps.md`. The 25 records are not in the map chain.

Asset type: a 12-bit field at bits 48..59, stored bit-reversed:
`type = reverse_12_bits((guid >> 48) & 0x0FFF) + 1`, `index = guid & 0xFFFFFFFF`.
This reproduces the published `types.txt` table and lands on known landmarks
(132 type-`0x075` heroes on the `0x02E00000` prefix).

## Gaps

- **The CDN index page selection is read** (2026-11-05). `CdnIndexPageSelect`
  (`0x1424da730`) walks the `pageCount * keyBytes` last-key array, and
  `CdnIndexPageResident` (`0x1424da590`) is the per-page read. `ow-casc`'s
  `find_page` now follows the client's expression. Why the earlier search
  missed it: it reads the footer fields at `index+0x6c/0x70/0x74/0x78/0x7c`
  (base `+0x58`), not at `0x14/0x1c/0x20/0x24`; its `memcmp` length is a
  register (`min(keyLen,keyBytes)`), so no immediate 16 appears; the stride
  is a runtime `IMUL`, not `SHL`; and one caller lives in code Ghidra did not
  promote to a function. Still open: the batch wrappers
  `0x1424b0e50`/`0x1424b0ef0` are read only at the expression level, and the
  config-to-slot wiring is an inference.
- The CDN HTTP transport itself is still a TACTLib port and has no real
  endpoint test, but the page request shape (range offset/length, full-page
  requirement, per-page checksum reject) is now read from the client.
- All 305 type-`0x09F` map headers now open: 42 from the encoded store and
  263 from bundles. See `formats/maps.md`.
- The record byte's low seven bits are read as a class, not as a meaning.

## What the port refuses that the client would fault on (2026-11-06)

Four input checks exist because the client has none and would read past the end
of its own buffers, which in Rust is either a panic or an allocation sized by a
file. Each one is a documented divergence, the same kind as
`parse_index_footer` returning 8 where the client has no divide-by-zero guard
after its entry-size step.

| input | client | port |
|---|---|---|
| `.idx` entry geometry: key width 16, size width 4, offset width 0/4/5/6 | the footer check only requires a non-zero entry size; the decoder then reads `entry[20..24]` on a 21..23-byte stride | `PageIndex::open_with_key` and `IndexFooter::entries_in_page` return `unsupported index entry geometry` |
| `.idx` tail longer than the file, or a `tail_len` that wrapped in 32 bits | allocates `tail_len`, seeks past the end | `open_with_key` refuses before the allocation (the tail is `checksum + last keys + checksums`) |
| encoding page count whose `(width + 16) * pages` product wrapped to zero | accepts the header, then reads past an empty page index on the first lookup | `EncodingFile::parse` returns `Error::SectionCountMismatch` |
| a record size in a bucket entry larger than its archive | allocates the size, reads, then reports a short read | `Store::data` sizes the archive first |

None of these can fire on a well-formed install: the section sizes the real
headers carry are the products' low 32 bits, so the checks are inert unless
the product wrapped. `cargo test --release -p ow-casc` covers the geometry and
wrapped-count cases.

## Verification

```text
cargo test --release -p ow-casc
cargo test --release -p ow-casc --test cdn_index_real -- --nocapture
cargo test --release -p ow-tank
cargo test --release -p ow-tank --test manifest_154504_real -- --nocapture
```
