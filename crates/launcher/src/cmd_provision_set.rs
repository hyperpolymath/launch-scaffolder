// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `provision-set` subcommand — mint, realign and check a repository's
//! provisioning set against `PROVISIONING-STANDARD.adoc`.

use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};
use launch_scaffolder_common::provisioning::{
    canon::{CANON_ENV, Canon},
    check,
    licence::Refusal,
    mint::{self, Act, Options},
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
    /// Write the provisioning set: engine files realigned, minted files
    /// created when missing and replaced only while they are stubs.
    Mint(MintArgs),
    /// The same operation as `mint`, named for an existing repository.
    Realign(MintArgs),
}

#[derive(Debug, ClapArgs)]
struct MintArgs {
    /// `owner/name` (default: the `origin` remote).
    #[arg(long)]
    repo: Option<String>,
    /// app | library | tool | theory | docs (default: the deed's, else inferred).
    #[arg(long)]
    archetype: Option<String>,
    /// Copyright year (default: SOURCE_DATE_EPOCH, else this year).
    #[arg(long)]
    year: Option<i64>,
    /// Skip `mise lock` and `guix import crate` (both need the network).
    #[arg(long)]
    offline: bool,
    /// Repository to provision.
    #[arg(default_value = ".")]
    target: PathBuf,
}

/// Exit code for a licence refusal (standard §6): a ledger line, not a crash.
pub const EXIT_REFUSED: i32 = 3;

/// Run `provision-set`: check a repository, or mint/realign its provisioning
/// set. Exits 1 on drift or a FAIL, 3 on a licence refusal, 4 when an
/// external step (`mise lock`, `guix import crate`) failed.
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
        Action::Mint(m) | Action::Realign(m) => {
            let opts = Options {
                repo: m.repo,
                archetype: m.archetype,
                year: m.year,
                offline: m.offline,
            };
            match mint::mint(&m.target, &canon, &opts) {
                Ok(r) => {
                    println!(
                        "{} — {} (docs {}), archetype {}, languages {}",
                        r.slug,
                        r.licence.code,
                        r.licence.doc,
                        r.archetype,
                        r.langs.join(", ")
                    );
                    if let Some(from) = &r.inherited_from {
                        println!("  inherited set from {from}: re-minted");
                    }
                    if let Some(why) = r.licence.unratified {
                        println!("  UNRATIFIED doc licence: {why}");
                    }
                    for (path, act) in &r.files {
                        let tag = match act {
                            Act::Created => "create",
                            Act::Replaced(_) => "replace",
                            Act::Kept(_) => "keep",
                            Act::Skipped(_) => "SKIP",
                            Act::Failed(_) => "FAIL",
                        };
                        println!("  {tag:<8} {path}: {act}");
                    }
                    if r.files.iter().any(|(_, a)| matches!(a, Act::Failed(_))) {
                        std::process::exit(mint::EXIT_EXTERNAL);
                    }
                    Ok(())
                }
                Err(e) => match e.downcast_ref::<Refusal>() {
                    Some(refusal) => {
                        eprintln!("provision-set: refused: {refusal}");
                        std::process::exit(EXIT_REFUSED);
                    }
                    None => Err(e),
                },
            }
        }
    }
}
