# SPDX-License-Identifier: MPL-2.0
# launch-scaffolder — justfile

default:
    @just --list

# Build debug
build:
    cargo build --locked --workspace

# Build release (optimised, stripped, single-file binary)
release:
    cargo build --locked --workspace --release

# Run the binary with args
run *args:
    cargo run --locked -p launch-scaffolder -- {{args}}

# Run all tests
test:
    cargo test --locked --workspace

# Check all targets without changing the lockfile
check:
    cargo check --locked --workspace --all-targets

# Clippy — strict
lint:
    cargo clippy --locked --workspace --all-targets -- -D warnings

# Format
fmt:
    cargo fmt --all

# Check formatting without writing
fmt-check:
    cargo fmt --all -- --check

# Install the binary to ~/.cargo/bin
install:
    cargo install --locked --path crates/launcher

# Uninstall
uninstall:
    cargo uninstall launch-scaffolder

# Clean build artefacts
clean:
    cargo clean

# Pre-commit check sequence
pre-commit: fmt-check check lint test

# Full local validation sequence
validate: fmt-check check lint test validate-standard
    @echo "✓ validation passed"

# CI sequence
ci: validate
    @echo "✓ CI passed"

# Print the baked-in launcher standard
standard:
    cargo run --locked -p launch-scaffolder -- standard show

# Validate the launcher standard file
validate-standard:
    cargo run --locked -p launch-scaffolder -- standard validate

# Smoke test: mint a launcher from the stapeln example into /tmp
smoke-mint:
    cargo run --locked -p launch-scaffolder -- mint examples/stapeln.launcher.fixture.a2ml -o /tmp/stapeln-launcher.sh
    @echo "Smoke test: mint produced /tmp/stapeln-launcher.sh"

# Mint a launcher in-place. Pass the path to an <app>.launcher.a2ml file.
# Example:  just mint ../aerie/aerie.launcher.a2ml
mint config:
    cargo run --locked --release -p launch-scaffolder -- mint {{config}}

# Re-mint every scaffolder-managed launcher found under ROOT (the directory
# holding your clones; nothing machine-specific is assumed). Each managed
# config must resolve to exactly one file: a missing or duplicated clone is
# an error naming the candidates, never a guess. Edit the list when
# adding/removing managed repos. Exceptions live in
# docs/launcher-exceptions-2026-04-10.adoc.
mint-all root:
    #!/usr/bin/env bash
    set -euo pipefail
    BIN="./target/release/launch-scaffolder"
    [ -d "{{root}}" ] || { echo "✗ {{root}} is not a directory" >&2; exit 2; }
    [ -x "$BIN" ] || cargo build --locked --release
    status=0 n=0
    for rel in \
        aerie/aerie.launcher.a2ml \
        burble/burble.launcher.a2ml \
        game-server-admin/game-server-admin.launcher.a2ml \
        nqc/nqc.launcher.a2ml \
        panll/panll.launcher.a2ml \
        project-wharf/project-wharf.launcher.a2ml \
        stapeln/stapeln.launcher.a2ml ; do
        mapfile -d '' hits < <(find "{{root}}" -path '*/worktrees' -prune -o -path '*/archive' -prune \
            -o -path "*/$rel" -type f -print0)
        if [ "${#hits[@]}" -ne 1 ]; then
            echo "✗ $rel: ${#hits[@]} matches under {{root}} (need exactly one)" >&2
            for h in "${hits[@]}"; do echo "    $h" >&2; done
            status=1; continue
        fi
        "$BIN" mint "${hits[0]}"; n=$((n + 1))
    done
    echo "Estate re-mint: $n of 7 launchers"
    exit "$status"

# Generate cargo docs
doc:
    cargo doc --locked --workspace --no-deps --open

secret-scan-trufflehog:
    @command -v trufflehog >/dev/null || { echo "trufflehog is required for this scan" >&2; exit 127; }
    trufflehog filesystem . --only-verified
