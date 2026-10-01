// SPDX-License-Identifier: MPL-2.0
// Copyright (c) Jonathan D.A. Jewell <j.d.a.jewell@open.ac.uk>
//! Merge the provisioning contract into an existing Justfile
//! (`PROVISIONING-STANDARD.adoc` §1: the Justfile is *merged*, never replaced).
//!
//! * `mod provision 'build/just/provision.just'` is added once.
//! * Every contract verb the Justfile does not define gets a root delegation;
//!   a verb it does define (its own `build`, `test`, …) is its override and kept.
//! * A `doctor`, `setup` or `heal` that is estate boilerplate (the generic
//!   "Running diagnostics for" family) is removed: the canon verb does that job
//!   properly. A custom one is renamed `doctor-local` / `setup-local` /
//!   `heal-local`, which the canon verb runs.
//! * Any other contract verb whose body is the unedited RSR template's
//!   placeholder (`# TODO: Replace with your ...`) is removed, so the canon
//!   verb runs instead of a fake pass.
//!
//! `just` itself is the judge of the result: the merge is kept only when
//! `just --summary` afterwards lists every contract verb and every recipe the
//! file had before (renamed ones under their new name). Otherwise the original
//! bytes are restored and the merge is reported as skipped.

use super::mint::{Act, VERBS, delegations};
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Text found only in the generic, repo-agnostic doctor/heal bodies that
/// earlier estate sweeps stamped into Justfiles.
pub const BOILERPLATE: &[&str] = &[
    "Running diagnostics for",
    "Toolchain Health Check",
    "Attempting auto-repair for",
    "Heal — Automatic Tool Installation",
];

/// Text found only in the unedited RSR template's recipe bodies: a contract
/// verb carrying it (`test:` ... `# TODO: Replace with your test command` ...
/// `@echo "Tests passed!"`) is a placeholder that would shadow the canon verb
/// with a fake pass, not the repository's own override.
pub const PLACEHOLDER: &[&str] = &["# TODO: Replace with your"];

/// The verbs a repository may already implement in its own way.
const LOCAL: &[&str] = &["setup", "doctor", "heal"];

const MOD_LINE: &str = "mod provision 'build/just/provision.just'";

/// Fold several justfiles in `target` into one, which `just` needs before it
/// will run at all ("Multiple candidate justfiles found"). The file with the
/// most recipes is kept (`Justfile` on a tie); each other file's recipes that
/// it lacks are appended to it, and a recipe both define keeps the kept file's
/// body unless that body is a template placeholder or boilerplate and the
/// other's is not. The other file is then removed. The result must
/// parse whenever the kept file parsed before, or it is restored, nothing is
/// removed, and the fold is reported as skipped. Returns the kept file's name
/// and one report line per other file.
pub fn fold(target: &Path, names: &[&str]) -> Result<(String, Vec<(String, Act)>)> {
    let mut texts = Vec::new();
    for n in names {
        let p = target.join(n);
        texts
            .push(std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?);
    }
    let keep = (0..names.len())
        .max_by_key(|&i| (recipe_names(&texts[i]).len(), names[i] == "Justfile"))
        .unwrap_or(0);
    let kept = names[keep].to_string();
    let parsed_before = summary(target, &kept).is_ok();
    let mut merged = texts[keep].clone();
    let mut notes = Vec::new();
    for (i, n) in names.iter().enumerate().filter(|&(i, _)| i != keep) {
        let lines: Vec<&str> = texts[i].lines().collect();
        let (mut added, mut shadowed) = (Vec::new(), Vec::new());
        for (h, l) in lines.iter().enumerate() {
            let Some(r) = header_name(l) else { continue };
            let have: Vec<&str> = merged.lines().collect();
            let (s, _, e) = span_at(&lines, h);
            if let Some((hs, _, he)) = span(&have, r) {
                // A template placeholder never beats the repository's own body.
                let stub = |b: &str| PLACEHOLDER.iter().chain(BOILERPLATE).any(|m| b.contains(m));
                let theirs = lines[s..e].join("\n");
                if stub(&have[hs..he].join("\n")) && !stub(&theirs) {
                    let mut out: Vec<&str> = have[..hs].to_vec();
                    out.extend(theirs.lines());
                    out.extend(&have[he..]);
                    merged = out.join("\n") + "\n";
                    added.push(r.to_string());
                } else {
                    shadowed.push(r.to_string());
                }
                continue;
            }
            if !merged.ends_with('\n') {
                merged.push('\n');
            }
            merged.push('\n');
            merged.push_str(&lines[s..e].join("\n"));
            merged.push('\n');
            added.push(r.to_string());
        }
        let why = match (added.is_empty(), shadowed.is_empty()) {
            (true, true) => format!("no recipes; {kept} is the one `just` runs"),
            (true, false) => format!("every recipe is already in {kept}: {}", shadowed.join(" ")),
            (false, true) => format!("folded into {kept}: {}", added.join(" ")),
            (false, false) => format!(
                "folded into {kept}: {}; {kept}'s own kept for: {}",
                added.join(" "),
                shadowed.join(" ")
            ),
        };
        notes.push(((*n).to_string(), why));
    }
    let path = target.join(&kept);
    std::fs::write(&path, &merged).with_context(|| format!("writing {}", path.display()))?;
    if parsed_before {
        if let Err(e) = summary(target, &kept) {
            std::fs::write(&path, &texts[keep])?;
            let why = format!("folding them into {kept} breaks it ({e}): fold them by hand");
            let skipped = notes
                .into_iter()
                .map(|(n, _)| (n, Act::Skipped(why.clone())))
                .collect();
            return Ok((kept, skipped));
        }
    }
    let mut out = Vec::new();
    for (n, why) in notes {
        std::fs::remove_file(target.join(&n)).with_context(|| format!("removing {n}"))?;
        out.push((n, Act::Removed(why)));
    }
    Ok((kept, out))
}

/// Merge into `target/name`. `provision_just` is the canon module's text.
pub fn merge(target: &Path, name: &str, provision_just: &str) -> Result<Act> {
    let path = target.join(name);
    let original =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let before = summary(target, name);

    let lines: Vec<&str> = original.lines().collect();
    let mut drop = vec![false; lines.len()];
    let mut renames: Vec<(usize, &str)> = Vec::new();
    let mut replaced = Vec::new();
    let mut renamed = Vec::new();

    for verb in LOCAL {
        let Some((start, header, end)) = span(&lines, verb) else {
            if before.as_ref().is_ok_and(|b| b.iter().any(|r| r == verb)) {
                return Ok(Act::Skipped(format!(
                    "`{verb}` comes from an import, not {name}: merge it by hand"
                )));
            }
            continue;
        };
        let deps = lines[header].split_once(':').map_or("", |(_, d)| d);
        if deps.contains("provision::") {
            continue; // already a delegation
        }
        let body = lines[header..end].join("\n");
        if BOILERPLATE.iter().any(|m| body.contains(m)) {
            drop[start..end].iter_mut().for_each(|d| *d = true);
            replaced.push(*verb);
        } else {
            let local = format!("{verb}-local");
            if before.as_ref().is_ok_and(|b| b.contains(&local)) || span(&lines, &local).is_some() {
                return Ok(Act::Skipped(format!(
                    "a custom `{verb}` and a `{local}` both exist: merge them by hand"
                )));
            }
            renames.push((header, verb));
            renamed.push(*verb);
        }
    }

    for verb in VERBS.iter().filter(|v| !LOCAL.contains(v)) {
        let Some((start, header, end)) = span(&lines, verb) else {
            continue;
        };
        let body = lines[header..end].join("\n");
        if PLACEHOLDER.iter().any(|m| body.contains(m)) {
            drop[start..end].iter_mut().for_each(|d| *d = true);
            replaced.push(*verb);
        }
    }

    if lines.iter().any(|l| header_name(l) == Some("provision")) {
        return Ok(Act::Skipped(format!(
            "{name} has its own `provision` recipe, which clashes with `mod provision`: rename it by hand"
        )));
    }

    // A file `just` cannot parse is merged only when a mechanical repair is
    // available, each undoing damage an earlier estate sweep did:
    // * boilerplate removal (above);
    // * lines left at column 0 inside a shebang body are re-indented (the body
    //   is one script, so the indent changes nothing it runs);
    // * column-0 `//` comments become `#`;
    // * a duplicate recipe whose body is the Nix sweep's dead `flake.guix`
    //   fallback is dropped (`guix develop` and `flake.guix` do not exist).
    // `just` judges the result below.
    let mut indent = vec![false; lines.len()];
    let mut slashes = vec![false; lines.len()];
    let mut repaired = !replaced.is_empty();
    if before.is_err() {
        for (i, l) in lines.iter().enumerate() {
            let Some(name) = header_name(l) else {
                continue;
            };
            if drop[i] {
                continue;
            }
            let (start, h, end) = span_at(&lines, i);
            let dup = lines
                .iter()
                .filter(|o| header_name(o) == Some(name))
                .count()
                > 1;
            if dup && lines[h..end].iter().any(|b| b.contains("flake.guix")) {
                drop[start..end].iter_mut().for_each(|d| *d = true);
                repaired = true;
                continue;
            }
            if !lines
                .get(h + 1)
                .is_some_and(|b| b.trim_start().starts_with("#!"))
            {
                continue;
            }
            for j in h + 1..end {
                if !lines[j].is_empty() && !lines[j].starts_with([' ', '\t']) {
                    indent[j] = true;
                    repaired = true;
                }
            }
        }
        for (i, l) in lines.iter().enumerate() {
            if l.starts_with("//") && !indent[i] {
                slashes[i] = true;
                repaired = true;
            }
        }
    }
    if let Err(e) = &before
        && !repaired
    {
        return Ok(Act::Skipped(format!(
            "{name} does not parse, and no mechanical repair applies: {e}"
        )));
    }

    let mut out = String::with_capacity(original.len() + 2048);
    for (i, line) in lines.iter().enumerate() {
        if drop[i] {
            continue;
        }
        match renames.iter().find(|(h, _)| *h == i) {
            Some((_, verb)) => {
                let at = line.find(verb).unwrap_or(0);
                out.push_str(&line[..at + verb.len()]);
                out.push_str("-local");
                out.push_str(&line[at + verb.len()..]);
            }
            None => {
                if indent[i] {
                    out.push_str("    ");
                }
                match line.strip_prefix("//").filter(|_| slashes[i]) {
                    Some(rest) => {
                        out.push('#');
                        out.push_str(rest);
                    }
                    None => out.push_str(line),
                }
            }
        }
        out.push('\n');
    }
    // Removing a block can leave a run of blank lines; keep at most one.
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }

    let defined: Vec<String> = recipe_names(&out);
    let has_mod = out.lines().any(|l| {
        let l = l.trim_start();
        (l.starts_with("mod provision") || l.starts_with("mod? provision"))
            && l["mod".len()..]
                .trim_start_matches('?')
                .trim_start()
                .starts_with("provision")
    });
    let added: Vec<&str> = VERBS
        .iter()
        .copied()
        .filter(|v| !defined.iter().any(|d| d == v))
        .collect();
    if has_mod && added.is_empty() && replaced.is_empty() && renamed.is_empty() {
        return Ok(Act::Kept("already merged".into()));
    }
    let block = delegations(provision_just, &defined);
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    out.push_str("# --- Provisioning contract (PROVISIONING-STANDARD §2) ---------------------\n");
    out.push_str(
        "# Merged by `launch-scaffolder provision-set`. A recipe defined above overrides\n",
    );
    out.push_str("# the canon one; doctor/setup/heal run this repo's *-local recipes.\n");
    if !has_mod {
        out.push_str(MOD_LINE);
        out.push_str("\n\n");
    }
    if !block.is_empty() {
        out.push_str(&block);
        out.push('\n');
    }

    std::fs::write(&path, &out).with_context(|| format!("writing {}", path.display()))?;
    let after = summary(target, name);
    let lost = verify(before.as_deref().ok(), after.as_deref(), &renamed);
    if let Some(why) = lost {
        std::fs::write(&path, &original)
            .with_context(|| format!("restoring {}", path.display()))?;
        return Ok(Act::Skipped(format!(
            "merge rejected by `just --summary`, original restored: {why}"
        )));
    }

    let mut what = Vec::new();
    if !added.is_empty() {
        what.push(format!("{} delegation(s) added", added.len()));
    }
    if !replaced.is_empty() {
        what.push(format!(
            "boilerplate {} replaced by the canon",
            replaced.join("/")
        ));
    }
    if !renamed.is_empty() {
        what.push(format!(
            "custom {} kept as {}",
            renamed.join("/"),
            renamed
                .iter()
                .map(|v| format!("{v}-local"))
                .collect::<Vec<_>>()
                .join("/")
        ));
    }
    if before.is_err() {
        what.push("repairs a Justfile that did not parse".into());
    }
    Ok(Act::Replaced(what.join("; ")))
}

/// Why the merged file is unacceptable, or `None`.
fn verify(
    before: Option<&[String]>,
    after: Result<&[String], &anyhow::Error>,
    renamed: &[&str],
) -> Option<String> {
    let after = match after {
        Ok(a) => a,
        Err(e) => return Some(format!("the merged file does not parse: {e}")),
    };
    let has = |r: &str| after.iter().any(|a| a == r);
    let mut missing: Vec<String> = VERBS
        .iter()
        .filter(|v| !has(v))
        .map(|v| v.to_string())
        .collect();
    missing.extend(
        VERBS
            .iter()
            .filter(|v| !has(&format!("provision::{v}")))
            .map(|v| format!("provision::{v}")),
    );
    for r in before.unwrap_or_default() {
        let now = if renamed.contains(&r.as_str()) {
            format!("{r}-local")
        } else {
            r.clone()
        };
        if !has(&now) {
            missing.push(now);
        }
    }
    (!missing.is_empty()).then(|| format!("missing {}", missing.join(", ")))
}

/// `just --summary` for the Justfile, as recipe names.
fn summary(target: &Path, name: &str) -> Result<Vec<String>> {
    let o = Command::new("just")
        .arg("--justfile")
        .arg(target.join(name))
        .arg("--working-directory")
        .arg(target)
        .arg("--summary")
        .output()
        .context("running `just --summary` (is just >= 1.42 on PATH?)")?;
    if !o.status.success() {
        let err = String::from_utf8_lossy(&o.stderr);
        anyhow::bail!("{}", err.lines().next().unwrap_or("just failed").trim());
    }
    Ok(String::from_utf8_lossy(&o.stdout)
        .split_whitespace()
        .map(str::to_string)
        .collect())
}

/// The recipe name a column-0 line declares, if it is a recipe header.
fn header_name(line: &str) -> Option<&str> {
    let l = line.strip_prefix('@').unwrap_or(line);
    let end = l.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))?;
    let name = &l[..end];
    if name.is_empty() || !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    if matches!(
        name,
        "set"
            | "alias"
            | "export"
            | "import"
            | "mod"
            | "if"
            | "else"
            | "fi"
            | "for"
            | "done"
            | "then"
    ) {
        return None;
    }
    // Parameters are words, optionally `=default` (quoted or bare), then `:`.
    let mut rest = &l[end..];
    loop {
        rest = rest.trim_start_matches([' ', '\t']);
        if let Some(r) = rest.strip_prefix(':') {
            return (!r.starts_with('=')).then_some(name);
        }
        let r = rest.trim_start_matches(['+', '*', '$']);
        let w = r
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(r.len());
        if w == 0 {
            return None;
        }
        rest = &r[w..];
        if let Some(r) = rest.strip_prefix('=') {
            rest = match r.chars().next() {
                Some(q @ ('"' | '\'')) => &r[1..][r[1..].find(q)? + 1..],
                _ => &r[r.find([' ', ':']).unwrap_or(r.len())..],
            };
        }
    }
}

/// A column-0 line that starts something new: a header or a directive.
fn starts_item(line: &str) -> bool {
    header_name(line).is_some()
        || ["set ", "alias ", "export ", "import", "mod ", "mod? ", "["]
            .iter()
            .any(|p| line.starts_with(p))
}

/// `(first line of the doc comment, header line, end)` of recipe `name`.
///
/// The body is everything up to the next column-0 header or directive (and
/// that item's own doc comment), so a body with stray unindented lines, which
/// `just` rejects, is still taken whole.
fn span(lines: &[&str], name: &str) -> Option<(usize, usize, usize)> {
    let header = lines.iter().position(|l| header_name(l) == Some(name))?;
    Some(span_at(lines, header))
}

/// [`span`] of the recipe whose header is line `header`.
fn span_at(lines: &[&str], header: usize) -> (usize, usize, usize) {
    let mut start = header;
    while start > 0 && (lines[start - 1].starts_with('#') || lines[start - 1].starts_with('[')) {
        start -= 1;
    }
    let mut end = header + 1;
    while end < lines.len() && !starts_item(lines[end]) {
        end += 1;
    }
    // Trailing comments and blanks before the next item belong to it.
    while end > header + 1 {
        let l = lines[end - 1];
        if l.trim().is_empty() || l.starts_with('#') {
            end -= 1;
        } else {
            break;
        }
    }
    (start, header, end)
}

fn recipe_names(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(header_name)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_recognised_and_directives_are_not() {
        assert_eq!(header_name("build:"), Some("build"));
        assert_eq!(header_name("@doctor: build"), Some("doctor"));
        assert_eq!(header_name("release version:"), Some("release"));
        assert_eq!(header_name("ai-warmup who=\"user: x\":"), Some("ai-warmup"));
        assert_eq!(header_name("serve port='8080' *args:"), Some("serve"));
        assert_eq!(header_name("x := \"y\""), None);
        assert_eq!(header_name("set shell := [\"bash\"]"), None);
        assert_eq!(header_name("if command -v x; then"), None);
        assert_eq!(header_name("    echo hi:"), None);
        assert_eq!(header_name("# doctor:"), None);
    }

    #[test]
    fn a_span_takes_stray_unindented_lines_and_leaves_the_next_doc() {
        let src = "# Diagnose\ndoctor:\n    #!/usr/bin/env bash\n    a\n# Optional tools\nif x; then\n    b\nfi\n    c\n\n# Repair\nheal:\n    d\n";
        let lines: Vec<&str> = src.lines().collect();
        assert_eq!(span(&lines, "doctor"), Some((0, 1, 9)));
        assert_eq!(span(&lines, "heal"), Some((10, 11, 13)));
    }

    fn repo(justfile: &str) -> std::path::PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!("justfile-test-{}-{n}", std::process::id()));
        let canon = crate::provisioning::canon::Canon::Baked;
        for rel in crate::provisioning::canon::ENGINE_FILES {
            let p = d.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, canon.file(rel).unwrap()).unwrap();
        }
        std::fs::write(d.join("Justfile"), justfile).unwrap();
        d
    }

    fn provision_just() -> String {
        String::from_utf8(
            crate::provisioning::canon::Canon::Baked
                .file("build/just/provision.just")
                .unwrap()
                .into_owned(),
        )
        .unwrap()
    }

    const BROKEN: &str = "# Build\nbuild:\n    cargo build\n\n# Self-diagnostic\ndoctor:\n    #!/usr/bin/env bash\n    echo \"Running diagnostics for x\"\nif command -v y >/dev/null; then\n    echo ok\nfi\n\n# Help\nhelp-me:\n    #!/usr/bin/env bash\n    echo \"\"\necho \"FIRST TIME SETUP:\"\n";

    #[test]
    fn identical_justfiles_fold_to_one_and_the_copy_is_removed() {
        let src = "# Build\nbuild:\n    echo b\n";
        let d = repo(src);
        std::fs::write(d.join("justfile"), src).unwrap();
        let (kept, acts) = fold(&d, &["Justfile", "justfile"]).unwrap();
        assert_eq!(kept, "Justfile");
        assert_eq!(acts.len(), 1);
        assert!(
            matches!(&acts[0].1, Act::Removed(w) if w.contains("already in Justfile")),
            "{acts:?}"
        );
        assert!(!d.join("justfile").exists());
        assert_eq!(std::fs::read_to_string(d.join("Justfile")).unwrap(), src);
        std::fs::remove_dir_all(&d).unwrap();
    }

    /// The merge and fold tests run `just --summary`, so need `just` >= 1.42 on PATH.
    /// rust-ci skips this module by name; launcher-artefacts runs and counts it.
    mod needs_just {
        use super::*;

        #[test]
        fn boilerplate_is_replaced_and_a_broken_file_repaired() {
            let d = repo(BROKEN);
            assert!(
                summary(&d, "Justfile").is_err(),
                "the control must not parse"
            );
            let act = merge(&d, "Justfile", &provision_just()).unwrap();
            assert!(matches!(act, Act::Replaced(_)), "{act}");
            let after = summary(&d, "Justfile").unwrap();
            for v in VERBS {
                assert!(after.iter().any(|r| r == v), "{v} missing");
            }
            assert!(after.iter().any(|r| r == "help-me") && after.iter().any(|r| r == "build"));
            let text = std::fs::read_to_string(d.join("Justfile")).unwrap();
            assert!(!text.contains("Running diagnostics for"));
            assert!(text.contains("    echo \"FIRST TIME SETUP:\""));
            assert_eq!(
                merge(&d, "Justfile", &provision_just()).unwrap(),
                Act::Kept("already merged".into())
            );
        }

        #[test]
        fn a_custom_doctor_becomes_doctor_local() {
            let d = repo("doctor:\n    @echo mine\n");
            merge(&d, "Justfile", &provision_just()).unwrap();
            let after = summary(&d, "Justfile").unwrap();
            assert!(
                after.iter().any(|r| r == "doctor-local") && after.iter().any(|r| r == "doctor")
            );
            let clash = repo("doctor:\n    @echo a\ndoctor-local:\n    @echo b\n");
            assert!(matches!(
                merge(&clash, "Justfile", &provision_just()).unwrap(),
                Act::Skipped(_)
            ));
        }

        #[test]
        fn a_template_placeholder_verb_is_replaced_and_a_real_one_kept() {
            let d = repo(
                "test *args:\n    @echo \"Running tests...\"\n    # TODO: Replace with your test command\n    @echo \"Tests passed!\"\n\nbench:\n    cargo bench\n",
            );
            let act = merge(&d, "Justfile", &provision_just()).unwrap();
            assert!(
                matches!(act, Act::Replaced(ref w) if w.contains("test")),
                "{act}"
            );
            let text = std::fs::read_to_string(d.join("Justfile")).unwrap();
            assert!(!text.contains("Tests passed!"), "the placeholder must go");
            assert!(
                text.contains("test: provision::test"),
                "the canon verb must take over"
            );
            assert!(
                text.contains("    cargo bench"),
                "a real override must stay"
            );
            assert!(!text.contains("bench: provision::bench"));
        }

        #[test]
        fn sweep_damage_is_repaired_and_a_provision_recipe_refused() {
            let src = "// SPDX-License-Identifier: MPL-2.0\n\nguix-shell:\n    guix shell -D -f guix.scm\n\n# fallback\nguix-shell:\n    @if [ -f \"flake.guix\" ]; then guix develop; fi\n";
            let d = repo(src);
            assert!(
                summary(&d, "Justfile").is_err(),
                "the control must not parse"
            );
            assert!(matches!(
                merge(&d, "Justfile", &provision_just()).unwrap(),
                Act::Replaced(_)
            ));
            let text = std::fs::read_to_string(d.join("Justfile")).unwrap();
            assert!(text.starts_with("# SPDX") && !text.contains("flake.guix"));
            assert!(text.contains("guix shell -D -f guix.scm"));
            let clash = repo("provision:\n    @echo mine\n");
            assert!(matches!(
                merge(&clash, "Justfile", &provision_just()).unwrap(),
                Act::Skipped(_)
            ));
        }

        #[test]
        fn an_unrepairable_file_is_left_byte_identical() {
            let src = "build:\n    cargo build\nthis is not just syntax\n";
            let d = repo(src);
            assert!(matches!(
                merge(&d, "Justfile", &provision_just()).unwrap(),
                Act::Skipped(_)
            ));
            assert_eq!(std::fs::read_to_string(d.join("Justfile")).unwrap(), src);
        }

        #[test]
        fn a_real_body_replaces_a_template_placeholder_of_the_same_name() {
            // The kept file is the bigger, unedited RSR template; the other file holds
            // the recipe the author actually wrote.
            let d = repo(
                "# Build\nbuild:\n    # TODO: Replace with your build command\n    @echo built\n\nci:\n    echo ci\n\ndocs:\n    echo d\n",
            );
            std::fs::write(d.join("justfile"), "build:\n    cargo build --release\n").unwrap();
            let (kept, acts) = fold(&d, &["Justfile", "justfile"]).unwrap();
            assert_eq!(kept, "Justfile");
            let text = std::fs::read_to_string(d.join("Justfile")).unwrap();
            assert!(
                text.contains("cargo build --release") && !text.contains("TODO"),
                "{text}"
            );
            assert!(
                matches!(&acts[0].1, Act::Removed(w) if w.starts_with("folded into Justfile: build")),
                "{acts:?}"
            );
            assert_eq!(summary(&d, "Justfile").unwrap(), ["build", "ci", "docs"]);
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn the_richer_justfile_is_kept_and_the_others_recipes_join_it() {
            // action-trust-layers' shape: the real recipes are in the lowercase file.
            let d = repo("# Help\nhelp:\n    echo h\n");
            std::fs::write(
            d.join("justfile"),
            "# Build\nbuild:\n    cargo build\n\n# Test\ntest:\n    cargo test\n\nhelp:\n    echo other\n",
        )
        .unwrap();
            let (kept, acts) = fold(&d, &["Justfile", "justfile"]).unwrap();
            assert_eq!(kept, "justfile");
            let text = std::fs::read_to_string(d.join("justfile")).unwrap();
            assert_eq!(text.matches("help:").count(), 1, "{text}");
            assert!(
                text.contains("cargo build") && text.contains("echo other"),
                "{text}"
            );
            assert!(!d.join("Justfile").exists());
            assert!(
                matches!(&acts[0].1, Act::Removed(w) if w.contains("help")),
                "{acts:?}"
            );
            assert_eq!(summary(&d, "justfile").unwrap(), ["build", "help", "test"]);
            std::fs::remove_dir_all(&d).unwrap();
        }

        #[test]
        fn a_fold_that_would_break_the_kept_file_is_undone() {
            // `x` is a variable in the kept file; appending a recipe that reassigns it
            // is a parse error, so nothing may be removed.
            let kept = "x := \"1\"\n\nbuild:\n    echo {{x}}\n\ntest:\n    echo t\n";
            let d = repo(kept);
            std::fs::write(d.join(".justfile"), "lint:\n    echo {{y}}\n").unwrap();
            let (k, acts) = fold(&d, &["Justfile", ".justfile"]).unwrap();
            assert_eq!(k, "Justfile");
            assert!(
                matches!(&acts[0].1, Act::Skipped(w) if w.contains("by hand")),
                "{acts:?}"
            );
            assert!(d.join(".justfile").exists());
            assert_eq!(std::fs::read_to_string(d.join("Justfile")).unwrap(), kept);
            std::fs::remove_dir_all(&d).unwrap();
        }
    }
}
