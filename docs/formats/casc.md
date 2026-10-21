# The CASC container

How an Overwatch installation stores bytes, and how to get them back. Every
statement here was checked against a retail install
(`2.23.0.3.151177`, product `pro`, branch `eu`); the checks are named so they can
be re-run.

## Layout on disk

```
<install>/.build.info
<install>/data/casc/config/<xx>/<yy>/<key>     build, CDN, patch and keyring configs
<install>/data/casc/data/<bucket:02X>*.idx     the bucket index
<install>/data/casc/data/data.NNN              the stored records
<install>/data/casc/indices/<archive-hash>.index
<install>/data/casc/ecache/<bucket:02X>*.idx   bucket tables the client cached; merged
<install>/data/casc/pro/                    agent-managed, not read
```

`config/` files are addressed by their own key, two directory levels deep:
`config/<key[0..2]>/<key[2..4]>/<key>`.

For the reference install, `data/casc/data` is ~75 GB across 82 `data.NNN`
archives, with 16 bucket index files and a 103 MB encoding file.

## `.build.info`

A header row names the columns, then one row per branch. Column names carry a
`!TYPE:WIDTH` suffix to strip (`Branch!STRING:0`, `Build Key!HEX:16`, ...).

Those `!` suffixes are not decoration: without a width the hex columns have no
implicit length. Relevant columns: `Branch`, `Active`, `Build Key`, `CDN Key`,
`KeyRing`, `Version`, `Product`.

The row to use is the active one whose product matches the game. For Overwatch
the product is `pro`, and for a retail Windows install the version reads
`2.23.0.3.151177`.

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
`root` carries one, a content key.

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
| 0x08 | `u16` version, must be 7 |
| 0x0A | `u8` bucket number |
| 0x0B | `u8` extra bytes, 0 |
| 0x0C | `u8` size field width, 4 |
| 0x0D | `u8` offset field width, 5 |
| 0x0E | `u8` key width, 9 |
| 0x0F | `u8` file-offset bits, 30 |
| 0x10 | `u64` maximum addressable size |
| 0x18 | 8 bytes of padding |

Then a block header (`i32` byte count, `i32` hash) and that many 18-byte entries
starting at 0x28:

```
EKey[9] | location[5, big-endian] | size u32 little-endian
```

**The two fields disagree on endianness.** The 40-bit location splits as
`archive = value >> 30`, `offset = value & (2^30 - 1)` read big-endian; `size` is
a plain little-endian `u32`. This is the single easiest thing to get wrong here.
It was caught by resolving the encoding file's own key and comparing the size
against the build config's `encoding-size`: `102569617 = 102569587 + 30`, the 30
accounting for the data header.

`size` counts the whole stored record including that header.

The file is preallocated, so a zero-padded tail follows the entries; the entry
count comes from the block header, never from the file length. In the reference
bucket 00 the tail is 43,842 bytes of zeros.

Bucket selection hashes the nine key bytes: XOR them all, then `(i & 0x0F) ^ (i >> 4)`.

Entries are sorted by key, so a bucket can be binary-searched.

## BLTE

A stored record is either raw `BLTE` or preceded by a 30-byte data header:
`md5[16]`, `size u32` at offset 16, then 10 bytes of something that is not
zero-padding in the reference files and is not otherwise used here. Verify the
container by looking for `BLTE` at offset 0, and if it is absent, at offset 30.

After the magic, a big-endian `u32` frame-header size:

* `0` — one chunk covering the rest of the container, its decoded length implied.
* non-zero — at offset 8 sit `0x0F`, a 24-bit big-endian chunk count, and then
  that many 24-byte records of `stored u32 BE`, `decoded u32 BE`, `md5[16]`.
  Chunks begin at offset 12.

Each chunk starts with a one-byte mode:

| mode | meaning |
|---|---|
| `N` | stored |
| `Z` | zlib |
| `E` | encrypted, see below |
| `F` | recursive frame, not implemented |

### The `E` mode

```
u8  key-name length, must be 8
u8  key name[8]                   read as a little-endian u64 to index the keyring
u8  iv length, must be 4 or 8
u8  iv[iv-length]                 a 4-byte IV is zero-extended to 8
u8  cipher, 'S' for Salsa20 ('A' for ARC4 is not implemented)
... ciphertext, whose plaintext begins with the real mode byte
```

The block ordinal is XORed into the low word of the IV, so identical chunks do
not encrypt identically on disk.

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

**Known bug, recorded not fixed:** the shared full-file parser
(`container.rs`, `parse_archive_index`) computes
`(footerAt − (12 + 2·checksumSize)) / perPage`. Because `footerAt` already
sits inside the file's tail, the correct numerator is `footerAt −
checksumSize` (= `length − (12 + 2·checksumSize)`); the parser instead
subtracts the whole 12 + 2·checksumSize from `footerAt` and is **one page
short in every file** (`perPage` always exceeds `12 + checksumSize`).
Measured: `indices/e8fa0643…index` (the loose file index) 138 reported vs 139
true; the 4,148-byte smallest archive index 0 vs 1; `2f46d225…index`
(archive group) 63,147 vs 63,148; `6d4cc37a…index` (patch group) 11,920 vs
11,921. The CDN layer reads its own `PageIndex` with the correct arithmetic;
fixing the shared parser changes existing lookup results and is a separate
decision (see `STATUS-handoff.md`).

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

## Known gaps

* The 62 trailing bytes of the encoding file, and the 10 bytes after the data
  header's size field, are unidentified. Neither is needed to read data.
* `ecache/` **is** read — the container merges bucket `.idx` tables from both
  `data/` and `ecache/`, so a key the client has cached locally resolves
  without the network. `pro/` is untouched; a normal read never visits it.
* `A` (ARC4) chunks and `F` (frame) chunks are not implemented. Neither appears
  in the reference install's gameplay assets.
