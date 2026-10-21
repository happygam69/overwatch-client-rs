# The content surface: unlocks, cosmetics, loot boxes, achievements

What a player owns and how they get it: the largest body of data in the
installation that nothing here read before this. It is worth reading for its own
sake and for one join in particular — a skin theme names the models a cosmetic
swaps in, and that is the first half of the only route from "this cosmetic
exists" to "this is what it looks like".

Reader: `crates/ow-tank/src/unlocks.rs`. CLI: `cargo run -p ow-tank --bin
content` (no arguments prints the usage). Test: `cargo test -p ow-tank --release
--test unlocks_real -- --nocapture`.

## How the numbers here were established

Every claim below carries one of the evidence classes in `docs/provenance.md`:

* **install** — measured by running a reader over this installation's bytes. The
  census and every resolution count are this class.
* **published-check** — the structure is declared by overtools/OWLib, an
  MIT-licensed *published reconstruction*. It is a claim about the game made by a
  third party, not a measurement. Where this document says a hash was
  **confirmed**, the class hash was also seen as a real asset's root instance on
  this install, or the field was seen in a real payload and read.
* **assumed** — inferred here. There are two such places and both are named.

The reference install measured is the retail Windows build at
`E:\Overwatch`, manifests `Win_SPWin_RDEV_EExt.cmf` plus
`Win_SPWin_RDEV_LenUS_text_EExt.cmf`. Asset type ids come from
`TankTonka/Models/Common.cs`, the reference tooling's own enumeration.

## The measured census

Full walk, no cap (`ow-tank-content --types --scan 100000`):

| type | name | registered | readable | parsed | not downloaded | instances | root classes |
|---|---|---|---|---|---|---|---|
| `0x058` | progression unlocks | 56 | 45 | 45 | 11 | 45 | `0x8CE1D110` ×45 |
| `0x062` | stat | 2,918 | 0 | 0 | 2,918 | 0 | — |
| `0x063` | catalog | 43 | 20 | **0** | 23 | 0 | — |
| `0x068` | achievement | 164 | 0 | 0 | 164 | 0 | — |
| `0x0A5` | unlock | **19,122** | **0** | 0 | 19,122 | 0 | — |
| `0x0A6` | skin theme | 15,395 | 1,752 | 1,752 | 13,643 | 23,449 | `0xEF85B312` ×1,707, `0x42270D59` ×45 |
| `0x0C8` | ranked season | **0** | 0 | 0 | 0 | 0 | — |
| `0x0CF` | loot box | 11 | 2 | 2 | 9 | 2 | `0x56B6D12E` ×2 |
| `0x0D0` | voice conversation | 2,440 | 447 | 447 | 1,993 | 1,954 | `0xA408D74F` ×447 |
| `0x103` | customizable skin theme | 31 | 31 | 31 | 0 | 408 | `0xEF85B312` ×31 |

**The biggest content type on this install is not readable.** 19,122 unlock
assets are in the manifest registry and *none* of them is downloaded
(**install**). That is not a reader failure — `data.bytes_of` returns an error
for every one of them, the same as it does for 13,643 of the 15,395 skin
themes. The unlock reader is therefore **not tested against real data**; see
§ "What is not tested".

**Ranked season ships no assets at all on this install** (**install**): type
`0x0C8` is absent from the registry. The reader exists and is exercised only by
synthetic layout, so treat it as **not tested**.

## Unlocks

### The subclass is the type

An unlock asset holds one instance, and its *exact* class says what it is. The
mapping is `unlocks::unlock_kind_of`, transcribed from
`DataTool/DataModels/Unlock.cs:218` (**published-check**). Twenty-eight classes
are named; all twenty-eight are **confirmed**, in the sense that the reader's
table is pinned by a unit test and every one of them is a class the reference
tooling declares.

The mapping is over exact classes, not subtrees, and the order is load-bearing
once: `STU_A458D547` (a souvenir) derives from `STUUnlock_Emote`, so testing the
emote class first would call every souvenir an emote. The reference tooling says
the same in a comment at `Unlock.cs:222`, and `unlocks.rs` has a unit test
(`a_souvenir_is_not_an_emote`) so the ordering cannot be quietly broken.

The three intermediate classes are **not** in the mapping, and that is faithful:
`STUUnlock` (`0x6760479E`), `STU_3021DDED` (`0x3021DDED`) and `STU_895A2A7A`
(`0x895A2A7A`) are all real shipped shapes whose root the reference tooling's
switch does not list, so this reader calls them `Unknown` too
(**published-check**, and the test pins it).

### What each unlock carries

Confirmed on this install only by the *structure* being read; no unlock bytes
were available, so every unlock field below is **published-check** and
**not tested against real data**:

| field | class | what it is | confirmed |
|---|---|---|---|
| `0xB48F1D22` | `STUUnlock` | `m_name`, a display-text **asset**, not a string | decl only |
| `0x3446F580` | `STU_3021DDED` | `m_3446F580`, the description text (`Unlock.cs:131`) | decl only |
| `0x53145FAF` | `STU_3021DDED` | `m_53145FAF`, the "available in" text (`Unlock.cs:129`) | decl only |
| `0xBB99FCD3` | `STU_3021DDED` | `m_rarity`, `STUUnlockRarity` | decl only |
| `0xA7B393BF` | `STUUnlock` | `m_A7B393BF`, `Enum_5A7EBE59`: a **named** category | decl only |
| `0xBEE9BCDA` | `STU_3021DDED` | `m_BEE9BCDA`, the gallery-filter category list | decl only |
| `0x00B16A0B` | `STUUnlock` | `m_00B16A0B`, a Battle.net **product id** (`Unlock.cs:133`) | decl only |
| `0x4C167404` | `STUUnlock_SkinTheme` | `m_skinTheme`, the skin a skin unlock grants | decl only |
| `0x37AB13D3` | `STU_C3C6FD9E` | `m_hero` | decl only |
| `0xE2A492C7` | `STU_3F17D547` | the parent skin unlock of a mythic customization | decl only |
| `0xA0D4ABDE` | `STUUnlock_Emote` | `m_emote`, the `STUAnimAlias` | decl only |
| `0x2E5FBA99` | `STUUnlock_Pose` | `m_pose` | decl only |
| `0xA4A66AB6` | `STUUnlock_PortraitFrame` | `m_rankTexture` | decl only |
| `0x7AB4E3F8` | `STUUnlock_LootBox` | `m_lootBoxType`, `Enum_BABC4175` | decl only |
| `0x2F922165` | `STUUnlock_LootBox` | a rarity that **replaces** `m_rarity` (`Unlock.cs:162`) | decl only |

`Enum_5A7EBE59` (`TankLib/STU/Types/Enums/Misc.cs:2372`) is the one field worth
emphasis: it names an unlock's category — skin, emote, voice line, weapon charm,
name card, loot box, battle pass — *without* depending on the subclass, so an
emote and a skin are comparable without knowing either one's class. **Not tested
against real data.**

### No price is recoverable, and that is a finding

`Unlock::price()` returns `None` unconditionally, and there is nothing to
recover it from on these classes (**published-check**): the money-adjacent
fields are `m_00B16A0B`, which the reference tooling calls a Battle.net *product
id* — a catalogue identifier, not an amount — and `m_A7B393BF`/`m_A649130B`,
which are a category and an unnamed enum. Prices live in store data this project
does not read. The method exists so the gap is a documented `None` with a unit
test (`a_price_is_never_invented`) rather than a field a caller goes looking for.

## Skin themes — the part that matters for the renderer

### What is in the asset

`STUSkinTheme` (`0x42270D59`) and the mythic `STU_EF85B312` (**confirmed**: both
are real roots, 45 and 1,707 assets of type `0x0A6` respectively, plus all 31 of
type `0x103`).

Measured on a strided 120-asset sample of type `0x0A6` (**install**):

| what | measured |
|---|---|
| assets sampled / parsed | 12 / 12 (11 mythic, 1 plain) |
| `m_internalName` read as text | **0** |
| weapon-slot skins (`m_BECFBDBA`) read | 48, **48 resolve in the registry** |
| `m_weaponType` raw values | `{1: 12, 2: 12, 3: 12, 4: 12}` — golden, jade, galactic, crimson wolf |
| **distinct** weapon-slot skin GUIDs | **4**, all of asset type `362` (`0x16A`) |
| swapped-in models (`m_BCC55571`) read | **0** |
| portraits (`m_ECCC4A5D`) read | **0** |
| team variants (`m_70D5F086`) | 168 elements, **336 of their references resolve** |
| mythic slots (`m_942A6CCA`) | 43; `m_id` and `m_displayText` present on all 43, `m_texture` on **0** |
| `m_57CE9041` item counts per slot | `{0:11, 32:11, 64:8, 72:3, 96:1, 104:7, 112:1, 120:1}` |

Four findings come out of that, and three of them are negative:

1. **Most of the declared fields are not in the payload at all.** One plain theme
   (`0x0a500000000048ce`, **install**) carries five fields out of the ~21 its
   class declares: `m_16051DC7`, `m_resourceKey`, `m_5FC3164C`, `m_70D5F086`,
   `m_E9DA85BB`. `m_A5C9CA02` (internal name), `m_BCC55571` (models) and
   `m_ECCC4A5D` (portrait) are simply absent. This is the data pipeline dropping
   default-equal values — the same trap `settings.rs` documents for hero settings
   — so an absent field means "the class default", and a reader that treats a
   missing field as an error would fail on most themes. **This is why the model
   list is empty: `m_BCC55571` is not present, not misread.**

2. **`m_5FC3164C` is *not* a per-theme weapon restyle.** All 48 references
   resolve, and the typed layer agrees with the raw reader on 16 of 16 sampled
   elements, so the read is right — but there are only **four distinct GUIDs** in
   the whole sample and they are the same four on every theme, one per weapon
   variation tier (golden, jade, galactic, crimson wolf). This is consistent
   with the reference tooling's own use of the field: `SkinTheme.cs:179` calls
   `GetReplacements(heroWeaponThing.m_BECFBDBA)` on it, i.e. it is a *tier*
   reference to resolve through the resource graph, not a per-skin asset
   (**published-check**, consistent with **install**).

3. **The mythic customization slots are real and they are the interesting part.**
   43 slots across 11 mythic skins, every one carrying an identifier and a
   display-text asset, with 32 to 120 items each. `Unlock.cs:200` walks exactly
   this list (`m_942A6CCA` → slot → `m_id` → `m_57CE9041` → item) to name a mythic
   customization option, which is what confirms the hash chain (**published-check**).

4. **Asset type 362 (`0x16A`) is not in the reference tooling's asset-type
   list.** `TankTonka/Models/Common.cs` has no `0x16A` entry, so the four
   weapon-tier assets are a type the reference tooling does not name (**install**).

### The skin join that is still missing

`SkinTheme` gives the models, the portrait, the team colours and the per-tier
weapon references. It does **not** give the materials or the body textures, and
it cannot: those are not in the asset. The reference tooling builds them from the
**resource graph's skin block**, which maps a base asset to the asset that
replaces it for a given skin (`DataTool/SaveLogic/Unlock/SkinTheme.cs:236`,
`GetReplacements`, reading `m_skins[skinGUID].m_assets` →
`(m_srcAsset, m_destAsset)`) (**published-check**).

`crates/ow-tank/src/trg.rs` parses the resource graph's **package** block and
not its **skin** block, so that join is open. Until it is read, cosmetics cannot
be made visible: the model can be placed and nothing can be painted on it. This
is the concrete next step, and it is a `trg.rs` change.

## Loot boxes

### The pool is not in the box

`STULootBox` (`0x56B6D12E`, **confirmed**: both readable assets of type `0xCF`)
carries the *presentation*: `m_chestEntity` (`0xCBE2DADD`), `m_baseEntity`,
`m_idleEffect`, `m_modelLook` (`0x041CE51F`), `m_baseModelLook` (`0x9B180535`),
`m_name`, `m_celebration`, `m_shopCards` (inline), `m_lootBoxType`
(`0x7AB4E3F8`) and a `m_hidePucks` byte.

Measured on both readable assets (**install**): **3 model-look references read,
all 3 of asset type `0x01A`**, and the raw and typed readers agree on 2 of 2.
That is the only material-adjacent handle a loot box has, and it is a *look*, not
a skin.

The drop pool is elsewhere, in two pieces:

* `STULootBoxUnlocks` (`0xF818D652`) carries `m_unlocks` (an inline
  `STUUnlocks`) and `m_lootBoxType` — reached from
  `STUProgressionUnlocks.m_lootBoxesUnlocks` (`0x473494FF`).
* `STULootBoxRules` (`0xAF3DFC87`) carries `m_rarityRules` and
  `m_currencyRarityRules`, arrays of `STULootBoxRarityRules` whose elements are
  `{m_rarity, m_A07D6F99, m_CBE06580}` — reached from the generic-settings asset
  carrying `STUGenericSettings_LootBoxes` (`0x9044BCFE`), declared at
  `STUGenericSettings_LootBoxes.cs:6` (**published-check**).

Measured (**install**): **the rarity table is absent from this install.** 69
generic-settings assets are registered, 17 are readable, all 17 have distinct
root classes, and **none** is `STUGenericSettings_LootBoxes`. So the rarity
weights are **not tested against real data**, and `loot_box_rules` returns an
empty list here — reported rather than treated as a pass.

`m_A07D6F99` and `m_CBE06580` are called "weights" in the reader because they sit
beside `m_rarity` in a class named `RarityRules`; the reference tooling never
reads them and **what they weigh is not established** (**published-check**).

### The pool, measured

`STUProgressionUnlocks` (`0x8CE1D110`, **confirmed**: 45 readable assets) is the
reward track. Measured over all 45 readable assets (**install**):

| what | measured |
|---|---|
| assets read | 45 of 56 registered, 0 parse failures |
| `m_7846C401` level entries | 43, and **all 43 grant something** |
| `m_9135A4B2` read | **0** on the first 8 assets — the field is absent from the payload, the same field-drop as the skin themes |
| `m_otherUnlocks` / per-level unlock references | **7,462 read, 7,462 resolve** |
| `m_lootBoxesUnlocks` elements | 8–9 per asset |
| loot-box pool unlock references | **5,865 read, 5,865 resolve** |

Those 13,327 references are the strongest check in this document. Each came out
of a two-hop walk — an inline `STUUnlocks`, then a reference array in the
dynamic block whose elements are sixteen bytes with the GUID at **+8** — so a
wrong stride or a missing deobfuscation shows up as GUIDs that name nothing. All
of them name something (**install**).

### The trap that cost the most time here

`STUUnlock.m_unlocks`, `STU_3021DDED.m_9135A4B2`,
`STUProgressionUnlocks.m_otherUnlocks`, `STURankedData.m_58066D8F` and
`STU_1757E817.m_unlocks` are all declared
`ReaderType = typeof(InlineInstanceFieldReader)` with a declared size of 24
(**published-check**). That means the field *is* the instance: the first four
bytes at the field are its **field-bag index**, not an element count.

Reading them as `i32 count` then payloads returned an empty list for every one
of them — **0 unlock references across all 45 progression assets**, against
13,327 after the fix, while the typed layer read the same bytes fine. It is
recorded here because the mistake produces *no error at all*: an empty list looks
exactly like a theme that grants nothing. The fix is
`unlocks::inline_instance_unlocks`; the arrays-of-instances shape is what
`Stu::inline_elements` walks, and the two are different things.

The element layout itself — sixteen bytes per element, GUID at `+8`, obfuscated
with the *array's* field hash — is copied from the typed layer's `read_element`
(`crates/ow-stu/src/types.rs:1030`) rather than re-derived, and validated by the
13,327 resolutions above.

## Voice conversations

`STUVoiceConversation` (`0xA408D74F`, **confirmed**: 447 readable assets, 1,954
instances). Measured on a strided sample (**install**):

| what | measured |
|---|---|
| conversations sampled | 17 |
| lines read (`m_90D76F17` → `m_E295B99C`) | 55, **all of one asset type: 111 (`0x6F`)** |
| lines resolving in the registry | **0** |
| stimuli read (`m_stimulus`) | 17, **all of type 120 (`0x78`)** |
| lines per conversation | `{2: 5, 3: 4, 4: 7, 5: 1}` |
| `m_weight` present | **0** of 17 |
| `m_265A80ED` element counts | `{0: 17}` |

`m_stimulus` is **confirmed**: `0x78` is `VoiceStimulus` in the reference
tooling's own asset-type list (`TankTonka/Models/Common.cs:47`), and every
sampled stimulus comes out as `0x78`.

The lines are **not** confirmed. They come out as type `0x6F`, and `0x6F` has no
entry in the reference tooling's asset-type list; the C# declares
`teStructuredDataAssetRef<STUVoiceLine>` and the list's nearest entry is
`VoiceLineSet = 0x70` (`Common.cs:44`). Either `STUVoiceLine` is its own type
that the reference tooling's list does not cover, or the declared field is the
wrong one. This reader reports the type it measured and does not choose. The
0-of-55 resolutions are almost certainly "not downloaded" rather than "wrong" —
every voice-conversation asset that was readable pointed at voice-line assets
that were not — but that is **assumed**, and it is the one place in this
document where an assumption carries an interpretation.

## Things that are simply not there

* **Type `0x063` (Catalog) is not an STU container.** All 20 readable catalog
  assets fail `Stu::parse` with `read past end`, and their first four bytes are
  **`DUTS`** (**install**):

  ```text
  0x0460000000000001: 23448 bytes, first 48:
    44 55 54 53 01 00 00 00 20 00 00 00 00 00 00 00 00 00 00 00 00 00
    00 00 00 00 00 00 1c 00 00 00 21 8b 61 85 00 00 00 00 62 00 00 00
  ```

  `44 55 54 53` is `DUTS`. So the type id in the reference tooling's list is
  right and the *container* is a different format this tree does not read. The
  reader exists, it is simply never reached, and the test names `0x063` as a
  known-unparsed type rather than quietly weakening the "zero parse failures"
  assertion for every type. **What `DUTS` is: not investigated.**
* **`m_level` and `m_stars` on a portrait frame** are adjacent `ushort`s and the
  raw layer has no sixteen-bit reader, so they are read as the low and high
  halves of one `u32`. That follows from the container's little-endian order, not
  from an assumption about the data. **Not tested against real data** (no
  portrait frame was readable).
* **`teStructuredDataDateAndTime`** on a ranked season is eight bytes on this
  build and is **not decoded** — `RankedSeason` carries it as raw bytes.
  Decoding it is an open item, not a value.
* **`m_5351832E`** on an achievement has a generated default of `-1`, so a stored
  `-1` means "not authored" rather than "minus one". Recorded because reading it
  as a number would be wrong in a way nothing would catch.

## Most field names are hashes, and that is the reference tooling's loss

The reference tooling could identify only 3,910 of the 23,204 fields its own
generator declares, and those cover 480 distinct field hashes. So a field named
`m_displayName` would be the exception. Where the reference tooling has only a
hash, this reader keeps the hash in the constant name — `UNNAMED_F57B051E`,
`UNNAMED_290B2ADF`, `UNNAMED_3DFAC8CA` — and says so. **Every name this reader
prints is a reference-tooling name, never one fitted here.** Six whole unlock
classes are named `STU_<hash>` by the reference tooling itself
(`STU_C3C6FD9E` is the hero unlock, `STU_3F17D547` a mythic customization, and
so on); the mapping between them and meaning comes from
`DataTool/DataModels/Unlock.cs`, which is where the reader's `UnlockKind` table
is transcribed from.

## What is not tested

Stated plainly, because the census above makes it easy to over-read:

* **The unlock reader is not tested against real data.** 19,122 assets, none
  downloaded. Every unlock field hash in this document is
  **published-check only**. The subclass→kind mapping and the "no price" answer
  *are* tested — by unit tests over the mapping and over the absent-price case.
* **The achievement, stat and catalog readers are not tested against real data**
  (0 of 164, 0 of 2,918, and 20 readable assets in a container this tree cannot
  parse respectively).
* **The ranked-season reader is not tested at all**: the type ships zero assets
  on this install.
* **The loot-box rarity weights are not tested**: the class that carries them is
  not in any readable settings asset here.
* **Everything about materials and body textures for a skin is untested**, because
  it is not in the skin asset. See § "The skin join that is still missing".

## The next steps, in order of yield

1. **Read the resource graph's skin block** (`trg.rs`). That is the only thing
   standing between a skin theme and a drawable skin: it carries the
   `srcAsset → destAsset` substitutions that turn a base model into a skin.
2. **Read the `DUTS` container** for type `0x063`. Two assets' worth of bytes are
   enough to tell whether it is a fixed-header table; 43 are registered and 20
   are readable, so this is bounded.
3. **The voice-line asset type.** Decide between "`STUVoiceLine` is `0x6F`" and
   "the declared field is wrong" by finding a readable voice-line asset and
   checking what type its neighbours use. Until one is downloaded, this is
   guesswork either way.
4. **Nothing about the unlock fields can be progressed on this install.** If the
   unlock assets are wanted, the operator's call is whether to fetch them; the
   reader is written and waiting.

## Reproducing

```text
cargo run -p ow-tank --bin content -- --types                      # the census
cargo run -p ow-tank --bin content -- skinthemes --limit 5 --scan 400
cargo run -p ow-tank --bin content -- progressions --limit 5
cargo run -p ow-tank --bin content -- lootboxes
cargo run -p ow-tank --bin content -- rules                       # the rarity table (empty here)
cargo run -p ow-tank --bin content -- --guid 0x0a500000000048ce   # one asset, field by field
cargo test -p ow-tank --release --test unlocks_real -- --nocapture
```

`--limit` bounds how many *records* are printed and never the census. `--scan N`
bounds how many assets of each type are opened; the census says `CAPPED` when it
was, because the reference install's 19,122 undownloaded unlock assets make an
uncapped census take minutes of CASC misses.