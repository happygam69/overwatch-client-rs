# Engine object

The object the client calls "the engine" is one `0x470`-byte allocation whose
address lives in a single global. This page records what has been read about
it: the creation step, both constructors, the interface registry inside it, and
the value codecs it uses. Everything here is evidence class **read** unless a
line says otherwise; the open list is at the end.

Crate: `crates/ow-engine` (`codec`, `registry`, `world`).

## The creation step (`0x140585220`)

The map-load entry `0x14055a0f0` calls it once (`E8` at `0x14055a205`), then
calls `ConstructEngineSubsystems` (`0x140554220`) — the function that fills the
object with its ~40 subsystems.

In order (this is the whole function, `__chkstk` wrapper aside):

1. `Alloc(0x470, 0)` (`0x141baff30`). Null means "no object": every later step
   is skipped and the global is stored null.
2. `0x141c57c40` — returns the value passed as the constructor's second
   argument (`rdx`). Not read yet.
3. `WorldCtor(world, that, 0, 0)` — `rcx = world`, `rdx = 0x141c57c40`'s
   result, `r8 = r9 = 0` (`0x1405838c0`).
4. `mov [0x143b7f410], rax` — the **only** store to the engine global in the
   image; 5,486 rip-relative reads in 3,484 `.pdata` functions read it.
5. `0x141c52fc0` with `rcx = world` — after publication, so a callee can
   already see the global. Not read.
6. `0x141b33e20` on a stack object at `rsp+0x20`, a `0x6f` byte staged into the
   object at `+0xe08` and `0x141c61a80` (a debug/name assertion pair; `0x6f` is
   `'o'`). Not read.
7. `0x141c51cc0(world, 0x1441880e8, &stack)`; the returned object's dword at
   `+0x138` is stored to `world+0x468` (`0x1405852c6..0x1405852cd`). The key
   `0x1441880e8` is the **same global** the constructor reads for that field,
   so this looks like a lookup that replaces a placeholder with the real
   record. Not read.
8. `0x141c56970(world)` — not read.
9. Teardown: `0x142f83900` into the stack object, `0x141c61a30`, stack cookie
   check, `ret`.

## The constructors

`WorldCtor` (`0x1405838c0`, ends at `0x140583c41`) calls `WorldBaseCtor`
(`0x141c507c0`) first and then writes every field from `+0x1c8` on. Its own
arguments: `rcx = world`, `rdx` stored at `+0x00` (the vtable slot is written by
the base constructor), `r8`/`r9` unused, stack slot `+0x40` used as the mixing
state for the encoded fields.

Vtables, in order: base writes `0x14328c990`, the derived constructor overwrites
it with `0x142f81090` and finally `0x142f83aa0`. Only the third is the live
one; the other two exist while the object is half-built.

Every field either constructor writes, with the value it writes, is listed in
`crates/ow-engine/src/world.rs` module docs next to the struct. The struct is
`#[repr(C)]` and `tests/world_layout.rs` asserts all 44 offsets, the `0x470`
size and each written value.

### The two encoded fields that are not plain constants

- `+0x460` is written **twice**. First at `0x140583bb2` with the address codec
  evaluated at 0; the constructor's stored constant `0xf45f13eca6129815` is
  exactly that codec's inner mix of 0, so the two written forms
  (`SEED_B(0xf45f13eca6129815) ^ 0x689f22ff8960a2c7` and "codec at 0") agree,
  which is a useful self-check. Then, at `0x140583c2b`, it is overwritten with
  the codec applied to the **address** of a `0x1b8`-byte object the constructor
  has just built. So the field is an encoded heap pointer; its value depends on
  the process's allocator, not on the image.
- `+0x468` is `mov eax, dword ptr [rip + 0x3c04524]` (`0x140583bbe`), i.e. a
  copy of the u32 at `0x1441880e8`. That address is in the zero-fill part of
  `.data` (`.data` is raw-backed only up to `0x143b79600`), so at process start
  the constructor reads 0. The creation step then overwrites the field (step 7
  above).

### The `0x1b8`-byte object

`Alloc(0x1b8, 0)`, then `0x140be33c0` on it. That function returns its own
argument, so the constructor's `rbx` is the object's address. It fills four
copies of a six-dword struct — dwords from `0x1442a31d0..0x1442a31dc`, the fifth
from `0x143870e48`, plus the integers 1, 2, 3, 4 at `+0x48`, `+0x90`, `+0xd8`,
`+0x120` — and the constructor drops the pointer. Nothing in `World` refers to
it; only the encoded address in `+0x460` survives.

## The interface registry at `+0x188`

`RegisterWorldInterface` (`0x141c52d10`) appends to a `u64` array at
`world+0x188`; `0x140767da0` grows it. The header is the client's usual
`{ data, len, capacity }` but the **capacity word carries a tag in bit 31**:
`0x80000000` means the buffer is borrowed (owned by the caller) and must not be
freed or reallocated in place. Growth is `newcap = max(cap + cap/2, len +
requested)`, and a borrowed buffer is always copied.

Every stored pointer is transformed. The transform is inline in the function
itself (`0x141c52d44..0x141c52d59`): `xor 0x238e31e8cd062f58`, `ror 0x31`,
`add 0x34b5f414245bf6ef`. `crates/ow-engine/src/codec.rs` carries that codec and
the four out-of-line ones, each with the address of the helper that performs it.
`INTERFACE_ARRAY.apply(0)` is
`0x4daa5a973c0808b6`, which is exactly what the base constructor stores at
`+0x058`: the "no pointer" value is this codec's encoding of zero, not `0`.

The pointer→index hash map at `+0x198` (built by `0x141c553a0`, inserted into by
`0x141c56fb0`) is **not** modelled yet.

## The subsystem schedule (`0x140554220`, read and modelled)

`ConstructEngineSubsystems` is 6041 bytes (`0x140554220..0x1405559b8`; the
`ret` is at `0x1405559b8`, int3 padding after). It is the only writer of the
subsystem window. Crate: `crates/ow-engine/src/subsystems.rs`;
`tests/subsystem_schedule.rs` pins every fact below.

**The table is a decode of the instruction stream, not a measurement of
output.** `dev/eng_dump.py` sweeps the function's own bytes from the offline
image (`dev/data/offline_recover/module_offline.bin`, base `0x140000000`);
`dev/eng_verify_schedule.py` re-derives each row from those instructions and
diffs it against the census in `dev/engine_subsystems.csv`:

- an object is a **token** — the address of the `Alloc` call that produced it,
  or of the factory call that returned it; `mov`/`lea` copy a token, a load or
  a fresh constant clears it, so a value that does not come from an object has
  none;
- a **slot store** is `mov qword ptr [ENGINE + slot], reg` with `slot` in
  `0x220..0x448`; its producer is the constructor or factory that ran on the
  register's token, and the allocation size is attributed only while the
  register still holds the object itself (the `+0x448` adjustor stores a
  derived value and has no size);
- a **registration** is the `RegisterWorldInterface` (`0x141c52d10`) call whose
  RDX argument carries the same token, whether it came from the register or
  from a slot the token was stored into;
- a **vtable store** is `lea reg,[rip+X]` followed by `mov qword ptr [obj(+off)], reg`.

`dev/eng_gen_subsystems.py` writes `SUBSYSTEMS` and `TAIL_ATTACH` from the
verified table (`dev/engine_subsystems.verified.csv`). The decode reconciles on
every count, and the function's bytes in the Ghidra project equal the offline
image at each site sampled (entry `0x140554220`, the first store
`0x1405542c9`, the stub `0x141c578c0`, the adjustor's `Alloc` at `0x140554b6f`,
the decode site `0x140555309`):

| quantity | count |
|---|---|
| instructions in the function | 1531 |
| `Alloc(size, 0)` calls (`0x141baff30`) | 80 (first `0x140554241`, `0x10`; last `0x1405556b2`) |
| `RegisterWorldInterface` calls (`0x141c52d10`) | 84 (first `0x14055426d`, last `0x1405556e3`) |
| stores into `world+0x220..+0x448` | 65, one per slot, no slot twice |
| tail attaches (`0x140bc7590`) | 21 |
| distinct allocation sizes, all 80 allocations | 21 (`0x10`..`0x980`) |
| distinct allocation sizes of the 61 stored ones | 19 (`0x10`..`0x7d0`) |

Why the numbers differ: of the 84 registrations, 64 carry a member slot's
object (one block stores two slots) and 20 register an object that never
reaches a slot; of the 80 allocations, 61 are stored straight into their slot,
one reaches `+0x448` as an adjustor, and 18 are constructed and dropped. So
**20 of the 84 constructions never reach a slot** — the registration count
overstates the table, and the table is 65 entries.

Five offsets inside the 70-slot window are never written and are therefore
absent rather than guessed: `+0x228`, `+0x230`, `+0x350`, `+0x418`, `+0x440`.
The function contains no indexed store into the window, so they are plain
fields (or another function's), not a missed member.

Structural facts, all carried by the model:

- **31 of the 65 slots share one constructor**, `0x141c578c0` — a 28-byte stub
  that writes vtable `0x142f7d778` and the cookie `0x7495174655b5d763` and
  returns its argument. Every caller overwrites `[obj]` with its own vtable
  almost immediately, so `0x142f7d778` survives only inside the stub. That
  vtable is a real one — three methods (`0x14054c370`, `0x14054ee60`,
  `0x14054d540`, each a `lea rax,[rip+X]; ret` accessor) and `_purecall` at
  `+0x18` — so the stub is the constructor of an **abstract base** that the 31
  slots derive from, not a placeholder. It was created as a function in the
  project in this pass (`SubsystemStubCtor`).
- **Four slots have no inline `Alloc`**: `+0x220` is a decoded pointer from
  `[world+0x330]+0x60` — the codec is `0x1405437f0` (`ror 0xf`,
  `xor 0x5a1b63795b8475ea`, `sub 0x4998612e34f258d3`, `ror 0x1e`), called at
  `0x140555325` and created as `DecodeEnginePtr220` in this pass. What follows
  the call (`0x14055532a..0x14055536a`) is the *same* inline chain `WorldCtor`
  applies to this field: `xor 0x6e`, `add 0x6c5df24a0815916e`,
  `xor 0x3c1986ede8c2b44a`, `ror 0x10`, `xor 0xc2300451bcb9e837`,
  `add 0x5c04429ca265e813`, stored at `0x140555372`. So the derived
  constructor's `+0x220` value is this expression's zero form, and the subsystem
  pass overwrites it with the same expression applied to a decoded pointer —
  `World::new`'s `+0x220` is a placeholder of exactly this shape. `+0x340` and `+0x358` are factory returns
  (`0x140594090`, `0x1406cc990`); `+0x448` is an adjustor over the `0x68`-byte
  object built at `0x140554b6f`. `+0x220` is the one slot store with no
  registration of its own (the follow-on `Alloc(0x980)` object is registered,
  but that object never reaches a slot).
- **`+0x330` is read back inside the function** (`0x140555309`) to feed
  `+0x220`, so the schedule depends on its own earlier store: a sequence, not a
  flat list.
- `+0x3f8` and `+0x420` have distinct primary vtables but the same secondary
  vtable `0x142f7e168` (`0x14055515d`, `0x140555616`).
- The tail attach pass (`0x1405556e8..0x140555985`) is **not** a list of new
  members: 21 times it loads `[0x143b7f410]`, reads an already-filled slot and
  re-attaches that subsystem's `+0x10` base through `0x140bc7590`. Only the
  last entry (`+0x268`) is guarded (`cmp qword ptr [rsi+0xe0], 0` at
  `0x14055597c`); the other 20 are unconditional. No row of the table is fed by
  a rip-relative global: every one of the 65 stores comes from a computed
  register.
- One block (`0x140554301..0x140554418`) is anti-tamper code: it reads a cookie
  from the context object, compares it against a constant and calls junk
  targets plus one registration with a garbage interface. It allocates nothing
  and stores no slot.

### What the model carries, and what it does not

`SUBSYSTEMS` has one row per slot store, in **store order** (the first store is
`+0x408` at `0x1405542c9`, the last `+0x438` at `0x1405556d9`; slot order is a
lookup, `subsystem(slot)`). Each row carries the slot offset, the source (an
`Alloc` size, one of the two factories, the `+0x220` decode, or the `+0x448`
adjustor), the producer, the primary vtable, the secondary vtable bases with
their offsets, the store site and the registration site. `World` grew
`subsystem_slot`, `subsystem_slot_address` and `set_subsystem_slot`, which
refuse the five empty offsets, so a caller cannot address a slot the client
never writes.

The vtable fields are **the stores the block itself performs**, not everything
the class installs. A producer may set its own vtables inside its body: slot
`+0x288` (`Alloc(0x18)`, producer `0x1405bc5c0`) has no inline store, and the
constructor calls the shared stub and then installs `0x142f84920` at `+0x00`
and `0x142f84998` at `+0x10` itself — two interfaces on a `0x18`-byte object,
with the stub's cookie keeping `+0x08`. So an empty `vtable` means "the block
stores none", not "the class has none".

Not carried, and open: the **class identity** of each subsystem (a name for
the constructor/vtable pair), and the 20 registered objects that never reach a
slot (`+0x220`'s object is one of the four sources, the anti-tamper
registration another).

### The constructors and their vtables

`dev/eng_class_profile.py` reads each of the 35 distinct producers' own body
(bounded by its `.pdata` range, or by its terminating instruction for the two
leaf functions `.pdata` omits: the stub and the `+0x220` decoder), tracks `this`
in RCX and its copies, and records every `lea reg,[rip+X]` followed by
`mov [this+off],reg` as a vtable store. It writes
`dev/engine_classes.json`; `dev/eng_gen_classes.py` turns that into
`ow_engine::classes`; `crates/ow-engine/tests/class_layout.rs` pins it.

- **35 constructors, 96 installs, 117 distinct tables.** 70 tables are
  installed by a constructor's own body and 47 by the schedule block, and the
  two sets are **disjoint** (70 + 47 = 117). The 47 block-stored tables are the
  31 stub-slot tables plus 16 others.
- **The stub is a base constructor, not a class of its own.** All 31 stub slots
  also have a block store at `+0x00`: the block calls `0x141c578c0` (which
  writes `0x142f7d778` and the cookie `0x7495174655b5d763`) and then overwrites
  `+0x00` with the slot's own table, so `0x142f7d778` survives in no slot. The
  31 slots are 31 derived classes sharing one base constructor.
- **Where the object's `+0x00` table comes from**: the block for 32 slots, the
  constructor for 30, and neither for 3 — `+0x220` (decoded pointer), `+0x340`
  and `+0x358` (factory returns). For those three the table is not observable
  in this function: open, not empty.
- **`+0x2f0` is an aggregation of six sub-objects.** Its constructor
  (`0x14072c560`, `0x39c` bytes, 16 installs, 70 field stores, 12 calls) writes
  the shared base table `0x142f8ac68` at `+0x10`, `+0x90`, `+0x100`, `+0x158`,
  `+0x290` and `+0x2e0`, then a different derived table at each same offset.
  Ghidra's decompilation of that function shows the same order; the data keeps
  both stores so the overwrite is visible.
- **`+0x308` is the one non-stub slot the block re-points**: its constructor
  installs `0x1430a2910` five times (`+0x00`..`+0x28`), then the block stores
  `0x142f7f298` at `+0x00`.
- **A table's length is only read where it is bounded from both sides.** 44 of
  the 117 tables are exactly 15 code entries — for 39 the next known table
  begins at `+0x78`, for 5 a non-code word does. Those non-code words are not
  sentinels: they are ordinary `.rdata` (ASCII such as `"SteamUse"`,
  `"advapi32"`, format fragments like `"%s (2s)"`, and float pairs such as
  `65535.0`/`500000.0`), which is why the section interleaves tables with data.
  The remaining tables stop at the next known table (2-entry and 5-entry
  sub-interfaces are common), and two have no neighbour and no non-code word
  within 128 entries — their length is **not read**, and the JSON records the
  count as a lower bound together with the word that stopped the read.
- **The tables are densely padded with a no-op.** `0x14053a420` (three bytes,
  `c2 00 00` = `ret 0`) is the most common entry by far: 1,060 of the entries
  read across the 117 tables. The next most common are the static accessors
  `0x14055dfe0` (102) and `0x14053a070`/`0x14054a160` (97 each). So a 15-entry
  interface is normal because most of its methods are the same empty `ret`.
- **Four tables contain a curated name**: `0x142f7f488+0xd8`
  (`RegisterLocalHitEffectsSettings`, slot `+0x2a8`'s secondary),
  `0x142f7f898+0x270` (`TickSequenceCameraView`, slot `+0x2c0`'s primary),
  `0x142f87958+0x108` (`FirstPersonCommandUpdateEntry`, `+0x2e8`'s
  constructor-installed secondary) and `0x142f89570+0xf0` (`SetCameraFov`,
  slot `+0x2b0`'s constructor-installed primary). **None of the four names the
  slot's class yet**: the first two reads ran past 15 entries into neighbouring
  tables of the bank, and the last two carry the name at entry 30 and 33 of a
  run four 15-entry tables long. What would name a class is a method that calls
  a known subsystem API *inside the slot's own first 15 entries*; no slot has
  one yet.

### What the slots are used for (a consumer lower bound)

`dev/engine_slot_consumers.py` finds every RIP-relative reference to
`0x143b7f410` (5,487 references in 3,484 `.pdata` functions; the outermost
containing range, because `.pdata` also lists mid-function fragments), then
disassembles each such function and follows `engine` → `[engine+slot]` → the
object's fields and vtable, dropping volatile registers at every call and
clearing a register on any other write. Result
(`dev/engine_slot_consumers.csv`, 616 records):

- **35 of the 65 member slots** are touched by a function that loads the
  engine global itself; the other 30 are not, and that is **not** proof they
  are unused — a consumer that receives the engine pointer as an argument is
  invisible to this scan. `+0x220` is the known case: the function that stores
  it reads it back, but from `rbx`, never from the global.
- Most touched slots are only **loaded and passed on** (one `slot_read`, the
  value going into the next call's argument). Eight are dereferenced by the
  consumer itself: `+0x238`, `+0x248`, `+0x258`, `+0x290` (which a consumer also
  writes), `+0x2e8`, `+0x338`, `+0x358` and `+0x408`.
- The busiest are `+0x290` (129 accesses in 89 functions), `+0x248` (99 in 39),
  `+0x240` (66 in 54), `+0x238` (58 in 35) and `+0x278` (34 in 25); the
  quietest are `+0x268`, `+0x308`, `+0x310`, `+0x370`, `+0x388` and `+0x390`
  with one access each.
- The census names no slot. Two named consumers published in an earlier
  version of this file are **withdrawn**: `0x1406bc420`
  (`UpdateSequenceCameraViewState7`) does not read the engine global at all
  (its first global read is `[rip+0x34bd27a]` = `0x1441796a8`), and `+0x310`'s
  single consumer `0x140618f70` has no curated name. The 22,241-access figure
  and the per-slot table that came with it were produced by a looser scan, are
  not reproducible with the shipped script, and are withdrawn with them.

So the next identification step is per-slot: take a slot with few consumers,
read the constructor and one consumer, and settle the class. The census makes
that cheap by listing them; it does not do it.

## The base constructor `WorldBaseCtor` (`0x141c507c0`)

It writes the range `+0x000..+0x1c8`; the derived constructor writes everything
from `+0x1c8` on. Read in full (979 bytes); `crates/ow-engine/src/world.rs`
carries the field table and the tests pin every offset and written value.

Three things about it are easy to get wrong and are worth stating separately:

- **Its fifth argument, not its fourth, becomes `+0x008`.** Entry spills `rdx`,
  `r8`, `r9d` to `[rsp+0x10]`, `[rsp+0x18]`, `[rsp+0x20]`; `[rsp+0x20]` is the
  home slot of the fifth argument, and `cmp dword [rsp+0x90], 0`
  (0x141c507df) is the first thing the function does. The byte at `+0x008` is
  `(fifth argument == 0)`. The sixth argument lands at `+0x178`.
- **`+0x018` is written twice.** `0x389aae49b6cecf17` (0x141c5081b) is a
  placeholder; at 0x141c50aa0 it becomes an `Alloc`ed pointer put through the
  `0x18317a531d116c0c` codec family (`sub`, `ror 0x1a`,
  `add 0x8fb1b242c9b3a42d`, `xor 0x73cc01350b9bc072`). The decoded address is
  then used by a loop at 0x141c50ac0 that writes `0x10000 / 0x10` entries, each
  holding an `Alloc(0x80)` pointer and a zero dword at `+0x08`.
- **`+0x058` is written twice.** `INTERFACE_ARRAY(0)` (0x141c5086c) is also a
  placeholder; at 0x141c50b76 it becomes
  `((r8 ^ 0x238e31e8cd062f58) ror 0x31) + 0x34b5f414245bf6ef`.

So two of the fields that look like constants are encoded heap pointers. The
same is true of `+0x460`, which the derived constructor writes twice.

All three double writes have the same shape, and the arithmetic proves it: the
value written first is the codec evaluated at **zero**, i.e. the codec's "no
pointer" value.

| field | first write | codec at 0 |
|---|---|---|
| `+0x018` | `0x389aae49b6cecf17` | `encode_q_18(0)` = `0x389aae49b6cecf17` |
| `+0x058` | `0x4daa5a973c0808b6` | `encode_q_58(0)` = `0x4daa5a973c0808b6` |
| `+0x460` | the constructor's `0xf45f13eca6129815` form | `OBJ_460(0)` = the same |

That is a self-check on the constants and on the codecs: if a constant were
mis-transcribed, its codec would not evaluate to it. `tests/world_layout.rs`
pins both.

Other writes read in this pass: `+0x010` = 0, `+0x078` = 0 (byte), `+0x07c` =
`0xffffffff`, `+0x080` = 0, sub-constructors `0x141c23430(+0xc8, 0x20)` and
`0x141c23430(+0x108, 0x20)`, `+0x130` = `0x389aae49b6cecf17`, `+0x148` = a
value modulo `0x21`, `+0x160` = `0x8b8bda8c7a56872b`, `+0x170` = 0 (interlocked),
`+0x188..+0x1b0` = 0, `+0x1b8` = `0xffffffff`, `+0x1bc` = 0, `+0x1c0` = 0 (byte),
and a tail call to `0x141c55770`.

## Open

1. `0x141c57c40` — the constructor's second argument.
2. `0x141c52fc0` (called right after publication), `0x141c56970`,
   `0x141c55770` (the base constructor's tail call).
3. The stack "debug" pair `0x141b33e20` / `0x141c61a80` and the `'o'` byte.
4. `0x141c51cc0` — the lookup that produces the real `+0x468` record, and what
   `+0x138` of its result is.
5. The pointer→index map at `+0x198` (`0x141c553a0`, `0x141c56fb0`).
6. `ConstructEngineSubsystems`' schedule and attach list are **read and
   modelled** (`ow_engine::subsystems`); what is open is the meaning of the
   rows: a name for each of the 35 constructors and 117 vtables, and the 20
   registered objects that never reach a slot.
7. The TLS-indexed table read just before `+0x148` (index from a dword global,
   `gs:[0x58]`), and the `0x10000`-byte table the `+0x018` loop fills.
8. `+0x110..+0x130` and `+0x0d0..+0x0f8` are still unwritten by any read code
   path — assumed zero from the allocator, not proven.
9. The subsystem window's five empty offsets are known not to be written by
   this function; whether they are read anywhere (the 3,484 functions that read
   the engine global) is not checked.
10. What each of the 65 subsystems **is**. `dev/engine_slot_consumers.py` now
    gives a per-slot consumer list (a lower bound, see above); the class
    itself has to be read one slot at a time — constructor, vtable methods and
    one consumer.
