// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! launch-scaffolder — cross-platform launcher minter, provisioner, configurator.
//!
//! One binary, five subcommands:
//!
//! - `mint`       — generate a new launcher from a `<app>.launcher.a2ml` config
//! - `provision`  — install (`--integ`) or uninstall (`--disinteg`) a launcher
//! - `config`     — get/set/validate the config section of a launcher
//! - `realign`    — re-mint existing launchers against the current standard
//! - `standard`   — inspect or validate the launcher standard itself
//!
//! See README.adoc at the repo root for the design rationale and the full
//! command surface. This file is deliberately thin — all real work lives in
//! `launch-scaffolder-common`.

use anyhow::Result;
use clap::{Parser, Subcommand};

mod cmd_config;
mod cmd_mint;
mod cmd_provision;
mod cmd_realign;
mod cmd_standard;

/// launch-scaffolder — build and maintain cross-platform launchers from A2ML specs.
#[derive(Debug, Parser)]
#[command(
    name = "launch-scaffolder",
    version,
    about,
    long_about = None,
    propagate_version = true,
)]
struct Cli {
    /// Override the launcher standard file. Defaults to the standard baked
    /// into the binary at build time, or the value of $LAUNCH_SCAFFOLDER_STANDARD
    /// if set.
    #[arg(
        long,
        value_name = "FILE",
        env = "LAUNCH_SCAFFOLDER_STANDARD",
        global = true
    )]
    standard: Option<std::path::PathBuf>,

    /// Increase verbosity (can be passed multiple times).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate a new launcher from a <app>.launcher.a2ml config.
    Mint(cmd_mint::Args),

    /// Install (--integ) or uninstall (--disinteg) a launcher on the current system.
    Provision(cmd_provision::Args),

    /// Get, set, or validate the config section of an existing launcher.
    Config(cmd_config::Args),

    /// Re-mint one or more existing launchers against the current standard,
    /// preserving their [exceptions] block. Bulk-realignment entry point.
    Realign(cmd_realign::Args),

    /// Show or validate the launcher standard file.
    Standard(cmd_standard::Args),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialise tracing with a default filter level based on --verbose count.
    let default_level = match cli.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level)),
        )
        .init();

    match cli.command {
        Command::Mint(args) => cmd_mint::run(args, cli.standard.as_deref()),
        Command::Provision(args) => cmd_provision::run(args, cli.standard.as_deref()),
        Command::Config(args) => cmd_config::run(args, cli.standard.as_deref()),
        Command::Realign(args) => cmd_realign::run(args, cli.standard.as_deref()),
        Command::Standard(args) => cmd_standard::run(args, cli.standard.as_deref()),
    }
}
