#!/usr/bin/env bash
# SPDX-License-Identifier: PMPL-1.0-or-later
#
# wave1-apply.sh — Wave 1 ruleset creation for OWNER (default: hyperpolymath)
#
# For each MISSING repo:
#   1. Generate a POST plan line in PLAN_FILE
#   2. POST the canonical reference ruleset
#
# Writes one TSV line per repo to RESULTS_FILE:
#   repo  status  ruleset_id  detail
#
# Usage:
#   bash wave1-apply.sh              # apply to REPOS_FILE
#   bash wave1-apply.sh --dry-run    # generate plan only
#   OWNER=The-Metadatastician bash wave1-apply.sh
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OWNER="${OWNER:-hyperpolymath}"
REPOS_FILE="${REPOS_FILE:-/tmp/ruleset-audit/wave1-repos.txt}"
PLAN_FILE="${PLAN_FILE:-/tmp/ruleset-audit/wave1-plan.jsonl}"
RESULTS_FILE="${RESULTS_FILE:-$SCRIPT_DIR/wave1-results.tsv}"
DRY_RUN=false

[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

require_cmd() {
    local cmd="$1"
    command -v "$cmd" >/dev/null 2>&1 || {
        echo "ERROR: required command '$cmd' not found in PATH" >&2
        exit 2
    }
}

preflight() {
    require_cmd gh
    require_cmd jq
    if [ ! -f "$REPOS_FILE" ]; then
        echo "ERROR: repos file not found: $REPOS_FILE" >&2
        exit 2
    fi
    if [ ! -f "$SCRIPT_DIR/reference-ruleset.json" ]; then
        echo "ERROR: reference ruleset missing: $SCRIPT_DIR/reference-ruleset.json" >&2
        exit 2
    fi
    if ! gh auth status >/dev/null 2>&1; then
        echo "ERROR: gh auth invalid. Run: gh auth login -h github.com" >&2
        exit 2
    fi
}

generate_plan() {
    : > "$PLAN_FILE"
    while IFS=$'\t' read -r repo _private; do
        [ -z "$repo" ] && continue
        jq -cn \
            --arg owner "$OWNER" \
            --arg repo "$repo" \
            --arg body_file "$SCRIPT_DIR/reference-ruleset.json" \
            '{method:"POST", path:("repos/"+$owner+"/"+$repo+"/rulesets"), body_file:$body_file, repo:$repo}' \
            >> "$PLAN_FILE"
    done < "$REPOS_FILE"
}

apply_repo() {
    local repo="$1"

    if [[ "$DRY_RUN" == "true" ]]; then
        echo -e "$repo\tDRY_RUN\t\tplan only"
        return
    fi

    local result=""
    local err_file=""
    local err_text=""
    local ruleset_id=""

    err_file=$(mktemp)
    result=$(gh api "repos/$OWNER/$repo/rulesets" -X POST --input "$SCRIPT_DIR/reference-ruleset.json" 2>"$err_file") || {
        err_text=$(cat "$err_file")
        rm -f "$err_file"
        if echo "$err_text" | grep -Eiq 'name must be unique|already exists'; then
            echo -e "$repo\tALREADY_PRESENT\t\tBase ruleset already exists"
            return
        fi
        if echo "$err_text" | grep -Eiq 'upgrade to github pro|make this repository public to enable this feature|feature is not available for private repositories'; then
            echo -e "$repo\tPLAN_GATED\t\tfeature unavailable for this repository visibility/plan"
            return
        fi
        err_text=$(echo "$err_text" | tr '\n' ' ' | tr '\t' ' ' | sed 's/  */ /g' | sed 's/^ *//; s/ *$//')
        echo -e "$repo\tERROR\t\t$err_text"
        return
    }
    rm -f "$err_file"

    ruleset_id=$(echo "$result" | jq -r '.id // empty')
    echo -e "$repo\tOK\t$ruleset_id\tcreated Base ruleset"
}

preflight
generate_plan

echo "owner=$OWNER" >&2
echo "repos_file=$REPOS_FILE" >&2
echo "plan_file=$PLAN_FILE" >&2
echo "results_file=$RESULTS_FILE" >&2
[[ "$DRY_RUN" == "true" ]] && echo "=== DRY RUN — no mutations ===" >&2

: > "$RESULTS_FILE"
total=$(wc -l < "$REPOS_FILE")
i=0

while IFS= read -r repo; do
    repo="${repo%%$'\t'*}"
    [ -z "$repo" ] && continue
    i=$((i+1))
    printf '[%d/%d] %s\n' "$i" "$total" "$repo" >&2
    apply_repo "$repo" >> "$RESULTS_FILE"
done < "$REPOS_FILE"

echo "" >&2
echo "=== Wave 1 complete ===" >&2
echo "Results: $RESULTS_FILE" >&2
echo "Plan: $PLAN_FILE" >&2
awk -F'\t' '{print $2}' "$RESULTS_FILE" | sort | uniq -c | sort -rn >&2
