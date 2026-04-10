<!-- SPDX-License-Identifier: PMPL-1.0-or-later -->
# Estate-wide branch-protection remediation — 2026-04-10

## What triggered this

While pushing the `cmd_provision` work to seven downstream launcher
repos (aerie, burble, game-server-admin, nextgen-databases, panll,
project-wharf, stapeln), some pushes printed a "Bypassed rule
violations: Changes must be made through a pull request" audit line
and others pushed silently. That inconsistency prompted an
investigation: the documented standard is "PR required, 0 approvals,
enforce_admins=false, required_signatures ON" — why were only some
repos enforcing it?

## What we found

**The warnings are not the problem.** When a repo carries the full
5-rule ruleset with admin listed as a bypass actor, GitHub emits the
"Bypassed rule violations" line as the audit trail for an
*authorised* admin direct-push. That is the system working as
designed — it is the accountability record, not noise.

**The real problem is drift.** A read-only audit against all 315
non-archived `hyperpolymath/*` repos (see `docs/ruleset-audit-2026-04-10/`
for the raw data) surfaced this distribution:

| Class | Count | State |
|---|---|---|
| OK | 113 | Exactly the documented 5-rule reference — no action |
| DRIFT | 168 | Ruleset exists but differs from reference (see breakdown below) |
| MISSING | 29 | No ruleset at all on the default branch — includes `launch-scaffolder` itself |
| ERROR (API 403) | 5 | Private repos on GitHub Free; rulesets require Pro-or-higher on private repos |

**Drift shapes (non-exclusive — some repos match more than one):**

- 147 have `code_quality` + 3 history rules + `required_signatures` but are **missing `pull_request`**.
- 11 have 4 history rules + `required_signatures`, no `code_quality`, still missing `pull_request`.
- 3 match the reference shape plus an extra `required_status_checks` rule (legitimate CI contract; preserved).
- 4 are kitchen-sink rulesets with `copilot_code_review`, `code_scanning`, `required_deployments` — looks like someone clicked GitHub's "suggested ruleset" button in the UI.
- 2 are minimal and missing both `pull_request` and `required_signatures`.

**The 5 ERROR cases** are all private repos: `007`,
`.git-private-farm`, `hyperpolymath-sovereign-registry`,
`blog-drafts`, `repos-monorepo`. The GitHub API returns
`"Upgrade to GitHub Pro or make this repository public to enable this
feature"` — rulesets on private repos need at least GitHub Pro.

## Reference shape

Documented in
`~/.claude/projects/-var-mnt-eclipse-repos/memory/feedback_branch_protection.md`
and confirmed empirically against `game-server-admin` / `panll` /
`project-wharf`:

```json
{
  "name": "Base",
  "target": "branch",
  "enforcement": "active",
  "conditions": { "ref_name": { "exclude": [], "include": ["~DEFAULT_BRANCH"] } },
  "rules": [
    { "type": "required_signatures" },
    { "type": "deletion" },
    { "type": "non_fast_forward" },
    { "type": "required_linear_history" },
    {
      "type": "pull_request",
      "parameters": {
        "required_approving_review_count": 0,
        "dismiss_stale_reviews_on_push": false,
        "required_reviewers": [],
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_review_thread_resolution": false,
        "allowed_merge_methods": ["merge", "squash", "rebase"]
      }
    }
  ],
  "bypass_actors": [
    { "actor_id": 5, "actor_type": "RepositoryRole", "bypass_mode": "always" }
  ]
}
```

`actor_id=5` is GitHub's built-in Admin role. Repos with an additional
`required_status_checks` rule keep it — that's an additive CI contract,
not drift.

## Remediation strategy

Three waves, each gated on a human-readable diff before apply:

1. **Wave 1 — 29 MISSING repos → create the reference from scratch.** Lowest risk (no existing state to clobber).
2. **Wave 2 — 168 DRIFT repos → delete-and-recreate to match reference.** Preserves `required_status_checks` where present.
3. **Wave 3 — 5 private-Free repos → apply classic branch protection (works on Free) OR switch to rulesets once GitHub Education / Pro is available.**

The three waves are independent and can be tackled in any order.

## Wave 1 outcome (2026-04-10)

**21 repos brought to compliance; 8 upstream forks intentionally skipped.**

Forks skipped per explicit user decision: `awesome-elixir`,
`awesome-haskell`, `awesome-lua`, `awesome-selfhosted`, `awesome-v`,
`file`, `lua-filters`, `rescript`. Rationale: these are not original
hyperpolymath work, and a fork-local ruleset could friction future
rebases against upstream without delivering proportional policy value.

Apply sequence (single POST per repo via `gh api`):

- 19/21 succeeded on the first POST.
- 2/21 (`MinixSDK.jl`, `thejeffparadox`) rejected with
  `"Validation Failed: Name must be unique"`. Investigation showed both
  had a pre-existing broken `Base` ruleset that the audit classified
  as MISSING (correctly — neither was firing on the default branch):
  - `MinixSDK.jl` had `enforcement=disabled`, `include=["~ALL"]`, 9 rules
    including `code_quality`, `code_scanning`, `copilot_code_review`,
    and 8 bypass actors (Write, Maintain, Admin, plus 5 Integration apps).
    Effectively dead config.
  - `thejeffparadox` had `enforcement=active` but `include=[]`
    (targeted no branches), only 2 rules, no bypass actors.
    Effectively orphaned.
- Both were delete-and-recreated cleanly.

Post-apply audit: **21/21 compliant**. `launch-scaffolder` is now
gated; subsequent direct pushes from admin will emit the expected
"Bypassed rule violations" audit breadcrumb as the accountability
record, matching the rest of the estate.

## Wave 2 and 3 — deferred

Wave 2 (168 DRIFT) and Wave 3 (5 private) are captured in
`.machine_readable/6a2/STATE.a2ml` as future work, and Wave 3 is
specifically waiting on GitHub Education approval. If Pro-via-Education
unlocks rulesets on private repos (the API error message says it does;
the rulesets docs are ambiguous and describe org-scoped rulesets), the
fastest empirical test after approval is:

```bash
gh api --method PUT "repos/hyperpolymath/007/rulesets" \
  --input docs/ruleset-audit-2026-04-10/reference-ruleset.json
```

If that succeeds → the same tooling handles Wave 3 identically to
Wave 1. If it fails → fall back to classic branch protection, which
works on Free for private repos and has equivalent semantics (PR
required, linear history, required signatures, no force push, no
deletion, admin bypass via `enforce_admins=false`).

## Takeaways

1. The "Bypassed rule violations" warnings are the correct audit trail
   for admin direct-push through a PR gate. **Do not try to silence
   them.** Their presence is proof the gate exists.
2. The estate's ruleset config had drifted substantially. 113/315
   (36%) conformed to the documented standard; the rest needed work.
3. `code_quality` in a ruleset is a trap on a polyglot estate —
   CodeQL doesn't support every language we use, and the rule
   behaves inconsistently. Gate code quality at the workflow layer,
   not the branch-rule layer.
4. GitHub Free blocks rulesets on private repos. This matters for
   any estate with private-primary repos; plan for GitHub
   Pro/Education/Team accordingly.
5. A read-only estate audit before *any* write is cheap and catches
   classification bugs (e.g. the two "MISSING" repos that actually
   had broken pre-existing rulesets). Always audit twice: once to
   classify, once after apply to verify.

## Audit artefacts

All raw data and scripts used for Wave 1 are archived in
`docs/ruleset-audit-2026-04-10/`:

- `repos.tsv` — 315 non-archived repos with their privacy flag
- `forks.txt` — fork list used to exclude the 8 upstream forks
- `report.jsonl` — per-repo classification (OK/DRIFT/MISSING/ERROR)
- `audit.sh` — the read-only classifier (can be rerun at any time)
- `reference-ruleset.json` — the canonical body used for POSTs
- `wave1-repos.txt`, `wave1-apply.txt` — input lists
- `wave1-plan.jsonl` — the dry-run plan approved before apply
- `wave1-results.tsv` — per-repo outcome, including the two recoveries
