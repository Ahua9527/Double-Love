#!/usr/bin/env bash
# Build one relocatable ASR/Speaker model runtime for release packaging.
# How to run: scripts/migration/build-model-runtime.sh [output-dir]
# (default: build/model-runtime-sources)
# Requirements: macOS arm64, uv with `python install` and `pip compile` support,
# network for lock resolution and pip downloads.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
resolve_output_dir() {
  local output_dir="$1"
  if [[ "$output_dir" != /* ]]; then
    output_dir="$PWD/$output_dir"
  fi
  printf '%s\n' "$output_dir"
}

# Resolve this before any build subshell changes its working directory.
OUT="$(resolve_output_dir "${1:-$ROOT/build/model-runtime-sources}")"
REQUIREMENTS="$ROOT/sidecars/model-runtime-requirements.in"
LOCKFILE="$ROOT/sidecars/model-runtime-requirements.lock"
lock_check_tmp=""

cleanup_lock_check() {
  if [[ -n "$lock_check_tmp" ]]; then
    rm -f "$lock_check_tmp"
  fi
}
trap cleanup_lock_check EXIT

need_uv() {
  if ! command -v uv >/dev/null 2>&1; then
    echo "uv is required (install it on the release machine)." >&2
    exit 2
  fi
  if ! uv python install --help >/dev/null 2>&1; then
    echo "uv does not support the required command: uv python install" >&2
    exit 2
  fi
  if ! uv pip compile --help >/dev/null 2>&1; then
    echo "uv does not support the required command: uv pip compile" >&2
    exit 2
  fi
}

verify_lockfile() {
  lock_check_tmp="$(mktemp "${TMPDIR:-/tmp}/double-love-model-runtime-lock.XXXXXX")"
  if ! uv pip compile "$REQUIREMENTS" \
    --python-version 3.12 \
    --generate-hashes \
    --output-file "$lock_check_tmp"; then
    echo "Unable to compile the shared model runtime lockfile from $REQUIREMENTS." >&2
    return 2
  fi

  if ! diff -u \
    <(sed '1,2d' "$LOCKFILE") \
    <(sed '1,2d' "$lock_check_tmp"); then
    echo "Shared model runtime lockfile drifted; update $LOCKFILE before building." >&2
    return 2
  fi

  rm -f "$lock_check_tmp"
  lock_check_tmp=""
}

assert_no_runtime_auxiliary_paths() {
  local root="$1" path
  for path in "$root/.gitignore" "$root/.lock" "$root/.temp"; do
    if [[ -e "$path" || -L "$path" ]]; then
      echo "shared model runtime contains a uv auxiliary path: $path" >&2
      exit 2
    fi
  done
  for path in "$root"/cpython-*-macos-aarch64-none; do
    if [[ -L "$path" ]]; then
      echo "shared model runtime contains a top-level CPython symlink: $path" >&2
      exit 2
    fi
  done
  for path in "$root/.venv/bin/pip" "$root/.venv/bin/pip3" "$root/.venv/bin/pip3.12"; do
    if [[ -e "$path" || -L "$path" ]]; then
      echo "shared model runtime contains a pip launcher: $path" >&2
      exit 2
    fi
  done
}

verify_model_runtime() {
  local root="$1" python="$1/.venv/bin/python"
  local site_packages stdlib path
  if [[ ! -x "$python" || ! -d "$root/double_love_asr" || ! -d "$root/double_love_speaker" ]]; then
    echo "Shared model runtime is incomplete: $root" >&2
    exit 2
  fi
  if [[ -e "$root/asr" || -e "$root/speaker" ]]; then
    echo "Shared model runtime contains the legacy asr/ or speaker/ layout." >&2
    exit 2
  fi
  if [[ -f "$root/.venv/pyvenv.cfg" ]] && grep -q '^home = ' "$root/.venv/pyvenv.cfg"; then
    echo "Shared model runtime is an ordinary virtualenv, not a clean-machine runtime." >&2
    exit 2
  fi
  assert_no_runtime_auxiliary_paths "$root"
  site_packages="$(PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$python" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')"
  stdlib="$(PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$python" -c 'import sysconfig; print(sysconfig.get_paths()["stdlib"])')"
  (
    cd "$root"
    PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$python" - <<'PY'
import importlib.metadata as metadata
import sysconfig
from pathlib import Path

import double_love_asr
import double_love_speaker.engine
import double_love_speaker.mlx_resnet
import double_love_speaker.silero_mlx
import huggingface_hub
import mlx
import mlx_qwen3_asr
import numpy

for package, version in {"mlx": "0.32.1", "mlx-qwen3-asr": "0.3.5", "numpy": "2.5.2"}.items():
    actual = metadata.version(package)
    if actual != version:
        raise SystemExit(f"{package} must be {version}, got {actual}")
for package in (
    "pip", "setuptools", "wheel", "modelscope", "mlx-audio", "transformers", "scipy",
    "miniaudio", "sounddevice", "tokenizers", "torch", "torchaudio", "wespeaker",
    "silero-vad", "onnxruntime",
):
    try:
        metadata.version(package)
    except metadata.PackageNotFoundError:
        continue
    raise SystemExit(f"shared model runtime must not contain {package}")

paths = sysconfig.get_paths()
purelib = Path(paths["purelib"])
stdlib = Path(paths["stdlib"])
for module in (
    "pip", "setuptools", "wheel", "_distutils_hack", "pkg_resources", "modelscope",
    "mlx_audio", "transformers", "scipy", "miniaudio", "sounddevice", "tokenizers",
    "torch", "torchaudio", "wespeaker", "silero_vad", "onnxruntime",
):
    if (purelib / module).exists():
        raise SystemExit(f"site-packages contains forbidden package path: {purelib / module}")
for path in purelib.iterdir():
    if path.name.startswith(("pip-", "setuptools-", "wheel-")) and path.name.endswith(".dist-info"):
        raise SystemExit(f"site-packages contains forbidden dist-info: {path}")
if (stdlib / "ensurepip").exists():
    raise SystemExit(f"runtime still contains standard-library ensurepip: {stdlib / 'ensurepip'}")
for path in Path.cwd().rglob("*"):
    if path.name == "__pycache__" or path.suffix in {".pyc", ".pyo"}:
        raise SystemExit(f"runtime contains Python bytecode: {path}")
PY
  )
  local asr_hello speaker_hello
  (
    cd "$root"
    asr_hello="$(printf '%s\n' '{"cmd":"hello"}' | PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 DOUBLELOVE_ASR_MOCK=1 "$python" -m double_love_asr)"
    case "$asr_hello" in *'"event": "ready"'*'"mock": true'*) ;; *) echo "ASR mock hello failed: $asr_hello" >&2; exit 2 ;; esac
    speaker_hello="$(printf '%s\n' '{"cmd":"hello"}' | PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 DOUBLELOVE_SPEAKER_MOCK=1 "$python" -m double_love_speaker)"
    case "$speaker_hello" in *'"event": "ready"'*'"mock": true'*) ;; *) echo "Speaker mock hello failed: $speaker_hello" >&2; exit 2 ;; esac
  )
  echo "Shared model runtime import/version/mock hello passed: $root"
}

assert_no_forbidden_backend_files() {
  local root="$1" site_packages module found
  site_packages="$(PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$root/bin/python" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')"
  for module in \
    modelscope mlx_audio transformers scipy miniaudio sounddevice tokenizers \
    torch torchaudio wespeaker silero_vad onnxruntime; do
    if [[ -e "$site_packages/$module" ]]; then
      echo "shared model runtime contains a forbidden backend module: $site_packages/$module" >&2
      exit 2
    fi
  done
  found="$(find "$root" -type f \
    \( -name 'libtorch*.dylib' -o -name 'libonnxruntime*.dylib' \) -print -quit)"
  if [[ -n "$found" ]]; then
    echo "shared model runtime contains a forbidden backend artifact: $found" >&2
    exit 2
  fi
}

prune_model_runtime() {
  local root="$1" site_packages stdlib path
  site_packages="$(PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$root/.venv/bin/python" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')"
  stdlib="$(PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$root/.venv/bin/python" -c 'import sysconfig; print(sysconfig.get_paths()["stdlib"])')"

  # Keep this list explicit: dependency tests/docs/examples are removable, but
  # model assets, native libraries, tokenizer files, LICENSE, and METADATA stay.
  local explicit_paths=(
    "$root/.gitignore"
    "$root/.lock"
    "$root/.temp"
    "$root/.venv/bin/pip"
    "$root/.venv/bin/pip3"
    "$root/.venv/bin/pip3.12"
    "$site_packages/pip"
    "$site_packages"/pip-*.dist-info
    "$site_packages/setuptools"
    "$site_packages"/setuptools-*.dist-info
    "$site_packages/wheel"
    "$site_packages"/wheel-*.dist-info
    "$site_packages/_distutils_hack"
    "$site_packages/pkg_resources"
    "$site_packages/distutils-precedence.pth"
    "$site_packages/anyio/tests"
    "$site_packages/certifi/tests"
    "$site_packages/charset_normalizer/tests"
    "$site_packages/click/tests"
    "$site_packages/fsspec/tests"
    "$site_packages/h11/tests"
    "$site_packages/httpcore/tests"
    "$site_packages/httpx/tests"
    "$site_packages/huggingface_hub/tests"
    "$site_packages/idna/tests"
    "$site_packages/mlx/tests"
    "$site_packages/mlx/examples"
    "$site_packages/mlx/docs"
    "$site_packages/mlx_qwen3_asr/tests"
    "$site_packages/mlx_qwen3_asr/examples"
    "$site_packages/mlx_qwen3_asr/docs"
    "$site_packages/numpy/_core/tests"
    "$site_packages/numpy/_pyinstaller/tests"
    "$site_packages/numpy/doc"
    "$site_packages/numpy/f2py/tests"
    "$site_packages/numpy/fft/tests"
    "$site_packages/numpy/lib/tests"
    "$site_packages/numpy/linalg/tests"
    "$site_packages/numpy/ma/tests"
    "$site_packages/numpy/matrixlib/tests"
    "$site_packages/numpy/polynomial/tests"
    "$site_packages/numpy/random/tests"
    "$site_packages/numpy/testing/tests"
    "$site_packages/numpy/tests"
    "$site_packages/numpy/typing/tests"
    "$site_packages/packaging/tests"
    "$site_packages/packaging/docs"
    "$site_packages/packaging/examples"
    "$site_packages/regex/tests"
    "$site_packages/requests/tests"
    "$site_packages/tqdm/tests"
    "$site_packages/urllib3/test"
    "$site_packages/yaml/tests"
    "$stdlib/ensurepip"
  )

  for path in "${explicit_paths[@]}"; do
    rm -rf "$path"
  done

  for path in "$root"/cpython-*-macos-aarch64-none; do
    if [[ -L "$path" ]]; then
      rm -f "$path"
    fi
  done

  # Bytecode is never a runtime asset and must not be regenerated by the gates.
  find "$root" -type d -name '__pycache__' -prune -exec rm -rf {} +
  find "$root" -type f \( -name '*.pyc' -o -name '*.pyo' \) -delete
}

need_uv
if [[ ! -f "$REQUIREMENTS" ]]; then
  echo "Missing shared runtime requirements input: $REQUIREMENTS" >&2
  exit 2
fi
if [[ ! -f "$LOCKFILE" ]]; then
  echo "Missing shared runtime lockfile: $LOCKFILE" >&2
  exit 2
fi
verify_lockfile

echo "==> building shared model runtime in $OUT"
rm -rf "$OUT"
mkdir -p "$OUT"

# Install standalone CPython exactly once. It is moved under the one shared root;
# both sidecars use this interpreter in the packaged app.
uv python install --install-dir "$OUT" cpython-3.12 >/dev/null
install_dir="$(find "$OUT" -maxdepth 1 -type d -name 'cpython-*' -print -quit)"
if [[ -z "$install_dir" || ! -x "$install_dir/bin/python" ]]; then
  echo "standalone Python install failed" >&2
  exit 2
fi
mv "$install_dir" "$OUT/.venv"
rm -f "$OUT/uv-install.log" 2>/dev/null || true

if [[ ! -x "$OUT/.venv/bin/python" ]]; then
  echo "shared runtime Python is missing: $OUT/.venv/bin/python" >&2
  exit 2
fi

# Install the complete, hashed dependency closure exactly once.
(cd "$OUT" && \
  PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 \
  "$OUT/.venv/bin/python" -m pip install --quiet --break-system-packages \
    --require-hashes -r "$LOCKFILE")

cp -R "$ROOT/sidecars/asr/double_love_asr" "$OUT/double_love_asr"
cp -R "$ROOT/sidecars/speaker/double_love_speaker" "$OUT/double_love_speaker"

(cd "$OUT" && \
  PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 \
  "$OUT/.venv/bin/python" - <<'PY'
import importlib.metadata as metadata

import double_love_asr
import double_love_speaker.engine
import double_love_speaker.mlx_resnet
import double_love_speaker.silero_mlx
import huggingface_hub
import mlx
import mlx_qwen3_asr
import numpy

expected = {
    "mlx": "0.32.1",
    "mlx-qwen3-asr": "0.3.5",
    "numpy": "2.5.2",
}
for package, version in expected.items():
    actual = metadata.version(package)
    if actual != version:
        raise SystemExit(f"{package} must be {version}, got {actual}")

for package in (
    "modelscope",
    "mlx-audio",
    "transformers",
    "scipy",
    "miniaudio",
    "sounddevice",
    "tokenizers",
    "torch",
    "torchaudio",
    "wespeaker",
    "silero-vad",
    "onnxruntime",
):
    try:
        metadata.version(package)
    except metadata.PackageNotFoundError:
        continue
    raise SystemExit(f"shared model runtime must not contain {package}")
PY
)

assert_no_forbidden_backend_files "$OUT/.venv"

prune_model_runtime "$OUT"
verify_model_runtime "$OUT"

if [[ -f "$OUT/.venv/pyvenv.cfg" ]] && grep -q '^home = ' "$OUT/.venv/pyvenv.cfg"; then
  echo "shared runtime still references a build-machine Python home" >&2
  exit 2
fi

echo "Shared model runtime ready in $OUT"
