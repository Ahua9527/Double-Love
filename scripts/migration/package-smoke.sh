#!/usr/bin/env bash
# Purpose: verify the unpacked macOS Electron package and boot it through Playwright.
# Run: scripts/migration/package-smoke.sh [path/to/Double Love Studio.app].
# Requirements: release host, Electron build/package output, pnpm, and macOS arm64.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
STUDIO="$ROOT/studio"
APP="${1:-${DOUBLELOVE_PACKAGED_APP:-}}"

if [[ -z "$APP" ]]; then
  APP="$(find "$STUDIO/release" -type d -name 'Double Love Studio.app' -print -quit 2>/dev/null || true)"
fi

assert_no_runtime_auxiliary_paths() {
  local root="$1" path
  for path in "$root/.gitignore" "$root/.lock" "$root/.temp"; do
    if [[ -e "$path" || -L "$path" ]]; then
      echo "packaged shared model runtime contains a uv auxiliary path: $path" >&2
      exit 2
    fi
  done
  for path in "$root"/cpython-*-macos-aarch64-none; do
    if [[ -L "$path" ]]; then
      echo "packaged shared model runtime contains a top-level CPython symlink: $path" >&2
      exit 2
    fi
  done
  for path in "$root/.venv/bin/pip" "$root/.venv/bin/pip3" "$root/.venv/bin/pip3.12"; do
    if [[ -e "$path" || -L "$path" ]]; then
      echo "packaged shared model runtime contains a pip launcher: $path" >&2
      exit 2
    fi
  done
}

verify_model_runtime() {
  local root="$1" python="$1/.venv/bin/python"
  if [[ ! -x "$python" || ! -d "$root/double_love_asr" || ! -d "$root/double_love_speaker" ]]; then
    echo "Packaged shared model runtime is incomplete: $root" >&2
    exit 2
  fi
  if [[ -e "$root/asr" || -e "$root/speaker" ]]; then
    echo "Packaged model runtime contains the legacy asr/ or speaker/ layout." >&2
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
    raise SystemExit(f"packaged model runtime must not contain {package}")

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
if [[ -z "$APP" || ! -d "$APP" ]]; then
  echo "Missing unpacked app. Run: pnpm --dir studio pack:dir" >&2
  exit 2
fi
APP="$(cd "$(dirname "$APP")" && pwd)/$(basename "$APP")"
RESOURCES="$APP/Contents/Resources"
FRAMEWORK_RESOURCES="$APP/Contents/Frameworks/Electron Framework.framework/Versions/A/Resources"
EXECUTABLE="$APP/Contents/MacOS/Double Love Studio"

require_file() {
  if [[ ! -f "$1" ]]; then
    echo "Missing packaged file: $1" >&2
    exit 2
  fi
}

if [[ ! -x "$EXECUTABLE" ]]; then
  echo "Missing packaged app executable: $EXECUTABLE" >&2
  exit 2
fi
if [[ ! -x "$RESOURCES/double-love-desktop-host" ]]; then
  echo "Missing packaged desktop host: $RESOURCES/double-love-desktop-host" >&2
  exit 2
fi
require_file "$RESOURCES/app.asar"
require_file "$RESOURCES/icon.icns"
require_file "$RESOURCES/bindings/host-protocol/schema/HostRequest.schema.json"
require_file "$RESOURCES/bindings/host-protocol/schema/HostResponse.schema.json"
require_file "$RESOURCES/runtime/README.md"
if [[ ! -x "$RESOURCES/model-runtime/.venv/bin/python" ]]; then
  echo "Missing packaged shared model Python runtime: $RESOURCES/model-runtime/.venv/bin/python" >&2
  exit 2
fi
for package in double_love_asr double_love_speaker; do
  if [[ ! -d "$RESOURCES/model-runtime/$package" ]]; then
    echo "Missing packaged model sidecar package: $RESOURCES/model-runtime/$package" >&2
    exit 2
  fi
done
if [[ -e "$RESOURCES/model-runtime/asr" || -e "$RESOURCES/model-runtime/speaker" ]]; then
  echo "Packaged model runtime contains the legacy asr/ or speaker/ layout." >&2
  exit 2
fi
verify_model_runtime "$RESOURCES/model-runtime"
require_file "$FRAMEWORK_RESOURCES/browser_v8_context_snapshot.bin"

FUSES="$(pnpm --dir "$STUDIO" exec electron-fuses read --app "$APP")"
for expected in \
  'RunAsNode is Disabled' \
  'EnableCookieEncryption is Enabled' \
  'EnableNodeOptionsEnvironmentVariable is Disabled' \
  'EnableNodeCliInspectArguments is Disabled' \
  'EnableEmbeddedAsarIntegrityValidation is Enabled' \
  'OnlyLoadAppFromAsar is Enabled' \
  'LoadBrowserProcessSpecificV8Snapshot is Enabled' \
  'GrantFileProtocolExtraPrivileges is Disabled' \
  'WasmTrapHandlers is Enabled'; do
  if ! grep -Fq "$expected" <<<"$FUSES"; then
    printf '%s\n' "$FUSES" >&2
    echo "Unexpected Electron fuse state: $expected" >&2
    exit 2
  fi
done
printf '%s\n' "$FUSES"

DOUBLELOVE_PACKAGED_APP="$APP" \
  pnpm --dir "$STUDIO" exec playwright test e2e/package-smoke.spec.ts

echo "Packaged Electron smoke passed: $APP"
