# The frontend pipeline: menu -> mode -> map -> character select -> spawn

The question (operator, 2026-11-08): follow the client from the front-end menu,
through the mode selector and the practice range, to the character selector and
a spawned character, and read the logic of each stage.

## The answer in one line

The front end is **not native UI**. The menu, the mode selector, the character
selector and the custom-game browser are **web pages inside an embedded
Chromium (CEF) browser that lives in the game process**, and the page talks to
the engine over a **JSON message table with 50 numbered types** - which is now
read in full (`FUN_142ed1be0`, section 2). The page's content (playlists,
heroes, rulesets) is **bought over the network** (bnet HTTP + lobby messages).
The pipeline's native stages are two: **start the map**
(`0x14054fbe0` -> `RunMapLoadEntry 0x14055a0f0`) and **the player/entity
layer** (`GetPlayerComponentByIndex 0x14214dcc0`). Those two are where a 1:1
implementation can exist; the UI stages have no client-side *gameplay*
expression to copy, only a data contract.

## 1. The UI layer is CEF, and the client names its whole API

Read:

- the browser module name `BlizzardBrowser/` (`0x1436165e8`) and the log
  category `BNL_Browser` (`0x143615ee0`);
- the callback table's own diagnostics, in the client's own order
  (`.rdata` `0x143616040..0x143616640`): the two required names `onConnected`
  and `onDisconnect`, then the optional names `onWindowSizeRequested`,
  `onBufferTextureUpdated`, `onSharedTextureUpdated`, `onCursorChangeRequest`,
  `onPageLoadBegin`, `onPageLoadCompleted`, `onDataPost`,
  `onRegisteredFunctionCalled`, `onExternalLink`, `onScrollPositionChanged`,
  `onScrollSizeChanged`, `onTitleChanged`, `onAudioStateChanged`, `onGetZoom`,
  `onGetCookie`, `onCookiesCleared`, `onPermissionsCleared`,
  `onJavascriptDialog`, `onAssetUrlRequested`, `onBrowserReset`; three
  deprecated names are still accepted;
- the validator that reads a callback config against that list
  (`0x142ec6cb0`), its factory chain `0x142ec84f0` -> `0x142ecab00` ->
  `0x142ec6cb0` (the factory has no caller: internal linkage);
- the page configuration keys in the create-browser message: `start_url`
  (`0x143616778`), `cache_path` (`0x143616788`), `remote_debug_port`
  (`0x1436167d0`), user agent, cache/storage/plugin/console switches,
  background colour, hardware acceleration, process rate; and the URL policy
  strings "file URLs are not supported", "Invalid start URL." (`0x143615e80`
  area).

This is also why the image contains **no menu vocabulary at all**: a
case-insensitive byte search over the whole module image finds `practice` 0,
`training` 0, `workshop` 0, `arcade` 0, `hero select` 0, `mode select` 0,
`frontend` 0, and `spawn` 0. The one `.html` in the image is a MIME type in the
HTTP table (`text/plain`, `text/html`, `application/xml`), not a path. The page
ships in the install (CASC), not in the executable, and its `start_url`
producer is not read.

**The developer overlay is Dear ImGui, docking branch, shipped in the game.**
The image carries ImGui's own string table at `0x14328fa10..0x143290b30`:
`imgui.ini`, `imgui_log.txt`, `Debug##Default`, `HoveredId: 0x%08X`,
`Click to break in debugger!` (`0x143290610`), `Press ESC to abort picking.`,
the dock-node strings (`DockSpace`, `DockNode`, `DockId=%X`, `#TITLEBAR`), and
the menu/popup ID formats (`##Menu_%02d`, `##Popup_%08x`, `##MainMenuBar`).
So the client has a live ImGui debug overlay with a widget-ID picker and a
"break in debugger" button. Its *own* labels are not plaintext: the CVar name
table is encoded in this image (see `dev/fp_table.py`), so the overlay's feature
list is only readable through the client's decoder, not by a string search.

## 2. The page <-> engine contract: one router, 50 message types

`FUN_142ed1be0` (4092 bytes) is the router. It is the whole contract:

1. parse the JSON body (failure logs `Received invalid JSON.`);
2. read the field `type` (`0x143437678`) and require it to be an integer
   (tag bit `0x20`);
3. `*host == 0` selects the **service** table (host -> browser, codes
   `0x00..0x1a`); `*host == 1` selects the **client** table (browser -> host,
   codes `0x1b..0x31`);
4. dispatch on the code; each case reads its own named fields, and logs
   `Received <name> ... with missing fields.` (level 4) or
   `[On<Name>] callback not assigned.` (level 2) through the `BNL_Browser`
   logger (`0x14248aa40`/`0x14248ad50`/`0x14248b130`);
5. an unknown code logs `Unknown browser service message type. | Code: '%d'` or
   `Unknown browser client message type. | Code: '%d'`.

**Service messages (host -> browser), `*host == 0`.** Fields in the order the
handler reads them:

| code | what it does | JSON fields | handler |
|---|---|---|---|
| `0x00` | create the browser | `user_agent`, `allow_all_certs`, `enable_local_storage`, `enable_plugins`, `enable_javascript_console`, `start_url`, `show_native_scrollbar`, `background_color`, `enable_hardware_acceleration`, `browser_process_rate` | `FUN_142ed6180` |
| `0x01` | navigate | `nav_type` | `FUN_142ed6e40` |
| `0x02` | key/path | `key`, `path` | `FUN_142ed98a0` |
| `0x03` | key/json | `key`, `json` | `FUN_142eda620` |
| `0x04` | content type | `content_type` | `FUN_142eda9d0` |
| `0x05` | register asset URL | (diagnostic only) | `FUN_142ed9650` |
| `0x06` | asset URL response | `response`, `request_id`, `size`, `guid` | `FUN_142ed3d90` |
| `0x07` | cookie | `cookie` | `FUN_142edc030` |
| `0x08` | name/url | `name`, `url` | `FUN_142ed58d0` |
| `0x09` | URL allow-list entry | `entry`, `is_regex`, `flags` | `FUN_142ed2ea0` |
| `0x0a` | scope/persist | `scope`, `persist` | `FUN_142ed9c50` |
| `0x0b` | **register javascript variable** | `scope`, `value` | `FUN_142eda0d0` |
| `0x0c` | **call javascript callback** | `callback_id`, `args` | `FUN_142ed5180` |
| `0x0d` | cleanup javascript callback | `callback_id` | inline (calls `host+0x72`) |
| `0x0e` | clear cookies | (diagnostic only) | inline (calls `host+0x7a`) |
| `0x0f` | callback, no fields | - | inline (calls `host+0x82`) |
| `0x10` | set audio mute | `state`, `bar`; `Received invalid mute state. Type: '%d'` | `FUN_142edbd70` |
| `0x11` | callback, no fields | - | inline (calls `host+0x92`) |
| `0x12` | set zoom | `zoom` | inline (calls `host+0x9a`) |
| `0x13` | host | `host` | `FUN_142ed5c80` |
| `0x14` | zoom per host | `zoom`, `host` | `FUN_142edc3a0` |
| `0x15` | input/result/guid | `input`, `result`, `guid` | `FUN_142ed4d90` |
| `0x16` | x/y | `x`, `y` | `FUN_142edc6d0` |
| `0x17` | set focus | field `f` of length 1 | inline (calls `host+0xc2`) |
| `0x18` | register scheme handler | `scheme` | `FUN_142edae50` |
| `0x19` | callback, no fields | - | inline (calls `host+0xca`) |
| `0x1a` | callback, no fields | - | inline (calls `host+0xd2`) |

**Client messages (browser -> host), `*host == 1`.** Each one is an event that
fires a host callback:

| code | event | JSON fields | handler |
|---|---|---|---|
| `0x1b` | `OnReady` | - | inline (`host+0xda`) |
| `0x1c` | browser termination reason (`onBrowserDisconnectReason`) | `reason` | `FUN_142ed44e0` |
| `0x1d` | `OnCookiesCleared` | - | inline (`host+0xea`) |
| `0x1e` | `OnPermissionsCleared` | - | inline (`host+0xf2`) |
| `0x1f` | `OnPageLoadBegin` | `new_url` | `FUN_142ed8de0` |
| `0x20` | `OnPageLoadCompleted` | `browser_state` | `FUN_142ed9050` |
| `0x21` | **`OnDataPost`** (page -> engine payload) | `data` | `FUN_142ed49c0` |
| `0x22` | `OnExternalLink` | `link_type` | `FUN_142ed5580` |
| `0x23` | **`OnRegisteredFunctionCalled`** (page calls a native function) | - | `FUN_142edb0a0` |
| `0x24` | `OnScrollPositionChanged` | - | `FUN_142edb810` |
| `0x25` | `OnScrollSizedChanged` (the client's own spelling) | - | `FUN_142edbac0` |
| `0x26` | `OnTitleChanged` | - | `FUN_142edc960` |
| `0x27` | `OnAudioStateChanged` | - | `FUN_142ed4210` |
| `0x28` | `OnGetZoom` | - | `FUN_142ed5fa0` |
| `0x29` | guid | `guid` | `FUN_142ed7d50` |
| `0x2a` | `OnGetCookie` | `cookies`, `result` | `FUN_142ed7590` |
| `0x2b` | `OnCursorRequest` | `cursor`; `Received invalid cursor type. Type: '%d'` | `FUN_142ed46c0` |
| `0x2c` | asset URL request | `header_map` | `FUN_142ed3740` |
| `0x2d` | asset URL complete | - | `FUN_142ed3560` |
| `0x2e` | `OnJavascriptDialog` | `dialog_type`, `default_prompt`, `message` | `FUN_142ed86e0` |
| `0x2f` | `OnJavascriptConsoleMessage` | `source`, `line`, `message`, `method` | `FUN_142ed8160` |
| `0x30` | scheme load begin | `scheme` | `FUN_142edb550` |

**What this means for the pipeline.** A button in the page reaches native code
by exactly one of two doors: `0x21` (`onDataPost`, an opaque `data` string) or
`0x23` (`onRegisteredFunctionCalled`, a native function the page calls by
name). Everything the menu does - pick a mode, queue, start the range, pick a
hero - is therefore a *host-side* handler behind the `+0x218` / `+0x238`
slots of the browser host object, not a native menu state machine. Finding
those two filled with the game's own functions is the remaining front-end read
(queue item 18, item 1).

## 3. The content is server data, not shipped data

Read:

- the bnet API gateway protobuf pair `__OnAPIGatewayRequest` /
  `__OnAPIGatewayResponse` in the descriptor blob at `0x143146e60` (uri,
  pair_id, oauth_client_id, bnet_id, post_data, http_status_code,
  duration_ms, attempt_id): the page's data fetches are HTTP calls made on its
  behalf;
- the lobby/matchmaking log categories `LobbySystem_C`,
  `ClientLobbyMessageSystem_C`, `NetworkEventSystem_C` and the
  `ERROR_MATCHMAKING_*` codes: the mode choice becomes a lobby message;
- the **gameplay contract that remains** is the custom-game protocol,
  `game/Core/Utilities/Protobuf/CustomGame.proto`, whose serialized descriptor
  is readable in the image at `0x1435f1767` (path string `0x1435f149d`, build
  path `D:\Madness\production\Worker\2_25_0_1\...`, so this image is build
  **2.25.0.1**). Its schema is the whole "mode" vocabulary: `CGGameRuleset`
  (gameModeGUID, gameRulesetGUID, masterVars, globalAdminVars, teams, flags,
  logic, logicStr, gameModeType, mapRules, teamRules, rulesetMapPairs, ...),
  `CGOptions` (mapEnvironmentScenarioPairs, pregameOptions, isStadium,
  difficultyGUID, minimumLatencyNS, ...), `CGMapRules`/`CGNextMapRule`/
  `CGMapChangeRule`/`CGBackToLobbyRule`, `CGTeamRules` with
  `CGPerformTeamShuffleRule`, `CGGameRulesetTeam`
  (`teamIndex`, `numTeamMembers`, `heroesArrayIsWhitelist`, `heroGUIDs`,
  `allHeroBodyVars`, `controllerVars`), the Workshop-style logic graph
  `PBCGLogic*` (targets carry `team`, `slot`, `hero`), `PBCGEntityEntry`
  (pos, facing, scale, for dummy entities) and `PersistedCGSettings`
  (version, options).

**The character selector's rules are here, not in an engine selector.**
`CGGameRulesetTeam.heroesArrayIsWhitelist` + `heroGUIDs` + `numTeamMembers`
say who may play what, and `CGPregameOptions.enableHeroDraft`,
`enableHeroDraftBan`, `enableHeroDraftMirrorMatch` are the draft phase. Which
hero a given player actually picked is a UI-side value; the engine's copy of it
arrives with the spawn message.

## 4. The native tail: map load, then the player array

Read:

- the map-start sequence `FUN_14054fbe0` (695 bytes): it allocates and
  constructs the map subsystem stub (`Alloc 0x141baff30`, ctor `0x141c578c0`),
  stores the pointer **mangled** (`xor 0xd5` + a `sub`/`xor`/`ror 0x30`
  encode), then runs `RunMapLoadEntry 0x14055a0f0`, then `0x14055abc0`
  (452 bytes) and `0x14055a8c0` (754 bytes), then reaches the client object's
  `+0x240` slot via `[[0x143b796a8] + 0x240] -> 0x1406c1bc0`, then an
  **obfuscated tail** (opaque predicates and a fake epilogue), so the sequence
  is read and the tail's meaning is not;
- `0x14055a0f0` is already documented as the map-load entry (`formats/maps.md`,
  `formats/static-collision.md`): it registers the decoded CVar
  `debug.showMapLoadAfterMS`, runs `ConstructEngineSubsystems 0x140554220`, and
  builds the map-load camera-view world at `0x140be1570`;
- the boot path into it: `FUN_14053a080` (61 bytes) calls `0x140550870`,
  `0x141bbe310` and `FUN_14054fbe0`. Ghidra reports `0x140540fd0` (`entry`,
  25 bytes) with no callees and `FUN_14053a080` with no callers, so the last
  hop is indirect or under-documented - not a logic finding;
- the player layer: `GetPlayerComponentByIndex 0x14214dcc0` (64 bytes, **640
  call sites** - the accessor is named but inlined everywhere). Entity
  `+0x1a0` -> `+0x290` -> `{+0x60 array, +0x68 flag}`, index from
  `entity+0x1a8`, stride 8. Players are an indexed component array; the
  640 readers make this the seam any spawn work has to go through.

The repo already knows the hero -> entity half of the spawn (row 16,
`formats/stu.md`: hero STU -> gameplay entity -> schema entries) and the
movement half (row 27: the walk, with the viewer's start position taken from
the practice map's spawn-room trigger volume, floor `y = -4.006`). What is
missing is the **position and hero of a spawned player as the client computes
them**, i.e. the level-load message's producer (queue item 11) plus whichever
of the 640 sites is the spawn write.

## 4a. The selector's roster: the mode's hero pool (read 2026-11-09)

**The question this answers:** when a mode is entered, what decides which
heroes the selector offers? Not an engine screen - the mode's own data.

Read:

- `STUGameMode` (`0x494C33C4`) carries `m_teams` (`0xD440A0F7`), an inline
  array of `STUGameModeTeam` (`0xD797394C`). Each team carries
  `m_availableHeroes` (`0x59C86C8D`, `+64`), an embedded instance of
  `STU_5427ADC1`. `STUGameRulesetTeam` (`0x87C00EC0`) carries the same object
  at `+56`; no ruleset-team instance occurs in any offline asset (the mode's
  `gameRulesetSchemas` name `STUGameRulesetSchema` assets, and a full
  `STUGameRuleset` is server data), so that half is declared, not measured.
- `STU_5427ADC1` (`0x5427ADC1`, 88 bytes) is the restriction object: skin
  themes (`0x62E537BD`), `0x6183A698`, and three per-hero inline arrays at
  `+40`/`+56`/`+72` whose entries all carry a hero (`0x71DBD5E9`) plus an
  unlock (`0x69196615`, `0x967E7BD4`) or an identifier (`0x1F753B44`).
- `STU_D1DC7C55` (`0xD1DC7C55`, 88 bytes) declares nothing: the **empty
  marker**, every hero selectable. `STU_3C14F504` (`0x3C14F504`) and
  `STU_C45DE560` (`0xC45DE560`), 104 bytes, add `m_heroes` (`0x056D3E39`,
  `+88`): the whitelist.
- All four classes are also **runtime component types**, registered as one
  consecutive stub block, each calling `RegisterComponentTypeDescriptor` with
  the class hash and its size, 8-byte aligned: `0x1402dc690` (base, `0x58`),
  `0x1402dc790` (marker, `0x58`), `0x1402dc890` (whitelist `0x3C14F504`,
  `0x68`), `0x1402dc990` (whitelist `0xC45DE560`, `0x68`). Their interface
  tables are `0x143e79a90`, `0x143e79bf0`, `0x143e79eb0`, `0x143e79d50`; their
  constructors `0x141342c50`, `0x141342b60`, `0x141342bb0`, `0x141342c00`. So a
  whitelist reaches gameplay as a `0x68`-byte component with `m_heroes` at
  `+0x58`.

The roster is now read as data (`ow_tank::hero_select`, tests in
`crates/ow-tank/tests/hero_select_real.rs`). Measured on this install:

| mode | asset | teams |
|---|---|---|
| Practice Range | `0x0230000000000018` | two teams, **both the empty marker**: every hero |
| Escort | `0x023000000000011e` | both teams whitelist the same 29 heroes |
| Stadium | `0x0230000000000143` | team 0 whitelists 33, team 1 the empty marker |
| Mei's Snowball Offensive | `0x0230000000000008` | both teams whitelist exactly Mei (`0x02E00000000000DD`) |

Every mode team on this install answers with one of the three cases above; a
pool class the reader does not know fails the test rather than being guessed.
Every mode whitelist in this install is `STU_C45DE560`; `STU_3C14F504` is
declared and registered as a component type but no mode team uses it (its
whitelist is reached through the ruleset team or a server-sent custom game).

**Still open.** Who *consumes* the component (the interface tables above are
the seam), the pick itself (a page value, section 5), and the write that turns
a picked hero into a spawned player (section 6, item 3).

## 4b. The hero the mode applies: runtime variable 940 (read 2026-11-09)

The roster says who *may* be picked; this says who *is* applied, and it is in
the mode's own script, not in a table and not in the page:

- `STUGameModeTeam.m_controllerScript` (`0xA2781AA4`) and `m_bodyScript`
  (`0x76E8C82A`) are asset refs to `STUStatescriptGraph` (class `0x7236F6E3`,
  asset type `0x01B`). The practice-range mode's teams run controllers
  `0x05800000000013c1` and `0x0580000000000c90` and share body
  `0x0580000000000c8e`.
- Each controller graph carries exactly **one** `STUStatescriptStateHero`
  (`0x91428992`). Its `hero` input (`0x37AB13D3`) is a `STUConfigVarDynamic`
  (`0x4B093107`) whose `m_identifier` (`0x4D2DB658`) is **940** in both teams'
  controllers; its `controller` input (`0x176ED263`) is a
  `STUConfigVarEntityID` (`0xB5A0CAF0`). The body graph has no such node.
- The state is a runtime component type: stub `0x140402690` registers
  `RegisterComponentTypeDescriptor` class `0x91428992`, size `0xF0`, handle
  `0x2CFABF7924BC00A5`, ctor `0x1417afc20`, field adapter `0x1417b0250`
  (`0x176ED263` -> the controller, `0xA228776` -> an int) and interface table
  `0x14404EEC0`.
- **Install survey (whole install, one walk):** 137 type-`0x0C5` modes name 35
  distinct team controller graphs; 13 modes have a hero-setting node; **12
  nodes read runtime variable 940** and exactly one writes its hero into the
  graph instead ("Assault" `0x023000000000016b`, hero `0x02E0000000000009`).

So the mode never looks its hero up in `m_availableHeroes` and the selector
never hands the mode a value directly: the script reads **one runtime
variable**. What binds variable 940 to a hero — the client's dynamic-variable
table and the store that writes it — is the remaining read, and that store is
the pick. Read and pinned by `ow_tank::graph::hero_states_of` and
`ow_tank::hero_select::mode_team_scripts`, tests in
`crates/ow-tank/tests/hero_pick_real.rs`.

## 4c. The hero asset's own fields: name, playable marker, movement row (read 2026-11-14)

The roster says which hero assets a mode offers; the asset itself says how to
name one and what the walk applies:

- **The name.** `STUHero.m_0EDCE350` (class offset `48` in the reference's
generated `STUHero.cs`, an untyped `teStructuredDataAssetRef`): a display
string asset of type `0x07C`. Its payload is `teString`: two header bytes, then
the text up to the final NUL. `ow_tank::hero_select::hero_name` decodes it and
strips the trailing space and the non-breaking space — the same two characters
the reference's clean-name helper strips (`DataTool/Helper/IO.cs`,
`GetCleanString`) — and nothing else. The install reads names like `Tracer`,
`D.Va`, `Lúcio`, `Torbjörn`, `Soldier: 76`; all 54 playable heroes on this
build name themselves and no two names collide after case folding.
- **The playable marker.** `STUHero.m_64DC571F` (class offset `904`, one byte)
is the marker the reference calls `IsHero` (`DataTool/DataModels/Hero/Hero.cs:53`).
An open mode's roster is every type-`0x075` asset; the practice range's is
**135 hero assets, 54 playable and 81 NPC**. The marker and the name are *not*
the same split: the range's `Training Bot` (asset index `363`) is an NPC hero
with a name, so a player picker filters on the marker, not on the name.
- **The movement row.** The hero's `m_gameplayEntity` (`0x950BBA06`) carries the
`STUCharacterMoverComponent` (`0x6EFDBD73`). `ow_tank::entity::MoverRow` reads
the named floats at the class's own offsets: forward `0x19452F15` (`0xB0`; 5.5
common, 6.0 for Tracer/Genji/Anran here), side `0xCFD3E635` (`0xA8`; equal to
forward), back `0x61D6B4E9` (`0xAC`; 4.95, 5.4 for the 6.0 trio), crouch
`0x0DDD18C3` (`0xBC`; 3.0) and terminal fall `0xB74245E8` (`0xEC`; 30.0). The
client's runtime slot is STU offset + 4
([`character-controller.md`](character-controller.md)); the unnamed
`0x031FE7EE` scalar (`0xDC`) is 5.5 for every playable hero and is *not* the
forward row.

Tests: `crates/ow-tank/tests/hero_select_real.rs` reads the whole roster of
four modes, checks every playable hero's name, marker and mover row, and pins
Tracer at 6.0 m/s plus the Training Bot as a named non-playable hero.

### The viewer's select stage (`ow-client`, 2026-11-14)

The client's pick is a page value and nothing in the image turns it into a
spawned character (section 6, item 3; section 4b). `ow-client` therefore adds a
**declared viewer default**, in the same class as its spawn volume:

- `--list-heroes` prints the practice mode's roster: the mode's own pool, the
  playable marker's filter, each hero's name and forward speed.
- On a live launch with a terminal and no `--hero`, the roster is printed and
  one line is read: a hero name (case, spaces and punctuation folded, so
  `D.Va` and `DVA` match), an asset index, or empty for no hero.
  `--hero <name|index>` names the pick directly; `--no-hero` skips the prompt;
  a name or index outside the mode's roster is an error, never a default.
- The read consequences are applied: the scene places the picked hero's **own
  first-person model and weapons** (the hero asset's view-model entity, via
  the scene's `--hero <index> --view-model --first-person`), the walk's wish
  magnitude is the hero's forward base speed, and the fall clamp is the hero's
  terminal fall. The view model is re-placed at the camera's own transform
  every frame (the unit test
  `camera_tests::the_view_model_frame_is_the_camera_frame_at_every_pose`
  pins that frame), and its weapons by the FP skin's bind-pose bone frame.
  The client's first-person **animation** is not read, so the arms are drawn
  in bind pose (`FIT-ANI-02`/`FIT-CAM-06` still apply to the framing).
- The pick itself is not claimed: what the client does with the hero id after
  the spawn, and what writes runtime variable 940, remain open.

## 5. Stage verdicts

| stage | where it lives | verdict |
|---|---|---|
| menu shell | CEF page + callback table (`0x142ec6cb0`, names at `0x143616040`) + the 50-type router `0x142ed1be0` | **contract read in full.** No native menu behaviour to port; the page ships in the install. `start_url` producer **open** |
| mode selector | UI + bnet gateway protos (`0x143146e60`) + lobby systems; the mode's data contract is `CustomGame.proto` | **out of scope as network** (`systems-coverage.md`, "Not in scope"); the *schema* is read, the lobby protocol is not, the client-side consumer of `CGGameRuleset` is **open** |
| practice range (map load) | `0x14054fbe0` -> `0x14055a0f0` -> `0x14055abc0` / `0x14055a8c0` | **sequence read**, entry body read and documented; the two follow-up bodies read only by their callees, tail obfuscated, so **partly open** |
| character selector | the roster is the mode's `m_team`\[\].`m_availableHeroes` (`0x59C86C8D`): empty marker (`0xD1DC7C55`) = every hero, whitelist (`0x3C14F504`/`0xC45DE560`, `m_heroes` `0x056D3E39`) = those heroes; the hero asset names itself (`m_0EDCE350`, type `0x07C`), marks itself playable (`m_64DC571F`) and carries its movement row through `m_gameplayEntity`; the mode applies the hero through `STUStatescriptStateHero` (`0x91428992`) from runtime variable `940` | **roster, names, marker and movement row read and tested** (`ow_tank::hero_select`, `ow_tank::entity::MoverRow`, `ow_tank::graph::hero_states_of`); the viewer stage is `ow-client --hero`/prompt; what writes variable 940 is **open** |
| character spawn | `GetPlayerComponentByIndex 0x14214dcc0` + the hero->entity path (row 16) + the map's start position | **open** on the position/hero producer; the *entities* and the *movement* halves are read and implemented |

## 6. What is still open

1. **The two host slots.** `+0x218` (`onDataPost`) and `+0x238`
   (`onRegisteredFunctionCalled`) are the only doors from the page into the
   game. Whoever writes those two slots is the front end's gameplay entry. The
   callback table's filler (`0x142ec6cb0`) takes them from a config object, so
   the next read is the *producer* of that object, not the validator.
2. `0x14055abc0` and `0x14055a8c0` (452 / 754 bytes) are read only by their
   callees; the map-start tail is obfuscated.
3. The write side of the player component array - who sets a player's position
   and hero - is not found. The accessor is inlined at 640 sites, so caller
   filtering cannot find it; the work is a store census.
4. The character selector's **rules are read as data** (section 4a): the
   mode's per-team `m_availableHeroes` is either the empty marker
   (`STU_D1DC7C55`, every hero) or a whitelist (`STU_3C14F504` /
   `STU_C45DE560`, `m_heroes`); both classes are runtime component types. The
   practice range is the empty case, Mei's Snowball Offensive is a one-hero
   whitelist. Section 4c adds the hero asset's own name, playable marker and
   movement row, and `ow-client` now offers a picker that applies those read
   values. The **pick** itself is still a page value, and the write that turns
   it into a spawned character is not found; those remain open, and the UI is
   still not faked.

## 7. Method notes (why the negative results are trustworthy, and their limits)

- **Project change (2026-11-08, operator-approved).** The router was split
  across three `.pdata` fragments of one logical function,
  `0x142ed9c50..0x142eda0ca`; `FUN_142ed9c50` (0x158), `FUN_142ed9da8` (new,
  created from its `.pdata` start at `0x142ed9da8`) and `FUN_142eda0d0`
  (0x549) now hold it, and the program was saved. The decompiler shows them
  with unrecovered registers (`unaff_*`, `Removing unreachable block`), which
  is a fragment artifact, not logic: every conclusion above comes from the
  switch cases, the field names and the callback slots, all of which are
  explicit.
- The vocabulary absences in section 1 are whole-image byte searches over the
  module image. They are evidence that the client does not *name* these stages
  natively, not proof that no code implements them: a name can be built at run
  time.
- The browser/UI literals live in **two** places: a run inside `.text`
  (`0x142ed9xxx..0x142f2c000`, which is why a "strings in `.rdata`" census
  misses them) and the `.rdata` block `0x143615e80..0x143617700`. A string
  census that only reads `.rdata` half-misses this layer.
- The router has **no caller** in Ghidra: it is handed to the browser object as
  a callback, which is consistent with the callback table read in section 1.
- `GetPlayerComponentByIndex` having 640 callers means caller lists are useless
  for locating the spawn: the accessor is inlined by name. Size, offset and
  store-site work is required instead of caller filtering.
- Nothing here is a decompile excerpt: every claim is one of (a) a function
  entry address with its callee list and byte size, (b) a string's address, or
  (c) a `.pdata` extent. Everything else is named as open.

## 8. Where this goes next (queue)

1. Find the producer of the browser host's callback config - the functions
   stored at `+0x218` and `+0x238` of the host object (item 1 above). This is
   the front end's actual gameplay entry.
2. Read `0x14055abc0` and `0x14055a8c0` bodies (stage 3's two unknown steps).
3. Find the write side of the player component array (stage 5's position and
   hero), starting from store sites rather than calls.
4. The selector's roster, names, playable marker and movement rows are read
   (sections 4a/4c, `ow_tank::hero_select`, `ow_tank::entity::MoverRow`), and
   `ow-client` applies them as a declared viewer default. What is left is the
   component's consumer, the page pick's binding (variable 940's writer), and
   the spawn write; the roster no longer blocks any of them.
