import sys
import tempfile
import types
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "sidecars" / "speaker"))

from double_love_speaker import engine  # noqa: E402


class _FakeSpeakerModel:
    def extract_embedding(self, path):
        return [1.0, 0.0]


class SpeakerOfflineTests(unittest.TestCase):
    def test_loads_from_absolute_directory_instead_of_language_alias(self):
        calls = []
        fake_module = types.SimpleNamespace(load_model=lambda value: calls.append(value) or _FakeSpeakerModel())
        previous = sys.modules.get("wespeaker")
        sys.modules["wespeaker"] = fake_module
        with tempfile.TemporaryDirectory() as temp:
            model_dir = Path(temp)
            (model_dir / "config.yaml").write_text("model: resnet34\n", encoding="utf-8")
            (model_dir / "avg_model.pt").write_bytes(b"fixture")
            engine._load_wespeaker(str(model_dir))
            self.assertEqual(calls, [str(model_dir)])
            self.assertEqual(engine.os.environ["HF_HUB_OFFLINE"], "1")
            self.assertEqual(engine.os.environ["TRANSFORMERS_OFFLINE"], "1")
        if previous is None:
            sys.modules.pop("wespeaker", None)
        else:
            sys.modules["wespeaker"] = previous

    def test_missing_local_model_is_a_fatal_path_error(self):
        with self.assertRaises(engine.SpeakerError) as context:
            engine._load_wespeaker("chinese")
        self.assertEqual(context.exception.code, "SPEAKER_MODEL_PATH_INVALID")


if __name__ == "__main__":
    unittest.main()
