// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! Per-app `<app>.launcher.a2ml` config parser.
//!
//! A2ML currently parses as TOML — the `a2ml-rs` crate is not yet at feature
//! parity, so we lean on `toml` and keep the surface conservative. Everything
//! optional defaults to `None` so partial configs round-trip cleanly through
//! `config get / set`.

use crate::Result;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level per-app launcher config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    pub project: Project,
    pub repo: Repo,
    pub runtime: Runtime,
    #[serde(default)]
    pub icon: Option<Icon>,
    #[serde(default)]
    pub integration: Option<toml::Value>,
    #[serde(default, rename = "soft-attach")]
    pub soft_attach: Option<SoftAttach>,
    #[serde(default)]
    pub exceptions: Option<toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Project {
    pub name: String,
    pub display: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Optional freedesktop `GenericName=` field.
    #[serde(default)]
    pub generic_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repo {
    pub path: String,
}

/// Runtime shape selector. Three worlds: local server with a URL, plain
/// process launch, or remote web app.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    /// Local server: `--auto` starts the server and opens a browser at `url`.
    #[default]
    ServerUrl,
    /// Background process: no URL, no browser, just start/stop/status.
    Process,
    /// Remote web app: no local server, just opens a browser at a remote URL.
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Runtime {
    /// Optional — defaults to `server-url` to match the majority of configs.
    #[serde(default)]
    pub kind: RuntimeKind,

    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub url: Option<String>,

    /// Ordered list of startup commands to try. First executable one wins.
    /// Entries may reference `{repo-dir}` which the launcher expands at
    /// runtime to the value of `[repo].path`.
    #[serde(default)]
    pub startup_command_search: Vec<String>,

    /// Alternative: explicit argv vector. If set, `startup_command_search`
    /// is ignored. Useful for process-kind launchers (e.g. `nqc --gui`).
    #[serde(default)]
    pub command: Vec<String>,

    /// Where the generated launcher writes its pid file.
    ///
    /// Default (when unset): `+$XDG_RUNTIME_DIR+`, falling back to
    /// `+$XDG_STATE_HOME+` and then to `+~/.local/state+`, as
    /// `+launch-scaffolder/<app>/server.pid+`. Before 2026-09-25 the default was
    /// `+/tmp/<app>-server.pid+` — world-writable and predicted entirely by
    /// the app name, so any local user could create or symlink the path
    /// before the launcher's first run and steer what it later killed or
    /// removed (#48). The default is emitted into the script as a SHELL
    /// expression under a unique per-app directory. Explicit paths are shell-
    /// quoted and their parent directory must be user-owned and not group/world
    /// writable; the launcher never changes permissions on an explicit parent.
    ///
    /// Set it to override, e.g. `+pid-file = "~/run/myapp.pid"+`. A leading
    /// `+~/+` is expanded at launcher runtime. Shared writable locations such
    /// as `/tmp` are refused by the generated script.
    #[serde(default)]
    pub pid_file: Option<String>,
    /// Where the generated launcher writes its log.
    ///
    /// Default (when unset): `+$XDG_STATE_HOME+`, falling back to
    /// `+~/.local/state+`, as `+<app>-server.log+`. The state directory
    /// rather than the runtime directory because a log has to survive a
    /// logout, which `+$XDG_RUNTIME_DIR+` does not promise. Same history and
    /// the same override mechanism as [`Runtime::pid_file`].
    #[serde(default)]
    pub log_file: Option<String>,

    #[serde(default = "default_wait_seconds")]
    pub wait_for_url_timeout_seconds: u32,
}

fn default_wait_seconds() -> u32 {
    15
}

/// Names become filenames, desktop IDs, and shell-visible identifiers. Keep
/// them a single safe path component and prevent control characters or option
/// injection into generated launcher paths.
fn validate_project_name(name: &str) -> Result<()> {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        anyhow::bail!("project.name must not be empty");
    };
    if name.len() > 80 {
        anyhow::bail!("project.name must be at most 80 ASCII bytes");
    }
    if !first.is_ascii_alphanumeric()
        || !bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        anyhow::bail!(
            "project.name must start with an ASCII letter or digit and contain only ASCII letters, digits, '.', '_' or '-'; got {name:?}"
        );
    }
    Ok(())
}

fn validate_display_value(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("{field} must not be empty");
    }
    // A horizontal tab is representable in the generated DEED and desktop
    // metadata (both escape it). Reject every other control character so
    // config values cannot inject lines or terminal control sequences.
    if value.chars().any(|ch| ch.is_control() && ch != '\t') {
        anyhow::bail!("{field} must not contain control characters other than tab");
    }
    Ok(())
}

fn validate_no_controls(field: &str, value: &str) -> Result<()> {
    if value.chars().any(char::is_control) {
        anyhow::bail!("{field} must not contain control characters");
    }
    Ok(())
}

fn validate_url(url: &str) -> Result<()> {
    validate_no_controls("runtime.url", url)?;
    let authority = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or_default());
    if url.chars().any(char::is_whitespace)
        || authority.is_none_or(|host| {
            host.is_empty() || host.starts_with(':') || host.contains('@')
        })
    {
        anyhow::bail!("runtime.url must be an absolute HTTP(S) URL with a host, no credentials, and no whitespace");
    }
    Ok(())
}

fn validate_state_path(field: &str, path: &str) -> Result<()> {
    if path.trim().is_empty() {
        anyhow::bail!("{field} must not be empty");
    }
    validate_no_controls(field, path)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Icon {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct SoftAttach {
    #[serde(default)]
    pub tools: Vec<String>,
}

impl LauncherConfig {
    /// Load and parse a `<app>.launcher.a2ml` file from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading launcher config {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing launcher config {}", path.display()))
    }

    /// Parse a config from an in-memory string. Separate from `load` so tests
    /// don't need to touch the filesystem.
    pub fn parse(text: &str) -> Result<Self> {
        let cfg: LauncherConfig = toml::from_str(text)?;
        cfg.validate()?;
        Ok(cfg)
    }

    /// Shape-check the config. Runs after parsing so errors reference the
    /// *meaning* of the bad field, not the raw serde position.
    pub fn validate(&self) -> Result<()> {
        validate_project_name(&self.project.name)?;
        validate_display_value("project.display", &self.project.display)?;
        if let Some(value) = &self.project.description {
            validate_display_value("project.description", value)?;
        }
        if let Some(value) = &self.project.generic_name {
            validate_display_value("project.generic-name", value)?;
        }
        if let Some(version) = &self.project.version {
            if version.trim().is_empty() || version.chars().any(char::is_whitespace) {
                anyhow::bail!("project.version must be non-empty and contain no whitespace");
            }
            validate_no_controls("project.version", version)?;
        }
        if let Some(license) = &self.project.license {
            validate_display_value("project.license", license)?;
        }
        for category in &self.project.categories {
            if category.is_empty()
                || !category
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                anyhow::bail!(
                    "project.categories must contain ASCII alphanumeric/hyphen tokens; got {category:?}"
                );
            }
        }
        if self.repo.path.trim().is_empty() {
            anyhow::bail!("repo.path must not be empty");
        }
        validate_no_controls("repo.path", &self.repo.path)?;
        if let Some(url) = &self.runtime.url {
            validate_url(url)?;
        }
        if let Some(icon) = &self.icon {
            validate_state_path("icon.source", &icon.source)?;
        }
        if let Some(path) = &self.runtime.pid_file {
            validate_state_path("runtime.pid-file", path)?;
        }
        if let Some(path) = &self.runtime.log_file {
            validate_state_path("runtime.log-file", path)?;
        }
        if self.runtime.command.first().is_some_and(String::is_empty) {
            anyhow::bail!("runtime.command[0] must name an executable");
        }
        for (index, item) in self.runtime.command.iter().enumerate() {
            validate_no_controls(&format!("runtime.command[{index}]"), item)?;
        }
        for (index, item) in self.runtime.startup_command_search.iter().enumerate() {
            if item.is_empty() {
                anyhow::bail!("runtime.startup-command-search[{index}] must not be empty");
            }
            validate_no_controls(&format!("runtime.startup-command-search[{index}]"), item)?;
        }
        match self.runtime.kind {
            RuntimeKind::ServerUrl => {
                if self.runtime.url.is_none() && self.runtime.port.is_none() {
                    anyhow::bail!(
                        "runtime.kind = server-url requires either runtime.url or runtime.port"
                    );
                }
            }
            RuntimeKind::Process => {
                if self.runtime.command.is_empty() && self.runtime.startup_command_search.is_empty()
                {
                    anyhow::bail!(
                        "process runtime requires runtime.command or runtime.startup-command-search"
                    );
                }
            }
            RuntimeKind::Remote => {
                if self.runtime.url.is_none() {
                    anyhow::bail!("runtime.kind = remote requires runtime.url");
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stapeln_example() {
        let txt = include_str!("../../../examples/stapeln.launcher.fixture.a2ml");
        let cfg = LauncherConfig::parse(txt).expect("stapeln example must parse");
        assert_eq!(cfg.project.name, "stapeln");
        assert_eq!(cfg.project.display, "Stapeln");
        assert_eq!(cfg.runtime.port, Some(4010));
        assert_eq!(cfg.runtime.kind, RuntimeKind::ServerUrl);
        assert_eq!(cfg.runtime.startup_command_search.len(), 2);
    }

    #[test]
    fn server_url_without_url_or_port_fails() {
        let txt = r#"
            [project]
            name = "x"
            display = "X"
            [repo]
            path = "/tmp/x"
            [runtime]
            kind = "server-url"
        "#;
        assert!(LauncherConfig::parse(txt).is_err());
    }

    #[test]
    fn rejects_path_traversal_project_names() {
        let txt = r#"
            [project]
            name = "../victim"
            display = "X"
            [repo]
            path = "/tmp/x"
            [runtime]
            kind = "process"
            command = ["x"]
        "#;
        let err = LauncherConfig::parse(txt).unwrap_err().to_string();
        assert!(err.contains("project.name"));
    }

    #[test]
    fn rejects_control_characters_in_desktop_metadata() {
        let txt = r#"
            [project]
            name = "x"
            display = "X\nExec=sh"
            [repo]
            path = "/tmp/x"
            [runtime]
            kind = "process"
            command = ["x"]
        "#;
        assert!(LauncherConfig::parse(txt).is_err());
    }

    #[test]
    fn rejects_http_urls_without_a_host_or_with_credentials() {
        for url in ["https:///path", "http://?query=1", "https://user@example.test/"] {
            let txt = format!(
                r#"
                    [project]
                    name = "x"
                    display = "X"
                    [repo]
                    path = "/tmp/x"
                    [runtime]
                    kind = "remote"
                    url = {url:?}
                "#
            );
            assert!(LauncherConfig::parse(&txt).is_err(), "accepted {url}");
        }
    }

    #[test]
    fn rejects_non_http_runtime_urls() {
        let txt = r#"
            [project]
            name = "x"
            display = "X"
            [repo]
            path = "/tmp/x"
            [runtime]
            kind = "remote"
            url = "javascript:alert(1)"
        "#;
        assert!(LauncherConfig::parse(txt).is_err());
    }

    #[test]
    fn process_kind_requires_command() {
        let txt = r#"
            [project]
            name = "x"
            display = "X"
            [repo]
            path = "/tmp/x"
            [runtime]
            kind = "process"
        "#;
        assert!(LauncherConfig::parse(txt).is_err());
    }
}
