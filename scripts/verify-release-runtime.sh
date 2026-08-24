#!/usr/bin/env bash
# Hard release gate for the local-first beta runtime. Never silently ship a developer-only build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MEDIA="$ROOT/studio/build/runtime"
MODELS="$ROOT/studio/build/model-runtime"

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

for binary in "$MEDIA/ffmpeg" "$MEDIA/ffprobe"; do
  if [[ ! -x "$binary" ]]; then
    echo "Missing bundled media runtime: $binary" >&2
    exit 2
  fi
done
if ! "$MEDIA/ffmpeg" -hide_banner -filters 2>/dev/null | awk '{print $2}' | grep -qx ass; then
  echo "Bundled ffmpeg must include the ass/libass filter." >&2
  exit 2
fi

if [[ ! -d "$MODELS" || ! -x "$MODELS/.venv/bin/python" ]]; then
  echo "Missing bundled shared model Python runtime: $MODELS/.venv/bin/python" >&2
  exit 2
fi
if [[ ! -d "$MODELS/double_love_asr" || ! -d "$MODELS/double_love_speaker" ]]; then
  echo "Bundled shared model runtime must contain both sidecar packages." >&2
  exit 2
fi
if [[ -e "$MODELS/asr" || -e "$MODELS/speaker" ]]; then
  echo "Bundled model runtime contains the legacy asr/ or speaker/ layout." >&2
  exit 2
fi
if [[ -f "$MODELS/.venv/pyvenv.cfg" ]] && grep -q '^home = ' "$MODELS/.venv/pyvenv.cfg"; then
  echo "Bundled shared model runtime is an ordinary virtualenv, not a clean-machine runtime." >&2
  exit 2
fi

PYTHON="$MODELS/.venv/bin/python"
(cd "$MODELS" && \
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
assert_no_forbidden_backend_files "$MODELS/.venv"
verify_model_runtime "$MODELS"

echo "Release runtime verification passed."
