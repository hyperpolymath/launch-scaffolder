// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! Classify a repository's licence from its own licence text, as
//! `PROVISIONING-STANDARD.adoc` §6 requires before a single file is minted.
//!
//! Minted files carry an SPDX header from birth, so the header must match the
//! repository's classification in `3-practice/LICENCE-POLICY.adoc`. Evidence is
//! the repository's licence files and nothing else: when they do not settle the
//! question the answer is a refusal, never a guess.

use std::path::Path;

/// LICENCE-POLICY Rule 2: the only repositories that may carry PMPL. A PMPL
/// declaration anywhere else is drift, so it is refused, not minted.
pub const PMPL_REGISTER: &[&str] = &[
    "palimpsest-license",
    "palimpsest-plasma",
    "consent-aware-web",
    "insolvency-tycoon",
    "sim-public-relations",
];

/// Repositories outside the provisioning set (standard §1): `007` is all
/// rights reserved and the vaults are private stores.
pub const OUT_OF_SCOPE: &[&str] = &["007", "dev-notes-vault", "memory-vault"];

/// A classified repository: the SPDX identifiers minted files carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Licence {
    /// `__LICENSE__`: code, config and scripts.
    pub code: &'static str,
    /// `__DOC_LICENSE__`: prose documents.
    pub doc: &'static str,
    /// The Guix `license` field.
    pub guix: &'static str,
    /// Why `doc` was chosen, when no ruling names it. The campaign puts this
    /// in the PR body so the owner can ratify or correct it.
    pub unratified: Option<&'static str>,
}

const MPL: Licence = Licence {
    code: "MPL-2.0",
    doc: "CC-BY-SA-4.0",
    guix: "license:mpl2.0",
    unratified: None,
};
const AGPL: Licence = Licence {
    code: "AGPL-3.0-or-later",
    doc: "AGPL-3.0-or-later",
    guix: "license:agpl3+",
    unratified: Some(
        "LICENCE-POLICY Rule 3 names no prose licence for son-shared repositories, so docs carry the code licence",
    ),
};
// Guix has no PMPL; LICENCE-POLICY Rule 2 makes MPL-2.0 its legal fallback.
const PMPL: Licence = Licence {
    code: "PMPL-1.0-or-later",
    doc: "PMPL-1.0-or-later",
    guix: "license:mpl2.0",
    unratified: Some(
        "LICENCE-POLICY Rule 2 names no prose licence for PMPL repositories, so docs carry the code licence",
    ),
};

/// Why a repository was not classified. Each is a ledger line, not an error to
/// work around.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    OutOfScope(String),
    NoLicenceFile,
    Unrecognised(Vec<String>),
    Ambiguous(Vec<&'static str>),
    PmplOutsideRegister(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::OutOfScope(r) => {
                write!(f, "{r} is outside the provisioning set (standard §1)")
            }
            Refusal::NoLicenceFile => write!(f, "no LICENSE, LICENCE, COPYING or LICENSES/ file"),
            Refusal::Unrecognised(files) => write!(
                f,
                "licence text in {} is not MPL-2.0, AGPL-3.0 or PMPL (third-party or fork?)",
                files.join(", ")
            ),
            Refusal::Ambiguous(found) => {
                write!(f, "licence files disagree: {}", found.join(" and "))
            }
            Refusal::PmplOutsideRegister(r) => write!(
                f,
                "{r} declares PMPL but is not in the LICENCE-POLICY Rule 2 register (drift: flag, do not mint)"
            ),
        }
    }
}

impl std::error::Error for Refusal {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Signal {
    Mpl,
    Agpl,
    Pmpl,
}

impl Signal {
    fn name(self) -> &'static str {
        match self {
            Signal::Mpl => "MPL-2.0",
            Signal::Agpl => "AGPL-3.0",
            Signal::Pmpl => "PMPL",
        }
    }
}

/// What one licence text declares. PMPL is derived from MPL and names it, so a
/// Palimpsest text is PMPL however often it mentions Mozilla.
fn signal(text: &str) -> Option<Signal> {
    let t = text.to_ascii_lowercase();
    if t.contains("palimpsest") || t.contains("pmpl-1.0") {
        Some(Signal::Pmpl)
    } else if t.contains("gnu affero general public license") || t.contains("agpl-3.0") {
        Some(Signal::Agpl)
    } else if (t.contains("mozilla public license") && t.contains("2.0")) || t.contains("mpl-2.0") {
        Some(Signal::Mpl)
    } else {
        None
    }
}

fn is_licence_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n.starts_with("LICENSE") || n.starts_with("LICENCE") || n.starts_with("COPYING")
}

/// Classify `target`, whose repository name is `repo`.
pub fn classify(target: &Path, repo: &str) -> Result<Licence, Refusal> {
    if OUT_OF_SCOPE.contains(&repo) {
        return Err(Refusal::OutOfScope(repo.to_string()));
    }
    // The root licence files are the declaration. LICENSES/ (REUSE layout) is
    // read only when the root has none, and there its CC-BY-SA text is the prose
    // licence, not a competing code licence.
    let mut files: Vec<std::path::PathBuf> = read_dir_sorted(target)
        .into_iter()
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(is_licence_name)
        })
        .collect();
    if files.is_empty() {
        files = read_dir_sorted(&target.join("LICENSES"))
            .into_iter()
            .filter(|p| p.is_file())
            .collect();
    }
    if files.is_empty() {
        return Err(Refusal::NoLicenceFile);
    }
    let mut found: Vec<Signal> = Vec::new();
    let mut unread = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        match signal(&text) {
            Some(s) => {
                if !found.contains(&s) {
                    found.push(s);
                }
            }
            None => unread.push(
                f.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            ),
        }
    }
    match found.as_slice() {
        [] => Err(Refusal::Unrecognised(unread)),
        [Signal::Mpl] => Ok(MPL),
        [Signal::Agpl] => Ok(AGPL),
        [Signal::Pmpl] if PMPL_REGISTER.contains(&repo) => Ok(PMPL),
        [Signal::Pmpl] => Err(Refusal::PmplOutsideRegister(repo.to_string())),
        many => Err(Refusal::Ambiguous(many.iter().map(|s| s.name()).collect())),
    }
}

fn read_dir_sorted(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(files: &[(&str, &str)]) -> std::path::PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("licence-test-{}-{n}", std::process::id()));
        for (p, body) in files {
            let f = d.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, body).unwrap();
        }
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const MPL_TEXT: &str =
        "Mozilla Public License Version 2.0\n==================================\n";
    const AGPL_TEXT: &str = "                    GNU AFFERO GENERAL PUBLIC LICENSE\n                       Version 3, 19 November 2007\n";
    const PMPL_TEXT: &str =
        "Palimpsest-MPL License 1.0\nderived from the Mozilla Public License Version 2.0\n";

    #[test]
    fn mpl_repo_gets_cc_by_sa_docs() {
        let d = repo(&[("LICENSE", MPL_TEXT)]);
        assert_eq!(classify(&d, "aerie"), Ok(MPL));
    }

    #[test]
    fn agpl_control_is_classified_agpl_and_flagged() {
        let d = repo(&[("LICENSE.txt", AGPL_TEXT)]);
        let l = classify(&d, "idaptik").unwrap();
        assert_eq!(l.code, "AGPL-3.0-or-later");
        assert_eq!(l.guix, "license:agpl3+");
        assert!(l.unratified.is_some());
    }

    #[test]
    fn pmpl_only_inside_the_register() {
        let d = repo(&[("LICENSE", PMPL_TEXT)]);
        assert_eq!(
            classify(&d, "insolvency-tycoon").unwrap().code,
            "PMPL-1.0-or-later"
        );
        assert_eq!(
            classify(&d, "aerie"),
            Err(Refusal::PmplOutsideRegister("aerie".into()))
        );
    }

    #[test]
    fn refusals_name_their_reason() {
        assert_eq!(classify(&repo(&[]), "x"), Err(Refusal::NoLicenceFile));
        assert_eq!(
            classify(&repo(&[("LICENSE", "MIT License\n")]), "x"),
            Err(Refusal::Unrecognised(vec!["LICENSE".into()]))
        );
        assert_eq!(
            classify(&repo(&[("LICENSE", MPL_TEXT), ("COPYING", AGPL_TEXT)]), "x"),
            Err(Refusal::Ambiguous(vec!["AGPL-3.0", "MPL-2.0"]))
        );
        assert_eq!(
            classify(&repo(&[("LICENSE", MPL_TEXT)]), "007"),
            Err(Refusal::OutOfScope("007".into()))
        );
    }

    #[test]
    fn reuse_layout_is_read_when_the_root_has_no_licence() {
        let d = repo(&[
            ("LICENSES/MPL-2.0.txt", MPL_TEXT),
            (
                "LICENSES/CC-BY-SA-4.0.txt",
                "Creative Commons Attribution-ShareAlike 4.0\n",
            ),
        ]);
        assert_eq!(classify(&d, "x"), Ok(MPL));
    }
}
