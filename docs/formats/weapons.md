# Weapons and state-script graphs

## Current state

- **File side: read** - loadout/volley fields, fire interval, projectile
  speed/lifetime/motion, and the damage candidates.
- **Class surface: read** - `m_amount` literals labelled, 48 typed-only
  motions inventoried.
- **Open:** the compiled damage node behind the type table, and live
  confirmation of the four `?` readings (cooldown/damage).

**Verify:** `cargo test --release -p ow-tank` (data-side); damage stays
supplied, so there is no client-side gate for it.


Following a hero to its weapon behaviour. This is where damage, fire interval
and projectile speed should be — and this document is mostly about how far the
chain got and what is still missing, because the honest answer is that the
numbers have not been extracted yet.

## The chain

```text
hero            m_gameplayEntity
  entity        STUWeaponComponent
    component   m_weapons: an inline array of STUWeaponDefinition
      weapon    m_graph: STUStatescriptGraphWithOverrides
        graph   a statescript graph asset (type 0x01B)
```

Walking that chain for all heroes prints each weapon's graph. On the
reference install:

```
 index   slot  graph                  type  present
     2      1  0x0580000000000019    0x01B  true
     3      1  0x0580000000000016    0x01B  true
     6      1  0x0580000000001683    0x01B  true
     6      2  0x058000000000168c    0x01B  true
    10      1  0x058000000000001b    0x01B  true
...
71 heroes with weapons, 56 graph references (56 present), 56 empty slots,
8 heroes unreadable
```

**Every one of the 56 graph references resolves to an asset that exists and is of
the graph type.** That is the check on this layer: an off-by-one in the element
stride or a wrong field offset yields 64-bit values that resolve to nothing, and
none do.

Two shapes had to be handled to get here, and both are traps:

* **The weapon array is inline and sparse.** Its first element is empty in the
  reference install — a slot with `bag_index = -1` that occupies only four bytes.
  A reader that assumes elements are dense reads the next element's bytes as its
  own and produces a plausible-looking wrong graph.
* **The overrides wrapper is 12 bytes here**, a leading `i32` then the graph
  reference. The generated type for the same class describes a 40-byte shape with
  the reference *first*. The bytes were followed, not the type.

## Named parameters, and the wall behind them

A literal is meaningless until the *field* that owns it is named, and an embedded
instance records the field hash it arrived through. So joining `(owning class,
field hash)` to a name turns "there is a float of 250" into "this weapon's
projectile speed is 250". Doing that join for every hero gives:

```
54 heroes, 56 graphs, 889 named parameters
```

Distinct parameters found:

| count | parameter |
|---|---|
| 333 | `value` |
| 245 | `timeout` |
| 90 | `destination` |
| 53 | `projectileLifetime` |
| 51 | `numProjectilesPerShot` |
| 38 | `numShotsPerSecond` |
| 21 | `answer` |
| 18 | `projectileSpeed` |
| 16 | `aimID` |
| 9 | `radius` |
| 7 | `amount` |
| 3 | `castLength` |
| 2 | `animRecoilImpulseFactor` |
| 1 each | `speed`, `speedScalar`, `castRadius` |

Real readings, with names attached — Widowmaker's graph, for instance:

```
STUStatescriptStateWeaponVolley.projectileSpeed = 250
STUStatescriptStateWeaponVolley.projectileLifetime = 10
STUStatescriptStateRayCast.castLength = 100
STUStatescriptStateWait.timeout = 0.5
```

The names are not guesses. They come from the reference tooling's generated
types, which name the fields of classes it has identified; a field it has not
identified is called `m_1A2B3C4D` and is **dropped** rather than printed, so
"named" and "not named yet" stay distinguishable. Adding coverage is adding
identified classes, not changing code.

### Two readings a graph forces, and neither is a parsing bug

**A graph can declare several firing modes, and nothing says which is
primary.** Widowmaker's weapon graph declares `numShotsPerSecond` of both 2 and
1 — an unscoped and a scoped rate — and a `projectileSpeed` of 250 belonging to
a thrown mine, not to her rifle. Every distinct value found is kept (they are
all in the table above); the mode has to be resolved by preference instead: a
`castLength` means a raycast, which is direct evidence of hitscan, and that
outranks a projectile speed. Without that preference her rifle would be
simulated as a projectile weapon, which it is not.

**`numShotsPerSecond = 999` is not a rate.** It means "as fast as input
allows", and it appears on weapons whose real cadence comes from somewhere
else. Treating it as a number gives 999 shots a second; clamping it gives 60,
which is equally invented. The honest reading is that no rate was declared, and
the simulation defaults to 1/s with an explicit override.

Both are cases where the file does not contain the answer, so the reading has
to say so rather than pick a number.

### Damage: a thorough negative result

Per-shot damage was searched for in seven places, and it is not in any of them.
Recording the search matters as much as recording a find, because it stops the
next person repeating it:

| where | result |
|---|---|
| named fields of weapon graphs | 889 named parameters, **none named damage** |
| generic `value` parameters in weapon graphs | 333 of them, semantics not resolvable without graph topology |
| projectile entities reached from `m_entityDef` | 4 to 5 instances each, visual components only |
| every 4-byte field of all 123 readable hero entities | 72 distinct fields, none a plausible damage value |
| weapon definitions | two shapes only (`m_graph`, `m_script`); no override arrays |
| hero loadout lists | `m_heroLoadout` is empty in this build |
| all 124 manifests in the root file, unioned | the entity references that fail to resolve are in **none** of them |

One thing the search did turn up is real, correctly-read health-modification
amounts. `STUStatescriptStateModifyHealth` owns a `STUStatescriptModifyHealth`
whose `m_amount` is a `STUConfigVar`; following that to its literal gives actual
figures, and only six heroes have any:

| hero | amounts |
|---|---|
| 41 | 50, 400 |
| 121 | 70 |
| 221 | 5 |
| 302 | 10 |
| 810 | 90 |
| 963 | five readings of 1e-05 |

50 on hero 41 is Swift Strike's damage, and 1e-05 is the classic "tiny damage
used as a trigger". So these are ability and heal figures, not weapon damage —
useful, but not the thing being looked for.

**A trap worth recording.** `m_amount` is declared as a `STUConfigVar` with an
embedded reader, so the bytes at that field are the **index of an instance**, not
a number. Reading them as a float yields a denormal such as `8.1e-44` — a value
that looks like data and is not. Every reading in the table above goes through
the instance table's owner links to the ConfigVar and reads *its* `m_value`, and
`ow-tank`'s `FieldNames` now carries each field's declared type so the two cases
cannot be confused.

### So where is it?

Two possibilities remain, and they are distinguishable:

1. **Inside the generic `value` parameters.** Then it is a data problem and needs
   the graph's links walked, as described below.
2. **Computed rather than stored** — the graph fires and the simulation derives
   damage. Then no amount of data reading will find it, and the executable is
   needed.

The simulation therefore treats damage as an explicit input — `ow-view --play
--damage <f>` supplies it — while every ballistic parameter is read from the
install, rather than inventing numbers and presenting them as extracted.

### Damage is not a named field

**No parameter in any weapon graph is named `damage`.** That is the wall, and it
is worth being precise about because two different explanations lead in two
different directions:

* It is hidden in one of the 333 generic `value` parameters.
* It is not in the weapon graph at all — the weapon graph may only *fire*, with
  the damage applied by whatever the projectile or raycast hits.

The generic `value` parameters are generic for a structural reason.
`STUStatescriptActionSetVar` has `m_value`, `m_key`, `m_index` and `m_out_Var`,
and `m_out_Var` is of a class, `STU_076E0DBA`, that **declares no fields at
all**. It is a marker wrapping "the variable being written". So *which* variable
is set is not written down as a name anywhere — it is expressed by the graph's
links, which is the same as saying the meaning is in the topology.

Resolving that means following the graph: from a set-var, along its output, to
the node that consumes the variable, and identifying that node. That is
interpreting a visual scripting language rather than reading a record, and it is
the next real problem — the same one flagged above, now with a precise address.

## Why the numbers are not here yet

A graph is not a record, it is a **program**. A graph asset holds:

| what | class |
|---|---|
| nodes | `STUStatescriptBase[]`, `STUStatescriptState[]` |
| links between them | `STUGraphLink` |
| typed connection points | `STUStatescriptInputPlug`, `STUStatescriptOutputPlug` |
| entries, the places execution starts | `STUStatescriptEntry*` |
| numeric literals | `STUConfigVarFloat`, `STUConfigVarInt`, nested inside the states |
| sync variables shared across the graph | `STUStatescriptSyncVar[]` |

Graphs are shipped and addressable: 5,566 assets of type `0x01B`, and one for a
weapon is 6 to 11 KB with 80 to 160 instances.

So the floats are reachable in principle. The problem is that nothing says which
float is the fire interval. Two of them might be `0.5` and `0.05` with no
distinguishing feature beyond which node owns them, and identifying that node
means understanding the semantics of a visual scripting language: which state
class fires a weapon, which input plug takes a rate, which output feeds damage.

That is a different kind of work from everything so far. The pools were found by
grouping tokens across heroes and checking the values; here there is no
equivalent shortcut, because the *structure* carries the meaning rather than an
identifier. Three ways forward, roughly in order of cost:

1. ~~Per-hero overrides.~~ **Run, and ruled out.** Across all heroes, weapon
   definitions come in exactly two shapes: 55 carrying only `m_graph` and 18 only
   `m_script`. No weapon definition carries the override array `m_1EB5A024`, so
   there are no per-weapon overrides in this build and the numbers must be in the
   graphs.
2. **Node semantics by class.** Enumerate the state classes actually used by
   weapon graphs (`STUStatescriptStateWeaponVolley`,
   `STUStatescriptWeaponVolleyLaunch`, `STUStatescriptWeaponProjectileMotion`,
   ...) and read the config vars they own. `STUStatescriptWeaponProjectileMotion`
   is the one to start with: it has exactly two config vars, and projectile
   motion has exactly two parameters worth having, speed and gravity.
3. **Cross-checking against published values.** Damage and fire rate are on the
   wiki for every hero, so a candidate can be confirmed or killed quickly. The
   wiki is non-commercially licensed, so it is usable to check a value and not to
   build a dataset from — see `docs/provenance.md`.

None of this needs the executable. It is all still data — just data shaped like a
program, so it needs reading rather than looking up. If a value turns out to be
computed rather than stored, that is when the executable would become necessary,
and the `0x05800000` graph assets are the place to check first.

## Known gaps

* Per-shot damage is not in the files; spread, ammo and falloff are still
  unread. Fire interval and projectile speed are read from the volley state's
  slots — see the update at the end.
* The 56 empty weapon slots are not explained. Some are genuinely empty slots;
  whether others are weapons stored somewhere else is not established.
* 8 of 132 heroes have entities that are recorded but not downloaded, so they
  contribute nothing here either.

## Update: the volley state's geometry, and what `0xFEC435C6` is

The volley state read cleanly once every graph's literals were dumped with
hashes (the `weapon-raw` dump, ~6,700 rows across 57 graphs) and joined to the
published weapon tables. What the join shows:

* **`0xFEC435C6` is the volley's cast length, not damage.** Its per-hero values
  {100 on most heroes, 150/170/180/200/400/500/1000 on specific ones} line up
  with a *range* figure and with exactly one literal the named-params output
  already knew in another class (`STUStatescriptStateRayCast.castLength = 100`
  for Widowmaker). The volley runtime keeps it at slot 0x118, beside
  `projectileLifetime` (0x110) and `projectileSpeed` (0x128). Damage in a
  *range* unit is nonsense while "cast length in metres" fits every hero's
  number, so the reading is: cast length.
* The volley state is a **trigger pull** record: the volley runtime object's
  56 float slots carry `projectileMotions` (0xd0), `projectileEntity` (0xf8),
  `projectileVisual` (0x100), `aimID` (0x108), `projectileLifetime` (0x110),
  `castLength` (0x118), `projectileSpeed` (0x128), …
  `numProjectilesPerShot` (0x1c0), `numShotsPerSecond` (0x1c8). The published
  tables' `Shots per volley` column fits the same idea: Reaper fires a
  20-pellet volley, Tracer 2 bullets, Genji 3 shurikens per volley.
* The remaining 40 unnamed volley slots are in `dev/volley_slots.txt` with the
  addresses their values were extracted at; none was damage under the per-hero
  join.
* **The motions are read through the typed layer** (2026-10-17): a volley's
  `m_projectileMotions` (`0x351BD93F`) is an embedded
  `Array(Instance(STUStatescriptWeaponProjectileMotion 0x487AE227))`, and the
  real motions are stored as instance indices to **top-level** instances, so
  the raw reader — `volley::volleys_of`, which resolves children through the
  instance table's embedder links and matches the base class exactly — finds
  none of them. Measured over heroes 1–6 (17 volleys in 4 of 5 weapon graphs):
  0 raw-visible motions, 8 typed-only, every one class `0xE1A5B579`
  (`STU_E1A5B579`, a subclass with two extra fields `0x599AB8D7`/`0xD0C4CF49`)
  and embedder-less, on graphs `0x05800000000000b9` (hero 5) and
  `0x0580000000001683`. Any motion sweep must use the typed array read; the
  raw walk is blind to this storage shape by construction
  (`tests/stu_typed_real.rs`).
* **Per-shot damage stayed out of the files** — the seven previous file-side
  places plus this attempt now make eight. The `ow-sim` combat path keeps
  taking it as an explicit input, and `ow-view --play` wires it as
  `--damage <f>`, so the harness runs without inventing figures.

## The damage nodes are in the graph data after all

The earlier per-shot-damage search walked the *weapon* graphs for named
parameters and found none. A targeted round this session found where damage
actually lives:

* `STUStatescriptModifyHealth` (class `0xA8BC2948`) carries **`m_amount`**
  (`0xAF872E86`, STU offset 72) as a `STUConfigVar`; the wrapping state
  `STUStatescriptStateModifyHealth` (`0x826613B7`) holds it at offset 200 via
  `m_modifyHealth` (`0x0FD4CCC6`) plus an operation enum (`m_DD792FF0`,
  `Enum_D4BAD8E7`, default `x60671788`) and two bytes.
* A full-corpus scan — new tool
  `cargo run -p ow-tank --bin params -- --hero N|--graph 0x… [--field 0x…] [--all-graphs]` —
  finds **4,106 `m_amount` instances across the 5,687 graph assets**, of which
  the ones on the real node read as integer literals: `10000` ×102, `1` ×94,
  `50` ×92, `9999` ×66, `999` ×46, `75` ×44, `999999` ×39, `30` ×30, `5` ×26,
  `25` ×26, `500` ×23, `20` ×21, `40` ×19, `10` ×37 …
* **Weapon graphs carry none of them.** Neither do the heroes' shared worker
  manager script (`0x0580000000000015` for the heroes checked) nor the per-
  weapon `m_script` pointers — the hits inside weapon graphs are a different
  class that reuses the same field-name hash (`0xCEBDAF17`, always `Int(1)`).
  The real nodes sit in ability/system graphs (e.g. `0x058000000000001a`).
* Per-weapon wiring is therefore a **join still to make**: weapon →
  (ability graph | impact graph) → `ModifyHealth` node → `amount`. The
  candidate carrier for the per-weapon value is
  `STUStatescriptSchemaEntry` (`0x5866C45E`, the "statescriptEntry" anchor the
  systems-plan lists): `{token ref (STUIdentifier), STUConfigVar value}`, held
  in `STUStatescriptGraphWithOverrides.m_1EB5A024` — the per-parameter override
  list on each weapon's graph wrapper. This build's wrapper is the 12-byte
  shape `weapons.rs` follows, so whether the entries are present there is the
  next read.

Status: the *literals* are extracted (this section); the *semantics* — which
are weapon damage, which are heal/percent/multiplier (the `10000`/`9999`
cluster looks like percentage-style values, not points) — is decided by the
operation enum and the graph wiring, and is the open item.

### Attribution: what the damage-node inventory is, and the two tools

Two CLIs carry this work (both read-only, both in `ow-tank`):

```text
cargo run -p ow-tank --bin params  -- --hero N | --graph 0xGUID [--field 0xHASH] [--all-graphs]
cargo run -p ow-tank --bin refwalk -- (--hero N | --from 0xGUID) [--depth D] [--field 0xHASH]
```

* `params` prints the named parameters of the selected graphs (`all_params_of`,
  whose field names come from the reference types). With
  `--field 0xAF872E86` it prints every `ModifyHealth.amount` literal.
* `refwalk` follows every asset reference, instance by instance, from a hero's
  gameplay entity (or any asset), bounded by `--depth`, and reports the
  statescript graphs reached and the damage literals inside them.

**Inventory so far** (build `2.25.0.0.154088`, retail install):

| fact | value |
|---|---|
| `m_amount` instances across all 5,687 graph assets | 4,106 (includes a name-hash collision class `0xCEBDAF17`, always `Int(1)`) |
| graphs holding real `STUStatescriptModifyHealth` nodes | **627**, 937 nodes |
| common literals | `10000`×102, `1`×94, `50`×92, `9999`×66, `999`×46, `75`×44, `999999`×39, `30`×30, `5`×26, `25`×26, `500`×23, `20`×21, `400`×19 |
| note | the `10000`/`9999` cluster reads like percentage-style values (a basis-point scale), not damage points; the operation enum decides and is not yet interpreted |
| example value sets per graph | `0x…2020`: {400, 9999, 160, 20}; `0x…562d`: {30, 80, 40}; `0x…1688`: {10000×3, 800, 25} |

**The attribution is not done, and two paths are already closed:**

* the schema-entry **token names are unobtainable**: the token is an asset ref
  to `STUIdentifier`, an empty 8-byte type with no shipped assets — so a token
  cannot be named from the files, and the per-weapon override list is absent
  from this build's 12-byte wrapper (`weapons.rs` follows the bytes);
* the reference walk is **not** the join. `refwalk` now follows single refs
  *and* inline ref arrays, and roots at both the hero asset and its gameplay
  entity; `--hero 2 --depth 5` and `--hero 3 --depth 5` reach **14,072 / 15,739
  assets and exactly one graph each** — no damage graph, no loadout graph. The
  hero's ability graphs are therefore **resolved at runtime** (by token or by a
  name-keyed table), not through asset references, so a purely file-side join
  from hero to damage node does not exist as a walk.

Remaining routes to the per-shot number, in order of expected yield:

1. **The live process** (operator's call): the instantiated graph nodes carry
   their resolved `amount` values in memory, and ownership names the hero —
   a read of the runtime objects, no code needed.
2. **The v2b full-process capture** — the same idea on the standing snapshot:
   the currently loaded heroes' node objects and their resolved values should
   be resident; finding them is an object-shape hunt in 10.6 GB.
3. Re-opening the code route with the new fact that the volley's methods are
   gap-bound (see below) — a fresh capture of three exact address ranges makes
   the fire path readable and the damage application follows the same route.

### The volley class, read from the captures (this round)

Three separate pieces of the volley came out of the captures and the live
session, all committed to `docs` and `D:/ow-dumps`:

**1. The registrar was captured all along, and it decodes.** The range
capture's `7ff64b73e000_00006000_00040000.bin` covers the registrar entry
(`range 0x7ff64b73f190`); the function is the standard registration idiom —
`lea rcx,[field descriptor]; mov r8d, <field hash>; mov [rsp+0x20],
<runtime offset>; call register` — and one call per field. Parsing the whole
0x6000 window (capstone, `dev/` tooling) yields the **complete field tables of
four classes**; the volley's is the run of 57 fields at runtime offsets
`0x100..0x2BD` containing `numShotsPerSecond` (`0x02F3F5D4`) at **0x1C0**,
then `0x1C8..0x2B0` and two byte fields at `0x2BC`/`0x2BD`. Full table:
`D:/ow-dumps/trace/volley_fields.txt`. The field descriptors themselves live
at `range 0x7ff64e088a68..0x7ff64e0896e0` and are captured with the same dump
(layout: `{class ptr, offset, type ptr, flags, field hash, self}` — no name
string; names still need the OWLib hash join).

**2. The live vtable is fully readable and its class-specific slots are
infrastructure.** The volley vtable at live `0x7ff711888c30` (range
`0x7ff64d1cac30`) reads completely, and its 8 class-specific slots point into
`range 0x7ff64b735xxx` — a region that was **zero in every standing capture**
back then and was **captured live this round** to
`D:/ow-dumps/live_warm3/volley_slots/region_0x7ff70fdf5000_0x2000.bin`
(the registrar-adjacent 0x3000-region dump came with it). It is warm in the
current standing image now (`docs/capture-coverage.md` measures it), so the
region is no longer a gap. Disassembly of the
four unique targets shows reflection and bookkeeping only — two
`lea rax,[static]; ret` descriptor getters, two RTTI equality checks
(`mov rax,[rdx+0x88]; cmp [global],rax; sete al`), and two registered-object
list scans — the same "the class computes nothing" shape the mover class
showed. The firing behaviour is therefore not in these slots; it runs through
the statescript graph execution path — whose per-frame drive and per-node
dispatch are now read in `docs/formats/statescript.md` (drive
`FUN_7ff7c5770230` at host-vtable slot `+0x320`; node dispatch by type index
`node+0x1a8` via `FUN_7ff7c6ed4dc0`).

**3. The live firing path works and is drivable.** The hero-select grid is
click-navigable (damage row: 53 px spacing, row 2 third icon = Soldier: 76;
verified by the on-screen name each click), Soldier fires with the pulse
rifle (ammo 30→25→21→20 across CUA drags), and the ultimate meter rises ~6%
per hit — consistent with Soldier's ~19 damage per shot at the game's charge
rate. The per-shot damage number itself is still not directly extracted; the
designed live probe (before/after heap delta of exactly one shot on a bot,
restricted to the bot-actor windows) is ready but needs the current session's
actor-family addresses re-resolved after the re-entry.

### Every `m_amount` site in the capture, read (this round)

The capture contains exactly nine occurrences of the `m_amount` field hash
`0xAF872E86` as an immediate in the module's text. Scanning the standing
capture (`module_v2full.bin`, file offset → capture VA by
`VA = offset + 0x7ff649e60000`) and reading each site classifies all nine:

| capture address | idiom | what it is |
|---|---|---|
| `0x7ff64a142e90`, `0x7ff64b474dcd`, `0x7ff64b67fe91`, `0x7ff64b683c61` | `mov r8d, 0xAF872E86` | field **registration** (hash handed to the register call), the idiom already documented for the volley table |
| `0x7ff64b67b0de`, `0x7ff64b67dfc2` | `cmp edx, 0xAF872E86` | field **predicates** — same family as `0x7ff64b67d230` and `0x7ff64b67d580`, which Ghidra decompiles whole: per-class functions that open with `mov byte [r9], 1` (a touched marker), then chain `cmp edx, <field hash>`; the `m_amount` branch dereferences the object and tail-jumps a vtable slot (`jmp [rax+0x18]`), a delegate-and-compare shape |
| `0x7ff64b7c3fbe`, `0x7ff64b7c8dc7` | `cmp edx, 0xAF872E86` | the same predicate family in another class (`FUN_7ff64b7c8db0` decompiles: hash branches set defaults 0/1/10 and return), no arithmetic |

Two of the decompilable ones are **default providers**: `FUN_7ff64b67d230`
answers the `m_amount` hash with the float `1.0f` and `FUN_7ff64b7c8db0`-class
code answers it with `10`; the delegating one forwards to the field's own
getter. None of the nine applies damage.

That closes the "read the hash sites" route: the `m_amount` literal is read by
the reflection layer (registration, predicates, defaults), and the per-shot
arithmetic must run in the graph-execution path that consumes the resolved
node — consistent with the volley finding that firing runs through the
statescript executor rather than class methods. The three sites at
`0x7ff64b67b0de`, `0x7ff64b67dfc2` and `0x7ff64b7c3fbe` sit in code the
standing captures carry but that no analysis has reached (no function entry);
they were read by disassembling their windows in `module_v2full.bin`
(coordinates above) rather than by decompiling a function.

One connective detail worth keeping: the float constant at capture
`0x7ff64ce43ae4` (module VA `0x7ff7c7d03ae4`, a float) is loaded both by the
`m_amount` predicate at `0x7ff64b67b0fe` and by the MDLC deserializer
(`docs/formats/materials.md`), so the material path and this field path share
a global float constant — a NaN/epsilon-style default rather than anything
damage-specific.

### Volley literal readings, typed motions, and the loadout walk (this round)

File-side only, no capture: a throwaway census probe over the retail install
(build `2.25.0.0.154088`) walked every hero's weapons and every volley, then
was deleted — the code that stays is `volley::motions_of_typed`,
`volley::is_motion_shaped`, `entity::{loadout_guids_of_hero, ability_of,
loadouts_of}`, `weapons::modify_health_amounts` and
`graph::ability_nodes_of`. Probe outputs (outside the repo):
`D:/ow-dumps/combat/loadout_census*.tsv`. Counts: 135 hero assets, 73 with
weapon slots, 56 graphs reached, 174 volleys, 291 modify-health nodes in
weapon graphs, 16 ability states in weapon graphs.

**Volley literals: what the `?` names now mean.** The five reference-typed
names stand (`numShotsPerSecond`, `numProjectilesPerShot`, `projectileSpeed`,
`projectileLifetime`, `aimID` — *reference*, from the OWLib generated types).
Four more fields carry value-shape readings (*assumed*), with the install
distributions that motivate them (*install*, 149–174 volleys per field):

| field | reading | install distribution |
|---|---|---|
| `0xFEC435C6` | `castLength?` | {f100×56, i100×19, i200×19, f400×18, i1000×13, i180×10, i150×5, …} — metre-scale, and the one named `castLength` the raycast state carries is 100 for Widowmaker |
| `0xA55BC904` | `minShots?` | i1 ×153 of 157 declaring, else i2/i3 — near-constant 1 |
| `0xA17BE89B` | `maxShots?` | i1 ×106, sentinel-scale 999 ×14 / 9999 ×13 / 99999 ×15, else i2/i3/i100 — the same 999-family the rate uses for "uncapped" |
| `0xECDAB602` | `enabled?` | boolean, true on 128 of 153 declaring volleys |

The min/max pairing is not just parallel shape: hero 41's volleys carry
(A55, A17) = (2, 2) and (3, 3) with pellet count 1 — fixed 2- and 3-shot
bursts — while hold-to-fire volleys pair rate 999 with A17 = 1. The `?`
stays on all four until a second source (capture or live object) agrees.

Still unnamed with measured distributions (*install*, no reading assigned):
`0x3B0AD973` (bool true ×12), `0x79CE614D` (bool true ×7), `0xB752FBAE`
(f 0.5 ×2), `0x5F40D9D3` (i1/i2 ×1 each), `0xF2810377` (unread class
`0xF0D5C4EB` ×2), `0xEA1CCF04` and `0x465B08A6` (bool true ×1 each),
`0x27713F07` (f 0.1 ×1). The volley runtime keeps ~40 such slots
(`dev/volley_slots.txt`); none was damage under the per-hero join.

**Motions: the typed layer sees all 48, and their literals are inventoried.**
The raw walk finds 0 of 174 volleys' motions; the typed
`m_projectileMotions` array finds 48, every one class `0xE1A5B579` with no
embedder back-link, every one accepted by the declaration check
(`is_motion_shaped`: the class declares the two base motion fields up its base
chain). Their 76 stored literals span 21 fields — the complete file-side
inventory of projectile-motion numbers:

* The base fields have **no measured values**: `0x30A41C77` children are always
  `STUConfigVarBool` with no stored `m_value` (zero literals in 48 motions),
  and `0x468EC3BE` is never carried by any sampled motion. Both names stay
  hedged (`speed?`, `gravity?`) with nothing behind them.
* `0x599AB8D7` reads **`gravity?`** (*assumed*): `f 9.8` on 5 motions across
  heroes 5, 6 and 810 — the Earth-gravity signature, on the subclass field that
  sits where the base gravity field would. The subclass keeps the base's first
  field and replaces the second, which is why the base gravity slot is empty.
* The rest is single-digit-occurrence shape, documented not named (*install*):
  `0x717EAFE7` ints {1000×5, 100, 300, 2500, 5000}, `0x43832542` ints
  {180×4, 90×4, 105, 361}, `0xD0C4CF49`/`0x02DDA941` bool true, sub-second
  floats (0.05/0.1/0.2/0.5/0.6/0.08/0.0625), and one `i -9999` sentinel beside
  an `i 2` at the same field `0x81D5D395`.

**The loadout walk: `m_heroLoadout` is empty in this build (*install*,
measured, twice).** Hero 2's field (`0x77FED604`, STU offset `+0xd7`, 4-byte
inline count) reads count 0, and all 135 heroes enumerate zero loadouts —
the reader is not mis-shaped (a wrong stride yields 64-bit values that resolve
to nothing; here the count itself is 0). So per-hero ability enumeration has
nothing to walk, and `ListAbilities` parity is the global list instead: 899
assets of type `0x09E`, 591 readable on this install, **every readable one
carrying a `STULoadout` instance of class `0x07A0E32F`** (*install* — this is
also what pins the class hash the new reader uses). Two reference notes
(*reference*, OWLib `STULoadout.cs` / `Loadout.cs` / `STUHero.cs`): the hidden
/ secondary-weapon rule is `m_0E679979 >= 1` / `== 2`, the button label is two
hops (`m_logicalButton` → `STU_C5243F93` → its `m_name` at `0xC08C4427`, not
the loadout's own `m_name` hash), and the category codes are the
`LoadoutCategory` enum 0–6. One stale-constant finding: `classes::LOADOUT`
(`0x32290C2E`) matches **no** class in the reference types and is used by no
reader; the loadout code carries the install-measured `0x07A0E32F` locally and
`classes.rs` was left untouched.

**Cooldown and damage stay hedged, now with the exact reason attached.**
Weapon graphs hold 16 ability states (`graph::ability_nodes_of`, class
`0xC38B92B4`), but the cooldown is a marker variable
(`m_out_CooldownVar`, `0xBBC9EFFF`) — present as a child, never a literal —
so no seconds exist file-side. Damage amounts in weapon graphs are now
attributable per graph (`weapons::modify_health_amounts`, 291 nodes), but
whether each is damage, heal, trigger or percentage-scale is still the
operation enum's call. `Ability.cooldown` / `Ability.damage` are therefore
`None` by construction and display as `cooldown?` / `damage?`.

### Sandbox integration (this round, no new capture)

File-side only, plus decompiles of what the standing captures already carry.
No live capture was taken; the volley fire loop and the damage application
stay where the ledger left them (graph-executor runtime code, gap-bound —
row 5's next anchor).

**What the sandboxes now read** (*install* throughout, build `2.25.0.0.154088`):

* the full mover: base speed plus every other float the mover component
  declares (`ow_tank::entity::mover_fields_of` — back/side/crouch/terminal
  slots, the one integer field skipped), so both harnesses keep the
  install's slots instead of defaulting them;
* the primary volley per weapon graph (`volley::weapon_figs_of`, same
  qualification order as `weapon_model_of`): literal pellets, projectile
  lifetime, and the typed-motion gravity (`m_599AB8D7`, `9.8` where stored,
  via the runtime class registry — `None` without `OW_OWLIB`). **The mode
  rule was corrected by a full-hero census** (`D:/ow-dumps/wfigs_census.txt`,
  the throwaway `wfigs` probe; 52 heroes carry a picked volley): a *named*
  `projectileSpeed` literal outranks the `?`-assumed `castLength?` **on the
  picked volley** — the earlier "raycast outranks projectile" preference was
  reasoned at the *graph* level (several volleys aggregated, Widowmaker's
  mine vs rifle) and is superseded here. Measured: hero 5 = speed 85 +
  gravity 9.8 + pellets 5 on a volley that also carries cast 1000 (a
  projectile weapon, not a 1 km hitscan), hero 810 = speed 70 + gravity 9.8;
  every other hero carries no speed literal, so `castLength?` is the hitscan
  range and the 200 m default stands where even that is absent;
* damage as *candidates*: the modify-health literals across the hero's
  weapon graphs ride in the HUD note unlabelled (`--damage` stays the
  explicit input);
* abilities as join state: the per-hero walk is still empty (measured
  0/135 heroes), so the note reports the per-hero count against the global
  `0x09E` count with `cooldown?`/`damage?` staying `?`.

Mechanics that changed with it: `ow-sim` projectiles now carry and apply
their gravity per sub-step (was a `0.0` no-op), and the `ow-view` harness
fires pellets per pull with the volley's range, flying projectiles with the
volley's speed/gravity/lifetime where declared. Verified: hero 3 headless
reads `hitscan x1 r100` with the `0.04` candidate; hero 41 reads `hitscan
x1 r100 interval 1.000s` with `0.05` candidates; hero 5 reads `PJ x5 s85
g9.8 0.50s`; the Bevy HUD shows `back 5.4 crouch 3.0 fall max 30` from the
install.

**The visible round, same day:** the first windowed run showed "no
projectiles, no changes" because three things stacked — the windowed loop
never copied the mouse's fire state into the sim (**left click fired
nothing**; the wiring line was simply absent), hero 3 is hitscan (nothing
flies by construction), and nothing in the viewer drew bots or projectiles.
Fixed: the fire wiring; a marker batch in `ow-view --play` (one white box
per bot, parked underground when dead, one velocity-stretched box per
projectile, both indexed at palette slot 0 = the identity); the HUD line
reordered so the weapon text cannot clip past the overlay's 64 columns.
Also fixed in the same pass: the `Anim::Blend` palette omitted the
`[identity, bone 0, …]` leading slot the renderer's convention requires
(the `Bones` path had it), so every skinned vertex under `--anim` read the
*next* bone's matrix — a one-bone shift contributing to the "recognizable
but deformed" reload. And `--anim auto` on a hero whose view model names no
blend set now warns and runs unposed instead of killing the window.

**What the Ghidra round added** (*decompiled* in `module_v2full.bin`, v2b
base `0x7ff7c4d20000`; *capture-data* negatives where stated):

* the graph push `FUN_7ff7c6ec9070` walks the element list and dispatches
  through the trampoline `FUN_7ff7c6ed4dc0` — the executor shape row 5
  already carries; no new timing semantic came out of it;
* the mover advance `FUN_7ff7c59ff690` decompiles to obfuscated control
  flow (junk blocks, encrypted slots) — the tier-3 control finding stands;
* `dmWorld::Create` (v2b `0x7ff7c7a4cd70`) confirms the 0x3fff-entry handle
  table with obfuscated pointers, the `0x4d0` allocation and the
  `m_index` write — the Domino construction row 10 already carries;
* the `m_amount` (`0xAF872E86`) and `numShotsPerSecond` (`0x02F3F5D4`)
  hashes each occur as an immediate exactly once/twice in the image
  (`0x7ff7c8eb60f8`, `0x7ff7c8f48fd8`) with **no code xrefs** — the
  class-hash-dispatch-absent finding stands, so the volley fire timing and
  the damage application have no file-reachable consumer in these captures
  and remain the declared capture requirement.
