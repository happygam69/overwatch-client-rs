# Crates

## Active

| crate | role |
|---|---|
| `ow-casc` | container: build info, configs, keyring, bucket index, encoding, BLTE, root |
| `ow-stu` | STU v1/v2 reader |
| `ow-tank` | asset semantics: GUIDs, classes, entities, models, clips, weapons, maps, `mapresolve` |
| `ow-scene` | map geometry from the resolver's chunks and the models they place |
| `ow-client` | the one launchable client: practice range, renderer, HUD, free camera |
| `xtask` | the project gate |

`ow-client`'s default launch is the practice range selected through
`ow-tank::mapresolve` — the header's `m_mapName`, then the first paired mode
whose `STUGameMode.displayName` is exactly `Practice Range`. It does not read a
chunk seed, a capture or a hand-picked GUID list. The chunk flags
(`--chunks`, `--chunks-from`, `--chunk-file`, `--cluster`) are explicit
diagnostics, not the default. See [`../docs/rebuild-plan.md`](../docs/rebuild-plan.md).

## Parked reference code

These are **not workspace members** and are not built:

- `ow-sim` — the read body/camera arithmetic.

It is kept as reference for the gameplay rebuild, not as a source of truth. Do
not re-introduce it into the build until its layers pass the oracle; see
[`../docs/rebuild-plan.md`](../docs/rebuild-plan.md).

The old `ow-view` and `ow-game` crates are deleted; their renderer lives in
`ow-client`.
