<!-- SPDX-License-Identifier: PMPL-1.0-or-later -->
# `docs/ruleset-audit-2026-04-10/`

Raw data and scripts from the estate-wide branch-protection ruleset
audit conducted on 2026-04-10. The human-readable post-mortem lives
at [`../branch-protection-remediation-2026-04-10.md`](../branch-protection-remediation-2026-04-10.md) — **read that first**.

## Contents

| File | Description |
|---|---|
| `audit.sh` | Read-only classifier. `bash audit.sh` re-runs the audit and rewrites `report.jsonl`. Uses `gh api` — requires an authenticated GitHub CLI session. |
| `reference-ruleset.json` | Canonical 5-rule `Base` ruleset body. The one POST body used to create/recreate every compliant ruleset in Wave 1. |
| `repos.tsv` | `<repo>\t<isPrivate>` — the 315 non-archived `hyperpolymath/*` repos enumerated at audit time. |
| `forks.txt` | The 8 upstream forks inside `hyperpolymath/*`. Used to exclude forks from Wave 1 per an explicit user decision. |
| `report.jsonl` | One JSON record per repo from the audit run: `{repo, state, ...}` where `state ∈ {OK, DRIFT, MISSING, ERROR}`. |
| `wave1-apply.sh` | Wave 1 creator for `MISSING` repos (POST reference ruleset). Supports `OWNER` override and `--dry-run` plan generation. |
| `wave1-repos.txt` | The 29 repos classified as MISSING (pre-fork-exclusion). |
| `wave1-apply.txt` | The 21 repos that actually received Wave 1 writes (29 MISSING minus 8 forks). |
| `wave1-plan.jsonl` | The dry-run plan file. One line per planned API call — `{method, path, body_file, repo}`. |
| `wave1-results.tsv` | Per-repo apply outcome. All 21 now show `OK` with their `ruleset_id`. |

## Re-running the audit

```bash
# From launch-scaffolder/ root.
cd docs/ruleset-audit-2026-04-10/

# Refresh the repo list (optional — only needed if new repos have
# been added or repos have been archived since the last run):
OWNER=hyperpolymath
gh repo list "$OWNER" --limit 500 --no-archived \
  --json name,isPrivate --jq '.[] | "\(.name)\t\(.isPrivate)"' > repos.tsv

# Re-audit:
OWNER="$OWNER" bash audit.sh

# Summarise drift:
jq -r .state report.jsonl | sort | uniq -c
```

## Re-applying to a new batch (e.g. Wave 2)

```bash
# Pick your wave — DRIFT this time:
jq -r 'select(.state=="DRIFT") | .repo' report.jsonl > /tmp/wave2-repos.txt

# Exclude forks:
comm -23 <(sort /tmp/wave2-repos.txt) forks.txt > /tmp/wave2-apply.txt

# For Wave 2 the existing (broken) ruleset must be deleted first,
# because POST will fail with "Name must be unique" on every repo.
# The two-step is: GET the existing Base ruleset id, DELETE it,
# then POST the reference body. Script this only after a dry-run
# review of the plan file.

# Dry-run with alternate owner:
OWNER=The-Metadatastician \
REPOS_FILE=/tmp/wave2-apply.txt \
bash wave2-apply.sh --dry-run
```

## Wave 1 (MISSING repos)

```bash
# Build Wave 1 repo list from current report:
jq -r 'select(.state=="MISSING") | .repo' report.jsonl > /tmp/wave1-repos.txt

# Dry-run / plan generation:
OWNER=The-Metadatastician \
REPOS_FILE=/tmp/wave1-repos.txt \
PLAN_FILE=/tmp/ruleset-audit/wave1-plan-The-Metadatastician.jsonl \
RESULTS_FILE=/tmp/ruleset-audit/wave1-results-The-Metadatastician.tsv \
bash wave1-apply.sh --dry-run

# Apply:
OWNER=The-Metadatastician \
REPOS_FILE=/tmp/wave1-repos.txt \
PLAN_FILE=/tmp/ruleset-audit/wave1-plan-The-Metadatastician.jsonl \
RESULTS_FILE=/tmp/ruleset-audit/wave1-results-The-Metadatastician.tsv \
bash wave1-apply.sh
```

## Why the artefacts live here, not in `~/security-fixes/`

Per global memory, batch fix scripts usually live under
`~/security-fixes/` (e.g. `fix-permissions.jl`,
`enable-branch-protection.jl`). That directory didn't exist at the
time of this audit. Rather than create a new top-level location with
one set of artefacts in it, the Wave 1 data lives alongside its
post-mortem in `launch-scaffolder/docs/` — the scaffolder was the
tool that surfaced the drift problem when `cmd_provision` was being
pushed to downstream repos, and its own `launch-scaffolder` repo was
in the MISSING list. A future Wave 2 script may move under
`~/security-fixes/` or `ambientops/` once the estate-wide batch-fix
pattern has more than one instance and the right home is obvious.
