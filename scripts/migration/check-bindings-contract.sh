#!/usr/bin/env bash
# Purpose: regenerate tracked engine and host protocol bindings/schemas, then verify generated files and CLI JSON contracts are clean.
# How to run: scripts/migration/check-bindings-contract.sh
# Requirements: git and the locked Rust workspace toolchain; run from any directory inside this checkout; no credentials or network access are required after dependencies are cached.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

cargo test -p double-love-engine --locked export_bindings
cargo test -p double-love-desktop-host --locked export_bindings
git diff --exit-code HEAD -- bindings
untracked_bindings="$(git ls-files --others --exclude-standard -- bindings)"
if [[ -n "$untracked_bindings" ]]; then
  printf 'untracked generated bindings:\n%s\n' "$untracked_bindings" >&2
  exit 1
fi
cargo test -p double-love-cli --locked --test cli
