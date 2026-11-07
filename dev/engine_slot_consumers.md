# Engine slot consumers — a lower bound on who uses `[engine + slot]`

`dev/engine_slot_consumers.py` answers one question: which functions touch
`[engine + slot]`, for each of the 65 member slots of the window
`+0x220..+0x448`? It is the input for naming the subsystems, not a naming.

## Method

1. **RIP references to the engine global.** `0x143b7f410` is referenced by
   5,487
   `rip`-relative instructions in 3,484 `.pdata` functions. The scan is the one
   `dev/engine_ref_scan.py` uses (a `disp32` at the end of the instruction
   means `target == p + 4 + disp`), vectorised with numpy and cached in
   `dev/data/engine_refs_143b7f410.txt`.
2. **Function bodies.** `.pdata` also lists mid-function fragments as their own
   ranges, so the address is resolved to the **outermost** containing range —
   the function a reader would call by name.
3. **Origins.** Inside the body three origins are followed: a register loaded
   from `[rip + 0x143b7f410]` is `engine`; a register loaded from
   `[engine + S]` is `slot=S`; a register computed as `engine + S` is `&slot=S`.
   Any memory operand on such a register is recorded with its displacement and
   read/write/call kind. A call drops the Windows x64 volatile registers
   (`rax`, `rcx`, `rdx`, `r8`–`r11`); any other write to a register clears its
   origin; `xor rX,rX` clears it. So each record is what the instruction says.
4. Output `dev/engine_slot_consumers.csv`
   (`slot,fn,callsite,kind,disp,detail`), not tracked (raw read output).

## What it says (fresh full run)

- 616 accesses; **35 of the 65 member slots** are touched by a function that
  loads the engine global itself.
- The other 30 are **not** proven unused: a consumer that receives the engine
  pointer as an argument never appears. `+0x220` is the demonstration — the
  function that stores it reads it back from `rbx`, not from the global.
- Eight slots are dereferenced by their consumer (fields or a vtable call):
  `+0x238`, `+0x248`, `+0x258`, `+0x290` (also written), `+0x2e8`, `+0x338`,
  `+0x358`, `+0x408`. The rest are loaded and passed on.
- Busiest: `+0x290` 129 accesses / 89 functions, `+0x248` 99 / 39, `+0x240`
  66 / 54, `+0x238` 58 / 35, `+0x278` 34 / 25. Quietest: `+0x268`, `+0x308`,
  `+0x310`, `+0x370`, `+0x388`, `+0x390` at one access each.

### Withdrawn figures

An earlier version of this file reported "22,241 accesses", "36 of 65 slots",
busiest `+0x248` at 5,917 / 572, quietest `+0x288` at 4 / 1, and two named
consumers. They came from a looser scan that this script does not reproduce,
and both named consumers are **wrong**:

- `0x1406bc420` (`UpdateSequenceCameraViewState7`) never reads the engine
  global — its first global read is `[rip+0x34bd27a]` = `0x1441796a8`, and it
  has no `[reg+0x240]`/`[reg+0x248]` operand at all;
- `+0x310` has exactly one consumer, `0x140618f70`, which has no curated name
  (so `MapPreloadMessageApply` was misattributed).

Do not quote the retracted numbers; re-run the script instead.

## Caveats

- A co-occurring curated name is a **hint, not a read**. No slot has a named
  consumer today.
- Ghidra's own function boundaries cover less of this code than `.pdata`
  (`0x1406bbe90` is a function, `0x1406bc190` is not), so `.pdata` drives the
  enumeration and Ghidra is used for names only.
- The script runs in ~6 minutes: the body pass disassembles every referencing
  function with capstone in detail mode. The reference pass is cached.

## Usage

```sh
python dev/engine_slot_consumers.py            # all slots
python dev/engine_slot_consumers.py --slot 0x2e8
```

The per-slot summary prints access counts, function counts, the access kinds
and the first named consumers.
