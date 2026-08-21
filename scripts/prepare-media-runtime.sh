#!/usr/bin/env bash
# Prepare the ffmpeg runtime that is bundled with the macOS app. This runs only on the release
# machine; end users never need Homebrew or Python after installation.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/src-tauri/resources/runtime"
FFMPEG_SOURCE="${DOUBLELOVE_FFMPEG_SOURCE:-}"
FFPROBE_SOURCE="${DOUBLELOVE_FFPROBE_SOURCE:-}"

if [[ -z "$FFMPEG_SOURCE" || -z "$FFPROBE_SOURCE" ]]; then
  echo "Set DOUBLELOVE_FFMPEG_SOURCE and DOUBLELOVE_FFPROBE_SOURCE to the vetted release binaries." >&2
  exit 2
fi
if [[ ! -x "$FFMPEG_SOURCE" || ! -x "$FFPROBE_SOURCE" ]]; then
  echo "Both supplied runtime paths must be executable files." >&2
  exit 2
fi
if ! "$FFMPEG_SOURCE" -hide_banner -filters 2>/dev/null | awk '{print $2}' | grep -qx ass; then
  echo "The supplied ffmpeg lacks the required ass/libass filter." >&2
  exit 2
fi

mkdir -p "$TARGET"
cp "$FFMPEG_SOURCE" "$TARGET/ffmpeg"
cp "$FFPROBE_SOURCE" "$TARGET/ffprobe"
chmod 755 "$TARGET/ffmpeg" "$TARGET/ffprobe"
echo "Prepared bundled media runtime in $TARGET"
