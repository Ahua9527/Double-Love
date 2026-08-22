#!/usr/bin/env bash
# Hard release gate for the local-first beta runtime. Never silently ship a developer-only build.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MEDIA="$ROOT/studio/build/runtime"
MODELS="$ROOT/studio/build/model-runtime"

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
"$MODELS/asr/.venv/bin/python" -c "import double_love_asr" >/dev/null
"$MODELS/speaker/.venv/bin/python" -c "import double_love_speaker" >/dev/null
echo "Release runtime verification passed."
