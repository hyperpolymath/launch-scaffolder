// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `provision-set` subcommand — mint, realign and check a repository's
//! provisioning set against `PROVISIONING-STANDARD.adoc`.

use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};
use launch_scaffolder_common::provisioning::{
    canon::{CANON_ENV, Canon},
    check,
};
use std::path::PathBuf;

#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Use a canon directory (a `CANON` file and a `templates/` tree) instead
    /// of the canon baked into the binary.
    #[arg(long, value_name = "DIR", env = CANON_ENV, global = true)]
    canon: Option<PathBuf>,

    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Fail if the engine files differ from the canon; otherwise run the
    /// repository's provision-check.sh and exit with its code.
    Check {
        /// Downgrade unfilled repository-specific slots to warnings.
        #[arg(long)]
        dev: bool,
        /// Repository to check.
        #[arg(default_value = ".")]
        target: PathBuf,
    },
}

pub fn run(args: Args) -> Result<()> {
    let canon = Canon::resolve(args.canon.as_deref())?;
    match args.action {
        Action::Check { dev, target } => {
            let drift = check::engine_drift(&target, &canon)?;
            if !drift.is_empty() {
                eprintln!(
                    "provision-set check: engine differs from {} — run `launch-scaffolder provision-set realign`:",
                    canon.reference()?
                );
                for d in &drift {
                    eprintln!("  FAIL {d}");
                }
                std::process::exit(1);
            }
            let code = check::conformance(&target, dev)?;
            std::process::exit(code);
        }
    }
}
