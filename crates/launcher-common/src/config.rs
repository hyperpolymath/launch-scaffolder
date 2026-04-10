// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

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

    #[serde(default)]
    pub pid_file: Option<String>,
    #[serde(default)]
    pub log_file: Option<String>,

    #[serde(default = "default_wait_seconds")]
    pub wait_for_url_timeout_seconds: u32,
}

fn default_wait_seconds() -> u32 {
    15
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
                        "runtime.kind = process requires runtime.command or runtime.startup-command-search"
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
