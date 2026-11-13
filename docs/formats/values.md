# The reflection value system

Status: **identified, not ported** — engine infrastructure, out of the
gameplay split (`dev/corpus.py` lists `reflection` as out of scope next to
`engine`). It is recorded because the image's largest *unread* body lives here
and because a gameplay feature that reads a value must read it through this
system's layout.

What is read: the variant record, its destructor, its text formatter, the
lazily-decoded format-string table, and the structure of the 35,848-byte
reader. What is not: the reader's 113 case bodies.

## The variant record

A value is a 32-byte record:

| offset | width | field |
|---|---|---|
| `+0x00` | 8 | payload — a pointer for the heap kinds, the value inline for the scalar kinds |
| `+0x08` | 4 | element count (array kinds) |
| `+0x0c` | 4 | **tag** |

Tags seen in the read functions:

| tag | meaning | evidence |
|---|---|---|
| `1` | boolean, inline in the low byte at `+0x00` | `ValueToText` case 0 reads `*param` as a byte |
| `2`, `3` | scalar kinds, no allocation | the destructor ignores them |
| `5`, `0xb` | one pooled allocation at `+0x00` | `ValueDestroy` returns it to the pool at `0x144137230` |
| `0xc`, `0xd` | array; elements are `0x10` (tag `0xc`) or `0x20` (tag `0xd`) bytes; `+0x08` is the count | `ValueDestroy` walks `count * stride` and destroys each element |
| `0xff` | unspecified / empty | the reader assigns `0xff` to an empty input |

`ValueDestroy` `0x140583c50` (272 B, **464 callers**) is the destructor every
holder of a value calls; it is inside the read set and can be used to confirm
the tag map.

## The text vocabulary

The value system prints values through format strings that are stored
**XOR-obfuscated in `.rdata` and decoded in place on first use**. The record
shape, from `ValueToText` `0x14193f560`:

| offset from the record | field |
|---|---|
| `+0x00` | 8-byte key |
| `+0x08` | a dword: bits 0..23 = length, bits 24..31 = the "already decoded" flag |
| `+0x0b` | the same flag byte, as the guard reads it |
| `+0x0c` | the ciphertext: `plain[i] = cipher[i] ^ key[i & 7]` |

The first record is at `0x14398f610` (key `379b4ed22c318269`, length 2) and
decodes to `%d`. Walking the table from there yields the vocabulary of a
variant printer — vectors, quaternions, int4, arrays, objects, and the
unspecified marker:

```text
%d   %f   %s   %.4f   %016llx
[%f, %f, %f]            [%f, %f, %f, %f]
(%.3f, %.3f, %.3f)      (%.3f, %.3f, %.3f, %.3f)
(%d, %d, %d, %d)
Array of %d             array of %d
Object                  object
Unspecified             0 (unspecified)
true                    false
```

Two spellings differ only in case (`Array of %d` / `array of %d`,
`Object` / `object`) — the client has a "verbose" and a "compact" form.

`ValueToText` `0x14193f560` switches on `tag - 1` and uses these strings; the
`%.3f`/`%.4f`/`%016llx` set is what a component's field dump looks like.

## The 35,848-byte reader (`0x14217f060`)

This is the largest body in the image that this project has not read
(`dev/fn.py biggest ow-tank --depth all`). Its shape is now known even though
its cases are not:

- **Extent**: `0x14217f060..0x142187c68`, four `.pdata` pieces —
  176 + 35,103 + 79 + **490** bytes. The last piece is the epilogue
  (`0x142187a7e..0x142187a9d`) followed by a **data table** at
  `0x142187aa0..0x142187c64`.
- **The table is a 113-entry switch table**: 113 little-endian four-byte RVAs,
  112 distinct, spanning `0x14217f262..0x1421878e9` — every entry a target
  inside the body. The dispatcher adds the image base to an entry
  (`lea`/`mov` with `0x2187aa0` as the displacement) and jumps to it, which is
  why no plain `jmp [mem]` shows up in a linear scan.
- **113 is a count, not a guess**: the guard is `cmp eax, 0x71` before the
  table read, and the table is exactly `0x71 * 4 = 452` bytes.
- **The entry contract**: the fifth argument is an output record the function
  initialises — a word at `+0x00`, dwords at `+0x04` and `+0x10`, and a
  `std::vector`-shaped member constructed at `+0x28`. The third argument is an
  object holding a **string** (`[obj+0x20]` data, `[obj+0x28]` capacity, with a
  16-byte inline form selected by `capacity <= 1`). An **empty string ends the
  call early**, and then the fourth argument — when non-null — is set to tag
  `0xff` ("unspecified") through `0x141941500`.
- So it reads *from* an object that carries a string, *into* a value with an
  array member: a text-to-value reader with one case per registered kind.
- **The output is optional**: the callee tests the fifth argument for null
  before initialising it, and `0x142157bf0` calls with only four arguments —
  a deliberate "parse for effect, keep nothing" call. That null test is the
  independent confirmation of which argument is the output.
- **Why the corpus calls it opaque and Ghidra times out**: the switch table
  sits inside the function's own `.pdata` extent, so a linear walk from the
  entry decodes it as code and desyncs; and Ghidra's function object covers
  only the first 30,404 bytes, still too large for its decompiler to finish
  (`decompile_failed: timeout`, both over MCP and headless).

### Its callers

| caller | bytes | what it does before the call |
|---|---:|---|
| `0x142153ab0` | 191 | asks an object's vtable `+0x10` slot whether it is type token **`0x143d04610`**; if not, calls the reader with the object |
| `0x142157bf0` | 427 | the same type test, then reads into a temporary and builds through `0x1421580a0` / `0x142158450` |
| `0x14217e4a0` | 48 | a virtual slot itself — **no callers**, and its two `.rdata` slots (`0x143399fc0`, `0x143399ff8`) belong to vtables that nothing references statically |

`0x143d04610` lies in the image's **zero-fill `.data` tail** with readers and
no writer: the token is the *address* of the slot, used as a type identity, not
a stored id. `docs/ghidra-image-audit.md` §2.1 explains why reading it always
gives zero.

## The helper family it calls

827 call sites into **87** distinct functions, dominated by one cluster:

| region | count | what it is (read) |
|---|---:|---|
| `0x14193d1c0..0x141942e50` | 36 fn | the value/string library: `ValueToText` `0x14193f560`, a c-string→value assign `0x141942e50`, recursive release `0x14193e940`, plus ~30 per-kind helpers not read |
| `0x14217e570..0x14217eed0` | 6 fn | the array helpers, including `0x14217e790` — a vector erase that destroys `0x18`-byte elements from an index |
| `0x140583c50` | 1 fn | `ValueDestroy`, called **67 times** (one per error path) |
| `0x141c4b210` | 1 fn | pool release (71 sites) — calls the pooled object's vtable `+0x60`, `+0x48` and `+0x40(5)` |

**`0x141942e50` is 0x160 bytes from `TypeIndexByHash` `0x141943290`**, and the
pool release is 0x180 from `RegisterTypeWithSize` `0x141c4b050`; the type
registry's own functions sit in the same two pages
(`RegisterStaticValueObject` `0x141c25110`, `RegisterFloatValueRecord`
`0x141c5bfc0`, `TypeRegistrationCore` `0x141c5f5e0`). That places the value
system unambiguously inside the reflection/type registry.

## Open

- The 113 case bodies: what each registered kind parses and which tag it
  produces. This is the actual read expression, and it is one case at a time.
- What the 113-entry index *is* — a type id, a registered-type index, or a
  token kind. The guard `cmp eax, 0x71` bounds it; the producer of the index
  is not read.
- The scale constants in the body: `1,000,000` appears as a bound
  (`cmp edx, 0xf4240`) and `1000` and `0x80000008` as values; which kinds use
  them is not read.
- `0x14217e4a0` and its vtables: the class that owns the reader is not named.
- Whether any gameplay feature reads a tunable through this path. If it does,
  the gameplay read site is the unit to port, not this dispatcher.

## Reproducing the decode

Read 1,024 bytes at `0x14398f610`, then for each offset where the dword at
`+0x08` has bits 0..23 in `1..24` and a clean bit 24, XOR the next `length`
bytes with the 8-byte key at `+0x00` (key byte index `i & 7`). Eighteen records
decode to printable ASCII; the first is `%d`.
