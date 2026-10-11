# STU: structured data

STU is where Overwatch keeps gameplay data. A hero, a loadout, an ability's state
graph and the generic gameplay settings are all STU assets. This is the format
the combat work depends on.

## Versions

Version 1 begins with the ASCII `STUD`. Version 2 has no magic at all — the
absence of that marker is what selects it — and starts with a CRC64 checksum
over its first 36 bytes. Retail Overwatch 2 assets are version 2; version 1 is
not implemented here.

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
definitions. `ow-tank` encodes this as
`classes::HERO_SETTING_FLOAT_DEFAULTS`.

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
hash is unknown.

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
rebuilding that table.

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

The probe prints the resolved reference, its type and whether it is registered,
plus the field layout, which is the quickest way to see whether a build's field
set matches what a name table expects.

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
(index 294), 61,706 bytes, 992 instances, and it holds **377 float settings**.
Of those, 42 leave both bounds at the class default and 59 have a default other
than 1.0. The values are percentages of the normal behaviour:

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

* **The settings do not nest in an array.** All 992 instances — the root set, one
  record per hero, and the settings — are listed in the instance table and tagged
  with what embedded them, so they can be read directly. The embedded-array
  links do not have to be walked, which is why this works before array support
  does.
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

* Version 1 STU is not implemented.
* **Inline instance arrays are read** (see `ow-stu`'s `inline_elements`): an
  `i32` count followed by payloads that each carry their own field bag, including
  the empty `-1` elements that occur in practice.
* Embedded arrays in the dynamic block are located but their elements' *fields*
  are not read, which is what the weapon-graph overrides need.
* Version 2 structures whose inner layout differs from the generated type must be
  followed by their bytes; the weapon overrides wrapper is one, and it is
  documented in `docs/formats/weapons.md`.
* The scalar node types that hold combat numbers are unmapped.
