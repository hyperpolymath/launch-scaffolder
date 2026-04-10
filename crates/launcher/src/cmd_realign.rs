// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! `realign` subcommand — bulk re-mint existing launchers against the current
//! standard.
//!
//! Realign is the estate-maintenance counterpart to `mint`: given a set of
//! already-committed `<app>.launcher.a2ml` configs, regenerate each one's
//! sibling `<app>-launcher.sh` using the current template and standard. The
//! per-app `[exceptions]` block rides along inside the input config, so no
//! special preservation logic is needed — re-parsing the same A2ML input
//! yields the same `LauncherConfig` (exceptions included), and any diff in
//! the output comes from changes to the template or standard.
//!
//! Discovery precedence:
//!
//! 1. Explicit positional `CONFIGS…` — use exactly these.
//! 2. `--search-root <DIR>` — walk DIR for `*.launcher.a2ml`.
//! 3. Otherwise — walk the canonical estate root
//!    (`/var/mnt/eclipse/repos`, the default since this tool is an
//!    estate-maintenance command by design). Override with
//!    `--search-root` for narrower scans.
//!
//! Prune rules for walks: `target/`, `.git/`, `node_modules/`,
//! `_exploratory/`, `.archive*/`.
//!
//! **Fixture-vs-live rule.** Files ending in `.launcher.fixture.a2ml`
//! are treated as test fixtures and skipped; only `.launcher.a2ml`
//! (without `.fixture.`) is considered a live config. This is the
//! project-wide convention for distinguishing fixture inputs from
//! estate-owned configs — see `examples/README.md`.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use launch_scaffolder_common::{config::LauncherConfig, standard::LauncherStandard, template};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Marker suffix used by fixture/example files that must **not** be
/// picked up by estate walks. See [`is_live_config`] and the fixture
/// convention documented in `crates/launcher-common/examples/README.md`.
const FIXTURE_EXT: &str = ".launcher.fixture.a2ml";
const LIVE_EXT: &str = ".launcher.a2ml";

/// Canonical estate root used by `--all`.
const ESTATE_ROOT: &str = "/var/mnt/eclipse/repos";

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Explicit `<app>.launcher.a2ml` configs to realign. If empty, walk
    /// `--search-root` (or the canonical estate root if unset).
    #[arg(value_name = "CONFIG")]
    pub configs: Vec<PathBuf>,

    /// Walk DIR recursively for live `*.launcher.a2ml` files. Defaults to
    /// the canonical estate root (`/var/mnt/eclipse/repos`).
    #[arg(long, value_name = "DIR")]
    pub search_root: Option<PathBuf>,

    /// Report what would change and exit. Writes nothing.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// CI mode: exit 1 if any launcher would change. Implies `--dry-run`.
    #[arg(long)]
    pub check: bool,

    /// Do not mark regenerated output files executable.
    #[arg(long)]
    pub no_chmod: bool,

    /// Keep processing after a config fails to parse or render. Default is
    /// to stop at the first error.
    #[arg(long)]
    pub keep_going: bool,
}

/// Outcome of realigning a single config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Output on disk already matches regenerated output.
    Unchanged,
    /// Output was (or would be) rewritten.
    Updated,
    /// No sibling `*-launcher.sh` existed; fresh file created.
    Created,
}

pub fn run(args: Args, standard_path: Option<&Path>) -> Result<()> {
    let dry_run = args.dry_run || args.check;

    let configs = discover_configs(&args)?;
    if configs.is_empty() {
        println!("launch-scaffolder realign: no launcher configs found");
        return Ok(());
    }

    let standard = LauncherStandard::resolve(standard_path)?;

    let mut unchanged = 0usize;
    let mut updated = 0usize;
    let mut created = 0usize;
    let mut failed = 0usize;

    for config_path in &configs {
        let outcome = match realign_one(config_path, &standard, dry_run, args.no_chmod) {
            Ok(o) => o,
            Err(e) => {
                if args.keep_going {
                    tracing::warn!("realign failed for {}: {:#}", config_path.display(), e);
                    eprintln!("✗ {}: {:#}", config_path.display(), e);
                    failed += 1;
                    continue;
                } else {
                    return Err(e);
                }
            }
        };

        match outcome {
            Outcome::Unchanged => {
                unchanged += 1;
                println!("= {}", config_path.display());
            }
            Outcome::Updated => {
                updated += 1;
                let verb = if dry_run { "would update" } else { "updated" };
                println!("~ {} ({})", config_path.display(), verb);
            }
            Outcome::Created => {
                created += 1;
                let verb = if dry_run { "would create" } else { "created" };
                println!("+ {} ({})", config_path.display(), verb);
            }
        }
    }

    println!(
        "\nrealign summary: {} unchanged, {} updated, {} created, {} failed ({} total)",
        unchanged,
        updated,
        created,
        failed,
        configs.len()
    );

    if args.check && (updated > 0 || created > 0) {
        anyhow::bail!(
            "--check: {} launcher(s) would change",
            updated + created
        );
    }
    if failed > 0 && !args.keep_going {
        anyhow::bail!("{} launcher(s) failed to realign", failed);
    }
    Ok(())
}

/// Realign a single config; returns whether the output was/would be changed.
fn realign_one(
    config_path: &Path,
    standard: &LauncherStandard,
    dry_run: bool,
    no_chmod: bool,
) -> Result<Outcome> {
    let config = LauncherConfig::load(config_path)
        .with_context(|| format!("loading config {}", config_path.display()))?;
    let script = template::render(&config, standard)?;

    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    let out = parent.join(format!("{}-launcher.sh", config.project.name));

    let existing = std::fs::read_to_string(&out).ok();
    let outcome = match existing {
        Some(ref current) if current == &script => Outcome::Unchanged,
        Some(_) => Outcome::Updated,
        None => Outcome::Created,
    };

    if outcome == Outcome::Unchanged || dry_run {
        return Ok(outcome);
    }

    std::fs::write(&out, &script)
        .with_context(|| format!("writing {}", out.display()))?;

    if !no_chmod {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&out)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&out, perms)?;
        }
    }

    tracing::info!("realigned {} → {}", config.project.name, out.display());
    Ok(outcome)
}

/// Resolve the list of configs to realign according to the documented
/// precedence. Walks are filtered through [`is_pruned`] to skip obvious
/// noise directories.
fn discover_configs(args: &Args) -> Result<Vec<PathBuf>> {
    if !args.configs.is_empty() {
        return Ok(args.configs.clone());
    }
    let root: PathBuf = args
        .search_root
        .clone()
        .unwrap_or_else(|| PathBuf::from(ESTATE_ROOT));

    tracing::debug!("walking {} for launcher configs", root.display());

    let mut out = Vec::new();
    let walker = WalkDir::new(&root).follow_links(false).into_iter();
    // Estate walks routinely hit unreadable dirs (e.g. docker-owned DB data
    // dirs under project-wharf). Log and skip them instead of bailing —
    // they can't contain launcher configs anyway.
    for entry in walker.filter_entry(|e| !is_pruned(e.path())) {
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
        let path = entry.path();
        if is_live_config(path) {
            out.push(path.to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

/// Directories that should never contribute launcher configs.
///
/// Fixture isolation is handled at the file-name level (see
/// [`is_live_config`] and `FIXTURE_EXT`), not by pruning `examples/`,
/// so that real launcher configs living under a repo-local
/// `examples/` directory are still discoverable.
fn is_pruned(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(name, "target" | ".git" | "node_modules" | "_exploratory")
        || name.starts_with(".archive")
}

/// `true` iff `path`'s file name is a live launcher config. Files with
/// the fixture suffix (`.launcher.fixture.a2ml`) are deliberately
/// excluded so test inputs cannot be picked up by estate walks.
fn is_live_config(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.ends_with(LIVE_EXT) && !name.ends_with(FIXTURE_EXT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pruned_covers_expected_dirs() {
        assert!(is_pruned(Path::new("/x/target")));
        assert!(is_pruned(Path::new("/x/.git")));
        assert!(is_pruned(Path::new("/x/node_modules")));
        assert!(is_pruned(Path::new("/x/_exploratory")));
        assert!(is_pruned(Path::new("/x/.archive-2026-04-10")));
        assert!(is_pruned(Path::new("/x/.archive-2027-01-01")));
        assert!(!is_pruned(Path::new("/x/examples"))); // live repos may have this
        assert!(!is_pruned(Path::new("/x/crates")));
        assert!(!is_pruned(Path::new("/x/aerie")));
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
