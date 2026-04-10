// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! `mint` subcommand — generate a launcher script from a `<app>.launcher.a2ml`
//! config, rendered through the Tera template and the active standard.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use launch_scaffolder_common::{config::LauncherConfig, standard::LauncherStandard, template};
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

    std::fs::write(&out, &script).with_context(|| format!("writing {}", out.display()))?;

    if !args.no_chmod {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&out)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&out, perms)?;
        }
    }

    tracing::info!("minted {} → {}", config.project.name, out.display());
    println!("✓ minted {} → {}", config.project.display, out.display());
    Ok(())
}
