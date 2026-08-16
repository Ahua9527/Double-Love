"""共享转录管线：读 wav → 切块 → 调引擎 → 词时间戳转源采样域 → 发事件。

引擎（mock / transcriber）只需实现 transcribe_chunk：
    transcribe_chunk(pcm: bytes, *, model: str, language: str, cancel) -> list[dict]
返回词列表 [{"text", "start", "end", ...}]，start/end 为 chunk 相对秒（浮点）。
浮点秒只存在于引擎边界；进入事件流之前一律转成源采样域整数（Rational Time 基准）。
"""

import struct

DEFAULT_CHUNK_SECONDS = 30
PREPARED_RATE = 16_000  # 准备音频规格：16kHz/mono/pcm_s16le


class AsrError(Exception):
    """带协议错误码的失败；fatal=True 表示任务整体失败。"""

    def __init__(self, code: str, message: str, fatal: bool = True):
        super().__init__(message)
        self.code = code
        self.fatal = fatal


def read_wav_pcm(path: str) -> bytes:
    """解析 RIFF 取 data 块；只接受 16kHz/mono/pcm_s16le（导入步骤的保证）。"""
    with open(path, "rb") as handle:
        blob = handle.read()
    if len(blob) < 12 or blob[:4] != b"RIFF" or blob[8:12] != b"WAVE":
        raise AsrError("ASR_BAD_WAV", f"不是有效的 WAV 文件：{path}")
    fmt = None
    data = None
    offset = 12
    while offset + 8 <= len(blob):
        chunk_id = blob[offset : offset + 4]
        size = struct.unpack("<I", blob[offset + 4 : offset + 8])[0]
        body = blob[offset + 8 : offset + 8 + size]
        if chunk_id == b"fmt ":
            fmt = body
        elif chunk_id == b"data":
            data = body
        offset += 8 + size + (size & 1)  # RIFF 块按 2 字节对齐
    if fmt is None or data is None:
        raise AsrError("ASR_BAD_WAV", "WAV 缺少 fmt 或 data 块")
    audio_format, channels, rate = struct.unpack("<HHI", fmt[:8])
    (bits,) = struct.unpack("<H", fmt[14:16])
    if (audio_format, channels, rate, bits) != (1, 1, PREPARED_RATE, 16):
        raise AsrError(
            "ASR_BAD_WAV",
            "准备音频规格不符（需要 16kHz/mono/pcm_s16le，"
            f"实际 format={audio_format} channels={channels} rate={rate} bits={bits}）",
        )
    return data


def _to_source_samples(samples_16k: int, source_rate: int) -> int:
    """16k 域 → 源采样域：先乘后除、四舍五入，全程整数。"""
    return (samples_16k * source_rate + PREPARED_RATE // 2) // PREPARED_RATE


def run(cmd: dict, cancel, emit, transcribe_chunk) -> None:
    task_id = cmd.get("task_id", "")
    model = cmd.get("model", "qwen3-asr-1.7b")
    language = cmd.get("language", "auto")
    source_rate = int(cmd.get("source_sample_rate", 48_000))
    chunk_seconds = int(cmd.get("chunk_seconds", DEFAULT_CHUNK_SECONDS))
    if source_rate <= 0 or chunk_seconds <= 0:
        raise AsrError("ASR_BAD_COMMAND", "source_sample_rate/chunk_seconds 必须为正整数")

    data = read_wav_pcm(cmd.get("wav_path", ""))
    total_samples = len(data) // 2
    chunk_samples = chunk_seconds * PREPARED_RATE
    total_chunks = max(1, (total_samples + chunk_samples - 1) // chunk_samples)

    word_count = 0
    for index in range(total_chunks):
        if cancel.is_set():
            emit({"event": "cancelled", "task_id": task_id})
            return
        base = index * chunk_samples
        pcm = data[base * 2 : (base + chunk_samples) * 2]
        if not pcm:
            continue
        words = transcribe_chunk(pcm, model=model, language=language, cancel=cancel)
        converted = []
        for word in words:
            start_16k = base + round(float(word["start"]) * PREPARED_RATE)
            end_16k = base + round(float(word["end"]) * PREPARED_RATE)
            if end_16k <= start_16k:
                continue  # 引擎偶发的零长/反向词，丢弃而不是传播
            converted.append(
                {
                    "raw_text": word["text"],
                    "display_text": word["text"],
                    "start_sample": _to_source_samples(start_16k, source_rate),
                    "end_sample": _to_source_samples(end_16k, source_rate),
                    "confidence": word.get("confidence"),
                    "language": word.get("language"),
                }
            )
        word_count += len(converted)
        emit({"event": "words", "task_id": task_id, "chunk": index, "words": converted})
        emit(
            {
                "event": "progress",
                "task_id": task_id,
                "completed": index + 1,
                "total": total_chunks,
                "message": f"已转录 {index + 1}/{total_chunks} 段",
            }
        )
    emit({"event": "done", "task_id": task_id, "word_count": word_count})
