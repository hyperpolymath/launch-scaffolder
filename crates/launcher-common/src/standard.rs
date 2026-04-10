// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! `launcher-standard.a2ml` loader.
//!
//! The standard file documents the shape of a compliant launcher. Most
//! fields are consumed by the Tera template at render time (via template
//! context), so the parser is deliberately shallow: we load the raw TOML,
//! validate that the required top-level sections are present, and hand the
//! `toml::Value` tree to the template layer.
//!
//! A stricter, strongly-typed parser can land once the standard itself
//! stabilises — right now it's still in 0.1.x and the shape is evolving.

use crate::Result;
use anyhow::Context;
use std::path::Path;

/// The default standard, baked into the binary at build time. Override with
/// `--standard <file>` or `$LAUNCH_SCAFFOLDER_STANDARD` for dev workflows.
pub const BAKED_STANDARD: &str = include_str!("../../../standards/launcher-standard.a2ml");

/// Parsed standard. Currently just a raw `toml::Value` — see module docs.
#[derive(Debug, Clone)]
pub struct LauncherStandard {
    pub raw: toml::Value,
    pub spec_version: String,
}

impl LauncherStandard {
    /// Load the baked-in standard.
    pub fn baked() -> Result<Self> {
        Self::parse(BAKED_STANDARD)
    }

    /// Load a standard from a file on disk.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading standard {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing standard {}", path.display()))
    }

    /// Parse a standard from an in-memory string.
    pub fn parse(text: &str) -> Result<Self> {
        let raw: toml::Value = toml::from_str(text).context("standard is not valid TOML/A2ML")?;
        let spec_version = raw
            .get("spec")
            .and_then(|s| s.get("version"))
            .and_then(|v| v.as_str())
            .context("standard is missing [spec].version")?
            .to_string();

        for required in [
            "required-modes",
            "runtime",
            "integration",
            "a2ml-metadata-block",
        ] {
            if raw.get(required).is_none() {
                anyhow::bail!("standard is missing required section [{required}]");
            }
        }

        Ok(Self { raw, spec_version })
    }

    /// Resolve a standard using the documented three-step precedence:
    ///
    /// 1. An explicit file path (typically from `--standard <FILE>` or
    ///    `$LAUNCH_SCAFFOLDER_STANDARD`, which clap already merges into
    ///    one `Option`).
    /// 2. The canonical path in the `standards` monorepo, if present.
    /// 3. The baked-in fallback compiled into the binary at build time.
    ///
    /// This is the entry point every subcommand should use. Duplicating
    /// the three-step precedence across subcommands was a hazard after
    /// `cmd_realign` landed, so it lives here instead.
    pub fn resolve(flag: Option<&Path>) -> Result<Self> {
        if let Some(path) = flag {
            tracing::debug!("loading standard from flag: {}", path.display());
            return Self::load(path);
        }
        let canonical =
            Path::new("/var/mnt/eclipse/repos/standards/launcher/launcher-standard.a2ml");
        if canonical.exists() {
            tracing::debug!(
                "loading standard from canonical path: {}",
                canonical.display()
            );
            return Self::load(canonical);
        }
        tracing::debug!("loading baked standard");
        Self::baked()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_standard_parses() {
        let s = LauncherStandard::baked().expect("baked standard must parse");
        assert_eq!(s.spec_version, "0.1.0");
    }
}
