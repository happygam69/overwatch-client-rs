# Statescript: the runtime objects and the state registry (the executor is hash-less)

## Current state

- **Data side: read** - graph assets (type `0x01B`), node records and the
  typed damage fields.
- **Runtime: read** - object graph, variable layer, the executor walk and
  the per-class statics (reflection/metaobject only).
- **Open:** the node type index's source is unproven (first-occurrence /
  traversal order is the surviving rule); reading the impl entry needs an
  executor-heap capture.

**Verify:** `cargo run -p ow-tank --bin params -- --hero 2 --all-graphs`.


The statescript system's **data** side is already read (`ow-tank` walks graph
assets of type `0x01B`; `--bin params` extracts `STUStatescriptModifyHealth`
amount literals across 5,687 assets; `weapons.md` classifies every
`m_amount` field-hash site as reflection). This round adds the **runtime**
side, and refutes the obvious route to the executor. Coordinates are v2b
session; range = v2b − `0x17AEC0000`, exec = v2b − `0x71820000`.

## The class hash is not dispatched anywhere (measured)

`STUStatescriptModifyHealth` is `[STU(0xA8BC2948, 416)]` in the reference
class file. Scanning **both** captures for that immediate:

* module image (`.text`): **0** occurrences of `0xA8BC2948`;
* all 1,706 `dumps_exec` regions: **0** occurrences;
* the field hash `0xAF872E86`: 0 in exec; 9 in the module image, all already
  classified as registration/predicate/default sites.

So the runtime does **not** switch on class hashes, and the field-hash thread
leads to reflection only. A graph's nodes must carry *resolved* descriptions at
runtime (the class descriptor's field table gives hash → offset once, at load),
and execution switches on something that is not either hash. That closes the
"pull the thread from the hash immediates" route for the executor — it was the
right hypothesis for the reflection layer and is measurably wrong for the
executor.

## The statescript variable pool (read whole)

`FUN_7ff7c6eda1c3`-family (v2b `0x7ff7c6eda1c0..0x7ff7c6eda326`) constructs the
runtime's **variable pool** and installs it into its owner:

* it lays out a **16-entry table of 0x20-byte nodes** from `owner+0x20` to
  `owner+0x220`, each node initialised `{self-pointer at −0xc, 0 at −4,
  0x80000001 at 0}` — an intrusive free list with an "empty" marker;
* allocates the pool object (**0x68 bytes**, `alloc(0x68,0)`), zero-filling
  fields `+0x00`, `+0x08`, `+0x10`, `+0x18`, `+0x20`, `+0x28` (word), `+0x2a`
  (byte), `+0x30`, `+0x38`, and making `+0x40`/`+0x48` self-referential list
  heads;
* decodes the name record at `0x7ff7c88ebe30` in place (the decode-once idiom)
  and stores the name **`"Statescript Var Pool"`** at `+0x08`;
* sets `+0x18 = 0x10000` (a flags word) and `+0x28 = 2` (a kind);
* attaches a debug view: the shared helper `0x7ff7c6881c90` (the 629-caller
  stub) plus `0x7ff7c71185f0(obj, 0xb8, 1)` → stored at `+0x38`;
* writes the pool pointer into the owner at `+0x10`, zeros `owner+0x18` and
  `owner+0x230`.

The owner is a `0x230+`-byte runtime object; the adjacent function
(`0x7ff7c6eda330`) builds a second 16×0x20 table of the same shape, so this
band is a series of runtime-collection constructors.

## The state registry band

The state-class *names* are live strings, and their single reference sites land
in a registry/runtime band (`0x7ff7c570xxxx`):

| name (v2b string) | site (v2b / exec) |
|---|---|
| `Statescript State Set Wind` `0x7ff7c86af564` | `0x7ff7c570c7b9` / `0x7ff753eec7b9` |
| `Statescript State Physical Button Handler` `0x7ff7c86af58c` | `0x7ff7c571207a` / `0x7ff753ef207a` |
| `ParallelStatescriptDataflowUpdate` `0x7ff7c86acc6c` | `0x7ff7c56b4719` / `0x7ff753e94719` |
| `Statescript Var Pool` `0x7ff7c88ebe3c` | `0x7ff7c6eda2af` / `0x7ff7556ba2af` |

The read site (`0x7ff7c570c740..0x7ff7c570c82f`) shows what the band does: it
builds a local lookup object (`0x7ff7c6e564e0`, `0x7ff7c6e566d0`), decodes the
state's name on first use, calls a **name-keyed lookup/create**
(`0x7ff7c6e56130(name, …)`), and then manages the state object with a
**refcount** (`XADD.LOCK [obj+0x8], -1`, release on reaching zero through
`0x7ff7c6e562c0` and `free(…, 0x10)`) and an intrusive list at `owner+0x70`.
That is the registry that owns state instances — again runtime code, not
reflection.

Other statescript strings found, for future anchoring: `"StatescriptEntity"`
(`0x7ff7c87f50b4`, no code ref), `"Unknown Statescript Event"`
(`0x7ff7c804bc08`, no ref — message catalog), and
`anim.enableLogStatescriptAnim` (the animation side's log switch).

## The runtime instance, the holder, and the variable layer (second pass)

Following the pool's call sites gives the runtime object graph:

* **`FUN_7ff7c6eda1b0`** is the variable-pool constructor seen above; its only
  caller is `FUN_7ff7c5771740` (v2b `0x7ff7c5771740`, exec
  `0x7ff753f51740`), which is the **runtime instance's initializer**. It
  `alloc(0x240,0)`s the holder, constructs the pool into it, caches it at
  **`instance+0x15a8`**, then:
  * calls `FUN_7ff7c6edc380(holder, instance+0x4a0)` — the **apply pass**: it
    walks a **16-record table of stride 0x20** (`+0x20..0x220`, each record a
    base pointer plus a count at `+8`, elements at stride 0x10) and, per
    element, resolves a slot by id (`FUN_7ff7c6edea10(runtime, elem_id)`),
    copies the sub-array at `+0x40`/`+0x48`, sets flag bit `| 0x20` at `+0xb0`
    and copies the `+0x50` payload — the **variable-slot bind/apply layer**;
  * iterates the instance's **element array** (`[+0x30]` pointers, count at
    `[+0x38]`) calling `FUN_7ff7c57e6760` per element.
* `FUN_7ff7c57e6760` is the element's **rebind/lifecycle** helper: it frees the
  element's old holder (`[elem+0x418]`, 0x240 bytes) through
  `FUN_7ff7c6eda530`, then calls **vtable slot `+0x70`** on the element to get
  the instance back and re-allocates a 0x240 holder when the instance's
  init flag (`+0x1394`) is set. So the script *elements* are vtable objects
  (slot 0x70 = "my instance") and the instance is shared.

So the object graph is: **instance (≥0x15B0 bytes)** — flags/guard at
`+0x1394`, element array `+0x30`/`+0x38`, a subobject at `+0x4a0` carrying the
16-record variable table, holder cached at `+0x15a8` — **holder (0x240)** — pool
at `+0x10`, 16 × 0x20 variable slots at `+0x20..0x220` (count `0x10` at
`+0x220`, marker `0x80000001`) — **pool (0x68)**. The `0x7ff7c6edxxxx` band is
the runtime's translation unit: the constructor family (`0x…a1b0`, `0x…a330`,
`0x…a3a0`, `0x…a410`, `0x…a470`, `0x…a480`), the holder teardown
(`0x…a530`), the apply pass (`0x…c380`), and the slot resolver (`0x…ea10`).

## The walk hunt, third pass: lazy creation, the state interface, and the dataflow component

Three concrete results, and the walk is still not isolated — but the search
space is now measured rather than guessed.

**The script runtime is created lazily from a wrapper.**
`FUN_7ff7c57679d0(param_1, param_2)` (v2b `0x7ff7c57679d0`) queries an
interface on `param_2` (`vtable+8` against descriptor
`0x7ff7c89c4b30`); when the query is absent and a flag at
**`[param_1+0x724]`** is not yet set, it sets the flag and calls the runtime
initializer on **`*(param_1+0x40)`** — so `param_1` is the script-holding
wrapper, `+0x40` is the script instance, and creation happens on first use.
The function has **zero static callers** (it is reached through the
interface), and the runtime initializer it calls
(`FUN_7ff7c5771740`) has exactly **one** call site — this one.

**The state interface has 302 users.** RIP-relative references to descriptor
`0x7ff7c89c4b30` number **302**, spread across
`0x7ff7c529…0x7ff7c5b…` — the state implementations' translation units. So
the *states themselves are in the capture*; the walk is one of those users
(or their caller). This is why the class-hash route was wrong: the runtime
never needs the hash when the state objects are already typed by their
vtables.

**The dataflow component is present, with its own vocabulary**: `"DataFlow"`
(`0x7ff7c87f4cfc`), `"DataFlow Buffer"` (`0x7ff7c887f864`),
`"SingleThreadedDataFlowUpdate"` (`0x7ff7c88ea38c`) and
`"ParallelStatescriptDataflowUpdate"` (`0x7ff7c86acc6c`) — the first three
unreferenced (marker/catalog names), the parallel one referenced once at
`0x7ff7c56b4719` where it is handed to the named-lookup
(`edx=8` size, `r8=name`). The **query failure path** is captured at v2b
`0x7ff7c6a53d7e` (`"Unable to find queried dataflow 0x%08x or dataflow
component is missing."`, log level `0x2b`, return code `6`) inside an
*obfuscated* function that hashes the queried dataflow id
(`v = ([0x7ff7c89c3558] ^ …) ^ 0x1e1bd7260a5582dc`) before the test — the same
obfuscation family as the map scene's secure struct.

What remains, with the anchors now measured: the per-frame walk is **not** one
of the 302 interface users directly (they are the state bodies); it is the
caller that iterates the instance's element array (`+0x30`/`+0x38`) and
dispatches per state — most cheaply reached from the wrapper (`+0x724` flag,
instance at `+0x40`) once its class is named, or from the element vtable's
state slots beside the known `+0x70` "get instance" slot.

## The walk hunt, fourth pass: what the wrapper is not

The naming route through the class registry closed with a negative worth
recording: the wrapper (flag `+0x724`, instance at `+0x40`) and the instance
(`≥0x15B0`, `+0x1394`/`+0x15A8`) are **engine classes with no STU class
record**. The only class descriptors near the wrapper's implied size (`≥0x725`)
are `STU_5E731805` (0x750) and `STU_CCFE5483` (0x748), and both are **volume
descriptors**, not script runtimes — their fields are
`m_position`/`m_rotation`/`m_direction`/`m_radius`/`m_falloff` on the
`STU_820A411A` (0x678, 13 subclasses) → `STU_11E07FD7` (0x500) →
`STU_E9DE9DF8 : STUGraphNode` chain. No descriptor in the artifact has an
instance-scale size (0x15xx–0x19xx) at all. The wrapper's vtable slot is not
stored in the capture in plaintext *or* in the documented pointer-encoding
idiom (both tested), consistent with an engine object built through the
service locator rather than an STU type.

Confirmed sites from this pass, for the next read:

* the wrapper's own stores: `mov word [rdi+0x724]` at `0x7ff7c5766d48` and
  `mov byte [rdi+0x724],1; mov ecx,[rdi+0x40]` at `0x7ff7c5767a09` — band
  `0x7ff7c5766xxx–0x7ff7c5767axx` is the wrapper's code;
* the holder cache (`+0x15A8`) writers/readers: `mov [rbx+0x15a8],rdi` at
  `0x7ff7c576f620` (band `0x7ff7c576fxxx`), and a nine-site cluster at
  `0x7ff7c54f8xxx–0x7ff7c54f9xxx` that repeatedly passes `lea rcx,[rdi+0x15a8]`
  into calls — plausibly the instance's teardown/rebuild path;
* the shape scan for the walk loop (an element-array `[+0x30]` load followed by
  a vtable call within 0x60 bytes) yields **204 sites** — too broad to
  disambiguate while the wrapper's identity is unknown.

Next, in order of expected yield: (1) the **name oracle** over the three bands
(wrapper `0x7ff7c5766xxx`, instance/holder `0x7ff7c576fxxx`, runtime
`0x7ff7c6edxxxx`) — the same XOR name-record decode that named other systems,
run as a search; (2) the **live route**: the instance's element array and
holder are plain memory, and the project's live tooling already traces
player-node objects, so the same before/after technique applies to the script
instance while a graph runs.

## The walk, fifth pass: the drive function is found

The walk is **`FUN_7ff7c5770230`** — slot **100 (`+0x320`)** of the script
host's vtable. The host is the object the bind path and the adopt helper
receive as `owner`; its class vtable sits at **`0x7ff7c7e37000`** (**174
slots**, every target inside the `0x7ff7c57xxxxx` band), and the host carries
the manager region at `+0xc70`, the **executor pointer at `+0xcb0`**
(`= manager+0x40`), the utility field `+0xbd8` (the tiny slot-116 method
`0x7ff7c5772050` reads it), and the flag bytes `+0x929`/`+0xc51`.

The drive, read as pseudocode:

* derives a list selector through the secure-pointer idiom out of the host
  field `+0xbd8`, mapping the key through the **bitset at `+0x110`** and the
  byte table at `+0x130` (popcount masks `0x5555…`/`0x3333…`) — the same
  bitset/popcount model the bind path uses to count instances;
* walks the **script-instance array — pointer at `owner+0x30`, count at
  `owner+0x38`** (the same array the initializer rebinds at attach), filtering
  each element on `element+0x3b0 & 0x80000`:
  * call **element vtable slot 43 (`+0x158`)**,
  * push a value with `0x7ff7c6ec9070(element, value, 1)`,
  * call **element vtable slot 44 (`+0x160`)**;

  the same 43/44 pair brackets the adopt helper's registration, so the pair is
  the element's *step* entry/exit and the push between them is the step's
  parameter — the dispatch over states that `weapons.md` was looking for;
* a second walk branch runs over a list at **`host+0x1d0`** (count `+0x1d8`)
  with the predicates `0x7ff7c6ec7830` / `0x7ff7c6eca190` / `0x7ff7c6ecabd0`;
* the pass counts iterations at `+0x68` and **stops at 20,000** — a runaway
  guard — then ends by calling the host's own slot `+0x260` with a bool derived
  from the type table (`0x7ff7c69ca5c0(x, 0x24)`).

This also fixes two earlier readings: on the array owner `+0x38` is the
element **count** and `+0x30` the **pointer** (init's loop walks them), and the
element's back-pointer (`element+0x38`, read by the 98-slot element vtable's
getter `0x7ff7c52a74d0` occupying slots 11–14) points at this same host.

**The wrapper route is closed as a walk route.** `FUN_7ff7c57679d0` is a
lazy-init gate: it queries `param_2`'s vtable slot 1 against the statescript
interface `0x7ff7c89c4b30` (the descriptor with 302 users = the state bodies),
and when the query is absent and the manager's `+0x724` is clear, sets the flag
and runs the initializer `FUN_7ff7c5771740` on `*(manager+0x40)`. All **eight**
manager methods reached through the host's `ADD RCX,0xc70; JMP` forwarding
thunks (host slots `+0x428`, `+0x488`, `+0x490`, `+0x498`, `+0x4a0`, `+0x4a8`,
`+0x4b0`, `+0x550`) were read: observer-list add/remove on the manager's three
containers (`+0xC8`/`+0xD0`, `+0xE0`/`+0xE8`, `+0xF0`/`+0xF8`), a pair setter
(`+0x6D0`), and conditional variants. No dispatch among them.

**The bind path** is the host's slot `+0x528` (`FUN_7ff7c5772080`): it decodes
the graph reference through the secure-pointer idiom (immediates inside
`0x7ff7c5772089..0x7ff7c57720d3`), looks the entry up (field `+0x448`), calls
`0x7ff7c68eee50(host+0x4a0, entry)`, runs the lazy gate, then counts the
entity's same-type instances by popcount over the `+0x110` bitset and logs
**"Added %d script instances to same entity (graph: %016llx)"** (name record
`0x7ff7c86b0b20`, decoded in place). The **adopt helper** `FUN_7ff7c5771ee8`
(host, element, u16, u32) prepares a 0x40-byte record (`0x7ff7c69c80d0`),
stores the id at `element+0x420`, calls element slot 2 (`+0x10`) with the u16,
queries host slot 33 (`+0x108`), ORs `0x400` into `element+0x3b0`, registers
through `0x7ff7c6ed1650`, stores `element+0x8c`, then runs the 43/44 pair.

One external anchor remains open: the manager flag `+0x724` has a **second
writer** at `0x7ff7c74e110c` (band `0x7ff7c74exxxx`) — the path that turns a
script runtime on from outside the band read so far.

## The step slots, sixth pass: the busy pair, the payload, and the registry join

Slots **43/44 (`+0x158`/`+0x160`) are a reentrancy counter pair on the
executor**, not state logic: slot 43 (`0x7ff7c57e77b0`) calls the element's
`+0x70` instance getter and **increments `executor+0xd20`**; slot 44
(`0x7ff7c57e76f0`) decrements it. The step payload is the third call the drive
makes between them, `FUN_7ff7c6ec9070(element, value, flag)`, which:

* saves `executor+0x430` and sets it to the element (the executor's "current
  element", restored on every exit path);
* selects one of the element's **two subobject slots — `+0x318` (flag 1) or
  `+0x300` (flag 0)** — and treats the pointer stored there as a member at
  **`+0x158` inside the first node** (node base = pointer − `0x158`);
* walks the **node chain** (each node's next link at `node+0x160`, same
  −`0x158` base convention), skipping nodes whose `node+0x1b4 & 1` is clear or
  whose context field `node+0x190` equals `executor+0x490`;
* tail-dispatches the first qualifying node through **`FUN_7ff7c6ed4dc0`**, a
  trampoline: `impl = (*(*(node+0x1a0)+0x290))[+0x60 or +0x68]` table entry,
  indexed by the node's **type index at `node+0x1a8`** (the `+0x68 == 1` test
  selects the base table). This is the dispatch the class-hash refutation
  predicted: **not a hash — a per-graph type-index table**.

The element's `+0x310`/`+0x328` counts and the executor guard `+0x914 == -1`
gate the deferred path that re-enters the manager through
`FUN_7ff7c5768df0(manager, …)`; the executor's slot 4 (`+0x20`) is its
forwarding method.

**The class registry does contain the statescript state classes** — keyed by
their runtime hash, which **equals the STU hash**. `STUStatescriptStateModifyHealth`
is registry row `826613b7`: size `0x128`, 15 fields, parent
`0x7ff64df54b90`, field array at v2b `0x7ff7c8eb5500`. The field-descriptor
format is **7 qwords — `{class, offset, type, flags, 0, 0, field_hash}`** —
and the offsets are the **STU offsets verbatim**, which pins the state's
runtime layout: `m_95F1FB0B` at `0xC0`, **`m_modifyHealth` at `0xC8`**, nine
config vars `0xD0..0x108`, the output plugs `0x5DA62D02`/`0xA0B58111` at
`0x110`/`0x118`. The inheritance chain is in `class_inheritance.txt`
(`STUStatescriptBase` `0xC2DC60F6`/`0x58`, `STUGraphNode` `0x25C4F930`/
`0x10`). The 416-byte struct `0xA8BC2948` is **absent** from the registry — it
is embedded data, not a runtime class — so `m_amount` (`0xAF872E86`, offset
**72**, typed `STUConfigVar` in TankLib) is read through the state's
`m_modifyHealth` object at `+0x48`.

The value path is therefore: `state+0xC8` → data `+0x48` (`STUConfigVar`) →
the config-var evaluator (row 22's consumer path) → health. The function that
applies it is one of the compiled node implementations behind the
`node+0x1a8` table.

**The per-class statics are the reflection accessors** — measured on this
class: its `static_58..90` targets are field-hash compare dispatchers
(`cmp ecx, 0x48BED081 / 0x79758CE9 / 0xFF9E82FA`; `cmp edx, 0x7B2B3F5C`) and
a vtable-installing constructor. That is exactly why every in-image use of a
field hash (the nine `m_amount` sites included) is reflection: those sites
*are* the generated per-class accessor set. Their code is cold in the v2b
capture but resident in the **range** capture (`dumps_range/
7ff64b687000_00006000_*.bin` covers three of them), and those region files are
PE-wrappable like any other (`dev/mkpe.py`).

## The impl hunt, seventh pass: the class region is metaobject, and the table needs a live graph

The compiled implementation for a state class is **not** in the per-class code
region, and the per-graph type table that names it **does not exist in the
standing captures** — both measured this pass.

**The per-class region is generated metaobject code.** For
`STUStatescriptStateModifyHealth` (`826613b7`), the class's own code band
(range `0x7ff64b687xxx..0x7ff64b68axxx`, resident in the range capture;
PE-wrapped as `dev/work/r_stateimpl.exe`) contains only:

* the **factory** `0x7ff64b689510` — `alloc(0x128)` (the class size), zeroes
  `+0x18/+0x28/+0x38/+0x70/+0x80`, stores `-1` at `+0x58` and installs the
  class vtable `0x7ff64d1a5da8` (range);
* the class **vtable** `0x7ff64d1a5da8` — per-class slots are more of the
  same generated helpers; the shared slots point into `0x7ff64a39xxxx` /
  `0x7ff64c08xxxx`;
* **uniform 84-byte accessors** — `FUN_7ff64b689100` and `FUN_7ff64b6895b0`
  both read `this+0xC8` (the `m_modifyHealth` field offset pinned in the sixth
  pass), i.e. the generated get/set pair;
* **field-hash compare dispatchers** — `0x7ff64b687ac0` (`cmp ecx,
  0x48BED081 / 0x79758CE9 / 0xFF9E82FA`), `0x7ff64b6884b0` (`cmp edx,
  0x7B2B3F5C / …`), `0x7ff64b689ca0`, `0x7ff64b68aac0`;
* a **delegate** `0x7ff64b68a3b0` calling `0x7ff64c095800(descriptor,
  x)`.

No function in the region applies health, evaluates a config var, or branches
into gameplay. So the class's *behaviour* is hand-written engine code selected
elsewhere — consistent with the type-index dispatch.

**The dispatch table is absent from the standing captures (measured).** The
live capture contains real script elements: scanning `dumps_v2b` for the
element vtable `0x7ff7c7e40360` finds instances (e.g. `0x219822941e0`)
sharing executor `0x21a352c47d0` with **107 elements** (`X+0x30` array,
`X+0x38` count), the pool holder live at `X+0x15a8`, `X+0x1394` inited, and
`X+0x490 = 0x108eef` (the value the node gate compares). But **every element's
chain slots are null** (`E+0x300 = E+0x318 = 0`): no node chain was attached
at capture time, so no `node+0x1a8` and no table at `[node+0x1a0]+0x290`
exist to read. A heap-wide scan for the context qword finds only 2 hits, none
a node. The impl entry therefore needs a capture taken **while a graph is
running with its node chain live** — a capture requirement of the same class
as rows 15/17/20, not a further static-reading step. The exact object to grab
then: `E+0x300`/`E+0x318` non-null, then `S = [G+0x290]`, `P = [S+0x60]`,
`mode = [S+0x68]`, `impl = [[P + (0 if mode==1 else 8)]][node+0x1a8]`.

**The `+0x724` flag is set at construction, from an argument.** The second
writer at `0x7ff7c74e110c` is the **tail of the manager's constructor** (v2b
`0x7ff7c74e10a0..0x7ff7c74e118b`; the prologue is cold in every standing
capture — checked in v2b, range and exec): it zeroes `+0x700`/`+0x708`, stores
`+0x710 = 0`, `+0x714 = 1`, `+0x718 = 1`, allocates a **0x80000-byte buffer and
stores it behind the pointer slot at `+0x728`** (OOM path via
`0x7ff7c74917f0`), then writes **`+0x724 = (arg != 0)`** and a previously
unseen companion byte **`+0x725 = (arg2 != 1)`**, stores another pointer at
`+0x8f8`, runs a bounds pass over the `+0x728` region (`0x7ff7c74d2640`), calls
`0x7ff7c74e1c40(manager, arg3, 0, 0)` and returns the manager. So the script
runtime is enabled **once, at construction, by the owner that builds the
manager** — the wrapper's gate flag is not a gameplay toggle, and the manager
carries a 512 KB scratch area and two construction-time flags rather than an
enable/disable API.

**A live-captured band was identified and its base verified.**
`live_dump/statescript_band.bin` (3 MB, live session) is the live capture of
v2b `0x7ff7c6310000..0x7ff7c6610000`: the solved base (`0x7ff70fb10000`,
live delta `v2b = live + 0xB6800000`, consistent with the live module base
`0x7ff70e520000`) makes **27 % of 32-byte windows byte-identical** to the v2b
slice where v2b is hot (matching the slice's 30 % hot fraction), while every
control shift — including ±0x1000 — scores **0 %**. The band is code: its
calls concentrate on `0x7ff7c744cxxx` and `0x7ff7c6f5cxxx`, and its data
references hit the `0x7ff7c8b4xxxx` descriptor arrays.

**Wrapping the band as a PE and decompiling it settles what it is** (this
pass; `dev/work/r_band.exe`, base v2b `0x7ff7c6310000`):

* its two hottest callees are **MSVC's `_Init_thread_footer` /
  `_Init_thread_header` pair** (`0x7ff7c744cef0`/`0x7ff7c744cf5c`, the
  thread-lazy-init protocol) and `FUN_7ff7c6f5c7a0`, a **type-descriptor
  initializer** that stores `&0x7ff7c7d03a00` (the shared type object) plus
  seven parameters into a record;
* its own `FUN_7ff7c63c4760` is an **id-keyed create-by-id service**: for
  `id == 0xA53100D5` it instantiates from the template `0x7ff7c8b4d060` and
  interface-queries `0x7ff7c8b4cfc0`, returning the object or null;
* the band contains **zero references to `node+0x1a8` or `graph+0x290`** — it
  is a C++ TU of lazily-initialized registrations and descriptor
  constructions, **not the type-index loader**. The earlier note that "the
  loader TU is the band identified above" is corrected.

**The type index is copied data.** The two `+0x1a8` sites that do exist — in
the bind/host band, which is wrapped as `r_walk.exe` — are the **node copy
constructor** `FUN_7ff7c57725b0` and the **copy-assignment**
`FUN_7ff7c5772b80`: both copy the `+0x1A0` (graph), `+0x1A8` (type index) and
`+0x1B4` (flag) triple verbatim as part of the node payload, and the larger
one does a COM-style refcounted release of its `+0x9C` interface
(`lock dec [obj+0xC]; if old == 1 { if (!vtbl[6](obj)) vtbl[0](obj, 5) }`)
before stealing the source's pointer. So a node never computes its index; it
**inherits it from the prototype it is copied from**. The remaining anchor for
the hash → index assignment is therefore the **prototype / type-table
builder** (the code that writes `+0x1a8` from a counter while instantiating
prototypes, or the static prototype array itself), not this band.

**Reusable artefacts from this pass**: `dev/find_script_instances.py` (vtable
scan + object chase over a capture), `dev/heap_ptr_runs.txt` (every run of
≥24 consecutive module code pointers in `dumps_v2b` — 2,382 arrays; the
vtable band appears mirrored module↔heap, unexplained and noted), and the
base-solver recipe (score candidate bases by mapping `E8` targets onto
function-start bytes of the same build, then verify by byte identity; a
wrong base collapses to 0 %).

## The node vector, and why nothing writes `+0x1a8` from a counter (eighth pass)

The node/state objects live in a **vector of 0x2a0-byte elements** (the node
struct is `0x2a0`, matching the copy constructors' extent). Three container
operations are read (v2b; all in the host band, in the `r_walk.exe` region):

* **Grow** `FUN_7ff7c577a6a0(owner, want)`: geometric growth of
  `{array @ +0x0, count @ +0x8, capacity|flag @ +0x0c}`, moving elements with
  the node **copy constructor** `FUN_7ff7c57725b0` and destroying the old ones
  with `FUN_7ff7c5772990`.
* **Remove** `FUN_7ff7c577325c(limit, key)`: scans the vector at `owner+0x8`
  (count `owner+0x10`) for the element whose **key pair at `+0x0`/`+0x4`**
  matches `key -> *(u32)(key+0x134)` with the byte at `+0x27a` clear, copies the
  found element to a stack temp (copy constructor), compacts by
  **copy-assignment** `FUN_7ff7c5772b80`, destroys the tail, decrements the
  count, and releases a refcounted companion object.
* The add-side branch sets the byte at `owner+0x51a` and registers through
  `FUN_7ff7c577a7f0` → `FUN_7ff7c6d7eee0` (the **bitset helper** used by the
  node gate), i.e. membership is bit-set-flagged, not a plain push.

**The `+0x1a8` census closes the counter hypothesis.** A module-wide scan for
`+0x1a8` stores finds 218 sites, of which almost all are stack frames
(`mov [rsp+0x1a8]`). The object stores on node-like headers
(`AND dword [obj+0x1b4], 0x80000000; mov [obj+0x1a8], reg` in the
`0x7ff7c52abxxx` / `0x7ff7c53dxxxx` / `0x7ff7c545xxxx` bands, and a pure clear
at `0x7ff7c5558e9d`) are **reset/adopt/clear** helpers on node-like classes —
none reads or increments a counter, and no site writes an immediate. Combined
with the copy constructors (which carry `+0x1A0`/`+0x1A8`/`+0x1B4` by value),
the conclusion is that **the type index is never computed at runtime at all**:
it enters the node as data, from the graph asset's deserializer or from a
static prototype. The next anchor is therefore the **graph deserializer** (or
a file-side check: whether ow-tank's graph reader sees a per-node u32 that can
serve as a type index — the asset stores each node's class hash, so the index
is plausibly the node type's first-occurrence position in the graph).

## Ninth pass: the file-side index-space check

**File-side check: the index space is the graph's node-type set, and the assignment is first-occurrence order** (`dev/mdlc_probe/examples/graphidx.rs`, run against the install; 5,687 graph assets of type `0x01B`):

* **No per-node index exists in the file format.** `ow-stu`'s instance records carry
  `{class hash, embedder field, embedder index, size, data offset}` and nothing
  else — so the runtime index is assigned by the deserializer, as the code-side
  census already required.
* **The node set per graph is small.** Filtering `STUStatescriptState*` /
  `STUStatescriptAction*` / `STUStatescriptEntry` / `STUGraphContainer`
  instances over the 5,185 graphs with ≥2 nodes gives a distinct-node-type
  count `D` spread over **2..26, mode ~3–19** — exactly the small per-graph
  index space the dispatch table's two-variant array pair (`S+0x60`, mode at
  `S+0x68`) is sized for.
* **Nodes are interleaved, not grouped by type**: only **18 %** of graphs have
  type-contiguous node sequences (a node's plugs/links/config vars follow it in
  the bag, then the next node). So "run order" is excluded as the index rule.
* **Hash order is excluded too**: in graph `0x0580000000000016` the
  first-occurrence order runs `0x1E50E1A8` (Entry), `0x594C2D80` (StateStack),
  `0x75001A77` (ActionSetVar), `0x76F0CF47` (BooleanSwitch), then
  `0x6540C278` (StateWait) — non-monotonic in the class hash.
* **First-occurrence (traversal) order survives** as the only rule consistent
  with all of the above: the deserializer walks the graph's nodes in bag order
  and assigns each node its class's first-encounter index, which is why the
  index can never appear as an immediate, why the table is per graph, and why
  the copy constructors carry it by value. **Consistent, not yet proven** — the
  proof needs a live capture with a node chain (then `node+0x1a8` can be
  compared against the probe's first-occurrence index for that graph).

The probe also makes the file-side join usable: for any graph it lists the node
instances in order with resolved class names, so a graph that contains
`STUStatescriptStateWeaponVolley` (e.g. `0x0580000000000016` at node index 14 of
its first-occurrence order) or `STUStatescriptStateModifyHealth` (first
occurrence 18 there) can be checked against runtime indices as soon as one is
captured.

## Tenth pass: the graph materializer, and what it does not assign (this round)

The graph-asset deserializer the last pass sent us after is now located and
read. It is a **materializer**, and the load-bearing result is negative: it
never touches `node+0x1a8`. What it assigns, in record order, is a *key
vector*.

**The chain.** `FUN_7ff7c6f7e310` materializes one graph: it builds the
container, links each node record (`FUN_7ff7c6ff6ea0` applies the record's
optional fields onto the node and its target), then walks an array of
**0x10-byte records** and calls `FUN_7ff7c6ff6880(container, record)` for each
— in the order the records sit in the array, which is drained (count zeroed)
afterwards. A record is `{u32 key; u8 flag; u8 flag; u16 kind; u32;
void* target}`; `kind` 1 and 2 take different branches. After the records the
container is installed into the world through `FUN_7ff7c6feec30` plus four
world virtuals (`+0x3c8` query, `+0x3b8` set, `+0x3d8`/`+0x3e0` values stored
at `entity+0x14`).

**Per record** (`FUN_7ff7c6ff6880`): the record's key is looked up in a hash
map at `graph+0x1c8` (0xC-stride entries `{next@+4, key@+8}`, 8-byte values,
mask at `+0x18`; the hash is the obfuscated `((k*0x1001+0x7ed55d16) …)*9 +
0xfd7046c5` family already known from the field registry). The hit (or zero)
is the **node**; `FUN_7ff7c6ff6ac0` then rebinds it:

* `node+0xa8` is the **target**. If it already equals the record's target the
  call returns without touching anything; otherwise the old target's name
  entry is erased and the new one inserted, both through the target's first
  qword (the wrapper, below) as a string header;
* both the old and the new target get a **changed byte set at `wrapper+0x2a0`**
  — this is what the `+0x1a8`-census round saw as `node+0x2a0 = 1`; it is a
  rebind dirty mark, and the check that produced this section also showed the
  "0x2a0" the census had flagged in that band is the displacement of this very
  store, not an allocation size;
* the record's key is **appended to the u32 vector at `graph+0x1d0`** (grown
  through the module allocator `FUN_7ff7c698c710`), and only when the target
  actually changed.

So the runtime does keep a **first-encounter order** — of *keys*, in record
order. `node+0x1a8`, the dispatch index, is not written anywhere in this path,
and the module-wide census already found no counter writer: **the deserializer
does not perform the first-occurrence assignment either.** The index still
enters as data.

**What a key is.** `FUN_7ff7c6fef270(graph, key)` resolves a key as
`[graph+8][(key>>8)&0xff] + 0x80 + (key>>16)*8` — a two-level per-graph table
(section, index) — and takes the resolved object's **class id** (a u16), which
is then queried against the world (`FUN_7ff7c6f72190(world, class, 0x9e)`).
So a key is a **typed reference into the graph's own tables**, not a class
hash. Two readings fit the read code, and the low-byte test above separates
them: either the reference names the *target* (the component a node is bound
to — the rebind path also registers the target under two name hashes,
`node+0x120`/`node+0x124`, into maps that live on the graph's entity at
`entity+0x30`/`entity+0x60`, which is how named components are looked up), or
it names the *node itself* in the asset's node list, in which case the low
byte is the node's authored type index and `graph+0x1d0` is exactly the
first-occurrence order `graphidx` measures from the file.

**The wrapper, identified.** The "node" here is the 0x2a0-byte member object:
`+0xa8` target, `+0xb0` (kind-2 path), `+0x120`/`+0x124` name hashes, `+0x158`
self slot (this is why `E+0x300 = wrapper+0x158`, and the chain links are
`wrapper+0x160`), `+0x190` context, `+0x1a0` graph, `+0x1a8` dispatch index,
`+0x1b4` flags, `+0x2a0` changed byte. The target's first qword points back at
the wrapper.

**The dispatcher, instruction-verified.** `FUN_7ff7c6ed4dc0` is
`idx = (int)node[0x1a8]; S = [graph+0x290]; pair = [S+0x60]; entry =
([S+0x68] == 1 ? pair : pair+8)[idx]`, returning the qword entry. `S` is a
per-graph object with the pair at `+0x60` and the mode at `+0x68`; its builder
is not in this path.

**File-side tail census (supporting).** Every statescript state class in the
generated set declares the same tail `{0x1A0, 0x1A8, 0x1B0, 0x1B8}`, and a
handful of classes (e.g. `STUStatescriptStateCreateEntity`) additionally
declare a field at `0x1A4`. Under the measured "runtime slot = STU offset + 4"
rule those would land at `+0x1A4`/`+0x1AC`/`+0x1B4`/`+0x1BC`, while the runtime
wrapper's quad is `{+0x198, +0x1A0, +0x1A8, +0x1B0}` — no constant shift maps
one onto the other, so the quad is **wrapper metadata, not mirrored STU
fields**. The same rule is corroborated by the record applier, which writes the
target at `+0x2b0`/`+0x2b4`/`+0x2b9`/`+0x2ba` — offsets that only exist if the
runtime object is STU size + 4.

**Capture state (updated).** The warm live capture has real elements again, but
their executors live in a heap that capture did not include (`0x2423cc…` in
that session) and both chain slots are still null, and a shape scan of
`module_heap.bin` for the graph container (map `+0x1c8`, key vector `+0x1d0`,
table `S` at `+0x290`) found no candidate. The comparison the last pass
specified — a live `node+0x1a8` against `graphidx`'s first-occurrence index —
therefore still needs a capture that includes the executor's heap, or a
graph-attached element. That capture can also dump `graph+0x1d0`'s keys: if the
low byte of a key is the dispatch index, the key vector's low bytes must equal
the 2..26 index space, which would make the index **authored data** in the
file's references rather than an assignment at all.

## What this closes and what remains

Closed: the class-hash route (measured dead for the executor), the variable
pool's structure, the state-registry idiom, the **runtime object graph with its
variable layer** (pool, holder, 16-slot table, instance layout, resolver
`FUN_7ff7c6edea10`, apply pass `FUN_7ff7c6edc380`, element rebind
`FUN_7ff7c57e6760`), the **lazy creation path** (wrapper `+0x724` → executor
`+0x40`), the **per-frame walk** (drive `FUN_7ff7c5770230` at host slot
`+0x320`), the **step semantics** (slots 43/44 = executor busy pair; payload
dispatch by node type index via `FUN_7ff7c6ed4dc0`), the **class registry
join** (STU hash = registry key; field-descriptor format; the state's runtime
layout; `m_amount` path and type), the **nature of the per-class code**
(metaobject only: factory, vtable, uniform accessors, hash dispatchers), the
**capture state** (elements live, chains absent), and now the **graph
materializer** (record shape, key map and key-order vector, the rebind path
with its name registry and dirty byte, the wrapper's field map, and the
dispatcher's exact indexing). Remaining:

* the **implementation entry** behind `node+0x1a8` — requires a capture with
  a live node chain (spec above), and the tenth pass sharpened the spec: the
  warm session's elements exist but their executors sit in an uncaptured heap
  (`0x2423cc…` there), so the capture must include the executor's heap region
  or a graph-attached element. With it, one read yields ModifyHealth's
  applying function *and* the `graph+0x1d0` keys that settle the index source;
* the **graph-asset deserializer** — located and read this round (tenth pass):
  it is a materializer that binds each 0x10-byte record's target into
  `graph+0x1c8` and appends the record's key to `graph+0x1d0` in record order.
  It assigns **no** type index, so the first-occurrence rule the file-side
  check left standing is **not implemented here**; what the runtime keeps in
  first-encounter order is the *key* vector. The index source is now narrowed
  to **data**, with the testable candidate that it is the key's low byte (the
  capture that gives a live chain can read `graph+0x1d0` and check the low
  bytes against the 2..26 index space);
* the manager **constructor's prologue** (cold in all three captures; the tail
  at `0x7ff7c74e10a0` is read) — the owner that builds the manager and passes
  the flag arguments.

## Method

Class hash taken from the operator's OWLib checkout (`TankLib/STU/Types/
STUStatescriptModifyHealth.cs`, `[STU(0xA8BC2948, 416)]`); immediate scans over
the module image and all 1,706 `dumps_exec` regions; RIP-relative scans for the
runtime string anchors; disassembly windows for the pool constructor and the
registry site (both in code the analyses had not turned into functions); `E8`
call-site scans for the wrapper/factory/adopt helpers; a data-side qword scan
for the class vtables (element class `0x7ff7c7e40360`, host class
`0x7ff7c7e37000`), which recovers the slot numbers; and REA over a region PE
(`dev/mkpe.py`, `0x7ff7c5767000..0x7ff7c5775000`) for every function read in
the fifth pass. Sixth pass: `class_descriptors.txt` /
`class_inheritance.txt` are keyed by the same hashes as the STU classes
(`grep` the hash), so any STU class resolves to its registry row, size, field
array and per-class statics; field-order ground truth comes from the local
TankLib checkout (`E:\CASCReader\OWLib\TankLib\STU\Types\`); and cold-in-v2b
code is read by PE-wrapping the matching `dumps_range` region file.

Tenth pass: the flattened v2b module image is imported in the Ghidra MCP
project (`module_v2full.bin`, image base `0x7ff7c4d20000`, 68,865 functions),
so the materializer chain is read by decompile there (the `r_6ff6.exe` REA
region wrapper was built first and kept as the REA-side probe); the function
entry for a cold address is found by listing `E8` targets landing in the
region, and the dispatcher was verified instruction-by-instruction because its
Ghidra signature is wrongly marked non-returning. Two new tools:
`dev/live_chain.py` chases element → executor → chain → graph in the warm live
capture (module addresses translate by `-0xB6800000`), and
`dev/graph_scan.py` scans a heap image for the graph container's shape (map
`+0x1c8`, key vector `+0x1d0`, table `S` at `+0x290` with its pair and mode).
The states' declared tail fields come from a census of the generated TankLib
class files (fields at declared offsets `0x1A0..0x1B8`), and the `0x2a0`
"allocation" the earlier scan flagged was re-read as the displacement of the
`wrapper+0x2a0` dirty-byte store.
