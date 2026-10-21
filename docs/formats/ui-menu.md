# The UI layer: ImGui, and the menu strings it draws

The client's menus and HUD are built on **statically linked dear imgui with
docking** — not a bespoke UI toolkit and not a data-driven UI format. This was
established from the capture by three independent checks, all reproducible:

1. **The hash function in the client is `ImHashStr`.** The routine that opens
   the nav-windowing list (`FUN_7ff7c6a17ed0`, v2b; range
   `0x7ff64bb57ed0`; exec `0x7ff7551f7ed0`) hashes `"###NavWindowingList"`
   with imgui's exact algorithm: seed `0xffffffff`, a `###` run resets the
   seed, `h = (h >> 8) ^ table[(h & 0xff) ^ c]` per byte, and the result is
   complemented (`~h`) before use.
2. **The table is the CRC32 table.** v2b `0x7ff7c80b1170` (range
   `0x7ff64d1f1170`) is byte-exact the standard CRC32 table (poly
   `0xEDB88320`), which is the table imgui's `ImHashStr` uses.
3. **The literals are imgui's own.** Adjacent in `.rdata` (v2b
   `0x7ff7c80b1848..0x7ff7c80b1890`): `(Popup)`, `##MainMenuBar`,
   `(Main menu bar)`, `(Dock node)`, `(Untitled)`, `###NavWindowingList` —
   the window-class names imgui's docking code uses for the main menu bar,
   dock nodes, popups and untitled windows.

## What the routine does

`FUN_7ff7c6a17ed0` is the per-frame **window-list/layout pass**:

* it caches the hashed id of `###NavWindowingList` on the UI global
  (`DAT_7ff7c8fd5df0` + `0x2090`) the first time it runs;
* it sizes and pushes a clip rect on the same global (fields around
  `+0x1dd4..+0x1e18`, flags at `+0x1dc0` bit `0x10` and bit `1`), then calls
  the `Begin("###NavWindowingList")`-shaped entry (`FUN_7ff7c6a021b0`);
* it walks the window array (count at global `+0x1cc8`, array at `+0x1cd0`)
  and for each window derives its display name from its flags: popup
  (`(Popup)`), main menu bar (`strcmp(name, "##MainMenuBar")` →
  `(Main menu bar)`), dock node (`(Dock node)`), else `(Untitled)`, and calls
  the per-window entry;
* it then applies a **UI attribute table** at v2b `0x7ff7c80b1570`: records of
  stride `0xC` = `{int type, int subtype, u32 offset}`; the paths seen are
  `type=8, subtype=1|2` writing one float (or a float2 at `+0x1638`/`+0x163c`)
  into the config object — i.e. the client's own control-property application,
  keyed by field offset rather than by name.

The menu/HUD layer is therefore a UI *layout* problem against imgui widgets,
and a specific control can be found by its `##` id (`##MainMenuBar` for the
menu bar). The mode text the earlier rounds hunted (`Practice Range`) is not a
UI literal — it lives in the statescript/UI graph assets, which is consistent
with it being absent from the module strings.

## The animation system's entry points

The scan for animation anchors (all one-reference strings in module data)
gives the registration sites of the animation subsystem rather than its pose
code:

| anchor (v2b) | address | what it is |
|---|---|---|
| `"SKELETON"` | `0x7ff7c87f10f4`, ref at `0x7ff7c68806b0` (exec `0x7ff7550606b0`) | skeleton debug/display config |
| `"camera.FirstPersonAnimationWeight"` (+ the whole first-person camera animation block at `0x7ff7c8695814..0x7ff7c8695ebc`) | ref at `0x7ff7c52c252e` | camera-animation config registration |
| `"model for the first person animation"` | `0x7ff7c869de84`, ref at `0x7ff7c53aba3e` | view-model selection config |
| animation debugger / blend-tree / proxy-retarget block | `0x7ff7c87f1a1c..0x7ff7c87f23fc` | knob names: `'slot outro'` additive animations, animation debugger display, bind-pose idle |
| `"Enable domino procedural animation. i.e. ragdoll and (domino) IK"` | `0x7ff7c87f278c` | the ragdoll/IK switch tying animation to the Domino physics of `domino.md` |

Nearby data worth keeping: the telemetry protos carry the build provenance
`D:\Madness\production\Worker\2_25_0_0\ProTelemetry\generated_protos\...`, and
the lobby/network layer is named by its debug strings
(`LobbySystem_C::LobbyConnectHelloErrorHandler`,
`ClientLobbyMessageSystem_C::__LobbyServerConnectionUp`, `lobby_profile_oauth_prod.json`,
`https://toolstrayapi.prod.team4.dev.cloud.blizzard.net/api/LobbyAdvertise`).

## Method and limits

The UI routine was decompiled twice, by two independent stacks — the Ghidra
project (`module_v2full.bin`, v2b coordinates) and REA over a minimal PE built
from the capture region (`dev/mkpe.py`, range coordinates) — and the two
readings agree on every structure and string. One reference per anchor was
found by RIP-relative scan of the module image; the scan covers code bytes
present in the capture, so a site in an uncaptured region would not appear.

Not yet read: the imgui *widget* set the game registers (the game-specific
controls behind the `##` ids), the map/streaming code path, and the animation
pose evaluation (`MSKL` gives the skeleton table; the evaluator that consumes
it is still to be located). Those are the next anchors from here.
