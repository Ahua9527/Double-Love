import importlib.util
import json
import re
import sys
import tempfile
import types
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "sidecars" / "speaker"))

from double_love_speaker import engine, mlx_resnet  # noqa: E402


class _FakeVAD:
    def __init__(self, probability: float = 0.9, probabilities=None):
        self.probability = probability
        self.probabilities = iter(probabilities or [])
        self.calls = []

    def initial_state(self, *, sample_rate: int):
        self.calls.append(("initial", sample_rate))
        return object()

    def feed(self, chunk, *, state, sample_rate: int):
        self.calls.append((len(chunk), sample_rate))
        return [[next(self.probabilities, self.probability)]], state


class _FakeSamples:
    def __init__(self, size: int):
        self.size = size

    def __len__(self):
        return self.size

    def __getitem__(self, value):
        if isinstance(value, slice):
            start, stop, step = value.indices(self.size)
            return _FakeSamples(max(0, (stop - start + (step - 1)) // step))
        return 0.0

    def astype(self, _dtype):
        return self

    def __truediv__(self, _value):
        return self


def _fake_numpy():
    module = types.ModuleType("numpy")
    module.int16 = object()
    module.float32 = object()
    module.frombuffer = lambda value, dtype: _FakeSamples(len(value) // 2)
    module.pad = lambda value, widths: _FakeSamples(value.size + widths[1])
    return module


class SpeakerMLXTests(unittest.TestCase):
    def setUp(self):
        engine._VAD_MODELS.clear()
        engine._SPEAKER_MODELS.clear()

    def tearDown(self):
        engine._VAD_MODELS.clear()
        engine._SPEAKER_MODELS.clear()

    def test_vad_uses_local_directory_and_256ms_batches(self):
        fake_vad = _FakeVAD()
        calls = []
        original_load = engine.load
        previous_numpy = sys.modules.get("numpy")
        engine.load = lambda path: calls.append(path) or fake_vad
        sys.modules["numpy"] = _fake_numpy()
        try:
            with tempfile.TemporaryDirectory() as temp:
                model_dir = Path(temp)
                (model_dir / "config.json").write_text(
                    json.dumps(
                        {
                            "threshold": 0.5,
                            "min_speech_duration_ms": 250,
                            "min_silence_duration_ms": 100,
                            "speech_pad_ms": 30,
                        }
                    ),
                    encoding="utf-8",
                )
                (model_dir / "model.safetensors").write_bytes(b"weights")
                pcm = (b"\x00\x00" * (engine.VAD_BLOCK_SAMPLES * 2))
                segments = engine._vad_segments(pcm, str(model_dir))
                self.assertEqual(calls, [model_dir])
                self.assertEqual(fake_vad.calls[0], ("initial", 16_000))
                self.assertEqual(len(fake_vad.calls) - 1, 16)
                self.assertEqual(segments, [(0, engine.VAD_BLOCK_SAMPLES * 2)])
                self.assertEqual(engine.os.environ["HF_HUB_OFFLINE"], "1")
                self.assertEqual(engine.os.environ["TRANSFORMERS_OFFLINE"], "1")
        finally:
            engine.load = original_load
            if previous_numpy is None:
                sys.modules.pop("numpy", None)
            else:
                sys.modules["numpy"] = previous_numpy

    def test_vad_256ms_handles_silence_and_separate_speech_regions(self):
        config = {
            "threshold": 0.5,
            "min_speech_duration_ms": 250,
            "min_silence_duration_ms": 100,
            "speech_pad_ms": 0,
        }
        silence = engine._vad_256ms_segments(
            _FakeVAD(0.01),
            _FakeSamples(engine.VAD_BLOCK_SAMPLES),
            config,
        )
        self.assertEqual(silence, [])
        probabilities = [0.9] * 8 + [0.01] * 8 + [0.9] * 8
        regions = engine._vad_256ms_segments(
            _FakeVAD(probabilities=probabilities),
            _FakeSamples(engine.VAD_BLOCK_SAMPLES * 3),
            config,
        )
        self.assertEqual(
            regions,
            [
                (0, engine.VAD_BLOCK_SAMPLES),
                (engine.VAD_BLOCK_SAMPLES * 2, engine.VAD_BLOCK_SAMPLES * 3),
            ],
        )

    def test_speaker_loader_accepts_only_current_local_mlx_weight_layout(self):
        calls = []
        original = mlx_resnet.load_resnet34_embedding
        mlx_resnet.load_resnet34_embedding = lambda path: calls.append(path) or object()
        try:
            with tempfile.TemporaryDirectory() as temp:
                model_dir = Path(temp)
                (model_dir / "config.json").write_text("{}", encoding="utf-8")
                (model_dir / "weights.npz").write_bytes(b"weights")
                loaded = engine._load_speaker(str(model_dir))
                self.assertIsNotNone(loaded)
                self.assertEqual(calls, [model_dir / "weights.npz"])
                self.assertIs(loaded, engine._load_speaker(str(model_dir)))
                self.assertEqual(calls, [model_dir / "weights.npz"])
        finally:
            mlx_resnet.load_resnet34_embedding = original

    def test_legacy_alias_and_missing_files_are_rejected_before_loading(self):
        with self.assertRaises(engine.SpeakerError) as context:
            engine._load_speaker("chinese")
        self.assertEqual(context.exception.code, "SPEAKER_MODEL_PATH_INVALID")
        with tempfile.TemporaryDirectory() as temp:
            model_dir = Path(temp)
            (model_dir / "config.json").write_text("{}", encoding="utf-8")
            with self.assertRaises(engine.SpeakerError) as context:
                engine._load_speaker(str(model_dir))
            self.assertEqual(context.exception.code, "SPEAKER_MODEL_FILES_MISSING")

    def test_runtime_source_has_no_legacy_inference_imports(self):
        source = (ROOT / "sidecars" / "speaker" / "double_love_speaker" / "engine.py").read_text(
            encoding="utf-8"
        )
        self.assertIn("from .silero_mlx import load", source)
        self.assertNotIn("mlx_audio", source)
        for forbidden in ("import torch", "import torchaudio", "import wespeaker", "silero_vad", "onnxruntime"):
            self.assertNotIn(forbidden, source)

    def test_release_requirements_pin_only_mlx_runtime_dependencies(self):
        requirements = (ROOT / "sidecars" / "speaker" / "requirements.txt").read_text(
            encoding="utf-8"
        )
        self.assertIn("mlx==0.32.1", requirements)
        self.assertIn("numpy==2.5.2", requirements)
        for forbidden in (
            "mlx-audio",
            "transformers",
            "scipy",
            "miniaudio",
            "sounddevice",
            "tokenizers",
            "torch",
            "torchaudio",
            "wespeaker",
            "silero-vad",
            "onnxruntime",
        ):
            self.assertNotIn(forbidden, requirements)

    @unittest.skipUnless(importlib.util.find_spec("numpy"), "runtime NumPy is not installed")
    def test_fbank_is_fixed_16khz_80_bin_and_handles_short_audio(self):
        import numpy as np

        short = mlx_resnet.fbank_80(np.zeros(100, dtype=np.float32))
        self.assertEqual(short.shape, (1, 1, 80))
        one_second = mlx_resnet.fbank_80(np.zeros(16_000, dtype=np.float32))
        self.assertEqual(one_second.shape, (1, 98, 80))
        self.assertTrue(np.isfinite(one_second).all())

    @unittest.skipUnless(importlib.util.find_spec("numpy"), "runtime NumPy is not installed")
    def test_fbank_matches_the_frozen_kaldi_reference(self):
        import numpy as np

        sample = np.arange(16_000, dtype=np.float32)
        waveform = (
            0.6 * np.sin(2 * np.pi * 220 * sample / 16_000)
            + 0.25 * np.sin(2 * np.pi * 730 * sample / 16_000)
            + 0.05 * np.sin(2 * np.pi * 37 * sample / 16_000)
        ).astype(np.float32)
        features = mlx_resnet.fbank_80(waveform)[0]
        expected = [0.716498, 0.152295, -0.396847, 0.820831, 0.891459, 0.223390, -0.008036]
        actual = [
            features[0, 0],
            features[1, 10],
            features[7, 25],
            features[20, 40],
            features[50, 63],
            features[75, 79],
            features[97, 5],
        ]
        np.testing.assert_allclose(actual, expected, rtol=2e-4, atol=2e-4)
        self.assertAlmostEqual(float(np.sum(features * features)), 4044.636230, places=2)

    @unittest.skipUnless(
        importlib.util.find_spec("numpy") and importlib.util.find_spec("mlx"),
        "runtime NumPy/MLX is not installed",
    )
    def test_signed_mlx_resnet34_runs_with_fake_features_without_model_weights(self):
        import numpy as np

        model = mlx_resnet._resnet34_embedding()()
        output = model.extract_embedding(np.zeros((1, 98, 80), dtype=np.float32))
        self.assertEqual(tuple(output.shape), (1, 256))

    @unittest.skipUnless(
        importlib.util.find_spec("numpy") and importlib.util.find_spec("mlx"),
        "runtime NumPy/MLX is not installed",
    )
    def test_weight_key_mapping_loads_only_the_fixed_local_npz_layout(self):
        import numpy as np
        import mlx.nn as nn

        template = mlx_resnet._resnet34_embedding()()
        weights = {}
        for key, value in nn.utils.tree_flatten(template.parameters()):
            source_key = key.replace(".shortcut_conv.", ".shortcut.0.")
            source_key = source_key.replace(".shortcut_bn.", ".shortcut.1.")
            source_key = source_key.replace("fc.", "seg_1.")
            source_key = re.sub(
                r"(layer[1-4])\.layers\.(\d+)\.",
                r"\1.\2.",
                source_key,
            )
            weights[f"resnet.{source_key}"] = np.array(value)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "weights.npz"
            np.savez(path, **weights)
            loaded = mlx_resnet.load_resnet34_embedding(path)
            output = loaded.extract_embedding(np.zeros((1, 98, 80), dtype=np.float32))
            self.assertEqual(tuple(output.shape), (1, 256))


if __name__ == "__main__":
    unittest.main()
