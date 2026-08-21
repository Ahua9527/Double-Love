"""确定性 mock 引擎：不依赖任何第三方包，供测试与开发自举。

每 0.5 秒一个词（词长 0.3 秒），词汇表固定循环——同样的输入永远得到
同样的输出，golden 断言可以直接写死数值。
"""

import time

from . import pipeline

VOCAB = ["开拍", "镜一", "第一次", "开始", "好", "停", "保一条", "再来", "过", "收工"]

_WORD_STEP = 0.5
_WORD_LEN = 0.3


def transcribe_chunk(
    pcm: bytes,
    *,
    model: str,
    model_dir: str = "",
    aligner_dir: str = "",
    language: str,
    cancel,
) -> list[dict]:
    del model, model_dir, aligner_dir, language  # mock 不关心
    seconds = len(pcm) / 2 / pipeline.PREPARED_RATE
    words = []
    position = 0.0
    index = 0
    while position + _WORD_LEN + 0.05 <= seconds:
        words.append(
            {
                "text": VOCAB[index % len(VOCAB)],
                "start": position,
                "end": position + _WORD_LEN,
                "confidence": 0.99,
                "language": "zh",
            }
        )
        position += _WORD_STEP
        index += 1
    time.sleep(0.02)  # 让取消窗口在测试中可观测
    return words


def run(cmd: dict, cancel, emit) -> None:
    pipeline.run(cmd, cancel, emit, transcribe_chunk)
