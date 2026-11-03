//! Shared by the integration tests: the format export they read.

use std::path::{Path, PathBuf};

/// The text of the format export `format/<id>.json`, looked up where the
/// Python package (`ninjask.paths`) writes it: under `$NINJASK_DATA`, under
/// `$NINJASK_HOME/data/derived`, in this checkout's `data/derived/`, or, for
/// a checkout vendored as a subdirectory, in the enclosing repository's.
/// `None`, with a note on stderr, when there is none: the test is skipped.
pub fn format_export(id: &str) -> Option<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(d) = std::env::var("NINJASK_DATA") {
        dirs.push(d.into());
    }
    if let Ok(h) = std::env::var("NINJASK_HOME") {
        dirs.push(Path::new(&h).join("data/derived"));
    }
    dirs.push(root.join("data/derived"));
    dirs.push(root.join("../data/derived"));
    for d in dirs {
        if let Ok(text) = std::fs::read_to_string(d.join("format").join(format!("{id}.json"))) {
            return Some(text);
        }
    }
    eprintln!("skipped: no format export {id}.json (see README, Working from source)");
    None
}
