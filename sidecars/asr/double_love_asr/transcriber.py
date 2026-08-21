"""真实 MLX 引擎：mlx-qwen3-asr（Qwen3-ASR + Qwen3-ForcedAligner 逐词时间戳）。

模型：qwen3-asr-1.7b（默认）/ qwen3-asr-0.6b（PRD 冻结的两档）。
权重经 HF 缓存分发；prepare-asr.sh 会把 HF_HOME 指到 ~/.cache/double-love/models
并预下载，运行时保持离线。
"""

import os
import tempfile
import wave

from . import pipeline
from .pipeline import AsrError

MODEL_IDS = {
    "qwen3-asr-1.7b": "Qwen/Qwen3-ASR-1.7B",
    "qwen3-asr-0.6b": "Qwen/Qwen3-ASR-0.6B",
}
LANG_IDS = {"zh": "Chinese", "en": "English"}

_sessions: dict = {}


def _session(model: str):
    """按模型缓存 Session（加载一次，跨 chunk/任务复用）。"""
    repo = MODEL_IDS.get(model)
    if repo is None:
        supported = ", ".join(sorted(MODEL_IDS))
        raise AsrError("ASR_MODEL_UNKNOWN", f"未知模型：{model}（支持：{supported}）")
    if repo not in _sessions:
        try:
            from mlx_qwen3_asr import Session
        except ImportError as error:
            raise AsrError(
                "ASR_ENGINE_UNAVAILABLE",
                "未安装 mlx-qwen3-asr；请先运行 scripts/prepare-asr.sh",
            ) from error
        try:
            _sessions[repo] = Session(model=repo)
        except Exception as error:
            raise AsrError(
                "ASR_MODEL_LOAD_FAILED",
                f"模型加载失败：{error}（可运行 scripts/prepare-asr.sh 预下载权重）",
            ) from error
    return repo, _sessions[repo]


def transcribe_chunk(pcm: bytes, *, model: str, language: str, cancel) -> list[dict]:
    del cancel  # MLX 推理不可中断；取消点在 chunk 边界（pipeline 检查）
    repo, session = _session(model)
    language_name = None if language == "auto" else LANG_IDS.get(language, language)

    # mlx_qwen3_asr 接受文件路径：chunk 写成临时 16kHz wav
    fd, tmp_path = tempfile.mkstemp(suffix=".wav")
    try:
        with os.fdopen(fd, "wb") as raw:
            with wave.open(raw, "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(pipeline.PREPARED_RATE)
                wav.writeframes(pcm)
        kwargs = {"return_timestamps": True}
        if language_name:
            kwargs["language"] = language_name
        try:
            if hasattr(session, "transcribe"):
                result = session.transcribe(tmp_path, **kwargs)
            else:
                # 兜底：Session 无 transcribe 方法时退回一次性 API（会重复加载模型，慢但正确）
                from mlx_qwen3_asr import transcribe as one_shot

                result = one_shot(tmp_path, model=repo, **kwargs)
        except Exception as error:
            raise AsrError(
                # 当前 sidecar 的一次 transcribe 命令由一个 worker 负责；这里抛出后
                # worker 会结束，因此不能把它伪装成“可继续”的局部错误。
                "ASR_TRANSCRIBE_FAILED", f"转录失败：{error}", fatal=True
            ) from error
    finally:
        os.unlink(tmp_path)

    detected = getattr(result, "language", None)
    words = []
    for segment in result.segments or []:
        words.append(
            {
                "text": segment["text"],
                "start": float(segment["start"]),
                "end": float(segment["end"]),
                "confidence": segment.get("confidence"),
                "language": detected,
            }
        )
    return words


def run(cmd: dict, cancel, emit) -> None:
    pipeline.run(cmd, cancel, emit, transcribe_chunk)
