// SPDX-License-Identifier: MPL-2.0
//! `mint → parse → realign → parse`, on both dialects (launch-scaffolder #40,
//! criterion 5).
//!
//! The legacy leg is driven by **today's real emitter** — `template::render`,
//! which is the identical call `cmd_realign::realign_one` makes to produce
//! the file it writes (`cmd_realign.rs:156`). So this is the round trip the
//! criterion asks for, without going through the CLI: `realign` has no
//! rewrite path in phase 1, it re-renders.
//!
//! The deed leg is built from the values the real emitter just produced,
//! rather than from a string typed here. That is deliberate: a hand-written
//! deed sample asserts the deed reader against whatever the author happened
//! to type, whereas this asserts it against what `mint` actually emits today.

use launch_scaffolder_common::{
    config::LauncherConfig, metadata_block, standard::LauncherStandard, template,
};

const CFG: &str = r#"
[project]
name = "stapeln"
display = "Stapeln"
version = "0.1.0"

[repo]
path = "/srv/stapeln"

[runtime]
kind = "server-url"
url = "http://localhost:4010"
"#;

fn mint() -> String {
    let cfg = LauncherConfig::parse(CFG).expect("config parses");
    let std_ = LauncherStandard::baked().expect("baked standard loads");
    template::render(&cfg, &std_, None).expect("render succeeds")
}

/// The legacy leg: mint, parse, realign, parse.
///
/// `realign` re-renders from the same config, so the second render must be
/// byte-identical to the first — that equality is what `Outcome::Unchanged`
/// is decided on (`cmd_realign.rs:163`). If it ever stops holding, realign
/// rewrites every launcher on every run, and this test says so.
#[test]
fn mint_parse_realign_parse_is_stable_for_the_legacy_form() {
    let minted = mint();
    let first = metadata_block::parse_from_text(&minted)
        .expect("minted launcher parses")
        .expect("minted launcher carries a metadata block");
    assert!(
        !first.is_deed(),
        "phase 1 mint must still emit the legacy dialect"
    );
    assert_eq!(first.missing_required(), Vec::<String>::new());

    let realigned = mint();
    assert_eq!(
        minted, realigned,
        "realign re-renders from the template; a fresh mint must be unchanged"
    );

    let second = metadata_block::parse_from_text(&realigned)
        .expect("realigned launcher parses")
        .expect("realigned launcher carries a metadata block");
    assert_eq!(first.scalars, second.scalars);
    assert_eq!(first.lists, second.lists);
}

/// Phase 1 does not touch the emitter. Stated as a test rather than left to
/// the diff, so phase 2 has to delete this line deliberately.
#[test]
fn phase_one_mint_emits_the_legacy_markers_and_not_the_deed_ones() {
    let minted = mint();
    assert!(minted.contains(metadata_block::LEGACY_BEGIN));
    assert!(!minted.contains(metadata_block::DEED_BEGIN));
}

/// Render the values of a parsed block as a `@launcher-deed` block.
///
/// Built from the emitter's own output so it cannot drift from what `mint`
/// produces. Every scalar the emitter emits must land somewhere in the deed
/// form — `unreachable` below is the guard that makes that true rather than
/// assumed.
fn as_deed_script(block: &metadata_block::MetadataBlock) -> String {
    let get = |k: &str| block.scalar(k).unwrap_or_default().to_string();

    let standards = block
        .lists
        .iter()
        .find(|(k, _)| k == "standards-compliance")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let standards_rendered = standards
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(" ");

    // Fail loudly if the emitter grows a scalar the deed dialect has no slot
    // for: silently dropping it is exactly the phase-2 regression this whole
    // change exists to prevent.
    for (k, _) in &block.scalars {
        match k.as_str() {
            "id"
            | "type"
            | "version"
            | "app-name"
            | "app-display"
            | "app-url"
            | "runtime-kind"
            | "standard-spec-version"
            | "generator" => {}
            other => panic!(
                "the emitter emits scalar `{other}`, which the deed dialect \
                 cannot express — extend `flatten_praxis_deed` before phase 2"
            ),
        }
    }

    format!(
        "#!/usr/bin/env bash\n\
         # @launcher-deed begin\n\
         # ;; SPDX-License-Identifier: MPL-2.0\n\
         # (praxis-deed\n\
         #   :schema-version  \"1.0.0\"\n\
         #   :canonical-name  \"{id}\"\n\
         #   :beholding-chora #u5\"estate/chora\"\n\
         #   (artefact :type \"{ty}\" :version \"{version}\"\n\
         #             :generator \"{generator}\")\n\
         #   (app :name \"{app_name}\" :display \"{app_display}\"\n\
         #        :url \"{app_url}\" :runtime-kind \"{runtime_kind}\")\n\
         #   (compliance :standard-version \"{spec}\"\n\
         #               :standards ({standards_rendered})))\n\
         # @launcher-deed end\n\
         \n\
         echo hi\n",
        id = get("id"),
        ty = get("type"),
        version = get("version"),
        generator = get("generator"),
        app_name = get("app-name"),
        app_display = get("app-display"),
        app_url = get("app-url"),
        runtime_kind = get("runtime-kind"),
        spec = get("standard-spec-version"),
    )
}

/// The compat guarantee against the **live emitter**: a deed-dialect launcher
/// carrying the values today's `mint` produces flattens to exactly what the
/// legacy reader returns for that same mint.
///
/// This is the "on both forms" half of criterion 5. It is stronger than the
/// in-module test of the same shape, which compares against a committed
/// fixture; this one moves if the emitter moves.
#[test]
fn both_dialects_agree_on_what_todays_emitter_produces() {
    let minted = mint();
    let legacy = metadata_block::parse_from_text(&minted)
        .expect("parses")
        .expect("has a block");

    let deed_script = as_deed_script(&legacy);
    let deed = metadata_block::parse_from_text(&deed_script)
        .expect("the generated deed script parses")
        .expect("the generated deed script carries a block");

    assert!(
        deed.is_deed(),
        "the generated script must be read as the deed dialect"
    );
    assert_eq!(
        legacy.scalars, deed.scalars,
        "scalars must not differ by dialect"
    );
    assert_eq!(legacy.lists, deed.lists, "lists must not differ by dialect");
    assert_eq!(deed.missing_required(), Vec::<String>::new());
}

/// The deed leg of `realign`: phase 1 refuses to rewrite a deed block in
/// place, so a deed-carrying launcher is only ever regenerated, never edited.
#[test]
fn realign_cannot_edit_a_deed_launcher_in_place() {
    let minted = mint();
    let legacy = metadata_block::parse_from_text(&minted).unwrap().unwrap();
    let deed_script = as_deed_script(&legacy);

    let err = format!(
        "{:#}",
        metadata_block::rewrite_scalar(&deed_script, "app-display", "Renamed").unwrap_err()
    );
    assert!(
        err.contains("read-only"),
        "expected a refusal rather than a corrupted launcher, got: {err}"
    );
}
