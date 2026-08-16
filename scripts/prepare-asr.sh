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

echo "==> 创建虚拟环境 $VENV"
python3 -m venv "$VENV"
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

echo "==> 预下载 Qwen3-ASR-1.7B 权重到 $HF_HOME（约 3.4GB，仅需一次）"
(cd "$ASR_DIR" && "$VENV/bin/python" -c "
from mlx_qwen3_asr import Session
Session(model='Qwen/Qwen3-ASR-1.7B')
print('model ready')
")
echo "==> 完成。运行时引擎离线使用本地缓存。"
