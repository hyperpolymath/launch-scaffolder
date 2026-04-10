// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! `provision` subcommand — integrate or disintegrate a launcher with the
//! current desktop.
//!
//! Option-(b) design: all real work lives in
//! `launch_scaffolder_common::integration`. This file is a thin CLI that
//! resolves configs, drives the confirm prompt, and forwards to the
//! library. Bulk mode (`--all` or any multi-config invocation) prompts
//! before touching `~/.local/share/applications` unless `--no-confirm`
//! is set.
//!
//! The generated shell script still carries a fallback arm — when
//! `launch-scaffolder` isn't on `PATH`, the old shell implementation
//! runs. That means this subcommand is always the fast path, never the
//! only path.

use anyhow::{Context, Result};
use clap::{ArgGroup, Args as ClapArgs};
use launch_scaffolder_common::{
    config::LauncherConfig,
    discovery,
    integration::{self, DisintegOpts, IntegOpts, IntegReport},
};
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, ClapArgs)]
#[command(group(
    ArgGroup::new("action")
        .required(true)
        .args(["integ", "disinteg"]),
))]
pub struct Args {
    /// Explicit `<app>.launcher.a2ml` configs to provision. Empty means
    /// "walk `--search-root` (or the estate root)" — but the walk
    /// requires `--all` for safety.
    #[arg(value_name = "CONFIG")]
    pub configs: Vec<PathBuf>,

    /// Install desktop entry + shortcut + icon + launcher binary.
    #[arg(long)]
    pub integ: bool,

    /// Remove everything `--integ` installs.
    #[arg(long)]
    pub disinteg: bool,

    /// Walk DIR recursively instead of the canonical estate root.
    #[arg(long, value_name = "DIR")]
    pub search_root: Option<PathBuf>,

    /// Required to process all discovered configs in a walk. Without
    /// this, an empty positional list is an error so bulk integration
    /// can never happen by accident.
    #[arg(long)]
    pub all: bool,

    /// Reinstall even if the launcher is already integrated. Only
    /// meaningful with `--integ`.
    #[arg(long)]
    pub force: bool,

    /// Skip the bulk-mode confirmation prompt. Default is to prompt
    /// when more than one config is involved.
    #[arg(long)]
    pub no_confirm: bool,

    /// Describe actions and exit. Writes nothing, runs no external
    /// commands.
    #[arg(short = 'n', long)]
    pub dry_run: bool,
}

pub fn run(args: Args, _standard: Option<&Path>) -> Result<()> {
    let configs = resolve_configs(&args)?;
    if configs.is_empty() {
        println!("launch-scaffolder provision: no launcher configs to process");
        return Ok(());
    }

    // Load every config up-front so the preview is accurate before we
    // prompt / touch anything.
    let loaded: Vec<(PathBuf, LauncherConfig)> = configs
        .iter()
        .map(|p| {
            let cfg = LauncherConfig::load(p)
                .with_context(|| format!("loading config {}", p.display()))?;
            Ok((p.clone(), cfg))
        })
        .collect::<Result<Vec<_>>>()?;

    let action = if args.integ {
        "integrate"
    } else {
        "disintegrate"
    };
    print_plan(action, &loaded, args.dry_run);

    if !args.dry_run
        && loaded.len() > 1
        && !args.no_confirm
        && !confirm(&format!(
            "About to {action} {n} launcher(s). Proceed?",
            action = action,
            n = loaded.len()
        ))?
    {
        println!("aborted.");
        return Ok(());
    }

    let mut ok = 0usize;
    let mut failed = 0usize;
    for (config_path, cfg) in &loaded {
        let result = if args.integ {
            do_integ(config_path, cfg, &args)
        } else {
            do_disinteg(cfg, &args)
        };
        match result {
            Ok(report) => {
                ok += 1;
                render_report(&cfg.project.display, &report);
            }
            Err(e) => {
                failed += 1;
                eprintln!("✗ {}: {:#}", cfg.project.display, e);
            }
        }
    }

    println!(
        "\nprovision summary: {} {} ok, {} failed ({} total)",
        ok,
        action,
        failed,
        loaded.len()
    );

    if failed > 0 {
        anyhow::bail!("{} launcher(s) failed to {}", failed, action);
    }
    Ok(())
}

fn do_integ(config_path: &Path, cfg: &LauncherConfig, args: &Args) -> Result<IntegReport> {
    let script_path = sibling_script(config_path, cfg);
    let opts = IntegOpts {
        force: args.force,
        dry_run: args.dry_run,
    };
    integration::integ(cfg, &script_path, &opts)
}

fn do_disinteg(cfg: &LauncherConfig, args: &Args) -> Result<IntegReport> {
    let opts = DisintegOpts {
        dry_run: args.dry_run,
    };
    integration::disinteg(cfg, &opts)
}

/// Render a single app's report as human text. Mirrors the shell's
/// `log "  + ..."` style from the original template.
fn render_report(app: &str, report: &IntegReport) {
    for line in &report.actions {
        println!("  [{}] {}", app, line);
    }
    for line in &report.skipped {
        println!("  [{}] · {}", app, line);
    }
    if report.already_present && report.actions.is_empty() {
        println!("  [{}] already integrated (use --force to reinstall)", app);
    }
}

/// Emit a human-readable plan before we start doing anything.
fn print_plan(action: &str, loaded: &[(PathBuf, LauncherConfig)], dry_run: bool) {
    let lead = if dry_run { "would" } else { "will" };
    println!(
        "launch-scaffolder provision: {} {} the following:",
        lead, action
    );
    for (p, cfg) in loaded {
        println!(
            "  - {} ({}) from {}",
            cfg.project.display,
            cfg.project.name,
            p.display()
        );
    }
    println!();
}

/// Resolve the set of configs to provision.
fn resolve_configs(args: &Args) -> Result<Vec<PathBuf>> {
    if !args.configs.is_empty() {
        return Ok(args.configs.clone());
    }
    if !args.all {
        anyhow::bail!(
            "no configs given: pass one or more <app>.launcher.a2ml paths, \
             or use `--all` to walk the estate root (or `--search-root <DIR> --all` \
             for a narrower scan)"
        );
    }
    let root: PathBuf = args
        .search_root
        .clone()
        .unwrap_or_else(|| PathBuf::from(discovery::ESTATE_ROOT));
    discovery::walk_live_configs(&root).with_context(|| format!("walking {}", root.display()))
}

/// Compute the sibling script path `<config-parent>/<app>-launcher.sh`.
fn sibling_script(config_path: &Path, cfg: &LauncherConfig) -> PathBuf {
    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{}-launcher.sh", cfg.project.name))
}

/// Interactive y/N confirmation. Returns `Ok(false)` on EOF or a blank
/// / non-affirmative answer, `Ok(true)` only on explicit 'y'/'yes'.
fn confirm(prompt: &str) -> Result<bool> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    write!(lock, "{prompt} [y/N] ")?;
    lock.flush()?;
    let stdin = io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line)? == 0 {
        return Ok(false);
    }
    let trimmed = line.trim().to_ascii_lowercase();
    Ok(matches!(trimmed.as_str(), "y" | "yes"))
}
