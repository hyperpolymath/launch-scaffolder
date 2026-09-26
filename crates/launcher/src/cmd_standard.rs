// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `standard` subcommand — inspect and validate the launcher standard.

use anyhow::{Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use launch_scaffolder_common::standard::{BAKED_STANDARD, LauncherStandard};
use std::path::Path;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Print the resolved launcher standard (the baked copy when no external
    /// standard is available).
    Show,
    /// Parse and validate the selected launcher standard.
    Validate,
}

pub fn run(args: Args, standard_path: Option<&Path>) -> Result<()> {
    match args.action {
        Action::Show => show(standard_path),
        Action::Validate => validate(standard_path),
    }
}

fn show(standard_path: Option<&Path>) -> Result<()> {
    let text = match LauncherStandard::source_path(standard_path) {
        Some(path) => std::fs::read_to_string(&path)
            .with_context(|| format!("reading standard {}", path.display()))?,
        None => {
            // Preserve the usual fallback diagnostic before selecting the
            // embedded text. `show` parses the exact bytes it will print below.
            LauncherStandard::resolve(standard_path)?;
            BAKED_STANDARD.to_string()
        }
    };
    let standard = LauncherStandard::parse(&text).context("validating selected launcher standard")?;
    print!("{text}");
    tracing::debug!("showed launcher standard version {}", standard.spec_version);
    Ok(())
}

fn validate(standard_path: Option<&Path>) -> Result<()> {
    let standard = LauncherStandard::resolve(standard_path)?;
    let required = standard.metadata_required_fields()?;
    let platforms = standard.platforms()?;
    let covered = standard.lifecycle_phases_covered()?;
    let deferred = standard.lifecycle_phases_deferred()?;

    let modes = standard
        .doc
        .clause("required-modes")
        .context("standard is missing the (required-modes) clause")?;
    for key in ["runtime", "integration", "meta"] {
        let field = modes
            .field(key)
            .with_context(|| format!("(required-modes) is missing :{key}"))?;
        let entries = field
            .str_list();
        if entries.is_empty() {
            anyhow::bail!("(required-modes :{key}) must be a non-empty string list");
        }
    }

    println!(
        "✓ launcher standard {} — {} metadata fields, {} platforms, {} phases covered, {} deferred",
        standard.spec_version,
        required.len(),
        platforms.len(),
        covered.len(),
        deferred.len(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_baked_standard_passes_semantic_validation() {
        validate(None).expect("the committed standard should validate");
    }
}
