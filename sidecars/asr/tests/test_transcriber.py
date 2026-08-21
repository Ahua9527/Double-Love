import sys
import tempfile
import types
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "sidecars" / "asr"))

from double_love_asr import transcriber  # noqa: E402


class _Result:
    language = "Chinese"
    segments = [
        {"text": "你好", "start": 0.10, "end": 0.35, "confidence": 0.91},
        # 零长区间必须被丢弃，不能变成可剪辑锚点。
        {"text": "坏锚点", "start": 0.4, "end": 0.4, "confidence": 0.1},
    ]


class _FakeSession:
    calls = []

    def __init__(self, model):
        self.model = model

    def transcribe(self, path, **kwargs):
        self.calls.append((self.model, path, kwargs))
        return _Result()


class _FakeAligner:
    calls = []

    def __init__(self, model_path):
        self.model_path = model_path
        self.calls.append(model_path)


class TranscriberOfflineTests(unittest.TestCase):
    def test_explicit_local_session_and_forced_aligner(self):
        fake_module = types.SimpleNamespace(Session=_FakeSession, ForcedAligner=_FakeAligner)
        previous = sys.modules.get("mlx_qwen3_asr")
        sys.modules["mlx_qwen3_asr"] = fake_module
        transcriber._sessions.clear()
        transcriber._aligners.clear()
        with tempfile.TemporaryDirectory() as temp:
            asr = Path(temp) / "asr"
            aligner = Path(temp) / "aligner"
            for model_dir in (asr, aligner):
                model_dir.mkdir()
                (model_dir / "config.json").write_text("{}", encoding="utf-8")
                (model_dir / "model.safetensors").write_bytes(b"fixture")
            pcm = b"\x00\x00" * 16_000
            words = transcriber.transcribe_chunk(
                pcm,
                model="qwen3-asr-0.6b",
                model_dir=str(asr),
                aligner_dir=str(aligner),
                language="zh",
                cancel=None,
            )
            self.assertEqual([word["text"] for word in words], ["你好"])
            self.assertEqual(_FakeSession.calls[0][0], str(asr))
            self.assertIs(_FakeSession.calls[0][2]["forced_aligner"], transcriber._aligners[str(aligner)])
            self.assertEqual(_FakeAligner.calls, [str(aligner)])
            self.assertEqual(transcriber.os.environ["HF_HUB_OFFLINE"], "1")
            self.assertEqual(transcriber.os.environ["TRANSFORMERS_OFFLINE"], "1")
        if previous is None:
            sys.modules.pop("mlx_qwen3_asr", None)
        else:
            sys.modules["mlx_qwen3_asr"] = previous

    def test_repo_id_is_rejected_before_importing_runtime(self):
        with self.assertRaises(transcriber.AsrError) as context:
            transcriber._local_model_dir("Qwen/Qwen3-ASR-0.6B", label="ASR")
        self.assertEqual(context.exception.code, "ASR_MODEL_PATH_INVALID")


if __name__ == "__main__":
    unittest.main()
