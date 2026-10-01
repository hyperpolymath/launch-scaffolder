// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! Insert the README `[[ai-install]]` section (`PROVISIONING-STANDARD.adoc` §1:
//! *inserted*, never regenerated).
//!
//! The rendered `README-ai-install.adoc.tmpl` has two parts: a TIP that goes
//! straight after the document header, and the `[[ai-install]]` section that
//! goes before the first level-2 section. Once present the section belongs to
//! the README and is never touched again; a README that already has an
//! "AI-Assisted Installation" section of its own only gains the anchor, so the
//! launcher's `ai-setup` can read its sentence.

use super::mint::Act;
use anyhow::{Context, Result};
use std::path::Path;

const ANCHOR: &str = "[[ai-install]]";

/// Insert `rendered` (the template with the repo's values) into the README.
pub fn insert(target: &Path, rendered: &str) -> Result<Act> {
    let path = target.join("README.adoc");
    if !path.is_file() {
        let why = if target.join("README.md").is_file() {
            "README.md: the section is AsciiDoc; add it by hand"
        } else {
            "no README.adoc to insert the AI-install section into"
        };
        return Ok(Act::Skipped(why.into()));
    }
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let Some(out) = merged(&text, rendered) else {
        return Ok(Act::Kept("README.adoc already has [[ai-install]]".into()));
    };
    std::fs::write(&path, &out.0).with_context(|| format!("writing {}", path.display()))?;
    Ok(Act::Replaced(out.1.into()))
}

/// The README with the section in place and what was done, or `None` when it
/// already has the anchor.
fn merged(text: &str, rendered: &str) -> Option<(String, &'static str)> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.iter().any(|l| l.trim() == ANCHOR) {
        return None;
    }
    let sections = level2_headings(&lines);

    // An existing section of the same purpose gains the anchor and nothing else.
    if let Some(&i) = sections.iter().find(|&&i| {
        lines[i]
            .to_ascii_lowercase()
            .starts_with("== ai-assisted install")
    }) {
        let mut out: Vec<&str> = lines.clone();
        out.insert(i, ANCHOR);
        return Some((
            join(&out),
            "anchor added to the existing AI-assisted section",
        ));
    }

    let (tip, section) = match rendered.split_once(&format!("\n{ANCHOR}")) {
        Some((t, s)) => (t.trim_end(), format!("{ANCHOR}{}", s.trim_end())),
        None => ("", rendered.trim_end().to_string()),
    };

    // The section goes before the first level-2 heading, above its own anchor
    // and attribute lines; with none it is appended.
    let at = sections.first().map_or(lines.len(), |&h| {
        let mut s = h;
        while s > 0 && (lines[s - 1].starts_with('[') || lines[s - 1].starts_with("//")) {
            s -= 1;
        }
        s
    });
    let head = header_end(&lines).min(at);

    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 80);
    out.extend(lines[..head].iter().map(|l| l.to_string()));
    if !tip.is_empty() {
        pad(&mut out);
        out.push(tip.to_string());
        out.push(String::new());
    }
    out.extend(lines[head..at].iter().map(|l| l.to_string()));
    pad(&mut out);
    out.push(section);
    out.push(String::new());
    out.extend(lines[at..].iter().map(|l| l.to_string()));
    let mut s = out.join("\n");
    while s.contains("\n\n\n") {
        s = s.replace("\n\n\n", "\n\n");
    }
    if !s.ends_with('\n') {
        s.push('\n');
    }
    Some((s, "AI-install section inserted"))
}

fn join(lines: &[&str]) -> String {
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// Ensure the output ends with a blank line before a new block.
fn pad(out: &mut Vec<String>) {
    if out.last().is_some_and(|l| !l.trim().is_empty()) {
        out.push(String::new());
    }
}

/// Indices of `== ` headings outside delimited blocks.
fn level2_headings(lines: &[&str]) -> Vec<usize> {
    let mut open: Option<&str> = None;
    let mut v = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_end();
        let delim = t.len() >= 4
            && ["-", ".", "=", "*", "+", "_", "/"]
                .iter()
                .any(|c| t.chars().all(|x| x.to_string() == *c))
            || t == "```"
            || t.starts_with("|===");
        if delim {
            match open {
                Some(o) if o == t => open = None,
                None => open = Some(t),
                _ => {}
            }
            continue;
        }
        if open.is_none() && t.starts_with("== ") {
            v.push(i);
        }
    }
    v
}

/// The line after the document header (`= Title` and the attribute/author lines
/// that follow it up to the first blank line); 0 when there is no title.
fn header_end(lines: &[&str]) -> usize {
    let Some(t) = lines.iter().position(|l| l.starts_with("= ")) else {
        return 0;
    };
    lines[t..]
        .iter()
        .position(|l| l.trim().is_empty())
        .map_or(lines.len(), |p| t + p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: &str = "[TIP]\n====\nsay it\n====\n\n[[ai-install]]\n== AI-Assisted Installation (Recommended)\n\nbody\n";

    #[test]
    fn tip_after_header_section_before_first_heading() {
        let src = "// SPDX\n= Title\n:toc:\n\nIntro.\n\n[#usage]\n== Usage\n\n----\n== not a heading\n----\n";
        let (out, _) = merged(src, R).unwrap();
        let tip = out.find("[TIP]").unwrap();
        let intro = out.find("Intro.").unwrap();
        let anchor = out.find("[[ai-install]]").unwrap();
        let usage = out.find("[#usage]").unwrap();
        assert!(
            out.find(":toc:").unwrap() < tip && tip < intro && intro < anchor && anchor < usage
        );
        assert!(!out.contains("\n\n\n"));
        assert!(merged(&out, R).is_none(), "second run must be a no-op");
    }

    #[test]
    fn an_existing_section_only_gains_the_anchor() {
        let src = "= T\n\n== AI-Assisted Installation\n\nSay X.\n";
        let (out, what) = merged(src, R).unwrap();
        assert_eq!(
            out,
            "= T\n\n[[ai-install]]\n== AI-Assisted Installation\n\nSay X.\n"
        );
        assert!(what.contains("anchor"));
    }

    #[test]
    fn headings_inside_blocks_are_ignored_and_no_heading_appends() {
        let src = "= T\n\n....\n== x\n....\n";
        let (out, _) = merged(src, R).unwrap();
        assert!(out.trim_end().ends_with("body"));
    }
}
