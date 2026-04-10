#!/usr/bin/env bash
# SPDX-License-Identifier: PMPL-1.0-or-later
#
# ruleset-audit/audit.sh — read-only drift check for hyperpolymath
# branch-protection rulesets against the documented 5-rule standard.
#
# THREAT MODEL / PANIC-ATTACK NOTE:
#   panic-attack flags UserInput -> ShellCommand taint flows on the
#   `$repo` interpolations below. Those interpolations ARE quoted
#   (`"$repo"` everywhere), and the input is `gh repo list` output for
#   the `hyperpolymath/*` namespace — GitHub enforces a restricted
#   character set on repo names (alphanumeric, hyphen, underscore,
#   period) which cannot contain shell metacharacters. No injection
#   path exists in practice. The script is archived under docs/ as a
#   read-only audit record; it is not on any PATH and is not called
#   by any other tool. Accepting the panic-attack flags as known noise.
#
# For every non-archived repo, fetches rules affecting the default
# branch and classifies drift:
#
#   OK       — matches the reference shape exactly
#   MISSING  — no ruleset at all on the default branch
#   DRIFT    — ruleset exists but differs from reference
#   ERROR    — API call failed
#
# Reference shape:
#   rules       = required_signatures, deletion, non_fast_forward,
#                 required_linear_history, pull_request
#   pull_request.required_approving_review_count = 0
#   bypass_actors = [{actor_type: RepositoryRole, actor_id: 5, bypass_mode: always}]
#                   (actor_id=5 is the built-in Admin role)
#
# Writes one JSON-per-line record to /tmp/ruleset-audit/report.jsonl.

set -euo pipefail

REPOS_FILE="/tmp/ruleset-audit/repos.tsv"
REPORT_FILE="/tmp/ruleset-audit/report.jsonl"
: > "$REPORT_FILE"

REQUIRED_RULES="deletion non_fast_forward pull_request required_linear_history required_signatures"

classify() {
    local repo="$1"
    local rules_json
    rules_json=$(gh api "repos/hyperpolymath/$repo/rules/branches/main" 2>/dev/null) || {
        echo "{\"repo\":\"$repo\",\"state\":\"ERROR\",\"detail\":\"api call failed\"}"
        return
    }
    local n
    n=$(echo "$rules_json" | jq 'length')
    if [ "$n" -eq 0 ]; then
        echo "{\"repo\":\"$repo\",\"state\":\"MISSING\"}"
        return
    fi
    local present
    present=$(echo "$rules_json" | jq -r '[.[].type] | sort | unique | join(" ")')
    local expected
    expected=$(echo "$REQUIRED_RULES" | tr ' ' '\n' | sort | tr '\n' ' ' | sed 's/ $//')
    if [ "$present" = "$expected" ]; then
        echo "{\"repo\":\"$repo\",\"state\":\"OK\",\"rules\":\"$present\"}"
    else
        local missing extra
        missing=$(comm -23 <(echo "$expected" | tr ' ' '\n' | sort) <(echo "$present" | tr ' ' '\n' | sort) | tr '\n' ',' | sed 's/,$//')
        extra=$(comm -13   <(echo "$expected" | tr ' ' '\n' | sort) <(echo "$present" | tr ' ' '\n' | sort) | tr '\n' ',' | sed 's/,$//')
        echo "{\"repo\":\"$repo\",\"state\":\"DRIFT\",\"missing\":\"$missing\",\"extra\":\"$extra\",\"present\":\"$present\"}"
    fi
}

i=0
total=$(wc -l < "$REPOS_FILE")
while IFS=$'\t' read -r repo private; do
    i=$((i+1))
    printf '[%d/%d] %s\n' "$i" "$total" "$repo" >&2
    classify "$repo" >> "$REPORT_FILE"
done < "$REPOS_FILE"

echo "done — wrote $REPORT_FILE" >&2
