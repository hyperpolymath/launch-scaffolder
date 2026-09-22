// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! `launcher-standard_praxis.deed` loader.
//!
//! The standard file is the canonical declarative description of a compliant
//! launcher. Owner ruling **D73-C** converted it from `.a2ml` to the estate's
//! live `.deed` format, so this module reads DEED via [`crate::deed`] rather
//! than TOML.
//!
//! Two things about the format are load-bearing here and are easy to get
//! wrong, so they are stated rather than left to be rediscovered:
//!
//! 1. **A deed carries two versions.** `:schema-version` is the version of the
//!    DEED *grammar* (`"1.0.0"`); `:standard-version` is the version of *this
//!    document* (`"0.4.0"`). [`LauncherStandard::spec_version`] is the second.
//!    Reading the first produces a confident wrong answer rather than an
//!    error — and because `1.0.0` sorts above `0.4.0`, the mistake reads as an
//!    upgrade.
//! 2. **Order is not semantic.** The resolution ladders carry precedence in an
//!    explicit `:priority` integer, and the deed's own comment says consumers
//!    MUST consult them in ASCENDING priority order. Nothing here may depend
//!    on file position; [`crate::deed::Node::children_by_priority`] is the
//!    only way rungs are read.

use crate::Result;
use crate::deed::{self, Node};
use anyhow::Context;
use std::path::{Path, PathBuf};

/// The default standard, baked into the binary at build time. Override with
/// `--standard <file>` or `$LAUNCH_SCAFFOLDER_STANDARD` for dev workflows.
pub const BAKED_STANDARD: &str = include_str!("../../../standards/launcher-standard_praxis.deed");

/// Clauses a standard must carry for this tool to do anything useful with it.
/// A file that parses as a deed but lacks these is a *different* deed, not a
/// damaged launcher standard, and the error should say so.
const REQUIRED_CLAUSES: [&str; 5] = [
    "resolution",
    "required-modes",
    "runtime",
    "integration",
    "metadata-block",
];

/// A parsed launcher standard.
#[derive(Debug, Clone)]
pub struct LauncherStandard {
    /// The whole parsed document. Consumers navigate it with the [`deed`]
    /// helpers rather than being handed a pre-flattened map, because the
    /// standard's shape is still evolving and a flattening would have to
    /// change with it.
    pub doc: Node,
    /// The standard's own version, from `:standard-version`.
    pub spec_version: String,
}

impl LauncherStandard {
    /// Load the baked-in standard.
    pub fn baked() -> Result<Self> {
        Self::parse(BAKED_STANDARD)
    }

    /// Load a standard from a file on disk.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading standard {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing standard {}", path.display()))
    }

    /// Parse a standard from an in-memory string.
    pub fn parse(text: &str) -> Result<Self> {
        if looks_like_the_old_toml_format(text) {
            anyhow::bail!(
                "this looks like the retired TOML/A2ML launcher standard. The launcher \
                 standard is now a praxis DEED (`launcher-standard_praxis.deed`, owner \
                 ruling D73-C); see hyperpolymath/standards#960"
            );
        }

        let doc = deed::parse(text).context("standard is not a valid deed")?;

        for required in REQUIRED_CLAUSES {
            if doc.clause(required).is_none() {
                anyhow::bail!("standard is missing the required clause ({required})");
            }
        }

        // Asserted by name, not by position: the module doc explains why file
        // order is never load-bearing, and `(resolution)` in the real deed
        // happens to list `desktop-tools-search` first.
        let resolution = doc.clause("resolution").expect("checked above");
        if resolution.clause(STANDARD_SEARCH).is_none() {
            anyhow::bail!(
                "standard's (resolution) clause is missing ({STANDARD_SEARCH}) — without it \
                 this tool cannot locate the standard on a host that does not use the \
                 eclipse-mount layout"
            );
        }

        let spec_version = doc
            .str_field("standard-version")
            .context(
                "standard is missing :standard-version. Note this is NOT :schema-version, \
                 which is the version of the DEED grammar itself",
            )?
            .to_string();

        Ok(Self { doc, spec_version })
    }

    /// Resolve a standard using the documented precedence:
    ///
    /// 1. An explicit file path (typically from `--standard <FILE>` or
    ///    `$LAUNCH_SCAFFOLDER_STANDARD`, which clap already merges into one
    ///    `Option`).
    /// 2. The `(resolution)(standard-search)` ladder from the baked standard,
    ///    walked in ASCENDING `:priority` order — first existing path wins.
    /// 3. The baked-in fallback compiled into the binary at build time.
    ///
    /// This replaces a single hard-coded `/var/mnt/eclipse/...` path, which
    /// silently fell through to the baked copy on every host that does not use
    /// that layout — a downgrade with no diagnostic.
    pub fn resolve(flag: Option<&Path>) -> Result<Self> {
        Self::resolve_with(flag, |k| std::env::var(k).ok(), |p| p.exists())
    }

    /// [`Self::resolve`] with the environment and the filesystem injected.
    ///
    /// Tests must use this rather than `resolve`. `std::env::set_var` is
    /// process-global while cargo runs tests in parallel threads, so an
    /// env-mutating test corrupts its neighbours rather than isolating itself.
    pub fn resolve_with(
        flag: Option<&Path>,
        env: impl Fn(&str) -> Option<String>,
        exists: impl Fn(&Path) -> bool,
    ) -> Result<Self> {
        if let Some(path) = flag {
            tracing::debug!("loading standard from flag: {}", path.display());
            return Self::load(path);
        }

        for path in Self::search_ladder(&env) {
            if exists(&path) {
                tracing::debug!("loading standard from ladder: {}", path.display());
                return Self::load(&path);
            }
        }

        // Deliberately `info!`, not `debug!`. Falling back to the baked copy
        // means the on-disk standard was not found, so this build may be
        // rendering launchers against a stale spec. That is worth seeing at
        // default verbosity.
        tracing::info!(
            "no standard found on the (standard-search) ladder; using the baked copy \
             (version {})",
            baked_version().unwrap_or("unknown")
        );
        Self::baked()
    }

    /// The `(resolution)(standard-search)` rungs of the *baked* standard, in
    /// ascending `:priority` order, with `$VAR` expanded.
    ///
    /// The ladder is read from the baked copy by necessity: this is the code
    /// that finds the on-disk standard, so it cannot already have it.
    fn search_ladder(env: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
        let Ok(baked) = deed::parse(BAKED_STANDARD) else {
            return Vec::new();
        };
        let Some(search) = baked
            .clause("resolution")
            .and_then(|r| r.clause(STANDARD_SEARCH))
        else {
            return Vec::new();
        };

        ladder_from(search, env)
    }
}

/// Read one `*-search` clause's rungs into concrete paths.
///
/// Split out from [`LauncherStandard::search_ladder`] so a test can feed it a
/// document whose rungs are NOT already in ascending file order. Against the
/// real deed the two are indistinguishable — its rungs happen to be written
/// 10, 20, 30, 40, 50 — so a test that can only see the real deed asserts the
/// ordering rule without being able to detect its absence. That was measured,
/// not assumed: a mutant replacing `children_by_priority` with file order left
/// every test in this module green.
fn ladder_from(search: &Node, env: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    search
        .children_by_priority("path")
        .into_iter()
        .filter_map(|rung| rung.str_field("value"))
        .filter_map(|v| expand_vars(v, env))
        .map(PathBuf::from)
        .collect()
}

/// The ladder that locates this file itself, as named in the deed.
const STANDARD_SEARCH: &str = "standard-search";

/// Expand `$NAME` occurrences, or return `None` if any is unset.
///
/// Returning `None` is the whole point. `unwrap_or_default()` would turn an
/// unset `$HP_ESTATE_ROOT` into the path `/standards/launcher/...`, which is a
/// real absolute path that could exist and is emphatically not the rung the
/// standard described. An unset variable means "this rung does not apply on
/// this host", so the rung is skipped.
fn expand_vars(value: &str, env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(idx) = rest.find('$') {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + 1..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(after.len());
        if end == 0 {
            // A bare `$` is not a variable reference; keep it verbatim.
            out.push('$');
            rest = after;
            continue;
        }
        let (name, tail) = after.split_at(end);
        out.push_str(&lookup(name, env)?);
        rest = tail;
    }
    out.push_str(rest);
    Some(out)
}

/// Resolve one variable, honouring the single documented default.
fn lookup(name: &str, env: &impl Fn(&str) -> Option<String>) -> Option<String> {
    if let Some(v) = env(name) {
        return Some(v);
    }
    // The deed documents this default in the rung's own `:note`:
    // "XDG default; XDG_DATA_HOME defaults to $HOME/.local/share". It is the
    // only variable with a fallback, and honouring it is what makes the XDG
    // rung usable on a host that has never exported XDG_DATA_HOME — which is
    // most of them.
    if name == "XDG_DATA_HOME" {
        return env("HOME").map(|h| format!("{h}/.local/share"));
    }
    None
}

/// Detect the retired TOML/A2ML standard so the error can name the cure.
///
/// Both tells are checked because either alone has a false negative: a TOML
/// file may open with a comment rather than a `[section]`, and a file of only
/// section headers carries no `key = "value"` line.
fn looks_like_the_old_toml_format(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        l.starts_with('[') || (l.contains(" = ") && !l.starts_with(';'))
    })
}

/// The `:standard-version` of the baked copy, for diagnostics.
fn baked_version() -> Option<&'static str> {
    BAKED_STANDARD
        .lines()
        .find_map(|l| l.trim().strip_prefix(":standard-version"))
        .map(|v| v.trim().trim_matches('"'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deed::Value;
    use std::collections::HashMap;

    /// Build an `env` closure from pairs. Never `std::env::set_var` — cargo
    /// runs tests in parallel threads and the process environment is shared.
    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn baked_standard_parses_and_reports_its_own_version() {
        let s = LauncherStandard::baked().expect("baked standard must parse");

        // Derived from the baked source rather than hardcoded, so the check
        // cannot rot when the standard is bumped. An earlier version of this
        // test hardcoded "0.1.0" and failed for a reason that had nothing to
        // do with parsing, which is what it is named for.
        let declared = baked_version().expect("baked standard must declare :standard-version");
        assert_eq!(s.spec_version, declared);
        assert!(!s.spec_version.is_empty());
    }

    /// The trap this module exists to avoid: the two versions are different
    /// values on the same form, and both look like plausible spec versions.
    #[test]
    fn spec_version_is_standard_version_not_schema_version() {
        let s = LauncherStandard::baked().unwrap();
        let schema = s
            .doc
            .str_field("schema-version")
            .expect("a deed always carries :schema-version");
        assert_ne!(
            s.spec_version, schema,
            "spec_version must be :standard-version, not the DEED grammar version"
        );
        assert_eq!(schema, "1.0.0", "the DEED grammar version");
        assert_eq!(s.spec_version, "0.4.0", "the standard's own version");
    }

    #[test]
    fn every_required_clause_is_present_in_the_baked_standard() {
        let s = LauncherStandard::baked().unwrap();
        for required in REQUIRED_CLAUSES {
            assert!(
                s.doc.clause(required).is_some(),
                "baked standard must carry ({required})"
            );
        }
        assert!(
            s.doc
                .clause("resolution")
                .and_then(|r| r.clause(STANDARD_SEARCH))
                .is_some()
        );
    }

    /// The old format must be diagnosed, not merely rejected. A bare "not a
    /// valid deed" would send a reader hunting for a syntax error in a file
    /// that is simply the wrong format.
    #[test]
    fn the_retired_toml_standard_is_named_in_the_error() {
        let err = LauncherStandard::parse("[spec]\nversion = \"0.2.0\"\n").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("praxis DEED"), "{msg}");
        assert!(msg.contains("standards#960"), "{msg}");
    }

    /// The teeth for the ordering rule. The rungs below are deliberately
    /// written out of order (30, 10, 20) so that reading them in file order
    /// and reading them by `:priority` give different answers — which is the
    /// only way this test can fail when the rule is broken.
    #[test]
    fn ladder_is_ordered_by_priority_even_when_file_order_disagrees() {
        let text = concat!(
            ";; SPDX-License-Identifier: MPL-2.0\n",
            "(praxis-deed :schema-version \"1.0.0\" :standard-version \"9.9.9\"\n",
            "  (resolution (standard-search\n",
            "    (path :priority 30 :value \"/third\")\n",
            "    (path :priority 10 :value \"/first\")\n",
            "    (path :priority 20 :value \"/second\")))\n",
            "  (required-modes) (runtime) (integration) (metadata-block))\n"
        );
        let doc = deed::parse(text).expect("fixture must parse");
        let search = doc
            .clause("resolution")
            .and_then(|r| r.clause(STANDARD_SEARCH))
            .expect("fixture must carry the ladder");

        // Guard the guard: if the fixture ever gets tidied into ascending file
        // order this test silently loses its teeth, so assert the disorder.
        let file_order: Vec<i64> = search
            .clauses_named("path")
            .filter_map(|p| p.field("priority").and_then(Value::as_int))
            .collect();
        assert_eq!(file_order, vec![30, 10, 20], "fixture must stay scrambled");

        let ladder = ladder_from(search, &env_of(&[]));
        assert_eq!(
            ladder,
            vec![
                PathBuf::from("/first"),
                PathBuf::from("/second"),
                PathBuf::from("/third")
            ]
        );
    }

    #[test]
    fn baked_ladder_expands_and_keeps_its_documented_order() {
        let env = env_of(&[
            ("HP_ESTATE_ROOT", "/estate"),
            ("XDG_DATA_HOME", "/xdg"),
            ("HOME", "/home/u"),
        ]);
        let ladder = LauncherStandard::search_ladder(&env);
        assert!(!ladder.is_empty(), "the baked ladder must yield rungs");

        // priority 10 is $HP_ESTATE_ROOT, 20 is $XDG_DATA_HOME, 30 is the
        // eclipse mount. Asserting the first two positions pins the order.
        assert_eq!(
            ladder[0],
            PathBuf::from("/estate/standards/launcher/launcher-standard_praxis.deed")
        );
        assert_eq!(
            ladder[1],
            PathBuf::from("/xdg/hyperpolymath/standards/launcher/launcher-standard_praxis.deed")
        );
    }

    /// An unset variable must remove its rung, never expand to empty. The
    /// failure mode being guarded is silent and plausible: `""` would turn
    /// `$HP_ESTATE_ROOT/standards/...` into `/standards/...`, a real absolute
    /// path that is not what the standard named.
    #[test]
    fn an_unset_variable_skips_its_rung_rather_than_expanding_to_empty() {
        let env = env_of(&[("HOME", "/home/u")]);
        let ladder = LauncherStandard::search_ladder(&env);
        for p in &ladder {
            assert!(
                !p.starts_with("/standards"),
                "unset HP_ESTATE_ROOT leaked an empty expansion: {}",
                p.display()
            );
        }
        // $HOME is set, so the two $HOME rungs and the literal eclipse rung
        // survive; $HP_ESTATE_ROOT is not, so its rung does not.
        assert!(ladder.iter().any(|p| p.starts_with("/var/mnt/eclipse")));
        assert!(ladder.iter().any(|p| p.starts_with("/home/u")));
    }

    /// The one documented default, stated in the rung's own `:note`.
    #[test]
    fn xdg_data_home_defaults_to_home_local_share() {
        let env = env_of(&[("HOME", "/home/u")]);
        let ladder = LauncherStandard::search_ladder(&env);
        assert!(
            ladder.iter().any(|p| p.starts_with("/home/u/.local/share")),
            "XDG_DATA_HOME must default to $HOME/.local/share: {ladder:?}"
        );
    }

    /// With neither `$HOME` nor `$XDG_DATA_HOME`, the XDG rung must vanish
    /// rather than producing `/.local/share/...`.
    #[test]
    fn xdg_rung_vanishes_when_home_is_also_unset() {
        let env = env_of(&[]);
        let ladder = LauncherStandard::search_ladder(&env);
        for p in &ladder {
            assert!(!p.starts_with("/.local"), "{}", p.display());
        }
    }

    #[test]
    fn resolve_with_takes_the_first_existing_rung_and_ignores_later_ones() {
        let env = env_of(&[("HP_ESTATE_ROOT", "/nope"), ("HOME", "/home/u")]);
        let ladder = LauncherStandard::search_ladder(&env);
        let wanted = ladder[1].clone();

        // Claim every rung exists from index 1 on. The resolver must pick
        // index 1, proving it stops at the first hit rather than the last.
        let exists = move |p: &Path| p != ladder[0];
        let err = LauncherStandard::resolve_with(
            None,
            env_of(&[("HP_ESTATE_ROOT", "/nope"), ("HOME", "/home/u")]),
            exists,
        )
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains(&wanted.display().to_string()),
            "should have tried {} first, said: {msg}",
            wanted.display()
        );
    }

    /// With no rung existing, the baked copy is used — and it is a real,
    /// complete standard, not a stub.
    #[test]
    fn falls_back_to_the_baked_copy_when_no_rung_exists() {
        let s = LauncherStandard::resolve_with(None, env_of(&[]), |_| false)
            .expect("baked fallback must work");
        assert_eq!(s.spec_version, "0.4.0");
    }

    /// Pin the vendored standard by content hash.
    ///
    /// `include_str!` reaches outside this crate into `standards/`, so an
    /// edit there changes this binary's behaviour with no diff in this crate
    /// and no test naming the change. The pin makes that edit announce
    /// itself. The digest is sha256 of the file as vendored from
    /// `hyperpolymath/standards` (git blob `c751c4ec`); update both together.
    #[test]
    fn the_vendored_standard_is_pinned_by_content() {
        use sha2::{Digest, Sha256};
        let got = format!("{:x}", Sha256::digest(BAKED_STANDARD.as_bytes()));
        assert_eq!(
            got, "73dd64f3d2a2282c9bfee06acf4a1d511bcc7ded5c7b30dcf15b6f85c8e4f871",
            "standards/launcher-standard_praxis.deed changed; re-vendor deliberately \
             and update this pin in the same commit"
        );
    }

    /// Values are reachable through the deed API, so the template layer is not
    /// left needing a flattening this module deliberately does not provide.
    #[test]
    fn ladder_rungs_carry_the_value_field_the_real_deed_uses() {
        let s = LauncherStandard::baked().unwrap();
        let rungs = s
            .doc
            .clause("resolution")
            .unwrap()
            .clause(STANDARD_SEARCH)
            .unwrap()
            .children_by_priority("path");
        assert_eq!(rungs.len(), 5);
        for r in &rungs {
            assert!(matches!(r.field("value"), Some(Value::Str(_))));
            assert!(r.field("priority").and_then(Value::as_int).is_some());
        }
    }
}
