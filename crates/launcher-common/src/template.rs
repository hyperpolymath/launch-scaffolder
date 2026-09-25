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
/// The mode flags the generated script's main switch implements.
///
/// This is the launcher's own mode surface, so it lives next to the template
/// rather than in the standard's `(required-modes)` clause: that clause says
/// what a compliant launcher MUST accept, and this says what this one DOES
/// accept. They are not the same, and conflating them would let the block
/// claim a mode the script does not implement (the standard also requires
/// `--version`, which this script does not yet have — a live finding, not
/// something to paper over by copying the standard's list).
///
/// [`tests::the_declared_modes_are_the_arms_of_the_main_switch`] asserts this
/// list and the template's `case "$MODE"` arms agree in both directions, so
/// the claim cannot drift from the script.
pub const LAUNCHER_MODES: &[&str] = &[
    "--start",
    "--stop",
    "--status",
    "--browser",
    "--web",
    "--auto",
    "--integ",
    "--disinteg",
    "--help",
];

/// Render a list of strings as the inside of a DEED list: `"a" "b" "c"`.
///
/// Each item goes through [`deed_escape`], the same filter the template
/// applies to every other value emitted into the block. Values that arrive
/// from the standard are not necessarily inert — a platform name holding a
/// `"` would close the string early and make the whole block unparseable.
fn deed_list(values: &[String]) -> Result<String> {
    let mut out = Vec::with_capacity(values.len());
    for v in values {
        out.push(format!("\"{}\"", deed_escape(v).map_err(tera::Error::msg)?));
    }
    Ok(out.join(" "))
}

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
    ctx.insert(
        "app_license",
        config.project.license.as_deref().unwrap_or("MPL-2.0"),
    );

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
    //
    // The port gets ONE answer in the generated script. With no explicit
    // `[runtime].url` the template emits `APP_PORT` and composes the URL from
    // it; with one, it emits the URL and no `APP_PORT` at all, because a second
    // spelling of the port could only disagree with the URL the launcher
    // actually dials (#49 AC3 — shellcheck saw the unused variable, the defect
    // was the duplicate).
    let port = config.runtime.port.unwrap_or(0);
    ctx.insert("app_port", &port);
    let url_string = match (&config.runtime.url, config.runtime.port) {
        (Some(u), _) => u.clone(),
        (None, Some(p)) => format!("http://localhost:{p}"),
        (None, None) => String::new(),
    };
    // A config that sets both and has them disagree is almost certainly a
    // mistake, and the generated script no longer carries the second value that
    // would once have shown it — so say so at mint time instead.
    let disagreement = config
        .runtime
        .url
        .as_deref()
        .zip(config.runtime.port)
        .filter(|(url, p)| !url.contains(&format!(":{p}")));
    if let Some((url, p)) = disagreement {
        tracing::warn!(
            "[runtime].url = {url} does not carry [runtime].port = {p}; the URL wins, so \
             the port is not emitted into the launcher separately"
        );
    }
    ctx.insert("url", &url_string);

    // PID / log file defaults, per-user rather than in a world-writable
    // directory.
    //
    // The previous defaults were `/tmp/<name>-server.pid` and
    // `/tmp/<name>-server.log`: world-writable AND predicted entirely by the
    // project name, so any local user could create or symlink the path before
    // the launcher's first run and steer the `kill` / `rm` the script later
    // performs on it (`is_running`, `clear_stale_pid`, `stop_server`). That is
    // Hypatia alerts 82 and 83 (#48).
    //
    // Both defaults are therefore SHELL expressions, not paths resolved here:
    //
    //   * `$XDG_RUNTIME_DIR` for the pid, because the pid is per-session state
    //     and the runtime directory is already per-user and 0700. It falls
    //     back to `$XDG_STATE_HOME` and then to `~/.local/state`, per the XDG
    //     base directory spec, so the launcher still works on a host with no
    //     runtime directory (cron, containers, a bare tty).
    //   * `$XDG_STATE_HOME` for the log, because a log must survive a logout —
    //     which is precisely what `$XDG_RUNTIME_DIR` does not promise.
    //     `mktemp` is deliberately not used for either: an unpredictable name
    //     is unusable for a pid file that another invocation has to find.
    //
    // Resolving them at mint time instead would bake one machine's paths into
    // a script that may run on another, so the expansion is left to the shell
    // and the directory is created by the script before first write.
    let pid_file = config.runtime.pid_file.clone().unwrap_or_else(|| {
        format!(
            "${{XDG_RUNTIME_DIR:-${{XDG_STATE_HOME:-$HOME/.local/state}}}}/{}-server.pid",
            config.project.name
        )
    });
    let log_file = config.runtime.log_file.clone().unwrap_or_else(|| {
        format!(
            "${{XDG_STATE_HOME:-$HOME/.local/state}}/{}-server.log",
            config.project.name
        )
    });
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

    // The four declarations the standard's `(metadata-block
    // :required-fields)` has always demanded and `mint` never emitted (#41).
    // The platforms and the lifecycle phases are the standard's own
    // vocabulary, read from its clauses rather than restated here; the modes
    // are this template's, from [`LAUNCHER_MODES`].
    //
    // Anything that cannot be read out of the standard is an error rather
    // than an empty list: a block that declares `()` for `platforms` would
    // satisfy a presence check while claiming nothing.
    ctx.insert(
        "modes",
        &deed_list(
            &LAUNCHER_MODES
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>(),
        )?,
    );
    ctx.insert(
        "platforms",
        &deed_list(&_standard.platforms().context(
            "the standard carries no (platforms) clause, so the launcher cannot \
             declare the `platforms` field its metadata block requires",
        )?)?,
    );
    ctx.insert(
        "lifecycle_phases_covered",
        &deed_list(&_standard.lifecycle_phases_covered().context(
            "the standard carries no (lifecycle-phases :covered …), so the launcher \
             cannot declare the `lifecycle-phases-covered` field its block requires",
        )?)?,
    );
    ctx.insert(
        "lifecycle_phases_deferred",
        &deed_list(&_standard.lifecycle_phases_deferred().context(
            "the standard carries no (lifecycle-phases :deferred …), so the launcher \
             cannot declare the `lifecycle-phases-deferred` field its block requires",
        )?)?,
    );

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

        // The lists need stating rather than comparing wholesale, because
        // #41 taught the emitter four declarations the 2026-09-22 emitter did
        // not make. Comparing `legacy.lists == minted.lists` would either fail
        // (hiding a real drift behind a known one) or, if "fixed" by trimming
        // the new entries, stop noticing a change to `standards-compliance`.
        //
        // So: every list the pre-phase launcher carries must be carried
        // identically, no scalar may appear or vanish, and the ONLY addition
        // may be the four declarations the standard has always required.
        for (key, values) in &legacy.lists {
            assert_eq!(
                minted.list(key),
                Some(values.as_slice()),
                "the deed emitter changed list `{key}`"
            );
        }
        let added: Vec<&String> = minted
            .lists
            .iter()
            .filter(|(k, _)| legacy.list(k).is_none())
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            added,
            vec![
                "modes",
                "platforms",
                "lifecycle-phases-covered",
                "lifecycle-phases-deferred"
            ],
            "the only lists today's mint may add to a pre-phase launcher are the four \
             declarations #41 taught it to emit"
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

    /// The port has exactly one spelling in the generated script (#49 AC3).
    ///
    /// `APP_PORT` used to be emitted into every `server-url` launcher and read
    /// by none of them — shellcheck SC2034 — while the `URL` line above it
    /// hardcoded the same port. The unused variable was the symptom; two answers
    /// to one question was the defect, and the two could silently disagree.
    #[test]
    fn the_port_has_exactly_one_spelling_in_the_generated_script() {
        let std_ = LauncherStandard::baked().expect("baked standard should parse");

        // No explicit URL, so APP_PORT is emitted — because something reads it:
        // the very next line composes the URL out of it.
        let composed = stapeln_config();
        assert_eq!(composed.runtime.url, None, "fixture config sets no url");
        let script = render(&composed, &std_, None).expect("renders");
        assert!(
            script.contains("APP_PORT=\"4010\""),
            "the composed arm must state the port it composes from"
        );
        assert!(
            script.contains("URL=\"http://localhost:${APP_PORT}\""),
            "and must compose the URL from it, so the two cannot disagree"
        );

        // An explicit URL is the whole answer: the port is inside it, and a
        // second `APP_PORT=` beside it could only contradict it.
        let mut explicit = stapeln_config();
        explicit.runtime.url = Some("http://localhost:4010".into());
        let script = render(&explicit, &std_, None).expect("renders");
        assert!(
            !script.contains("APP_PORT="),
            "a launcher with an explicit [runtime].url must not also state the port"
        );
        assert!(script.contains("URL=\"http://localhost:4010\""));
    }

    // ---------------------------------------------------------------
    // #48 — the default pid/log location is not a predictable path in a
    // world-writable directory.
    // ---------------------------------------------------------------

    /// The DEFAULT pid/log paths, spelled out as literals.
    ///
    /// Written as literal strings on purpose. #48 AC4 forbids asserting them
    /// by recomputing the same `format!` the renderer uses: an equality whose
    /// right-hand side is derived from the left cannot fail when both move
    /// together, so such a test would have passed happily while the default sat
    /// in `/tmp`. The fixture config sets neither value (see
    /// `stapeln.launcher.fixture.a2ml`), so these are defaults, not overrides.
    ///
    /// They are shell expressions rather than paths: the launcher runs on the
    /// user's machine, which is not necessarily the machine it was minted on.
    const DEFAULT_PID_LINE: &str =
        "PID_FILE=\"${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}}/stapeln-server.pid\"";
    const DEFAULT_LOG_LINE: &str =
        "LOG_FILE=\"${XDG_STATE_HOME:-$HOME/.local/state}/stapeln-server.log\"";

    /// A config that sets neither `pid-file` nor `log-file` mints a launcher
    /// whose state lands under a per-user directory — never in `/tmp`.
    #[test]
    fn default_pid_and_log_paths_are_per_user_not_world_writable() {
        let cfg = stapeln_config();
        assert!(
            cfg.runtime.pid_file.is_none() && cfg.runtime.log_file.is_none(),
            "the fixture config must set neither, or this asserts nothing"
        );
        let script = render_stapeln();

        assert!(
            script.contains(DEFAULT_PID_LINE),
            "the default pid line changed; it must stay out of world-writable space. \\
             Looking for: {DEFAULT_PID_LINE}"
        );
        assert!(
            script.contains(DEFAULT_LOG_LINE),
            "the default log line changed; it must stay out of world-writable space. \\
             Looking for: {DEFAULT_LOG_LINE}"
        );

        // The finding, stated directly: no `/tmp` path is emitted by default.
        for line in script.lines() {
            if line.starts_with("PID_FILE=") || line.starts_with("LOG_FILE=") {
                assert!(
                    !line.contains("/tmp/"),
                    "`{line}` puts launcher state in world-writable /tmp with a name \\
                     predictable from the project (#48)"
                );
            }
        }
    }

    /// An explicit `pid-file` / `log-file` in the config still wins, unchanged
    /// (#48 AC2).
    ///
    /// Including the `~/` spelling, which is expanded by `integration.rs`
    /// rather than by the renderer.
    #[test]
    fn explicit_pid_and_log_paths_still_win_unchanged() {
        let std_ = LauncherStandard::baked().expect("baked standard should parse");
        let mut cfg = stapeln_config();
        cfg.runtime.pid_file = Some("/var/run/stapeln.pid".into());
        cfg.runtime.log_file = Some("~/logs/stapeln.log".into());

        let script = render(&cfg, &std_, None).expect("renders");

        assert!(
            script.contains("PID_FILE=\"/var/run/stapeln.pid\""),
            "an explicit pid-file must be emitted verbatim"
        );
        assert!(
            script.contains("LOG_FILE=\"~/logs/stapeln.log\""),
            "an explicit log-file must be emitted verbatim, `~` included"
        );
        assert!(
            !script.contains("XDG_RUNTIME_DIR") && !script.contains("XDG_STATE_HOME"),
            "the XDG defaults must not appear when the config states both paths"
        );
        // The directory-creation helper is unconditional: it also has to work
        // for an explicit path the user has not created yet.
        assert!(
            script.contains("ensure_state_dirs"),
            "state directories must be created whatever path the config chose"
        );
    }

    /// The generated launcher creates its state directories 0700 before writing
    /// (#48 AC3).
    ///
    /// Two assertions, because either alone is vacuous: `mkdir -p` without a
    /// mode creates the directory with the caller's umask (commonly 0755), and
    /// `mkdir -p -m` applies the mode only to the deepest directory it creates
    /// — so the mode is stated separately, and the test looks for both halves.
    #[test]
    fn the_launcher_creates_its_state_directories_0700_before_writing() {
        let script = render_stapeln();

        assert!(
            script.contains("chmod 0700 \"$pid_dir\" \"$log_dir\""),
            "the launcher must set 0700 on the directories it is about to write into"
        );
        assert!(
            script.contains("mkdir -p \"$pid_dir\" \"$log_dir\""),
            "the launcher must create the directories it is about to write into"
        );
    }

    /// `ensure_state_dirs` runs BEFORE the first write, not after.
    ///
    /// The previous test pins what the helper does; this one pins the ordering
    /// property that makes it a fix rather than a decoration: a directory
    /// created after the pid file is written is no protection at all.
    #[test]
    fn state_dirs_are_ensured_before_the_first_pid_write() {
        let script = render_stapeln();
        let start = script
            .find("start_server()")
            .expect("start_server is defined in the template");
        let body = &script[start..];
        let ensure = body
            .find("ensure_state_dirs")
            .expect("start_server must ensure its state dirs before writing");
        let write = body
            .find(">\"$LOG_FILE\"")
            .expect("start_server writes the log");
        assert!(
            ensure < write,
            "`ensure_state_dirs` must run before the launcher writes $LOG_FILE"
        );
    }

    /// Every mode flag the template's main switch handles.
    ///
    /// Read out of [`LAUNCHER_TEMPLATE`] rather than restated, so the block's
    /// `modes` declaration is pinned to the script's actual behaviour instead
    /// of to a list that happens to match today.
    fn main_switch_arms() -> Vec<String> {
        let mut arms = Vec::new();
        let mut in_switch = false;
        for line in LAUNCHER_TEMPLATE.lines() {
            let t = line.trim();
            if t.starts_with("case \"$MODE\" in") {
                in_switch = true;
                continue;
            }
            if in_switch && t == "esac" {
                break;
            }
            if !in_switch {
                continue;
            }
            // An arm is a pattern list followed by `)`. Bodies are indented
            // commands, tera tags, and `;;` terminators — none of which start
            // with `--` or `*`.
            if !(t.starts_with("--") || t.starts_with('*')) {
                continue;
            }
            let Some(patterns) = t.split(')').next() else {
                continue;
            };
            for p in patterns.split('|') {
                arms.push(p.trim().to_string());
            }
        }
        arms
    }

    /// The `modes` the block declares are the modes the script implements, in
    /// both directions (#41).
    ///
    /// A one-directional check would be satisfiable by declaring modes the
    /// script does not have (the block claims a surface it does not offer) or
    /// by implementing modes it does not declare (the standard's required
    /// field under-reports). Both are checked, and the wildcard `*)` and the
    /// `-h` alias are excluded by name rather than silently — a new arm that
    /// is neither must be declared here or the test fails.
    ///
    /// Vacuity guard: the arms are read from the template, so if the main
    /// switch were ever renamed or removed the extraction returns nothing and
    /// the test fails instead of passing on an empty list.
    #[test]
    fn the_declared_modes_are_the_arms_of_the_main_switch() {
        let arms = main_switch_arms();
        assert!(
            arms.len() >= LAUNCHER_MODES.len(),
            "vacuity: the template's main switch was not found; found {arms:?}"
        );

        for mode in LAUNCHER_MODES {
            assert!(
                arms.iter().any(|a| a == mode),
                "`mint` declares `{mode}`, but the template's main switch has no such \
                 arm — the block would claim a mode the script does not implement"
            );
        }

        let undeclared: Vec<&String> = arms
            .iter()
            .filter(|a| a.as_str() != "*" && a.as_str() != "-h")
            .filter(|a| !LAUNCHER_MODES.contains(&a.as_str()))
            .collect();
        assert_eq!(
            undeclared,
            Vec::<&String>::new(),
            "the main switch handles arms the block does not declare; add them to \
             LAUNCHER_MODES so the launcher's declared surface is complete"
        );
    }

    /// The emitted block declares the four fields the standard has always
    /// required and no launcher carried until #41.
    ///
    /// Values are checked here as well as presence: a `platforms` field is a
    /// claim about the world, and an empty or invented list would satisfy a
    /// presence check while saying nothing.
    #[test]
    fn a_minted_launcher_declares_the_four_fields_it_never_used_to_carry() {
        let block = crate::metadata_block::parse_from_text(&render_stapeln())
            .expect("parses")
            .expect("has a block");

        let declared_modes: Vec<String> = LAUNCHER_MODES.iter().map(|m| m.to_string()).collect();
        assert_eq!(
            block.list("modes"),
            Some(declared_modes.as_slice()),
            "the declared modes must be exactly the modes the script implements"
        );

        let std_ = LauncherStandard::baked().expect("baked standard loads");
        assert_eq!(
            block.list("platforms"),
            Some(
                std_.platforms()
                    .expect("the standard declares platforms")
                    .as_slice()
            )
        );
        assert_eq!(
            block.list("lifecycle-phases-covered"),
            Some(
                std_.lifecycle_phases_covered()
                    .expect("the standard declares the covered phases")
                    .as_slice()
            )
        );
        assert_eq!(
            block.list("lifecycle-phases-deferred"),
            Some(
                std_.lifecycle_phases_deferred()
                    .expect("the standard declares the deferred phases")
                    .as_slice()
            )
        );

        for key in [
            "modes",
            "platforms",
            "lifecycle-phases-covered",
            "lifecycle-phases-deferred",
        ] {
            assert!(
                !block.list(key).unwrap().is_empty(),
                "`{key}` is declared empty, which is no declaration at all"
            );
        }
        assert_eq!(block.missing_required(), Vec::<&'static str>::new());
    }
}
