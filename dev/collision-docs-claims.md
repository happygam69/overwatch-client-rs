# Collision docs: every claim, organised for checking against the client binary

**Purpose.** A distillation of eleven design docs into a checklist that can be
walked against `Overwatch.exe` in the Ghidra project (`owmod_clean`). For each
doc: (1) the status line quoted verbatim, (2) every Ghidra address cited with
the name the doc gives it, the one-line claim, and whether the doc marks it
read / partly read / open, (3) a numbered list of what the doc says is OPEN or
NOT READ (quoted verbatim with file:line), (4) what the doc claims as verified
and by what mechanism (differential unit, emulator probe, install measurement),
with the script or test path, (5) which install assets and which numbers.

**Method.** Read-only. Nothing was executed against the binary; no `cargo` was
run. All line numbers refer to the docs as they stand in this checkout.
Addresses are current-image (`0x140000000` + RVA), copied from the docs.

**How to use it.** Section 12 lists the cross-doc contradictions — resolve
those first, because a claim that appears in two docs is usually wrong in one.
Section 13 is the ranked gap list: where the client has code we have never read.

---

## 1. `docs/formats/static-collision.md` (3038 lines, 18 checks)

### 1.1 Status in one line (verbatim)

> The range's entity chunks name **none** of the fracturable assets (0 of 550
> placeables, 0 byte-level hits in 202,688 bytes); the range's **models do carry a
> prebuilt collision mesh** (`STU_537E56E4`: 161 of 229 models, 272 blocks); its
> **consumer is found** -- `FUN_14210bac0` walks the model's collision list, which
> this build keeps at **`+0x140`** (not the reference's `+0x118`), unpacks the
> block with `0x141b53af0`, fills a `0x4c`-byte shape record with `MeshShapeFill`
> (`0x142cae540`) and hands it to the collision query `0x142caf6c0`

(lines 14-22)

> **Check 15 closes the chunk question, and the answer is "no chunk".**
> ... The kind-`0x11` chunk still has no consumer in the read set.

(lines 78, 92-93)

> The range's static collision has no producer in the read paths

(line 34)

### 1.2 Address table

State column: **read** = the doc reads the body and draws conclusions from it;
**partly** = read to a named boundary; **open** = doc says unread.

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x14210bac0` | `FUN_14210bac0` | reads the model's collision holder list at `+0x140`, filters `holder+0x20==0` / `holder+0x08!=0`, unpacks each mesh and calls the query | read |
| `0x1421088e0` | `FUN_1421088e0` | broadphase: four bucket arrays at `param_1+0x1e8` (`0x40` stride), sphere cull vs `model+0x450`, first hit wins, caller callback | read |
| `0x14212e070` | `FUN_14212e070` | scene-side query **owner** (not the Domino world): `0x18`-byte entries at `+0x2b0`/`+0x2b8`, six bounding volumes, lazy broadphase at `+0x2d8` under lock `+0x2c0` | read |
| `0x14202c3a0` | `FUN_14202c3a0` | fills the query context from `[param_1+0x6a8]`, callback `FUN_14203d6c0`, then `0x14212d710` | read |
| `0x141fb87e0` | `FUN_141fb87e0` | the query's entry point (called from `0x141fb69f0`-fn, `0x141fb8460`-fn) | read |
| `0x141b53af0` | (the unpacker) | block → `0x30`-byte source: node/point/triangle pointers, counts, `m_height`; tag rule at `field+8==1` | read + implemented (`ow_tank::mesh_shape`) |
| `0x142cae540` | `MeshShapeFill` | writes the `0x4c`-byte (19-dword) shape record: scale quad, `1/scale` quad (a `divss`), three pointers, four words, `scale*m_5BD33E19` | read + implemented |
| `0x142caf6c0` | (the mesh query) | bounds the point array, gathers candidates via `0x142caea70` into an `0x80`-entry stack buffer, per-triangle transform `0x142cc5ba0` + test `0x142cc6740`; **writes nothing persistent** | read end to end |
| `0x142caea70` | (gather) | collects candidate triangles into the stack buffer; spills freed via `0x142cadf00` | read (and `fn/MeshGather` per systems-coverage) |
| `0x1420a02f6` | (kind-`0x11` candidate consumer) | **is a `.pdata` fragment** of `0x14209fc90`; not a function start — the "unreachable" verdict was wrong | read (correction) |
| `0x14209fc90` | (the assembler) | model-holder consumer: walks `[STUModel+0x140]`, filter `holder+0x2a==1`, fills a `0x50`-byte context with four arrays | read |
| `0x14209ea30` / `0x14209ea60` / `0x1420a07a0` | context ctor / dtor / reset | zeroes `0x50` bytes, sets `+0x3c=1`; the dtor is a tail jump to the reset | read |
| `0x14201ae60` | (the per-model body builder) | root of a six-fragment chain to `0x14201c887`; walks holders, four-arm shape switch, `OwnerCreateBody` + one `ShapeAddToBody` per holder | read + implemented (`ow_tank::model_body`) |
| `0x14201af49`, `0x14201b1cd` | (fragment starts of the above) | third and sixth fragments — earlier notes wrongly treated them as functions | read (correction) |
| `0x14201e790` | (teardown) | releases bodies `+0x88`/`+0x90`/`+0x9c`, frees baked `0x30` records, clears flag bytes `+0xb9`/`+0xba`/`+0x331`/`+0x384`/`+0x386` | read |
| `0x142022030` | (caller 1) | "make the body match the state": compare has-body vs should-have-body, tear down/rebuild | read |
| `0x1420195e0` | (caller 2) | mask-membership change: per holder index lookup in `[component+0x8b0]` via `0x140678850` | read |
| `0x142014b50` | (caller 3) | rebuild with a new mask set; clears `+0x58`/`+0x59` | read |
| `0x142015f70` | (caller 4) | destroy or build; one branch tail-jumps into the teardown | read |
| `0x142018cc0`, `0x142018f40`, `0x142019880`, `0x142024d30`, `0x142023cd0`, `0x142017c60`, `0x14201cf40` | state-sync / update / refcount hub / attach | the component's per-frame and membership machinery | read |
| `0x14200a9a0` | (the collision component's constructor) | installs three vtables, initialises every field the builder reads | read |
| `0x14200a5f0` | (the class allocation) | two branches; the allocating one asks for **`0xad0`** via `0x140553a70` | read |
| `0x14200bab0` | (the registration) | `RegisterTypeWithSize(container, name, 0x14, 0xad0, 0, buildId)`; name decodes to `Unknown`; **no static reference** | read |
| `0x14200f840` | (pair-with-a-node entry) | ctor + encoded owner at `+0x08` + byte `4` at `+0x10` + primary slot 4; **no static reference** | read |
| `0x14200ee80` | attach hook (interface slot 1) | takes the node from `[entity+0x250]`, copies the entity transform quad and `[node+0x6c8]`; **tests nothing asset-derived** | read |
| `0x14201cf40` | primary slot 4 (attach) | supplies `FUN_142017010`'s eight arguments from its entity argument; type-id lookups `4`, `0x11`, `0x31` | read |
| `0x142017010` | (the node-binding entry) | refuses when a pending node is set, else `0x141b34010` + `0x141b3c6d0` + callback pair at `+0x568`/`+0x570`; twelve `E8` callers | read |
| `0x142f7d608` / `0x142f7d648` / `0x142f7d458` / `0x1433633c8` | primary vtable / base interface / third vtable / type object | the component's four tables, decoded slot by slot | read |
| `0x142f7d320` | (a **neighbouring** folded vtable) | not this component's; 0 RTTI locator, 0 eight-byte pointers, 162 no-`.pdata` stubs copy it into globals | read (correction) |
| `0x142cc3d90` | `OwnerCreateBody` | rejects `id>=0x3fff` and the sentinel `0xc439701cd97ec6aa`, decodes the world through `0x1442a0bc0`, requires `world+0x15c==0`, returns `slot<<0x30|gen<<0x20|index` | read |
| `0x142cc3f50` | (shape-entry twin of `OwnerCreateBody`) | same bounds/sentinel/decode; allocates from `world+0x2a0` (stride `0x50`) via `0x142cbe600` + `0x142cd4290` | read |
| `0x142cc3c80` | (the world allocator) | walks `0x1442a0bc0` for the sentinel, allocates `0x4d0`, `DmWorldConstruct`, stores the encoded pointer, writes `world+0x194` | read |
| `0x142cbeb70` | `DmWorldConstruct` | lays out the eight pool records | read |
| `0x142cde350` | `ConstructDominoWorld` | world constructor; copies the parent's slot instead of allocating | read |
| `0x14202f800` | (owner-id accessor, no `.pdata`) | `u16` at `[node+0x620]+0x31f8`, else the global `0x1433674e0` = `0xffff` | read |
| `0x14202c660` | `WorldBase` **slot 8** (`0x143367670`) | copies the creation record's `+0x10` to `[node+0x454]`, `+0x18` to `[node+0x630]`, two `0x142cc3c80` slots, `0x3210`-byte geometry at `+0x620` when bit `0x200` | read |
| `0x1420271d0` / `0x142027c90` / `0x1420282e0` / `0x1420266d0` | `WorldBaseCtor` / dtor / deleting dtor / type alloc | `0x910`-byte class, vtable `0x143367630` | read |
| `0x141c5ff40` | `RegisterNodeType` | push-front list insert, **144** registration sites, head `0x144188170` | read |
| `0x1404f6e10` / `0x142029f00` / `0x141c57970` | `RegisterNodeType2` / `NodeType2Factory` / lookup | type `2` → class registered as kind `0x14`, size `0x910` | read |
| `0x141c4b050` | `RegisterTypeWithSize` | 283 call sites; `0x14` on 160 of them; `0xad0` exactly once (`0x14200bbb1`) | read |
| `0x1407803f0` | type object's slot 2 | **a release**, not an instantiate (corrected, check 17) | read (correction) |
| `0x142cbcd40` | (the character's ingest entry) | allocates a `0x10` holder, copies the descriptor to a `0xa0` record with the slot at `+0x86`, calls `0x142cf5440` | read |
| `0x142cf5440`, `0x142cf35a0` | (holder / query-record builder) | `Alloc(0x590)` + fills tag, world slot, handles; tag `0` → `0x142cc3f50`, tag `1` → `OwnerCreateBody` | read |
| `0x142cbcee0` | `RunCharacterControllerSweep` | nine `E8` sites, all mover/AI; one meaningful call `0x142cf5660(*param_1, buffer, &query)` | read |
| `0x142cf5660` / `0x142cf3f80` | 10-byte thunk / sweep body | variant tag at `*world`, handle at `+0x10` (rigid) or `+0x08` (character); **zero indirect calls** | read |
| `0x142cbcec0` | `VariantPosition` | shape pool `+0x2a0` (stride `0x50`) / body pool `+0x288` (stride `0x1c0`) | read |
| `0x142cd4d90`, `0x142cd5270`, `0x142cd4ff0`, `0x142cd50f0`, `0x142cd5070`, `0x142cd5170` | Domino accessors | pair/shape pool `+0x2b8` (`0xc0`), link pool `+0x348` (`0x58`) | read |
| `0x1442a0bc0` | world handle table | slot → encoded world pointer; single writer `0x142cc3bb0` | read |
| `0x140ca5db0` | (mover per-sweep setup) | builds the stack descriptor; slot from `0x14202f800` at `0x140ca609b`, stored at descriptor `+0x46` | read |
| `0x140ca37d0` | `ConstructMoverBase` | `0x1b70`-byte base, vtable `0x1431101e8`; zeroes `+0x1ad8` at `0x140ca3b08` | read |
| `0x142cdf8e0`, `0x142cdf930` | (world-creation wrappers) | `Alloc(0x170)` + `0x142cde350`; exactly one `E8` caller each, both in `0x142090e00` | read |
| `0x142090e00` | (the streaming owner) | 13 contiguous `.pdata` entries, `0x142090e00..0x142094d34`; builds a world description on the stack; **third holder consumer, mode `+0x2a == 4`** | read |
| `0x1420c63b0`, `0x1420ca550` | the two "siblings" | attachment-driven shape builders over a `0x38`-byte record table, **not** models' holder lists | read (correction) |
| `0x14213c440` | `GeometryBuild` | node-type-2 build; reads `C+0xf0`, `C+0x98`, `GeometryCopyParams`; **never** reads `C+0x08` | read |
| `0x1420d0ef0` | geometry factory | picks `Alloc(0x240)` + `0x142135c10` or `Alloc(0xf0)` + `0x142132f20` on `[node+0x29]` | read |
| `0x142132f20`, `0x142133010`, `0x1421334b0`, `0x142133790`, `0x1421347b0`, `0x142134790`, `0x142133350`, `0x142133b30`, `0x142133cf0`, `0x142135b70`, `0x142134f70`, `0x142135030`, `0x1421332a0`, `0x142133280`, `0x142133110`, `0x142133da0`, `0x1421337e0` | the `0xf0` class's slots (`0x143394fb0`) | displacement census per slot; **no slot reads `+0x140`/`+0x118`**; `+0x50` (`0x142134f70`) calls `OwnerCreateBody` at `0x142135330` on `C` | read |
| `0x143394ed0`, `0x143394fb0`, `0x1433950b8` | geometry vtables | base / `0xf0` class / `0x240` class | read |
| `0x142caed30` | the mesh walk (`MeshSweepEntry` body) | node stride `0x20`, triangle stride `0x20`, vertex stride `0x10`, box/mask/axis/leaf fields; supports the hull branch | read + implemented (`ow_tank::mesh_world`) |
| `0x142caed10` | `MeshSweepEntry` | 26-byte forwarder to `0x142caed30` | read |
| `0x142cc45a0` | descriptor dispatcher | copies the descriptor into a `0x73`-byte stack block | read |
| `0x142cbd510` | (the shape query driver) | runs `sweep_bounds` then the narrowphase; four-pass BVH walk | read |
| `0x142cd5540` | `sweep_bounds` | swept bounds — verified as `fn/CollisionSweepBounds` | read + verified |
| `0x142cc1610` | `ShapeNarrowphase` | transform + filter + type dispatch — verified as `fn/ShapeNarrowphase` | read + verified |
| `0x142cce430` | `MeshTriangleTest` | analytic path only when the support count is `0` and all expansion lanes `<= 0.005`; else `0x142cc9460` | read |
| `0x142cc9460` | (the closest-point solver) | GJK-style closest point with a cached simplex; verified as `fn/ClosestPointGjk` | read + verified |
| `0x142cc8d80` / `0x142cc7740` | 3-simplex / 4-simplex | region-by-region sub-simplex reductions, implemented as `simplex3`/`simplex4` | read + implemented |
| `0x141ff90e0`, `0x141ff90a0`, `0x141ff90d0`, `0x141ff9851`, `0x142003720`, `0x142000620` | the `0x300`-byte scene component | ctor (kind `0x14`, size `0x300`, name `Unknown`, vtables `0x1433632e0`/`0x143363320`); update writes `+0x140`..`+0x16c`; `0x142000620` writes local `+0xd0`/`+0xe0`/`+0xf0` | read, **with the placed-entity `+0xd0`/`+0xf0` writer open** |
| `0x141b3d1c0` | (`[node+0xe8] → +0x28`, out of line) | the model accessor the siblings use | read |
| `0x141ffe7b0` | (guard leaf) | exactly `(byte >> 7 ^ 1) & 1` over `[second+0x242]` | read |
| `0x141510382` | (holder field registration) | declares `0x89AABA94` at `+0x2a` — a **declared field**; no code store writes it | read |
| `0x142010330`, `0x142010a20`, `0x141bb0120`, `0x140678850` | mask/id helpers | id count, per-id mask table, holder index lookup | read |

### 1.3 OPEN / NOT READ, quoted

1. static-collision.md:52 — "What is still unread is which *static* map nodes are given the component (check 9's open list)."
2. static-collision.md:358-360 — "### What is still open here, exactly / The element layouts of the node array (`32` B) and the triangle array (`16` B)." *(superseded by check 16 — see §12.1)*
3. static-collision.md:901 — the meaning of the two vectors the builder forms ("centre of mass and inertia is the likely reading") "was **not** read."
4. static-collision.md:923 — "that block's own field mapping was not followed"
5. static-collision.md:958 — "The record's **producer** is still unread"
6. static-collision.md:1129-1136 — "(1) **The component's owner, and therefore which map nodes get one.** ... the site that gives a *static map prop's* node a collision component was not read."
7. static-collision.md:1136-1141 — "(2) **The second argument's type.** ... so the class it points at is not named here."
8. static-collision.md:1142-1150 — "(3) **The descriptor's numbers above `+0x20`.** ... the per-field source of every one of those writes was not followed to the field level"
9. static-collision.md:1151-1157 — "(4) **The inertia/centre-of-mass block.** ... the per-shape area/inertia terms (`0x142cc41a0`-family) were not read."
10. static-collision.md:1334 — "**What is not read, and is the exact missing read for "which nodes receive the component":** the site that *allocates* this component for a node."
11. static-collision.md:1476 — "**Not read here:** `0x142cbd190(holder, [mover+0x650])`, called at `0x140ca6139` right after the store, and the meaning of the mover's `+0x650`."
12. static-collision.md:1873 — "**The exact unread store.** One call form is left: the transfer through `[WorldBase vtable + 0x40]` that passes the creation record in the second argument, plus the code that fills that record's dword at `+0x10` and qword at `+0x18`."
13. static-collision.md:2005-2010 — "**The exact unread store.** The store that puts a value with bit `0x200` into the `+0x10` dword of the record that reaches slot 8, together with whatever writes `+0x18`. It is not in any of the 632 enumerated dispatch sites"
14. static-collision.md:2228-2231 — "(1) The **consumer** of the context's arrays: ... nothing in the read set consumes the four arrays."
15. static-collision.md:2232-2235 — "(2) The **wrapper class** at `node+0xe8`/`node+0x110` is used, not named"
16. static-collision.md:2236-2238 — "(3) The four arms' **record writers** (`0x141b53af0`, `0x141b53b70`, `0x142cd1760`, `0x142cd35a0`) are not read"
17. static-collision.md:2239 — "(4) The **id remap**'s intent (which wrapper should win) is unread."
18. static-collision.md:2616-2640 — check 16's four open items: "**The capsule (support-point) contact.** ... what remains open is the type-0/type-1 leaves' own handoff to the solver"; "**The scene's placement transform.** ... was not read operation for operation"; "**The query's layer mask and the query's start.** ... runtime values"; "**The client's broadphase order.** ... (`FUN_1421088e0`), which is not read here"
19. static-collision.md:2757 — "The node record's producer is still unread."
20. static-collision.md:2900 — "the writer of `+0xd0`/`+0xf0` for a placed map entity is still unread, so no range sweep names a map location yet"
21. static-collision.md:3009 — "| the range's placed props against the gate | **not tested** (unchanged from check 12's row): the gate bits are runtime state and the record's writer is still unread |"
22. static-collision.md:2002 — "| is the range's gate decidable from the install? | **no, still open**: the flags word's producer is unread, so no census can stand in for it |"
23. static-collision.md:2757-2770 — "the next read is the **poller of `service+0x78`**: the all-preloads-done callback is `0x141fe0f80` ... and it only sets the loaded byte at `+0x78` and clears `+0x70`"
24. static-collision.md:2955-2960 — "the record's **producer** is still the anchor -- now a writer that must supply a settings description, not a placement"

### 1.4 Verified, and how

| claim | mechanism / path |
|---|---|
| the range names none of the 16 fracturable GUIDs | install test `crates/ow-tank/tests/static_collision_real.rs::check_1_the_range_names_no_fracturable_descriptor` (550 placeables, 202,688 bytes, 0 hits) |
| the models carry the mesh block, 161 of 229; every block embedded by `0x2DD96D6D` via `0xB7C8314A` | `static_collision_real::check_2_the_models_carry_the_prebuilt_mesh_and_its_embedder` |
| the block's stored mesh and its shape record, bit-exact | `crates/ow-tank/tests/model_mesh_real.rs::the_range_models_prebuilt_mesh_and_its_shape_record` |
| collision is per model asset (one `STUModel`, 163 with a list, 279 holders, 0 model→model refs) | `crates/ow-tank/tests/model_schema_real.rs::the_range_models_collision_fields_and_where_they_live` |
| the range's holders against the builder's filter (272 mesh, mask all ones, mode default `0`) | `model_body_real::the_range_models_holder_plan` |
| the filter / four-arm switch / type bytes / mesh scale | `ow_tank::model_body::tests::{the_world_mask_is_selected_by_bit_25_of_the_flags_dword, the_holder_filter_is_the_mode_byte_the_mesh_and_the_mask, the_shape_mode_is_the_four_arm_switch, the_descriptor_type_byte_is_the_shape, the_holder_array_is_the_tag_swap, the_mesh_scale_is_uniform_within_the_epsilon, the_mesh_descriptor_scale_is_maxss_or_one}` |
| the unpack + `0x4c` record, dword for dword | `mesh_shape::tests::{the_shape_record_is_mesh_shape_fill_dword_for_dword, the_array_header_offset_is_the_tag_test, a_zero_scale_is_an_infinity_lane, the_field_offsets_are_the_unpackers}` |
| the world-slot codec and the body-handle packing | `domino_world::tests::{the_two_addends_are_inverses_modulo_2_pow_64, the_world_pointer_round_trips_bit_exactly, the_empty_slot_is_the_encoding_of_a_null_world, the_body_handle_is_slot_then_generation_then_index, the_invalid_body_handle_is_ffff_0000_ffff_ffff, the_slot_bound_is_strictly_below_0x3fff, the_body_pool_is_0x1c0_per_index}` |
| the range's mesh shapes assemble into one addressable world | `crates/ow-tank/tests/mesh_world_real.rs::the_ranges_mesh_shapes_assemble_into_one_world` (4,797,872-byte buffer) |
| one pinned sweep is an exact hit | `mesh_world_real::the_pinned_model_mesh_sweeps_to_an_exact_hit` (fraction `0x3e8051b8`, triangle `7`) |
| the assembler takes nothing on this map | `crates/ow-tank/tests/model_shape_source_real.rs::the_range_takes_no_holder_into_the_shape_source` |
| the closest-point solver equals the client's bytes | differential unit `fn/ClosestPointGjk` via the `dev/verify_harness` probe entry `gjk_closest`: **264 cases**, whole `0x40` output block + `AL`, 74.5% of 47 branch sites, 12/12 mutants, 70% coverage gate |
| the 3-simplex / 4-simplex bodies | `dev/gjk_simplex_diff.py` (60-case run: block, count and barycentrics exact; four-vertex direction within one ULP on the low lanes); `dev/gjk_closest_diff.py` (17 captured + 120 seeded descriptors) |
| the support-hull reach | `dev/mesh_support_diff.py`: two three-rung ladders one ULP apart, client and `ow-sim` step across at the same rung |
| 18 support-point queries agree exactly | `dev/mesh_support_diff.py` (18 agreements, 0 failures) |
| the dispatch census and the reachability negatives | `dev/callers.py`, `dev/fn_extent.py`, `dev/static_rva_tables.py`, `dev/xref_rva.py`, `dev/rip_refs.py`, `dev/disp_scan.py`, `dev/static_field_scan.py`, `dev/static_slots.py`, `dev/static_model_schema.py`, `dev/pool_scan.py`, `dev/slot_call_backscan.py`, `dev/slot_reg_scan.py`, `dev/call_slot_scan.py`, `dev/rd.py` |

### 1.5 Install assets and numbers

- Scene: the practice range resolves **34 chunks**; the entity chunk is `0x0DD0_000B_0F69_002E` with **550 placeables** in **202,688 bytes**; **229** model assets. The kind-`0x11` chunk is `0x0DD0_0011_0000_0688`, **21.8 MB**, **36** sectors of `0x58` each threading four runs (12-byte vertices, 1-byte attributes, 16-byte records, 4-byte indices).
- Models: 229 of 229 have an `MSTU`; **161** carry the mesh block; **272** mesh-block instances; **279** holders of class `0x2DD96D6D`; **7** holders with no mesh; mesh histogram `{0:68, 1:60, 2:96, 3:2, 4:2, 6:1}`; holder-list histogram `{0:66, 1:61, 2:96, 3:2, 4:2, 6:2}`; **163** models with a holder list; **0** model→model references.
- Pinned block, model `0x0d0000000000078c`: node array **288 B** (9×32), **8** points, triangle array **384 B**, `nodeCount` 9, `+0x3c` 12, `height` 5, `+0x44` `96.0f`; the first four points are the corners of a 4-unit box; the record at unit scale is `3f800000`×8 then the three offsets, `9, 8, 12, 5, 42c00000`.
- Correction that matters: the triangle stride is **`0x20`**, so 384 B is **12** triangles, not `24 × 16`.
- Strides re-derived from the client: node `0x20`, triangle `0x20`, vertex `0x10`; `0x38B71436 × 0x20` equals the triangle array's byte length on **272 of 272** blocks.
- Distinct node/triangle layer masks: the same **20** values, `0x3a`..`0x1fff`.
- Assembled world: **272** shapes from **161** models in **4,797,872 bytes**; a single line over the union box `(-28.35,-14.51,-14.01)..(32.21,100.01,52.51)` hits on **3 of 272** shapes; nearest is shape **49**, model `0x0d000000000010f5`, point `(-13.212505, 100.0, 2.619999, 0.0)`, fraction `0x3c8aedfa`, triangle `14`.
- Pinned single sweep: start `(0.9950001, 6.01, 0.9950001)`, end `(0.9950001, -2.01, 0.9950001)`, expansion `0`, count `0`, mask `0xffff`, 4 internal nodes / 1 leaf, hit `(0.9950001, 4.0, 0.9950001, 0.0)`, normal `(0,1,0,0)`, fraction `0x3e8051b8`, triangle `7`, ids `[0x8000, 0]`.
- Store censorship: 37 stores write displacement `0x454`; exactly one is a dword store inside `0x14202c660` (`0x14202c6e4`).
- Registration census: 283 `RegisterTypeWithSize` sites, `0x14` on 160, `0xad0` once; 144 node types; 632 enumerated slot-8 dispatch sites (492 memory-indirect + 31 tail-jump + 109 register-indirect); 119,802 function groups.

---

## 2. `docs/formats/shape-ingest.md` (370 lines)

### 2.1 Status in one line (verbatim)

> **The pools are written by exactly one allocator pair, reached only from
> `ShapeAddToBody` and `ShapeAddToOwner`; and every producer that fills the
> *map's* shapes does so from a `BrushTreeToMesh` result.** There is no chunk
> input on the shape path ... The collision chunk (`kind 0x11`) therefore does
> **not** feed the Domino shape pool by any route read here.

(lines 13-23)

> **The source data is the brush, not the collision chunk.**

(line 281)

### 2.2 Address table

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x142cbeb70` | `DmWorldConstruct` | lays out eight pool records at `record+0x288 + 0x18*i` from eight config counts; a zero count writes `{0,0,0,0,-1}`; a non-zero count writes the free chain | read |
| `0x142cbe130` | `DmWorldClear` | writes the same eight records to `{0,0,0,0,-1}` without touching allocations | read |
| `0x142cc04f0` | (the destructor) | frees each pointer and writes `{0,0,0,-1}`; **`0x142cc054f` is inside it** | read (corrects `domino.md`) |
| `0x142cb00c0` | `ShapePoolAlloc` | pop `pool[+0x14]`, take the next index from the element's link dword, bump used, raise high-water; grow `cap/2+cap` (min 2) | read |
| `0x142cb0a20` | (the shape-entry creator) | allocate an index, fill the `0xc0` record from the descriptor; exactly one caller | read |
| `0x142cd4350` | (the owner-side twin) | same for owners; exactly one caller | read |
| `0x142cb7f10` | `ShapeEntryFromDescriptor` | descriptor → `0xc0` entry + per-type pool record; switches on `source+0x67` | read |
| `0x142cb3940` | `ShapeAddToBody` | decode handle, check generation, allocate entry, call `0x142cb7f10`, append, refresh bounds | read |
| `0x142cd4ef0` | `ShapeAddToOwner` | the owner-side twin | read |
| `0x142cb5e80`, `0x142cb6100`, `0x142cb6290`, `0x142cb5fc0` | `ShapePoolAllocType1/2/3/4` | the per-type pool allocators; one caller each (`ShapeEntryFromDescriptor`) | read |
| `0x142cae540` | `MeshShapeFill` | five callers: `0x142cb7f10`, `0x140c264b0`, `0x142077580`, `0x14209fc90`, `0x14210bac0` | read |
| `0x142cd31f0` | `ShapeFillType2` | two callers: `0x142cb7f10`, `0x142cd35a0` | read |
| `0x14213d0b0` | `GeometryFinalize` | the only `ShapeAddToBody` caller whose input is a list of *collision entries*; two sites, `0x14213d530` / `0x14213d9bb` | read |
| `0x14213ce50` | (finalise driver) | calls `GeometryFinalize`; has no `E8` caller | read |
| `0x142136010` | (the `0x60` appender) | appends one `0x60` element to `geometry+0x108`; **one** caller, `GeometryBuild` `0x14213c440` at `0x14213c924` | read |
| `0x1421360a0` | (the `0x100` appender) | appends one `0x100` element to `geometry+0x118`; **two** callers, `0x14213a510` (`0x14213aadc`) and `0x14213acf0` (`0x14213b00c`) | read (corrects `maps.md`) |
| `0x14213a510` | `AppendCollisionEntryFromBrushSource` | brush source snapshot → `BrushTreeToMesh` → one `0x100` entry | read |
| `0x14213acf0` | (second array-B appender) | `0x28`-stride brush node list → mesh → one `0x100` entry per node | outline only |
| `0x14213c240` | (shared mesher step) | the common step of the two array-B appenders | read |
| `0x14213bbc0` | (brush-asset driver) | calls the two appenders; carries the GUID-type-`0x0A` gate | read |
| `0x142cf23c0` | `BrushTreeToMesh` | **four** callers in the whole image, all brush appenders | read |
| `0x14213c440` | `GeometryBuild` | the node-type-2 build: model → brush ring → mesh → one array-A entry | read |
| `0x142ccb1b0` | `BuildBvh` | the collider **face**-BVH builder (not a sector reader); already a verified unit `fn-BuildBvh.json` (`ow_sim::collider::build`) | read + verified |
| `0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c` | `BuildBvh`'s three callers | two-way split wrappers over the same `0x60`-stride face array; none reachable by `E8`/`E9`/pointer | read |
| `0x14209fc90` | (re-classified here) | **not** an owner-container assembler: it reads the **model** at `[[node+0x110]+0x28]` else `0x141b3d1c0(node)` and walks `STUModel+0x140` with `holder+0x2a == 1`; scratch context `0x50` bytes | read |
| `0x14209fc90` sites `0x1420251d1` / `0x142025229` | context alloc/free around the call | the `0x50`-byte scratch context | read |
| `0x142cd35a0` | (type-2 filler for the mover) | second `ShapeFillType2` route, from `0x14209fc90` | read |
| `0x14201b1cd` | (the per-model builder, as a fragment) | the `crates/ow-tank` `ShapeAddToBody` census counted it as its own function; it belongs to the group starting `0x14201ae60` | read (correction) |

### 2.3 OPEN / NOT READ, quoted

1. shape-ingest.md:325-332 — "1. **An inline entry appender.** §5 is a census of the two helpers. A function that grew and wrote one of the arrays itself, without calling either helper, would not appear — a displacement scan cannot separate `[reg+0x114]` on a geometry object from the same displacement on any other class"
2. shape-ingest.md:333-338 — "2. **The node-type table is not enumerated.** 144 node types are registrable ... Only type 2 (`GeometryBuild`) and the GUID-type-`0x0A` brush driver were followed."
3. shape-ingest.md:339-342 — "3. **`0x14213acf0` is read only in outline.** ... the entry's fields past `+0xb0` (the handles at `+0x40`.. and the scale quad) are not decoded."
4. shape-ingest.md:343-345 — "4. **GeometryBuild's `0x60` array-A entry** is read only as far as the finaliser consumes it ... Its own field meanings are not named."
5. shape-ingest.md:346-348 — "5. **The collision chunk's own consumer** stays open exactly as `formats/chunk-kinds.md` §6 records — now with the additional knowledge that it is not the Domino shape pool."
6. shape-ingest.md:349-352 — "**Not tested.** This file's conclusion is a census of call sites, verified by byte scan and instruction decode, not a differential unit."

### 2.4 Verified, and how

Nothing in this file is verified by a differential unit: "there is nothing to implement: no conversion is read". Its evidence is a **store census with an exclusive sentinel** plus exclusive-single-caller chains, reproducible by `dev/pool_scan.py`:

- `python dev/pool_scan.py sentinel` — the eight `-1` free-head writers over the whole image, which yields exactly `DmWorldConstruct`, `DmWorldClear` and the destructor; the only other single-hit functions are seven unrelated objects (`0x141b774b0`, `0x141dbc7b0`, `0x141dbce40`, `0x141dc5050`, `0x141dcaae0`, `0x141f1bb70`, `0x14203c700`).
- `python dev/pool_scan.py calls 0xTARGET` — the verified `E8` census of each pool path node, grouped into non-chained `.pdata` groups (119,802 groups, the same count `docs/ghidra-project.md` reports).
- `dev/pool_lea_args.py 0x108 0x118` — every `lea reg,[base+off]` for the two offsets, with the call it feeds (how the append helpers' arguments were pinned).
- `dev/data/read/refs.py` — all six reference spellings, used for the three `BuildBvh` callers' reachability.

### 2.5 Install assets and numbers

None measured. This file is a census of code, not of data. Pool table (from `DmWorldConstruct`): bodies `+0x288` `0x1c0` ← `config+0x20`; owners `+0x2a0` `0x50` ← `+0x24`; **shape entries `+0x2b8` `0xc0` ← `+0x28`**; type 0 `+0x2d0` `0x20` ← `+0x2c`; type 1 `+0x2e8` `0x30` ← `+0x30`; type 2 `+0x300` `0x50` ← `+0x34`; type 3 `+0x318` `0x50` ← `+0x38`; type 4+ `+0x330` `0x50` ← `+0x3c`. Free-chain link offsets: bodies `+0x00`, owners `+0x20`, shape entries `+0xa0`, type 0 `+0x14`, type 1 `+0x24`, types 2/3/4+ `+0x4c`. Counts: 5,703 stores over 40 pool displacements; 1,129 functions touching ≥3 pool bases (all noise); `ShapeAddToBody` has **27 `E8` sites in 12 function groups**.

---

## 3. `docs/formats/shape-narrowphase.md` (301 lines)

### 3.1 Status in one line (verbatim)

> `ShapeNarrowphase` `0x142cc1610` is the leaf that turns one `0xc0`-byte shape
> entry into a hit record. It is the third of the four query links ... Evidence
> class: **read**, and the leaf is **verified** against the client's bytes as
> `fn/ShapeNarrowphase`.

(lines 3-8)

### 3.2 Address table

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x142cc1610` | `ShapeNarrowphase` | transform, filter, dispatch, hit-record assembly, callback and sink push | read + verified |
| `0x142cb8b80` | `ShapeWorldTransform` | position at `+0x00`, quaternion at `+0x10` into a `0x20`-byte output | read + verified (`fn/ShapeWorldTransform` per systems-coverage) |
| `0x142cbb350` | `ShapeQueryFilter` | `(context+0x08, entry+0x98)`; the filter words are passed **inline** | read |
| `0x142cb8d60` | (the dispatch label) | inside `ShapeWorldTransform`'s extent, never split as a function; reads `entry[0x20]` as a **signed byte** and tail-jumps to the per-type primitive | read |
| `0x142ccdf40` | sphere | pool `record+0x2d0`, stride `0x20` — `fn/CollisionSphere` | read + verified |
| `0x142ccd7b0` | capsule | pool `record+0x2e8`, stride `0x30` — `fn/CollisionCapsule` | read + verified |
| `0x142cd3c80` | type 2 (convex hull as face planes) | pool `record+0x300`, stride `0x50`; analytic plane-entry walk with a GJK fallback; chained cluster to `0x142cd4151` | read, **not implemented** |
| `0x142caed10` → `0x142caed30` | `MeshSweepEntry` → the mesh walk | pool `record+0x318`, stride `0x50` — `fn/MeshSweep` | read + verified (small-expansion analytic path) |
| `0x142cd0ae0` | type 4+ owner (height field) | pool `record+0x330`, stride `0x50`; a single kind, walked by a rectangle scan + 2D DDA | read, **not implemented** |
| `0x142cd0540` | type 4 leaf | per-cell test, extent `0x142cd0540..0x142cd0818` (root `0xc4`) | read |
| `0x142cd31f0` | `ShapeFillType2` | fills the `+0x300` pool from `[descriptor+0x68]` and `word[descriptor+0x60]` | read |
| `0x142cd35d0` | (type-2 destructor) | confirms the four allocation sizes `count0*16`, `count1*16`, `count1`, `count2*4` | read |
| `0x142ccf330` | `ShapeFillType4` | fills the `+0x330` pool at `0x142cb809b` | read |
| `0x142cc1450` | (sink push) | pushes the sink record at `sink+0x80` | **not verified** |
| `0x142f84a00` | (three zero quads) | the hit record's initial `+0x00`/`+0x10`/`+0x20` | read |
| `0x143110818` | (type-2 plane guard) | `1.1754944e-35`, an effective `|B|` threshold of `sqrt(q/3)` | read |
| `0x142cc1700` | (the declared dead edge) | the type-4 path's unreached edge in the wrapper | declared |
| `0x143ac5f40` | (frame cookie) | a leaf with a frame cookie reads it; the fixture must declare it as an allowed image read | read (harness note) |

### 3.3 OPEN / NOT READ, quoted

1. shape-narrowphase.md:256-262 — "**What stays open on both.** The descriptor's source fields (`src+0x10`, `src+0x18`, the counts) are not tied to a named class; the ingest uses the descriptor's transform while the primitive uses the entry's, and the two agree only while the entry pose is identity. Type 4's DDA parameter and increment formulas need a lane-by-lane re-derivation before implementation even though the index arithmetic is settled, and no producer of a `+0x330` descriptor was traced."
2. shape-narrowphase.md:285-286 — "**Not verified: the sink push** `0x142cc1450` and the sink record's layout. It is the reason all four declared dead edges are dead."
3. shape-narrowphase.md:287-289 — "**Not verified: the hit callback's return path**, beyond the fact that it is called with those three arguments in that order."
4. shape-narrowphase.md:290-292 — "**Not verified: types 2 and 4+.** Both primitives are read (above) but not implemented, so the Rust reports `Narrowphase::UnreadType` for them rather than guessing."
5. shape-narrowphase.md:141-147 (extent caveat) — "`dev/fn_extent.py` reports only the `.pdata` entry that covers the entry address, and for these three functions that entry is much shorter than the body ... that wider bound is not pinned by a chain walk and should be treated as a reading aid."

### 3.4 Verified, and how

- `ow_sim::collide::shape_narrowphase` is verified as `fn/ShapeNarrowphase`: **264 cases**, every channel, branch coverage **100%** over the wrapper's own `0x204` bytes (3/3 reachable of 7 sites, 4 single-edge sites declared), **11 of 12 mutants** killed. The corpus covers sphere, capsule and the type-3 mesh walk.
- `0x142cc9460` (the GJK the triangle test hands support-point / large-expansion queries to) is `fn/ClosestPointGjk`: 264 cases, whole `0x40`-byte output + `AL`, 74.5% branch coverage, 12/12 mutants.
- The unit drives the wrapper directly with a world assembled from one arena blob and the sink left null.

### 3.5 Install assets and numbers

None. The file notes the practice range's own type (3, mesh) is exercised through the wrapper.

---

## 4. `docs/formats/domino.md` (602 lines)

### 4.1 Status in one line (verbatim)

> **Read in the current image (2026-10-31):** the world constructor
> (`DmWorldConstruct`), the inv-dt writer (`DmWorldInvDt0Writer`), the substep
> driver (`DmWorldSubstepDriver`), the catch-up loop (`FUN_142cdfc50`), the
> island assembler (`FUN_142cc2510`), the island solve driver
> (`FUN_142cf81d0`), the per-constraint dispatcher (`ConstraintDispatch`), the
> broadphase refit walker (`FUN_142cb3330`) and the residual query
> (`DmResidualQuery`).

(lines 10-16)

> So the remaining unknown is **the producer of the `source` descriptors**: who
> turns the map's collision chunk into the descriptors this builder consumes.

(line 292)
### 4.2 Address table

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x142cbeb70` | `DmWorldConstruct` | the world constructor; eight pool records at `record+0x288 + 0x18*i`; presizes the auxiliary pools; writes each pool's free chain | read |
| `0x142cc3050` | `DmWorldInvDt0Writer` | writes `m_inv_dt0` (lane 0 of `1/dt`) only when all four dt lanes are `> 0`; both phases end in the broadphase step | read |
| `0x142cc3630` | `DmWorldSubstepDriver` | broadcasts dt, validates each island ref (`idx < +0x294`, `body+0 == idx`, `body+0x198 == ref>>0x20`), sets `body+0x1b3`, calls `FUN_142cc2510` `param_5` times | read |
| `0x142cdfc50` | catch-up loop | while the residual minimum `< 0.5` and fewer than `param_3` iterations, one step then a residual query | read |
| `0x142cc2510` | island assembler | bump `world+0x18c`; touch-mark / pair / link walks; solve; refit; sync; free temps | read |
| `0x142cf81d0` | island solve driver | velocity integrate, constraint dispatch, position integrate with clamps, final rounds, sleep pass, writeback | read |
| `0x142cd9c90` | `ConstraintDispatch` | fills the `0xe0` constraint record from two bodies; cases 1, 2, 4..9 (no case 3) | read |
| `0x142cb3330` | broadphase refit walker | walks `body+0x158` and calls `0x142cf32d0` on `world+0x1b0` | read |
| `0x142cdf710` | `DmResidualQuery` | per island item (stride `0x40`): fast/slow residual accumulation and minimum tracking | read |
| `0x142d07170` | `RotationInertiaSolve` | case-6 solver: body quaternions, inverse-mass pair, inertia diagonals, type-6 state | read, **closed form open** |
| `0x142cbdc60` | `DmBroadphaseReinit` | BVH builder over the pass sub-records (not an ingest); `-1` in a slot means "no tree" | read |
| `0x14208c070` | `DmTickKeeper` | decides how many steps run; recurses into children whose time is behind | read |
| `0x142cbe130`, `0x142cc04f0` | reset / destructor | reset writes `{0,0,0,0,-1}`; the destructor starts at `0x142cc04f0` (`0x142cc054f` is inside it) | read |
| `0x142cb00c0` | `ShapePoolAlloc` | the per-element pop (free head `pool[0x14]`, link in the element's own type-specific dword) | read |
| `0x142cb7f10` | `ShapeEntryFromDescriptor` | the geometry writer: bounds/filter/ids/radii/flags, then a switch on `source+0x67` | read |
| `0x142cb3940` / `0x142cd4ef0` | `ShapeAddToBody` / `ShapeAddToOwner` | the two "add a shape to a world" entries | read |
| `0x142cb5e80`, `0x142cb6100`, `0x142cb6290`, `0x142cb5fc0` | per-type pool allocators | types 1, 2, 3, 4+ | read |
| `0x142cd31f0` | `ShapeFillType2` | copies a vertex array, an index/flag source and counts `+0x2c`/`+0x30`, scaled by the descriptor's scale | read |
| `0x142cae540` | `MeshShapeFill` | type 3: `+0x68` is a float scale, `+0x70` a pointer to the `0x30`-byte mesh source | read |
| `0x142ccf330` | `ShapeFillType4` | types 4+: `param_3[0x0d]` and `*(u16*)(param_3+0x0c)` | read |
| `0x141d71200` | (a class constructor, **not** the fill) | zero-fills ~`0x45a0` bytes, vtable `0x14331c570`; every pool/handle store is a **zero**; `+0x31f8` is cleared, not published | read (correction) |
| `0x142cc3c80` | (the world create) | the only caller of `DmWorldConstruct`; `0x4d0`-byte record; the encoded pointer goes into `0x1442a0bc0`; `record[0x194] = handle` | read |
| `0x142cc3d90` | (the world decode) | rejects `handle > 0x3fff` and the sentinel, then decodes | read |
| `0x14202c660` | (the world/geometry owner's initialise) | writes the owner's fields; only when `[owner+0x454] & 0x200` does it allocate the `0x3210`-byte geometry at `owner+0x620`; creates **two** worlds from one config | read |
| `0x142cde350` | `ConstructDominoWorld` | reads the world description (rotation, layer count, floats, pointers, child list) | read |
| `0x141ff51a0` | `MapChunkCellBytesFixup` | converts 17 self-relative qwords per cell record; the encoding was settled under the oracle | read + verified |
| `0x141baff30` | (the allocator the fixup's mode-0 branches use) | entry second qword `0` aborts; the mode-0 branches allocate then read the zeroed allocation (declared dead) | read |
| `0x142cf2010` | (mesh → collider faces) | emits `2*count` vertices (`z ± thickness`) and `2*count` faces of `0x14` bytes, plus two side triangles per open edge | read (implemented in `ow_sim::collider::shell`) |
| `0x142cf23c0` | `BrushTreeToMesh` | the brush CSG walk, op codes 0..5 over `0x60`-byte hulls | read (see §6) |
| `0x142ccc120` | (the type-3 mesh-source filler) | fills the `0x30`-byte type-3 mesh source from `{vertices, faces, count}` | read |
| `0x142cf3080`, `0x142cee200`, `0x142cedfe0`, `0x142cfd110`, `0x142caf970`, `0x142cf3330`, `0x142cc1820`, `0x142cc18c0`, `0x142cc2ae0`, `0x142694c30` | world constructors / steppers | pass records, containers, auxiliary pools, broadphase steps, the registry hook | named |
| `0x1404f6e10`, `0x142029f00`, `0x141c4b050`, `0x1407803f0`, `0x1420271d0`, `0x1420266d0` | node-type-2 registration chain | with slot 2 being a **release**, not an instantiate (corrected here and in `static-collision.md` check 17) | read (corrected) |
| `0x1420c63b0`, `0x142cf35a0`, `0x14201b...` | `ShapeAddToBody`/`ShapeAddToOwner` callers | "the character/actor collision setup ... not yet a map loader" | read (superseded — see §12.3) |
| `0x142cadee0`, `0x142cadf00`, `0x142efe950`, `0x142efe2b0`, `0x141bb0120`, `0x141c4b270`, `0x141c5ef30`, `0x141c5f420` | allocators / helpers | pool allocation, zeroing, growth, the allocation strategy's slots `0x50`/`0x58` | named |

### 4.3 OPEN / NOT READ, quoted

1. domino.md:367-377 — "**Open (2026-11-01 navigation):** the handle registry's owner is an engine object installed at `0x142cc3bb0` ... The fixture/geometry fill is world-side, not handle-side, and is still unread. The dedicated `dmBody` mass/fixture writer (no single body constructor was located in this pass; bodies are zeroed pool records whose fields are filled by paths not yet attributed), the consumer of `m_inv_dt0`, the numeric tick pair (the dt scalar and substep/iteration counts flow in from above `FUN_14208c190` and are not read), and the closed form of `RotationInertiaSolve` (a conditioned-sweep solver, verified not the integrator)."
2. domino.md:580-595 — the four "What must be read next" items: "**The `dmBody` mass/fixture writer.**"; "**The tick pair.** *The dt scalar, the substep count and the iteration counts ... their numeric values are not read.*"; "**The `m_inv_dt0` consumer.** *The writer is read; no reader was found.*"; "**`RotationInertiaSolve` closed form.** *The reads/writes above are the interface; the internal sweep arithmetic is not reduced.*"
3. domino.md:292-294 — "the remaining unknown is **the producer of the `source` descriptors**" *(superseded — see §12.3)*
4. domino.md:580+ and :596-601 — "Status of this pass: schema tables above are current-image reads with the `(1 fn)` marks showing what still needs a second read; no Rust code was changed, so there is nothing to verify by test yet." → the whole `domino.md` solver is **not differential-tested by this doc**.
5. domino.md (pool layout table, ~line 415) — "the middle of the list and its order are not decoded" (the auxiliary pools between `config+0x40` and `config+0x6c`).
6. domino.md:~300 — the file-to-runtime entry conversion ("`{cell_off, 0}` to `{record pointer, 1}`") "is still an observation, not a verified unit".

### 4.4 Verified, and how

- `MapChunkCellBytesFixup` `0x141ff51a0` → `ow_tank::collision::fixup_cell_bytes`, differential unit **`fn/MapChunkCellBytesFixup`**: 264 iterations plus the edge pool, every pointee byte compared after the call, **12/12 mutants** killed. Outer modes 1 and 2/3, entry modes 1 and 2/3 covered; the two mode-0 (allocate-then-deref-null) branches are declared dead because the allocator dereferences the process heap vtable at `0x144188200` (null in the offline image).
- Findings verified "by two or more independent reads (constructor write + consumer read, or instruction-level disassembly)": the world layout, the body stride, the constraint record, the solver-pool strides.
- `BuildBvh` (`0x142ccb1b0`) was already a unit under `dev/verify_harness/units/fn-BuildBvh.json` (candidate `ow_sim::collider::build`), though it belongs to `shape-ingest.md`'s census rather than this file.
- Everything else in `domino.md` is read-only: no golden vectors, no first-divergence test — the doc says so explicitly.

### 4.5 Install assets and numbers

None measured directly. `dmWorld` field map: bodies `+0x288` `0x1c0`; `+0x2a0` `0x50`; `+0x2b8` `0xc0`; `+0x2d0` `0x20`; `+0x2e8` `0x30`; `+0x300`/`+0x318`/`+0x330` `0x50`; `+0x348` `0x58`; `+0x360` `0x18`; `+0x378` `0x40`; `+0x390` `0xe0`; `+0x3a8` `0x60`; `+0x3c0` `0x110`; `+0x3d8` 4; `+0x3f0` `0x120`; `+0x408` `0x1d0`; `+0x420` `0x2e0`; `+0x438` `0x310`; `+0x450` `0x180`; `+0x468` `0x200`; `+0x4a0` `0xaa0`. Sleep thresholds: `|W|² > 0.007615436`, `|V|² > 0.0025000002` (`0.05²`); position clamp `|v·dt|² > 16.0` scaled by `4.0/|v·dt|`; angular clamp `|w·dt|² > 0.6168503` scaled by `(π/4)/|w·dt|`.

---

## 5. `docs/formats/brush-collision.md` (257 lines)

### 5.1 Status in one line (verbatim)

> The brush's **record formats**, its **vertex decode** and its **ring builder**
> are read and implemented (`ow_tank::brush`, eleven units; every arithmetic
> result in them is pinned to the client's own bytes). The **tree → mesh** walk is read only up to its per-node op
> classification; the op case bodies and the triangle emission are **open**. No
> install asset carrying a brush was found — measured, see the last section.

(lines 18-23)

### 5.2 Address table

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x14213c440` | `GeometryBuild` | node-type-2 build; chain `geometry+0x80` → `[node+0x38]` → `[+0x40]` → `[+0x48]` = `C`; reads `C+0xf0`, `C+0x98`/`C+0xa0`, `C+0x24`/`C+0x28` via `0x141b5cfb0`; `C+0xc0`, `+0x110`..`+0x16c`, `+0x178`/`+0x186`, `+0x194`/`+0x195` | read |
| `0x14202f800` | (shape-id resolver) | `[geometry+0x88]+0x250` → the first `u16` becomes `geometry+0x9c`; `-1` rejects the build | read |
| `0x141b5cfb0` | (reference-block resolver) | resolves `C+0x24`/`C+0x28` and yields the `u16` stamped at `geometry+0xa8` (`& 0x7fff`) | read |
| `0x142132cf0` | `GeometryCopyParams` | copies `C`'s collision parameters into the config block | read |
| `0x142ced620` | (the ring builder) | instance points + two scales → a brush; `roundf((v*scale)*400)` then `CVTTSS2SI`; ring links; ordering pass | read + implemented + emulator-pinned |
| `0x142ced510` | (the vertex decoder) | `(x / divisor) * 0.0025` with `z = w = 0`; a zero divisor is the client's only failure mode | read + implemented |
| `0x142cf0360` | (the ring walk) | walks `+0x04` until the root returns, then `+0x10` to the next ring | read (see §6 for the full hull stage) |
| `0x142ced1a0` | (edge-list builder) | third independent reader of the brush record | read |
| `0x14213a510` | `AppendCollisionEntryFromBrushSource` | copies `count << 5` bytes of vertices and `count * 0x28` bytes of nodes | read |
| `0x142cf23c0` | `BrushTreeToMesh` | writes a `0x20`-byte mesh: `{vertices, u32 count, triangles, u32 count}`, 16-byte vertices, 24-byte triangles | partly read (ops open at the time of writing) |
| `0x142cf07d0`, `0x142cf0cc0`, `0x142cf01b0`, `0x142cefd70`, `0x142cef7f0`, `0x142ceeee0`, `0x142cf0ae0` | the six op bodies and the emission | named as unread here; read in `brush-mesh.md` | **open at this file** |
| `0x142cf2010` | `MeshToColliderFaces` | read end to end: `2n` thickened vertices, `f` outward + `f` inward `0x14`-byte faces, two stitch triangles per open edge when the radius `>= 0.005` | read + implemented (`ow_sim::collider::shell`) |
| `0x1421ce150`, `0x143f505c0` | class check / table | the loaded brush object's class check | read |
| `0x141fd3a90`, `0x141fd3b20` | GUID resolver / type extractor | the brush belongs to an asset of GUID type **`0x00A`**; `0x14213bc8f` compares against `0x0a` | read |
| `0x1435ed130` | (`400.0` splat) | the `1/400` unit scale, in all four lanes | read |
| `0x1431102d0` | (`{1.0, 1.0, 0.0, 0.0}`) | zeroes `z`/`w` before quantisation | read |
| `0x1442a0158` | (allocator hook word) | where the oracle probe installs its bump-allocator stub | read (harness note) |
| `0x14213bbc0` | (the brush-asset driver, group start) | the driver whose call site is `0x14213bd61`; `0x14213bbe6` is **inside** it | read (correction) |
### 5.3 OPEN / NOT READ, quoted

1. brush-collision.md:26-32 — "**The instance array's field encoding is open.** `GeometryBuild` reads two qwords at `C+0x98`/`C+0xa0` ... but which of the two layouts `C` actually holds, and what `C`'s class is, is **not read**." *(the encoding rule was later settled in `brush-mesh.md`; `C`'s class was settled in `model-collision.md`)*
2. brush-collision.md:135-165 — "**Open from here:** the six op case bodies in `BrushTreeToMesh` (which mutate the hull list through `FUN_142cf07d0`, `FUN_142cf0cc0`, `FUN_142cf01b0`, `FUN_142cefd70`) and the final emission (`FUN_142cef7f0` / `FUN_142ceeee0` then `FUN_142cf0ae0`), which is where the winding and the triangle indices are decided. Until those are read, **no brush-to-mesh conversion is implemented**" *(superseded by `brush-mesh.md`)*
3. brush-collision.md:223-244 — "**What is open, in order of what would unblock it:** 1. **`C`'s class and its `+0x98` array encoding.** ... 2. **Type-`0x00A` assets are not in the manifests.** ... 3. **The six brush ops and the emission in `BrushTreeToMesh`.** ... 4. **A differential harness unit.** Both brush functions allocate through the image's allocator hook, and the cargo tier needs an entry in `crates/ow-tank/src/bin/verify_probe.rs`, which this session did not own."
4. brush-collision.md:~186 — "Its differential unit is still open for the reason recorded in `docs/open-questions.md` item 6: it allocates its two arrays internally."

### 5.4 Verified, and how

- `dev/brush_vertex_oracle.py` runs **the client's own** `0x142ced510` and `0x142ced620` under the emulator; its output for a square, the reversed square, a triangle, a degenerate triple, a non-uniformly scaled square and two half-way rounding cases is what the unit tests pin, field for field. Because the builder allocates through the image's allocator hook, the probe installs a bump-allocator stub at hook word `0x1442a0158` and declares the region it hands out — which is why the differential runs in a script rather than a `dev/verify_harness` unit.
- Units: `brush::tests::{the_header_is_the_size_the_builder_writes, the_vertex_decoder_matches_the_client, the_quantiser_rounds_half_away_from_zero, the_square_ring_matches_the_client, the_other_windings_match_the_client, a_repeated_point_rejects_the_brush, the_two_scales_are_separate, the_records_round_trip_and_are_guarded}`.
- Install tests: `crates/ow-tank/tests/brush_real.rs::{no_asset_of_the_brush_source_type_exists_in_this_install, a_model_carries_no_brush_container_chunk}`.

### 5.5 Install assets and numbers

| measurement | result |
|---|---|
| records in the content manifests this install opens (`Win_SPWin_RDEV_EExt.cmf` plus the enUS text manifest) | 1,425,794 over 177 distinct types |
| records of GUID type `0x00A` | **0** (neighbours `0x00C` 32,280 and `0x00D` 34,027 are populated) |
| the same census with `WinPrism_` forced | 1,514,352 records, still **0** of type `0x00A` |
| chunk tags across 300 type-`0x00C` model assets | `MDLC`, `MRNM`, `MSTU`, `MESD`, `mskl`, `CLTH` — no brush container |

Record formats: brush object `0x20` bytes (`+0x00` vertices, `+0x08` nodes, `+0x10`/`+0x14` counts, `+0x18` root); vertex `0x20` bytes (`+0x00`/`+0x08` signed 64-bit `x`/`y`, `+0x10` signed 64-bit divisor, `+0x18` u32 ordinal); node `0x28` bytes (`+0x00` prev, `+0x04` next, `+0x08` vertex, `+0x0c` always `-1`, `+0x10` next_ring, `+0x14`/`+0x18` this node's quantised x/y, `+0x1c`/`+0x20` the following node's x/y, `+0x24` leaf, `+0x25` descending, `+0x26` uninitialised).

---

## 6. `docs/formats/brush-mesh.md` (962 lines; note: two sections are duplicated verbatim)

### 6.1 Status in one line (verbatim)

> The conversion is **read and implemented**. The hull stage
> (`ow_tank::brush_mesh::hull_stage`), the status tree, the six ops and the emission
> are in `ow_tank::brush_convert`; the client's own code was run under the emulator
> end to end (`dev/brush_mesh_oracle.py`) and **all four** of its meshes reproduce
> bit for bit, half-edge marks included

(lines 22-26)

> `C`'s instance encoding is read; **`C`'s class is not**. The store census was run
> with `disp8` as well as `disp32`, and it reaches a wall rather than an answer:
> no function in the image stores both `+0x186` and `+0x98`

(lines 47-50) *(class resolved later in `model-collision.md`)*

### 6.2 Address table

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x142cf23c0` | `BrushTreeToMesh` | builds one stack context, dispatches the ops in **sorted order**, emits the mesh; triangle count `nodeCount − 2 + 2·rings`, vertex count = node count | read + implemented |
| `0x142cf0360` | `FUN_142cf0360` (hull stage) | one `0x30` workspace vertex + one `0x60` hull per ring node; ring close; edge pairs; op classification; sort | read + implemented + emulated |
| `0x142ceeb40`, `0x142ceee80` | (sort + comparator) | the image's introsort-shaped routine and its comparator; `true` for the raw `1`; ties fall back to address order (total order) | read |
| `0x142ceca80` | (the sort key) | compares `x/divisor` and `y/divisor` as exact rationals in signed 128 bits (`SEXT816`), sign flipped when the divisors' sign bits differ | read |
| `0x142cf07d0` | (op 0 / the intrusive insert) | red-black insert, colour **1 = black, 2 = red**; compares hulls by geometry | read |
| `0x142cf0cc0` | (the erase) | full case split (root pre-fix, leaf, one child, two children); **clears only the four tree fields** `+0x00`/`+0x08`/`+0x10`/`+0x18` | read |
| `0x142cefe30` | (the rebalance) | the red/black sibling cases; every path refreshes the root slot by following parents | read |
| `0x142cf1060` / `0x142cf10a0` | left rotate / right rotate | the insert fixup's rotations | read |
| `0x142cefd70` | `lookup` | binary search down `+0x00`/`+0x08`/`+0x10` keyed by the workspace vertex; **always** returns the nearest node, never "not found" | read |
| `0x142cecb60` | (side-of-edge predicate) | sign of `(B−A)×(P−A)` with the divisor multiplied through, in signed 128 bits, times the divisor's sign | read |
| `0x142ceca20` | (equality test) | the two points are the same rational point | read |
| `0x142cecb10` | (no-`.pdata` leaf) | `−1` / `+1` / `0` — the negative of `cross(B−A, C−A)`'s sign | read |
| `0x142ced450` / `0x142ced380` / `0x142cf0900` | reorder thunk / the real comparator / the insert key | `0x142ced450(X,Y)` **is** `FUN_142ced380(Y,X)`; `y` descending, `x` ascending with sign-bit normalisation; the three-way insert key | read |
| `0x142cf01b0` | op 5 | the only body that allocates **four** hulls | read |
| `0x142cef7f0` | the `+0x58 == 0` emission variant | **not implemented** | partly |
| `0x142ceeee0` | the emission (`GeometryBuild` selects it) | ear-tag pass (tags 6/7/8), clipping list, re-tests, last-three fan | read + implemented |
| `0x142cf0ae0` | the face writer | `nodeCount` × `0x10` vertices; `0x18`-byte triangles, six `i32`: `index(faces[k])`, `mark(twin)`, `index(next)`, `mark`, `index(prev)`, `mark` | read + implemented |
| `0x142cf10e0` | (mesh measure) | used by the brush ingest's area test | read |
| `0x142cd56e0` | `ColliderTriangleDegenerate` | rejects the flattened triangle the emission can produce | read (already a verified unit) |
| `0x14213c440` | `GeometryBuild` | passes `args[2] = 1`, so the node-type-2 route uses the non-zero emission variant | read |
| `0x1420d0ef0` | node-type factory | picks between `Alloc(0x240)`+`0x142135c10` and `Alloc(0xf0)`+`0x142132f20` on `[node+0x29]` | read |
| `0x142135c10` | (the `0x240` geometry ctor) | installs `0x1433950b8` at `+0x00` (slot `+0x08` = `0x142138ce0`, slot `+0x18` = `GeometryBuild`) and `0x1433950b0` at `+0xa0`; only **zeroes** `+0x80`/`+0x88` | read |
| `0x142138ce0` | the node setter | stores `geometry+0x80 = node` and `geometry+0x78 = *flag`; also writes `geometry+0x88` | read, **with `geometry+0x88`'s source unread** |
| `0x14204cbce`..`0x14204cfc9` | the node-type-2 update | writes `[node+0x38]` at `0x14204cc20` as a move out of `[node+0xe8]`; reads `[+0x40]`/`[+0x48]`; creates or refreshes the geometry at `[node+0x30]` | read |
| `0x14208e74e` / `0x14208e75f` | the jump-table dispatch | `mov ecx,[base+rax*4+0x2090988]; add; jmp rcx` — the real reachability route to `0x14208edf6` | read (correction) |
| `0x14469a84`, `0x14469a6c`, `0x14469xxxx` | (`.pdata` copy, **not** a dispatch table) | 12-byte `{unwind RVA, begin RVA, end RVA}` triples | read (correction) |
| `0x1442a0158` | allocator hook word | where the emulator probe installs its stub | read (harness note) |

### 6.3 OPEN / NOT READ, quoted

1. brush-mesh.md:931-940 — "1. **`C`'s class and the value of `C+0xa0`, and why a displacement census cannot reach them.** ... So **no function in the image stores both `+0x186` and `+0x98`**" *(class resolved in `model-collision.md`; `C+0xa0`'s value still unread there)*
2. brush-mesh.md:941-949 — "2. **`C`'s producer: `[node+0xe8]` was searched for and not found.** ... The missing stores are: the writer of `[node+0xe8]`, the writer of `[object+0x40]`, the writer of `[instance+0x48]`."
3. brush-mesh.md:950-953 — "3. **Whether a two-ring brush can bridge.** The hole fixture faults in the face writer; whether a real brush's op sequence bridges instead is unknown until a real brush is decoded"
4. brush-mesh.md:954-956 — "4. **`FUN_142cef7f0`** (the `+0x58 == 0` variant) is read only structurally; the node-type-2 route does not select it."
5. brush-mesh.md:957-961 — "5. **A `dev/verify_harness` unit for either stage.** Both differentials are the emulator probe ... Making it a cargo unit needs an entry in `crates/ow-tank/src/bin/verify_probe.rs`, which this session did not own."
6. brush-mesh.md:~816 — "**Where this read stops, exactly.** The producer of `[instance+0x48]` was not read" (from the duplicated `C` section).
7. brush-mesh.md:~840 — the decrypted-pointer access: "the node's fallback transform is read through a **decrypted pointer** ... so a branch of `GeometryBuild` reads memory whose address comes from data."
8. brush-mesh.md (refusals table) — "`+0x58 == 0` (the other emission variant) | `FUN_142cef7f0` | `Err`, not implemented"
### 6.4 Verified, and how

`dev/brush_mesh_oracle.py` runs **both** stages: `mesh()` calls `0x142cf23c0` itself and returns the mesh struct plus every vertex's four `u32` bit patterns and every triangle's six `i32`. The fixture is generated by a Python copy of `0x142ced620` (`build_brush`, `roundf` as two `f32` steps, same ordering pass). Pinned results (evidence class **emulated**):

| brush | nodes | vertices | triangles | client output | port |
|---|---:|---:|---:|---|---|
| square, ccw | 4 | 4 | 2 | `(3,−1,0,−1,1,3)`, `(3,2,1,−1,2,−1)` | exact |
| triangle, ccw | 3 | 3 | 1 | `(2,−1,0,−1,1,−1)` | exact |
| five-point L | 6 | 6 | 4 | four faces, two clips, interlocking marks | exact |
| five-point arrow | 5 | 5 | 3 | `(3,6,0,−1,1,3)`, `(3,2,1,−1,2,−1)`, `(0,0,3,−1,4,−1)` | exact |

Units (25 in `ow_tank::brush_convert` / `brush_mesh::tests`): `the_square_hull_stage_matches_the_client` (ops 2,1,3,0), `two_rings_walk_and_close_independently` (0,5,4,1,3), `the_zigzag_covers_all_but_one_op` (2,1,0,5,4), `the_op_table_is_the_client_one`, `the_comparator_is_the_rational_one`, `the_square_mesh_matches_the_client`, `the_triangle_mesh_matches_the_client`, `the_ell_mesh_matches_the_client`, `the_arrow_mesh_matches_the_client`, `a_clockwise_ring_is_refused`, `a_two_node_ring_is_refused`, `the_other_variant_is_not_implemented`.

**Three client faults, measured under the emulator**, reproduced as `Err` rather than a mesh: a **clockwise** ring (reads address `0x40` at `0x142cf265f`, after the op-3 lookup returns the null tree root — so the sweep assumes counter-clockwise); a **two-node** ring (same place); a **two-ring brush** whose chains do not fill the allocation (reads address `0x20` in the face writer at `0x142cf0b6b`).

**Correction worth flagging:** this file's first pass claimed ops 2 and 5 read `+0x30`; they read `+0x28`. The arrow's mesh is the discriminator.

### 6.5 Install assets and numbers

The only install data referenced is `C`'s payload (in `model-collision.md`). Displacement census figures reported here and in `model-collision.md`: `+0x98` 5,789 stores; `+0xa0` 4,866; `+0xc0` 3,367; `+0xf0` 2,392; `+0x110` 1,993; `+0x178` 931; `+0x194` 136; `+0x186` **5**; `+0x195` **1**. Intersections: `{+0x98,+0xc0,+0xf0,+0x110}` → 180 functions; `{+0x98,+0xa0,+0x194,+0xc0,+0xf0}` → 13; `{+0x98,+0xa0,+0x194,+0xc0,+0x110,+0x186}` → **0**; `{+0x98,+0x194,+0x195}` → **0**. The single `+0x195` store is `0x142915c1f` in `0x142915b60` (a different layout `+0x168..+0x214`). GUID-resolver intersection: 742 qword `[reg+0xe8]` store sites in 612 functions against 1,004 resolver callers → **0**.

---

## 7. `docs/formats/model-collision.md` (529 lines)

### 7.1 Status in one line (verbatim)

> `C` is registered class **`0x564B1018`, size `0x198`**, vtable `0x1431dd118`;
> its instance-point array is the `Vec3A[]` field `0x88FCECD7` of the inline
> sub-object at `C+0x90`, read through a 16-byte slot at `C+0x98` plus a tag at
> `C+0xa0`.

(lines 21-24)

> **Two things stay open**: the class id of the object that *owns* the `+0x48`
> pointer to `C` (the producer of `[node+0xe8]`), and the writer of
> `[geometry+0x88]+0xe0` -- the scale.

(lines 33-36)

### 7.2 Address table

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x14213c440` | `GeometryBuild` | `geometry+0x80` → `[node+0x38]` → `[+0x40]` → `[+0x48]` = `C`; instance array read at `0x14213c54b` and `0x14213c936` | read |
| `0x142135c10` | (the `0x240` geometry ctor) | base vtable `0x143394ed0`, then overwrites `+0x00` with `0x1433950b8` | read |
| `0x142138ce0` | (the node setter, slot `+0x08`) | writes `geometry+0x78` (arg3) and `geometry+0x80` (node), and `geometry+0x88` (source unread) | **partly** — the store into `geometry+0x88` is at `0x142138e89` inside a region that is not code |
| `0x14204cb40` | (the node-type-2 update) | the whole function; `0x14204cbce` is its **second half**, not a separate entry | read (correction) |
| `0x14204824a`..`0x1420483f6` | (the node's geometry dispatch) | the only static caller (`E8` at `0x14204831e`); calls the update with `this - 0x20`; chooses type-2 vs `0x14204bbc0` on bit 0 of `this+0x50`; **itself has no static reference** | read |
| `0x141b251d0` | (generic release) | calls vtable slot `+0x18`, sets flag `0x2000` at `+0x1c`, unlinks from one of two global lists, defers destruction via `0x141b253c0`, decrements `0x14413879c` | read |
| `0x141b253c0` | (deferred-destruction queue) | ORs `0x4000`, links into a per-thread list, allocates a `0x28` entry | read |
| `0x1421cd360` | `RegisterComponentTypeDescriptor` | 7,956 call sites; fields including size `+0xa0`, align `+0xa4`, type hash `+0xb8` | read |
| `0x1413354b0` | `C`'s construct-in-place (`desc+0x60`) | writes the vtable and three inline sub-object vtables (`0x1431dd118`, `0x1431dceb8`, `0x1431dd080`, `0x1431dcf50`), and four asset-ref sentinels `-1` at `+0xb8`/`+0xc8`/`+0xd8`/`+0xe8` | read |
| `0x141335610` | `C`'s factory (`desc+0x58`) | allocate `0x198` and construct | read |
| `0x141334ea0`, `0x141335e60` | per-field hash dispatch (`desc+0x68`, `+0x70`) | 22 of the class's 36 field hashes appear as immediates in both | read |
| `0x1402db070` | `C`'s own registration function | registers hash `0x564B1018` | read |
| `0x141b53c40` | (the 14-byte flag fold) | `h = (h << 1) + byte` over 14 bytes, wrapping to 16 bits, in the byte order `6,2,13,12,8,1,3,5,4,11,10,0,9,7` | read + implemented |
| `0x1421d3710`, `0x1421d26d0` | (the generic offset pass / the STUD loader) | there is no per-class field reader: the order is the asset's | read |
| `0x141b5cfb0` | (reference-block resolver) | reads `C+0x24`/`C+0x28` | read |
| `0x142132cf0` | `GeometryCopyParams` | reads `C+0xc0`, `+0xd0`, `+0x110`, `+0x114`, `+0x118`, `+0x11c`, `+0x130`..`+0x174`, `+0x194`, `+0x195` | read |
| `0x141fd3a90`, `0x141fd3b20` | GUID resolver / type extractor | the route to `C` is an entity component, then a reference to a GUID type `0x112` asset | read |
| `0x140543440`, `0x1404c19b0` etc. | (helpers on the unread path) | `0x140543440` is not a `.pdata` function start and no `.pdata` range covers it | read (limit) |
### 7.3 OPEN / NOT READ, quoted

1. model-collision.md:499-503 — "1. **`C`'s owner class** -- the identity of the object at `[node+0x38]` and of its `[+0x40]` STU instance, i.e. the producer of `[node+0xe8]`. The update that promotes `+0xe8` is reachable (one static caller, the dispatcher at `0x14204824a`), but the dispatcher is itself held as a runtime pointer."
2. model-collision.md:504-508 — "2. **`[W+0xe0]`, the ring builder's scale** -- see above: the store into `geometry+0x88` is in a region that is not code, so the object and its field stay unnamed. The ring is pinned at the clamp floor `1.0` and re-derives in one line once this is read."
3. model-collision.md:267-269 — "**Not read:** who writes the record and the tag (the loader), and so what makes a tag `1` -- the read expression is exact, the producer is not."
4. model-collision.md:349-352 — "**What is not read**, exactly: the identity of the class at `[node+0x38]` and of its `[+0x40]` STU instance, and so whether that `+0x40` shift is the runtime header of an entity component."
5. model-collision.md:355-360 — "An inferred reading that is **not** claimed here: that `+0x40` is the entity-component runtime header and that `node+0x38` is an instance of a `STU_09CB5A77`-like component. The arithmetic fits, but nothing in the image was read to confirm it"
6. model-collision.md:36 — "both are named exactly in "What is still open"" (the two open items above)
7. model-collision.md:~63 — "**Partly read:** the same setter also writes `geometry+0x88` ... The probe's body (`0x142138e27`..`0x142138e89`) does not decode linearly, so what `geometry+0x88` points at is unread"

### 7.4 Verified, and how

| claim | mechanism / path |
|---|---|
| the array slot's two orders and the tag test | `model_collision::tests::{the_array_record_is_read_the_way_geometry_build_reads_it, the_tag_is_a_whole_qword_test}` |
| the zero-count rejection | `model_collision::tests::a_zero_count_is_refused` |
| the 14-byte fold and its byte order | `model_collision::tests::the_flag_fold_uses_the_client_byte_order` (`0x3fff` for fourteen ones; byte 6 heaviest) |
| the class identity and field offsets are consistent | `model_collision::tests::the_class_identity_is_the_registered_one` |
| the registry enumeration itself | `dev/model_registry.py --hash 0x564b1018` (size `0x198`, align 8, ctor `0x1413354b0`) |
| the class's declared layout | `dev/model_class_search.py --min-size 0x190 --offset 0x186` — **one class of 7,782** |
| the field-hash dispatch agreement | `dev/model_class_search.py`'s parse against the immediates of `0x141334ea0`/`0x141335e60` (22 of 36) |
| the seven `C` assets and their instances | `model_collision_real::the_collision_descriptor_is_reachable_through_its_asset_reference` |
| the real ring and mesh, bit for bit | `model_collision_real::the_real_brush_ring_and_mesh_are_pinned` |
| the practice range carries no `C` | `model_collision_real::the_practice_range_places_no_model_that_carries_the_descriptor` |

### 7.5 Install assets and numbers

- **32,278 model assets scanned** (of 32,280 readable), **none** mentions the class hash; the practice range's 229 models, 79 placed assets and 34 chunks carry no `C`.
- Nine entity assets carry a `STU_09CB5A77`-like component with field `0xA7ED6024` → **seven distinct `C` assets**:

| entity asset | `C` asset | `FSTU` header | `C` payload | ring |
|---|---|---|---|---|
| `0x04000000000044cd` | `0x0888000000000029` | `(16, 42206)` | 1394 | rectangle |
| `0x04000000000044cf` | `0x088800000000002a` | `(16, 42082)` | 1398 | slanted quad |
| `0x04000000000044d5` | `0x088800000000002c` | `(16, 42206)` | 1394 | rectangle |
| `0x0400000000004782` | `0x0888000000000035` | `(16, 42206)` | 1394 | rectangle |
| `0x0400000000004783` | `0x0888000000000033` | `(16, 42082)` | 1398 | slanted quad |
| `0x0400000000004784` | `0x0888000000000033` | `(16, 42082)` | 1398 | slanted quad |
| `0x0400000000004933` | `0x0888000000000038` | `(16, 42206)` | 1394 | rectangle |
| `0x0400000000004934` | `0x0888000000000039` | `(16, 42082)` | 1398 | slanted quad |
| `0x0400000000004936` | `0x0888000000000039` | `(16, 42082)` | 1398 | slanted quad |

- Quantised rings: rectangles `x = 0,0,800,800`, `y = 400,9,9,400`; slanted quads `x = 0,0,795,795`, `y = 400,9,-399,2`. All seven meshes: **4 vertices, 2 triangles**, both `[3,-1,0,-1,1,3]` and `[3,2,1,-1,2,-1]`.
- Payload, `0x0888000000000029`: 20 fields in ascending hash order, e.g. `0x20DE3871` (1220 B of pieces) at `+0x0008`, `0x8649028E` (32 B, `STU_537E56E4` carrying a 224-byte node array, 8 points, 384-byte triangle array, `nodeCount` 7, `height` 4) at `+0x1273`, `0xB7C8314A` at `+0x1341`, `0xC9C8A2AB` (24 B) at `+0x1353`, `0xCBCB2AB9` (9 B) at `+0x1381`.
- Only the **first** 14-byte flag record is stored in all seven; the second is absent, so it sits at the class default → folded to `0x3fff − 0x2000 = 0x1fff`.
- The ring is pinned at the clamp floor `max(1.0, [W+0xe0]) = 1.0`.

---

## 8. `docs/formats/chunk-kinds.md` (424 lines)

### 8.1 Status in one line (verbatim)

> It is now read, from two independent places, and the answer is the other way
> round: **kind `0x11` is `MAP_CHUNK_COLLISION` and kind `0x12` is
> `MAP_CHUNK_PATHING`.**

(lines 4-6)

> So the body is a prebuilt `MeshArrays` in the client's exact layout, in world
> coordinates, with its BVH already built. **The consumer function is still not
> read** - what is read is the container, the client's record layout, and the
> arithmetic above

(lines 311-315)

### 8.2 Address table

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x1439aa320` | (the kind table) | `0x24` entries, stride `0x20`: `+0x00` slot, `+0x08` kind id, `+0x18` name pointer; `+0x10`/`+0x14` raw, **no reader found** | read |
| `0x141b32e20` | `MapChunkGuid` | builds `0x0DD0000000000000 | (id << 32) | map` | read |
| `0x141b32e70` | `ChunkSlotForGuid` | scans all `0x24` entries comparing `(guid >> 32) & 0xFF`, returns the `+0x00` slot | read |
| `0x1404c19b0` | (the name de-obfuscator) | 61 `int3` bytes before it, one `ret` at `0x1404c24ee`; `p[k] ^= *(u8*)(p - 0xC + (k & 7))`, length = low 24 bits of the dword at `p-4` | read |
| `0x141ff54e0` | `FinishMapChunkAsset` | 964 bytes; dispatches on `ChunkSlotForGuid(work+0x70)`; cases `7`, `8`, `0xf`, `0x1d`, `0x1e`, `0x1f`, `0x21`, `0x22`, `0x23`; every case falls through to `FUN_141fe37a0(work, payload, size, 1)` and returns `0x11` | read end to end |
| `0x142e492a0` | (the two collision kinds' fixup) | converts four header qwords (`+0x10`..`+0x28`) null-preserving, then walks a `0x58`-stride array (count `+0x08`) adding the base to four qwords, and a `0x38`-stride array (count `+0x0c`) adding the base to three qwords — **unconditionally** | read + implemented |
| `0x141b53a70` / `0x141b53a60` | (the two element passes) | the `0x58` and `0x38` walks | read |
| `0x142e49270` | (two-pointer relocator) | streaming / additive-pathing | read |
| `0x142e49360` | (three-pointer relocator) | pathing header `+0x00`/`+0x08`/`+0x18` | read |
| `0x142e493a0` | (`+0x2E8`/`+0x2F0` relocator) | slot `0x1e` / `0x22` | read |
| `0x142e493e0` | (one-pointer relocator) | soundarea, `+0x08`, null when zero | read |
| `0x141fe37a0` | (the payload handoff) | stores the pointer at `work + 0x60`, sets flags, bumps a per-type byte counter — **consumes nothing** | read |
| `0x141ff51a0` | `MapChunkCellBytesFixup` | pathing only: 17 self-relative qwords per cell | read + verified elsewhere |
| `0x1421d26d0` | `StuLoadSubAssets` | compares the STU magic `0x53545544`; the pathing slots go through it | read |
| `0x1421c34b0`, `0x141b32e50`, `0x1421c3780` | sub-chunk container / GUID lookup / per-record | slot `8` | read |
| `0x142ccb1b0` | `BuildBvh` | **not** callerless: five `E8` sites in `0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c` | read (correction) |
| `0x142e4d920` | (the other `FUN_141fe37a0` caller) | a graph/serializer allocator over its own vtable objects, not a chunk reader | read |
| `0x1439aa0e8`, `0x1439aa0f0` | (name key / length dword) | the XOR key and the length dword of one name blob | read |

### 8.3 OPEN / NOT READ, quoted

1. chunk-kinds.md:377-395 — "1. **The consumer of the collision sectors is not read.** This is the blocker this file was asked to settle, and it stays open." — with the five checks that were made, and the correction that `BuildBvh` **is** reachable.
2. chunk-kinds.md:396-402 — "2. **What the sector counts and flags mean.** `+0x2C`/`+0x30`/`+0x34` are element counts by the tiling proof; the floats at `+0x20..+0x28`, `+0x38`, `+0x3C`, `+0x40`, `+0x48` and the dwords at `+0x4C`/`+0x50` are unread."
3. chunk-kinds.md:403 — "3. **The additive collision payload was not opened.** Same fixup, no sample."
4. chunk-kinds.md:404 — "4. **The 8 bytes at `+0x38`..`+0x40`** are outside the header as `FUN_142e492a0` models it and before array B's first run; they are covered by the payload's exact tiling but no structure names them."
5. chunk-kinds.md:315-318 — "**The consumer function is still not read** - what is read is the container, the client's record layout, and the arithmetic above; the interpretation (a world-space triangle mesh swept by the type-3 leaf `0x142caed30`) is the one the data supports"
6. chunk-kinds.md:373-374 — "**Not tested** against the shipped bytes of `0x142e492a0`."
7. chunk-kinds.md:21-22 — "| `+0x10` | `u32` | raw; no reader found |" and "| `+0x14` | `u32` | raw; no reader found |" (the kind table's two raw dwords)
8. chunk-kinds.md:239-241 — "the three dwords at `+0x2c`, `+0x30` and `+0x34` are the element counts of the four runs ... **derived, not read**"
### 8.4 Verified, and how

- `crates/ow-tank/tests/chunkkind_real.rs`, **7 tests, all passing**: (1) the two collision kinds are `0x11`/`0x10` and the two pathing kinds `0x12`/`0x23`, by name and by slot; (2) the table carries the eight placeable names previously established from the install, every slot present once, every kind id distinct; (3) the fixup arithmetic (header RVAs keep a zero as zero, every sector and tail qword gets the base, the tail's fourth qword is not converted); (4) a non-positive count runs no pass and a negative count is refused; (5) truncated payloads and counts past the payload are refused; (6) **install**: `0x0DD0001100000688` opens at 21,858,688 bytes and parses; (7) **install**: the kind-`0x12` chunk opens at 8,792,256 bytes and is refused by the collision reader.
- The payload measurements come from `dev/chunk11_probe.py` (asks `ow-tank-look --hex` for windows; never copies the 21.8 MB asset).
- The name decode is checked against **eight placeable names established from the install by a different route** (`MAP_PLACEABLE_MODEL_GROUP`, `_ENTITY`, `_LIGHT`, `_SOUND`, `_EFFECT`, `_TEXT`, `_REFLECTIONPOINT`, `_AREA` → slots `0x01`, `0x08`, `0x06`, `0x09`, `0x0A`, `0x05`, `0x03`, `0x07`).
- Implementation: `crates/ow-tank/src/chunkkind.rs` — `TABLE`, `Collision::parse`, `sectors`, `tails`, `Sector::counts`, `Sector::runs`, `Collision::run_bytes`, `Sector::vertices`, `Collision::check_sector_tiling`, `fixup_in_place`.
- **Not a harness unit**: `0x142e492a0`'s exactness rests on synthetic edge cases plus the real payload, not on an oracle call.

### 8.5 Install assets and numbers

- `0x0DD0001100000688` (kind `0x11`): **21,858,688 B**. Header: `+0x00` 0, `count_a` **36**, `count_b` **1**, `+0x10` 0, `+0x18` 0, array A `0x14D3810`, array B `0x14D8948`, `+0x30` `0xFFFFFFFFFFFFFFFF`. Tiling: `[0,0x40)` header; `[0x40,0x31CE30)` array B run 1; `[0x31CE30,0xA581D0)` run 2; `[0xA581D0,0x14D3810)` run 3; `[0x14D3810,0x14D8948)` 36 sectors of `0x58`; `[0x14D8948,0x14D8A00)` the single `0x38` record.
- Sector counts over the 36 records: `(32,28,116)`×4, `(8,6,24)`×4, `(10,10,36)`×4, `(13,10,42)`×4, `(14,9,42)`×4, `(8,6,24)`×10, `(12,8,36)`×6. Memory order always `+0x00`, `+0x18`, `+0x08`, `+0x10`. Sector 0's first vertex is `(16.224941, 8.705036, -3.820958)` (bits `0x4181CCAE 0x410B47D4 0xC0748A94`).
- **§4b, the tail element as the level collider**: node array `0x031CE30` count **236,957** stride `0x20`; vertex array `0x0000040` count **203,999** stride `0x10`; triangle array `0x0A581D0` count **343,474** stride `0x20`; then `30`, a float `745095.6`, `0x1FFF`. Tiling closes: `0x40 + 203,999*0x10 = 0x31CE30`, `+ 236,957*0x20 = 0xA581D0`, `+ 343,474*0x20 = 0x14D3810`. The tree is a **full binary tree**: 118,478 internal nodes + 118,479 leaves. First node box `(-103.469,-38.510,-161.028)..(206.159,51.071,148.317)`, second child `62,684`, mask `0x1FFF`, axis `0`, leaf count `0`. First triangle corners `120319, 120323, 120317`, two ids `181`, mask `0x1FFF`.
- `0x0DD0001200000688` (kind `0x12`): **8,792,256 B** — refused by the collision reader (its `+0x20` qword is inside its `STUD` blob).

---

## 9. `docs/formats/maps.md` — collision-relevant sections

### 9.1 Status in one line (verbatim)

> The map ships collision chunks kind `0x11` (`MAP_CHUNK_COLLISION`) and kind
> `0x10` (`teMAP_CHUNK_ADDITIVE_COLLISION`); pathing is kind `0x12`
> (`MAP_CHUNK_PATHING`) and `0x23` (additive). That mapping is now **read** from
> the client's own kind table and its name decoder ... The client draws
> the visual mesh only; there is no character collision implementation.

(lines 824-832)

> | half | state |
> |---|---|
> | chunk → STU sub-asset → cell container (slot `0x1e`/`0x22`, header, 17 arrays, pointer fixup) | **read, `fn/MapChunkCellBytesFixup` verified** |
> | cell container → shape records in the query world | **open** |

(lines 1296-1298)

> - **Open:** the client's *producer* of the bundle directory ..., the streaming
>   policy (which chunks load when), the entity-placeable layout and the
>   **collision mesh**.

(lines 59-61)

### 9.2 Address table (collision-relevant only)

| address | name given in doc | one-line claim | state |
|---|---|---|---|
| `0x141ff54e0` | `FinishMapChunkAsset` | dispatch on slot; `slot 0x1e`: `MapChunkHeaderFixup` + `StuLoadSubAssets(*payload)+0x68` + `MapChunkCellBytesFixup`; `slot 0x22`: same with `+0x08` | read |
| `0x142e492a0` | (two-collision-kind fixup) | two counted arrays `0x58`/`0x38` | read |
| `0x142e49270`, `0x142e49360`, `0x142e493a0`, `0x142e493e0` | the per-slot relocator family | two / three / `+0x2E8`+`+0x2F0` / `+0x08` pointers; each treats a stored `0` as null | read |
| `0x1421d26d0` | `StuLoadSubAssets` | the STUD loader; compares `0x53545544` | read |
| `0x141ff51a0` | `MapChunkCellBytesFixup` | the 17-pointer pass — verified as `fn/MapChunkCellBytesFixup` | read + verified |
| `0x14213d0b0` | `GeometryFinalize` | copies the `0x50`-stride transforms, per `0x60`-byte cell entry allocates a `0xb0` record, creates one body, builds a type-3 mesh source on the stack and calls `ShapeAddToBody`; stores the handle at the record's `+0x50` | read |
| `0x1421384e0` | `GeometryClear` | fixes the geometry object's array layout by what it frees | read |
| `0x14213c440` | `GeometryBuild` | node-type-2 build; **never reads the collision chunk**; allocates one `0x60` entry from `geometry+0x108`, builds the transform from `[geometry+0x88]+0x98`, `BrushTreeToMesh`, `MeshToColliderFaces`, appends the u16 array via `0x142139080` | read |
| `0x142cf23c0` / `0x142cf2010` | `BrushTreeToMesh` / `MeshToColliderFaces` | four / two call sites, all brush paths | read |
| `0x142139080` | (u16-array append) | appends the entry's u16 array to `geometry+0xf8` | read |
| `0x14213528a` | `GeometryIngestCandidateB` | per-entry shape builder: three `{pointer, count}` pairs at `+8`/`+0x10`, `+0x38`/`+0x40`, `+0x68`/`+0x70` with a **`0x60` record stride**; one body + one shape per record; connects with `FUN_142cb34f0` | read |
| `0x141b53b70` | `func_141b53b70` | copies `record[i]` at a `0x60` stride from each pair's array | read |
| `0x142cb9a50` | `FUN_142cb9a50(shape, quad)` | returns the nearest connect point; squared-distance compared | read |
| `0x142cb34f0` | `FUN_142cb34f0(body, quad, point, 1)` | the body↔shape connect | read |
| `0x142136010` / `0x1421360a0` | the two append helpers | `0x60` / `0x100` element appenders | read |
| `0x14213a510` | `AppendCollisionEntryFromBrushSource` | the brush ingest, read end to end (2011 bytes after the false-`noreturn` repair) | read |
| `0x14213acf0` | (second brush ingest) | second `0x100` appender caller | read |
| `0x142cb3940`, `0x142cc3d90`, `0x142cd4ef0`, `0x142cb7f10`, `0x142cae540` | the five add/build handles | the shape-producer census targets | read |
| `0x1420c63b0` | (the "obvious world ingest") | 10,500 bytes, four `ShapeAddToBody` sites; no code caller, no plain pointer | read (ruled out) |
| `0x14202c660` | (the scene ctor) | `0x3210`-byte geometry at `scene+0x620`, two worlds at `+0x31f8`/`+0x31fa`, a body at `+0x31fc`, registers the geometry with the world via `0x14209ae30` | read |
| `0x14209a8f0` | `GeometryBaseCtor` | 128 slots of `0x60` at `+0x60` | read |
| `0x142cc4a90` | (world-side registration) | stores the geometry at `world+0x160` | read |
| `0x1442a0bc0` | the handle table | single writer `0x142cc3bb0`; `0x142cc3be0` resets a record's bodies | read |
| `0x141c5ff40` | `RegisterNodeType` | **144** call sites (not 100); slots are static data `0x28` apart from `0x143b8e5c8` | read |
| `0x142191d00` | a class registrar (type `0x13`) | 44 bytes; allocates a `0x30` descriptor, stores the type at `byte[+8]`, decodes the name in place, calls `RegisterTypeWithSize` | read |
| `0x141ff33f0` | (map/chunk asset registrar) | map asset type `1`; chunk asset type `0xBC`, size `0x1f`; slots `LoadMapAssetChunks` `0x141ff5c80`, `0x141ff5bd0`, `ReleaseMapAssetChunks` `0x141ff58b0` | read |
| `0x141ff53e0` | (the only virtual that walks bytes) | slot `8`: the GUID-keyed sub-chunk container | read |
| `0x1400fdcb0` | (the pathing component's registration) | type `0x9199BF6B`, size `0x78`, align 8, ctor `0x140f91550`, vtable `0x14314acc8`, dtor `0x140f8f310`, four declared fields | read |
| `0x143c3f060`, `0x143c3c620`, `0x143c3efc0`, `0x140f84590`, `0x1421cdb90`, `0x1421cdfd0` | registered descriptor / field table | in the installed image both `0x143c3c620` and `0x143c3efc0` are **all zero** — the field table is built at runtime and does not exist as immediates | read |
| `0x1438846c0`, `0x1438846cc` | (name blob) | decodes to `Unknown` | read |
| `0x1420a02f6` | (the "shape-source assembler") | first called unreachable, then reachable (fragment of `0x14209fc90`, `E8` at `0x1420251f5`), then **not a chunk consumer** | read (three corrections) |
| `0x142e47a30`, `0x142cd7200`, `0x142cd7b20`, `0x142cb34f0`, `0x142cf10e0`, `0x142401de0` | helpers on the census paths | pointer decode, face walk, body↔shape connect, mesh measure, a diagnostic `memcpy` | named |
| `0x140c264b0` / `0x142077580` | the other `MeshShapeFill` callers | a collider emit / a computed box over a `0x58`-stride record | read |
| `0x142cdf9f2` | (a Domino body-list appender) | `0x40`-byte elements; appends them itself; no caller, no pointer | read (ruled out) |

### 9.3 OPEN / NOT READ, quoted

1. maps.md:59-61 — "- **Open:** the client's *producer* of the bundle directory ..., the streaming policy (which chunks load when), the entity-placeable layout and the **collision mesh**."
2. maps.md:1296-1298 — "| cell container → shape records in the query world | **open** |"
3. maps.md:1495-1500 — "**The collision consumer is still open, and that is the blocker now.** ... The missing read is the one both routes name: **who fills the Domino shape-entry pool at `[record+0x2b8]`**."
4. maps.md:1054-1056 — "So the decoder's contract is now concrete: **three arrays of `0x60`-byte records plus their counts**, per entry, in a `0x30`-byte source. What fills them from the 17 cell arrays is still open."
5. maps.md:1322-1330 — "**What is known about the consumer's target, for whoever picks this up** ... A consumer therefore needs the collision sub-asset base and a 17-array walk"
6. maps.md:109-111 — "- **The entity body is open.** Its 39,156 reference-shaped qwords never resolve to an asset, so the field is not a GUID"
7. maps.md:~1015 — "The one unread piece on that path is the **BVH builder** that fills the node block the narrowphase walks."
8. maps.md:1410-1414 — "What is still unwritten is the **chunk node type's build**: which of the 144 types takes the collision chunk and turns its cells into the `0x30`-byte type-3 mesh source."
9. maps.md (pathing section retraction) — "**Retraction.** The first version of this section said "no client code turns the cells into shapes in this image" and "the payload is never re-parsed". Both claims are **withdrawn**." and "The state is **open**. What follows is a census and a list of attempts, not a proof that the reader is absent."
10. maps.md:~1465 — "**Why a "data reference" here is not an invocation route.** ... so the three "unreachable" claims here are **not** relied on either way"
11. maps.md:~1440 — "**Project defect to know about.** `GeometryBuild` (`0x14213c440`) is stored as an **84-byte** function with no callers and no callees, while its real body is `0x14213c440..~0x14213d0b0` (3,184 bytes)"
12. maps.md:93-97 — "**The entity body is open.** Its 39,156 reference-shaped qwords never resolve to an asset"

### 9.4 Verified, and how

- `fn/MapChunkCellBytesFixup` (264 iterations + edge pool, 12/12 mutants) — the pathing container's runtime fixup, "verified", with the two allocate-then-deref-null mode-0 branches declared dead.
- `crates/ow-tank/tests/collision_cells_real.rs` — install test opening `0x0DD0001200000688`: pins the cell count (220), the first cell `0x31F8` / record `0x3208` / block `0x67F8`, the 17 boundaries `0xA0 .. 0x65F8`, the last cell's end `0x862700`, every array's extent, and (for `tables`) the three tiled array headers `+0x98` (222 × `0x28`), `+0x2358` (6 × `0x20`), `+0x2428` (220 × `0x10`) with `Error::ArrayNotTiled` when a span does not divide.
- `fn/CollisionSweepBounds` (`0x142cd5540`) and `fn/ShapeNarrowphase` referenced from here.
- Map-level oracles: `crates/ow-tank/tests/map_header_bundle_real.rs` (record byte, bundle GUID and offset, 1132-byte length, class `0x02514B6A`, `mapName`, `m_map`; the index prints 38,320 bundles, 0 malformed, 721,204 entries, 0 duplicates; header census 305/305) and `map_resolve_real.rs` (three variations, mode `0x18`'s single variation, four stored chunk references, 31 synthesised GUIDs, 34 chunks walked; placeables **1,358**, models **229**, entities **550**).
- **Not tested:** the client's bundle-directory producer; the producer of the level-load GUID lists; the kind-`0x12` chunk body layout at the placeable level.

### 9.5 Install assets and numbers

- Pathing chunk `0x0DD0001200000688`: **8,792,256 B** (`0x8628C0`); header `+0x00000` RVA `0x20`, `+0x00008` RVA `0x862720`, `+0x00010` 1, `+0x00018` RVA `0x862878` (a `0x48`-byte matrix block); two STUD v1 blobs at `0x20` and `0x862720`; the first region is a STUD v1 blob whose magic is `0x53545544`, count 1, record `{i32 0x20, u32 0}`.
- The pathing component's instance `{u32 class hash; u32 next}` = `0x9199BF6B`, `0`; field values `+0x10 -> 0x98`, `+0x58 -> 0x2358`, `+0x68 -> 0x2428`, plus a u64 at `+0x20`. Container-relative: header `0x2428`, entries `0x2438`, first cell `0x31F8`, first record `0x3208`, last cell's end `0x862700`; first record's 17 offsets `0xA0, 0xC8, 0x130, 0x1E0, 0x228, 0x280, 0x388, 0x648, 0x888, 0x8D0, 0x998, 0xD78, 0x1188, 0x1548, 0x2A58, 0x5618, 0x65F8`. Cell 86 has `0xA0` six times then `0xA8`.
- Array totals over the 220 cells: `arr0` 11,984; `arr1` 56,488; `arr2` 62,800; `arr3` 43,336; `arr4` 54,160; `arr5` 168,472; `arr6` 400,184; `arr7` 148,928; `arr8` 19,352; `arr9` 23,368; `arr10` 250,720; `arr11` 277,496; `arr12` 319,360; `arr13` 1,781,600; `arr14` 4,338,448; `arr15` 695,816; `arr16` 87,784. Cell 0's lengths: 40, 104, 176, 72, 88, 264, 704, 576, 72, 200, 992, 1040, 960, 5392, 11200, 4064, 512.
- Census figures: `ShapeAddToBody` **27 `E8` sites / 13 functions** (Ghidra reported 3); `OwnerCreateBody` 17 / 15; `ShapeAddToOwner` 1 / 1; `ShapeEntryFromDescriptor` 2 / 2; `MeshShapeFill` 5 / 5; **28 distinct producing functions**, 52 verified sites. Whole-copy route: 8,923 `memcpy`/`memmove` sites, exactly one with size `0x78` (`0x142402824`, a diagnostic string); 15 runs of ≥6 consecutive `xmm` loads off one base, all `0x70` or `0x80` bytes, none `0x78`.
- Practice-range spawn room: the trigger volume at `(71.169, 0.948, 53.110)` with half extents `(11.861, 5.179, 7.608)`; the collision floor under its centre is **`y = -4.006`**.

---

## 10. `docs/deviations.md` (37 lines)

### 10.1 Status in one line (verbatim)

> **This file is a removal list, not an accepted design.** Every row is code
> that does not yet match the client, carries the same `FIT-*` id next to the
> code, and must be deleted or replaced by a read implementation.

(lines 3-5)

> The simulation deviations are gone with the deleted movement/combat code. One
> `ow-tank` deviation remains (`FIT-ANI-07`); the rest are presentation and
> view-model framing in `ow-client` / `ow-scene`.

(lines 11-13)

### 10.2 Address table

**None.** This file cites no Ghidra address.

### 10.3 OPEN / NOT READ, quoted

Every row is an admission of unread client behaviour. Collision-relevant reads:
none — no row concerns collision, parsing or the sweep. The camera rows are the
only ones that touch the movement question:

1. deviations.md:22 — "`FIT-CAM-03` | Depth clear between world and FP passes; the client's exact pass boundary is unread."
2. deviations.md:23 — "`FIT-CAM-05` | Viewer eye height `1.75 m`; the client's own camera height is unread."
3. deviations.md:24 — "`FIT-CAM-06` | Whole FP rig rotated by camera pitch instead of the client's per-bone distribution."

### 10.4 Verified, and how

The file is the **enforcement mechanism**, not a claim: "`cargo xtask deviation-check` fails if a `FIT-*` id in the code is not in this table, or if a new id appears at all" (lines 8-9). `cargo xtask verify` runs it as part of the one gate. Collision code carries **no** `FIT-*` marker, so nothing in the collision path is registered as a known deviation — which is the strongest form of the "1 to 1" claim available without a differential unit.

### 10.5 Install assets and numbers

None.

---

## 11. `docs/systems-coverage.md` (59 lines)

### 11.1 Status in one line (verbatim)

Rows 12 (Collision), 27 (Movement, live) and 3 (Domino physics) are the
collision-relevant ones. Verbatim extracts:

> | 3 | Domino physics | tick driver, substep, constraint dispatch and integration functions located; the world schema is read (config-sized pools, `DmWorldConstruct`, `ShapePoolAlloc`, `ShapeEntryFromDescriptor`, `ShapeAddToBody`/`ToOwner`); the integrator, the descriptor producer and the tick parameters are **open** | `formats/domino.md` |

> the 4-simplex `0x142cc7740` is implemented and executed by that unit's tetrahedron cases). The shape **producers** are the open half: a 28-function census finds no reader of the pathing container's payload (the Domino-side candidates are unreachable) ... **the per-model transform is read and verified** (`fn/ShapeWorldTransform`, `ow_sim::collide::shape_world_transform`: 328 cases, every channel, coverage 100%, 9/9 mutants)

(row 12, ~line 20)

> the collision is assembled from the install in two producers - the map's **level collider** (`MAP_CHUNK_COLLISION`'s tail element: 203,999 vertices, 236,957 BVH nodes and 343,474 triangles in the client's own mesh record layout and world coordinates, `ow_tank::collision_level`, install test `collision_level_real`, written up in `formats/chunk-kinds.md` 4b: this is the floors, walls and stairs a character walks on) and the per-model blocking volumes (`ow_tank::range_world`: the `0x14201ae60` holder filter, the `STU_537E56E4` meshes and the map chunk's per-entry transform - 273 shapes at 3,018 placements, install test `range_world_real`)

(row 27, ~line 57)

### 11.2 Address table (collision rows only, condensed — each claim is a repetition of the owning doc)

| address | name given | claim | state |
|---|---|---|---|
| `0x142cb00c0` | `ShapePoolAlloc` | pool write path | read |
| `0x142cb7f10` | `ShapeEntryFromDescriptor` | pool write path | read |
| `0x142cb3940` / `0x142cd4ef0` | `ShapeAddToBody` / `ShapeAddToOwner` | pool write path | read |
| `0x141ff51a0` | `MapChunkCellBytesFixup` | pathing fixup, **verified** `fn/MapChunkCellBytesFixup`, 12/12 mutants | verified |
| `0x1420a02f6` / `0x14209fc90` / `0x1420251f5` / `0x142024d30` / `0x142019944` | assembler / fragment / call site / caller / update slot | the collision chunk's consumer is **reachable** but reads the model, not a chunk | read |
| `0x1420328c0` | (candidate evaluator wrapper) | `0x78`-byte descriptor of constants + record + block pointers → `0x14203dae0` | read |
| `0x14203dae0` | (the obfuscated collision query) | the geometry `world+0x620`, handle `[geometry+0x31f8]` | read |
| `0x142cc45a0` | (handle → Domino record) | via `0x1442a0bc0`, descriptor + block | read |
| `0x142cbd510` | (four-pass BVH walk) | read | read |
| `0x142cd5540` | `sweep_bounds` | **verified** `fn/CollisionSweepBounds` | verified |
| `0x142cc1610` | `ShapeNarrowphase` | **verified** `fn/ShapeNarrowphase`: 264 cases, every channel, 3/3 reachable, 11/12 mutants | verified |
| `0x142ccdf40` / `0x142ccd7b0` | sphere / capsule | verified on their no-support-point path (`fn/CollisionSphere`, `fn/CollisionCapsule`) | verified |
| `0x142caed30` | type-3 mesh walk | `fn/MeshSweep`, verified on its small-expansion analytic path | verified |
| `0x142cd3c80` / `0x142cd0ae0` | type 2 / type 4+ | **read**, not implemented → `Narrowphase::UnreadType` | read, not implemented |
| `0x142cc9460` | closest-point GJK | **verified** `fn/ClosestPointGjk`: 264 cases, 74.5%, 12/12 | verified |
| `0x142cd58a0` | (per-triangle accept) | **verified** `fn/BoxTriangleOverlap`: 413 cases, whole return register, 100% coverage, 12/12 | verified |
| `0x142caea70` | gather | **verified** `fn/MeshGather`: 330 cases, 90% coverage with two dead edges declared, 28/28 mutants | verified |
| `0x142cc7740` | 4-simplex | implemented, executed by the unit's tetrahedron cases | implemented |
| `0x142003720` / `0x142000620` | the scene component's update / local-pose writer | writes `+0x140`..`+0x16c`; `0x142000620` writes local `+0xd0` quaternion / `+0xe0` scale / `+0xf0` position | read + verified as `fn/ShapeWorldTransform` |
| `0x14202c660` / `0x14202d268` / `0x14202f800` | node init / its `OwnerCreateBody` / owner-id accessor | a node's world holds its own static body | read |
| `0x14200a9a0` / `0x14200f840` / `0x14201cf40` | component ctor / pair-with-node / attach | the component is named | read |
| `0x143367630` | `WorldBase` vtable | slot 8 = the creation entry; disp8 form `FF 5x 40` needed (`dev/call_slot_scan.py`: 505 vs 0 disp32) | read |
### 11.3 OPEN / NOT READ, quoted (collision rows)

1. systems-coverage.md:20 (row 3) — "the integrator, the descriptor producer and the tick parameters are **open**"
2. systems-coverage.md:20 (row 12) — "The shape **producers** are the open half: a 28-function census finds no reader of the pathing container's payload (the Domino-side candidates are unreachable)"
3. systems-coverage.md:20 (row 12) — "the chunk's own consumer is unknown"
4. systems-coverage.md:20 (row 12) — "Open: whether a placed node passes the gate (`+0x454` bit `0x200` from the node creation record's `+0x10`)."
5. systems-coverage.md:20 (row 12) — "the brush route is read down to the ring builder and implemented in `ow_tank::brush` (11 units, oracle-pinned) but the install opens **0** type-`0x00A` assets and the tree→mesh ops are unread" *(the ops were read later, in `brush-mesh.md` — this row is stale)*
6. systems-coverage.md:20 (row 2, movement) — "the exact `in_vec`, the actor `+0x38` vtable, the `+0x58/+0x60/+0x68` virtuals, the live service object, the velocity->position stage, `FUN_1420163b0` and collision are **open**"
7. systems-coverage.md:57 (row 27) — "Open and declared: the input stage (buttons -> local wish), the character sweep's Domino aftermath (`0x142cf3f80` is read; its shape-transform write and pair lanes are not ported), the movement asset (speed/jump/gravity; free fall is a documented constant), and the player spawn selector"

### 11.4 Verified, and how (as this file states it)

Verified leaves named for the collision chain, with their case counts:
`fn/CollisionSweepBounds`, `fn/ShapeNarrowphase` (264 cases, every channel, 3/3 reachable, 11/12 mutants), `fn/CollisionSphere`, `fn/CollisionCapsule`, `fn/MeshSweep`, `fn/ClosestPointGjk` (264 cases, 74.5%, 12/12), `fn/BoxTriangleOverlap` (413 cases, 100%, 12/12), `fn/MeshGather` (330 cases, 90%, 28/28), `fn/ShapeWorldTransform` (328 cases, 100%, 9/9), `fn/MapChunkCellBytesFixup` (12/12), `fn/Mrnm*`, `fn/BuildBvh`. Install tests: `collision_cells_real`, `collision_level_real`, `range_world_real`, `spawn_room_real`, `mapchunk_kinds`.

### 11.5 Install assets and numbers

- Level collider: **203,999 vertices**, **236,957 BVH nodes**, **343,474 triangles**.
- Per-model volumes: **273 shapes at 3,018 placements** (`ow_tank::range_world`, `range_world_real`); elsewhere "**3,017** instances" (row 12) — a one-off discrepancy worth reconciling.
- Walk test: 5.5 m forward in one second at 5.5 m/s; no idle drift; 5.5 m strafe; a six-second turning walk keeps its height; a twenty-metre drop lands.
- Spawn room floor `y = -4.006`.

---

## 12. Cross-doc contradictions (resolve these first)

These are places where two docs make incompatible claims about the same address
or the same route. A reader who trusts the wrong one will chase a phantom.

**12.1 The model mesh's node/triangle element layout.** `static-collision.md:358`
("What is still open here, exactly … The element layouts of the node array
(`32` B) and the triangle array (`16` B)") versus `static-collision.md` check 16
and `shape-narrowphase.md` ("triangle stride **`0x20`**", "the `24 × 16` the byte
count alone suggested" is corrected). **Check 16 wins**; the check-4 paragraph is
stale text and should be read as superseded.

**12.2 Whether the Domino world is filled at all.** `static-collision.md:34`
("The range's static collision has no producer in the read paths"),
`shape-ingest.md:279-300`, and `systems-coverage.md` row 12's earlier clauses say
nothing fills the world; `static-collision.md` checks 8/9/16 and
`systems-coverage.md` row 27 say the **per-model builder does fill it** (272
shapes over 161 models). The resolution the docs converge on is: the Domino world
**is** filled per model, and what is unwritten is the **kind-`0x11` chunk's**
producer. `systems-coverage.md` row 12 still contains both halves in one cell and
reads as if it contradicts itself.

**12.3 Who produces the shape source descriptors.** `domino.md:292`
("the remaining unknown is **the producer of the `source` descriptors**"),
`maps.md:1496-1500` ("who fills the Domino shape-entry pool at
`[record+0x2b8]`") and `shape-ingest.md` §7 ("the brush, and there is no second
feeder") versus `static-collision.md` check 9 ("the only producer of
`ShapeAddToBody` shapes that walks a model's holder list … answers
`domino.md`'s open question") and `shape-ingest.md`'s own `ShapeAddToBody`
census (27 sites in 12 groups, of which `0x14201ae60` is one). **The per-model
builder is a third feeder the pool census knows about but `domino.md` and
`shape-ingest.md`'s conclusions do not mention.** Re-run
`dev/pool_scan.py calls 0x142cb3940` and check whether `0x14201ae60` is in the
12 groups; if it is, §7's "the brush remains the only producer" is wrong.

**12.4 `[record+0x2b8]`: "pair pool" or "fixtures / shape entries"?**
`domino.md:42` ("index 2 `0x2b8` **fixtures** `0xc0`") versus `domino.md:428`
("`+0x2b8` | pair pool, stride `0xc0` … pair walks"), and
`static-collision.md` check 6's whole "why it is not the Domino world" argument
rests on calling `+0x2b8` a *pair* pool. `shape-narrowphase.md` uses
`record[0x2b8] + shape * 0xc0` as the **shape entry**. Same stride, two names.
The label must be settled because check 6's negative depends on it.

**12.5 `0x14208edf6` "no static reference".** `brush-mesh.md` first says the
parameter-block writer "has **no static reference at all**", then corrects it:
"the real jump table dispatch at `0x14208e74e` (`mov ecx,[base+rax*4+0x2090988];
add; jmp rcx`)". Any later claim of the form "0 `E8`/`E9`/pointer, therefore
unreachable" must be re-tested against that 4-byte-RVA form first.

**12.6 `0x1420a02f6` reachability.** Three successive verdicts inside one doc
family: unreachable (`maps.md` first), reachable as a fragment of `0x14209fc90`
(both docs' correction), and "not a chunk consumer at all"
(`static-collision.md` check 15). Only the last is current, and
`systems-coverage.md` row 12 still describes it as "the collision chunk's
consumer is **reachable**".

**12.7 `0x142cc054f` vs `0x142cc04f0`.** `domino.md` originally cited
`0x142cc054f` as the destructor; `shape-ingest.md` §1 corrects it to
`0x142cc04f0` (with `0x142cc054f` inside it) and `domino.md` now carries the
correction. Also `0x142cb0a73` → group start `0x142cb0a20`, and
`0x14213bbe6` → group start `0x14213bbc0`.

**12.8 `ShapeAddToBody` caller counts.** `maps.md` says "27 sites in **13**
functions"; `shape-ingest.md` says "27 sites in **12** function groups" and
explains the difference (a chained fragment `0x14201b1cd` was counted as a
function). Ghidra reports **3**. Use 12 groups.

**12.9 `0x1421360a0`'s caller count.** `maps.md` originally: "exactly one caller,
the brush ingest"; `shape-ingest.md`: "**two** callers, `0x14213a510` and
`0x14213acf0`, both brush". Two.

**12.10 `geometry+0x114` writers.** `maps.md` says "**three** sites, all in the
geometry module: `0x14213a965`, `0x1421388dd`, `0x14213d559`"; `shape-ingest.md`
says "The "three writers" figure was not reproduced by this scan and should be
re-derived before it is relied on." **Treat as unverified.**

**12.11 Placements: 3,017 vs 3,018.** `systems-coverage.md` row 12 says the
offline probe "places all **3,017** instances"; row 27 says "273 shapes at
**3,018** placements". One of the two is a typo or an off-by-one in the count of
the probe's own output.

**12.12 `brush-mesh.md` is internally duplicated.** "## The whole conversion,
pinned by the emulator", "### Three inputs that make the client fault" and
"## The model descriptor `C`" each appear **twice**, with different text in the
`C` section (the second copy retains the "no static reference at all" claim that
the first copy corrected). The doc should be de-duplicated before it is used as
an anchor.

---

## 13. The fifteen most important gaps, ranked

Ranked by "how much gameplay is blocked per unit of reading", with the exact
address to open next. Each is stated as the doc states it.

1. **The producer of the kind-`0x11` collision chunk (world-space prebuilt mesh).**
   `chunk-kinds.md` §4b proves the payload's tail element is a complete
   `MeshArrays` (203,999 vertices, 236,957 nodes, 343,474 triangles) in the
   client's exact record layout, and says "**The consumer function is still not
   read**". No code reads `FUN_142e492a0`'s arrays apart from the fixup itself.
   Next read: the callers of `FinishMapChunkAsset`'s slot-`0x1d`/`0x21` path, or
   the `FUN_141fe37a0` consumer of `work+0x60`. Addresses `0x142e492a0`,
   `0x141fe37a0`, `0x141ff54e0`. **This is the single largest unread block of
   collision data in the client.**

2. **The entity-side driver that instantiates the collision component for a
   node.** Everything else in the per-model route is read (ctor `0x14200a9a0`,
   allocation `0x14200a5f0` with `0xad0`, registration `0x14200bab0`, attach
   `0x14201cf40`, hook `0x14200ee80`), but all of it is reached virtually only.
   The driver walks an entity's component list and calls the type object's
   slot 1. Next read: the entity/component module around `0x14077f000`, i.e. the
   caller of `0x1433633c8 + 8`. Without it, "how many bodies the practice range
   has" cannot be stated.

3. **The producer of the `WorldBase` slot-8 creation record.** `0x14202c660`
   reads the record's `+0x10` (flags, bit `0x200` gates the geometry) and `+0x18`
   (the Domino world settings description) but nothing in the image is shown to
   write either. All 632 dispatch sites of slot `0x143367670` are enumerated and
   none passes such a record. Next read: the writer that supplies the bits, per
   `static-collision.md` check 18 — a writer that must supply a *settings
   description*, not a placement.

4. **The asset-preload completion poller.** `0x141fe0f80` (service vtable
   `0x142f8d970` slot `+0x38`) only sets the loaded byte at `+0x78` and clears
   `+0x70`; the level start is **polled** by whoever reads `service+0x78`, and
   the service pointer is runtime-encoded through `0x14054a010(context)`. This is
   the named next anchor for the map-load chain and therefore for anything the
   load path feeds.

5. **The per-model transform writer for a placed map entity.** `0x142000620`
   writes the scene component's local `+0xd0` quaternion / `+0xe0` scale /
   `+0xf0` position and is *verified* as `fn/ShapeWorldTransform` (328 cases,
   100%, 9/9 mutants), but "the writer of `+0xd0`/`+0xf0` for a placed map
   entity is still unread" (`static-collision.md:2900`). Until then every range
   sweep reports a surface in a local frame, not a map location.

6. **The `+0xd0`/`+0xf0` source class of the builder's second argument.**
   `0x141ff90e0` is read (kind `0x14`, size `0x300`, name `Unknown`), but which
   class the *entity* record at `[entity+0x250]` belongs to, and what writes the
   pose for a placed prop, is open (`static-collision.md` check 9 item 2).

7. **`0x142cbd190(holder, [mover+0x650])`.** Called at `0x140ca6139`
   immediately after the character's sweep record is created; unread, together
   with the meaning of `[mover+0x650]`. This is the per-frame rebuild question
   and it sits directly on the character's collision path
   (`static-collision.md:1476`).

8. **The consumer of the shape-source assembler's four arrays.** `0x14209fc90`
   fills a `0x50`-byte context at `[component+0xc8]` with four arrays (mesh
   `0x60`, point cloud `0x70`+header, capsule `0x50`, sphere `0x40`), and "the
   only reads and writes of `[component+0xc8]` in the component's module are the
   build (`0x1420251be`/`0x1420251e8`) and the teardown (`0x1420251fc`,
   `0x142020cdd`, `0x142021afb`) sites, so nothing in the read set consumes the
   four arrays". A whole per-holder shape model is produced and dropped.

9. **The two unread narrowphase primitives.** Type 2 `0x142cd3c80` (face-plane
   convex hull with an analytic plane walk + GJK fallback) and type 4+
   `0x142cd0ae0`/`0x142cd0540` (height-field grid, rectangle scan + 2D DDA) are
   read but not implemented, so `Narrowphase::UnreadType` is returned. Type 4's
   DDA parameter/increment formulas "need a lane-by-lane re-derivation", and no
   producer of a `+0x330` descriptor was traced.

10. **The sink push `0x142cc1450` and the sink record layout.** "Not verified…
    It is the reason all four declared dead edges are dead"
    (`shape-narrowphase.md:285`). Every client query that publishes through the
    sink is unmodelled.

11. **The brush's source class `C` is named but its owner is not, and the scale
    producer is unread.** `C` = class `0x564B1018`, size `0x198` (solid, by four
    independent agreements), but the class at `[node+0x38]`/`[+0x40]` and the
    writer of `[geometry+0x88]+0xe0` (the ring's scale) are open; the store into
    `geometry+0x88` at `0x142138e89` sits in a region that is not code. The
    seven real `C` rings are therefore pinned at the clamp floor `1.0` only.

12. **`BuildBvh`'s sector-reader candidates.** `0x142ccb1b0` is *reachable*
    (five `E8` sites in `0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c`), and
    `chunk-kinds.md` §6 names those three functions as the live candidates for
    the collision-sector reader. Nobody has read them for a sector-array access.

13. **The `dmBody` mass/fixture writer and the tick numbers.** "No dedicated
    body constructor was located… the writers of `+0x164/+0x168/+0x16c`,
    `+0x170/+0x174`, `+0x198` and the fixture fields are unattributed", and the
    dt scalar / substep count / iteration counts "flow in from above
    `FUN_142cdfc50` ← `0x14208c190`" unread. The solver's own
    `RotationInertiaSolve` (`0x142d07170`) closed form is also open.

14. **`FUN_142cef7f0`, the `+0x58 == 0` emission variant.** Read only
    structurally; `GeometryBuild` selects the other variant, so the second
    emission path is untested and unimplemented (`Err`).

15. **The brush ingest's unread tails.** `0x14213acf0` "is read only in outline"
    (the entry's fields past `+0xb0` are not decoded), `GeometryBuild`'s
    `0x60` array-A entry's own field meanings are not named, and whether a
    two-ring brush can bridge its hole (or is a client bug) needs a real brush's
    ring order — the last of which is now available from the seven `C` assets,
    so this one is cheap.

**One meta-gap.** `shape-ingest.md` §7 concludes "the brush remains the only
producer" of collision material, but its own census target `ShapeAddToBody` has
27 sites in 12 groups and one of them is the **per-model** builder
`0x14201ae60`. Either the census's classification of that group is wrong, or §7's
conclusion is too strong. This is a one-command check
(`dev/pool_scan.py calls 0x142cb3940`) and it decides whether the map's static
collision is brush-only or model-only.

---

## Appendix A — every script and differential unit the eleven docs name

Useful when checking a claim: each row is a claim that can be re-run.

### A.1 Ghidra / image probes (`dev/`)

| script | what it answers |
|---|---|
| `dev/pe_layout.py --addr <addr>` | raw / zero-fill / unmapped (is a zero read real?) |
| `dev/fn_extent.py <addr> [--group]` | the `.pdata` chain: root, every fragment, contiguity |
| `dev/callers.py <addr>` | `E8`/`E9`/`jcc`/8-byte pointer/4-byte RVA references, mapped to chain roots |
| `dev/rip_refs.py <addr>` | rip-relative references to a table or slot |
| `dev/disp_scan.py <off> [--writes]` | every operand at a displacement, re-decoded with capstone |
| `dev/static_rva_tables.py <rva>` | image-base-relative RVA switch tables and whether a target is in one |
| `dev/xref_rva.py` | whole-image 4-byte RVA scan |
| `dev/call_slot_scan.py <slot> <end>` | both call encodings (`FF /2` disp8 and disp32), re-decoded |
| `dev/slot_call_backscan.py <slot> <end>` | register-carried slot value → `call reg`/`jmp reg` (109 vs 96 sites) |
| `dev/slot_reg_scan.py` | the forward-direction twin (32,414 loads at `+0x40`) |
| `dev/static_field_scan.py <hash…>` | where a field hash appears as an immediate |
| `dev/static_slots.py --vt <vtable>` | per-slot displacements read by a vtable |
| `dev/static_model_schema.py <installer>` | a class's field table from its registration function |
| `dev/pool_scan.py {writes,reads,func,calls,at,sentinel}` | the Domino pool store census and its `-1` sentinel discriminator |
| `dev/pool_lea_args.py 0x108 0x118` | every `lea reg,[base+off]` and the call it feeds |
| `dev/model_registry.py [--hash H]` | the 7,956 `RegisterComponentTypeDescriptor` call sites → hash, size, align, hooks |
| `dev/model_class_search.py --min-size S --offset O` | reference-registry class search |
| `dev/probe_node_types.py [--ingest]` | the 144 node types and their class registrars |
| `dev/find_field_writes.py` | displacement store census with `disp8` **and** `disp32` |
| `dev/rd.py <start> <end>` | linear disassembly across a region |
| `dev/data/read/refs.py <addr>` | all six reference spellings (8-byte, 4-byte RVA, both obfuscated forms) |
| `dev/data/read/census.py <targets…>` | the shape-producer census |
| `dev/chunk11_probe.py` | the kind-`0x11` container windows (never copies the 21.8 MB asset) |
| `dev/gjk_simplex_diff.py` / `dev/gjk_closest_diff.py` / `dev/mesh_support_diff.py` | the three GJK / support-hull differentials |
| `dev/brush_vertex_oracle.py` | `0x142ced510` + `0x142ced620` under the emulator (allocator stub at `0x1442a0158`) |
| `dev/brush_mesh_oracle.py` | `0x142cf23c0` end to end, both emission stages |

### A.2 Differential units (`dev/verify_harness`, named `fn/<Name>`)

`fn/ClosestPointGjk` (`0x142cc9460`), `fn/ShapeNarrowphase`
(`0x142cc1610`), `fn/CollisionSweepBounds` (`0x142cd5540`),
`fn/CollisionSphere` (`0x142ccdf40`), `fn/CollisionCapsule` (`0x142ccd7b0`),
`fn/MeshSweep` (`0x142caed30`), `fn/BoxTriangleOverlap` (`0x142cd58a0`),
`fn/MeshGather` (`0x142caea70`), `fn/ShapeWorldTransform` (`0x142cb8b80`),
`fn/MapChunkCellBytesFixup` (`0x141ff51a0`), `fn/BuildBvh`
(`0x142ccb1b0`, `dev/verify_harness/units/fn-BuildBvh.json`),
`fn/VariantPosition`, `fn/RigidPosition`, `fn/CharacterPosition`,
`fn/WorldPosition`, `fn/ShapeDispatch`, `fn/ShapeSphere`, `fn/ShapeCapsule`,
`fn/ShapeBox`, `fn/ShapePoints`, `fn/MoveCharacterController`,
`fn/MoveCharacterControllerShape`, `fn/ApplyCharacterControllerMove`,
`fn/SetCharacterControllerVelocity`.

**Gaps in the unit set that the docs themselves name:** no unit for the sink
push `0x142cc1450`; no unit for types 2 and 4+; no unit for
`FUN_142cef7f0`; no unit for the brush stages (emulator probe instead, because
they allocate through the image's allocator hook); no unit for
`0x142e492a0` ("**Not tested** against the shipped bytes"); no unit for
`domino.md`'s integrator, `RotationInertiaSolve`, or the body mass/fixture
writers.

### A.3 Install tests named

`static_collision_real`, `model_mesh_real`, `model_schema_real`,
`model_body_real`, `model_shape_source_real`, `mesh_world_real`,
`model_collision_real`, `brush_real`, `chunkkind_real`, `collision_cells_real`,
`collision_level_real`, `range_world_real`, `spawn_room_real`,
`map_header_bundle_real`, `map_resolve_real`, `practice_range_real`,
`mapchunk_kinds`, `manifest_154504_real`, `hero_select_real`, `hero_pick_real`.

---

## Appendix B — what this distillation did not cover

- `maps.md` outside its collision sections (placeable tables, model/look joins,
  the bundle path, spawn records) was read only where it bears on collision.
- `shape-ingest.md`, `shape-narrowphase.md`, `domino.md`, `brush-collision.md`,
  `brush-mesh.md`, `model-collision.md` and `chunk-kinds.md` were read in full.
- `static-collision.md` was read in full (all 18 checks and the checks table).
- The docs' *implementation* claims (that `ow_tank::x` matches the doc) were
  taken at face value; no Rust was read and no test was run.
