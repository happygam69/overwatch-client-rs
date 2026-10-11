# Weapons and state-script graphs

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

`ow-probe weapons` walks it for all heroes and prints each weapon's graph. On the
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
projectile speed is 250". `ow-probe weapon-params` does that for every hero:

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

* The numbers themselves: damage, fire interval, projectile speed, spread, ammo,
  falloff. None read.
* The 56 empty weapon slots are not explained. Some are genuinely empty slots;
  whether others are weapons stored somewhere else is not established.
* 8 of 132 heroes have entities that are recorded but not downloaded, so they
  contribute nothing here either.
