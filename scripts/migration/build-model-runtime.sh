#!/usr/bin/env bash
# Purpose: build relocatable ASR/Speaker model runtimes (standalone CPython + pinned deps + sidecar package) for release packaging.
# How to run: scripts/migration/build-model-runtime.sh [output-dir]  (default: build/model-runtime-sources)
# Requirements: macOS arm64, uv (for standalone CPython), network for pip downloads; run from the repo root.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="${1:-$ROOT/build/model-runtime-sources}"
UV_PYTHON="${DOUBLELOVE_RUNTIME_PYTHON:-cpython-3.12}"

need_uv() {
  if ! command -v uv >/dev/null 2>&1; then
    echo "uv is required (brew install uv)." >&2
    exit 2
  fi
}

build_one() {
  local name="$1" package="$2" requirements="$3"
  local target="$OUT/$name"
  echo "==> building $name runtime in $target"
  rm -rf "$target"
  mkdir -p "$target"
  cp -R "$ROOT/sidecars/$name/$package" "$target/"

  local standalone="$target/.python-standalone"
  rm -rf "$standalone"
  uv python install --install-dir "$target" "$UV_PYTHON" >/dev/null
  local install_dir
  install_dir="$(find "$target" -maxdepth 1 -type d -name 'cpython-*' -print -quit)"
  if [[ -z "$install_dir" || ! -x "$install_dir/bin/python" ]]; then
    echo "standalone python install failed for $name" >&2
    exit 2
  fi
  mv "$install_dir" "$target/.venv"
  rm -f "$target/uv-install.log" 2>/dev/null || true

  # Package must import from any cwd (the verify gate imports from the repo root).
  local site_packages
  site_packages="$("$target/.venv/bin/python" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')"
  cp -R "$target/$package" "$site_packages/$package"

  "$target/.venv/bin/python" -m pip install --quiet --break-system-packages -r "$requirements"
  "$target/.venv/bin/python" -c "import $package" >/dev/null

  # Gate contract: no pyvenv.cfg with a home= entry.
  if [[ -f "$target/.venv/pyvenv.cfg" ]] && grep -q '^home = ' "$target/.venv/pyvenv.cfg"; then
    echo "$name runtime still references a build-machine home" >&2
    exit 2
  fi
  echo "==> $name runtime ready"
}

need_uv
mkdir -p "$OUT"
build_one asr double_love_asr "$ROOT/sidecars/asr/requirements.txt"
build_one speaker double_love_speaker "$ROOT/sidecars/speaker/requirements.txt"
echo "Model runtimes ready in $OUT"
