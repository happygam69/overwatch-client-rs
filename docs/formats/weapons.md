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

1. **Per-hero overrides.** `STUStatescriptGraphWithOverrides` can override graph
   properties through schema entries — the same `(token, value)` pairs that
   worked for the pools. If a weapon's damage is an override rather than a graph
   literal, it is directly readable and identifiable by the same
   group-across-heroes method. This is the cheapest experiment and it has not
   been run.
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
