# Double Love Speaker sidecar notices

- Silero VAD — MIT License — https://github.com/snakers4/silero-vad
- WeSpeaker — Apache License 2.0 — https://github.com/wenet-e2e/wespeaker
  - pinned source commit: `dfa741957e5c11f477623b6e583d67d0af25ee88`
  - Chinese ResNet34 weights: `Wespeaker/wespeaker-cnceleb-resnet34`
  - pinned model revision: `f5a201849aa7cae741ec75cd02a0bc9dd5712ca2`

The sidecar processes prepared local audio only. Speaker embeddings are never printed to stdout,
written to logs, exported, or sent to an agent request.
