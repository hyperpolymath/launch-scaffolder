<!-- SPDX-License-Identifier: PMPL-1.0-or-later -->
# Launcher Scaffolder Exceptions — 2026-04-10

Five launchers in `/var/mnt/eclipse/repos/.desktop-tools/` are declared
**out of scope for `launch-scaffolder` minting** as of 2026-04-10. They
stay hand-written for concrete reasons captured below. Each row records
what would have to change in `launch-scaffolder` before the launcher
could be moved into scaffolder management.

Companion document: [`compliance-audit-2026-04-10.md`](compliance-audit-2026-04-10.md).
See [§Cross-reference against the compliance audit](#cross-reference-against-the-compliance-audit)
at the end for the reconciliation notes — discrepancies are flagged, not
silently patched.

## Exceptions table

| Launcher | Current location | Reason it can't be scaffolder-managed | Extra / custom modes exposed beyond the standard template | Migration trigger (what would have to land in `launch-scaffolder` first) |
|---|---|---|---|---|
| `hypatia-launcher.sh` | `/var/mnt/eclipse/repos/.desktop-tools/` | Remote web app (`https://nesy-prover.dev`) with a **conditional** front-end: if `gossamer` is on `PATH` *and* `~/.config/hypatia/gossamer.conf.json` exists, `exec gossamer`; otherwise fall through to `xdg-open`/`firefox`. No daemon, no PID file, no `start`/`stop` semantics. The `--cli` mode `exec`s a separate script (`hypatia-cli.sh`) and is a different runtime shape again. | `--tour`, `--gui`, `--local`, `--dev`, `--cli`, `--scan`, plus `exec gossamer …` dispatch | `launch-scaffolder` needs a `shape: remote-web-app` runtime in its template engine that (a) emits no daemon machinery, (b) supports a conditional gossamer-vs-browser dispatch block, and (c) allows a sibling `exec`-delegated CLI subcommand (`--cli`/`--scan`) to live in the same launcher. |
| `invariant-path-launcher.sh` | `/var/mnt/eclipse/repos/.desktop-tools/` | Bespoke Rust-CLI wrapper: every invocation shells out to `cargo run --manifest-path … -p invariant-path-cli --`, writes results to `/tmp/invariant-path-last-scan.json`, and exits. Not a server, not a daemon, no readiness probe, no browser. `--status` reports the last scan output, not a running process. Also owns its own `--scan-file` and `--open-output` modes that touch a well-known output file. | `--scan [repo] [profile]`, `--scan-file <file> [profile]`, `--cli <args…>` (pass-through), `--open-output`, `--status` (reports last-scan summary), `--auto` (scan default repo) — all dispatched through `cargo run` | `launch-scaffolder` needs a `shape: one-shot-cli` runtime that emits (a) no PID file, (b) a `cargo run` / `just` / `deno task` dispatcher with configurable backend, (c) a well-known `OUTPUT_FILE` contract, (d) an optional `jq`-based summariser, and (e) a `--status` that reports "last run output" instead of "process state". |
| `opsm-launcher.sh` | `/var/mnt/eclipse/repos/.desktop-tools/` | Its runtime is **a login shell**. The "daemon" is `nohup bash -lc "$COMMAND_SCRIPT"` where `$COMMAND_SCRIPT` sources `~/.bashrc.d/tools/opsm`, runs `opsm-runtime list && opsm-runtime doctor`, and ends in `exec bash`. The tracked PID is therefore an interactive bash session, not the real OPSM runtime. Also depends on a profile fragment that only exists on the author's machine, so the launcher is non-portable by construction. | `--system-update` (custom), plus the implicit "source profile → `exec bash`" behaviour of `--start` / `--auto` | `launch-scaffolder` needs a `shape: shell-context` runtime that (a) deliberately does **not** produce a PID-file-tracked daemon, (b) runs the command synchronously in a foreground login shell, (c) declares its profile-source dependencies in the per-app manifest so a dependency-check step can detect missing fragments, and (d) supports arbitrary extra modes like `--system-update` as first-class entries in the manifest. Alternative: the user decides OPSM should not have a launcher at all and is invoked interactively only, in which case this file is deleted rather than migrated. |
| `ambientops-launcher.sh` | `/var/mnt/eclipse/repos/.desktop-tools/` | Not just the `.sh` — its companion `.desktop` file exposes input-device management as `Desktop Action` entries, so the launcher contract is **file-pair** (`.sh` + `.desktop`), not just the shell script. The two custom `.sh` modes (`--toggle-input-devices`, `--emergency-input-restore`) are there specifically to be reachable from the Action entries in the `.desktop` file. | `--toggle-input-devices`, `--emergency-input-restore`, both called via `repo-quicklaunch.sh … just <recipe>` | `launch-scaffolder` needs first-class support for **extra `.desktop` `[Desktop Action …]` entries** declared in the per-app manifest, and the ability to bind each Action to a custom mode name in the emitted `.sh`. Today the scaffolder only emits the stock `stop;status;` Action pair described in `launcher-standard.adoc §Desktop File Standard`; it cannot emit arbitrary additional Actions. |
| `idaptik-launcher.sh` | `/var/mnt/eclipse/repos/.desktop-tools/` | Similar to ambientops — the `.sh` + `.desktop` file pair is the contract. Idaptik's `.desktop` also declares itself as a URI handler (`MimeType=x-scheme-handler/idaptik;` with `Exec=… %u`), so the launcher receives a `%u` URI argument from the desktop environment on activation and must route it. The `.sh` additionally owns two display-front-end choices (`--gossamer` and `--tray`) that the standard `--browser` arm does not cover, plus a pre-start port check (`lsof -i :8080`). | `--browser`, `--web`, `--gossamer`, `--tray`, plus `%u` URI handling from the `.desktop` file and `lsof`-based port pre-check | `launch-scaffolder` needs (a) a **URI-handler manifest field** that emits the correct `MimeType=`, `Exec=… %u` and argv handling in the `.sh`, (b) a declarable list of **alternative display front-ends** (`browser` / `gossamer` / `tray` / …) with a fallback chain, and (c) an optional `shape: server-with-url` augmentation for a pre-start port-conflict probe via `lsof` or `ss`. |

## Scaffolder-managed subset (for completeness)

The 11 launchers minus the 5 exceptions above leave **6 launchers** that
the parallel session's scaffolder minting pass can own today:

| Launcher | Runtime shape | Notes |
|---|---|---|
| `aerie-launcher.sh` | background-process | Wraps `repo-quicklaunch.sh → just tour`. Hardcoded `/home/hyper/Desktop/…` path to fix. |
| `burble-launcher.sh` | server-with-url (port 4020, Phoenix) | Clean template fit — already most like the reference. |
| `game-server-admin-launcher.sh` | background-process | Needs the double-indirection to `~/.local/bin/game-server-admin-launcher` untangled before minting. |
| `nqc-launcher.sh` | background-process | Hardcoded `/home/hyper/.bin/nqc`. No `REPO_DIR`. |
| `panll-launcher.sh` | server-with-url (port 8000) | Accepts bare-word aliases (`serve`, `start`, …) alongside `--foo` forms; scaffolder should decide whether to preserve that. |
| `project-wharf-launcher.sh` | background-process | Same hardcoded `repo-quicklaunch.sh` path as aerie/ambientops. |

These six collectively need only `--integ`/`--disinteg`/explicit `--help`
added and the hardcoded `$HOME` paths parameterised — no new runtime
shapes, no `.desktop` Actions, no URI handlers, no shell-context daemons.
That matches what the compliance audit flagged as the fleet-level gaps.

## Cross-reference against the compliance audit

The compliance audit was written before the exception list was declared.
Reconciling the two surfaces the following discrepancies, flagged here
rather than silently corrected in the audit.

### Discrepancy 1 — opsm runtime-shape classification

- **Audit says:** in the fleet-level runtime-shape taxonomy table,
  `opsm-launcher.sh` is listed under `background-process` alongside
  aerie, ambientops, game-server-admin, nqc, project-wharf (6/11).
- **Audit also says** (finding #8, narrative): the tracked PID is
  actually an `exec bash` login shell, not a real daemon, and the
  scaffolder should treat it as a "distinct runtime shape".
- **Exception list says:** opsm is explicitly a shell-context launcher,
  not a daemon.
- **Reconciliation:** the exception list is authoritative. The audit's
  taxonomy row for opsm is wrong — `opsm` belongs to a fifth runtime
  shape (`shell-context`) that the audit narrated but did not add to
  its 4-shape table. After reclassification the corrected counts are:
  `background-process` = 5 (not 6), `server-with-url` = 3,
  `remote-web-app` = 1, `one-shot-cli` = 1, `shell-context` = 1.
  **Not patching the audit file**; this note records the correction.

### Discrepancy 2 — `.desktop` file scope gap

- **Audit scope:** the audit read only the `*-launcher.sh` files. It
  did not open any `.desktop` files.
- **Exception list asserts** that two launchers (ambientops, idaptik)
  cannot be scaffolder-managed *because of* `.desktop` file content —
  Action entries for ambientops, URI-handler + `%u` for idaptik.
- **Reconciliation:** not a contradiction; an audit-scope gap. The
  audit did not make any claim about `.desktop` files at all, so the
  exception list extends rather than contradicts it. Flag: a follow-up
  audit should read the paired `.desktop` files before the scaffolder
  emits anything for ambientops or idaptik, and the scaffolder's
  per-app manifest schema needs `.desktop` Actions + URI-handler fields
  before either launcher can be migrated.

### Discrepancy 3 — invariant-path extras list is a narrower summary

- **Audit extras list:** `--scan`, `--scan-file`, `--cli`, `--open-output`
  (four extras beyond the standard modes).
- **Exception list summary:** `--scan / --cli / --status / --auto →
  cargo run dispatch` (three-extras-plus-auto framing).
- **Reconciliation:** not a contradiction; the exception list is a
  higher-level summary and omits `--scan-file` and `--open-output`
  for brevity. **Both modes must be preserved** in the scaffolder's
  eventual `one-shot-cli` template, and the exception row above
  explicitly records them.

### Discrepancy 4 — opsm extras

- **Audit extras list:** `--system-update`.
- **Exception list summary:** "shell-context launcher, not a daemon"
  (no explicit mention of `--system-update`).
- **Reconciliation:** not a contradiction; the exception list describes
  the *shape*, the audit describes the *mode surface*. The exception row
  above preserves `--system-update` as a required custom mode so the
  eventual `shell-context` manifest has to declare it.

### Discrepancy 5 — hypatia extras alignment

- **Audit extras list:** `--tour`, `--gui`, `--local`, `--dev`, `--cli`,
  `--scan`.
- **Exception list summary:** `--tour / --gui / --local / --dev / --cli /
  --scan + gossamer exec`.
- **Reconciliation:** matching. No discrepancy. The gossamer `exec`
  dispatch was called out in the audit narrative but not in its extras
  column; the exception row above hoists it back to first-class status
  as a required capability.

### Discrepancy 6 — ambientops extras alignment

- **Audit extras list:** `--toggle-input-devices`,
  `--emergency-input-restore`.
- **Exception list summary:** matches exactly.
- **Reconciliation:** no discrepancy on the `.sh` side. The `.desktop`
  side is covered by Discrepancy 2 above.

### Discrepancy 7 — idaptik extras alignment

- **Audit extras list:** `--web`, `--gossamer`, `--tray`.
- **Exception list summary:** `--gossamer`, `--tray`, plus `%u`
  handling. `--web` (an alias of `--browser`) is not mentioned.
- **Reconciliation:** `--web` is a trivial alias the scaffolder can
  emit automatically for any `server-with-url` shape. `%u` handling is
  covered by Discrepancy 2. No substantive contradiction.

## Summary

- **5 launchers declared out of scope** for `launch-scaffolder` minting:
  hypatia, invariant-path, opsm, ambientops, idaptik.
- **6 launchers remain in scope:** aerie, burble, game-server-admin,
  nqc, panll, project-wharf.
- **Cross-reference against the compliance audit:** 7 reconciliation
  notes, 1 material correction (opsm runtime-shape), 1 scope gap
  (`.desktop` files not inspected), 0 hard contradictions.
- **Scaffolder template-engine gaps implied by the exceptions**, in
  priority order:
  1. `shape: one-shot-cli` (invariant-path)
  2. `shape: remote-web-app` with conditional gossamer/browser dispatch (hypatia)
  3. `.desktop` Actions + URI-handler manifest fields (ambientops, idaptik)
  4. `shape: shell-context` (opsm — or deletion)
  5. Pre-start `lsof`/`ss` port probe for `server-with-url` (idaptik;
     also surfaces in the audit for burble/panll as a recommended
     standard pattern)

None of the in-scope 6 launchers need any of the above before they can
be minted, so the parallel session's current scaffolder-minting pass is
not blocked by this document.
