#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NODE_GYP="$(find "$ROOT/node_modules/.pnpm" -path '*/node-gyp/bin/node-gyp.js' -print -quit)"
if [[ -z "$NODE_GYP" ]]; then
  echo "node-gyp is missing; run pnpm install" >&2
  exit 2
fi
node "$NODE_GYP" rebuild \
  --directory "$ROOT/native/avfoundation-player" \
  --target="$(node -p "require('$ROOT/node_modules/electron/package.json').version")" \
  --arch=arm64 \
  --dist-url=https://electronjs.org/headers
mkdir -p "$ROOT/build/native"
cp "$ROOT/native/avfoundation-player/build/Release/avfoundation_player.node" "$ROOT/build/native/avfoundation_player.node"
