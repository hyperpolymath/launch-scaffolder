#!/usr/bin/env bash
# SPDX-License-Identifier: PMPL-1.0-or-later
#
# wave2-apply.sh — Wave 2 ruleset remediation for OWNER (default: hyperpolymath)
#
# For each DRIFT repo:
#   1. List existing rulesets → find "Base" ruleset ID
#   2. GET the ruleset → preserve required_status_checks config if present
#   3. DELETE the existing ruleset
#   4. POST reference ruleset (with RSC appended if repo had it)
#
# Writes one TSV line per repo to wave2-results.tsv:
#   repo  status  ruleset_id  detail
#
# Usage:
#   bash wave2-apply.sh              # process all 168 DRIFT repos
#   bash wave2-apply.sh --dry-run    # print plan, no mutations
#   OWNER=The-Metadatastician bash wave2-apply.sh
#
# THREAT MODEL / PANIC-ATTACK NOTE:
#   `$repo` interpolations are all quoted; input is from report.jsonl
#   which is `gh repo list` output. GitHub enforces alphanumeric/hyphen/
#   underscore/period on repo names — no shell metacharacters possible.
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OWNER="${OWNER:-hyperpolymath}"
REPOS_FILE="${REPOS_FILE:-/tmp/ruleset-audit/wave2-repos.txt}"
RESULTS_FILE="${RESULTS_FILE:-$SCRIPT_DIR/wave2-results.tsv}"
DRY_RUN=false

[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

REF_RULESET="$(cat "$SCRIPT_DIR/reference-ruleset.json")"

apply_repo() {
    local repo="$1"

    # ── 1. List rulesets ──────────────────────────────────────────────
    local rulesets
    rulesets=$(gh api "repos/$OWNER/$repo/rulesets" 2>/dev/null) || {
        echo -e "$repo\tERROR\t\tlist rulesets API failed"
        return
    }

    # Find the "Base" ruleset (or any ruleset targeting the default branch)
    local ruleset_id
    ruleset_id=$(echo "$rulesets" | jq -r '
        .[] | select(.name == "Base" or (.conditions.ref_name.include // [] | contains(["~DEFAULT_BRANCH"])))
        | .id' | head -1)

    if [[ -z "$ruleset_id" ]]; then
        # No matching ruleset found — treat as now-MISSING, POST fresh
        echo -e "$repo\tNO_BASE_RULESET\t\tno Base ruleset found — skipping (re-audit needed)" >&2
        echo -e "$repo\tSKIP_NO_BASE\t\tno Base ruleset found"
        return
    fi

    # ── 2. GET full ruleset config ────────────────────────────────────
    local current
    current=$(gh api "repos/$OWNER/$repo/rulesets/$ruleset_id" 2>/dev/null) || {
        echo -e "$repo\tERROR\t$ruleset_id\tGET ruleset failed"
        return
    }

    # Check for required_status_checks rule (preserve if present)
    local has_rsc
    has_rsc=$(echo "$current" | jq -r '[.rules[].type] | contains(["required_status_checks"])')

    # Build the new ruleset payload
    local new_ruleset
    if [[ "$has_rsc" == "true" ]]; then
        # Extract the required_status_checks rule config verbatim
        local rsc_rule
        rsc_rule=$(echo "$current" | jq '[.rules[] | select(.type == "required_status_checks")] | .[0]')
        # Inject RSC into the reference ruleset rules array
        new_ruleset=$(echo "$REF_RULESET" | jq --argjson rsc "$rsc_rule" '.rules += [$rsc]')
    else
        new_ruleset="$REF_RULESET"
    fi

    if [[ "$DRY_RUN" == "true" ]]; then
        local rsc_note=""
        [[ "$has_rsc" == "true" ]] && rsc_note=" (+RSC preserved)"
        printf '[DRY-RUN] %s — DELETE ruleset %s, POST reference%s\n' "$repo" "$ruleset_id" "$rsc_note" >&2
        echo -e "$repo\tDRY_RUN\t$ruleset_id\t$(echo "$new_ruleset" | jq -c '.rules | map(.type)')"
        return
    fi

    # ── 3. DELETE existing ruleset ────────────────────────────────────
    gh api "repos/$OWNER/$repo/rulesets/$ruleset_id" -X DELETE 2>/dev/null || {
        echo -e "$repo\tERROR_DELETE\t$ruleset_id\tDELETE failed"
        return
    }

    # ── 4. POST reference ruleset ─────────────────────────────────────
    local post_result
    post_result=$(echo "$new_ruleset" | \
        gh api "repos/$OWNER/$repo/rulesets" \
            -X POST \
            --input - 2>/dev/null) || {
        echo -e "$repo\tERROR_POST\t$ruleset_id\tPOST failed after DELETE"
        return
    }

    local new_id
    new_id=$(echo "$post_result" | jq -r '.id')
    local rsc_note=""
    [[ "$has_rsc" == "true" ]] && rsc_note="+RSC"
    echo -e "$repo\tOK\t$new_id\t$rsc_note"
}

# ── Main loop ─────────────────────────────────────────────────────────────

if [[ "$DRY_RUN" == "true" ]]; then
    echo "=== DRY RUN — no mutations ===" >&2
fi
echo "owner=$OWNER" >&2
echo "repos_file=$REPOS_FILE" >&2
echo "results_file=$RESULTS_FILE" >&2

: > "$RESULTS_FILE"
total=$(wc -l < "$REPOS_FILE")
i=0

while IFS= read -r repo; do
    i=$((i+1))
    printf '[%d/%d] %s\n' "$i" "$total" "$repo" >&2
    apply_repo "$repo" >> "$RESULTS_FILE"
done < "$REPOS_FILE"

echo "" >&2
echo "=== Wave 2 complete ===" >&2
echo "Results: $RESULTS_FILE" >&2
awk -F'\t' '{print $2}' "$RESULTS_FILE" | sort | uniq -c | sort -rn >&2
