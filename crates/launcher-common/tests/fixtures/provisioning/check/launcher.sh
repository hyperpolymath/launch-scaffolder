#!/usr/bin/env bash
# SPDX-License-Identifier: MPL-2.0
# SPDX-FileCopyrightText: 2026 test
#
# @launcher-deed begin
# ;; SPDX-License-Identifier: MPL-2.0
# (praxis-deed
#   :schema-version  "1.0.0"
#   :canonical-name  "x-launcher"
#   :beholding-chora #u5"estate/chora"
#   (artefact :type "launcher" :version "0.1.0"
#             :generator "provision-set")
#   (app :name "x" :display "x"
#        :url "https://github.com/x" :archetype "x")
#   (compliance :standard-version "0.5.0"
#               :standards ("launcher-standard.adoc"
#                           "PROVISIONING-STANDARD.adoc"))
#   (modes :accepted ("--setup" "--doctor" "--heal" "--ai-setup" "--help" "--version"
#                     "--start" "--stop" "--status" "--auto" "--integ" "--disinteg"))
#   (platforms :supported ("linux" "macos" "windows"))
#   (lifecycle-phases :covered ("provision" "diagnose" "heal")
#                     :deferred ("install" "run")))
# @launcher-deed end
#
# The launcher for a library / tool / theory / docs repository
# (launcher-standard 0.5.0, archetype profile). Every mode is implemented in
# build/just/provision-modes.sh, and every provisioning mode delegates to the
# Justfile. This file is minted by `provision-set`; do not hand-edit it.
# Repo-specific facts belong in .machine_readable/descriptiles/provisioning.deed.
set -uo pipefail

REPO_DIR="$(cd "$(dirname "$(readlink -f "${BASH_SOURCE[0]}")")" && pwd)"

if [ ! -f "$REPO_DIR/build/just/provision-modes.sh" ]; then
  echo "launcher.sh: build/just/provision-modes.sh is missing — this checkout is incomplete." >&2
  echo "Re-clone, or restore it: git checkout -- build/just/provision-modes.sh" >&2
  exit 1
fi
# shellcheck source=build/just/provision-modes.sh
. "$REPO_DIR/build/just/provision-modes.sh"

hp_launcher_main "$@"
