// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
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

//!
//! # Two dialects
//!
//! Every launcher minted to date carries the legacy block above. Newer
//! ones may instead carry a DEED block, which this module reads as
//! well:
//!
//! ```text
//! # @launcher-deed begin
//! # ;; SPDX-License-Identifier: MPL-2.0
//! # (praxis-deed
//! #   :schema-version  "1.0.0"
//! #   :canonical-name  "burble-launcher"
//! #   :beholding-chora #u5"estate/chora"
//! #   (artefact :type "launcher" :version "0.1.0"
//! #             :generator "launch-scaffolder")
//! #   (app :name "burble" :display "Burble"
//! #        :runtime-kind "server-url")
//! #   (compliance :standard-version "0.4.0"
//! #               :standards ("launcher-standard_praxis.deed")))
//! # @launcher-deed end
//! ```
//!
//! Both flatten to the same [`MetadataBlock`], so every existing caller
//! is dialect-blind. The DEED form is **not** re-scanned here: the `#`
//! prefix is stripped and the text handed to [`crate::deed::parse`], the
//! normative grammar. A second scanner in this file would be cheaper and
//! would be a second DEED grammar that nothing forces to agree with the
//! first.
//!
//! Note the new marker does not say `a2ml`. A2ML is retired; a marker
//! carrying the name would propagate it into every launcher minted from
//! here on.

use crate::Result;
use crate::deed;
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

/// Markers for the legacy block emitted by every launcher minted to date.
pub const LEGACY_BEGIN: &str = "# @a2ml-metadata begin";
/// Closing marker for [`LEGACY_BEGIN`].
pub const LEGACY_END: &str = "# @a2ml-metadata end";

/// Markers for the DEED-dialect block. Deliberately **not** named
/// `a2ml`: A2ML is retired, and a new marker carrying the retired name
/// would propagate it into every launcher minted from here on.
pub const DEED_BEGIN: &str = "# @launcher-deed begin";
/// Closing marker for [`DEED_BEGIN`].
pub const DEED_END: &str = "# @launcher-deed end";

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

    /// Whether this block was written in the DEED dialect (`@launcher-deed`)
    /// rather than the legacy `@a2ml-metadata` form.
    ///
    /// Derived from the captured marker line rather than stored, so it
    /// cannot drift from the text the block was parsed out of.
    pub fn is_deed(&self) -> bool {
        self.raw_lines
            .first()
            .is_some_and(|l| l.trim_start().starts_with(DEED_BEGIN))
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
///
/// Dispatches on which marker pair the script carries. A script holding
/// both is rejected rather than silently preferring one — two blocks
/// mean two answers to the same question, and picking one quietly is
/// how a launcher ends up describing itself twice.
pub fn parse_from_text(text: &str) -> Result<Option<MetadataBlock>> {
    let lines: Vec<&str> = text.lines().collect();
    let legacy = find_marked_span(&lines, LEGACY_BEGIN, LEGACY_END)?;
    let deed_span = find_marked_span(&lines, DEED_BEGIN, DEED_END)?;

    match (legacy, deed_span) {
        (Some(_), Some(_)) => bail!(
            "script carries BOTH a `{}` block and a `{}` block — exactly one is expected",
            LEGACY_BEGIN,
            DEED_BEGIN
        ),
        (Some((start, end)), None) => {
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
        (None, Some((start, end))) => {
            let raw_lines: Vec<String> = lines[start..=end].iter().map(|s| s.to_string()).collect();
            let (scalars, lists) = parse_deed_body(&raw_lines)?;
            Ok(Some(MetadataBlock {
                scalars,
                lists,
                raw_lines,
                start_line: start,
                end_line: end,
            }))
        }
        (None, None) => Ok(None),
    }
}

/// Locate a `begin` / `end` marker pair, returning inclusive line indices.
///
/// Matching is on `trim_start().starts_with(..)`, the same test the
/// original reader used, so an indented marker still counts.
fn find_marked_span(lines: &[&str], begin: &str, end: &str) -> Result<Option<(usize, usize)>> {
    let Some(start) = lines.iter().position(|l| l.trim_start().starts_with(begin)) else {
        return Ok(None);
    };
    let Some(rel_end) = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with(end))
    else {
        bail!(
            "metadata block starting at line {} has no matching `{end}`",
            start + 1
        );
    };
    Ok(Some((start, start + 1 + rel_end)))
}

/// Strip the shell comment prefix from every line of an embedded block,
/// yielding the text as the embedded language actually wrote it.
///
/// A DEED's own comment marker is `;;` and `#` is not legal anywhere in
/// the grammar, so the `#` must come off before [`deed::parse`] sees the
/// text. Exactly one following space is removed, so the block's own
/// indentation survives — a deed treats spaces as separators, so the
/// indentation is cosmetic, but preserving it keeps error line/column
/// reports aligned with what a reader sees in the script.
fn uncomment(body: &[String]) -> Result<String> {
    let mut out = String::new();
    for (i, line) in body.iter().enumerate() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix('#') else {
            bail!(
                "line {} of the embedded deed block is not a `#` comment line: {:?}",
                i + 1,
                line
            );
        };
        out.push_str(rest.strip_prefix(' ').unwrap_or(rest));
        out.push('\n');
    }
    Ok(out)
}

/// Parse a `# @launcher-deed` block by handing the uncommented text to
/// the real DEED parser, then flattening the result into the same
/// scalar/list shape the legacy block produces.
///
/// The delegation is the point. A second s-expression scanner living
/// here would be cheaper today and would manufacture a second DEED
/// grammar that nothing forces to agree with `deed.rs` — it would keep
/// passing while being wrong the moment the normative grammar moves.
#[allow(clippy::type_complexity)]
fn parse_deed_body(
    raw_lines: &[String],
) -> Result<(Vec<(String, String)>, Vec<(String, Vec<String>)>)> {
    // Drop the begin/end markers; everything between them is the deed.
    let body = &raw_lines[1..raw_lines.len().saturating_sub(1)];
    let text = uncomment(body)?;
    let node = deed::parse(&text).context("parsing the embedded `@launcher-deed` block")?;
    flatten_praxis_deed(&node)
}

/// Flatten a `praxis-deed` node into the flat key/value shape the rest
/// of `launch-scaffolder` already consumes.
///
/// One source per key, no fallbacks: a key that is absent from the deed
/// is absent from the block, and [`MetadataBlock::missing_required`]
/// reports it exactly as it would for a malformed legacy block.
#[allow(clippy::type_complexity)]
fn flatten_praxis_deed(
    node: &deed::Node,
) -> Result<(Vec<(String, String)>, Vec<(String, Vec<String>)>)> {
    if node.head != "praxis-deed" {
        bail!(
            "an embedded `@launcher-deed` block must be a `praxis-deed`, got `{}`",
            node.head
        );
    }
    // `:beholding-chora` names the vocabulary this document is read
    // against. A tool may not declare its own vocabulary, so a praxis
    // deed without one resolves against nothing.
    match node.field("beholding-chora") {
        Some(deed::Value::Uuid5(_)) => {}
        Some(other) => {
            bail!("`:beholding-chora` must be a uuid5 literal (`#u5\"…\"`), got {other:?}")
        }
        None => bail!("a `praxis-deed` must carry `:beholding-chora`"),
    }

    let artefact = node.clause("artefact");
    let app = node.clause("app");
    let compliance = node.clause("compliance");

    let mut scalars: Vec<(String, String)> = Vec::new();
    // Insertion order matches the legacy block so that anything
    // iterating `scalars` sees the same sequence for either dialect.
    push_scalar(&mut scalars, "id", node.str_field("canonical-name"));
    push_scalar(
        &mut scalars,
        "type",
        artefact.and_then(|c| c.str_field("type")),
    );
    push_scalar(
        &mut scalars,
        "version",
        artefact.and_then(|c| c.str_field("version")),
    );
    push_scalar(
        &mut scalars,
        "app-name",
        app.and_then(|c| c.str_field("name")),
    );
    push_scalar(
        &mut scalars,
        "app-display",
        app.and_then(|c| c.str_field("display")),
    );
    push_scalar(
        &mut scalars,
        "app-url",
        app.and_then(|c| c.str_field("url")),
    );
    push_scalar(
        &mut scalars,
        "runtime-kind",
        app.and_then(|c| c.str_field("runtime-kind")),
    );
    push_scalar(
        &mut scalars,
        "standard-spec-version",
        compliance.and_then(|c| c.str_field("standard-version")),
    );
    push_scalar(
        &mut scalars,
        "generator",
        artefact.and_then(|c| c.str_field("generator")),
    );

    let mut lists: Vec<(String, Vec<String>)> = Vec::new();
    if let Some(items) = compliance.and_then(|c| c.field("standards")) {
        let Some(raw) = items.as_list() else {
            bail!("`(compliance :standards …)` must be a list, got {items:?}");
        };
        // `str_list` skips non-strings silently. Compare the counts so a
        // symbol or integer smuggled into the list is reported rather
        // than quietly dropped from the compliance claim.
        let strs = items.str_list();
        if strs.len() != raw.len() {
            bail!(
                "`(compliance :standards …)` must hold only strings; {} of {} entries are not",
                raw.len() - strs.len(),
                raw.len()
            );
        }
        lists.push((
            "standards-compliance".to_string(),
            strs.into_iter().map(|s| s.to_string()).collect(),
        ));
    }

    Ok((scalars, lists))
}

/// Push `key` only when the deed actually carried a value for it.
///
/// A free function rather than a closure so it can borrow `out`
/// mutably once per call instead of holding it for the whole block.
fn push_scalar(out: &mut Vec<(String, String)>, key: &str, val: Option<&str>) {
    if let Some(v) = val {
        out.push((key.to_string(), v.to_string()));
    }
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

    // Phase 1 reads both dialects but rewrites only the legacy one.
    // This scanner looks for `key = "value"` and a quoted span; handed a
    // deed form it would match nothing, or worse, the wrong quotes.
    // Refusing is the honest outcome — the caller gets an error, never a
    // corrupted launcher.
    if block.is_deed() {
        bail!(
            "`{}` blocks are read-only: `rewrite_scalar` edits `key = \"value\"` lines \
             and cannot safely edit a DEED form — regenerate the launcher instead",
            DEED_BEGIN
        );
    }
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
# SPDX-License-Identifier: MPL-2.0
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

    // ---------------------------------------------------------------
    // Phase 1 — the compat reader (launch-scaffolder #40)
    // ---------------------------------------------------------------

    /// The launcher captured from today's `mint`, committed so the
    /// backwards-compatibility claim is checked against a real artefact
    /// rather than against a string written by the same hand that wrote
    /// the parser. Generating the fixture after the emitter changes
    /// would assert the new emitter against the new parser, which passes
    /// regardless of whether compatibility survived.
    const LEGACY_FIXTURE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/metadata_block/minted-2026-09-22_stapeln-launcher.sh"
    );

    /// A DEED-dialect block carrying **exactly** the fixture's values.
    /// The content is deliberately identical so that comparing the two
    /// isolates the reader: any difference is the parser's, not the data's.
    const DEED_SAMPLE: &str = r##"#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
#
# @launcher-deed begin
# ;; SPDX-License-Identifier: MPL-2.0
# (praxis-deed
#   :schema-version  "1.0.0"
#   :canonical-name  "stapeln-launcher"
#   :beholding-chora #u5"estate/chora"
#   (artefact :type "launcher" :version "0.1.0"
#             :generator "launch-scaffolder")
#   (app :name "stapeln" :display "Stapeln"
#        :url "http://localhost:4010" :runtime-kind "server-url")
#   (compliance :standard-version "0.4.0"
#               :standards ("launcher-standard.adoc"
#                           "LM-LA-LIFECYCLE-STANDARD.adoc"
#                           "cross-platform-system-integration-modes")))
# @launcher-deed end

echo hi
"##;

    /// Replace a known fragment of the DEED sample to exercise malformed inputs.
    fn deed_sample_with(find: &str, replace: &str) -> String {
        assert!(
            DEED_SAMPLE.contains(find),
            "test would be vacuous: {find:?} is not in DEED_SAMPLE"
        );
        DEED_SAMPLE.replace(find, replace)
    }

    /// The committed pre-phase launcher remains readable by the legacy parser.
    #[test]
    fn the_committed_legacy_fixture_still_parses() {
        let text = std::fs::read_to_string(LEGACY_FIXTURE)
            .unwrap_or_else(|e| panic!("reading {LEGACY_FIXTURE}: {e}"));
        let block = parse_from_text(&text)
            .unwrap()
            .expect("fixture has a block");

        assert!(!block.is_deed(), "the fixture is the legacy dialect");
        assert_eq!(block.scalar("id"), Some("stapeln-launcher"));
        assert_eq!(block.scalar("app-name"), Some("stapeln"));
        assert_eq!(block.scalar("app-display"), Some("Stapeln"));
        assert_eq!(block.scalar("app-url"), Some("http://localhost:4010"));
        assert_eq!(block.scalar("generator"), Some("launch-scaffolder"));
        assert_eq!(block.scalar("standard-spec-version"), Some("0.4.0"));
        assert_eq!(
            block.list("standards-compliance"),
            Some(
                &[
                    "launcher-standard.adoc".to_string(),
                    "LM-LA-LIFECYCLE-STANDARD.adoc".to_string(),
                    "cross-platform-system-integration-modes".to_string(),
                ][..]
            )
        );
        assert_eq!(
            block.missing_required(),
            Vec::<&'static str>::new(),
            "the fixture must satisfy every required key"
        );
    }

    /// A DEED block exposes the same required scalar and list keys.
    #[test]
    fn a_deed_dialect_block_parses() {
        let block = parse_from_text(DEED_SAMPLE)
            .unwrap()
            .expect("block present");
        assert!(block.is_deed());
        assert_eq!(block.scalar("id"), Some("stapeln-launcher"));
        assert_eq!(block.scalar("type"), Some("launcher"));
        assert_eq!(block.scalar("runtime-kind"), Some("server-url"));
        assert_eq!(block.list("standards-compliance").map(<[_]>::len), Some(3));
        assert_eq!(block.missing_required(), Vec::<&'static str>::new());
    }

    /// The compat guarantee, stated as an equality rather than as two
    /// separate lists of assertions: every existing caller reads through
    /// `scalars` / `lists`, so if those match, no caller can tell the
    /// dialects apart.
    #[test]
    fn both_dialects_flatten_to_the_same_values() {
        let legacy_text = std::fs::read_to_string(LEGACY_FIXTURE).unwrap();
        let legacy = parse_from_text(&legacy_text).unwrap().unwrap();
        let deed_block = parse_from_text(DEED_SAMPLE).unwrap().unwrap();

        assert_eq!(
            legacy.scalars, deed_block.scalars,
            "scalar key/value sequence must be identical across dialects"
        );
        assert_eq!(
            legacy.lists, deed_block.lists,
            "list key/value sequence must be identical across dialects"
        );
    }

    /// Reject scripts with conflicting legacy and DEED metadata blocks.
    #[test]
    fn a_script_carrying_both_dialects_is_rejected() {
        let legacy_text = std::fs::read_to_string(LEGACY_FIXTURE).unwrap();
        let both = format!("{legacy_text}\n{DEED_SAMPLE}");
        let err = format!("{:#}", parse_from_text(&both).unwrap_err());
        assert!(err.contains("BOTH"), "unexpected error: {err}");
    }

    /// Refuse in-place edits to a DEED block instead of corrupting its syntax.
    #[test]
    fn rewrite_scalar_refuses_a_deed_block_rather_than_corrupting_it() {
        let err = format!(
            "{:#}",
            rewrite_scalar(DEED_SAMPLE, "version", "9.9.9").unwrap_err()
        );
        assert!(err.contains("read-only"), "unexpected error: {err}");
    }

    /// Require a chora reference in every embedded praxis deed.
    #[test]
    fn a_deed_block_without_beholding_chora_is_rejected() {
        let text = deed_sample_with("#   :beholding-chora #u5\"estate/chora\"\n", "");
        let err = format!("{:#}", parse_from_text(&text).unwrap_err());
        assert!(err.contains("beholding-chora"), "unexpected error: {err}");
    }

    /// Reject a chora reference that is not a UUID5 literal.
    #[test]
    fn a_deed_block_with_a_non_uuid5_chora_is_rejected() {
        let text = deed_sample_with(
            ":beholding-chora #u5\"estate/chora\"",
            ":beholding-chora \"estate/chora\"",
        );
        let err = format!("{:#}", parse_from_text(&text).unwrap_err());
        assert!(err.contains("uuid5"), "unexpected error: {err}");
    }

    /// `Value::str_list` skips non-strings silently, so a symbol or
    /// integer smuggled into `:standards` would vanish from the
    /// compliance claim rather than being reported. The reader compares
    /// the string count against the list count for exactly this case;
    /// without this test that comparison is dead code.
    #[test]
    fn a_non_string_entry_in_standards_is_reported_not_silently_dropped() {
        let text = deed_sample_with(r#""LM-LA-LIFECYCLE-STANDARD.adoc""#, "42");
        let err = format!("{:#}", parse_from_text(&text).unwrap_err());
        assert!(
            err.contains("only strings"),
            "expected a strictness error naming the non-string entry, got: {err}"
        );
    }

    /// Reject embedded deeds whose root form is not `praxis-deed`.
    #[test]
    fn a_deed_block_that_is_not_a_praxis_deed_is_rejected() {
        let text = deed_sample_with("(praxis-deed", "(repo-deed");
        let err = format!("{:#}", parse_from_text(&text).unwrap_err());
        assert!(err.contains("praxis-deed"), "unexpected error: {err}");
    }

    /// Proof that the DEED text really is handed to `deed::parse` and
    /// not to a second scanner living in this file: a tab is invalid
    /// *anywhere* in a deed, which is a rule only the real grammar
    /// knows. A hand-rolled scanner here would happily accept this.
    #[test]
    fn the_real_deed_grammar_is_the_one_enforcing_the_block() {
        let text = deed_sample_with(
            ":canonical-name  \"stapeln-launcher\"",
            ":canonical-name\t\"stapeln-launcher\"",
        );
        let err = format!("{:#}", parse_from_text(&text).unwrap_err());
        assert!(err.contains("HTAB"), "unexpected error: {err}");
    }

    /// The `;;` SPDX header is mandatory and must sit at byte zero of
    /// the deed text, so `uncomment` has to remove the `#` *and* the one
    /// space after it — no more, or the block's indentation is eaten.
    #[test]
    fn uncomment_strips_the_hash_and_exactly_one_space() {
        let body = vec![
            "# ;; SPDX-License-Identifier: MPL-2.0".to_string(),
            "#   :schema-version \"1.0.0\"".to_string(),
            "#".to_string(),
        ];
        assert_eq!(
            uncomment(&body).unwrap(),
            ";; SPDX-License-Identifier: MPL-2.0\n  :schema-version \"1.0.0\"\n\n"
        );
    }

    /// Require every embedded DEED line to retain its shell comment prefix.
    #[test]
    fn a_deed_block_line_without_a_hash_prefix_is_rejected() {
        let text = deed_sample_with("#   :canonical-name", "   :canonical-name");
        let err = format!("{:#}", parse_from_text(&text).unwrap_err());
        assert!(
            err.contains("not a `#` comment line"),
            "unexpected error: {err}"
        );
    }
}
