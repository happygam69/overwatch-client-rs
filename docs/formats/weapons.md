# Weapons, volleys and statescript

A weapon is a `STUWeaponDefinition` whose behaviour is a statescript graph.
The install supplies the graph and its literals; the client's executor
supplies the timing and the damage application.

## Current state

- **File side: read.** Hero → entity → weapon → graph, the volley fields, the
  projectile motions and the `ModifyHealth` literals.
- **Runtime executor: located in the current image.** The drive, node push,
  slot executor, variable pool and graph materializer are read.
- **Open:** the per-node implementation behind the type index, the fire loop,
  and the damage application. No per-shot damage value is claimed.

## The chain

```text
hero            m_gameplayEntity
  entity        STUWeaponComponent
    component   m_weapons: an inline array of STUWeaponDefinition
      weapon    m_graph: STUStatescriptGraphWithOverrides
        graph   a statescript graph asset (type 0x01B)
```

Every graph reference resolves to a real type-`0x01B` asset. The weapon array
is inline and sparse: the first element can be a four-byte empty slot
(`bag_index = -1`), so a dense reader mis-aligns. The overrides wrapper is
12 bytes here (`i32`, then the graph reference), not the 40-byte shape in the
generated type.

## Volley fields

The volley runtime keeps the weapon's numbers in float slots. The named fields
come from the reference types; the value-shape readings are marked:

| field | reading | source |
|---|---|---|
| `numShotsPerSecond` (`0x02F3F5D4`) | rate; `999` means "as fast as input allows", not 999/s | reference name |
| `numProjectilesPerShot` | pellets per pull | reference name |
| `projectileSpeed` | m/s; absent on hitscan weapons | reference name |
| `projectileLifetime` | seconds | reference name |
| `aimID` | aim/assist id | reference name |
| `0xFEC435C6` | cast length (metres); `100` on most hitscan weapons | value shape |
| `0xA55BC904` | minimum shots per burst; `1` on almost all | value shape |
| `0xA17BE89B` | maximum shots per burst; `1` or the 999-family sentinel | value shape |
| `0xECDAB602` | enabled flag | value shape |

The mode rule: a **named `projectileSpeed` literal outranks** the unconfirmed
`castLength?`. A weapon with a speed is a projectile weapon; one with only a
cast length is hitscan with that range. The mode is not guessed from the
weapon's name.

The typed `m_projectileMotions` array is the only way to see the motions: the
raw instance-table walk finds none because the elements are stored as top-level
indices without an embedder back-link. The motion subclass `0xE1A5B579`
carries the stored gravity field (`0x599AB8D7`, `9.8` on the sampled motions).

## Damage data

`STUStatescriptStateModifyHealth` (`0x826613B7`) owns
`STUStatescriptModifyHealth` (`0xA8BC2948`) through `m_modifyHealth`
(`0x0FD4CCC6`). Its `m_amount` (`0xAF872E86`, STU offset `72`) is a
`STUConfigVar`, not a float: read the instance and then the ConfigVar's
`m_value`.

A full-corpus scan finds 4,106 `m_amount` instances across 5,687 graph assets;
627 graphs carry real `ModifyHealth` nodes. The literals are real, but **the
attribution is not done**: the operation enum (`m_DD792FF0`,
`Enum_D4BAD8E7`) decides whether a value is damage, healing, a trigger or a
percentage. Weapon graphs themselves carry none of the weapon damage; the
nodes sit in ability/impact graphs reached at runtime.

The candidate per-weapon carrier is `STUStatescriptSchemaEntry`
(`0x5866C45E`, `{token, value}`) in
`STUStatescriptGraphWithOverrides.m_1EB5A024`. The token is an asset ref to
`STUIdentifier`, an empty type with no shipped assets, so the token cannot be
named from the files; the join has to come from the runtime graph.

## The executor

The script host drives elements through its vtable slot `+0x320`:

| function | address | role |
|---|---|---|
| `StatescriptDriveWalk` | `0x140a264c0` | the per-frame walk; calls element slots 43/44 around a parameter push |
| `StatescriptNodePush` | `0x142141f70` | the parameter push; walks the node chain and dispatches by type index |
| `StatescriptSlot43Executor` | `0x140a9ba70` | the executor step that sets the busy counter |
| `StatescriptGraphMaterialize` | `0x1421f6ae0` | the graph materializer; walks the 0x10-byte node records |
| `StatescriptVarPoolConstruct` | `0x1421530b0` | constructs the runtime variable pool |
| `StatescriptApplyPass` | `0x142155280` | applies each record's default |
| `StatescriptSlotResolve` | `0x142157910` | resolves an element by id |

A node is a 0x2a0-byte wrapper. The dispatch index is at `+0x1a8`; the
executor takes it into a per-graph table and tail-dispatches. The wrapper is
not an STU class. The materializer keeps a first-encounter key order at
`graph+0x1d0` but does not write `+0x1a8`; the index enters as data. The
surviving candidate for its source is the key's low byte, which has to be
confirmed against a live graph.

## Open

1. **The fire loop.** How `numShotsPerSecond` times shots and how a burst is
   shaped. The volley class's own vtable slots are reflection/bookkeeping; the
   behaviour is in the graph executor.
2. **The node implementation behind `+0x1a8`.** One read there yields the
   `ModifyHealth` applying function and the graph's type-index keys.
3. **The weapon → damage join.** Weapon graph → ability/impact graph →
   `ModifyHealth` → resolved amount. The file-side reference walk does not
   reach the ability graphs.
4. **Spread, falloff, ammo and cooldown.** The graphs declare marker variables
   (`m_out_CooldownVar`) but no literals; the consumers are open.

## Verification

`cargo run -p ow-tank --bin params -- --hero 2 --all-graphs` prints the
graph's named parameters and damage literals. There is no combat implementation until the fire and damage paths are read.
