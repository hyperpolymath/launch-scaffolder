// SPDX-License-Identifier: MPL-2.0
//! DEED reader — a hand-rolled tokenizer and recursive-descent parser for the
//! estate's `.deed` format.
//!
//! The normative grammar is `1-formats/deed/spec/abnf/deed.abnf` (DEED v1.0.0)
//! in `hyperpolymath/standards`. This module implements **syntax only**; the
//! praxis-level schema checks for `launcher-standard_praxis.deed` live in
//! [`crate::standard`]. Keeping the two apart is what lets the corpus fixtures
//! exercise the parser without dragging launcher semantics in.
//!
//! # Why hand-rolled
//!
//! The grammar is small and closed, and `launch-scaffolder` already vendors no
//! parser-generator. A `nom`/`pest` dependency would buy nothing here and would
//! put a third-party crate between us and a normative estate grammar.
//!
//! # The one ambiguity, and how it is resolved
//!
//! `(a b c)` is a **list of symbols** after a `:keyword`, and a **clause** with
//! head `a` in body position. The ABNF disambiguates by position, not by
//! lookahead:
//!
//! ```text
//! field  = keyword token-sep value          ; "(" here starts a list
//! clause = "(" symbol *(token-sep (field / clause)) [token-sep] ")"
//! ```
//!
//! So [`Parser::parse_value`] reads `(` as a list and [`Parser::parse_body`]
//! reads `(` as a clause. There is no backtracking anywhere in this parser.
//!
//! # Order is not semantic
//!
//! Per the grammar's closing note, file order carries no meaning in DEED.
//! Where precedence matters it is carried by an explicit `:priority` integer.
//! Consumers MUST sort; see [`Node::children_by_priority`].

use anyhow::{Result, bail};

/// A DEED value: the right-hand side of a `:keyword value` field, or an
/// element of a list.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `"…"` — a double-quoted string, escapes already decoded.
    Str(String),
    /// `10`, `-3`, `007`. Leading zeros are decimal, never octal.
    Int(i64),
    /// A bare symbol such as `nohup`, `yellow`, `MPL-2.0`, `Type.Software`.
    Sym(String),
    /// `#t` / `#f`. Strict lowercase; `true`/`false` are not booleans.
    Bool(bool),
    /// `#u5"name"` — carries the *name* input, not a computed UUID. Nothing in
    /// `launch-scaffolder` needs the digest, so none is derived here.
    Uuid5(String),
    /// `( … )` in value position, including the empty list `()`.
    List(Vec<Value>),
    /// `'sym` or `'(a b)`. Only symbols and lists may be quoted.
    Quoted(Box<Value>),
}

impl Value {
    /// The string payload, for `Str` only. A symbol is deliberately not
    /// coerced: `:mode "--auto"` and `:mode auto` are different documents.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The symbol name, for `Sym` only.
    pub fn as_sym(&self) -> Option<&str> {
        match self {
            Value::Sym(s) => Some(s),
            _ => None,
        }
    }

    /// The integer payload, for `Int` only.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The boolean payload, for `Bool` only.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The elements of a list, for `List` only.
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(v) => Some(v),
            _ => None,
        }
    }

    /// Every element of a list that is a string, as `&str`. Non-string
    /// elements are skipped rather than erroring — callers that care use
    /// [`Value::as_list`] directly.
    pub fn str_list(&self) -> Vec<&str> {
        match self {
            Value::List(v) => v.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// A human-readable type name, for error messages.
    fn kind(&self) -> &'static str {
        match self {
            Value::Str(_) => "string",
            Value::Int(_) => "integer",
            Value::Sym(_) => "symbol",
            Value::Bool(_) => "boolean",
            Value::Uuid5(_) => "uuid5",
            Value::List(_) => "list",
            Value::Quoted(_) => "quoted",
        }
    }
}

/// A DEED form or clause: a head symbol, its `:keyword value` fields, and its
/// nested clauses.
///
/// Fields and clauses are held separately because their relative order is not
/// semantic. Within `clauses`, source order is preserved so that a consumer
/// which sorts by `:priority` can be shown to differ from one which does not.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// The head symbol: `praxis-deed`, `resolution`, `path`, …
    pub head: String,
    pub fields: Vec<(String, Value)>,
    pub clauses: Vec<Node>,
}

impl Node {
    /// The value of the first `:keyword` field on this node, if present.
    pub fn field(&self, key: &str) -> Option<&Value> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The string payload of the first `:keyword` field, or `None` if the
    /// field is absent or has another value kind.
    pub fn str_field(&self, key: &str) -> Option<&str> {
        self.field(key).and_then(Value::as_str)
    }

    /// The first direct child clause with this head.
    pub fn clause(&self, head: &str) -> Option<&Node> {
        self.clauses.iter().find(|c| c.head == head)
    }

    /// Every direct child clause with this head, in source order.
    pub fn clauses_named<'a>(&'a self, head: &'a str) -> impl Iterator<Item = &'a Node> {
        self.clauses.iter().filter(move |c| c.head == head)
    }

    /// Every direct child clause with this head, **sorted ascending by its
    /// `:priority` integer**.
    ///
    /// This is the only correct way to read a DEED ladder. File order is not
    /// semantic (see the module docs), so a consumer that iterates `clauses`
    /// directly is relying on a lint convention rather than on the document.
    ///
    /// A child with no `:priority`, or a non-integer one, sorts after every
    /// child that has one; ties keep source order (the sort is stable).
    pub fn children_by_priority<'a>(&'a self, head: &'a str) -> Vec<&'a Node> {
        let mut out: Vec<&Node> = self.clauses_named(head).collect();
        out.sort_by_key(|c| {
            c.field("priority")
                .and_then(Value::as_int)
                .unwrap_or(i64::MAX)
        });
        out
    }
}

// ---------------------------------------------------------------------------
// Lexing / parsing
// ---------------------------------------------------------------------------

/// The four legal document heads, and the filename suffix each dispatches from.
/// Dispatch is exact-stem-first: `estate_chora.deed` is an `estate-deed`, never
/// a `repo-deed`, even though it also matches the `*_chora.deed` shape.
pub const DOC_HEADS: [&str; 4] = [
    "estate-deed",
    "repo-deed",
    "estate-atlas-deed",
    "praxis-deed",
];

struct Parser {
    src: Vec<char>,
    pos: usize,
    line: usize,
}

/// Parse a complete deed document and return its single top-level form.
///
/// The whole input must be consumed: trailing non-whitespace after the closing
/// `)` is a parse error, per the grammar's EOF enforcement note. The document
/// must also have an SPDX header and exactly one string-valued top-level
/// `:schema-version` field; any syntax or structural violation returns an
/// error.
pub fn parse(text: &str) -> Result<Node> {
    // A tab is invalid *anywhere* in a deed, not merely as a separator — this
    // matches `deed_lint.py`, which tests the raw text before lexing. A literal
    // tab inside a string is therefore also rejected; `\t` (two characters) is
    // the legal way to write one.
    if let Some(idx) = text.find('\t') {
        let line = text[..idx].matches('\n').count() + 1;
        bail!("line {line}: HTAB (tab) is an invalid separator anywhere in a deed (K9-consistent)");
    }

    let mut p = Parser {
        src: text.chars().collect(),
        pos: 0,
        line: 1,
    };
    p.parse_header()?;
    p.skip_sep()?;
    let form = p.parse_form()?;
    p.skip_sep()?;
    if p.pos < p.src.len() {
        let c = p.src[p.pos];
        bail!(
            "line {}: trailing content after the closing ')': {c:?} — a deed holds exactly one form",
            p.line
        );
    }

    // The ABNF states this as a side condition on `form`, not as a separate
    // production: "Exactly one field MUST have keyword ':schema-version' with a
    // STRING value." It is therefore part of DEED *syntax* and belongs here,
    // not in the praxis-schema layer — a document without it is not a deed at
    // all, whatever its doc-head.
    //
    // Note the value need only be a string, not a semver: the upstream corpus
    // carries :schema-version "not-string-issue" as a VALID fixture.
    let versions: Vec<&(String, Value)> = form
        .fields
        .iter()
        .filter(|(k, _)| k == "schema-version")
        .collect();
    match versions.as_slice() {
        [(_, Value::Str(_))] => {}
        [(_, other)] => bail!(
            "a deed form's :schema-version must be a STRING value, got {}",
            other.kind()
        ),
        other => bail!(
            "a deed form must carry exactly one :schema-version STRING field (found {})",
            other.len()
        ),
    }

    Ok(form)
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.src.get(self.pos + k).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn starts_with(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(i, c)| self.peek_at(i) == Some(c))
    }

    /// `header = 1*spdx-line`, `spdx-line = ";;" SP "SPDX-" 1*text-char line-end`.
    ///
    /// Only the leading run of SPDX lines is the header. Any further `;;` lines
    /// (the launcher deed has fourteen of them) are ordinary comments and are
    /// consumed later as `token-sep`.
    fn parse_header(&mut self) -> Result<()> {
        let mut seen = 0usize;
        while self.starts_with(";; SPDX-") {
            for _ in 0..8 {
                self.bump();
            }
            let mut payload = 0usize;
            loop {
                match self.peek() {
                    None => bail!("line {}: SPDX header line has no line-end", self.line),
                    Some('\n') => {
                        self.bump();
                        break;
                    }
                    Some('\r') => {
                        if self.peek_at(1) == Some('\n') {
                            self.bump();
                            self.bump();
                            break;
                        }
                        bail!(
                            "line {}: bare CR is not a line-end (CRLF or LF only)",
                            self.line
                        );
                    }
                    Some(_) => {
                        self.bump();
                        payload += 1;
                    }
                }
            }
            if payload == 0 {
                bail!(
                    "line {}: SPDX header line is empty after ';; SPDX-'",
                    self.line - 1
                );
            }
            seen += 1;
        }
        if seen == 0 {
            bail!("line 1: a deed must open with at least one ';; SPDX-…' header line");
        }
        Ok(())
    }

    /// `token-sep = 1*(SP / line-end / comment)`, but zero repetitions are
    /// tolerated here. Returns whether at least one separator was consumed so
    /// callers can enforce required separation.
    fn skip_sep(&mut self) -> Result<bool> {
        let start = self.pos;
        loop {
            match self.peek() {
                Some(' ') | Some('\n') => {
                    self.bump();
                }
                Some('\r') => {
                    if self.peek_at(1) == Some('\n') {
                        self.bump();
                        self.bump();
                    } else {
                        bail!(
                            "line {}: bare CR is not a line-end (CRLF or LF only)",
                            self.line
                        );
                    }
                }
                // `comment = ";" *text-char line-end`. A single ";" opens it,
                // so ";;" is a comment whose text begins with ";".
                Some(';') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                    // EOF closes a trailing comment; the grammar wants a
                    // line-end but rejecting here would only punish a missing
                    // final newline, which `deed_lint.py` also tolerates.
                    self.bump();
                }
                _ => return Ok(self.pos != start),
            }
        }
    }

    fn at_sep(&self) -> bool {
        matches!(self.peek(), Some(' ') | Some('\n') | Some('\r') | Some(';'))
    }

    /// `form = "(" doc-head 1*(token-sep (field / clause)) [token-sep] ")"`
    fn parse_form(&mut self) -> Result<Node> {
        if self.peek() != Some('(') {
            bail!("line {}: a deed form must start with '('", self.line);
        }
        self.bump();
        let head = self.lex_symbol()?;
        if !DOC_HEADS.contains(&head.as_str()) {
            bail!(
                "line {}: invalid doc-head {head:?}; valid heads: {}",
                self.line,
                DOC_HEADS.join(", ")
            );
        }
        if self.peek() != Some(')') && !self.at_sep() {
            bail!(
                "line {}: doc-head {head} must be followed by a separator before the first field",
                self.line
            );
        }
        let (fields, clauses) = self.parse_body(&head)?;
        if fields.is_empty() && clauses.is_empty() {
            bail!(
                "line {}: a deed form must carry at least one field or clause",
                self.line
            );
        }
        Ok(Node {
            head,
            fields,
            clauses,
        })
    }

    /// `clause = "(" symbol *(token-sep (field / clause)) [token-sep] ")"`
    ///
    /// Zero items is legal: `(deployment)` is a well-formed clause.
    fn parse_clause(&mut self) -> Result<Node> {
        debug_assert_eq!(self.peek(), Some('('));
        self.bump();
        // "clause '(' must be followed immediately by a clause symbol (no
        // separator)" — this is what keeps `( a b )` from being read as a
        // clause in body position.
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {}
            _ => bail!(
                "line {}: clause '(' must be followed immediately by a clause symbol (no separator)",
                self.line
            ),
        }
        let head = self.lex_symbol()?;
        if self.peek() != Some(')') && !self.at_sep() {
            bail!(
                "line {}: clause ({head}) head must be followed by a separator or ')'",
                self.line
            );
        }
        let (fields, clauses) = self.parse_body(&head)?;
        Ok(Node {
            head,
            fields,
            clauses,
        })
    }

    /// The shared body of a form and a clause: separator-delimited
    /// `:keyword value` fields and nested `(symbol …)` clauses, until the
    /// matching `)`.
    #[allow(clippy::type_complexity)]
    fn parse_body(&mut self, head: &str) -> Result<(Vec<(String, Value)>, Vec<Node>)> {
        let mut fields = Vec::new();
        let mut clauses = Vec::new();
        let mut parsed_item = false;
        loop {
            let had_sep = self.skip_sep()?;
            match self.peek() {
                None => bail!(
                    "line {}: unbalanced parens: ({head}) never closes",
                    self.line
                ),
                Some(')') => {
                    self.bump();
                    return Ok((fields, clauses));
                }
                Some(':') => {
                    if parsed_item && !had_sep {
                        bail!(
                            "line {}: fields and clauses in ({head}) must be separated by a separator",
                            self.line
                        );
                    }
                    let (k, v) = self.parse_field()?;
                    fields.push((k, v));
                    parsed_item = true;
                }
                // In BODY position "(" opens a clause, never a list.
                Some('(') => {
                    if parsed_item && !had_sep {
                        bail!(
                            "line {}: fields and clauses in ({head}) must be separated by a separator",
                            self.line
                        );
                    }
                    clauses.push(self.parse_clause()?);
                    parsed_item = true;
                }
                Some(c) => bail!(
                    "line {}: expected field (':keyword …') or clause ('(symbol …)') in ({head}), got {c:?}",
                    self.line
                ),
            }
        }
    }

    /// `field = keyword token-sep value`
    fn parse_field(&mut self) -> Result<(String, Value)> {
        debug_assert_eq!(self.peek(), Some(':'));
        self.bump();
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {}
            _ => bail!(
                "line {}: malformed keyword: ':' must be followed by a symbol",
                self.line
            ),
        }
        let key = self.lex_symbol()?;
        if !self.at_sep() {
            bail!(
                "line {}: keyword :{key} must be followed by a separator before its value",
                self.line
            );
        }
        self.skip_sep()?;
        let val = self.parse_value()?;
        // The grammar's boolean production is `#t` / `#f` only, and says
        // explicitly: "Never true, false, yes, no, 1, 0". A bare `true` lexes
        // as a perfectly legal symbol, so this is the rule that catches the
        // author who meant a boolean — it is a real check, not a formality.
        if let Value::Sym(s) = &val {
            if matches!(s.as_str(), "true" | "false" | "yes" | "no") {
                bail!(
                    "line {}: :{key} has bare symbol {s} — booleans are #t / #f only; \
                     true/false/yes/no are parse errors",
                    self.line
                );
            }
        }
        Ok((key, val))
    }

    /// `value = string / symbol / integer / boolean / uuid5 / quoted / list`
    fn parse_value(&mut self) -> Result<Value> {
        match self.peek() {
            None => bail!(
                "line {}: unexpected end of input, expected a value",
                self.line
            ),
            Some('"') => Ok(Value::Str(self.lex_string()?)),
            // In VALUE position "(" opens a list, never a clause.
            Some('(') => self.lex_list(),
            Some('#') => self.lex_hash(),
            Some('\'') => {
                self.bump();
                let inner = self.parse_value()?;
                match inner {
                    Value::Sym(_) | Value::List(_) => Ok(Value::Quoted(Box::new(inner))),
                    other => bail!(
                        "line {}: only symbols and lists may be quoted, not {}",
                        self.line,
                        other.kind()
                    ),
                }
            }
            Some(':') => bail!(
                "line {}: stray keyword — a keyword may only lead a field, not stand as a value",
                self.line
            ),
            Some(c) if c == '-' || c.is_ascii_digit() => self.lex_integer(),
            Some(c) if c.is_ascii_alphabetic() => Ok(Value::Sym(self.lex_symbol()?)),
            Some(c) => bail!(
                "line {}: cannot lex a value starting at {c:?} \
                 ('=' as a field separator is not a deed; '[section]' is not a deed)",
                self.line
            ),
        }
    }

    /// `list = "(" [value *(token-sep value)] [token-sep] ")"`
    fn lex_list(&mut self) -> Result<Value> {
        debug_assert_eq!(self.peek(), Some('('));
        self.bump();
        let mut items = Vec::new();
        let mut parsed_item = false;
        loop {
            let had_sep = self.skip_sep()?;
            match self.peek() {
                None => bail!("line {}: unbalanced parens: list never closes", self.line),
                Some(')') => {
                    self.bump();
                    return Ok(Value::List(items));
                }
                _ => {
                    if parsed_item && !had_sep {
                        bail!(
                            "line {}: list values must be separated by a separator",
                            self.line
                        );
                    }
                    items.push(self.parse_value()?);
                    parsed_item = true;
                }
            }
        }
    }

    /// `boolean = "#t" / "#f"` and `uuid5 = "#u5" string`.
    fn lex_hash(&mut self) -> Result<Value> {
        debug_assert_eq!(self.peek(), Some('#'));
        if self.starts_with("#u5") {
            for _ in 0..3 {
                self.bump();
            }
            if self.peek() != Some('"') {
                bail!(
                    "line {}: uuid5 must be followed immediately by a string: #u5\"name\"",
                    self.line
                );
            }
            return Ok(Value::Uuid5(self.lex_string()?));
        }
        let b = match (self.peek_at(1), self.peek_at(2)) {
            // A trailing identifier character means this is not a boolean at
            // all (`#true`), so it falls through to the catch-all below.
            (Some('t'), n) if !is_sym_continue(n) => true,
            (Some('f'), n) if !is_sym_continue(n) => false,
            _ => bail!(
                "line {}: unrecognised #-form: only #t, #f and #u5\"…\" are legal \
                 (#T and #F are invalid — booleans are strict lowercase)",
                self.line
            ),
        };
        self.bump();
        self.bump();
        Ok(Value::Bool(b))
    }

    /// `integer = ["-"] 1*DIGIT`. Leading zeros are decimal, never octal.
    fn lex_integer(&mut self) -> Result<Value> {
        let start = self.line;
        let mut s = String::new();
        if self.peek() == Some('-') {
            s.push('-');
            self.bump();
        }
        let mut digits = 0usize;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.bump();
                digits += 1;
            } else {
                break;
            }
        }
        if digits == 0 {
            bail!("line {start}: '-' must be followed by at least one digit");
        }
        if is_sym_continue(self.peek()) {
            bail!("line {start}: malformed token: number followed by identifier characters");
        }
        let n: i64 = s
            .parse()
            .map_err(|_| anyhow::anyhow!("line {start}: integer {s} does not fit in i64"))?;
        Ok(Value::Int(n))
    }

    /// `symbol = ALPHA *( ALPHA / DIGIT / "." / "*" / "/" / "<" / ">" / "=" /
    /// "!" / "?" / "+" / "-" / "_" )`
    fn lex_symbol(&mut self) -> Result<String> {
        match self.peek() {
            Some(c) if c.is_ascii_alphabetic() => {}
            Some(c) => bail!(
                "line {}: a symbol must start with a letter, got {c:?}",
                self.line
            ),
            None => bail!(
                "line {}: unexpected end of input, expected a symbol",
                self.line
            ),
        }
        let mut s = String::new();
        while is_sym_continue(self.peek()) {
            s.push(self.bump().expect("peeked"));
        }
        Ok(s)
    }

    /// `string = DQUOTE *( str-char / escape ) DQUOTE`, with exactly four
    /// escapes: `\"` `\\` `\n` `\t`. `\r` and `\uXXXX` are invalid.
    fn lex_string(&mut self) -> Result<String> {
        debug_assert_eq!(self.peek(), Some('"'));
        let start = self.line;
        self.bump();
        let mut out = String::new();
        loop {
            let c = match self.bump() {
                None => bail!("line {start}: unterminated string"),
                Some(c) => c,
            };
            match c {
                '"' => return Ok(out),
                '\\' => match self.bump() {
                    None => bail!("line {start}: dangling backslash"),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(bad) => bail!(
                        "line {}: invalid escape \\{bad} — exactly four escapes are legal: \
                         \\\" \\\\ \\n \\t",
                        self.line
                    ),
                },
                c if (c as u32) < 0x20 => bail!(
                    "line {}: raw control character U+{:04X} inside string (use legal escapes)",
                    self.line,
                    c as u32
                ),
                c => out.push(c),
            }
        }
    }
}

fn is_sym_continue(c: Option<char>) -> bool {
    matches!(
        c,
        Some(c) if c.is_ascii_alphanumeric()
            || matches!(c, '.' | '*' | '/' | '<' | '>' | '=' | '!' | '?' | '+' | '-' | '_')
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap a form body in a minimal legal document.
    fn doc(body: &str) -> String {
        format!(
            ";; SPDX-License-Identifier: MPL-2.0\n(repo-deed :schema-version \"1.0.0\" {body})\n"
        )
    }

    fn ok(body: &str) -> Node {
        parse(&doc(body)).unwrap_or_else(|e| panic!("expected {body:?} to parse, got: {e:#}"))
    }

    fn err(body: &str) -> String {
        match parse(&doc(body)) {
            Ok(_) => panic!("expected {body:?} to be rejected, but it parsed"),
            Err(e) => format!("{e:#}"),
        }
    }

    /// **The central design point.** The same four bytes `(a b)` are a LIST
    /// after a keyword and a CLAUSE in body position. Nothing but position
    /// distinguishes them, and getting this backwards is the single most
    /// likely way to write a DEED parser that is subtly wrong rather than
    /// obviously broken.
    #[test]
    fn paren_is_a_list_in_value_position_and_a_clause_in_body_position() {
        let as_value = ok(r#":names ("a" "b")"#);
        assert_eq!(
            as_value
                .field("names")
                .and_then(Value::as_list)
                .map(<[Value]>::len),
            Some(2),
            "after a keyword, '(' must open a LIST"
        );
        assert!(as_value.clauses.is_empty(), "a value list is not a clause");

        let as_clause = ok("(names :first \"a\")");
        assert!(as_clause.fields.iter().all(|(k, _)| k == "schema-version"));
        assert_eq!(
            as_clause.clauses.len(),
            1,
            "in body position, '(' must open a CLAUSE"
        );
        assert_eq!(as_clause.clauses[0].head, "names");
    }

    #[test]
    fn empty_list_is_legal_but_empty_clause_is_not() {
        assert_eq!(
            ok(":previous-names ()").field("previous-names"),
            Some(&Value::List(vec![]))
        );
        // `clause = "(" symbol …` — there is no headless clause production.
        assert!(err("()").contains("must be followed immediately by a clause symbol"));
    }

    /// A clause's `(` must be followed *immediately* by its head symbol. This
    /// is what stops `( a b )` being read as a clause in body position.
    #[test]
    fn clause_head_admits_no_leading_separator() {
        assert!(err("( names :x 1)").contains("must be followed immediately by a clause symbol"));
    }

    #[test]
    fn keyword_must_be_separated_from_its_value() {
        assert!(err(":names(\"a\")").contains("must be followed by a separator"));
        assert!(err(":names\"a\"").contains("must be followed by a separator"));
    }

    #[test]
    fn body_items_must_be_separated() {
        for bad in [
            r#":a "x":b "y""#,
            r#":a "x"(nested)"#,
            r#"(first):a "x""#,
            "(first)(second)",
        ] {
            assert!(
                err(bad).contains("fields and clauses in (repo-deed) must be separated"),
                "{bad:?} should require a separator between body items"
            );
        }
    }

    #[test]
    fn list_values_must_be_separated() {
        for bad in [r#":items ("a""b")"#, ":items (1#t)", ":items (#t(foo))"] {
            assert!(
                err(bad).contains("list values must be separated"),
                "{bad:?} should require a separator between list values"
            );
        }

        assert_eq!(
            ok(r#":items ("a")"#).field("items"),
            Some(&Value::List(vec![Value::Str("a".into())]))
        );
    }

    /// `=` is a legal symbol *character*, never a field separator. A parser
    /// that splits on `=` reads TOML and calls it a deed.
    ///
    /// Note the asymmetry, which is easy to get wrong in the permissive
    /// direction: `symbol = ALPHA *( ALPHA / DIGIT / … / "=" / … )`, so `=` may
    /// appear INSIDE a symbol but a symbol must still START with a letter.
    /// `>=` is therefore not a symbol either.
    #[test]
    fn equals_is_a_symbol_character_not_a_separator() {
        assert_eq!(ok(":op a=b").field("op"), Some(&Value::Sym("a=b".into())));
        assert_eq!(
            ok(":op v1.2-rc").field("op"),
            Some(&Value::Sym("v1.2-rc".into()))
        );
        assert!(err(":name = \"x\"").contains("'=' as a field separator is not a deed"));
        assert!(
            err(":op >=").contains("cannot lex a value"),
            "a symbol must start with a letter"
        );
    }

    #[test]
    fn exactly_four_escapes_are_legal() {
        assert_eq!(
            ok(r#":s "a\n b\t c\\ d\"e""#).str_field("s"),
            Some("a\n b\t c\\ d\"e")
        );
        assert!(err(r#":s "bad \r""#).contains("invalid escape"));
        // Written this way deliberately: a literal backslash-u sequence in this
        // source has been silently decoded by tooling in transit before now,
        // which turned this case into `"bad A"` — a string that parses fine and
        // made the assertion vacuous. Composing the backslash at runtime cannot
        // be mangled that way.
        let u_escape = format!(r#":s "bad {}u0041""#, '\\');
        assert!(
            format!("{:#}", parse(&doc(&u_escape)).unwrap_err()).contains("invalid escape"),
            "\\uXXXX is not one of the four legal escapes"
        );
        // A trailing backslash must not swallow the structure that follows it.
        // Here the next character is the form's closing paren, so the parser is
        // required to report an invalid escape rather than quietly consuming
        // the `)` and hunting for a closing quote that no longer exists.
        assert!(err(r#":s "dangling \"#).contains("invalid escape"));

        // A raw LF inside a string is rejected on its own terms, so a string
        // can only run to EOF if the input stops with no trailing newline at
        // all. `doc()` cannot produce that — it always appends the closer.
        let truncated = concat!(
            ";; SPDX-License-Identifier: MPL-2.0\n",
            "(repo-deed :schema-version \"1.0.0\" :s \"never closed"
        );
        assert!(
            format!("{:#}", parse(truncated).unwrap_err()).contains("unterminated string"),
            "a string running to EOF must be reported as unterminated"
        );

        // …and the stricter rule that makes the above hard to reach in the
        // first place: a literal newline inside a string is not a str-char.
        assert!(
            format!(
                "{:#}",
                parse(concat!(
                    ";; SPDX-License-Identifier: MPL-2.0\n",
                    "(repo-deed :schema-version \"1.0.0\" :s \"two\nlines\")\n"
                ))
                .unwrap_err()
            )
            .contains("raw control character"),
            "a raw LF inside a string must be rejected, not absorbed"
        );
    }

    #[test]
    fn booleans_are_strict_lowercase_hash_forms() {
        assert_eq!(ok(":a #t :b #f").field("a"), Some(&Value::Bool(true)));
        assert_eq!(ok(":a #t :b #f").field("b"), Some(&Value::Bool(false)));
        for bad in [":a #T", ":a #true", ":a #F", ":a #x"] {
            assert!(
                err(bad).contains("unrecognised #-form"),
                "{bad} should be rejected"
            );
        }
        // `true` lexes as a legal SYMBOL; it is rejected as a value because the
        // grammar says booleans are #t/#f only — not because it fails to lex.
        for bad in [":a true", ":a false", ":a yes", ":a no"] {
            assert!(
                err(bad).contains("booleans are #t / #f only"),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn uuid5_must_be_followed_immediately_by_a_string() {
        assert_eq!(
            ok(r#":id #u5"estate/chora""#).field("id"),
            Some(&Value::Uuid5("estate/chora".into()))
        );
        assert!(err(r#":id #u5 "estate/chora""#).contains("immediately by a string"));
        assert!(err(":id #u5 x").contains("immediately by a string"));
    }

    #[test]
    fn only_symbols_and_lists_may_be_quoted() {
        assert_eq!(
            ok(":q 'sym").field("q"),
            Some(&Value::Quoted(Box::new(Value::Sym("sym".into()))))
        );
        assert!(
            matches!(ok(":q '(a b)").field("q"), Some(Value::Quoted(b)) if matches!(**b, Value::List(_)))
        );
        assert!(err(r#":q '"str""#).contains("only symbols and lists may be quoted"));
        assert!(err(":q '1").contains("only symbols and lists may be quoted"));
    }

    #[test]
    fn integers_are_decimal_with_leading_zeros_permitted() {
        assert_eq!(
            ok(":n 007").field("n"),
            Some(&Value::Int(7)),
            "leading zeros are NOT octal"
        );
        assert_eq!(ok(":n -3").field("n"), Some(&Value::Int(-3)));
        assert_eq!(ok(":n 0").field("n"), Some(&Value::Int(0)));
        assert!(err(":n 12abc").contains("number followed by identifier characters"));
    }

    #[test]
    fn a_keyword_may_only_lead_a_field() {
        assert!(err(":outer :inner").contains("stray keyword"));
        assert!(err(":list (:a)").contains("stray keyword"));
    }

    /// `token-sep` is SP / line-end / comment. A single `;` opens a comment, so
    /// `;;` is a comment too — including in the middle of a form.
    #[test]
    fn comments_are_separators_anywhere_in_the_form() {
        let n = ok(":a \"x\" ; trailing comment\n  :b \"y\"");
        assert_eq!(n.str_field("a"), Some("x"));
        assert_eq!(n.str_field("b"), Some("y"));
    }

    #[test]
    fn bare_cr_is_not_a_line_end() {
        let text = ";; SPDX-License-Identifier: MPL-2.0\n(repo-deed\r:schema-version \"1.0.0\")\n";
        assert!(format!("{:#}", parse(text).unwrap_err()).contains("bare CR"));
        // CRLF is fine.
        let crlf =
            ";; SPDX-License-Identifier: MPL-2.0\r\n(repo-deed\r\n:schema-version \"1.0.0\")\r\n";
        assert!(parse(crlf).is_ok(), "CRLF is a legal line-end");
    }

    #[test]
    fn the_whole_input_must_be_consumed() {
        let text = ";; SPDX-License-Identifier: MPL-2.0\n(repo-deed :schema-version \"1\") extra\n";
        assert!(format!("{:#}", parse(text).unwrap_err()).contains("trailing content"));
        // …but trailing whitespace and comments are fine.
        let tidy =
            ";; SPDX-License-Identifier: MPL-2.0\n(repo-deed :schema-version \"1\")\n;; bye\n";
        assert!(parse(tidy).is_ok());
    }

    #[test]
    fn only_the_four_doc_heads_are_accepted() {
        for head in DOC_HEADS {
            let text =
                format!(";; SPDX-License-Identifier: MPL-2.0\n({head} :schema-version \"1\")\n");
            assert!(parse(&text).is_ok(), "{head} is a legal doc-head");
        }
        let text = ";; SPDX-License-Identifier: MPL-2.0\n(chora-deed :schema-version \"1\")\n";
        assert!(format!("{:#}", parse(text).unwrap_err()).contains("invalid doc-head"));
    }

    /// Only the LEADING run of `;; SPDX-` lines is the header. The launcher
    /// deed carries fourteen further `;;` comment lines before its form, and
    /// they must be consumed as separators, not mistaken for header lines.
    #[test]
    fn comments_between_header_and_form_are_separators() {
        let text = ";; SPDX-FileCopyrightText: © 2026 someone\n\
                    ;; SPDX-License-Identifier: MPL-2.0\n\
                    ;;\n\
                    ;; A note about provenance.\n\
                    (repo-deed :schema-version \"1.0.0\")\n";
        assert_eq!(parse(text).expect("parses").head, "repo-deed");
    }

    #[test]
    fn schema_version_must_be_present_exactly_once_and_be_a_string() {
        let one = ";; SPDX-License-Identifier: MPL-2.0\n(repo-deed :schema-version \"1.0.0\")\n";
        assert!(parse(one).is_ok());

        let none = ";; SPDX-License-Identifier: MPL-2.0\n(repo-deed :canonical-name \"x\")\n";
        assert!(format!("{:#}", parse(none).unwrap_err()).contains("(found 0)"));

        let two = ";; SPDX-License-Identifier: MPL-2.0\n\
                   (repo-deed :schema-version \"1\" :schema-version \"2\")\n";
        assert!(format!("{:#}", parse(two).unwrap_err()).contains("(found 2)"));

        let sym = ";; SPDX-License-Identifier: MPL-2.0\n(repo-deed :schema-version v1)\n";
        assert!(format!("{:#}", parse(sym).unwrap_err()).contains("must be a STRING"));
    }

    /// A tab is invalid anywhere, *including inside a string*, matching
    /// `deed_lint.py`, which tests the raw text before lexing.
    #[test]
    fn tabs_are_invalid_even_inside_a_string() {
        assert!(err(":a \"x\ty\"").contains("HTAB"));
        assert!(err(":a\t\"x\"").contains("HTAB"));
        // The two-character escape is the legal way to write one.
        assert_eq!(ok(r#":a "x\ty""#).str_field("a"), Some("x\ty"));
    }

    #[test]
    fn missing_priority_sorts_after_every_rung_that_has_one() {
        let n = ok("(ladder (p :priority 20 :v \"b\") (p :v \"none\") (p :priority 10 :v \"a\"))");
        let ladder = n.clause("ladder").expect("(ladder …)");
        let vs: Vec<&str> = ladder
            .children_by_priority("p")
            .iter()
            .map(|c| c.str_field("v").unwrap())
            .collect();
        assert_eq!(vs, vec!["a", "b", "none"]);
    }
}
