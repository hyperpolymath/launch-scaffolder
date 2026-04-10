// SPDX-License-Identifier: PMPL-1.0-or-later
// Copyright (c) 2026 Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>

//! Parser, renderer, and in-place rewriter for the
//! `# @a2ml-metadata begin ... # @a2ml-metadata end` block embedded at
//! the top of every generated launcher script.
//!
//! Example input (from a real generated launcher):
//!
//! ```text
//! # @a2ml-metadata begin
//! # (
//! #   id                   = "burble-launcher"
//! #   type                 = "launcher"
//! #   version              = "0.1.0"
//! #   app-name             = "burble"
//! #   runtime-kind         = "server-url"
//! #   standards-compliance = [
//! #     "launcher-standard.adoc"
//! #     "LM-LA-LIFECYCLE-STANDARD.adoc"
//! #   ]
//! #   generator             = "launch-scaffolder"
//! # )
//! # @a2ml-metadata end
//! ```
//!
//! The format is not standard TOML — key lines are `#`-prefixed,
//! scalars are double-quoted, and list values are space-separated
//! items on their own lines. We parse it with a hand-rolled scanner
//! rather than trying to coerce it into a real A2ML parser, because
//! (a) the format is 100% controlled by `launcher.sh.tera`, and
//! (b) we only care about a fixed set of keys.

use crate::Result;
use anyhow::{Context, bail};
use std::path::Path;

/// Required scalar keys that every well-formed metadata block must
/// carry, per `launcher-standard.adoc`.
pub const REQUIRED_SCALAR_KEYS: &[&str] = &[
    "id",
    "type",
    "version",
    "app-name",
    "app-display",
    "runtime-kind",
    "standard-spec-version",
    "generator",
];

/// Parsed metadata block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataBlock {
    /// Scalar key → string value, in insertion order.
    pub scalars: Vec<(String, String)>,
    /// List key → list of string values, in insertion order.
    pub lists: Vec<(String, Vec<String>)>,
    /// The raw lines of the block (inclusive of begin/end markers),
    /// retained for lossless rewrites.
    pub raw_lines: Vec<String>,
    /// Line indices inside the host file.
    pub start_line: usize,
    pub end_line: usize,
}

impl MetadataBlock {
    /// Lookup a scalar value by key.
    pub fn scalar(&self, key: &str) -> Option<&str> {
        self.scalars
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Lookup a list value by key.
    pub fn list(&self, key: &str) -> Option<&[String]> {
        self.lists
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_slice())
    }

    /// Validate the block carries every required key. Returns the list
    /// of missing keys (empty on success).
    pub fn missing_required(&self) -> Vec<&'static str> {
        REQUIRED_SCALAR_KEYS
            .iter()
            .copied()
            .filter(|k| self.scalar(k).is_none())
            .collect()
    }
}

/// Extract the metadata block from a generated launcher script.
///
/// Returns `Ok(None)` if no block is present — callers decide whether
/// that's an error.
pub fn parse_from_script(path: &Path) -> Result<Option<MetadataBlock>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_from_text(&text)
}

/// In-memory variant of [`parse_from_script`], separate so tests don't
/// need the filesystem.
pub fn parse_from_text(text: &str) -> Result<Option<MetadataBlock>> {
    let lines: Vec<&str> = text.lines().collect();
    let Some(start) = lines
        .iter()
        .position(|l| l.trim_start().starts_with("# @a2ml-metadata begin"))
    else {
        return Ok(None);
    };
    let Some(rel_end) = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with("# @a2ml-metadata end"))
    else {
        bail!(
            "metadata block starting at line {} has no matching `# @a2ml-metadata end`",
            start + 1
        );
    };
    let end = start + 1 + rel_end;

    let raw_lines: Vec<String> = lines[start..=end].iter().map(|s| s.to_string()).collect();

    let (scalars, lists) = parse_body(&raw_lines)?;
    Ok(Some(MetadataBlock {
        scalars,
        lists,
        raw_lines,
        start_line: start,
        end_line: end,
    }))
}

/// Parse the body lines (between `begin` and `end`). Skips `(` / `)`
/// wrapper lines.
#[allow(clippy::type_complexity)]
fn parse_body(raw_lines: &[String]) -> Result<(Vec<(String, String)>, Vec<(String, Vec<String>)>)> {
    let mut scalars: Vec<(String, String)> = Vec::new();
    let mut lists: Vec<(String, Vec<String>)> = Vec::new();
    // State for multi-line lists: Some((key, accum)) while inside a `[ ... ]` block.
    let mut pending_list: Option<(String, Vec<String>)> = None;

    for (idx, line) in raw_lines.iter().enumerate() {
        // Skip begin / end markers.
        if idx == 0 || idx == raw_lines.len() - 1 {
            continue;
        }
        let stripped = strip_comment_prefix(line);
        let trimmed = stripped.trim();

        // Skip wrapper `(` / `)`.
        if trimmed == "(" || trimmed == ")" || trimmed.is_empty() {
            continue;
        }

        // Continuing an open list?
        if pending_list.is_some() {
            if trimmed == "]" {
                // `is_some` above guarantees `take` yields Some, but we
                // avoid `.unwrap()` anyway so panic-attack / clippy are
                // happy and a refactor can't accidentally fall through.
                if let Some((key, values)) = pending_list.take() {
                    lists.push((key, values));
                }
                continue;
            }
            if let Some(item) = unquote(trimmed) {
                if let Some((_, accum)) = pending_list.as_mut() {
                    accum.push(item);
                }
                continue;
            }
            bail!(
                "unexpected line inside list body at line {}: {:?}",
                idx,
                trimmed
            );
        }

        // key = "scalar" | key = [ ... | key = [ item item ... ]
        let Some(eq_idx) = trimmed.find('=') else {
            bail!("unparseable metadata line {}: {:?}", idx, trimmed);
        };
        let key = trimmed[..eq_idx].trim().to_string();
        let rhs = trimmed[eq_idx + 1..].trim();

        if let Some(scalar) = unquote(rhs) {
            scalars.push((key, scalar));
            continue;
        }

        if let Some(after) = rhs.strip_prefix('[') {
            // Either inline `[ "a" "b" ]` or multi-line open `[`.
            let after = after.trim();
            if after.is_empty() {
                pending_list = Some((key, Vec::new()));
                continue;
            }
            if let Some(close) = after.strip_suffix(']') {
                let items = close
                    .split_whitespace()
                    .filter_map(unquote_owned)
                    .collect::<Vec<_>>();
                lists.push((key, items));
                continue;
            }
            // `[ "a"` pattern on the same line, rest on following lines.
            let mut items: Vec<String> = Vec::new();
            for piece in after.split_whitespace() {
                if let Some(v) = unquote(piece) {
                    items.push(v);
                }
            }
            pending_list = Some((key, items));
            continue;
        }

        bail!("unparseable metadata rhs at line {}: {:?}", idx, rhs);
    }

    if let Some((key, _)) = pending_list {
        bail!("metadata list `{}` has no closing `]`", key);
    }

    Ok((scalars, lists))
}

/// Strip the leading `# ` (or `#`) that every metadata line carries.
fn strip_comment_prefix(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        rest
    } else if let Some(rest) = trimmed.strip_prefix('#') {
        rest
    } else {
        trimmed
    }
}

fn unquote(s: &str) -> Option<String> {
    unquote_owned(s)
}

fn unquote_owned(s: &str) -> Option<String> {
    let s = s.trim();
    let s = s.strip_suffix(',').unwrap_or(s).trim();
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map(|s| s.to_string())
}

/// In-place rewrite: replace the scalar value of `key` with `new_value`
/// in `text`, preserving the original formatting (column alignment,
/// surrounding whitespace). Errors if `key` is absent or is a list key.
pub fn rewrite_scalar(text: &str, key: &str, new_value: &str) -> Result<String> {
    let block = parse_from_text(text)?.context("no @a2ml-metadata block found in input")?;
    if block.scalar(key).is_none() {
        if block.list(key).is_some() {
            bail!(
                "key `{}` is a list, not a scalar — set not supported for lists",
                key
            );
        }
        bail!("key `{}` not present in metadata block", key);
    }

    let mut lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    let mut patched = false;

    // Scan only the block range — we don't touch anything outside it.
    for line in &mut lines[block.start_line..=block.end_line] {
        let inner = strip_comment_prefix(line);
        let trimmed_inner = inner.trim();
        let Some(eq_idx) = trimmed_inner.find('=') else {
            continue;
        };
        let this_key = trimmed_inner[..eq_idx].trim();
        if this_key != key {
            continue;
        }
        // Only operate on scalar lines (no `[`).
        let rhs = trimmed_inner[eq_idx + 1..].trim();
        if rhs.starts_with('[') {
            continue;
        }
        // Preserve the prefix (leading `# ` + whitespace + key + whitespace + `=` + whitespace)
        // by substituting only the quoted value span.
        let Some(value_start) = line.find('"') else {
            continue;
        };
        let rest = &line[value_start + 1..];
        let Some(value_end_rel) = rest.find('"') else {
            continue;
        };
        let value_end = value_start + 1 + value_end_rel;
        let mut rewritten = String::with_capacity(line.len() + new_value.len());
        rewritten.push_str(&line[..value_start + 1]);
        rewritten.push_str(new_value);
        rewritten.push_str(&line[value_end..]);
        *line = rewritten;
        patched = true;
        break;
    }

    if !patched {
        bail!(
            "found key `{}` in parsed block but could not rewrite in-place",
            key
        );
    }

    // Preserve trailing newline if the input had one.
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"#!/usr/bin/env bash
# SPDX-License-Identifier: PMPL-1.0-or-later
#
# @a2ml-metadata begin
# (
#   id                   = "burble-launcher"
#   type                 = "launcher"
#   version              = "0.1.0"
#   app-name             = "burble"
#   app-display          = "Burble"
#   app-url              = "http://localhost:4020"
#   runtime-kind         = "server-url"
#   standards-compliance = [
#     "launcher-standard.adoc"
#     "LM-LA-LIFECYCLE-STANDARD.adoc"
#   ]
#   standard-spec-version = "0.1.0"
#   generator             = "launch-scaffolder"
# )
# @a2ml-metadata end
#
echo "not the block"
"#;

    #[test]
    fn parses_scalars_and_lists() {
        let block = parse_from_text(SAMPLE).unwrap().unwrap();
        assert_eq!(block.scalar("id"), Some("burble-launcher"));
        assert_eq!(block.scalar("version"), Some("0.1.0"));
        assert_eq!(block.scalar("app-name"), Some("burble"));
        assert_eq!(block.scalar("runtime-kind"), Some("server-url"));
        assert_eq!(block.scalar("generator"), Some("launch-scaffolder"));
        let compliance = block.list("standards-compliance").unwrap();
        assert_eq!(compliance.len(), 2);
        assert_eq!(compliance[0], "launcher-standard.adoc");
    }

    #[test]
    fn validates_required_keys() {
        let block = parse_from_text(SAMPLE).unwrap().unwrap();
        assert!(block.missing_required().is_empty());
    }

    #[test]
    fn detects_missing_required_keys() {
        let trimmed = SAMPLE.replace("#   version              = \"0.1.0\"\n", "");
        let block = parse_from_text(&trimmed).unwrap().unwrap();
        assert_eq!(block.missing_required(), vec!["version"]);
    }

    #[test]
    fn rewrites_scalar_in_place() {
        let out = rewrite_scalar(SAMPLE, "version", "0.2.0").unwrap();
        assert!(out.contains("version              = \"0.2.0\""));
        assert!(!out.contains("version              = \"0.1.0\""));
        // Everything outside the block is untouched.
        assert!(out.contains("#!/usr/bin/env bash"));
        assert!(out.contains("echo \"not the block\""));
        // Re-parsing the result should round-trip.
        let reparsed = parse_from_text(&out).unwrap().unwrap();
        assert_eq!(reparsed.scalar("version"), Some("0.2.0"));
    }

    #[test]
    fn rejects_set_on_list_key() {
        let err = rewrite_scalar(SAMPLE, "standards-compliance", "x").unwrap_err();
        assert!(err.to_string().contains("list, not a scalar"));
    }

    #[test]
    fn rejects_set_on_missing_key() {
        let err = rewrite_scalar(SAMPLE, "nonexistent", "x").unwrap_err();
        assert!(err.to_string().contains("not present"));
    }

    #[test]
    fn returns_none_when_no_block_present() {
        assert!(parse_from_text("no block here\n").unwrap().is_none());
    }

    #[test]
    fn errors_on_unterminated_block() {
        let unterminated = "# @a2ml-metadata begin\n# (\n#   id = \"x\"\n";
        let err = parse_from_text(unterminated).unwrap_err();
        assert!(err.to_string().contains("no matching"));
    }
}
