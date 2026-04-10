// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

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
use tera::{Context as TeraContext, Tera};

/// The canonical template, baked into the binary.
pub const LAUNCHER_TEMPLATE: &str = include_str!("../../../templates/launcher.sh.tera");

/// Render a launcher shell script from a parsed config.
///
/// The standard is passed for forward-compatibility: once the template
/// needs fields from the standard at render time (rather than at runtime
/// inside the generated script), they'll flow through here. For now the
/// template is self-contained and only reads `config`.
pub fn render(config: &LauncherConfig, _standard: &LauncherStandard) -> Result<String> {
    let mut tera = Tera::default();
    tera.add_raw_template("launcher.sh", LAUNCHER_TEMPLATE)
        .context("registering launcher template with Tera")?;

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
        config
            .project
            .license
            .as_deref()
            .unwrap_or("PMPL-1.0-or-later"),
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

    tera.render("launcher.sh", &ctx)
        .context("rendering launcher template")
}
