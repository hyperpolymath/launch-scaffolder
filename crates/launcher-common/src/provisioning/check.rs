// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `provision-set check`: the engine files must equal the canon byte for byte,
//! and only then is the repository's own `provision-check.sh` trusted to judge
//! the rest. A drifted checker cannot be trusted to check, so drift is terminal.

use super::canon::{Canon, ENGINE_FILES};
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// One engine file that does not match the canon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    Missing(String),
    Differs(String),
}

impl std::fmt::Display for Drift {
    /// Describe the affected path and whether its engine file is missing or changed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Drift::Missing(p) => write!(f, "{p}: missing"),
            Drift::Differs(p) => write!(f, "{p}: differs from the canon"),
        }
    }
}

/// Every engine file under `target` that is absent or not byte-identical to the canon.
pub fn engine_drift(target: &Path, canon: &Canon) -> Result<Vec<Drift>> {
    let mut out = Vec::new();
    for rel in ENGINE_FILES {
        let want = canon.file(rel)?;
        match std::fs::read(target.join(rel)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                out.push(Drift::Missing(rel.to_string()))
            }
            Err(e) => {
                return Err(e).with_context(|| format!("reading {}", target.join(rel).display()));
            }
            Ok(have) if have != *want => out.push(Drift::Differs(rel.to_string())),
            Ok(_) => {}
        }
    }
    Ok(out)
}

/// Run the repository's `build/just/provision-check.sh` and return its exit
/// code. A child killed by a signal is a failure (1), never a pass.
pub fn conformance(target: &Path, dev: bool) -> Result<i32> {
    let script = target.join("build/just/provision-check.sh");
    let mut cmd = Command::new("bash");
    cmd.arg(&script);
    if dev {
        cmd.arg("--dev");
    }
    cmd.arg(target);
    let status = cmd
        .status()
        .with_context(|| format!("running {}", script.display()))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioning::canon::BAKED;

    /// Write the baked engine files into a test directory, creating parent directories.
    fn mint_engine(dir: &Path) {
        for rel in ENGINE_FILES {
            let (_, b) = BAKED.iter().find(|(p, _)| p == rel).unwrap();
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b).unwrap();
        }
    }

    /// Recreate a temporary test directory identified by process ID and case name.
    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ls-provcheck-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Verify that freshly copied canon engine files produce no drift findings.
    #[test]
    fn identical_engine_has_no_drift() {
        let d = scratch("clean");
        mint_engine(&d);
        assert!(engine_drift(&d, &Canon::Baked).unwrap().is_empty());
    }

    /// Planted positives: one byte changed and one file removed must both be seen.
    #[test]
    fn a_changed_byte_and_a_missing_file_are_both_drift() {
        let d = scratch("dirty");
        mint_engine(&d);
        let lib = d.join("build/just/provision-lib.sh");
        let mut b = std::fs::read(&lib).unwrap();
        b.push(b'\n');
        std::fs::write(&lib, b).unwrap();
        std::fs::remove_file(d.join("build/just/provision.just")).unwrap();
        let got = engine_drift(&d, &Canon::Baked).unwrap();
        assert_eq!(
            got,
            vec![
                Drift::Differs("build/just/provision-lib.sh".into()),
                Drift::Missing("build/just/provision.just".into()),
            ]
        );
    }

    /// A refreshed channels.scm is not drift: it is not an engine file.
    #[test]
    fn a_repinned_channels_scm_is_not_drift() {
        let d = scratch("chan");
        mint_engine(&d);
        std::fs::create_dir_all(d.join("guix")).unwrap();
        std::fs::write(
            d.join("guix/channels.scm"),
            "(list (channel (name 'guix) (commit \"0000000000000000000000000000000000000000\")))\n",
        )
        .unwrap();
        assert!(engine_drift(&d, &Canon::Baked).unwrap().is_empty());
    }
}
