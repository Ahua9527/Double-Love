"""Deterministic speaker sidecar used by Rust integration tests and manual protocol checks."""

from .engine import read_wav_pcm, to_source_samples


def diarize(cmd: dict, cancel, emit) -> None:
    task_id = cmd.get("task_id", "")
    source_rate = int(cmd.get("source_sample_rate", 48_000))
    pcm = read_wav_pcm(cmd.get("wav_path", ""))
    total = len(pcm) // 2
    span = min(16_000 * 2, total)
    segments = []
    if span:
        segments.append(
            {
                "cluster_id": "cluster-1",
                "start_sample": 0,
                "end_sample": to_source_samples(span, source_rate),
                "confidence": 0.99,
            }
        )
    if cancel.is_set():
        emit({"event": "cancelled", "task_id": task_id})
        return
    emit(
        {
            "event": "speaker_segments",
            "task_id": task_id,
            "segments": segments,
            "embeddings": [{"cluster_id": "cluster-1", "values": [1.0, 0.0]}] if span else [],
        }
    )
    emit({"event": "diarization_done", "task_id": task_id, "segment_count": len(segments)})
