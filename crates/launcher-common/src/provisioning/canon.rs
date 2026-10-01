// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! The provisioning canon: `standards/3-practice/provisioning/templates`,
//! vendored at `standards/provisioning/` and baked into the binary.
//!
//! The baked table is the default. `--canon DIR` (or
//! `$LAUNCH_SCAFFOLDER_PROVISIONING_CANON`) selects a directory laid out like
//! `standards/provisioning/` instead — a `CANON` file and a `templates/` tree —
//! so an unreleased canon can be tried without rebuilding.
//!
//! Two tests keep the table honest: it must list exactly the files under
//! `templates/` (a file added to the vendor copy and not to the table would be
//! silently never minted), and its content is pinned by digest, so a re-vendor
//! announces itself in the test diff rather than passing unnoticed.

use anyhow::{Context, Result, bail};
use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Environment override for the canon directory.
pub const CANON_ENV: &str = "LAUNCH_SCAFFOLDER_PROVISIONING_CANON";

/// `standards@<sha>` the baked table was vendored from.
pub const BAKED_CANON_REF: &str = include_str!("../../../../standards/provisioning/CANON");

/// Engine files: copied byte-for-byte, owned by realign, and byte-compared by
/// `provision-set check`. `guix/channels.scm` is deliberately absent: it is
/// minted once and then re-pinned per repository by `just toolchain-refresh`,
/// so a byte comparison would fail every refreshed repository. It is checked
/// instead by the commit-pin predicate in `provision-check.sh`.
pub const ENGINE_FILES: &[&str] = &[
    "build/just/provision-check.sh",
    "build/just/provision-lib.sh",
    "build/just/provision-modes.sh",
    "build/just/provision.just",
];

/// Every canon file, by path relative to `templates/`, sorted bytewise.
pub static BAKED: &[(&str, &[u8])] = &[
    (
        ".machine_readable/descriptiles/provisioning_praxis.deed.tmpl",
        include_bytes!(
            "../../../../standards/provisioning/templates/.machine_readable/descriptiles/provisioning_praxis.deed.tmpl"
        ),
    ),
    (
        "Justfile.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/Justfile.tmpl"),
    ),
    (
        "README-ai-install.adoc.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/README-ai-install.adoc.tmpl"),
    ),
    (
        "build/just/provision-check.sh",
        include_bytes!(
            "../../../../standards/provisioning/templates/build/just/provision-check.sh"
        ),
    ),
    (
        "build/just/provision-lib.sh",
        include_bytes!("../../../../standards/provisioning/templates/build/just/provision-lib.sh"),
    ),
    (
        "build/just/provision-modes.sh",
        include_bytes!(
            "../../../../standards/provisioning/templates/build/just/provision-modes.sh"
        ),
    ),
    (
        "build/just/provision.just",
        include_bytes!("../../../../standards/provisioning/templates/build/just/provision.just"),
    ),
    (
        "docs/AI_INSTALLATION_GUIDE.adoc.tmpl",
        include_bytes!(
            "../../../../standards/provisioning/templates/docs/AI_INSTALLATION_GUIDE.adoc.tmpl"
        ),
    ),
    (
        "docs/SETUP.adoc.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/docs/SETUP.adoc.tmpl"),
    ),
    (
        "guix/channels.scm",
        include_bytes!("../../../../standards/provisioning/templates/guix/channels.scm"),
    ),
    (
        "guix/guix.scm.cargo.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/guix/guix.scm.cargo.tmpl"),
    ),
    (
        "guix/guix.scm.source.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/guix/guix.scm.source.tmpl"),
    ),
    (
        "guix/manifest.scm.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/guix/manifest.scm.tmpl"),
    ),
    (
        "launcher.sh.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/launcher.sh.tmpl"),
    ),
    (
        "llm-warmup-dev.adoc.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/llm-warmup-dev.adoc.tmpl"),
    ),
    (
        "llm-warmup-maintainer.adoc.tmpl",
        include_bytes!(
            "../../../../standards/provisioning/templates/llm-warmup-maintainer.adoc.tmpl"
        ),
    ),
    (
        "llm-warmup-user.adoc.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/llm-warmup-user.adoc.tmpl"),
    ),
    (
        "mise.toml.tmpl",
        include_bytes!("../../../../standards/provisioning/templates/mise.toml.tmpl"),
    ),
];

/// Where canon bytes come from.
#[derive(Debug, Clone)]
pub enum Canon {
    Baked,
    Dir(PathBuf),
}

impl Canon {
    /// The explicit override if given, else the baked table.
    pub fn resolve(dir: Option<&Path>) -> Result<Self> {
        match dir {
            None => Ok(Canon::Baked),
            Some(d) => {
                let t = d.join("templates");
                if !t.is_dir() {
                    bail!("canon override {} has no templates/ directory", d.display());
                }
                Ok(Canon::Dir(d.to_path_buf()))
            }
        }
    }

    /// The `standards@<sha>` this canon names.
    pub fn reference(&self) -> Result<String> {
        let raw = match self {
            Canon::Baked => BAKED_CANON_REF.to_string(),
            Canon::Dir(d) => std::fs::read_to_string(d.join("CANON"))
                .with_context(|| format!("reading {}", d.join("CANON").display()))?,
        };
        Ok(raw.trim().to_string())
    }

    /// The bytes of one canon file.
    pub fn file(&self, rel: &str) -> Result<Cow<'static, [u8]>> {
        match self {
            Canon::Baked => BAKED
                .iter()
                .find(|(p, _)| *p == rel)
                .map(|(_, b)| Cow::Borrowed(*b))
                .with_context(|| format!("{rel} is not in the baked provisioning canon")),
            Canon::Dir(d) => {
                let p = d.join("templates").join(rel);
                std::fs::read(&p)
                    .map(Cow::Owned)
                    .with_context(|| format!("reading canon file {}", p.display()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vendor_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../standards/provisioning/templates")
    }

    #[test]
    fn the_table_lists_exactly_the_vendored_files() {
        let mut on_disk: Vec<String> = walkdir::WalkDir::new(vendor_dir())
            .into_iter()
            .map(|e| e.expect("walk vendored canon"))
            .filter(|e| e.file_type().is_file())
            .map(|e| {
                e.path()
                    .strip_prefix(vendor_dir())
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        on_disk.sort();
        let table: Vec<String> = BAKED.iter().map(|(p, _)| p.to_string()).collect();
        assert_eq!(
            table, on_disk,
            "regenerate BAKED from standards/provisioning/templates"
        );
    }

    #[test]
    fn every_engine_file_is_in_the_table() {
        for e in ENGINE_FILES {
            assert!(BAKED.iter().any(|(p, _)| p == e), "{e} missing from BAKED");
        }
        assert!(!ENGINE_FILES.contains(&"guix/channels.scm"));
    }

    /// Digest over sorted `path NUL sha256 LF` lines. A re-vendor changes it;
    /// bump this pin and `standards/provisioning/CANON` in the same commit.
    #[test]
    fn the_baked_canon_is_pinned_by_content() {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        for (p, b) in BAKED {
            h.update(p.as_bytes());
            h.update([0u8]);
            h.update(format!("{:x}", Sha256::digest(b)).as_bytes());
            h.update(b"\n");
        }
        let got = format!("{:x}", h.finalize());
        assert_eq!(
            got, PINNED_DIGEST,
            "the baked provisioning canon changed: confirm standards/provisioning/CANON \
             names the commit it came from, then update this pin in the same commit"
        );
        assert!(BAKED_CANON_REF.trim().starts_with("standards@"));
    }

    const PINNED_DIGEST: &str = "f4337dd6d042fd028fde2f6c7d970edd223225ad29d033135dc3128065a50e83";
}
