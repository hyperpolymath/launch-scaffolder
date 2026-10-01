// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `provision-set mint` (and `realign`, which is the same operation): write a
//! repository's provisioning set from the canon, following the ownership rules
//! of `PROVISIONING-STANDARD.adoc` §1.
//!
//! * Engine files are copied byte for byte and always realigned.
//! * Minted files are created when missing and replaced only while they are
//!   still stubs, or when the set was inherited from another repository (the
//!   deed's `:repo` names someone else). Once filled, the repository owns them.
//! * A `mise.toml` that pins a banned tool is replaced, carrying over its other
//!   `[tools]` entries.
//!
//! Every fact about the repository (languages, tools, Guix specs, where the
//! files go) comes from the engine's own `provision-lib.sh`, run against the
//! target, so the generator and `just doctor` can never disagree about them.
//! Only `.tmpl` files are slot-filled; `__SPEC_*__` slots are left for the
//! repository-specific pass (standard §5).

use super::canon::{Canon, ENGINE_FILES};
use super::licence::{self, Licence};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `__COPYRIGHT_HOLDER__` for every minted file.
pub const HOLDER: &str = "Jonathan D.A. Jewell (hyperpolymath) <j.d.a.jewell@open.ac.uk>";

/// The contract verbs (standard §2): `just <verb>` must work in every repository.
pub const VERBS: &[&str] = &[
    "setup",
    "doctor",
    "heal",
    "dev-shell",
    "toolchain-refresh",
    "ai-setup",
    "ai-warmup",
    "eval",
    "config-show",
    "opsm",
    "build",
    "test",
    "bench",
    "lint",
    "fmt",
    "fmt-check",
    "run",
    "deps",
];

const ARCHETYPES: &[&str] = &["app", "library", "tool", "theory", "docs"];
const DEED: &str = ".machine_readable/descriptiles/provisioning_praxis.deed";
const LIB: &str = "build/just/provision-lib.sh";
const WRAP: usize = 80;

#[derive(Debug, Default, Clone)]
pub struct Options {
    /// `owner/name`; default: the `origin` remote.
    pub repo: Option<String>,
    /// Overrides the deed's archetype.
    pub archetype: Option<String>,
    /// `__YEAR__`; default: `SOURCE_DATE_EPOCH`, else the current year.
    pub year: Option<i64>,
    /// Skip the steps that need the network or Guix (`mise lock`,
    /// `build/guix/crates.scm`); the files they write are then left for
    /// `just toolchain-refresh`.
    pub offline: bool,
}

/// What happened to one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    Created,
    Replaced(String),
    Kept(String),
    Skipped(String),
    /// An external step (`mise lock`, `guix import crate`) did not produce a
    /// valid file: a ledger line, and the CLI exits [`EXIT_EXTERNAL`].
    Failed(String),
}

/// Exit code when the set was written but an external step failed.
pub const EXIT_EXTERNAL: i32 = 4;

impl std::fmt::Display for Act {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Act::Created => write!(f, "created"),
            Act::Replaced(why) => write!(f, "replaced ({why})"),
            Act::Kept(why) => write!(f, "kept ({why})"),
            Act::Skipped(why) => write!(f, "skipped ({why})"),
            Act::Failed(why) => write!(f, "FAILED ({why})"),
        }
    }
}

#[derive(Debug)]
pub struct Report {
    pub slug: String,
    pub licence: Licence,
    pub archetype: String,
    pub langs: Vec<String>,
    pub inherited_from: Option<String>,
    pub files: Vec<(String, Act)>,
}

/// Mint (or realign) the provisioning set in `target`.
pub fn mint(target: &Path, canon: &Canon, opts: &Options) -> Result<Report> {
    let target = &target
        .canonicalize()
        .with_context(|| format!("resolving {}", target.display()))?;
    let slug = match &opts.repo {
        Some(s) => s.clone(),
        None => origin_slug(target).unwrap_or_else(|| {
            format!(
                "hyperpolymath/{}",
                target.file_name().unwrap_or_default().to_string_lossy()
            )
        }),
    };
    let name = slug.rsplit('/').next().unwrap_or(&slug).to_string();
    // Refuse before writing anything (standard §6).
    let licence = licence::classify(target, &name)?;
    let mut files = Vec::new();

    for rel in ENGINE_FILES {
        let act = write_file(target, rel, &canon.file(rel)?, "engine realigned")?;
        files.push((rel.to_string(), act));
    }

    let lib = Lib(target.to_path_buf());
    let langs = lib.lines(&["langs"])?;
    let old_deed = std::fs::read_to_string(target.join(DEED)).ok();
    let deed_repo = old_deed.as_deref().and_then(|d| deed_field(d, "repo"));
    let inherited_from = deed_repo.filter(|r| r != &slug && !r.contains("__"));
    let archetype = match &opts.archetype {
        Some(a) => a.clone(),
        None => old_deed
            .as_deref()
            .filter(|_| inherited_from.is_none())
            .and_then(|d| deed_field(d, "archetype"))
            .filter(|a| ARCHETYPES.contains(&a.as_str()))
            .unwrap_or_else(|| if langs == ["docs"] { "docs" } else { "library" }.to_string()),
    };
    if !ARCHETYPES.contains(&archetype.as_str()) {
        bail!(
            "archetype {archetype:?} is not one of {}",
            ARCHETYPES.join(", ")
        );
    }

    let year = opts.year.unwrap_or_else(current_year);
    let mut vars: BTreeMap<&str, String> = BTreeMap::new();
    vars.insert("APP_NAME", name.clone());
    vars.insert("REPO_SLUG", slug.clone());
    vars.insert("YEAR", year.to_string());
    vars.insert("COPYRIGHT_HOLDER", HOLDER.to_string());
    vars.insert("LICENSE", licence.code.to_string());
    vars.insert("DOC_LICENSE", licence.doc.to_string());
    vars.insert("GUIX_LICENSE", licence.guix.to_string());
    vars.insert("ARCHETYPE", archetype.clone());
    vars.insert("LANGS", langs.join(", "));

    let m = Minter {
        target,
        canon,
        lib: &lib,
        inherited: inherited_from.is_some(),
    };

    // The deed first: the fact verbs below read it.
    files.push((
        DEED.to_string(),
        m.minted(DEED, &format!("{DEED}.tmpl"), &vars)?,
    ));

    let guix_dir = lib.out(&["guix-dir"])?;
    let gpre = if guix_dir == "build" { "build/" } else { "" };
    let (app_version, synopsis, description) = describe(target, &name);
    let guix_specs = lib.words(&["guix-specs"])?;
    let mise_tools = lib.words(&["mise-tools"])?;
    vars.insert("GUIX_PREFIX", gpre.to_string());
    vars.insert("APP_VERSION", app_version);
    vars.insert("SYNOPSIS", scheme_escape(&synopsis));
    vars.insert("DESCRIPTION", scheme_escape(&description));
    vars.insert("GUIX_GAPS", or_none(lib.out(&["guix-gaps"])?));
    vars.insert("TOOL_TABLE", lib.out(&["tool-table"])?);
    vars.insert("SYSTEM_DEPS_SECTION", lib.out(&["system-deps", "adoc"])?);
    vars.insert("SYSTEM_DEPS_AI", lib.out(&["system-deps", "ai"])?);
    vars.insert(
        "MISE_TOOLS",
        mise_tools
            .iter()
            .map(|t| format!("`{t}`"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    vars.insert("GUIX_SPECS", quoted_atoms(&guix_specs));
    vars.insert("GUIX_PKG_SPECS", quoted_atoms(&guix_specs));

    // mise.toml: minted, but a banned tool is a replace condition.
    let banned = lib.predicate(&["mise-banned"])?;
    let (mut carried, mut unread) = (Vec::new(), String::new());
    if let Some(hits) = &banned {
        (carried, unread) = carry_over_tools(target, hits)?;
    }
    vars.insert("MISE_TOOLS_TOML", mise_tools_toml(&mise_tools, &carried));
    let act = match banned {
        Some(hits) if target.join("mise.toml").exists() => m.force(
            "mise.toml",
            "mise.toml.tmpl",
            &vars,
            &format!("pinned banned tool(s): {hits}{unread}"),
        )?,
        _ => m.minted("mise.toml", "mise.toml.tmpl", &vars)?,
    };
    files.push(("mise.toml".into(), act));

    // The Guix trio, beside whichever guix.scm the repository already keeps.
    let cargo = langs.iter().any(|l| l == "rust") && target.join("Cargo.toml").is_file();
    let guix_tmpl = if cargo {
        "guix/guix.scm.cargo.tmpl"
    } else {
        "guix/guix.scm.source.tmpl"
    };
    let gs = format!("{gpre}guix.scm");
    files.push((gs.clone(), m.minted(&gs, guix_tmpl, &vars)?));
    let gm = format!("{gpre}manifest.scm");
    files.push((gm.clone(), m.minted(&gm, "guix/manifest.scm.tmpl", &vars)?));
    let gc = format!("{gpre}channels.scm");
    files.push((gc.clone(), m.minted_bytes(&gc, "guix/channels.scm")?));
    if cargo && !target.join("build/guix/crates.scm").is_file() {
        let act = if opts.offline {
            Act::Skipped(
                "offline: generate with `just toolchain-refresh`; doctor reports PV-W24 until then"
                    .into(),
            )
        } else {
            crates_scm(&lib, &gs)?
        };
        files.push(("build/guix/crates.scm".into(), act));
    }

    // Docs and warm-ups go where set-files puts them (root, docs/ or docs/onboarding/).
    let set_files = lib.lines(&["set-files"])?;
    let place = |base: &str| -> String {
        set_files
            .iter()
            .find(|p| p.rsplit('/').next() == Some(base))
            .cloned()
            .unwrap_or_else(|| base.to_string())
    };
    for (dest, tmpl) in [
        ("docs/SETUP.adoc".to_string(), "docs/SETUP.adoc.tmpl"),
        (
            "docs/AI_INSTALLATION_GUIDE.adoc".to_string(),
            "docs/AI_INSTALLATION_GUIDE.adoc.tmpl",
        ),
        (place("llm-warmup-user.adoc"), "llm-warmup-user.adoc.tmpl"),
        (place("llm-warmup-dev.adoc"), "llm-warmup-dev.adoc.tmpl"),
        (
            place("llm-warmup-maintainer.adoc"),
            "llm-warmup-maintainer.adoc.tmpl",
        ),
    ] {
        files.push((dest.clone(), m.minted(&dest, tmpl, &vars)?));
    }

    // launcher.sh: generated for library/tool/theory/docs; a hand-written
    // launcher or an app's launcher (minted from its own config) is kept.
    let launcher = target.join("launcher.sh");
    let act = match std::fs::read_to_string(&launcher) {
        Ok(s) if !generated_launcher(&s) => {
            Act::Kept("hand-written: give it the provisioning modes by sourcing build/just/provision-modes.sh".into())
        }
        Err(_) if archetype == "app" => {
            Act::Skipped("an app launcher is minted by `launch-scaffolder mint` from the app's config".into())
        }
        _ => {
            let body = render(&canon_text(canon, "launcher.sh.tmpl")?, &vars);
            write_file(target, "launcher.sh", body.as_bytes(), "re-rendered from the deed")?
        }
    };
    files.push(("launcher.sh".into(), act));

    let readme = render(&canon_text(canon, "README-ai-install.adoc.tmpl")?, &vars);
    files.push((
        "README.adoc".into(),
        super::readme::insert(target, &readme)?,
    ));

    // Justfile: created whole when absent, otherwise merged (justfile.rs), which
    // `just --summary` must accept or the original is restored.
    let justfile = ["Justfile", "justfile", ".justfile"]
        .into_iter()
        .find(|j| target.join(j).is_file());
    let act = match justfile {
        Some(j) => {
            super::justfile::merge(target, j, &canon_text(canon, "build/just/provision.just")?)?
        }
        None => {
            vars.insert(
                "DELEGATIONS",
                delegations(&canon_text(canon, "build/just/provision.just")?, &[]),
            );
            let body = render(&canon_text(canon, "Justfile.tmpl")?, &vars);
            write_file(target, "Justfile", body.as_bytes(), "")?
        }
    };
    files.push((justfile.unwrap_or("Justfile").into(), act));

    // Last, because it reads the mise.toml written above.
    let act = if opts.offline {
        Act::Skipped("offline: run `mise lock`; provision-check fails until then".into())
    } else {
        mise_lock(&lib)?
    };
    files.push(("mise.lock".into(), act));

    Ok(Report {
        slug,
        licence,
        archetype,
        langs,
        inherited_from,
        files,
    })
}

struct Minter<'a> {
    target: &'a Path,
    canon: &'a Canon,
    lib: &'a Lib,
    inherited: bool,
}

impl Minter<'_> {
    /// A minted file: created when missing, replaced while a stub or inherited.
    fn minted(&self, dest: &str, tmpl: &str, vars: &BTreeMap<&str, String>) -> Result<Act> {
        let body = render(&canon_text(self.canon, tmpl)?, vars);
        match self.stub_reason(dest)? {
            Some(why) => write_file(self.target, dest, body.as_bytes(), &why),
            None => Ok(Act::Kept("filled: owned by the repository".into())),
        }
    }

    fn minted_bytes(&self, dest: &str, src: &str) -> Result<Act> {
        match self.stub_reason(dest)? {
            Some(why) => write_file(self.target, dest, &self.canon.file(src)?, &why),
            None => Ok(Act::Kept("filled: owned by the repository".into())),
        }
    }

    fn force(
        &self,
        dest: &str,
        tmpl: &str,
        vars: &BTreeMap<&str, String>,
        why: &str,
    ) -> Result<Act> {
        let body = render(&canon_text(self.canon, tmpl)?, vars);
        write_file(self.target, dest, body.as_bytes(), why)
    }

    /// Why `dest` may be (re)written, or `None` when the repository owns it.
    fn stub_reason(&self, dest: &str) -> Result<Option<String>> {
        let path = self.target.join(dest);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(Some("missing".into()));
        };
        if self.inherited {
            return Ok(Some("inherited from another repository's set".into()));
        }
        if dest.ends_with(".scm") {
            return Ok(self
                .lib
                .predicate(&["guix-stub", dest])?
                .map(|r| format!("Guix stub: {r}")));
        }
        if let Some(slot) = mechanical_residue(&text) {
            return Ok(Some(format!("never minted: __{slot}__ unfilled")));
        }
        if dest.contains("llm-warmup-")
            && text.contains("for overview.")
            && text.contains("Key Commands")
        {
            return Ok(Some("generic warm-up boilerplate".into()));
        }
        Ok(None)
    }
}

/// The mechanical slots the generator fills. Residue of one of these means the
/// file was never minted; `__SPEC_*__` residue means it was minted but not yet
/// specialised, which is the repository's to finish (doctor PV-W29).
const MECHANICAL: &[&str] = &[
    "APP_NAME",
    "REPO_SLUG",
    "YEAR",
    "COPYRIGHT_HOLDER",
    "LICENSE",
    "DOC_LICENSE",
    "GUIX_LICENSE",
    "ARCHETYPE",
    "LANGS",
    "GUIX_PREFIX",
    "APP_VERSION",
    "SYNOPSIS",
    "DESCRIPTION",
    "GUIX_GAPS",
    "TOOL_TABLE",
    "SYSTEM_DEPS_SECTION",
    "SYSTEM_DEPS_AI",
    "MISE_TOOLS",
    "MISE_TOOLS_TOML",
    "GUIX_SPECS",
    "GUIX_PKG_SPECS",
    "DELEGATIONS",
];

fn mechanical_residue(text: &str) -> Option<&'static str> {
    MECHANICAL
        .iter()
        .copied()
        .find(|k| text.contains(&format!("__{k}__")))
}

/// Replace every `__KEY__` whose KEY is in `vars`, in one left-to-right pass, so
/// a value can never be re-substituted (a description that mentions `__init__`
/// stays as written). A value marked as a list of atoms is wrapped at
/// [`WRAP`] columns with its continuation lines aligned under the first atom.
pub fn render(tmpl: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = String::with_capacity(tmpl.len());
    let mut rest = tmpl;
    while let Some(i) = rest.find("__") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let key = after.find("__").map(|j| &after[..j]).filter(|k| {
            k.starts_with(|c: char| c.is_ascii_uppercase())
                && k.chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        });
        match key.and_then(|k| vars.get(k).map(|v| (k, v))) {
            Some((k, v)) => {
                if let Some(atoms) = v.strip_prefix(ATOMS) {
                    let col = out.len() - out.rfind('\n').map_or(0, |n| n + 1);
                    out.push_str(&wrap_atoms(atoms, col));
                } else {
                    out.push_str(v);
                }
                rest = &after[k.len() + 2..];
            }
            None => {
                out.push('_');
                rest = &rest[i + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Marks a value as space-separated Scheme atoms for [`render`] to wrap.
const ATOMS: &str = "\u{0}atoms\u{0}";

fn quoted_atoms(specs: &[String]) -> String {
    let atoms: Vec<String> = specs
        .iter()
        .map(|s| format!("\"{}\"", scheme_escape(s)))
        .collect();
    format!("{ATOMS}{}", atoms.join(" "))
}

fn wrap_atoms(atoms: &str, col: usize) -> String {
    let mut out = String::new();
    let mut width = col;
    for (n, a) in atoms.split(' ').filter(|a| !a.is_empty()).enumerate() {
        if n > 0 {
            // Room for " atom" plus the closing parens the template adds.
            if width + 1 + a.len() > WRAP - 3 {
                out.push('\n');
                out.push_str(&" ".repeat(col));
                width = col;
            } else {
                out.push(' ');
                width += 1;
            }
        }
        out.push_str(a);
        width += a.len();
    }
    out
}

fn scheme_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn or_none(s: String) -> String {
    if s.trim().is_empty() {
        "none".into()
    } else {
        s
    }
}

/// `[tools]` lines: the canon's tools, then the other entries carried over from
/// a replaced `mise.toml`. A carried entry keeps its own value: the
/// repository's pin of a tool the canon also lists is a decision, not drift.
fn mise_tools_toml(tools: &[String], carried: &[(String, String)]) -> String {
    let value = |t: &String| {
        carried
            .iter()
            .find(|(k, _)| k == t)
            .map_or_else(|| "\"latest\"".to_string(), |(_, v)| v.clone())
    };
    let mut lines: Vec<String> = tools
        .iter()
        .map(|t| format!("{} = {}", toml_key(t), value(t)))
        .collect();
    for (k, v) in carried {
        if !tools.iter().any(|t| t == k) {
            lines.push(format!("{} = {v}", toml_key(k)));
        }
    }
    lines.join("\n")
}

fn toml_key(k: &str) -> String {
    if k.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        k.to_string()
    } else {
        format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// The non-banned `[tools]` entries of an existing `mise.toml`, values as TOML,
/// and a note for the replace reason when the file is not TOML at all: mise
/// cannot read such a file either, so nothing in it was in effect to carry.
fn carry_over_tools(target: &Path, banned: &str) -> Result<(Vec<(String, String)>, String)> {
    let text = std::fs::read_to_string(target.join("mise.toml"))?;
    let table: toml::Table = match toml::from_str(&text) {
        Ok(t) => t,
        Err(e) => {
            let why = e.message().trim().to_string();
            return Ok((
                Vec::new(),
                format!("; it was not valid TOML ({why}), so no tool was carried over"),
            ));
        }
    };
    let banned: Vec<&str> = banned.split_whitespace().collect();
    let mut out = Vec::new();
    if let Some(tools) = table.get("tools").and_then(|t| t.as_table()) {
        for (k, v) in tools {
            if !banned.contains(&k.as_str()) {
                out.push((k.clone(), v.to_string()));
            }
        }
    }
    Ok((out, String::new()))
}

/// One root delegation per contract verb the root Justfile does not define,
/// each with the module recipe's own doc comment, e.g.
/// `build: provision::build` or `ai-warmup who="user": (provision::ai-warmup who)`.
pub fn delegations(provision_just: &str, existing: &[String]) -> String {
    let mut out = Vec::new();
    let mut doc: Option<&str> = None;
    for line in provision_just.lines() {
        if let Some(d) = line.strip_prefix("# ") {
            doc = Some(d);
            continue;
        }
        let sig = line.trim_end();
        let is_recipe = sig.starts_with(|c: char| c.is_ascii_lowercase())
            && sig.ends_with(':')
            && !sig.contains(":=");
        if !is_recipe {
            if !line.starts_with(' ') {
                doc = None;
            }
            continue;
        }
        let sig = &sig[..sig.len() - 1];
        let (name, params) = sig.split_once(' ').unwrap_or((sig, ""));
        if VERBS.contains(&name) && !existing.iter().any(|e| e == name) {
            let comment = doc.map(|d| format!("# {d}\n")).unwrap_or_default();
            if params.is_empty() {
                out.push(format!("{comment}{name}: provision::{name}"));
            } else {
                let args: Vec<&str> = params
                    .split_whitespace()
                    .map(|p| p.split('=').next().unwrap_or(p))
                    .collect();
                out.push(format!(
                    "{comment}{name} {params}: (provision::{name} {})",
                    args.join(" ")
                ));
            }
        }
        doc = None;
    }
    out.join("\n\n")
}

/// `true` for a launcher this generator rendered (and may re-render).
fn generated_launcher(text: &str) -> bool {
    text.contains("@launcher-deed begin") && text.contains(":generator \"provision-set\"")
}

/// `:key "value"` from a deed.
fn deed_field(deed: &str, key: &str) -> Option<String> {
    let pat = format!(":{key} ");
    deed.lines().find_map(|l| {
        let l = l.trim_start();
        let rest = l.strip_prefix(&pat)?.trim_start().strip_prefix('"')?;
        rest.split_once('"').map(|(v, _)| v.to_string())
    })
}

/// `owner/name` from the `origin` remote of a GitHub checkout.
fn origin_slug(target: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(target)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let url = String::from_utf8(out.stdout).ok()?;
    let url = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let path = url
        .split_once("github.com")
        .map(|(_, p)| p.trim_start_matches([':', '/']))?;
    let mut parts = path.splitn(2, '/');
    let (o, r) = (parts.next()?, parts.next()?);
    (!o.is_empty() && !r.is_empty() && !r.contains('/')).then(|| format!("{o}/{r}"))
}

/// `(version, synopsis, description)`: the Cargo package's, else the README's
/// first prose paragraph, else the name.
fn describe(target: &Path, name: &str) -> (String, String, String) {
    let cargo: Option<toml::Table> = std::fs::read_to_string(target.join("Cargo.toml"))
        .ok()
        .and_then(|t| toml::from_str(&t).ok());
    let field = |k: &str| -> Option<String> {
        let c = cargo.as_ref()?;
        let pkg = c.get("package")?.as_table()?;
        match pkg.get(k)? {
            toml::Value::String(s) => Some(s.clone()),
            // `version.workspace = true`: the workspace's own value.
            _ => c
                .get("workspace")?
                .get("package")?
                .get(k)?
                .as_str()
                .map(str::to_string),
        }
    };
    let version = field("version").unwrap_or_else(|| "0.1.0".into());
    let description = field("description")
        .or_else(|| readme_paragraph(target))
        .unwrap_or_else(|| format!("{name}, a hyperpolymath repository."));
    let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
    (version, synopsis_of(&description), description)
}

fn readme_paragraph(target: &Path) -> Option<String> {
    let text = ["README.adoc", "README.md", "README"]
        .iter()
        .find_map(|f| std::fs::read_to_string(target.join(f)).ok())?;
    let mut para = Vec::new();
    let mut in_block = false;
    for line in text.lines() {
        let t = line.trim();
        if t == "----" || t == "...." || t == "```" || t.starts_with("```") || t == "////" {
            in_block = !in_block;
            continue;
        }
        let markup = t.starts_with(['=', '#', ':', '[', '!', '<', '|', '*', '-', '.', '+', '>'])
            || t.starts_with("//")
            || t.starts_with("image:")
            || t.starts_with("ifdef")
            || t.starts_with("endif")
            || t.starts_with("toc::");
        if in_block || t.is_empty() || markup {
            if !para.is_empty() && (t.is_empty() || markup) {
                break;
            }
            continue;
        }
        para.push(t);
    }
    (!para.is_empty()).then(|| para.join(" "))
}

/// The first sentence, without its full stop, at most 79 characters.
fn synopsis_of(description: &str) -> String {
    let first = description
        .split(". ")
        .next()
        .unwrap_or(description)
        .trim_end_matches('.');
    if first.len() <= 79 {
        return first.to_string();
    }
    let mut s = String::new();
    for w in first.split_whitespace() {
        if s.len() + w.len() + 1 > 79 {
            break;
        }
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(w);
    }
    s
}

fn current_year() -> i64 {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64)
        });
    year_of_days(secs.div_euclid(86_400))
}

/// The proleptic Gregorian year of a day count since 1970-01-01 (Hinnant's
/// `civil_from_days`).
fn year_of_days(z: i64) -> i64 {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(m <= 2)
}

fn canon_text(canon: &Canon, rel: &str) -> Result<String> {
    String::from_utf8(canon.file(rel)?.into_owned()).with_context(|| format!("{rel} is not UTF-8"))
}

/// Write `bytes` to `target/rel` unless it already holds them. Shell scripts and
/// the launcher are made executable.
fn write_file(target: &Path, rel: &str, bytes: &[u8], why: &str) -> Result<Act> {
    let path = target.join(rel);
    let old = std::fs::read(&path).ok();
    if old.as_deref() == Some(bytes) {
        set_exec(&path, rel)?;
        return Ok(Act::Kept("up to date".into()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    set_exec(&path, rel)?;
    Ok(if old.is_some() {
        Act::Replaced(why.to_string())
    } else {
        Act::Created
    })
}

fn set_exec(path: &Path, rel: &str) -> Result<()> {
    #[cfg(unix)]
    if rel.ends_with(".sh") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod {}", path.display()))?;
    }
    let _ = (path, rel);
    Ok(())
}

/// The engine's `provision-lib.sh`, run against the target. A verb that fails
/// is an error: a fact the generator cannot establish is never guessed.
/// The Guix command for `crates-scm`, e.g. a wrapper that runs guix in a
/// container with the target mounted; the engine reads it as `GUIX`.
pub const GUIX_ENV: &str = "LAUNCH_SCAFFOLDER_GUIX";

/// Write `build/guix/crates.scm` through the engine's `crates-scm` verb. The
/// result is accepted only when `guix-stub` then passes on `guix_scm`: a
/// containerised guix loses its exit status, so the file is the evidence.
fn crates_scm(lib: &Lib, guix_scm: &str) -> Result<Act> {
    let guix = std::env::var(GUIX_ENV).unwrap_or_else(|_| "guix".into());
    let o = lib.run_env(&["crates-scm"], &[("GUIX", &guix)])?;
    match lib.predicate(&["guix-stub", guix_scm])? {
        None if o.status.success() => Ok(Act::Created),
        why => Ok(Act::Failed(format!(
            "{}crates-scm: {}",
            why.map(|w| format!("{guix_scm}: {w}; "))
                .unwrap_or_default(),
            last_line(&[&o.stdout, &o.stderr], &o.status)
        ))),
    }
}

/// Pin `mise.toml` in `mise.lock` with `mise lock`, unless the engine's
/// `mise-lock-gaps` already finds every tool pinned and checksummed: bumping
/// versions is `toolchain-refresh`'s job, not mint's. The target is trusted
/// for this one process through the environment, not mise's trust database.
fn mise_lock(lib: &Lib) -> Result<Act> {
    if lib.predicate(&["mise-lock-gaps"])?.is_none() {
        return Ok(Act::Kept("pinned and checksummed".into()));
    }
    let existed = lib.0.join("mise.lock").is_file();
    let o = Command::new("timeout")
        .args(["600", "mise", "lock"])
        .env("MISE_TRUSTED_CONFIG_PATHS", &lib.0)
        .current_dir(&lib.0)
        .output()
        .context("running timeout 600 mise lock")?;
    Ok(match lib.predicate(&["mise-lock-gaps"])? {
        None if existed => Act::Replaced("re-locked: the old lock had gaps".into()),
        None => Act::Created,
        Some(gap) => Act::Failed(format!(
            "{gap}; mise lock: {}",
            last_line(&[&o.stderr, &o.stdout], &o.status)
        )),
    })
}

/// The first error a failed command reported (mise ends with a version and a
/// "Run with --verbose" trailer, which name nothing), else its last line, else
/// its exit status when it said nothing.
fn last_line(streams: &[&[u8]], status: &std::process::ExitStatus) -> String {
    streams
        .iter()
        .find_map(|s| {
            let text = String::from_utf8_lossy(s);
            let lines: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            lines
                .iter()
                .find(|l| l.contains("ERROR") || l.starts_with("error:"))
                .or(lines.last())
                .map(|l| l.to_string())
        })
        .unwrap_or_else(|| format!("no output, {status}"))
}

/// The target repository's engine library, `build/just/provision-lib.sh`.
struct Lib(PathBuf);

impl Lib {
    /// Run a verb of the target's `provision-lib.sh`.
    fn run(&self, args: &[&str]) -> Result<std::process::Output> {
        self.run_env(args, &[])
    }

    /// Run a verb of the target's `provision-lib.sh` with extra environment.
    fn run_env(&self, args: &[&str], env: &[(&str, &str)]) -> Result<std::process::Output> {
        Command::new("bash")
            .arg(self.0.join(LIB))
            .args(args)
            .env("PROVISION_ROOT", &self.0)
            .envs(env.iter().copied())
            .current_dir(&self.0)
            .output()
            .with_context(|| format!("running {LIB} {}", args.join(" ")))
    }

    fn out(&self, args: &[&str]) -> Result<String> {
        let o = self.run(args)?;
        if !o.status.success() {
            bail!(
                "{LIB} {} failed ({}): {}",
                args.join(" "),
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            );
        }
        Ok(String::from_utf8(o.stdout)?
            .trim_end_matches('\n')
            .to_string())
    }

    fn lines(&self, args: &[&str]) -> Result<Vec<String>> {
        Ok(self
            .out(args)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }

    fn words(&self, args: &[&str]) -> Result<Vec<String>> {
        Ok(self
            .out(args)?
            .split_whitespace()
            .map(str::to_string)
            .collect())
    }

    /// A predicate verb: exit 0 is "no", exit 1 is "yes, because <stdout>".
    fn predicate(&self, args: &[&str]) -> Result<Option<String>> {
        let o = self.run(args)?;
        match o.status.code() {
            Some(0) => Ok(None),
            Some(1) => Ok(Some(String::from_utf8_lossy(&o.stdout).trim().to_string())),
            _ => bail!(
                "{LIB} {} failed ({}): {}",
                args.join(" "),
                o.status,
                String::from_utf8_lossy(&o.stderr).trim()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(kv: &[(&'static str, &str)]) -> BTreeMap<&'static str, String> {
        kv.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn render_is_single_pass_and_leaves_unknown_slots() {
        let v = vars(&[("A", "__B__ and __init__"), ("B", "x")]);
        assert_eq!(
            render("__A__ / __B__ / __SPEC_Q__ / a__b", &v),
            "__B__ and __init__ / x / __SPEC_Q__ / a__b"
        );
        assert_eq!(render("___A__", &vars(&[("A", "v")])), "_v");
    }

    #[test]
    fn atoms_wrap_under_the_first_atom() {
        let specs: Vec<String> = [
            "git",
            "bash",
            "coreutils",
            "nss-certs",
            "just",
            "mise",
            "shellcheck",
            "chez-scheme",
            "gmp",
            "gcc-toolchain",
            "zig",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let v: BTreeMap<&str, String> = [("S", quoted_atoms(&specs))].into_iter().collect();
        let out = render(" (list __S__))", &v);
        for line in out.lines() {
            assert!(line.len() <= WRAP, "{line:?} is wider than {WRAP}");
        }
        let cont: Vec<&str> = out.lines().skip(1).collect();
        assert!(!cont.is_empty(), "eleven specs must wrap");
        assert!(cont.iter().all(|l| l.starts_with("       \"")), "{out}");
        let tokens: Vec<&str> = out.split_whitespace().collect();
        assert_eq!(tokens[1], "\"git\"");
        assert_eq!(tokens.last(), Some(&"\"zig\"))"));
    }

    #[test]
    fn delegations_cover_every_verb_with_its_doc() {
        let canon = Canon::Baked;
        let pj = canon_text(&canon, "build/just/provision.just").unwrap();
        let d = delegations(&pj, &[]);
        for v in VERBS {
            assert!(
                d.lines()
                    .any(|l| l.starts_with(&format!("{v}:")) || l.starts_with(&format!("{v} "))),
                "no delegation for {v}:\n{d}"
            );
        }
        assert!(d.contains("# Diagnose the environment"));
        assert!(d.contains("ai-warmup who=\"user\": (provision::ai-warmup who)"));
        assert!(d.contains("fmt-check: provision::fmt-check"));
        assert!(!d.contains("langs:"), "langs is not a contract verb");
        let some = delegations(&pj, &["doctor".into(), "build".into()]);
        assert!(!some.contains("doctor:") && !some.contains("build:") && some.contains("heal:"));
    }

    #[test]
    fn deed_fields_and_slugs() {
        let d = "(praxis-deed\n  :repo         \"hyperpolymath/rsr-template-repo\"\n  :archetype    \"library\"        ; app | …\n";
        assert_eq!(
            deed_field(d, "repo").as_deref(),
            Some("hyperpolymath/rsr-template-repo")
        );
        assert_eq!(deed_field(d, "archetype").as_deref(), Some("library"));
        assert_eq!(deed_field(d, "ports"), None);
    }

    #[test]
    fn synopsis_is_one_short_sentence() {
        assert_eq!(synopsis_of("Does a thing. Then more."), "Does a thing");
        let long = "word ".repeat(40);
        assert!(synopsis_of(&long).len() <= 79);
        assert_eq!(scheme_escape(r#"a "q" \ b"#), r#"a \"q\" \\ b"#);
    }

    #[test]
    fn years_from_day_counts() {
        assert_eq!(year_of_days(0), 1970);
        assert_eq!(year_of_days(20_454), 2026); // 2026-01-01
        assert_eq!(year_of_days(20_453), 2025); // 2025-12-31
        assert_eq!(year_of_days(11_016), 2000); // 2000-02-29
    }

    #[test]
    fn carried_pins_survive_and_extras_follow() {
        let tools = vec!["just".to_string(), "rust".to_string()];
        let carried = vec![
            ("rust".to_string(), "\"1.85\"".to_string()),
            ("cargo:cargo-nextest".to_string(), "\"latest\"".to_string()),
        ];
        assert_eq!(
            mise_tools_toml(&tools, &carried),
            "just = \"latest\"\nrust = \"1.85\"\n\"cargo:cargo-nextest\" = \"latest\""
        );
    }

    #[test]
    fn carry_over_drops_banned_and_survives_a_file_that_is_not_toml() {
        let d = std::env::temp_dir().join(format!("carry-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let mise = d.join("mise.toml");
        std::fs::write(&mise, "[tools]\npython = \"3\"\nzig = \"0.14\"\n").unwrap();
        let (carried, note) = carry_over_tools(&d, "python").unwrap();
        assert_eq!(carried, vec![("zig".to_string(), "\"0.14\"".to_string())]);
        assert!(note.is_empty());
        std::fs::write(&mise, "[tools]\nbun = \"1\"\nbun = \"1\"\n").unwrap();
        let (carried, note) = carry_over_tools(&d, "python").unwrap();
        assert!(carried.is_empty());
        assert!(note.contains("not valid TOML") && note.contains("duplicate key"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn mechanical_residue_ignores_spec_slots() {
        assert_eq!(mechanical_residue("x __SPEC_USAGE__ y"), None);
        assert_eq!(mechanical_residue("x __APP_NAME__ y"), Some("APP_NAME"));
    }
}
