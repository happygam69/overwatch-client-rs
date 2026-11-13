# Content surfaces: unlocks, cosmetics, loot boxes

What a player owns and how they get it. The main join is a skin theme naming
the models a cosmetic swaps in: the first half of "this cosmetic exists → this
is what it looks like".

Reader: `crates/ow-tank/src/unlocks.rs`. CLI:
`cargo run -p ow-tank --bin content`. Test:
`cargo test -p ow-tank --release --test unlocks_real`.

## Current state

The bundle-aware asset path now opens assets that the earlier store-only
census missed. Cosmetic coverage is not a gameplay gate, so this document does
not claim a missing-content count; run the content survey after a bundle-aware
pass if a count is needed.

## Unlocks

An unlock asset holds one instance, and its exact class says what it is. The
mapping is `unlocks::unlock_kind_of`, transcribed from the reference's
`Unlock.cs`; twenty-eight classes are named and pinned by a unit test.

The mapping is over exact classes, not subtrees. Order matters once:
`STU_A458D547` (a souvenir) derives from `STUUnlock_Emote`, so testing the
emote class first would call every souvenir an emote. The test
`a_souvenir_is_not_an_emote` pins the order.

The three intermediate classes (`STUUnlock` `0x6760479E`, `STU_3021DDED`,
`STU_895A2A7A`) are real shipped shapes whose root the reference switch does
not list; this reader calls them `Unknown`.

Unlock fields (structure confirmed by the class declarations; **not tested
against real unlock bytes** because none are downloaded):

| field | class | what it is |
|---|---|---|
| `0xB48F1D22` | `STUUnlock` | display-text asset |
| `0x3446F580` | `STU_3021DDED` | description text |
| `0x53145FAF` | `STU_3021DDED` | "available in" text |
| `0xBB99FCD3` | `STU_3021DDED` | rarity |
| `0xA7B393BF` | `STUUnlock` | category enum |
| `0xBEE9BCDA` | `STU_3021DDED` | gallery filter list |
| `0x00B16A0B` | `STUUnlock` | Battle.net product id |
| `0x4C167404` | `STUUnlock_SkinTheme` | the skin it grants |
| `0x37AB13D3` | `STU_C3C6FD9E` | hero |
| `0xE2A492C7` | `STU_3F17D547` | parent mythic skin |
| `0xA0D4ABDE` | `STUUnlock_Emote` | the `STUAnimAlias` |

## Skin themes and the model join

A skin theme names the models a cosmetic swaps in. The read themes resolve to
model references; the join is the same one `formats/materials.md` uses for
looks. Cosmetic coverage is not part of the gameplay gate.

## Open

- The unlock reader has no real-data test.
- The catalog's arrays. All twenty readable type-`0x063` assets parse as STU
  and are **version 1** (measured: `ow-tank-content types`, 20 version-1, 176
  instances); a version 1 asset writes **no per-instance field bags**, so the
  raw reader has no offset to place `m_assets` at and every catalog reports
  `assets=0 longs=0`. The typed layer, which takes the class layout from the
  reference's declarations, reads 2,918 GUIDs out of `0x0460000000000001`.
  Closing this means the client's own v1 class schema, not a reference
  declaration: the raw reader cannot derive a field offset from the file.
- Five of the twenty catalogs have a root class no reader here claims
  (`0x4B69C3BC`, `0x6A887A13`, `0x71172629`, `0x7C543D41`, `0xAEBBF451`; the
  last one holds 157 instances). The fifteen others are `STUCatalog`
  `0x85618B21`).

## Verification

```text
cargo test --release -p ow-tank --test unlocks_real
cargo run -p ow-tank --bin content -- types --scan 100000
```
