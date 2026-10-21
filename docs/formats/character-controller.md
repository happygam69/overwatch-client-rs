# The character mover: the entity component behind movement

## Current state

- **Mover data and arithmetic: read/measured.** Slots, jump, and the
  accel/friction/gravity rates, including crouch, air control and reversal,
  are measured and verified against keyed traces (`trace_diff keys
  --by-state`).
- **Mover update path: mapped.** Advance -> invoker -> collision sweep. The
  concrete class is a registered engine class whose own name is "Unknown".
- **Control law: tier 3.** The client's ramp expression is obfuscated (no
  rate literals, no computed velocity store, encrypted vtable slots);
  `ow-sim` uses the measured replacement.
- **Open:** crouch timing on a keyed trace, the steep-slide rule, and the
  operator's feel check.

**Verify:** `trace_diff keys <trace> --by-state` (the Verification
section below); `cargo test --release -p ow-sim`.


This records what a session of decomp + file reading established about
`STUCharacterMoverComponent` — the component that carries a hero's movement
figures — and the client machinery around entity components in general.
Addresses are coordinates in the operator's own capture (`dev/dumps_range`,
gitignored) at build `2.25.0.0.154088`; the *file-side* facts come from
the per-hero mover dump and do not move with the build.

## What was established from the captured client

### The entity-component router

`FUN_7ff64b7ea450` (2892 B; heap twin `0x161b42ea450`) is the component type
router: given a 32-bit component type hash, it binary-searches a compare chain
and writes a small component id as `mov byte ptr [rbx], <id>`. The pairing was
extracted mechanically from the compare leaves
(`dev/extract_component_table.py`, result in `dev/component_table.txt`): **120
component ids**, each bound to one hash. The three that matter here:

| id | type hash | class (from the reference tooling) |
|---|---|---|
| 0x26 | `0x6EFDBD73` | `STUCharacterMoverComponent` |
| 0x38 | `0x15CB6532` | `STUWeaponComponent` |
| 0x3A | `0xA658CA1C` | `STUHealthComponent` |

Fifteen leaves resist the mechanical pairing (still hashed, id unknown); the
table records those as gaps rather than forcing comment lines.

### The mover's runtime slots, two independent ways

The captured client holds a **reflective field table** for the mover runtime
object in the data areas (`0x7ff64dc80fb0` run, 51 scalar records of
`{u64 hash; ptr type; u64 offset; ptr aux; ...}`, stride 0x38). Its anchor
records:

| hash | runtime slot | note |
|---|---|---|
| `1302b05c` (inline instance 1) | 0x48 | STU offset 8 |
| `031FE7EE` | **0xe0** | STU offset 220 = 0xDC |
| `8895A9C8` | 0xe4 | STU offset 224 = 0xe0 |
| `4A5244C1` | 0x118 | STU offset 276 = 0x114 |
| `d9ecb731` | ? | STU offset 284 = 0x11c |

Every scalar's runtime slot is its STU offset **+ 4**, over all overlapping
records. The same rule holds on the volley runtime object, where the registrar
in `FUN_161b423db83`+ (its mapped twin fragment starts at `0x7ff64b73e000`)
pairs each field hash with **the slot offset of the allocation immediately
after** — matching the reflective table exactly on every named anchor.

### The volley runtime slots (for completeness)

Same registrar pattern, same data shape; the volley object's slots are in
`dev/volley_slots.txt` (56 triples hash → slot → encoded-name-blob address).
Named anchors: `projectileMotions` 0xd0, `projectileEntity` 0xf8,
`projectileVisual` 0x100, `aimID` 0x108, `projectileLifetime` 0x110,
`projectileSpeed` 0x128 — confirmed both by the registrar and by the reflexive
schema in the data area.

## What was established from the files

The mover dump reads one `STUCharacterMoverComponent` instance per hero from
the gameplay entities: per hero, every 4-byte field as `hash=value`, typed
through the declared-type parse of the generated class file.
`dev/mover_values.txt` is the full dump (122 heroes, 10 not readable, 3
without a mover).

The class file (`E:\CASCReader\OWLib\TankLib\STU\Types\STUCharacterMoverComponent.cs`
— the operator's own checkout, referenced by path) declares the class as
`[STU(0x6EFDBD73, 304)]` with scalar fields from STU offset 8 (two inline
instances of 64 and 32 bytes), unnamed as `m_<hash>` — which is why a role
*vocabulary* has to be built from values, not from a name table.

Observations from the dump (values, no invented names):

* `031fe7ee` — **5.5 for every readable hero.** The only single-value
  denominator. This is the base speed slot (per `docs/provenance.md` the
  published figure for most heroes is 5.5 on move speed).
* `aebad1b0` — 45 for all. Constant.
* `b74245e8` — 30 for all. Constant.
* `6f25aa23` — 6.2 for all. Constant.
* `6b7fdb90` — 10 for all. Constant.
* `e0a0fe25` — 15 for all. Constant.
* `e54438f7` — 1 for all. Constant.
* `4e03e892` — 2 for all. Constant.
* `e631ba84` — 3.5 for most, 5 for two bot heroes (`900`, `906`), 7 for a third
  (`915`, `916`) — the training-bot outliers.
* `d51c467d`, `5d90dbdb`, `d9ecb731`, `5632f333`, `808374ca`, `f227cbb6`,
  `e91d1664`, `d4a3c694`, `fd92c072`, `1ff06061` — never overridden; equal to
  the class defaults (50, 8, 15, 0.5, 0.5, 1, 4.5, 1, 1, 1).
* The **co-varying family** — `19452f15`, `61d6b4e9`, `cfd3e635` — takes
  values 2.5, 2, 3, 4, 4.25, 4.5, 5.4, 5.5, 6 across heroes; always equal to
  each other within one hero except bots. 19452f15 was `6` for Tracer (3) and
  Genji (41); this is the family the wiki would call the backpedal/strafe
  speeds if the labels were confirmed.
* `1972c687` — varies widely (1.7, 2, 2.1, 2.3, 2.5, 2.825, 2.9, 4); the
  plausible **jump-velocity** slot, since it is the only large-range per-hero
  scalar besides the speed family.
* `08ffaa0a`, `4a576bc2`, `7a70e3e3`, `915ff368` — small fractions (0.3/0.5/0.7/
  1/1.5), co-varying in filters that look like input deadzone/ramp to read out
  per `docs/provenance.md` the same way pools were confirmed: as **groups with
  identical values on identical heroes**, not as names.

Nothing in the dump invents names. The role labels above are in the doc only
where the *value* is a single constant across all heroes (`031fe7ee` = 5.5, the
speed) or clearly a per-hero scalar (`1972c687`).

## What still needs the capture

The values are in. What is not in: **the update loop** that consumes them —
how the game produces velocity from input and the speed slots, what fragment
of Domino (dm*) decides the integration order, the tick count. The Domino
physics reflection pool at `0x7ff64d56a4d8` holds the named fields
(`linearVelocity`, `gravityScale`, `linearDamping`, `softContactFrequencyHz`,
`characterPush`) and a `m_inv_dt0` on `dmWorld` reading a tick value — the
pool is the anchor for that next read.

## The class surface, read out — and why the update is not on it

The third capture (`dumps_exec`, delta `0x1096A0000`, `exec = range + delta`;
ground truth and coverage in `docs/systems-plan.md`) covers the four mover
statics that were gaps, and all four are introspection: allocator
(`Alloc(0x130,0)` — the object is the STU size exactly), constructor (writes
the three vtables below), default comparator and default provider. The volley's
`0x7ff64b75d0a0`, labelled a fire method on the second capture, is its default
comparator.

The runtime object the constructor builds carries **three interface vtables**
(exec `0x7ff7566e31f8` at `+0`, `0x7ff7566e30c8` at `+8`, `0x7ff7566e3160` at
`+0x48`), each with a per-class slot 8 that calls the field-table walker with
the mover descriptor, and shared/`ret` implementations elsewhere. Nothing in
the mover's own translation unit (exec `0x7ff754662xxx`) computes with the
fields.

`dev/find_refs.py` over the exec capture makes that conclusion hard: the mover
descriptor has **5** referencing sites and the health descriptor **1**, every
one of them a lazy registration stub, an introspection attach, or a
`lea rax,[rip+…]; ret` descriptor getter. No gameplay code names the class,
the descriptor or the vtables. Together with `dev/sweep_consumers.py`, whose
filtered rankings still surface only reflection walkers, deserializers and
copiers (the reflection passes enumerate every offset by construction; the
mover's displacements are not distinctive), this says the consumer must be
reached **through the physics body the component configures** — the Domino
route in `docs/systems-plan.md` — not through the component's own surface.

The 15 router leaves that the mechanical pairing missed are recoverable from
the compare tree, and are the same missing piece for combat (health 0x3a is
already anchored).

## Method provenance

* Component router pairing: capstone replay over `FUN_7ff64b7ea450`, extraction
  in `dev/extract_component_table.py` (jet, `je`-leaf forward slicing).
* Registrar slot pairing: capstone walk over `FUN_161b423db83`+' window
  (`dev/volley_slots.txt` for volley; the mover pairing is spot-checked
  inline).
* STU class file: the operator's own OWLib checkout, read as a path reference,
  not vendored.
* The mover data itself: the per-hero dump, verified by the fact that every
  field lands on an expected parametric pattern (constant rows, co-varying
  rows, bot outliers) — that shape cannot come from a wrong offset.

## The mover's per-hero fields, named by value shape

A published gameplay description (`Movement_speed`, Overwatch Wiki, checked
values only per `docs/provenance.md`) states exact figures; the extracted
per-hero dump matches the *shape* of every claim it makes, and the figures
match where the claims cover heroes we extracted:

| field hash | role in the mover | install per-hero value | published figure | verdict |
|---|---|---|---|---|
| `19452F15` | forward base speed | 5.5 everywhere except exactly three heroes at 6.0 | 5.5; 6.0 for Tracer/Genji/Anran | named: shapes match to the hero count |
| `CFD3E635` | sideways base speed | equal to `19452F15` on every hero | published says sideways = forward base | named by construction |
| `61D6B4E9` | backpedal base speed | 4.95 for most, 5.4 for the fast trio | 90% of forward (4.95 / 5.4) | named and verified per pair |
| `0DDD18C3` | crouch speed | 3 for most, 2/2.5 for a few | 3.0 for all; a few exception heroes | named with caveats |
| `B74245E8` | terminal fall speed | 30 everywhere | 30 m/s max falling | two-source constant, agreed |
| `1972C687` | unlabelled | 1.7–4.0, varies per hero | (not covered) | stays hash-labelled |

The published jump impulse is 5.72 m/s and a jump maxes at 0.98 m of gain; the
extracted mover dump has no single matching 5.72 float while our jump
controller uses it, so it is *applied* per the rule file, not read from the
component. It will graduate when the impulse consumer is found in the runtime
code.

Everything above lands as data the mover reader in `ow-tank` already exposes,
and `MoverSpec::back_speed()` / `crouch_speed()` etc. in `ow-sim::movement`
look them up by hash; the "unlabelled" table and its caveats mean the other
40+ scalars stay unnamed rather than guessed into roles.

The repo's own `ow-view --play` runs a first-person mover with all the figures
above driven live on the map geometry; its headless trace makes the values
diffable.

## The mover descriptor and its field table (from the capture)

The mover component's class descriptor sits at `0x7ff64dc80980` (found by scanning
for a descriptor whose hash slot `+0xb8` equals `0x6EFDBD73`): size **0x130**, the
STU-declared 304 exactly; **52 fields**; methods at `0x7ff64afc2160`, `0x7ff64afc22e0`,
`0x7ff64afc2370`, `0x7ff64afc24b0` (shared defaults beyond those); parent descriptor
`0x7ff64ddcffd0` (the component base). The health component's descriptor, same scan,
sits at `0x7ff64dddb2d0`: size 0x68, 12 fields.

**Field entry layout (corrected, stride 0x38, triple-verified):**

```
+0x00 u64 kind        (node vtable 0x7ff64ce43a00)
+0x08 u64 offset      (STU offset; runtime slot = offset + 4 for scalars)
+0x10 u64 field descriptor object
+0x30 u64 field hash
```

Verified three ways: the decompiled walker reads offset at +8 and the descriptor at
+0x10; brute-forcing the health array framing against the class file's twelve offsets
finds exactly one alignment; and the mover hash column at +0x30 reproduces
`m_1302B05C`, `m_9947871A`, `m_761CC74E` in order.

The mover's full table (offset / field hash / field descriptor):

| # | offset | hash | | # | offset | hash |
|---|---|---|---|---|---|---|
| 0 | 0x8 | `1302b05c` | | 26 | 0xd4 | `4e03e892` |
| 1 | 0x48 | `9947871a` | | 27 | 0xd8 | `142da8d4` |
| 2 | 0x68 | `761cc74e` | | 28 | 0xdc | `031fe7ee` |
| 3 | 0x78 | `1972c687` | | 29 | 0xe0 | `8895a9c8` |
| 4 | 0x7c | `7a70e3e3` | | 30 | 0xe4 | `ec289987` |
| 5 | 0x80 | `4a576bc2` | | 31 | 0xe8 | `c2c40969` |
| 6 | 0x84 | `03e04168` | | 32 | 0xec | `b74245e8` |
| 7 | 0x88 | `915ff368` | | 33 | 0xf0 | `975f21a5` |
| 8 | 0x8c | `d97b3735` | | 34 | 0xf4 | `b99ec983` |
| 9 | 0x90 | `d4ae9d05` | | 35 | 0xf8 | `cef4898c` |
| 10 | 0x94 | `5c6b4602` | | 36 | 0xfc | `e91d1664` |
| 11 | 0x98 | `6b7fdb90` | | 37 | 0x100 | `f227cbb6` |
| 12 | 0x9c | `e54438f7` | | 38 | 0x104 | `808374ca` |
| 13 | 0xa0 | `aebad1b0` | | 39 | 0x108 | `06b1ef76` |
| 14 | 0xa4 | `08ffaa0a` | | 40 | 0x10c | `3d76c05d` |
| 15 | 0xa8 | `cfd3e635` | | 41 | 0x110 | `b31ad888` |
| 16 | 0xac | `61d6b4e9` | | 42 | 0x114 | `4a5244c1` |
| 17 | 0xb0 | `19452f15` | | 43 | 0x118 | `5d90dbdb` |
| 18 | 0xb4 | `b470b8da` | | 44 | 0x11c | `d9ecb731` |
| 19 | 0xb8 | `aa6f4ce1` | | 45 | 0x120 | `5632f333` |
| 20 | 0xbc | `0ddd18c3` | | 46 | 0x124 | `d51c467d` |
| 21 | 0xc0 | `daa0618b` | | 47 | 0x128 | `6db41d17` |
| 22 | 0xc4 | `e631ba84` | | 48 | 0x12a | `d4a3c694` |
| 23 | 0xc8 | `d24bf4e6` | | 49 | 0x12b | `fd92c072` |
| 24 | 0xcc | `e0a0fe25` | | 50 | 0x12c | `1ff06061` |
| 25 | 0xd0 | `6f25aa23` | | 51 | 0x12d | `5aa2e07c` |

## The mover's jump struct, verified from the client's own defaults

The second capture (different session — module base shifted by ASLR,
`docs/systems-plan.md` has the translation rule) finally held the mover
component's descriptor functions, and the default provider/comparator pair
reads out the component's full default table (`dev/mover_defaults.txt`).

The standout: the mover's second inline struct — `m_9947871A`
(`STU_97288E54`) at STU offset 72 — carries the vertical/jump configuration,
and the client's comparator hardcodes its defaults. Six fields, six bit-exact
matches to the generated class file, including the field that mirrors a
published figure:

| hash | runtime slot | class file default | client constant |
|---|---|---|---|
| `16882D22` | 80 (0x50) | 20f | `0x41a00000` |
| `746B3248` | 84 (0x54) | 20f | `0x41a00000` |
| `F6B890EC` | 88 (0x58) | **0.98f** | `0x3f7ae148` |
| `D68B7D62` | 92 (0x5c) | 0.7f | `0x3f333333` |
| `9FCDBE78` | 96 (0x60) | 0.7f | `0x3f333333` |
| `766AFA5A` | 100 (0x64) | 0.2f | `0x3e4ccccd` |

(Runtime slot = STU offset + 4; these are absolute runtime slots since 72 is
measured from the component's STU base, making them 76+4... as tabulated.)

**0.98 m is the published jump apex height** ("heroes can reach a height of
0.98 meters before they start moving downwards") — the mover's own defaults
carry it, so the sim's jumps and the client's numbers now share a verified
constant rather than an assumed one.

## The character step's callers, and the interface cluster (this round)

The Domino chain in `docs/formats/domino.md` ends at the character-shaped step
`FUN_7ff75625e790`; this round found what calls it, and where the mover band
meets it. All addresses below are exec-capture coordinates
(`dev/module.bin`), the session whose code the step chain was read from.

**The chain, upward:**

```text
FUN_7ff7541df690   entity advance (mover interface band)
  │                only reference: a slot at v2b 0x7ff7c7f033f0
  └─ FUN_7ff756225fe0(worldHandle, stepState, request)
       └─ FUN_7ff75625e790(world, scratch, request)   the character step
```

* `FUN_7ff7541df690` reads a physics world handle at `param_1 + 0x35b*8`
  (offset 0x1AD8), and selects between a character path (taken when the bytes
  at `+0x34f` and `+0xcb` are both set) and a plain-body path. The character
  path looks a **32-bit id at `param_1+0xe4`** up in a 0x30-stride table
  (`param_1[0xd2]`, count `param_1[0xd3]`) whose entries carry a `(min,max)`
  u32 range at `+0x1c`/`+0x20` and an object at `+0x28`, and virtual-calls
  slot `0x78` of that object (an accept/filter test, iterated to the first
  accepting entry). The plain-body path is a compact advance —
  `position += (scalar·component + base) · dt + offset` — issuing two calls on
  the world handle (`FUN_7ff756225fc0`, `FUN_7ff7562262b0`).
* `FUN_7ff756225fe0` builds the step request from a small struct (a position
  pair and five scalars), sets the constants **0x10 and 0x20**, calls the step
  with a 512-byte scratch buffer, then sizes and fills **two arrays of
  0x20-byte elements** on the step-state object — capacity/pointer at
  `+0x208`/`+0x200`, and `+0x618`/`+0x610` with its inline buffer at `+0x210`
  — storing the two counts it reads back at `+0x20c` and `+0x61c`. So the
  "character step" is a **query with two output lists**, not a bare
  integrator.

**The interface cluster.** The reference that names the advance is a qword at
v2b `0x7ff7c7f033f0` (exec `0x7ff755f033f0`), inside a contiguous block of
interface tables at v2b `0x7ff7c7f03030..0x7ff7c7f03448`. The mover's three
documented interface vtables sit in the same block as 0x98-byte, 19-slot
records (exec `0x7ff7566e30c8`, `0x7ff7566e3160`, `0x7ff7566e31f8`), and the
block is followed by the family's constants (3.0, 0.5, 2.1, π/2, FLT_MAX) and
its debug format strings (`%s-%03d`, `%03d`, ` (%s)`) — a class-family data
island, not a lone vtable.

**Address-space note for the tooling.** The pointers stored in the module dumps
are session addresses of the dump's own session: the slots above hold
`0x7ff7c5…` values in `module_v2full.bin` (the v2b session), so an exec-capture
address converts to that dump by `+0x71820000`, and to the standing range
capture by `−0x1096A0000`. The `file = VA − 0x7ff649e60000` rule holds for
range coordinates exactly as before; it is the *stored pointers*, not the file
offsets, that follow the session base.

### Who invokes the advance, and the object it belongs to (found)

The advance is **slot 15** (`vtbl+0x78`) of the interface vtable at exec
`0x7ff7566e3378` (v2b `0x7ff7c7f03378`, range `0x7ff64c863378`). Tracing that
slot's installers and overriders produces the whole dispatch chain and the
object's identity:

* **Base constructor `FUN_7ff7541dae80`** installs that vtable at object `+0`
  and a second one (exec `0x7ff7566e3430`) at `+0x20`, plus **three embedded
  0x160-byte subobjects** at `+0x268`, `+0x3C8`, `+0x528`, each carrying
  `0x32d0` at `+0`, the mover interface vtable `0x7ff7566e30c8` at `+8` and
  `0x7ff7566e3160` at `+0x48`. So this object embeds mover-interface
  implementations; that is why the mover's interface vtables are referenced
  three times from the constructor band.
* The base object's own fields are exactly the ones the advance reads:
  `+0x35b` (the physics world handle, zeroed in the constructor, released in
  the destructor, read by the advance at `+0x1AD8`), `+0xe4` (the id the
  advance range-looks-up), `+0xd2`/`+0xd3` (the 0x30-stride `(min,max)` range
  table it walks), `+0x21a` (flags registered by `FUN_7ff753b32310`).
* **Concrete constructor `FUN_7ff753d20750`** calls the base constructor and
  re-installs vtables exec `0x7ff75656e000` (`+0`) and `0x7ff75656e0b8`
  (`+0x20`) — the concrete class. Its own method band is `0x7ff753d20xxx`.
* **The advance's invoker is the concrete class's slot-15 override
  `FUN_7ff753d21260`.** Disassembly is unambiguous: it *directly calls* the
  base advance (`call 0x7ff7541df690` at `0x7ff753d212bc`), immediately calls
  its own slot 21 (`call [rax+0xa8]`), then **consumes the query's two output
  lists** — reading the step-state fields the step caller fills
  (`[r15+0x200]` pointer and `[r15+0x20c]` count) — iterating the returned
  0x20-byte elements, subtracting the object's own 16-byte vector at `+0x1B80`
  from each and keeping a running maximum of a horizontal dot-product score
  (`mulps`/`shufps`/`addps`/`comiss`). The method is a scored probe over the
  advance's contact list, not a pass-through.
* The **construction is dispatched indirectly**: the concrete constructor's
  only caller is `FUN_7ff753d20710` (43 bytes, no static callers) — it calls
  `FUN_7ff755197130(param+0x10)` and constructs the class only when a flag bit
  at `param+0x28` is set. The driver that builds and drives these objects is
  therefore still not named statically; the object family is.
* The destructor chain confirms the same object: concrete deleting destructor
  slot 0 = `FUN_7ff753d207d0` → base destructor `FUN_7ff7541db9c0` → deleting
  thunk `FUN_7ff7541dc100`; the base destructor releases the world handle
  (`FUN_7ff756225f70(param[0x35b])`) and frees the range table, the
  `0x18`-stride list (`+0x216`/`+0x217`), the `0x40`-stride list
  (`+0xec`/`+0xed`), a guid pair (`+0x34d`) and 0x50 bytes at `+0x361`.

What this adds: the advance's invoker is now a named function with a verified
call site, the object it belongs to has a known constructor/destructor pair and
vtable set, and the returned contact lists are known to be *scored* against the
object's `+0x1B80` vector. Still open: the class's name (its descriptors are
not in the extracted class artifacts, so it is an engine class, not an
STU-generated one), and the driver that constructs these objects — the input
path enters above `FUN_7ff753d21260`, whose motion arguments come from its own
fields.

## The construction path behind the mover class (this round)

All addresses v2b; exec = v2b − `0x71820000`, range = v2b − `0x17AEC0000`.

**The advance has exactly one caller in the image.** A full-module scan for
`E8` into the base advance (`v2b 0x7ff7c59ff690`, exec `0x7ff7541df690`) returns
a single call site, `v2b 0x7ff7c55412bc` — the concrete slot-15 override itself.
Nothing else invokes the advance directly; every other path must go through the
vtable, so "the driver" is whatever holds the object, not whatever calls the
advance.

**The class is a registered engine class, named `"Unknown"`.** The name
records are `0x7ff7c86a72d8` and `0x7ff7c86a72c0` — each
`{key, len = 7, "Unknown"}` in plaintext (the registrar's XOR loop is a
verify/no-op for these). A **registrar** at `FUN_7ff7c5540870` builds the class
definition and registers it:

* allocates 0x30 bytes (pool allocator `FUN_7ff7c6918e00`) for the definition;
* writes the **factory table pointer `0x7ff7c7d8dfd0`** into it and initializes
  the sub-record via `FUN_7ff7c69b6d60(def+0x10, id)`;
* registers through `FUN_7ff7c69b6f10(def+0x10, …, 0x14, 0x1c00)` →
  `FUN_7ff7c69cc160` (the type-registration core: allocates per-flag tables
  through the pool allocator, computes the class alignment 4/8/16 from the
  flags word, and registers against the TLS type id);
* the size passed is **`0x1c00`** — the mover object's size, and the same
  constant the deleting destructor frees with.

**The class's code band** (v2b `0x7ff7c55405d0..0x7ff7c5544xxx`) is the family:

* the factory table `0x7ff7c7d8dfd0` = `{0x7ff7c54bb320, 0x7ff7c5540710}`;
  `0x7ff7c5540710` is an 18-byte adapter calling the **shared** init
  `FUN_7ff7c69b7130(obj+0x10)` (321 callers — not per-class);
* the constructor `FUN_7ff7c5540750` installs the vtables `0x7ff7c7d8e000` and
  `0x7ff7c7d8e0b8` and initializes the object's tail fields (`+0x1ba0` list
  head, `+0x1ba8`, `+0x1bac = 0x80000004`, `+0x1bf2`, the vector block around
  `+0x1ba0`);
* the deleting destructor `0x7ff7c55407d0` releases `[+0x1ba0]` (via
  `0x7ff7c529ee20` with the static `0x7ff7c884bd08`), calls the base destructor
  `FUN_7ff7c59fb9c0`, and frees with `FUN_7ff7c6918e60(this, 0x1c00)`;
* **adjustor thunks**: `0x7ff7c55407c4` (`this − 0x20`) and `0x7ff7c5540850`,
  which forwards `[rdx]->slot 1(this, this+0x1c0)` — the object embeds an
  **interface subobject at `+0x1c0`**, matching the interface family's 0x1c0
  geometry.

**The registration list is static and its bootstrap is not in the captures.**
A registry at v2b `0x7ff7c89dc810..0x7ff7c89dcc80+` holds `prev`-linked
records of 0x28 bytes — `{prev, handle, registrar, ctx = 0x7ff7c8fcad20,
active}` — with the sentinel `prev = 1` at `0x7ff7c89dc8b0` and inactive slots
skipped. The family's registrars appear consecutively:
`0x7ff7c553f900, 0x7ff7c553ff60, 0x7ff7c55405d0, 0x7ff7c5540870 (this class),
0x7ff7c5542830, 0x7ff7c5543790, 0x7ff7c5543bd0, 0x7ff7c5543db0,
0x7ff7c5544750, …`. The `handle` fields are byte offsets 1–2 bytes apart into
an **encoded descriptor stream** at `0x7ff7c8fcxxxx` (not decodable without
that stream's key). No code in the module references the list — **zero
RIP-relative references and zero external qword pointers** — so the walker
that calls each registrar at startup lives outside the captured bands.

**The component attach path is named.** `FUN_7ff7c5c20bd0` (v2b) is an
interface-type cascade — a run of `vtable[1](obj, &descriptor)` queries against
a dozen interface descriptors, each miss/case dispatching to its own handler —
and its **mover case** (`0x7ff7c8ebb650`) stores the three mover interface
vtables `0x7ff7c7f030c8` / `0x7ff7c7f03160` / `0x7ff7c7f032d0` into local
interface slots and calls the mover setup `FUN_7ff7c59ff390` (its only
caller). This is the closest thing to a driver that exists as a named function:
it *attaches* the mover interfaces to a component object.

Summed up: the "driver" is the type system's factory path — registrar →
definition → factory table (`{create adapter, shared init}`) — plus the
component attach cascade that installs the mover interfaces. What stays open is
the startup walker of the registrar list (outside the captures) and the class's
real name (registered as `"Unknown"`).

**Cross-link (row 7 pairing):** the component router pairs this component as
**id `0x26` → `STUCharacterMoverComponent`** — schema size `0x130`, which is
exactly the live mover object's size (52 fields, parent
`STUMirroredEntityComponent`; the registry sweep had missed the class because
it registers through the lazy TLS-guarded path). So the mover object this
document follows is the runtime of the mover *component*, and its schema name
is known even though the engine class itself registers as "Unknown". See
`docs/systems-plan.md` and `dev/component_router_paired.txt`.

## The mover integrates per frame, and its rates are measured (this round)

The keyed captures (`D:/ow-dumps/trace/player_fast3_keys.jsonl`,
`player_keyed.jsonl`) write the traced node **once per frame**, and the
per-frame displacement is `v * frame_dt` with a stable per-frame dt — the
plateau step repeats to all bits (0.1777004 m) and only slips ~0.6% on
individual writes. The write cadence over 189 intervals is 6.46 samples at
200 Hz = **0.0323 s**, not 1/30 s (which would be 6.67). So the *character
mover* integrates once per frame with the frame's own dt; the fixed 1/30 s
step in `domino.md` is the *world* tick, not the mover's stride. This corrects
the earlier "the mover is written at 30 Hz" reading — it was the sample count,
not the physics.

Measured from the traces (all at dt = 0.032309 s, the frame dt that makes the
step exactly `5.5 * dt`):

| quantity | measurement | value |
|---|---|---|
| forward/side travel speed | plateau step 0.1777004 m | **5.500 m/s** = the `19452F15` slot |
| backpedal speed | plateau step 0.1599036 m | **4.950 m/s** = the `61D6B4E9` slot = 0.9 x forward |
| diagonal (W+D) | same plateau | full speed, input normalised, no penalty |
| acceleration | 3.9216 m/s gained in the first full frame from rest | **121.4 m/s^2** |
| friction | constant 2.345 m/s shed per frame (2.391/2.303 and 2.353/2.333) | **72.6 m/s^2** |
| gravity | constant 0.5613 m/s shed per frame, variance tracks frame dt | **17.37 m/s^2** |
| jump impulse | first-frame dy 0.15426 m = (v0 - g*dt)*dt; discrete ascent 0.7333 m | **5.33 m/s** |
| airborne, no input held | the traced step holds its exact bits for the whole fall, no decay | **preserved** |
| airborne, direction held | the step grows +0.00533 m per frame until it holds at the slot (`player_ctrl.jsonl`, 2026-10-15: 19.24 / 19.45 / 19.10 m/s^2 over three runs) | **19.2 m/s^2** = `AIR_ACCEL` |
| crouch walk | plateau step 0.04956 m at 1/60 s; clean segments read 2.960 / 2.970 / 2.973 / 3.011 m/s | **3.000 m/s** = the `0DDD18C3` slot |
| crouch transition | the slot switch completes in 1-2 frames at the ground rate: 5.5 -> 3.46 -> 2.97 m/s (i=10147) and 2.97 -> 4.16 -> 5.54 m/s (i=8023) | rate-limited, no clamp and no reset |
| direction | ramp step parallel to (sin yaw, cos yaw) — plateaus agree with the recorded yaw to 0.006° | vector ramp, not per-axis |

Two structural facts follow from the ramp shapes: the approach is along the
velocity *vector* (a per-component clamp would swing the direction during a
turn, and the traced ramp keeps it exactly), and the release sheds a constant
speed (linear, not proportional to v: the decrements are 2.35 at v = 5.1 and
2.33 at v = 2.7).

**Code anchors for the same band** (v2b coordinates, `module_v2full.bin`):

* `FUN_7ff7c5b14a84` is a collision-sweep helper: it builds a session
  (`FUN_7ff7c7a45e40`), then loops `i = 1..3` sweeping `(i/3) * delta`
  (`DAT_7ff7c7e1ae88` = 0.3333333) through `FUN_7ff7c7a46260` +
  `FUN_7ff7c7a45fe0`, with a request carrying **1/60** (0x3c888889) and
  **0.005**, and the constant **1.6666666 (5/3)** stored just before the
  session create. The same query/invoker pair is called from
  `FUN_7ff7c5c1cbf0` (the step API from the earlier round) and from
  `FUN_7ff7c5a0e610` / `FUN_7ff7c5a187d0` / `FUN_7ff7c59ff690`.
* The ramp constants are **not literals in the code module**: a scan of
  `module_v2full.bin` finds zero hits for 3.9216, 2.35, 117.6, 122.2, 70.56,
  73.33, 5.72 or 4.95. The speeds are per-hero data (as established above) and
  the rates are computed — the mover control (Q3) is still unread as code, so
  the rates above stand as *trace-measured*, not as the client's expressions.
* The mover's own jump struct defaults (earlier round: gravity 20/20, apex
  **0.98**, 0.7/0.7/0.2) do **not** match the measured pair: at g = 17.37 and
  v0 = 5.33 the discrete apex is 0.733 m, and 0.98 m would need
  v0 = 5.83 m/s. The 20/0.98 pair is the class default; the live values are
  what the trace shows. A gravity scale of 0.8685 x 20 would produce 17.37 —
  unverified, recorded as a candidate.

**Verification.** `cargo run -p ow-sim --bin trace_diff -- keys
D:/ow-dumps/trace/player_fast3_keys.jsonl --by-state` replays the trace's own
key timeline through the model with each segment's own frame dt and prints a
per-segment coverage table: **no divergence across 101 compared writes** (all
101 inside the client's frame-jitter band, 75 environment-affected
wall-slide/terrain writes screened out, 20 skipped at input transitions), max
step deviation **0.0018616 m** on a 0.178 m step (1.0%). The unit tests in
`ow-sim::movement` carry the measured golden vectors (the two-frame ramp, the
jump arc, the preserved airborne velocity).

## The character movement pipeline, as far as it is read (this round)

This round walked the movement chain end to end by *anchors* rather than by
following calls from an entry point, because the subsystem is large and partly
obfuscated. All addresses are v2b (`module_v2full.bin`); exec = v2b −
`0x71820000`.

**The object.** The character/mover object is 0x1c00 bytes (the size the class
registrar carries). Its fields, as read from the code that touches them:

| byte offset | what the code does with it |
|---|---|
| `+0x10d0` | a u32 flags word; every layer reads or sets bits in it (`0x80000c`, `0x20000`, `0x80000`) |
| `+0x10ec` | a float timer, compared against the input's `[7]` |
| `+0x1140..0x114c` | a 4-float **local wish vector** — the quantity the movement layers pass around |
| `+0x1520`/`+0x1524` | a u32 pair (a state id) compared against the input's `[0x114]`/`[0x115]` |
| `+0x1a77`/`+0x1a78`/`+0x1a79` | state bytes (a movement state id 0x7b/0x7c is written into `+0x1a79`) |
| `+0x1a90..+0x1acc` | a 4×4 matrix (the character's world transform; translation at `+0x1ac0`) |
| `+0x1ad8` | the physics world handle (`param_1[0x35b]`) |
| `+0x1b80..+0x1bbf` | the committed transform: position quad + three 16-byte basis vectors |
| `+0x1cd0` | a pointer field compared against the null sentinel |

**The layers, in the order the code implies.**

* `FUN_7ff7c59ff690` — **the advance** (2215 bytes; the interface vtable's
  slot 15, confirmed by the qword at v2b `0x7ff7c7f033f0` = this address). It
  computes the desired position as
  `out_i = (a * X_i + pos_i) * b + off_i` for the four components, where `X`
  is the object's quad at `+0xc0..0xcc`, `pos` is the input position, and
  `a`/`b`/`off` are parameters; it reads the world handle at `+0x1ad8` and the
  flags at `+0x10d0`; and it dots a direction against `[obj+0xc0]`
  (`MULPS xmm2, [RDI+0xc0]` at `0x7ff7c5a11a20`) — so the quad at `+0xc0` is a
  velocity-like vector (a dot with it is a speed). Its character path is the
  sweep (below); its plain-body path is the formula. Note the object it reads
  (`+0x1ad8`, `+0x1b80`) is the 0x1c00-byte mover, *not* the 0x130-byte mover
  component: the component's own field at runtime `+0xc0` is the crouch speed
  (STU 0xbc + 4), so the two objects' layouts must not be conflated.
* `FUN_7ff7c5541260` — **the invoker** (773 bytes; the concrete class's slot-15
  override, vtable `0x7ff7c7d8e000` + 0x78). It calls the advance, calls its
  own slot 21 (`+0xa8`), stores the new position into the object's `+0x1b80`
  quad, then computes the movement delta and dot-products it against every
  contact normal in the step state's output list (`[state+0x200]`, 0x20-byte
  elements, normal at `+0..+8`), keeping the maximum; if that exceeds
  `_DAT_7ff7c86a72f0` it looks the element's id at `+0x14` up
  (`FUN_7ff7c7a43560`) and dispatches a handler (`FUN_7ff7c6d506b0`). So the
  invoker is *advance + collision notification*, not the control.
* `FUN_7ff7c5b14a84` — the **collision-sweep helper** (from the previous
  round): session create `FUN_7ff7c7a45e40`, then `i = 1..3` sweeping
  `(i/3) * delta` (`DAT_7ff7c7e1ae88` = 0.3333333) through
  `FUN_7ff7c7a46260` + `FUN_7ff7c7a45fe0`, request carrying 1/60 and 0.005,
  and the constant 5/3 stored before the session create.
* `FUN_7ff7c5a12c50` — **the per-frame character state machine** (530 bytes).
  Reads the flags, the timer and the state id; writes the state byte
  `+0x1a79` (0x7b/0x7c); on a state change runs `FUN_7ff7c5a00a50` +
  `FUN_7ff7c5a00e80`; transforms the local vector at `+0x1140` by the matrix at
  `+0x1a90` and applies it (`FUN_7ff7c59f1b90` + `FUN_7ff7c59f9bd0` — the same
  apply helper the advance uses for its snap); calls the ground probe
  `FUN_7ff7c5a0e610` *conditionally* (on a flag-bit change or the timer);
  then `FUN_7ff7c5a14c60`, `FUN_7ff7c5a18450`, `FUN_7ff7c5a186a0(state)`.
* `FUN_7ff7c5c552e0` (and its siblings `FUN_7ff7c5c54d80` /
  `FUN_7ff7c5c55a20`) — **state transitions**: they preserve the local vector,
  copy it and the object's `+0x40..0x4c` quad out, call
  `FUN_7ff7c5a12e70`, clear a flag bit, then call the probe
  `FUN_7ff7c5a187d0` and `FUN_7ff7c5a14690`.
* `FUN_7ff7c59fd050` (327 bytes, called from `FUN_7ff7c5a12e70`) — the
  **wish-vector update wrapper**: reads `+0x1140..0x114c`, calls
  `FUN_7ff7c5a0ce70` with the state id pair and a context, writes the result
  back, then applies the *difference* of two vectors from
  `FUN_7ff7c59f69b0` / `FUN_7ff7c59f67d0` via `FUN_7ff7c59f9df0`.
* `FUN_7ff7c5a0ce70` (443 bytes, 4 callers) — a **frame conversion**: it takes
  a matrix from `FUN_7ff7c5a10210`, inverts it (adjugate + `rcpps` + one Newton
  refinement), and transforms the passed 4-vector by the inverse. It converts
  the stored vector between coordinate spaces; it is *not* the input→velocity
  ramp.
* The **data applier** (a function in the `0x7ff7c59e8xxx` band) walks the
  component's field records (`[component+0x1a8 + 8i]`) and stores each
  converted float into consecutive runtime slots `+0xac + 4i` — the mover's
  per-hero data (the crouch speed at runtime `+0xc0`, the speed family at
  `+0xe0`/`+0xe4`, `daa0618b` = 35, `e631ba84` = 3.5, `e0a0fe25` = 15 for most
  heroes — per `dev/mover_values.txt`).

**What remains, and where the evidence points.** The step that turns WASD into
the local wish vector — the ramp itself — was *not* found as code, and the
evidence now points away from code: (a) a scan of the whole module finds **no
literal** for any measured rate (3.9216, 2.35, 117.6, 122.2, 5.72, 4.95: zero
hits), (b) the speeds are per-hero *data* the applier copies in, and (c) the
layers above consume a *vector* plus a *state id pair* (`+0x1520`/`+0x1524`),
which is how the statescript runtime identifies a state. The working hypothesis
is therefore that the input→velocity rule lives in the hero's **statescript
graph** (the runtime for which is already read: materializer `FUN_7ff7c6f7e310`,
node dispatch `[S+0x68]==1 ? pair : pair+8)[node+0x1a8]`), not in a C++
control — which is also why no fitted constant appears in the code. That is a
hypothesis, not a reading, and it is recorded as such. The band is also partly
obfuscated (`FUN_7ff7c5a191c0` is 34 bytes that decompile to junk), so a
straight-line decompile of that band is not reliable.

**One more closure from the same band.** `FUN_7ff7c59f7a70` (5005 bytes, a
vtable method) zeroes a 0x100-byte block and then interpolates the character's
display transform component-wise:
`out = (dst − src) * k + src` (`MOVUPS xmm1,[r15+0x390]; SUBPS xmm1,[rbx+0x10];
MULPS xmm1,xmm6; ADDPS xmm1,[rbx+0x10]; MOVUPS [r14+0x390],xmm1`, repeated per
16-byte lane at `+0x390`/`+0x3a0`/…). That is the **visual smoothing** of the
transform, and it explains the trace differ's "settling" class: after a stop the
traced node's last millimetres are this lerp converging, not the mover's
integration — which is exactly how the differ treats them (writes below 1 cm are
classed settling, not movement). It also confirms the traced node is the
display/animation-side transform, as `domino.md` already recorded.

## The ramp hunt: what is falsified, and what it needs (this round)

Three routes were closed this round, each with a concrete negative:

0. **The graphs carry no crouch state either.** A dedicated grep of every
   graph asset of type 0x01B (`params --hero 2 --all-graphs`, filtered on
   `crouch|duck|sneak|prone|stance`) finds no state and no field — so the
   crouch, like the base walk, is engine-level, and its transition timing is
   the same class as the ramp: not in the data, not readable in the obfuscated
   control, measurable only from a trace. That is why the operator's capture is
   the sole route to it, and why the result will enter the M6 table as tier 3
   with a written boundary like the ramp's.
1. **The statescript graphs do not carry the movement values.** Walking every
   graph asset of type 0x01B (`cargo run -p ow-tank --bin params -- --hero 2
   --all-graphs`, then filtered on the mover's field hashes) finds **no row
   owned by `19452F15` (forward), `CFD3E635` (side), `61D6B4E9` (back),
   `0DDD18C3` (crouch) or `1972C687`** — the only `B74245E8` rows are other
   states' fields that share the hash, with values 1.25/3/4, not the 30 m/s
   terminal. So the mover's speeds are per-hero *component data* (as the STU
   extraction already showed), not script literals, and the input→velocity ramp
   is not a graph node.
2. **Neither capture contains a store to the mover's velocity quad.** A byte
   scan of `module_v2full.bin` *and* of all 1695 exec region dumps in
   `D:/ow-dumps/dumps_exec` (patterns: `movss`/`movups`/`movaps`/`movdqu` to
   `[reg+0xc0]`, with the REX and disp32 forms, stack bases excluded) finds
   **zero** sites. The only writes to `+0xc0` are the data applier's per-field
   scalar stores and a zero-fill loop.
3. **No literal for any measured rate exists in either capture.**
   `121.4 (0x42F2CCCD)`, `72.6 (0x42913333)`, `17.37 (0x418AF5C3)`,
   `5.33 (0x40AA8F5C)`, `5.5 (0x40B00000)`, `4.95 (0x409E6666)` — zero hits in
   the v2b module and zero in the exec regions. (60.0, 7.5 and 20.0 do exist;
   36.0 does not.)

The captures are partial — `module_v2full.bin` is **52% zero bytes** (1388 runs
of ≥0x1000) — and although the bands read this round (advance, invoker, state
machine, character band, frame conversion) are resident, the code that computes
the ramp from input + the mover's data is not among the resident pages. The
candidate arithmetic was checked against the data and does not close either:
`gravity = 35 × 0.5 = 17.5` is within 0.75% of the measured 17.37, but no
product of the mover's fields reproduces 121.38 and 72.58 together (35 × 3.5 =
122.5 is 0.9% high; the ratio 121.38/72.58 = 1.672 is 0.34% off 5/3).

**What unblocks it:** one capture whose resident pages include the mover
control (the pages the v2b and exec dumps both lack), taken while holding
`ctrl` and jumping mid-motion — that single capture would let the ramp be read
*and* measure crouch timing and air control directly. New live captures are the
operator's decision (the client runs Warden).

## A better capture, and the rate hypothesis with numbers (this round)

**`module_warm.bin` is a far more complete capture of the same module.** Same
size as `module_v2full.bin` (0x42e0000) but **14.1% zero bytes against 52.0%**,
and its file base is `0x7ff7c4d20c00` (verified by fingerprinting the invoker's
`call` at file 0x8206bc against v2b `0x7ff7c55412bc`; the v2full dump's offset
for the same bytes is 0xc00 higher). It is therefore the capture the earlier
notes asked for: the mover band `0x7ff7c776a000` is **zero-filled in v2full and
resident in warm**. The four hot bands were extracted
(`D:/ow-dumps/warm_regions/`, 0x7ff7c766/770/774/776) and wrapped with
`dev/mkpe.py` into `mover_ctl.exe` (328,704 bytes, image base 0x7ff7c765f000,
4 sections), which is imported in the Ghidra project as 640 analyzed
functions — a durable asset for the next session.

**What the warm bands answered, and what they did not.** The `+0xc0` hits in
`0x7ff7c776a030` are a **reset function** (it zeroes a big object and writes
1.0f identity values; the object's layout includes the transform at `+0x1b80`,
so it is the same entity/mover family). No *computed* store to the velocity
quad exists in the warm capture either — not as movss/movups/movaps/movdqu,
not as mov32/mov64/imm, not in SIB form (all four scanned). This matches the
earlier heap-sweep negative: **the character's velocity is computed per frame
and consumed within the mover; it is not a stored field the captures expose.**

**The rate hypothesis, with its arithmetic.** The measured rates are
`ACCEL = 121.38 m/s²` and `FRICTION = 72.58 m/s²` (from 0.1267069 m of extra
displacement over one 0.032309 s frame and a constant 2.345 m/s shed per
frame). Per 1/30 s tick those are **60.69 and 36.29**, i.e. within ~1% of
**60 and 36**, whose ratio is exactly **5/3** — the same 5/3 constant the
collision-sweep helper stores (`DAT_7ff7c7e1ae88`'s sibling, 0x3FD55555). Both
literals exist in the captures (60.0: 151 hits in v2full, 105 in warm; 36.0: 86
and 39) but never paired within 0x60 bytes, so the code that uses them is not
yet identified — and 60.0 is also a common frame-rate constant, so the pairing
is weak evidence on its own. The mover's unlabelled per-hero scalar
`1972c687` = 2.0 for most heroes (1.7–4.0 across the cast), and
`2.0 × (60, 36) = (120, 72)` sits 1.2%/0.8% below the measured pair — inside
the measurement band. **Recorded as a hypothesis: the ramp is a base rate
(60, 36) scaled by the per-hero scalar, applied on the 1/30 s tick while the
position integrates per frame.** It is not a reading, and the graph walk rules
out the script as its home: `params --hero 2 --all-graphs` shows only *ability*
movement states (`STUStatescriptStateMoveTo` with dash-like values 100/50/10 m,
`STUStatescriptStateShockwave.speed`, `WeaponVolley.projectileSpeed`), no base
walk state.

## The speed-field readers are the field machinery, not movement (this round)

Following the rate hypothesis into `mover_ctl.exe` produced one more decisive
negative. The byte-pattern scan for readers of the mover's forward-speed field
`[obj+0xb4]` finds eight sites; decompiling them shows they are **not movement
code**:

* `0x7ff7c59f6f31` — the field-by-field **comparator** (dirty-mask gated
  `UCOMISS` pairs between two objects; the whole field table is walked this
  way);
* `0x7ff7c7663dc9` — the **script opcode applier** (`switch(opcode)`, case 5
  does `*(float*)(obj+0xb4) += value` with a cap and a flag set, case 0x11
  accumulates, 0x1d–0x21 dispatch to a sub-object) — a generic
  field-modification node, not the mover;
* the rest are **data false positives**: the pattern
  `f3 0f 10 87 b4 00 00 00` occurs inside the module's data region, where the
  bytes decode as ASCII (`0x6170206e` = "n pa", `0x65766572` = "reve", …), and
  the raw disassembly around them starts on zero bytes.

So the mover's own speed field is read *only* by the generic field machinery.
Combined with the earlier results — no computed store to the velocity quad in
any encoding in v2full, the exec regions or warm; no literal for any measured
rate; the graphs carry only ability movement — the consistent conclusion is:
**the movement parameters are copied out of the component once, at bind time,
into the controller's own state** (the bind path `FUN_7ff7c6e08510` is already
read: the config's float is stored once into a derived counter), and the ramp
reads that cached copy, which is not at any offset this hunt has keyed on. That
is the next, better-formed question: find the controller's cached parameter
block (bind-time copies of `19452F15`/`61D6B4E9`/`0DDD18C3`/`1972C687`).

Also settled this round: the `0x7ff7c776axxx` band in warm is a **reset
function for a 6×0xd8-matrix + 16×0xd8-matrix object** (identity writes, not
mover data), and `lea [obj+0xc4]` there is an inline-buffer container idiom on
a stack object — both false positives, now excluded.

## The verdict: the control is obfuscated (tier 3), and the behaviour is measured

Four rounds of hunting have exhausted every readable route to the ramp, and the
evidence now points to one conclusion rather than another offset coincidence.
**The character movement control is obfuscated** — most plausibly a bytecode/VM
layer, using the same `(ptr + K) ^ K2` pointer idiom this project already
identified in the Domino handle table and in the mover class's vtables.

The evidence, accumulated:

1. **The concrete class's second vtable carries encrypted slots.**
   `0x7ff7c7d8e0b8`'s slots 13+ read as noise
   (`0xa24b69a6c1deedc9`, `0xa5ae2a583bd75a70`, `0x8928c6c0ee4c0b07`, …) where
   slots 0–12 are ordinary code pointers — the same `(ptr+K)^K2` shape as the
   handle table.
2. **Junk functions in the character band.** `FUN_7ff7c5a191c0` is 34 bytes
   that decompile to hundreds of kilobytes of repeated
   `*in_RAX = *in_RAX + cVar8` — decompiler-hostile bytes, not real code.
3. **No literal for any measured rate** exists in v2full, in the 1695 exec
   regions, or in warm (121.4, 72.6, 17.37, 5.33, 5.5, 4.95 — zero hits;
   60.0/36.0 exist but never paired within 0x60 bytes, and 60.0 is also a
   frame-rate constant).
4. **No computed store to the mover's velocity quad** in any encoding — movss,
   movups, movaps, movdqu, mov32, mov64, imm32, SIB forms — in any capture. The
   only writes are the data applier's scalars and reset/zero loops.
5. **No movement code reads the mover's speed fields.** The `[obj+0xb4]`
   readers are the field comparator and the script opcode applier; the
   `[obj+0xe0]` readers are skeletal-animation functions (quaternion math, bone
   transforms at +0xd0/+0xe0/+0xf0). Every hit outside those was a data false
   positive (the byte pattern decodes as ASCII) or a stack frame.

**What this means for the goal.** The movement *behaviour* is fully measured
from the client's own traces and verified against them (101/101 comparable
writes inside the frame-jitter floor, max step deviation 0.0018616 m), and the
measured arithmetic is implemented in `ow-sim::movement` with golden-vector
tests. The *implementation* — the client's own expressions — is obfuscated and
is therefore **declared tier 3** under this project's own rule ("unrecoverable
stages declared tier 3", `AGENTS.md`): the numbers stand as trace-measured, not
as a reading, and no further capture of the same kind will change that. The one
remaining *behavioural* gap (crouch transition timing) is measurable from a
trace rather than readable from code, which is why the operator's capture is
the right next step for it.

Assets left for the next session: `D:/ow-dumps/warm_regions/` (the four mover
bands plus `mover_ctl.exe`, and the four speed-reader bands plus
`speed_readers.exe`), both imported in the Ghidra project.

## The slope-following rule, measured (this round)

The keyed trace's backpedal climb (`player_keyed.jsonl`, i=352, frames 22–36)
walks up a slope at a **constant** horizontal 4.699 m/s and vertical
+1.523 m/s — a 17.9° grade — and the vector magnitude is
`sqrt(4.699² + 1.523²) = 4.940 m/s`, i.e. **the backpedal slot 4.95 to 0.2%**.
So on walkable ground the mover keeps the *slot speed along the ground
surface*: the horizontal component is `speed·cos(slope)` and the vertical
`speed·sin(slope)`, both constant while the slope is constant. (The same trace's
flat plateaus are the same number without the vertical component: 0.1599036 m
per frame = 4.95 × 0.032309.)

The direction is the wish's, not a rotated one: the per-write heading is
constant at **9.7°** through the climb, and the forward run down the same ramp
is constant at **−170.5°** — the exact reverse to 0.2°. So the slope rule
changes the *speed* along the surface, not the horizontal heading. (That also
means this trace cannot tell a uniform horizontal scaling from a true surface
projection — they agree when the wish lies along the fall line, which it does
here. The runs' tails swing — 144°, 113°, 163° on the way down, −35.4° at the
climb's end — exactly where they meet geometry, which is the terrain the differ
screens.)

The steeper case is different and stays unmodelled: the strafe run of
`player_keyed.jsonl` (i=616..760, 77 writes) descends 2.51 m over 10.12 m and
contains a **straight constant phase** (i=630..709, 43 writes) at 4.2539 m/s
horizontal and −1.9738 m/s vertical — a 24.89° grade — with magnitude
**4.6895 m/s, not a slot value** (0.853 × the 5.5 walk slot). The phase's net
grade equals its instantaneous grade and its direction is constant (5.747 m of
straight path at 144.67°), i.e. a fall-line-shaped descent; the run ends
exactly where the traced jump happens, so the terrain there has a ~2.5 m
descent. The trace cannot say whether the client called it grounded: the
recorded samples carry no body flags and their `vel` field is zero (the traced
node is the display transform, not the body), and with no yaw field the wish
direction during the slide is unrecorded (the session's walk direction is
pinned to ~189.5° only *before* and *after* it, by the stop-glide residuals at
i=263 and i=1056). The geometry check is available but bounded — measured this
round: neither the default 12-chunk slice nor the 31-chunk interiors set
(`dev/range_chunks.txt`) reaches the trace area (even the trace's own spawn
point returns the fallback floor; the interiors floors sit at y ≈ 4–6 around
(24..30, −4..1)), and `--max-chunks` walks 41,811 chunks in manifest order, so
confirming the grade needs a proximity walk seeded at (54..61, 12..21). It
would only confirm the terrain, not the speed rule, which is the part that
differs: this is a slide/lost-footing state rather than ground-following, and
its rule belongs to the obfuscated control (tier 3) or to terrain behaviour. **Implemented** (this round): `Controller::ground_normal` (default `[0,1,0]`)
scales the wish direction by the directional slope's cosine and carries the
sine share as `slope_vertical`, applied to the height while grounded — so the
mover holds the slot speed along the surface. The golden test
(`slope_following_holds_the_slot_speed_along_the_surface`) drives the traced
17.9° grade and asserts the exact rule (|v| = 4.95 to 0.002) plus the traced
components (4.699 horizontal, 1.523 vertical, each within the trace's own
~0.2% sampling precision). Nothing in the harness sandbox exercises it yet
(the ground there is flat, so the path is dormant and the flat traces are
unchanged), but the rule is no longer unmodelled.

## The jump and gravity are now verified against the client's trace

Until this round the vertical model (impulse 5.33 m/s, gravity 17.37 m/s²) was
verified only by unit tests against the traced arc. The differ now verifies it
against the trace itself, on `player_keyed.jsonl` (the session that contains
the standing jump at i=852):

```
no divergence across 18 compared write(s): ... max vertical delta error 0.0014184 m
```

What the differ had to learn for that, and why each piece is legitimate:

* **Yaw inference.** That trace carries no yaw field (it parses as yaw 0
  everywhere), so the yaw is recovered from the first free-movement step:
  `yaw = atan2(dx, dz)` under the client's own convention (the step direction
  is `(sin yaw, cos yaw)`). The convention is measured on the flat trace's
  straight plateaus against its recorded yaw to **0.006°** (w: −170.4387
  against 189.5668; d: −90.0018; a: +89.9979; s: 180 ± 0.29), and the keyed
  session's walk direction is pinned to ≈189.5° by its stop-glide residuals
  (i=263: −170.25; i=1056: −170.45), matching the sibling's constant 189.5668.
  A first step can read up to ~0.5° off the facing when a previous velocity
  blends into it, and one inferred yaw is only as good as the trace's
  straightness — both bounds are written down rather than hidden.
* **A per-segment vertical baseline.** A trace crosses several ground heights
  (0.711 at the start, −1.673 where the jump happens, −2.289 at the end), so
  each compared segment starts aligned to the client's height and gets its own
  ground; a jump keeps its take-off ground through the descent after the key
  release, so the arc stays in scope.
* **Terrain screens for the vertical, two shapes.** A *step*: an interior
  frame moving vertically more than 1 cm (the display transform's own lerp
  wobble stays under 6 mm). A *sustained* one-signed vertical (>75% of a
  segment's frames): a ramp, a ledge or a slide; the threshold is 75% and not
  60% because a jump's key release splits its arc across segments and a
  9-up/6-down half reads as 64%. A *net* height change alone is **not** terrain
  — a flat segment that merely begins on a different level than the previous
  segment's end is comparable, because the model is aligned to the client's
  height at each segment head (the earlier net-height screen over-excluded
  whole segments and was removed).
* **Write gaps are crossed, not skipped.** The client integrates every frame,
  but a standing character writes nothing, so the collapsed write list has
  gaps. The replay advances the missing frames with the held input (no input,
  so the velocity decays at `FRICTION` exactly as the client's did). A jump
  from rest after a long idle left the ground at 0.81 m/s before this — the
  real captures masked it behind the input-transition skip, and a synthetic
  crouch/air fixture (`dev/synth_ctrl_trace.py`) exposed it.
* **The jump's input timing is *not* the ramp's, and it is a rising edge.**
  The movement ramp carries a measured one-write delay, but the traced jump's
  *first* space write already carries the rise, so the impulse uses the current
  write's keys; and the gate feeds it as a rising edge (`space` and not the
  previous write's `space`), matching the sim's `Input` contract — a held space
  must not re-jump the model on landing, which the client does not do. No trace
  distinguishes held from edge (every traced jump releases space before
  landing), so the edge reading is the contract's, not a measurement of the
  client's own edge handling (still unread, Q3).
* **The arc is compared by its *deltas*.** The traced take-off is 1–2 frames
  earlier than the first write that shows it (the key lands inside a frame,
  which no sampling sees), so the absolute dy of a jump is one write out of
  phase. The *deltas* of dy are phase-invariant and are exactly `g·dt²`; they
  match the client to **1.4 mm** across the whole arc, against a 6 mm band.
  A landing is a snap clamp on both sides and is skipped, and the grounded tail
  is out of scope because of the phase.

The flat trace's regression (`player_fast3_keys.jsonl`) still shows no
divergence — 101 compared writes, max step deviation 0.0018616 m — and the
differ now prints a per-segment coverage table (writes / compared / env /
skipped / settling / jitter) instead of only a total, so the screened share is
visible rather than implied. That table is what drove the screen refinements
above: it showed whole walk/strafe/diagonal segments excluded by the net-height
screen (now gone), then surfaced the display node's ~6 mm vertical wobble (now
a settling skip) and a real ~3 cm step-down in the client's y during the
backpedal (terrain — screened by the step rule, not compared).

## The crouch, the reversal and air control, measured (2026-10-15)

The three behaviours the earlier keyed traces could not see — crouch timing,
air control and the reverse — are now measured from dedicated captures, and
the mover implements them (`AIR_ACCEL`, the crouch slot, the opposing-friction
rule in `ow-sim::movement`).

**Captures** (`D:/ow-dumps/trace/`, all taken by the agent's injected schedule,
read-only sampling):

| file | setting | client fps | sampling | why |
|---|---|---|---|---|
| `player_ctrl.jsonl` | practice range | 300 (capped) | 60 Hz | first crouch/air run; the character pinned on geometry in its later cycles |
| `player_ctrl2.jsonl` | custom map (open) | 30 | 60 Hz | first open run, same schedule |
| `player_ctrl3.jsonl` | custom map | 30 | 200 Hz | the client's frame time resolved — write intervals read 30/35 ms (a 33.3 ms cadence quantised by the sampler) |
| `player_ctrl4.jsonl` | custom map | 30 | 200 Hz | the schedule now turns ~180° each cycle and walks back, so the drift that pinned the character stops |

**Finding the node was the hard part, and the old signature is why.** The
documented layout scan (`dev/find_node.py`) matched only static data in these
sessions: 26-28 candidates, every one at a zero-ish position whose +0x00 was a
small integer, not a pointer — measured: none of them moved when the player
walked, and none reacted to injected mouse motion (so neither test "proved"
anything about injection while they were the only candidates). The live node
was found by behaviour instead (`dev/hunt_node.py`): loose layout match (a
map-scale position, a non-zero yaw, a unit quaternion, a uniform scale) →
walk (a candidate must advance 3-40 m at a walking pace) → stop → **cluster**:
the player's transform is copied into many arenas and every copy walks the
*same* distance, which no coincidence produces (10 copies at
`0x0216b0d48…` in the custom map, all advancing 14.098 m in 2.5 s). The
`find_node.py` matcher itself was also ~3x slow (gather indexing where a
contiguous slice is equal); the fast version returns identical hits and
`--cands-out`/`--from` let the walk confirm be re-run without a rescan.

**Three record layouts now exist in the docs' sense**, all reachable:

* 2026-10-13 (the keyed traces): feet at +0xd0, yaw in degrees at +0xdc, unit
  quaternion at +0xe0, scale at +0xf0, repeats at +0x108/+0x138/+0x178;
* the practice-range session: a 0x30-byte record (position + yaw + quaternion +
  scale) repeated at +0x30/+0x70, with *two* variants of each copy — one holds
  the position and one drops 0.465 m when crouched;
* the custom-map session: **4x4 transform matrices** — rotation rows at
  base+0x80/+0x90/+0xA0, the translation row at base+0xD0 (the traced feet
  triple), the *capsule* copy at base+0x110 (same x/z, +1.39 m in y). Its third
  rotation row is the object's +Z axis in world space, and
  **`yaw = atan2(rz.x, rz.z)` matched the walk direction to 0.02-0.27° on three
  facings** (measured) — which is exactly the differ's convention, so the
  recorder derives the yaw from that row (`live_trace.py node --yaw-row 0xc0`,
  which also refuses an address whose row is not a unit vector).

**Which copy is the feet** is decided by crouch: the stance-following copies
drop 0.427-0.465 m, the feet copy is bit-stable (`dev/feet_probe.py`). The
recorder wants the feet copy: its vertical is the body's, not the stance's.

**Mouse injection does turn the view.** 3000 px of relative `mouse_event`
motion rotated the walking direction by **-143.5°**, measured against a known
real node (recorded here because an earlier note in this project — and in
`dev/hunt_node.py` — claimed the opposite; that test watched only the template
junk above, which cannot show a yaw change). Consequence for a capture: the
*facing* can change mid-run (the operator also moved the camera), which is why
the custom-map captures record a per-sample yaw instead of relying on the
differ's single inferred value.

**Measured** (all at the client's own write cadence; the client's frame time is
derived per segment by the differ and per run by `dev/air_probe.py`):

| quantity | measurement | value |
|---|---|---|
| crouch walk | plateau steps 0.04956 m/16.7 ms and 0.0967 m/32.2 ms; five clean segments read 2.960 / 2.970 / 2.973 / 2.998 / 3.011 m/s | **3.0 m/s**, the `0DDD18C3` slot |
| walk / backpedal in the same captures | 5.507-5.512 m/s and 4.953-4.955 m/s | 5.5 / 4.95 (slots confirmed a second time) |
| crouch transition | the slot switch is the ordinary ACCEL-limited approach, 1-2 frames: 5.5 → 3.46 → 2.97 m/s (`player_ctrl.jsonl` i=10147) and 2.97 → 4.16 → 5.54 m/s (i=8023) | no clamp, no reset, no overshoot below the slot |
| airborne, no input | the step holds its exact value through the fall | preserved |
| airborne with a direction held | per-write step increase +0.00535 m at 16.7 ms and +0.0199 m at 33.0 ms (= dt² scaling); five clean runs give 17.1-19.2 m/s², and the speed reaches the 5.5 slot in ~0.3 s from rest | **18.5 m/s²** = `AIR_ACCEL` |
| reverse (walk → backpedal) | +5.5 → (one write) → -0.5..-4.0 → plateau; the rate reads **210-231 m/s²** on the true reversals, where ACCEL alone is 121.4 and **ACCEL + FRICTION = 194** | friction applies *in addition* while the wish opposes the velocity |
| jump/gravity in the open map | max vertical delta error 0.0019 m over the whole 300 s capture | gravity and impulse hold |

**Gate state** (`trace_diff keys <file> --by-state`; the `--by-state` gate, not
the strict replay):

| capture | compared | client-jitter band | env-screened | first divergence |
|---|---|---|---|---|
| `player_ctrl4.jsonl` | 3696 | 3600 | 2124 | 0.0080 m (a reversal write, 4.5% of the step) |
| `player_ctrl3.jsonl` | 3137 | 3000 | 3403 | 0.0066 m (3.7%) |
| `player_ctrl2.jsonl` | 2641 | 2413 | 3894 | 0.0420 m (wall cycles) |

The vertical is the strongest column: `player_ctrl4` reports **max vertical
delta error 0.0019 m** (the 6 mm band) across the whole capture, jumps
included. The horizontal residuals sit at 2-5% of a step and are concentrated
at the reversal writes and at writes where the client's own frame time varied;
that floor is the *sampling* limit (a 200 Hz sampler resolving a 30 fps client's
write times to ±5 ms, ±15% on a single interval), not a model error — the
regression that would separate them is a capture with a lower, *constant* client
frame time.

Two screen refinements came out of these captures, both required by measured
behaviour rather than guesses:

* **the stall screen**: a segment whose steps are uniformly far below what the
  wish promised is walking into geometry. The size-change test cannot see it —
  a wall slide keeps every step small, so its ratio to its own median is 1 —
  and it lists a 77-write stall at a 0.004 m median step with only 2 steps
  above the settle band, so the test is computed from the interior steps, not
  from the `big` set. With it, `player_ctrl4` screens 2124 writes the older
  gate compared as model error;
* **the record's own write intervals** feed the model's dt per segment (already
  the case), which is why the 60 fps practice-range capture — where a 300 fps
  client is *sampled*, not resolved — still gates: the steps are exact, and the
  per-segment mean dt recovers the effective frame time.
