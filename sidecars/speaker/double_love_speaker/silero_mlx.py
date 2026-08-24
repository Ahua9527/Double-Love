"""Minimal, local-only 16 kHz Silero VAD implementation for Double Love.

The model shape and weight names follow the 16 kHz branch of MLX Audio 0.5.0's
Silero implementation.  Only the small streaming surface used by ``engine`` is
implemented here: a fixed 512-sample chunk, a 64-sample context, and the model
forward pass.  Audio I/O, resampling, model registries, and network access are
intentionally outside this module.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping


SAMPLE_RATE = 16_000
CHUNK_SAMPLES = 512
CONTEXT_SAMPLES = 64
WINDOW_SAMPLES = CONTEXT_SAMPLES + CHUNK_SAMPLES

_MODEL_FILES = {"config.json", "model.safetensors"}
_CONFIG_VERSION = "v6"
_CONFIG_SOURCE = "silero_vad PyPI 6.2.1 (torch.hub snakers4/silero-vad)"
_CONFIG_KEYS = {
    "model_type",
    "architecture",
    "version",
    "source",
    "dtype",
    "threshold",
    "min_speech_duration_ms",
    "min_silence_duration_ms",
    "speech_pad_ms",
    "branch_16k",
}
_BRANCH_KEYS = {
    "sample_rate",
    "filter_length",
    "hop_length",
    "pad",
    "cutoff",
    "context_size",
    "chunk_size",
}
_BRANCH_16K = {
    "sample_rate": SAMPLE_RATE,
    "filter_length": 256,
    "hop_length": 128,
    "pad": CONTEXT_SAMPLES,
    "cutoff": 129,
    "context_size": CONTEXT_SAMPLES,
    "chunk_size": CHUNK_SAMPLES,
}

EXPECTED_WEIGHT_KEYS = frozenset(
    {
        "vad_16k.conv1.bias",
        "vad_16k.conv1.weight",
        "vad_16k.conv2.bias",
        "vad_16k.conv2.weight",
        "vad_16k.conv3.bias",
        "vad_16k.conv3.weight",
        "vad_16k.conv4.bias",
        "vad_16k.conv4.weight",
        "vad_16k.final_conv.bias",
        "vad_16k.final_conv.weight",
        "vad_16k.lstm.Wh",
        "vad_16k.lstm.Wx",
        "vad_16k.lstm.bias",
        "vad_16k.stft_conv.weight",
    }
)


class SileroModelError(ValueError):
    """Raised when a local Silero config or checkpoint is not the fixed layout."""


class SileroDependencyError(SileroModelError):
    """Raised when the local MLX dependency is unavailable."""


@dataclass(frozen=True)
class Config:
    threshold: float
    min_speech_duration_ms: int
    min_silence_duration_ms: int
    speech_pad_ms: int


@dataclass(frozen=True)
class State:
    state: object
    context: object
    sample_rate: int


def _read_config(path: Path) -> Config:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except Exception as error:
        raise SileroModelError("Silero config is not valid JSON") from error
    if not isinstance(value, dict) or set(value) != _CONFIG_KEYS:
        raise SileroModelError("Silero config contains an unsupported structure")
    if value["model_type"] != "silero_vad" or value["architecture"] != "silero_vad":
        raise SileroModelError("Silero config is not the expected model")
    if value["version"] != _CONFIG_VERSION:
        raise SileroModelError("Silero config version is not v6")
    if value["source"] != _CONFIG_SOURCE:
        raise SileroModelError("Silero config source is not the expected fixed source")
    if value["dtype"] != "float32":
        raise SileroModelError("Silero config must use float32")
    branch = value["branch_16k"]
    if not isinstance(branch, dict) or set(branch) != _BRANCH_KEYS or branch != _BRANCH_16K:
        raise SileroModelError("Silero 16 kHz branch config is not fixed")
    if isinstance(value["threshold"], bool) or not isinstance(value["threshold"], (int, float)):
        raise SileroModelError("Silero threshold is invalid")
    for key in ("min_speech_duration_ms", "min_silence_duration_ms", "speech_pad_ms"):
        if isinstance(value[key], bool) or not isinstance(value[key], int) or value[key] < 0:
            raise SileroModelError(f"Silero {key} is invalid")
    threshold = float(value["threshold"])
    if not 0.0 <= threshold <= 1.0:
        raise SileroModelError("Silero threshold is out of range")
    return Config(
        threshold=threshold,
        min_speech_duration_ms=value["min_speech_duration_ms"],
        min_silence_duration_ms=value["min_silence_duration_ms"],
        speech_pad_ms=value["speech_pad_ms"],
    )


def map_weight_keys(source: Mapping[str, object]) -> dict[str, object]:
    """Map only the verified ``vad_16k.*`` checkpoint into local module names."""
    keys = set(source)
    if any(not isinstance(key, str) for key in keys):
        raise SileroModelError("Silero checkpoint contains a non-string key")
    if keys != EXPECTED_WEIGHT_KEYS:
        raise SileroModelError("Silero checkpoint keys do not match the 16 kHz layout")
    return {key.removeprefix("vad_16k."): source[key] for key in sorted(keys)}


def _local_model_dir(value: str | Path) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute() or not path.is_dir() or path.is_symlink():
        raise SileroModelError("Silero model directory must be a local absolute directory")
    entries = list(path.iterdir())
    if any(entry.name not in _MODEL_FILES or entry.is_symlink() for entry in entries):
        raise SileroModelError("Silero model directory contains unsupported files")
    for name in _MODEL_FILES:
        file = path / name
        if not file.is_file() or file.is_symlink():
            raise SileroModelError("Silero model directory is incomplete")
    return path


def _reflect_pad_right(mx, value, pad: int):
    if value.shape[-1] <= pad:
        raise ValueError("Silero reflect padding requires more than 64 samples")
    indices = mx.arange(value.shape[-1] - 2, value.shape[-1] - pad - 2, -1)
    return mx.concatenate([value, mx.take(value, indices, axis=-1)], axis=-1)


def _model_class(mx, nn, config: Config):
    class SileroVAD(nn.Module):
        def __init__(self):
            super().__init__()
            self.config = config
            self.stft_conv = nn.Conv1d(
                1,
                258,
                kernel_size=256,
                stride=128,
                padding=0,
                bias=False,
            )
            self.conv1 = nn.Conv1d(129, 128, kernel_size=3, padding=1)
            self.conv2 = nn.Conv1d(128, 64, kernel_size=3, stride=2, padding=1)
            self.conv3 = nn.Conv1d(64, 64, kernel_size=3, stride=2, padding=1)
            self.conv4 = nn.Conv1d(64, 128, kernel_size=3, padding=1)
            self.lstm = nn.LSTM(128, 128)
            self.final_conv = nn.Conv1d(128, 1, kernel_size=1)

        def __call__(self, value, state=None):
            value = mx.array(value, dtype=mx.float32)
            if value.ndim == 1:
                value = value[None, :]
            if value.ndim != 2 or value.shape[-1] != WINDOW_SAMPLES:
                raise ValueError(f"Silero expects a {WINDOW_SAMPLES}-sample window")
            hidden = cell = None
            if state is not None:
                if state.ndim != 3 or state.shape[0] != 2 or state.shape[-1] != 128:
                    raise ValueError("Silero state must have shape (2, batch, 128)")
                hidden, cell = state[0], state[1]
            value = _reflect_pad_right(mx, value, CONTEXT_SAMPLES)
            value = self.stft_conv(value[..., None])
            real = value[..., :129]
            imag = value[..., 129:]
            value = mx.sqrt(real * real + imag * imag)
            value = nn.relu(self.conv1(value))
            value = nn.relu(self.conv2(value))
            value = nn.relu(self.conv3(value))
            value = nn.relu(self.conv4(value))
            hidden_seq, cell_seq = self.lstm(value, hidden=hidden, cell=cell)
            new_state = mx.stack(
                [hidden_seq[:, -1, :], cell_seq[:, -1, :]],
                axis=0,
            )
            value = nn.relu(hidden_seq)
            value = nn.sigmoid(self.final_conv(value))
            probability = mx.mean(mx.squeeze(value, axis=-1), axis=1, keepdims=True)
            return probability, new_state

        def initial_state(self, batch_size: int = 1, sample_rate: int = SAMPLE_RATE) -> State:
            if sample_rate != SAMPLE_RATE:
                raise ValueError("Double Love Silero only supports 16000 Hz")
            return State(
                state=None,
                context=mx.zeros((batch_size, CONTEXT_SAMPLES), dtype=mx.float32),
                sample_rate=SAMPLE_RATE,
            )

        def feed(self, chunk, state: State | None = None, sample_rate: int = SAMPLE_RATE):
            if sample_rate != SAMPLE_RATE:
                raise ValueError("Double Love Silero only supports 16000 Hz")
            chunk = mx.array(chunk, dtype=mx.float32)
            if chunk.ndim == 1:
                chunk = chunk[None, :]
            if chunk.ndim != 2 or chunk.shape[-1] != CHUNK_SAMPLES:
                raise ValueError(f"Silero expects exactly {CHUNK_SAMPLES} samples")
            if state is None:
                state = self.initial_state(batch_size=chunk.shape[0], sample_rate=sample_rate)
            if state.sample_rate != sample_rate:
                raise ValueError("Silero state sample rate does not match the chunk")
            probability, lstm_state = self(
                mx.concatenate([state.context, chunk], axis=-1),
                state=state.state,
            )
            return probability, State(
                state=lstm_state,
                context=chunk[:, -CONTEXT_SAMPLES:],
                sample_rate=SAMPLE_RATE,
            )

    return SileroVAD


def _load_mlx():
    try:
        import mlx.core as mx
        import mlx.nn as nn
    except Exception as error:
        raise SileroDependencyError("MLX is unavailable for the local Silero runtime") from error
    return mx, nn


def load(model_dir: str | Path):
    """Load the exact local 16 kHz checkpoint; never resolves or downloads a model id."""
    path = _local_model_dir(model_dir)
    config = _read_config(path / "config.json")
    mx, nn = _load_mlx()
    try:
        source = mx.load(str(path / "model.safetensors"))
        mapped = map_weight_keys(source)
        model = _model_class(mx, nn, config)()
        model.load_weights(list(mapped.items()), strict=True)
        model.eval()
        return model
    except SileroModelError:
        raise
    except Exception as error:
        raise SileroModelError("Silero checkpoint could not be loaded strictly") from error
