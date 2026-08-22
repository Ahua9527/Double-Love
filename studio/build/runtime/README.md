# Double Love media runtime

Release builds place `ffmpeg` and `ffprobe` in this directory before Electron packaging.
Both binaries must be universal or Apple Silicon compatible, hardened-runtime compatible, and the
`ffmpeg -filters` output must contain the `ass` filter (libass). They are intentionally not stored
in Git; use `scripts/prepare-media-runtime.sh` on the signed release machine.
