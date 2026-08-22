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
require_file "$RESOURCES/model-runtime/README.md"
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
