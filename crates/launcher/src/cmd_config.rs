// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `config` subcommand — get, set, or validate the metadata block
//! embedded in a generated launcher script.
//!
//! Two dialects exist and this surface reads both: the DEED form
//! (`@launcher-deed`), which `mint` emits, and the retired
//! `@a2ml-metadata` form carried by launchers minted before that
//! change. Reading is dialect-agnostic; `set` is not, and says so —
//! a DEED form cannot be safely edited in place, so `set` refuses it
//! and directs the caller to re-mint instead.
//!
//! All real work lives in `launch_scaffolder_common::metadata_block`.
//! This file is the CLI surface: three sub-actions, one script path
//! per invocation, and a hard warning on `set` that the next
//! `launch-scaffolder realign` will overwrite the change unless the
//! source `<app>.launcher.a2ml` is updated first.

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use launch_scaffolder_common::metadata_block;
use std::path::{Path, PathBuf};

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Print the value of a scalar metadata key from a generated script.
    Get {
        /// Path to a generated `<app>-launcher.sh`.
        script: PathBuf,
        /// Key to read (e.g. `version`, `app-name`, `runtime-kind`).
        key: String,
    },
    /// Replace a scalar metadata value in place. Warns that realign
    /// will overwrite the change unless the source config is updated.
    Set {
        /// Path to a generated `<app>-launcher.sh`.
        script: PathBuf,
        /// Scalar key to update.
        key: String,
        /// New value. Not quoted — passed through verbatim into the
        /// block's existing `"..."` slot.
        value: String,
    },
    /// Check that the embedded metadata block is well-formed and
    /// carries every required key.
    Validate {
        /// Path to a generated `<app>-launcher.sh`.
        script: PathBuf,
    },
}

pub fn run(args: Args, _standard: Option<&Path>) -> Result<()> {
    match args.action {
        Action::Get { script, key } => cmd_get(&script, &key),
        Action::Set { script, key, value } => cmd_set(&script, &key, &value),
        Action::Validate { script } => cmd_validate(&script),
    }
}

fn cmd_get(script: &Path, key: &str) -> Result<()> {
    let block = metadata_block::parse_from_script(script)?.with_context(|| {
        format!(
            "no launcher metadata block (@launcher-deed or @a2ml-metadata) found in {}",
            script.display()
        )
    })?;
    if let Some(v) = block.scalar(key) {
        println!("{v}");
        return Ok(());
    }
    if let Some(list) = block.list(key) {
        for item in list {
            println!("{item}");
        }
        return Ok(());
    }
    anyhow::bail!("key `{}` not present in metadata block", key)
}

fn cmd_set(script: &Path, key: &str, value: &str) -> Result<()> {
    let text =
        std::fs::read_to_string(script).with_context(|| format!("reading {}", script.display()))?;
    let rewritten = metadata_block::rewrite_scalar(&text, key, value)?;
    std::fs::write(script, &rewritten).with_context(|| format!("writing {}", script.display()))?;
    println!("✓ {}: {} = \"{}\"", script.display(), key, value);
    eprintln!(
        "⚠ realign will overwrite this change. Update the source <app>.launcher.a2ml \
         and re-mint to persist it."
    );
    Ok(())
}

fn cmd_validate(script: &Path) -> Result<()> {
    let block = metadata_block::parse_from_script(script)?.with_context(|| {
        format!(
            "no launcher metadata block (@launcher-deed or @a2ml-metadata) found in {}",
            script.display()
        )
    })?;
    let missing = block.missing_required();
    if missing.is_empty() {
        println!(
            "✓ {} — {} scalar keys, {} list keys, all required fields present",
            script.display(),
            block.scalars.len(),
            block.lists.len()
        );
        return Ok(());
    }
    for key in &missing {
        eprintln!("✗ missing required key: {key}");
    }
    anyhow::bail!(
        "{}: {} required key(s) missing from the launcher metadata block",
        script.display(),
        missing.len()
    )
}
