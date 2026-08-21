"""Silero VAD + WeSpeaker local diarization implementation.

Only anonymous segment labels and cluster centroids leave this module. The Rust host stores the
centroids in the local project database and never writes them to logs or export payloads.
"""

import math
import os
import tempfile
import wave
from pathlib import Path


PREPARED_RATE = 16_000


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
    right_norm = math.sqrt(sum(b * b for b in right))
    return numerator / max(left_norm * right_norm, 1e-12)


def _mean(vectors: list[list[float]]) -> list[float]:
    length = len(vectors[0])
    return [sum(vector[index] for vector in vectors) / len(vectors) for index in range(length)]


def _vad_segments(pcm: bytes, vad_model_dir: str) -> list[tuple[int, int]]:
    try:
        import numpy as np
        import torch
        from silero_vad import get_speech_timestamps, load_silero_vad
    except Exception as error:
        raise SpeakerError(
            "SPEAKER_DEPENDENCY_MISSING",
            "Silero VAD 依赖不可用；请重新运行 prepare-speaker.sh。",
        ) from error
    # 当前 Silero 权重随签名 App 的 Python runtime 分发。要求 Rust 仍传入一个
    # 本地目录/`bundled` 标识，防止未来误把 repo id 交给此处而触发联网下载。
    if vad_model_dir and vad_model_dir != "bundled":
        vad_path = Path(vad_model_dir).expanduser()
        if not vad_path.is_absolute() or not vad_path.is_dir():
            raise SpeakerError("SPEAKER_MODEL_PATH_INVALID", "Silero VAD 必须使用本地 bundled runtime。")
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    audio = torch.from_numpy(np.frombuffer(pcm, dtype=np.int16).copy()).float() / 32768.0
    model = load_silero_vad(onnx=True)
    timestamps = get_speech_timestamps(audio, model, sampling_rate=PREPARED_RATE)
    return [(int(item["start"]), int(item["end"])) for item in timestamps if item["end"] > item["start"]]


def _load_wespeaker(speaker_model_dir: str):
    model_dir = Path(speaker_model_dir).expanduser()
    if not model_dir.is_absolute() or not model_dir.is_dir():
        raise SpeakerError("SPEAKER_MODEL_PATH_INVALID", "本地 WeSpeaker 模型目录不可用。")
    if not (model_dir / "config.yaml").is_file() or not (model_dir / "avg_model.pt").is_file():
        raise SpeakerError("SPEAKER_MODEL_FILES_MISSING", "本地 WeSpeaker 模型文件不完整。")
    try:
        import wespeaker
    except Exception as error:
        raise SpeakerError(
            "SPEAKER_DEPENDENCY_MISSING",
            "WeSpeaker 依赖不可用；请重新运行 prepare-speaker.sh。",
        ) from error
    try:
        os.environ["HF_HUB_OFFLINE"] = "1"
        os.environ["TRANSFORMERS_OFFLINE"] = "1"
        # 只传绝对目录；不再传 "chinese"，避免 API 自动下载或访问 WESPEAKER_HOME。
        return wespeaker.load_model(str(model_dir))
    except Exception as error:
        raise SpeakerError(
            "SPEAKER_MODEL_MISSING",
            "本地 WeSpeaker 模型不可用；请先在设置中完成下载。",
        ) from error


def _embedding(model, wav: Path) -> list[float]:
    try:
        value = model.extract_embedding(str(wav))
        if hasattr(value, "detach"):
            value = value.detach().cpu().numpy()
        if hasattr(value, "tolist"):
            value = value.tolist()
        if value and isinstance(value[0], list):
            value = value[0]
        return [float(item) for item in value]
    except Exception as error:
        raise SpeakerError("SPEAKER_EMBEDDING_FAILED", "无法提取本地声纹嵌入。") from error


def diarize(cmd: dict, cancel, emit) -> None:
    task_id = cmd.get("task_id", "")
    source_rate = int(cmd.get("source_sample_rate", 48_000))
    vad_model_dir = cmd.get("vad_model_dir", "bundled")
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
    model = _load_wespeaker(speaker_model_dir)
    clusters: list[dict] = []
    rows: list[tuple[int, int, str]] = []
    threshold = float(os.environ.get("DOUBLELOVE_SPEAKER_CLUSTER_THRESHOLD", "0.82"))
    with tempfile.TemporaryDirectory(prefix="double-love-speaker-") as temp_dir:
        for index, (start, end) in enumerate(speech):
            if cancel.is_set():
                emit({"event": "cancelled", "task_id": task_id})
                return
            # WeSpeaker needs enough speech to build a stable embedding; short VAD bursts are merged
            # by attaching them to the nearest discovered cluster only after an embedding is available.
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
