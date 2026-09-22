// SPDX-License-Identifier: MPL-2.0
//! The DEED reader is run against the *same* corpus as `deed_lint.py`.
//!
//! This file exists for one reason. The estate has exactly one normative DEED
//! grammar — `1-formats/deed/spec/abnf/deed.abnf` in `hyperpolymath/standards`
//! (owner ruling 2026-09-19, standards#837). A second implementation that is
//! only ever tested against files it was written from will quietly drift into
//! being a *second* authority: it accepts documents the estate rejects, or
//! rejects documents the estate accepts, and nothing notices until a launcher
//! is minted from a file the linter would have refused.
//!
//! Running the upstream corpus is the cheapest available defence. Every file
//! under `fixtures/deed/valid/` must parse; every file under `invalid/` must
//! not. A disagreement here is a real finding about one of the two readers,
//! never a reason to quietly drop the fixture.
//!
//! See `fixtures/deed/MANIFEST.sha256` for provenance and the refresh
//! procedure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use launch_scaffolder_common::deed::{self, Value};
use sha2::{Digest, Sha256};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/deed")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `.deed` under a subdirectory, sorted, so failures name a stable file.
fn corpus(sub: &str) -> Vec<PathBuf> {
    let dir = fixtures_dir().join(sub);
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "deed"))
        .collect();
    out.sort();
    out
}

/// The vendored copies still match the digests recorded when they were taken.
///
/// This is the drift detector. Without it, "the corpus passes" degrades into
/// "the corpus, as someone later edited it to pass, passes" — which is how a
/// vendored fixture set stops testing anything.
#[test]
fn vendored_corpus_matches_its_manifest() {
    let root = fixtures_dir();
    let manifest = read(&root.join("MANIFEST.sha256"));

    let mut recorded: BTreeMap<String, String> = BTreeMap::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (digest, rel) = line
            .split_once("  ")
            .unwrap_or_else(|| panic!("malformed manifest line: {line:?}"));
        recorded.insert(rel.to_string(), digest.to_string());
    }
    assert!(
        !recorded.is_empty(),
        "MANIFEST.sha256 records no files — the drift detector would pass vacuously"
    );

    let mut on_disk: BTreeMap<String, String> = BTreeMap::new();
    for sub in ["valid", "invalid"] {
        for path in corpus(sub) {
            let name = path.file_name().expect("file name").to_string_lossy();
            let digest = hex::encode(Sha256::digest(std::fs::read(&path).expect("read")));
            on_disk.insert(format!("{sub}/{name}"), digest);
        }
    }

    // Compare the SETS, not the counts: a file added and a file deleted in the
    // same edit leaves the count unchanged.
    let recorded_names: Vec<&String> = recorded.keys().collect();
    let on_disk_names: Vec<&String> = on_disk.keys().collect();
    assert_eq!(
        recorded_names, on_disk_names,
        "the set of vendored fixtures differs from the set the manifest records"
    );

    for (name, want) in &recorded {
        assert_eq!(
            on_disk.get(name),
            Some(want),
            "{name} has been edited in place since it was vendored — \
             refresh it from standards rather than adjusting it to pass"
        );
    }
}

/// Every upstream-valid document parses.
#[test]
fn every_valid_fixture_parses() {
    let files = corpus("valid");
    assert!(files.len() >= 5, "valid corpus is suspiciously small");
    for path in files {
        let text = read(&path);
        if let Err(e) = deed::parse(&text) {
            panic!(
                "{} is VALID upstream but this reader rejected it: {e:#}\n\
                 The ABNF is the authority. Extend the parser; do not drop the fixture.",
                path.display()
            );
        }
    }
}

/// Every upstream-invalid document is rejected, **and for the defect it is
/// named after**.
///
/// Rejection alone is a weak assertion: a fixture can fail for an accidental
/// reason (a tab check firing before the `[section]` it was written to catch)
/// and the test still passes, so the rule it was meant to cover is never
/// exercised. Each fixture therefore pins a fragment of its expected message.
/// If a refactor changes the wording, update the fragment *after* confirming
/// the new message still describes the same defect.
#[test]
fn every_invalid_fixture_is_rejected_for_the_right_reason() {
    // fixture stem -> a distinctive fragment of the message it must produce
    let expected: BTreeMap<&str, &str> = BTreeMap::from([
        ("inequals", "'=' as a field separator is not a deed"),
        ("inescape-u", "invalid escape"),
        ("inhead", "invalid doc-head"),
        (
            "inmissing-schema",
            "exactly one :schema-version STRING field",
        ),
        ("inno-header", "must open with at least one"),
        ("insection", "expected field"),
        ("intab", "HTAB (tab) is an invalid separator"),
        ("intrailing", "trailing content after the closing"),
        ("intrue-literal", "booleans are #t / #f only"),
        ("inunbalanced", "never closes"),
    ]);

    let files = corpus("invalid");
    assert!(files.len() >= 10, "invalid corpus is suspiciously small");

    for path in files {
        let text = read(&path);
        let name = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .to_string();
        let stem = name.trim_end_matches("_chora.deed").to_string();

        let err = match deed::parse(&text) {
            Ok(_) => panic!(
                "{name} is INVALID upstream but this reader accepted it.\n\
                 A reader more permissive than deed_lint.py mints launchers from \
                 files the estate rejects."
            ),
            Err(e) => format!("{e:#}"),
        };

        let want = expected.get(stem.as_str()).unwrap_or_else(|| {
            panic!(
                "{name} is a new invalid fixture with no expected-reason entry. \
                 Add one rather than letting it pass on any rejection at all."
            )
        });
        assert!(
            err.contains(want),
            "{name} was rejected, but for the WRONG reason.\n  expected to contain: {want}\n               actual: {err}\nA fixture that fails incidentally does not test the rule it names."
        );
    }
}

/// **The mutant killer.**
///
/// `scrambled-priority_praxis.deed` lists its rungs in file order 30, 10, 20,
/// 20. A reader that ignores `:priority` and iterates the file returns them in
/// that order and passes every test written against the real
/// `launcher-standard_praxis.deed`, whose ladder happens to be written in
/// ascending order already.
///
/// To confirm this test has teeth: comment out the `sort_by_key` in
/// `Node::children_by_priority` and watch it go red.
#[test]
fn ladder_is_ordered_by_priority_not_by_file_position() {
    let path = fixtures_dir().join("valid/scrambled-priority_praxis.deed");
    let doc = deed::parse(&read(&path)).expect("fixture parses");

    let search = doc
        .clause("resolution")
        .expect("(resolution …)")
        .clause("standard-search")
        .expect("(standard-search …)");

    // Sanity: the FILE really is scrambled. If this ever reads 10,20,20,30 the
    // fixture has been tidied and the test below has silently lost its teeth.
    let file_order: Vec<i64> = search
        .clauses_named("path")
        .map(|p| {
            p.field("priority")
                .and_then(Value::as_int)
                .expect(":priority")
        })
        .collect();
    assert_eq!(
        file_order,
        vec![30, 10, 20, 20],
        "the fixture is no longer scrambled — this test can no longer detect \
         a reader that ignores :priority"
    );

    let sorted = search.children_by_priority("path");
    let priorities: Vec<i64> = sorted
        .iter()
        .map(|p| {
            p.field("priority")
                .and_then(Value::as_int)
                .expect(":priority")
        })
        .collect();
    assert_eq!(
        priorities,
        vec![10, 20, 20, 30],
        "rungs must sort ascending"
    );

    let values: Vec<&str> = sorted
        .iter()
        .map(|p| p.str_field("value").expect(":value"))
        .collect();
    assert_eq!(
        values,
        vec![
            "$FIRST/first.deed",
            "$SECOND_BETA/beta.deed",
            "$SECOND_GAMMA/gamma.deed",
            "$THIRD/third.deed"
        ],
        "the tie at priority 20 must keep source order — the sort must be stable"
    );
}

/// A clause with a head and no body is legal (`(deployment)` in the
/// rsr-template-repo fixture), and nesting is preserved.
#[test]
fn headless_clauses_and_nesting_survive() {
    let doc = deed::parse(&read(
        &fixtures_dir().join("valid/rsr-template-repo_chora.deed"),
    ))
    .expect("fixture parses");
    assert_eq!(doc.head, "repo-deed");

    // `(deployment)` is not a top-level clause — it sits inside `(playbook)`,
    // alongside four more head-only clauses on the same line. Reaching it at
    // all is the nesting half of this test.
    let playbook = doc.clause("playbook").expect("(playbook …)");
    for head in [
        "deployment",
        "incident-response",
        "release-process",
        "docs-format",
        "maintenance-operations",
    ] {
        let empty = playbook
            .clause(head)
            .unwrap_or_else(|| panic!("({head}) is a clause with a head and no body"));
        assert!(
            empty.fields.is_empty() && empty.clauses.is_empty(),
            "({head}) should have parsed with an empty body"
        );
    }

    // A sibling clause on the same nesting level that DOES have a body, so the
    // test cannot pass by finding everything empty.
    let skeleton = playbook.clause("skeleton").expect("(skeleton …)");
    assert_eq!(skeleton.str_field("version"), Some("1.0"));
}
