// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! launch-scaffolder shared library.
//!
//! This crate is the heart of the launch-scaffolder tool. It contains:
//!
//! - [`standard`] — parse the `launcher-standard.a2ml` spec file.
//! - [`config`] — parse per-app `<app>.launcher.a2ml` config files.
//! - [`template`] — render a launcher shell script from a standard + config.
//! - [`platform`] — cross-platform file path, permission, and dispatch helpers.
//! - [`integrity`] — SHA-256 integrity manifest generation (SPARK-verifiable
//!   in a future phase via Zig FFI to an Ada/SPARK module).
//! - [`exceptions`] — merge logic for standard + config + per-app `[exceptions]`
//!   overrides.
//!
//! The `launch-scaffolder` binary crate in this workspace is a thin CLI over
//! these modules; all meaningful logic lives here so future surfaces (PanLL
//! panel, library consumers, test harnesses) can reuse it without depending
//! on the `clap` or subcommand infrastructure.

pub mod config;
pub mod exceptions;
pub mod integrity;
pub mod platform;
pub mod standard;
pub mod template;

/// Crate-wide result type.
pub type Result<T> = anyhow::Result<T>;

/// Crate version, sourced from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
