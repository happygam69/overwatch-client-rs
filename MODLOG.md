# MODLOG

Milestones only. Findings and addresses live in `docs/formats/`; current state
in `STATUS-handoff.md`; the work queue in `docs/open-questions.md`.

## 2026-11-02 - the reachable assembler reads the model, not the chunk

- `0x14209fc90` is reachable (called at `0x1420251f5` from the component
  update), but its `[r8+0x140]` list is the **model**
  (`[[node+0x110]+0x28]`, else `0x141b3d1c0(node)`) and its filter is
  `holder+0x2a == 1` -- the per-model body builder reads the same list with
  `== 0`. Its `0x50`-byte context is scratch, allocated and freed around the
  call.
- The `+0x2a` mode byte is a declared holder field (`0x89AABA94` registered at
  `+0x2a` by `0x141510382`), default 0. On the range the assembler's plan is
  **empty**: 279 holders, 0 carry the byte (`ow_tank::model_shape_source`, 7
  unit tests, install test `model_shape_source_real`; counterfactual mode 1 gives
  272 mesh entries).
- So the chunk's own consumer is unknown again, and the chunk-route correction
  in maps.md/open-questions is narrowed accordingly.

## 2026-11-02 - the chunk assembler is on the component-update path

- `0x1420a02f6` is a `.pdata` **chained fragment**; the chain root is
  **`0x14209fc90`** (four entries to `0x1420a075d`), with one `E8` at
  **`0x1420251f5`** inside `0x142024d30`, called from the collision component's
  per-frame update slot (`0x142019944`, slot `0x142f7d688`) and from
  `0x14201fbe3`. The earlier "chunk consumer unreachable" verdict (and the
  277-table RVA zero) measured the fragment start and is withdrawn in
  `maps.md`, `open-questions.md` and `systems-coverage.md`.
- The `WorldBase` slot-8 dispatch census is complete in every encoding: 492
  distinct `call [reg+0x40]` disp8, 0 disp32, 31 `jmp [reg+0x40]`, 109
  register-indirect (new `dev/slot_call_backscan.py`), and the slot's address has
  no rip-relative reference; no site passes a creation record. The creation path
  goes through an obfuscated chained fragment (`0x14055a0f0`), which is why the
  decompiler sweep cannot follow it.
- Open: the chunk assembler's **descriptor-container producer** and the record's
  `+0x10`/`+0x18` writers.

## 2026-11-02 - WorldBase slot 8, and a disp8 slot-scan defect

- `0x14202c660` is **slot 8** of the `WorldBase` vtable `0x143367630`
  (`0x143367630 + 8*8 = 0x143367670`); the earlier "slot 32" measured `0x100`
  from a neighbouring table base. `WorldBase` is `0x910` bytes (ctor
  `0x1420271d0`, allocator `0x1420266d0`), and NodeType2's factory `0x142029f00`
  registers a `0x910` class with kind `0x14`.
- Tool defect: `call qword ptr [reg+disp]` has a disp8 form (`FF 5x 40`). The
  image has **0** disp32 forms for `+0x40` and **505** disp8 sites, so a
  disp32-only scan reports a false negative for every slot below `0x80`.
  `dev/call_slot_scan.py` now scans both.
- Measured negatives: none of the 505 `+0x40` sites passes a creation record,
  and kind `0x14` is a **family** (160 `RegisterTypeWithSize` sites, sizes
  `0x18`..`0x31868`); identity is the size `0xad0` (one site, `0x14200bbb1`),
  so no kind-based install census is valid.
- Open: the slot-8 transfer including register-indirect forms, and the writer of
  the record's `+0x10`/`+0x18`.

## 2026-11-02 - the component class, and the character's world is the node's world

- The collision component: constructor `0x14200a9a0` (`0xad0` bytes; primary
  vtable `0x142f7d608`, base interface `0x142f7d648` at `+0x20`, third vtable
  `0x142f7d458` at `+0x118`), registration `0x14200bab0` (kind `0x14`, size
  `0xad0`, name `Unknown`), type-object vtable `0x1433633c8` slot 1 the factory
  `0x14200a5f0`, pair-with-node `0x14200f840` -> `0x14201cf40(object, entity)`.
  The attach hooks test no asset-derived condition.
- The character's `[controller+0x1ad8]` is written only at `0x140ca6132` from
  `0x142cbcd40`; the mover descriptor `+0x46` is `0x14202f800(node)` (the `u16`
  at `[node+0x620]+0x31f8`), the same accessor the per-model builder uses. So the
  character and the per-model bodies share the node's world slot.
- Open: whether a placed node passes the node gate (`[node+0x454]` bit `0x200`
  from the node creation record `+0x10`); the record's producer is unread and is
  runtime data, so no install test is written (`not tested`).

## 2026-11-02 - the per-model body's owner is a Domino world slot

- `0x14202f800` returns `[[node+0x620]+0x31f8]` (a `u16`); `OwnerCreateBody`
  decodes it through `0x1442a0bc0` and the same addends/XOR as the character
  sweep, rejecting `>= 0x3fff`. The world allocator's inverse makes the empty
  slot exactly `encode(0)`; `ow_tank::domino_world` (7 tests) pins both.
- `0x14202c660` allocates two worlds per node (`+0x31f8`/`+0x31fa`) and creates
  the node's own static body from `+0x31f8` at `0x14202d268`; per-model bodies
  are siblings of it in the node's world.
- The attachment gate is read field by field (`+0x454` bit `0x200`, `+0x1c`
  bits `0x20`/`0x40`, model `+0x84` bit `0x80`, `+0x242` bit 7) and is **runtime**
  data, so whether the range's props pass is not decidable from assets; that is
  stated as not tested, not faked.
- Open: which class allocates the component (interface table `0x142f7d320` is a
  COMDAT-folded shared base) and whether the character's own handle names the
  same world slot.

## 2026-11-02 - collision is built per model instance, and it is live

- The per-model body builder is **`0x14201ae60..0x14201c887`** (a six-entry
  `.pdata` chain). The earlier `0x14201af49`/`0x14201b1cd` were fragment starts
  inside it; scanning them found no reference, scanning the root finds four `E8`
  callers (`0x142022030`, `0x1420195e0`, `0x142014b50`, `0x142015f70`), all
  runtime state-sync functions of one collision component.
- It walks `[model+0x140]`, filters `[holder+0x2a]`, the mesh and the mask word
  (bit 25 of `[component+0x420]` selects `+0x580`/`+0x470`), and emits one
  Domino shape per selected holder through `ShapeAddToBody`/`OwnerCreateBody`;
  mode 0 is the mesh (type 3).
- `ow_tank::model_body` (8 unit tests) + install test `model_body_real`: the
  range's 229 models -> 279 holders -> 272 mesh blocks, mask all ones, filter
  byte and shape mode at the class default 0, so **all 272 mesh holders become
  type-3 mesh shapes over 161 models**.
- Open: which nodes receive the component (interface table `0x142f7d320`, no
  RTTI or self-pointer) and whether the range's static props receive it.

## 2026-11-02 - the sweep's world is the Domino world; no map source reaches it

- `RunCharacterControllerSweep` (`0x142cbcee0`) takes the world struct as
  `param_1`; its one real call is the 10-byte thunk `0x142cf5660` to
  `0x142cf3f80` (four `.pdata` chunks). The world struct's first dword is a
  variant tag; both routes decode the handle through `0x1442a0bc0` and walk
  `[world+0x2b8]` (`0xc0` entries) and `[world+0x348]` (`0x58`). Zero indirect
  calls in all four chunks; no call to the scene-side query owner.
- Verdict: the model mesh is query-only, the brush needs a `C` the range lacks,
  the chunk is unreachable, and the character ingest's six callers are all
  mover/AI. **No read path inserts the practice range's static collision into
  the Domino world.** Scope statement; the pool-A importer `0x1421363d1` is the
  one named unread anchor.

## 2026-11-02 - the model mesh is query-only; the Domino world has no map source

- `0x142caf6c0` writes nothing persistent: it bounds the query points, collects
  candidate triangles into an inline `0x80`-entry stack buffer and runs
  `0x142cc6740` per triangle. Its owner `FUN_14212e070` is a scene-side static
  collision **query** owner (0x18-byte entries, six bounding volumes, lazy
  broadphase), not the Domino world. Triangle stride corrected to `0x20`
  (12 records of 32 B for the box, not 24 x 16).
- The `f0` class's slot `+0x50` (`0x142134f70`) reads `C` and builds the
  fracturable body. The character ingest `0x142cbcd40` has six callers, all in
  the mover/AI ranges; `RunCharacterControllerSweep`'s nine `E8` sites likewise.
- Verdict: no read path fills the character's Domino world with the practice
  range's static collision. The last read is whether the sweep queries the
  scene-side owner through a vtable; otherwise this is a scope decision.

## 2026-11-02 - the model-mesh consumer read; the offset was +0x140, not +0x118

- The second geometry class (`0x142132f20`) is not the consumer: its 20 slots
  never read `+0x118`/`+0x140`. The client registers `STUModel` at `0x200`
  bytes and the field registration passes `0x140` (proved by the same helper's
  offsets for `C`).
- Consumer `FUN_14210bac0`: holder pointers, requires `holder+0x20 == 0` and
  `holder+0x08 != 0`; calls `0x141b53af0` and `MeshShapeFill` (`0x4c`-byte
  record: scale, 1/scale, three pointers, four words, `scale*m_5BD33E19`),
  consumed by `0x142caf6c0`. **No vertex conversion — the mesh is passed by
  pointer.** `ow_tank::mesh_shape` (4 tests). Install: 272 blocks, first is a
  4-unit box (9 nodes, 8 points, 16 triangles).
- The path calls no shape-add and its caller is a broadphase
  (`FUN_1421088e0` -> `0x14212e070`), so whether the model mesh is the
  character's world or query-only is the next read.

## 2026-11-02 - the static collision is the model mesh; its consumer is the open read

- The range carries no `C`: the 550-placeable kind-`0x0B` chunk was searched
  byte- and field-level against the nine referrer entities and seven `C`
  assets, 0 hits. The brush route is dead for this map.
- The range's models carry a **prebuilt collision mesh**: 161 of 229 models,
  272 `STU_537E56E4` instances inside 279 holders of class `0x2DD96D6D`,
  reached as `STUModel 0x871BD3D0 -> 0xCB4D298D (+0x118) -> 0x2DD96D6D ->
  0xB7C8314A -> STU_537E56E4`. `GeometryBuild` does not read it; the consumer
  is the missing read, and the second geometry class `0x142132f20` is the
  candidate.
- The chunk's consumer is not RVA-dispatch reachable: 277 tables holding code
  RVAs, 0 hits for `0x20a02f6`; the control `0x208edf6` hits table
  `0x142090988`, validating the method.
- New doc: `docs/formats/static-collision.md`; install test pins the mesh
  counts (2 tests).

## 2026-11-02 - C is a fracturable descriptor; no model carries it

- All 32,278 readable model assets were scanned for class `0x564B1018`: zero.
  `C` lives in entity assets (GUID type `0x112`, chunks `FRAC` + `FSTU`),
  reached from entity component `0x09CB5A77` field `0xA7ED6024`; nine entity
  assets name seven `C` assets, each with four instance points.
- The range's 229 models, 79 placed assets and 34 chunks carry none, so the
  brush route is the **fracturable** path, not the map's static collision. A
  real brush was still pinned from a `C` asset (4 points -> 4 vertices, 2
  triangles) conditional on the `max(1.0, [W+0xe0])` scale floor, whose writer
  is not read.
- Static collision is open again with three leads: the range's 550 entity
  references vs the seven `C` assets; `STU_537E56E4` (a prebuilt mesh: 7 nodes,
  8 points, 16 triangles) in static models' `MSTU`; and the collision chunk's
  36-sector mesh, whose consumer search should be re-run with the RVA-dispatch
  method.

## 2026-11-02 - C named by type identity

- The release path `0x141b251d0` is generic (vtable slot `+0x18`, flags, deferred
  destruction), so the class cannot come from there; type identity did:
  **`C` = registered class `0x564B1018`, size `0x198`, vtable `0x1431dd118`**
  (registration `0x1402db070` passes `r8d = 0x564B1018` and `size 0x198`; 22 of
  36 field hashes are immediates in the client's dispatch
  `0x141334ea0`/`0x141335e60`; OWLib used only as a check).
- `C+0x98` is a 16-byte array header over the inline sub-object's `Vec3A[]`,
  `C+0xa0` the encoding tag (`1` = `{points,count}`); implemented in
  `ow_tank::model_collision` (5 tests). `+0x48` is an asset reference
  (`teStructuredDataAssetRef<STU_564B1018>`, declared at `+0x08` of
  `STU_09CB5A77`).
- Correction: the node-type-2 update is one function at `0x14204cb40` with one
  static caller, `0x14204824a`.
- Next: the model `MSTU`'s `InstanceInfo` of type `0x564B1018` -> the real brush.

## 2026-11-02 - the chain to C ends in a measured negative

- Three probes for `[node+0xe8]` (the pending object moved into `[node+0x38]`)
  are negative: the `{+0x28,+0x29,+0xe8}` fingerprint hits only a hash-table
  relocator; "calls the GUID resolver" ∩ "stores `[reg+0xe8]`" is empty (742
  sites in 612 functions against 1 004 resolver callers); and no function in
  the node-type module writes `+0xe8` at all. `[object+0x40]` and
  `[instance+0x48]` are only read/cleared/compared wherever found.
- Correction: the `0x14469xxxx` records are `.pdata` copies, not a dispatch
  table, so the earlier "`0x14208edf6` is reachable through an RVA table" is
  withdrawn. The only computed jump is a fragment switch at `0x14208e74e`.
- Census line closed. Two different approaches remain: the node's loader
  (vtable/callback), and a data-side search in the model asset for `C`'s
  `+0x98` record.

## 2026-11-02 - the chain to C walked backwards; the RVA-table blind spot

- The geometry is created by the node-type factory `FUN_1420d0ef0` (two classes
  on `[node+0x29]`); `geometry+0x80` is written by the class's vtable slot
  `+0x08` (`0x142138ce0`); the node-type-2 update
  (`0x14204cbce..0x14204cfc9`) moves `[node+0xe8]` into `[node+0x38]` and reads
  the same three links `GeometryBuild` does. The producers of `[object+0x40]`
  and `[instance+0x48]` are outside that function: next anchor is `[node+0xe8]`.
- Correction: `0x14208edf6` does have a static route — a 4-byte RVA
  computed-jump table at `0x142090988`, indexed at `0x14208e74e`. Reachability
  claims must test RVA tables; the earlier "unreachable" notes are marked as
  not relied on in `formats/maps.md`.

## 2026-11-02 - the brush -> mesh conversion is bit-exact on all four fixtures

- The five-point arrow's divergence was not the tag test: ops 1, 2 and 5 all
  read the `+0x28` link and the port had 2 and 5 on `+0x30`. Single-stepping the
  client's tag pass and hooking the tree calls located it; the fix is two lines
  and the arrow is now asserted against the client's own faces. All four pinned
  meshes match, vertices and half-edge marks included (25 brush tests).
- `C`'s class: the disp8-aware census is a structural negative. No function
  stores both `+0x186` and `+0x98`, and `0x14208edf6` (a near-perfect
  `GeometryCopyParams` field-set writer) has no static reference at all, so its
  "builds C" reading is downgraded. A displacement is not a type; the next
  anchor is the producer of `[instance+0x48]` read backwards from
  `GeometryBuild`'s chain.

## 2026-11-02 - the brush -> mesh walk read and implemented

- The status tree (insert fixup, erase + rebalance, both rotations, the
  three-way insert key) read from the instruction stream: the comparator is
  `FUN_142ced380` behind the `0x142ced450` thunk (y descending, x ascending,
  128-bit rationals); `0x142cecb10` is a no-`.pdata` leaf decoded linearly; the
  insert fixup's colour registers were inverted by the decompiler and are now
  right.
- The ear predicate read from the `SHUFPS` immediates; the clipping tail's
  three repair cases and the last-three fan read; the erase's four cases read.
- `ow_tank::brush_convert` lands ops 0-5, the emission and the face writer;
  `dev/brush_mesh_oracle.py` runs `0x142cf23c0` end to end. Three of the four
  pinned meshes reproduce bit for bit; the five-point arrow diverges and is
  recorded open (client ear list empty after the tag pass, port tags three
  ears — a `< 0` vs `<= 0` on a collinear reflex tip).
- Faults refused, not approximated: clockwise ring, two-node ring, two-ring
  allocation, the `+0x58 == 0` variant.
- 24 brush tests; gate green.

## 2026-11-02 - the shape pools enumerated; the chunk is not the shape source

- World pool table read (`DmWorldConstruct 0x142cbeb70`: eight records at
  `+0x288 + 0x18*i`; shape entries index 2 -> `+0x2b8`, element `0xc0`), with
  the constructor, clear and destructor (`0x142cc04f0`) as the only field
  writers (verified store census, 119,802 function groups, `-1` free-head
  sentinel as the discriminator).
- Pool contents can only be created through `ShapeAddToBody`/`ShapeAddToOwner`
  -> `ShapeEntryFromDescriptor`; every caller that appends collision material
  is a brush-mesh producer, and `BrushTreeToMesh`'s four callers are the four
  brush appenders. The collision chunk is therefore not the shape source; only
  an inline appender to `geometry+0x114`/`+0x124` remains possible.
- Corrections folded in: the brush ingest appends a `0x100` entry to
  `geometry+0x118` (not a `0x60` one), the `0x100` appender has two callers,
  `ShapeAddToBody` sites are 12 groups (not 13), the destructor group starts at
  `0x142cc04f0`, and `BuildBvh`'s callers are split wrappers, all unreachable.
- New doc: `docs/formats/shape-ingest.md`.

## 2026-11-02 - the chunk kinds are read: 0x11 collision, 0x12 pathing

- Kind table `0x1439aa320` plus its name decoder `0x1404c19b0` read (length =
  low 24 bits of the dword at `name-4`, key = eight bytes at `name-0xC`, byte
  `i` XORed with `key[i & 7]`). All 36 names decode; the eight placeable names
  the project already had come out exactly.
- Result: kind `0x11` = `MAP_CHUNK_COLLISION`, `0x12` = `MAP_CHUNK_PATHING`,
  `0x10` additive collision, `0x23` additive pathing. The per-slot dispatch in
  `FinishMapChunkAsset` agrees. So the 220-cell `STU_9199BF6B` container is the
  **pathing** chunk; the earlier label was inverted.
- Collision chunk `0x0DD0001100000688` (21,858,688 B) container read and
  implemented (`ow-tank::chunkkind`, 7 tests): counts 36/1, `0x58`/`0x38`
  arrays, `FUN_142e492a0` fixup, four-run per-sector tiling pinned for all 36.
- The collision consumer is still unread: who fills the Domino shape-entry
  pool at `[record+0x2b8]`. Correction: `BuildBvh` has five call sites in three
  functions, so it is not callerless.
- New doc: `docs/formats/chunk-kinds.md`.

## 2026-11-02 - the brush route read and implemented; the chunk labels are unread

- Node type 2's build chain read: `GeometryBuild` (`0x14213c440`) resolves the
  model's collision descriptor `C` and builds the brush at runtime; the four
  "leaf builders" are a header init, a mesh-struct init, the ring builder
  (`0x142ced620`) and the edge list, with `0x142ced510` decoding vertices.
- `ow_tank::brush` lands the record formats, the vertex decode and the ring
  builder (11 units). Every arithmetic expectation is the client's own output:
  `dev/brush_vertex_oracle.py` runs `0x142ced510` and `0x142ced620` under the
  emulator (allocator-hook stub) and the tests pin those bits.
- Measured: **no** GUID-type-`0x00A` asset in either manifest this install
  opens (0 of 1,425,794 records; 0 of 1,514,352 with `WinPrism_` forced), and
  300 model assets carry no brush container. Pinned by `brush_real.rs`.
- The chunk-kind ↔ name mapping is **not read**: the `0x1439aa320` name strings
  are obfuscated and undecoded, so "kind `0x12` = collision" is an assumption.
  The kind-`0x11` chunk (21.8 MB) is geometry-shaped (RVA arrays into float
  triples), the kind-`0x12` chunk (8.79 MB) is the 220-cell container.
- New doc: `docs/formats/brush-collision.md`.

## 2026-11-02 - the three follow-up readers: Domino-side, unreachable

- Read the three anchors named after the corrected census. `0x1420c63b0`: a
  `0x38`-stride node/transform record array at `[owner+0xd0]`, a `0x20`-stride
  transform-handle array at `[owner+0xe0]`, output into the caller's records,
  ends in `OwnerCreateBody` + four `ShapeAddToBody`. `0x140c2813f`: mesh source
  from a brush collider (`ColliderMeshEmit`). `0x142078562`: a box computed
  from a stored mesh source's bounds into a `0x58`-stride record.
  `0x1420a02f6`: a mesh-source assembler over the owner's `+0x140`/`+0x148`
  descriptor container (`0x60`/`0x40`-stride arrays, pair unpack
  `0x141b53b70`). `0x142cdf9f2`: appends `0x40`-byte entries to the Domino
  owner's body vector and calls the handle setter `0x142cb4f20`.
- Reachability read for all five: no `E8` (but `0x1420c63b0`'s own recursion),
  no `E9`, no immediate, no 8-byte pointer, no obfuscated-pointer encoding. The
  dword-RVA hits are the image's `.pdata`-copy/code-range tables — every
  function start carries one, `ShapeAddToBody` included — so they are not
  invocation routes.
- Result: the chunk path is neither opened nor closed by these; the single next
  reader is the producer of the owner's mesh-descriptor container at
  `+0x140`/`+0x148`. Details in `docs/formats/maps.md`, "The three next
  readers: Domino-side, and unreachable". New probe `dev/data/read/refs.py`
  (reachability in six spellings).

## 2026-11-02 - the collision cell census, corrected

- The earlier entry here ("the cell path has no reader in the image") was
  **wrong** and is retracted. It rested on Ghidra's caller metadata, which this
  project already documents as incomplete. Measured: Ghidra reports 3 callers
  for `ShapeAddToBody` (`0x142cb3940`) where a verified `E8` scan finds 27
  sites in 13 functions, and `0x14208d010`/`0x142090e00` do not call any shape
  builder (their callees are `0x142015510`, `0x142083d50`, `__chkstk`).
- New census tooling in `dev/data/read/`: `census.py` (union of Ghidra callers
  and a verified direct scan, with per-site decode verification and `.pdata`
  function mapping) and `profile.py`/`digest.py` (per-function reads, calls and
  builder windows).
- Corrected census: 27 + 17 + 1 + 2 + 5 verified sites over the five add/build
  handles, 28 distinct producing functions, each classified; none calls
  `StuLoadSubAssets`, none loads the chunk work record's payload at `+0x60`,
  none reads the component's `{pointer, mode}` headers. Whole-copy route
  checked: one `memcpy(…, 0x78)` in the image (`0x142402824`), a diagnostic
  string fragment.
- The state is therefore **open**, not "absent". The container work from the
  previous entry (three tiled arrays, `ow_tank::collision::tables`, unit and
  install tests) is unaffected and stands.

## 2026-11-02 - the collision cell path has no reader in the image (RETRACTED)

**Withdrawn in full.** The two bullets below drew a conclusion from Ghidra's
caller metadata; the correction entry above supersedes them. Kept only so the
record shows what was claimed and why it failed.

- Read the consumer end of the collision chunk and found none:
  `FinishMapChunkAsset` (`0x141ff54e0`) hands the payload to the chunk work
  record through `FUN_141fe37a0` (`work+0x60`), `StuLoadSubAssets`
  (`0x1421d26d0`) returns the first sub-record's instance (hence `+0x68` for
  slot `0x1e` and `+0x08` for slot `0x22`), the component descriptor
  `0x143c3f060` is referenced only by registry plumbing, its vtable
  `0x14314acc8` holds type stubs plus the destructor `0x140f8f310`, and the
  `STUD` magic is compared in exactly one function image-wide, the loader.
- Searched twice image-wide for a reader (one base reading `+0x10`/`+0x58`/
  `+0x68`; a `0x60` element stride): no hit in a map, mover, geometry or
  collision module. [Retracted: the searches stand as attempts, the conclusion
  "open by absence" does not.]
- Replaced the guesswork on the component's shape with a proof: the three
  array fields tile the blob with no gap (`0x98` 222 x `0x28`; `0x2358`
  6 x `0x20`; `0x2428` 220 x `0x10` to the first cell `0x31F8`), so the
  strides follow by exact division. `ow_tank::collision::tables`, two unit
  tests, and an install test on the practice payload pin it.
- `ow-tank-look --hex` gained `--at`, so a deep payload offset can be dumped
  without a throwaway program.

## 2026-11-02 - the writer's composition, read

- `0x140cb86c0` read end to end: the seed/speed endpoint pair (`A = point +
  seed*dir`, `B = point - speed*dir`, two different scalars), the `2 *
  [shape+0x2d2c]` seed floor, pass 0's cached query and its shape-set pair,
  both ring branches (direction fan and shape fan), the pentagon refine, the
  back-transform pass and the output write, all tabulated in
  `docs/formats/character-controller.md`.
- New read units: `fan::ring_sweep` (count and the `0.35` radius floor),
  `fan::ring_offset` (`(sin*R, 0, cos*R, 0)`) and `fan::shape_ring_offset`
  (`(sin*(seed-0.015), 0.015, cos*(seed-0.015), 0)`). 114 `ow-sim` units green.
- **Superseded the same day:** `fan::solve` was written and tested (the entry
  below "`fan::solve`, the writer's decision in code"), and the five helpers
  named here were read in that pass (`FUN_1426cd3d0` = `cosf`, `0x140c9f110`
  returns a zero quad, `0x141ae3eb0` is the shape's box corner + extent,
  `0x140cc2290` the mesh-face refinement). The shape-ring branch and that
  refinement stay behind the `CandidateSolver` seam.

## 2026-11-02 - FinishMapChunkAsset repaired; two consumer hypotheses ruled out

- `FinishMapChunkAsset` (`0x141ff54e0`) was stored as 12 bytes; after the
  false-`noreturn` repair it is 964 bytes and decompiles end to end, confirming
  the slot `0x1e`/`0x22` cell dispatch and adding its tail (upload accounting
  `FUN_141fe37a0`, return `0x11`) and the full case list.
- Ruled out as the cell consumer: the geometry object (brush-only, proven by the
  writer and caller counts) and `FUN_1420c63b0` (10 500 bytes, four
  `ShapeAddToBody` sites, but no code caller and no plain pointer — its three
  data xrefs are internal switch tables whose neighbours point inside itself).

## 2026-11-02 - the brush ingest, read end to end; Ghidra's false noreturns

- `AppendCollisionEntryFromBrushSource` (`0x14213a510`, 2011 bytes) read fully:
  brush snapshot -> `BrushTreeToMesh` -> leaf/chord-scale area test -> one `0x60`
  entry (`MeshToColliderFaces` into `entry+0x20`, per-face stamp writes, handle
  to `geometry+0xf8`).
- Geometry object layout recovered from the constructor and two destructors:
  source array at `+0xb0` (stride `0xb0`, count `+0xc0`), five
  `{data, u32 capacity, u32 count}` vector headers at `+0xf8`, `+0x108`,
  `+0x118`, `+0x128`, `+0x138`.
- Negative result: the entry count `+0x114` is written in exactly three places
  image-wide (the brush ingest, a clear, the build's reset), and the `+0x118`
  vector's append helper (`0x1421360a0`) builds `0x100`-byte elements with the
  brush ingest as its only caller. **This node class is brush-only**; the chunk's
  cells must be built by a different class.
- Ghidra defect fixed the mechanical way: three callees were falsely `noreturn`
  (`0x141bb00e0` allocator, `0x142ced8f0` throw, `0x142efe2b0` memcpy), which had
  truncated every allocating function; a dry-run `clear_flow_and_repair` lists
  them. Recipe in `docs/ghidra-project.md`.
- New tool `dev/find_field_writes.py`: whole-image load/store scan by struct
  displacement (what the field-writer question actually needs).

## 2026-11-02 - cells to shapes: the consumer's contract, and one hypothesis killed

- Read `GeometryIngestCandidateB` (`0x14213528a`): the decoder's output form is a
  `0x30`-byte mesh source of **three `{pointer,count}` pairs** over arrays of
  **`0x60`-byte records**, one body per entry, one type-3 shape per record, the
  nearest connect point picked by `FUN_142cb9a50`, then `FUN_142cb34f0`.
- Measured all 17 arrays over the 220 practice-range cells: none is a fixed
  0x60/0x30/0x20-stride view; all are 4-byte granular. The records are built,
  not reinterpreted. Numbers recorded in `formats/maps.md`; the next anchor is
  the builder.

## 2026-11-02 - `fan::solve`, the writer's decision in code

- `ow-sim::fan::solve(body, ctx, solver)` now runs the composition that was
  read: the `body+0x659` gate, both endpoints (`point + seed*dir`,
  `point - speed*dir`), `cos(turn_limit)` as the facing threshold, the ring →
  refine → back-transform order, and the two reflection bytes (`|=` then
  assignment). The five helpers behind it are read and recorded:
  `0x1426cd3d0` is `cosf`, `0x140ca7400` is the cone angle in degrees clamped
  to `[0, 1.5707844]`, `0x140c9f110` returns a zero quad (the extrapolation is
  zero), `0x141ae3eb0` is the shape's box corner+extent (not a basis), and
  `0x140cc2290` is the mesh-face refinement. 123 `ow-sim` units.
- The evaluator, pass 0's cache, the shape-ring branch and the mesh-face pass
  stay behind `CandidateSolver` / `SolveContext`: `solve` never invents a score.

## 2026-11-02 - the ring pass's rule, and where the fan read stops

- `ow-sim::fan::accepts_ring(alignment, threshold, gate)`: site `0x140cb9b6e`'s
  acceptance is `gate && alignment > threshold`, strict, with the gate fed
  from the hit record's flags byte. All four of the writer's call sites now
  have their acceptance rule read and unit-tested (25 fan units).
- The fan read stops here deliberately: the **composition** — which site runs
  under which input, the ring pass's carried-forward endpoint, and the
  `+0x1140` write — is not read, so `fan::solve` stays unwritten and
  `CandidateSolver` stays a seam. `STATUS-handoff.md` carries the blocker with
  its next anchors.

## 2026-11-02 - a wrong citation found and fixed

- `facing`'s address was recorded as `0x140cb92a0`; disassembling that range
  shows the radius-scale and the direction splats (`MULSS [rdi+0x5b8]`,
  `MOVUPS [rdi+0xc0]`), not the dot. The arithmetic is from the refinement's
  comparison at **site `0x140cb9770`**; the address attribution in `fan.rs`,
  the plan and this log is corrected. The code was never wrong, the citation
  was.

## 2026-11-02 - the fan's refinement is read and implemented

- `ow-sim::fan` gains the refinement: `horizontal_unit`, `back_direction`,
  `refine_direction` (the `-normal` sibling with its fallback),
  `refined_offset` (`pentagon*0.02 + refined*0.005`), `refined_sweep` (the
  `0.04` ground probe), `accepts_refine` (`flag_bit_0x10000 || facing >
  best_y`, strict, `best_y` captured before the loop) and `refine_pentagon`
  (keep-first, replace-on-facing, break-on-facing, five candidates).
- The back-transform is no longer "not read": the direction is `-xyz`
  normalized in the horizontal plane with `y` kept as `n*0.5*0.0`, and the
  degeneracy guard is `1.17549435e-36`.
- Six new units; the fan now has 24. `fan::solve` stays unwritten: the outer
  walk over the writer's four evaluator call sites is the remaining read.

## 2026-11-02 - the producer seam lands, the loop does not

- `ow-sim::fan` gains `CandidateSolver` (`evaluate(a, b) -> Option<SweepHit>`),
  `SweepHit { score, normal }`, and `facing(dir, normal)` — the refinement's
  second acceptance term (site `0x140cb9770`), `y`, `x`, `z` summed left to
  right.
- `policy::PolicySolver` implements the trait over `candidate_hit_policy`, so
  the whole chain runs end to end against a synthetic world. Three new tests.
- The producer's loop is **not** written: each candidate is wrapped in a
  per-candidate refinement block that is only partly read. It is queued as
  `docs/open-questions.md` 1a, with `fan::solve` blocked behind it.

## 2026-11-02 - the hit policy is implemented and tested

- `crates/ow-sim/src/policy.rs` lands: the callback `0x14202bc90` as
  `candidate_hit_policy(hit, packed, ctx, world)`, with `PackedShape`,
  `HitRecord`, `HitFilter`, `Hit`, `World` and the `Geometry`/`VariantLookup`
  traits that supply the map data the policy reads. Twelve unit tests, gate
  green; the harness unit waits on a seeded geometry object.
- The key chain is reproduced, not simplified: `decode_geometry_pointer`
  (`0x14202beba`, constants pinned by a round-trip test) and the entry decode
  in `geometry_key`, including the anti-tamper globals as `key_mix`/`key_salt`.
- The **first** call site's presence binding is `(word & 0x0f) >> 4`, always
  zero, so the dedupe scan over `[world+0x240]` is unreachable in this build.
  `dead_presence_term` keeps the formula and a test asserts it over all 65536
  slot words; the live binding takes the presence bit from `0x10`.
- One documented non-identical value: with no world the client leaves a stale
  register in `rec+0x20`; the Rust record writes `0` there.

## 2026-11-02 - the collision answer is read end to end, the hit policy stays open

- `CollisionHandleDispatch` (`0x142cc45a0`) re-read at the instruction level: it
  bounds the handle at `0x3fff`, decodes the record with the same
  `(raw + 0xb3269b88d44d5fb2) ^ 0x77600ba5adcc265c` as the evaluator wrapper,
  builds a five-field driver context (world, `request+0x58`, the callback,
  `request+0x60`'s answer record, the observation slice) and calls
  `CollisionQueryDriver`. The branch on `request+0x71` appends to a
  **0xaa0-byte observation ring** at `record+0x4a0`; `0x142cadf00` is that
  slice's inline-vector release, not a query helper (two earlier notes
  corrected).
- `CollisionQueryDriver` (`0x142cbd510`) read: four passes on `request+0x5e`'s
  bits, the sweep AABB and its per-lane expansion, the three per-entry masks
  (swept AABB, slab test with `0.005` slack, `+0x2c == -1` leaf test), the
  `0x100`-entry worklist (`CollisionWorklistPush`), and the leaf call into
  `ShapeNarrowphase`.
- `ShapeNarrowphase` (`0x142cc1610`) read: it is the only caller of the policy
  and therefore fixes the contract — `callback(&hit, packedShapeDescriptor,
  answerRecord)`. The thunk `0x14202c580` is byte-exact
  (`MOV RAX,R8 / MOV R8,RDX / MOV RDX,RCX / MOV RCX,RAX / JMP`), so the policy
  body receives `(hit, packed, record)`. The earlier "first parameter is the
  descriptor the dispatcher passed third" note is superseded.
- The policy body `0x14202bc90` (1797 bytes) is **read**. The earlier
  "computed-jump dispatcher" reading was wrong: `0x14202bd06`'s `ADD`/`ROR`
  chain is a **pointer decode**, and the far branch targets
  (`0x142049e89`, `0x14202df4a`, `0x14202e840`, `0x142030dc2`) are junk-filler
  obfuscation between blocks. A linear sweep decodes the body to exactly its
  end (427 instructions, no data islands, no indirect jumps), so the live path
  is plain code. Three obfuscation idioms resolved: junk-between-blocks, a
  constant-injection stub (`0x142018990` stores `0x10`/`0xf`/`4`/`0` through
  its four pointer arguments), and pointer/value decode chains.
- Contract: `(hit, packed, ctx)` -> 1 no hit / 2 accepted / 3 rejected. Gates:
  `world = 0x1420c2360(ctx)`; `0x1420af950` (62 B) rejects unless the hit's
  required mask meets `world[0x280]`, its forbidden mask does not, and the
  world is absent from its exclusion list; a decoded geometry key dedupes
  against `[world+0x240]` x `[world+0x248]` 0x10-byte entries. Mode switch on
  `hit+0x34`: 0 = presence (`hit+0x39 = 1`, return 1); 1 = fill the 0x50-byte
  record at `[hit+0x10]` and return 2; >= 2 = build a local record and
  classify through `hit[0](&record, [hit+0x08]+0x470)`. Outputs: `hit+0x30`
  (the score the fan compares), `hit+0x38`, `hit+0x39`.
- `dev/probe_policy_blocks.py` (new) decodes a function from the recovered
  image, distinguishes a computed jump from junk filler, and reports
  unresolved/outside targets. Note for anyone re-reading this area: code bytes
  come from `dev/data/offline_recover/module_offline.bin` (flat, offset ==
  VA - base); the install's `.text` is encrypted.
- Stage is **read**; what remains for it is the map-data geometry, not code.
  No gameplay code was written for it yet.
- Docs: `formats/character-controller.md` gains "The collision query driver
  and the hit contract"; `movement-plan.md` stage rows updated;
  `open-questions.md` item 1 narrowed to the policy body.

## 2026-11-02 - collider shell corrected: the live path is the brush tree

- `MeshToColliderFaces` (`0x142cf2010`) re-read with the decompiler: the
  thickening is **z only** (x/y/w pass through `+-0.0`) and the index array is
  ordered outward-block, inward-block, then the stitching pairs. Two real
  divergences in `crates/ow-sim/src/collider.rs::shell` fixed, plus the surface
  id word it was not writing; its tests now assert the read behaviour.
  Differential unit still pending (**not tested**).
- `GeometryBuild` (`0x14213c440`), `ColliderFaceAppend` (`0x142cca460`) and
  `ColliderMeshEmit` (`0x142ccc120`) read. `GeometryBuild` is the only cell
  producer and it builds from the model's brush tree, so the map collider comes
  from brush trees, not from the 17-array chunk (see
  `docs/open-questions.md` item 6).
- Ghidra bridge endpoints confirmed reachable (`/get_xrefs_to`,
  `/force_decompile`, `/disassemble_bytes`) with program name `Overwatch.exe`.
- Gate: `cargo xtask verify --fast` green (`cargo test -p ow-sim --lib`: 90
  passed).

## 2026-11-02 - collider triangle skip test verified, shell builder read

- `fn/TriangleDegenerate` (`0x142cd56e0`) implemented in
  `crates/ow-sim/src/collide.rs` and verified: differential over 30 declared
  cases (bit-exact), 100% branch coverage, mutation kill rate 100% (9/9
  probed of 12 generated). Its boundary
  cases sit exactly on `1e6`, `6.24999984e-06` and `2.49999994e-05`.
- `MeshToColliderFaces` (`0x142cf2010`) read in full and recorded in
  `dev/data/read/decoder-notes.md`: it builds a double-sided shell (vertices
  offset by +-epsilon, per face two triangles with opposite winding, plus side
  walls in the flag mode). Not implemented.
- Gate: `cargo xtask verify --fast` green.

## 2026-11-02 - Collision: the degenerate-triangle skip, verified

- `crates/ow-sim/src/collide.rs` gains `triangle_degenerate`: the read
  expression of the client's per-triangle skip test (`0x142cd56e0`) — the
  non-finite corner test on the low three lanes, the three scalar box-extent
  compares against `1e6`, the three squared-edge compares against
  `6.24999984e-06`, and the cross-length compare against `2.49999994e-05`, in
  that order, with `AL` 0 or 1 as the only output.
- `fn/TriangleDegenerate` verifies it differentially through
  `crates/ow-sim/src/bin/verify_probe.rs` and
  `dev/verify_harness/triangle_fixture.py`: 96 random cases plus a declared
  pool that sits on both sides of every threshold and on each threshold
  exactly, coverage 100% of its 6 branch sites, 12/12 mutants killed.
- The collider-mesh chain upstream of the query is now read end to end and
  named in `docs/function-names.csv`: `MeshToColliderFaces` (`0x142cf2010`),
  `ColliderMeshEmit` (`0x142ccc120`), `ColliderFaceAppend` (`0x142cca460`),
  `BrushTreeToMesh` (`0x142cf23c0`). None of those four is implemented yet.
- The pool records that feed the cell/sector builders come from brush/model
  primitives via the creator at `0x1421363d1`, not from the 17-array chunk
  container; that reframing is recorded in `docs/open-questions.md` item 6,
  "Round 2".

## 2026-11-02 - Collision: the type-3 mesh narrowphase, verified

- `crates/ow-sim/src/collide.rs` gains `mesh_sweep` + `mesh_triangle`: the read
  expression of the type-3 BVH walk (`0x142caed30`) and the per-triangle test
  (`0x142cce430`'s fast path), including the `0xFF` index-run grouping with its
  single-corner carry, the last-accepted-triangle rule and the published
  0x44-byte hit. The walk's counters (`internals`/`leaves` at `+0x3c`/`+0x40`)
  are kept because they are part of the output the query returns.
- `fn/MeshSweep` verifies it differentially at `0x142caed30` through
  `crates/ow-sim/src/bin/verify_probe.rs` and
  `dev/verify_harness/mesh_fixture.py`: 328 cases over 16 fixture shapes and
  the edge pool, byte-for-byte on the whole 0x44-byte output, coverage 61.1%
  with the unread support-point path declared dead, 10/12 mutants.
- The walk's input is pinned. A 0x60 cell entry holds a **0x30-byte mesh
  source** at `+0x20` (`{u64 nodes; u64 second; u64 third; u32 count; 5
  floats}`); the brush path fills it with `MeshToColliderFaces`
  (`0x142cf2010`), which has only two callers, both geometry builds. The
  chunk decoder must fill the same struct from the cell arrays.
- New read tools (git-ignored, `dev/data/read/`): `d.py` (disassemble an image
  range), `callers.py` (scan `E8 rel32` for direct callers), `covhunt.py`
  (list a unit's uncovered branch sites).
- Open: the support-point path (`0x142cc9460`), the BVH builder, and the cell
  decoder itself.

## 2026-11-02 - Collision: the cell container and the runtime fixup

- `ow_tank::collision::parse` reads the collision chunk's STUD blob, the
  `0x9199BF6B` component, the `m_cellBytes` table and all 220 cells with their
  17 arrays. The install test `crates/ow-tank/tests/collision_cells_real.rs`
  pins the tiling, the first/last records and every array extent.
- `ow_tank::collision::fixup_cell_bytes` is `MapChunkCellBytesFixup`
  (`0x141ff51a0`) and is verified by `fn/MapChunkCellBytesFixup`: 264
  iterations plus the edge pool, 12/12 mutants, outer modes 1 and 2/3 and entry
  modes 1 and 2/3. The two mode-0 allocation branches are declared dead.
- The v1 `STUD` magic is the u32 `0x53545544` the loader compares; on disk that
  is `DUTS`. `ow-stu`'s `MAGIC_V1` and its synthetic builder are corrected.
- The node-type registry is enumerated: `0x141c5ff40` is a linked-list add to
  `0x144188170` with 144 `{u16 type; factory}` registrations, types `1..=0xb4`.
- Open: the cell-to-shape decoder. Candidates and dead ends are in
  `docs/open-questions.md` item 6, "Search map for the decoder".

## 2026-11-01 - Collision: world, pools, query and primitives

- World create is `0x142cc3c80`; a world is a `0x4d0` header plus eight
  strided pools. `ShapePoolAlloc`, `ShapeEntryFromDescriptor`,
  `ShapeAddToBody` and `ShapeAddToOwner` are read. The descriptor producer is
  still open.
- The collision chunk payload is decoded as data: two STUD v1 blobs, the
  `0x9199BF6B` component, a per-sector table and 220 cells whose 17 arrays
  tile each record.
- The query chain is read to the narrowphase: `0x1420328c0` -> `0x14203dae0`
  -> `0x142cc45a0` -> `0x142cbd510` (four-pass BVH) -> `0x142cd5540` +
  `0x142cc1610` -> five per-type primitives.
- Verified: `fn/CollisionSweepBounds`, `fn/CollisionSphere`,
  `fn/CollisionShapeFilter`, `fn/CollisionCapsule`, and the type-0/1 analytic
  paths. The support-point path and types 2, 3 and 4+ remain open.
- The 0x100 sector entry is read (body config, 0x40-stride matrix array,
  stolen u16 array, transform flag); the writer of `geometry+0x118`/`+0x124`
  is the next anchor.

## 2026-11-01 - Movement: the live velocity law

- Both live update paths compose `state_transform(wish) + ramp_vector(body)`
  and hand it to `SetCharacterControllerVelocity`; implemented as
  `ow-sim::frame`. The ramp and state-vector leaves are verified.
- The wish source is a closed fan over collision-scored candidates, not a
  button-to-direction conversion. The fan arithmetic is read and implemented
  in `ow-sim::fan`; its candidate evaluator is the collision query above.
- Verified units: `fn/MoverRampVector`, `fn/MoverRampScalar`,
  `fn/MoverRampScalarOffset`, `fn/BuildMoverPhysicsCommand`,
  `fn/MoverWishSpring`, `fn/MoverWishForward`, `fn/MoverWishInverse`,
  `fn/MoverWishCopy`, `fn/StateTransform`, `fn/MoverStateVector`,
  `fn/DefaultMoverSettings`, `fn/ApplyMoverSettings`,
  `fn/ApplyStateModifiers`, `fn/MoverGate`, `fn/CharacterPosition`,
  `fn/RigidPosition`, `fn/VariantPosition`, `fn/WorldPosition`,
  `fn/MoveCharacterController`, `fn/ApplyCharacterControllerMove`,
  `fn/SetCharacterControllerVelocity`, the four shape-bound leaves,
  `fn/ShapeDispatch` and `fn/MoveCharacterControllerShape`.
- Open: the wish producer's evaluator, the velocity-to-position stage, and the
  map-chunk-to-shape ingest. `MoverInvoker` is likely dead but not proven.

## 2026-11-01 - Tooling: clean Ghidra map and the harness

- The clean Ghidra map has 120,028 function symbols and 6,676,292
  instructions; `.pdata` starts are the authority for function entries.
- `Alloc` (`0x141baff30`) is no longer marked noreturn; truncated callers
  decompile in full.
- The movement corpus has 1,544 `.pdata` bodies with linear disassembly and
  694 decompiles. The harness gained `startRegs`, MS x64 stack arguments,
  range reads, `vec4` returns and state snapshots.

## 2026-10-31 - Renderer and replay

- D3D11 is the production path; D1-D5 are implemented in `ow-render` (device
  and swap chain, targets, frame order, the 64-opcode replay dispatch and the
  clear/draw/viewport handlers).

## 2026-10-30 - CASC: CDN index, page cache and encoding lookup

- The CDN index footer/page layout, the archive page cache and the encoding
  page lookup are read; the index parser is install-tested.
- Verified: `fn/CdnIndexFooterParse`, `fn/EncodingHeaderParse`,
  `fn/EncodingFindCKey`, `fn/EncodingFindEKey`.

## 2026-10-29 - Texture chain and exact uploads

- The format switch, descriptor builder and descriptor size walk are verified
  (`fn/TextureFormatFamilyBpp`, `fn/TextureDescriptor`,
  `fn/TextureDescriptorSizeWalk`).
- BC6H, RGBA16F and R16_FLOAT keep exact half words; R16_UNORM keeps its exact
  `u16` words. UINT, BC CPU decode, the sampler and the live GPU readback stay
  open.

## 2026-10-28 - CASC: BLTE and index writes

- The BLTE frame header, block reader, key-state intervals, page table and
  index entry search are read. `ow-casc` follows the client's validation order
  and status split.

## 2026-10-27 - Bundle, map and manifest units

- `fn/ChunkGuid`, `fn/BundleLookup`, `fn/BundleDirectoryLookup`,
  `fn/CmfKey154504` and `fn/CmfIv154504` are differential units.
- The client has no mode-to-variation selector; the map arrives as type-`0x002`
  GUIDs in the level-load message. The practice-range launch is a viewer
  default.
- The one client launches the practice range: 34/34 chunks, 1,358 placeables,
  229 models, 550 entities, bit-exact bounds.

## 2026-10-26 - Manifest, CASC oracle and the rebuilt gate

- The build-154504 manifest decrypts to 1,301,598 GUID-sorted records; the key
  and IV are derived from the installed client's own table.
- Eight CASC leaf units are verified; the client's zero-length and
  trailing-twelve-byte index-hash defects were fixed.
- The runtime crates were parked and the workspace reduced to the data layer.
  The practice-range chunk seed and the synthetic map claim were removed;
  `cargo xtask verify` became the one gate.
- `fn/BuildBvh`: the collider BVH builder (`0x142ccb1b0`) is read and
  verified. The split rule turned out to be a 64-bin surface-area heuristic on
  the centroid spread, not the array-half split the earlier note recorded, and
  the partition is a swap pass rather than a stable one. Differential, 100%
  branch coverage, mutation 10/11. `crates/ow-sim/src/collider.rs` now holds
  the shell, the face records, the builder and the flattening in one chain.
