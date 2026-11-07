# ConstructEngineSubsystems (0x140554220) — engine subsystem table

Read-only extraction of the subsystem slots of the engine object (pointer arrives
in RDX; RCX is an unrelated context pointer). Deliverable table:
`dev/engine_subsystems.csv` (65 rows, sorted by slot offset).

## Method

1. **Flat image disassembly, linear sweep, whole body.**
   `python dev/eng_dump.py 0x140554220 0x1405559b9 dev/engine_dump.asm` ->
   `dev/engine_dump.asm` (capstone over
   `dev/data/offline_recover/module_offline.bin`, mapping offset = VA -
   0x140000000). 1531 instructions, 0x140554220..0x1405559b8 (`ret` at
   0x1405559b8; 0x1405559b9 is int3). The sweep decodes cleanly end to end
   (only `.byte` lines inside the obfuscated block at
   0x140554301..0x140554418), so there are no alignment holes
   where a store could hide. `dev/eng_verify_schedule.py` bounds its walk to
   the same `[0x140554220, 0x1405559b9)` range — a wider dump includes the
   next function's first `Alloc` and reports 81.
2. **Block segmentation.** Every subsystem construction terminates in
   `call 0x141c52d10` (`RegisterWorldInterface`). The 84 such calls cut the body
   into 84 straight-line blocks; each block was read as
   `Alloc(size, 0) -> [ctor] -> store into engine -> RegisterWorldInterface`.
3. **Slot detection.** A row exists only if the instruction stream contains
   `mov qword ptr [ENGINE + DISP], REG` with DISP in 0x220..0x448. The engine
   base register is `rbx` (`mov rbx, rdx` at 0x140554234) or, in three blocks, a
   fresh `mov REG, qword ptr [rip + 0x3625290..]` reload of the engine global
   0x143b7f410.
4. **Producer of each stored pointer** was taken from the block itself: the
   `mov ecx, SIZE` / `call 0x141baff30` pair, the constructor call that receives
   the allocation in RCX, and (where the class is a base-ctor-only stub) the
   `lea REG,[rip+X]` + `mov [OBJ],REG` vtable store in the same block.
5. The pre-existing decompile dump `dev/decompiles/engine_ConstructEngineSubsystems_0x140554220.c`
   was used only as a cross-check on the *reachable* sequence and on slot offsets
   (its "unreachable block" fragments were ignored); its offsets agree
   (`param_2 + 0x102` = 0x408 etc.).
6. **The table is now a decode, not a measurement.**
   `dev/eng_verify_schedule.py` re-derives every row from the instruction stream
   (object tokens, the constructor or factory that ran on each, the in-place
   vtable stores, the slot store and the `RegisterWorldInterface` call whose RDX
   carries the same token) and diffs it against `dev/engine_subsystems.csv`;
   `dev/eng_gen_subsystems.py` writes `crates/ow-engine/src/subsystems.rs` from
   the verified result. The offline bytes were also compared with the Ghidra
   project (`Overwatch.exe`) at the entry, the first store, the stub, the
   adjustor's `Alloc` and the decode site: identical.

## Counts, reconciled

| quantity | count | detail |
|---|---|---|
| `call 0x141baff30` (Alloc(size,0)) | **80** | first 0x140554241, last 0x1405556b2 |
| `call 0x141c52d10` (RegisterWorldInterface) | **84** | first 0x14055426d, last 0x1405556e3 |
| stores into the engine object, 0x220..0x448 | **65** | first 0x1405542c9 (`[rbx+0x408]`), last 0x1405556d9 (`[rbx+0x438]`) |
| `call 0x140bc7590` (tail attach pass) | **21** | 0x140555704..0x14055599f |

Reconciliation:

* 84 RWI calls = **64 blocks that store a slot** + **20 blocks that register an
  object without storing it**.
  * of the 64, one block (0x140554ac1..0x140554ae3) stores **two** slots
    (0x340, 0x358) -> 65 stores.
  * the 20 register-only blocks: 0x140554220, 0x140554272, 0x1405542d8,
    0x140554301 (no allocation at all: anti-tamper/decoy), 0x140554ae8,
    0x140554b3f, 0x140554b68, 0x140554b94, 0x140554da0, 0x140554dd6,
    0x140554e0c, 0x140554e42, 0x140554ed8, 0x140554f20, 0x140554f9c,
    0x140555208, 0x1405552e0, 0x14055553d, 0x14055558a, 0x1405555c0.
* 80 Alloc calls = **61** whose result is stored into its own slot
  + **1** whose result reaches slot 0x448 as an adjustor-computed subobject
  + **18** whose object is registered and then dropped (17 of the blocks above
  plus the 0x980 allocation inside the 0x220 block).
* 65 slots = 61 inline `Alloc`+ctor + 4 non-`Alloc` sources
  (0x220 decoded pointer, 0x340 and 0x358 factory returns, 0x448 adjustor).
* The 21 tail attach calls are **not** stores: for each of the 21 slots listed in
  the CSV notes the tail does
  `mov rcx,[engine+SLOT]; lea rdx,[rcx+0x10]; cmove rdx,0; call 0x140bc7590`
  — i.e. it re-attaches the object's **secondary base** (+0x10). The 0x268 one is
  guarded by `cmp qword ptr [rsi+0xe0], 0` at 0x14055597c.

## Slots not resolved

Five offsets in 0x220..0x448 are **never written in this function**:
`0x228`, `0x230`, `0x350`, `0x418`, `0x440`. The literals do not occur anywhere
in the 1532-instruction body (checked by scanning every instruction operand, not
just `mov` stores), and there is no indexed store (`[rbx + REG*8 + disp]`) that
could cover them. The decompile dump contains no write to them either. They are
therefore left out of the CSV rather than filled with a guess: either they are
not subsystem slots (plain fields) or another function owns them. 65 + 5 = 70 =
the full 0x220..0x448 stride-8 window.

## Distinct values

* **Alloc sizes (21 distinct, all 80 sites):**
  0x10, 0x18, 0x20, 0x28, 0x30, 0x38, 0x40, 0x58, 0x68, 0x70, 0x88, 0x90,
  0xb0, 0xc0, 0xd0, 0x100, 0x1b8, 0x2f8, 0x390, 0x7d0, 0x980.
  (0x980 and 0x20 appear only in register-only allocations.)
* **Primary vtable stores (`mov [obj], REG` with REG from `lea`): 44 stores,
  44 distinct values** — every primary vtable in this function is unique. The
  32 slots whose CSV `vtable` cell is filled are the ones whose primary vtable
  is written in the same block as the slot store; the other 12 belong to
  objects that are constructed and registered but never reach a slot.
* **Secondary vtables at `+0x10`: 15 stores, 14 distinct values** —
  0x142f7e168 (twice: 0x14055515d for slot 0x3f8 and 0x140555616 for slot
  0x420, i.e. the same class), 0x142f7e1f0, 0x142f7e378, 0x142f7e400,
  0x142f7e620, 0x142f7e6c8, 0x142f7eb80, 0x142f7ec08, 0x142f7ec90,
  0x142f7ed18, 0x142f7f000, 0x142f7f488, 0x142f7f510, 0x142f7fd08.
  Slot 0x278 additionally carries three further bases (0x142f7e6f8,
  0x142f7e728, 0x142f7e740) at higher offsets of its object.
* **Total: 59 inline vtable stores, 58 distinct vtable values** (44 primary +
  15 secondary), all of them `lea`-of-a-0x142f…-constant targets inside this
  body. Every value in the CSV `vtable` column was matched back to a real
  `lea rip` target in the function (32 of 32).

## What was surprising

* **The 31 "base constructor only" slots all share one stub, and Ghidra has no
  function at it.** `0x141c578c0` (28 bytes, "Function not found" in the project)
  is `lea rax,[rip+0x1325eb1]; mov [rcx],rax; movabs rax,0x7495174655b5d763;
  mov [rcx+8],rax; mov rax,rcx; ret` — it installs vtable 0x142f7d778 and an
  8-byte cookie at +8. Every one of the 31 callers immediately overwrites
  `[obj]` with its own vtable, so 0x142f7d778 never survives construction.
* **20 of the 84 constructions never reach an engine slot.** They allocate,
  construct, set a vtable and register the object, then drop the pointer. Any
  count of "subsystems" taken from the RWI call count (84) or the Alloc count (80)
  overstates the table; only the 65 stores are members.
* **Slot 0x220 is not a constructor product.** `([engine+0x330]+0x60)` is passed
  by address to `0x1405437f0` (also not a Ghidra function: `mov rax,[rcx]; ror
  rax,0xf; xor/sub/ror; mov [rcx],rax; ret` — an in-place pointer decoder), and
  the decoded value is then run through a second xor/add/ror chain
  (0x14055532a..0x14055536a) before being stored. So 0x220 holds a *decoded*
  pointer derived from the object in slot 0x330, not a fresh allocation.
* **Slot 0x448 is an adjustor result, not an object.** It is
  `rdi + 0x10 + (int32)[[rdi+0x10]+4]` where rdi is the 0x68-byte object built at
  0x140554b6f (ctor 0x140723890) and registered at 0x140554b8f. Slot 0x448 and
  that registration therefore share one object.
* **One block is pure anti-tamper.** 0x140554301..0x140554418 contains no
  allocation and no slot store: it xors a context-object cookie (`[rsi+8]`,
  where rsi = the RCX argument) with `[0x143b79598 + 0x1b2]` and compares it
  against 0x7a4f79f62f45d148, then calls four junk targets
  (0x1405726e6, 0x1405435a0, 0x14056e66e, 0x1405554bd) and finally
  `RegisterWorldInterface` with a garbage RDX. Its decompiler "unreachable
  blocks" are this trap, not real control flow.
* **Two slots are the same class.** 0x3f8 and 0x420 have different primary
  vtables but the identical secondary vtable +0x10 = 0x142f7e168.
* **Slot 0x308 is the only slot whose dedicated constructor's vtable is then
  overwritten inline** (ctor 0x140bdf740, override at 0x1405546b7) — a
  class-with-base pattern that does not occur elsewhere in this function.
* Slot 0x330 is read back inside the same function (0x140555309) to feed 0x220,
  i.e. the function depends on its own earlier store.
## Independent re-check of the table

The CSV was re-derived a second time, by a different route, straight from the
flat-image dump (`dev/engine_dump.asm`, 1532 instructions, decoded with
capstone from 0x140554220):

* Every instruction matching `mov [REG + DISP], REG` with DISP in
  0x220..0x448 was collected: **65 hits, one per slot, no slot written twice,
  no slot outside the CSV.** The 65 `store_insn` values match the CSV column
  exactly (the dump prints `000140555372`, the CSV `0x140555372`; 0 mismatches
  after normalising the prefix).
* For each of the 65 stores the stored register was walked back up to 40
  instructions to find its producer. **No store is fed by a rip-relative
  global load** (0 hits). So no row can be `global:<address>`: every slot in
  0x220..0x448 is produced by an `Alloc`+ctor, by a factory call, by the 0x220
  decoder chain, or by the 0x448 adjustor.

### The globals that do appear, and why they are not slot sources

The body has 32 non-`lea` rip-relative memory references. They resolve to
exactly three globals (targets computed with capstone, insn size + disp):

* **0x143b7f410** — the engine pointer itself, re-read 28 times
  (0x1405547dd, 0x1405547ee, 0x140554b22, 0x140554b33, 0x140554d8d,
  0x140554f90, 0x1405553c3, 0x1405553f7, and 0x1405556e8..0x140555985).
  Three blocks use it to reach the engine object instead of `rbx`, and the
  whole tail attach pass (21 sites) reloads it once per call.
* **0x143b79598** — read once at 0x140554301, the anti-tamper block's cookie
  global (`+0x1b2` is the byte compared against the context cookie).
* **0x143b796a8** — read twice (0x140554bc4, 0x140555282) as the **RCX
  argument** to the helper at 0x14054acf0, which receives the engine object in
  RDX. It is passed to a helper, never stored into a slot.

### Note on the brief's "~25 pre-existing global singletons"

This function contains no such attachment. What the brief describes is almost
certainly the tail pass at 0x1405556e8..0x14055599f: 21 sites that
`mov rax,[0x143b7f410]` -> `mov rcx,[rax + SLOT]` -> `call 0x140bc7590`.
Those reads go *through* the engine global to reach a slot that this same
function filled a few hundred instructions earlier; they re-attach the
object's secondary base (+0x10) and introduce no new subsystem object. The
table therefore has 65 members, all allocated or derived inside this function.

## Residual risk / limits of this extraction

* **Four rows have an empty `alloc_size`** — 0x220, 0x340, 0x358, 0x448 —
  because their producer is not an inline `Alloc` at this call site: 0x340
  (0x140594090) and 0x358 (0x1406cc990) are factory calls that allocate
  inside themselves; 0x220 is a decoded pointer; 0x448 is an adjustor result
  over the 0x68-byte object built at 0x140554b6f. Reading their true sizes
  means reading those callees, which is outside this function.
* **Five offsets in the window are never written** (0x228, 0x230, 0x350,
  0x418, 0x440) and are therefore absent from the CSV, not guessed.
* **The Ghidra bridge was not used for the instruction stream.** It answered
  only name/size queries; every address in the CSV comes from capstone over
  the local flat image, so nothing here depends on the decompiler's junk
  control flow.
