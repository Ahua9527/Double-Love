#!/usr/bin/env bash
# Purpose: build a bumped local macOS update artifact and serve a generic feed on 127.0.0.1.
# Run: scripts/migration/local-update-feed.sh [port] (set DOUBLELOVE_FEED_VERSION to override 0.2.1-feed).
# Requirements: macOS arm64, pnpm, python3, built Studio Electron output, and the release desktop host.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
STUDIO="$ROOT/studio"
FEED_VERSION="${DOUBLELOVE_FEED_VERSION:-0.2.1-feed}"
PORT="${1:-${DOUBLELOVE_LOCAL_UPDATE_PORT:-}}"
WORK_DIR="$(mktemp -d "${TMPDIR:-/tmp}/double-love-update-feed.XXXXXX")"
BUILD_DIR="$WORK_DIR/build"
FEED_DIR="$WORK_DIR/feed"
SERVER_PID=""

cleanup() {
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT
trap 'exit 0' INT TERM

if [[ ! -f "$STUDIO/out/main/index.js" || ! -f "$STUDIO/out/renderer/index.html" ]]; then
  echo "Missing Electron output. Run: pnpm --dir studio electron:build" >&2
  exit 2
fi
if [[ ! -x "$ROOT/target/release/double-love-desktop-host" ]]; then
  echo "Missing release host. Run: cargo build -p double-love-desktop-host --release --locked" >&2
  exit 2
fi
if [[ ! "$FEED_VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+-[0-9A-Za-z.-]+$ ]]; then
  echo "DOUBLELOVE_FEED_VERSION must be a prerelease semantic version" >&2
  exit 2
fi

mkdir -p "$BUILD_DIR" "$FEED_DIR"
CSC_IDENTITY_AUTO_DISCOVERY=false \
  pnpm --dir "$STUDIO" exec electron-builder \
    --mac zip --arm64 --publish never \
    -c.extraMetadata.version="$FEED_VERSION" \
    -c.directories.output="$BUILD_DIR" \
    -c.electronDist="$STUDIO/node_modules/electron/dist"

ZIP_PATH="$(find "$BUILD_DIR" -maxdepth 1 -type f -name '*.zip' -print -quit)"
FEED_APP="$(find "$BUILD_DIR" -type d -name 'Double Love Studio.app' -print -quit)"
if [[ -z "$ZIP_PATH" || ! -f "$ZIP_PATH" || -z "$FEED_APP" || ! -d "$FEED_APP" ]]; then
  echo "electron-builder did not produce the feed .app and ZIP" >&2
  exit 2
fi
APP_UPDATE_CONFIG="$FEED_APP/Contents/Resources/app-update.yml"
if [[ ! -f "$APP_UPDATE_CONFIG" ]] || ! grep -Fq 'provider: github' "$APP_UPDATE_CONFIG"; then
  echo "electron-builder did not generate the production app-update.yml" >&2
  exit 2
fi

node - "$ZIP_PATH" "$FEED_DIR" "$FEED_VERSION" <<'NODE'
const { copyFileSync, readFileSync, statSync, writeFileSync } = require('node:fs')
const { createHash } = require('node:crypto')
const { basename, join } = require('node:path')
const [zipPath, feedDir, version] = process.argv.slice(2)
const artifact = basename(zipPath)
const target = join(feedDir, artifact)
copyFileSync(zipPath, target)
const bytes = readFileSync(target)
const sha512 = createHash('sha512').update(bytes).digest('base64')
const size = statSync(target).size
const quotedArtifact = JSON.stringify(artifact)
const manifest = [
  `version: ${version}`,
  'files:',
  `  - url: ${quotedArtifact}`,
  `    sha512: ${sha512}`,
  `    size: ${size}`,
  `path: ${quotedArtifact}`,
  `sha512: ${sha512}`,
  `releaseDate: ${JSON.stringify(new Date().toISOString())}`,
  '',
].join('\n')
writeFileSync(join(feedDir, 'latest-mac.yml'), manifest, { mode: 0o600 })
NODE

if [[ -z "$PORT" ]]; then
  PORT="$(python3 - <<'PY'
import socket
with socket.socket() as server:
    server.bind(('127.0.0.1', 0))
    print(server.getsockname()[1])
PY
)"
fi
if [[ ! "$PORT" =~ ^[0-9]+$ ]] || (( PORT < 1 || PORT > 65535 )); then
  echo "Port must be an integer from 1 through 65535" >&2
  exit 2
fi

python3 -u -m http.server "$PORT" --bind 127.0.0.1 --directory "$FEED_DIR" &
SERVER_PID="$!"
python3 - "$PORT" "$FEED_DIR" "$ZIP_PATH" "$FEED_APP" <<'PY'
import json
import os
import sys
port, feed_dir, zip_path, app_path = sys.argv[1:]
print('DOUBLELOVE_LOCAL_UPDATE_FEED_READY=' + json.dumps({
    'url': f'http://127.0.0.1:{port}/',
    'feedDir': feed_dir,
    'artifact': os.path.basename(zip_path),
    'appPath': app_path,
}, separators=(',', ':')), flush=True)
PY
wait "$SERVER_PID"
