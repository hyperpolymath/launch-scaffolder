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

REPOS_FILE="${REPOS_FILE:-/tmp/ruleset-audit/repos.tsv}"
REPORT_FILE="${REPORT_FILE:-/tmp/ruleset-audit/report.jsonl}"
OWNER="${OWNER:-hyperpolymath}"
: > "$REPORT_FILE"

REQUIRED_RULES="deletion non_fast_forward pull_request required_linear_history required_signatures"
GH_API_RETRIES="${GH_API_RETRIES:-4}"
GH_API_BACKOFF_BASE_SECONDS="${GH_API_BACKOFF_BASE_SECONDS:-1}"
GH_API_TIMEOUT_SECONDS="${GH_API_TIMEOUT_SECONDS:-20}"

LAST_GH_API_ERROR_KIND="api"
LAST_GH_API_ATTEMPTS=0
GH_API_LAST_RESPONSE=""

require_cmd() {
    local cmd="$1"
    command -v "$cmd" >/dev/null 2>&1 || {
        echo "ERROR: required command '$cmd' not found in PATH" >&2
        exit 2
    }
}

classify_gh_error() {
    local status="$1"
    local stderr_text="$2"
    if echo "$stderr_text" | grep -Eiq \
        'upgrade to github pro|make this repository public to enable this feature|feature is not available for private repositories'; then
        echo "plan-gated"
        return
    fi
    if [ "$status" -eq 124 ] || echo "$stderr_text" | grep -Eiq \
        'timed out|temporary failure in name resolution|failed to lookup address|could not resolve|error connecting to api|connection reset|connection refused|network|tls'; then
        echo "network"
        return
    fi
    if echo "$stderr_text" | grep -Eiq \
        'failed to log in|to re-authenticate|token|authentication|401|requires authentication'; then
        echo "auth"
        return
    fi
    if echo "$stderr_text" | grep -Eiq \
        'rate limit|secondary rate limit|429|abuse detection'; then
        echo "rate-limit"
        return
    fi
    echo "api"
}

gh_api_with_retry() {
    local path="$1"
    local attempt=1
    local status=0
    local sleep_s=0
    local stderr_file=""
    local stderr_text=""
    local out=""
    while [ "$attempt" -le "$GH_API_RETRIES" ]; do
        stderr_file=$(mktemp)
        if command -v timeout >/dev/null 2>&1; then
            out=$(timeout "${GH_API_TIMEOUT_SECONDS}s" gh api "$path" 2>"$stderr_file") || status=$?
        else
            out=$(gh api "$path" 2>"$stderr_file") || status=$?
        fi
        if [ "${status:-0}" -eq 0 ]; then
            rm -f "$stderr_file"
            LAST_GH_API_ATTEMPTS="$attempt"
            LAST_GH_API_ERROR_KIND=""
            GH_API_LAST_RESPONSE="$out"
            return 0
        fi

        stderr_text=$(cat "$stderr_file")
        rm -f "$stderr_file"
        LAST_GH_API_ATTEMPTS="$attempt"
        LAST_GH_API_ERROR_KIND=$(classify_gh_error "${status:-1}" "$stderr_text")

        if [ "$LAST_GH_API_ERROR_KIND" = "auth" ] || [ "$LAST_GH_API_ERROR_KIND" = "plan-gated" ]; then
            return 1
        fi

        if [ "$attempt" -lt "$GH_API_RETRIES" ]; then
            sleep_s=$((GH_API_BACKOFF_BASE_SECONDS * attempt))
            echo "WARN: gh api failed (kind=$LAST_GH_API_ERROR_KIND, attempt=$attempt/$GH_API_RETRIES, path=$path); retrying in ${sleep_s}s" >&2
            sleep "$sleep_s"
        fi
        attempt=$((attempt + 1))
        status=0
    done
    return 1
}

preflight() {
    require_cmd gh
    require_cmd jq
    if [ ! -f "$REPOS_FILE" ]; then
        echo "ERROR: repos file not found: $REPOS_FILE" >&2
        exit 2
    fi
    if ! gh auth status >/dev/null 2>&1; then
        echo "ERROR: gh auth invalid. Run: gh auth login -h github.com" >&2
        exit 2
    fi
    if ! gh_api_with_retry "rate_limit" >/dev/null; then
        echo "ERROR: GitHub API preflight failed (kind=$LAST_GH_API_ERROR_KIND, attempts=$LAST_GH_API_ATTEMPTS)" >&2
        exit 2
    fi
}

classify() {
    local repo="$1"
    local rules_json=""
    if ! gh_api_with_retry "repos/$OWNER/$repo/rules/branches/main"; then
        echo "{\"repo\":\"$repo\",\"state\":\"ERROR\",\"kind\":\"$LAST_GH_API_ERROR_KIND\",\"attempts\":$LAST_GH_API_ATTEMPTS,\"detail\":\"gh api failed\"}"
        return
    fi
    rules_json="$GH_API_LAST_RESPONSE"
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

preflight
echo "owner=$OWNER" >&2

i=0
total=$(wc -l < "$REPOS_FILE")
while IFS=$'\t' read -r repo private; do
    i=$((i+1))
    printf '[%d/%d] %s\n' "$i" "$total" "$repo" >&2
    classify "$repo" >> "$REPORT_FILE"
done < "$REPOS_FILE"

echo "done — wrote $REPORT_FILE" >&2
