#!/usr/bin/env bash
# Purpose: capture a repeatable Phase-1 migration baseline (environment, quality gates, exit codes, test/skip summaries, bundle sizes) into an ignored evidence directory.
# How to run: scripts/migration/baseline.sh [--fast] [--out DIR]   (--fast skips Electron packaging/smoke gates; default output: evidence/migration-baseline/<utc-timestamp>/)
# Requirements: macOS arm64 dev machine with node, pnpm, cargo, rustc, python3, ffmpeg, ffprobe on PATH; run from anywhere inside the repo; no credentials are read or written.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
FAST=0
OUT_BASE="$ROOT/evidence/migration-baseline"
while [ $# -gt 0 ]; do
  case "$1" in
    --fast) FAST=1 ;;
    --out) shift; OUT_BASE="${1:?--out requires a directory}" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift
done

# Resolve caller-relative --out before gates change directory to ROOT.
mkdir -p "$OUT_BASE"
OUT_BASE="$(cd "$OUT_BASE" && pwd)"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="$OUT_BASE/$STAMP"
mkdir -p "$OUT"
SUMMARY="$OUT/summary.tsv"
LOG="$OUT/run.log"
export PYTHONPYCACHEPREFIX="$OUT/python-pycache"
mkdir -p "$PYTHONPYCACHEPREFIX"
: >"$SUMMARY"
: >"$LOG"

echo "baseline output: ${OUT#$ROOT/}"

record() { # name <tab> exit_code_or_SKIP <tab> note
  printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$SUMMARY"
}

redact_log() {
  local line
  while IFS= read -r line || [ -n "$line" ]; do
    line="${line//"$ROOT"/<ROOT>}"
    if [ -n "${HOME:-}" ]; then
      line="${line//"$HOME"/<HOME>}"
    fi
    printf '%s\n' "$line"
  done
}

run_gate() { # name command...
  local name="$1"; shift
  echo "== gate: $name" | tee -a "$LOG"
  { printf '\$'; printf ' %q' "$@"; printf '\n'; } | redact_log >>"$LOG"
  ( cd "$ROOT" && "$@" ) 2>&1 | redact_log >>"$LOG"
  local code=${PIPESTATUS[0]}
  record "$name" "$code" "$*"
  if [ "$code" -ne 0 ]; then
    echo "   FAILED ($code)" | tee -a "$LOG"
  fi
  return "$code"
}

check_ffmpeg_ass_filter() {
  ffmpeg -hide_banner -filters 2>/dev/null | awk '{print $2}' | grep -qx ass
}

# ---- environment (tool names + versions only; no absolute paths, no credentials) ----
{
  echo "utc=$STAMP"
  echo "os=$(sw_vers -productVersion 2>/dev/null || echo unknown) arch=$(uname -m)"
  echo "node=$(node --version 2>/dev/null || echo missing)"
  echo "pnpm=$(pnpm --version 2>/dev/null || echo missing)"
  echo "rustc=$(rustc --version 2>/dev/null | awk '{print $2}' || echo missing)"
  echo "python3=$(python3 --version 2>/dev/null | awk '{print $2}' || echo missing)"
  echo "ffmpeg=$(ffmpeg -version 2>/dev/null | head -1 | awk '{print $3}' || echo missing)"
  echo "ffprobe=$(ffprobe -version 2>/dev/null | head -1 | awk '{print $3}' || echo missing)"
  echo "git_head=$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo "git_dirty=$(git -C "$ROOT" status --porcelain 2>/dev/null | wc -l | tr -d ' ')"
  echo "fast_mode=$FAST"
} >"$OUT/environment.txt"

FAILED=0

# ---- required tool presence (fail, never skip, in this strict gate) ----
for tool in node pnpm cargo rustc python3 ffmpeg ffprobe; do
  if command -v "$tool" >/dev/null 2>&1; then
    record "tool:$tool" 0 "present"
  else
    record "tool:$tool" 1 "MISSING"
    FAILED=1
  fi
done

# ---- gates ----
run_gate "ffmpeg:ass-filter" check_ffmpeg_ass_filter || FAILED=1
run_gate "web:lint" pnpm lint || FAILED=1
run_gate "web:test" pnpm test || FAILED=1
run_gate "web:build" pnpm build || FAILED=1
run_gate "studio:lint" pnpm --dir studio lint || FAILED=1
run_gate "studio:test" pnpm --dir studio test || FAILED=1
run_gate "studio:build" pnpm --dir studio build || FAILED=1
run_gate "rust:fmt" cargo fmt --all -- --check || FAILED=1
run_gate "rust:clippy" cargo clippy --workspace --all-targets --locked -- -D warnings || FAILED=1
run_gate "rust:test" cargo test --workspace --locked -- --nocapture || FAILED=1
run_gate "py:asr" python3 -m unittest discover -s sidecars/asr/tests -p 'test_*.py' || FAILED=1
run_gate "py:speaker" python3 -m unittest discover -s sidecars/speaker/tests -p 'test_*.py' || FAILED=1
# bash expands these globs only after run_gate has changed to ROOT.
run_gate "py:compile" bash -c "python3 -m py_compile sidecars/asr/double_love_asr/*.py sidecars/speaker/double_love_speaker/*.py" || FAILED=1
if [ "$FAST" -eq 0 ]; then
  run_gate "electron:release-host" cargo build --release -p double-love-desktop-host --locked || FAILED=1
  run_gate "electron:build" pnpm --dir studio electron:build || FAILED=1
  run_gate "electron:pack-dir" env CSC_IDENTITY_AUTO_DISCOVERY=false pnpm --dir studio pack:dir || FAILED=1
  run_gate "electron:package-smoke" scripts/migration/package-smoke.sh || FAILED=1
else
  record "electron:release-host" "SKIP" "--fast"
  record "electron:build" "SKIP" "--fast"
  record "electron:pack-dir" "SKIP" "--fast"
  record "electron:package-smoke" "SKIP" "--fast"
fi
run_gate "git:diff-check" git diff --check || FAILED=1

# ---- test / skip summaries (parsed from the captured log; best-effort) ----
{
  echo "# vitest (web + studio)"
  grep -E '^\s+(Test Files|Tests)\s' "$LOG" | sed 's/^[[:space:]]*//' || true
  echo "# cargo test"
  grep -E 'test result:' "$LOG" | sort | uniq -c || true
  echo "# cargo self-skips (tests that return early)"
  grep -E '(^|[[:space:]])skip:' "$LOG" | sort | uniq -c || true
  echo "# python unittest"
  grep -E '^(Ran [0-9]+ tests|OK|FAILED)' "$LOG" || true
} >"$OUT/test-summary.txt"

# ---- bundle sizes (repo-relative paths only) ----
{
  for target in "dist" "studio/out" "studio/release/mac-arm64/Double Love Studio.app"; do
    if [ -e "$ROOT/$target" ]; then
      printf '%s\t%s\n' "$target" "$(du -sm "$ROOT/$target" 2>/dev/null | awk '{print $1" MiB"}')"
    else
      printf '%s\t%s\n' "$target" "absent"
    fi
  done
} >"$OUT/bundle-sizes.txt"

# ---- machine-readable verdict ----
PASS_COUNT="$(awk -F '\t' '$2=="0"' "$SUMMARY" | wc -l | tr -d ' ')"
FAIL_COUNT="$(awk -F '\t' '$2!="0" && $2!="SKIP"' "$SUMMARY" | wc -l | tr -d ' ')"
SKIP_COUNT="$(awk -F '\t' '$2=="SKIP"' "$SUMMARY" | wc -l | tr -d ' ')"
if [ "$FAILED" -ne 0 ]; then
  VERDICT="FAIL"
  COMPLETENESS="$([ "$SKIP_COUNT" -eq 0 ] && echo COMPLETE || echo INCOMPLETE)"
elif [ "$SKIP_COUNT" -ne 0 ]; then
  VERDICT="PASS_FAST"
  COMPLETENESS="INCOMPLETE"
else
  VERDICT="PASS"
  COMPLETENESS="COMPLETE"
fi
{
  echo "gates_passed=$PASS_COUNT"
  echo "gates_failed=$FAIL_COUNT"
  echo "gates_skipped=$SKIP_COUNT"
  echo "verdict=$VERDICT"
  echo "completeness=$COMPLETENESS"
} >"$OUT/verdict.txt"

echo "gates passed: $PASS_COUNT, failed: $FAIL_COUNT, skipped: $SKIP_COUNT"
if [ "$FAILED" -ne 0 ]; then
  echo "BASELINE FAIL — see ${OUT#$ROOT/}/summary.tsv" >&2
  exit 1
fi
echo "BASELINE $VERDICT ($COMPLETENESS) — evidence in ${OUT#$ROOT/}"
