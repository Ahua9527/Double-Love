# Double Love model runtime

Release builds place one self-contained, relocatable local Python runtime here:

- `.venv/bin/python` is shared by both sidecars.
- `double_love_asr/` contains the pinned Qwen ASR sidecar.
- `double_love_speaker/` contains the Silero/MLX speaker sidecar.

The app resolves this resource root without PATH, Homebrew, or system-Python fallback. Runtime model
weights remain in the user's local Double Love model directory and are installed explicitly before
offline use.
The release builder installs the complete hashed dependency closure, validates imports, then removes
only the explicit dependency paths for pip/setuptools/wheel, standard-library `ensurepip`, known
dependency tests/test data/examples/docs, and Python bytecode. It preserves non-removed distribution
metadata, LICENSE/METADATA files, model assets, tokenizer configuration, and native dylib/metallib/so
artifacts. The build, prepare, verify, and package-smoke gates repeat the import/version, forbidden
package, legacy-layout, bytecode, and mock-hello checks.
Use `scripts/prepare-model-runtime.sh` and `scripts/verify-release-runtime.sh` on the release
machine. Ordinary development virtualenvs are deliberately rejected because they normally point
back to the build machine's Python. No runtime binary or user model is committed to Git.
