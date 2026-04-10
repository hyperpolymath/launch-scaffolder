// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! Shared launcher-config discovery (walks + fixture filtering).
//!
//! Originally lived in `cmd_realign`. Extracted here as soon as
//! `cmd_provision` became the second caller — three users is the
//! traditional threshold but the logic was already non-trivial, and
//! keeping one authoritative copy avoids drift between the subcommand
//! that "checks" (realign) and the subcommand that "acts" (provision).
//!
//! Fixture-vs-live convention lives here as module-level constants so
//! the rule is testable and greppable from one place.

use crate::Result;
use anyhow::Context;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Canonical estate root used when no `--search-root` is supplied.
pub const ESTATE_ROOT: &str = "/var/mnt/eclipse/repos";

/// Suffix that marks a live, estate-owned launcher config.
pub const LIVE_EXT: &str = ".launcher.a2ml";

/// Suffix that marks a test fixture / worked example. Must **not** be
/// picked up by estate walks. See `examples/README.md`.
pub const FIXTURE_EXT: &str = ".launcher.fixture.a2ml";

/// Walk `root` and return every live launcher config, sorted.
///
/// Walk errors (typically `EACCES` from docker-owned DB data dirs under
/// `project-wharf` and friends) are logged at `debug` and skipped — they
/// cannot contain launcher configs.
pub fn walk_live_configs(root: &Path) -> Result<Vec<PathBuf>> {
    tracing::debug!("walking {} for launcher configs", root.display());
    let mut out = Vec::new();
    let walker = WalkDir::new(root).follow_links(false).into_iter();
    for entry in walker.filter_entry(|e| !is_pruned_dir(e.path())) {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                tracing::debug!("skipping unreadable path during walk: {}", err);
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if is_live_config(entry.path()) {
            out.push(entry.path().to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

/// Convenience: walk the canonical estate root.
pub fn walk_estate() -> Result<Vec<PathBuf>> {
    walk_live_configs(Path::new(ESTATE_ROOT))
        .with_context(|| format!("walking estate root {}", ESTATE_ROOT))
}

/// Directories that should never contribute launcher configs.
///
/// Fixture isolation is handled at the file-name level, not at the
/// directory level — this prune list only covers build/dev noise
/// (`target/`, `.git/`, `node_modules/`, `_exploratory/`, `.archive*/`).
pub fn is_pruned_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(name, "target" | ".git" | "node_modules" | "_exploratory")
        || name.starts_with(".archive")
}

/// `true` iff `path`'s file name is a live launcher config. Files with
/// the fixture suffix are excluded so test inputs cannot be picked up
/// by estate walks.
pub fn is_live_config(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(LIVE_EXT) && !name.ends_with(FIXTURE_EXT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_covers_expected_dirs() {
        assert!(is_pruned_dir(Path::new("/x/target")));
        assert!(is_pruned_dir(Path::new("/x/.git")));
        assert!(is_pruned_dir(Path::new("/x/node_modules")));
        assert!(is_pruned_dir(Path::new("/x/_exploratory")));
        assert!(is_pruned_dir(Path::new("/x/.archive-2026-04-10")));
        assert!(is_pruned_dir(Path::new("/x/.archive-2027-01-01")));
        assert!(!is_pruned_dir(Path::new("/x/examples")));
        assert!(!is_pruned_dir(Path::new("/x/crates")));
    }

    #[test]
    fn fixture_suffix_is_not_a_live_config() {
        assert!(is_live_config(Path::new("/r/stapeln.launcher.a2ml")));
        assert!(!is_live_config(Path::new(
            "/r/stapeln.launcher.fixture.a2ml"
        )));
        assert!(!is_live_config(Path::new("/r/README.md")));
        assert!(!is_live_config(Path::new("/r/stapeln.a2ml")));
    }
}
