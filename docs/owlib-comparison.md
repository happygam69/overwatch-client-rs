# OWLib vs E:\OverwatchTest — side-by-side system comparison and gaps

Companion docs: `docs/systems-coverage.md`, `docs/formats/*`, `STATUS-handoff.md`. OWLib reference: `E:\CASCReader\OWLib` (.NET 10 toolchain).

## 1. CASC / install / containers / net / CDN

| Area | OWLib (TACTLib) | Ours (ow-casc) | Gap |
|---|---|---|---|
| Install detection | Agent/protobuf DB, `.build.info`, build/CDN/installation info, region/platform args | `Store`, `Install` open by path | We do not parse Battle.net Agent DB; rely on supplied path |
| Dynamic containers | CASC `.idx` v7 buckets + `data.*`, residency checks | CASC index/encoding + BLTE; six index sources merged (bucket `.idx` from both `data/` and `ecache/`, per-archive `indices/*.index`, `shmem` group index, archive-group, patch-archive-group) | Superseded 2026-10-17: no gap. We additionally read the two bucket directories and the writable `shmem` layer, which the reference does not |
| Static containers | Steam/static container handler and key layout bits | Not handled | No gap: this build ships no `key-layout-*` files, so the handler is unreachable here |
| VFS / TVFS | `VFSFileTree.Open`, TVFS header/path/span/container-file tables | Not handled | No gap: no `vfs-root` in this Prometheus build config |
| BLTE | full decoder: N/Z/E/F, encrypted blocks, Salsa20; ARC4 named but unsupported | `blte::decode`: N/Z/E + Salsa20, decoded-size verified. `F` (BLTEARCHIVE) unimplemented **on both sides** | No gap. `E` mode is at parity and ours is the only one with a test (published ECRYPT keystream) |
| Encoding | `EncodingHandler` CKey→EKey pages + EKey ESPEC/file sizes | `EncodingFile` | Close. Ours slices the encoding file instead of materialising ~100 MB of pages. **Zero tests on either side** |
| Manifests | CMF/AssetPackage/TRG cryptos and Tank product handler | ow-tank manifest/package/trg/keytable | Key derivations: **ours 2 CMF + 1 TRG, reference 253 + 141.** Ours hard-fails an unknown build where the reference falls back to the nearest older build. Real gap, and the largest in this table |
| Keyring | support keyring download/parse | read from `OW_SUPPORT_KEYRING` or auto-discovered under the install | No gap. We do not *download* it, and do not need to for a local install |
| CDN / patch / NGDP | online mode, CDN index, HTTP client, patch manifests | `ow-casc/src/cdn.rs`: hand-written std-only HTTP client (own header/chunked/Range parsing), on-disk cache, three index-flavour widths (6/5/0), patch-archive-group, host fallback, redirects, 1 GiB body cap. Off unless `OW_CDN` is set | Superseded 2026-10-17: "no network code" was true before that date. Ours **refuses TLS rather than downgrading**; the reference uses `HttpClient` |
| Downloading | NGDP RIBBIT clients | n/a | **Still a gap, and the only remaining one in this table.** We read a `.build.info` by path; we cannot ask a version service what to install from scratch. Not needed for a retail install that already exists |
| Product handlers | Overwatch Tank, Diablo IV, D2R, WoW, MNDX, etc. | Overwatch only | Not needed, but fewer formats |

## 2. STU structured data

| Area | OWLib (TankLib STU) | Ours (ow-stu/ow-tank) | Gap |
|---|---|---|---|
| Format | V1 (`STUD`) and V2 | both: V1 header + record table + class-offset fields; V2 unchanged | none measured; V1 paths have no real asset to run on (bucket `0x05A`/`0x05E` empty in the install) — synthetic tests only |
| Parser | V2 header CRC64, instance bag, inline arrays, field-info bags, dyn/data sections | V2 layout, V1 layout, typed layer with full field-reader dispatch | — |
| Typed classes | 7,782 generated `STU_<hash>`/named classes via TankLibHelper | registry loaded at runtime from the local OWLib checkout (7,782 classes, 644 enums, 23,204 fields); no generated code in repo | none for this install; names need the checkout present (deliberate, clean-room) |
| Instance refs | index-based embedder/inline/hash-map instance refs | V2 index refs, V1 record-offset refs, inline payloads and hash-map instances all traversal-typed | raw embedder-link walk still blind to back-link-less instance arrays (measured: volley motions) |
| Asset refs | V1 direct GUID, V2 deobfuscation CRC/field/XOR/swap | V1 raw 16-byte `pad+guid`, V2 deobfuscate | compatible |
| Enums | registry and generated enums | 644 enums read (underlying width + member names) | — |
| Field readers | Default/Embedded/Inline factory architecture | `ReaderKind` Default/Embedded/Inline per field; primitives, enums, `Math` structs, strings, refs, arrays, maps, defaults incl. `Defaulted` detection | — |
| Type registry | reflection-by-hash registry; inversion tables | `ow_tank::classreg` loads the checkout's sources; hash→class/field/enum/default resolution | checkout dependency is deliberate (nothing vendored) |
| Name tables | RegisteredSTUTypes/KnownTypes/KnownFields/KnownEnums | names/field names resolve through the same runtime registry; the repo itself still ships no names | — |
| Serialization (write) | **100% not implemented** — every `Serialize` is a `NotImplementedException`; the generic writer exists but is on the `.tank` path, not STU | none; read-only | No gap. Both are read-only, which the previous wording ("methods mostly not implemented") understated |
| Asset repo scan | TankTonka reference collection | ow-tank `assoc.rs` | The five classification arrays were diffed and are **identical** (89 STUv2 entries both sides). Real gap is naming, not classification: reference names 74 types in `0x000–0x0FF`, we name 8 |

## 3. Asset/chunk containers and asset identity

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| GUID model | `teResourceGUID` bit fields and labels | `guid` in ow-tank | likely comparable |
| Chunked container | magic `0xF123456F`, `IChunk`, 0xC65C serialization tag | chunks and custom container | likely comparable |
| Asset cross-build diff | LZ4 GUID/CKey diff | none | gap for build differencing |
| Diff utilities | `Diff.cs` | none | gap |

## 4. Model geometry

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| Model chunk (MDLC) | bounding box, material offsets/count, material GUID array | model.rs likely reads | verify parity |
| Render mesh (MRNM) | vertex buffers, index buffer, semantics, submesh index remapping, index remap tables, vertex data GUID buffers | model.rs parses MRNM | likely partial: verify vertex-data GUID resolution/remap, streaming LOD |
| Vertex attributes | all texcoord sets, Color1/2, SubmeshFlags | `Mesh.uv_sets` / `colors` / `flags` — 68 models scanned: uv sets {1:215, 2:173, 3:2} submeshes, 118 with colours, flags {10:1, 12:95, 14:224, 46:70} | good |
| Skeleton (mskl) | matrices, hierarchy, bindpose deltas, IDs, inverted matrices, lookup table | skeleton.rs | likely good |
| Cloth (CLTH) | `DominoClothData`, `ClothDesc`, vertices, vertex driver weights, fake hierarchy | `cloth.rs` parses all of that and ports `CreateFakeHierarchy`; `Skin::from_skeleton_with_cloth` re-parents (7 models / 823 bones on the install) | closed for export/pose reparenting; cloth *simulation* not reimplemented |
| Hardpoints (MHRP) | matrix + GUID pairings | no hits | gap |
| Model STU (MSTU) | STUModel | `mstu.rs` + `Model::mstu`; offset is payload-relative, verified on 60 models | good |
| Material override / STUModel hardpoints for export | Export pipeline | custom model preview | gap |
| Export formats | .owmdl streaming LOD, .owanimclip, .seanim, .smd ref pose | no standard exports | export pipeline gap |
| Mesh processing | generation of Submesh, vertex semantic mapping | likely at least viewer | need parity test |

## 5. Materials / textures / shaders

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| Material header | shader group/source, GUIDx03A | material.rs | likely good |
| Material data | static inputs, texture table, names/hash | material.rs | likely good |
| Texture header/payload | mip count, format, surface, cubemap/array, OW2 format hack, DDS export | texture.rs decode/preview | no export chain and possibly fewer formats |
| Shader code | compressed DXBC, type | `shader.rs::parse_code` — gzip payload, sizes checked | container read; bytecode semantics out of scope (renderer) |
| Shader group | quality tiers, instance pair selection | `shader.rs::parse_group` — 184-byte header, six arrays, 100 openable groups verified | array semantics (A/B/D/F) unnamed |
| Shader instance | resources, samplers, vertex layout, buffers | `shader.rs::parse_instance` | ported from the reference; no `0x086` asset installed, so unvalidated |
| Shaderelling density | not in topic of gameplay | gameplay mostly needs materials/textures | asset-level gap only |

## 6. Map chunks

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| `teMapChunk` base and placeable structs | models/model groups, single model, entity+STU instance data, light, reflection point, text, sound, effect, sequence, area, dummy | `mapchunk.rs`: the declared kind is read (GUID bits 32..39); model group/light/sound/effect/text all resolve 100% on real data; entity fitted only | remaining: entity field typing, sequence bodies (none installed), dummy kinds |
| Map header | module file | mapchunk.rs | likely good |
| Environment/objective extraction | DataTool extract-map-envs/objectives | scene builder | higher-level extraction absent |
| Placeable STU instance data | paths | likely covered | verify |

## 7. Animation

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| `teAnimation` | scale/position/rotation keyframes, both compressed rotations, legacy position, hash/effect GUID | clip.rs decoders | likely good but verify both compression paths |
| Export clips | owanimclip/seanim | not present | export gap |
| Anim nodes dump | DataTool modes | not present | debug/graph extraction gap |
| View-model placement | measured | ow-sim/ow-scene maybe | match claim |
| Semantic clip names | none (12-hex CKeys; GUIDNames has 4 POTG names) | none | not a gap: **0 installed AnimAlias (0x014) assets**, hero alias `0x0c80000000000477` not installed; idle/reload identity is behavioral (c71/c65, operator-recognized) |
| Pose evaluation | absent (teAnimation is a decoder; no runtime evaluator) | `BlendPose` + `palette_full`; the slot is `{quat, translation, scale}` and the card P is applied | behaviour: clip quats and P translations drive (2026-10-19) |

## 8. Effects

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| Effect chunks | ECAT, ESHT, ECMP, ECSS, component chunks (effect/entity/entity-control/model/particle/ribbon/sound/voice) | no hits | major coverage gap |
| Hardpoint bindings | effect components use hardpoint GUIDs | no hardpoints | gap |
| Effect timing | `ECMP` start/duration offsets/transform/indices | none | gap |

## 9. Replay and conversations/text

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| Player replay | format version, build, map, checksum, params, highlight | none | major gap |
| Player highlight | camera/player/team data | none | gap |
| HeroData | skins/sprays/voicelines/emotes, POTG | model/skin only | game asset extraction gap |
| Game replay MP4 conversion | ReplayMp4Tool | none | major gap (different medium) |
| Display text / substrings | `teString`, `teSubtitleThing` | no hits | text/subtitle coverage gap |
| Locale pipeline | extract/list loc strings | no assets downloaded | gap |
| Conversations | preview + extract-hero-conversations | none | gap |

## 10. Render/combat/gameplay domains

| Area | OWLib | Ours (ow-sim) | Gap |
|---|---|---|---|
| Data side | class hashes only | measured movement and body integration | Ours goes beyond OWLib into sim |
| Combat | none (limited) | world/ray/hit, rules/damage | our strength |
| Movement/domino | file-side and constants | measured and extracted | our strength |
| Animation-driven movement hints | STU fields | `clip.rs` decodes 309/350 clips (3,907 tracks, 38,527 keyed frames). The reference's header struct is misaligned on this build and decodes **zero** clips | **Our strength.** The reference's `teAnimation.cs` reads `BoneCount` as the top half of our `family` word and two section offsets as `0` |

## 11. DataTool user features

| Area | OWLib | Ours | Gap |
|---|---|---|---|
| CLI modes | ~many modes from reflection | probes/viewer only | broad product gap |
| Extract models/maps/textures/audio/effects/animations | yes | partial | gap |
| Audio WEM→OGG | yes | none | gap |
| Lists (heroes, maps, unlocks, abilities, talents, game modes, rulesets, missions, progressions) | yes | not exposed as user commands | gap |
| Texture extraction | yes | PNG decode for sampled textures | gap |
| CAS encoding/index comparisons | CASCEncDump | none | gap |
| Package search/extract/info | TankPackage | none | gap |
| GUI browser | TankView (WPF) | wgpu viewer (ow-view) | different scope; preview panes missing |
| Assets export to formats | OWMDL/OWMAP/OWMAT/OWANIMCLIP OWMAP | none | export conversion gap |
| Anim clip nodes | extraction | no asset exporter | gap |

## 12. Architecture/policy

- OWLib: full .NET extraction toolchain plus GUI. Ours: focused read-only Rust sim and viewer; intentionally does not ship game data or assets.
- **OWLib simulates no gameplay and renders nothing in 3D.** Verified across its 1,426 `.cs` files: zero `FixedUpdate`/`NavMesh`, no graphics library in any `.csproj` (`TankView` is WPF with `<Image>` thumbnails), and `TankLib/Replay` parses the `prp` header then stops at `byte[] DecompressedBuffer`. Its ceiling is reading the client's *inputs*; it can never be a reimplementation of its *computation*, because it never opens the running game. Do not read the tables below as "we are behind a reference engine" — there is no reference engine.
- Our gaps predominantly reflect product scope rather than parser defects: no high-level extract/list command surface, no package workflow tools, no audio, no replay MP4, no conversation/text tools, no unlock/cosmetic/lootbox surface.
- Our strengths over OWLib: verified movement arithmetic (measured from the client's own captures, not fitted), Domino integration read out of the executable, combat trace diff, a 3D renderer and a Bevy sandbox where the reference has neither, and **224 tests against the reference's 2**.

## Priority gaps to close (recommended)
1. Add the missing per-build manifest key derivations (reference has 253 CMF + 141 TRG; we have 2 + 1) and the nearest-older-build fallback.
2. Wire map-prop material lookup in the scene — `placeable.look` is parsed but unused, so every map draw binds a 1×1 white pixel even on a fully-keyed install.
3. Read `BoneIdOffset` from the submesh descriptor and apply it to bone indices; the reference does and we do not.
4. Complete the effect chunk parsers in the container's dispatch (9 of the reference's 18 kinds have no path) and add hardpoint/cloth to the model chain.
5. Implement BC6H — the only remaining texture *format* hole, 8.5% of the install, and the only decoder we lose to the reference.
6. Walk `m_heroSpecificSettings` in the settings reader so tunables know which hero and global-vs-per-hero scope they belong to.
7. Add high-level extraction commands for packages/maps/textures/materials/animation/export formats to reach DataTool parity.
8. Add audio WEM→OGG, text/locale, replay and conversation coverage.
9. Keep policy by never committing OWLib-style generated classes or game assets.

Status: not tested for OWLib runtime; the comparison is from static source inventory of OWLib plus our own measured numbers. Our own figures come from `cargo test` and from the docs named in each row.
