# Page crypt

The loader keeps `.text` encrypted on disk and decrypts pages on demand when
they become executable. This file records the page transform, the evidence for
it, the offline recovery that now covers the whole executable, and the optional
work to read the loader's `w` picker.

Evidence classes: `static`, `live capture`, `decompiled`, `install`. Nothing
here is fitted. All addresses are RVA in the current-build loader unless marked
`v2b` (merged image base `0x7ff7c4d20000`) or `live` (session base).

## Status

- **Data half: solved.** Every page is XORed with a 4 KB window drawn uniformly
  from one static 16 KiB cycle in `.rdata`. All 2,285 current-build oracle rows
  verify byte-exact. Cross-run check: 1,676 pages from `run_20261021_105102`
  reproduce the same `w` as the `run_20261021_033126` oracle, zero conflicts;
  597 pages gave no window (cold/partial), not a different `w`. So `w` is
  build-stable, not a per-run draw.
- **Code half: live-verified.** The candidate `0x14380e0` is the page-XOR
  routine. Its dword XOR loop `0x144c839` was sampled as a **leaf PC** in
  `cpu_20261021_124450.etl`, with the known caller `0x1015a94` (function
  `0x1012430`) directly above it. The candidate's post-`VirtualProtect`
  return `0x14462cb` was also sampled, from the second caller `0x1a0d536`
  (function `0x1a0a3f0`). PadB `0x10c1e20` is live too, but its dword loops
  (`0x10c7692`, `0x10c9835`) are called from `0x8a4ad0`, a separate path.
  See "Live ETW leaf evidence".
- **Still open (picker only):** the **`w` source** and the **faulting
  rip/first handler**. The ETW sampling proves the loop executes, not which
  page it decrypted or where `w` came from. The 5th argument is a hash-table
  lookup at `0x1012e1a` (`0x1ce59b0` -> `0x3fa280` -> `hash(rdi) % count`);
  the value is a type-A record whose fields do not contain `w` plainly.
- **Coverage solved offline (2026-10-23):** `dev/offline_page_recover.py`
  recovered all 12,076 `.text` pages and the `.pdata` table from the on-disk
  ciphertext without a live capture. Held-out oracle 2,284/2,284; warm images
  4,887/4,888 and 5,350/5,351; one declared 16-byte tail. See "Offline
  recovery" below. The blind solver replaces the need for the `w` picker; a
  new build re-extracts the pad and reruns it.
- **Tier 3 boundary:** the `w` **derivation** is inside the loader's own
  obfuscated VM/dispatcher (`0x2511e0` dispatcher, 22 KB
  `0x40bee0..0x411536`; `0x40cd87` is a mid-function handler block). The
  earlier "Denuvo VM" label was wrong; the protection is the loader's own
  code, not Denuvo. The picker is not recovered; this no longer blocks
  reading the executable.
- **Latest page-diff run:** `run_20261021_105102` is phase-marked and passes
  the correlation gate as `ATTRIBUTABLE-CANDIDATE`. It has no faulting rip,
  handler or `VirtualProtect` arguments.
- **Next step:** the picker is optional static work (reverse the loader VM).
  For coverage, use the offline solver; no fault-synchronous capture is
  required.

## The transform

For a page at file offset `o`:

```
P[o..o+0x1000] = C[o..o+0x1000] XOR K[(w + 0) .. (w + 0x1000)]
```

- `C` is the on-disk ciphertext (also the bytes in memory while the page is
  still protected).
- `P` is the plaintext after the page becomes executable.
- `K` is the static 16 KiB cycle, and `w` is a 4-byte-granular start into it.
  The starts are uniform and i.i.d. across pages; the measured 3,068 starts
  cover all four 4 KB quadrants. `w` is stable across runs (1,676/1,676 pages
  match with zero conflicts), so this is a build-time draw, not a fresh draw
  at each fault.
- The window start is **not** derived from the RVA, page index, page content,
  hash, region, image base, host pointer, a modular counter, or an affine map.
  Those tests are all negative.
- The transform is XOR only. It is not AES, ChaCha, ECB, a short repeating key
  or a block cipher; entropy and diffusion tests agree. The 16 KiB cycle repeats
  by construction, but each page gets a fresh start.
- The plaintext persists after the protection change. A page that decrypts does
  not re-encrypt during the session, so a protection-aware reader catches it.

### The 16 KiB cycle

- Location: `.rdata` at `0x1d77f80`. Table body ends at `0x1d7bf6c`; a
  deterministic tail extension completes the cycle at `0x1d7bf80`.
- Size: `0x4000` bytes. The shipped loader stores it rotated by 19 bytes.
- Evidence: `X = C_disk ^ P_warm` is identical across independent captures
  (5,714 pages, zero mismatches); the table matches the observed stream for
  2,285/2,285 current-build pages.
- Window selection: 3,068 starts measured at 4-byte granularity; all four
  4 KB quadrants are observed and the frequencies are consistent with uniform.
  No C or RVA structure selects one.
- The cycle contains a 16-byte zero splice at `0x53a000`. It is part of the
  observed stream, not a missing row or a capture failure; all 2,285 oracle
  rows verify including the splice.

The per-page stream is therefore known for the current build. The code path
that chooses the window at fault time is still unread, but the blind
ciphertext solver no longer depends on it (see "Offline recovery" below).

## Oracle and coverage

- Artifact: `D:/ow-dumps/pagepak/keystream_current.npy` plus
  `keystream_current_meta.json`, built from
  `run_20261021_033126` (2,285 pages).
- `1,684/2,285` rows are full 4 KB windows; the rest are shorter tail pages.
  All 2,285 were checked against the cycle and verify.
- The older `D:/ow-dumps/pagepak/keystream.npy` (9,449 rows) is the previous
  build and must not be used as a current-build oracle.
- `run_20261021_033126` is also the main coverage run: 15,636 pages and 443
  protection flips. `encrypted.csv` lists pages read only as cipher.

The data side is closed and page coverage is solved offline. No new capture is
needed for either; the only remaining page-cipher question is the picker's VM
derivation, which is optional static work.

## The candidate applier

**`0x14380e0..0x144ee19` (93,497 bytes) is CANDIDATE, not verified.**

Why it is the leading candidate:

- Eight of the ten pad-base references in loader `.text` sit in this
  function (the other two are in the secondary pad family; see below).
- It contains two of the nine loader `VirtualProtect` call sites:
  `0x14462c5` and `0x144df0a`. Both target slot `0x1dce438`.
- It contains nine pad-indexed byte-xor leaves and two dword-xor leaves.
- It contains four `and reg,0x3fff` sites (`0x14381f5`, `0x14398ca`,
  `0x1448198`, `0x144bfc6`); the `0x144bfc6` value is parked and consumed
  inside the function.
- Two `E8` callers enter the function directly (`0x1015a94`, `0x1a0d536`),
  and no `lea`/immediate function-pointer take targets the entry; the static
  caller set is closed.

Why it is not verified:

- It is a 93 KB VM with many states; the page-XOR path is one state, and no
  observed caller yet passes a page-sized buffer to it.
- A single function can hold the right imports and still be a decoy; the disk
  loader deliberately mangles imports, exports, load config and `.gxfx`.
- The function's `VirtualProtect` sites are not reached by any observed
  protection flip; one page (`0x144d000`, holding VP#2 `0x144df0a`) was not
  observed as plain and is zero-filled in the live loader image.
- No fault-time record ties a flip to this region, its window mask, or its
  VP arguments.

**Rival `0x442110..0x4484a6`** is excluded from first place but remains on the
watchlist: it holds VP sites `0x4431cf` and `0x448460`, and it reads the pad
**end** rather than the pad base. It has no pad-base arms, no window mask and
no load-xor-store leaves. A hit on both candidate and rival stays
`UNATTRIBUTABLE`.

Other VP holders are outside the candidate span: `0x837b20`, `0xffaa20`,
`0x13216b0`, `0x16429d0`. Their profiles do not match a page applier.

### Static refinement (2026-10-21)

Re-derived from `loader_disk_rva.bin` with capstone and from the live loader
image `D:/ow-dumps/dev_work/live_loader_flat.bin`. No new live read.

- **Pad-base census:** exactly ten `lea reg,[rip+disp]` sites target
  `0x1d77f80` - eight inside the candidate span (`0x1438132`, `0x143ee78`,
  `0x1442bea`, `0x144583b`, `0x1447dd0`, `0x144bca8`, `0x144d5a3`,
  `0x144d5f3`), one in `0x10b72f0` (`0x10b7366`), one in `0x10c1e20`
  (`0x10c1e5d`). The older "three RIP referrers" note is superseded.
- **Function boundary confirmed; `.pdata` is intact.** All 5,701 entries are
  sorted and structurally sane once the last 281 are allowed to end in the
  `.text` raw padding (`0x1d53234..0x1d53ac1`, up to the raw end). The earlier
  "mangled `.pdata`" note was a parser-bound error. `0x14380e0..0x144ee19` is
  one function; the call to `0x1450337` is a normal call into the next
  function, not evidence of a region. The two direct `E8` callers stand, and
  no `lea`/immediate function-pointer take targets the entry, so the static
  caller set is closed.
- **The function has a page-XOR state.** At `0x14388e7` it loads a descriptor
  `(len, field, w, dest)` from frame fields and jumps to `0x144968f`; the
  dword loop at `0x144c839` uses that `w` and `dest`. The descriptor is built
  at `0x144819e..0x14481fc` from `[rsp+0x4f8] + [rsp+0x60]` and registers
  `ecx/eax/edx`; `w` is `edx` at `0x14481c1`. This is the concrete page path.
- **Three pad-consuming regions share one call shape** - three small integer
  keys plus a data pointer:
  - `0x14380e0` <- `0x1015a94`, `0x1a0d536`;
  - `0x10b72f0` <- `0x442c41`, `0x4462f0`, `0x12c4d08`, `0x12c6555`;
  - `0x10c1e20` <- `0x8a4ad0` (wrapper `0x8a46d0`, entered from `0x7f9742`).
  All three resemble a polymorphic data/string decoder family. The page path
  above is one state inside the candidate, so the shared outer call shape is
  a caution, not a disqualifier.
- **Alternate signature in `0x10c1e20`:** it is the only region in loader
  `.text` with a four-byte-granular pad mask - nine `and reg,0x3ffc` sites
  (`0x10c4a94`, `0x10c6b51`, `0x10c702a`, `0x10c7052`, `0x10c7692`,
  `0x10c881e`, `0x10c8828`, `0x10c982f`, `0x10c98b6`) and dword pad loads at
  `0x10c7698`, `0x10c70c2`, `0x10c9835`. Every measured window start is
  4-byte aligned, so this is a stronger page-window signature than the
  candidate's byte-granular `0x3fff` mask. It is still unproven: the only
  observed caller passes fixed small keys and a pointer, not a 4 KB page.
- **`.eid` is structured, not opaque.** The 4.78 MB "EIDOLON" section
  (RVA `0x1f80000`) is byte-identical between disk and the live loader image,
  so it is not decrypted in place. It contains low-entropy tables (e.g.
  `0x1000`, `0x171000`) mixed with high-entropy blobs, so a runtime copy in
  the heap, not the module image, is the likely home of any per-page `w`
  records. Treat this as a lead for a read-only heap/minidump, not a finding.
- **Hash look-alike:** the same region contains the MurmurHash3 64-bit
  finalizer constants (`0xff51afd7ed558ccd`, `0xc4ceb9fe1a85ec53`) at
  `0x10c6839` / `0x10c68e8`. The shift count in that block resolves to 33
  from the `.data` dword at `0x1e13250`, and the sequence multiplies before
  the first shift, so it is not the stock finalizer. The two constants occur
  25-27 times in loader `.text` (name/import hashing), so this is a lead,
  not evidence that the window is hash-derived.
- **Cold pages:** the live loader image differs from disk on 909 `.text`
  pages; sampled pages in that set are all-zero in the capture. The set
  includes `0x1445000` and `0x144d000` (pad arms `0x144583b`, `0x144d5a3`,
  `0x144d5f3` and VP#2 `0x144df0a`), which is consistent with one-shot code
  that is gone by capture time, but a cold-page capture cannot separate
  "erased" from "never read". The candidate entry, padA and padB pages are
  byte-identical to disk. Do not read the diff as a build mismatch. The VP#1
  page (`0x1446000`) and both dword-leaf pages (`0x144b000`, `0x144c000`) are
  persistent; **if** the erase is real, VP#2 and the pad arms at `0x144583b`,
  `0x144d5a3`, `0x144d5f3` belong to a one-shot initial batch, while VP#1 and
  the dword leaves are the runtime path. Hypothesis only.

### Watchlist

RVA, loader image (add the session loader base for live addresses):

| What | RVA |
|---|---:|
| Candidate entry | `0x14380e0` |
| Candidate end (exclusive) | `0x144ee19` |
| Pad base / end | `0x1d77f80..0x1d7bf80` |
| Candidate VP call #1 | `0x14462c5` |
| Candidate VP call #2 | `0x144df0a` |
| Candidate window mask | `0x144bfc6` |
| VP import slot | `0x1dce438` |
| Rival A entry | `0x442110` |
| Rival A VP sites | `0x4431cf`, `0x448460` |
| Pad family A entry | `0x10b72f0` |
| Pad family A pad arm | `0x10b7366` |
| Pad family B entry | `0x10c1e20` |
| Pad family B pad arm | `0x10c1e5d` |
| Pad family B mask sites | `0x10c4a94`, `0x10c6b51`, `0x10c702a`, `0x10c7052`, `0x10c7692`, `0x10c881e`, `0x10c8828`, `0x10c982f`, `0x10c98b6` |
| Pad family B caller | `0x8a4ad0` |

The two pad-family entries are **secondary** watch targets. Do not spend
fault-rip slots on them until the primary candidate/rival shape is resolved;
they exist so a later run can test the alternate signature without rediscovery.

For the pid-6000 / loader base `0x7ffc52d50000` session the resolved values
were: entry `0x7ffc541880e0`, VP#1 `0x7ffc541962c5`, VP#2 `0x7ffc5419df0a`,
window `0x7ffc5419bfc6`, pad `0x7ffc54ac7f80..0x7ffc54acbf80`, slot
`0x7ffc54b1e438`. These are **session-specific**; resolve fresh from the new
run's `meta.json` and never reuse the pid-6000 absolute file.

Watchlist closure for the primary set: all 22 leaves and eight pad-base arms
sit inside the candidate span; 11 non-candidate sites sit outside; the pad and
import slot are data. The 2026-10-21 census adds two pad-base arms outside the
span (`0x10b7366`, `0x10c1e5d`), so "eight arms in one function" describes the
candidate region only, not all pad consumers.

## Negative results

These are settled; do not repeat them without a new build.

- No VEH registration, no `AddVectoredExceptionHandler`, no
  `QueueUserAPC`-based call in loader imports.
- No static reference to `NtContinue`; the `NtContinue` slot `0x1dcdd58` is
  read by one forwarder at `0x1d63880` inside `0x1d63858`, not by a page
  handler.
- `.pdata` unwind sweep: eight chained handlers, all C++ dispatch; none is a
  page decryptor.
- Deep-stack SEH/VEH hunt: chain empty, no handler frames on 86 sampled
  stacks; the only live VEH is NVIDIA's, with zero loader registrations.
- No 4 KB page walker outside CRT `memmove`; no ARX loop tied to page extents;
  1,009 ARX-shaped functions triaged, none is a page decryptor; 163 direct
  syscall sites, zero real stubs.
- No verified static consumer of the 16 KiB table. The 2026-10-21 census
  replaces "three RIP referrers" with ten `lea` sites in three VM regions; no
  one of them is yet tied to a page fault.
- No cipher-constant family, no absolute-address immediates, no `.eid`
  references in static code. Everything is runtime-computed via gs/fs walks.
- CAPI surface: only four slots have static refs (`CryptAcquireContext`,
  `CryptCreateHash`, `CryptDestroyKey`, `CryptReleaseContext`), all in three
  functions, all with small-int arguments; `CryptDecrypt`/`CryptEncrypt`/
  `CryptGenKey` are absent.
- Replay-emulation of the TLS callback with live `.data`/`.rdata` overlays
  matches the disk baseline; the decryptor is not in the offline TLS path.
- Idle and menu polling produce no loader protection flips. Active gameplay is
  required, and pages decrypt transiently.

- The window start is not a stock hash of page-derived input: standard and
  variant fmix64, fmix32, CRC32, Adler32, FNV-1a 32/64, MurmurHash3 x64 128 of
  `C` and `P`, MD5/SHA-1/SHA-256 truncations, u16/u32/u64 words at twelve
  `C`/`P` offsets, every direct bitfield of `C`/`P`, preferred-image-base VAs
  and page +/- 1 are all chance-level.
- No raw or hashed per-page start was found in the disk loader, the live
  loader image, the live main-module image or `.eid`: `w` and `w/4` were
  scanned at every stride 1..256 and every alignment; 8-byte and 4-byte seeds
  were tried through both fmix64 forms at 8-byte strides. `.eid` is a
  structured section, so this is a search negative, not proof of absence.
- Loader `.pdata` is **valid** (RVA `0x1f00000`, size `0x10b3c`): 5,701
  sorted entries; the tail 281 end in `.text` raw padding. Function extents
  are usable.

The import model explains the static dead end: on-disk IAT slots hold small
descriptor IDs, while runtime slots hold heap-thunk pointers. The genuine live
band resolves 286/286 only with the live TEB/PEB; `0x1dce438` is
`KERNEL32!VirtualProtect` (`0x7ffcd1d5bfb0`). The applier is not a named
import.

## Live evidence and the gate

`events.csv` schema:
`t,phase,kind,module,addr,size,old_prot,new_prot,note`. It records protection
flips only. It does **not** record faulting rip, first handler, or VP
arguments.

- `run_20261021_033126`: 15,636 pages, 443 flips, all phase 0. Gate:
  `UNATTRIBUTABLE`.
- `run_20261021_104439` (pid 2348): 120 flips, all phase 0; run was not
  phase-marked. Gate: `UNATTRIBUTABLE`.
- `run_20261021_105102` (pid 4100, 90.3 s, 1,597 flips; phases
  `{0:1449, 1:51, 2:97}`): gate `ATTRIBUTABLE-CANDIDATE`. Phase 1 is
  hero-select (50 in-range RVAs); phase 2 is practice-range (95 RVAs).
- Same-session cookie run `run_20261021_105145`; loader base
  `0x7ffc52d50000` stable across the 63-series pair.

The gate definition is narrow: it requires at least one non-zero-phase flip
and a live loader base. A pass proves only that the phase markers reached the
event stream. It cannot promote the candidate without a fault-time rip side.
`run_20261021_104439` is the control: phase 0 everywhere -> un-attributable.

The phase-marked run buys a 95-RVA in-range target set and a 50-RVA
hero-select set for the next fault-rip source. The ETW/fault-rip spec from
session 61l is still unbuilt; that is D1/D2.

## Live ETW leaf evidence (2026-10-21)

Capture: `D:/ow-dumps/etw/cpu_20261021_124450.etl` (2,986,344,448 B), WPR CPU
profile started before launch. The loader name occurs with base
`0x7ffc52d50000`, the same base as the run-63 series. A raw qword scan of the
ETL (no full decode) finds code addresses in the pad family; the surrounding
bytes are stack arrays, so the **first frame** is the sampled leaf and the
following frames are return addresses.

- **Candidate page loop, 6 samples.** Leaf `0x144c839` (the dword XOR loop
  inside `0x14380e0`), then
  `0x1015a99` (return into caller `0x1012430` at the `call 0x14380e0` at
  `0x1015a94`), `0x163f6b1` (function `0x163f3f0`), `0x1a29511` (function
  `0x1a29430`), then ntdll. This is the page-XOR path.
- **Candidate VP return, 26 samples.** The frame chain starts in ntdll (inside
  the `VirtualProtect` call), then `0x14462cb` (return after the indirect VP
  call at `0x14462c5`), then `0x1a0d53b` (return into `0x1a0a3f0`, the
  `call 0x14380e0` at `0x1a0d536`), then `0x7f97b1`, `0xc7838e`. The VP path
  is live from the second caller.
- **PadB dword loops, 3 samples each.** Leaves `0x10c7692` and `0x10c9835`,
  then `0x8a4ad5` (return into `0x8a46d0`, the `0x8a4ad0` family), `0x7f9747`,
  `0xc7838e`. PadB is a separate live decrypt loop, not the candidate.
- `0x10c35a0` (padB call into `0x1a6b030`) is also a sampled leaf, 4 samples,
  same `0x8a4ad5` caller.

The candidate loop sample is the strongest single fact: the instruction that
loads `w` at `[rsp+0x42c]` and XORs the page is executing in a normal
practice-range session, and its caller is the already-known `0x1015a94`.
ETW sampling is probabilistic and does not carry registers or the page
descriptor, so this does **not** give `w`; it names the applier.

### Page metadata table in the dump (2026-10-21)

At heap VA `0x1ba36b90000` (dump range `0x1ba36b90000 + 0x41000`) the full
dump holds **1,845 records of 0x90 bytes**. A record starts with a main-image
page VA, then two 64-bit values with constant high halves and varying low bits,
flags, four more 64-bit values and three pointers. Example (page 8958,
`0x7ff6a507e000`): `0xc17c5c1b848f07f2`, `0x6f4e2a8774a3da15`.

This is real page metadata, not a decrypt table: `w` is not at a fixed offset
in the record. For the 860 records whose page has an oracle `w`, the plain
forms fail (best 3/860): every shift of the first three hashes, `% 4096`,
xor/add/mul of the first two, `fmix64` of either or their xor, seeded
Murmur3/CRC32 over the page index/RVA/VA, every u16/u32/qword field, and a
single constant xor/sub/add of any 16/32-bit field. The `w` source is
therefore still open.

The likely **5th-argument table** is at heap VA `0x1ba37437d58`: 1,439
consecutive qwords, each pointing at a `0x90`-byte **type-A record**. A type-A
record has the page VA at `+0x88`, hashes at `+0x00`, `+0x18`, `+0x28..+0x60`,
three pointers at `+0x68/+0x70/+0x78` and a flag at `+0x80`. A **type-B**
record (page VA at `+0x00`, hashes at `+0x08`, `+0x20..+0x48`, six pointers at
`+0x50..+0x80`) is the integrity record. The index that selects a type-A
record comes from `0x1ce59b0`; that function and the record-to-page mapping are
the next anchor. The selector is a **hash-table lookup**, not a page index:
`0x1ce59b0` wraps `0x3fa280`, which calls `0x1494090` (the hash over `rdi`)
and returns `hash % count` (`div` at `0x3fa3b3`). The type-A record is the
bucket value; no field or constant encoding yields `w`, so `w` is computed or
decrypted inside the VM, not stored in the record.

The selector chain is itself VM-wrapped: `0x1ce59b0` sets `ecx=0x64`,
`edx=0x51`, stack `0x3d` and calls `0x3fa280`; `0x3fa280` calls `0x1494090`
(which sets `ecx=0x41`, `r8d=0x46`, `r9d=0x13` and calls `0x2511e0`) and also
`0x40cd87` (22 KB). `0x3fa280` reduces the returned value with
`div [rsp+0x2c]` at `0x3fa3b3`. The opcode constants and the 1,312-byte
`0x2511e0` / 22 KB `0x40cd87` are the VM frontier; the native page loop
`0x144c839` is reached from the same caller, so `w` crosses from VM state
into the native descriptor.

## Runbook for the operator

0. Read the new run's `meta.json` loader base and resolve the watchlist:
   `python dev/faultsync_resolve.py <loader_base_hex>`.
   Never reuse an absolute watchlist from another session: those addresses
   are per-session and only the RVA watchlist travels.
1. Run the phase-marked capture (90 s, practice range, first-person):
   `python dev/live_page_diff.py --seconds 90 --out D:/ow-dumps/live_page_diff --all-modules --surge-seconds 90 --include-data --focus-file D:/ow-dumps/live_page_diff/run_20261021_033126/gaps.focus`
   Mark menu -> hero select -> in-range with Enter.
2. Run the gate:
   `python dev/faultsync_correlate.py <new-run>/events.csv`
   It must print `ATTRIBUTABLE-CANDIDATE`. If it prints `UNATTRIBUTABLE`,
   the phase marks did not land; do not interpret the run.
3. In the same session, grab the cookie/import band:
   `python dev/ldr_cookie_grab.py`
   It writes `iat.bin`, `stubs.bin`, `teb.bin`, `peb.bin`.
   **Warning:** `--help` performs a grab; never pass flags expecting a dry
   print. The 2026-10-21 accidental grab is disclosed below.
4. Attribution rule: a candidate VP hit counts only with no same-fault rival
   A hit. Candidate leaf + VP on the same fault is the `ATTRIBUTABLE` shape.
   A hit on both candidate and rival stays `UNATTRIBUTABLE`.
5. Optional, independent of the flips: an elevated ETW CPU-sample trace.
   `powershell -ExecutionPolicy Bypass -File dev/etw_cpu_capture.ps1`
   Start it **before** launching Overwatch, enter the practice range, stop
   after 30-60 s. The ETL lands in `D:\ow-dumps\etw\`. Scan it with
   `python dev/etw_raw_scan.py <etl> --base <loader_base>`.
   Goal: a sampled leaf PC in `0x14380e0..0x144ee19`, `0x10b72f0..0x10bb000`
   or `0x10c1e20..0x10d06c4`. This is probabilistic (1 kHz sampling); a hit
   names the applier, a miss proves nothing. **Tested**: the 2026-10-21
   trace named the candidate's dword loop `0x144c839` (see above).
6. Optional, closes the data side: a read-only private-RW minidump of the
   game while it is in the practice range.
   `python dev/dump_process.py Overwatch.exe D:\ow-dumps\ow_heap_<stamp>.dmp`
   Then `python dev/scan_dump.py <dump> --pattern w` (and `--pattern pad`,
   `--pattern eid`). `w` is build-stable, so the oracle run's `w` sequence is
   the needle. The dumper is read-only; it writes only the dump. Tested on a
   Python child, **not yet on the game**; `--full` is available if the heap is
   not in the private-RW set.

The band is per-session. A same-session heap/module map is required to resolve
thunk pointers; do not compare absolute heap addresses across boots.

## Artifacts and tools

Dumps (outside the repo):

- `D:/ow-dumps/loader_disk_rva.bin` - current-build flat loader, offset == RVA.
  Verified byte-identical to the installed `Overwatch_loader.dll` on 7,507/7,520
  `.text` pages.
- `D:/ow-dumps/loader_after_diff.bin` - older warm loader build; not a current
  oracle.
- `D:/ow-dumps/module_after_diff.bin` - merged main image, v2b base
  `0x7ff7c4d20000`.
- `D:/ow-dumps/pagepak/keystream_current.npy` - current oracle.
- `D:/ow-dumps/live_page_diff/run_20261021_105102/` - latest phase-marked run.
- `D:/ow-dumps/live_page_diff/run_20261021_033126/` - coverage + oracle run.
- `D:/ow-dumps/live_cookie/run_20261021_105145/` - same-session cookie grab.
- `D:/ow-dumps/etw/cpu_20261021_124450.etl` - WPR CPU trace (2.98 GB) with
  the live candidate leaf samples.
- `D:/ow-dumps/ow_heap_full.dmp` - 13.5 GB full minidump of the same series
  (base `0x7ff6a2d80000`, loader `0x7ffc52d50000`).
- `D:/ow-dumps/ow_heap_norm.dmp` - 446 KB normal dump; no heap.
- `D:/ow-dumps/dev_work/ghost_loader.bin` - the loader's private ghost copy at
  `0x1ba39390000` (pad and `.eidsig` present; `.eid` byte-identical to disk).

Tools:

- `dev/live_page_diff.py` - preferred read-only page capture.
- `dev/warm_poll.py` - fallback continuous warm-page poll.
- `dev/ldr_cookie_grab.py` - same-session IAT/stub/TEB/PEB grab (no `--help`).
- `dev/faultsync_watchlist.txt` - RVA watchlist.
- `dev/etw_cpu_capture.ps1` - elevated read-only WPR CPU-sample capture.
- `dev/etw_raw_scan.py` - raw qword scan of an ETL; finds the loader base
  from the UTF-16 name and counts pad-family addresses.
- `dev/etw_count.py` - exact counts/offsets of known loader addresses.
- `dev/dump_process.py` - read-only minidump; on the game `--normal` and
  `--full` work, the private-RW default returns `E_INVALIDARG`.
- `dev/scan_dump.py` - scan a minidump for the `w` run, the pad, or the
  EIDOLON marker (contiguous fast pass; `--slow` for strided tables).
- `dev/work/etw_context.py` - print the stack arrays around ETL qword hits.
- `dev/work/ghost_scan.py` - extract the ghost loader and stride-search its
  regions for the `w` run (negative).
- `dev/work/dump_stride_search.py` - fixed-stride `w`/`w4` search over a
  full dump (negative).
- `dev/work/dump_stacks.py`, `dev/work/dump_cand_stack.py` - scan minidump
  thread stacks for loader code addresses and the known page caller.
- `dev/work/pagehash_w.py` - parse the 0x90-byte page metadata table and test
  `w` against the two per-page hashes (negative on 860 matched pages).
- `dev/work/find_meta_table.py` - find the qword table(s) that point into the
  page-metadata range (the 1,439-entry table at `0x1ba37437d58`).
- `dev/work/meta_record_w.py`, `dev/work/read_meta_record.py` - inspect the
  type-A/type-B records and test fields for `w` (negative).
- `dev/faultsync_resolve.py` - rebase watchlist to a fresh loader base.
- `dev/faultsync_correlate.py` - phase/gate attribution.
- `dev/emu_tls.py` - offline Unicorn TLS-callback harness.
- `dev/ldr_sweep.py`, `dev/ldr_cryptoscan.py`, `dev/ldr_vehunt.py`,
  `dev/ldr_iat.py` - static loader probes.
- `dev/offline_page_recover.py` - recovers `.text`/`.pdata` from the installed
  executable with the pad and ciphertext-only scoring (Plan A).
- `dev/pagepak_build.py` - idempotent keystream-table builder.
- `dev/page_crypt_x.py`, `dev/page_crypt_battery.py` - page-pair tests.

## Offline recovery (2026-10-23)

`dev/offline_page_recover.py` recovers `.pdata` and `.text` from the installed
`Overwatch.exe` with no live capture and no process access.  Artifacts and
`REPORT.md` are under `D:/ow-dumps/offline_recover/`.

- **Pad:** the bytes at `0x1d77f80` are the logical cycle verbatim.  All 2,284
  single-window oracle rows match a 4-byte-aligned `w`; a 19-byte rotation only
  matches with a non-aligned `w` (`+19 -> w%4=3`, `-19 -> w%4=1`), so it is an
  alias, not the stored phase.
- **Splice:** the remaining oracle row (`0x53a000`) is a two-window stream:
  window `1524` for bytes `0..1056`, 16 zero keystream bytes, window `1512`
  from byte `1072`.  Two-window validation is 2,285/2,285.  The blind recovery
  finds the same `w_prefix=1524`, `w_suffix=1512`, split `1056` from the page
  alone, and both warm images match the result byte-exactly.
- **`.pdata`:** the disk section is plaintext, not page-encrypted: 201,246
  sorted `RUNTIME_FUNCTION` entries, every start/end in `.text`, every unwind
  RVA in `.rdata`, and byte-identical to `module_v21_clean.bin` /
  `module_v21d.bin`.  No pad window parses to a valid table.
- **`.text`:** all 12,076 pages recovered.  Held-out oracle top-1 is
  2,284/2,284 (100%), min gap 1.21 nats/byte.  Warm images match 4,887/4,888
  and 5,350/5,351 readable pages.  Three pages are embedded RVA tables
  (`0x23bb000`, `0x2e1f000`, `0x2e20000`) and score by 32-bit RVA content.
- **Boundary:** `0x540000` has a 32-byte plaintext island at its PE entry
  function (`0x540fd0`), recovered by anchoring on `AddressOfEntryPoint`; the
  last 16 bytes (`0x540ff0..0x540fff`, a `jmp` thunk plus `int3` padding in the
  warm image) match neither the window nor the identity and are declared, not
  faked.  The page stays marked ambiguous in `text.csv`.
- **In Ghidra (2026-10-23):** the image is the program `Overwatch.exe` at its
  preferred base `0x140000000`, auto-analysed to **151,260 functions** with all
  12,076 `.text` pages initialized.  Read a doc's v2b address there as
  `0x140000000 + (v2b - 0x7ff7c4d20000)`; it is the program to use for pages
  that are zero in `module_v21_clean.bin`.

## Corrections and retractions

- **Session 54:** the apparent control file was an emulator artifact; it is
  not evidence of a live pointer.
- **Session 55:** the early import census (111 slots) was from a stale merged
  exe image; only the live TEB/PEB-resolved 286-slot band is trustworthy.
- **Session 61b:** the changed-page label was corrected from `.rdata` to
  `.data`.
- **Session 61u:** `ldr_cookie_grab.py --help` performed an unsanctioned but
  read-only grab (`run_20261021_094721`). The process was untouched; the
  footgun is recorded so it is not repeated.
- **Build boundary:** warm loader images predate the 2026-10-20 install
  update. Warm-vs-disk comparisons across that boundary are invalid, including
  the former "7,178 identical loader pages" claim.
- **Coverage:** `run_20261021_031637` shows zero changed rows only because it
  did not use `--include-data`; it is not evidence against the changed-page
  census.

## Session index

| Sessions | Outcome |
|---|---|
| 2-4 | Loader recon; tight tier; `.eid` and engine pages caught transiently |
| 5-6 | Per-page XOR stream; disk loader is plaintext ground truth; build mismatch |
| 7-8 | Packer mapped; offline TLS emulation; runtime-only gate |
| 9-13 | Static decryptor hunt negative; CAPI/containment closed |
| 14-17 | Fault-time capture, cold-page polls, SUEF triage |
| 18-29 | Flip-gated traces, cookie churn, SEH/VEH, unwind, slot resolution |
| 30-41 | ARX/syscall/VP profiles, oracle reuse, nonce/structure tests, pool model |
| 42-49 | 16 KiB `.rdata` cycle; window draw; tail extension |
| 50-59 | Xor-loop/picker/import hunts; live thunks; VP = `VirtualProtect` |
| 60-62 | Candidate staging, gate/tooling, phase path |
| 63 | Phase-marked live run passes gate; candidate still unverified |
| 64 | Static refinement: pad census, VM extent, padB mask, fmix64 look-alike; `w` table/hash search negative |

## Open questions

1. **D1:** the faulting rip, first handler and `VirtualProtect` arguments for
   a page decrypt. Until then `0x14380e0` stays CANDIDATE.
2. **D2:** a same-session mapping of the heap-thunk band and module map, plus
   the ETW/fault-rip source. `live_page_diff` and the cookie grab cover the
   heap/TEB half, not the rip half.
3. **Window choice:** the distribution is measurably i.i.d. uniform across
   pages, but `w` is stable across runs (1,676/1,676, zero conflicts). The
   value was fixed at build/pack time or is reconstructed deterministically;
   it is not a per-fault random draw. The consumer is the missing applier; no
   separate picker has been found.
4. **New build:** if the install updates, rebuild
   `loader_disk_rva.bin`, re-verify the 16 KiB cycle and refresh the oracle.
   All addresses in this file are build-specific.
