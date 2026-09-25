// SPDX-License-Identifier: MPL-2.0
//! `mint → parse → realign → parse`, on both dialects (launch-scaffolder #40,
//! criterion 5).
//!
//! The deed leg is driven by **today's real emitter** — `template::render`,
//! which is the identical call `cmd_realign::realign_one` makes to produce
//! the file it writes (`cmd_realign.rs:156`). So this is the round trip the
//! criterion asks for, without going through the CLI: `realign` has no
//! rewrite path, it re-renders.
//!
//! ⭐ The direction of this file inverted at phase 2 and the inversion is the
//! point. In phase 1 `mint` emitted the legacy block and the deed sample was
//! generated from it; now `mint` emits the deed block and the **legacy**
//! sample is generated from *that*. Either way the generated side is built
//! from the values the real emitter just produced rather than from a string
//! typed here, so the compat guarantee is asserted against what `mint`
//! actually does today and not against whatever the author happened to type.

use launch_scaffolder_common::{
    config::LauncherConfig, metadata_block, standard::LauncherStandard, template,
};

/// The config both committed stapeln artefacts are minted from.
///
/// A committed file rather than a TOML literal here so the CI artefact gate
/// (`launcher-artefacts.yml`) mints from the SAME input these tests render —
/// one config, two consumers, no drift between them.
const CFG: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/config/stapeln.launcher.fixture.a2ml"
));

fn mint() -> String {
    let cfg = LauncherConfig::parse(CFG).expect("config parses");
    let std_ = LauncherStandard::baked().expect("baked standard loads");
    template::render(&cfg, &std_, None).expect("render succeeds")
}

/// The deed leg: mint, parse, realign, parse.
///
/// `realign` re-renders from the same config, so the second render must be
/// byte-identical to the first — that equality is what `Outcome::Unchanged`
/// is decided on (`cmd_realign.rs:163`). If it ever stops holding, realign
/// rewrites every launcher on every run, and this test says so.
#[test]
fn mint_parse_realign_parse_is_stable_for_the_deed_form() {
    let minted = mint();
    let first = metadata_block::parse_from_text(&minted)
        .expect("minted launcher parses")
        .expect("minted launcher carries a metadata block");
    assert!(
        first.is_deed(),
        "phase 2 mint must emit the DEED dialect, not the retired @a2ml-metadata form"
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

/// Phase 2 converts the emitter. Stated as a test rather than left to the
/// diff, so a revert has to delete this line deliberately — the same guard
/// the phase-1 version of this test provided, pointing the other way.
#[test]
fn phase_two_mint_emits_the_deed_markers_and_not_the_legacy_ones() {
    let minted = mint();
    assert!(minted.contains(metadata_block::DEED_BEGIN));
    assert!(!minted.contains(metadata_block::LEGACY_BEGIN));
}

/// Render the values of a parsed block as a legacy `@a2ml-metadata` block.
///
/// Built from the emitter's own output so it cannot drift from what `mint`
/// produces. Every scalar the emitter emits must land somewhere in the legacy
/// form — `panic` below is the guard that makes that true rather than
/// assumed, and it is what would catch a future field added to the deed
/// dialect that the compat reader could not express for older launchers.
fn as_legacy_script(block: &metadata_block::MetadataBlock) -> String {
    let get = |k: &str| block.scalar(k).unwrap_or_default().to_string();

    let standards = block
        .lists
        .iter()
        .find(|(k, _)| k == "standards-compliance")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let standards_rendered = standards
        .iter()
        .map(|s| format!("#     \"{s}\"\n"))
        .collect::<String>();

    // Fail loudly if the emitter grows a scalar the legacy dialect has no slot
    // for: silently dropping it is exactly the regression this whole change
    // exists to prevent.
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
                "the emitter emits scalar `{other}`, which the legacy dialect \
                 cannot express — the compat reader would drop it"
            ),
        }
    }

    format!(
        "#!/usr/bin/env bash\n\
         # @a2ml-metadata begin\n\
         # (\n\
         #   id                   = \"{id}\"\n\
         #   type                 = \"{ty}\"\n\
         #   version              = \"{version}\"\n\
         #   app-name             = \"{app_name}\"\n\
         #   app-display          = \"{app_display}\"\n\
         #   app-url              = \"{app_url}\"\n\
         #   runtime-kind         = \"{runtime_kind}\"\n\
         #   standards-compliance = [\n\
         {standards_rendered}\
         #   ]\n\
         #   standard-spec-version = \"{spec}\"\n\
         #   generator             = \"{generator}\"\n\
         # )\n\
         # @a2ml-metadata end\n\
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

/// The compat guarantee against the **live emitter**: a legacy-dialect
/// launcher carrying the values today's `mint` produces flattens to exactly
/// what the deed reader returns for that same mint.
///
/// This is the "on both forms" half of criterion 5, and it is the test that
/// proves already-minted launchers are not stranded: whatever `mint` emits
/// today, the retired dialect carrying the same values still reads back
/// identically. It is stronger than the in-module test of the same shape,
/// which compares against a committed fixture; this one moves if the emitter
/// moves.
#[test]
fn both_dialects_agree_on_what_todays_emitter_produces() {
    let minted = mint();
    let deed = metadata_block::parse_from_text(&minted)
        .expect("parses")
        .expect("has a block");

    let legacy_script = as_legacy_script(&deed);
    let legacy = metadata_block::parse_from_text(&legacy_script)
        .expect("the generated legacy script parses")
        .expect("the generated legacy script carries a block");

    assert!(
        deed.is_deed(),
        "today's mint must be read as the deed dialect"
    );
    assert!(
        !legacy.is_deed(),
        "the generated script must be read as the legacy dialect"
    );
    assert_eq!(
        legacy.scalars, deed.scalars,
        "scalars must not differ by dialect"
    );
    assert_eq!(legacy.lists, deed.lists, "lists must not differ by dialect");
    assert_eq!(deed.missing_required(), Vec::<String>::new());
}

/// The deed leg of `realign`: a deed block is never rewritten in place, so a
/// deed-carrying launcher is only ever regenerated.
///
/// Since phase 2 this runs against a **real minted launcher** rather than a
/// generated sample, which is what makes it the honest statement of the CLI's
/// new behaviour: `launch-scaffolder config set` now refuses on every newly
/// minted launcher, by design, and directs the caller to re-mint.
#[test]
fn realign_cannot_edit_a_minted_deed_launcher_in_place() {
    let minted = mint();

    let err = format!(
        "{:#}",
        metadata_block::rewrite_scalar(&minted, "app-display", "Renamed").unwrap_err()
    );
    assert!(
        err.contains("read-only"),
        "expected a refusal rather than a corrupted launcher, got: {err}"
    );
}

/// The committed pre-phase fixture is a real launcher minted before the
/// emitter changed, and it must keep reading — that is the whole compat
/// promise, asserted against an artefact rather than a reconstruction.
#[test]
fn a_launcher_minted_before_phase_two_still_reads() {
    let legacy = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/metadata_block/minted-2026-09-22_stapeln-launcher.sh"
    ))
    .expect("the pre-phase fixture is committed");

    let block = metadata_block::parse_from_text(&legacy)
        .expect("a pre-phase launcher still parses")
        .expect("a pre-phase launcher carries a metadata block");

    assert!(!block.is_deed(), "the fixture is the retired dialect");
    assert_eq!(block.missing_required(), Vec::<String>::new());

    // The same config this fixture was minted from, through today's emitter.
    let deed = metadata_block::parse_from_text(&mint())
        .expect("parses")
        .expect("has a block");
    assert_eq!(
        block.scalars, deed.scalars,
        "a pre-phase launcher and a launcher minted today must carry the same values"
    );
    assert_eq!(block.lists, deed.lists);
}

/// The post-phase fixture: a launcher minted **by this change**, committed as
/// an artefact.
///
/// It is the mirror of the pre-phase fixture above, and it guards the other
/// direction. That one proves a launcher minted before phase 2 still reads;
/// this one will prove a launcher minted *during* phase 2 still reads after
/// some later tightening of the DEED grammar. `deed::parse` is shared with the
/// rest of the estate and will keep moving; the launchers this emitter has
/// already written will not. Without a committed artefact there is nothing to
/// notice that they had been stranded.
#[test]
fn a_launcher_minted_by_phase_two_reads_and_matches_todays_emitter() {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/metadata_block/minted-2026-09-23_stapeln-launcher-deed.sh"
    ))
    .expect("the post-phase fixture is committed");

    let block = metadata_block::parse_from_text(&fixture)
        .expect("the post-phase fixture parses")
        .expect("the post-phase fixture carries a metadata block");

    assert!(block.is_deed(), "the fixture is the DEED dialect");
    assert_eq!(block.missing_required(), Vec::<String>::new());

    let minted = metadata_block::parse_from_text(&mint())
        .expect("parses")
        .expect("has a block");
    assert_eq!(
        block.scalars, minted.scalars,
        "the committed deed fixture and today's mint must agree on every scalar"
    );
    assert_eq!(block.lists, minted.lists);
}

/// ⭐ The committed DEED fixture is **currency-locked**: it must be byte-identical
/// to what `mint` emits today.
///
/// The two tests above compare the fixture's parsed metadata against a fresh
/// mint, which is the right check for the compat question and the wrong one for
/// the artefact question. Nothing pinned the fixture's *body*, so the shell the
/// scanners read (Hypatia's #48 findings, shellcheck's #49 findings) could sit
/// arbitrarily far from the shell the tool actually writes — a stale artefact
/// that keeps a closed defect looking open, or an open one looking closed.
///
/// Byte equality is also what makes the fixture safe to lint in CI: the gate
/// lints a launcher minted during the run, and this test is the proof that the
/// committed copy of it is the same file.
///
/// Re-mint it rather than editing it — see `fixtures/metadata_block/README.adoc`.
#[test]
fn the_committed_deed_fixture_is_what_mint_emits_today() {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/metadata_block/minted-2026-09-23_stapeln-launcher-deed.sh"
    ))
    .expect("the post-phase fixture is committed");

    let minted = mint();
    assert!(fixture == minted, "{}", first_difference(&fixture, &minted));
}

/// Name the lines where two texts diverge.
///
/// Without this a currency failure dumps two ~460-line scripts into the log and
/// leaves the reader to diff them by eye — which is how a stale fixture gets
/// "fixed" by regenerating it without anyone reading what moved.
fn first_difference(fixture: &str, minted: &str) -> String {
    let want: Vec<&str> = fixture.lines().collect();
    let got: Vec<&str> = minted.lines().collect();
    let mut out = format!(
        "the committed fixture ({} lines) is not what mint emits today ({} lines)\n",
        want.len(),
        got.len()
    );
    let mut shown = 0;
    for i in 0..want.len().max(got.len()) {
        let a = want.get(i).copied().unwrap_or("<no such line>");
        let b = got.get(i).copied().unwrap_or("<no such line>");
        if a != b {
            out.push_str(&format!(
                "  line {n}\n    fixture {a:?}\n    minted  {b:?}\n",
                n = i + 1
            ));
            shown += 1;
            if shown == 20 {
                out.push_str("  … (further differences omitted)\n");
                break;
            }
        }
    }
    out.push_str("re-mint the fixture; see fixtures/metadata_block/README.adoc");
    out
}
