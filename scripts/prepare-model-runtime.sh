#!/usr/bin/env bash
# Copy one verified, relocatable model runtime into the Studio resource tree.
# This is a release-machine step: end users should not need Homebrew or Python.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/studio/build/model-runtime"
SOURCE="${DOUBLELOVE_MODEL_RUNTIME_SOURCE:-}"
STAGE_ROOT=""

cleanup_stage() {
  if [[ -n "$STAGE_ROOT" && -d "$STAGE_ROOT" ]]; then
    rm -rf "$STAGE_ROOT"
  fi
}
trap cleanup_stage EXIT

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
    if metadata.version(package) != version:
        raise SystemExit(f"{package} has an unexpected version")
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
    raise SystemExit("runtime still contains standard-library ensurepip")
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
}

replace_runtime() {
  local source="$1" target="$2"
  local target_parent stage_runtime next_root previous_root runtime_name

  if [[ ! -d "$target" || ! -f "$target/README.md" ]]; then
    echo "Shared model runtime target must contain its tracked README: $target/README.md" >&2
    exit 2
  fi

  target_parent="$(dirname "$target")"
  STAGE_ROOT="$(mktemp -d "$target_parent/.model-runtime-stage.XXXXXX")"
  stage_runtime="$STAGE_ROOT/runtime"
  next_root="$STAGE_ROOT/next"
  previous_root="$STAGE_ROOT/previous"

  ditto "$source" "$stage_runtime"
  verify_model_runtime "$stage_runtime"

  mkdir -p "$next_root"
  ditto "$target/." "$next_root"
  for runtime_name in .venv double_love_asr double_love_speaker asr speaker; do
    rm -rf "$next_root/$runtime_name"
  done
  for runtime_name in .venv double_love_asr double_love_speaker; do
    ditto "$stage_runtime/$runtime_name" "$next_root/$runtime_name"
  done
  if [[ ! -f "$next_root/README.md" ]]; then
    echo "Staged shared model runtime lost its tracked README: $next_root/README.md" >&2
    exit 2
  fi
  verify_model_runtime "$next_root"

  if ! mv "$target" "$previous_root"; then
    echo "Unable to move the existing shared model runtime aside: $target" >&2
    exit 2
  fi
  if ! mv "$next_root" "$target"; then
    if ! mv "$previous_root" "$target"; then
      echo "Unable to restore the existing shared model runtime after replacement failure." >&2
      exit 2
    fi
    echo "Unable to install the staged shared model runtime: $target" >&2
    exit 2
  fi

  rm -rf "$STAGE_ROOT"
  STAGE_ROOT=""
}

if [[ -z "$SOURCE" ]]; then
  echo "Set DOUBLELOVE_MODEL_RUNTIME_SOURCE to one self-contained shared runtime root." >&2
  echo "The development sidecars/asr and sidecars/speaker directories are not release sources." >&2
  exit 2
fi
if [[ ! -d "$SOURCE" ]]; then
  echo "Shared model runtime source does not exist: $SOURCE" >&2
  exit 2
fi

source_real="$(cd "$SOURCE" && pwd -P)"
SOURCE="$source_real"
target_parent="$(cd "$(dirname "$TARGET")" && pwd -P)"
target_real="$target_parent/$(basename "$TARGET")"
if [[ "$source_real" == "$target_real" ]]; then
  echo "Shared model runtime source must not be the Studio target: $SOURCE" >&2
  exit 2
fi

PYTHON="$SOURCE/.venv/bin/python"
if [[ ! -x "$PYTHON" || ! -d "$SOURCE/double_love_asr" || ! -d "$SOURCE/double_love_speaker" ]]; then
  echo "Shared model runtime must contain .venv/bin/python, double_love_asr/, and double_love_speaker/." >&2
  exit 2
fi
if [[ -e "$SOURCE/asr" || -e "$SOURCE/speaker" ]]; then
  echo "Shared model runtime must not contain the legacy asr/ or speaker/ layout." >&2
  exit 2
fi
if [[ -f "$SOURCE/.venv/pyvenv.cfg" ]] && grep -q '^home = ' "$SOURCE/.venv/pyvenv.cfg"; then
  echo "Shared model runtime looks like an ordinary virtualenv and may reference the build machine." >&2
  exit 2
fi

(cd "$SOURCE" && \
  PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 \
  "$PYTHON" - <<'PY'
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
assert_no_forbidden_backend_files "$SOURCE/.venv"
verify_model_runtime "$SOURCE"

replace_runtime "$SOURCE" "$TARGET"
verify_model_runtime "$TARGET"
echo "Prepared shared model runtime in $TARGET"
