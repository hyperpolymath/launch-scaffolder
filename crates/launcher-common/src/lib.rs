// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! launch-scaffolder shared library.
//!
//! This crate contains the implemented shared logic for the CLI:
//!
//! - [`deed`] and [`standard`] — parse DEED and resolve/validate the launcher standard.
//! - [`config`] — parse and validate per-app TOML descriptors with legacy `.a2ml` names.
//! - [`template`] — render a Bash launcher with safely quoted config values.
//! - [`discovery`] — find live estate descriptors while excluding fixtures.
//! - [`integration`] — atomically install/remove Linux desktop entries and launcher files.
//! - [`fs_utils`] — same-filesystem atomic file replacement.
//! - [`metadata_block`] — read current DEED metadata and legacy A2ML metadata.
//!
//! [`platform`], [`integrity`], and [`exceptions`] remain placeholders, are not
//! called by the CLI, and must not be advertised as implemented APIs. Native
//! macOS and Windows integration are also outstanding.
//!
//! The `launch-scaffolder` binary crate in this workspace is a thin CLI over
//! these modules; future library consumers can reuse the implemented logic
//! without depending on the `clap` or subcommand infrastructure.

pub mod config;
pub mod deed;
pub mod discovery;
pub mod exceptions;
pub mod fs_utils;
pub mod integration;
pub mod integrity;
pub mod metadata_block;
pub mod platform;
pub mod standard;
pub mod template;

/// Crate-wide result type.
pub type Result<T> = anyhow::Result<T>;

/// Crate version, sourced from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
