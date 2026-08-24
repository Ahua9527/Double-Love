"""Pure-MLX local diarization: Silero VAD v6 + WeSpeaker ResNet34.

The host passes only already-verified, absolute model directories.  This module never
accepts a repository id and never imports source code from a model directory, so a model
download cannot turn into executable Python in the desktop process.
"""

from __future__ import annotations

import json
import math
import os
import tempfile
import wave
from pathlib import Path

from .silero_mlx import SileroDependencyError
from .silero_mlx import load


PREPARED_RATE = 16_000
VAD_CHUNK_SAMPLES = 512
VAD_BLOCK_SAMPLES = VAD_CHUNK_SAMPLES * 8
_VAD_MODELS: dict[str, object] = {}
_SPEAKER_MODELS: dict[str, object] = {}


class SpeakerError(Exception):
    def __init__(self, code: str, message: str, fatal: bool = True):
        super().__init__(message)
        self.code = code
        self.fatal = fatal


def read_wav_pcm(path: str) -> bytes:
    try:
        with wave.open(path, "rb") as reader:
            spec = (
                reader.getnchannels(),
                reader.getsampwidth(),
                reader.getframerate(),
                reader.getcomptype(),
            )
            if spec != (1, 2, PREPARED_RATE, "NONE"):
                raise SpeakerError(
                    "SPEAKER_BAD_WAV",
                    "准备音频必须是 16kHz、单声道、16-bit PCM WAV。",
                )
            return reader.readframes(reader.getnframes())
    except SpeakerError:
        raise
    except Exception as error:
        raise SpeakerError("SPEAKER_BAD_WAV", "无法读取准备音频。") from error


def to_source_samples(samples_16k: int, source_rate: int) -> int:
    return (samples_16k * source_rate + PREPARED_RATE // 2) // PREPARED_RATE


def _write_segment(path: Path, pcm: bytes, start: int, end: int) -> None:
    with wave.open(str(path), "wb") as writer:
        writer.setnchannels(1)
        writer.setsampwidth(2)
        writer.setframerate(PREPARED_RATE)
        writer.writeframes(pcm[start * 2 : end * 2])


def _cosine(left: list[float], right: list[float]) -> float:
    numerator = sum(a * b for a, b in zip(left, right))
    left_norm = math.sqrt(sum(a * a for a in left))
    right_norm = math.sqrt(sum(a * a for a in right))
    return numerator / max(left_norm * right_norm, 1e-12)


def _mean(vectors: list[list[float]]) -> list[float]:
    length = len(vectors[0])
    return [sum(vector[index] for vector in vectors) / len(vectors) for index in range(length)]


def _local_model_dir(value: str, *, label: str, files: tuple[str, ...]) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute() or not path.is_dir():
        raise SpeakerError("SPEAKER_MODEL_PATH_INVALID", f"本地 {label} 模型目录不可用。")
    if any(not (path / file).is_file() for file in files):
        raise SpeakerError("SPEAKER_MODEL_FILES_MISSING", f"本地 {label} 模型文件不完整。")
    return path


def _vad_scalar(value) -> float:
    """Convert an evaluated MLX scalar (or a test double) without importing another backend."""
    try:
        value = value[0][0]
    except (IndexError, TypeError):
        pass
    if hasattr(value, "item"):
        value = value.item()
    return float(value)


def _vad_256ms_segments(model, samples, config: dict) -> list[tuple[int, int]]:
    """Run Silero in 8×32ms batches and aggregate each 256ms block with noisy-OR.

    The local Silero model performs the MLX forward pass and carries its LSTM state.
    The grouping here reduces synchronization overhead for offline editor media while keeping
    all time values as 16 kHz integer samples.
    """
    original_length = int(samples.size)
    if original_length == 0:
        return []
    padding = (-original_length) % VAD_BLOCK_SAMPLES
    if padding:
        import numpy as np

        samples = np.pad(samples, (0, padding))
    state = model.initial_state(sample_rate=PREPARED_RATE)
    probabilities: list[float] = []
    for block_start in range(0, int(samples.size), VAD_BLOCK_SAMPLES):
        product = 1.0
        block = samples[block_start : block_start + VAD_BLOCK_SAMPLES]
        for chunk_start in range(0, VAD_BLOCK_SAMPLES, VAD_CHUNK_SAMPLES):
            probability, state = model.feed(
                block[chunk_start : chunk_start + VAD_CHUNK_SAMPLES],
                state=state,
                sample_rate=PREPARED_RATE,
            )
            product *= 1.0 - max(0.0, min(1.0, _vad_scalar(probability)))
        probabilities.append(1.0 - product)

    threshold = float(config.get("threshold", 0.5))
    min_speech = max(1, round(float(config.get("min_speech_duration_ms", 250)) * PREPARED_RATE / 1000))
    min_silence = max(1, round(float(config.get("min_silence_duration_ms", 100)) * PREPARED_RATE / 1000))
    pad = max(0, round(float(config.get("speech_pad_ms", 30)) * PREPARED_RATE / 1000))
    raw: list[tuple[int, int]] = []
    start: int | None = None
    last_speech_end: int | None = None
    silence = 0
    for index, probability in enumerate(probabilities):
        block_start = index * VAD_BLOCK_SAMPLES
        block_end = min(original_length, block_start + VAD_BLOCK_SAMPLES)
        if block_start >= original_length:
            break
        if probability >= threshold:
            if start is None:
                start = block_start
            last_speech_end = block_end
            silence = 0
            continue
        if start is None:
            continue
        silence += block_end - block_start
        if silence >= min_silence:
            end = last_speech_end or block_start
            if end - start >= min_speech:
                raw.append((start, end))
            start = None
            last_speech_end = None
            silence = 0
    if start is not None:
        end = last_speech_end or original_length
        if end - start >= min_speech:
            raw.append((start, end))

    padded: list[tuple[int, int]] = []
    for start, end in raw:
        start = max(0, start - pad)
        end = min(original_length, end + pad)
        if padded and start <= padded[-1][1]:
            padded[-1] = (padded[-1][0], max(padded[-1][1], end))
        else:
            padded.append((start, end))
    return padded


def _vad_segments(pcm: bytes, vad_model_dir: str) -> list[tuple[int, int]]:
    model_dir = _local_model_dir(
        vad_model_dir,
        label="Silero VAD",
        files=("config.json", "model.safetensors"),
    )
    key = str(model_dir)
    try:
        import numpy as np
    except Exception as error:
        raise SpeakerError(
            "SPEAKER_DEPENDENCY_MISSING",
            "MLX Silero VAD 依赖不可用；请重新安装本机模型运行时。",
        ) from error
    if key not in _VAD_MODELS:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        try:
            _VAD_MODELS[key] = load(model_dir)
        except SileroDependencyError as error:
            raise SpeakerError(
                "SPEAKER_DEPENDENCY_MISSING",
                "MLX Silero VAD 依赖不可用；请重新安装本机模型运行时。",
            ) from error
        except Exception as error:
            raise SpeakerError("SPEAKER_MODEL_MISSING", "本地 Silero VAD 模型不可用。") from error
    try:
        samples = np.frombuffer(pcm, dtype=np.int16).astype(np.float32) / 32768.0
        config = json.loads((model_dir / "config.json").read_text(encoding="utf-8"))
        return _vad_256ms_segments(_VAD_MODELS[key], samples, config)
    except SpeakerError:
        raise
    except Exception as error:
        raise SpeakerError("SPEAKER_VAD_FAILED", "MLX Silero VAD 无法分析准备音频。") from error


def _load_speaker(speaker_model_dir: str):
    model_dir = _local_model_dir(
        speaker_model_dir,
        label="说话人识别",
        files=("config.json", "weights.npz"),
    )
    key = str(model_dir)
    if key not in _SPEAKER_MODELS:
        try:
            from .mlx_resnet import load_resnet34_embedding

            _SPEAKER_MODELS[key] = load_resnet34_embedding(model_dir / "weights.npz")
        except SpeakerError:
            raise
        except Exception as error:
            raise SpeakerError("SPEAKER_MODEL_MISSING", "本地 MLX 说话人模型不可用。") from error
    return _SPEAKER_MODELS[key]


def _embedding(model, wav: Path) -> list[float]:
    try:
        from .mlx_resnet import fbank_80, read_pcm_wav

        features = fbank_80(read_pcm_wav(wav))
        value = model.extract_embedding(features)
        if hasattr(value, "tolist"):
            value = value.tolist()
        if value and isinstance(value[0], list):
            value = value[0]
        return [float(item) for item in value]
    except Exception as error:
        raise SpeakerError("SPEAKER_EMBEDDING_FAILED", "无法提取本地 MLX 声纹嵌入。") from error


def diarize(cmd: dict, cancel, emit) -> None:
    task_id = cmd.get("task_id", "")
    source_rate = int(cmd.get("source_sample_rate", 48_000))
    vad_model_dir = cmd.get("vad_model_dir", "")
    speaker_model_dir = cmd.get("speaker_model_dir", "")
    if source_rate <= 0:
        raise SpeakerError("SPEAKER_BAD_COMMAND", "source_sample_rate 必须为正整数。")
    pcm = read_wav_pcm(cmd.get("wav_path", ""))
    emit({"event": "progress", "task_id": task_id, "completed": 0, "total": None, "message": "正在检测语音区间…"})
    speech = _vad_segments(pcm, vad_model_dir)
    if cancel.is_set():
        emit({"event": "cancelled", "task_id": task_id})
        return
    if not speech:
        emit({"event": "speaker_segments", "task_id": task_id, "segments": [], "embeddings": []})
        emit({"event": "diarization_done", "task_id": task_id, "segment_count": 0})
        return
    model = _load_speaker(speaker_model_dir)
    clusters: list[dict] = []
    rows: list[tuple[int, int, str]] = []
    threshold = float(os.environ.get("DOUBLELOVE_SPEAKER_CLUSTER_THRESHOLD", "0.82"))
    with tempfile.TemporaryDirectory(prefix="double-love-speaker-") as temp_dir:
        for index, (start, end) in enumerate(speech):
            if cancel.is_set():
                emit({"event": "cancelled", "task_id": task_id})
                return
            if end - start < PREPARED_RATE // 2:
                continue
            wav = Path(temp_dir) / f"segment-{index}.wav"
            _write_segment(wav, pcm, start, end)
            vector = _embedding(model, wav)
            best_index = -1
            best_score = -1.0
            for cluster_index, cluster in enumerate(clusters):
                score = _cosine(vector, cluster["centroid"])
                if score > best_score:
                    best_index, best_score = cluster_index, score
            if best_index >= 0 and best_score >= threshold:
                cluster = clusters[best_index]
                cluster["vectors"].append(vector)
                cluster["centroid"] = _mean(cluster["vectors"])
            else:
                cluster = {"label": f"cluster-{len(clusters) + 1}", "vectors": [vector], "centroid": vector}
                clusters.append(cluster)
            rows.append((start, end, cluster["label"]))
            emit(
                {
                    "event": "progress",
                    "task_id": task_id,
                    "completed": index + 1,
                    "total": len(speech),
                    "message": f"已分析 {index + 1}/{len(speech)} 个语音区间",
                }
            )
    segments = [
        {
            "cluster_id": label,
            "start_sample": to_source_samples(start, source_rate),
            "end_sample": to_source_samples(end, source_rate),
            "confidence": 0.80,
        }
        for start, end, label in rows
    ]
    embeddings = [
        {"cluster_id": cluster["label"], "values": cluster["centroid"]}
        for cluster in clusters
    ]
    emit({"event": "speaker_segments", "task_id": task_id, "segments": segments, "embeddings": embeddings})
    emit({"event": "diarization_done", "task_id": task_id, "segment_count": len(segments)})
