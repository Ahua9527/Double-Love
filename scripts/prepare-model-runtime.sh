#!/usr/bin/env bash
# Copy prebuilt, relocatable local model runtimes into the Studio resource tree before a release.
# This is intentionally a release-machine step: end users should not need Homebrew or Python.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/studio/build/model-runtime"
ASR_SOURCE="${DOUBLELOVE_ASR_RUNTIME_SOURCE:-}"
SPEAKER_SOURCE="${DOUBLELOVE_SPEAKER_RUNTIME_SOURCE:-}"

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

if [[ -z "$ASR_SOURCE" || -z "$SPEAKER_SOURCE" ]]; then
  echo "Set DOUBLELOVE_ASR_RUNTIME_SOURCE and DOUBLELOVE_SPEAKER_RUNTIME_SOURCE to self-contained runtime directories." >&2
  echo "The development sidecars/.venv directories are intentionally not accepted for a release." >&2
  exit 2
fi

validate_runtime() {
  local name="$1" source="$2" package="$3"
  if [[ ! -d "$source/$package" || ! -x "$source/.venv/bin/python" ]]; then
    echo "$name runtime must contain $package/ and a self-contained .venv/bin/python." >&2
    exit 2
  fi
  if [[ -f "$source/.venv/pyvenv.cfg" ]] && grep -q '^home = ' "$source/.venv/pyvenv.cfg"; then
    echo "$name runtime looks like a normal virtualenv and may reference the build machine's Python." >&2
    echo "Provide a verified relocatable Python distribution instead." >&2
    exit 2
  fi
  "$source/.venv/bin/python" -c "import $package" >/dev/null
}

validate_runtime "ASR" "$ASR_SOURCE" "double_love_asr"
validate_runtime "Speaker" "$SPEAKER_SOURCE" "double_love_speaker"
assert_no_legacy_backend_files "$ASR_SOURCE/.venv" "ASR"
assert_no_legacy_backend_files "$SPEAKER_SOURCE/.venv" "Speaker"

# The ASR runtime also owns the ModelScope JSONL downloader.  Verify the exact SDK pair
# before copying it into the signed app resource tree; do not rely on a build-machine CLI.
"$ASR_SOURCE/.venv/bin/python" -c '
import importlib.metadata as metadata
import modelscope
import modelscope_hub
expected = {"modelscope": "1.39.1", "modelscope-hub": "0.2.0"}
for package, version in expected.items():
    actual = metadata.version(package)
    if actual != version:
        raise SystemExit(f"{package} must be {version}, got {actual}")
for forbidden in ("torch", "torchaudio", "wespeaker", "silero-vad", "onnxruntime"):
    try:
        metadata.version(forbidden)
    except metadata.PackageNotFoundError:
        continue
    raise SystemExit(f"ASR runtime must not contain {forbidden}")
assert modelscope.__version__ == expected["modelscope"]
assert modelscope_hub.__version__ == expected["modelscope-hub"]
import double_love_asr.modelscope_download
'

"$SPEAKER_SOURCE/.venv/bin/python" -c '
import importlib.metadata as metadata
import mlx, mlx_audio, numpy
import double_love_speaker.engine, double_love_speaker.mlx_resnet
expected = {"mlx": "0.31.1", "mlx-audio": "0.5.0"}
for package, version in expected.items():
    actual = metadata.version(package)
    if actual != version:
        raise SystemExit(f"{package} must be {version}, got {actual}")
for forbidden in ("torch", "torchaudio", "wespeaker", "silero-vad", "onnxruntime"):
    try:
        metadata.version(forbidden)
    except metadata.PackageNotFoundError:
        continue
    raise SystemExit(f"Speaker runtime must not contain {forbidden}")
'

# These are generated, fully-owned resource directories. Merging a new runtime
# over the previous release can leave removed packages (notably torch/ONNX)
# behind, so replace both trees as a unit before copying verified sources.
rm -rf "$TARGET/asr" "$TARGET/speaker"
mkdir -p "$TARGET/asr" "$TARGET/speaker"
ditto "$ASR_SOURCE/double_love_asr" "$TARGET/asr/double_love_asr"
ditto "$ASR_SOURCE/.venv" "$TARGET/asr/.venv"
ditto "$SPEAKER_SOURCE/double_love_speaker" "$TARGET/speaker/double_love_speaker"
ditto "$SPEAKER_SOURCE/.venv" "$TARGET/speaker/.venv"
echo "Prepared model runtimes in $TARGET"
