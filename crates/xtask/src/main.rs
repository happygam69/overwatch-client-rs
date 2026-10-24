//! The project's own commands: one way to ask "is this tree healthy?".
//!
//! ```text
//! cargo xtask verify          # the full gate: fmt, clippy, every test
//! cargo xtask verify --fast   # the same without the install-reading tests
//! cargo xtask dev-index       # is dev/INDEX.md up to date?
//! cargo xtask fit-check       # does the code match the fitted ledger?
//! ```
//!
//! The gate is what "done" means in this project: a change is done when
//! `cargo xtask verify` passes and the docs that describe it are updated.

use std::process::{Command, ExitCode};

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

fn verify(fast: bool) -> bool {
    let mut ok = true;
    ok &= run("cargo fmt --check", "cargo", &["fmt", "--check"], None);
    ok &= run(
        "cargo clippy",
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
        None,
    );
    // The install-reading tests are the slow half; on the fast path only the
    // libraries and the viewer are checked.
    let suites: Vec<Vec<&str>> = if fast {
        vec![
            vec!["test", "--release", "-p", "ow-casc", "--lib"],
            vec!["test", "--release", "-p", "ow-tank", "--lib"],
            vec!["test", "--release", "-p", "ow-sim"],
            vec!["test", "--release", "-p", "ow-view"],
            vec!["test", "--release", "-p", "ow-scene"],
        ]
    } else {
        vec![vec!["test", "--release", "--workspace"]]
    };
    for suite in suites {
        ok &= run("cargo test", "cargo", &suite, None);
    }
    ok &= python("make_index.py", &["--check"]);
    ok &= python("fit_check.py", &["--check"]);
    ok
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("verify");
    let ok = match cmd {
        "verify" => verify(args.iter().any(|a| a == "--fast")),
        "dev-index" => python("make_index.py", &["--check"]),
        "fit-check" => python("fit_check.py", &["--check"]),
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

  verify          fmt, clippy and every test (the full gate)
  verify --fast   the same, without the tests that read the install
  dev-index       check that dev/INDEX.md matches the tools on disk
  fit-check       check that the fitted ledger and the code agree
";
