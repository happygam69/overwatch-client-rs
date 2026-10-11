# The practice range

`ow-range` joins the two halves of the project: `ow-tank` reads what the
installation says, `ow-sim` turns it into a fight, and the range is the harness
that lets a person pull the trigger.

```sh
export OW_INSTALL='E:\Overwatch'
export OW_TACTLIB='path/to/TACTLib/TACTLib'
export OW_OWLIB='path/to/OWLib'
export OW_SUPPORT_KEYRING='path/to/Overwatch.keyring'   # optional

cargo run --release -p ow-range
cargo run --release -p ow-range -- script.txt           # non-interactive
```

```
> hero 10
hero 10 (0x02e000000000000a): 175 health, 0 armour, 0 shields, 1 weapon(s)
  slot 1: castLength=100 destination=100 numProjectilesPerShot=1
          numShotsPerSecond=2/1 projectileLifetime=10 projectileSpeed=250 ...
> damage 12
> target add 200 15
> ttk
200 hp at 12.0 damage per shot (1 pellets x 12.0) = 17 shots, 8.50s at 2.00 shots/s
> fire 3
3 shot(s) in 1.50s: 3/3 pellets hit, 36.0 damage applied, 24.0 dps
  target 0: 164/200 hp
```

## What comes from the install, and what does not

This is the part worth reading, because the range is only as honest as this
distinction.

**Read from the installation.** Health, armour and shields per hero, with the
pools kept separate rather than summed, so a 50 health / 150 shield hero behaves
differently from a 200 health one. Per weapon: shots per second, projectiles per
shot, projectile speed, projectile lifetime, and the cast length of a raycast.

**Supplied by the operator.** Per-shot damage, via `damage <n>`. It could not be
extracted — `docs/formats/weapons.md` records the seven places it was looked for
— so the range asks rather than inventing a number. Ditto `rate <n>`, for the case
below.

**Assumed, in `ow-sim/src/rules.rs`.** The order in which damage comes off the
three pools, and how armour mitigates it. These are game *rules*, and rules are
not in the files, which hold values. Both are named constants with the reasoning
beside them, and the `rules`, `order` and `armour` commands change them at
runtime so the difference is visible instead of buried.

## Two ambiguities the range surfaces rather than hides

**A graph can declare several firing modes, and nothing says which is the
primary.** Widowmaker's weapon graph declares `numShotsPerSecond` of both 2 and 1
— an unscoped and a scoped rate — and a `projectileSpeed` of 250 belonging to a
thrown mine, not to her rifle. The range prints every distinct value it found
(`params` shows them all) and resolves the mode by preference: a `castLength`
means a raycast, which is direct evidence of hitscan, and that outranks a
projectile speed. Without that preference her rifle would be simulated as a
projectile weapon, which it is not.

**`numShotsPerSecond = 999` is not a rate.** It means "as fast as input allows",
and it appears on weapons whose real cadence comes from somewhere else. Treating
it as a number gives 999 shots a second; clamping it gives 60, which is equally
invented. The range reports that no rate was declared and defaults to 1/s, with
`rate <n>` to set the real one.

Neither of these is a parsing bug. Both are cases where the file does not contain
the answer, and the range's job is to say so.

## Commands

| command | effect |
|---|---|
| `heroes [filter]` | list hero indices |
| `hero <index>` | select a hero and print everything extracted for it |
| `params` | every distinct named parameter the weapon graph declared |
| `damage <n>` | per-projectile damage for the current shot |
| `rate <n>` | shots per second, when the graph declares only the sentinel |
| `target add <hp> <m>` | spawn a target, optionally armoured |
| `target [clear]` | list or remove targets |
| `fire [n]` | fire n shots and report hits, damage and dps |
| `ttk` | shots and seconds to kill the nearest target |
| `rules` / `order` / `armour` | inspect and change the assumed rules |
| `quit` | leave |

## What the simulation does and does not model

Modelled: fixed-tick stepping, a deterministic spread cone (seeded, so a shot is
reproducible and therefore testable), hitscan raycasts against vertical capsules,
nearest-target selection, per-pellet resolution for multi-projectile weapons,
distance falloff, the three health pools, and armour mitigation.

Not modelled yet, in rough order of how much it would change a fight:

* **Projectile flight.** A projectile weapon is resolved as an instant hit at
  its maximum range rather than travelling, so lead and travel time are absent.
* **Abilities.** Nothing reads the state graphs, so no cooldowns, no mobility, no
  ultimates. The graphs are readable (`ow-probe weapon-params`) but interpreting
  them is the open problem described in `docs/formats/weapons.md`.
* **Reloads and ammo.** The extracted data does not currently carry magazine
  sizes to the range.
* **Movement.** No gravity, no acceleration, no hero move speeds. The targets
  stand still.
* **Shield regeneration**, and any hero-specific healing.

A fight in the range is therefore a duel of ballistics and geometry, not yet a
duel of heroes.
