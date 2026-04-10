<!-- SPDX-License-Identifier: PMPL-1.0-or-later -->
# `examples/` — fixture inputs for `launch-scaffolder`

Every file in this directory is a **test fixture**, not a live per-app
launcher config. Nothing under here is picked up by estate walks
(`launch-scaffolder realign`, `mint --all`, etc.).

## Fixture-vs-live naming rule

| Purpose | File-name suffix | Discovered by estate walks? |
|---|---|---|
| Live per-app config | `<app>.launcher.a2ml` | **Yes** |
| Fixture / example input | `<app>.launcher.fixture.a2ml` | **No** |

The `.fixture.` infix is the single mechanism separating fixtures from
live configs — the discovery code in
`crates/launcher/src/cmd_realign.rs::is_live_config` looks for exactly
this suffix and skips anything matching it. Do not rely on directory
names (`examples/`, `tests/`, …) for isolation; a file named
`foo.launcher.a2ml` anywhere under the estate will be treated as a
live config regardless of which directory it lives in.

## Rule of thumb for contributors

- New fixture? Name it `<something>.launcher.fixture.a2ml`.
- New live launcher? Name it `<app>.launcher.a2ml` and place it in the
  target repo, not here.
- Never copy a live config into this directory without renaming it to
  the fixture suffix.
