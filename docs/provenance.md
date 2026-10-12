# Method and provenance

How the facts in `docs/formats/` were established, what is verified as opposed to
believed, and what is still open. The point is that a reader can tell the
difference without re-doing the work.

## Rules

1. **Nothing from the install is committed.** The `.gitignore` is a whitelist.
   Art, models, sounds, maps, locale data, extracted tables, keyring contents and
   per-build key tables all stay on the machine.
2. **No decompiled code.** None has been used so far, and no disassembly output
   is committed. If binary analysis becomes necessary for combat behaviour that
   turns out not to be data-driven, it will be recorded in `CREDITS.md` the same
   way the source reading is.
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
which is the shape a combat model here would need to match. Tick rate: community
measurements put launch Overwatch at roughly 21 server updates a second and later
builds at 63; there is no Blizzard statement and no measurement for the current
OW2 build, so treat any figure as unverified.

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
* Version 1 STU.
