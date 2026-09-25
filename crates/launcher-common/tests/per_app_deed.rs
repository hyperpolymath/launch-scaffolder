// SPDX-License-Identifier: MPL-2.0
//! The per-app launcher descriptor form (#42) — conformance pair and
//! vocabulary.
//!
//! `docs/per-app-launcher-descriptor-deed.adoc` rules the filename arm
//! (`<app>.launcher_praxis.deed`), the beholding question (`#u5"estate/chora"`)
//! and the field vocabulary. This file is what stops that document from being
//! prose: the valid fixture carries every clause and field the vocabulary
//! names, and each one is asserted reachable through the reader, so a
//! vocabulary change that the fixture does not reflect — or a clause the
//! reader cannot reach — fails here instead of during a conversion of 21
//! files.
//!
//! The fixtures live in their own tree rather than in `fixtures/deed/`,
//! because that corpus is **manifest-locked**: `MANIFEST.sha256` asserts set
//! equality between what is vendored upstream and what is on disk, so adding
//! a file there means refreshing the corpus from `standards`. Upstreaming
//! this trio is follow-up.
//!
//! Rejections pin a fragment of the expected message, the same discipline the
//! vendored corpus uses (`deed_corpus.rs`). Rejection alone is a weak
//! assertion: a fixture can fail for an accidental reason and still pass, so
//! the rule it was written to cover is never exercised.

use std::path::{Path, PathBuf};

use launch_scaffolder_common::deed::{self, Value};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/per_app_deed")
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The ruled form: `stapeln.launcher_praxis.deed`.
///
/// The filename is asserted as well as the contents — arm B is chosen
/// precisely because `stapeln.launcher` is a legal stem that dispatches to
/// `praxis-deed`, and a fixture named anything else would not be testing the
/// ruling.
#[test]
fn the_ruled_filename_is_the_one_that_is_committed() {
    let path = fixtures_dir().join("valid/stapeln.launcher_praxis.deed");
    assert!(
        path.exists(),
        "the valid fixture must be named `<app>.launcher_praxis.deed` (#42 arm B)"
    );

    let text = read(&path);
    let doc = deed::parse(&text).expect("the valid descriptor parses");

    // The filename dispatch and the doc-head must agree — a mismatch is a
    // validation error, and this is where that is enforced for this form.
    assert_eq!(doc.head, "praxis-deed");
    assert_eq!(doc.str_field("canonical-name"), Some("stapeln"));
}

/// The three required praxis fields, including the beholding ruling: a uuid5
/// naming `estate/chora`, never a bare filename.
#[test]
fn the_required_praxis_fields_are_present_and_beholding_is_a_uuid5() {
    let doc = deed::parse(&read(
        &fixtures_dir().join("valid/stapeln.launcher_praxis.deed"),
    ))
    .expect("parses");

    assert_eq!(doc.str_field("schema-version"), Some("1.0.0"));
    assert_eq!(doc.str_field("canonical-name"), Some("stapeln"));

    match doc.field("beholding-chora") {
        Some(Value::Uuid5(name)) => assert_eq!(
            name, "estate/chora",
            "ruled: every converted descriptor beholds the estate chora (#42 ruling 2)"
        ),
        other => panic!(":beholding-chora must be a uuid5, got {other:?}"),
    }
}

/// Every clause and field the vocabulary names is present and typed.
///
/// Written as one assertion block per clause so a failure names the clause,
/// and so adding a field to the vocabulary without adding it to the fixture
/// is a failure rather than a silent omission.
#[test]
fn every_field_of_the_vocabulary_is_reachable() {
    let doc = deed::parse(&read(
        &fixtures_dir().join("valid/stapeln.launcher_praxis.deed"),
    ))
    .expect("parses");

    // `:standard-version` is the DOCUMENT version of the launcher standard,
    // not the grammar's `:schema-version`. Conflating them makes a stale
    // document read as a newer spec.
    let compliance = doc.clause("compliance").expect("(compliance …)");
    assert_eq!(compliance.str_field("standard-version"), Some("0.4.0"));

    let project = doc.clause("project").expect("(project …)");
    for (key, want) in [
        ("name", "stapeln"),
        ("display", "Stapeln"),
        ("description", "A launcher descriptor, in deed form"),
        ("version", "0.1.0"),
        ("license", "MPL-2.0"),
        ("generic-name", "Stack Manager"),
    ] {
        assert_eq!(
            project.str_field(key),
            Some(want),
            "(project :{key} …) must read back as a string"
        );
    }
    assert_eq!(
        project.field("categories").map(Value::str_list),
        Some(vec!["Development", "Utility"]),
        "`categories` is a list of strings, not a string"
    );

    let repo = doc.clause("repo").expect("(repo …)");
    assert_eq!(repo.str_field("path"), Some("/srv/stapeln"));

    let runtime = doc.clause("runtime").expect("(runtime …)");
    assert_eq!(runtime.str_field("kind"), Some("server-url"));
    assert_eq!(runtime.str_field("url"), Some("http://localhost:4010"));
    // Integers are integers: a port that reads back as a string has been
    // through a TOML-shaped reader.
    assert_eq!(runtime.field("port").and_then(Value::as_int), Some(4010));
    assert_eq!(
        runtime
            .field("wait-for-url-timeout-seconds")
            .and_then(Value::as_int),
        Some(15)
    );
    assert_eq!(
        runtime.field("startup-command-search").map(Value::str_list),
        Some(vec!["./stapeln", "cargo run"])
    );
    // The empty list is the deed spelling of "no explicit command".
    assert_eq!(
        runtime.field("command").and_then(Value::as_list),
        Some(&[][..])
    );

    let icon = doc.clause("icon").expect("(icon …)");
    assert_eq!(
        icon.str_field("source"),
        Some("{repo-dir}/assets/icon-256.png")
    );

    let soft_attach = doc.clause("soft-attach").expect("(soft-attach …)");
    assert_eq!(
        soft_attach.field("tools").map(Value::str_list),
        Some(vec!["feedback-o-tron", "hypatia"])
    );
}

/// The two defects a straight TOML→deed rename produces, each rejected for
/// the reason it is named after.
#[test]
fn the_invalid_pair_is_rejected_for_the_right_reason() {
    let expected: &[(&str, &str)] = &[
        // A `[section]` header: TOML's structure, which a deed does not have.
        ("insection", "expected field"),
        // A tab as a separator, which TOML permits and deed.abnf forbids.
        ("intab", "HTAB (tab) is an invalid separator"),
    ];

    for (stem, want) in expected {
        let path = fixtures_dir().join(format!("invalid/{stem}_launcher_praxis.deed"));
        let err = match deed::parse(&read(&path)) {
            Ok(_) => panic!(
                "{} is INVALID but the reader accepted it: the form would let a \\
                 renamed TOML file through as a deed (#42)",
                path.display()
            ),
            Err(e) => format!("{e:#}"),
        };
        assert!(
            err.contains(want),
            "{stem} was rejected for the WRONG reason.\n  expected to contain: {want}\n               \
             actual: {err}\nA fixture that fails incidentally does not test the rule it names."
        );
    }
}

/// Both invalid fixtures exist and both are named for the form under test.
///
/// Vacuity guard: the loop above iterates a literal list, so a file deleted
/// from disk would leave it silently shorter. Asserting the directory's
/// contents keeps the pair a pair.
#[test]
fn the_invalid_pair_is_still_a_pair() {
    let dir = fixtures_dir().join("invalid");
    let found: Vec<String> = std::fs::read_dir(&dir)
        .expect("invalid/ exists")
        .map(|e| {
            e.expect("dir entry")
                .file_name()
                .to_string_lossy()
                .to_string()
        })
        .filter(|n| n.ends_with(".deed"))
        .collect();
    assert_eq!(
        found.len(),
        2,
        "the conformance pair must stay a pair; found {found:?}"
    );
    for name in &found {
        assert!(
            name.ends_with("_launcher_praxis.deed"),
            "{name} is not named for the ruled form, so it is not testing the ruling"
        );
    }
}
