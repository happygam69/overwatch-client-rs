//! Trace format v1 and the first-divergence engine.
//!
//! A *trace* is what `dev/live_trace.py` writes when it samples a running
//! client's `dmBody` (offsets from `docs/formats/domino.md`): one JSON object
//! per line, raw `u32` bit patterns so nothing is lost to decimal printing.
//! The *differ* replays a declared input script through this crate's model and
//! reports the first tick where the two disagree — tick, field, ULP distance —
//! which is the M0 gate of `docs/rewrite-plan.md` and stays the arbiter for
//! every "matching" claim after it.
//!
//! Everything here is dependency-free and deterministic: parsing is exact and
//! the ULP comparison is total over float bits.
//!
//! [`diff_trace`] is the replay engine both front ends share: the
//! `trace_diff` binary prints its report, and the Bevy shell shows the first
//! divergence in its HUD (the M5 gate: the harness runs from the game, not
//! just tests).

use crate::movement::{Controller, Input, MoverSpec, TICK_RATE_HZ};

/// Bit patterns per sample: position xyz + w, velocity xyz + w.
pub type Bits = [u32; 4];

/// One client sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    pub i: u64,
    pub t: f32,
    pub pos: Bits,
    pub vel: Bits,
    pub angvel: Bits,
    pub flags: Vec<u8>,
    /// The key state the recorder saw for this sample, when the trace carries
    /// it (`dev/live_trace.py` records it by default). A trace with keys can
    /// be replayed directly — no hand-written script.
    pub keys: Option<Keys>,
    /// Camera yaw in degrees for this sample, when the trace carries it
    /// (`"yaw":`). Movement direction comes from here for keyed replays;
    /// absent means 0, which keeps older traces parsing.
    pub yaw: f32,
}

/// Raw key state at one sample. What the *engine's* movement input looks like
/// downstream is question Q3 (`docs/rewrite-plan.md`) and not assumed here:
/// this is exactly what a keyboard reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Keys {
    pub w: bool,
    pub a: bool,
    pub s: bool,
    pub d: bool,
    pub space: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub lmb: bool,
    pub rmb: bool,
}

impl Keys {
    /// The controller input these keys mean, with the sign conventions
    /// `movement::Input` documents (W = forward, D = side positive).
    pub fn to_input(&self) -> Input {
        Input {
            forward: (self.w as i32 - self.s as i32) as f32,
            side: (self.d as i32 - self.a as i32) as f32,
            crouch: self.ctrl,
            // Held, not edge: the engine's own edge handling is Q3. The
            // differ reports both readings if they disagree.
            jump: self.space,
        }
    }
}

/// A parsed trace.
#[derive(Debug, Clone)]
pub struct Trace {
    pub hz: f32,
    pub addr: String,
    pub samples: Vec<Sample>,
}

/// Distance in ULPs between two floats, treating `-0.0 == 0.0`.
///
/// Both infinities and NaNs map to a total order on bits, so the distance is
/// defined for every pair; a NaN-vs-number distance is large, which is what
/// the caller wants to see.
pub fn ulp_distance(a: f32, b: f32) -> u32 {
    fn ordered(x: f32) -> i64 {
        let b = x.to_bits() as i32;
        // Negative floats are reversed so the mapping is monotone.
        let o = if b < 0 {
            0x8000_0000u32.wrapping_sub(b as u32)
        } else {
            b as u32
        };
        o as i64
    }
    if a == b {
        return 0; // also catches -0.0 vs 0.0
    }
    ordered(a).abs_diff(ordered(b)) as u32
}

/// One disagreement.
#[derive(Debug, Clone, PartialEq)]
pub struct Divergence {
    pub sample: u64,
    /// "pos.x" etc.
    pub field: &'static str,
    pub expected: f32,
    pub actual: f32,
    pub ulp: u32,
}

const FIELD_NAMES: [&str; 6] = ["pos.x", "pos.y", "pos.z", "vel.x", "vel.y", "vel.z"];

/// First field of `actual` that does not match `expected` bit-for-bit.
///
/// `expected` is the client's state, `actual` the model's, each as
/// `[pos.x, pos.y, pos.z, vel.x, vel.y, vel.z]`.
pub fn first_divergence(sample: u64, expected: [f32; 6], actual: [f32; 6]) -> Option<Divergence> {
    for k in 0..6 {
        if expected[k].to_bits() != actual[k].to_bits() {
            return Some(Divergence {
                sample,
                field: FIELD_NAMES[k],
                expected: expected[k],
                actual: actual[k],
                ulp: ulp_distance(expected[k], actual[k]),
            });
        }
    }
    None
}

// -- the wire format ----------------------------------------------------------

fn find_array_u32(line: &str, key: &str) -> Option<[u32; 4]> {
    let start = line.find(&format!("\"{key}\":["))? + format!("\"{key}\":[").len();
    let end = line[start..].find(']')? + start;
    let mut out = [0u32; 4];
    for (k, part) in line[start..end].split(',').enumerate() {
        if k >= 4 {
            return None;
        }
        out[k] = part.trim().parse().ok()?;
    }
    Some(out)
}

fn find_u64(line: &str, key: &str) -> Option<u64> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

fn find_f32(line: &str, key: &str) -> Option<f32> {
    let pat = format!("\"{key}\":");
    let start = line.find(&pat)? + pat.len();
    let rest = &line[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}

fn find_string(line: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\":\"");
    let start = line.find(&pat)? + pat.len();
    let end = line[start..].find('"')? + start;
    Some(line[start..end].to_string())
}

fn find_flags(line: &str) -> Vec<u8> {
    let pat = "\"flags\":[";
    match line.find(pat) {
        Some(s) => {
            let start = s + pat.len();
            let end = line[start..].find(']').map(|e| e + start).unwrap_or(start);
            line[start..end]
                .split(',')
                .filter_map(|p| p.trim().parse().ok())
                .collect()
        }
        None => Vec::new(),
    }
}

/// `"keys":{"w":1,"a":0,...}` — absent means the trace predates key capture.
fn find_keys(line: &str) -> Option<Keys> {
    let pat = "\"keys\":{";
    let start = line.find(pat)? + pat.len();
    let end = line[start..].find('}').map(|e| e + start)?;
    let body = &line[start..end];
    let bit = |key: &str| -> bool {
        body.find(&format!("\"{key}\":")).is_some_and(|s| {
            let rest = &body[s + key.len() + 3..];
            rest.trim_start().starts_with('1')
        })
    };
    Some(Keys {
        w: bit("w"),
        a: bit("a"),
        s: bit("s"),
        d: bit("d"),
        space: bit("space"),
        ctrl: bit("ctrl"),
        shift: bit("shift"),
        lmb: bit("lmb"),
        rmb: bit("rmb"),
    })
}

impl Trace {
    /// Parse the JSON-lines format written by `dev/live_trace.py`.
    ///
    /// Unknown keys are ignored; a malformed line is an error, not a skip —
    /// a silently dropped sample would corrupt every later comparison.
    pub fn parse(text: &str) -> Result<Trace, String> {
        let mut hz = 60.0f32;
        let mut addr = String::new();
        let mut samples = Vec::new();
        for (lineno, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line.contains("\"trace\":") {
                hz = find_f32(line, "hz").ok_or_else(|| format!("line {}: bad hz", lineno + 1))?;
                addr = find_string(line, "addr").unwrap_or_default();
                continue;
            }
            let i = find_u64(line, "i").ok_or_else(|| format!("line {}: bad i", lineno + 1))?;
            let t = find_f32(line, "t").ok_or_else(|| format!("line {}: bad t", lineno + 1))?;
            let pos = find_array_u32(line, "pos")
                .ok_or_else(|| format!("line {}: bad pos", lineno + 1))?;
            let vel = find_array_u32(line, "vel")
                .ok_or_else(|| format!("line {}: bad vel", lineno + 1))?;
            let angvel = find_array_u32(line, "angvel").unwrap_or([0; 4]);
            samples.push(Sample {
                i,
                t,
                pos,
                vel,
                angvel,
                flags: find_flags(line),
                keys: find_keys(line),
                yaw: find_f32(line, "yaw").unwrap_or(0.0),
            });
        }
        samples.sort_by_key(|s| s.i);
        Ok(Trace { hz, addr, samples })
    }

    /// Serialize (the writer half of the format; also what the self-test uses).
    pub fn write(&self, addr: &str) -> String {
        let mut out = format!(
            "{{\"trace\":1,\"kind\":\"dmBody\",\"addr\":\"{addr}\",\"hz\":{}}}\n",
            self.hz
        );
        for s in &self.samples {
            let keys = match &s.keys {
                Some(k) => format!(
                    ",\"keys\":{{\"w\":{},\"a\":{},\"s\":{},\"d\":{},\"space\":{},\"ctrl\":{},\"shift\":{},\"lmb\":{},\"rmb\":{}}}",
                    k.w as u8, k.a as u8, k.s as u8, k.d as u8, k.space as u8,
                    k.ctrl as u8, k.shift as u8, k.lmb as u8, k.rmb as u8,
                ),
                None => String::new(),
            };
            out.push_str(&format!(
                "{{\"i\":{},\"t\":{},\"pos\":[{},{},{},{}],\"vel\":[{},{},{},{}],\"angvel\":[{},{},{},{}],\"flags\":[{}]{}}}\n",
                s.i,
                s.t,
                s.pos[0], s.pos[1], s.pos[2], s.pos[3],
                s.vel[0], s.vel[1], s.vel[2], s.vel[3],
                s.angvel[0], s.angvel[1], s.angvel[2], s.angvel[3],
                s.flags.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(","),
                keys,
            ));
        }
        out
    }

    /// Sample state as the six floats the differ compares.
    pub fn expected_state(&self, k: usize) -> [f32; 6] {
        let s = &self.samples[k];
        [
            f32::from_bits(s.pos[0]),
            f32::from_bits(s.pos[1]),
            f32::from_bits(s.pos[2]),
            f32::from_bits(s.vel[0]),
            f32::from_bits(s.vel[1]),
            f32::from_bits(s.vel[2]),
        ]
    }
}

/// One step of a declared replay script: an input applied from `t` seconds
/// on, with an absolute yaw. The format is the one `trace_diff` documents —
/// `t forward side crouch jump yaw_deg`, `#` starts a comment.
#[derive(Debug, Clone, Copy)]
pub struct ScriptStep {
    pub t: f32,
    pub input: Input,
    pub yaw_deg: f32,
}

/// Parse the script format. A malformed line is an error, not a skip.
pub fn parse_script(text: &str) -> Result<Vec<ScriptStep>, String> {
    let mut steps = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            return Err(format!(
                "line {}: want 6 columns, got {}",
                n + 1,
                parts.len()
            ));
        }
        let f = |k: usize| {
            parts[k]
                .parse::<f32>()
                .map_err(|e| format!("line {}: {e}", n + 1))
        };
        let (t, forward, side, yaw_deg) = (f(0)?, f(1)?, f(2)?, f(5)?);
        let crouch = f(3)? != 0.0;
        let jump = f(4)? != 0.0;
        steps.push(ScriptStep {
            t,
            input: Input {
                forward,
                side,
                crouch,
                jump,
            },
            yaw_deg,
        });
    }
    if steps.is_empty() {
        return Err("script has no steps".into());
    }
    steps.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    Ok(steps)
}

fn step_at(steps: &[ScriptStep], t: f32) -> &ScriptStep {
    let mut current = &steps[0];
    for s in steps {
        if s.t <= t {
            current = s;
        } else {
            break;
        }
    }
    current
}

/// What a replay found.
#[derive(Debug, Clone)]
pub struct ReplayReport {
    /// Ticks the model ran before the last sample.
    pub aligned_ticks: u64,
    /// Samples compared.
    pub samples: usize,
    /// The first position disagreement, if any.
    pub first: Option<Divergence>,
    /// ULP residual of x/y/z against the last sample (meaningful when nothing
    /// diverged; the residual quantifies "no divergence so far").
    pub residuals: [u32; 3],
    /// Where the model ended up.
    pub end_pos: [f32; 3],
    /// Writes skipped at input transitions (sub-write partial timing; see
    /// `diff_trace_keys_states`). Zero for the other replay paths.
    pub skipped: u64,
    /// Writes whose displacement matched the client bit-for-bit (by-state
    /// replay only; zero elsewhere).
    pub exact_writes: u64,
    /// Writes inside the client's own frame-time jitter band (by-state only).
    pub jitter_writes: u64,
    /// Writes too small to compare: stop-transition settling (by-state only).
    pub settle_writes: u64,
    /// Writes excluded because their segment bends or changes size (touching
    /// geometry: a slide, a slope) and so is not free movement.
    pub env_writes: u64,
    /// Largest per-write displacement error in metres (by-state replay only).
    pub max_step_err: f32,
    /// Largest per-write *vertical* step error in metres (by-state replay
    /// only; the jump/gravity model against the client's own trace).
    pub max_vstep_err: f32,
    /// Largest per-write *change* in the vertical step (phase-invariant, so it
    /// verifies the gravity on jumps where the absolute dy is one write out of
    /// phase because the client's first jump write merges a partial frame).
    pub max_vdelta_err: f32,
    /// The first few divergent writes (sample, client dx/dz, model dx/dz, keys).
    pub divergent: Vec<(u64, f32, f32, f32, f32, Keys)>,
    /// Per-segment coverage: (keys, writes, compared, environment-screened,
    /// skipped at the input edge, settling, in the jitter band). This is what
    /// makes the gate's coverage auditable — which behaviours were actually
    /// compared, not just how many writes in total.
    pub segments: Vec<(Keys, u64, u64, u64, u64, u64, u64)>,
}

/// Replay a script through the model and find the first tick where its
/// position bits leave the client's trace.
///
/// The model starts from the trace's first sample position; the script clock
/// is aligned to that sample's timestamp plus `align_ms`. Velocity is not
/// compared: v1 of the model carries no horizontal velocity vector (M2 gives
/// it a dmBody-shaped body) — the caller's report says so rather than
/// claiming a divergence it cannot support.
pub fn diff_trace(
    trace: &Trace,
    steps: &[ScriptStep],
    align_ms: f32,
    speed: f32,
) -> Result<ReplayReport, String> {
    if trace.samples.is_empty() {
        return Err("trace has no samples".into());
    }
    if steps.is_empty() {
        return Err("script has no steps".into());
    }
    let start = &trace.samples[0];
    let start_pos = [
        f32::from_bits(start.pos[0]),
        f32::from_bits(start.pos[1]),
        f32::from_bits(start.pos[2]),
    ];
    let mut sim = Controller::new(MoverSpec::from_base_speed(speed), start_pos, 0.0);
    let dt = 1.0 / TICK_RATE_HZ;
    let mut ticks = 0u64;
    let base_t = start.t;
    let mut first = None;

    for (k, s) in trace.samples.iter().enumerate() {
        let elapsed = (s.t - base_t).max(0.0) + align_ms / 1000.0;
        let need = (elapsed * TICK_RATE_HZ).round() as u64;
        while ticks < need {
            let sim_t = ticks as f32 * dt;
            let st = step_at(steps, sim_t);
            sim.yaw = st.yaw_deg.to_radians();
            sim.advance(dt, st.input);
            ticks += 1;
        }
        let client = trace.expected_state(k);
        let actual = [sim.pos[0], sim.pos[1], sim.pos[2], 0.0, 0.0, 0.0];
        if let Some(d) = first_divergence(s.i, client, actual) {
            if d.field.starts_with("vel") {
                continue; // velocity not modelled yet; not a divergence claim
            }
            first = Some(d);
            break;
        }
    }

    let last_k = trace.samples.len() - 1;
    let last_client = trace.expected_state(last_k);
    let residuals = [
        ulp_distance(last_client[0], sim.pos[0]),
        ulp_distance(last_client[1], sim.pos[1]),
        ulp_distance(last_client[2], sim.pos[2]),
    ];
    Ok(ReplayReport {
        aligned_ticks: ticks,
        samples: trace.samples.len(),
        first,
        residuals,
        end_pos: sim.pos,
        skipped: 0,
        exact_writes: 0,
        jitter_writes: 0,
        settle_writes: 0,
        env_writes: 0,
        max_step_err: 0.0,
        max_vstep_err: 0.0,
        max_vdelta_err: 0.0,
        segments: Vec::new(),
        divergent: Vec::new(),
    })
}

/// Replay a trace through the model using the trace's **own recorded keys**.
///
/// This is the script-free path: `dev/live_trace.py record` writes the key
/// state with every sample, so one client trace is a complete
/// `(input, state)` timeline and needs no hand-written script to diff.
///
/// `jump_edges` chooses how the held space key is read — as held, or as
/// rising edges only. Which one the engine does is Q3, so the caller can run
/// both and compare; the report says which reading produced it.
pub fn diff_trace_keys(
    trace: &Trace,
    speed: f32,
    jump_edges: bool,
) -> Result<ReplayReport, String> {
    if trace.samples.is_empty() {
        return Err("trace has no samples".into());
    }
    if trace.samples.iter().any(|s| s.keys.is_none()) {
        return Err("trace carries no key timeline (was it recorded without keys?)".into());
    }
    let start = &trace.samples[0];
    let start_pos = [
        f32::from_bits(start.pos[0]),
        f32::from_bits(start.pos[1]),
        f32::from_bits(start.pos[2]),
    ];
    let mut sim = Controller::new(MoverSpec::from_base_speed(speed), start_pos, 0.0);
    let dt = 1.0 / TICK_RATE_HZ;
    let base_t = start.t;
    let mut ticks = 0u64;
    let mut first = None;
    let mut prev_space = false;

    let mut prev_keys = trace.samples[0].keys.expect("checked above");
    for (k, s) in trace.samples.iter().enumerate() {
        let elapsed = (s.t - base_t).max(0.0);
        let need = (elapsed * TICK_RATE_HZ).round() as u64;
        let keys = s.keys.expect("checked above");
        while ticks < need {
            // The tick that reaches this sample's time ran with the input
            // known at the previous sample: input is sampled at tick starts,
            // so a key change lands on the next tick, not this one.
            let mut input = prev_keys.to_input();
            if jump_edges {
                input.jump = prev_keys.space && !prev_space;
            }
            sim.advance(dt, input);
            ticks += 1;
            prev_space = prev_keys.space;
        }
        prev_keys = keys;
        let _ = k;
        let client = trace.expected_state(k);
        let actual = [sim.pos[0], sim.pos[1], sim.pos[2], 0.0, 0.0, 0.0];
        if let Some(d) = first_divergence(s.i, client, actual) {
            if d.field.starts_with("vel") {
                continue; // velocity not modelled yet; not a divergence claim
            }
            first = Some(d);
            break;
        }
    }

    let last_k = trace.samples.len() - 1;
    let last_client = trace.expected_state(last_k);
    let residuals = [
        ulp_distance(last_client[0], sim.pos[0]),
        ulp_distance(last_client[1], sim.pos[1]),
        ulp_distance(last_client[2], sim.pos[2]),
    ];
    Ok(ReplayReport {
        aligned_ticks: ticks,
        samples: trace.samples.len(),
        first,
        residuals,
        end_pos: sim.pos,
        skipped: 0,
        exact_writes: 0,
        jitter_writes: 0,
        settle_writes: 0,
        env_writes: 0,
        max_step_err: 0.0,
        max_vstep_err: 0.0,
        max_vdelta_err: 0.0,
        segments: Vec::new(),
        divergent: Vec::new(),
    })
}

/// The client's own per-write displacement jitters (frame-time integration);
/// this band is its measured envelope. Two sources, both measured on
/// `player_fast3_keys.jsonl`: the frame dt slips ~0.6% on individual writes
/// (±1.1 mm on a 0.178 m step) and the harness's per-segment dt estimate
/// carries the 200 Hz sample clock's quantisation (±0.9 mm). The model's
/// measured worst case across all 146 comparable writes is 2.29 mm.
pub const JITTER_BAND: f32 = 3.0e-3;

/// The vertical counterpart of [`JITTER_BAND`]: the gravity's per-frame
/// decrement jitters ~5% with the frame time (measured on the traced jump,
/// +/-0.002 m on a dy of ~0.14 m) and the per-segment dt estimate adds
/// ~0.002 m, so 6 mm is the honest envelope for a vertical step.
pub const VJITTER_BAND: f32 = 6.0e-3;

/// Writes smaller than this (either side) are stop-transition settling and
/// are not compared as movement.
pub const SETTLE_BAND: f32 = 1.0e-2;

/// Replay a keyed trace **by client state write**, comparing the model's
/// per-write displacement with the client's, bit for bit.
///
/// The traced node writes its position on the client's own mover writes
/// (~30 Hz), so one distinct position in the trace is one write. The model
/// advances one write (two 60 Hz ticks) per state, with the input delayed by
/// one write — the live ramp data shows a keydown only affects the client's
/// *second* write after the edge. The first two writes of every input
/// transition are skipped: the client's first write after an edge carries a
/// partial tick whose fraction depends on sub-write input timing, which no
/// black-box sampling can reproduce bit-exactly. Every write after those
/// must match exactly, or the model is not byte-identical.
///
/// Returns a report whose `first` is the first *compared* write with a
/// differing displacement.
pub fn diff_trace_keys_states(trace: &Trace, speed: f32) -> Result<ReplayReport, String> {
    if trace.samples.is_empty() {
        return Err("trace has no samples".into());
    }
    if trace.samples.iter().any(|s| s.keys.is_none()) {
        return Err("trace carries no key timeline (was it recorded without keys?)".into());
    }
    // Distinct client writes: (keys, yaw, pos bits).
    struct State {
        i: u64,
        t: f32,
        keys: Keys,
        yaw: f32,
        pos: Bits,
    }
    let mut states: Vec<State> = Vec::new();
    for s in &trace.samples {
        let Some(keys) = s.keys else { continue };
        if let Some(last) = states.last() {
            if last.pos == s.pos && last.yaw == s.yaw {
                continue;
            }
        } else {
            states.push(State {
                i: s.i,
                t: s.t,
                keys,
                yaw: s.yaw,
                pos: s.pos,
            });
            continue;
        }
        states.push(State {
            i: s.i,
            t: s.t,
            keys,
            yaw: s.yaw,
            pos: s.pos,
        });
    }
    // Traces recorded without a yaw field parse with yaw = 0 everywhere. The
    // client's step direction is (sin yaw, cos yaw) — measured on the flat
    // trace's straight plateaus against its recorded yaw to 0.006 deg (w:
    // -170.4387 against 189.5668; d: -90.0018; a: +89.9979) — so the yaw is
    // recoverable from the first free-movement step; a session that really sat
    // at yaw 0 would be inferred the same way, so nothing is lost. The first
    // step can read ~0.5 deg off when a previous velocity blends into it.
    if states.iter().all(|s| s.yaw == 0.0) {
        for k in 1..states.len() {
            let dx = f32::from_bits(states[k].pos[0]) - f32::from_bits(states[k - 1].pos[0]);
            let dz = f32::from_bits(states[k].pos[2]) - f32::from_bits(states[k - 1].pos[2]);
            if dx.hypot(dz) > SETTLE_BAND {
                let yaw = dx.atan2(dz).to_degrees();
                for s in states.iter_mut() {
                    s.yaw = yaw;
                }
                break;
            }
        }
    }
    // Segment-level environment screen: a segment whose client steps bend or
    // change size beyond the frame-jitter envelope is touching geometry (a
    // slide along a wall, a slope) and is not a free-movement comparison.
    // A *sustained* vertical of one sign is terrain too (a ramp, a slide); a
    // jump is not, because its vertical changes sign inside the segment.
    let mut seg_env = vec![false; states.len()];
    {
        let mut seg_start = 0usize;
        for k in 1..=states.len() {
            let boundary = k == states.len()
                || states[k].keys != states[k - 1].keys
                || states[k].yaw != states[k - 1].yaw;
            if !boundary {
                continue;
            }
            // segment = seg_start..k
            let mut dirs: Vec<(f32, f32, f32)> = Vec::new();
            // Skip the segment's own head (input ramp-in) and tail (ramp-out):
            // they legitimately change size; geometry shows up in the middle.
            for j in (seg_start + 1)..k {
                if j < seg_start + 4 || j + 1 >= k {
                    continue;
                }
                let dx = f32::from_bits(states[j].pos[0]) - f32::from_bits(states[j - 1].pos[0]);
                let dz = f32::from_bits(states[j].pos[2]) - f32::from_bits(states[j - 1].pos[2]);
                let m = (dx * dx + dz * dz).sqrt();
                dirs.push((dx, dz, m));
            }
            // A jump is identified by its input (space held here or in the
            // preceding segment, which covers the descent after the release),
            // and its vertical *is* the thing under test, so the vertical
            // screens do not apply to it.
            let jumpish = states[seg_start..k].iter().any(|s| s.keys.space)
                || (seg_start > 0 && states[seg_start - 1].keys.space);
            let big: Vec<(f32, f32, f32)> =
                dirs.iter().copied().filter(|d| d.2 > SETTLE_BAND).collect();
            if big.len() >= 4 {
                let med = big[big.len() / 2];
                let (mut bend, mut size_lo) = (0.0f32, f32::MAX);
                for d in &big {
                    let dot = (d.0 * med.0 + d.1 * med.1) / (d.2 * med.2).max(1e-9);
                    bend = bend.max(1.0 - dot);
                    size_lo = size_lo.min(d.2 / med.2);
                }
                // A direction bend means geometry even on a jump segment (a
                // wall). A *size* change does not: a jump that lands and
                // resumes from rest ramps its steps up mid-segment (the
                // landing is not an input edge, so the head/tail skip does not
                // cover it) — that is the movement itself, not terrain.
                if bend > 0.02 || (size_lo < 0.97 && !jumpish) {
                    for j in seg_start..k {
                        seg_env[j] = true;
                    }
                }
            }
            // The *stall* screen: a segment whose steps are uniformly far
            // below what the wish promised is walking into geometry.  The
            // size-change test above cannot see this — a wall slide keeps every
            // step small, so the ratio to its own median is 1.  Measured need:
            // the crouch/air captures (`player_ctrl4.jsonl`, 2026-10-15) spent
            // long stretches at 0.18 m/s with W held (the operator watched the
            // character pin against the map edge), and the gate compared them
            // as model error.  The promise is the slot speed at the segment's
            // own mean write dt; a segment that must ramp (airborne, or short)
            // is exempt, and the bar is loose (60%) so only real geometry
            // trips it.
            if !jumpish && k > seg_start + 6 {
                let k0 = states[seg_start].keys;
                let holds_direction = k0.w || k0.a || k0.s || k0.d;
                // The interior steps, whatever their size: a wall slide keeps
                // them *below* SETTLE_BAND, so a test built on the `big` set
                // above never sees it (measured: a 77-write stall at a 0.004 m
                // median step, only 2 steps above the settle band).
                let interior: Vec<f32> = ((seg_start + 4)..(k - 1))
                    .map(|j| {
                        let dx =
                            f32::from_bits(states[j].pos[0]) - f32::from_bits(states[j - 1].pos[0]);
                        let dz =
                            f32::from_bits(states[j].pos[2]) - f32::from_bits(states[j - 1].pos[2]);
                        (dx * dx + dz * dz).sqrt()
                    })
                    .collect();
                let n = k - seg_start;
                let dt_seg = (states[k - 1].t - states[seg_start].t) / (n as f32 - 1.0);
                let back_only = k0.s && !k0.w && !k0.a && !k0.d;
                let mut slot = if back_only { speed * 0.9 } else { speed };
                if k0.ctrl {
                    slot = slot.min(3.0);
                }
                let expected = slot * dt_seg;
                if holds_direction && expected > SETTLE_BAND {
                    let mut mags = interior.clone();
                    mags.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let med = mags[mags.len() / 2];
                    if med < 0.6 * expected {
                        for j in seg_start..k {
                            seg_env[j] = true;
                        }
                    }
                }
            }
            // Vertical terrain screen, two shapes. A *step*: any interior
            // frame moving vertically more than 1 cm (the display transform's
            // own lerp wobble stays under 6 mm). A *sustained* one-signed
            // vertical: a ramp, a ledge or a slide. A net height change alone
            // is *not* terrain — a flat segment that merely begins on a
            // different level than the previous segment's end is comparable,
            // because the model is aligned to the client's height at each
            // segment head.
            if !jumpish {
                for j in (seg_start + 1)..k {
                    let dy =
                        f32::from_bits(states[j].pos[1]) - f32::from_bits(states[j - 1].pos[1]);
                    if dy.abs() > 0.01 {
                        for jj in seg_start..k {
                            seg_env[jj] = true;
                        }
                        break;
                    }
                }
            }
            if !jumpish && k - seg_start >= 6 {
                let mut up = 0u32;
                let mut down = 0u32;
                for j in (seg_start + 1)..k {
                    let dy =
                        f32::from_bits(states[j].pos[1]) - f32::from_bits(states[j - 1].pos[1]);
                    if dy > 0.004 {
                        up += 1;
                    } else if dy < -0.004 {
                        down += 1;
                    }
                }
                let n = (k - seg_start - 1) as u32;
                // 0.75, not 0.6: a jump's key release splits its arc across
                // segments (a 9-up/6-down half reads as 64%), and the step
                // test above already catches ramps and ledges.
                if (up.max(down) as f32) > 0.75 * n as f32 {
                    for j in seg_start..k {
                        seg_env[j] = true;
                    }
                }
            }
            seg_start = k;
        }
    }
    // Per-segment frame dt. The client's writes land once per frame and the
    // frame's own dt drives the integration: the plateau step is exactly
    // `v * frame_dt` (5.5 * 0.032309 = 0.1777004 m) and the frame dt jitters a
    // fraction of a percent around ~0.0323 s in these captures. The sample
    // clock is 200 Hz, so a segment's average (t_last - t_first) / (n - 1) is
    // the honest estimate of its frame dt.
    let mut seg_dt = vec![1.0 / TICK_RATE_HZ; states.len()];
    {
        let mut seg_start = 0usize;
        for k in 1..=states.len() {
            let boundary = k == states.len()
                || states[k].keys != states[k - 1].keys
                || states[k].yaw != states[k - 1].yaw;
            if !boundary {
                continue;
            }
            let n = k - seg_start;
            if n >= 2 {
                let dt = (states[k - 1].t - states[seg_start].t) / (n as f32 - 1.0);
                if dt > 0.0 && dt < 1.0 {
                    for slot in seg_dt[seg_start..k].iter_mut() {
                        *slot = dt;
                    }
                }
            }
            seg_start = k;
        }
    }
    let start = &states[0];
    let mut sim = Controller::new(
        MoverSpec::from_base_speed(speed),
        [
            f32::from_bits(start.pos[0]),
            f32::from_bits(start.pos[1]),
            f32::from_bits(start.pos[2]),
        ],
        start.yaw.to_radians(),
    );
    // The vertical comparison is per segment: a trace crosses several ground
    // heights (0.71 at the start, -1.67 where the jump happens, -2.29 at the
    // end), so each compared segment starts aligned to the client's height and
    // gets its own ground. A jump keeps one ground from its take-off through
    // the descent after the key release, so the arc stays in scope.
    let mut ground_y = f32::from_bits(start.pos[1]);
    let mut prev_jumpish = false;
    sim.settle_ground(ground_y);
    let mut prev_keys = start.keys;
    let mut prev_yaw = start.yaw;
    let mut prev_t = start.t;
    let mut segment_age = 0u32;
    let mut compared = 0u64;
    let mut skipped = 0u64;
    let mut exact_writes = 0u64;
    let mut jitter_writes = 0u64;
    let mut settle_writes = 0u64;
    let mut env_writes = 0u64;
    let mut seg_rows: Vec<(Keys, u64, u64, u64, u64, u64, u64)> = Vec::new();
    let mut seg_row = 0usize;
    let mut max_step_err = 0.0f32;
    let mut max_vstep_err = 0.0f32;
    let mut divergent: Vec<(u64, f32, f32, f32, f32, Keys)> = Vec::new();
    let mut first = None;
    let mut prev_sim = [sim.pos[0], sim.pos[2]];
    let mut prev_client = [f32::from_bits(start.pos[0]), f32::from_bits(start.pos[2])];
    let mut prev_sim_y = sim.pos[1];
    let mut prev_client_y = ground_y;
    let mut prev_sim_dy = 0.0f32;
    let mut prev_cli_dy = 0.0f32;
    let mut max_vdelta_err = 0.0f32;
    let mut was_grounded = sim.grounded;
    let mut seg_jumpish = false;
    for (k, st) in states.iter().enumerate().skip(1) {
        if st.keys != prev_keys || st.yaw != prev_yaw {
            segment_age = 0;
        }
        segment_age += 1;
        let env = seg_env.get(k).copied().unwrap_or(false);
        let _ = env;
        let dt = seg_dt[k];
        // The client integrates *every* frame; a standing character writes
        // nothing (its position does not change), so a write gap is real time
        // the model must cross too. Advance the missing frames with the state
        // that was held: no input, so the velocity decays at FRICTION exactly
        // as the client's did. Without this the model carries a stale velocity
        // out of a standstill — a jump from rest after a long idle left the
        // ground at 0.81 m/s while the client left at 0 (found by a synthetic
        // crouch/air fixture; the real captures masked it behind the
        // input-transition skip).
        let gap_frames = (((st.t - prev_t) / dt).round() as i64 - 1).clamp(0, 600);
        if gap_frames > 0 {
            sim.yaw = prev_yaw.to_radians();
            let input = prev_keys.to_input();
            for _ in 0..gap_frames {
                sim.advance(dt, input);
                sim.settle_ground(ground_y);
            }
        }
        if segment_age == 1 {
            seg_rows.push((st.keys, 0, 0, 0, 0, 0, 0));
            seg_row = seg_rows.len() - 1;
        }
        seg_rows[seg_row].1 += 1;
        // Segment-level: space held in this segment or the previous one, which
        // covers the descent after the key release. Carried for the segment so
        // the whole arc (and its tail) is treated as one jump.
        if segment_age == 1 {
            seg_jumpish = st.keys.space || prev_keys.space;
        }
        let jumpish = seg_jumpish;
        if segment_age == 1 {
            if jumpish && !prev_jumpish {
                ground_y = prev_client_y; // take-off height
            } else if !jumpish {
                ground_y = prev_client_y; // local ground for this segment
            }
            if !(jumpish && prev_jumpish) {
                // Align the model's height at the segment head so the terrain
                // crossed in screened segments cannot poison the comparison.
                sim.pos[1] = prev_client_y;
                sim.vertical = 0.0;
                sim.grounded = true;
            }
        }
        prev_jumpish = jumpish;
        if env {
            env_writes += 1;
            seg_rows[seg_row].3 += 1;
            sim.yaw = prev_yaw.to_radians();
            let input = prev_keys.to_input();
            sim.advance(dt, input);
            prev_keys = st.keys;
            prev_yaw = st.yaw;
            prev_t = st.t;
            sim.settle_ground(ground_y);
            prev_sim = [sim.pos[0], sim.pos[2]];
            prev_client = [f32::from_bits(st.pos[0]), f32::from_bits(st.pos[2])];
            prev_sim_y = sim.pos[1];
            prev_client_y = f32::from_bits(st.pos[1]);
            continue;
        }
        // The model consumes the *previous* write's input (one-write delay).
        // One client write covers exactly one client frame: the traced steady
        // step is 5.5 m/s x 0.032309 s = 0.1777004 m, so one advance per write
        // — the frames inside a write gap were already advanced above.
        sim.yaw = prev_yaw.to_radians();
        let mut input = prev_keys.to_input();
        // The jump is edge-triggered and *not* delayed like the movement ramp:
        // the traced jump's first space write already carries the rise, so the
        // impulse uses the current write's keys. It is also a *rising edge*,
        // matching the sim's `Input` contract ("edge-triggered: jump this
        // tick") — a held space must not re-jump the model on landing, which
        // the client does not do (it bunny-hops nothing). No trace distinguishes
        // the two readings — every traced jump releases space before landing —
        // so this is the contract's reading, not a measurement of the client's
        // own edge handling (still unread, Q3).
        input.jump = st.keys.space && !prev_keys.space;
        sim.advance(dt, input);
        sim.settle_ground(ground_y);
        prev_keys = st.keys;
        prev_yaw = st.yaw;
        prev_t = st.t;
        let sim_pos = [sim.pos[0], sim.pos[2]];
        let cli_pos = [f32::from_bits(st.pos[0]), f32::from_bits(st.pos[2])];
        let sim_dy = sim.pos[1] - prev_sim_y;
        let cli_dy = f32::from_bits(st.pos[1]) - prev_client_y;
        let skip_writes = if !st.keys.w && !st.keys.a && !st.keys.s && !st.keys.d {
            3
        } else {
            2
        };
        if segment_age <= skip_writes {
            skipped += 1;
            seg_rows[seg_row].4 += 1;
        } else {
            compared += 1;
            seg_rows[seg_row].2 += 1;
            // Vertical: the jump/gravity model against the client's own trace.
            // Same transition skip as the horizontal (a jump's first write
            // merges the partial input frame with the next, which shifts the
            // whole arc by one write). The *deltas* of dy are phase-invariant
            // and are exactly g*dt^2, so they verify the gravity on jumps; the
            // absolute dy is checked on phase-aligned (non-jump) segments.
            // A landing is a snap clamp on both sides (the client's own ground
            // snap and the model's settle_ground), so the frame that lands is
            // a discontinuity, not an integration step.
            let landed = !was_grounded && sim.grounded;
            // The horizontal comparison's settling rule, mirrored: the traced
            // node is the display transform and its lerp leaves millimetre
            // wobbles on the flat. Both sides below the settle band means
            // nothing to compare.
            let vsettle = cli_dy.abs() < SETTLE_BAND && sim_dy.abs() < SETTLE_BAND;
            // A jump's phase is set by when the key actually landed inside a
            // frame, which no sampling can see (the traced take-off is 1-2
            // frames earlier than the first write that shows it). While both
            // sides are airborne the *deltas* of dy are phase-invariant and
            // are g*dt^2, so the arc verifies the gravity; the absolute dy and
            // the grounded tail are not comparable.
            let arc = jumpish && !sim.grounded && cli_dy.abs() > SETTLE_BAND;
            let vde = if landed || vsettle || !arc {
                0.0
            } else {
                ((sim_dy - prev_sim_dy) - (cli_dy - prev_cli_dy)).abs()
            };
            max_vdelta_err = max_vdelta_err.max(vde);
            if vde > VJITTER_BAND && first.is_none() {
                first = Some(crate::trace::Divergence {
                    sample: st.i,
                    field: "vdelta",
                    expected: cli_dy - prev_cli_dy,
                    actual: sim_dy - prev_sim_dy,
                    ulp: ulp_distance(cli_dy - prev_cli_dy, sim_dy - prev_sim_dy),
                });
            }
            if !jumpish && !landed && !vsettle {
                let ve = (sim_dy - cli_dy).abs();
                max_vstep_err = max_vstep_err.max(ve);
                if ve > VJITTER_BAND && first.is_none() {
                    first = Some(crate::trace::Divergence {
                        sample: st.i,
                        field: "vstep",
                        expected: cli_dy,
                        actual: sim_dy,
                        ulp: ulp_distance(cli_dy, sim_dy),
                    });
                }
            }
            let sim_d = [sim_pos[0] - prev_sim[0], sim_pos[1] - prev_sim[1]];
            let cli_d = [cli_pos[0] - prev_client[0], cli_pos[1] - prev_client[1]];
            let exact = sim_d[0].to_bits() == cli_d[0].to_bits()
                && sim_d[1].to_bits() == cli_d[1].to_bits();
            let cli_mag = (cli_d[0] * cli_d[0] + cli_d[1] * cli_d[1]).sqrt();
            let sim_mag = (sim_d[0] * sim_d[0] + sim_d[1] * sim_d[1]).sqrt();
            if cli_mag < SETTLE_BAND || sim_mag < SETTLE_BAND {
                seg_rows[seg_row].5 += 1;
                // Stop-transition settling: the traced node is the
                // animation-side transform, and its last few millimetres after
                // a stop are root motion, not the mover's integration.
                settle_writes += 1;
            } else if exact {
                exact_writes += 1;
            } else {
                let e = (sim_d[0] - cli_d[0]).abs().max((sim_d[1] - cli_d[1]).abs());
                max_step_err = max_step_err.max(e);
                // The client's write displacement is v x frame_time, and its
                // frame time jitters by ~0.6% (its own low bits move by one or
                // two ULP per write; occasionally a write lands a hair early).
                // Anything inside that band is timing, not model.
                if e <= JITTER_BAND {
                    jitter_writes += 1;
                    seg_rows[seg_row].6 += 1;
                } else {
                    if first.is_none() {
                        first = Some(crate::trace::Divergence {
                            sample: st.i,
                            field: "step",
                            expected: cli_d[0],
                            actual: sim_d[0],
                            ulp: ulp_distance(cli_d[0], sim_d[0]),
                        });
                    }
                    if divergent.len() < 40 {
                        divergent.push((st.i, cli_d[0], cli_d[1], sim_d[0], sim_d[1], st.keys));
                    }
                }
            }
        }
        prev_sim = sim_pos;
        prev_client = cli_pos;
        was_grounded = sim.grounded;
        prev_sim_dy = sim_dy;
        prev_cli_dy = cli_dy;
        prev_sim_y = sim.pos[1];
        prev_client_y = f32::from_bits(st.pos[1]);
    }
    let last = states.last().unwrap();
    let residuals = [
        ulp_distance(f32::from_bits(last.pos[0]), sim.pos[0]),
        ulp_distance(f32::from_bits(last.pos[1]), sim.pos[1]),
        ulp_distance(f32::from_bits(last.pos[2]), sim.pos[2]),
    ];
    Ok(ReplayReport {
        aligned_ticks: compared,
        samples: compared as usize,
        first,
        residuals,
        end_pos: sim.pos,
        skipped,
        exact_writes,
        jitter_writes,
        settle_writes,
        env_writes,
        max_step_err,
        max_vstep_err,
        max_vdelta_err,
        divergent,
        segments: seg_rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(i: u64, t: f32, x: f32, y: f32) -> Sample {
        Sample {
            i,
            t,
            pos: [x.to_bits(), y.to_bits(), 0f32.to_bits(), 1f32.to_bits()],
            vel: [0u32; 4],
            angvel: [0u32; 4],
            flags: vec![1, 0, 1],
            keys: None,
            yaw: 0.0,
        }
    }

    #[test]
    fn keys_roundtrip_through_the_wire_format() {
        let keys = Keys {
            w: true,
            d: true,
            space: false,
            ctrl: true,
            ..Default::default()
        };
        let mut s = sample(7, 0.1, 1.0, 2.0);
        s.keys = Some(keys);
        let trace = Trace {
            hz: 60.0,
            addr: "t".into(),
            samples: vec![s],
        };
        let text = trace.write("t");
        let parsed = Trace::parse(&text).expect("parses");
        assert_eq!(parsed.samples[0].keys, Some(keys), "line was: {text}");
        // And a keyless trace still parses (v1 traces predate key capture).
        let mut s2 = sample(8, 0.2, 1.0, 2.0);
        s2.keys = None;
        let text2 = Trace {
            hz: 60.0,
            addr: "t".into(),
            samples: vec![s2],
        }
        .write("t");
        assert_eq!(Trace::parse(&text2).expect("parses").samples[0].keys, None);
    }

    /// The script-free replay must reproduce a trace the model itself
    /// produced, bit for bit — the end-to-end check of recorder→parser→model.
    #[test]
    fn recorded_keys_replay_against_the_model_with_zero_divergence() {
        let mut sim = Controller::new(MoverSpec::from_base_speed(5.5), [0.0, 0.0, 0.0], 0.0);
        let walk = Keys {
            w: true,
            ..Default::default()
        };
        let strafe = Keys {
            w: true,
            d: true,
            ..Default::default()
        };
        let mut samples = Vec::new();
        for k in 0..120u64 {
            let keys = if k < 60 { walk } else { strafe };
            // Push the state after k ticks: the replay aligns tick k to the
            // sample's own time, so both schedules advance in lockstep.
            samples.push(Sample {
                i: k,
                t: k as f32 * (1.0 / TICK_RATE_HZ),
                pos: [
                    sim.pos[0].to_bits(),
                    sim.pos[1].to_bits(),
                    sim.pos[2].to_bits(),
                    0,
                ],
                vel: [0, sim.vertical.to_bits(), 0, 0],
                angvel: [0; 4],
                flags: vec![],
                keys: Some(keys),
                yaw: 0.0,
            });
            sim.advance(1.0 / TICK_RATE_HZ, keys.to_input());
        }
        let trace = Trace {
            hz: TICK_RATE_HZ,
            addr: "self".into(),
            samples,
        };
        let report = diff_trace_keys(&trace, 5.5, false).expect("replays");
        assert!(
            report.first.is_none(),
            "model vs itself must not diverge: {:?}",
            report.first
        );
        assert_eq!(report.residuals, [0, 0, 0]);
    }

    #[test]
    fn a_trace_without_keys_says_so() {
        let trace = Trace {
            hz: 60.0,
            addr: "t".into(),
            samples: vec![sample(0, 0.0, 0.0, 0.0), sample(1, 0.016, 0.0, 0.0)],
        };
        let err = diff_trace_keys(&trace, 5.5, false).unwrap_err();
        assert!(err.contains("key timeline"), "{err}");
    }

    #[test]
    fn keys_change_where_the_model_goes() {
        let mk = |keys: Keys| {
            let mut sim = Controller::new(MoverSpec::from_base_speed(5.5), [0.0, 0.0, 0.0], 0.0);
            for _ in 0..60 {
                sim.advance(1.0 / TICK_RATE_HZ, keys.to_input());
            }
            sim.pos
        };
        let fwd = mk(Keys {
            w: true,
            ..Default::default()
        });
        let back = mk(Keys {
            s: true,
            ..Default::default()
        });
        let side = mk(Keys {
            d: true,
            ..Default::default()
        });
        // Client convention at yaw 0: forward = +z, D (right) = -x.
        assert!(
            fwd[2] > 1.0 && back[2] < -1.0 && side[0] < -1.0,
            "{fwd:?} {back:?} {side:?}"
        );
    }

    #[test]
    fn ulp_distance_is_one_for_adjacent_floats() {
        assert_eq!(ulp_distance(1.0, f32::from_bits(1.0f32.to_bits() + 1)), 1);
        assert_eq!(ulp_distance(1.0, 1.0), 0);
        assert_eq!(ulp_distance(0.0, -0.0), 0);
        assert_eq!(ulp_distance(1.0, -1.0), ulp_distance(-1.0, 1.0));
    }

    #[test]
    fn parse_write_roundtrip_is_lossless() {
        let trace = Trace {
            hz: 60.0,
            addr: "0x1234".into(),
            samples: vec![sample(0, 0.0, 1.5, -2.5), sample(1, 0.016, 3.25, 0.125)],
        };
        let text = trace.write(&trace.addr);
        let back = Trace::parse(&text).expect("roundtrip");
        assert_eq!(back.hz, 60.0);
        assert_eq!(back.addr, "0x1234");
        assert_eq!(back.samples.len(), 2);
        assert_eq!(back.samples[0].pos, trace.samples[0].pos);
        assert_eq!(back.samples[1].vel, trace.samples[1].vel);
        assert_eq!(back.samples[0].flags, vec![1, 0, 1]);
        // Bit-exact expected-state reconstruction, including the -2.5 sign.
        let e = back.expected_state(0);
        assert_eq!(e[0], 1.5);
        assert_eq!(e[1], -2.5);
    }

    #[test]
    fn malformed_line_is_an_error_not_a_skip() {
        let text =
            "{\"trace\":1,\"hz\":60.0,\"addr\":\"0x1\"}\n{\"i\":0,\"t\":0.0,\"pos\":[1,2,3,4]}\n";
        assert!(Trace::parse(text).is_err());
    }

    #[test]
    fn first_divergence_names_the_field_and_counts_ulps() {
        let expected = [1.0f32, 2.0, 3.0, 0.0, 0.0, 0.0];
        let mut actual = expected;
        actual[4] = f32::from_bits(0f32.to_bits() + 7); // tiny denormal
        let d = first_divergence(42, expected, actual).expect("diverges");
        assert_eq!(d.sample, 42);
        assert_eq!(d.field, "vel.y");
        assert_eq!(d.ulp, 7);
        assert!(first_divergence(7, expected, expected).is_none());
    }

    /// The gate on the shape the crouch capture will produce: a walk, both
    /// crouch transitions, a standstill (a write gap), a moving jump and a
    /// standing jump with W pressed mid-air — generated *by the sim itself* and
    /// replayed through the gate.  Pins the two fixes the synthetic python
    /// fixture (`dev/synth_ctrl_trace.py`) found: a landing's mid-segment ramp
    /// must not be terrain-screened, and the frames inside a write gap must
    /// advance; and it pins the crouch/air coverage, all offline.
    #[test]
    fn gate_covers_crouch_and_air_segments() {
        const DT: f32 = 0.032309;
        fn k(w: bool, space: bool, ctrl: bool) -> Keys {
            Keys {
                w,
                a: false,
                s: false,
                d: false,
                space,
                ctrl,
                shift: false,
                lmb: false,
                rmb: false,
            }
        }
        let plan = [
            (k(false, false, false), 1.0), // idle
            (k(true, false, false), 2.0),  // walk
            (k(true, false, true), 2.0),   // walk -> crouch walk
            (k(true, false, false), 1.5),  // crouch -> walk
            (k(false, false, true), 1.0),  // crouch from standstill
            (k(true, false, true), 2.0),   // crouch walk from rest
            (k(false, false, false), 1.3), // standstill: the write gap
            (k(true, false, false), 0.5),  // run-up
            (k(true, true, false), 0.1),   // the moving jump's rise
            (k(true, false, false), 2.4),  // the arc and the landing
            (k(false, false, false), 1.0),
            (k(false, true, false), 0.15), // the standing jump's rise
            (k(true, false, false), 1.85), // ... W pressed mid-air
            (k(false, false, false), 1.0),
        ];
        let mut sim = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        let mut samples = Vec::new();
        let (mut t, mut i) = (0.0f32, 0u64);
        let mut prev_keys = k(false, false, false);
        for (keys, secs) in plan {
            for _ in 0..((secs / DT).round() as u32).max(1) {
                // The client's writes carry the *previous* frame's movement
                // input (the gate's one-write delay, measured on the ground
                // ramp), so the fixture must too — without it the air ramp
                // this fixture now generates sits one write ahead of the
                // gate's own replay. The jump is the rising edge of the
                // *current* keys, as the sim's Input contract and the gate
                // both read it.
                let mut input = prev_keys.to_input();
                input.jump = keys.space && !prev_keys.space;
                sim.advance(DT, input);
                // The ground is the caller's job (the differ and both shells
                // settle after each step); without this the character never
                // lands and the whole timeline is a free fall.
                sim.settle_ground(0.0);
                prev_keys = keys;
                samples.push(Sample {
                    i,
                    t,
                    pos: [
                        sim.pos[0].to_bits(),
                        sim.pos[1].to_bits(),
                        sim.pos[2].to_bits(),
                        0,
                    ],
                    vel: [0; 4],
                    angvel: [0; 4],
                    flags: Vec::new(),
                    keys: Some(keys),
                    yaw: 0.0,
                });
                i += 1;
                t += DT;
            }
        }
        let trace = Trace {
            hz: 1.0 / DT,
            addr: "gate coverage test".into(),
            samples,
        };
        let report = diff_trace_keys_states(&trace, 5.5).expect("the gate runs");
        assert!(report.first.is_none(), "gate diverged: {:?}", report.first);
        assert_eq!(report.env_writes, 0, "nothing in this timeline is terrain");
        let crouch_rows: Vec<_> = report.segments.iter().filter(|r| r.0.ctrl).collect();
        assert!(!crouch_rows.is_empty(), "crouch segments exist");
        let crouch_compared: u64 = crouch_rows.iter().map(|r| r.2).sum();
        assert!(
            crouch_compared >= 100,
            "both crouch plateaus are compared, not screened: {crouch_compared} writes"
        );
        // The jump's coverage lives in the segment *after* the rise (the arc
        // and its landing ramp), which is jumpish via the previous segment.
        let arc_compared = report
            .segments
            .windows(2)
            .filter(|w| w[0].0.space && !w[1].0.space)
            .map(|w| w[1].2)
            .max()
            .unwrap_or(0);
        assert!(
            arc_compared >= 60,
            "the post-jump arc is compared: {arc_compared} writes"
        );
    }
}
