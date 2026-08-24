import json
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "sidecars" / "speaker"))

from double_love_speaker import silero_mlx  # noqa: E402


def _fixed_config():
    return {
        "model_type": "silero_vad",
        "architecture": "silero_vad",
        "version": "v6",
        "source": "silero_vad PyPI 6.2.1 (torch.hub snakers4/silero-vad)",
        "dtype": "float32",
        "threshold": 0.5,
        "min_speech_duration_ms": 250,
        "min_silence_duration_ms": 100,
        "speech_pad_ms": 30,
        "branch_16k": dict(silero_mlx._BRANCH_16K),
    }


class SileroMLXTests(unittest.TestCase):
    def test_fixed_16khz_stream_contract(self):
        self.assertEqual(silero_mlx.SAMPLE_RATE, 16_000)
        self.assertEqual(silero_mlx.CHUNK_SAMPLES, 512)
        self.assertEqual(silero_mlx.CONTEXT_SAMPLES, 64)
        self.assertEqual(silero_mlx.WINDOW_SAMPLES, 576)

    def test_fixed_config_is_loaded_and_8khz_shape_is_not_accepted(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "config.json"
            path.write_text(json.dumps(_fixed_config()), encoding="utf-8")
            config = silero_mlx._read_config(path)
            self.assertEqual(config.threshold, 0.5)
            self.assertEqual(config.min_speech_duration_ms, 250)

            invalid = _fixed_config()
            invalid["branch_16k"]["chunk_size"] = 256
            path.write_text(json.dumps(invalid), encoding="utf-8")
            with self.assertRaises(silero_mlx.SileroModelError):
                silero_mlx._read_config(path)

            for field, value in (("version", "v5"), ("source", "unexpected")):
                invalid = _fixed_config()
                invalid[field] = value
                path.write_text(json.dumps(invalid), encoding="utf-8")
                with self.subTest(field=field):
                    with self.assertRaises(silero_mlx.SileroModelError):
                        silero_mlx._read_config(path)

    def test_weight_mapping_requires_exact_vad_16k_keys(self):
        source = {key: object() for key in silero_mlx.EXPECTED_WEIGHT_KEYS}
        mapped = silero_mlx.map_weight_keys(source)
        self.assertEqual(
            set(mapped),
            {key.removeprefix("vad_16k.") for key in silero_mlx.EXPECTED_WEIGHT_KEYS},
        )

        missing = dict(source)
        missing.pop(next(iter(missing)))
        with self.assertRaises(silero_mlx.SileroModelError):
            silero_mlx.map_weight_keys(missing)

        extra = dict(source)
        extra["vad_8k.conv1.weight"] = object()
        with self.assertRaises(silero_mlx.SileroModelError):
            silero_mlx.map_weight_keys(extra)

    def test_model_directory_is_absolute_and_contains_only_two_model_files(self):
        with tempfile.TemporaryDirectory() as temp:
            model_dir = Path(temp)
            (model_dir / "config.json").write_text(json.dumps(_fixed_config()), encoding="utf-8")
            (model_dir / "model.safetensors").write_bytes(b"fixture")
            self.assertEqual(silero_mlx._local_model_dir(model_dir), model_dir)

            (model_dir / "README.md").write_text("unsafe extra", encoding="utf-8")
            with self.assertRaises(silero_mlx.SileroModelError):
                silero_mlx._local_model_dir(model_dir)

        with self.assertRaises(silero_mlx.SileroModelError):
            silero_mlx._local_model_dir("model-id")


if __name__ == "__main__":
    unittest.main()
