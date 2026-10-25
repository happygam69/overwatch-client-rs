# The CASC container

How an Overwatch installation stores bytes, and how to get them back. Every
statement here was checked against a retail install
(`2.23.0.3.151177`, product `pro`, branch `eu`); the `ecache`, `indices` and
`shmem` statements were re-measured on `2.25.0.1.154504` (2026-10-25), and are
marked with that build.

## Layout on disk

```
<install>/.build.info
<install>/data/casc/config/<xx>/<yy>/<key>     build, CDN, patch and keyring configs
<install>/data/casc/data/<bucket:02X>*.idx     the bucket index
<install>/data/casc/data/data.NNN              the stored records
<install>/data/casc/indices/<archive-hash>.index   CDN routes, not local locations
<install>/data/casc/ecache/*.idx               cached chunk manifests, not content
<install>/data/casc/pro/                       v8 `.idx` files; no `data.NNN`, not content
```

`config/` files are addressed by their own key, two directory levels deep:
`config/<key[0..2]>/<key[2..4]>/<key>`.

For the reference install, `data/casc/data` is ~75 GB across 82 `data.NNN`
archives, with 16 bucket index files and a 103 MB encoding file. The
`2.25.0.1.154504` install has 93 `data.NNN` archives.

### Three directories that are not a second content store

Measured on `2.25.0.1.154504` (2026-10-25). They share the byte format of the
real store just enough to look like one, which is why each was tried as one.

**`ecache/`** mirrors the shape of `data/`: sixteen `.idx` files, a v7 bucket
header, a `data.000`. Its 103 bucket entries all name archive 0 and resolve to
`ecache/data.000` (77,538 B). But:

* 87 records carry a BLTE container, and in every one the container is
  **exactly its frame header** (`frameHeader + 30 == record size`): a chunk
  table with no payload;
* the other 16 records are a 30-byte header and no BLTE at all;
* total payload bytes across the file: **0**.

So an `ecache` key cannot be read as a stored record. Reading one at its
`ecache/data.000` offset is what produces
`malformed BLTE: chunk runs past the container`; the bytes are simply not in
that file. The tables list 256 KiB chunks with 16-byte hashes; what consumes
them is not established here, but whatever it is, it is not local content.
`data.NNN` is the only local content store.

**`indices/`** is the CDN's map, not the local one. The archive-group `archive`
ordinal runs 0..826 while the local bucket `archive` is a pack ordinal 0..92;
group/index sizes are exactly 30 B smaller than the local bucket size for
1,138,136 of 1,139,009 shared keys (the local record header), and index offsets
are CDN offsets. One index file is not even one pack: the 197,788 B
`eb2a63c8267fb4316cda61a8eed9fa3f.index` has 8,097 entries whose 1,326 bucket
hits span local packs 7, 8, 55, 56 and 91, at unrelated offsets. These files
answer "where would the CDN serve this?" (`cdn_route_of`), never "where is it
on disk?".

**`shmem`** is not an index at all. The three files are 20,480 B, header
`05 00 00 00 54 01 00 00` plus `Global\e:/Overwatch/data/casc/...`, the rest
zero. There is no footer for any index parser to find. In the client,
`FUN_14250d8d0` builds the `shmem` path, checks magic `0x2ab8` and version
parameters 4/5/6: it is a named shared-memory mapping. It carries no content.

## `.build.info`

A header row names the columns, then one row per branch. Column names carry a
`!TYPE:WIDTH` suffix to strip (`Branch!STRING:0`, `Build Key!HEX:16`, ...).

Those `!` suffixes are not decoration: without a width the hex columns have no
implicit length. Relevant columns: `Branch`, `Active`, `Build Key`, `CDN Key`,
`KeyRing`, `Version`, `Product`.

The row to use is the active one whose product matches the game. For Overwatch
the product is `pro`, and for a retail Windows install the version reads
`2.23.0.3.151177`.

### Two layouts: the install root and the game folder

There are two directory shapes, and the client treats them differently:

* **Install root:** `.build.info` and `data/casc` in the same folder. This is
  what the project opens by default (`E:\Overwatch`).
* **Game folder:** a subfolder such as `_retail_` with `.flavor.info` and the
  executable; `.build.info` and `data/casc` are one level up. On the reference
  machine, `_retail_/.flavor.info` is exactly
  `Product Flavor!STRING:0` + `pro`, and the client reads
  `_retail_/../.build.info`.

The client's `BuildInfoLoadFromInstall` (`0x14248f2a0`) builds
`<install>/../.build.info` whenever `.flavor.info` exists. Its
`BuildInfoSelectRow` (`0x1424aec30`) then picks the **first active row whose
Product equals the flavor**. With no flavor it requires every row's Product to
be equal and returns the **last active** row; a mixed-product table has no
answer.

The row layout is fixed by the descriptor table at `0x143471150`: **Product is
at +0**, Branch at +8, Active at +0x10, Build Key at +0x11, and so on. The
first field is therefore Product, not Branch — that is why the `.flavor.info`
value is `pro`.

`ow-casc` implements both layouts and the same selection rule
(`install.rs::resolve_layout` / `select_build_info`), including the
`<install>/../.build.info` path. Opening either `E:\Overwatch` or
`E:\Overwatch\_retail_` now yields the same 1,147,814-key store on the
reference install.

## Config files

`key = value value ...` lines, `#` starts a comment. Values are lists.

Build config, for the reference install:

```
root = 604df916259d0deedc271fc423cb3bf7
encoding = 86c1f68ac18d45c0dba39e44ca616790 8fc6b7222e13154cac4575344abd6eb3
encoding-size = 103652703 102569587
build-name = prometheus-2_23_0_3-151177
build-number = 151177
build-product = Prometheus
```

`encoding` carries two keys: the content key first, the encoding key second.
`root` carries one, a content key. The client's build-config parser
(`BuildConfigParse`, `0x1424ccc00`, `encoding` case) stores them at build-config
`+0x1a0` and `+0x1b4`; only the encoding key names a stored artifact, and the
two sizes in `encoding-size` are that one file's decoded and stored sizes
(`111,058,223` and `109,882,544` on the current install). `ow-casc` reads the
line the same way: the encoding key is tried first, and the other value is only
a fallback for a build that writes the pair in the other order.

CDN config carries `archives` (857 entries here), `file-index`,
`archive-group`, and the patch variants. `indices/<archive-hash>.index` files are
named after the entries of `archives`, which is how they correspond.

Note there is no `key-layout-*` in the build config, so the key-layout based
container variant does not apply to this product; the bucket index is the path.

## Keyring

```ini
key-1b3e4e1ecfb25877 = 3de60d37c664723595f27c5cdbf08bfa
```

The name's **last 16 hex digits** are the key id. They are written big-endian and
addressed little-endian, so the parsed value is byte-swapped before use. The
reference install's keyring holds 63 keys.

This is read from the client's own `key-` config handler
(`0x1424cebd0`): it parses the name's last **8 bytes** with `0x14248f040` and
the value's 16 bytes with `0x14248efd0`, and the BLTE E-block lookup uses those
8 bytes as a little-endian `u64`. So the swap is not a convention — it is the
byte order of the address. `ow-casc/src/install.rs` has a unit test for both
spellings.

### A second keyring flavour, with the opposite byte order

The community "support" keyring, which supplies keys a build's own keyring
omits, uses a different spelling:

```
8D7B6DFEB0FED1CA 5666D1E9F0B65B36026EA593EB617910 1 # comment
```

`<id> <key> [1|0]`, space separated, `0` meaning disabled. Here the id is used
**as written**, with no swap — the opposite of the config flavour. A file is
told apart by whether any line begins `key-`.

This matters: a key that is correct but registered under a byte-swapped id
simply will not be found, and the symptom is an asset that cannot be opened
rather than a wrong-looking key.

For the reference install the build keyring alone cannot open one hero asset;
with the support keyring merged in, all 132 open.

## Bucket index: encoding key to location

Sixteen files, `<bucket:02X>*.idx`. Each begins with a 32-byte header:

| offset | field |
|---|---|
| 0x00 | `i32` unknown (16 in the reference files) |
| 0x04 | `i32` header hash |
| 0x08 | `u16` version, 7 or newer |
| 0x0A | `u8` bucket number |
| 0x0B | `u8` extra bytes, 0 |
| 0x0C | `u8` size field width, 4 |
| 0x0D | `u8` offset field width, 5 |
| 0x0E | `u8` key width, 9 |
| 0x0F | `u8` file-offset bits, 30 |
| 0x10 | `u64` maximum addressable size |
| 0x18 | 8 bytes of padding |

**The client checks only the version floor and the three widths.** Its parser
(`DynamicIndexSegmentParse`, `0x14250fcf0` in the current build) rejects
`version < 7` and fields other than 4/5/9, but never looks at the
extra-bytes or offset-bits bytes; a v8 file with the same widths is read
identically. `ow-casc/src/index.rs` follows that rule.

The section header's second word is a 32-bit hash of the section body (the
16-byte file header for the usual `sectionSize = 16`), and the entry block's
second word is a rolling hash over the 18-byte entries. The client verifies
both before using the segment (`0x14250fcf0`, calling `0x142512c60`).

`ow-casc/src/index.rs` now implements the same hash (`hash_index`, from
`0x142512c60`) and rejects a mismatch. The implementation was checked against
**all sixteen** `.idx` files of the reference install: every file-header hash
and every complete entry-block rolling hash matches the stored value. A corrupt
index is therefore rejected exactly as the client rejects it, rather than
silently resolving to arbitrary bytes.

**The key-to-entry sub-index is empty in this install.** It starts at
`align4k(sectionSize + entrySize + 0x1007)`; in every one of the sixteen files
the whole remaining area is zeros, so the client's per-record
hash (`0x142513230`) never runs. `ow-casc` does not read the sub-index, and
there is no real vector here to verify that hash against.

Then a block header (`i32` byte count, `i32` hash) and that many 18-byte entries.
The block's position is `align16(sectionSize + 0x17)`, then 8 bytes of its own
header; the usual `sectionSize = 16` puts the entries at 0x28:

```
EKey[9] | location[5, big-endian] | size u32 little-endian
```

**The two fields disagree on endianness.** The 40-bit location splits as
`archive = value >> 30`, `offset = value & (2^30 - 1)` read big-endian; `size` is
a plain little-endian `u32`. This is the single easiest thing to get wrong here.
It was caught by resolving the encoding file's own key and comparing the size
against the build config's `encoding-size`: `102569617 = 102569587 + 30`, the 30
accounting for the data header.

`size` counts the whole stored record including that header. The client decodes
the two fields in `DynamicIndexDecodeEntries` (`0x1425209a0`): a 5-byte
big-endian value from entry bytes 9..14 and a 4-byte little-endian value from
bytes 14..18. Measured against this install, that is the only reading for which
the encoding file's own entry size matches `encoding-size`.

**A record of exactly 30 bytes stores no payload.** Its bytes are the data
header alone: in the empty records inspected, a fixed 14-byte marker, the key's
first two bytes in reverse, `size = 30` at offset 16, and 10 further bytes.
None of that is needed to read it as empty. The client's read path (`DynamicContainerRead`,
`0x1424f48d0`) adds `0x1e` to the record offset and returns a zero-length range
without opening a BLTE container. `ow-casc` returns an empty `Vec` for the same
shape. A full scan of all 1,148,620 bucket entries found **1,488** such
records; 1,146,325 ordinary 30-byte headers, 806 raw-`BLTE` duplicates (the
same 806 keys described below), and one record whose bytes are neither shape
(reported separately below).

The file is preallocated, so a zero-padded tail follows the entries; the entry
count comes from the block header, never from the file length. In the reference
bucket 00 the tail is 43,842 bytes of zeros.

Bucket selection hashes the nine key bytes: XOR them all, then `(i & 0x0F) ^ (i >> 4)`.

Entries are sorted by key, so a bucket can be binary-searched.

**The index is keyed by nine bytes, not sixteen.** Two encoding keys can share
a prefix, and this install has 806 such prefixes (1,148,620 entries for
1,147,814 distinct keys). All 806 are the same payload stored twice: one entry
points at the 30-byte data header and its `size` includes it, the other points
30 bytes later at the raw `BLTE` and its `size` excludes it. Both decode to the
same bytes, so a reader that keeps either entry is correct here; the client
keeps both as two ranges of one key state and picks the one containing the
requested range.

## BLTE

A stored record is either raw `BLTE` or preceded by a 30-byte data header:
`md5[16]`, `size u32` at offset 16, then 10 further bytes. **The header is
skipped unconditionally; its fields are not needed to read the record.** The
client's `DynamicContainerRead` adds `0x1e` before reading, and the BLTE
container itself declares its frame lengths. **All 1,148,620 bucket entries
were scanned (2026-10-25):** 1,146,325 are a 30-byte header followed by BLTE,
1,488 are header-only empty files, 806 are raw `BLTE` duplicates, and one
record is neither shape (reported below). So a reader must not trust the
header's size when deciding where BLTE starts; it checks for `BLTE` at offset
0, otherwise at offset 30.

**The one record that is neither shape** is key prefix
`590388b317f5a5e93f`, archive 0, offset `0x24690cd2`, 10,739 B. Its 30-byte
header has the index size at offset 16, but the payload starts
`50 41 02 10 10 10 10 00 01 02 01 ca ff ...` and contains the encoding
content key `01caff700674babb...`. It has **no encoding entry and no
root-manifest line**, so no registered asset reaches it; it is an orphan entry.
`ow-casc` reports `not a BLTE container` if asked for it directly. The client's
handling of this unreferenced key is not established, so no raw-file path is
invented for it.

After the magic, a big-endian `u32` frame-header size:

* `0` — one chunk covering the rest of the container, its decoded length implied.
* non-zero — at offset 8 sit `0x0F`, a 24-bit big-endian chunk count, and then
  that many 24-byte records of `stored u32 BE`, `decoded u32 BE`, `md5[16]`.
  **Chunks begin at the frame-header size, `12 + 24·count`** — the header size
  field counts the magic and itself, and there is no hash between the table and
  chunk 0. `BlteFrameHeaderParse` (`0x1424a90e0`) enforces exactly
  `headerSize == 12 + 24·count` for type `0x0F`.

Each chunk starts with a one-byte mode:

| mode | meaning |
|---|---|
| `N` | stored |
| `Z` | zlib |
| `E` | encrypted, see below |
| `4` | LZ4-HC block, client-only; not implemented here |
| `F` | recursive frame, not implemented |

### The `E` mode

```
u8  key-name length, must be 8
u8  key name[8]                   read as a little-endian u64 to index the keyring
u8  iv length, 0..8
u8  iv[iv-length]                 zero-extended to the cipher's 8 bytes
u8  cipher, 'S' for Salsa20 ('A' for ARC4 is not implemented)
... ciphertext, whose plaintext begins with the real mode byte
```

The block ordinal is XORed into the low word of the IV, so identical chunks do
not encrypt identically on disk. The client's block reader (`BlteBlockRead`,
`0x1424d61b0`) rejects only `8 < iv-length`; it zero-extends a short IV exactly
as `ow-casc/src/blte.rs` now does. Builds in the wild use 4 or 8.

The E key name is a little-endian `u64`: an encrypted chunk with key name
`9e 3a e1 5b 0f 83 f6 a5` is addressed as `0xa5f6830f5be13a9e`, which is the
support keyring's `A5F6830F5BE13A9E` line. Measured 2026-10-25 on a real E
chunk; this is the check that ties the two byte orders together.

**End-to-end check (2026-10-25):** ekey `2fc810bb53dfd7906842b8f95846305b`
(archive 35, offset `0x97d99de`, 16,695 B) decodes to **31,216 B** with the
support keyring. Its first chunk is `E` with that key name, so the LE key id,
the zero-extended 8-byte IV, the ordinal XOR and Salsa20 are all exercised on
real bytes, not only on the synthetic vector.

### Salsa20, exactly

Two details make the difference between correct output and noise, and neither is
visible from the algorithm's name:

* **20 rounds**, not the 8-round variant some BLTE documentation names.
* Keys are 16 bytes, so the state uses the **`expand 16-byte k`** constants with
  the key placed at both key word positions — not the 32-byte-key layout.

`ow-casc/src/blte.rs` pins both with the published ECRYPT reference keystream,
which fails if either is wrong.

## Encoding file: content key to encoding key

The encoding file is itself stored under an encoding key, given in the build
config. Its layout:

```
header (22 bytes)
  u16 LE signature "EN"
  u8  version, 1
  u8  content-key width, 16
  u8  encoding-key width, 16
  u16 BE content-key page size in KiB
  u16 BE encoding-key page size in KiB
  u32 BE content-key page count
  u32 BE encoding-key page count
  u8  unknown
  u32 BE espec block size
espec block
content-key page index   (page count * 32: first key[16], page hash[16])
content-key page bodies  (page count * page size)
encoding-key page index  (page count * 32)
encoding-key page bodies (page count * page size)
```

The client parses that header in `EncodingHeaderParse` (`0x142498920`) and
reads the sections in the order above in `EncodingSectionsRead`
(`0x1424d1c60`). It requires `byte 17 == 0`, both page sizes non-zero, both page
counts non-zero and the espec block non-zero; a header failing any of those is
rejected before a page is read. `ow-casc/src/encoding.rs` now makes the same
checks. (The client also accepts content/encoding key widths of 1..16; this
product writes 16/16, which is what `ow-casc` implements.)

**Every page index precedes every page body.** Reading the two interleaved
misaligns the entire file, and the failure is not obvious: page counts and sizes
still look sane, and the total consumed is close to the file length. The check
that settles it is arithmetic on a known key — the root file's content key must
land on a page boundary. For the reference install:

```
22 + 1364907 (espec) + 14959*32 + 14959*4096 = 1843617   bodies begin
1843617 + 5631*4096                          = 24908193  page 5631
root content key at 24909377, first entry of that page at 24908193
entry 31 at +1178, its key field at +1184 = 24909377     exact
```

In the reference file there are 62 bytes after the last section, of unknown
purpose. A parser must tolerate them rather than treat them as an error.

Page bodies are runs of entries; a zero count terminates one:

```
content-key entry
  u16 LE encoding-key count
  u32 BE content size
  content key[16]
  encoding key[16] * count
encoding-key entry
  encoding key[16]
  u32 BE espec index
  u40 BE file size
```
An espec index of `0xFFFFFFFF` terminates the page.

Page sizes are 4 KiB in the reference install, with 14,959 content-key pages and
9,820 encoding-key pages.

## Root file

A text list, decoded through its content key:

```
#FILEID|MD5|CHUNK_ID|PRIORITY|MPRIORITY|FILENAME|INSTALLPATH
RetailClient/Overwatch.exe|0ccc2fccebcaf379052ff80bbf51e38d|0|0|255|RetailClient/Overwatch.exe|Overwatch.exe
Manifest/Win_SPWin_RDEV_EExt.cmf|B16ED5CF5EF79F00D8BAB691BF18EB10|0|0|255|TactManifest/Win_SPWin_RDEV_EExt.cmf|
```

The second field is the file's content key. This is where the manifests are
found. 216 lines, 128 of them manifests, in the reference install.

Manifests come in three kinds: `.cmf` (content manifest), `.apm` (asset package
manifest) and `.trg` (resource graph). **The reference install ships no `.apm` at
all** — only `.cmf` and `.trg` — so the package-manifest path is dead code for
this build and the content manifest is the asset registry.

Manifest names encode platform, region and locale: `Win_SPWin_RDEV_EExt.cmf` and
`Win_SPWin_RDEV_LfrFR_text_EExt.cmf`.

### The three `.cmf` kinds are told apart only by name

Base, text and speech manifests all share the `.cmf` extension, so a matcher that
filters on extension and locale alone will happily return a *speech* manifest
when asked for text. The marker segment (`text`, `speech`) is the only
distinction, and it is a silent failure when it is missed: the manifest opens
fine and merges plausible-looking records.

Region has to be matched exactly first and then relaxed, because the names do not
agree: on the reference install the base manifest is `Win_SPWin_RDEV_EExt.cmf`
while the enUS text manifest is also `RDEV`... but the RCN variants exist in
parallel, and a `WinPrism_` build is present too. Requiring one region for every
kind finds nothing for some of them.

### A manifest does not cover everything

Loading the base manifest alone is not enough. On the reference install:

| manifest | records |
|---|---|
| `Win_SPWin_RDEV_EExt.cmf` (base) | 1,208,956 |
| `Win_SPWin_RDEV_LenUS_text_EExt.cmf` | 117,346 |
| merged | 1,326,302 |

The two are **completely disjoint** — no GUID appears in both. Speech adds an
overlapping set again. So the base manifest is not a superset of the localised
ones, and code that reads only the base is missing roughly 117,000 assets.

Merging is add-only-missing with the base authoritative, matching the reference
tooling's registration order.

### Some records are recorded but not present

Being in the registry does not mean the bytes are on disk. A record whose content
key has no entry in the encoding file cannot be opened, and this is not rare: of
the 67 assets of the settings type, only 15 open, and only 1 of those is the
hero-settings file. The other 52 are named in the manifest but their content keys
are absent from the encoding file, which is the signature of content that was
never downloaded. Not verified further, and worth knowing before concluding a
parser is broken.

Still true of the current install (`2.25.0.0.154088`), measured 2026-10-17:
sampling 40 of the 332 `0x0D4` assets, 5 fail and all 5 are encoding-table
misses — `abbc8b151b5ba20316210057d17fe7a5`, `1e51043c7cd1693cf1cc39e312784158`,
`c225acc8efcc462b5531bfe12337224e`, `f1ab44ff546950ae502e0e0c01f8e3b7`,
`fd5cb4dcbb13e2cd853d112642734d4f`. No CDN route can help those: with no
encoding entry there is no ekey, and without an ekey no index names an
artifact (`ow-casc/src/container.rs` `cdn_route_of` reports "in no local
index" — measured for all five).

## Remote fetching (opt-in, live-verified 2026-10-17)

*(This section was measured against the current install, `2.25.0.0.154088`,
product `pro`, branch `eu`; the sections above carry their own build.)*

The client can stream content its install does not have locally; `ow-casc` can
now do the same, **off by default**. `OW_CDN=1|on|true` enables it;
`OW_CDN_CACHE` (default `target/cdn-cache`) and `OW_CDN_TIMEOUT` seconds
(default 60) tune it. The client is `ow-casc/src/cdn.rs`; the fallback hook is
`Store::open_ekey`, and it fires **only when the local store has no answer** —
`KeyNotFound`, or an index entry whose archive file is not on disk. Decoding
of locally present bytes never falls back: a keyring problem is not a CDN
problem. With `OW_CDN` unset every path is byte-for-byte the old behaviour.

### Routes and naming

From the reference's `HttpCDNClient`/`ICDNClient`; every shape below was then
exercised against the live CDN:

* `data/<k[0..2]>/<k[2..4]>/<k>` — a whole file by key (loose files).
* `data/…/<archive-hash>` + `Range: bytes=off-(off+len-1)` — one archive
  slice, the group-entry route.
* `data/…/<hash>.index` — the suffix is raw concatenation; the reference's
  `FetchIndexFile` passes `".index"` *with* the dot.
* `config/<k[0..2]>/<k[2..4]>/<k>` — config files are plain text; no BLTE
  decode (the reference's `FetchConfig` returns raw).
* Bare hosts (`level3.blizzard.com`) get `http://`; https is refused — the
  client has no TLS. Hosts are tried in order and **any** failure falls
  through to the next host, matching `HttpCDNClient`; the last error surfaces
  only when every host failed. (Observed live: level3 answering 403 for a
  nonexistent name, eu.cdn then answering 404 for it.)

Lookup order for a key the local store misses mirrors `ClientHandler`:
archive-group first (Range on the archive the entry names), then the loose
file index (whole GET). This crate additionally consults the
**patch archive group** in between — a deliberate extension: the pinned
reference's `CDNIndexHandler` reads only `archives` + `archive-group`, but the
patch group's index is present locally and patch entries are otherwise
unreachable.

### Index geometry, measured from the install's own files

The `indices/*.index` files (archive group, patch group, loose file index)
share one footer scheme:

* footer at `len - checksumSize - 12`, self-identifying
  (`f[0]=1, f[1]=0, f[2]=0, f[7]=checksumSize`; `checksumSize = 8` everywhere
  in the current install);
* `pageCount = (length - (12 + 2·checksumSize)) / (pageSize + keySize +
  checksumSize)` — the reference's `GetTableParameters`; all pages are full
  `pageSize` bytes;
* then the per-page last-key array (`pageCount × 16`), the page checksums,
  8 pad bytes, the footer, 8 trailing bytes.

Entries are `key[16] | size u32 BE | …` with three measured flavours:

| index | after `size` | entry size |
|---|---|---|
| `archive-group` | archive ordinal **u16 BE**, then offset u32 BE | 26 |
| `patch-archive-group` | archive ordinal **u8**, then offset u32 BE | 25 |
| `file-index` (loose) | — (size is the whole entry payload) | 20 |

The footer's `offsetBytes` field (6 or 5) is the discriminator between the two
group flavours; the patch width was pinned by parsing the first entries of
`6d4cc37a…index`: ordinals 3–81 against a 96-entry `patch-archives` list, and
offsets under 0x1000_0000 — read as one u48 offset they would instead be
≥ 34 GiB. A key is found by binary search over the last-key array (first page
whose last key is `>= key`), then a linear scan of that page, capped by the
footer's element count.

**A page-count trap, now fixed.** The correct numerator is `footerAt −
checksumSize` (= `length − (12 + 2·checksumSize)`), because `footerAt` already
sits inside the file's tail. The old full-file parser (`parse_archive_index`,
removed from `container.rs` on 2026-10-25) subtracted the whole
`12 + 2·checksumSize` from `footerAt` and was **one page short in every file**.
The on-demand `PageIndex` uses the correct expression. The measurements that
exposed it are kept as the regression vector: `indices/e8fa0643…index` (loose
file index) 138 vs 139 true; the 4,148-byte smallest archive index 0 vs 1;
`2f46d225…index` (archive group) 63,147 vs 63,148; `6d4cc37a…index` (patch
group) 11,920 vs 11,921.

### Live evidence (2026-10-17)

* `data/bc/3f/bc3f31c89d64d2bf7f372690079fd211.index` fetched (4,148 B) and
  **byte-for-byte equal** to the local file; same for
  `data/10/64/1064c91150b26f5531ef0402e2635e38.index`. Both are patch/main
  archive indexes, so index files are served under the plain `data` route.
* A **dropped-page key** (`08633eb2b3d351eacf9a4576a14c53c2`: present in the
  archive group, absent from bucket tables and from this crate's parse of its
  own archive's index) was found via the group route — archive
  `1064c911…` at `0x34e86a1`, 608 B — fetched as a Range GET and **decoded to
  2,480 B** end to end (`look --cdn-fetch --cdn-key …`). This is also the
  practical demonstration of the off-by-one above: the same bytes were on the
  local disk, but the parser's dropped page hid them.
* Loose route: `2f7161c0815b4287c18a2b57a637502b` (the install manifest's
  ekey) fetched whole, 4,569 B, `BLTE` magic — matching the file index's
  recorded size exactly. Re-running serves the same bytes from
  `target/cdn-cache` with no network call (verified).
* `5c98ebc39c4ced65486935cb85968b58` (the `encoding` line's first key) is in
  no CDN index and is **not fetchable**: level3 answers 403, eu.cdn 404 for
  `data/5c/98/…`. It is the content key of the same file whose second key
  (`169fff…`, the artifact) opens fine — the pair is ckey/ekey of one file,
  not two files, and only the ekey names an artifact.

## Verified against the client (2026-10-25)

Read in the `Overwatch.exe` Ghidra program: build `2.25.0.1.154504`, base
`0x140000000`, all `.text` pages. Names are in `docs/function-names.csv`.

| what | client function | what was checked |
|---|---|---|
| virtual offset split | `DynamicContainerRead` `0x1424f48d0`; `DynamicContainerReadPage` `0x1424fcd20`; `DynamicContainerOpenArchive` `0x14250c8f0` | the read adds `0x1e` (the 30-byte data header); archive = offset `>> 30`, in-archive offset = `& (2^30-1)`; the archive names `data.%03d` |
| index entry fields | `DynamicIndexDecodeEntries` `0x1425209a0` | 5-byte big-endian location from entry bytes 9..14, 4-byte little-endian size from bytes 14..18 |
| bucket selection | `DynamicKeyStateLookup` `0x142501b40` | XOR of the nine key bytes, `(x ^ (x >> 4)) & 0xf`, then the per-bucket table |
| segment parse | `DynamicIndexSegmentParse` `0x14250fcf0` | section/file headers, version floor 7, widths 4/5/9, 18-byte entries, key-to-entry table |
| index hashes | `0x142512c60` | 32-bit file-header hash and rolling entry hash; all sixteen `.idx` files match the stored values |
| encoding header | `EncodingHeaderParse` `0x142498920` | `EN`, version 1, byte 17 zero, page sizes/counts and espec non-zero |
| encoding sections | `EncodingSectionsRead` `0x1424d1c60` | header, espec, both indexes, both body arrays — the order `encoding.rs` uses |
| BLTE frame header | `BlteFrameHeaderParse` `0x1424a90e0` | type `0x0f`, header size `12 + 24·count`, chunks start at the header size, no hash between the table and chunk 0 |
| BLTE block dispatch | `BlteBlockRead` `0x1424d61b0` | `E` header layout and IV length 0..8, then `N`, `Z` or LZ4 `4` |
| Salsa20 | `Salsa20StateInit` `0x1424e3230`; `Salsa20Xor` `0x1424e32c0` | emulated both client functions with key `01..10`, nonce `10..17` and a non-zero 64-bit counter; the 128-byte output matches standard Salsa20/20. The counter-0 case is pinned in `blte.rs` as `matches_the_client_state_layout_with_every_word_nonzero` |
| empty file | `DynamicContainerRead` `0x1424f48d0` | a 30-byte record returns a zero-length range without opening BLTE |
| `encoding` pair | `BuildConfigParse` `0x1424ccc00` | `vals[0]` is the content key, `vals[1]` the encoding key; only the latter names an artifact |
| keyring | `0x1424cebd0`; `0x14248f040`; `0x14248efd0` | `key-<id>` parses the id as 8 bytes and the key as 16; the id is a little-endian u64, so the written hex is byte-swapped |
| build-info row | `BuildInfoSelectRow` `0x1424aec30`; descriptor table `0x143471150` | Product is row +0, Branch +8, Active +0x10; with a flavor it is the first active row matching Product, with no flavor the last active row if all Products agree |

The Salsa20 check used the client's own code under Ghidra's emulator, not a
reimplementation: the ECRYPT vector alone only covers a zero nonce and one
non-zero key byte, while the all-non-zero vector covers every state word.

### Differences that remain

* Frame type `0x10` (40-byte chunk records with a 16-byte per-chunk key) is
  parsed by the client (`BlteFrameHeaderParse`) but not by `ow-casc`. A full
  scan of all 1,147,131 BLTE headers in this install found type `0x0F` only.
* The client's LZ4 mode `4` (`BlteBlockReadLz4`, `0x1424d6e50`) is not
  implemented. A full scan of every first chunk found 849,632 `Z`, 274,993
  `N` and 22,506 `E`; no `4`, `A` or `F` chunk exists in this install.
* `A` (ARC4) and `F` (frame) chunks are still not implemented.
* The client accepts encoding key widths 1..16; `ow-casc` implements the
  16/16 layout this product writes.
* A bucket can be split across several `data/0X........idx` segments; the
  client opens the segment number named by its container header, while
  `ow-casc` scans and merges every `.idx` file for that bucket. This install
  has exactly one segment per bucket (16 files), so the two agree here.
* The `indices/*.index` page formula (`PageIndex`) is derived from the
  reference's `CDNIndexHandler` and from the measured footers, **not** from
  the client's decompiled CDN handler. The local read path does not use it;
  with `OW_CDN` off, no behaviour depends on it.

## Known gaps

* The 62 trailing bytes of the encoding file, and the 10 bytes after the data
  header's size field, are unidentified. Neither is needed to read data.
* `ecache/` is **not** a content store and is not indexed: its records are
  chunk manifests with no payload (measured above). A key that exists only
  there is not locally readable; the CDN route is the honest answer.
* `indices/*.index` are read only by `cdn_route_of`, on demand. They are never
  merged into local locations: their ordinals, offsets and sizes belong to the
  CDN.
* `shmem` is a named shared-memory mapping (`FUN_14250d8d0`, magic `0x2ab8`),
  not a group index; the reader does not open it.
* `pro/` is untouched; a normal read never visits it. Its 16 v8 `.idx` files
  (131,072 B each, 88 entries total) have no `pro/data.NNN` beside them, so
  there is no content there either.
* `A` (ARC4) chunks, `F` (frame) chunks, LZ4 `4` chunks and frame type `0x10`
  are not implemented. A full scan of all 1,148,620 bucket entries found none
  of them in this install, but the client's block reader handles `4` and its
  frame parser handles `0x10`, so this is a real capability gap rather than a
  format that does not exist.
* The BLTE frame-header hash and the per-chunk MD5s are not checked by
  `ow-casc`. The client's header parser verifies the frame header when its
  caller supplies the hash; the normal read still checks each chunk's decoded
  size, which `ow-casc` also does. This is a corruption-detection difference,
  not a difference for valid content.

