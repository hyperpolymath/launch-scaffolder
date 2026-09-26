// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `mint` subcommand — generate a launcher script from a `<app>.launcher.a2ml`
//! config, rendered through the Tera template and the active standard.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use launch_scaffolder_common::{
    config::LauncherConfig,
    fs_utils::{existing_mode_or, write_atomic, write_atomic_unmodified},
    standard::LauncherStandard,
    template,
};
use std::path::{Path, PathBuf};

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Path to the per-app `<app>.launcher.a2ml` config file.
    #[arg(value_name = "CONFIG")]
    pub config: PathBuf,

    /// Output path for the generated launcher script. Defaults to
    /// `<config-parent>/<app-name>-launcher.sh`.
    #[arg(short = 'o', long = "out", value_name = "FILE")]
    pub out: Option<PathBuf>,

    /// Print the generated script to stdout instead of writing a file.
    #[arg(long)]
    pub stdout: bool,

    /// Do not mark the output file executable (default is to chmod +x).
    #[arg(long)]
    pub no_chmod: bool,
}

pub fn run(args: Args, standard_path: Option<&Path>) -> Result<()> {
    let config = LauncherConfig::load(&args.config)
        .with_context(|| format!("loading config {}", args.config.display()))?;

    let standard = LauncherStandard::resolve(standard_path)?;
    let script = template::render(&config, &standard, Some(&args.config))?;

    if args.stdout {
        print!("{}", script);
        return Ok(());
    }

    let out = args.out.unwrap_or_else(|| {
        let parent = args.config.parent().unwrap_or_else(|| Path::new("."));
        parent.join(format!("{}-launcher.sh", config.project.name))
    });

    if args.no_chmod && !out.exists() {
        write_atomic_unmodified(&out, script.as_bytes())
            .with_context(|| format!("writing {}", out.display()))?;
    } else {
        let mode = if args.no_chmod {
            existing_mode_or(&out, 0o644)
        } else {
            0o755
        };
        write_atomic(&out, script.as_bytes(), mode)
            .with_context(|| format!("writing {}", out.display()))?;
    }

    tracing::info!("minted {} → {}", config.project.name, out.display());
    println!("✓ minted {} → {}", config.project.display, out.display());
    Ok(())
}
