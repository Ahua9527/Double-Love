#!/usr/bin/env bash
# Hard release gate for the local-first beta runtime. Never silently ship a developer-only build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MEDIA="$ROOT/studio/build/runtime"
MODELS="$ROOT/studio/build/model-runtime"

assert_no_legacy_backend_files() {
  local root="$1" name="$2" found site_packages module
  site_packages="$("$root/bin/python" -c 'import sysconfig; print(sysconfig.get_paths()["purelib"])')"
  for module in torch torchaudio wespeaker silero_vad onnxruntime; do
    if [[ -e "$site_packages/$module" ]]; then
      echo "$name runtime contains a forbidden legacy backend module: $site_packages/$module" >&2
      exit 2
    fi
  done
  found="$(find "$root" -type f \
    \( -name 'libtorch*.dylib' -o -name 'libonnxruntime*.dylib' \) -print -quit)"
  if [[ -n "$found" ]]; then
    echo "$name runtime contains a forbidden legacy backend artifact: $found" >&2
    exit 2
  fi
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
for runtime in asr speaker; do
  if [[ ! -x "$MODELS/$runtime/.venv/bin/python" ]]; then
    echo "Missing bundled $runtime Python runtime." >&2
    exit 2
  fi
  if [[ -f "$MODELS/$runtime/.venv/pyvenv.cfg" ]] && grep -q '^home = ' "$MODELS/$runtime/.venv/pyvenv.cfg"; then
    echo "Bundled $runtime runtime is an ordinary virtualenv, not a clean-machine runtime." >&2
    exit 2
  fi
done
assert_no_legacy_backend_files "$MODELS/asr/.venv" "ASR"
assert_no_legacy_backend_files "$MODELS/speaker/.venv" "Speaker"
"$MODELS/asr/.venv/bin/python" -c '
import importlib.metadata as metadata
import modelscope, modelscope_hub
import double_love_asr, double_love_asr.modelscope_download
assert metadata.version("modelscope") == "1.39.1"
assert metadata.version("modelscope-hub") == "0.2.0"
assert modelscope.__version__ == "1.39.1"
assert modelscope_hub.__version__ == "0.2.0"
for forbidden in ("torch", "torchaudio", "wespeaker", "silero-vad", "onnxruntime"):
    try:
        metadata.version(forbidden)
    except metadata.PackageNotFoundError:
        continue
    raise SystemExit(f"ASR runtime must not contain {forbidden}")
'
"$MODELS/speaker/.venv/bin/python" -c '
import importlib.metadata as metadata
import mlx, mlx_audio, numpy
import double_love_speaker.engine, double_love_speaker.mlx_resnet
assert metadata.version("mlx") == "0.31.1"
assert metadata.version("mlx-audio") == "0.5.0"
for forbidden in ("torch", "torchaudio", "wespeaker", "silero-vad", "onnxruntime"):
    try:
        metadata.version(forbidden)
    except metadata.PackageNotFoundError:
        continue
    raise SystemExit(f"Speaker runtime must not contain {forbidden}")
'
echo "Release runtime verification passed."
