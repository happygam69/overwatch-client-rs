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
<install>/data/casc/ecache/, pro/              agent-managed, not needed to read
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
`Win_SPWin_RDEV_LfrFR_text_EExt.cmf`. Skip the locale-tagged ones and match the
platform and region prefixes.

## Known gaps

* The 62 trailing bytes of the encoding file, and the 10 bytes after the data
  header's size field, are unidentified. Neither is needed to read data.
* The `ecache/` and `pro/` directories are untouched; a normal read never
  visits them.
* `A` (ARC4) chunks and `F` (frame) chunks are not implemented. Neither appears
  in the reference install's gameplay assets.
