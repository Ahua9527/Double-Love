# Double Love model runtime

Release builds place two self-contained, relocatable local Python runtimes here:

- `asr/` contains `double_love_asr/` and `.venv/bin/python` with the pinned Qwen ASR runtime.
- `speaker/` contains `double_love_speaker/` and `.venv/bin/python` with Silero VAD and the pinned WeSpeaker runtime.

The app resolves these resources before looking at a developer's PATH. Runtime model weights remain
in the user's local Double Love model directory and are installed explicitly before offline use.
Use `scripts/prepare-model-runtime.sh` and `scripts/verify-release-runtime.sh` on the release
machine. Ordinary development virtualenvs are deliberately rejected because they normally point
back to the build machine's Python. No runtime binary or user model is committed to Git.
