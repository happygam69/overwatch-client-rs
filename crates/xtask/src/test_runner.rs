//! Discovers and runs the workspace's test binaries in parallel.
//!
//! `cargo test` runs one test binary at a time, and the install-reading
//! suites each spend minutes re-opening the same install, so a full gate
//! walks them all serially. The runner below builds every test binary once
//! (`cargo test --no-run --message-format=json`), reads the executable
//! paths out of the compiler-artifact messages, and runs the binaries in a
//! small thread pool. The suites are read-only on the install, which makes
//! that safe, and they share the OS page cache, which makes it faster than
//! the serial walk: the second binary to touch an index does not pay for
//! the disk again.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::json::{self, Value};

/// One test executable cargo built, with a name for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestBinary {
    pub label: String,
    pub path: PathBuf,
}

/// What happened when one binary ran.
#[derive(Debug)]
pub struct BinaryResult {
    pub label: String,
    pub ok: bool,
    pub duration: Duration,
    /// stdout + stderr, kept for the report and printed only on failure.
    pub output: String,
}

/// Reads the test executables out of `cargo test --no-run
/// --message-format=json` output, one document per line.
///
/// A line becomes a binary when it is a `compiler-artifact` whose
/// `profile.test` is true and whose `executable` is a string: exactly the
/// binaries this cargo invocation would have run. Unparseable or
/// non-artifact lines are skipped — cargo's stream also carries progress
/// and warning documents the runner does not care about. The result is
/// deduplicated (a fresh rebuild re-emits the same executable) and sorted,
/// so the run order is stable across machines.
pub fn parse_binaries<'a, I>(lines: I) -> Vec<TestBinary>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut seen: Vec<TestBinary> = Vec::new();
    for line in lines {
        let Ok(doc) = json::parse(line) else {
            continue;
        };
        if doc.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let is_test = doc
            .get("profile")
            .and_then(|p| p.get("test"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(path) = doc.get("executable").and_then(Value::as_str) else {
            continue; // rlibs and other non-executable artifacts
        };
        if !is_test {
            continue; // build scripts and bins built as dependencies
        }
        let Some(target) = doc.get("target") else {
            continue;
        };
        let name = target.get("name").and_then(Value::as_str).unwrap_or("");
        let kinds = target
            .get("kind")
            .and_then(Value::as_arr)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .unwrap_or_default();
        let package = package_name(doc.get("package_id").and_then(Value::as_str).unwrap_or(""));
        let label = if package.replace('-', "_") == name {
            format!("{name} ({kinds})")
        } else {
            format!("{package}/{name} ({kinds})")
        };
        let binary = TestBinary {
            label,
            path: PathBuf::from(path),
        };
        if !seen.iter().any(|b| b.path == binary.path) {
            seen.push(binary);
        }
    }
    seen.sort_by(|a, b| (&a.label, &a.path).cmp(&(&b.label, &b.path)));
    seen
}

/// The crate name out of a cargo `package_id`.
///
/// Both shapes occur in the wild: the current
/// `path+file:///…/crates/ow-casc#0.1.0` and the older
/// `ow-casc 0.1.0 (path+file:///…)`. Registry ids
/// (`registry+…#aes@0.8.4`) are not expected here — every test binary
/// belongs to a workspace member — but the first-token fallback still
/// produces something printable for them.
fn package_name(package_id: &str) -> String {
    if let Some(rest) = package_id.strip_prefix("path+file://") {
        let source = rest.split('#').next().unwrap_or(rest);
        return source
            .rsplit('/')
            .next()
            .unwrap_or(source)
            .trim_end_matches('/')
            .to_string();
    }
    package_id
        .split_whitespace()
        .next()
        .unwrap_or(package_id)
        .to_string()
}

/// Runs `binaries` in a pool of `jobs` threads and returns the results in
/// the input order. Each result line is printed as its binary finishes, so
/// a detached log shows progress without interleaved output; the captured
/// output of failures is returned for the caller to print once the pool is
/// done.
pub fn run_all(binaries: &[TestBinary], jobs: usize, extra_args: &[String]) -> Vec<BinaryResult> {
    let jobs = jobs.max(1).min(binaries.len().max(1));
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(usize, BinaryResult)>();
    let mut slots: Vec<Option<BinaryResult>> = binaries.iter().map(|_| None).collect();
    thread::scope(|scope| {
        for _ in 0..jobs {
            let tx = tx.clone();
            let next = &next;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(binary) = binaries.get(i) else {
                    break;
                };
                let result = run_one(binary, extra_args);
                // The receiver lives in the scope's main thread; a send
                // failure would mean it is gone, and there is nothing to
                // do but stop.
                if tx.send((i, result)).is_err() {
                    break;
                }
            });
        }
        drop(tx);
        for (i, result) in rx {
            println!(
                "-- {}: {} ({})",
                result.label,
                if result.ok { "ok" } else { "FAILED" },
                fmt_duration(result.duration)
            );
            slots[i] = Some(result);
        }
    });
    slots.into_iter().flatten().collect()
}

fn run_one(binary: &TestBinary, extra_args: &[String]) -> BinaryResult {
    let start = Instant::now();
    let mut cmd = Command::new(&binary.path);
    cmd.args(extra_args);
    match cmd.output() {
        Ok(out) => BinaryResult {
            label: binary.label.clone(),
            ok: out.status.success(),
            duration: start.elapsed(),
            output: format!(
                "--- {} stdout ---\n{}--- {} stderr ---\n{}",
                binary.label,
                String::from_utf8_lossy(&out.stdout),
                binary.label,
                String::from_utf8_lossy(&out.stderr)
            ),
        },
        Err(e) => BinaryResult {
            label: binary.label.clone(),
            ok: false,
            duration: start.elapsed(),
            output: format!("could not run {} ({e})", binary.path.display()),
        },
    }
}

/// `11m 31s` / `1.0s` — for the per-binary and summary lines.
pub fn fmt_duration(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 60.0 {
        format!("{secs:.1}s")
    } else {
        format!(
            "{}m {:.0}s",
            secs.div_euclid(60.0) as u64,
            secs.rem_euclid(60.0)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(name: &str, kinds: &str, package: &str, exe: &str, test: bool) -> String {
        format!(
            r#"{{"reason":"compiler-artifact","package_id":"{package}","target":{{"kind":[{kinds}],"name":"{name}"}},"profile":{{"test":{test}}},"executable":"{exe}"}}"#
        )
    }

    #[test]
    fn only_test_artifacts_become_binaries() {
        let lines = [
            // An integration test: kept.
            artifact(
                "index_find_real",
                "\"test\"",
                "path+file:///ws/crates/ow-casc#0.1.0",
                "/ws/target/release/deps/index_find_real-1",
                true,
            ),
            // A build script: has an executable but is not a test.
            artifact(
                "build-script-build",
                "\"custom-build\"",
                "path+file:///ws/crates/ow-casc#0.1.0",
                "/ws/target/release/build/ow-casc-build",
                false,
            ),
            // A dependency rlib: no executable at all.
            r#"{"reason":"compiler-artifact","package_id":"registry+https://…#aes@0.8.4","target":{"kind":["lib"],"name":"aes"},"profile":{"test":false},"executable":null}"#.to_string(),
            // Not an artifact message at all.
            r#"{"reason":"compiler-message","message":"…" }"#.to_string(),
            // Not even JSON.
            "    Compiling ow-casc v0.1.0".to_string(),
        ];
        let found = parse_binaries(lines.iter().map(String::as_str));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label, "ow-casc/index_find_real (test)");
        assert_eq!(
            found[0].path,
            PathBuf::from("/ws/target/release/deps/index_find_real-1")
        );
    }

    #[test]
    fn duplicate_artifacts_collapse() {
        let line = artifact(
            "ow_sim",
            "\"lib\"",
            "path+file:///ws/crates/ow-sim#0.1.0",
            "/ws/target/release/deps/ow_sim-abc",
            true,
        );
        // A fresh rebuild re-emits the same executable; run it once. The
        // package is `ow-sim`, the lib target `ow_sim` — one label, no
        // redundant prefix.
        let found = parse_binaries([line.as_str(), line.as_str()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label, "ow_sim (lib)");
    }

    #[test]
    fn two_crates_with_the_same_target_name_stay_distinct() {
        // ow-casc and ow-sim both ship a `verify_probe` bin.
        let lines = [
            artifact(
                "verify_probe",
                "\"bin\"",
                "path+file:///ws/crates/ow-casc#0.1.0",
                "/ws/target/release/deps/verify_probe-1",
                true,
            ),
            artifact(
                "verify_probe",
                "\"bin\"",
                "path+file:///ws/crates/ow-sim#0.1.0",
                "/ws/target/release/deps/verify_probe-2",
                true,
            ),
        ];
        let found = parse_binaries(lines.iter().map(String::as_str));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].label, "ow-casc/verify_probe (bin)");
        assert_eq!(found[1].label, "ow-sim/verify_probe (bin)");
    }

    #[test]
    fn the_result_is_sorted_by_label() {
        let lines = [
            artifact(
                "b_test",
                "\"test\"",
                "path+file:///ws/crates/ow-b#0.1.0",
                "/b",
                true,
            ),
            artifact(
                "a_test",
                "\"test\"",
                "path+file:///ws/crates/ow-a#0.1.0",
                "/a",
                true,
            ),
        ];
        let found = parse_binaries(lines.iter().map(String::as_str));
        assert_eq!(
            found.iter().map(|b| b.label.as_str()).collect::<Vec<_>>(),
            ["ow-a/a_test (test)", "ow-b/b_test (test)"]
        );
    }

    #[test]
    fn old_package_id_shapes_still_resolve() {
        let found = parse_binaries([artifact(
            "ow_sim",
            "\"lib\"",
            "ow-sim 0.1.0 (path+file:///ws/crates/ow-sim)",
            "/ws/deps/ow_sim-abc",
            true,
        )
        .as_str()]);
        assert_eq!(found[0].label, "ow_sim (lib)");
    }

    #[test]
    #[cfg(unix)]
    fn the_pool_runs_binaries_and_reports_failures() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "xtask-pool-test-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let ok = dir.join("ok.sh");
        let bad = dir.join("bad.sh");
        fs::write(&ok, "#!/bin/sh\nexit 0\n").unwrap();
        fs::write(
            &bad,
            "#!/bin/sh\necho boom on stdout\necho boom on stderr >&2\nexit 3\n",
        )
        .unwrap();
        fs::set_permissions(&ok, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&bad, fs::Permissions::from_mode(0o755)).unwrap();

        let binaries = vec![
            TestBinary {
                label: "ok (test)".into(),
                path: ok.clone(),
            },
            TestBinary {
                label: "bad (test)".into(),
                path: bad.clone(),
            },
        ];
        for jobs in [1, 4] {
            let results = run_all(&binaries, jobs, &[]);
            assert_eq!(results.len(), 2, "jobs={jobs}");
            // Results come back in input order, not completion order.
            assert!(results[0].ok);
            assert!(!results[1].ok);
            assert!(results[1].output.contains("boom on stdout"));
            assert!(results[1].output.contains("boom on stderr"));
            assert!(results[1].duration.as_nanos() > 0);
        }

        // More jobs than binaries is fine, and so is one binary.
        let one = run_all(&binaries[..1], 8, &[]);
        assert_eq!(one.len(), 1);
        assert!(one[0].ok);

        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extra_args_reach_each_binary() {
        // The fixture is this very test binary: `--skip` is libtest's own
        // filter, so a run that skips named tests must still run the rest
        // and report the skips as filtered out. A bare positional name
        // would be libtest's *include* filter and run nothing, which is
        // what this test exists to catch. Skipping itself as well keeps
        // the self-run from recursing.
        let binary = TestBinary {
            label: "self (test)".into(),
            path: std::env::current_exe().unwrap(),
        };
        let args = vec![
            "--skip".to_string(),
            "durations_format_compactly".to_string(),
            "--skip".to_string(),
            "extra_args_reach_each_binary".to_string(),
        ];
        let results = run_all(&[binary], 1, &args);
        assert!(results[0].ok, "self-run failed:\n{}", results[0].output);
        assert!(
            results[0].output.contains("2 filtered out"),
            "both names must be skipped, not selected:\n{}",
            results[0].output
        );
        assert!(
            results[0].output.contains("0 failed"),
            "the rest of the suite must still run:\n{}",
            results[0].output
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_missing_executable_is_a_failure_not_a_panic() {
        let binaries = vec![TestBinary {
            label: "gone (test)".into(),
            path: PathBuf::from("/nonexistent/xtask-test-binary"),
        }];
        let results = run_all(&binaries, 1, &[]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].ok);
        assert!(results[0].output.contains("could not run"));
    }

    #[test]
    fn durations_format_compactly() {
        assert_eq!(fmt_duration(Duration::from_millis(950)), "0.9s");
        assert_eq!(fmt_duration(Duration::from_secs(59)), "59.0s");
        assert_eq!(fmt_duration(Duration::from_secs(691)), "11m 31s");
    }
}
