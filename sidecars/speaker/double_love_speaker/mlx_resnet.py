"""Signed-in-app MLX ResNet34 implementation for the fixed WeSpeaker weights.

This is intentionally not loaded from the model directory.  It accepts only the checked
`weights.npz` file and fixed 16 kHz / 80-bin log-mel features.
"""

from __future__ import annotations

import re
import wave
from pathlib import Path


SAMPLE_RATE = 16_000
MEL_BINS = 80


def read_pcm_wav(path: Path):
    import numpy as np

    with wave.open(str(path), "rb") as reader:
        if (
            reader.getnchannels(),
            reader.getsampwidth(),
            reader.getframerate(),
            reader.getcomptype(),
        ) != (1, 2, SAMPLE_RATE, "NONE"):
            raise ValueError("expected 16 kHz mono PCM WAV")
        return np.frombuffer(reader.readframes(reader.getnframes()), dtype=np.int16).astype(np.float32) / 32768.0


def _mel_filterbank():
    import numpy as np

    fft_size = 512
    # Match torchaudio.compliance.kaldi.get_mel_banks exactly: Kaldi excludes the
    # Nyquist FFT bin and builds each triangle in mel space instead of rounding
    # the band edges to integer FFT bins.
    mel_low = 1127.0 * np.log1p(20.0 / 700.0)
    mel_high = 1127.0 * np.log1p((SAMPLE_RATE / 2) / 700.0)
    mel_delta = (mel_high - mel_low) / (MEL_BINS + 1)
    mel_frequencies = 1127.0 * np.log1p(
        (np.arange(fft_size // 2, dtype=np.float32) * SAMPLE_RATE / fft_size) / 700.0
    )
    filters = np.zeros((MEL_BINS, fft_size // 2 + 1), dtype=np.float32)
    for index in range(MEL_BINS):
        left = mel_low + index * mel_delta
        center = left + mel_delta
        right = center + mel_delta
        up = (mel_frequencies - left) / (center - left)
        down = (right - mel_frequencies) / (right - center)
        filters[index, :-1] = np.maximum(0.0, np.minimum(up, down))
    return filters


_MEL_FILTERBANK = None


def fbank_80(samples):
    """Fixed 25 ms / 10 ms 80-bin log-mel features with utterance CMN."""
    import numpy as np

    global _MEL_FILTERBANK
    if _MEL_FILTERBANK is None:
        _MEL_FILTERBANK = _mel_filterbank()
    frame_length = 400
    frame_shift = 160
    if samples.size < frame_length:
        samples = np.pad(samples, (0, frame_length - samples.size))
    frame_count = 1 + (samples.size - frame_length) // frame_shift
    end = (frame_count - 1) * frame_shift + frame_length
    if end > samples.size:
        samples = np.pad(samples, (0, end - samples.size))
    frames = np.stack(
        [samples[index * frame_shift : index * frame_shift + frame_length] for index in range(frame_count)]
    )
    # Kaldi's default fbank frontend removes each frame's DC component and
    # applies a 0.97 pre-emphasis before the configured Hamming window.
    frames -= np.mean(frames, axis=1, keepdims=True)
    previous = np.concatenate([frames[:, :1], frames[:, :-1]], axis=1)
    frames = frames - 0.97 * previous
    frames *= np.hamming(frame_length).astype(np.float32)
    spectrum = np.abs(np.fft.rfft(frames, n=512)) ** 2
    features = np.log(
        np.maximum(spectrum @ _MEL_FILTERBANK.T, np.finfo(np.float32).eps)
    ).astype(np.float32)
    features -= np.mean(features, axis=0, keepdims=True)
    return features[None, :, :]


def _mlx():
    try:
        import mlx.core as mx
        import mlx.nn as nn
    except Exception as error:
        raise RuntimeError("MLX 说话人运行时不可用") from error
    return mx, nn


def _basic_block(nn):
    class BasicBlock(nn.Module):
        def __init__(self, in_channels: int, out_channels: int, stride: int = 1):
            super().__init__()
            self.conv1 = nn.Conv2d(
                in_channels,
                out_channels,
                kernel_size=3,
                stride=stride,
                padding=1,
                bias=False,
            )
            self.bn1 = nn.BatchNorm(out_channels)
            self.conv2 = nn.Conv2d(
                out_channels,
                out_channels,
                kernel_size=3,
                padding=1,
                bias=False,
            )
            self.bn2 = nn.BatchNorm(out_channels)
            self.has_shortcut = stride != 1 or in_channels != out_channels
            if self.has_shortcut:
                self.shortcut_conv = nn.Conv2d(
                    in_channels,
                    out_channels,
                    kernel_size=1,
                    stride=stride,
                    bias=False,
                )
                self.shortcut_bn = nn.BatchNorm(out_channels)

        def __call__(self, value):
            identity = value
            value = nn.relu(self.bn1(self.conv1(value)))
            value = self.bn2(self.conv2(value))
            if self.has_shortcut:
                identity = self.shortcut_bn(self.shortcut_conv(identity))
            return nn.relu(value + identity)

    return BasicBlock


def _resnet34_embedding():
    mx, nn = _mlx()
    BasicBlock = _basic_block(nn)

    class TemporalStatisticsPooling(nn.Module):
        def __call__(self, value):
            mean = mx.mean(value, axis=2)
            std = mx.sqrt(mx.var(value, axis=2) + 1e-7)
            return mx.concatenate([mean, std], axis=-1).reshape(value.shape[0], -1)

    class ResNet34Embedding(nn.Module):
        def __init__(self):
            super().__init__()
            self.conv1 = nn.Conv2d(1, 32, kernel_size=3, padding=1, bias=False)
            self.bn1 = nn.BatchNorm(32)
            self.layer1 = self._layer(BasicBlock, 32, 32, 3, 1)
            self.layer2 = self._layer(BasicBlock, 32, 64, 4, 2)
            self.layer3 = self._layer(BasicBlock, 64, 128, 6, 2)
            self.layer4 = self._layer(BasicBlock, 128, 256, 3, 2)
            self.pool = TemporalStatisticsPooling()
            self.fc = nn.Linear(256 * 2 * 10, 256)

        @staticmethod
        def _layer(block, in_channels, out_channels, count, stride):
            return nn.Sequential(
                block(in_channels, out_channels, stride),
                *[block(out_channels, out_channels) for _ in range(count - 1)],
            )

        def __call__(self, features):
            if features.ndim == 3:
                features = mx.expand_dims(features, axis=-1)
            value = mx.transpose(features, (0, 2, 1, 3))
            value = nn.relu(self.bn1(self.conv1(value)))
            value = self.layer1(value)
            value = self.layer2(value)
            value = self.layer3(value)
            value = self.layer4(value)
            return self.fc(self.pool(value))

        def extract_embedding(self, features):
            value = self(mx.array(features, dtype=mx.float32))
            mx.eval(value)
            return value

    return ResNet34Embedding


def load_resnet34_embedding(weights_path: Path):
    mx, _ = _mlx()
    if not weights_path.is_file():
        raise FileNotFoundError("speaker weights are missing")
    model = _resnet34_embedding()()
    source = mx.load(str(weights_path))
    mapped = []
    for key, value in source.items():
        if not key.startswith("resnet."):
            continue
        key = key.removeprefix("resnet.")
        key = key.replace(".shortcut.0.", ".shortcut_conv.")
        key = key.replace(".shortcut.1.", ".shortcut_bn.")
        key = key.replace("seg_1.", "fc.")
        key = re.sub(r"(layer[1-4])\.(\d+)\.", r"\1.layers.\2.", key)
        mapped.append((key, value))
    if len(mapped) < 100:
        raise ValueError("speaker weights do not contain the expected ResNet tensors")
    model.load_weights(mapped, strict=True)
    model.eval()
    return model
