//! What discovery finds from the roots a binary actually uses.

use ow_tank::keytable::Sources;
use std::path::PathBuf;

#[test]
fn from_the_install_root_alone() {
    let root = PathBuf::from(r"E:\Overwatch");
    if !root.is_dir() {
        return;
    }
    let sources = Sources::discover(&[root]);
    eprintln!(
        "tactlib={:?}\nowlib={:?}",
        sources.tactlib_checkout, sources.owlib_checkout
    );
}
