# The systems-extraction plan

A sweep to map as many of the client's subsystems as possible from the capture,
using the patterns this project has already validated. The goal is an *index of
the engine*: every runtime class, its fields, its consumers — so later work
starts from a map instead of a search.

Everything below is anchored in the standing capture (`dev/dumps_range`,
gitignored) at build `2.25.0.0.154088`, already imported into the Ghidra
project `dev/ghidra_run` as `module.bin` (flat dump), `module_mapped.bin`
(base `0x7ff649000000`) and `module_heap.bin` (base `0x161b0000000`).

## The four patterns that work (the sweep repeats them)

1. **The class-registration thunk.** Every runtime class is registered by a
   small block: `sub rsp,0x98; …decode loop…; mov r8d,<CLASS_HASH>; …
   [rsp+0x20]=<SIZE>; call 0x7ff64c094b90`. Seen at `0x7ff64a2f2c40`+
   (one thunk registers `0xEED6D952`, the volley class, size 0x2c0).
   **Sweeping every caller of `0x7ff64c094b90` yields the complete runtime
   class registry: hash → size → registration neighbourhood.**
2. **The field registrar.** Within a class's registration region, each field is
   registered by `lea rcx/rdx,[rip+…]; mov r8d,<FIELD_HASH>; call 0x161b4b9c7a0
   (mapped twin region); mov ecx,<SLOT>; call 0x161b3928140`. The slot pairs
   with the hash (`volley_slots.txt`, `character-controller.md`).
3. **The reflective schema tables.** Data regions hold per-class runs of
   `{u64 hash; ptr type; u64 offset; ptr aux}` records (stride 0x38), e.g. the
   mover at `0x7ff64dc80fb0`, the volley at `0x7ff64e088948`, ModifyHealth at
   `0x7ff64dc7f280`. The float type-object is `0x7ff64ce43a00`; enumerating its
   sibling type-objects (int/bool/vec3/ref readers live near `0x7ff64dbef4c0`)
   types every field without reading a function.
4. **Value-shape identification.** Dump a field's values across all heroes
   (the `mover` / `weapon-raw` dumps), join by hash, and check shapes
   against published figures (check-only, per `docs/provenance.md`). This named
   the mover's speed family and killed the `0xFEC435C6`-is-damage hypothesis.

Plus two lookup structures already decoded: the **component router**
(`FUN_7ff64b7ea450`, 119 component ids mapped to type hashes) and the
**comparator chains** (`0x7ff64b7542d0/0x7ff64b755a20/0x7ff64b7590d0`,
field-hash → default comparisons).

**The router is now fully paired** (this round). The registry sweep had left
40 of the 119 router hashes without a row; every one of them has a class
descriptor in the image after all — found by scanning for the descriptor's own
`+0xb8` class-hash field instead of the registry walk. The missing 40 are the
lazily registered classes (the TLS-guarded path, the same reason Phase 4e
missed the mover and health), and the pairing returns size, field count and
parent for each. Parent chains close on two roots: `STUEntityComponent`
(`0x7395D144`) and `STUMirroredEntityComponent` (`0x773DC32C`). Names from the
TankLib checkout resolve 18 of the ids, including the ones the last rounds
worked on: **`STUCharacterMoverComponent`** (id `0x26`, size `0x130`, 52
fields — the live mover object of character-controller.md), **`STUStatescriptComponent`**
(`0x36`, `0x28`, 3 fields), `STUHealthComponent` (`0x3a`, `0x68`, 12),
`STUFirstPersonComponent` (`0x35`, `0x70`, 7), `STUAIBehaviorComponent`
(`0x5e`, `0xe0`, 11), `STUFallingDamageComponent` (`0x57`), `STUEscortComponent`
(`0x71`), `STUImpactEffectOverrideComponent` (`0x59`). Artifact:
`dev/component_router_paired.txt` (id, hash, name, size/fields/parent); the
remaining ids are paired but have no TankLib name.

## Phases

### Phase 0 — make the Ghidra project the accumulating map (half a day)

* Name what is known, via the bridge, so the project carries the knowledge:
  `FUN_7ff64b7ea450` → `ComponentRouter`, the registrar regions, the schema
  tables, the Domino pool, the class-thunk cluster. Plate comments with the
  doc pointers (`docs/formats/*.md`).
* Run analysis on `module_mapped.bin` (targeted, region by region if the full
  pass is too slow) so `get callers` works; today xrefs are empty because the
  import skipped auto-analysis.
* Keep the Capstone pipeline (`dev/disasm_field_sites.py`, `find_refs.py`) as
  the bulk scanner; Ghidra for decompiles once a target is located.

### Phase 1 — the runtime class registry (the big index)

* `find_refs(0x7ff64c094b90)` over all capture regions → every call site.
* For each site, back-slice the thunk for `r8d` (class hash) and
  `[rsp+0x20]` (size) → `dev/class_registry.txt`.
* Join against the STU class files (OWLib checkout) by hash: sizes cross-check
  the generated `[STU(hash, size)]` declarations and flag drift.

### Phase 2 — field layouts for every interesting class

* Sweep the `mov r8d,<FIELD_HASH>; …; mov ecx,<SLOT>` pattern inside each
  class's registration region (extend `extract_component_table.py` into a
  general `sweep_registrars.py`) → `dev/class_fields/<class-hash>.txt`.
* Cross-join with the STU class files: the *runtime slot = STU offset + 4*
  rule (validated on the mover and the volley) turns every generated `.cs`
  into a naming source; a coverage report says which fields stay hash-only.
* Enumerate the type-object family (siblings of `0x7ff64ce43a00`) so the
  schema tables can be typed mechanically.

### Phase 3 — name pools sweep (the cheapest big win)

The Domino physics pool (`0x7ff64d56a4d8`, plaintext `dmBody`/`dmWorld`/…
near a `D:\Madness\production\...` path) proves plaintext identifier pools
exist in the capture. Scan every region for the pool signature: runs of
camelCase identifiers, 4+ chars, high letter ratio, near a source-path string.
Each pool found names a whole subsystem (the prize would be a statescript node
pool — it would name every graph node class the weapon graphs use).

### Phase 4 — per-system consumer reads (behaviour)

For each system below: locate via its registrar/known-hash sites, decompile
the update/consumer in CodeBrowser, record findings in
`docs/formats/<system>.md` with addresses. Ordered by value for the rewrite:

| system | entry anchors | what we want |
|---|---|---|
| mover update loop | mover registrar (Phase 1); Domino pool names (`linearDamping`, `gravityScale`) | accel/friction ramps, air control, jump-impulse source |
| damage application | 83 `ModifyHealth.m_amount` sites; health component id 0x3a | the pool-order rule (replaces the assumed `rules.rs`), armour mitigation, headshot multiplier |
| statescript VM | `statescriptEntry` 0x5866C45E sites; graph assets type 0x01B | the interpreter loop: how entries fire, sync vars, waits |
| projectile integration | `projectileMotions` 0x351BD93F; volley slot 0xd0 | motion types, gravity on projectiles, collision handoff |
| buff/status stacking | mover consumers; the published "+75% cap" shape | the modifier stack semantics |
| ultimate charge | ModifyHealth consumers (charge-on-damage) | charge per damage point |
| tick rate | `dmWorld.m_inv_dt0` live instance; scheduler | the real fixed tick (open question since the GDC talk note) |
| material table builder | count-writer cluster `0x7ff64c686447`, `0x7ff64c6a1879`, `0x7ff64c6b445b`, `0x7ff64c6c88af` | the blocked materials link (`docs/formats/materials.md`) |
| input/aim | `aimID` 0xF9808409 sites; spread fields | spread pattern, aim pipeline |

### Phase 5 — validation by value shape

Each system's extracted constants get a per-hero dump and a shape check
against published figures where they exist.
The health-pool and mover-speed identifications are the templates: name only
what the value shape pins, keep the rest hash-labelled.

## Housekeeping

* New dev tooling: `sweep_registrars.py`, `scan_name_pools.py`,
  `dump_class_registry.py` — all following the existing regions.py/capstone
  pattern, output under `dev/` (gitignored), conclusions promoted into
  `docs/formats/` only when a check would fail if they were wrong.
* Ghidra naming discipline: every named function/doc pointer lands as a bridge
  `rename_function`/plate comment the same session it is learned.
* The plan's phases are independent; Phase 1+3 are the highest
  sense-making-per-hour and unblock everything else.

## Phase results

### Phase 0 — done

`module.bin` (flat OS dump), `module_mapped.bin` (mapped-code regions, base
`0x7ff649000000`), `module_heap.bin` (heap code regions, base `0x161b0000000`)
are in the `owmod` project. Named so far: `ComponentRouter_ByTypeHash`
(`0x7ff64b7ea450`), `FieldDescriptorDispatch_Volley` (`0x7ff64b7542d0`),
`FieldDefaultCompareChain` (`0x7ff64b755a20`),
`VolleyFieldRegistrarFragment` (`0x7ff64b73e000`); bookmarks on the mover /
volley / ModifyHealth schema tables, the Domino pool, and the class-thunk
cluster. Ghidra's relocation-fixture dialogs should be **cancelled** for these
programs: the captures hold loader-final addresses.

### Phase 1 — the runtime class registry (swept)

`dev/sweep_class_registry.py` (vectorised) found **4,801 call sites** of the
registrar (`0x7ff64c094b90` mapped / `0x161b4b9c7a0` heap twin) and recovered
**4,263 unique classes** into `dev/class_registry.txt`: `{hash, size,
literal_size, name, thunk, call sites, static descriptor pointers}`.

* The name column is a **stub**: every recovered class decodes the plaintext
  `Unknown` (or fails, in 239 of them). The thunk pattern *does* decrypt a
  name (8-byte xor key at `lea r8,[rip+]`, length at key+8, plaintext at
  key+0xC once the decode-once guard has run), but what it decrypts for these
  classes is the client's own fallback label — the real names are not here.
  The naming prize stays with the Phase-3 pool sweep.
* Every row carries its static pointers, and they are meaningful: the volley
  class's statics include the comparator chain (`0x7ff64b7542d0`) and what are
  evidently schema-table pointers — so registry rows are a direct route to
  each class's layouts and comparators.
* Cross-join with the OWLib generated types: **242 of 4,263** classes are
  known STU declarations, and every checkable one shows runtime size ≥
  declared size by 8–16 bytes (e.g. `STUStatescriptGraph` 472 vs 456) —
  consistent with the runtime-slot = STU offset + 4 rule plus a small runtime
  header.
* Sizes are recovered only where the thunk stores it as an immediate in
  `[rsp+0x20]`; several hundred rows read `0x0` from that parse gap and need a
  wider slot walk later.

### Phase 3 — name pools (swept)

`dev/scan_name_pools.py` over every captured region, 1,397 candidate
clusters. The payload pools are OS/WinAPI shadows and the *registry name
column itself* (thousands of literal `Unknown` fallbacks at
`0x7ff64d81f3d4..0x7ff64d93xxxx` — the mapped image's class registry rows).
The engine's own type names (`WeaponVolley`, `CharacterMover`, `Volley`,
`STU…`) are **not present as plaintext anywhere in the capture** — the only
statescript hits are prose strings (`Unknown Statescript Event`,
`FTUE Statescript`, etc.). Conclusion: client-visible naming for engine
classes is genuinely not resident here; the reference library stays the name
source and the registry supplies the *layout* side. One real pool did surface
with names + descriptions: the config-var register
(`0x7ff64d7ecc74..0x7ff64d93xxxx`: `HUD.3PAnims.enabled`,
`anim.debugDrawIK`, `anim.enableLogStatescriptAnim`…), useful for engine
toggles, indexed and grep-able from `dev/name_pools.txt`.

### Phase 4a — volley class methods (first consumer read)

The registry entry of the volley class carries its own hooks, and they lead to
the class's C++ object: the constructor at `0x7ff64b757750` sits on vtable
`0x7ff64d1cac30`. Read out (mapped capture):

| vtable | addr | reading |
|---|---|---|
| [0] | `0x7ff64a39a6c0` | `ret` stub |
| [1]/[5] | `0x7ff64b754980` / `0x7ff64b754d90` | return a static pointer (`lea rax,[rip+…]`, `ret`) |
| [2] | `0x7ff64b756d30` | walks the registration-instance table (the table at `0x7ff64e081900`, 6 entries), true when a pointer is in it |
| [3] | `0x7ff64b7553b0` | compares `[this+0x88]` against the table head — an "am I registered / is active" check |
| [4] | `0x7ff64b754030` | destructor: frees three arrays at `+0xc0/+0xd0/+0xe0`, then the object (0x2c0) when told to |

So the class object is infrastructure (registration + lifetime) — the
**per-volley runtime behaviour is not in these methods** and one of the class's
statics (`0x7ff64b75d0a0`) fell outside the mapped capture entirely. The next
read for the volley's actual fire logic needs either a fresh capture targeting
that address range, or working from the graph-execution side (statescript VM)
later in Phase 4. Registry row for the volley is
`class_registry.txt` line `eed6d952` (size `0x2c0`).

### Phase 4b — statescript graph runner class (0x7236F6E3, size 0x1d8) and volley config instances

The graph class vtable (`0x7ff64d182288` anchor) is *the same infrastructure
shape* as the volley class: registration-instance checks, static-pointer
returns, destructor with three array frees, no behaviour on the class itself —
so the actual graph VM is dispatched elsewhere (likely inside
`FUN_7ff64c09xxxx`-region engine code, where the shared infrastructure
implementations live).

The volley config's five registered instances
(`0x7ff64e081900` table: `0x7ff64df54af0`, `0x7ff64df54400`,
`0x7ff64de40640`, `0x7ff64de404e0`, `0x7ff64de40380`) keep
their config *slots as ConfigVar pointer tables* — {pointer-table fields} with
things like `aabb count at slot 0x138 = 10`, and one entry (slot 0x140) holding
the object size 0x2c0 bytes exactly — not a per-hero fixture, more of the
scratch template the volley binds against at runtime. This is consistent with
the sim design: volley *values* live in the STU ConfigVar pool read from
files, so our sim data pipeline remains source of truth.


### Phase 4c — the introspection core (decompiled)

Four functions decompile the reflection system itself:

* `InitializeIntrospectionDescriptor` (`0x7ff64c094b90`, renamed in-project):
  fills one node from the thunk's 18 arguments. Layout confirmed:

  ```
  +0x00 vtable = 0x7ff64ce43a00      +0x58..+0x90  nine static pointers
  +0x08 instances ptr (arg18)        +0xa0  u32 size      (arg5)
  +0x50 u64 token (arg9)             +0xa4  u32 (arg6, 8 in practice)
  +0xb0 u32 (arg7, 4 / 0)            +0xa8  u32 (arg8, 0)
  +0xb8 u32 class hash (arg3)        +0xbc  u32 flags (arg4, 0)
  ```

  The router's `dword[+0xb8]` compare is now fully explained: it compared this
  hash slot. Every registry row's argument block can be labelled from this.
* Field-table walkers (`0x7ff64c095300` slot `+0x60`; `0x7ff64c095800` slot
  `+0x48`) walk a descriptor's field array — **stride 0x38, count at +0x98,
  array at +0x28, parent chain at +0x20** — calling each field's virtual with
  `(field, offset + delta)`. That is the inheritance-flattening/rebase pass, and
  it is the code-side confirmation of the 0x38 record shape our data scans found.
* `InitializeFieldDescriptor` (heap twin `0x161b4b9c7a0`; the mapped twin is
  outside every current capture) builds one field object:

  ```
  +0x00 vtable (field-node class)   +0x18 count/aux      +0x30 u32 field hash
  +0x08 u32 slot offset             +0x20 parent object  +0x34 u32 flags
  +0x10 previous-alloc ptr
  ```

  The `lea rcx,[rip+…]; mov r8d,<HASH>; …; mov [rsp+0x20],<SLOT>; call` pattern
  in every field registrar is now fully decoded: hash→+0x30, slot→+0x08,
  parent→+0x20, prev→+0x10, and the two constant args (8, 2/4) land in +0x18/
  +0x34.
* The node vtable at `0x7ff64ce43a00` has **non-pointer bytes in its low slots**
  and stubs/traps at +0x48 (aborts with 0xf8) with an obfuscated initializer at
  +0x60 — base-class slots; the real per-kind behaviour lives in subclass
  overrides (`docs/systems-plan.md` note: read vtable entries defensively).

All four are renamed/bookmarked in the `owmod` project.

### Phase 4d — the descriptor map and inheritance trees (swept)

With the node layout known, `dev/dump_class_descriptors.py` walked **all 4,263
registry classes** and dumped their static descriptor objects into
`dev/class_descriptors.txt` (vtable, instances, parent, field array, field
count, token, nine static method/table pointers, size, hash) and
`dev/class_inheritance.txt` (hash → parent hash → field count).

Findings from the map:

* **Most classes are leaves.** Of the static method-pointer population, a
  handful of pointers dominate: `0x7ff64a3a8a60` is shared by 2,215 classes,
  `0x7ff64a39a030` by 1,990, and the pairs `0x7ff64ae223d0`/`0x7ff64a3a8ca0`
  (1,580) and `0x7ff64a932480`/`0x7ff64a4c1a60` (1,569). Only a few hundred
  classes carry distinct behaviour — consistent with a schema universe where
  most entries describe fields rather than act.
* **The inheritance trees are readable and nameable.** Parents resolve by
  chasing each descriptor's `+0x20` in the capture; the reference library then
  names the chain. Worked examples:
  * `STUWeaponComponent` (`0x15CB6532`) → `STUMirroredEntityComponent`
    (`0x773DC32C`) → `STUEntityComponent` (`0x7395D144`).
  * `STUStatescriptStateWeaponVolley` (`0xEED6D952`, runtime 0x2c0, 64 fields)
    → `STUStatescriptState` (`0x7C37840C`, 0xc0) → `STUStatescriptBase`
    (`0xC2DC60F6`, 0x58) → `STUGraphNode` (`0x25C4F930`, 0x10) → `STUGraphItem`
    (`0x8FAF5745`, 0x10) → `0x95E4A53A` (size 8; unnamed in the reference).
  * Parent-class runtime sizes **equal** their declared STU sizes exactly
    (0xc0 = 192 etc.); only concrete leaf classes carry the +8..+16 runtime
    delta — so `runtime slot = STU offset + 4` composes cleanly up the chain.
* The volley's descriptor (`0x7ff64e0819a0`) also demonstrates the two
  protected slots at `+0x38`/`+0x40` holding obfuscated (non-pointer-looking)
  values — the same protection idiom catalogued in
  `docs/formats/materials.md`; they are recorded raw, never decoded silently.

Artifacts (gitignored `dev/`): `class_registry.txt`, `class_descriptors.txt`,
`class_inheritance.txt`, plus `sweep_class_registry.py`,
`dump_class_descriptors.py`, `join_registry.py`, `scan_name_pools.py`.

### Phase 4e — the mover and health component descriptors (capture-found)

The registry sweep missed these two classes (they register through the lazy
TLS-guarded thunks, not the eager cluster), but scanning the capture for any
object whose `+0xb8` equals the target hash finds them exactly once each:

| class | descriptor | size | fields | methods at +0x58/+0x60/+0x68/+0x70 |
|---|---|---|---|---|
| `STUCharacterMoverComponent` `0x6EFDBD73` | `0x7ff64dc80980` | 0x130 (= STU 304) | 52 | `0x7ff64afc2370`, `0x7ff64afc22e0`, `0x7ff64afc2160`, `0x7ff64afc24b0` |
| `STUHealthComponent` `0xA658CA1C` | `0x7ff64dddb2d0` | 0x68 (= STU 104) | 12 | `0x7ff64b1b6c80`, `0x7ff64b1b4450`, `0x7ff64b1b04c0`, `0x7ff64b1ba3e0` |

Both share the component base descriptor at `0x7ff64ddcffd0`.

**Field-entry layout, corrected and triple-verified** (the movers' full 52-field
table is in `docs/formats/character-controller.md`):

```
+0x00 u64 kind (node vtable 0x7ff64ce43a00)   +0x10 u64 field descriptor
+0x08 u64 offset (STU offset)                 +0x30 u64 field hash
```

Verified by (a) the decompiled walker, (b) brute-forcing the health array
against the class file's twelve offsets — exactly one alignment fits, and
(c) the mover hash column reproducing the class-file hashes in order.

**The health component's compare layer confirms the defaults.** Decompiling
`0x7ff64b1b04c0` yields a field-hash → default comparison: `0x5AF3C1F6` against
`DAT_7ff64cf42318` (= **7.5f**), `0xE0EE1282` against 0.0, `0xCDB100D1` and
`0xA80BAB3F` against byte 1, `0xE64867CE` against the `0xFFFFFFFF` bit pattern
(int `-1`). Every constant equals the declared default in
`STUHealthComponent.cs` — the introspection compare-layer is faithful, and the
five fields are pinned as data.

**Capture-gap correction.** The mover component's four method addresses
(`0x7ff64afc2xxx`) fall in a region **no capture covers** — the flatten
zero-fills, and decompiling zeros reads as `add [rax], al` loops, which is why
the first whole-function attempt exploded into a repeating byte-add loop and
was misread as protection. The lesson: run `space.read` before decompiling a
pointer; a zero-decompilation means a capture gap, not a cipher. These four
methods need a fresh capture covering `0x7ff64afc2000`+, exactly like the
volley's `0x7ff64b75d0a0` gap.

### Phase 4f — the descriptor static-slot roles are now all known

Decompiling the shared and per-class method pointers completed the picture of
what a class descriptor's static slots mean. The base-class implementations:

| shared address | used by | behaviour |
|---|---|---|
| `0x7ff64a3a8ca0` | 1,580 classes | default provider: `return 0` — "unknown field has no default" |
| `0x7ff64a39a030` | 1,990 classes | compare-side base: `return 1` |
| `0x7ff64a39a6c0` | many | `ret` no-op stub |

And the per-class overrides, verified on two classes:

* **`STUHealthComponent`**: allocator (`0x7ff64b1b6c80`, size 0x68 — in a
  capture gap), constructor (`0x7ff64b1b4450`, sets vtable `0x7ff64d12c598`
  and four `-1` refs), **default-comparator** (`0x7ff64b1b04c0`), and
  **default-provider** (`0x7ff64b1ba3e0`) — the provider writes the defaults as
  **bit-exact constants**: `0x40f00000` (= 7.5f) for `0x5AF3C1F6`,
  `0xffffffff` for `0xE64867CE`, `1` for both byte fields, `0` for
  `0xE0EE1282`. Comparator, provider and generated class file agree field for
  field.
* **`STUStatescriptStateWeaponVolley`**: allocator (`0x7ff64b758260` —
  `Alloc(0x2c0, 0)`, its object size), constructor (`0x7ff64b757750`),
  default-side comparator pair (`0x7ff64b755a20`, `0x7ff64b7590d0`), fire
  method (`0x7ff64b75d0a0`, capture gap).

So a registry row now reads as: **size → allocator → constructor + vtable →
per-field default comparator + provider → dispatch chains**, with every
unshared pointer addressable and either decompiled or pinned to a named
capture gap.

Capture gaps are now precisely enumerated for a fresh dump: the mover's four
methods (`0x7ff64afc2000..`), the volley fire method (`0x7ff64b75d000..`), the
health allocator (`0x7ff64b1b6000..`), and the parent-descriptor area where
inheritance trees terminate at `0x95E4A53A`.

### Phase 4g — family method-sets, and the statescript base's enum default

Reading the shared family method-sets confirms the pattern one level deeper:

* The **408-class statescript family** (`0x7ff64b55d2c0` / `0x7ff64b55f680`)
  is a default **comparator/provider pair** for exactly one field hash:
  `0xA2287776`, compared to 0 and provided as 0. The reference class file names
  it: `STUStatescriptBase.m_A2287776 = Enum_46B5ADCA.x1F0B0C4D` — so the
  client's "default 0" pins that symbolic enumerator as **case zero** of the
  enum. The same class file documents `m_clientOnly` / `m_serverOnly` bytes and
  the remote sync-var arrays on the state base — the client/server execution
  split of the statescript runtime, visible in the type.
* **Thunks are not distinct behaviour.** `0x7ff64aeb52e0` (the "209-family"
  member) decompiles to `jmp 0x7ff64a39a030` — a per-class thunk into the
  shared stub. Any future family count must chase `jmp` thunks before calling
  a method set "distinct".

The descriptor slot vocabulary is now complete and verified across three
classes (mover, health, volley) and the two shared fallbacks — a registry row
reads: **size, allocator, constructor+vtable, default comparator+provider,
dispatch chains**, with unread pointers being capture gaps rather than
unknowns.

### Phase 4h — two hypotheses tested and closed

* **The ComponentRouter has three callers, all enumeration paths.** Ref-scanning
  its call sites (`0x7ff64a494306`, `0x7ff64a5dfe90`, `0x7ff64ad03eb9`) shows
  loops over descriptor lists where the router's returned id is compared and
  filtered (`cmp eax, 0x64 / je skip`, `cmp eax, 0x41 / je skip`) — component
  lookup for listing/serialization, not per-component tick. The mover's update
  is therefore *not* reached through this router; the earlier conclusion stands
  that the update consumer lies behind the enumerated capture gaps.
* **The descriptor "instances" tables are shared registration metadata, not
  live configs.** Both the health and mover descriptor tables list the same
  three pointers after their self-reference
  (`0x7ff64ddcff30`, `0x7ff64ddcfdd0`, `0x7ff64e16b7b0`); reading them as
  component instances yields no values. The volley's "five template instances"
  (Phase 4b) are the same artefact. The per-hero configuration genuinely lives
  in the STU assets — where the data layer already reads it — and the runtime
  objects are rebuilt from those at load.

## The second capture: the gaps are filled, and the map translates

The operator took two fresh dumps (`dumps_v2` mapped, `dumps_v2b` all) of the
same build (`2.25.0.0.154088`) from a **different session** — and they exposed
the addressing rule this project needs from now on:

### The ASLR rule (verified)

The game's module was mapped at allocation base `0x7ff7c4d20000` this session
versus `0x7ff649e60000` in the standing capture. **Everything translates by the
same delta (`0x17AEC0000`**: verified on five anchors' bytes *and* on the
embedded pointers inside the mover's field table, e.g. `0x7ff64ce43a00 →
0x7ff7c7d03a00`). Consequences:

* Addresses in these docs are **capture-relative**; a new session's delta
  translates them wholesale.
* Dumper output manifests differ per run and later runs leave unlisted files
  (558 in `dumps_range`); `regions.py` now prefers a merged
  `manifest_full.txt` so nothing on disk is invisible to analysis.

### Descriptor-slot semantics, corrected

The four per-class statics on a component descriptor are **allocator,
constructor, default-comparator, default-provider** — introspection slots, not
gameplay methods (the earlier "methods" naming was optimistic). Verified on the
mover: `Alloc(0x130,0)`, ctor, `default comparator` (`5.60519e-45` = the
bit-pattern denormal of int 4 — a float-shaped read of an int default), and
default provider (`mov dword [rcx], 0x41700000` = 15f). The gameplay update is
*not* among them.

### The mover's default table, and its jump struct

`dev/extract_defaults.py` turns a comparator/provider pair into a field-default
table (`dev/mover_defaults.txt`: 19 entries covering the component, its **two
inline structs** and the component base). The star: inline struct
`STU_97288E54` (the mover's `m_9947871A`, STU offset 72) holds six floats, and
the client's own defaults match the class file **six-for-six, bit-exact**:

| hash | offset | class file | client default bits |
|---|---|---|---|
| `16882D22` | 8 | 20f | `0x41a00000` = 20.0 |
| `746B3248` | 12 | 20f | `0x41a00000` = 20.0 |
| `F6B890EC` | 16 | **0.98f** | `0x3f7ae148` = 0.98 |
| `D68B7D62` | 20 | 0.7f | `0x3f333333` = 0.7 |
| `9FCDBE78` | 24 | 0.7f | `0x3f333333` = 0.7 |
| `766AFA5A` | 28 | 0.2f | `0x3e4ccccd` = 0.2 |

`0.98` is the published jump apex height ("heroes reach 0.98 m before moving
downwards") — the mover's vertical struct carries it as a default, and the
comparator shows the embedding: the mover's compare chain descends into these
six fields, so the runtime struct composes them at STU offset 72 + 8..28.

### The volley's provider (decompiled whole)

The volley's default provider (~30 fields, including the inherited
`0xA2287776` base enum → 0) distinguishes two mechanisms:

* **value fields** — literal writes: `0x41700000` (15f), `0x41f00000` (30f),
  `0x43480000` (200f), `0x40400000` (3f), `0x41a00000` (20f), byte/dword 0/1,
  and one 64-bit GUID default `0x06900000000002C`;
* **reference fields** — `projectileSpeed`, `projectileLifetime`,
  `projectileEntity`, `castLength`, `aimID` and others build a *reference
  default* via a three-call group (`create + wrap + store`), i.e. these slots
  are ConfigVar-backed at runtime, resolved from the graph — exactly what the
  sim assumes when it reads ballistics from the statescript graphs.

New artifacts: `dumps_v2`/`dumps_v2b`, `manifest_full.txt`,
`dev/module_v2.bin` (Ghidra base `0x7ff7c3ec0000`), `extract_defaults.py`,
`mover_defaults.txt`.

## The third capture (`dumps_exec`), and the class surfaces read out

The operator's `dumps_exec` is **executable regions only** (protect `0x20`) at
module base `0x7ff753500000`, and it covers the gaps the earlier sessions
listed. The uniform delta from `dumps_range` is **`0x1096A0000`**, verified
byte-for-byte on three independent anchors, so the translation is
`exec = range + 0x1096A0000` (a new `OW_DELTA` value for the dev tools).
`dev/module.bin` is `flatten.py`'s image of it (Ghidra base
`0x7ff753000000`, analysed, 70,834 functions). **Data regions are not in this
capture** — descriptors, the Domino pool and every vtable body live in
`dumps_range`/`dumps_v2b`, so the two sides pair up.

Read out (exec addresses):

| function | exec | reading |
|---|---|---|
| `AllocateMoverComponent` | `0x7ff754662370` | `Alloc(0x130, 0)` — object size = the STU size exactly |
| `InitializeMoverComponent` | `0x7ff7546622e0` | installs the object's three vtables (`0x7ff7566e31f8` at `+0`, `0x7ff7566e30c8` at `+8`, `0x7ff7566e3160` at `+0x48`), then `-1` refs |
| `CompareMoverFieldDefault` | `0x7ff754662160` | per-field-hash compare against the class defaults; `[r9]` = found |
| `ProvideMoverFieldDefault` | `0x7ff7546624b0` | per-field-hash default writes (`4`, `0x41700000`, `0x3f000000`, …) |
| `CompareVolleyFieldDefault` | `0x7ff754dfd0a0` | **correction**: the volley's default comparator, *not* a fire method (the Phase 4f label was optimistic) |

The mover object's three vtables are three interfaces; slot 8 of each is a
per-class introspection attach that calls the field-table walker
(`0x7ff755735800`, the Phase 4c `+0x48` walker) with the class descriptor, and
the rest of the distinct slots are shared or `ret` stubs. The class's own
translation unit (exec `0x7ff754662xxx`) contains **no gameplay code**.

### The identity census, and why offset hunts cannot finish this

`dev/find_refs.py` over `dumps_exec` (2 s) answers "what code names this class"
exactly, and the answer is the movement work's key negative result:

* mover descriptor `0x7ff757320980`: **5 refs** — two lazy registration stubs,
  the introspection attaches, one `lea`-getter;
* mover descriptor #2 `0x7ff757320820`: 6 refs, same shape;
* health descriptor `0x7ff75747b2d0`: **1 ref**, and it is
  `lea rax,[rip+…]; ret` — an entry in a *table of descriptor getters*.

**No gameplay code references a component class, its descriptor, or its
vtables.** `dev/sweep_consumers.py` (new: executable regions only, per-class
slots, reads from object bases, reflection-walk runs excluded — it documents
the three filters it took to get there) surfaced only schema machinery:
typed field walkers, bit-packed deserializers, copiers. The reason is
structural: the engine's own reflection passes enumerate every serialized
offset *by construction*, and the mover's small displacements are reused by
unrelated structs, so no displacement signature identifies the movement
consumer. **The update loop must be reached through the physics body the
component configures, not through the component.** Next read below.

### The Domino route (entry point verified)

The Domino reflection pool is present and readable in `dumps_v2b` at
`0x7ff7c842a4d8` (= range `0x7ff64d56a4d8` + the v2 delta `0x17AEC0000`), and it
is far more than a pool: the region around it is the **Domino schema in
plaintext** — every physics class with its field names and its typed defaults.
The cross-delta the dev tools did not yet carry: **v2b → exec = `0x71820000`**
(verified against the shared `ret` stub: `0x7ff7c525a6c0` ↔ exec
`0x7ff753a3a6c0`; a wrong value here silently yields coincidental hits).

Classes whose fields are named in place: `dmBody` (`linearVelocity`,
`angularVelocity`, `gravityScale`, `linearDamping`, `angularDamping`,
`sleepTime`, `mass`, `invMass`, `inertiaScale`, `characterMover`, …),
`dmWorld` (`m_index`, **`m_inv_dt0`**, `m_gravity`, `m_broadPhase`, `m_bodies`,
`m_fixtures`, `m_spheres`, `m_capsules`, `m_polytopes`, `m_contacts`,
`m_convexContacts`, `m_awakeBodies`, `m_awakeContacts`, `m_useSleep`,
`m_useWarmStarting`, `m_useContinuous`), `dmFixture`, `dmDynamicTree`,
`dmManifoldPoint`, `dmManifold`, `dmContact::Edge`, `dmContact`, `dmSphere`,
`dmCapsule`, `dmSubEdge`, `dmPolytope`, `dmBroadPhase`, `dmGJKCache`,
`dmSATCache`, `dmConvexContact`, plus `bcPQTransform` / `bcMtx44` and the
`Domino Warning:` / GJK messages.

The schema is **offset-encoded**: no qword anywhere in the module's 42 MB of
captured data points at those name strings (scanned), while u32 values do
resolve as blob-relative name offsets (e.g. `0x70` → `dmSweep`). The defaults
are inline typed runs (a field's default repeated per component: 1.0f,
0.5f×4, 2.0f×4, `FLT_EPSILON`×4, `FLT_MAX`×4, 0.005f×4, 1/50×4, …). Two of
them matter for byte-exactness and are **candidates until the consumer is
read**:

* **1/60** as a 4-wide run at v2b `0x7ff7c842b180` (`0x3C888889`) — the only
  familiar `1/N` in the physics defaults, i.e. a 60 Hz inverse-dt candidate;
  1/50 (`0x7ff7c842a4c0`) and 1/100 (`0x7ff7c842aee0`, `0x7ff7c842b490`) also
  appear.

Beside the `dmWorld` name sits a table whose first entries are code pointers, a
deleting destructor among them (exec `0x7ff7555875c0`: sets vptr
`0x7ff756972710`, asserts, frees **0x88**). That table is **not** dmWorld's: the
class's name getter decodes to **`teSceneDeferredCmd_ExitScene`** (below), and
exactly two live instances of it exist in the v2b heap (a full 10.6 GB scan:
`0x219a7f4f170`, `0x219a811b1a0`, both region `0x219a7ea0000`) — so the table
is a scene-command class's, and nothing in that neighbourhood names dmWorld.
No module-data qword references those instances; the owner holds them
heap-side.

### The name oracle, and what the capture does carry

The engine's class names **are** resident — XOR-encoded, not absent. The idiom
is a decode-once getter: a record of `{u8 key[8]; u32 count | flag<<24; char
enc[count]}` in the module's data, `enc[i] ^= key[i % 8]` once, the flag
cleared, the plaintext returned. An empty-structure scan for `{key, count|flag,
enc}` over the module's 39 MB of captured data decoded real content, among it:

* whole class families: `teSceneDeferredCmd_*` (Enter/ExitScene, HideEntity,
  HideEntityByHandle, UnhideEntity, Destroy, DestroyByHandle, ChangeFlags,
  SetTarget, AttachToParent, CustomCallback, PlaceOnExitList, UseParentTarget,
  SetEffectTargetHardpoint, RestoreBreakableByHandle, DestroyEjectedParts,
  UpdateEjectedParts), `teSceneDeferredEvent_*`, `teEffectDeferredEvent_*`
  (Ragdoll*, ShatterBreakable, ApplyImpulse, CameraShake*, Cloth*, …),
  `teRenderMgr`, `teRenderList`, `teDrawContext_Prism`, `teDriverCmdList*`,
  `teDescriptorTableCache_Prism`, `teMoviePlayer`, `teLensFlare`,
  `teOcclusionQuery`, `teHardDriveSrc`, and the locale/language table;
* engine messages that name systems: **`Missing movement component`**,
  **`Failed to add movement state.`**, **`Client cannot understand movement
  checksum.`** (`0x7ff7c869ad60`, `0x7ff7c869ad88`, `0x7ff7c869d158`), and
  `Scene Manager has %d scenes still in existence.`

So Phase 3's "engine type names are not plaintext in the capture" needs the
qualifier that they are **present encoded** — the `te*` families came back
whole. What does *not* decode anywhere in the scanned data is a `dm*`/mover/
weapon class name, so the naming oracle covers the scene/render/effect side and
the messages, and the component/weapon classes still need the reference
library for names.

The three movement messages are the movement system's own words; walking them
as addresses closes that lead: over **v2b's own executable regions** (1,354
regions, 22 MB) there are **no RIP-relative references** to any of the three
strings, the record keys, or the count dwords, and **no data qword** in the
module range holds those addresses either. The same block that holds them also
holds the locale list and `Overwatch has encountered a critical error during
startup…`, and it sits *past* the module's schema band (`0x7ff7c869xxxx`), so
these are lookups by id from a mapped string/locale table, not code-anchored
strings. The movement system has to be reached another way.

One more decode attempt, recorded so it is not repeated: the schema region
does contain 0x10-stride records `{u64 ptr; u32 name_offset; u32 zero}` whose
`name_offset`s resolve to `dmBody` fields — a run at `0x7ff7c7d3a930` names
`mass`, `fixtureCount`, `approximateRadius`, `linearDamping`, `maxRadius`,
`minRadius`, `inertiaScale`, `localCenter`, `bcMtx44` and a second at
`0x7ff7c7d201e0` names `linearVelocity`, `localCenter`, `bcMtx44`. It is
**not** the field-descriptor table: the `ptr`s resolve to unrelated strings
(`mass` → `allowed_url_regex`) and the name offsets do not follow field order.
The field offsets therefore remain undecoded — the one thing standing between
this capture and a byte-exact integration order.

Chasing the mover *by its component id* is also a dead end, and worth not
repeating: `mov ecx/edx/eax/r8d, 0x26` appears at **72 sites across 43 pages**
of v2b code (health `0x3a`: 30+ pages, weapon `0x38`: 550 sites on 321 pages) —
dispersed, the signature of per-component schema code (registration,
serialization, editors), not of a system. Every component-facing route tried so
far lands in the same place, so the remaining v2b-side lead is to
reference-scan the **schema tables themselves** (`0x7ff7c7d3a930`,
`0x7ff7c7d201e0`, and the whole blob neighbourhood) over v2b code: whatever
reads them is the reflection consumer, and the field offsets are what it
consumes. That scan is done and it *does* find code: the 0x10-stride table at
`0x7ff7c7d201e0` is read from **`0x7ff7c5365fa5`**, the field type object at
`0x7ff7c7d200b0` (`bcPQTransform`) from **`0x7ff7c536825c`**,
**`0x7ff7c5368690`**, `0x7ff7c53a3e3b`, `0x7ff7c54abbda`, the schema blob start
from `0x7ff7c76a8050`, the `dmBody` name from `0x7ff7c7a38c95`, and the 1/60
default run from `0x7ff7c7a5b1bd`. The `0x7ff7c536xxxx` page holds a tight
cluster of those sites and is the next decompile target, in
`module_v2full.bin` — it was not read this session because the Ghidra server
stopped answering while saving that 70 MB program. Failing that, the pragmatic
route to the goal is the empirical one — sample a live client's mover state
while moving (`live_mapscan.py`-style sampling) and fit/verify `ow-sim` against
it, which also supplies the verification baseline the goal asks for.

## Where this feeds the rewrite

Everything this document established is staged into implementation order in
[`docs/rewrite-plan.md`](rewrite-plan.md): the open items above become
questions with evidence needs (its Q1–Q8), the decompile targets and the
empirical route become two parallel lanes, and the first deliverable is the
verification harness that turns "matching the original" into a measurable
claim.

## The Domino schema, read out in full — and two corrections

The decompile round this document ended on resolved the opposite way and then
much better than expected. Full record in
[`docs/formats/domino.md`](formats/domino.md); the corrections in brief:

* **The `0x7ff7c536xxxx` cluster is not Domino code.** It is narrative-viewer
  UI serialization plus Wwise audio; the "`bcPQTransform` field-type object at
  `0x7ff7c7d200b0`" is the string `"title"`. The two remaining recorded sites
  were the real ones — the `dmBody` descriptor getter and field-array filler —
  and they led to the registry.
* **The whole physics schema is recovered** in one read: 21 classes, 179
  field records, name/offset/type each, read straight from the live capture's
  descriptor registry (`0x7ff7c90eb700`), including `dmBody` (40 fields,
  0x1c0) and `dmWorld` (`m_inv_dt0` @0x198, `m_gravity` @0x1a0).
* **The 1/60 candidate tick is falsified.** Its consumer is a mass-properties
  computation (signed-tetrahedron integration) and 1/60 is a density. The
  tick question is now precisely: who writes `dmWorld.m_inv_dt0`.
