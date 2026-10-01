#!/usr/bin/env bash
# render.sh TEMPLATE VARSFILE — replace __KEY__ with values from VARSFILE (KEY<TAB>value,
# value may contain \n for newlines). Unknown slots are left for provision-check to catch.
set -euo pipefail
# bash 5.2 expands & in a ${var//pat/rep} replacement to the match; values are literal.
shopt -u patsub_replacement 2>/dev/null || true
tmpl=$1 vars=$2
content=$(cat "$tmpl")
while IFS=$'\t' read -r k v; do
  [ -z "$k" ] && continue
  v=$(printf '%b' "$v")
  content=${content//"__${k}__"/$v}
done < "$vars"
printf '%s\n' "$content"
