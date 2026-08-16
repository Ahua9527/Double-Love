#!/usr/bin/env bash
# Double Love ASR 环境准备：
#   1. 建 sidecars/asr/.venv（不进 git）
#   2. 安装 pin 的依赖（mlx-qwen3-asr）
#   3. mock 协议自检（不下载模型，秒级）
#   4. 预下载 Qwen3-ASR-1.7B 权重到 ~/.cache/double-love/models（~3.4GB，可 --skip-model）
# 运行时保持离线：引擎只从本地 HF 缓存读权重。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ASR_DIR="$ROOT/sidecars/asr"
VENV="$ASR_DIR/.venv"
export HF_HOME="${DOUBLELOVE_MODELS_DIR:-$HOME/.cache/double-love/models}"

# mlx-qwen3-asr 需要 Python ≥ 3.10。寻找顺序：
#   $DOUBLELOVE_ASR_PYTHON → python3.13…3.10 → python3（若够新）→ uv 安装的用户级 3.12
find_python() {
  local candidate
  if [[ -n "${DOUBLELOVE_ASR_PYTHON:-}" ]] \
    && "$DOUBLELOVE_ASR_PYTHON" -c 'import sys; assert sys.version_info >= (3, 10)' 2>/dev/null; then
    echo "$DOUBLELOVE_ASR_PYTHON"; return 0
  fi
  for candidate in python3.13 python3.12 python3.11 python3.10 python3; do
    if command -v "$candidate" >/dev/null 2>&1 \
      && "$candidate" -c 'import sys; assert sys.version_info >= (3, 10)' 2>/dev/null; then
      command -v "$candidate"; return 0
    fi
  done
  if command -v uv >/dev/null 2>&1; then
    echo "==> 系统 Python 过旧，用 uv 安装用户级 Python 3.12（不改系统环境）" >&2
    uv python install 3.12 >&2
    uv python find 3.12 2>/dev/null && return 0
  fi
  return 1
}

PYTHON="$(find_python)" || {
  echo "找不到 Python ≥ 3.10。请安装后重试：brew install python@3.12（或 uv python install 3.12）" >&2
  exit 1
}
echo "==> 使用解释器 ${PYTHON} ($("$PYTHON" --version 2>&1))"

echo "==> 创建虚拟环境 $VENV"
"$PYTHON" -m venv "$VENV"
"$VENV/bin/pip" install --quiet --upgrade pip
"$VENV/bin/pip" install --quiet -r "$ASR_DIR/requirements.txt"

echo "==> mock 协议自检（不加载模型）"
READY="$(echo '{"cmd":"hello","version":1}' | (cd "$ASR_DIR" && DOUBLELOVE_ASR_MOCK=1 "$VENV/bin/python" -m double_love_asr) | head -1 || true)"
case "$READY" in
  *'"event": "ready"'*|*'"event":"ready"'*) echo "    ok: $READY" ;;
  *) echo "自检失败：未收到 ready 事件（输出：$READY）" >&2; exit 1 ;;
esac

if [[ "${1:-}" == "--skip-model" ]]; then
  echo "==> 跳过模型下载（--skip-model）；首次真实转录时会自动下载"
  exit 0
fi

echo "==> 预下载 Qwen3-ASR-1.7B 权重到 ${HF_HOME}（约 3.4GB，仅需一次）"
(cd "$ASR_DIR" && "$VENV/bin/python" -c "
from mlx_qwen3_asr import Session
Session(model='Qwen/Qwen3-ASR-1.7B')
print('model ready')
")
echo "==> 完成。运行时引擎离线使用本地缓存。"
