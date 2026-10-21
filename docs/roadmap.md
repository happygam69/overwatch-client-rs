# Roadmap: the rewrite

The goal, stated exactly: **a Rust rewrite of Overwatch, built on Bevy, that
reads its data from the operator's own installed copy at runtime and matches the
original's actual behaviour.** The first-person practice range — geometry, bots,
HUD, combat — is the *test bed* for that claim: systems are matched by running
them the way the game runs them. A terminal harness is not the target.

This document exists because the project drifted once already, and it is the
accounting of what exists, what blocks, and what is next.

The concrete plan for the two systems the rewrite is currently racing —
movement and combat, to bit-level equivalence with the original — is
[`docs/rewrite-plan.md`](rewrite-plan.md): it sequences the remaining
reverse-engineering, the `ow-sim` implementation, the Bevy shell, and defines
exactly what "matching" verifies. This file keeps the whole-project
accounting.

## How behaviour gets established

The game is too large for behaviour to be read off the files: a 65 MB binary
with code generated and protected at runtime, ~1.5 million assets, behaviour
spread across state graphs, engine code and unnamed data. **Matching the
original's actual behaviour therefore happens mainly through decompilation —
Ghidra against the operator's own process capture (`dev/`, gitignored)** —
while the files establish structure. Record the addresses and the conclusion,
never the disassembly; re-derive from the same capture rather than trusting a
note. `docs/formats/materials.md` is the worked example of the rule.

## What exists

| | state |
|---|---|
| CASC container, manifests, STU | working and verified |
| hero health / armour / shields | 127 of 135 heroes |
| weapon ballistics (rates, speeds, lifetimes, ranges) | 58 graphs with named parameters |
| map chunks, models, geometry | read; `ow-view` renders them |
| practice range | map identified: 31 distinct chunk contents |
| combat simulation (hitscan, projectiles, spread, falloff, pools, armour) | working, unit-tested |
| per-shot damage | **not in the files** — see `docs/formats/weapons.md`; capture search prepared |

## What the rewrite still needs, and where each piece stands

| layer | state | notes |
|---|---|---|
| **map geometry** | read and rendered | **45,302 map-chunk assets**, type `0x0BC`, placeables resolved to models |
| **models** (bots, props, weapon view models) | partially read | 30,402 assets of type `0x00C`; geometry at LOD 0 works, materials do not |
| **materials and textures** | **blocked; decomp in progress** | the model→material link is opaque; ruled-out schemes and the client-side map are in `docs/formats/materials.md` |
| **shaders** | **not reproducible** | the shader system is proprietary. Geometry and textures can be faithful; a pixel-identical look cannot. |
| **renderer** | exists, untextured | a `wgpu` window, first-person camera, depth buffer; surfaces shaded from normals |
| **character controller** | **implemented and verified** | movement, gravity, acceleration, stand-in collision; behaviour measured from the client's traces and gated per write by `trace_diff` (`docs/formats/character-controller.md`; crouch transition timing and the steep-slope slide remain open) |
| **combat numbers** | blocked | not in the files; see the Ghidra section |
| **HUD** | not started | health, ammo, crosshair, damage numbers, the wall panel from the range |

The asset side is the bulk of the work. Structure needs no reverse engineering
— the formats are documented and the assets are on disk — but **behaviour does**:
what the code does with those assets is established by decomp, per the note at
the top.

## Ghidra: what it can and cannot give

The expectation was that Ghidra supplies the missing reference point for
behaviour. Half true, and the half that is false matters.

**`Overwatch.exe` is encrypted at rest.** Measured on the retail binary:

```
.text    48,602,668 bytes   entropy 8.000   XR
.rdata   10,104,740 bytes   entropy 6.488   R
.data    13,202,108 bytes   entropy 4.558   WR
```

Entropy 8.000 is the maximum for byte data: the code section is not code on
disk. A normal compiled `.text` sits near 6.0–6.5. Consistent with that, the
whole 65 MB binary contains only **496 KB of printable strings**, and none of the
identifiers a combat implementation would want — `numShotsPerSecond`,
`projectileSpeed`, `HeroSetting` and `m_maxHealth` all occur zero times.

So loading the on-disk exe into Ghidra gives the loader and the decryptor, not
the simulation. It has been imported anyway (117,223 functions) so the
environment is ready, but **static analysis of the file as shipped will not
produce the combat constants.**

Getting at the decrypted code means capturing the image from a running process.
That is a different kind of step, and two things need saying plainly:

1. It is circumventing a protection layer rather than reading files. Whether
   that is acceptable is the operator's call, not a decision to make quietly.
2. **The client runs Warden.** Attaching a debugger or a memory dumper to a live
   session risks the account. That is a concrete cost to the person asking, and
   it should be weighed before, not after.

The policy that follows from both: **structure comes from the files; behaviour
comes from the capture.** The captures already taken (`dev/dumps_range` and
friends, read-only `ReadProcessMemory` dumps) are the standing reference for
decomp work and carry no further risk — they are the primary instrument for
matching the original's behaviour, per the note at the top. *New* captures mean
attaching to a live session again, and that stays the operator's decision, made
with the Warden cost stated rather than assumed.

## Preparing for a game update

The client forces updates, so the build this project was verified against
(`2.23.0.3.151177`) will be replaced. Two things were done so that the move costs
little:

**The key derivation no longer has to be hand-written per build.**
`ow-tank`'s manifest reader (`crates/ow-tank/src/manifest.rs`, key table in
`keytable.rs`) reads the provider's `Key()` and `IV()` source and interprets
it. Validated against build 150851: it reproduces the key and IV that
are independently confirmed by decrypting the real manifest, byte for byte. So a
new build is *evaluated* rather than guessed at.

It currently covers 47 of the 253 providers. The rest use constructs the
interpreter does not yet handle, and the list is known rather than unknown:

| missing construct | providers | example |
|---|---|---|
| declaration-with-initialiser and named constants (`uint kidx = ...`, `const uint increment = 3`) | 111 | `ProCMF_35328` |
| `switch (SignedMod(kidx, 3))` with small integer cases | 38 | `ProCMF_100672` |
| `kidx -= ...` and `--kidx` | 38 | `ProCMF_101600` |
| `buffer[i] = digest[...]` rather than `Keytable[...]` | 8 | `ProCMF_111387` |
| ternary `cond ? a : b` | 10 | `ProCMF_105760` |

All of them are small additions to the same interpreter; none needs a different
approach. Whether the new build falls in the covered 47 or needs the extension is
unknown until its build number is visible, which is why the failure list is
written down.

**A missing CDN config is no longer fatal.** During a patch the agent deletes the
outgoing build's CDN config before rewriting `.build.info`, leaving the install
briefly pointing at a file that no longer exists. Nothing in a local read needs
that file, so its absence now prints a note and continues. This was a real bug
that made the tool fail on an install that was merely mid-update.

## Staged plan

Each stage is meant to be visibly closer to the range.

1. **Chunked-data reader.** `ow-tank` gains a reader for the chunk container
   (header magic `0xF123456F`, chunk table, 4-character XOR-reversed chunk tags).
   This is the prerequisite for every asset.
2. **Map geometry.** Read a map-chunk asset, get its vertices and indices, and
   dump a mesh to a file. Verify by geometry statistics and by a rendered still.
3. **A window.** `wgpu` + `winit`, a depth buffer, a free camera, the map mesh.
   This is the first point at which the project *looks* like the target.
4. **Textures and materials.** TXTR decode to RGBA, sampled with approximated
   material parameters.
5. **Models.** Skeleton, RenderMesh and Hardpoint chunks, so the training bots
   and the weapon view model appear.
6. **Character controller.** Movement and gravity: **done** — the arithmetic is
   measured from the client's own captures, not fitted (see `rewrite-plan.md` §
   M6 and `STATUS-handoff.md` § movement). **Collision against map geometry is
   not done**: the meshes are drawn but not solid, and the play harness says so
   in `ow-view/src/play.rs` rather than pretending. Standing in for it is a
   ground plane plus four axis-aligned boxes, and the client's real ledge and
   slope handling is unmodelled (tier 3). Open sub-item.
7. **Combat and HUD.** The simulation already exists; it needs wiring to the
   rendered world plus health, ammo, crosshair and damage numbers.
8. **Bots.** The practice-range bots as entities with the health pools already
   extracted.

Stages 1–3 are the critical path and retire the largest risks: whether the
geometry can be read at all, and whether it renders. Everything after that is
incremental.

## What "1 to 1" can and cannot mean

Achievable: the same map, the same layout, the same bots, the same hero
statistics, the same weapon behaviour, a playable first-person camera, and a HUD
carrying the same information.

Not achievable: pixel-identical rendering. The shaders are proprietary and are
not shipped as data in a form that can be reimplemented from the install alone.

Worth stating once so it is not discovered late.

## Capturing the running image

The static file is useless for this, so the reference point has to come from the
process. What that took, and what it found:

**Where the code actually is.** Dumping only the image sections is not enough:
of the 358 MB of image regions, none contained a single known STU field hash.
The address space says why — the process holds 10.1 GB of committed private
memory of which only **21 MB is executable**, alongside **1,684 executable
mapped regions**. That shape is a client whose real code is generated or mapped
at runtime rather than left in the image.

Dumping the executable non-image regions — 1,706 regions, 79.3 MB — finds the
code. Known field hashes appear there as immediates:

| hash | field | occurrences |
|---|---|---|
| `0x07DD813E` | `m_value`, the ConfigVar literal | 142 |
| `0xAF872E86` | `ModifyHealth.m_amount` | 20 |
| `0x26E68021` | `projectileSpeed` | 3 |
| `0xBA490EB9` | `projectileLifetime` | 3 |
| `0x02F3F5D4` | `numShotsPerSecond` | 1 |
| `0xE25DDDA1` | `m_heroColor` | 1 |
| `0xC71EA6BC` | `m_graph` | 5 |

That is the reference point: the code that reads the fields the data layer could
not name is present, and each field's hash can be located at an absolute address.

**Method** (`dev/dump_process.py`, read-only, output gitignored):
`OpenProcess` with query and read rights, walk the address space with
`VirtualQueryEx`, keep committed readable regions, read each with
`ReadProcessMemory`, and write one file per region plus a manifest of
`base size protect type`. Keeping the addresses is what makes the capture
usable: a concatenated blob cannot be mapped back to a virtual address, and
without an address nothing can be loaded into a disassembler.

Also worth recording: the image sections *are* decrypted in memory — the 63 MB
region that is the executable's text drops from entropy 8.000 on disk to 6.433
in the process — but that is the loader's image, and it is not where the combat
code lives. Entropy alone would have suggested success.

**Next**: load the captured regions into Ghidra at their recorded addresses and
work outward from the hash immediates — find the functions that read
`numShotsPerSecond` and `m_amount`, and the damage field follows from what they
touch.

## Reading the capture: the address space, and a second opinion

Two toolchain facts were established by measurement this round, because no
reading is trustworthy without them.

**One address space per capture.** Every captured region file is named
`base_size_type`, and the module dumps keep the capture's own addresses: their
file offset for a byte is `VA - 0x7ff649e60000`, verified by matching whole
regions byte-for-byte (all 37 pages of one 148 KB region, plus sampled windows
in two more, including through the Ghidra program's own memory reads). With the
Ghidra project's program based at `0x7ff7c4d20000` and loaded flat, the
conversion the docs' capture coordinates need is

```text
ghidra address = capture address + 0x17AEC0000
```

The docs keep capture coordinates (`0x7ff64b...`) because those are the
coordinates the region files and the manifests were taken in. One trap worth
recording: `module_warm.bin` carries the *same build* with its content shifted
`-0xC00` in file offset, so the two dumps are interchangeable by address but
never by offset.

**A second decompiler, on the same bytes.** The REA MCP bridge drives its own
headless Ghidra, but its Windows profile admits PE targets only, and the
synthetic module PE will not import (its data directories are junk the loader
rejects: invalid export/debug/TLS RVAs). The fix is `dev/mkpe.py`: it wraps one
or more captured regions as a minimal, valid PE — no directories, one section
per region, `ImageBase = lowest region base - 0x1000` — so every address inside
the image equals the capture address again. Verified by decompiling the same
functions through both stacks and by byte comparison; the count-writer sites
in `docs/formats/materials.md` were read this way, from regions the module dump
does not carry at all.

## The update landed, and the reference derivation is wrong

The client updated to `2.25.0.0.154088`. What happened next is the clearest
evidence yet for why the executable matters.

The provider interpreter evaluates `ProCMF_154088` without error, and its output
is **provably correct**: it matches a hand-written implementation of the same
source byte for byte, for both the key and the IV.

And that key does not decrypt the manifest. Four combinations were tried —
32-byte and 16-byte keys crossed with AES-256 and AES-128 — and all four produce
a record list that is not GUID-sorted, which is the validity check that caught
every wrong derivation so far. The header is not the problem either: the magic
still reads `0x636d661b` at the same offset, so the body still begins at 48.

So the derivation is right and the *reference* is wrong: the tooling's provider
for the newest build does not match the newest build. That is the failure its own
error message warns about ("Manifest decryption failed ... can happen due the game
receiving a new patch that is not supported by this version"), and it means there
is no trustworthy reference for the newest manifest format. This is precisely the
situation the executable can settle and nothing else can.

### What the capture gives us instead

From the running process, 1,706 executable regions with addresses, containing
known field hashes as immediates, and 40 `cmp` sites against them (`cmp edx,
imm32` 33 times, `cmp ecx, imm32` 7 times). The engine dispatches on field hash,
which is the thread to pull:

* the set of hashes the combat code compares against is the set of fields that
  matter, extractable without any name table;
* each comparison is a labelled case, so the code under each one says what the
  field *does*;
* the manifest decryption routine is in the same captured image, so the key
  derivation can be read rather than sourced from tooling that is wrong.

That is the work now: disassemble around those 40 sites and around the manifest
crypto, from the captured regions, at the addresses the manifest records.

## Where this stands now

The section above ends on a diagnosis that turned out to be wrong, and the way it
was wrong is worth keeping.

### The manifest was never encrypted

The manifest being read was `Win_SPWin_RCN_EExt.cmf`. The install also carries
`WinPrism_SPWin_RCN_EExt.cmf`, for the same region, same build, same kind — and
**that one is in the clear**. Read little-endian, its magic spells `cmf`
followed by the version, so `IsEncrypted()` is false and there is no key to
derive at all.

So the failure was not "the reference derivation is wrong for this build". It was
"a plaintext manifest was handed to a key derivation", which fails for every
build. Both diagnoses fit the same symptom — garbage records — and the wrong one
was recorded first because only one of the two files was ever opened.

The lesson is in the shape of the fix: the selector now returns *candidates* and
the caller keeps the first that opens. Nothing in a file name says which of two
same-named manifests will parse, and a build can ship both an encrypted and a
clear one.

The 154088 derivation in the reference tooling is still unverified, and the
keytable it names *is* present in the client's own memory, so the constants are
right and the derivation is what differs. A search over the plausible seeds and
strides produced nothing that satisfies the format's own check — `entry[0].m_index
== 1`. Nothing needs it: the plaintext manifest carries 1,395,411 records.

### What reads now

| layer | state |
|---|---|
| container, buckets, BLTE, encoding, root file | works |
| content manifest | works, plaintext path, 1.5M records |
| structured data (STU v2) | works, including `teString` and inline arrays |
| chunked data (MDLC/MRNM/MSTU) | works; 38 of 40 models read geometry |
| map chunks and placeables | works; every model reference resolves |
| models to vertices and indices | works; 58 of 60 models produce geometry |
| rendering | works; wgpu, first-person camera, real map geometry |
| textures and materials | **not read** — surfaces are shaded from their normals |
| character controller and collision | **implemented and verified** — behaviour measured from the client's traces and gated per write by `trace_diff`; collision is a stand-in (plane + boxes), the map mesh is not solid (`docs/formats/character-controller.md`) |
| combat in the range | data side reads; not yet wired to the view |

### The practice range is not identified

This is the honest gap against the stated goal, and it is a *naming* problem, not
a reading one.

The install ships 42 `STUMapHeader` assets and none of them is the practice
range. The string "Practice Range" appears exactly once in the whole of the
readable data — inside a statescript graph that is the mode's UI, not a map. The
map headers' `m_map` references resolve to an asset type holding nothing but
empty bounding boxes, and their layer arrays are empty, so **the map-to-chunk
link is not in the data this program can read**.

What is left is naming: a map's name comes from a text asset, and the text
manifests — all nine of them, 124,000 records each — point at assets this install
has not downloaded. The UI strings are in the executable's image instead, which
is why "Practice Range" and "Training Bot" are findable in a memory capture and
not in the files.

Two ways forward, and the first is cheaper:

* group chunks by where their contents are and render each group, then recognise
  the practice range by eye — `ow-view --clusters`, `--tour` and `--shot` exist
  for exactly this;
* recover the map-to-chunk link from the executable, where the game itself walks
  it.

## The practice range is found

The map-to-chunk link is not in any file this program can read. It did not need
to be: **chunks belonging to one map tile it**, so the map is recoverable from
where the chunks' contents are.

The seed was one chunk caught in a running game's memory while the practice
range was loaded. A map chunk is BLTE-compressed on disk, so its bytes are only
recognisable once decoded -- and the game decodes a chunk, parses it and frees
the buffer, which is why two whole-capture scans and a four-minute watch found
nothing and one lucky scan found this. It matched three manifest records
byte-for-byte, which is how a buffer with no GUID gets tied back to one.

From there, a proximity walk over all 41,811 map chunks decodes each and keeps
the ones whose placements sit in the same place. 400 chunks landed within 60
units; grouping them by placeable count and centre collapses them to **31
distinct contents**, each shipped under between one and fifty-four GUIDs.

Two things had to be right for that to work, and both were wrong first:

* **The median, not the bounding box.** A chunk carries a few far-away
  placements -- skybox pieces -- whose box covers the whole map. By box distance
  every chunk is a neighbour of every other, and 10,056 chunks came back. By
  median, 400.
* **The seed's own centre is not the map's centre.** The first attempt used the
  seed chunk's box centre and got the same useless answer.

The result renders as interiors: floors, walls, pillars, railings, ceilings, a
curved structure. `ow-view --chunks-from` draws all 31 at once.

This is a *method*, not a table. The GUIDs are specific to one install and one
build, and they live in a gitignored file; what belongs in the repository is the
way of finding them, which needs nothing but the install.

## Textures: blocked at the first hop

The chain to a texture is short and fully documented:

```text
model          MDLC chunk: a material offset and a material count
  material     type 008: four GUIDs -- shader group, shader source, one
               unknown, and the material data
    data       type 0B3: STUMaterialData, holding a reference to the textures
      texture  type 004: a header and DXT/BC payload
```

The first hop is where it stops. `MDLC`'s material array does not hold GUIDs:
reading it plainly gives values whose type field is nonsense (0x81B, 0xA7E,
0x962, 0xAA8 on one model), and none resolve to an asset. They are obfuscated
the way STU asset references are — but not *with* the STU scheme. Five masks
were tried against one model, including the CRC64 of its first 36 bytes (which
is what STU uses), of its first 16, of the chunk's first 16, and of the GUID
itself in both byte orders. None produced a type-008 GUID.

So the encoding is unknown, and it is the one thing standing between a grey map
and a textured one. It is also the kind of question the captured executable can
answer directly: the code that reads those eight bytes is a short function, and
finding it means finding the mask. That is what the Ghidra project is for.

Recording it as blocked rather than guessing is the point. A mask invented to
make four values look like GUIDs would produce four plausible assets and a map
textured with someone else's materials.

### Update: the mask hunt, from the files and from the client

Both routes have now been worked, and the answer is neither a mask nor one of
the usual hashes. [`docs/formats/materials.md`](formats/materials.md) is the
full record: every cheap family is ruled out by a *test* (bit-permutation by
popcount, linear by GCD, thirteen hash families by membership over 99,392
GUIDs), and the client's own material machinery is mapped from the capture —
MDLC deserializer (values copied verbatim), the u32-key material table with
FNV-1a caching, the index-range counts that make models slices of a shared
table, and the per-material construction loop with its type queries. Two code
sites remain to be read for the derivation: the global table builder and
`0x7ff64a44b430`. That document also carries the Ghidra coordinates, so the
reading is reproducible from the same gitignored capture.
