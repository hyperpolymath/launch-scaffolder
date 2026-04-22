<!-- SPDX-License-Identifier: PMPL-1.0-or-later -->
# Transfer Verification Checklist — 2026-04-22

Scope: verify whether the following legacy repos were transferred from
`hyperpolymath` to `The-Metadatastician` (possibly renamed):

1. `repos-monorepo`
2. `.git-private-farm`
3. `blog-drafts`
4. `hyperpolymath-sovereign-registry`

## Current evidence (captured 2026-04-22)

| Legacy repo | Exists at `hyperpolymath/<repo>` | Visibility there | Exists at `The-Metadatastician/<same-name>` | Rules endpoint at source |
|---|---|---|---|---|
| `repos-monorepo` | Yes (`200`) | `private` | No (`404`) | `403` plan-gated |
| `.git-private-farm` | Yes (`200`) | `private` | No (`404`) | `403` plan-gated |
| `blog-drafts` | Yes (`200`) | `private` | No (`404`) | `403` plan-gated |
| `hyperpolymath-sovereign-registry` | Yes (`200`) | `private` | No (`404`) | `403` plan-gated |

Interpretation: these four repositories are currently still under the
`hyperpolymath` user account with private visibility, and are not present
under `The-Metadatastician` with the same names.

## Verification checklist

- [x] Confirm source-owner existence and visibility.
- [x] Confirm target-owner same-name existence (`404` for all four).
- [x] Check whether they were transferred with the same names in target org.
  - Result: no same-name matches under `The-Metadatastician`.
- [x] Check whether they were transferred and renamed in target org.
  - Result: no credible rename candidates among the current 8 repositories in
    `The-Metadatastician` inventory (see `transfer-map-2026-04-22.tsv`).
  - Export org repo inventory:
    ```bash
    gh repo list The-Metadatastician --limit 500 --json name,nameWithOwner,visibility,isPrivate \
      | jq -r '.[] | [.name, .nameWithOwner, .visibility, (.isPrivate|tostring)] | @tsv' \
      > /tmp/the-metadatastician-repos.tsv
    ```
  - Manually map candidate renames from `/tmp/the-metadatastician-repos.tsv`.
- [ ] Verify transfer event(s) in org audit log for each legacy repo name.
  - Example query (adjust phrase as needed):
    ```bash
    gh api 'orgs/The-Metadatastician/audit-log?phrase=repo%20transfer&per_page=100'
    ```
- [ ] Verify source-side repository redirect behavior for each legacy repo.
  - If transferred, GitHub usually redirects old clone URLs to new owner/name.
  - Example:
    ```bash
    git ls-remote https://github.com/hyperpolymath/repos-monorepo.git
    ```
- [x] Produce and store an explicit mapping file:
  - `docs/ruleset-audit-2026-04-10/transfer-map-2026-04-22.tsv` with columns:
    - `legacy_owner`
    - `legacy_repo`
    - `new_owner`
    - `new_repo`
    - `status` (`confirmed` or `needs-review`)
    - `evidence`
- [x] Update automation inputs to match the confirmed owner/name map.
  - Ruleset audit owner override:
    ```bash
    OWNER=The-Metadatastician bash docs/ruleset-audit-2026-04-10/audit.sh
    ```
  - `audit.sh` now accepts `OWNER`, `REPOS_FILE`, and `REPORT_FILE` env overrides.

## Exit criteria

- Every legacy repo has either:
  - a confirmed transfer+rename mapping in `transfer-map.tsv`, or
  - a confirmed "not transferred" disposition.
- Ruleset audits target the correct owner/repo namespace with no ambiguity.
