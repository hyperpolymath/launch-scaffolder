// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! Tera template rendering for generated launcher scripts.
//!
//! The template itself (`templates/launcher.sh.tera`) is baked into the
//! binary at build time via `include_str!()`, mirroring how the standard
//! file is baked. The render path is: config + standard → template context
//! → Tera → shell script text.

use crate::Result;
use crate::config::{LauncherConfig, RuntimeKind};
use crate::standard::LauncherStandard;
use anyhow::Context;
use std::path::Path;
use tera::{Context as TeraContext, Tera};

/// The canonical template, baked into the binary.
pub const LAUNCHER_TEMPLATE: &str = include_str!("../../../templates/launcher.sh.tera");

/// Render a launcher shell script with an embedded DEED block from a parsed config.
///
/// The supplied standard provides the block's `:standard-version`.
/// `config_path` points to the `<app>.launcher.a2ml` that produced `config`.
/// It is canonicalised when possible and embedded as `CONFIG_FILE=...` so
/// the script's `--integ`/`--disinteg` arms can delegate to
/// `launch-scaffolder provision`. Passing `None` leaves that value empty.
/// Rendering fails if a value emitted into the DEED block contains a
/// control character with no legal DEED string spelling.
pub fn render(
    config: &LauncherConfig,
    _standard: &LauncherStandard,
    config_path: Option<&Path>,
) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("launcher.sh", LAUNCHER_TEMPLATE)
        .context("registering launcher template with Tera")?;
    // Registered here rather than pre-escaping the context so the escape
    // applies at exactly the emission sites — the embedded deed block —
    // and every other interpolation in the script keeps its raw value.
    tera.register_filter("deedstr", deedstr_filter);

    let mut ctx = TeraContext::new();

    // --- [project] -----------------------------------------------------
    ctx.insert("app_name", &config.project.name);
    ctx.insert("app_display", &config.project.display);
    ctx.insert(
        "app_desc",
        config
            .project
            .description
            .as_deref()
            .unwrap_or(&config.project.display),
    );
    ctx.insert(
        "generic_name",
        config
            .project
            .generic_name
            .as_deref()
            .unwrap_or(&config.project.display),
    );
    // Categories joined as freedesktop's semicolon-terminated list.
    let categories_joined = if config.project.categories.is_empty() {
        "Utility;".to_string()
    } else {
        let mut s = config.project.categories.join(";");
        s.push(';');
        s
    };
    ctx.insert("app_categories", &categories_joined);
    ctx.insert(
        "app_version",
        config.project.version.as_deref().unwrap_or("1.0.0"),
    );
    ctx.insert("app_license", config.project.license.as_deref().unwrap_or("MPL-2.0"));

    // --- [repo] --------------------------------------------------------
    ctx.insert("repo_dir", &config.repo.path);

    // --- [runtime] -----------------------------------------------------
    let kind_str = match config.runtime.kind {
        RuntimeKind::ServerUrl => "server-url",
        RuntimeKind::Process => "process",
        RuntimeKind::Remote => "remote",
    };
    ctx.insert("runtime_kind", kind_str);
    ctx.insert("has_url", &(config.runtime.url.is_some()));

    // Default URL/port fallbacks so Tera `{{ url }}` never explodes.
    let port = config.runtime.port.unwrap_or(0);
    ctx.insert("app_port", &port);
    let url_string = match (&config.runtime.url, config.runtime.port) {
        (Some(u), _) => u.clone(),
        (None, Some(p)) => format!("http://localhost:{p}"),
        (None, None) => String::new(),
    };
    ctx.insert("url", &url_string);

    // PID / log file defaults follow the standard's pattern when unset.
    let pid_file = config
        .runtime
        .pid_file
        .clone()
        .unwrap_or_else(|| format!("/tmp/{}-server.pid", config.project.name));
    let log_file = config
        .runtime
        .log_file
        .clone()
        .unwrap_or_else(|| format!("/tmp/{}-server.log", config.project.name));
    ctx.insert("pid_file", &pid_file);
    ctx.insert("log_file", &log_file);
    ctx.insert("wait_seconds", &config.runtime.wait_for_url_timeout_seconds);

    // Explicit command vector vs search list.
    ctx.insert("explicit_command", &config.runtime.command);
    let startup_search: Vec<String> = config
        .runtime
        .startup_command_search
        .iter()
        .map(|s| s.replace("{repo-dir}", &config.repo.path))
        .collect();
    ctx.insert("startup_search", &startup_search);

    // --- [icon] --------------------------------------------------------
    let icon_source = config
        .icon
        .as_ref()
        .map(|i| i.source.replace("{repo-dir}", &config.repo.path))
        .unwrap_or_default();
    ctx.insert("icon_source", &icon_source);

    // --- metadata -----------------------------------------------------
    ctx.insert("spec_version", &_standard.spec_version);

    // Absolute path back to the source config, so the generated
    // script's --integ / --disinteg arms can delegate to
    // `launch-scaffolder provision --integ "$CONFIG_FILE"`.
    let config_path_str = config_path
        .and_then(|p| p.canonicalize().ok().or_else(|| Some(p.to_path_buf())))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    ctx.insert("config_file", &config_path_str);

    tera.render("launcher.sh", &ctx)
        .context("rendering launcher template")
}

/// Escape a value for a DEED string literal.
///
/// The exact inverse of [`crate::deed`]'s `lex_string`, deliberately no
/// stricter: the grammar admits exactly four escapes — `\"`, `\\`, `\n`,
/// `\t` — and rejects every *other* raw control character below `U+0020`.
/// A carriage return therefore has no legal spelling in a deed string at
/// all, so this reports it rather than dropping it: refusing to mint is
/// better than minting a launcher whose own metadata cannot be read back.
///
/// The tab case is the one that reaches furthest. [`crate::deed::parse`]
/// rejects a literal HTAB *anywhere* in the document, tested on the raw
/// text before lexing, so a tab that survived into a value would not merely
/// corrupt one field — it would make the whole block unparseable.
fn deed_escape(s: &str) -> std::result::Result<String, String> {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                return Err(format!(
                    "U+{:04X} has no legal spelling in a DEED string; the grammar \
                     admits exactly four escapes (\\\" \\\\ \\n \\t)",
                    c as u32
                ));
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Tera filter wrapping [`deed_escape`], registered as `deedstr`.
///
/// Tera autoescaping is HTML-shaped and does not fire for a `.sh` template
/// in any case, so every `{{ }}` inside the embedded deed block is a hole
/// without this: a display name holding one `"` closes the string early and
/// the minted launcher's metadata block cannot be parsed at all. The legacy
/// `@a2ml-metadata` reader was tolerant enough to hide that; the DEED
/// grammar is not, which is what makes emission a correctness surface.
/// Returns an error for non-string values or control characters that cannot
/// be represented in a DEED string.
fn deedstr_filter(
    value: &tera::Value,
    _args: &std::collections::HashMap<String, tera::Value>,
) -> tera::Result<tera::Value> {
    let s = value
        .as_str()
        .ok_or_else(|| tera::Error::msg(format!("`deedstr` takes a string, got `{value}`")))?;
    deed_escape(s)
        .map(tera::Value::from)
        .map_err(tera::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LauncherConfig, Project, Repo, Runtime, RuntimeKind};
    use crate::standard::LauncherStandard;

    fn sample_config() -> LauncherConfig {
        LauncherConfig {
            project: Project {
                name: "foo".into(),
                display: "Foo".into(),
                description: Some("Foo desc".into()),
                categories: vec!["Development".into()],
                version: None,
                license: None,
                generic_name: Some("Foo Thing".into()),
            },
            repo: Repo {
                path: "/tmp/foo".into(),
            },
            runtime: Runtime {
                kind: RuntimeKind::Process,
                port: None,
                url: None,
                startup_command_search: vec![],
                command: vec!["foo".into()],
                pid_file: None,
                log_file: None,
                wait_for_url_timeout_seconds: 15,
            },
            icon: None,
            integration: None,
            soft_attach: None,
            exceptions: None,
        }
    }

    /// The exact config the committed pre-phase fixture was minted from.
    ///
    /// Kept beside [`stapeln_fixture`] because the pair is the whole point:
    /// the same inputs, rendered by today's emitter and by the one that ran
    /// on 2026-09-22, must carry the same metadata in different dialects.
    fn stapeln_config() -> LauncherConfig {
        let mut c = sample_config();
        c.project.name = "stapeln".into();
        c.project.display = "Stapeln".into();
        c.project.version = Some("0.1.0".into());
        c.runtime.kind = RuntimeKind::ServerUrl;
        c.runtime.port = Some(4010);
        c
    }

    /// The launcher minted before Phase 2, committed as a fixture.
    fn stapeln_fixture() -> String {
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/metadata_block/minted-2026-09-22_stapeln-launcher.sh"
        ))
        .expect("the pre-phase fixture is committed")
    }

    fn render_stapeln() -> String {
        let std_ = LauncherStandard::baked().expect("baked standard should parse");
        render(&stapeln_config(), &std_, None).expect("rendering the stapeln launcher")
    }

    /// A freshly minted launcher carries a block the Phase-1 reader accepts,
    /// and accepts *as a deed* rather than by falling back to the legacy arm.
    ///
    /// Asserting `is_deed()` is what separates this from "some block parsed":
    /// the reader still understands both dialects, so a template that had
    /// silently kept emitting the legacy form would pass a parse-only test.
    #[test]
    fn a_minted_launcher_carries_a_parseable_deed_block() {
        let script = render_stapeln();
        let block = crate::metadata_block::parse_from_text(&script)
            .expect("the minted block parses")
            .expect("a minted launcher has a metadata block");
        assert!(
            block.is_deed(),
            "mint must emit the DEED dialect, not the retired @a2ml-metadata form"
        );
        assert!(
            block.missing_required().is_empty(),
            "minted block is missing required keys: {:?}",
            block.missing_required()
        );
    }

    /// ⭐ The paired control: Phase 2 changes the DIALECT, not the DATA.
    ///
    /// Rendering the pre-phase fixture's own config through today's emitter
    /// and flattening both must yield byte-identical scalars and lists. That
    /// is the owner's actual requirement — already-minted launchers are not
    /// stranded and newly-minted ones claim exactly what they used to — and
    /// it is stronger than checking nine values by hand, because a key this
    /// test never thought to name still has to match.
    #[test]
    fn the_deed_emitter_agrees_with_the_legacy_fixture_on_every_value() {
        let legacy = crate::metadata_block::parse_from_text(&stapeln_fixture())
            .expect("the committed fixture parses")
            .expect("the fixture has a metadata block");
        let minted = crate::metadata_block::parse_from_text(&render_stapeln())
            .expect("the minted block parses")
            .expect("a minted launcher has a metadata block");

        assert!(
            !legacy.is_deed(),
            "the fixture is the pre-phase legacy form"
        );
        assert!(minted.is_deed(), "today's mint is the DEED form");

        assert_eq!(
            legacy.scalars, minted.scalars,
            "the deed emitter changed a scalar the legacy block carried"
        );
        assert_eq!(
            legacy.lists, minted.lists,
            "the deed emitter changed a list the legacy block carried"
        );
    }

    /// A `"` in a display name must not be able to mint an unparseable
    /// launcher.
    ///
    /// This is the hole Tera leaves open: autoescaping is HTML-shaped and
    /// does not fire for `.sh` at all, so before `deedstr` the quote closed
    /// the string early. The legacy reader was tolerant enough to survive
    /// it; `deed::parse` rejects the whole document, so under Phase 2 the
    /// same input is the difference between a working launcher and one
    /// whose metadata can never be read back.
    ///
    /// Asserting the value ROUND-TRIPS — not merely that parsing succeeded —
    /// is what makes this fail against an identity `deedstr`.
    #[test]
    fn a_quote_in_a_display_name_round_trips_through_the_deed_block() {
        let mut c = stapeln_config();
        c.project.display = "Sta\"pe\\ln".into();
        let std_ = LauncherStandard::baked().expect("baked standard should parse");
        let script = render(&c, &std_, None).expect("rendering with a quoted display");

        let block = crate::metadata_block::parse_from_text(&script)
            .expect("a quoted display name must still mint a parseable block")
            .expect("a minted launcher has a metadata block");
        assert_eq!(
            block.scalar("app-display"),
            Some("Sta\"pe\\ln"),
            "the display name must survive escaping unchanged"
        );
    }

    /// `deed_escape` is the exact inverse of the grammar's `lex_string`.
    #[test]
    fn deed_escape_covers_exactly_the_four_legal_escapes() {
        assert_eq!(deed_escape("plain").unwrap(), "plain");
        assert_eq!(deed_escape("a\"b").unwrap(), "a\\\"b");
        assert_eq!(deed_escape("a\\b").unwrap(), "a\\\\b");
        assert_eq!(deed_escape("a\nb").unwrap(), "a\\nb");
        assert_eq!(deed_escape("a\tb").unwrap(), "a\\tb");
        // Not stricter than the lexer: U+007F is not a control char below
        // 0x20, and `lex_string` admits it, so this must too.
        assert_eq!(deed_escape("a\u{7f}b").unwrap(), "a\u{7f}b");
    }

    /// A carriage return has no legal spelling in a deed string, so it is
    /// refused rather than dropped: minting a launcher whose own metadata
    /// cannot be parsed is worse than refusing to mint.
    #[test]
    fn deed_escape_refuses_a_control_character_with_no_escape() {
        let err = deed_escape("a\rb").expect_err("CR has no legal deed escape");
        assert!(
            err.contains("U+000D"),
            "the error must name the character: {err}"
        );
    }

    /// An escaped tab must leave as the two-character `\t`, never a literal.
    ///
    /// `deed::parse` rejects a literal HTAB *anywhere* in the document,
    /// tested on the raw text before lexing — so a tab surviving into a
    /// value would not corrupt one field, it would make the entire block
    /// unparseable.
    #[test]
    fn a_tab_in_a_value_does_not_kill_the_whole_block() {
        let mut c = stapeln_config();
        c.project.display = "Sta\tpeln".into();
        let std_ = LauncherStandard::baked().expect("baked standard should parse");
        let script = render(&c, &std_, None).expect("rendering with a tabbed display");

        let block = crate::metadata_block::parse_from_text(&script)
            .expect("a tab must not make the block unparseable")
            .expect("a minted launcher has a metadata block");
        assert_eq!(block.scalar("app-display"), Some("Sta\tpeln"));
    }

    /// No `cmd && log ... || log ...` ternaries in the template.
    ///
    /// In that form a FAILING command on the success branch also fires the
    /// failure branch, so a run that actually worked reports both "generated"
    /// and "generation failed". Codacy flagged a real instance of this at
    /// launcher.sh.tera:410; it was spelled across three continued lines, so a
    /// single-line grep missed it — hence a test that joins continuations.
    ///
    /// `A && { B || C; }` is NOT this bug: the `||` is inside a braced group,
    /// making it a compound condition rather than a ternary.
    #[test]
    fn template_has_no_command_log_ternaries() {
        // Join backslash continuations so multi-line ternaries are visible.
        let joined = LAUNCHER_TEMPLATE.replace("\\\n", " ");
        for (i, line) in joined.lines().enumerate() {
            let l = line.trim();
            if l.starts_with('#') {
                continue; // comments may describe the pattern
            }
            if let Some(amp) = l.find("&& log") {
                if let Some(pipe) = l[amp..].find("|| log") {
                    // a braced group between them is the safe compound form
                    let between = &l[amp..amp + pipe];
                    assert!(
                        between.contains('{'),
                        "line {} is a `cmd && log || log` ternary; use if/else so a \
                         failing log on the success branch cannot fire the failure \
                         branch: {}",
                        i + 1,
                        l
                    );
                }
            }
        }
    }

    /// The template source must OPEN with the shebang.
    ///
    /// It previously opened with a Tera comment block, so every launcher the
    /// generator emitted began with a blank line: shellcheck SC2148, and a
    /// script the kernel will not dispatch by shebang. That single template
    /// defect produced ~20 identical one-line fix PRs across the estate, each
    /// of which the next `launch-scaffolder realign` would have overwritten.
    #[test]
    fn template_source_opens_with_shebang() {
        assert_eq!(
            LAUNCHER_TEMPLATE.lines().next(),
            Some("#!/usr/bin/env bash"),
            "the shebang must be the literal first line of launcher.sh.tera"
        );
    }

    /// And the RENDERED output must too — the property that actually matters.
    /// Asserting only on the template source would miss Tera emitting leading
    /// whitespace of its own, which is precisely how the original defect
    /// escaped notice.
    #[test]
    fn rendered_launcher_starts_with_shebang_on_line_one() {
        let cfg = sample_config();
        let std_ = LauncherStandard::baked().expect("baked standard should parse");
        let out = render(&cfg, &std_, None).expect("template should render");

        assert!(
            out.starts_with("#!/usr/bin/env bash\n"),
            "rendered launcher must begin with the shebang; got: {:?}",
            &out[..out.len().min(60)]
        );
        assert_eq!(
            out.lines().next(),
            Some("#!/usr/bin/env bash"),
            "shebang must be on line 1 of the rendered launcher"
        );
    }
}
