# OWLib comparison (archived 2026-11-12)

A one-time side-by-side of `overwatch-client-rs` and the OWLib tree, written
against commit `ce9dacd` and never linked from the docs index. Kept verbatim
as the record of that comparison; the live state is `docs/systems-coverage.md`
and the open items are `docs/open-questions.md`.

Companion to `docs/systems-coverage.md`, `docs/deviations.md` and
`docs/open-questions.md`. Written from a read of both trees (file lists,
source, tests). Scope of comparison: **data and content layer only** — OWLib is
a data-extraction library and has no gameplay, so mover/camera/weapons are out
of scope here and are tracked separately in `systems-coverage.md`.

Evidence basis:
- Rust: `E:\overwatch-client-rs` (`crates/`, `docs/`, `tests/`), commit `ce9dacd`.
- OWLib: `E:\OWLib` (`TACTLib`, `TankLib`, `DataTool`, `owm-formats`).

---

## 0. Identity and scope

| | overwatch-client-rs | OWLib |
|---|---|---|
| Language | Rust (2024), Bevy + wgpu | C# (.NET 9) |
| Purpose | Clean-room **gameplay** client (movement, combat, view model, camera) with a viewer | Data-extraction **toolchain** (DataTool, TankView, TankTonka) + libraries |
| Behaviour source | `Overwatch.exe` Ghidra image, operation-for-operation | Reverse-engineered formats, published reconstruction |
| Data source | operator's install, read in place | operator's install |
| Hard rule | "1 to 1 or not at all"; `FIT-*` markers forbidden new | none stated; pragmatic extraction |
| Rendered output | wgpu viewer (practice range) | Blender addon (`io_scene_owm`), export formats |
| Audio decode | none (explicit handoff to external tool) | yes (Wwise BNK/WEM → RIFF via ZstdNet + codebooks) |
| Network/UI | out of scope | out of scope (tooling) |

**Honest framing.** OWLib is a broader, older, more complete *content*
toolchain. Rust is a narrower but stricter *behaviour* project: every read path
is anchored to a Ghidra address and has a differential test. Comparing them is
comparing a measurement instrument to a workshop — different jobs, overlapping
data layer.

---

## 1. CASC / TACT container

### OWLib (`TACTLib`)
- `ContainerHandler` (dynamic, local CASC): 16 buckets (`0x10`), `IndexHeaderV2`
  v7, 9-byte truncated EKey, `UInt40BE` offset, 30-bit offset + archive index,
  256 GiB archive cap. Bucket = XOR-fold of all 9 ekey bytes.
- `StaticContainerHandler` (Steam): build-config `key-layout-*` bit extraction
  for chunk/archive/offset.
- `CDNIndexHandler`: loose files + per-archive + group indices, footer with
  version/page size/entry size/checksum sizes, binary search.
- `EncodingHandler`: `EN` header, CKey→EKey pages, EKey→ESpec pages, ESpec block.
- `VFSManifestReader` + `VFSFileTree`: TVFS format.
- BLTE: `BLTEDecoder` (static) + `BLTEStream` (seeking); modes N/Z/E/F (F not
  implemented); Salsa20 for `S` blocks; keyring with primary + support keyrings.
- `HttpCDNClient`: `{host}/{cdnPath}/{type}/{k0k1}/{k2k3}/{key}{suffix}`.
- `Keyring`: `key-<hex>` (id byte-swapped) + support `<id> <key> [enabled]`.
- `ClientHandler`: orchestrates install detection, product code, static vs
  dynamic, configs, encoding, VFS, product handler.
- `ContentManifestFile`, `ResourceGraph`, `AssetPackageManifest`,
  `ManifestCryptoHandler` for Tank product (under `Core/Product/Tank`, 402 files).

### Rust (`ow-casc`)
- `install.rs`: `BuildInfo::parse`, `resolve_layout` (install root or
  `_retail_` + `.flavor.info`), `select_build_info`, `Config`, `Install`,
  `parse_keyring` (both spellings), `detect_root`/`detect`, `config_path`.
- `index.rs`: `Location`, `PageRecord` (24 B), `PageTable` (linear scan to
  first zero hash), `SegmentEntries` (entries + pages), `find_range` (binary
  search + equal-range expansion), `find`, `key_state` (18-byte entries then
  page records via sink kinds 1–7), `resolve_range`, `unique_key_count`,
  `KeyState`/`IntervalSet` (resident/non-resident deltas, normalize, range
  resident), `bucket_of` (XOR fold + salt + mask), `parse_segment_file_name`,
  `parse_bucket_file` (section header, hash check, header fields, entry block
  hash, ≥60 pages), `hash_index` (two-word rolling hash), `key_hash`
  (lookup3 `hashlittle`).
- `container.rs`: `BucketIndex` (highest segment per bucket, all 16 required,
  header bucket byte verified), `Store`, `read_payload_of`/`read_payload`/
  `read_stored` (client's `(offset=0x1e, size-0x1e)`), `open_ekey`
  (BLTE decode + CDN fallback), `open_ckey`/`ensure_encoding`, `IndexFooter`
  (MD5 header, v0/1, checksum 0..=16, page size/entry size, 32-bit length
  gate), `PageIndex` (open, terminal checksum, key check MD5(terminal+footer),
  `find_page` binary search over last-key array, `read_page`, `group_lookup`/
  `loose_lookup`).
- `blte.rs`: `parse_frame_header` (magic, BE header size, frame type 0x0F/0x10,
  24/40-byte records, count u24, MD5 over `header_size`, expected-hash compare),
  `strip_data_header`, `decode` (empty/30-byte header-only → empty; direct
  container; multi-chunk; modes N/Z/E/4 LZ4), `decode_lz4_chunk` (11-byte
  header, version 0x01, declared size ≤u32, exponent 5..=16, slice loop with
  `max_compressed`), `lz4_decompress_safe`, `decrypt_chunk` (E: key-name len 8,
  key id LE, IV ≤8 zero-extended, type 'S', ordinal XOR into first IV word,
  Salsa20), `salsa20`/`salsa20_state_init`/`salsa20_core_state`, `md5`.
- `encoding.rs`: `EncodingHeader`/`write_header_fields` (22-byte, 'EN', v1,
  byte17 zero, widths 1..=16, page sizes/counts nonzero, espec nonzero, writes
  all fields before value checks), `SectionLayout::of` (wrapping 32-bit),
  `read_sections` (range reader rule), `EncodingFile::parse` (header + sections
  + espec last-byte NUL), `page_index_search` (lower-middle midpoint, miss =
  `lo.wrapping_sub(1)`), `find_ckey_record` (ckey page lookup, `u16 count`
  low byte, `u32 BE decoded size`, ckey, count*ekey, returns body offset),
  `find_ekey_record` (ekey page lookup, `ekey`, `u32 BE espec index`,
  `u40 BE stored size`, 0xFFFFFFFF terminates), `ekeys_for_ckey`, `stored_size`,
  `decoded_size`, `espec_entry_count`.
- `cdn.rs`: `enabled()` (OW_CDN), `CdnClient` (hosts, http prepended, https
  refused `TlsRequired`, cdn_path, cache dir, timeout, `from_env`), `fetch_data`,
  `fetch_config`, `fetch_range`, `fetch_data_range`, `fetch_index_footer`,
  `fetch_index_tail`, `fetch_index_page`, host fallback, redirect follow (http),
  chunked/plain parse, cache write-through, `build_url`.
- `pagecache.rs`: `PageCache` (16 slots × 0x10000), `read` (zero length → 0;
  closed → bypass; aligned → bypass; head fragment, middle whole-page direct,
  tail fragment), `find`/`claim`/`touch`/`load`, `invalidate`/`invalidate_range`/
  `close`/`reopen`/`invalidate_all`, `plan_read_into`/`Plan::new` (client's
  0xb0-byte read plan).

### Comparison

**Coverage of the same ground.** Both implement: install discovery, build/cdn
config, keyring (both spellings), dynamic 16-bucket index, BLTE N/Z/E, Salsa20
encryption, encoding manifest pages, CDN index footer/pages, HTTP CDN fetch,
page cache. Rust additionally implements: LZ4 `4` blocks (synthetic only, no
outer `4` observed), the page-table **writer** is documented but not
implemented (no read-path caller), the **key-state interval** machinery
(`KeyState`/`IntervalSet` normalize), `sink` kinds 1–7, `page_index_search`
miss rule, `find_ckey_record`/`find_ekey_record`, `SectionLayout` wrapping
arithmetic, CDN `fetch_index_page`/`fetch_index_tail`.

**Quality difference.** Rust's modules are operation-for-operation ports with
Ghidra addresses recorded in `docs/formats/casc.md` and `casc-1to1.md`, and
every leaf has a differential unit in `dev/verify_harness/`. OWLib's TACTLib
is a faithful but *reconstructed* reader: it parses the same bytes but its
constants and edge cases (e.g. corrupt headers, page-count gate, key-state
normalize) are discovered empirically, not pinned to client bytes. OWLib also
has **static (Steam) container** support, which Rust does not (Rust is
battle.net-only). OWLib has **VFS manifest** (`VFSFileTree`) and **client patch
manifest** (`ClientPatchManifest`), which Rust does not appear to implement
(no VFS module in `ow-casc`). OWLib has **group/loose index pages** fully
parsed; Rust's `PageIndex` covers loose + group lookup but the page *selection*
(last-key search) is explicitly open ("CDN index page selection was not found in
the image").

**Gaps in Rust (vs OWLib):** VFS manifest, static/Steam container, client patch
manifest, full group-index page selection (open), TLS (Rust refuses https).
**Gaps in OWLib (vs Rust):** no differential verification against client bytes;
no key-state interval normalize (it just reads); no page-table writer doc; no
LZ4 (F not implemented either, actually OWLib's F is "not implemented"); no
corrupt-header handling (`ALLOW_CORRUPT_HEADER` is mentioned but Rust has
`OpenIndexEntry` header detection).

**Verdict.** Rust is *stricter and more verified* on the dynamic path; OWLib is
*broader* (static + VFS + patch). Neither decodes TLS. Both stop at the same
place on BLTE `F`.

---

## 2. Tank format and chunks

### OWLib (`TankLib`)
- `teChunkedData` (the `0xF123456F` container): header, interleaved
  record/payload table, sentinel `0xC65C`, nested children via
  `serialized_size`. `decode_tag`/`encode_tag` (byte reversal, no XOR).
- Chunk types (20 files): `IChunk` interface; `teDataChunk_Dummy` (zero-ID
  passthrough); effect family (`ECMP`, `ECAT`/`ESHT`, `ECEF`, `ECEN`, `ECEC`,
  `ECMD`, `ECPR`, `ECRR`, `ECSO`, `ECVS`, `ECSS`); model family (`MDLC`,
  `MHRP`, `mskl`, `CLTH`, `MRNM`, `MSTU`).
- `teModelChunk_RenderMesh` (24 KB): VBO/IBO descriptors, vertex elements,
  submeshes, vertex data assets, LOD handling.
- `teModelChunk_Cloth` (9 KB): Domino cloth data (vertices, drivers, bones,
  fake hierarchy).
- `teModelChunk_Skeleton` (5 KB): hierarchy, matrices, bind pose, deltas,
  lookup, IDs.

### Rust (`ow-tank`)
- `chunks.rs` (308 lines, 3 tests): the `0xF123456F` container: header,
  interleaved record/payload table, sentinel `0xC65C`, nested children via
  `serialized_size`. `decode_tag`/`encode_tag` (byte reversal, no XOR).
  `Chunks::parse`, `payload`, `all_with_tag`, `summary`.
- `mstu.rs` (207 lines, 5 tests): `MSTU` chunk: 16-byte `{i64 offset, i64
  size}` header pointing at an STU blob.
- `skeleton.rs` (347 lines, 2 tests): `mskl` chunk: 13 i64 offsets, bone
  counts, parent forest, 48-byte bind poses, bone id hashes.
- `model.rs` (1,534 lines, 11 tests) + `mrnm.rs` (795 lines, 7 tests): the
  render-mesh descriptor layer, its three verified client functions, and the
  viewer's decode; `cloth.rs`/`skin.rs` (629 + 864 lines, 15 tests),
  `hardpoint.rs`, `effect.rs`, etc.

### Comparison
**Near-parity on the container.** Both read the same `0xF123456F` format with
the same sentinel and nested-children rule. Rust's chunk reader is smaller and
test-backed (synthetic + real). OWLib has more *typed* chunk classes; on the
model family Rust's descriptor layer is the one with differential proof
(`fn/MrnmFixup`, `fn/MrnmRuntimeSize`, `fn/MrnmHandler`), and the element
record is read from the client on both sides of the comparison below. Rust's
`chunks.rs` is explicitly documented as the container only; typed chunks are
separate modules.

**Quality note.** OWLib's chunk structs are `[StructLayout]` with named fields
and are consumed by the Blender exporter (`OverwatchModel`). Rust's chunk
modules are read-only parsers with tests; rendering is out of the gameplay
gate. Rust does not export to Blender.

**Gaps in Rust:** LOD selection rule (bit-mask vs equality, neither read),
cloth driver solving (data parsed, physics not run). **Gaps in OWLib:** no
differential verification; no gameplay use; a narrower element-format table
(see next).

### Vertex elements: what the client does, and what neither library can prove

The element record was read on 2026-11-05 from the client's own render-mesh
writer `FUN_14210c450` and its D3D11 consumer `0x142667be0`
(`docs/formats/model.md` § "The vertex element record"):

```text
{u8 semantic, u8 index, u8 format, u8 stream, u32 packed}
packed = offset << 12 | stride << 22
```

* **`>>22` is the stream stride, not a size.** OWLib's `VertexElementDescriptor`
  comments `// todo: >>22 = size?` and never reads it; Rust now reads it and
  checks it against the descriptor's stride (637 records, 68 models, 0
disagreements).
* **Format coverage.** OWLib's `SemanticFormat` handles only
  `0x2, 0x4, 0x6..0x9, 0xC` and throws on anything else, so it fails on
  `0x0, 0x1, 0x3, 0x5, 0xA` — including the four-float position `0x03` the
  client's writer uses. Rust handles `0x0..0xA` and `0xC`, pins `0x03` from
  that writer, and leaves `0x13` (writer-only, no model file carries it)
  undecoded rather than inventing a reading.
* **SNORM.** OWLib divides by 128; Rust divides by 127 and clamps at -1, which
  is the GPU's rule (`max(x/127, -1)`, byte `0x80` = exactly -1). Neither is a
  client *read*: the client never converts vertex bytes on the CPU — the image
  contains no half-float conversion instruction — it hands the engine format
  byte to D3D11 and the GPU converts.
* **Neither library can be verified 1 to 1 on the decode**, because there is no
  client decode to compare against. What is verifiable — the record layout, the
  offset and stride packings, and the per-format byte sizes (from offset
  adjacency and stride) — is now read and pinned on both sides of this file's
  comparison; what remains is the file-format-byte → engine-format-byte join,
  which is **open** and should be recorded as such, not guessed.

### Bone indices: the same join, and one place OWLib is ahead (2026-11-05)

Both libraries join a vertex's raw blend index to a bone through the `mskl`
Lookup, but they get there differently and only one of them reads the second
influence set.

- **OWLib.** `teModelChunk_Skeleton.Parse` reads `Lookup = ushort[RemapCount]`
  from `RemapOffset` (offset 80, count 114 — the same table the client's
  `MsklTableBuild` (`0x141b894d0`) copies to runtime `+0x48`).
  `teModelChunk_RenderMesh.GenerateMeshes` allocates `4*blendCount` indices per
  vertex (blendCount = the largest semantic index + 1) and fills
  `BoneIndices[k].AsSpan(element.Index * 4, 4)` with
  `(ushort)(thisIndices[m] + submeshDescriptor.BoneIdOffset)`. `OverwatchModel`
  then writes `skeleton.Lookup[ind]`, or `0xFFFF` when
  `ind >= skeleton.Lookup.Length`. Weights come out as floats (`UINT8_UNORM4`
  divides by 255).
- **Rust.** `skeleton.rs` reads the same Lookup; `model.rs` reads **only**
  `semantic 4, index 0` into four `u16`s and `semantic 5, index 0` into four
  `u8`s; `skin.rs::remap_index` maps a raw index through the Lookup and treats
  a missing entry or `0xFFFF` as "no bone"; the `BoneIdOffset` is parsed and
  **not applied**; weights stay `u8` and are divided by 255 in `blend_span`.
- **What the install settles.** On all 7,328 skinned model assets the Lookup is
  a permutation into `[0, bones_abs)` with no `0xFFFF`, `remap_count` is the
  largest raw index plus one on 7,308 of them (the rest leave spare slots, none
  overflow), the file stores the exact inverse at `mskl +88`, and OWLib's own
  claim is confirmed by geometry: the dominant influence's bone sits a median
  `0.122 m` from its vertices through the Lookup versus `0.591 m` through the
  raw index on Tracer's character mesh. Every install submesh has
  `BoneIdOffset` = `0`, so OWLib's addition is unobservable here.
- **Where OWLib is ahead.** 454 model assets declare a second
  `semantic 4 / index 1` element and their eight weights sum to exactly 255;
  OWLib fills both sets, Rust drops the second, so those influences are missing
  from the viewer (`ow-tank-1to1.md`).
- **What neither can settle.** The client's consumer. The client's reader
  sizes the tables but no function that joins a mesh index to a bone has been
  found, so the `BoneIdOffset` addition and the `>= length` / `0xFFFF` rules
  remain the reference's behaviour on both sides.

### Effect components: measured, and the reference is stale (2026-11-10)

This one changed the verdict, so it is recorded in full. OWLib's
effect-component structs (`TankLib/Chunks/teEffectComponent*.cs`,
`teEffect_CAT`/`teEffect_SHT`, `teEffectChunkComponent`/`teEffectChunkShaderSetup`)
describe **pre-2.x** payloads. The client's own builder (`FUN_142e4a780`) copies
the current fields, and the install agrees with the client on every size:
`ECMD` 32 B (reference 24), `ECEF` 32 (8), `ECEN` 80 (16), `ECEC` 64 (17),
`ECSO` 32 (8), `ECVS` 48 (8), `ECRR` 64 (8), `ECPR` 112 (40), `ECAT` 16 (8),
`ESHT` 32 (8). The old Rust `effect.rs` was a port of those structs, so it read
the first 8/24 bytes and called it the component; it is replaced.

Five of the reference's tags have **no read site in this image**: `ECMP`,
`ECSS`, `ECAT`, `ESHT` and `ECCV` appear nowhere in either byte order and are
not in the client's 0x34-entry kind table at `0x143610490`. Their payloads are
editor-side data, so no 1:1 implementation of those reference classes can come
from this image.

The closure is `crates/ow-tank/src/effect.rs` plus `tests/effect_real.rs`,
written from the client's copies and the GUID types each field carries on the
install; the page is `docs/formats/effects.md`. **Rust gap closed:** the effect
component family. **OWLib gap recorded:** its effect structs are stale, and it
reads five tags the client does not.

### Two model-family findings from the same audit (2026-11-10)

- **`MHRP` does not exist in this build.** OWLib's `teModelChunk_Hardpoint`
  describes a `{count, count, offset, offset}` header and 96-byte records. The
  tag id occurs nowhere in the image, and a census of all 32,280 models on the
  install finds no `MHRP` chunk. This build's hardpoints are
  `STUModelHardPoint` (`0xBE9DDFDF`) instances in the model's `MSTU` and on a
  look's `m_hardPoints`. The Rust `MHRP` port was deleted rather than kept as a
  parser for a chunk the client cannot read (`docs/formats/animation.md`).
- **The model chunk tags are `MOCL` and `CLTH`, not `LCOM`/`HTLC`.** The two
  dispatchers compare `0x4d4f434c` and `0x434c5448`; the file census over all
  models is `MDLC`, `MSTU`, `MRNM`, `mskl`, `MESD`, `CLTH`, `MOCL`. An earlier
  version of `docs/formats/model.md` rendered the two words' bytes in the wrong
  order; corrected, with the compare addresses recorded (`docs/formats/model.md`).

**`MDLC`'s header, checked instruction for instruction.** `MdlcMaterialRead`
(`0x141b88cb0`) copies `+0x10`/`+0x20`/`+0x30` as three 16-byte `MOVUPS` and
`+0x40` as one dword, then **skips `+0x44..+0x50`**. So OWLib's
`BoundingBox[16]` (0x10..0x50) has the right *range* but the wrong *reading*:
the 13 live floats are a box minimum, a box maximum, a sphere centre and a
radius — proven against the decoded vertices on two models — and the last row's
three lanes are dead. OWLib's `Padding[24]` (0x50..0x68) is worse than
mislabelled: those six dwords are copied into the model and differ per model,
so they are data. Its `MaterialOffset` (0x08), `MaterialCount` (0x68) and
`m_104` (0x70) are right, and its parser reads only the material array anyway,
so the error is in the struct's documentation, not in what it produces.
`ow-tank` had the mirror-image error: it read the 16 floats as a 4x4 matrix and
then dropped them. Both are corrected in `docs/formats/model.md`.

---

## 3. STU (structured data)

### OWLib (`TankLib/STU`)
- 521 files: 12 core, 11 primitives, 467 generated type classes, 28 enums.
- `teStructuredData`: magic `STUD` → V1; otherwise V2. V1: instance count +
  offsets, 8-byte instance prefix (hash + nextOffset), fields at
  `m_offset` ≥ 8, trailing pad to `m_size`. V2: 36-byte header, CRC64 checksum,
  `STUBag<STUInstance_Info>` (hash, embedder field/instance, size),
  `STUBag<STUInlineArray_Info>`, `STUBag<STUBag<STUField_Info>>` (hash + size),
  dynamic data buffer + main data buffer; instances created by hash via
  `teStructuredDataMgr`.
- `teStructuredDataMgr`: scans assembly for `[STUAttribute(hash, size)]`,
  builds instance/field maps; field readers by type; primitive factories.
- `DefaultStructuredDataFieldReader`, `EmbeddedInstanceFieldReader`,
  `InlineInstanceFieldReader`.
- Primitives: s8/u8/s16/u16/s32/u32/s64/u64/f32/f64/teString.
- Asset refs: `teStructuredDataAssetRef<T>` (GUID, V2 deobfuscates with header
  checksum + field hash + byte swaps).
- HashMaps: `teStructuredDataHashMap<T>` (dyn-data offset, key/value pairs,
  instance indices).
- Serialization attributes: `STUAttribute`, `STUFieldAttribute`,
  `STUEnumAttribute`.
- CRC64: reflected poly `0x95AC9329AC4BC9B5` (table verified against TACTLib).

### Rust (`ow-stu` + `ow-tank`)
- `ow-stu/src/lib.rs` (879 lines, 1 test): v1 (`STUD`) and v2 layouts,
  instance/field bags, inline arrays, dynamic block, asset-ref deobfuscation
  (`deobfuscate`), CRC64 header checksum.
- `ow-stu/src/crc.rs` (68 lines, 2 tests): CRC64 (reflected poly
  `0x95AC9329AC4BC9B5`), table verified against TACTLib.
- `ow-stu/src/types.rs` (~1398 lines, 4 tests): `ClassRegistry`, `ClassDef`/
  `FieldDef`/`EnumDef`, `Value`/`TypedField`, v1/v2 typed reads, defaults,
  arrays, maps, inline instances.
- `ow-tank`: `mstu.rs`, `classes.rs`, `classreg.rs`, `fieldnames.rs`.

### Comparison
**Format parity.** Both read V1 and V2 with the same header, bags, dynamic
block, asset-ref deobfuscation, CRC64. Rust's deobfuscation matches OWLib's
(field hash doubled, XOR header checksum, byte swaps). Both handle inline
arrays and embedded/inline instances.

**Coverage difference.** OWLib has 467 generated STU type classes with named
fields and is consumed by DataTool for extraction. Rust has a typed layer
(`ClassRegistry`, `Value`, `TypedField`) driven by a registry loaded from the
install, plus `fieldnames.rs`. Rust does not ship hand-written classes for
every type; it resolves class/field definitions from the install's own data.
OWLib's classes are a *published reconstruction* (generated), which is exactly
what Rust's docs warn against treating as ground truth (`sound.rs` module
comment: "a claim about structure, not a measurement of this install").

**Quality note.** Rust's STU reader is smaller and test-backed (7 tests in
`ow-stu`, plus integration). OWLib's is larger and battle-tested across
thousands of assets, but its field layouts are generated and sometimes wrong
(OWLib itself notes "please tell zingy" on variant bitmasks). Rust's typed
layer is more conservative and explicitly marks unverified hashes.

**Gaps in Rust:** no per-type named field structs for all ~thousands of types
(by design — registry-driven). **Gaps in OWLib:** generated layouts are
reconstructions; no client-byte verification.

---

## 4. Maps

### OWLib (`TankLib/teMapChunk.cs`)
- `teMapPlaceableData`: header (placeable count, instance-data offset +16,
  placeable offset, unknown). Placeables: 16-byte UUID + 3 flag bytes + u32
  size, then body. Body types: model group, entity, model, single model, light,
  reflection point, text, sound, effect, sequence, area, dummy.
- Entity instance data: table at `+104` (type u32 + relative offset), then STU
  parse at target.
- Model group: header (`Model`, `GroupCount`, `TotalModelCount`, `Mask`, 3
  unknowns), then groups (`ModelLook`, `UnkSize`, `EntryCount`), then entries
  (translation/scale/rotation + 14 unknown words).
- Model placeable: `teMapPlaceableModel`/`SingleModel` reuse structure:
  `Model`, `ModelLook`, translation, scale, rotation, unknown quat.
- Light, reflection point, text, sound, effect, area structs as documented.

### Rust (`ow-tank/src/mapchunk.rs` + `mapresolve.rs` + `mapguid.rs`)
- `MapChunk::parse` (header, placeables, skip-by-size). `Kind` enum with
  type ids. `detect` (scores Model vs Entity by resolving refs).
- `Model`/`SingleModel` body: model at `+0x00`, look at `+0x20`, translation
  `+0x30`, scale `+0x3C`, rotation `+0x48`, unknown rotation `+0x58`. (Deliberately
  differs from OWLib's generated struct, measured on 16,357 placeables.)
- `ModelGroup`, `Entity`, `Light`, `ReflectionPoint`, `Text`, `Sound`,
  `Effect`/`Sequence`, `Area` bodies ported field-for-field from OWLib, with
  real-file verification noted where measured.
- `mapresolve.rs` (805 lines, 3 tests): `STUMapHeader` (type `0x09F`),
  base map `0x002`, `m_supportedGameModes`, paired fields, layer key. Variation
  selector absent client-side (map arrives as type-`0x002` GUIDs in the
  level-load message).
- `mapguid.rs` (124 lines, 4 tests): synthesised chunk GUIDs
  (`0x0DD0000000000000 | (chunkKind[typeIndex] << 32) | (mapGuid & 0xFFFFFFFF)`),
  31 slots from `FUN_141ff5c80`, four extra fields.
- Integration: `mapchunk_kinds` (16), `map_header_bundle_real` (1),
  `map_resolve_real` (5), `practice_range_real` (3), `discovery` (1/2).

### Comparison
**High parity on parsing.** Rust ported OWLib's map placeable structs
field-for-field, with the one deliberate, measured correction on the model
placeable offset (OWLib's generated struct was shorter/wrong; Rust's measured
offsets resolve assets on 16,357 placeables). Both skip unknown kinds by
declared size. Both parse entity instance-data tables.

**Difference in approach.** OWLib is a pure data reader. Rust additionally
resolves the map-to-chunk link (`mapguid` synthesised GUIDs from the Ghidra
image), the map header variation logic (`mapresolve`), and the bundle asset
path. Rust's `Kind::detect` is reference-derived (only Model/Entity scored);
OWLib has no detection — the caller declares the kind.

**Quality note.** Rust's map module is more thoroughly tested on the install
(16 kind-sweep tests + practice range). OWLib's map reader is consumed by
DataTool for extraction and is mature but its "variant bitmask" path throws
(`if ((commonStructure.Unknown16 & 0x40) != 0) throw`), i.e. it refuses maps
with variants. Rust does not throw; it records them.

**Gaps in Rust:** the level-load message producer, bundle directory producer,
streaming policy (open). **Gaps in OWLib:** no behaviour claim; no differential
verification.

---

## 5. Textures

### OWLib (`TankLib/teTexture.cs` + `teTexturePayload.cs` + `TextureTypes.cs`)
- `TextureHeader` 32 bytes: Flags (short, bitfield), MipCount, Format (byte),
  Surfaces, UsageCategory, PayloadCount, Unk7, Width, Height, DataSize,
  Unk14NEW, Unk16 (ulong), Unk24 (ulong).
- OW2 hack: `if (Header.Format >= 0x1A) Header.Format -= 1;` (value added in
  the middle of the format enum).
- `TextureTypeFromHeaderByte`: maps adjusted byte to legacy FourCC (DXT1/3/5,
  ATI1/2, else Unknown → DX10 path).
- `GetPayloadGUID2`: external payloads are type `0x4D` resources, with payload
  index + payload bit packed into the GUID.
- Payload header 16 bytes: Mips, Surfaces, ImageSize, HeaderSize. `RawData`
  read as `ImageSize` bytes.
- DDS export: writes DX10 extended header with `Header.Format` as DXGI index;
  dimension from Flags; cubemap/array handling; mip count; payloads written
  high-index → low-index.
- Color parsing in payload is commented out (TODO); `Color1..Color5` dead.
- Naraka invalid-size workaround uses `Read` instead of `ReadExactly` (TODO).
- No per-mip splitting; color space implicit in DXGI format index.

### Rust (`ow-tank/src/texture.rs`, 5313 lines, 42 tests)
- `TextureHeader` 32 bytes, same fields. `ENGINE_TO_PM`/`PM_TO_DXGI` format
  tables, `dxgi_of`. `format_family_and_bpp`, `size_walk`, `cooked_size`,
  `descriptor_size_walk`. `TextureChunk` (16-byte). `BlockPlan`. `TextureLevel`/
  `Texture`. BC1..BC7 + BC6H, uncompressed families.
- Payload chain: `requested_payloads`, `block_span`. `Texture::requested_payloads`.
- Format switch, descriptor builder and descriptor size walk verified
  (`TextureFormatFamilyBpp`, `TextureDescriptor`, `TextureDescriptorSizeWalk`).
- BC6H/RGBA16F/R16F keep exact halves and upload as `Rgba16Float` (viewer
  substitution for DXGI 10/54/95/96); R16_UNORM keeps exact words and uploads
  as `R16Unorm` when `TEXTURE_FORMAT_16BIT_NORM` present, clamped
  `Rgba8Unorm` fallback otherwise.
- 1D/cube SRV views, sampler, volume depth, live R16 upload proof, UINT/BGRA
  decode, client material application and 55 practice-range placeholder draws
  are open.

### Comparison
**Deep parity on header and payload chain.** Both read the same 32-byte header
and 16-byte payload header, same OW2 format adjustment, same payload-GUID
derivation (type `0x4D`). Both export to DDS with DX10 extended header.

**Rust is ahead on verification.** Rust verified the format-family/bpp switch,
descriptor builder and descriptor size walk against client bytes; OWLib did not
(its format mapping is reconstructed). Rust keeps exact 16-bit/HDR values;
OWLib's color parsing is dead code (commented out), so it effectively treats
payloads as opaque blobs too — same end state, but Rust documented the reason.

**Rust is ahead on upload fidelity.** Rust preserves BC6H/RGBA16F/R16F halves
and R16_UNORM words with explicit format negotiation; OWLib's DDS export
passes the DXGI index through unchanged, which is equivalent for a DDS file
but does not address wgpu upload constraints.

**Quality note.** Rust's texture module is the largest in `ow-tank` (5313
lines, 42 unit tests + 5/2 integration) and is the most heavily verified
format in the project. OWLib's texture reader is smaller and less tested, but
it is consumed by DataTool's extraction and works in practice.

**Gaps in Rust:** client material application, 1D/cube SRV views, sampler,
volume depth, live R16 upload proof, UINT/BGRA decode, 55 placeholder draws
(open). **Gaps in OWLib:** no client-byte verification; dead color-parsing
code; invalid-size workaround.

---

## 6. Materials

### OWLib (`TankLib/teMaterial.cs` + `teMaterialData.cs`)
- `teMaterial`: file type `008`, 32-byte header, four GUIDs: ShaderGroup
  (`0x085`), ShaderSource (`0x088`), unknown (`0x03A`), MaterialData (`0x0B3`).
- `teMaterialData`: 64-byte `MatDataHeader` (static inputs offset/count, offset2,
  texture offset, offset4, counts). Texture record 16 bytes: `TextureGUID`
  (type `004`), `NameHash` (CRC32 of input name), `Flags` (byte).
- CRC32 reflected, standard polynomial `0xEDB88320`.

### Rust (`ow-tank/src/material.rs`, 573 lines, 7 tests)
- `STUModelLook` (`0x5207484B`), `STUModelMaterial` (`0x494B66C1`), `teMaterial`
  (type `0x008`) four GUIDs, `teMaterialData` (type `0x0B3`) texture records
  (16-byte stride, count byte at `+0x3C`, offset i64 at `+0x10`), CRC32 name
  hashing, `base_texture_of_material` (sRGB rule), `material_of_id` join.
- CRC32 zlib (polynomial `0xEDB88320`), verified vectors
  (`crc32(b"basecolor") == 0xAD10_F602`).
- Base texture: first record whose stored format resolves to an sRGB DXGI id.
  Explicitly a viewer heuristic, not a read client rule (open).
- Integration: `material_real` (6).

### Comparison
**Full parity on layout.** Both read the four-GUID material header, the 64-byte
material-data header, the 16-byte texture record (GUID + CRC32 name hash +
flags), and use the same CRC32. Rust's offsets and field positions match OWLib
exactly (texture offset at `+0x10`, count at `+0x3C`).

**Rust's additions.** Rust additionally joins model → look → material → data →
texture via `material_of_id` and `base_texture_of_material`, and verifies CRC32
against named vectors. Rust flags the sRGB-albedo rule as an explicit heuristic
(open), where OWLib's DataTool just exposes the records and lets the user pick.

**Quality note.** Rust is stricter (verified CRC vectors, explicit open marker
for the albedo rule). OWLib is more complete (it actually renders materials in
the Blender exporter, including normal/roughness/mask channels).

**Gaps in Rust:** client material application (which input is albedo by name
hash), 55 placeholder draws (open). **Gaps in OWLib:** no client-byte
verification; extraction-only.

---

## 7. Animation

### OWLib (`TankLib/teAnimation.cs`)
- Chunk type `006`. `AnimHeader` (priority, group, bone count, flags, unknowns,
  duration, FPS, effect, bone list offset, info table offset, padding).
- Frame count = `(int)(FPS * Duration) + 1`.
- `InfoTable`: ScaleCount/PositionCount/RotationCount, flags, indices offsets,
  data offsets (word ×4 relative to info-table stream position).
- `InfoTableFlags`: `NewScaleCompression = 0x100` (todo), `LegacyPositionFormat =
  0x200`, `NewRotationCompression = 0x400`.
- Scale: 6 bytes (3 ushorts) / 1024.
- Position: new = 3 half-floats + trailing u32 (ignored) / 32; legacy = 3
  floats.
- Rotation: new = 4 ushorts bit-packed → 3 components + axis; old = 3 ushorts
  with axis bits; w = sqrt(1 - min(sum squares,1)); `QuatFromAxisOrder`.
- Bone list (int[]), per-bone dictionaries (frame → value).
- `AnimFlags` enum empty (todo). `SomeTableOffset` ("new animation param table,
  usually float tracks") not read (todo). Effect not used. No interpolation.

### Rust (`ow-tank/src/clip.rs` + `skeleton.rs` + `blendtree.rs`)
- `clip.rs` (3777 lines, 37 tests): animation clip chunk (type `0x006`):
  header with frame count/fps/duration and stored reciprocals, ten-qword
  section table at `+0x30`, family word at `+0x00`, kind-byte dispatch table
  (`KIND_TABLE`, 31 cases) at `record+0xe`, per-kind sample decoding.
- `skeleton.rs` (347 lines, 2 tests): `mskl` chunk: 13 i64 offsets, bone
  counts, parent forest, 48-byte bind poses, bone id hashes.
- `blendtree.rs` (351 lines, 0 tests): `STUAnimBlendTree`: nodes
  (BranchByCategory, LayerChild, AnimationPose2d, Animation, LayerAdditive),
  param 1-based index into item's float array.
- Integration: `blend_bind_check` (1), examples `blendset_probe`,
  `blendtree_probe`, `clip_chan_probe`, `refclip_probe`, `refskel_probe`,
  `retarget_probe`, `hardpoint_probe`, `posecheck`.

### Comparison
**Different decomposition.** OWLib is a single file (`teAnimation.cs`) that
decodes the clip header and per-bone keyframes. Rust splits into `clip.rs`
(sampler with 31 kind cases), `skeleton.rs` (bind pose/hierarchy), and
`blendtree.rs` (blend tree nodes). Rust's clip reader is far deeper on the
*sampler* side (kind dispatch, section table, reciprocals) but does not
reproduce OWLib's exact keyframe dictionaries — it decodes samples per kind.

**Quality note.** OWLib's animation reader is simpler and leaves interpolation
to the consumer ("then slerp between or whatever"). Rust's is larger and has
more tests (37 unit) but leaves the Pose2d VM, reference retarget and
per-frame look consumer open (open questions 3, 4). OWLib does not implement
interpolation either.

**Gaps in Rust:** Pose2d VM, reference-skeleton retarget, per-frame look
consumer (open). **Gaps in OWLib:** `AnimFlags` empty, `NewScaleCompression`
todo, `SomeTableOffset` not read, no interpolation, no retarget.

---

## 8. Replay

### OWLib (`TankLib/Replay`)
- `tePlayerReplay` (magic `prp`): format version, build, map, game mode,
  unknown1 (bit 2 gates highlight info), unknown2/3, 32-byte map checksum
  (HMAC-SHA256 with fixed key), params block length, `ReplayParams` (start/end
  frame, duration, start/end ms, hero roster), optional `HighlightInfo`,
  zstd payload (compressed bytes returned as-is; decompression commented out).
- `tePlayerHighlight` (magic `phl`): format version, checksum, data length,
  4×int64 unknowns, replay version, build, player id, flags, map, game mode,
  highlight info array, hero roster, filler structs.
- `HeroData`: skin/weapon skin/potg animation, spray/voiceline/emote id
  arrays, announcer id, hero GUID.
- `HighlightInfo`: player name, kind, unknowns, elevation, team index, position/
  direction/up, hero/item skins, teams, highlight intro/type, timestamp, UUID.
- `ReplayChecksum.Compute`: HMAC-SHA256 with hard-coded 32-byte key.
- `Logical.ZstdBuffer`: reads remaining bytes; decompression uses ZstdNet into
  a fixed 1 MB buffer.

### Rust (`ow-tank/src/replay.rs`, 1383 lines)
- `prp`/`phl` containers, verified prefix (`Header`: format version, build,
  map, game mode; `Container`: through `0x6C`), `Params`, `Hero`, `HighlightInfo`,
  `ZstdBlock` (compressed bytes + optional declared uncompressed length from
  Zstd frame header).
- `find_replay`, `first_zstd_frame`, `frame_content_size`.
- Payload not decoded (no Zstd decoder; explicit).
- Integration: `replay_real` (2).

### Comparison
**Near-parity on container layout.** Both read the same magic, fields, hero
roster, highlight info. Rust's `Replay::parse` reproduces OWLib's field order
exactly and adds bounds checking and named errors.

**Key difference: Rust is honest about what is verified.** Rust explicitly
documents that the tail of the header (`+0x18` on) is *not verified* against
real files and that `params_block_length` reads as zero in every file measured,
so `Replay::parse` usually fails on real data rather than returning a
plausible roster. OWLib's reader parses the same bytes and would return a
roster without flagging the mismatch. Rust also verifies the length prefix
against following bytes (`find_replay`), which OWLib does not.

**Both stop at the same place on payload.** Neither decodes the Zstd payload
(OWLib's decompression is commented out; Rust has no decoder). Rust parses the
Zstd frame header to report a declared content size when present; OWLib uses
ZstdNet when asked. Rust does not compute HMAC (no SHA-2); OWLib does.

**Gaps in Rust:** no SHA-2/HMAC (checksum bytes exposed only), no Zstd
decode (by design). **Gaps in OWLib:** no verification of declared vs actual
layout; filler struct over-read noted in its own comments.

---

## 9. Sound

### OWLib
- TankLib: only STU-level references (`STUSoundBank` with `m_wwiseBankID`,
  `m_wwiseWEMFileIDs`, `m_wwiseWEMStreamIDs`, `m_wwiseEventIDPlay/Stop`,
  `m_soundBank`; `teEffectComponentSound` holds a sound GUID).
- DataTool: full Wwise bank parsing (`WwiseBank`, `WwiseBankHeader`, `DIDX`/
  `DATA`/`HIRC` chunks, `IBankObject` dispatch for types 1/2/3/4/11),
  WEM extraction, RIFF conversion (Vorbis/Opus) using bundled codebooks and
  `revorbstd`.

### Rust (`ow-tank/src/sound.rs`, 3565 lines)
- Wwise metadata, sound/voice manifests, WEM headers. Does not decode audio
  (explicit handoff to external tool). Class/field hashes from OWLib's
  generated STU declarations, cited by file and line. Five of eleven named
  types have no reference layout (counted, reported as `Record::Unnamed`).
- Integration: `sound_real` (7).

### Comparison
**Rust is metadata-only; OWLib decodes audio.** OWLib has a complete Wwise
bank/WEM → RIFF pipeline. Rust deliberately stops at metadata (WEM headers,
manifests) and hands audio decode to an external tool, per its 1:1 rule (no
audio decoder in the client's gameplay path).

**Quality note.** Rust's sound module is large (3565 lines) and honest about
which types are unnamed. OWLib's is broader and actually produces audio files.
Rust's hashes are cited to OWLib's own generated files, which Rust flags as
reconstructions, not measurements.

**Gaps in Rust:** audio decode (by design, external). **Gaps in OWLib:** no
client-byte verification of sound STU layouts.

---

## 10. Math, helpers, export formats

### OWLib
- `Math/`: `teVec2/3/3A/4`, `teQuat`, `teColorRGB/RGBA`, `teMtx43A`,
  `teUUID`, `teEntityID`, `DBID`.
- `Helpers/`: `DataSerializer` (reflection-based readable serializer,
  logical types including Zstd buffer), `CRC` (CRC32/CRC64), `Jenkins96`,
  `Logger`, `SliceStream`, `RememberMeStream`, `ConsoleSwatch`.
- `ExportFormats/`: `IExportFormat`; `OverwatchAnimationClip` (owanimclip),
  `OverwatchModel` (owmdl — richest: materials, skeleton, submeshes, vertices,
  normals, tangents, UVs, bone indices/weights, colors, indices, hardpoints,
  LOD filtering, cloth reparenting), `RefPoseSkeleton` (SMD), `SEAnim`.

### Rust
- Math is inlined into modules (`Vec3` in `replay.rs`, `[f32;3]` elsewhere);
  no shared math crate. `ow-tank` uses plain arrays.
- No export formats; no Blender integration.
- Hashing: CRC32/CRC64 in `ow-stu`/`material.rs`, verified.

### Comparison
**OWLib is far ahead on tooling.** Export to Blender, SMD, SEAnim, owanimclip,
owmdl with full vertex data. Rust has no export formats and no Blender
integration — it is a runtime, not an extractor.

**Rust is ahead on nothing here** except verified hash constants.

---

## 11. Testing and verification culture

| | Rust | OWLib |
|---|---|---|
| Unit tests | ow-casc 132, ow-tank 254, ow-stu 7 (+ integration 87 in ow-tank, 11 in ow-casc) | `TACTLibTest/Program.cs` (manual harness), `TankLib.Test` (separate project, not counted here) |
| Differential vs client | `dev/verify_harness/` units (fn-*.json) with first-divergence | none |
| Gate | `cargo xtask verify` (fmt, clippy -D warnings, tests, tool index, deviation check) | dotnet build/test |
| Provenance | `docs/provenance.md` evidence classes; addresses in `function-names.csv` | none documented |
| Honesty markers | `FIT-*` (legacy, forbidden new), open questions queue | TODO comments |

**Quality difference.** Rust has a *gate* that fails on unverified `FIT-*`
markers and on clippy warnings; OWLib has no equivalent. Rust's tests are
synthetic + real-install integration. OWLib's tests are lighter (a manual
`Program.cs` and a separate `TankLib.Test` project not examined here).

**Caveat.** OWLib is far more broadly exercised by real users (DataTool
extraction of whole installs), so its bugs are found in practice even without
a formal gate. Rust's strictness is a different kind of quality.

---

## 12. Summary table of gaps

### Rust gaps vs OWLib (content/tooling breadth)
- No VFS manifest reader, no static/Steam container, no client patch manifest.
- No render-mesh vertex/submesh/LOD aggregation (viewer-only).
- No cloth driver solving (data parsed only).
- No audio decode (by design).
- No export formats / Blender integration.
- No per-type named STU classes for all ~thousands of types (registry-driven
  by design).
- CDN page *selection* (last-key search) open.
- TLS not supported (http only).

### OWLib gaps vs Rust (verification/behaviour discipline)
- No differential verification against client bytes.
- No 1:1 operation-for-operation claim; layouts are reconstructed.
- Map variant bitmask path throws instead of handling.
- Texture color parsing dead code; invalid-size workaround.
- Animation `AnimFlags` empty, `NewScaleCompression` todo, `SomeTableOffset`
  not read, no interpolation.
- Replay layout not verified against real files; filler over-read noted.
- No key-state interval normalize, no page-table writer doc.
- Effect component structs are pre-2.x and short on every tag (see the table in
  §2); its `ECMP`/`ECSS`/`ECAT`/`ESHT` readers describe tags the client has no
  code for.
- No gameplay at all (out of scope for OWLib).

### Shared open edges
- Neither decodes BLTE `F` blocks meaningfully (OWLib: not implemented;
  Rust: no outer `4` observed).
- Neither decodes replay Zstd payload.
- Neither has a verified "which texture input is albedo" rule (Rust marks open;
  OWLib leaves it to the user).

---

## 13. Recommendation

The two projects are **complementary, not competitors.** OWLib is the right
reference for *breadth* (which types exist, which chunks exist, how to export).
Rust is the right reference for *depth* (exact bytes, verified offsets, honest
open markers). The Rust project already cites OWLib's generated STU classes as
a published reconstruction and warns against treating them as measurements —
that posture should be preserved. Where Rust's open questions need a layout
hint (e.g. map placeable kinds, sound types), OWLib's structs are a good
starting point, but every adoption must be re-verified against the install and
recorded with an address before it lands.

**Do not** import OWLib code into Rust (different language, different license
posture, and it would violate the 1:1 rule's "read the client's own code"
requirement). Use OWLib as a *pointer*, then measure.
