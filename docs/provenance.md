# Method and provenance

How the facts in `docs/formats/` were established, what is verified as opposed to
believed, and what is still open. The point is that a reader can tell the
difference without re-doing the work. The inverse index — every fitted,
assumed, tier-3 or stand-in behaviour in the simulation — is
[`fitted-ledger.md`](fitted-ledger.md).

## Rules

1. **Nothing from the install is committed.** The `.gitignore` is a whitelist.
   Art, models, sounds, maps, locale data, extracted tables, keyring contents and
   per-build key tables all stay on the machine.
2. **No decompiled code is committed.** Decompilation *is* used, and has been
   since the Domino physics work — but only its *conclusions* are written down,
   never its output. A finding is recorded as "function at `<address>` does X,
   because Y", naming the capture and the translation, so it can be re-derived.
   That is the rule `AGENTS.md` rule 6 sets, and it is what the rest of this
   document means by evidence class *decompiled*. No disassembly, no
   pseudocode, and no decompiler output is committed or quoted into `docs/`.
3. **Read the real files rather than guessing.** A format claim is only written
   down once something byte-level agrees with it.

## How claims are checked

Every load-bearing claim has a check that fails if the claim is wrong. These are
the ones that mattered:

| claim | check | what a wrong answer looks like |
|---|---|---|
| index entry layout | follow a candidate to its payload and require `BLTE` | payload bytes are not a container |
| index `size` endianness | the encoding file's size must equal the build config's `encoding-size` + 30 | a size several times the archive's length |
| encoding file section ordering | the root file's content key must land exactly on a page boundary | key found in the file but unreachable through the page index |
| BLTE key name and IV | an encrypted chunk must decrypt to a mode byte of `N` or `Z` | an unknown chunk mode |
| Salsa20 configuration | published ECRYPT reference keystream | wrong bytes for every encrypted chunk |
| manifest AES key length | the decrypted record list must be GUID-sorted | records in no order; AES-128 fails, AES-256 passes |
| manifest key derivation | the derived key must decrypt to a sorted list and a plausible record count | noise |
| asset GUID type | reproduce the published `types.txt` permutation table, and 132 heroes on the `0x02E00000` prefix | counts nothing like the expected landmarks |
| asset-ref deobfuscation | every hero's entity reference must exist in the registry | GUIDs that resolve to nothing |
| CRC64 polynomial | rebuild the published table from the polynomial | table mismatch |
| hero colour | four floats in 0..1 with alpha exactly 1, matching known heroes | arbitrary floats |
| health tokens | three pools split as the game splits them (Zenyatta 50 health / 150 shields, Symmetra 100 / 100) rather than a single total | totals that cannot be split |
| pool identity | armour and shield values equal the published live figures exactly, while health differs systematically | a token that matched nothing, or one that matched by coincidence |

Two of these were found by *failing* them: the index size endianness and the
Salsa20 configuration. Both produced plausible-looking output. That is the
argument for having the checks rather than eyeballing results.

## What came from where

* **Behaviour — the client itself.** Where a question was about *behaviour*
  rather than data, the answer came from decompiling the operator's own
  memory capture, read-only, through the REA/Ghidra tooling over
  `D:/ow-dumps` (outside the repo). This covers the Domino integration order
  and its damping formula, the fixed 1/30 s step and its "commanded, not
  accumulated" counter, the mover's runtime slot map, and the negative results
  that justify declaring a stage tier 3. Each is recorded as an address plus a
  conclusion; the working captures and the Ghidra project stay untracked. See
  `docs/formats/domino.md` and `docs/formats/character-controller.md` for the
  address algebra.
* **The container, BLTE, the encoding file, the manifest and STU** — worked out
  by reading a real installation, then cross-checked against
  overtools/OWLib and TACTLib. Where the two disagreed, the files won: the
  reference tooling's index handler gives the entry layout but its comment about
  the maximum addressable size does not match this build, and its manifest
  derivation asks for a 128-bit key where the file needs 256.
* **Key material that is not ours** — read at runtime, not vendored. The
  per-build 512-byte manifest table comes from a checkout the user points at
  (`OW_TACTLIB`) or a file they supply (`OW_KEYTABLE`). A supplementary keyring
  comes from `OW_SUPPORT_KEYRING`. See `ow-tank/src/keytable.rs` and
  `ow-casc/src/install.rs` for how, and `CREDITS.md` for why.
* **Type and class names** — the numbers are from the install; the *names* are
  from which registry the reference tools switch on. A name is a claim about
  meaning, so `docs/formats/stu.md` marks which ones were confirmed by reading
  values and which are only labels.
* **Structure and practice** — from the three reference projects. What was taken
  from each is itemised in `CREDITS.md`.

## External sources for cross-checking

Nothing below is vendored. These are places to check a number against, ordered by
how usable their licence is for a project like this:

| source | licence | what it is good for |
|---|---|---|
| [OverFast API](https://github.com/TeKrop/overfast-api) | MIT | machine-readable OW2 hero structure: role, subrole, and `hitpoints {health, armor, shields, total}`. Scrapes Blizzard's hero pages, so it carries no damage or cooldown numbers. |
| [workshop-rs](https://github.com/wrightkit/workshop-rs) | MIT | a canonical multi-locale Workshop catalogue of actions, values and parameters. The best permissively-licensed parameter list. |
| [Overwatch Wiki](https://overwatch.fandom.com) | CC BY-NC-SA 3.0 | the actual combat numbers — damage per bullet, falloff ranges, ammo, spread, cooldowns, ultimate cost, movement speed. **Non-commercial**, so usable to *check* a value, not to build a dataset from. |
| [OverPy](https://github.com/Zezombye/overpy) | GPL-3.0 | custom-game settings schema; copyleft, so read-only. |
| [OSTW](https://github.com/ItsDeltin/Overwatch-Script-To-Workshop) | none stated | element documentation. All rights reserved, so read-only at best. |

For architecture rather than data, the primary source is Blizzard's own GDC 2017
talk, *Overwatch Gameplay Architecture and Netcode* (Timothy Ford), available
free on YouTube. It describes the entity-component model and rollback netcode,
which is the shape a combat model here would need to match. Tick rate: this is
where "tick rate" is ambiguous and the ambiguity caused a stale row in
`rewrite-plan.md`, so be precise. The **network/server** update rate is a
different quantity and is *not* established here — community measurements put
launch Overwatch at roughly 21 server updates a second and later builds at 63,
with no Blizzard statement and no measurement for the current build. The
**client physics step** is a separate number and *is* established: 1/30 s,
decompiled from the module dt table (`DAT_7ff7c7d5d84c` = 0.0333333351, the
table's `+0x0c` entry), with one step per frame. `ow-sim`'s `TICK_RATE_HZ` is
that physics step, not a network rate.

Two things were searched for and **not found**, which is itself useful to know:
no public parser decodes replay frame payloads into positions or damage events
(the containers and their metadata are parsed, the frames are not), and no public
Ghidra or IDA write-up of the combat executable exists. Material that does exist
on the latter is cheat-oriented and was not consulted.

## Development scratch

`dev/` holds the decomp/analysis toolchain and the working data: the read-only
process capture (`dump_process.py`, `regions.py`), the practice-range capture
helpers (`capture_range.py`, `find_mapchunks.py`, `watch_chunks.py`,
`live_mapscan.py`), the decomp searches (`disasm_field_sites.py`,
`find_guid_constants.py`, `find_hash`-style constant scans), per-build manifest
support (`provider_interp.py`, `cmf_search.py`), capture viewing and Ghidra
import (`peek.py`, `flatten.py`), and the current question's working files
(GUID and value lists being tested against candidate transforms). It also holds
the process captures themselves and the Ghidra project (`ghidra_run/`) built
on them.

None of it is part of the product; `.gitignore` excludes all of it. The
one-off format probes that used to live here were deleted once
`docs/formats/` carried their verified results — the document is the record,
not the probe. Captures are re-derivable by re-running the capture scripts
against a running client, which is the operator's decision to make (see the
roadmap's Warden note).

## Open questions

Answered by reading, but not fully:

* 62 unknown trailing bytes in the encoding file.
* 10 bytes after the data header's size field, which are not zero-padding.
* The `unknown` byte in each manifest record; the 357 "entry" records.
* The meaning of the index header's first `i32` (16 in the reference files).
* The 512-byte manifest key table's construction. It is opaque; only its use is
  understood.
* Whether any combat behaviour is *not* data-driven and would need the
  executable read. Blizzard's engineering talks describe a server-authoritative
  simulation with a fixed tick; if the numbers live in files and only the
  integration lives in code, reading the files is enough, and that is the
  assumption to test next rather than an established fact.

Not started:

* Per-shot damage and cooldowns: the graphs are read and their parameters
  named, but the damage values do not surface; see `docs/formats/weapons.md`.

Closed since first written:

* **Version 1 STU.** The reader is implemented (`ow-stu`'s `parse_v1`, reached
  from `parse` when the blob opens with the `STUD` magic). It is covered only by
  synthetic tests — there is no v1 asset on this install to check it against,
  so treat it as **not tested against real data**. See `docs/formats/stu.md`.
