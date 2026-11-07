//! The hero-select stage: the mode's own roster, the pick, and the row the
//! walk applies.
//!
//! The real client has no native selector. Its menu, mode and character screens
//! are web pages in the embedded CEF browser, and the pick travels back through
//! the browser bridge as a page value (`docs/formats/frontend-pipeline.md`,
//! item 18); what binds that value to the spawned character is still unread.
//! The *rules*, however, are data and are read:
//!
//! * a mode's per-team `m_availableHeroes` says either "every hero" (the empty
//!   marker `STU_D1DC7C55`) or "exactly these heroes" (a whitelist), and
//!   [`ow_tank::hero_select::selectable_heroes`] reads that set;
//! * an open roster contains NPC heroes as well as player heroes, and
//!   `STUHero.m_64DC571F` is the marker that separates them, so the picker
//!   offers the marked ones;
//! * the picked hero's gameplay entity carries the `STUCharacterMoverComponent`
//!   whose forward base speed and terminal fall are the values this stage hands
//!   to the walk.
//!
//! The pick itself is therefore a **declared viewer default**, like the spawn
//! volume: the roster and every value applied are read, the choice is the
//! operator's. What the client does with a hero id after the spawn is open, so
//! no claim is made beyond the movement row and the hero's own model.

use std::io::{BufRead, IsTerminal, Write};

use ow_tank::character_collision::StepLimits;
use ow_tank::classreg::ClassRegistry;
use ow_tank::entity::{mover_row_of, mover_step_limits_of, MoverRow};
use ow_tank::hero_select::{
    mode_team_pools, resolve_hero, selectable_heroes, HeroChoice, HeroPool,
};
use ow_tank::keytable::Sources;
use ow_tank::mapresolve;

/// A pick error's text, without the generic `not found:` prefix: the message
/// already says what was not found.
fn pick_error(error: ow_tank::Error) -> String {
    match error {
        ow_tank::Error::NotFound(message) => message,
        other => other.to_string(),
    }
}

/// One selectable hero: the asset's own fields plus the movement row read from
/// its gameplay entity.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroEntry {
    pub hero: HeroChoice,
    pub row: Option<MoverRow>,
    /// The mover's vertical step limits (`+0x688`/`+0x68c`, derived by
    /// `ConstructMoverBody` from both capsule triples). The walk's floor probe
    /// reach is `-min`.
    pub step: Option<StepLimits>,
    /// The take-off lane (`0xEC289987`), the jump's launch speed, m/s.
    pub takeoff_lane: Option<f32>,
}

impl HeroEntry {
    /// The forward base speed (`0x19452F15`), the value the walk uses as its
    /// input magnitude.
    pub fn forward_speed(&self) -> Option<f32> {
        self.row.and_then(|row| row.forward)
    }

    /// The terminal fall speed (`0xB74245E8`), when the hero declares one.
    pub fn terminal_fall(&self) -> Option<f32> {
        self.row.and_then(|row| row.fall_terminal)
    }

    /// The gravity magnitude (`0xC2C40969`), a scale on the client's `10.0`:
    /// the frame's gravity is `-10.0 * gravity` along the up axis
    /// (`0x140ca72d0`, `docs/formats/character-controller.md` "The vertical
    /// law").
    pub fn gravity(&self) -> Option<f32> {
        self.row.and_then(|row| row.gravity)
    }

    /// The terminal speed while rising (`0x975F21A5`), the up half of the drag
    /// law (`0x140cb2064`). Absent on the heroes this install's class declares
    /// no value for, which the drag law reads as "no drag while rising".
    pub fn rise_terminal(&self) -> Option<f32> {
        self.row.and_then(|row| row.rise_terminal)
    }

    /// The take-off lane (`0xEC289987`), the up lane a standing jump starts
    /// with (`0x140cc1627`).
    pub fn takeoff_lane(&self) -> Option<f32> {
        self.takeoff_lane
    }

    /// How far down the floor probe may reach before a drop is a fall: the
    /// mover's `+0x688` limit as a positive distance (`2.5` m on the common
    /// hero, `2.8` on the large ones).
    pub fn step_reach(&self) -> Option<f32> {
        self.step.map(|limits| limits.reach())
    }
}

/// A mode's roster, as the selector shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ModeRoster {
    pub mode: u64,
    pub mode_name: String,
    /// One line per team: its index and what its `m_availableHeroes` says.
    pub rules: Vec<String>,
    /// Every playable hero the mode offers, in asset-index order.
    pub heroes: Vec<HeroEntry>,
    /// Hero assets in the mode's set that are not player heroes (NPCs). They
    /// are counted, not offered.
    pub npc_heroes: usize,
}

impl ModeRoster {
    fn choices(&self) -> Vec<HeroChoice> {
        self.heroes.iter().map(|entry| entry.hero.clone()).collect()
    }

    fn entry(&self, guid: u64) -> &HeroEntry {
        self.heroes
            .iter()
            .find(|entry| entry.hero.guid == guid)
            .expect("a resolved hero is in the roster")
    }

    /// A one-line description of what the teams declare.
    pub fn rule_summary(&self) -> String {
        self.rules.join("; ")
    }
}

/// Open the install and read the practice range's roster.
///
/// The mode is the practice range's own: the header's `m_mapName`, the
/// variation whose mode display name is `Practice Range`, and that mode's
/// teams. Nothing here is a GUID seed; see
/// [`ow_tank::mapresolve::find_practice_range`] for why this is a viewer
/// default.
pub fn practice_roster() -> Result<ModeRoster, String> {
    let root = ow_casc::install::detect_root()
        .ok_or("no Overwatch install found; set OW_INSTALL to the folder containing .build.info")?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = ow_tank::GameData::open(&root, &ow_tank::ManifestSelector::default(), &sources)
        .map_err(|e| format!("the install does not open: {e}"))?;
    let registry = ClassRegistry::load_from_sources(&sources);
    if registry.is_empty() {
        return Err(
            "no STU class checkout found (set OW_OWLIB to the asset-library checkout); the mode's \
             fields cannot be read without it"
                .into(),
        );
    }
    let range = mapresolve::find_practice_range(&data).map_err(|e| e.to_string())?;
    roster_of(
        &data,
        registry.inner(),
        range.variation.game_mode,
        &range.mode_name,
    )
}

/// Read one mode's roster: the playable heroes its teams offer and each one's
/// movement row.
fn roster_of(
    data: &ow_tank::GameData,
    registry: &ow_stu::types::ClassRegistry,
    mode: u64,
    mode_name: &str,
) -> Result<ModeRoster, String> {
    let teams = mode_team_pools(data, mode, registry).map_err(|e| e.to_string())?;
    let rules: Vec<String> = teams
        .iter()
        .map(|team| {
            let who = match team.team {
                Some(index) => format!("team {index}"),
                None => "team 0".to_string(),
            };
            let what = match &team.pool {
                HeroPool::All => "every hero".to_string(),
                HeroPool::Whitelist(heroes) => format!("{} named heroes", heroes.len()),
                HeroPool::Absent => "no hero pool (unread default)".to_string(),
                HeroPool::Unrecognized(hash) => format!("unknown pool class 0x{hash:08X}"),
            };
            format!("{who}: {what}")
        })
        .collect();
    let all = selectable_heroes(data, mode, registry).map_err(|e| e.to_string())?;
    let npc_heroes = all.iter().filter(|hero| !hero.playable).count();
    let mut heroes = Vec::new();
    for hero in all.into_iter().filter(|hero| hero.playable) {
        let entity = ow_tank::hero_select::hero_entity(data, hero.guid);
        let row = entity.and_then(|entity| mover_row_of(data, entity).ok().flatten());
        let step = entity.and_then(|entity| mover_step_limits_of(data, entity).ok().flatten());
        let takeoff_lane = entity.and_then(|entity| {
            ow_tank::entity::mover_takeoff_lane_of(data, entity)
                .ok()
                .flatten()
        });
        heroes.push(HeroEntry {
            hero,
            row,
            step,
            takeoff_lane,
        });
    }
    Ok(ModeRoster {
        mode,
        mode_name: mode_name.to_string(),
        rules,
        heroes,
        npc_heroes,
    })
}

/// Whether this launch is the live walk, the only launch that offers the
/// prompt. A diagnostic that exits before the window (`--shot`, `--tour`,
/// `--walk-test`, ...) must never block on stdin; the terminal check itself is
/// in [`choose`].
pub fn interactive_launch(args: &[String]) -> bool {
    const EXITS: [&str; 10] = [
        "--tour",
        "--shot",
        "--list-maps",
        "--walk-test",
        "--walk-column",
        "--wall-audit",
        "--frames",
        "--d3d11-frames",
        "--render-column",
        "--chunks",
    ];
    !args.iter().any(|a| EXITS.contains(&a.as_str()))
}

/// The picker's table: two columns of `[index] name  forward m/s`.
pub fn table(roster: &ModeRoster) -> Vec<String> {
    let cell = |entry: &HeroEntry| {
        let speed = entry
            .forward_speed()
            .map(|f| format!("{f:.2}"))
            .unwrap_or_else(|| "?".to_string());
        let gravity = entry
            .gravity()
            .map(|g| format!("{:.2}", g * 10.0))
            .unwrap_or_else(|| "?".to_string());
        let rise = entry
            .rise_terminal()
            .map(|f| format!("{f:.2}"))
            .unwrap_or_else(|| "-".to_string());
        format!(
            "[{:>4}] {:<20} {:>4} m/s  g {:>5}  rise {}",
            entry.hero.index,
            entry.hero.label(),
            speed,
            gravity,
            rise
        )
    };
    let mut lines = Vec::new();
    let mut pairs = roster.heroes.chunks(2);
    for pair in &mut pairs {
        let mut line = cell(&pair[0]);
        if let Some(second) = pair.get(1) {
            line.push_str("   ");
            line.push_str(&cell(second));
        }
        lines.push(line);
    }
    lines
}

/// Ask the operator for a pick, or resolve the one the command line named.
///
/// `spec` is `--hero <name|index>`. Without it, the roster is printed and the
/// pick is read from stdin **only when stdin is a terminal**; a non-interactive
/// launch gets no hero rather than a hidden default. An empty line means "no
/// hero" and keeps the viewer defaults.
pub fn choose<'a>(
    roster: &'a ModeRoster,
    spec: Option<&str>,
    interactive: bool,
) -> Result<Option<&'a HeroEntry>, String> {
    if let Some(spec) = spec {
        let choices = roster.choices();
        let hero = resolve_hero(&choices, spec).map_err(pick_error)?;
        return Ok(Some(roster.entry(hero.guid)));
    }
    if !interactive || !std::io::stdin().is_terminal() {
        return Ok(None);
    }

    println!(
        "\nhero select: {} (mode 0x{:016x}) - {}; {} playable hero(es){}",
        roster.mode_name,
        roster.mode,
        roster.rule_summary(),
        roster.heroes.len(),
        if roster.npc_heroes > 0 {
            format!(", {} NPC hero asset(s) not offered", roster.npc_heroes)
        } else {
            String::new()
        }
    );
    for line in table(roster) {
        println!("  {line}");
    }
    println!("  (the real client's pick is a CEF page value; this list is the mode data, the choice is yours)");

    let stdin = std::io::stdin();
    loop {
        print!("\nhero (name or index, empty = no hero): ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            println!("\nhero: no input; no hero selected");
            return Ok(None);
        }
        let spec = line.trim();
        if spec.is_empty() {
            return Ok(None);
        }
        let choices = roster.choices();
        match resolve_hero(&choices, spec) {
            Ok(hero) => return Ok(Some(roster.entry(hero.guid))),
            Err(e) => println!("{}", pick_error(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> ModeRoster {
        let hero = |index: u32, name: &str, forward: f32| HeroEntry {
            hero: HeroChoice {
                guid: 0x02E0_0000_0000_0000 | index as u64,
                index,
                name: Some(name.to_string()),
                playable: true,
            },
            row: Some(MoverRow {
                forward: Some(forward),
                ..MoverRow::default()
            }),
            step: Some(StepLimits {
                min: -2.5,
                max: 1.0,
            }),
            takeoff_lane: Some(6.0),
        };
        ModeRoster {
            mode: 0x0230_0000_0000_0018,
            mode_name: "Practice Range".into(),
            rules: vec!["team 0: every hero".into(), "team 1: every hero".into()],
            heroes: vec![
                hero(2, "Reaper", 5.5),
                hero(3, "Tracer", 6.0),
                hero(0xDD, "Mei", 5.5),
            ],
            npc_heroes: 81,
        }
    }

    #[test]
    fn a_named_pick_resolves_to_its_movement_row() {
        let roster = roster();
        let pick = choose(&roster, Some("tracer"), false).unwrap().unwrap();
        assert_eq!(pick.hero.index, 3);
        assert_eq!(pick.forward_speed(), Some(6.0));
    }

    #[test]
    fn an_empty_line_means_no_hero() {
        let roster = roster();
        // `interactive=false` is the non-terminal path: no prompt, no hero.
        assert!(choose(&roster, None, false).unwrap().is_none());
    }

    #[test]
    fn only_the_live_walk_asks_for_a_pick() {
        assert!(interactive_launch(&[]));
        assert!(interactive_launch(&["--hero".into(), "tracer".into()]));
        for flag in [
            "--walk-test",
            "--wall-audit",
            "--shot",
            "--tour",
            "--frames",
            "--chunks",
        ] {
            assert!(
                !interactive_launch(&[flag.to_string()]),
                "{flag} must not block on stdin"
            );
        }
    }

    #[test]
    fn the_table_names_the_index_and_the_speed() {
        let lines = table(&roster());
        assert_eq!(lines.len(), 2, "three heroes take two rows");
        assert!(lines[0].contains("[   2] Reaper"), "{:?}", lines[0]);
        assert!(lines[0].contains("5.50 m/s"), "{:?}", lines[0]);
        assert!(lines[1].contains("[ 221] Mei"), "{:?}", lines[1]);
    }
}
