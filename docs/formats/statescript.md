# Statescript: the runtime objects and the state registry

## Current state

- **Data side: read.** Graph assets (type `0x01B`), node records and the typed
  damage fields. `params` extracts `STUStatescriptModifyHealth` amount literals
  across 5,687 assets.
- **Runtime: read.** Object graph, variable layer, the executor walk and the
  per-class statics (reflection/metaobject only).
- **Execution: mapped, not decoded.** Dispatch is a per-graph type-index table,
  not a class hash. The implementation entry behind `node+0x1a8` needs a
  capture that includes the executor's heap.
- **Open:** the source of the node type index (first-occurrence/traversal order
  is the surviving rule; the key's low byte is the testable candidate).

**Verify:** `cargo run -p ow-tank --bin params -- --hero 2 --all-graphs`.

Addresses are v2b unless marked; `exec = v2b - 0x71820000`,
`range = v2b - 0x17AEC0000`.

## The system

A graph asset holds node records. At runtime each node is a **0x2a0-byte
wrapper** carrying a target, a graph, flags and a type index at `+0x1a8`.
Execution takes that index into a per-graph table and tail-dispatches the
result. The runtime does **not** switch on the STU class hash.

`STUStatescriptModifyHealth` is `[STU(0xA8BC2948, 416)]` in the reference class
file. Scanning both captures for that immediate:

* module image `.text`: **0** occurrences of `0xA8BC2948`;
* all 1,706 `dumps_exec` regions: **0** occurrences;
* the field hash `0xAF872E86`: 0 in exec; 9 in the module image, all already
  classified as registration/predicate/default sites.

So a graph's nodes carry *resolved* descriptions at runtime: the class
descriptor's field table gives hash -> offset once, at load. Execution switches
on something that is not either hash. This closes the "pull the thread from the
hash immediates" route.

## Data side

`ow-tank` walks graph assets of type `0x01B`; `--bin params` extracts
`STUStatescriptModifyHealth` amount literals and classifies every `m_amount`
field-hash site as reflection. `m_amount` is `0xAF872E86`, offset **72**, typed
`STUConfigVar` in TankLib.

Every statescript state class in the generated set declares the same tail
`{0x1A0, 0x1A8, 0x1B0, 0x1B8}`; a few classes (e.g.
`STUStatescriptStateCreateEntity`) add `0x1A4`. Under the measured runtime
slot = STU offset + 4 rule those would land at `+0x1A4`/`+0x1AC`/`+0x1B4`/
`+0x1BC`, while the runtime wrapper's quad is `{+0x198, +0x1A0, +0x1A8,
+0x1B0}`. No constant shift maps one onto the other, so the quad is wrapper
metadata, not mirrored STU fields. The record applier writes the target at
`+0x2b0`/`+0x2b4`/`+0x2b9`/`+0x2ba`, offsets that only exist if the runtime
object is STU size + 4.

## Runtime object graph

**Lazy creation.** `FUN_7ff7c57679d0` queries `param_2`'s vtable slot 1 against
the statescript interface descriptor `0x7ff7c89c4b30`. When the query is
absent and manager flag `+0x724` is clear, it sets the flag and calls the
runtime initializer on `*(manager+0x40)`. `param_1` is the script-holding
wrapper, `+0x40` is the script instance, and creation happens on first use. The
initializer `FUN_7ff7c5771740` has exactly one call site, this one.

The state interface has **302 users** in `0x7ff7c529...0x7ff7c5b...`; the state
objects are typed by their vtables, which is why the runtime never needs the
class hash.

**Instance and holder.** `FUN_7ff7c5771740` allocs a 0x240 holder, constructs
the variable pool into it, caches it at `instance+0x15a8`, runs the variable
apply pass `FUN_7ff7c6edc380`, then iterates the instance's element array
(`[+0x30]` pointers, count `[+0x38]`) calling `FUN_7ff7c57e6760`.

* instance (>=0x15b0 bytes): init flag `+0x1394`, element array
  `+0x30`/`+0x38`, variable subobject `+0x4a0`, holder `+0x15a8`;
* holder (0x240): pool at `+0x10`, 16 x 0x20 variable slots at `+0x20..0x220`
  (count 0x10 at `+0x220`, empty marker `0x80000001`);
* pool (0x68): name `"Statescript Var Pool"` at `+0x08`, flags `+0x18 =
  0x10000`, kind `+0x28 = 2`, debug view `+0x38`.

`FUN_7ff7c57e6760` is the element rebind/lifecycle helper: it frees the old
holder (`[elem+0x418]`, 0x240 bytes) through `FUN_7ff7c6eda530`, calls element
vtable slot `+0x70` to get the instance back, and allocates a new holder when
`instance+0x1394` is set. The script elements are vtable objects; the instance
is shared.

**Variable apply.** `FUN_7ff7c6edc380` walks the 16-record table (stride 0x20,
each record a base pointer plus count at +8, elements at stride 0x10). Per
element it resolves a slot by id (`FUN_7ff7c6edea10`), copies the sub-array at
`+0x40`/`+0x48`, sets flag bit `| 0x20` at `+0xb0`, and copies the `+0x50`
payload.

**State registry.** The state-class names are live strings whose single
reference sites land in `0x7ff7c570xxxx`:

| name (v2b string) | site |
|---|---|
| `Statescript State Set Wind` `0x7ff7c86af564` | `0x7ff7c570c7b9` |
| `Statescript State Physical Button Handler` `0x7ff7c86af58c` | `0x7ff7c571207a` |
| `ParallelStatescriptDataflowUpdate` `0x7ff7c86acc6c` | `0x7ff7c56b4719` |
| `Statescript Var Pool` `0x7ff7c88ebe3c` | `0x7ff7c6eda2af` |

The read site `0x7ff7c570c740..0x7ff7c570c82f` builds a local lookup object,
decodes the state's name on first use, calls a name-keyed lookup/create
(`0x7ff7c6e56130`), and manages the state object with a refcount
(`XADD.LOCK [obj+0x8], -1`, release through `0x7ff7c6e562c0`, `free(...,0x10)`)
and an intrusive list at `owner+0x70`.

## Execution

**The drive** is `FUN_7ff7c5770230`, slot 100 (`+0x320`) of the script host's
vtable. The host vtable is `0x7ff7c7e37000` (174 slots, all targets in
`0x7ff7c57xxxxx`); the host carries the manager at `+0xc70`, the executor
pointer at `+0xcb0` (= manager+0x40), the list-selector field `+0xbd8`, and
flags `+0x929`/`+0xc51`.

The drive:

* derives a list selector through the secure-pointer idiom from `+0xbd8`,
  mapping the key through the bitset at `+0x110` and the byte table at `+0x130`
  (popcount masks `0x5555...`/`0x3333...`);
* walks the script-instance array (`owner+0x30` pointer, `owner+0x38` count),
  filtering each element on `element+0x3b0 & 0x80000`;
* calls element slot 43 (`+0x158`), pushes a value with
  `FUN_7ff7c6ec9070(element, value, 1)`, then calls slot 44 (`+0x160`);
* runs a second walk branch over `host+0x1d0`/`+0x1d8`;
* counts iterations at `+0x68`, stops at **20,000**, then calls host slot
  `+0x260`.

Slots 43/44 are a reentrancy counter pair on the executor, not state logic:
slot 43 (`FUN_7ff7c57e77b0`) increments `executor+0xd20`; slot 44
(`FUN_7ff7c57e76f0`) decrements it.

**The step payload** is `FUN_7ff7c6ec9070`. It saves `executor+0x430` and sets
it to the element; selects element subobject `+0x318` (flag 1) or `+0x300`
(flag 0); treats the pointer there as a member at `+0x158` inside the first
node (node base = pointer - 0x158); walks the node chain (`node+0x160` next
link, same base convention), skipping nodes whose `+0x1b4 & 1` is clear or
whose `+0x190` equals `executor+0x490`; then tail-dispatches the first
qualifying node through `FUN_7ff7c6ed4dc0`.

**The dispatcher**, instruction-verified:

```text
idx   = (int)node[0x1a8]
S     = [graph+0x290]
pair  = [S+0x60]
entry = ([S+0x68] == 1 ? pair : pair + 8)[idx]
```

`S` is a per-graph object with the pair at `+0x60` and the mode at `+0x68`; its
builder is not in this path. The result is a qword entry.

**The bind path** is host slot `+0x528` (`FUN_7ff7c5772080`): decode the graph
reference, look the entry up (field `+0x448`), call
`0x7ff7c68eee50(host+0x4a0, entry)`, run the lazy gate, then count same-type
instances by popcount over the `+0x110` bitset. The adopt helper
`FUN_7ff7c5771ee8(host, element, u16, u32)` prepares a 0x40-byte record, stores
the id at `element+0x420`, calls element slot 2, queries host slot 33, ORs
`0x400` into `element+0x3b0`, registers through `0x7ff7c6ed1650`, and runs the
43/44 pair.

## The node wrapper and its index

The "node" is the 0x2a0-byte member object:

| offset | field |
|---|---|
| `+0xa8` | target |
| `+0xb0` | kind-2 path |
| `+0x120`/`+0x124` | name hashes |
| `+0x158` | self slot |
| `+0x160` | next chain link |
| `+0x190` | context |
| `+0x1a0` | graph |
| `+0x1a8` | dispatch index |
| `+0x1b4` | flags |
| `+0x2a0` | changed byte |

The target's first qword points back at the wrapper. `E+0x300 =
wrapper+0x158`, so `wrapper+0x160` is the chain link.

**The graph materializer** is `FUN_7ff7c6f7e310`. It builds the container,
links each node record (`FUN_7ff7c6ff6ea0` applies optional fields to the node
and target), then walks an array of **0x10-byte records** and calls
`FUN_7ff7c6ff6880(container, record)` in array order; the array is drained
(count zeroed) afterwards. A record is
`{u32 key; u8 flag; u8 flag; u16 kind; u32; void* target}`; kind 1 and 2 take
different branches. Afterwards the container is installed into the world
through `FUN_7ff7c6feec30` plus four world virtuals.

Per record, `FUN_7ff7c6ff6880` looks the key up in the hash map at
`graph+0x1c8` (0xC-stride entries `{next@+4, key@+8}`, 8-byte values, mask at
`+0x18`; the hash is the obfuscated `((k*0x1001+0x7ed55d16)...)*9 +
0xfd7046c5` family already known from the field registry). The hit (or zero) is
the node; `FUN_7ff7c6ff6ac0` rebinds it:

* `node+0xa8` is the target. If it already equals the record's target the call
  returns; otherwise the old target's name entry is erased and the new one
  inserted, through the target's first qword as a string header;
* both old and new target get the changed byte at `wrapper+0x2a0` (a rebind
  dirty mark);
* the record's key is appended to the u32 vector at `graph+0x1d0`, grown
  through `FUN_7ff7c698c710`, and only when the target actually changed.

So the runtime keeps a first-encounter order of *keys*, in record order.
`node+0x1a8` is not written anywhere in this path, and the module-wide census
found no counter writer: the deserializer does not perform the first-occurrence
assignment either. The index still enters as data.

A key is a **typed reference into the graph's own tables**, not a class hash:
`FUN_7ff7c6fef270(graph, key)` resolves
`[graph+8][(key>>8)&0xff] + 0x80 + (key>>16)*8`, takes the resolved object's
class id (u16), and queries it against the world
(`FUN_7ff7c6f72190(world, class, 0x9e)`). Two readings fit the read code:
either the reference names the target (the component a node is bound to), or it
names the node itself in the asset's node list, in which case the low byte is
the authored type index and `graph+0x1d0` is the first-occurrence order. The
low-byte test separates them.

## Class registry join

The registry contains the statescript state classes, keyed by their runtime
hash, which **equals the STU hash**. `STUStatescriptStateModifyHealth` is row
`826613b7`: size `0x128`, 15 fields, parent `0x7ff64df54b90`, field array at
v2b `0x7ff7c8eb5500`.

The field-descriptor format is **7 qwords**:
`{class, offset, type, flags, 0, 0, field_hash}`. The offsets are the STU
offsets verbatim, which pins the state's runtime layout:

* `m_95F1FB0B` at `0xC0`;
* **`m_modifyHealth` at `0xC8`**;
* nine config vars at `0xD0..0x108`;
* output plugs `0x5DA62D02`/`0xA0B58111` at `0x110`/`0x118`.

The inheritance chain is in `class_inheritance.txt` (`STUStatescriptBase`
`0xC2DC60F6`/`0x58`, `STUGraphNode` `0x25C4F930`/`0x10`). The 416-byte struct
`0xA8BC2948` is absent from the registry: it is embedded data, not a runtime
class. `m_amount` is read through the state's `m_modifyHealth` object at
`+0x48`. The value path is `state+0xC8` -> data `+0x48` (`STUConfigVar`) ->
the config-var evaluator -> health. The applying function is one of the
compiled node implementations behind `node+0x1a8`.

The per-class statics are the reflection accessors: field-hash compare
dispatchers and a vtable-installing constructor. Every in-image use of a field
hash, including the nine `m_amount` sites, is reflection. The `m_amount` path
and type are otherwise as the file side says.

## Ruled out

* **Class hash dispatch.** Zero occurrences of `0xA8BC2948` in the module and
  all 1,706 exec regions; the field hash leads only to reflection.
* **The wrapper as an STU class.** The wrapper (`+0x724`, instance `+0x40`)
  and instance (`>=0x15B0`, `+0x1394`/`+0x15A8`) are engine classes with no STU
  class record. The only descriptors near the wrapper's implied size are volume
  descriptors, not script runtimes.
* **The per-class code region as the implementation.** It is metaobject only:
  factory, vtable, uniform accessors, hash dispatchers.
* **The per-graph type table in the standing captures.** The table does not
  exist there; the compilations do. A live graph-attached element is needed.
* **First-occurrence assignment in the deserializer.** The materializer keeps
  key order at `graph+0x1d0` but never writes `node+0x1a8`.

## Open

* **The implementation entry behind `node+0x1a8`.** Needs a capture that
  includes the executor's heap (`0x2423cc...` in the warm session) or a
  graph-attached element. One read then yields ModifyHealth's applying function
  and the `graph+0x1d0` keys.
* **The index source.** The materializer does not assign it; the surviving
  candidate is the key's low byte, testable against the 2..26 index space in a
  live `graph+0x1d0`.
* **The manager constructor's prologue.** Cold in all three captures; the tail
  at `0x7ff7c74e10a0` is read. The manager flag `+0x724` has a second writer at
  `0x7ff7c74e110c`.
