# The content manifest

A `.cmf` is the asset registry: it maps a 64-bit asset GUID to the content key
that stores the asset. It is also the only part of the pipeline that uses
symmetric encryption beyond BLTE's chunk cipher.

## Header, stored in the clear

The first 48 bytes are not encrypted even when the body is:

| offset | field |
|---|---|
| 0x00 | `u32` build version |
| 0x04 | `u32` unknown |
| 0x08 | `u32` unknown |
| 0x0C | `u32` unknown |
| 0x10 | `u32` unknown |
| 0x14 | `u32` unknown |
| 0x18 | `u32` unknown |
| 0x1C | `i32` data patch record count |
| 0x20 | `i32` data record count |
| 0x24 | `i32` entry patch record count |
| 0x28 | `i32` entry count |
| 0x2C | `u32` magic |

The magic carries a `cmf` marker in bits 8..31. When the top byte is not part of
that marker the field is byte-reversed relative to the clear form, and its top
byte after reversal is the format version:

```
magic 0x636D661B -> (magic >> 8) == 0x00636D66  "cmf"  -> encrypted
                    swap -> 0x1B666D63, version = 0x1B = 27
```

The reference install reports build version **150851**, 1,208,956 data records
and 357 entries.

## Key derivation

The key and IV are generated from a 512-byte table and a handful of numbers taken
from the header, so the ciphertext is bound to the build: the same numbers that
encrypted it are the ones that decrypt it.

The derivation differs **per build**, and the table differs per build too — 252
distinct tables appear in the reference tooling's sources. This is why a build
whose derivation is not implemented cannot be opened even with correct key
material, and why the failure looks like garbage rather than a missing key.

For build 150851:

```
key:  cursor = table[data_records & 511]
      repeat 32 times: emit table[cursor]; cursor += data_records   (wrapping u32)

iv:   cursor = table[(2 * sha1(name)[13] - 16) & 511]
      repeat 16 times: emit table[cursor]
                       cursor += cursor_start % 29
                       emit ^= sha1(name)[(cursor + data_records) % 20] + 1
```

`name` is the manifest's file name with extension, ASCII, hashed with SHA-1.
For the reference install that is `Win_SPWin_RDEV_EExt.cmf`.

**AES-256, CBC, no padding.** The key is 32 bytes. This is worth stating because
the framework the algorithm came from asks for a 128-bit key at the call site;
using the first 16 bytes does not decrypt. The check that settles it is that one
choice yields a GUID-sorted record list and the other yields noise.

## Body

Decrypted, starting right after the 48-byte header:

```
entries: entry_count * 20 bytes
records: data_count  * 29 bytes
  u64 LE guid
  u32 LE size
  u8     unknown
  16     content key
```

The reference file is 35,066,928 bytes; the sections account for all but 16
bytes at the end.

**The record list is sorted by GUID.** That is not a convenience — it is what
makes correctness checkable. A wrong key, IV or derivation produces records that
are not sorted, and the sortedness test is what identified AES-256 over AES-128.
It also means the registry can be binary-searched.

## Asset identity

The GUID's type is a 12-bit field at bits 48..59, stored with its bits reversed:

```
mangled = (guid >> 48) & 0x0FFF
type    = reverse_12_bits(mangled) + 1
index   = guid & 0xFFFFFFFF
```

Confirmed two ways: the transform reproduces the published `types.txt`
permutation table row for row, and the decoded types land on known landmarks —
132 assets of type `0x075`, all on the `0x02E00000` GUID prefix heroes use.

Counts observed in the reference install:

| type | count | meaning |
|---|---|---|
| `0x004` | 191,599 | texture |
| `0x00C` | 30,402 | model |
| `0x054` | 67 | STU generic settings |
| `0x075` | 132 | hero |
| `0x09E` | 848 | loadout / ability |
| `0x0C5` | 129 | game mode |
| `0x0CF` | 11 | lootbox |
| `0x134` | 1,350 | talent / perk |

164 distinct types in total. The type numbers come from which registry the
reference tools switch on, not from anything in the game files; a type the tools
do not name is just a number.

## The resource graph

`.trg` is the other manifest kind and is used by builds at or above version
68309. It carries the dependency graph between assets. Reading an asset by GUID
does not need it — the content manifest is sufficient — so it is not parsed here
yet. Its header uses the same 512-byte table with different constants (the walk
is seeded from the skin count and stepped by the package count), which is
recorded so the work is small when it is wanted.

## Known gaps

* Only the build 150851 derivations are implemented. Adding a build means adding
  one function, the way the reference library carries one file per build.
* The 357 "entry" records are not interpreted. They are part of the patch and
  package machinery, and a clean retail install needs nothing from them.
* The `unknown` byte in each record is not interpreted.
