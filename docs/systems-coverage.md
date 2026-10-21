# Coverage: every client system, at a glance

One line per system: its state and the document that carries the findings.
The full evidence ledger - every decompiled path, address and open anchor,
verbatim - is [`history/coverage-detail.md`](history/coverage-detail.md).
"Read" means a decompiled path with recorded addresses, not a full
reimplementation.

| # | system | state | where |
|---|---|---|---|
| 1 | CASC container, manifests, BLTE | read and verified; remote fetching (opt-in `OW_CDN`); one known parser off-by-one, left as a decision | `formats/casc.md`, `formats/manifest.md` |
| 2 | Structured data (STU v1+v2), class registry | read end to end; typed layer over the full class registry; v1 implemented (synthetic tests only) | `formats/stu.md`, `formats/entity-schema.md` |
| 3 | Chunked assets, models, map chunks | read; skeleton/`MSTU` parsed; placeable kinds resolve, entity layout fitted | `formats/materials.md`, `formats/mstu.md`, `formats/maps-streaming.md` |
| 4 | Materials and textures | chain read and partly joined; textures decode (BC1-7, BC6H) with an external decoder oracle; the `0x008` keyring wall limits the covered subset | `formats/materials.md` |
| 5 | Statescript graphs, damage nodes | nodes, literals, runtime graph and vars read; per-class code is metaobject only; node type index source still open | `formats/statescript.md`, `formats/weapons.md` |
| 6 | Weapons, ballistics, volley | file side and class surface read; volley literals labelled; damage stays supplied | `formats/weapons.md` |
| 7 | Entity/component system | router mapped and fully paired (descriptors, sizes, fields, parents); TankLib naming only | `systems-plan.md` (Phase 4) |
| 8 | Character mover (data) | parameters read; horizontal/vertical arithmetic measured from keyed traces and verified | `formats/character-controller.md` |
| 9 | Mover update path | chain read; concrete class registration path read; the control itself is **tier 3** (obfuscated) | `formats/character-controller.md` |
| 10 | Domino physics | solver chain, integration order and dt read (1/30 s steps, commanded step count) | `formats/domino.md` |
| 11 | UI, menus, HUD | ImGui (docking) statically linked; window-list pass read | `formats/ui-menu.md` |
| 12 | Input | var registry and handler names read; handler update/command frames open | `formats/input-camera-audio.md` |
| 13 | Camera | var surface read; FP projection/pass implemented; per-frame consumer and world near/far open | `formats/input-camera-audio.md`, `formats/animation.md` |
| 14 | Audio | Wwise statically linked; metadata/WEM container read; decoding not started | `formats/input-camera-audio.md`, `formats/sound.md` |
| 15 | Animation | clips, pose path, blend tree and FP retarget measured; tree implemented; Pose2d VM and FP camera consumer open | `formats/animation.md` |
| 16 | Maps and streaming | data read and the map-load code path read; streaming policy open | `formats/maps-streaming.md` |
| 17 | Rendering pipeline | data surface read; the viewer draws textured, skinned models; animated draws and a real shading model open | `formats/rendering.md`, `formats/lighting.md` |
| 18 | Shaders | containers read; bytecode semantics out of scope by design | `formats/shaders.md` |

System states are also maintained in `../STATUS-handoff.md` ("Where we
are") and the milestone history in `../MODLOG.md`.
