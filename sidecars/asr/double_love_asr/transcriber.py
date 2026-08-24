"""真实 MLX 引擎：Qwen3-ASR + 显式 Qwen3-ForcedAligner。

Rust 模型管理器只把已校验的本地绝对目录传到 sidecar。这里禁止传入 repo id，
并在加载前设置 Hugging Face/Transformers 离线开关，避免首次转录时悄悄联网。
"""

import os
import tempfile
import wave
from pathlib import Path

from . import pipeline
from .pipeline import AsrError

LANG_IDS = {"zh": "Chinese", "en": "English"}
_sessions: dict[str, object] = {}
_aligners: dict[str, object] = {}


def _local_model_dir(path: str, *, label: str) -> str:
    """只接受模型管理器传入的本地绝对目录，不允许 repo id 或隐式联网。"""
    if not path:
        raise AsrError("ASR_MODEL_PATH_MISSING", f"缺少本地 {label} 模型目录")
    resolved = Path(path).expanduser()
    if not resolved.is_absolute() or not resolved.is_dir():
        raise AsrError("ASR_MODEL_PATH_INVALID", f"本地 {label} 模型目录不可用")
    if not (resolved / "config.json").is_file() or not any(resolved.glob("*.safetensors")):
        raise AsrError("ASR_MODEL_FILES_MISSING", f"本地 {label} 模型文件不完整")
    return str(resolved)


def _session(model_dir: str):
    """按绝对模型目录缓存 Session（加载一次，跨 chunk/任务复用）。"""
    model_dir = _local_model_dir(model_dir, label="ASR")
    if model_dir not in _sessions:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        try:
            from mlx_qwen3_asr import Session
        except ImportError as error:
            raise AsrError(
                "ASR_ENGINE_UNAVAILABLE",
                "未安装 mlx-qwen3-asr；请先准备 App 运行时",
            ) from error
        try:
            _sessions[model_dir] = Session(model=model_dir)
        except Exception as error:
            raise AsrError("ASR_MODEL_LOAD_FAILED", f"本地模型加载失败：{error}") from error
    return _sessions[model_dir]


def _forced_aligner(aligner_dir: str):
    """按绝对目录缓存 ForcedAligner，禁止包内默认下载。"""
    aligner_dir = _local_model_dir(aligner_dir, label="ForcedAligner")
    if aligner_dir not in _aligners:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        try:
            from mlx_qwen3_asr import ForcedAligner
        except ImportError as error:
            raise AsrError("ASR_ENGINE_UNAVAILABLE", "未安装 mlx-qwen3-asr；请先准备 App 运行时") from error
        try:
            _aligners[aligner_dir] = ForcedAligner(model_path=aligner_dir)
        except Exception as error:
            raise AsrError("ASR_ALIGNER_LOAD_FAILED", f"本地 ForcedAligner 加载失败：{error}") from error
    return _aligners[aligner_dir]


def transcribe_chunk(
    pcm: bytes,
    *,
    model: str,
    model_dir: str,
    aligner_dir: str,
    language: str,
    cancel,
) -> list[dict]:
    del cancel  # MLX 推理不可中断；取消点在 chunk 边界（pipeline 检查）
    if model not in {"qwen3-asr-1.7b-8bit", "qwen3-asr-0.6b-4bit"}:
        raise AsrError("ASR_MODEL_UNKNOWN", f"未知模型：{model}")
    session = _session(model_dir)
    aligner = _forced_aligner(aligner_dir)
    language_name = None if language == "auto" else LANG_IDS.get(language, language)

    # mlx_qwen3_asr 接受文件路径：chunk 写成临时 16kHz wav。
    fd, tmp_path = tempfile.mkstemp(suffix=".wav")
    try:
        with os.fdopen(fd, "wb") as raw:
            with wave.open(raw, "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(2)
                wav.setframerate(pipeline.PREPARED_RATE)
                wav.writeframes(pcm)
        kwargs = {"return_timestamps": True, "forced_aligner": aligner}
        if language_name:
            kwargs["language"] = language_name
        try:
            if not hasattr(session, "transcribe"):
                raise AsrError("ASR_ENGINE_UNAVAILABLE", "当前运行时没有 Session.transcribe")
            # 显式传入本地 ForcedAligner，避免 return_timestamps=True 隐式创建默认对齐器。
            result = session.transcribe(tmp_path, **kwargs)
        except AsrError:
            raise
        except Exception as error:
            raise AsrError("ASR_TRANSCRIBE_FAILED", f"转录失败：{error}", fatal=True) from error
    finally:
        try:
            os.unlink(tmp_path)
        except FileNotFoundError:
            pass

    detected = getattr(result, "language", None)
    words = []
    for segment in result.segments or []:
        text = segment.get("text") if isinstance(segment, dict) else getattr(segment, "text", None)
        start = segment.get("start") if isinstance(segment, dict) else getattr(segment, "start", None)
        end = segment.get("end") if isinstance(segment, dict) else getattr(segment, "end", None)
        confidence = segment.get("confidence") if isinstance(segment, dict) else getattr(segment, "confidence", None)
        if not text or start is None or end is None or float(end) <= float(start):
            continue
        words.append(
            {
                "text": str(text),
                "start": float(start),
                "end": float(end),
                "confidence": confidence,
                "language": detected,
            }
        )
    return words


def run(cmd: dict, cancel, emit) -> None:
    pipeline.run(cmd, cancel, emit, transcribe_chunk)
