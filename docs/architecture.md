# Architecture

Two inputs meet in this project: **data** from the operator's installation and
**behaviour** from the complete current-build `Overwatch.exe` image.

```text
install files (CASC)                     Overwatch.exe (Ghidra)
        |                                        |
     ow-casc  (keys -> bytes)             decompile / read
        |                                        |
     ow-stu   (bytes -> structured data)   addresses + conclusions
        |                                        |
     ow-tank  (bytes -> assets)                   |
        |                                        |
     ow-scene (map resolver -> geometry)          |
        |                                        |
     ow-client (renderer, HUD, camera) <----------+
```

- **Data** answers: values, schemas, registries, geometry, clip data.
  `ow-tank` exposes it; `ow-scene` assembles map geometry from the resolver's
  chunk set, and `ow-client` draws it.
- **Behaviour** answers: how the client computes. It is read from
  `Overwatch.exe` and recorded as an address plus a conclusion. A behaviour
  that has not been read is open, not guessed.

## Crates

| crate | role | inputs | key modules |
|---|---|---|---|
| `ow-casc` | container layer: build info, manifests, BLTE, CASC index | install | `install`, `container`, `encoding`, `index` |
| `ow-stu` | STU v1/v2 reader: instances, fields, typed values | asset bytes + class registry | `types`, `lib` |
| `ow-tank` | asset semantics: GUIDs, STU classes, models, skeletons, clips, blend trees, entities, weapons, settings, maps | asset bytes | `guid`, `classreg`, `classes`, `entity`, `blendtree`, `clip`, `skin`, `skeleton`, `model`, `hardpoint`, `fieldnames`, `keytable`, `mapresolve`, `mapchunk` |
| `ow-scene` | map geometry: the resolver's chunk set, model placeables, materials/textures, renderer-neutral vertex/index/instance buffers | `ow-tank` | `lib` |
| `ow-client` | the one launchable client: practice-range launch, `wgpu` renderer, HUD, free camera | `ow-scene`, `ow-tank` | `main`, `surface`, `hud` |
| `ow-render` | native D3D11 backend: device/swap chain, replay dispatch, state caches, engine batch records | `windows` | `device`, `frame`, `replay`, `state`, `batch` |
| `ow-sim` | differential-verified client arithmetic: mover ramp/command, Domino body, camera basis | none | `movement`, `body`, `camera` |

`ow-sim` is a workspace member now. It holds only the differential-verified
arithmetic; it is not trusted for map selection or a per-frame gameplay claim
until the control law and Domino dispatch pass the oracle
([`rebuild-plan.md`](rebuild-plan.md)).

Binary/example entry points live in `crates/ow-tank/src/bin` (`hero`,
`model`, `anim`, `stu`, `look`, `weapon` helpers) and
`crates/ow-tank/examples` (read-only probes). The canonical tool list is
[`tools.md`](tools.md).

## Runtime

`cargo run --release -p ow-client` opens the install, finds the practice-range
header by `m_mapName`, selects the first variation whose paired
`STUGameMode.displayName` is exactly `Practice Range`, and resolves that
variation's type-`0x002` scene map through `ow-tank::mapresolve`. The client
draws only the resolver's requested chunk set and the model placeables inside
it; it does not run proximity clustering or guess a chunk kind. That choice is
a **viewer default**: the current image has no client-side mode→variation
selector (its mode/variation field hashes occur only in the registration
functions), and the game's map arrives as type-`0x002` GUIDs in the level-load
message. The producer of those GUIDs is open. `--list-maps` prints every
readable header and its variation modes.

## Behaviour pipeline

1. **Open `Overwatch.exe`** in the project (`dev/ghidra_run/owmod_clean`). It is the
   complete current-build image: every `.text` page and the install's
   plaintext `.pdata`; the function census is in
   [`ghidra-project.md`](ghidra-project.md).
2. **Read the function.** Use the MCP bridge or the headless scripts in
   `docs/ghidra/`. Record the address and the conclusion in
   `docs/formats/<system>.md`; never paste disassembly.
3. **Name it.** When the role is settled, add the row to
   `docs/function-names.csv` and run `python dev/gh_apply_names.py`.
4. **Implement it** operation for operation in the verified layer, which is
   `ow-sim`: each leaf gets its harness unit before anything is composed on
   top of it.
5. **Prove it.** A pure function gets a golden vector; a stateful path gets a
   first-divergence test; a reconstructed client function gets a
   `dev/verify_harness/` unit.

## Address conventions

- The current image is a normal PE at its preferred base **`0x140000000`**.
  An address is `0x140000000 + RVA`.
- The docs prefer the **function name** from `docs/function-names.csv`; the
  CSV is the authority for its current address.
- The loader has its own image base and is not part of gameplay behaviour.
- There is one address frame. If an address is not in the current image, it
  is not a valid citation.

## Tests and verification

`cargo xtask verify` is the gate: fmt, clippy (warnings are errors), the
tests, the tool index, the deviation check and the address/function-entry
check. `--fast` leaves out the tests that read the install. Behind it:

- `cargo test --release -p ow-casc -p ow-stu -p ow-tank -p ow-scene` - data
  layer and map/client scene tests (the `*_real` tests read the install and
  skip themselves when there is none).
- `cargo test -p ow-client` - client-side camera math.
- Client functions: `python dev/verify_harness/cli.py all`; the CASC subset is
  `cli.py casc`.
- CASC/map oracle: see [`rebuild-plan.md`](rebuild-plan.md).

## Repo layout

```text
crates/     the workspace (see table)
docs/       current findings, plan and open questions
dev/        read-only analysis and verification tools (tracked); output is not
```
