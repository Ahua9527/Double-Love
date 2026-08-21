#!/usr/bin/env bash
# 本地说话人模型准备：创建独立环境、固定 WeSpeaker 提交、下载模型、运行 mock 协议自检。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPEAKER_DIR="$ROOT/sidecars/speaker"
VENV="$SPEAKER_DIR/.venv"
export WESPEAKER_HOME="${DOUBLELOVE_MODELS_DIR:-$HOME/.cache/double-love/models}/wespeaker"

find_python() {
  local candidate
  if [[ -n "${DOUBLELOVE_SPEAKER_PYTHON:-}" ]] \
    && "$DOUBLELOVE_SPEAKER_PYTHON" -c 'import sys; assert sys.version_info >= (3, 10)' 2>/dev/null; then
    echo "$DOUBLELOVE_SPEAKER_PYTHON"; return 0
  fi
  for candidate in python3.12 python3.11 python3.10 python3; do
    if command -v "$candidate" >/dev/null 2>&1 \
      && "$candidate" -c 'import sys; assert sys.version_info >= (3, 10)' 2>/dev/null; then
      command -v "$candidate"; return 0
    fi
  done
  return 1
}

PYTHON="$(find_python)" || {
  echo "找不到 Python ≥ 3.10。请安装后重试：brew install python@3.12" >&2
  exit 1
}
echo "==> 使用解释器 ${PYTHON} ($("$PYTHON" --version 2>&1))"
"$PYTHON" -m venv "$VENV"
"$VENV/bin/pip" install --quiet --upgrade pip
"$VENV/bin/pip" install --quiet -r "$SPEAKER_DIR/requirements.txt"

echo "==> mock 协议自检（不加载模型）"
READY="$(echo '{"cmd":"hello","version":1}' | (cd "$SPEAKER_DIR" && DOUBLELOVE_SPEAKER_MOCK=1 "$VENV/bin/python" -m double_love_speaker) | head -1 || true)"
case "$READY" in
  *'"event": "ready"'*|*'"event":"ready"'*) echo "    ok: $READY" ;;
  *) echo "自检失败：未收到 ready 事件" >&2; exit 1 ;;
esac

if [[ "${1:-}" == "--skip-model" ]]; then
  echo "==> 跳过 WeSpeaker 模型下载（--skip-model）"
  exit 0
fi

echo "==> 下载 WeSpeaker 中文模型到 ${WESPEAKER_HOME}（仅需一次）"
(cd "$SPEAKER_DIR" && "$VENV/bin/python" -c "import wespeaker; wespeaker.load_model('chinese'); print('speaker model ready')")
echo "==> 完成。运行时只读取本地模型缓存。"
