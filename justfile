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

# Generate cargo docs
doc:
    cargo doc --workspace --no-deps --open
