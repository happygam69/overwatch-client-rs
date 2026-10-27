# STU: structured data

STU is where Overwatch keeps gameplay data. A hero, a loadout, an ability's
state graph and the generic gameplay settings are all STU assets. This is the
format the combat and entity work depends on.

## Versions

Version 1 begins with the ASCII `STUD`. Version 2 has no magic — the absence of
that marker selects it — and starts with a CRC64 checksum over its first 36
bytes. Retail Overwatch 2 assets are version 2. **No version 1 asset ships in
this install**, so the v1 reader is exercised by synthetic tests only.

## Version 2 layout

```text
bag<InstanceInfo>          one record per instance
bag<InlineArrayInfo>       arrays embedded directly in a payload
bag<bag<FieldInfo>>        one inner bag per entry
i32 dynamic size
i32 dynamic offset
i32 data offset
```

The three bag headers occupy the first 24 bytes and the three `i32` follow, so
the header ends at byte 36 — exactly the length the checksum covers.

A bag is `i32 count; i32 offset`, then `count` items at `offset`. The offset is
**absolute**, not relative to the bag; reading a bag restores the reader to
just after its own header.

```text
InstanceInfo    u32 type hash; u32 embedder field hash; i32 embedder index; i32 size
InlineArrayInfo u32 type hash; i32 count
FieldInfo       u32 field hash; i32 size
```

Instance payloads sit consecutively in the data buffer, in instance order,
each `size` bytes. `data offset` is where that buffer begins.

## Reading an instance

A payload starts with an `i32` index into the field-bag array; `-1` means no
fields. Offsets are the running sum of the bag's sizes, with one wrinkle:

```text
p = 4
for (hash, size) in bags[bag_index]:
    if size == 0:
        size = i32 at payload[p]     # inline size
        p += 4
    field_offset = p
    p += size
```

**An absent field does not mean zero.** An instance stores only the fields
whose values differ from the class defaults. A missing field takes the default
declared on the class in the generated type definitions. The typed layer reads
those defaults through `ow_stu::types::ClassRegistry`, populated at runtime
from the local OWLib checkout; without the checkout the raw readers still
work.

A stored value equal to the class default is reported as `Value::Defaulted`;
a declared-but-absent field is `Value::Missing`.

## Strings

A `teString` field is an `i32` offset into the dynamic block:

```text
at payload[field_offset]:     i32 dynamic offset     (-1 = null)
at dyn_data[dynamic offset]:  i32 length
                              u32 checksum
                              i64 offset           (absolute, into the asset)
text: length bytes at that offset
```

Both hops must land inside their blocks; that is also a cheap way to find
string fields when the hash is unknown.

## Asset references

A reference field stores an 8-byte GUID, obfuscated:

```text
mixed = field_hash | (field_hash << 32)
bytes = (stored ^ mixed ^ header_checksum) as 8 little-endian bytes
swap bytes 0 and 3, 7 and 1, 2 and 6, 4 and 5
```

`header_checksum` is the CRC64 over the asset's first 36 bytes, reflected,
initial/final inverted, polynomial `0x95AC9329AC4BC9B5` (reflected form).
Version 1 stores the GUID raw in a 16-byte field (`u64 pad; u64 guid`).

Deobfuscating `m_gameplayEntity` on all 132 heroes yields GUIDs that all exist
in the asset registry, on the entity type `0x003`. A wrong checksum, field hash
or swap order produces values that resolve to nothing.

## Version 1 layout

```text
u32 magic "STUD"
u32 record count
record[count]              { i32 offset; u32 flags }
```

An instance lives at its record's `offset`; its index is its position in the
record table. References resolve by **record offset**, not index, so a later
record at the same offset wins. The instance starts with
`{ u32 class hash; u32 next }`; fields follow at class-declared offsets.
A non-positive offset is an empty slot; an instance's extent runs to the next
record's offset.

V1 differences: strings are a 16-byte `i64 info_offset; i64 pad` pointing at
the same triple, with file-absolute `text_offset`; asset refs are raw;
embedded refs are record offsets; arrays are `{i64 count; i64 data_offset}`
with file-absolute offsets; hash-map offsets are file-absolute.

## The typed layer

`ow_stu::types::ClassRegistry` turns hashes into classes, names, enum members
and defaults. `ow-tank::classreg` populates it from the local OWLib checkout's
generated sources at runtime — no generated code is copied into the
repository. On the reference checkout: 518 files, 7,782 classes, 644 enums,
23,204 declared fields.

- `stu.typed_field(instance, hash, &reg)` — one field; `Value::Missing` when
  the class declares it and the instance does not carry it.
- `stu.typed_fields(instance, hash, &reg)` — everything the instance carries,
  in payload order. V2 walks the payload's own bag; v1 walks the class's field
  order.

The readers cover primitives, enums, the `Math` structs, strings, asset refs,
embedded/inline instance refs, arrays and hash maps.

## MSTU: a model's own structured data

`MSTU` is a model asset's structured-data chunk: a two-`i64` header naming the
offset and size of an STU blob inside the chunk's payload.

```text
0x00  i64 offset   relative to the MSTU payload start
0x08  i64 size     STU blob length from `offset`
0x10  ...          the STU blob (offset 16 on every model inspected)
```

The offset is payload-relative, not asset-relative. On 60 of 60 models the STU
parses at `payload[offset..offset+size]` and on none at
`asset[offset..offset+size]`. `size` can leave trailing bytes; slice by `size`.
`ow-tank::mstu` exposes `parse_header` / `parse`, and `Model::mstu` is filled
when the chunk is present.

## Entities and health

```text
STUHero.m_gameplayEntity   ->  an entity asset
    STUEntityDefinition        its root instance
    STUStatescriptSchemaEntry  a (token, value) pair, one per property
        token                  names the property
        value                  a ConfigVar holding the literal
```

The entry is:

```text
STUStatescriptSchemaEntry
  0x07DD813E   the value: an embedded ConfigVar instance
  0x0D09D2D9   the token
```

The value instance's class decides how to read it: `STUConfigVarFloat`
(`0x7B45B325`) is `f32`, `STUConfigVarInt` (`0x12E434A1`) is `i32`,
`STUConfigVarBool` (`0xB567AADA`) is one byte. The token is an asset ref to an
empty `STUIdentifier` type; its **index** is the property key.

Identified tokens:

| token | property | evidence |
|---|---|---|
| 199 | max health | on every hero, values in health order |
| 1736 | max armour | only on armoured heroes |
| 221 | max shields | only on shielded heroes |

The split is the proof: Symmetra reads 100 health / 100 shields, Zenyatta
50 / 150, rather than a single total. The values are the install's; health
differs from the live patch, so label them with the build.

## Where the combat numbers are

| what | where |
|---|---|
| hero definition, theme, category, loadout list | `STUHero` (`0x7C3457DC`) |
| ability or weapon as a player sees it | loadout assets (`0x09E`) |
| weapon behaviour | `STUWeaponDefinition` (`0x61386B75`), a statescript graph |
| damage, cooldowns, projectile speed | scalar nodes in graphs, `STUConfigVar*` values |
| health pools | entity schema entries (above) |
| per-hero tuning multipliers | hero-settings assets (`0x054`), `STUHeroSettingFloat` with min/max/default |

The hero-settings floats are **multipliers** (1.0 = normal), not base combat
numbers. They enumerate what a custom game can tune; they do not supply
damage or cooldowns.

## Gaps

- v1 STU has no real-data run.
- The scalar node types that hold combat numbers are not mapped; see
  `formats/weapons.md`.
- The raw instance walk only follows embedder links; instances stored as
  top-level indices without a back-link are invisible to it (typed layer sees
  them).
- Hero entity coverage has not been re-counted through the bundle-aware asset
  path. D.Va declares no health pool on the hero entity.

## Verification

```text
cargo run -p ow-tank --release --bin stu -- --registry
cargo run -p ow-tank --release --bin stu -- --guid 0x0ca0000000000126 --fields
cargo test --release -p ow-stu
cargo test --release -p ow-tank
```
