# Entity schema: where the base numbers live

This is the layer that holds a hero's actual combat values — health, armour,
shields, and the rest. It is the first place in this project that a *base* number
comes out, as opposed to the tuning multipliers the settings assets carry.

## The chain

```
STUHero.m_gameplayEntity   ->  an entity asset
    STUEntityDefinition        its root instance
    STUStatescriptSchemaEntry  a (token, value) pair, one per property
        token                  names the property
        value                  a ConfigVar holding the literal
```

Read a hero, follow `m_gameplayEntity`, and the entity asset lists its properties
as schema entries. For the reference install, Reaper's entity is 4,089 bytes and
66 instances, of which 20 are schema entries.

## Entry layout, as it actually appears

```text
STUStatescriptSchemaEntry
  0x07DD813E   the value: an embedded ConfigVar instance
  0x0D09D2D9   the token
```

The value is **embedded**, so it is a separate instance in the instance table
whose `embedder_index` points back at the entry and whose `embedder_field` is
`0x07DD813E`. The value instance's own class decides how to read it, because
`STUConfigVarFloat`, `STUConfigVarInt`, `STUConfigVarBool` and others all share
the same value field hash `0x07DD813E` at the same offset:

| class | literal |
|---|---|
| `STUConfigVarFloat` (0x7B45B325) | `f32` |
| `STUConfigVarInt` (0x12E434A1) | `i32` |
| `STUConfigVarBool` (0xB567AADA) | one byte |

Note what is *not* needed: the entry does not list which properties exist by
name, and the entity does not use the embedded-array machinery that an earlier
guess assumed. Only the instance table's owner links are needed.

## The token is the property key, and it is obfuscated

The token field is a reference like any other, so it must be de-obfuscated with
the entity's header checksum and the token field's own hash. Raw, the tags look
like noise with a common middle (`3f41cbe032763e64`); resolved, each is a clean
GUID:

```
0x3f41cbe032763e64  ->  0x0d800000000000c7   type 0x01C, index 199
```

Type `0x01C` is the same token type the hero-setting identifiers use, and — as
with those — **no asset of that type is shipped anywhere**, so a token can never
be resolved to a definition file. It is a key, not a pointer. The *index* is the
property key.

This is what makes the layer readable without a name table: group entries by
resolved token index and the same index appears across heroes with different
values, which is exactly the behaviour of a property.

## Tokens identified, and how

| token | property | evidence |
|---|---|---|
| 199 | max health | present on every hero, values in health order |
| 1736 | max armour | present only on armoured heroes |
| 221 | max shields | present only on shielded heroes |

The values are checkable, and they pass:

| index | hero | health | armour | shields | total |
|---|---|---|---|---|---|
| 2 | Reaper | 250 | – | – | 250 |
| 3 | Tracer | 150 | – | – | 150 |
| 7 | Reinhardt | 200 | 250 | – | 450 |
| 9 | Winston | 350 | 200 | – | 550 |
| 21 | Bastion | 200 | 100 | – | 300 |
| 22 | Symmetra | 100 | – | 100 | 200 |
| 32 | Zenyatta | 50 | – | 150 | 200 |
| 64 | Roadhog | 700 | – | – | 700 |
| 104 | Zarya | 250 | – | 225 | 475 |
| 303 | Doomfist | 450 | – | – | 450 |
| 318 | Orisa | 275 | 275 | – | 550 |

The identifying detail is the **split**. A reader that reported a single total
would be unremarkable; reporting Symmetra as 100 rather than 225, and Zenyatta as
50 rather than 200, is only possible if the health pool is genuinely separate
from shields. Zenyatta's 50/150 split is one of the most distinctive numbers in
the game, and it falls out of two independent tokens without any hero name being
read.

### Cross-checked against a published source

Armour and shield values match the live published figures **exactly** for the
heroes that have them — Winston 200, Bastion 100, Zarya 225, Zenyatta 150 — which
pins tokens 1736 and 221.

Health values are *lower* than the live patch by 25 to 75 (Reaper 250 against a
published 300, Tracer 150 against 175). This is not a parsing error: it is
systematic across every hero, and armour and shields did not move while health
did, which is the signature of a health rebalance that landed after this
installation's build. **The tokens are right; the patch is old.** Anywhere these
numbers are used, they must be labelled as belonging to build `2.23.0.3.151177`
and not to the live game.

The published figures used for the comparison come from OverFast, which is
MIT-licensed and therefore usable for checking. See `docs/provenance.md`.

## Known gaps

* **8 of 132 heroes are not readable.** Their entity assets are named in the
  manifest but absent from the encoding file, the same "recorded but not
  downloaded" situation described in `docs/formats/casc.md`.
* **D.Va declares no health pool at all** in her entity schema, while every other
  hero does. She is the hero with two forms — mech and pilot — so her pool is
  presumably attached to a sub-entity rather than to the hero entity. Not
  investigated. The `health` command prints `-` for her rather than guessing.
* **Two entries look like pools but are not identified.** Token 1736 is read as
  armour; a token with values of 1000, 3000 and 3600 on three entries belongs to
  something that is not a hero pool, most likely a training bot or objective.
  Nothing here maps the remaining 69 tokens collected from hero entities.
* **Only the pools are read.** Damage, fire rate, projectile speed and cooldowns
  are not among the identified tokens, so the entity schema is not the whole of
  the combat data — the state-script graphs and weapon definitions still hold
  what is missing.
