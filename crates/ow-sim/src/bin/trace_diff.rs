//! The trace differ: replay a client trace (or a declared input script)
//! through `ow-sim` and find the first sample where it leaves the trace.
//!
//! ```text
//!     cargo run -p ow-sim --bin trace_diff -- selftest
//!         Instrument check: builds a trace from the model itself, round-trips
//!         it through the wire format, diffs it (expects zero divergence),
//!         then flips one bit and expects exactly that one divergence to be
//!         found. This validates the harness, not the model.
//!
//!     cargo run -p ow-sim --bin trace_diff -- keys <trace.jsonl> [--by-state] [--speed X] [--edges]
//!         Replays the trace's own key timeline.  With `--by-state` this is
//!         *the gate*: per client write, with the measured frame-jitter band
//!         and the terrain/transition screens, plus the per-segment coverage
//!         table (`docs/formats/character-controller.md`).  Without it, the
//!         older bit-exact per-sample comparison; `--edges` chooses how the
//!         held space key is read there.
//!
//!     cargo run -p ow-sim --bin trace_diff -- diff <trace.jsonl> --script <s.txt> [--align-ms N] [--speed X]
//!         Replays the script from the trace's first sample position; bit-exact
//!         per sample by design (the M0 instrument).  Reports the first sample
//!         whose position bits differ, with the ULP distance.
//! ```
//!
//! Script format: one step per line, `t forward side crouch jump yaw_deg`,
//! `#` starts a comment. A step applies from time `t` until the next step.
//! Example (`hold W for 2 s, then nothing`):
//!
//! ```text
//!     # t    fwd side crouch jump yaw
//!       0.0    1    0      0    0   0
//!       2.0    0    0      0    0   0
//! ```
//!
//! Exit codes: 0 = no divergence (or selftest pass), 1 = divergence / selftest
//! fail, 2 = usage or parse error.

use std::process::ExitCode;

use ow_sim::movement::{Controller, Input, MoverSpec, TICK_RATE_HZ};
use ow_sim::trace::{
    diff_trace, diff_trace_keys, diff_trace_keys_states, first_divergence, parse_script, Sample,
    Trace,
};

fn selftest() -> bool {
    let steps = parse_script("0 1 0 0 0 0\n").expect("builtin script");
    let input = Input {
        forward: 1.0,
        ..Default::default()
    };

    // Reference: run the model, capture into the wire format, parse it back.
    let mut reference = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
    let mut samples = Vec::new();
    let mut series = Vec::new();
    for k in 0..120u64 {
        reference.advance(1.0 / TICK_RATE_HZ, input);
        samples.push(Sample {
            i: k,
            t: (k as f32 + 1.0) * (1.0 / TICK_RATE_HZ),
            pos: [
                reference.pos[0].to_bits(),
                reference.pos[1].to_bits(),
                reference.pos[2].to_bits(),
                0,
            ],
            vel: [0, reference.vertical.to_bits(), 0, 0],
            angvel: [0; 4],
            flags: vec![],
            keys: None,
            yaw: 0.0,
        });
        series.push([
            reference.pos[0],
            reference.pos[1],
            reference.pos[2],
            0.0,
            reference.vertical,
            0.0,
        ]);
    }
    let trace = Trace {
        hz: TICK_RATE_HZ,
        addr: "selftest".into(),
        samples,
    };
    let text = trace.write("selftest");
    let parsed = match Trace::parse(&text) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("FAIL: selftest trace did not parse: {e}");
            return false;
        }
    };
    if parsed.samples.len() != 120 {
        eprintln!(
            "FAIL: selftest round-trip lost samples ({} != 120)",
            parsed.samples.len()
        );
        return false;
    }

    // Replay the parsed trace through a fresh model: must be identically zero.
    let mut replay = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
    for k in 0..120usize {
        replay.advance(1.0 / TICK_RATE_HZ, input);
        let actual = [
            replay.pos[0],
            replay.pos[1],
            replay.pos[2],
            0.0,
            replay.vertical,
            0.0,
        ];
        if let Some(d) = first_divergence(k as u64, parsed.expected_state(k), actual) {
            eprintln!(
                "FAIL: sim vs itself diverged at sample {} on {} ({:?} vs {:?}, {} ulp)",
                d.sample, d.field, d.expected, d.actual, d.ulp
            );
            return false;
        }
    }
    let _ = steps;

    // Perturb one bit of sample 60's pos.x in the reference series.
    let mut perturbed = series.clone();
    perturbed[60][0] = f32::from_bits(series[60][0].to_bits() ^ 1);
    let mut found = None;
    for k in 0..120usize {
        if let Some(d) = first_divergence(k as u64, perturbed[k], series[k]) {
            found = Some(d);
            break;
        }
    }
    match found {
        Some(d) if d.sample == 60 && d.field == "pos.x" && d.ulp == 1 => {
            println!("selftest: PASS (round-trip exact; single-bit perturbation found at sample 60, pos.x, 1 ulp)");
            true
        }
        other => {
            eprintln!("FAIL: perturbed bit not located correctly: {other:?}");
            false
        }
    }
}

fn diff(trace_path: &str, script_path: &str, align_ms: f32, speed: f32) -> Result<bool, String> {
    let text = std::fs::read_to_string(trace_path).map_err(|e| format!("{trace_path}: {e}"))?;
    let trace = Trace::parse(&text).map_err(|e| format!("{trace_path}: {e}"))?;
    if trace.samples.is_empty() {
        return Err("trace has no samples".into());
    }
    let script_text =
        std::fs::read_to_string(script_path).map_err(|e| format!("{script_path}: {e}"))?;
    let steps = parse_script(&script_text)?;

    let start = &trace.samples[0];
    let start_pos = [
        f32::from_bits(start.pos[0]),
        f32::from_bits(start.pos[1]),
        f32::from_bits(start.pos[2]),
    ];
    println!(
        "trace {}: {} samples @ {:.0} Hz from {} (start pos {:.3} {:.3} {:.3})",
        trace_path,
        trace.samples.len(),
        trace.hz,
        trace.addr,
        start_pos[0],
        start_pos[1],
        start_pos[2]
    );
    println!("model: speed {speed} m/s, aligning script by {align_ms} ms; comparing position bits");

    let report = diff_trace(&trace, &steps, align_ms, speed)?;
    if let Some(d) = &report.first {
        let s = trace
            .samples
            .iter()
            .find(|s| s.i == d.sample)
            .unwrap_or(&trace.samples[0]);
        println!(
            "FIRST DIVERGENCE  sample {}  t={:.4}s  {}  client {:?}  model {:?}  {} ulp  (|d|={:.6})",
            d.sample,
            s.t - start.t,
            d.field,
            d.expected,
            d.actual,
            d.ulp,
            (d.expected - d.actual).abs()
        );
        return Ok(false);
    }
    let last = &trace.samples[trace.samples.len() - 1];
    println!(
        "no position divergence across {} samples; final ULP residual x/y/z = {:?}; last t={:.4}s",
        report.samples,
        report.residuals,
        last.t - start.t
    );
    Ok(true)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    match args.first().map(String::as_str) {
        Some("selftest") => {
            if selftest() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Some("diff") => {
            let Some(trace) = args.get(1) else {
                eprintln!("usage: trace_diff diff <trace.jsonl> --script <s.txt> [--align-ms N] [--speed X]");
                return ExitCode::from(2);
            };
            let Some(script) = flag("--script") else {
                eprintln!("diff needs --script <s.txt>");
                return ExitCode::from(2);
            };
            let align = flag("--align-ms")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            let speed = flag("--speed").and_then(|v| v.parse().ok()).unwrap_or(5.5);
            match diff(trace, &script, align, speed) {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) => ExitCode::from(1),
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Some("keys") => {
            let Some(path) = args.get(1) else {
                eprintln!(
                    "usage: trace_diff keys <trace.jsonl> [--by-state] [--speed X] [--edges]"
                );
                return ExitCode::from(2);
            };
            let speed = flag("--speed").and_then(|v| v.parse().ok()).unwrap_or(5.5);
            let edges = args.iter().any(|a| a == "--edges");
            let text = match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error: {path}: {e}");
                    return ExitCode::from(2);
                }
            };
            let trace = match Trace::parse(&text) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("error: {path}: {e}");
                    return ExitCode::from(2);
                }
            };
            let start = trace.samples.first();
            if let Some(s0) = start {
                println!(
                    "trace {}: {} samples @ {:.0} Hz from {} (start pos {:.3} {:.3} {:.3})",
                    path,
                    trace.samples.len(),
                    trace.hz,
                    trace.addr,
                    f32::from_bits(s0.pos[0]),
                    f32::from_bits(s0.pos[1]),
                    f32::from_bits(s0.pos[2]),
                );
            }
            let by_state = args.iter().any(|a| a == "--by-state");
            println!(
                "model: speed {speed} m/s, jump read as {}; replaying the trace's own keys{}",
                if by_state || edges {
                    "rising edges"
                } else {
                    "held"
                },
                if by_state {
                    ", comparing per client write (the gate)"
                } else {
                    ""
                }
            );
            let report = if by_state {
                diff_trace_keys_states(&trace, speed)
            } else {
                diff_trace_keys(&trace, speed, edges)
            };
            match report {
                Ok(report) => match &report.first {
                    Some(d) => {
                        println!(
                            "FIRST DIVERGENCE  sample {}  {}  client {:?}  model {:?}  {} ulp  (|d|={:.6})",
                            d.sample,
                            d.field,
                            d.expected,
                            d.actual,
                            d.ulp,
                            (d.expected - d.actual).abs()
                        );
                        if report.samples > 0 && report.skipped > 0 || report.exact_writes > 0 {
                            println!(
                                "writes: {} exact, {} in the client's frame-jitter band, {} settling, {} environment-affected, {} compared, {} skipped at transitions; max step deviation {:.7} m; max vertical step deviation {:.7} m; max vertical delta error {:.7} m",
                                report.exact_writes,
                                report.jitter_writes,
                                report.settle_writes,
                                report.env_writes,
                                report.samples,
                                report.skipped,
                                report.max_step_err,
                                report.max_vstep_err,
                                report.max_vdelta_err
                            );
                            for (i, cx, cz, mx, mz, k) in report.divergent.iter().take(12) {
                                println!(
                                    "  diverging write at sample {i}: client ({cx:+.7},{cz:+.7}) model ({mx:+.7},{mz:+.7}) keys w{} a{} s{} d{}",
                                    k.w as u8, k.a as u8, k.s as u8, k.d as u8
                                );
                            }
                        }
                        ExitCode::from(1)
                    }
                    None => {
                        println!("per-segment coverage (keys: writes / compared / env-screened / skipped / settling / jitter-band):");
                        for (k, w, cmp, env, skip, settle, jit) in &report.segments {
                            println!(
                                "  w{} a{} s{} d{} space{} ctrl{}: {} writes / {} compared / {} env / {} skipped / {} settling / {} jitter",
                                k.w as u8, k.a as u8, k.s as u8, k.d as u8,
                                k.space as u8, k.ctrl as u8, w, cmp, env, skip, settle, jit
                            );
                        }
                        println!(
                            "no divergence across {} compared write(s): {} exact, {} in the client's frame-jitter band, {} settling, {} environment-affected, {} skipped at input transitions; max step deviation {:.7} m; max vertical step deviation {:.7} m; max vertical delta error {:.7} m; final ULP residual x/y/z = {:?}",
                            report.samples,
                            report.exact_writes,
                            report.jitter_writes,
                            report.settle_writes,
                            report.env_writes,
                            report.skipped,
                            report.max_step_err,
                            report.max_vstep_err,
                            report.max_vdelta_err,
                            report.residuals
                        );
                        ExitCode::SUCCESS
                    }
                },
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            }
        }
        _ => {
            eprintln!("usage: trace_diff selftest | trace_diff diff <t.jsonl> --script <s.txt> | trace_diff keys <t.jsonl>");
            ExitCode::from(2)
        }
    }
}
