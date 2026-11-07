# Engine object knowledge brief

> **Superseded in part (2026-11-06).** This brief was written before the
> constructors were read from Ghidra. Its "not read" rows for `+0x000..+0x187`,
> `+0x1c0..+0x21f` and `+0x46c` are now read: `WorldBaseCtor` `0x141c507c0` and
> `WorldCtor` `0x1405838c0` write every field in those ranges, and the field
> table lives in `crates/ow-engine/src/world.rs` with the findings in
> `docs/formats/engine.md`. Treat this file as the *prior* survey of the
> subsystem schedule, not as current layout truth.

Global singleton `_DAT_143b7f410` (VA `0x143b7f410`, the engine object).
Everything below comes from files already in this repo. Items marked
**[read for this brief]** were read from the local offline image with the
repo's own tools (`dev/rd.py`, `dev/engine_ref_scan.py`), not from Ghidra, and
are not yet written into `docs/`.

## 0. Identity and lifetime

| fact | evidence |
|---|---|
| global `0x143b7f410` holds the engine object | dev/eng_tab.py:7 (`ENGINE_GLOBAL = 0x143B7F410`); dev/eng_rip.py (resolves `p+len+disp`); dev/engine_dump.asm:1381-1385 |
| allocated as `0x470` bytes by bootstrap `0x140585220` | **[read for this brief]** `0x140585243 mov ecx,0x470 / xor edx,edx / call 0x141baff30` (`Alloc`, docs/function-names.csv:802) |
| constructed by `0x1405838c0(obj, arg, 0, 0)` | **[read for this brief]** `0x140585260 mov rdx,rax` (result of `0x141c57c40`) / `0x140585266 call 0x1405838c0`; `python dev/callers.py 0x1405838c0` → `0x140585266 [e8] in 0x140585220+0x46`, plus one data pointer `0x144542218 [dword-rva] (no .pdata)` |
| bootstrap stores it to the global, then runs three post-steps | **[read for this brief]** `0x14058526e mov [rip+…],rax` → `0x143b7f410`; `call 0x141c52fc0`; stack-string build (`0x141b33e20`, `0x141c61a80`); `call 0x141c51cc0`; `call 0x141c56970`; `ret 0x14058530d` |
| bootstrap's only `E8` caller is the map-load entry | `RunMapLoadEntry 0x14055a0f0`: `0x14055a205 -> 0x140585220 [e8]` (dev/callers.py); docs/function-names.csv:837; docs/formats/static-collision.md:2619 |
| the same entry calls the subsystem constructor `0x140554220` | docs/function-names.csv:844; docs/formats/static-collision.md:2622-2628; MODLOG.md:138-145; STATUS-handoff.md:426 |
| object referenced by RIP **5487** times across `.text` | **[read for this brief]** `python dev/engine_ref_scan.py 0x143b7f410` → `# 5487 references` (12 s), e.g. `0x140b8d378,0x140b8d180`, `0x140bad0b1,0x140bace90`, `0x140bc1a43,0x140bc19b3`. Only the constructor's own dump had been censused before (dev/eng_rip.txt: `engine_global_refs=29`, all inside `0x140554220`) |

## 1. Field map

Byte offsets from the object base. `dev/` artifacts are the reconstruction of
the constructor's linear-sweep disassembly `dev/engine_dump.asm` (1532
instructions, written by `dev/eng_dump.py 0x140554220 0x1405559b8`).

### 1a. Below the subsystem area

| offset | size | meaning | evidence |
|---|---|---|---|
| `+0x000..+0x187` | — | **not read**: no repo record of any field here. `0x1405838c0` has no decompile in `dev/decompiles/` and only two references (`0x140585266`, data `0x144542218`) | absent from docs/function-names.csv, docs/struct-layouts.csv (no engine rows), docs/glossary.md |
| `+0x188` | 8 | subsystem array **data pointer** (`void**`, stride 8) | dev/decompiles/engine_RegisterWorldInterface_0x141c52d10.c (append `*(arr+count*8)`); docs/function-names.csv:845; docs/formats/static-collision.md:2626 |
| `+0x190` | 4 | subsystem array **count** (i32) | same (incremented right after the append) |
| `+0x194` | 4 | subsystem array **capacity** (i32; bit 31 = owns a heap block) | dev/decompiles/engine_FUN_140767da0_0x140767da0.c: `(cap*2)>>1` for the length, `bit31\|len` restored on grow |
| `+0x198` | 8 | interface map **bucket array** (`u32[cap]`, `0xffffffff` = empty) | engine_RegisterWorldInterface_0x141c52d10.c: `FUN_141c553a0(&engine+0x198, 3)` when null |
| `+0x1a0` | 8 | interface map **entries**, stride `0x10`: `+0x00 u32 hash`, `+0x04 i32 next`, `+0x08 void* interface` | same |
| `+0x1a8` | 8 | interface map **values**, stride 8, one u64 per entry (encoded subsystem pointer; sentinel `0x4daa5a973c0808b6` written first) | same |
| `+0x1b0` | 4 | interface map **bucket count** (power of two; used as `count-1` mask) | same; `FUN_141c553a0` keeps it as its third qword (`+0x18`) |
| `+0x1b4` | 4 | interface map **used entry count** | same |
| `+0x1b8` | 4 | interface map **free-list head** | same |
| `+0x1bc` | 4 | interface map **free count** | same |
| `+0x1c0..+0x21f` | — | **not read** (gap; first subsystem slot is `0x220`) | — |

The interface map is one allocation: `cap*4` rounded up to 8, then `cap*0x10`,
then `cap*8` (dev/decompiles/engine_Func141c553a0_0x141c553a0.c, called
`RehashInterfaceMap` here). Hash is `((u64)interface >> 4) & 0x7fffffff`,
bucket `hash & (bucket_count-1)`, probe walks the `next` chain.

### 1b. Subsystem slots and the attach array

| offset | size | meaning | evidence |
|---|---|---|---|
| `+0x220` | 8 | subsystem slot 0 — from the factory call `0x1405437f0` (`0x140555325 -> 0x140555372`) | dev/eng_tab.json block 69; dev/engine_subsystems.raw.csv:2 |
| `+0x228` | 8 | **attach array** data pointer | dev/decompiles/engine_FUN_140bc7590_0x140bc7590.c |
| `+0x230` | 4 | attach array **count** (i32, incremented per attach) | same |
| `+0x234` | 4 | attach array **capacity** (grow helper `0x140bc76f0`) | same |
| `+0x238..+0x448` | 8 each | **subsystem slots** — 65 stored slots (with `0x220`) | dev/engine_subsystems.raw.csv (65 rows); dev/eng_tab.json (65 blocks); docs/function-names.csv:844 ("byte offsets 0x220..0x448") |

The attach header (`0x228`/`0x230`/`0x234`) sits in the hole between slot
`+0x220` and the run that starts at `+0x238`. On the 8-byte grid
`0x220..0x448` the unused offsets are exactly `0x228`, `0x230`, `0x350`,
`0x418`, `0x440` (computed from dev/engine_subsystems.raw.csv).

### 1c. Tail

| offset | size | meaning | evidence |
|---|---|---|---|
| `+0x448` | 8 | last slot, but not a fresh object: interior pointer `rdi + 0x10 + (i32)[[rdi+0x10]+4]`, where `rdi` is the `0x68`-byte object built by ctor `0x140723890` (`edx=1`); stored at `0x140554bba`, then `0x14054acf0(engine, …)` runs | dev/engine_dump.asm:0x140554b6f-0x140554bcb |
| `+0x468` | 4 | **i32 copied from a CVar value** `[cvar+0x138]`; the CVar comes from `0x141c51cc0(engine, <name string>)` | **[read for this brief]** `0x1405852c0 mov ecx,[rax+0x138]` / `0x1405852c6..0x1405852cd mov [engine+0x468],ecx`; lookup `0x1405852bb call 0x141c51cc0` with `lea rdx,[rip+…]` → `0x1441880e8` |
| `+0x46c..+0x46f` | 4 | **not read** (the object ends at `0x470`) | — |

**[read for this brief]** Other globals inside the same span, meaning
unverified: `0x143b7f4d0` (= `+0xc0`) is used as the head of an inline pointer
array by `0x140585330` (lookup loop `cmp rcx,rdx` / `[r8+rax*8+8]`, i.e.
"index of object"), and `0x143b7f558` (= `+0x148`) is compared with `[rdx+0x88]`
by `0x140585310` (`sete al`).

## 2. The subsystem constructor algorithm

`ConstructEngineSubsystems 0x140554220`: one body, 1532 instructions, entry
`rsi = param_1` (context, read at `+0x08` and `+0xe0`), `rbx = param_2` (**the
engine object**), `rbp = 0` (the null every failure path selects).
Reconstruction tools: dev/eng_table.py (docstring states the block grammar),
dev/eng_build.py, dev/eng_csv.py, dev/eng_tab.py; outputs dev/eng_table.txt,
dev/eng_blocks.txt, dev/eng_tab.json, dev/eng_recs.json,
dev/engine_subsystems.raw.csv; filtered view dev/engine_filt.asm
(dev/eng_filter.py).

### 2a. Per-subsystem block (repeated 84 times; `call 0x141c52d10` ends each block)

1. `xor edx,edx / mov ecx,SIZE / call 0x141baff30` → `Alloc(SIZE, 0)`
   (`Alloc`, docs/function-names.csv:802; Ghidra marks it noreturn, which is
   why the body had to be dumped rather than decompiled).
2. `test rax,rax / je L / mov rcx,rax / call CTOR / jmp L2 / L: mov rax,rbp`
   — construct in place, else the null `rbp`.
3. For the plain `0x10`-byte subsystems: ctor `0x141c578c0` (base ctor) then
   `lea rax,[rip+vtable] / mov [obj+0x00],rax` — the vtable is installed by the
   caller, not the ctor. Every slot's vtable is listed in
   dev/engine_subsystems.raw.csv column `vtable` (e.g. `+0x240` =
   `0x142f7fc90;0x142f7fd08`, `+0x278` =
   `0x142f7e758;0x142f7e6c8;0x142f7e6f8;0x142f7e728;0x142f7e740`).
4. `mov rdx,rax / mov [rbx+SLOT],rax` — only when the object **is** an engine
   slot (65 of the 84 blocks store).
5. `mov rcx,rbx / call 0x141c52d10` — `RegisterWorldInterface(engine, obj)`.

Inventory (dev/eng_table.txt header): **80 allocations, 84 registration calls,
21 attach calls, 113 store-shaped instructions.** Three slots come from factory
calls, not an allocation: `+0x220` ← `0x1405437f0`, `+0x340` ← `0x140594090`,
`+0x358` ← `0x1406cc990` (`0x140554ac1`, `0x140554acd`; both registered at
`0x140554ae3`, `0x140554af2`). One `0x980`-byte object (`alloc@0x140555379`,
ctor `0x140750b70`) is registered but never stored in a slot. The first two
allocations of the body (`0x10` @ `0x141c578c0`, vtables `0x142f7df88` /
`0x142f80d90`) are also registered but not stored (dev/eng_blocks.txt blocks
00, 01).

Sizes and constructors per slot (dev/eng_tab.json; `factory` = no allocation):

```
0x220 factory 0x1405437f0      0x238 0x7d0 @0x14064f080   0x240 0x88  @0x1406c0580
0x248 0xc0  @0x14055cc60       0x250 0x18  @0x141c578c0   0x258 0x70  @0x14073fc00
0x260 0x40  @0x1406735c0       0x268 0x58  @0x1405dac00   0x270 0x18  @0x141c578c0
0x278 0xb0  @0x141c578c0       0x280 0x38  @0x141c578c0   0x288 0x18  @0x1405bc5c0
0x290 0x90  @0x14060fd30       0x298 0x10  @0x141c578c0   0x2a0 0x10  @0x141c578c0
0x2a8 0x18  @0x141c578c0       0x2b0 0x18  @0x1406b58d0   0x2b8 0x10  @0x141c578c0
0x2c0 0x10  @0x141c578c0       0x2c8 0x10  @0x141c578c0   0x2d0 0x10  @0x1407493c0
0x2d8 0x18  @0x1406b5000       0x2e0 0x58  @0x1406a4de0   0x2e8 0x2f8 @0x140651630
0x2f0 0x390 @0x14072c560       0x2f8 0x88  @0x140598900   0x300 0x18  @0x141c578c0
0x308 0x30  @0x140bdf740       0x310 0x68  @0x140657b40   0x318 0x10  @0x141c578c0
0x320 0x10  @0x1406928b0       0x328 0x10  @0x141c578c0   0x330 0x68  @0x14074c5a0
0x338 0xd0  @0x14074a300       0x340 factory 0x140594090   0x348 0x10  @0x141c578c0
0x358 factory 0x1406cc990     0x360 0x1b8 @0x14066b100   0x368 0x40  @0x14061c0a0
0x370 0x28  @0x1406b4570       0x378 0x10  @0x1406c5380   0x380 0x10  @0x140741560
0x388 0x10  @0x141c578c0       0x390 0x10  @0x141c578c0   0x398 0x18  @0x141c578c0
0x3a0 0x18  @0x14061d1e0       0x3a8 0x10  @0x1405cf4f0   0x3b0 0x18  @0x1406c6d10
0x3b8 0x10  @0x141c578c0       0x3c0 0x10  @0x141c578c0   0x3c8 0x10  @0x141c578c0
0x3d0 0x10  @0x141c578c0       0x3d8 0x10  @0x141c578c0   0x3e0 0x30  @0x14061c4c0
0x3e8 0x18  @0x141c578c0       0x3f0 0x18  @0x141c578c0   0x3f8 0x18  @0x141c578c0
0x400 0x18  @0x141c578c0       0x408 0x100 @0x140662760   0x410 0x18  @0x141c578c0
0x420 0x18  @0x141c578c0       0x428 0x10  @0x141c578c0   0x430 0x10  @0x141c578c0
0x438 0x10  @0x141c578c0       0x448 interior pointer (see 1c)
```

### 2b. `RegisterWorldInterface 0x141c52d10`
(dev/decompiles/engine_RegisterWorldInterface_0x141c52d10.c)

`void RegisterWorldInterface(engine, subsystem)`:

1. Growth test on the array: `count+1 <= (capacity*2)>>1`, else
   `FUN_140767da0(&engine+0x188, …)` grows it (`new = cap + cap/4` when that
   covers the request, else the request; `Alloc(new*8)` through
   `func_0x000141c23490(&PTR_PTR_143a01d18, …)`; the old block is released by
   `func_0x000140578190` when bit 31 says it was heap).
2. **Encoded-pointer store** at `array[count]`:
   `store = ror64(ptr ^ 0x238e31e8cd062f58, 0x31) + 0x34b5f414245bf6ef`
   (rotate-left 15 after the xor, then add). The array holds no raw pointer.
3. Back-pointer: `subsystem[0x08] = (engine ^ 0xd6c0ac78d0e44481) + 0x9dd46acd84d192e2`
   — same encoding family (`subsystem[0]` is its vtable).
4. Interface-chain walk: the first interface is the return of
   `(*(vtable[0]))(subsystem)`; each next is `[interface+0x90]`; the walk stops
   at null or at the sentinel `&DAT_14417fbe0`.
5. Files each interface in the map at `+0x198` (lazy `FUN_141c553a0(&+0x198, 3)`)
   with `hash = ((u64)interface >> 4) & 0x7fffffff` and bucket
   `hash & (count-1)`, chaining through `entry.next`; on a hit it runs the
   anti-tamper check `FUN_141c24170(…)` and executes `swi 3` when it trips. A new
   entry comes from the free list (`+0x1b8`/`+0x1bc`) or the bump counter
   (`+0x1b4`).
6. The value written for the interface uses the same encoding as step 2 (the
   sentinel `0x4daa5a973c0808b6` is written first).

`FUN_141c56fb0` (dev/decompiles/engine_FileInterfaces_0x141c56fb0.c) is the
same body **without** the array append and growth — a "file this subsystem's
interfaces only" entry (steps 3-6). It has no row in docs/function-names.csv.

### 2c. Attach `0x140bc7590`
(dev/decompiles/engine_FUN_140bc7590_0x140bc7590.c)

`void FUN_140bc7590(engine, sub)`: decode `sub`, run `func_0x000140bc7280`,
then the same growth test on the **attach array** (`count+1`,
`(capacity*2)>>1`, grow helper `0x140bc76f0`), and store
`(x ^ 0xf1ceb9b2a03ed26b) + 0xcd043be4c3e77ef4`, where `x` mixes
`DAT_143848313`, `[[0x143b79598]+0x7d]` and the decoded value through a further
rotate. The attach array is therefore **process-encoded**: its contents are not
statically reconstructible; `+0x230` is the count.

Call sites `0x1405556e8`-`0x1405559a4` (21 calls, read from dev/engine_dump.asm):

- 20 calls whose argument is `[engine+SLOT] + 0x10` (the slot's interface
  sub-object), skipped when the slot is null, in call order:
  `0x368, 0x288, 0x2e8, 0x2d8, 0x300, 0x2e0, 0x270, 0x238, 0x3b0, 0x250,
  0x240, 0x290, 0x248, 0x398, 0x3e0, 0x3e8, 0x3f0, 0x3f8, 0x400, 0x410`.
- 1 gated call (`0x14055599f`, guarded by the byte test at `0x140555983`) with
  `[engine+0x268] + 0x10`.

## 3. Named functions in docs/function-names.csv

Only three rows describe the engine/subsystem machinery, plus the allocator the
constructor uses:

| name | address | role | csv line |
|---|---|---|---|
| `RunMapLoadEntry` | `0x14055a0f0` | map-load entry: registers `debug.showMapLoadAfterMS` (default `0xea60`), inits the node class table (`0x141c57c40`, `0x141c57970`), calls `ConstructEngineSubsystems 0x140554220`, then builds the `0x910` WorldBase + `0x2ad0` camera-view child via `0x140be1570` | docs/function-names.csv:837 |
| `ConstructEngineSubsystems` | `0x140554220` | allocates ~40 subsystems into `_DAT_143b7f410` at `0x220..0x448`, registers each through `0x141c52d10`, then attaches the global singletons `[engine+0x238..+0x410]` through `0x140bc7590`; one `E8` caller | docs/function-names.csv:844 |
| `RegisterWorldInterface` | `0x141c52d10` | appends to `+0x188`, sets `subsystem[1] = engine`, files the interface chain `[interface+0x90]` in the `+0x198` hash map | docs/function-names.csv:845 |
| `Alloc` | `0x141baff30` | size-and-flags allocator used by every constructor block (`ecx=size`, `edx=0`); flags bit 0 align 16, bit 1 minimum size; Ghidra treats it as noreturn | docs/function-names.csv:802 |

Decompiled in `dev/decompiles/` but **not** named in the csv: `0x141c553a0`
(interface-map allocate/rehash), `0x141c56fb0` (file interfaces only),
`0x140767da0` (`+0x188` array grow), `0x140bc7590` (attach), `0x140bc76f0`
(attach array grow).

## 4. Proven vs open

**Proven (read, with a reproducing artifact):**

- the object is at `0x143b7f410`, is `0x470` bytes, built by `0x1405838c0`,
  filled by `0x140554220` (docs/formats/static-collision.md:2619-2628;
  docs/function-names.csv:837, 844; MODLOG.md:138-145).
- the three container headers `+0x188`/`+0x198`/`+0x228` with their counts and
  capacities, the encoded-pointer arithmetic, the 65 slots and the 21 attach
  calls (dev/decompiles/engine_*.c, dev/eng_table.txt,
  dev/engine_subsystems.raw.csv, dev/engine_dump.asm:1381-1532).
- `+0x468` is a CVar value copied in the bootstrap (section 1c).

**Open — quoted:**

- docs/open-questions.md:1318-1320: "The service pointer is runtime-encoded
  (`0x14054a010(context)`), so the poller has to be found from the consumer
  side. This is the anchor, not a result. … `0x140554220` is the engine
  subsystem constructor, not the map load."
- docs/formats/static-collision.md:2658-2660: "Consequence: the 'creation path
  runs through obfuscation' argument is withdrawn, and `0x140554220` is not the
  anchor. The next anchor is the preload service's `vtable+0x38`."
- docs/formats/static-collision.md:2865: "`0x140554220` is the engine
  [subsystem constructor]" — recorded as a correction, not a subsystem census.

**Unknown (nothing in the repo):** the meaning of `+0x000..+0x187`,
`+0x1c0..+0x21f`, `+0x46c..+0x46f`; the class and role of all 65 subsystems
(dev/engine_subsystems.raw.csv still says `unknown` in every `ctor_or_source`
row); which of the 5487 RIP references read which offset (the census has been
run but is not written into docs); the 15-19 objects that are registered but
never stored (`alloc_calls=80 rwi_calls=84`, 65 stored slots) are not
individually accounted for. `grep -rn -i "engine|subsystem|singleton"`
returns nothing for this object in docs/struct-layouts.csv, docs/glossary.md or
docs/systems-coverage.md (the only hit, docs/systems-coverage.md:46, is the
renderer).

## 5. Existing Rust code and legacy debt

- **No Rust code models the engine object.** `grep -rn "143b7f410\|subsystem"
  crates/` finds nothing. The only `engine` hits are the renderer's own
  vocabulary for the GPU command stream and the swap-chain view:
  `crates/ow-render/src/batch.rs:1-3` ("The engine's per-frame command
  records"), `crates/ow-render/src/request.rs:136-147`
  (`DEFAULT_ENGINE_FORMAT`, the view's format byte), `crates/ow-render/src/d3d11.rs:212-239`.
- `docs/deviations.md` lists **no engine/subsystem `FIT-*` row**
  (docs/deviations.md:20-37: `FIT-ANI-02/04/07`, `FIT-CAM-03/05/06`,
  `FIT-PRE-01/02` only). There is no legacy engine-object code to delete; the
  object is simply unimplemented, so `AGENTS.md` rule 8 applies (an unread stage
  is *open*, not faked, and no new `FIT-*` marker may be added).
- `crates/ow-sim` is the parked gameplay layer (AGENTS.md tooling map) and the
  natural home for a later engine-layer model, but nothing in it touches this
  object today.

## 6. How the docs say a new subsystem/object is tested

docs/architecture.md:63-79, "Behaviour pipeline":

> 4. **Implement it** operation for operation in a verified layer; `ow-sim`
>    stays parked until its movement layer is read and verified.
> 5. **Prove it.** A pure function gets a golden vector; a stateful path gets a
>    first-divergence test; a reconstructed client function gets a
>    `dev/verify_harness/` unit.

- The gate is `cargo xtask verify` (fmt, clippy with warnings as errors, tests,
  tool index, deviation check, address/function-entry check); `--fast` skips the
  install-reading tests (docs/architecture.md:90-103; AGENTS.md rule 10).
- Differential harness: `python dev/verify_harness/cli.py all` / `cli.py
  fn/<Unit>` (dev/verify_harness/cli.py:1-13). Units are
  `dev/verify_harness/units/fn-<Name>.json`, with an optional Rust candidate
  variant (`fn-AnimTimeWrap.json` / `fn-AnimTimeWrap-rust.json`); each unit
  pairs a candidate implementation against the client's bytes and runs mutation
  probes.
- Golden-vector / first-divergence precedent: docs/formats/domino.md:599 ("a
  vector and a multi-step run gets a first-divergence test"); the `*_real` tests
  read the operator's install and self-skip when absent, e.g.
  `crates/ow-tank/tests/texture_real.rs`.
- **There is no `#[repr(C)]` layout test for a game object anywhere in
  `crates/`** (`grep -rn "offset_of!\|size_of::<" crates` returns nothing); the
  only `#[repr(C)]` types are GPU/FFI structs
  (`crates/ow-scene/src/lib.rs:8,283`, `crates/ow-client/src/main.rs:39`). The
  layout convention in practice is `docs/struct-layouts.csv`
  (`struct,offset,name,type,role`, e.g. `MoverBody,0x870,state_modifier_data,void *`)
  plus per-field citations in `docs/formats/<system>.md`.
- Because the constructor is a stateful path over encoded pointers, the honest
  proof shape is a **first-divergence test on container state** (`+0x188` count
  and order, `+0x198` bucket/entry/value triples, `+0x228` count). The attach
  array additionally needs the process key (`DAT_143848313`,
  `[[0x143b79598]+0x7d]`), so its exact values can only be checked against a
  live process, not a static golden vector.

## 7. What a single implementation step can start from

1. Model the container headers as read: `+0x188 [ptr, u32 count, u32 cap]`,
   `+0x198 [buckets, entries, values, i32 cap, i32 count, i32 free_head,
   i32 free_n]`, `+0x228 [ptr, u32 count, u32 cap]`, plus the 65 slot offsets
   (table in 2a).
2. Port `RegisterWorldInterface 0x141c52d10` operation for operation: the two
   encodes (`ror 0x31 (p ^ 0x238e31e8cd062f58) + 0x34b5f414245bf6ef`, and
   `(engine ^ 0xd6c0ac78d0e44481) + 0x9dd46acd84d192e2`), the growth rule
   (`count+1 <= cap*2>>1`, `new = cap + cap/4`), the interface walk
   (`vtable[0](sub)`, then `+0x90`, stop at null / `0x14417fbe0`) and the map
   geometry (`(cap*4+7)&~7` + `cap*0x10` + `cap*8`, hash `(ptr>>4)&0x7fffffff`).
3. Leave `+0x000..+0x187`, the 65 subsystem classes and the attach decode
   **open**; they are not read, and AGENTS.md rule 8 forbids a placeholder.
4. Test with `cargo xtask verify --fast`, a `dev/verify_harness/units/` unit for
   the registration and rehash arithmetic, and first-divergence on container
   state for a two-subsystem sequence.

## 8. Addendum — counts, vtables and traps from `dev/engine_subsystems.md`

That file (165 lines) is the census behind `dev/engine_subsystems.raw.csv`; its
method is documented at dev/engine_subsystems.md:5-38 (flat-image linear sweep
`dev/engine_dump.asm`, block segmentation on `call 0x141c52d10`, slot
detection requires `mov qword ptr [ENGINE+DISP],REG` with DISP in `0x220..0x448`,
engine base `rbx = rdx` at `0x140554234`).

### 8a. Counts, reconciled (dev/engine_subsystems.md:41-66)

| quantity | count | detail |
|---|---|---|
| `call 0x141baff30` (`Alloc(size,0)`) | 80 | first `0x140554241`, last `0x1405556b2` |
| `call 0x141c52d10` (`RegisterWorldInterface`) | 84 | first `0x14055426d`, last `0x1405556e3` |
| stores into the engine object, `0x220..0x448` | 65 | first `0x1405542c9` (`[rbx+0x408]`), last `0x1405556d9` (`[rbx+0x438]`) |
| `call 0x140bc7590` (tail attach pass) | 21 | `0x140555704..0x14055599f` |

- 84 RWI = 64 blocks that store a slot + 20 register-only blocks; one block
  (`0x140554ac1..0x140554ae3`) stores **two** slots (`0x340`, `0x358`) → 65
  stores (dev/engine_subsystems.md:47-50).
- The 20 register-only blocks: `0x140554220, 0x140554272, 0x1405542d8,
  0x140554301` (no allocation at all: anti-tamper/decoy), `0x140554ae8,
  0x140554b3f, 0x140554b68, 0x140554b94, 0x140554da0, 0x140554dd6, 0x140554e0c,
  0x140554e42, 0x140554ed8, 0x140554f20, 0x140554f9c, 0x140555208, 0x1405552e0,
  0x14055553d, 0x14055558a, 0x1405555c0` (dev/engine_subsystems.md:51-56).
- 80 Alloc = 61 stored into their own slot + 1 reaching `0x448` as an adjustor
  sub-object + 18 registered then dropped (17 of the above plus the `0x980`
  allocation inside the `0x220` block) (dev/engine_subsystems.md:57-60).
- 65 slots = 61 inline `Alloc`+ctor + 4 non-`Alloc` sources (`0x220` decoded
  pointer, `0x340`/`0x358` factory returns, `0x448` adjustor)
  (dev/engine_subsystems.md:61-63).
- Per-slot `call 0x141c52d10` totals only 58: seven slots have no registration
  call at all (dev/engine_subsystems.md:143-147). Any "subsystem count" taken
  from the 84 RWI calls or the 80 Alloc calls overstates the table — only the 65
  stores are members (dev/engine_subsystems.md:105-109).
- Never-written offsets in `0x220..0x448`: `0x228`, `0x230`, `0x350`, `0x418`,
  `0x440` (no literal operand anywhere in the 1532-instruction body, and no
  indexed store to cover them); `65 + 5 = 70` = the full stride-8 window
  (dev/engine_subsystems.md:70-76). Verified again for this brief:
  `grep "\[rbx + 0x228\]\|\[rbx + 0x230\]\|\[rbx + 0x234\]" dev/engine_dump.asm`
  returns nothing.

### 8b. Values (dev/engine_subsystems.md:80-98)

- Alloc sizes, 21 distinct across the 80 sites: `0x10, 0x18, 0x20, 0x28, 0x30,
  0x38, 0x40, 0x58, 0x68, 0x70, 0x88, 0x90, 0xb0, 0xc0, 0xd0, 0x100, 0x1b8,
  0x2f8, 0x390, 0x7d0, 0x980`; `0x20` and `0x980` occur only in register-only
  allocations.
- 32 distinct primary vtables, all unique (no slot shares a primary vtable):
  `0x142f7e0f0, 0x142f7e210, 0x142f7e300, 0x142f7e388, 0x142f7e638,
  0x142f7e758, 0x142f7ea90, 0x142f7eb08, 0x142f7eb90, 0x142f7ec18,
  0x142f7eca0, 0x142f7ee30, 0x142f7ef88, 0x142f7f1d0, 0x142f7f298,
  0x142f7f320, 0x142f7f398, 0x142f7f410, 0x142f7f5c8, 0x142f7f640,
  0x142f7f6b8, 0x142f7f730, 0x142f7f7a8, 0x142f7f820, 0x142f7f898,
  0x142f7fc18, 0x142f7fc90, 0x142f80088, 0x142f80b38, 0x142f80bb0,
  0x142f80c28, 0x142f80ca0`.
- 13 slots carry secondary vtables at `+0x10`, 15 extra stores: `0x142f7e168`
  (twice), `0x142f7e400, 0x142f7e620, 0x142f7e6c8, 0x142f7eb80, 0x142f7ec08,
  0x142f7ec90, 0x142f7ed18, 0x142f7f000, 0x142f7f488, 0x142f7fd08`, plus
  `0x142f7e6f8 / 0x142f7e728 / 0x142f7e740` for the three extra bases of slot
  `0x278`. 47 distinct vtable values are written inline in total.
- `0x3f8` and `0x420` have different primary vtables but the identical secondary
  vtable `+0x10 = 0x142f7e168` (dev/engine_subsystems.md:127-128).

### 8c. Behaviour worth knowing before modelling (dev/engine_subsystems.md:100-133)

- **The base ctor `0x141c578c0` is a 28-byte stub with no Ghidra function:**
  it installs vtable `0x142f7d778` and an 8-byte cookie `0x7495174655b5d763` at
  `+0x08`, and returns `rcx`. All 31 callers overwrite `[obj]` with their own
  vtable, so `0x142f7d778` never survives construction.
- **Slot `0x220` is a decoded pointer, not an object:** `[engine+0x330]+0x60` is
  passed by address to `0x1405437f0` (an in-place pointer decoder: `mov rax,[rcx]
  / ror rax,0xf / xor-sub-ror / mov [rcx],rax / ret`), and the result goes
  through a second xor/add/ror chain at `0x14055532a..0x14055536a` before the
  store.
- **Slot `0x330` is read back inside the same function** (`0x140555309`) to feed
  `0x220`: the constructor depends on its own earlier store.
- **Slot `0x448` is an adjustor result** `rdi + 0x10 + (i32)[[rdi+0x10]+4]` where
  `rdi` is the `0x68`-byte object built at `0x140554b6f` (ctor `0x140723890`) and
  registered at `0x140554b8f` — slot and registration share one object.
- **One block is pure anti-tamper:** `0x140554301..0x140554418` xors a
  context-object cookie (`[rsi+8]`) with `[0x143b7f598 + 0x1b2]`, compares it
  against `0x7a4f79f62f45d148`, calls four junk targets (`0x1405726e6,
  0x1405435a0, 0x14056e66e, 0x1405554bd`) and finally
  `RegisterWorldInterface` with a garbage `rdx`. The decompiler's "unreachable
  blocks" are this trap, not control flow. Note `0x143b7f598` lies **inside**
  the engine span (`+0x188`.. as an address) — flagged, unexplained.
- **Slot `0x308` is the only slot whose dedicated ctor vtable is overwritten
  inline** (ctor `0x140bdf740`, override at `0x1405546b7`).
- **Global-sourced slot:** `0x1405554a6` loads `[0x143b8b4b0]` and `0x1405554ba`
  stores it to `[rdi+0x3b8]` — a pre-existing singleton, so that row has empty
  `alloc_size` / `vtable` cells.
- The 21 tail attach calls are **not** stores: each is
  `mov rcx,[engine+SLOT] / lea rdx,[rcx+0x10] / cmove rdx,0 / call 0x140bc7590`,
  i.e. it re-attaches the slot's **secondary base** at `+0x10`; the `0x268` one
  is guarded by `cmp qword ptr [rsi+0xe0],0` at `0x14055597c`
  (dev/engine_subsystems.md:64-66).
- **Residual risk (dev/engine_subsystems.md:158-164):** for `0x340`/`0x358` the
  stored `rax` comes out of `0x14074a800` (called at `0x140554ac1`/`0x140554acd`)
  and is not materialised by any `Alloc` in this function — the source is known,
  the allocation size is not; it must be read inside `0x14074a800`.

## 9. One contradiction, resolved

`dev/engine_subsystems.md:153-155` claims slot `0x228` aliases `0x220`
("`0x1405542e8` loads `[rdi+0x220]` and `0x1405542f2` stores the same pointer
into `[rdi+0x228]`"). **That is a false positive**: in `dev/engine_dump.asm`
`0x1405542e8` does not exist — `0x1405542e7` is `je 0x1405542f3` and
`0x1405542e9` is `mov rcx,rax`, so the cited addresses fall inside instructions
starting at `0x1405542d8` (`xor edx,edx` / `mov ecx,0x58` / `call 0x141baff30` /
`test rax,rax`). It also contradicts the same file's line 70-76 ("never written
in this function"). The attach header therefore stands as section 1b says:
`+0x228 [ptr, u32 count, u32 cap]`, written only by `0x140bc7590` /
`0x140bc76f0`, and `0x228` is *not* a subsystem slot.
