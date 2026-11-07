# The 35 subsystem classes: constructors and installed vtables

`dev/eng_class_profile.py` reads these from the image; this file is the record
of one run, so a later session can look up "which constructor is which slot"
without re-deriving. It names nothing: a class is identified here by its
constructor address, because none of the 117 vtables has been tied to a slot's
own 15-entry table yet (see "What is open").

- producer: `dev/engine_classes.json` (local, not tracked).
- generator: `dev/eng_gen_classes.py` → `crates/ow-engine/src/classes.rs`.
- test: `crates/ow-engine/tests/class_layout.rs` (8 tests over the table).
- readable summary: `docs/formats/engine.md`, "The constructors and their
  vtables".

## How a row is read

The constructor's own body is swept (`dev/eng_class_profile.py`), bounded by its
`.pdata` range — or by its terminating instruction for the two leaf functions
`.pdata` omits (the stub `0x141c578c0` and the `+0x220` decoder `0x1405437f0`).
`this` is tracked in RCX and its copies (a `call` drops the volatile copies).
`lea reg,[rip+X]` followed by `mov [this+off],reg` is a vtable store; a
`mov [this+off],imm` is a field initialiser. A repeated offset is real: the
constructor writes a base table and then its own over the top.

`base` = calls the shared stub `0x141c578c0` (or is it). `own` = does not.

## The table

| constructor | bytes | base | slot | vtable stores (`+offset` = table) |
|---|---|---|---|---|
| `0x1405437f0` | 41 | own | 0x220 | - (decoder, not a constructor) |
| `0x14055cc60` | 323 | base | 0x248 | +0x0=`0x142f80f48` +0x10=`0x142f80fc0` |
| `0x140594090` | 38 | own | 0x340 | - (factory) |
| `0x140598900` | 83 | base | 0x2f8 | +0x0=`0x142f84270` |
| `0x1405bc5c0` | 44 | base | 0x288 | +0x0=`0x142f84920` +0x10=`0x142f84998` |
| `0x1405cf4f0` | 33 | base | 0x3a8 | +0x0=`0x142f84c70` |
| `0x1405dac00` | 178 | base | 0x268 | +0x0=`0x142f84dd0` +0x10=`0x142f84e48` +0x20=`0x142f84dc0` |
| `0x14060fd30` | 113 | base | 0x290 | +0x0=`0x142f85dd8` +0x10=`0x142f85e50` |
| `0x14061c0a0` | 69 | base | 0x368 | +0x0=`0x142f860f0` +0x10=`0x142f86168` +0x20=`0x142f86168` +0x30=`0x142f86168` |
| `0x14061c4c0` | 83 | base | 0x3e0 | +0x0=`0x142f86178` +0x10=`0x142f861f0` |
| `0x14061d1e0` | 44 | base | 0x3a0 | +0x0=`0x142f86208` +0x10=`0x142f86280` |
| `0x14064f080` | 135 | own | 0x238 | +0x0=`0x142f87588` +0x10=`0x142f87750` +0x768=`0x142f87760` |
| `0x140651630` | 85 | own | 0x2e8 | +0x0=`0x142f87810` +0x10=`0x142f87948` +0x2e0=`0x142f87958` |
| `0x140657b40` | 133 | base | 0x310 | +0x0=`0x142f87ba8` |
| `0x140662760` | 459 | base | 0x408 | +0x0=`0x142f88188` +0x10=`0x142f88200` |
| `0x14066b100` | 663 | base | 0x360 | +0x0=`0x142f882f0` +0x10=`0x142f88368` |
| `0x1406735c0` | 58 | base | 0x260 | +0x0= +0x10= +0x18= +0x20= +0x28= +0x38=`0x142f88700` (one table, six sub-objects) |
| `0x1406928b0` | 47 | base | 0x320 | +0x0=`0x142f88fa0` |
| `0x1406a4de0` | 162 | base | 0x2e0 | +0x0=`0x142f89078` +0x10=`0x142f890f0` +0x18=`0x142f89068` |
| `0x1406b4570` | 63 | base | 0x370 | +0x0=`0x142f89468` |
| `0x1406b5000` | 44 | base | 0x2d8 | +0x0=`0x142f894e0` +0x10=`0x142f89558` |
| `0x1406b58d0` | 41 | base | 0x2b0 | +0x0=`0x142f89570` |
| `0x1406c0580` | 104 | base | 0x240 | +0x0=`0x142f89990` +0x10=`0x142f89a08` |
| `0x1406c5380` | 33 | base | 0x378 | +0x0=`0x142f89ec8` |
| `0x1406c6d10` | 44 | base | 0x3b0 | +0x0=`0x142f89fb8` +0x10=`0x142f8a030` |
| `0x1406cc990` | 38 | own | 0x358 | - (factory) |
| `0x140723890` | 217 | base | 0x448 | +0x0=`0x142f8a728`→`0x142f8aa28`, +0x10=`0x142f8ab78`, +0x18=`0x142f7d758`→`0x142f8aaa0`, +0x20=`0x142f8aac0`, +0x60=`0x142f8a710` |
| `0x14072c560` | 924 | base | 0x2f0 | +0x0=`0x142f8ad80`; base+derived at +0x10, +0x90, +0x100, +0x158, +0x290, +0x2e0 (`0x142f8ac68` then `0x142f8ac90`/`0x142f8acb8`/`0x142f8ace0`/`0x142f8ad08`/`0x142f8ad30`/`0x142f8ad58`), plus `0x142f8ad08` at +0x1a8, +0x1d0, +0x1f8 |
| `0x14073fc00` | 51 | own | 0x258 | `0x142f8bc88` at +0x0, +0x50, +0x58, +0x60, +0x68 |
| `0x140741560` | 33 | base | 0x380 | +0x0=`0x142f8be90` |
| `0x1407493c0` | 33 | base | 0x2d0 | +0x0=`0x142f8c088` |
| `0x14074a300` | 164 | base | 0x338 | +0x0=`0x142f8c100`→`0x142f8c388`, +0x10=`0x142f8c190`, +0x20=+0x40=+0xa8=`0x142f8c418` |
| `0x14074c5a0` | 66 | own | 0x330 | +0x0=`0x142f8c508` +0x10=`0x142f8c5c0` +0x58=`0x142f7d8d0` +0x60=`0x142f8c640` |
| `0x140bdf740` | 51 | base | 0x308 | `0x1430a2910` at +0x0, +0x10, +0x18, +0x20, +0x28 |
| `0x141c578c0` | 28 | base | 31 slots | +0x0=`0x142f7d778` (overwritten by the block in every slot) |

`0x14072c560` was cross-checked against Ghidra's decompilation of the same
function: same six sub-objects, same base-then-derived order, same final state.

## The 117 tables

- 96 installs, 70 distinct constructor-installed tables, 47 distinct
  block-stored tables, disjoint sets (70 + 47 = 117).
- Of the 117, 44 have exactly 15 code entries; 19 have 2; 7 have 5.
- The read stops for three reasons: 85 at the next known table, 30 at a
  non-code word, 2 at the 128-entry cap (those two lengths are **not read**:
  `0x142f80088` and `0x142f7d8d0`).
- The most common entry is `0x14053a420` — three bytes, `c2 00 00` = `ret 0` —
  with **1,060** occurrences across the 117 tables; next are the static
  accessors `0x14055dfe0` (102) and `0x14053a070`/`0x14054a160` (97 each). Most
  of a 15-entry interface is the same empty `ret`, which is why 15 is the
  normal size.
- The non-code words are not sentinels: they are strings and float pairs in
  `.rdata` (`"SteamUse"`, `"advapi32"`, `"%s (2s)"`, `65535.0`/`500000.0`), so
  tables and data are interleaved and a table's end is only known when both
  sides are known.
- Four tables contain a curated name (offsets are from the table's start):
  `0x142f7f488+0xd8` `RegisterLocalHitEffectsSettings`, `0x142f7f898+0x270`
  `TickSequenceCameraView`, `0x142f87958+0x108` `FirstPersonCommandUpdateEntry`,
  `0x142f89570+0xf0` `SetCameraFov`. None of the four is in its slot's own
  first 15 entries, so none names a slot's class yet.

## What is open

1. **Every class name.** The unit that can be named is a 15-entry block: the
   table a slot's block or constructor installs, first 15 entries. A name needs
   a method inside that block calling a known subsystem API, or a consumer that
   identifies the subsystem. No slot has either yet.
2. **The three objects whose `+0x00` table is not observable** in the schedule:
   `+0x220` (decoded pointer), `+0x340`, `+0x358` (factory returns). Their
   classes have to be read at the factory or at a consumer.
3. **Two table lengths** stop at the 128-entry cap and are not read.
4. **`+0x308`'s two tables**: the constructor installs `0x1430a2910` and the
   block overwrites `+0x00` with `0x142f7f298`. Which one is the class and
   which is a wrapper is not read.
