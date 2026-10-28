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
`DynamicIndexSegmentLoad` (`0x14250fc60`) parses it. The parser rejects a
file whose header bucket at `+0x0A` disagrees with the bucket the name
selected, so a renamed file cannot silently load.

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
client's addition and `Store::read_stored` follows the boundary if a record
ever crosses it. No entry in this install crosses a `2^30` boundary, so
`(packed + 0x1e) >> 30` equals `packed >> 30` for every key.

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
followed by BLTE. It also parses all sixteen page tables: 2,560 pages, 53,760
record slots, 0 non-zero stored hashes, first zero at page 0 record 0, and no
page-table key resolves. `Store::entry_count` is 1,148,620; `Store::key_count`
is 1,147,814 because 806 keys are listed twice. The pure search is checked
against a linear first/last scan on a deterministic sorted corpus in
`find_range_agrees_with_a_linear_scan`; the page-table parser and lookup are
checked by `page_table_parses_and_selects_in_page_order`,
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
and the decode order. The type byte is passed to the entry sink: types 1 and 2
replace the chosen location, type 3 clears it, and types 4-7 affect only the
interval buffer or metrics. `ow-casc` models that chosen-entry effect; the
interval side effects remain open. The writer is `DynamicIndexPageInsert`
(`0x142511d20`): it asks `DynamicIndexFindEntry` for the first empty slot,
writes the key/location/size and the caller's type byte there, and stores
`DynamicIndexKeyHash(payload, 0x13, 0) | 0x80000000`. The write wrapper at
`0x1425033e0` passes type 6 or 7; the read path never supplies a page type on
this install because every page table is empty.

On this install every page table is dormant: 2,560 pages, 53,760 record slots,
**0** non-zero stored hashes, and the first zero is page 0 record 0 in every
one of the sixteen files. The client enters the branch because the page count
is non-zero (144-176 pages per file; 152 for `0300000159.idx`) and stops at the
first record, so no key resolves through the page table here. The synthetic
tests build non-zero tables and exercise first/middle/last records, type
1/2/3/6, a zero sentinel and page counts one and many.


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
1,484,507-byte espec block. The parser accepts every key width 1..16; the page
reader in `ow-casc` is fixed at 16-byte keys and refuses any other width
explicitly (`Error::UnsupportedKeyWidth`). That limit belongs to the reader,
not to the parser.

`EncodingSectionsRead` (`0x1424d1c60`) reads the sections in the client's
order. The root file's content key must land on a page boundary; that is the
check that a wrong section order fails.

## BLTE

`BlteFrameHeaderParse` (`0x1424a90e0`) expects `12 + 24*count` bytes, accepts
frame type `0x0F` or `0x10`, and validates the frame header's MD5; chunk 0
starts at that size and no hash sits between the table and the chunk.
`BlteBlockRead` (`0x1424d61b0`) accepts an `E` IV of 0..8 bytes, not only 4/8.
The shipped `decode` currently accepts only type `0x0F`, does not check the
declared frame size against `12 + 24*count`, and skips both MD5 fields; those
are open gaps in `ow-casc`, not claims about the client.

Block modes:

- `N` — stored;
- `Z` — zlib;
- `E` — Salsa20-encrypted, key from the keyring;
- `4` - LZ4 (**not implemented**; the client's reader is `BlteBlockReadLz4`
  at `0x1424d6e50`; no asset on this install uses it);
- `A` / `F` — ARC4 / frame variants (not used on this install).

Salsa20 is the standard 20-round cipher; `Salsa20StateInit`
(`0x1424e3230`), `Salsa20Xor` (`0x1424e32c0`) and the block helpers match the
Rust implementation.

A 30-byte record is a header-only empty file: `DynamicContainerRead`
(`0x1424f48d0`) adds `0x1e` and returns a zero-length range. A full scan of
all 1,148,620 bucket entries found 1,488 such records and one orphan payload
that is not BLTE.

## Oracle

Five CASC units run under `dev/verify_harness/`. The oracle is the current
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
  verified from the client's bytes. The page-table read path is implemented;
  its write path (`DynamicIndexPageInsert`) and the interval side effects remain open.

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

- The CDN index page formula is derived from the reference and measured
  footers, not from the client's decompiled CDN handler.
- All 305 type-`0x09F` map headers now open: 42 from the encoded store and
  263 from bundles. See `formats/maps.md`.
- The record byte's low seven bits are read as a class, not as a meaning.

## Verification

```text
cargo test --release -p ow-casc
cargo test --release -p ow-tank
cargo test --release -p ow-tank --test manifest_154504_real -- --nocapture
```
