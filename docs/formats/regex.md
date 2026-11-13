# Regular expressions (PCRE)

Status: **identified, not ported** — this is middleware the client calls, not
behaviour to reimplement. Same rule as Wwise in `formats/sound.md`.

The client statically links **PCRE 1**: the 8-bit, non-JIT 8.3x/8.4x line,
behind a ~2.2 KB game-side binding. No gameplay system owns a pattern; the
binding is used from input, the camera, the mover and Statescript as a general
string predicate ("does this string match any of these patterns").

This page exists because the image's largest function is part of it:
`PcreMatch` `0x1423add80` at **58,068 bytes** is the matcher body, and it was
recorded as unread until 2026-11-20 (`ow-tank-1to1.md` section F).

## How it was identified

Five independent reads agree, so this is not a guess from a size or a shape:

| address | name | what settles it |
|---|---|---|
| `0x1423bc3c0` | `PcreExec` | rejects a code block whose first dword is not `0x50435245` (the bytes `PCRE`) and whose flag byte bit 0 is clear — PCRE's magic-number check; then takes (code, extra, subject, length, startoffset, options, ovector, ovector count) and returns PCRE's negative `PCRE_ERROR_*` codes |
| `0x1423bd1c0` | `PcreFullinfo` | same magic check plus `PCRE_ERROR_NULL`/`BADMAGIC`, then a `what` switch that is PCRE's `PCRE_INFO_*` enumeration: 0 options (masked with the compile-only mask `0x27ff7a7f`), 1 size, 2 capture count at `+0x1e`, 3 backreference max, 4 first character, 5 first table, 7 name-entry size, 8 name count, 9 name table, `0x11` JIT size |
| `0x1423aca10` | `PcreCompile2` | references PCRE's compile-error text table at `0x14344c3c0` ("no error") and `0x14344c480..`: "missing ) after comment", "parentheses nested too deeply", "lookbehind assertion is not fixed length", "DEFINE group contains more than one branch", "subpattern name is too long (maximum 32 characters)", "too many named subpatterns (maximum 10000)", "octal value is greater than \377 in 8-bit non-UTF-8 mode", … |
| `0x1423ac9e0` | `PcreCompile` | 35 bytes; supplies a null `erroroffset`, which is PCRE's `pcre_compile` over `pcre_compile2` |
| `0x1423add80` | `PcreMatch` | called from `pcre_exec` only, calls itself 92 times, and is the only body in the image that consumes a compiled block this way |
| `0x1423bf290` | `PcreValidUtf8` | reached only when `code->options & PCRE_UTF8` is set and the caller did not pass `PCRE_NO_UTF8_CHECK`; returns the offending offset, which `pcre_exec` then stores in `ovector[0]` — PCRE's bad-UTF-8 contract |

**The release is not pinned.** PCRE embeds its version string (`"8.44 2020-02-11"`
and the like); that string is **not in this image** — searched `8.44 `, `8.45 `
and `2020-02-11`, no match — so the build carries no version anchor. The error
text set includes 8.34-and-later wording, which is what narrows it to the
8.3x/8.4x line. Whether the image also links PCRE's JIT is **open**: the
binding calls `PcreExec`, and no `pcre_jit_*` entry point is cited.

## The sizes

`PcreMatch` is 58,068 bytes in three contiguous `.pdata` pieces — 169 + 55,470
+ 2,429 — so that figure is its own code, not a span over its neighbours.
`python dev/fn.py read 0x1423add80` prints the pieces.

| function | bytes |
|---|---:|
| `PcreMatch` `0x1423add80` | 58,068 |
| `PcreCompile2` `0x1423aca10` | 4,973 |
| `PcreExec` `0x1423bc3c0` | 3,582 |
| `PcreFullinfo` `0x1423bd1c0` | 980 |
| `PcreValidUtf8` `0x1423bf290` | 914 |
| `PcreCompile` `0x1423ac9e0` | 35 |
| the binding below (11 functions) | 2,240 |

Two neighbours in the same block are not yet classified: `0x1423bc060`
(858 B, a byte compare of a table-addressed subject slice that returns `-2`
past the end, `-1` on a mismatch, else the length) and the rest of
`0x1423bd1c0..0x1423c0000`, which is presumably the `pcre_get_substring` /
`pcre_config` family but is unread.

## The game-side binding

A compiled pattern is held in a **holder**: the pattern string at `+0x00`
with its capacity at `+0x14` (small-string form inline, otherwise a heap
pointer), the compiled block at `+0x20`, the study block at `+0x28`. The exec
wrappers read exactly those two pointers, so `+0x28` is always null in this
build — no `pcre_study`.

`RegexCompile` `0x141b39f70` (140 B) builds the option word from its caller's
flag bits:

| caller bits | option word |
|---|---|
| base | `0x20000801` = `PCRE_UCP` \| `PCRE_UTF8` \| `PCRE_CASELESS` |
| bit 0 clear | `| PCRE_NO_AUTO_CAPTURE` (`0x1000`) |
| bit 1 set | `| PCRE_MULTILINE` (`0x2`) |
| bit 2 set | `| PCRE_DOTALL` (`0x4`) |

So a compiled pattern is caseless, UTF-8 and UCP by default, with automatic
capture off unless the caller asks for groups.

| address | bytes | shape |
|---|---:|---|
| `RegexCompile` `0x141b39f70` | 140 | compile the holder's string; store the code at `+0x20`; return success |
| `RegexCaptureSpans` `0x141b39d10` | 247 | `PCRE_INFO_CAPTURECOUNT`, then `NAMETABLE`/`NAMECOUNT`/`NAMEENTRYSIZE`, then emit one `{offset, length}` record per non-empty group |
| `RegexTest` `0x141b3a000` | 64 | `PcreExec` with no ovector; `true` on a match |
| `RegexScanCaptures` `0x141b3a040` | 223 | exec in a loop, keeping the capture spans of each match that reports more than one group |
| `RegexScanAll` `0x141b3a120` | 274 | the same loop without span extraction; resumes at the last non-empty group's end |
| `RegexTestCaptures` `0x141b3a240` | 151 | exec with a **30-int (15-pair) ovector**; if more than one group matched, keep the spans |
| `RegexTestCapturesNamed` `0x141b3a2e0` | 552 | exec as above, then assign each group's text into a `0x48`-byte record (string at `+0x20`, offset at `+0x40`, length at `+0x44`) and register each named group's name through the name-keyed helper `0x141bba6a0` |
| `RegexAnyTest` `0x141b428c0` | 130 | over an array of holders: `RegexTest`, returning on the first hit |
| `RegexAnyScanCaptures` `0x141b42950` | 153 | the same over `RegexScanCaptures`, scanning every holder |
| `RegexAnyScanAll` `0x141b429f0` | 153 | the same over `RegexScanAll`, scanning every holder |
| `RegexAnyCapturesNamed` `0x141b42a90` | 153 | the same over `RegexTestCapturesNamed`, scanning every holder |

The array variants take the subject length once (a `strlen` leaf at
`0x141bb7cb0`, which has no `.pdata` entry because it is three instructions
long) and pass it to every holder.

**How the ovector count was established.** Ghidra has no prototype for
`pcre_exec`, so every listing above prints four arguments. Windows x64 passes
arguments 5 to 8 of `pcre_exec` (startoffset, options, ovector, ovector count)
in the four stack slots at `[rsp+0x20]..[rsp+0x38]`, and each wrapper fills
exactly those slots immediately before the call: `0`, `0`, the address of a
128-byte stack buffer, and the literal `0x1e` (30 ints, 15 pairs).
`RegexTest` is the exception - it passes four explicit zeros, so it has no
ovector and cannot report captures.

The named-group path takes the group number from a name-table entry the way
PCRE stores it — two big-endian bytes at the entry start, the name at `+2` —
and adds the record base index to it.

**One latent overrun, recorded not reproduced:** `RegexTestCapturesNamed` loops
over `PCRE_INFO_CAPTURECOUNT` groups but reads its pairs from a 30-int
ovector. A pattern with more than 15 capture groups reads past that buffer.
Whatever the port does, it should clamp; a 1:1 copy would copy the overrun.

## What this means for the rewrite

Regex is a **call**, not a reimplementation: the Rust side takes a crate and
must reproduce the *binding's* contract — caseless + UTF-8 + UCP + no auto
capture by default, the caller's three flag bits, `{offset, length}` capture
records, named groups resolved through the name table, and "match any of the
holders in this array".

What the binding is *used for* is not read: its callers are spread across
input (`0x1405bf230`), the camera, the mover and Statescript
(`0x14072f4f0`, `0x14073a600`, `0x14086a820`, `0x140997ed0`, `0x140998150`,
`0x140a7b780`, `0x141b1db50`, `0x1407bc7f0`, `0x1407bc8c0`, `0x1407bc9d0`,
`0x14083ba80`, `0x142233d70`, `0x142271050`, `0x142271400`), and no read site
passes gameplay state into it. That is why the systems table lists regex as not
in scope.

## Open

- The exact PCRE release: the image carries no version string.
- Whether the JIT is linked and unused, or absent.
- `0x1423bc060` and the rest of the `0x1423bd1c0..0x1423c0000` block
  (`pcre_get_substring`, `pcre_config`, `pcre_study` …): unread.
- The 694-caller helper `0x141bba6a0` the named path uses to register a group
  name: read elsewhere as a name-keyed lookup (`formats/static-collision.md`),
  but its contract at this site is unread.
- What the ~14 call sites match against: the patterns are built at runtime, so
  nothing in the install says which globs or conditions they encode.
