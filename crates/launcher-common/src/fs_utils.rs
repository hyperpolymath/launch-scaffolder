// SPDX-License-Identifier: MPL-2.0
// Copyright (c) 2026 Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! Small filesystem helpers shared by the CLI and provisioning backends.

use anyhow::{Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Replace a file atomically without following a pre-existing symlink at the
/// destination. `mode` is applied on Unix before the temporary file is
/// published. The private temporary directory is created beside the target so
/// the final rename stays on one filesystem.
pub fn write_atomic(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    write_atomic_impl(path, contents, Some(mode))
}

/// Atomically replace a file without changing its permissions. For a new file,
/// the operating system's normal creation mode and process umask apply.
pub fn write_atomic_unmodified(path: &Path, contents: &[u8]) -> Result<()> {
    write_atomic_impl(path, contents, None)
}

fn write_atomic_impl(path: &Path, contents: &[u8], mode: Option<u32>) -> Result<()> {
    #[cfg(not(unix))]
    let _ = mode;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .context("atomic-write destination must have a file name")?;

    for _ in 0..100 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = name.to_os_string();
        temp_name.push(format!(".{}.{}.tmpdir", std::process::id(), sequence));
        let temp_dir = parent.join(temp_name);

        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            // The directory stays private even when the process umask is 000.
            builder.mode(0o700);
        }
        match builder.create(&temp_dir) {
            Ok(()) => {
                let temp = temp_dir.join("contents");
                let result = (|| -> Result<()> {
                    let mut options = OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(if mode.is_some() { 0o600 } else { 0o666 });
                    }
                    let mut file = options.open(&temp).with_context(|| {
                        format!("creating temporary file in {}", temp_dir.display())
                    })?;
                    file.write_all(contents)
                        .with_context(|| format!("writing temporary file {}", temp.display()))?;
                    file.sync_all()
                        .with_context(|| format!("syncing temporary file {}", temp.display()))?;
                    #[cfg(unix)]
                    if let Some(mode) = mode {
                        use std::os::unix::fs::PermissionsExt;
                        file.set_permissions(fs::Permissions::from_mode(mode))
                            .with_context(|| format!("setting permissions on {}", temp.display()))?;
                    }
                    drop(file);
                    fs::rename(&temp, path).with_context(|| {
                        format!("replacing {} with {}", path.display(), temp.display())
                    })?;
                    Ok(())
                })();
                let cleanup = fs::remove_dir_all(&temp_dir).with_context(|| {
                    format!("removing temporary directory {}", temp_dir.display())
                });
                result?;
                cleanup?;
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(e).with_context(|| {
                    format!("creating temporary directory in {}", parent.display())
                });
            }
        }
    }

    anyhow::bail!("could not allocate a unique temporary directory beside {}", path.display())
}

/// Read the current mode when possible, otherwise use `fallback`.
pub fn existing_mode_or(path: &Path, fallback: u32) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .unwrap_or(fallback)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn atomic_write_replaces_contents_and_applies_mode() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("launch-scaffolder-fs-{unique}"));
        fs::create_dir(&dir).unwrap();
        let target = dir.join("output.txt");
        fs::write(&target, b"old").unwrap();

        write_atomic(&target, b"new", 0o640).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "temporary file was left behind");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_write_unmodified_respects_normal_non_executable_creation_mode() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("launch-scaffolder-fs-umask-{unique}"));
        fs::create_dir(&dir).unwrap();
        let target = dir.join("output.txt");
        write_atomic_unmodified(&target, b"new").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o111, 0);
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
