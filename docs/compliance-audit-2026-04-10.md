<!-- SPDX-License-Identifier: PMPL-1.0-or-later -->
# Launcher Compliance Audit — 2026-04-10

> **FROZEN SNAPSHOT.** This document records the pre-migration state
> of the 11 hand-written launchers in `.desktop-tools/` as of
> 2026-04-10, *before* any `launch-scaffolder` work. Do not update it
> to reflect current state — the findings here are the historical
> justification for subsequent scaffolder design decisions, and later
> documents (see below) cross-reference this one by its frozen claims.
>
> **For current state of launcher management, read instead:**
>
> - `README.adoc` — live subcommand surface and architecture
> - `.machine_readable/6a2/STATE.a2ml` — current milestone + completion percentage
> - `docs/launcher-exceptions-2026-04-10.md` — reconciliation against this audit, including one correction (opsm runtime-shape classification; see its "Discrepancy 1" section)
> - `docs/branch-protection-remediation-2026-04-10.md` — estate-wide ruleset remediation that followed the scaffolder work
>
> Of the 11 launchers audited here, 6 have since been migrated to
> scaffolder management (aerie, burble, game-server-admin, nqc, panll,
> project-wharf — plus stapeln, which the audit did not cover because
> stapeln's launcher lived under `stapeln/scripts/` not
> `.desktop-tools/`). The remaining 5 are the declared exceptions
> (hypatia, invariant-path, opsm, ambientops, idaptik) documented
> with migration triggers in `docs/launcher-exceptions-2026-04-10.md`.

Read-only audit of the 11 hand-written launchers in
`/var/mnt/eclipse/repos/.desktop-tools/*-launcher.sh` against:

- `standards/docs/UX-standards/launcher-standard.adoc`
- `standards/docs/UX-standards/LM-LA-LIFECYCLE-STANDARD.adoc`

No launcher was modified. This document is the only file written inside
`launch-scaffolder/` for this audit.

## Scope

| # | Launcher | LOC |
|---|---|---|
| 1 | `aerie-launcher.sh` | 104 |
| 2 | `ambientops-launcher.sh` | 124 |
| 3 | `burble-launcher.sh` | 152 |
| 4 | `game-server-admin-launcher.sh` | 104 |
| 5 | `hypatia-launcher.sh` | 73 |
| 6 | `idaptik-launcher.sh` | 190 |
| 7 | `invariant-path-launcher.sh` | 173 |
| 8 | `nqc-launcher.sh` | 102 |
| 9 | `opsm-launcher.sh` | 116 |
| 10 | `panll-launcher.sh` | 161 |
| 11 | `project-wharf-launcher.sh` | 104 |

## Standard required modes

Per `launcher-standard.adoc §Standard Modes`:

`--start`, `--stop`, `--status`, `--auto` (default), `--browser` (alias of
`--auto`), `--integ`, `--disinteg`, `--help` / `-h`.

## Per-launcher compliance matrix

Legend: ✅ present · ⚠ implicit (falls through `*` to another mode) · ❌ missing · — N/A for runtime shape

| Launcher | start | stop | status | auto | browser | integ | disinteg | help | Extra modes | Runtime shape |
|---|---|---|---|---|---|---|---|---|---|---|
| aerie | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ⚠ | — | background process (wraps `repo-quicklaunch.sh → just tour`) |
| ambientops | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ⚠ | `--toggle-input-devices`, `--emergency-input-restore` | background process |
| burble | ✅ | ✅ | ✅ | ✅ | ⚠ (via `*`) | ❌ | ❌ | ⚠ | — | server-with-URL (`http://localhost:4020`, Phoenix) |
| game-server-admin | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ⚠ | `--gossamer` (alias of `--start`) | background process (re-invokes `~/.local/bin/game-server-admin-launcher --gossamer`) |
| hypatia | ❌ | ❌ | ✅ | ✅ | ❌ | ❌ | ❌ | ✅ | `--tour`, `--gui`, `--local`, `--dev`, `--cli`, `--scan` | remote web app (`https://nesy-prover.dev`), plus `exec`-based CLI subcommand |
| idaptik | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ⚠ | `--web`, `--gossamer`, `--tray` | server-with-URL (`http://localhost:8080`, Deno) |
| invariant-path | — | — | ✅ | ✅ | — | ❌ | ❌ | ✅ | `--scan`, `--scan-file`, `--cli`, `--open-output` | bespoke one-shot CLI (scan-on-demand, no daemon) |
| nqc | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ⚠ | — | background process (GUI via `~/.bin/nqc --gui`) |
| opsm | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ⚠ | `--system-update` | bespoke (`bash -lc` wrapper around `opsm-runtime` / profile-sourced functions) |
| panll | ✅ | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ⚠ | bare-word aliases: `serve`, `start`, `stop`, `status`, `browser`, `web`, `dev` | server-with-URL (`http://localhost:8000/public/`, `just serve`) |
| project-wharf | ✅ | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ | ⚠ | — | background process (wraps `repo-quicklaunch.sh → just tour`) |

## Per-launcher security notes

`set -euo pipefail`, variable quoting, `eval`, world-writable `/tmp` PID files,
and unpinned `curl | bash`.

| Launcher | `set -euo pipefail` | Quoting | `eval`? | `/tmp` PID file | `curl \| bash` | Other |
|---|---|---|---|---|---|---|
| aerie | ✅ | ✅ | none | `/tmp/aerie.pid` (standard-compliant predictable name, no `mktemp`) | none | Hardcoded `/home/hyper/Desktop/Repo-Projects/launchers/repo-quicklaunch.sh` — portability, not security |
| ambientops | ✅ | ✅ | none | `/tmp/ambientops.pid` | none | Same hardcoded `repo-quicklaunch.sh` path |
| burble | ✅ | ✅ | none | `/tmp/burble-server.pid` | none | `curl` used only for local readiness probe against `$URL`; not piped to shell |
| game-server-admin | ✅ | ✅ | none | `/tmp/game-server-admin.pid` | none | Hardcoded `/home/hyper/.local/bin/game-server-admin-launcher` (double-indirection to another launcher) |
| hypatia | ✅ | ✅ | none | no PID file (exec-based, foreground) | none | `exec gossamer` / `xdg-open` for remote URL; no daemon to track |
| idaptik | ✅ | ✅ | none | `/tmp/idaptik-server.pid` | none | Pre-start `lsof -i :8080` port probe; `curl` only for readiness probe; `notify-send` optional |
| invariant-path | ✅ | ✅ (all `${…}` form) | none | no PID file (one-shot) | none | Clean |
| nqc | ✅ | ✅ | none | `/tmp/nqc.pid` | none | Hardcoded `/home/hyper/.bin/nqc`; no `REPO_DIR` (no `cd`) |
| opsm | ✅ | ✅ (at top level) | none, **but** see note | `/tmp/opsm.pid` | none | ⚠ `nohup bash -lc "$COMMAND_SCRIPT"` and `nohup bash -c '…opsm system-update…'` — composed-command-string pattern. Not injection (no external input), but fragile: sources `~/.bashrc.d/tools/opsm` from inside a nohup login shell, ends in `exec bash`, so the tracked PID is a stand-in for an interactive shell. Also breaks on systems without that profile fragment. |
| panll | ✅ | ✅ | none | `/tmp/panll-server.pid` | none | `curl` only for readiness probe; exports `BROWSERSLIST_IGNORE_OLD_DATA=1` to silence caniuse warnings |
| project-wharf | ✅ | ✅ | none | `/tmp/project-wharf.pid` | none | Same hardcoded `repo-quicklaunch.sh` path as aerie/ambientops |

None of the 11 use `eval`. None pipe remote `curl` output to a shell. All 11
set `set -euo pipefail`. Variable quoting is generally clean; no unquoted
`$VAR` expansions that the auditor could find.

## Fleet-level findings

### 1. `--integ` / `--disinteg` coverage: **0 / 11**

No launcher implements system-integration or dis-integration modes. The
Desktop-file / Start-Menu / `~/.local/bin/` install surface described in
`launcher-standard.adoc §System Integration Modes` is not present anywhere.
This is the single largest gap against the standard and the most impactful
one to fix, since it is *the* reason the two standards (`launcher-standard`
and `LM-LA-LIFECYCLE-STANDARD`) were unified: one entry point for install,
uninstall, and runtime.

### 2. Explicit `--help` / `-h`: **2 / 11**

Only `hypatia-launcher.sh` and `invariant-path-launcher.sh` print a usage
text. The other nine rely on the `--auto|*` fall-through, so `./launcher.sh
--help` actually *starts* the application. The standard requires help to
print usage text plus detected platform and the files the launcher
reads/writes; no launcher satisfies the full requirement.

### 3. Explicit `--browser` branch: **2 / 11**

Only `idaptik` and `panll` have a dedicated `--browser` / `--web` branch.
`burble` accepts it implicitly via `*` fall-through (which does open a
browser, so behaviour is correct but the case arm is not explicit). The
remaining eight either do not launch a browser at all or bury the behaviour
inside `--auto`.

### 4. Readiness checking (`wait_for_server`): **3 / 11**

Only the three "server-with-URL" launchers (`burble`, `idaptik`, `panll`)
implement `wait_for_server`. This is consistent with the standard — the
pattern is only required for web/server apps — but it means the scaffolder
should detect runtime shape and only emit `wait_for_server` for the
`server-with-url` shape.

### 5. Port-conflict pre-check: **1 / 11**

Only `idaptik` checks `lsof -i :PORT` before starting. `burble` and `panll`
will silently run into port conflicts and surface them as "server did not
start within N seconds". Worth promoting to a standard pattern for the
server-with-url shape.

### 6. Hardcoded `$HOME` paths: **5 / 11**

`aerie`, `ambientops`, `project-wharf` hardcode
`/home/hyper/Desktop/Repo-Projects/launchers/repo-quicklaunch.sh`.
`game-server-admin` hardcodes `/home/hyper/.local/bin/game-server-admin-launcher`.
`nqc` hardcodes `/home/hyper/.bin/nqc`.

These are portability, not security, issues — but they directly contradict
design principle #5 ("no elevated privileges, user-level paths") and
principle #2 ("cross-platform"). The scaffolder should emit `${HOME}` or
resolve from `$PATH` via `command -v`.

### 7. Double-indirection in `game-server-admin`

`game-server-admin-launcher.sh` in `.desktop-tools/` calls
`/home/hyper/.local/bin/game-server-admin-launcher --gossamer`. If that
second file is itself this launcher (copied there by a hypothetical past
`--integ`), this is a loop. If it is a different file, there are two truths
about what "launching Game Server Admin" means. Worth untangling before
the scaffolder emits this class of launcher.

### 8. OPSM composed-command pattern

`opsm-launcher.sh` uses:

```sh
nohup bash -lc "$COMMAND_SCRIPT" >"$LOG_FILE" 2>&1 &
```

where `$COMMAND_SCRIPT` is a literal shell snippet that sources
`~/.bashrc.d/tools/opsm`, prints diagnostic output, and ends in `exec bash`.
The tracked PID is therefore a stand-in for an interactive shell. It is the
only launcher in the fleet whose daemon is actually a login shell. Not
injectable (no external input), but:

- fragile (requires the exact profile fragment to exist);
- non-portable (no `~/.bashrc.d/` on macOS);
- the PID file does not meaningfully represent the OPSM runtime.

Recommend the scaffolder treat "shell-function wrapper around a
profile-sourced tool" as a distinct runtime shape that does *not* attempt
daemon tracking, and instead runs the command synchronously in the
foreground (like `hypatia-launcher.sh --cli`).

### 9. `hypatia-launcher.sh` — a legitimately different shape

`hypatia-launcher.sh` is the odd one out: it has no PID file, no
`start_server`, no `stop_server`. It `exec`s into either `gossamer` (for
the remote GUI) or `hypatia-cli.sh` (for local scan), and its `--status`
only reports "launcher ready, URL: …". This is correct for its runtime
shape — **remote web app with optional local CLI subcommand** — and the
scaffolder should recognise this shape rather than force it into the
daemon template.

### 10. `invariant-path-launcher.sh` — also legitimately different

`invariant-path-launcher.sh` is the one-shot CLI shape: it runs a scan,
writes to `/tmp/invariant-path-last-scan.json`, and exits. `--start` /
`--stop` are semantically meaningless; the scaffolder should emit only
`--status` (last scan info) and `--help`, not the daemon modes. It is also
the only launcher that uses `${VAR}` brace form consistently, has its own
`--help` usage text, and redirects stderr to a log file per invocation.

## Runtime-shape taxonomy observed

The 11 launchers cluster into four distinct runtime shapes, suggesting the
scaffolder should accept a `shape:` field in its per-app manifest:

| Shape | Count | Launchers | Characteristic |
|---|---|---|---|
| `background-process` | 6 | aerie, ambientops, game-server-admin, nqc, opsm, project-wharf | `nohup` + PID file, no URL, no readiness probe |
| `server-with-url` | 3 | burble, idaptik, panll | `nohup` + PID file + `wait_for_server` against `$URL` + `open_browser` |
| `remote-web-app` | 1 | hypatia | no daemon; `exec gossamer`/`xdg-open` against a remote URL; optional `exec` CLI subcommand |
| `one-shot-cli` | 1 | invariant-path | run-and-exit; outputs to a well-known file; `--status` reports last run |

The standard's reference template covers `server-with-url` well. The other
three shapes are under-specified and are where hand-written drift has
accumulated.

## Summary

- **`set -euo pipefail`**: 11/11 ✅
- **Variable quoting**: 11/11 ✅
- **`eval`**: 0/11 ✅
- **`curl | bash`**: 0/11 ✅
- **`--start/--stop/--status/--auto`** (for applicable shapes): 9/9 ✅
- **Explicit `--browser` branch**: 2/11 ⚠
- **Explicit `--help`**: 2/11 ⚠
- **`--integ`**: 0/11 ❌
- **`--disinteg`**: 0/11 ❌
- **Readiness check** (for `server-with-url` shape): 3/3 ✅
- **Port pre-check** (for `server-with-url` shape): 1/3 ⚠
- **Hardcoded `$HOME` paths**: 5/11 ⚠
- **Runtime shapes observed**: 4 (scaffolder manifest should carry a shape field)

The fleet is **security-clean**, **structurally consistent within each
shape**, and **uniformly missing the `--integ` / `--disinteg` / explicit
`--help`** trio. Those three modes and the portability fixes are the main
work items for whichever session retires the hand-written launchers via
`launch-scaffolder`.

## Files consulted (read-only)

- `/var/mnt/eclipse/repos/standards/docs/UX-standards/launcher-standard.adoc`
- `/var/mnt/eclipse/repos/standards/docs/UX-standards/LM-LA-LIFECYCLE-STANDARD.adoc`
- `/var/mnt/eclipse/repos/.desktop-tools/aerie-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/ambientops-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/burble-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/game-server-admin-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/hypatia-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/idaptik-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/invariant-path-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/nqc-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/opsm-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/panll-launcher.sh`
- `/var/mnt/eclipse/repos/.desktop-tools/project-wharf-launcher.sh`

No file was modified. No file inside `/var/mnt/eclipse/repos/launch-scaffolder/`
other than this document was written.
