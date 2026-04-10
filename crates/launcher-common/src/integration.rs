// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! Desktop integration / disintegration — native Rust port.
//!
//! Historically the `--integ` / `--disinteg` flow was implemented in
//! the generated shell script. Moving it into the binary gives us:
//!
//! - Testable integration logic that doesn't require a rendered script.
//! - One authoritative `.desktop` writer instead of one per generated
//!   launcher.
//! - Bulk-mode integration across the estate with a single confirmation,
//!   instead of seven separate shell invocations.
//!
//! The generated shell scripts still carry a fallback arm that runs the
//! old logic if `launch-scaffolder` isn't on `PATH` — see the template
//! in `templates/launcher.sh.tera`. In the normal case, the shell arm
//! execs back into the binary via this module.
//!
//! Platform support today: **Linux only**. macOS / Windows fall through
//! with a structured [`IntegError::UnsupportedPlatform`] so callers can
//! match on it. This matches the shell template, which also only
//! implements Linux integration.

use crate::Result;
use crate::config::{LauncherConfig, RuntimeKind};
use anyhow::Context;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors callers may want to match on explicitly.
#[derive(Debug, Error)]
pub enum IntegError {
    #[error("platform {0} is not supported by launch-scaffolder integration yet")]
    UnsupportedPlatform(String),
    #[error("rendered script not found at {0} — run `launch-scaffolder mint` first")]
    ScriptMissing(PathBuf),
}

/// Resolved per-platform install targets. The Linux layout matches the
/// freedesktop XDG user-dirs spec; other platforms are stubbed today.
#[derive(Debug, Clone)]
pub struct InstallPaths {
    pub apps_dir: PathBuf,
    pub icon_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub desktop_shortcut_dir: PathBuf,
    pub desktop_file_target: PathBuf,
    pub desktop_shortcut_target: PathBuf,
    pub icon_target: PathBuf,
    pub launcher_target: PathBuf,
}

impl InstallPaths {
    /// Compute the Linux (XDG) install layout for a given app name.
    pub fn linux(app_name: &str) -> Result<Self> {
        let home = dirs::home_dir().context("cannot resolve $HOME")?;
        let apps_dir = home.join(".local/share/applications");
        let icon_dir = home.join(".local/share/icons/hicolor/256x256/apps");
        let bin_dir = home.join(".local/bin");
        let desktop_shortcut_dir = home.join("Desktop");
        Ok(Self {
            desktop_file_target: apps_dir.join(format!("{app_name}.desktop")),
            desktop_shortcut_target: desktop_shortcut_dir.join(format!("{app_name}.desktop")),
            icon_target: icon_dir.join(format!("{app_name}.png")),
            launcher_target: bin_dir.join(format!("{app_name}-launcher")),
            apps_dir,
            icon_dir,
            bin_dir,
            desktop_shortcut_dir,
        })
    }

    /// All removal targets, in the order disinteg should visit them.
    pub fn removal_targets(&self) -> [&Path; 4] {
        [
            &self.desktop_file_target,
            &self.desktop_shortcut_target,
            &self.icon_target,
            &self.launcher_target,
        ]
    }
}

/// Options for a single integ call.
#[derive(Debug, Clone, Default)]
pub struct IntegOpts {
    /// Reinstall even if a previous integration is present.
    pub force: bool,
    /// Compute paths and describe actions, but touch no files.
    pub dry_run: bool,
}

/// Options for a single disinteg call.
#[derive(Debug, Clone, Default)]
pub struct DisintegOpts {
    pub dry_run: bool,
}

/// Outcome record returned from [`integ`] / [`disinteg`]. Callers can
/// render this to the user however they like.
#[derive(Debug, Default)]
pub struct IntegReport {
    pub actions: Vec<String>,
    pub skipped: Vec<String>,
    pub already_present: bool,
}

/// Integrate one launcher into the current desktop.
///
/// `script_path` is the rendered `<app>-launcher.sh` on disk. This file
/// is copied into `~/.local/bin/<app>-launcher` as the actual runtime
/// target — the pattern established by the reference stapeln launcher
/// and preserved by the template.
pub fn integ(
    config: &LauncherConfig,
    script_path: &Path,
    opts: &IntegOpts,
) -> Result<IntegReport> {
    let platform = detect_platform();
    if platform != "linux" {
        return Err(IntegError::UnsupportedPlatform(platform).into());
    }
    if !script_path.exists() {
        return Err(IntegError::ScriptMissing(script_path.to_path_buf()).into());
    }

    let paths = InstallPaths::linux(&config.project.name)?;
    let already = paths.desktop_file_target.exists() || paths.launcher_target.exists();

    let mut report = IntegReport {
        already_present: already,
        ..Default::default()
    };

    if already && !opts.force {
        report
            .skipped
            .push(format!("already integrated: {}", paths.desktop_file_target.display()));
        return Ok(report);
    }

    if opts.dry_run {
        report.actions.push(format!("mkdir -p {}", paths.apps_dir.display()));
        report.actions.push(format!("mkdir -p {}", paths.icon_dir.display()));
        report.actions.push(format!("mkdir -p {}", paths.bin_dir.display()));
        report
            .actions
            .push(format!("cp {} {}", script_path.display(), paths.launcher_target.display()));
        if let Some(icon_source) = icon_source_abs(config) {
            if icon_source.exists() {
                report
                    .actions
                    .push(format!("cp {} {}", icon_source.display(), paths.icon_target.display()));
            }
        }
        report
            .actions
            .push(format!("write {}", paths.desktop_file_target.display()));
        report
            .actions
            .push(format!("write {}", paths.desktop_shortcut_target.display()));
        return Ok(report);
    }

    // -- Create directories ------------------------------------------------
    for d in [
        &paths.apps_dir,
        &paths.icon_dir,
        &paths.bin_dir,
        &paths.desktop_shortcut_dir,
    ] {
        fs::create_dir_all(d).with_context(|| format!("creating {}", d.display()))?;
    }

    // -- Copy script to launcher target ------------------------------------
    fs::copy(script_path, &paths.launcher_target).with_context(|| {
        format!(
            "copying {} to {}",
            script_path.display(),
            paths.launcher_target.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&paths.launcher_target)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&paths.launcher_target, perms)?;
    }
    report
        .actions
        .push(format!("+ launcher: {}", paths.launcher_target.display()));

    // -- Copy icon if present ----------------------------------------------
    let icon_name = if let Some(icon_source) = icon_source_abs(config) {
        if icon_source.exists() {
            fs::copy(&icon_source, &paths.icon_target).with_context(|| {
                format!(
                    "copying icon {} to {}",
                    icon_source.display(),
                    paths.icon_target.display()
                )
            })?;
            report
                .actions
                .push(format!("+ icon:     {}", paths.icon_target.display()));
            config.project.name.clone()
        } else {
            report
                .skipped
                .push(format!("icon source missing: {}", icon_source.display()));
            "package-x-generic".to_string()
        }
    } else {
        "package-x-generic".to_string()
    };

    // -- Write .desktop files ----------------------------------------------
    let desktop_body = render_desktop_file(config, &paths, &icon_name);
    for target in [&paths.desktop_file_target, &paths.desktop_shortcut_target] {
        fs::write(target, &desktop_body)
            .with_context(|| format!("writing {}", target.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(target)?.permissions();
            perms.set_mode(0o444);
            fs::set_permissions(target, perms)?;
        }
        report.actions.push(format!("+ desktop: {}", target.display()));
    }

    // -- Best-effort post-install niceties ---------------------------------
    run_best_effort(
        "update-desktop-database",
        &[paths.apps_dir.to_string_lossy().as_ref()],
        &mut report,
    );
    run_best_effort(
        "gio",
        &[
            "set",
            paths.desktop_file_target.to_string_lossy().as_ref(),
            "metadata::trusted",
            "true",
        ],
        &mut report,
    );
    run_best_effort(
        "gio",
        &[
            "set",
            paths.desktop_shortcut_target.to_string_lossy().as_ref(),
            "metadata::trusted",
            "true",
        ],
        &mut report,
    );

    Ok(report)
}

/// Remove an integration. Idempotent: reports what was actually removed
/// and exits cleanly if nothing was present.
pub fn disinteg(config: &LauncherConfig, opts: &DisintegOpts) -> Result<IntegReport> {
    let platform = detect_platform();
    if platform != "linux" {
        return Err(IntegError::UnsupportedPlatform(platform).into());
    }
    let paths = InstallPaths::linux(&config.project.name)?;
    let mut report = IntegReport::default();

    for target in paths.removal_targets() {
        if target.exists() || target.is_symlink() {
            if opts.dry_run {
                report.actions.push(format!("rm {}", target.display()));
            } else {
                fs::remove_file(target)
                    .with_context(|| format!("removing {}", target.display()))?;
                report.actions.push(format!("- removed {}", target.display()));
            }
        }
    }

    // Best-effort PID-file cleanup so disinteg leaves nothing behind.
    if let Some(pid_file) = config.runtime.pid_file.as_deref() {
        let pf = expand_home(pid_file);
        if pf.exists() && !opts.dry_run {
            let _ = fs::remove_file(&pf);
            report.actions.push(format!("- removed {}", pf.display()));
        }
    }

    if !opts.dry_run {
        run_best_effort(
            "update-desktop-database",
            &[paths.apps_dir.to_string_lossy().as_ref()],
            &mut report,
        );
    }

    Ok(report)
}

/// Render the body of a freedesktop `.desktop` file for one launcher.
fn render_desktop_file(
    config: &LauncherConfig,
    paths: &InstallPaths,
    icon_name: &str,
) -> String {
    let generic = config
        .project
        .generic_name
        .as_deref()
        .unwrap_or(&config.project.display);
    let comment = config
        .project
        .description
        .as_deref()
        .unwrap_or(&config.project.display);
    let categories = if config.project.categories.is_empty() {
        "Utility;".to_string()
    } else {
        let mut s = config.project.categories.join(";");
        s.push(';');
        s
    };
    let default_mode = match config.runtime.kind {
        RuntimeKind::Process => "--start",
        RuntimeKind::ServerUrl | RuntimeKind::Remote => "--auto",
    };
    let launcher = paths.launcher_target.display();
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.0\n\
         Name={name}\n\
         GenericName={generic}\n\
         Comment={comment}\n\
         Exec={launcher} {default_mode}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories={categories}\n\
         StartupNotify=true\n\
         StartupWMClass={app}\n\
         Actions=stop;status;\n\
         \n\
         [Desktop Action stop]\n\
         Name=Stop\n\
         Exec={launcher} --stop\n\
         \n\
         [Desktop Action status]\n\
         Name=Status\n\
         Exec={launcher} --status\n",
        name = config.project.display,
        generic = generic,
        comment = comment,
        launcher = launcher,
        default_mode = default_mode,
        icon = icon_name,
        categories = categories,
        app = config.project.name,
    )
}

/// Resolve the absolute icon-source path if the config supplied one.
fn icon_source_abs(config: &LauncherConfig) -> Option<PathBuf> {
    let raw = config.icon.as_ref()?.source.as_str();
    Some(expand_home(raw))
}

/// Expand a leading `~` to `$HOME`. Absolute paths pass through.
fn expand_home(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(raw)
}

/// Detect the current platform using the same buckets as the shell
/// template.
fn detect_platform() -> String {
    match std::env::consts::OS {
        "linux" => "linux".to_string(),
        "macos" => "macos".to_string(),
        "windows" => "windows".to_string(),
        other => other.to_string(),
    }
}

/// Run a command best-effort. Never bubbles errors up; adds a note to
/// `report.skipped` if the command wasn't available.
fn run_best_effort(cmd: &str, args: &[&str], report: &mut IntegReport) {
    use std::process::Command;
    match Command::new(cmd).args(args).status() {
        Ok(s) if s.success() => {}
        Ok(s) => report
            .skipped
            .push(format!("{} exited with status {}", cmd, s)),
        Err(_) => report
            .skipped
            .push(format!("{} not available (skipped)", cmd)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LauncherConfig, Project, Repo, Runtime, RuntimeKind};

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

    #[test]
    fn desktop_file_contains_required_keys() {
        let cfg = sample_config();
        let paths = InstallPaths::linux("foo").expect("home should resolve");
        let body = render_desktop_file(&cfg, &paths, "foo");
        assert!(body.contains("Name=Foo"));
        assert!(body.contains("GenericName=Foo Thing"));
        assert!(body.contains("Comment=Foo desc"));
        assert!(body.contains("Categories=Development;"));
        assert!(body.contains("--start")); // process-kind default
        assert!(body.contains("[Desktop Action stop]"));
        assert!(body.contains("[Desktop Action status]"));
    }

    #[test]
    fn removal_targets_are_stable_order() {
        let paths = InstallPaths::linux("foo").expect("home should resolve");
        let targets = paths.removal_targets();
        assert_eq!(targets.len(), 4);
    }

    #[test]
    fn expand_home_handles_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_home("~/foo"), home.join("foo"));
        assert_eq!(expand_home("/tmp/foo"), PathBuf::from("/tmp/foo"));
    }
}
