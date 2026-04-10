# SPDX-License-Identifier: PMPL-1.0-or-later
# launch-scaffolder — justfile

default:
    @just --list

# Build debug
build:
    cargo build --workspace

# Build release (optimised, stripped, single-file binary)
release:
    cargo build --workspace --release

# Run the binary with args
run *args:
    cargo run -p launch-scaffolder -- {{args}}

# Run all tests
test:
    cargo test --workspace

# Clippy — strict
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Format
fmt:
    cargo fmt --all

# Check formatting without writing
fmt-check:
    cargo fmt --all -- --check

# Install the binary to ~/.cargo/bin
install:
    cargo install --path crates/launcher

# Uninstall
uninstall:
    cargo uninstall launch-scaffolder

# Clean build artefacts
clean:
    cargo clean

# Pre-commit check sequence
pre-commit: fmt-check lint test

# Full CI sequence
ci: fmt-check lint test
    @echo "✓ CI passed"

# Print the baked-in launcher standard
standard:
    cargo run -p launch-scaffolder -- standard show

# Validate the launcher standard file
validate-standard:
    cargo run -p launch-scaffolder -- standard validate

# Smoke test: mint a launcher from the stapeln example into /tmp
smoke-mint:
    cargo run -p launch-scaffolder -- mint examples/stapeln.launcher.a2ml -o /tmp/stapeln-launcher.sh
    @echo "Smoke test: mint produced /tmp/stapeln-launcher.sh"

# Mint a launcher in-place. Pass the path to an <app>.launcher.a2ml file.
# Example:  just mint /var/mnt/eclipse/repos/aerie/aerie.launcher.a2ml
mint config:
    cargo run --release -p launch-scaffolder -- mint {{config}}

# Re-mint every scaffolder-managed launcher in the estate. Edit the list
# when adding/removing managed repos. Exceptions live in
# docs/launcher-exceptions-2026-04-10.md.
mint-all:
    #!/usr/bin/env bash
    set -euo pipefail
    BIN="./target/release/launch-scaffolder"
    [ -x "$BIN" ] || cargo build --release
    for cfg in \
        /var/mnt/eclipse/repos/aerie/aerie.launcher.a2ml \
        /var/mnt/eclipse/repos/burble/burble.launcher.a2ml \
        /var/mnt/eclipse/repos/game-server-admin/game-server-admin.launcher.a2ml \
        /var/mnt/eclipse/repos/nextgen-databases/nqc/nqc.launcher.a2ml \
        /var/mnt/eclipse/repos/panll/panll.launcher.a2ml \
        /var/mnt/eclipse/repos/project-wharf/project-wharf.launcher.a2ml \
        /var/mnt/eclipse/repos/stapeln/stapeln.launcher.a2ml ; do
        "$BIN" mint "$cfg"
    done
    @echo "✓ Estate re-mint complete (7 launchers)"

# Generate cargo docs
doc:
    cargo doc --workspace --no-deps --open
