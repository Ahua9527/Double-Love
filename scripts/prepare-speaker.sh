#!/usr/bin/env bash
# 本地 MLX 说话人运行时准备：只安装签名应用中的推理代码与依赖。
# 用户模型由桌面应用通过 ModelScope 下载、校验并传入本地绝对目录；本脚本不下载权重。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SPEAKER_DIR="$ROOT/sidecars/speaker"
VENV="$SPEAKER_DIR/.venv"

find_python() {
  local candidate
  if [[ -n "${DOUBLELOVE_SPEAKER_PYTHON:-}" ]] \
    && "$DOUBLELOVE_SPEAKER_PYTHON" -c 'import sys; assert sys.version_info >= (3, 11)' 2>/dev/null; then
    echo "$DOUBLELOVE_SPEAKER_PYTHON"; return 0
  fi
  for candidate in python3.13 python3.12 python3.11 python3; do
    if command -v "$candidate" >/dev/null 2>&1 \
      && "$candidate" -c 'import sys; assert sys.version_info >= (3, 11)' 2>/dev/null; then
      command -v "$candidate"; return 0
    fi
  done
  return 1
}

PYTHON="$(find_python)" || {
  echo "找不到 Python ≥ 3.11。请安装后重试：brew install python@3.12" >&2
  exit 1
}
echo "==> 使用解释器 ${PYTHON} ($("$PYTHON" --version 2>&1))"
if [[ -d "$VENV" ]]; then
  echo "==> 重建 Speaker 虚拟环境，移除旧 PyTorch/ONNX 后端"
  rm -rf "$VENV"
fi
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
  echo "==> 未下载模型；这是正常的。模型只能由桌面应用通过 ModelScope 受管安装。"
  exit 0
fi

echo "==> 验证本地 MLX Speaker/VAD 运行时（不下载权重）"
(cd "$SPEAKER_DIR" && "$VENV/bin/python" -c "
import importlib.metadata as metadata
import mlx, mlx_audio, numpy
import double_love_speaker.engine, double_love_speaker.mlx_resnet
assert metadata.version('mlx') == '0.31.1'
assert metadata.version('mlx-audio') == '0.5.0'
for forbidden in ('torch', 'torchaudio', 'wespeaker', 'silero-vad', 'onnxruntime'):
    try:
        metadata.version(forbidden)
    except metadata.PackageNotFoundError:
        continue
    raise SystemExit(f'Speaker runtime must not contain {forbidden}')
print('MLX speaker runtime ready')
")
echo "==> 完成。运行时只接受经模型管理器校验的本地 MLX 模型目录。"
