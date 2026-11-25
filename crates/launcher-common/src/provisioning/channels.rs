// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! Structured edit of a repository's `channels.scm`: make sure the estate Guix
//! channel is in the channel list, without disturbing anything else.
//!
//! `channels.scm` is minted once and is then repo-owned (the repository re-pins
//! its own `guix` channel with `just toolchain-refresh`), so realign cannot
//! overwrite it. This module is the one sanctioned way to evolve it: the file is
//! parsed as Scheme data (comments, strings and nesting honoured), the channel
//! list is read as a list of channel records, and the estate entry is inserted
//! as one new list element. Every other byte of the file is left exactly as it
//! was, so the repository's own `guix` pin, its comments and any other channels
//! survive. Nothing here concatenates text blindly: the insertion point is the
//! end of the last parsed element of the parsed `(list ...)` form.
//!
//! Refusals are loud and leave the file untouched: a file that is not a single
//! `(list (channel ...) ...)` form, a channel without a readable name or url,
//! and an entry that claims the estate's name or url but differs from the
//! canon's entry (a stale or hand-edited pin is the owner's to resolve; the
//! estate pin moves only when the canon `channels.scm` changes).
//!
//! The estate entry comes from the canon's own `channels.scm` when that file
//! carries a channel named `estate`; until the canon is re-vendored with one it
//! falls back to [`ChannelSpec::builtin_estate`].

use anyhow::{Result, anyhow, bail};

/// Name of the estate channel.
pub const ESTATE_NAME: &str = "estate";

/// A channel as far as this module needs to understand it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelSpec {
    pub name: String,
    pub url: String,
    pub branch: Option<String>,
    pub commit: Option<String>,
    /// (introduction commit, fingerprint with whitespace removed, upper-cased)
    pub introduction: Option<(String, String)>,
}

impl ChannelSpec {
    /// The built-in estate channel (owner decision, 2026-11-25).
    pub fn builtin_estate() -> Self {
        ChannelSpec {
            name: ESTATE_NAME.into(),
            url: "https://github.com/hyperpolymath/guix-channel".into(),
            branch: Some("main".into()),
            commit: Some("d4c84bde7a728a4a5bbc6bf865ed45e983d01c6d".into()),
            introduction: Some((
                "4b91d37a4bbb4bc8c16de791eb5bd2241fb26f4b".into(),
                norm_fp("E0B3 6C1B 28F6 E9E0 A460  3F47 FE6E 34E9 F311 0F92"),
            )),
        }
    }

    /// Render as a `(channel ...)` form, in the layout of the canon file. The
    /// first line carries no indentation; `indent` is prefixed to the rest.
    fn render(&self, indent: &str) -> String {
        let mut s = String::from("(channel\n");
        s.push_str(&format!("{indent} (name '{})\n", self.name));
        s.push_str(&format!("{indent} (url {})", quote(&self.url)));
        if let Some(b) = &self.branch {
            s.push_str(&format!("\n{indent} (branch {})", quote(b)));
        }
        if let Some(c) = &self.commit {
            s.push_str(&format!("\n{indent} (commit {})", quote(c)));
        }
        match &self.introduction {
            Some((c, fp)) => s.push_str(&format!(
                "\n{indent} (introduction\n{indent}  (make-channel-introduction\n{indent}   {}\n{indent}   (openpgp-fingerprint\n{indent}    {}))))",
                quote(c),
                quote(&group_fp(fp))
            )),
            None => s.push(')'),
        }
        s
    }
}

/// What [`ensure_estate`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The estate entry was already present and identical: file unchanged.
    Present,
    /// The estate entry was appended: the new file text.
    Added(String),
}

/// The estate entry the canon prescribes: the `estate` channel of the canon's
/// own `channels.scm` if it has one, else the built-in.
pub fn estate_from_canon(canon_channels_scm: &str) -> Result<ChannelSpec> {
    let parsed = parse_channels(canon_channels_scm)?;
    Ok(parsed
        .channels
        .into_iter()
        .map(|c| c.spec)
        .find(|c| c.name == ESTATE_NAME)
        .unwrap_or_else(ChannelSpec::builtin_estate))
}

/// Ensure `text` (a repository's `channels.scm`) lists `estate`.
/// Idempotent: an identical existing entry yields [`Outcome::Present`].
/// Errors, leaving the caller to leave the file untouched, when the text is not
/// a single channel list or an existing entry conflicts with `estate`.
pub fn ensure_estate(text: &str, estate: &ChannelSpec) -> Result<Outcome> {
    let parsed = parse_channels(text)?;
    let want_url = norm_url(&estate.url);
    for c in &parsed.channels {
        let same_name = c.spec.name == estate.name;
        let same_url = norm_url(&c.spec.url) == want_url;
        if same_name || same_url {
            if c.spec == *estate {
                return Ok(Outcome::Present);
            }
            bail!(
                "conflicting estate channel: existing entry (name '{}, url {}, commit {}) \
                 does not match the canon's (name '{}, url {}, commit {}); \
                 resolve it by hand, realign will not rewrite it",
                c.spec.name,
                c.spec.url,
                c.spec.commit.as_deref().unwrap_or("<none>"),
                estate.name,
                estate.url,
                estate.commit.as_deref().unwrap_or("<none>"),
            );
        }
    }
    // Insert after the last element, indented to line up with the first
    // existing channel (column of its opening paren on its own line).
    let (at, text_to_insert) = match parsed.channels.last() {
        Some(last) => {
            let first = &parsed.channels[0];
            let col = column_of(text, first.start);
            let indent = " ".repeat(col);
            (last.end, format!("\n{indent}{}", estate.render(&indent)))
        }
        None => (
            parsed.list_head_end,
            format!(" {}", estate.render("       ")),
        ),
    };
    let mut out = String::with_capacity(text.len() + text_to_insert.len());
    out.push_str(&text[..at]);
    out.push_str(&text_to_insert);
    out.push_str(&text[at..]);
    // The edit must itself parse and now contain the estate, exactly.
    let check = parse_channels(&out)
        .map_err(|e| anyhow!("internal error: edited channels.scm does not parse: {e}"))?;
    if !check.channels.iter().any(|c| c.spec == *estate) {
        bail!("internal error: edited channels.scm does not contain the estate entry");
    }
    Ok(Outcome::Added(out))
}

// ---------------------------------------------------------------------------
// A small Scheme-data reader: lists, strings, atoms, `;` and `#| |#` comments.

#[derive(Debug)]
enum Sexp {
    List {
        items: Vec<Sexp>,
        start: usize,
        end: usize,
    },
    Atom(String),
    Str(String),
}

struct Parsed {
    channels: Vec<Channel>,
    /// Byte offset just after the `list` atom, for inserting into an empty list.
    list_head_end: usize,
}

struct Channel {
    spec: ChannelSpec,
    start: usize,
    end: usize,
}

struct Reader<'a> {
    s: &'a str,
    b: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    fn skip_ws(&mut self) -> Result<()> {
        loop {
            match self.b.get(self.i) {
                Some(c) if c.is_ascii_whitespace() => self.i += 1,
                Some(b';') => {
                    while self.i < self.b.len() && self.b[self.i] != b'\n' {
                        self.i += 1;
                    }
                }
                Some(b'#') if self.b.get(self.i + 1) == Some(&b'|') => {
                    let mut depth = 1;
                    self.i += 2;
                    while depth > 0 {
                        match (self.b.get(self.i), self.b.get(self.i + 1)) {
                            (Some(b'|'), Some(b'#')) => {
                                depth -= 1;
                                self.i += 2;
                            }
                            (Some(b'#'), Some(b'|')) => {
                                depth += 1;
                                self.i += 2;
                            }
                            (Some(_), _) => self.i += 1,
                            (None, _) => bail!("unterminated #| comment"),
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn datum(&mut self) -> Result<Sexp> {
        self.skip_ws()?;
        let start = self.i;
        match self.b.get(self.i) {
            None => bail!("unexpected end of input"),
            Some(b'(') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_ws()?;
                    match self.b.get(self.i) {
                        None => bail!(
                            "unbalanced parentheses: list opened at byte {start} is never closed"
                        ),
                        Some(b')') => {
                            self.i += 1;
                            return Ok(Sexp::List {
                                items,
                                start,
                                end: self.i,
                            });
                        }
                        Some(_) => items.push(self.datum()?),
                    }
                }
            }
            Some(b')') => bail!("unbalanced parentheses: stray `)` at byte {start}"),
            Some(b'"') => {
                self.i += 1;
                let mut out = String::new();
                loop {
                    match self.b.get(self.i) {
                        None => bail!("unterminated string starting at byte {start}"),
                        Some(b'"') => {
                            self.i += 1;
                            return Ok(Sexp::Str(out));
                        }
                        Some(b'\\') => {
                            let c = self
                                .s
                                .get(self.i + 1..)
                                .and_then(|r| r.chars().next())
                                .ok_or_else(|| anyhow!("dangling escape in string"))?;
                            out.push(c);
                            self.i += 1 + c.len_utf8();
                        }
                        Some(_) => {
                            let c = self.s[self.i..].chars().next().unwrap();
                            out.push(c);
                            self.i += c.len_utf8();
                        }
                    }
                }
            }
            Some(_) => {
                while let Some(c) = self.b.get(self.i) {
                    if c.is_ascii_whitespace() || matches!(c, b'(' | b')' | b'"' | b';') {
                        break;
                    }
                    self.i += 1;
                }
                Ok(Sexp::Atom(self.s[start..self.i].to_string()))
            }
        }
    }
}

fn parse_channels(text: &str) -> Result<Parsed> {
    let mut r = Reader {
        s: text,
        b: text.as_bytes(),
        i: 0,
    };
    let top = r
        .datum()
        .map_err(|e| anyhow!("channels.scm is not parseable Scheme data: {e}"))?;
    r.skip_ws()
        .and_then(|_| {
            if r.i < r.b.len() {
                bail!("more than one top-level form (expected a single `(list (channel ...) ...)`)")
            }
            Ok(())
        })
        .map_err(|e| anyhow!("channels.scm: {e}"))?;
    let Sexp::List { items, .. } = &top else {
        bail!("channels.scm: top-level form is not a list");
    };
    // Position just after the `list` atom: find it by re-scanning from the list start.
    let list_head_end = match &top {
        Sexp::List { start, .. } => {
            let mut rr = Reader {
                s: text,
                b: text.as_bytes(),
                i: start + 1,
            };
            let _ = rr.skip_ws();
            let _ = rr.datum();
            rr.i
        }
        _ => 0,
    };
    match items.first() {
        Some(Sexp::Atom(a)) if a == "list" => {}
        _ => bail!("channels.scm: top-level form must be `(list (channel ...) ...)`"),
    }
    let mut channels = Vec::new();
    for item in &items[1..] {
        let Sexp::List {
            items: ci,
            start,
            end,
        } = item
        else {
            bail!("channels.scm: list element is not a `(channel ...)` form");
        };
        match ci.first() {
            Some(Sexp::Atom(a)) if a == "channel" => {}
            _ => bail!("channels.scm: list element at byte {start} is not a `(channel ...)` form"),
        }
        channels.push(Channel {
            spec: read_channel(&ci[1..], *start)?,
            start: *start,
            end: *end,
        });
    }
    Ok(Parsed {
        channels,
        list_head_end,
    })
}

fn field<'a>(fields: &'a [Sexp], key: &str) -> Option<&'a [Sexp]> {
    fields.iter().find_map(|f| match f {
        Sexp::List { items, .. } => match items.first() {
            Some(Sexp::Atom(a)) if a == key => Some(&items[1..]),
            _ => None,
        },
        _ => None,
    })
}

fn string_field(fields: &[Sexp], key: &str) -> Option<String> {
    match field(fields, key)?.first()? {
        Sexp::Str(s) => Some(s.clone()),
        _ => None,
    }
}

fn read_channel(fields: &[Sexp], at: usize) -> Result<ChannelSpec> {
    let name = match field(fields, "name").and_then(|v| v.first()) {
        Some(Sexp::Atom(a)) => a.trim_start_matches('\'').to_string(),
        _ => bail!("channels.scm: channel at byte {at} has no readable `(name 'x)`"),
    };
    let url = string_field(fields, "url")
        .ok_or_else(|| anyhow!("channels.scm: channel '{name} has no readable `(url \"...\")`"))?;
    let introduction = field(fields, "introduction").and_then(|v| match v.first()? {
        Sexp::List { items, .. } => {
            let commit = items.iter().find_map(|i| match i {
                Sexp::Str(s) => Some(s.clone()),
                _ => None,
            })?;
            let fp = items.iter().find_map(|i| match i {
                Sexp::List { items, .. } => match (items.first(), items.get(1)) {
                    (Some(Sexp::Atom(a)), Some(Sexp::Str(s))) if a == "openpgp-fingerprint" => {
                        Some(norm_fp(s))
                    }
                    _ => None,
                },
                _ => None,
            })?;
            Some((commit, fp))
        }
        _ => None,
    });
    Ok(ChannelSpec {
        name,
        url,
        branch: string_field(fields, "branch"),
        commit: string_field(fields, "commit"),
        introduction,
    })
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn norm_url(u: &str) -> String {
    u.trim()
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_ascii_lowercase()
}

fn norm_fp(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase()
}

/// Group a 40-hex fingerprint as Guix prints it: ten groups of four, with a
/// double space in the middle.
fn group_fp(fp: &str) -> String {
    let g: Vec<&str> = (0..fp.len())
        .step_by(4)
        .map(|i| &fp[i..(i + 4).min(fp.len())])
        .collect();
    if g.len() == 10 {
        format!("{}  {}", g[..5].join(" "), g[5..].join(" "))
    } else {
        g.join(" ")
    }
}

fn column_of(text: &str, at: usize) -> usize {
    let line_start = text[..at].rfind('\n').map_or(0, |n| n + 1);
    text[line_start..at].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUIX_ONLY: &str =
        include_str!("../../../../standards/provisioning/templates/guix/channels.scm");

    fn estate() -> ChannelSpec {
        ChannelSpec::builtin_estate()
    }

    fn added(text: &str) -> String {
        match ensure_estate(text, &estate()).expect("ensure") {
            Outcome::Added(t) => t,
            Outcome::Present => panic!("expected Added"),
        }
    }

    #[test]
    fn adds_estate_after_guix_and_keeps_every_other_byte() {
        let out = added(GUIX_ONLY);
        // Everything up to the guix channel's last `)` is untouched.
        let keep = GUIX_ONLY.trim_end().len() - 1;
        assert!(out.starts_with(&GUIX_ONLY[..keep]));
        assert!(out.contains("(commit \"ae77aeb9543de2661d739104bc4d3803d8c6f38b\")"));
        assert!(out.contains("(url \"https://github.com/hyperpolymath/guix-channel\")"));
        assert!(out.contains("\"E0B3 6C1B 28F6 E9E0 A460  3F47 FE6E 34E9 F311 0F92\""));
        let p = parse_channels(&out).unwrap();
        let names: Vec<_> = p.channels.iter().map(|c| c.spec.name.as_str()).collect();
        assert_eq!(names, ["guix", "estate"]);
        assert_eq!(p.channels[1].spec, estate());
    }

    #[test]
    fn second_pass_is_a_no_op() {
        let once = added(GUIX_ONLY);
        assert_eq!(ensure_estate(&once, &estate()).unwrap(), Outcome::Present);
    }

    #[test]
    fn keeps_a_repos_own_pin_and_other_channels() {
        let src = format!(
            "{}\n(list (channel (name 'guix) (url \"https://codeberg.org/guix/guix.git\") (commit \"{}\"))\n      (channel (name 'other) (url \"https://example.org/o.git\") (commit \"cc\")))\n",
            ";; Canon pin, verified",
            "b".repeat(40)
        );
        let out = added(&src);
        let p = parse_channels(&out).unwrap();
        let names: Vec<_> = p.channels.iter().map(|c| c.spec.name.as_str()).collect();
        assert_eq!(names, ["guix", "other", "estate"]);
        assert_eq!(
            p.channels[0].spec.commit.as_deref(),
            Some("b".repeat(40).as_str())
        );
        assert!(out.contains(";; Canon pin, verified"));
    }

    #[test]
    fn conflicting_entry_by_name_or_url_fails_loudly() {
        let stale =
            added(GUIX_ONLY).replace("d4c84bde7a728a4a5bbc6bf865ed45e983d01c6d", &"a".repeat(40));
        let e = ensure_estate(&stale, &estate()).unwrap_err().to_string();
        assert!(e.contains("conflicting estate channel"), "{e}");
        let by_url = "(list (channel (name 'mine) (url \"https://github.com/hyperpolymath/guix-channel.git\")))\n";
        assert!(ensure_estate(by_url, &estate()).is_err());
    }

    #[test]
    fn unparseable_or_foreign_shapes_are_refused() {
        for bad in [
            "",
            "(list (channel (name 'guix)",
            "(list (channel (name 'guix) (url \"u\"))))",
            "(cons* (channel (name 'guix) (url \"u\")) %default-channels)",
            "(list (channel (name 'guix) (url \"u\"))) (list)",
            "(list (channel (url \"u\")))",
            "(list 42)",
            "(list (channel (name 'guix) (url \"unterminated)))",
        ] {
            assert!(
                ensure_estate(bad, &estate()).is_err(),
                "should refuse {bad:?}"
            );
        }
    }

    #[test]
    fn comments_and_parens_in_strings_do_not_confuse_the_reader() {
        let src = "#| block ( |#\n;; ) line\n(list (channel (name 'guix) (url \"https://x/)(.git\") (commit \"aa\")))\n";
        let out = added(src);
        assert!(out.contains("block ("));
        assert_eq!(parse_channels(&out).unwrap().channels.len(), 2);
    }

    #[test]
    fn empty_list_gets_the_entry() {
        let out = added("(list)\n");
        assert_eq!(parse_channels(&out).unwrap().channels.len(), 1);
    }

    #[test]
    fn canon_without_estate_falls_back_to_builtin_and_with_estate_wins() {
        assert_eq!(estate_from_canon(GUIX_ONLY).unwrap(), estate());
        let mut custom = estate();
        custom.commit = Some("e".repeat(40));
        let canon = ensure_estate(GUIX_ONLY, &custom).unwrap();
        let Outcome::Added(canon) = canon else {
            panic!()
        };
        assert_eq!(estate_from_canon(&canon).unwrap(), custom);
    }
}
