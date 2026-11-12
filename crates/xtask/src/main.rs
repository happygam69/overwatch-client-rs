//! The project's own commands: one way to ask "is this tree healthy?".
//!
//! ```text
//! cargo xtask verify                    # the full gate: fmt, clippy, every test
//! cargo xtask verify --fast             # the same without the install-reading tests
//! cargo xtask verify --fail-fast        # stop at the first failing stage
//! cargo xtask verify --test-jobs N      # run N test binaries at once
//! cargo xtask verify -p ow-casc         # clippy and tests for one crate
//! cargo xtask verify --skip index_find_real   # leave the 19-minute suite out
//! cargo xtask dev-index                 # are the tool tables in docs/tools.md current?
//! cargo xtask deviation-check           # does the code match the deviations table?
//! cargo xtask frame-check               # do the docs cite only the one address frame?
//! ```
//!
//! The gate is what "done" means in this project: a change is done when
//! `cargo xtask verify` passes and the docs that describe it are updated.
//! The cheap doc checks run before the compiling stages so a stale table
//! is visible in seconds, and the test binaries — which `cargo test`
//! otherwise walks one at a time, re-opening the install in each — run in
//! a parallel pool. The default pool is four binaries at once; the
//! install suites are read-only, so they interleave safely and share the
//! page cache.

use std::process::{Command, ExitCode, Stdio};
use std::thread;

mod json;
mod test_runner;

use json::Value;

fn run(label: &str, program: &str, args: &[&str], env: Option<(&str, &str)>) -> bool {
    println!("\n== {label} ==");
    let mut cmd = Command::new(program);
    cmd.args(args);
    if let Some((k, v)) = env {
        cmd.env(k, v);
    }
    match cmd.status() {
        Ok(status) if status.success() => {
            println!("-- {label}: ok");
            true
        }
        Ok(status) => {
            println!("-- {label}: FAILED ({status})");
            false
        }
        Err(e) => {
            println!("-- {label}: could not run {program} ({e})");
            false
        }
    }
}

fn python(script: &str, args: &[&str]) -> bool {
    let mut all: Vec<String> = vec![format!("dev/{script}")];
    all.extend(args.iter().map(|a| a.to_string()));
    let all: Vec<&str> = all.iter().map(String::as_str).collect();
    // Windows names its interpreter `python`; most Linux distributions ship
    // only `python3`. Try both so the gate runs on either.
    let ok = run(&format!("{script} (python)"), "python", &all, None)
        || run(&format!("{script} (python3)"), "python3", &all, None);
    ok
}

/// The flags `verify` accepts. Parsed strictly: a typo'd flag is an error,
/// not a silent no-op.
struct VerifyArgs {
    fast: bool,
    fail_fast: bool,
    jobs: Option<usize>,
    /// `-p`/`--package`: scope clippy and the tests to these crates. Empty
    /// means the whole workspace, which is the gate's definition of done.
    packages: Vec<String>,
    /// `--skip`: a test-name filter handed to every test binary (libtest's
    /// own `--skip`). Empty runs everything.
    skips: Vec<String>,
}

fn parse_verify_args(args: &[String]) -> Result<VerifyArgs, String> {
    let mut out = VerifyArgs {
        fast: false,
        fail_fast: false,
        jobs: None,
        packages: Vec::new(),
        skips: Vec::new(),
    };
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        match arg {
            "--fast" => out.fast = true,
            "--fail-fast" => out.fail_fast = true,
            "--test-jobs" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--test-jobs needs a number (try --test-jobs 4)".into());
                };
                out.jobs = Some(parse_job_count(value)?);
                i += 1;
            }
            other if other.starts_with("--test-jobs=") => {
                out.jobs = Some(parse_job_count(other.trim_start_matches("--test-jobs="))?);
            }
            "--package" | "-p" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--package needs a crate name (try -p ow-casc)".into());
                };
                out.packages.push(parse_spec("--package", value)?);
                i += 1;
            }
            other if other.starts_with("--package=") => out.packages.push(parse_spec(
                "--package",
                other.trim_start_matches("--package="),
            )?),
            "--skip" => {
                let Some(value) = args.get(i + 1) else {
                    return Err("--skip needs a test name (try --skip index_find_real)".into());
                };
                out.skips.push(parse_spec("--skip", value)?);
                i += 1;
            }
            other if other.starts_with("--skip=") => {
                out.skips
                    .push(parse_spec("--skip", other.trim_start_matches("--skip="))?);
            }
            other => return Err(format!("unknown flag: {other}")),
        }
        i += 1;
    }
    Ok(out)
}

/// A `--package` or `--skip` value: a non-empty string, because an empty
/// one would either scope to nothing or skip everything.
fn parse_spec(flag: &str, text: &str) -> Result<String, String> {
    if text.is_empty() {
        Err(format!("{flag}: the value is empty"))
    } else {
        Ok(text.to_string())
    }
}

fn parse_job_count(text: &str) -> Result<usize, String> {
    text.parse::<usize>()
        .map_err(|_| format!("--test-jobs: {text:?} is not a number"))
        .and_then(|n| {
            if n == 0 {
                Err("--test-jobs must be at least 1".into())
            } else {
                Ok(n)
            }
        })
}

/// `--test-jobs` beats the `XTASK_TEST_JOBS` environment variable, which
/// beats the default of four (or the core count, whichever is smaller).
/// A malformed environment value is an error, not a silent default.
fn resolve_jobs(cli: Option<usize>) -> Result<usize, String> {
    if let Some(n) = cli {
        return Ok(n);
    }
    match std::env::var("XTASK_TEST_JOBS") {
        Ok(text) => parse_job_count(&text)
            .map_err(|e| format!("XTASK_TEST_JOBS: {e}"))
            .map(|n| n.max(1)),
        Err(_) => Ok(default_jobs()),
    }
}

fn default_jobs() -> usize {
    thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(4)
}

/// `--workspace` when no crate was named, else one `-p <spec>` per name.
/// Cargo accepts both on clippy and on test, so the scoping is one list.
fn scope_args(packages: &[String]) -> Vec<String> {
    if packages.is_empty() {
        return vec!["--workspace".to_string()];
    }
    packages
        .iter()
        .flat_map(|p| ["-p".to_string(), p.clone()])
        .collect()
}

fn run_owned(label: &str, program: &str, args: &[String]) -> bool {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(label, program, &refs, None)
}

/// The named packages that actually have a library target. `cargo test
/// --doc -p <bin-only crate>` is an error ("no library targets found"),
/// so the scoped doc stage asks `cargo metadata` first. A spec this does
/// not recognise is passed through unchanged: cargo's own error message
/// beats anything guessed here, and a failed metadata call must not turn
/// a green run red.
fn lib_packages(packages: &[String]) -> Vec<String> {
    let Ok(out) = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
    else {
        return packages.to_vec();
    };
    let Ok(doc) = json::parse(&String::from_utf8_lossy(&out.stdout)) else {
        return packages.to_vec();
    };
    let Some(entries) = doc.get("packages").and_then(Value::as_arr) else {
        return packages.to_vec();
    };
    let has_lib = |pkg: &Value| {
        pkg.get("targets")
            .and_then(Value::as_arr)
            .unwrap_or(&[])
            .iter()
            .any(|target| {
                target
                    .get("kind")
                    .and_then(Value::as_arr)
                    .unwrap_or(&[])
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|kind| kind == "lib" || kind == "rlib" || kind == "proc-macro")
            })
    };
    packages
        .iter()
        .filter(|want| {
            let named = entries
                .iter()
                .find(|pkg| pkg.get("name").and_then(Value::as_str) == Some(want.as_str()));
            named.is_none_or(has_lib)
        })
        .cloned()
        .collect()
}

fn verify(args: &VerifyArgs, jobs: usize) -> bool {
    let fast = args.fast;
    let fail_fast = args.fail_fast;
    // Cheap first: format and the four doc checks answer in seconds, so a
    // stale table is visible (and, with --fail-fast, fatal) before the
    // workspace spends minutes compiling.
    let mut ok = run("cargo fmt --check", "cargo", &["fmt", "--check"], None);
    ok &= python("make_index.py", &["--check"]);
    ok &= python("deviation_check.py", &["--check"]);
    ok &= python("frame_check.py", &["--check"]);
    ok &= python("pe_layout.py", &["--check"]);
    if !ok && fail_fast {
        return false;
    }

    let mut clippy: Vec<String> = vec!["clippy".to_string()];
    clippy.extend(scope_args(&args.packages));
    clippy.extend(
        ["--all-targets", "--", "-D", "warnings"]
            .iter()
            .map(|s| s.to_string()),
    );
    ok &= run_owned("cargo clippy", "cargo", &clippy);
    if !ok && fail_fast {
        return false;
    }

    // The install-reading tests are the slow half and they live in the
    // integration targets (`tests/*.rs`), which skip themselves when no
    // install is present. The fast path therefore keeps every library target
    // in the workspace - including the verified math in `ow-sim` and the
    // D3D11 units in `ow-render` - and drops the integration and doc targets.
    ok &= run_tests(fast, jobs, &args.packages, &args.skips);
    if !ok && fail_fast {
        return false;
    }

    if !fast {
        // Doc tests are compiled by this invocation itself (`--no-run`
        // does not cover them), and `--no-fail-fast` keeps every crate's
        // doc suite in the report once one fails. Scoped runs only take the
        // named crates that have a library at all.
        let scoped = (!args.packages.is_empty()).then(|| lib_packages(&args.packages));
        match scoped.as_deref() {
            Some([]) => println!(
                "\n== cargo test (doc) ==\n-- cargo test (doc): no library targets in the named crates"
            ),
            Some(libs) => {
                let mut doc: Vec<String> = vec!["test".to_string(), "--release".to_string()];
                for name in libs {
                    doc.push("-p".to_string());
                    doc.push(name.clone());
                }
                doc.extend(["--doc", "--no-fail-fast"].iter().map(|s| s.to_string()));
                ok &= run_owned("cargo test (doc)", "cargo", &doc);
            }
            None => {
                let mut doc: Vec<String> = vec![
                    "test".to_string(),
                    "--release".to_string(),
                    "--workspace".to_string(),
                ];
                doc.extend(["--doc", "--no-fail-fast"].iter().map(|s| s.to_string()));
                ok &= run_owned("cargo test (doc)", "cargo", &doc);
            }
        }
    }
    ok
}

/// Builds the test binaries once, then runs them in a parallel pool.
///
/// One `cargo test` invocation would walk the binaries serially, and each
/// install-reading suite re-opens the install before it starts; with a
/// 19-minute suite in the set, the serial walk sets the gate's floor.
/// Building with `--no-run` and running the executables ourselves also
/// runs every binary even after one fails, where `cargo test` stops at the
/// first failing one — the detached gate log then names every failure, not
/// just the first.
fn run_tests(fast: bool, jobs: usize, packages: &[String], skips: &[String]) -> bool {
    let mut base: Vec<String> = vec!["test".to_string(), "--release".to_string()];
    base.extend(scope_args(packages));
    if fast {
        base.push("--lib".to_string());
    }

    println!("\n== cargo test (build) ==");
    let mut cmd = Command::new("cargo");
    cmd.args(&base)
        .arg("--no-run")
        .arg("--message-format=json")
        .stdout(Stdio::piped())
        // Compile progress stays live on stderr; stdout carries the JSON.
        .stderr(Stdio::inherit());
    let Ok(out) = cmd.output() else {
        println!("-- cargo test (build): could not run cargo");
        return false;
    };
    if !out.status.success() {
        // JSON mode hides rustc's rendered diagnostics. Rebuild without it
        // so the failure is readable; the cache is warm, so this is cheap.
        let mut plain = base.clone();
        plain.push("--no-run".to_string());
        run_owned("cargo test (build, plain errors)", "cargo", &plain);
        println!("-- cargo test (build): FAILED ({})", out.status);
        return false;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let binaries = test_runner::parse_binaries(stdout.lines());
    if binaries.is_empty() {
        println!("-- cargo test (build): cargo reported no test binaries");
        return false;
    }
    println!(
        "-- cargo test (build): ok ({} binaries, running {} at a time)",
        binaries.len(),
        jobs
    );

    println!("\n== cargo test (run) ==");
    // A skip name can mean the whole suite (`index_find_real` is a binary)
    // or one test inside every binary; both are supported because both are
    // what the operator wants to type. A binary whose label matches is not
    // run at all; the rest get libtest's own `--skip`. A bare positional
    // argument would be libtest's *filter* (run only matching tests), the
    // opposite of the intent, so the flag is always spelled out.
    let (skipped, keep): (Vec<test_runner::TestBinary>, Vec<test_runner::TestBinary>) = binaries
        .into_iter()
        .partition(|b| skips.iter().any(|name| b.label.contains(name.as_str())));
    for binary in &skipped {
        println!("-- skipping binary: {}", binary.label);
    }
    let test_args: Vec<String> = skips
        .iter()
        .flat_map(|name| ["--skip".to_string(), name.clone()])
        .collect();
    if !skips.is_empty() {
        println!("-- skipping tests matching: {}", skips.join(", "));
    }
    let results = test_runner::run_all(&keep, jobs, &test_args);
    let failed: Vec<&test_runner::BinaryResult> = results.iter().filter(|r| !r.ok).collect();
    for result in &failed {
        println!("\n[failure output: {}]\n{}", result.label, result.output);
    }
    let wall = results
        .iter()
        .map(|r| r.duration)
        .sum::<std::time::Duration>();
    println!(
        "-- cargo test (run): {} binaries, {} failed, total {}",
        results.len(),
        failed.len(),
        test_runner::fmt_duration(wall)
    );
    failed.is_empty()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("verify");
    let ok = match cmd {
        "verify" => {
            let parsed = match parse_verify_args(&args[1..]) {
                Ok(parsed) => parsed,
                Err(e) => {
                    eprintln!("{e}\n\n{HELP}");
                    return ExitCode::from(2);
                }
            };
            let jobs = match resolve_jobs(parsed.jobs) {
                Ok(jobs) => jobs,
                Err(e) => {
                    eprintln!("{e}\n\n{HELP}");
                    return ExitCode::from(2);
                }
            };
            verify(&parsed, jobs)
        }
        "dev-index" => python("make_index.py", &["--check"]),
        "deviation-check" => python("deviation_check.py", &["--check"]),
        "frame-check" => python("frame_check.py", &["--check"]),
        "pe-layout" => python("pe_layout.py", &["--check"]),
        "help" | "--help" | "-h" => {
            println!("{}", HELP);
            true
        }
        other => {
            eprintln!("unknown command: {other}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    if ok {
        println!("\nall good");
        ExitCode::SUCCESS
    } else {
        println!("\nsomething failed");
        ExitCode::FAILURE
    }
}

const HELP: &str = "\
cargo xtask <command>

  verify            fmt, clippy and every test (the full gate); the test
                    binaries run in a parallel pool (4 at a time by default)
  verify --fast     the same, without the tests that read the install
  verify --fail-fast   stop at the first failing stage (default: run every
                    stage and report them all)
  verify --test-jobs N | XTASK_TEST_JOBS
                    how many test binaries run at once (default 4, or the
                    core count when that is smaller)
  verify -p <crate> (repeatable)
                    clippy and the tests for the named crates only; the
                    install suites live in the crates you leave out
  verify --skip <name> (repeatable)
                    do not run a test binary whose label matches (e.g.
                    --skip index_find_real); for the binaries that still
                    run, pass `--skip <name>` to libtest as well
  dev-index         check that the generated tool tables in docs/tools.md
                    match the tools on disk
  deviation-check   check that the deviations table and the code agree
  frame-check       check that the docs cite only the one address frame
  pe-layout         check which addresses are install bytes and which are BSS
";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_flags_is_the_full_gate() {
        let parsed = parse_verify_args(&[]).unwrap();
        assert!(!parsed.fast);
        assert!(!parsed.fail_fast);
        assert_eq!(parsed.jobs, None);
        // No crate named is the whole workspace: the gate is still the gate.
        assert!(parsed.packages.is_empty() && parsed.skips.is_empty());
        assert_eq!(scope_args(&[]), ["--workspace"]);
    }

    #[test]
    fn a_package_scopes_the_workspace_flag_away() {
        let parsed = parse_verify_args(&args(&["-p", "ow-casc", "--package=ow-sim"])).unwrap();
        assert_eq!(parsed.packages, ["ow-casc", "ow-sim"]);
        assert_eq!(
            scope_args(&parsed.packages),
            ["-p", "ow-casc", "-p", "ow-sim"]
        );
    }

    #[test]
    fn skips_are_collected_in_order() {
        let parsed = parse_verify_args(&args(&[
            "--skip",
            "index_find_real",
            "--skip=skin_pose_gate",
        ]))
        .unwrap();
        assert_eq!(parsed.skips, ["index_find_real", "skin_pose_gate"]);
    }

    #[test]
    fn empty_values_are_errors() {
        assert!(parse_verify_args(&args(&["--package="])).is_err());
        assert!(parse_verify_args(&args(&["--skip="])).is_err());
        assert!(parse_verify_args(&args(&["-p"])).is_err());
        assert!(parse_verify_args(&args(&["--skip"])).is_err());
    }

    #[test]
    fn known_flags_parse() {
        let parsed = parse_verify_args(&args(&["--fast", "--fail-fast"])).unwrap();
        assert!(parsed.fast && parsed.fail_fast);

        let parsed = parse_verify_args(&args(&["--test-jobs", "8"])).unwrap();
        assert_eq!(parsed.jobs, Some(8));

        let parsed = parse_verify_args(&args(&["--test-jobs=3"])).unwrap();
        assert_eq!(parsed.jobs, Some(3));
    }

    #[test]
    fn unknown_flags_are_errors() {
        assert!(parse_verify_args(&args(&["--faster"])).is_err());
        assert!(parse_verify_args(&args(&["fat"])).is_err());
    }

    #[test]
    fn bad_job_counts_are_errors() {
        assert!(parse_verify_args(&args(&["--test-jobs", "0"])).is_err());
        assert!(parse_verify_args(&args(&["--test-jobs", "-2"])).is_err());
        assert!(parse_verify_args(&args(&["--test-jobs", "four"])).is_err());
        assert!(parse_verify_args(&args(&["--test-jobs"])).is_err());
        assert!(parse_verify_args(&args(&["--test-jobs=",])).is_err());
    }

    #[test]
    fn the_job_count_caps_at_four_cores_by_default() {
        // The default never exceeds 4, whatever the machine reports.
        assert!(default_jobs() <= 4);
        assert!(default_jobs() >= 1);
    }
}
