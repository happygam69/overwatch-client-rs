# STU: structured data

STU is where Overwatch keeps gameplay data. A hero, a loadout, an ability's state
graph and the generic gameplay settings are all STU assets. This is the format
the combat work depends on.

## Versions

Version 1 begins with the ASCII `STUD`. Version 2 has no magic at all — the
absence of that marker is what selects it — and starts with a CRC64 checksum
over its first 36 bytes. Retail Overwatch 2 assets are version 2; **no version 1
asset ships in the reference install** (both candidate buckets are empty in the
merged registry), so the version 1 reader is exercised by synthetic tests only.

## Version 1 layout

```
u32 magic "STUD"
u32 record count
record[count]              { i32 offset; u32 flags }
```

An instance lives at its record's `offset`, and its index is its position in
the record table (references resolve by **record offset**, not index, so a
later record at the same offset wins — matching the reference). The instance
itself starts with an 8-byte prefix `{ u32 class hash; u32 next }`, and its
fields follow at offsets the *class* declares: version 1 writes no per-instance
field bags, so nothing can be read without the class registry. A record with a
non-positive offset holds no instance — the slot still exists, keeping indices
stable — and an instance's extent runs to the next record's offset.

The field encodings that differ from version 2:

* **Strings**: a 16-byte field `i64 info_offset; i64 pad` (positive = file
  offset, 0 or negative = null). At the offset sits the same
  `i32 size; u32 checksum; i64 text_offset` triple, with `text_offset`
  **file-absolute** (version 2's is dynamic-block-relative).
* **Asset references**: a 16-byte field `u64 pad; u64 guid` — the GUID is
  stored **raw**, not obfuscated.
* **Embedded instance references**: an `i32` record offset into the asset's own
  record table (version 2 stores an instance index).
* **Arrays**: the field is `i32 offset; i32 pad`; at the offset the descriptor
  is `i64 count; i64 data_offset`, both file-absolute; `count == -1` or a
  non-positive data offset is null. Elements are read sequentially from the
  data offset.
* **Hash maps** keep version 2's `i32 offset` shape; version 1's offsets are
  file-absolute because its whole file is the dynamic block.

## Version 2 layout

```
bag<InstanceInfo>          one record per instance
bag<InlineArrayInfo>       arrays embedded directly in a payload
bag<bag<FieldInfo>>        one inner bag per entry
i32 dynamic size
i32 dynamic offset
i32 data offset
```

The three bag headers occupy the first 24 bytes and the three `i32` follow, so
the header ends at byte 36 — which is exactly the length the version-2 checksum
covers.

A bag is `i32 count; i32 offset`, then `count` items at `offset`. **The offset is
absolute**, not relative to the bag, and reading a bag restores the reader to
just after its own header. So the outer field-bag array is walked by stepping the
inner bag headers 8 bytes at a time, while the inner bags' items live elsewhere
entirely.

```
InstanceInfo   u32 type hash; u32 embedder field hash; i32 embedder index; i32 size
InlineArrayInfo u32 type hash; i32 count
FieldInfo      u32 field hash; i32 size
```

Instance payloads sit consecutively in the data buffer, in instance order, each
`size` bytes. `data offset` is where that buffer begins.

## Reading an instance's fields

A payload starts with an `i32` index into the field-bag array; `-1` means no
fields. Offsets are then the running sum of the bag's sizes, with a wrinkle:

```
p = 4
for (hash, size) in bags[bag_index]:
    if size == 0:
        size = i32 at payload[p]     # the size is carried inline, and consuming
        p += 4                       # it advances the cursor first
    field_offset = p
    p += size
```

That inline-size rule is the reason a naive running sum lands on the wrong
offsets, and the reason the bag stores sizes at all rather than offsets.

Field *hashes* are 32-bit, not names. A name table exists in the community
tooling's generated sources but only names a minority of fields; a hash with no
name is just a number, and its width and offset are still enough to skip it
safely. That is what makes a generic walk possible without the full type
database.

**An absent field does not mean zero.** An instance stores only the fields whose
values differ from the class defaults, so most instances are missing most
fields. A reader that treats a missing field as zero reports nonsense — for a
bounded multiplier it reports "0 to 0" instead of "0 to 1". The rule is: a field
absent from the bag takes the value declared on the class in the generated type
definitions. The typed layer now reads those defaults from the local OWLib
checkout's class sources at runtime (`ow-tank::classreg`); the earlier in-repo
stopgap table (`classes::HERO_SETTING_FLOAT_DEFAULTS`) is superseded. One
consequence of the rule: a value equal to the class default is dropped, so
"authored as the default" is not distinguishable from "absent" in the bytes —
the typed layer reports a stored value equal to the default as `Defaulted`, and
among the measured hero-setting floats none does (see *What the hero-settings
assets actually contain* below).

For the reference install, the `STUHero` instance of hero 2 has 50 fields where
the community type definition lists around 68 attributes, and that gap is
exactly this rule. The field *set* also drifts between builds: two hashes appear
in this asset that the published type does not describe, and a field the
published type calls `m_internalName` is absent. Treat any field-name table as a
hint, not a specification.

## Strings

A `teString` field is an `i32` offset into the **dynamic block**, not text:

```
at payload[field_offset]:     i32 dynamic offset     (-1 means null)
at dyn_data[dynamic offset]:  i32 length
                              u32 checksum
                              i64 offset           (absolute, into the asset)
text: length bytes at that offset
```

Both hop lengths are needed: a field is a valid string only if both offsets land
inside their blocks, which is also a cheap way to *find* string fields when the
hash is unknown. Version 1 replaces the `i32` field offset with a 16-byte
`i64 info_offset; i64 pad` that points at the same triple, and its
`text_offset` is file-absolute (`§ Version 1 layout`).

## Asset references, and their obfuscation

A reference field stores an 8-byte GUID, obfuscated so that static analysis of
one asset reveals nothing:

```
mixed = field_hash | (field_hash << 32)
bytes = (stored ^ mixed ^ header_checksum) as 8 little-endian bytes
swap bytes 0 and 3, 7 and 1, 2 and 6, 4 and 5
```

`header_checksum` is the CRC64 over the asset's first 36 bytes. The CRC64 is
reflected, initial and final values inverted, polynomial `0x95AC9329AC4BC9B5`
(in reflected form) — derived here from the published table, and checked by
rebuilding that table. Version 1 stores the GUID **raw** in a 16-byte field
(`u64 pad; u64 guid`); the obfuscation and the checksum are version 2's.

**This is verified, not assumed.** Deobfuscating `m_gameplayEntity` on all 132
heroes yields GUIDs that all exist in the asset registry, on the entity type
`0x003`. A wrong checksum, a wrong field hash or a wrong swap order produces 64-bit
values that resolve to nothing; there is no way to get 132 of 132 by accident.

## Reading an asset

```rust
let mut stu = data.stu_of(0x02e0_0000_0000_0002)?;      // hero 2
let hero = stu.instance(classes::HERO).unwrap();         // STUHero
let entity = stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY);
let colour = stu.read_rgba(hero, classes::hero::HERO_COLOR);
```

Reading it back — the resolved reference, its type, whether it is registered,
plus the field layout — is the quickest way to see whether a build's field set
matches what a name table expects.

## The typed layer

Hashes become classes, names, enum members and defaults through
`ow_stu::types::ClassRegistry`, which `ow-tank::classreg` populates from the
local OWLib checkout's generated class sources at runtime — **no generated code
is copied into this repository**; without the checkout the layer is simply
empty and the raw readers still work. On the reference checkout the load is
**518 files, 7,782 classes, 644 enums, 23,204 declared fields**, with zero
unknown bases, field types or unparsed defaults.

* `stu.typed_field(instance, hash, &reg)` — one field; `Value::Missing` when the
  class declares it and this instance does not carry it.
* `stu.typed_fields(instance, &reg)` — everything the instance carries, in
  payload order. Version 2 walks the payload's own bag, so a field the registry
  does not know still appears (as raw bytes); version 1 walks the class's field
  order and reads each at its declared offset.
* The readers cover primitives, enums (underlying width + member names), the
  `Math` structs (`teVec2/3/3A/4`, `teQuat`, `teColorRGB(A)`, `teMtx43A`,
  `teUUID`, `teEntityID`), strings, asset refs, embedded and inline instance
  refs, arrays of all of the above, and hash maps (`u64 key -> Option<instance
  index>`, the index in the value's low 32 bits, `-1` = null, a `-1` field
  offset = empty map).
* A stored value equal to the class default comes back as
  `Value::Defaulted(value)`; a declared-but-absent field comes back as
  `Value::Missing`.

Probe: `cargo run -p ow-tank --release --bin stu -- --registry` (load stats),
`--class 0x7C3457DC` (one declaration), `--guid 0x0ca0000000000126 --fields`
(every instance typed), `--instance 1 --fields --deep` (recursing into in-place
instances).

## Where the combat numbers are

Data-driven, in STU, but scattered across a state-graph layer that is only
partly mapped:

| what | where |
|---|---|
| hero definition, theme, category, loadout list | `STUHero` (`0x7C3457DC`) |
| an ability or weapon as a player sees it: name, description, button, icon, category | loadout assets (`0x09E`) |
| weapon behaviour | `STUWeaponDefinition` (`0x61386B75`), whose body is a state-script graph |
| the actual numbers — damage, cooldowns, projectile speed, health pools | scalar nodes inside those graphs, e.g. `STUConfigVarFloat` values, and components such as `STUHealthComponent` (`0xA658CA1C`) |
| ability cooldown and stack outputs | `STUStatescriptStateAbility` fields, for example `m_out_CooldownVar` |
| **per-hero tuning multipliers, with declared bounds** | the hero-settings assets (type `0x054`): `STUHeroSettingFloat` carries a default, a minimum and a maximum |

`LoadoutCategory` distinguishes weapon, ability, passive, ultimate, perk, hero
stats and subrole, which is the cleanest hook into a combat model.

### What the hero-settings assets actually contain

These were the first target because a float that declares its own minimum and
maximum identifies itself, whatever its field hash is called. Having read them,
the honest summary is a correction to what was expected of them.

On the reference install the hero-settings asset is `0x0ca0000000000126`
(index 294), **63,775 bytes, 1,027 instances, holding 393 `STUHeroSettingFloat`
instances** (re-measured directly 2026-10-17; this section previously said
61,706 / 992 / 377, and the old "42 bounds defaulted" and "59 defaults other
than 1.0" counts were wrong too — the figures had not been re-taken after the
reader's last change, and both the raw reader and the typed layer now agree on
the numbers here). Of the 393, **44 leave both
bounds absent** (so they take the class defaults: min 0.0, max 1.0) and
**386 carry an authored `m_default`, none equal to 1.0; the 7 without one read
0.0** (the pipeline drops any value equal to the class default, so an authored
1.0 could not be distinguished from an absent field even if it occurred). The
values are percentages of the normal behaviour:

| default | min | max | step | meaning |
|---|---|---|---|---|
| 0.06 | 0.0001 | 1.0 | 0.0001 | a very small adjustable fraction |
| 0.30 | 0.0 | 1.0 | 0.01 | |
| 0.80 | 0.0 | 1.0 | 0.01 | |

So these are **not the base combat numbers.** They are the multipliers a player
can move in a custom game — 1.0 means normal, 0.8 means 80% — which makes them a
vocabulary of *tunable* quantities per hero rather than a source of damage and
cooldown figures. That is still useful: it enumerates what the game considers
adjustable, and it exercises the whole reading path. It is not what a combat
model needs first.

Two further limits found by reading them:

* **The settings do not nest in an array.** All 1,027 instances — the root set,
  one record per hero, and the settings — are listed in the instance table and
  tagged with what embedded them, so they can be read directly, without
  walking the embedded-array links (which is how the first pass read them,
  before the typed layer could follow arrays).
* **Settings cannot be named from here.** Each carries an `m_identifier`
  reference, but its target is a type for which **no asset is shipped anywhere in
  the install** (zero assets of that type exist in the merged registry, out of
  1.3 million). `STUIdentifier` is also a class with no fields. The identifiers
  are tokens, not files, so attaching a name to a setting needs the localised
  text layer, not the settings assets.

A second, independent source of per-match facts is the replay container, if this
ever needs validating against real play rather than against definitions. The
reference tooling parses a `prp` block holding a format version, a build number,
map and gamemode GUIDs, and a parameter block with a start and end frame, an
expected duration, and the heroes in the match, followed by a compressed payload.
The frame payload itself is not publicly decoded anywhere, so positions and
damage events are not reachable that way today.

### Still open

The base numbers — damage, health, cooldowns, projectile speeds — live in the
state-script graphs and in components such as `STUHealthComponent`. Reaching them
means reading nested and embedded instances' *fields* rather than just their
records, and deciding which scalar nodes are load-bearing. They have no
self-declaring bounds to identify them by, so this is the harder problem the
settings assets were mistaken for.

**None of the scalar values are mapped yet, and no tool in the reference
ecosystem dumps them either** — the shipped extractors stop at names and icons.
Reaching them means walking a state graph and deciding which scalar nodes are
load-bearing, which is the next real piece of work. It is also work that needs
the field-name table to go much further than it currently does, so expect to
identify numbers by their position and by what changes when they change, rather
than by name.

Fields whose meaning was *verified* by reading real values, and which can
therefore be trusted:

| field | hash | evidence |
|---|---|---|
| `m_gameplayEntity` | `0x950BBA06` | resolves to a registered entity asset for all 132 heroes |
| `m_heroColor` | `0xE25DDDA1` | four floats in 0..1 with alpha exactly 1, and hero-appropriate theme colours |
| `m_gender` | `0x7D88A63A` | small enum |
| `m_heroSize` | `0xAF4EC410` | enum; **absent** from the reference build's hero assets |

One caveat on `m_heroColor`: on its own it establishes that a hero has a
plausible theme colour, not *which* hero it is. Two things together make the
identification solid, though.

The hero GUID indices are the game's own ordering — index 2 upwards — and that
ordering is long-standing and widely published. Reading `m_heroColor` for those
indices gives colours that match those heroes individually, which is what turns
the ordering from a claim into a verified reading. Of the first 32 indices, 20
are self-identifying:

| index | colour | identifies |
|---|---|---|
| 2 | dark maroon | Reaper |
| 3 | orange | Tracer |
| 4 | pale cream | Mercy |
| 8 | strong blue | Pharah |
| 10 | purple | Widowmaker |
| 21 | olive green | Bastion |
| 32 | gold | Zenyatta |
| 41 | bright green | Genji |
| 66 | red | Cassidy |
| 101 | yellow | Junkrat |
| 104 | pink | Zarya |
| 110 | navy | Soldier: 76 |
| 121 | green | Lúcio |
| 122 | pink | D.Va |
| 221 | blue | Mei |
| 302 | purple | Sombra |
| 315 | blue | Ana |
| 318 | green | Orisa |
| 418 | purple | Moira |
| 458 | orange | Wrecking Ball |
| 518 | light blue | Echo |
| 545 | teal | Baptiste |

Several of these are unambiguous: pink appears for exactly Zarya and D.Va, and
teal and light blue for exactly Baptiste and Echo. Note also that indices 7
(Reinhardt) and 571 (Sigma) carry the *same* grey — both are grey-clad, so a
shared palette entry is not surprising, but it is a reminder that colour alone
is weak evidence for any individual hero.

**This table is documentation, not repository data.** The public hero-ID lists
that would let a program map index to name are either unlicensed or
non-commercially licensed, so no name table is shipped here. The clean-room path
to names is the install itself: the localised name of a hero lives in a text
asset, which is the same route the interface takes.

## Known gaps

* **Version 1 is implemented but has no real-data run**: no version 1 asset
  ships in the reference install (the `0x05A`/`0x05E` buckets are empty), so
  the layout above is covered by synthetic tests in `ow-stu` and skipped by
  the real-install gate.
* **Inline instance arrays are read** (see `ow-stu`'s `inline_elements`): an
  `i32` count followed by payloads that each carry their own field bag, including
  the empty `-1` elements that occur in practice.
* **Embedded arrays and hash maps read through the typed layer**, element
  classes resolving via the registry. The *raw* walk still follows only the
  instance table's embedder links, so instances stored as plain table indices
  without a back-link are invisible to it — measured on volley motions
  (`m_projectileMotions` elements are `STU_E1A5B579`, a
  `STUStatescriptWeaponProjectileMotion` subclass, stored top-level; raw sees 0,
  typed sees all 8 in the sample — see `docs/formats/weapons.md`).
* Version 2 structures whose inner layout differs from the generated type must be
  followed by their bytes; the weapon overrides wrapper is one, and it is
  documented in `docs/formats/weapons.md`.
* The scalar node types that hold combat numbers are unmapped.
