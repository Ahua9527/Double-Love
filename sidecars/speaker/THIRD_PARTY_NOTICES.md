# Double Love Speaker sidecar notices

- MLX — MIT License — <https://github.com/ml-explore/mlx>
- The minimal in-app 16 kHz Silero VAD implementation is based on the 16 kHz
  implementation and model layout from MLX Audio 0.5.0 — MIT License —
  <https://github.com/Blaizzy/mlx-audio>. The complete `mlx-audio` package is not bundled.
- MLX WeSpeaker ResNet34 weights — MIT License —
  `mlx-community/wespeaker-voxceleb-resnet34-LM` at
  `d34f9e11f648c7e83d077bf6e10da94ba56f7b72` —
  <https://www.modelscope.cn/models/mlx-community/wespeaker-voxceleb-resnet34-LM>
- MLX Silero VAD v6 weights — MIT License — `mlx-community/silero-vad-v6` at
  `c34917caf1d6fc01b763a4ab0345ff1724fdb9c2` —
  <https://www.modelscope.cn/models/mlx-community/silero-vad-v6>
- The in-app `mlx_resnet.py` is reviewed application code. It never imports or executes
  `resnet_embedding.py`, `convert.py`, examples, or any other source file from a model folder.
  The architecture follows the published WeSpeaker ResNet34 design.

Historical Chinese WeSpeaker/CN-Celeb files may remain in an existing application model root
only so their occupied space can be shown and explicitly cleaned. They are not present in the
speaker runtime, cannot be selected for inference, and are not downloaded by this application.

The sidecar processes prepared local audio only. Speaker embeddings are never printed to stdout,
written to logs, exported, or sent to an agent request.
