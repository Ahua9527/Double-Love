#!/usr/bin/env bash
# Purpose: regenerate engine and host protocol bindings/schemas, verify regeneration is idempotent, then check CLI JSON contracts.
# How to run: scripts/migration/check-bindings-contract.sh
# Requirements: git and the locked Rust workspace toolchain; run from any directory inside this checkout; no credentials or network access are required after dependencies are cached.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

snapshot="$(mktemp -d)"
trap 'rm -rf "$snapshot"' EXIT
cp -R bindings "$snapshot/bindings"

cargo test -p double-love-engine --locked export_bindings
cargo test -p double-love-desktop-host --locked export_bindings
diff -ru "$snapshot/bindings" bindings
cargo test -p double-love-cli --locked --test cli
