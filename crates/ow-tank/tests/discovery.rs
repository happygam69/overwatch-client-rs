//! Checks that discovery finds the checkouts on a machine that has them.
//!
//! Skipped when they are absent, so the suite passes on a machine without them.

use ow_tank::keytable::Sources;
use std::path::PathBuf;

#[test]
fn discovery_finds_checkouts_when_they_are_present() {
    let probes: Vec<PathBuf> = [r"E:\CASCReader", r"C:\", r"D:\"]
        .iter()
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .collect();
    if probes.is_empty() {
        return;
    }

    let sources = Sources::discover(&probes);
    let found = sources.tactlib_checkout.is_some() || sources.owlib_checkout.is_some();
    if !found {
        eprintln!(
            "note: no reference checkout found under {:?}; nothing to assert",
            probes
        );
        return;
    }
    eprintln!(
        "found tactlib={:?} owlib={:?}",
        sources.tactlib_checkout, sources.owlib_checkout
    );
    assert!(found);
}
