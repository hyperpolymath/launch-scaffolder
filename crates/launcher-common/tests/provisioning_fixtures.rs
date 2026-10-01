// SPDX-License-Identifier: MPL-2.0
//! The provisioning fixtures, driven through the real engine.
//!
//! `check/` is a broken repository with exactly three faults. Each repair in
//! `check-repairs/` must remove its own FAIL and no other (kill the mutant),
//! and all three together must bring `provision-check.sh` to rc 0 — the
//! positive control, without which "exactly three FAILs" could be a checker
//! that fails everything.
//!
//! `lang/` holds one small repository per detection case: `langs` must name
//! the right language, the deno leftovers must raise PV-W30 (and stop raising
//! it once removed), and an offline mint must merge each custom Justfile
//! recipe into its `-local` twin.
//!
//! The engine is bash and needs `just` and `git`: a missing tool fails these
//! tests loudly, because a skipped conformance test is not a pass.

use launch_scaffolder_common::provisioning::{
    canon::Canon,
    mint::{self, Act, Options},
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/provisioning");

/// The canon engine library, which the `lang/` fixtures do not carry.
const LIB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../standards/provisioning/templates/build/just/provision-lib.sh"
);

/// The three faults planted in `check/`, as `provision-check.sh` words them.
const CHECK_FAILS: [&str; 3] = [
    "root recipe missing: fmt-check",
    "mise.lock is empty (latest is not concrete; run: mise lock)",
    "guix.scm: no package field: version build-system home-page synopsis description license",
];

/// Copy fixture `rel` to a fresh directory named `tag` under cargo's test
/// tmpdir, and make it a git repository (the engine takes the repo name from
/// git, so an un-initialised copy would report the enclosing checkout).
fn scratch(rel: &str, tag: &str) -> PathBuf {
    let dst = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("provisioning-{tag}"));
    let _ = std::fs::remove_dir_all(&dst);
    let ok = Command::new("cp")
        .arg("-r")
        .arg(Path::new(FIXTURES).join(rel))
        .arg(&dst)
        .status()
        .expect("cp")
        .success();
    assert!(ok, "copying fixture {rel}");
    let ok = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dst)
        .status()
        .expect("git is required by the provisioning engine")
        .success();
    assert!(ok, "git init in {}", dst.display());
    dst
}

/// Run the repository's own `provision-check.sh --dev`; return its exit code
/// and the set of FAIL messages it printed.
fn provision_check(repo: &Path) -> (i32, BTreeSet<String>) {
    let out = Command::new("bash")
        .args(["build/just/provision-check.sh", "--dev", "."])
        .current_dir(repo)
        .output()
        .expect("bash");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let fails = stdout
        .lines()
        .filter_map(|l| l.trim().strip_prefix("FAIL"))
        .map(|l| l.trim().to_string())
        .collect();
    let code = out
        .status
        .code()
        .expect("provision-check.sh killed by a signal");
    (code, fails)
}

/// Run a verb of the canon `provision-lib.sh` against `repo`; return stdout
/// and stderr together.
fn lib(repo: &Path, verb: &str) -> String {
    let out = Command::new("bash")
        .arg(LIB)
        .arg(verb)
        .env("PROVISION_ROOT", repo)
        .current_dir(repo)
        .output()
        .expect("bash");
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// The FAIL set expected once the faults in `fixed` have been repaired.
fn expected_without(fixed: &[usize]) -> BTreeSet<String> {
    CHECK_FAILS
        .iter()
        .enumerate()
        .filter(|(i, _)| !fixed.contains(i))
        .map(|(_, f)| f.to_string())
        .collect()
}

/// Apply repair `i` (an index into [`CHECK_FAILS`]) to `repo`.
fn repair(repo: &Path, i: usize) {
    let repairs = Path::new(FIXTURES).join("check-repairs");
    match i {
        0 => {
            let jf = repo.join("Justfile");
            let mut s = std::fs::read_to_string(&jf).unwrap();
            s.push_str("fmt-check: provision::fmt-check\n");
            std::fs::write(jf, s).unwrap();
        }
        1 => {
            std::fs::copy(repairs.join("mise.lock"), repo.join("mise.lock")).unwrap();
        }
        2 => {
            std::fs::copy(repairs.join("guix.scm"), repo.join("guix.scm")).unwrap();
        }
        _ => unreachable!(),
    }
}

#[test]
fn check_fixture_fails_on_exactly_its_three_faults() {
    let repo = scratch("check", "check");
    let (code, fails) = provision_check(&repo);
    assert_eq!(code, 1);
    assert_eq!(fails, expected_without(&[]));
}

#[test]
fn each_repair_removes_only_its_own_fail() {
    for i in 0..CHECK_FAILS.len() {
        let repo = scratch("check", &format!("check-repair-{i}"));
        repair(&repo, i);
        let (code, fails) = provision_check(&repo);
        assert_eq!(code, 1, "repair {i}: two faults remain");
        assert_eq!(fails, expected_without(&[i]), "repair {i}");
    }
}

#[test]
fn all_repairs_together_pass() {
    let repo = scratch("check", "check-repaired");
    for i in 0..CHECK_FAILS.len() {
        repair(&repo, i);
    }
    let (code, fails) = provision_check(&repo);
    assert!(fails.is_empty(), "{fails:?}");
    assert_eq!(code, 0);
}

#[test]
fn langs_names_each_fixture_language() {
    for (fixture, lang) in [
        ("docsr", "docs"),
        ("idr", "idris2"),
        ("rustd", "rust"),
        ("rustr", "rust"),
        ("srcc", "docs"),
    ] {
        let repo = scratch(&format!("lang/{fixture}"), &format!("langs-{fixture}"));
        assert_eq!(lib(&repo, "langs").trim(), lang, "{fixture}");
    }
}

#[test]
fn doctor_warns_on_deno_leftovers_and_only_then() {
    for fixture in ["rustd", "rustr"] {
        let repo = scratch(&format!("lang/{fixture}"), &format!("deno-{fixture}"));
        assert!(
            lib(&repo, "doctor").contains("PV-W30 deno.json"),
            "{fixture}"
        );
        std::fs::remove_file(repo.join("deno.json")).unwrap();
        assert!(
            !lib(&repo, "doctor").contains("PV-W30"),
            "{fixture} without deno.json"
        );
    }
}

#[test]
fn offline_mint_keeps_custom_recipes_as_local_twins() {
    let canon = Canon::resolve(None).unwrap();
    let licence = concat!(env!("CARGO_MANIFEST_DIR"), "/../../LICENSES/MPL-2.0.txt");
    for (fixture, verb) in [("idr", "doctor"), ("rustd", "setup"), ("rustr", "doctor")] {
        let repo = scratch(&format!("lang/{fixture}"), &format!("mint-{fixture}"));
        std::fs::copy(licence, repo.join("LICENSE")).unwrap();
        let opts = Options {
            repo: Some(format!("hyperpolymath/{fixture}")),
            year: Some(2026),
            offline: true,
            ..Options::default()
        };
        let report = mint::mint(&repo, &canon, &opts).unwrap();
        let act = |path: &str| {
            report
                .files
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, a)| a.clone())
                .unwrap_or_else(|| panic!("{fixture}: no report line for {path}"))
        };
        assert!(
            matches!(act("Justfile"), Act::Replaced(ref w) if w.contains(&format!("{verb}-local"))),
            "{fixture}: {}",
            act("Justfile")
        );
        assert!(matches!(act("mise.lock"), Act::Skipped(_)), "{fixture}");
        let jf = std::fs::read_to_string(repo.join("Justfile")).unwrap();
        assert!(jf.contains(&format!("{verb}-local")), "{fixture}");
        assert!(
            jf.contains(&format!("{verb}: provision::{verb}")),
            "{fixture}"
        );
    }
}
