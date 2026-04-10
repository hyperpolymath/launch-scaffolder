// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! `mint` subcommand — scaffold stub. Full implementation lands in a
//! follow-up session once the workspace builds and the common crate has
//! its standard/config parsers.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::path::Path;

#[derive(Debug, ClapArgs)]
pub struct Args {
    // Subcommand-specific flags land here in follow-up.
}

pub fn run(_args: Args, _standard: Option<&Path>) -> Result<()> {
    tracing::warn!("subcommand `mint` is a scaffold stub — not yet implemented");
    println!("launch-scaffolder mint: not yet implemented (scaffold stub)");
    Ok(())
}
