# `ow-tank` audit — 2026-11-06

Scope: every module under `crates/ow-tank/src` (53,891 lines), its tests and the
claims in `docs/ow-tank-1to1.md`, `docs/texture-1to1.md` and
`docs/formats/*.md`. Read-only: nothing in the crate was changed for this
audit.

Method: the full gate was run on this machine
(`cargo xtask verify` → all good, 91 test targets, **0 install tests skipped**,
1 ignored: `full_install_material_data_census`), `fn/ChunkGuid` was re-run
through the harness (`VERIFIED`, 364 cases, 100% coverage, 3/3 mutants), and
every module was read. Findings marked **[checked]** were re-read in the source
during this audit; the rest were reported by module review and are listed with
their file and line so they can be checked before acting.

The rule this audit judges against is the project's own: a feature is done
only when the client's code was read, the implementation follows it, and a test
proves identity (`AGENTS.md`, `docs/provenance.md`).

## Verdict

The crate is in better shape than its size suggests, and its documentation is
unusually honest about what is not 1 to 1. The gate is green and the 16
`ow-tank` harness units really do verify. But the gate is green **with defects
that the tests were adjusted to accept**, and the dominant failure mode is the
one the project forbids: an unreadable value is silently turned into a
plausible one instead of being reported open.

Highest-value work, in order:

1. `texture.rs` DDS writer is off by four bytes — every exported DDS/BC asset is
   malformed (P0-1).
2. `mrnm.rs` element copy and `model.rs` index/round-trip paths can crash or
   fabricate geometry (P0-3, P0-4, P1-1..P1-4).
3. `skin.rs` / `clip.rs`: two contradictory pose paths, a test that asserts the
   opposite of its name, and the verified `fn/AnimTimeWrap` not used at all
   (P0-6..P0-8).
4. `entity.rs` array GUID offset, `volley.rs` dead ranking, `domino_world.rs`
   wrong constant (P0-2, P0-5, P0-9).
5. The systematic `unwrap_or(0)` / `unwrap_or_default()` decode fallbacks
   (section "Honesty") — these are the reason a wrong parse reads as data.

## What holds up

- The gate: `cargo fmt`, `clippy -D warnings`, every workspace test target, the
  tool index, the deviation check, the address-frame check and the PE-layout
  check all pass.
- The harness link is real, not decorative: `fn/ChunkGuid`, `fn/BundleLookup`,
  `fn/BundleDirectoryLookup`, `fn/MapChunkCellBytesFixup`, the three `MRNM`
  units, `fn/CmfKey154504`/`fn/CmfIv154504` and six texture leaves are the 16
  `ow-tank` units, and the one re-run here passed end to end.
- Only one `FIT-*` marker remains (`clip.rs:649`, `FIT-ANI-07`), and it is in
  `docs/deviations.md`; no new one has been added.
- The `chunks.rs` container walk (interleaved record/payload, sentinel tag),
  `manifest.rs`'s build-154504 derivation, `mapguid::chunk_guid`,
  `bundle.rs`'s entry lookup and the texture *container* header/descriptor
  arithmetic are exact on the install and have client addresses.
- The install tests genuinely ran here (no "install not readable" note in the
  whole run), so the numbers in `ow-tank-1to1.md` section B were earned on this
  machine, not skipped.

## P0 — output is wrong, or contradicts the read rule

| # | where | what is wrong | why it matters | state |
|---|---|---|---|---|
| P0-1 | `texture.rs:1019` | `to_dds` uses `pf_off = 76` as an index into `hdr`, but `hdr` starts after the 4-byte magic, so `DDS_PIXELFORMAT` (file 76) is written at file 80: `dwSize` stays 0, `dwFlags`/`dwFourCC`/masks shift one word, `hdr[28..32]` puts the mip count in `dwReserved1[0]`, and `hdr[108..112]` puts `DDSCAPS_TEXTURE` in `dwCaps2`. `tests/bc6h.rs:278` pins the shifted fourCC (`dds[88..92]`) and `to_dds_writes_a_bc1_dds` only checks `len() > 128`. | No DDS reader can find the fourCC; every BC export is unreadable and the DX10 block is never reached. The test cannot fail because it asserts the same wrong offset. | [checked] |
| P0-2 | `entity.rs:550` | `asset_ref_at(base + 4 + k*16, …)` reads the GUID at the *element start*; a 16-byte `teStructuredDataAssetRef` array element carries it at `+8` (`ow-stu/src/types.rs:1047`, and `unlocks.rs:2854`/`sound.rs:2998` read `+ k*16 + 8`), so the code deobfuscates the 8-byte pad. | `loadout_guids_of_hero` returns garbage GUIDs, never a usable loadout ref, and there is no error. No test pins a non-empty list. | [checked] |
| P0-3 | `mrnm.rs:527` | `out[ELEMENTS..ELEMENTS+bytes]` copies `ELEMENT_COUNT (u8) * 8` bytes into a 0xf8-byte record whose element region is 0xdc bytes. Any count above 27 makes `copy_from_slice` panic on a length mismatch. | A malformed or future model aborts the tool instead of being rejected. | [checked] |
| P0-4 | `model.rs:891` | the index buffer's `format` word is read and discarded (`let (_, _, index_at)`), and every index is read as `u16`; `Vec::with_capacity(indices_to_draw)` at `model.rs:892` takes a file `u32` (up to 8 GiB) before any bound check. | A 32-bit index buffer silently yields garbage triangles; a corrupt count aborts on allocation. | [checked] |
| P0-5 | `volley.rs:451` | `if !out.has_volley \|\| (rate, pellets) > best_key` — `has_volley` is only set at line 471, after the loop, so the guard is always true and the *last* graph with a volley overwrites the best one. The documented order ("literal non-999 rate, then pellets, then graph order") is never applied. | The chosen weapon graph (and therefore the volley/damage candidates) is whichever graph happened to be last, not the ranked one. | [checked] |
| P0-6 | `clip.rs:1889`, `3050` | `frame_at` computes `(seconds*fps).rem_euclid(frames) + 1.0`. The repo's own verified `fn/AnimTimeWrap` (`0x140b91ef0`) is a min/max clamp, or a truncating modular wrap only when the range flag bit 0 is set; the sampler ignores it and never applies the read wrap shaping at node `+0x24` (0 none, 2 mirror, 3 ping-pong, 4 square, 5 inverse, `animation.md:211`). | Negative/overshooting times sample a different frame than the client, and cyclic clips hold the last key instead of wrapping/mirroring. The verified reconstruction exists in the harness and is not shipped. | [checked] |
| P0-7 | `skin.rs:217` vs `skin.rs:266` | `posed_world` replaces the **absolute** bone rotation, then re-derives locals; `posed_world_full` replaces the **local** matrix. Both cite the same evidence lines and the same capture addresses. `animation.md:203` records the read rule as "a pose replaces the bind's local matrix". | One of the two is wrong. In the absolute path a posed bone's children do not follow (the code derives their locals from their own unposed absolutes). `ow-scene` renders the absolute path. | [checked] |
| P0-8 | `skin.rs:598` | the test `posing_a_bone_carries_its_children` asserts "an unposed descendant keeps its bind position" — the opposite of its name and of the property it was written to prove; `the_pose_pivot_is_multiplied_on_the_left` likewise no longer checks multiplication order. | The suite green-lights P0-7. A test was adjusted to match the implementation, which is precisely what the verification rule exists to prevent. | [checked] |
| P0-9 | `domino_world.rs:43` | `WORLD_TABLE: u64 = 0x1442_0bc0` is `0x14420BC0`; its own doc line 41, `formats/static-collision.md:598` and `ow-sim/src/world.rs:25` all say `0x1442a0bc0`. A digit was lost when the literal was re-grouped. | A documented address that is wrong is worse than a missing one: any consumer reads 0x98000 bytes into the wrong global. Currently unused, so nothing fails. | [checked] |
| P0-10 | `mapchunk.rs:691` | `read_placeable` computes `body_end = offset + size` (line 663) and never uses it; the model arm reads to `body + 0x68` bounded only by the asset. | A placeable whose declared `size` is `COMMON_SIZE + 1` reads the next placeable's UUID as its look/GUID/transform — invented geometry and an invented asset reference, which `range_world` then places. | [checked] |
| P0-11 | `collision_level.rs:194` | the triangle-corner check tracks only `worst` (`if corner > worst`), so it fires only when *every* corner is negative. A record `(-1, 5, 6)` passes. | A negative index is accepted and the sweep then silently reports no hit for that triangle instead of a parse error. | [checked] |
| P0-12 | `brush_convert.rs:986` | `ctx.hull[en as usize]` and `[pn as usize]` are indexed *before* the `en < 0 \|\| pn < 0` guard on the next line; `-1 as usize` is a huge index. | A ring with an unset neighbour panics instead of returning `Error::MalformedRing`, which is what every other path in the module does. | [checked] |
| P0-13 | `mapchunk.rs:133` vs `chunkkind.rs:127` | `Kind::type_id` maps `Model = 0x08`, `SingleModel = 0x02`, but the client's read table at `0x1439aa320` has no id `0x08` and no `0x02` (ids present: 0x00,0x01,0x03,0x04,0x06,0x07,0x09..). The install sweep `tests/mapchunk_kinds.rs:697` classifies real chunk assets by this mapping. | Either the mapping is from the reference and should be labelled as such, or the test classifies real chunks by ids the client does not have. Which one is wrong is not recorded anywhere. | [checked] |

## P1 — a bad or hostile file takes the process down

These are library functions reachable from asset bytes; a panic here is a
crash, not a test failure.

| # | where | what |
|---|---|---|
| P1-1 | `chunks.rs:209` | `walk` recurses once per nesting level with no cap; a crafted container of N/16 nested records overflows the stack. Reported twice, independently. |
| P1-2 | `shader.rs:177` | `Vec::with_capacity(uncompressed_size as usize)` reserves up to 4 GiB from a `u32` header word before the gzip stream is validated; `read_to_end` on it is unbounded too. |
| P1-3 | `cloth.rs:368`, `381` | `read_i16_array`/`read_f32_array` cast a stored `i64` offset straight to `usize`; a negative offset wraps and reads unrelated bytes (release) or panics (debug). `clip.rs:2449` already has the right `land` check. |
| P1-4 | `collision.rs:276`, `collision_level.rs:121`, `chunkkind.rs:535`, `678` | unchecked `offset + length` on file-derived values: debug panic, release wrap-then-pass. `checked_add` is used elsewhere in the same files. |
| P1-5 | `model.rs:944` | `vec![Vec::with_capacity(wanted); uv_set_count]` sizes 1..256 sets of `vertices_to_draw` entries from file counts (>100 MB per submesh). |
| P1-6 | `mesh_world.rs:286` | `push` validates with `assert!`/`assert_eq!` and is called from `RangeWorld::build`, which returns `Result`; one bad model panics the world build. |
| P1-7 | `clip.rs:3006` | `BlendPose::decode` slices `file[0x80+i*4..0x84+i*4].try_into().unwrap()` with unchecked indexing, and hardcodes `0x80` while its own comment says the ids come from section [1]'s offset. |

## Honesty — an unreadable value becomes a plausible one

This is the dominant pattern and the one that makes the other defects hard to
see. Each of these turns "cannot tell" into "a number", and none of them is
covered by the 1-to-1 doc:

- `model.rs:663` — a short `MDLC` or non-positive material pointer becomes an
  empty material list (same answer as "no materials").
- `model.rs:712` — a chunk whose payload lies outside the asset is `Ok(None)`
  ("absent"), not malformed.
- `model.rs:951` — a missing vertex remap falls back to `vertex_base + k`,
  inventing a vertex mapping.
- `material.rs:208` — four distinct failures (short payload, offset ≤ 0,
  count 0, past EOF) collapse into `Vec::new()`.
- `material.rs:337` — an unreadable texture asset becomes stored format `0`.
- `material.rs:95` — `MATERIAL_ID` read failures default to id 0, which
  matches any model slot of 0.
- `settings.rs:121` — `Err(_) => continue`, then `Ok(vec![])`: a broken settings
  asset reads as "this hero has none".
- `hero_select.rs:195` — a mode whose team pool cannot be read yields
  `Ok(vec![])`, while the module header promises the opposite.
- `entity.rs:240` — `mover_fields_of` defaults speed to `5.5` and treats any
  4-byte field as `f32`.
- `firstperson.rs:95` — a missing scalar falls back to the class default and
  then `0.0`; `read()` still returns `Some`.
- `unlocks.rs:1429` — `read_unlock_in` hardcodes instance 0, which the same
  file documents as wrong.
- `blendtree.rs:296` — an unwritable clip index becomes clip 0.
- `clip.rs:1573` — `find_vec_array`'s plausibility predicate is vacuous for
  scale (`u16/1024` is always finite and ≤ 64), so an invented scale array is
  reported as located.
- `text.rs:43` — unconditionally drops the last byte and cannot fail; `text.rs`
  has no address, no install measurement and no `docs/formats` row.
- `sound.rs:1571` — `_ => {}` silently drops every 2-byte and 8-byte field
  while the doc says "nothing on a class is lost".
- `packages.rs:160`, `206` — a corrupt record offset becomes an empty package
  and `find` swallows every load error.
- `manifest.rs:508` — an empty `if` branch that claims to handle BLTE; a
  BLTE-wrapped manifest would be parsed as clear and mis-diagnosed.
- `manifest.rs:116`, `521` — `data_patch_count`/`entry_patch_count` are parsed
  and never used; a patched manifest is read with the clean layout.
- `manifest.rs:131` — `Header::version()` is dead in production; `open` never
  checks the version nibble.
- `hero_select.rs:404` — a team that declares `All` replaces the union of the
  other teams' whitelists, and `HeroChoice` carries no team at all.

## Tests that cannot fail

- `skin.rs:598` — inverted (P0-8).
- `tests/bc6h.rs:278` — pins the shifted DDS offsets (P0-1).
- `replay.rs:1129` — `params_len()` walks the block with the same field order
  as the parser, writes that number into the block, then asserts the parser
  agrees: self-fulfilling even where the doc says the layout is wrong on real
  files.
- `entity.rs:767` — a test-local copy of the `hidden` rule; the production path
  is never exercised.
- `tests/mapchunk_kinds.rs:806` — the install sweep asserts only
  `placeables > 0`, `refs_resolved > 0` and "some non-model kind worked"; a
  body layout wrong by one field still passes.
- `tests/collision_level_real.rs:66` — a `0.01` tolerance on a value the module
  claims is bit-exact; `brush_convert`'s refusal tests assert only `is_err()`.
- 120 early `return`s across 39 `ow-tank` integration targets. On this machine
  every one of them ran; on CI (no install, per `.github/workflows/ci.yml`) all
  of them pass by doing nothing. The gate cannot distinguish "the data layer is
  correct" from "the data layer was never asked".

## Structure

- **Unwired modules.** `packages.rs` (234 lines), `trg.rs` (266) and
  `domino_world.rs` (247) have no caller anywhere in `crates/`; `blendtree.rs`
  is reached only by `examples/blendtree_probe.rs`. `trg.rs`'s header probe is
  reference-shaped, has no `docs/formats` row and no client address.
- **Dead public API**: `entity::all_loadout_guids`/`loadouts_of`/`ability_of`/
  `mover_fields_of`, `weapons::modify_health_amounts`, `graph::params_of`/
  `ability_nodes_of`, `volley::weapon_figs_of`, `weapon_model_of` — referenced
  by nothing outside their own file. Each is an unproven parse path that reads
  as shipped behaviour.
- **Duplicated constants**: `weapons::MOVER_COMPONENT`/`MOVER_SPEED_FIELD` and
  `classes::CHARACTER_MOVER_COMPONENT`/`entity::mover_fields::SPEED`; the
  reference's `inline_elements` re-implemented at `unlocks.rs:2004` with
  different edge behaviour.
- **Capture evidence still labelled "read"** in code comments:
  `skin.rs:218`/`267` (a `FUN_7ff6...` capture symbol and a `0x7ff6...` capture VA — `0x7ff6` is a
  capture frame, AGENTS.md rule 9) and `classes.rs:131` (a
  `D:/ow-dumps/dumps_range/...` path with register-level detail). The 1-to-1
  doc already classifies the skin path as capture-only; the comments do not.
- **Doc drift**: `docs/ow-tank-1to1.md` says "77 units"; there are 76 unit
  files (16 targeting `ow-tank`, which matches). Minor, but the ledger claims
  a number nobody recomputed.
- **`chunks.rs` is duplicated in spirit**: `emit`-style length checks are good
  there (`checked_add`, `get`), which makes the unchecked arithmetic in the
  sibling readers stand out as accidental rather than stylistic.

## Reported but not re-checked here

Module review also flagged: `texture.rs:225` (bpp 0 for stored formats 0x5A and
0x5D, which the client maps to DXGI 24/87), `texture.rs:1283` (two chunks may
claim one level; last wins), `texture.rs:1414` (`PayloadNotLoaded` names the
lowest missing payload, not the one holding the level), `sound.rs`'s header
fields read with `unwrap_or(0)` against a "bounds-checked throughout" claim,
`unlocks.rs:1513` (four bytes read from a `ushort` field, `STARS` never used),
`unlocks.rs:2608` (stride sampling degenerates to a prefix), `unlocks.rs:1535`
(invented display name against the module's own naming rule), `assoc.rs:316`
(double counting), `guid.rs:51` (payload index ≥ 16 wraps), `graph.rs:66` (one
embedded instance per `(owner, field)`; arrays lose all but the last),
`replay.rs:707` (first `prp` mismatch aborts the search),
`chunks.rs:146`/`151` (declared size clamped; `next <= at` ends the walk
silently), `range_world.rs:83` (bounds use the raw, unnormalised quaternion;
the test normalises), `range_world.rs:423` (the practice range is resolved
twice, and its chunks parsed three times), `texture.rs:310` (a `bias` larger
than the 4-bit field can make the size walk iterate ~4e9 times).

## How to test a fix

The gate is `cargo xtask verify`; the install-reading half is only meaningful
on this machine (it prints `note: install not readable here` and returns
silently anywhere else). For a parser fix, a failing first is worth more than
the gate: add a test that feeds the truncated/short record and asserts the
specific error, then the install test that pins the exact value
(`to_bits()` for floats, not a tolerance).
