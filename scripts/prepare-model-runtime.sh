#!/usr/bin/env bash
# Copy prebuilt, relocatable local model runtimes into the Studio resource tree before a release.
# This is intentionally a release-machine step: end users should not need Homebrew or Python.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/studio/build/model-runtime"
ASR_SOURCE="${DOUBLELOVE_ASR_RUNTIME_SOURCE:-}"
SPEAKER_SOURCE="${DOUBLELOVE_SPEAKER_RUNTIME_SOURCE:-}"

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

mkdir -p "$TARGET/asr" "$TARGET/speaker"
ditto "$ASR_SOURCE/double_love_asr" "$TARGET/asr/double_love_asr"
ditto "$ASR_SOURCE/.venv" "$TARGET/asr/.venv"
ditto "$SPEAKER_SOURCE/double_love_speaker" "$TARGET/speaker/double_love_speaker"
ditto "$SPEAKER_SOURCE/.venv" "$TARGET/speaker/.venv"
echo "Prepared model runtimes in $TARGET"
