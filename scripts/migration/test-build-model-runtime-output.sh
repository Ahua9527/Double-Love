#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BUILD_SCRIPT="$ROOT/scripts/migration/build-model-runtime.sh"
PREPARE_SCRIPT="$ROOT/scripts/prepare-model-runtime.sh"
VERIFY_SCRIPT="$ROOT/scripts/verify-release-runtime.sh"
PACKAGE_SMOKE_SCRIPT="$ROOT/scripts/migration/package-smoke.sh"
fixture_root=""

cleanup_fixture() {
  if [[ -n "$fixture_root" && -d "$fixture_root" ]]; then
    rm -rf "$fixture_root"
  fi
}
trap cleanup_fixture EXIT

fail() {
  echo "build-model-runtime regression failed: $1" >&2
  exit 1
}

assert_contains() {
  local haystack="$1" needle="$2" description="$3"
  [[ "$haystack" == *"$needle"* ]] || fail "$description"
}

assert_file_contains() {
  local script="$1" needle="$2" description="$3"
  rg -Fq "$needle" "$script" || fail "$description"
}

assert_python_env() {
  local script="$1" unguarded
  unguarded="$(awk '
    /"([^" ]*\/python|\$python|\$PYTHON)" -/ {
      if (($0 !~ /PYTHONDONTWRITEBYTECODE=1/ && previous !~ /PYTHONDONTWRITEBYTECODE=1/) ||
          ($0 !~ /PYTHONNOUSERSITE=1/ && previous !~ /PYTHONNOUSERSITE=1/)) {
        print FNR ":" $0
      }
    }
    { previous = $0 }
  ' "$script")"
  [[ -z "$unguarded" ]] || fail "Python invocation lacks both no-bytecode/no-user-site envs in $script: $unguarded"
}

assert_mock_env() {
  local script="$1" mock_lines
  mock_lines="$(rg -n 'DOUBLELOVE_(ASR|SPEAKER)_MOCK=1' "$script" || true)"
  [[ -n "$mock_lines" ]] || fail "mock hello invocation is missing in $script"
  while IFS= read -r line; do
    [[ "$line" == *'PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1'* ]] \
      || fail "mock hello lacks both no-bytecode/no-user-site envs in $script: $line"
  done <<< "$mock_lines"
}

assert_mock_cwd() {
  local script="$1" verify_source
  verify_source="$(sed -n '/^verify_model_runtime() {/,/^}/p' "$script")"
  assert_contains "$verify_source" $'  (\n    cd "$root"\n    asr_hello=' \
    "mock hello must run inside the shared runtime root in $script"
  assert_contains "$verify_source" '    speaker_hello=' \
    "speaker mock hello must remain in the shared-root block in $script"
}

assert_auxiliary_gate() {
  local script="$1" gate_source
  gate_source="$(sed -n '/^assert_no_runtime_auxiliary_paths() {/,/^}/p' "$script")"
  assert_contains "$gate_source" 'for path in "$root/.gitignore" "$root/.lock" "$root/.temp"; do' \
    "runtime auxiliary-file gate is missing from $script"
  assert_contains "$gate_source" 'for path in "$root"/cpython-*-macos-aarch64-none; do' \
    "top-level CPython symlink gate is missing from $script"
  assert_contains "$gate_source" '[[ -L "$path" ]]' \
    "top-level CPython check must be symlink-only in $script"
  assert_contains "$gate_source" 'for path in "$root/.venv/bin/pip" "$root/.venv/bin/pip3" "$root/.venv/bin/pip3.12"; do' \
    "pip launcher gate must name all three launchers in $script"
  assert_contains "$gate_source" '[[ -e "$path" || -L "$path" ]]' \
    "pip launcher gate must catch files and symlinks in $script"
  assert_file_contains "$script" 'assert_no_runtime_auxiliary_paths "$root"' \
    "runtime auxiliary-file gate is not called by $script"
}

run_prepare_fixture() {
  local target source
  fixture_root="$(mktemp -d "${TMPDIR:-/tmp}/double-love-prepare-regression.XXXXXX")"
  target="$fixture_root/studio/build/model-runtime"
  source="$fixture_root/build/source"
  mkdir -p "$fixture_root/scripts" "$target" \
    "$source/.venv/bin" "$source/.venv/lib/python3.12/site-packages" \
    "$source/double_love_asr" "$source/double_love_speaker"
  cp "$PREPARE_SCRIPT" "$fixture_root/scripts/prepare-model-runtime.sh"
  touch "$source/double_love_asr/__init__.py" "$source/double_love_speaker/__init__.py"
  cat >"$source/.venv/bin/python" <<'PY'
#!/usr/bin/env bash
set -euo pipefail
runtime_root="$(cd "$(dirname "$0")/../.." && pwd)"
if [[ "${DOUBLELOVE_ASR_MOCK:-}" == "1" || "${DOUBLELOVE_SPEAKER_MOCK:-}" == "1" ]]; then
  printf '%s\n' '{"event": "ready", "mock": true}'
elif [[ "$*" == *'sysconfig.get_paths()["purelib"]'* ]]; then
  printf '%s\n' "$runtime_root/.venv/lib/python3.12/site-packages"
fi
PY
  chmod +x "$source/.venv/bin/python"
  cp "$ROOT/studio/build/model-runtime/README.md" "$target/README.md"
  mkdir -p "$target/.venv" "$target/double_love_asr" "$target/double_love_speaker" \
    "$target/asr" "$target/speaker"

  DOUBLELOVE_MODEL_RUNTIME_SOURCE="$source" \
    bash "$fixture_root/scripts/prepare-model-runtime.sh"

  [[ -f "$target/README.md" ]] || fail "prepare fixture deleted the tracked README"
  cmp -s "$ROOT/studio/build/model-runtime/README.md" "$target/README.md" \
    || fail "prepare fixture changed the tracked README"
  [[ ! -e "$target/asr" && ! -e "$target/speaker" ]] \
    || fail "prepare fixture left the legacy asr/speaker layout"
  for runtime_path in .venv double_love_asr double_love_speaker; do
    [[ -d "$target/$runtime_path" ]] \
      || fail "prepare fixture did not install $runtime_path"
  done
}

function_source="$(sed -n '/^resolve_output_dir() {/,/^}/p' "$BUILD_SCRIPT")"
[[ -n "$function_source" ]] || fail "resolve_output_dir function is missing"
eval "$function_source"

default_output="$(resolve_output_dir "$ROOT/build/model-runtime-sources")"
[[ "$default_output" == "$ROOT/build/model-runtime-sources" ]] \
  || fail "default output path changed: $default_output"

relative_output="$(cd "$ROOT" && resolve_output_dir 'build/model-runtime-sources-shared')"
[[ "$relative_output" == "$ROOT/build/model-runtime-sources-shared" ]] \
  || fail "relative output path was not rooted at the worktree: $relative_output"
[[ "$relative_output" == /* ]] || fail "relative output path is not absolute"

normalize_line="$(rg -n '^OUT=\"\$\(resolve_output_dir' "$BUILD_SCRIPT" | head -1 | cut -d: -f1)"
first_output_cd_line="$(rg -n 'cd \"\$OUT\"' "$BUILD_SCRIPT" | head -1 | cut -d: -f1)"
[[ -n "$normalize_line" && -n "$first_output_cd_line" ]] \
  || fail "could not locate output normalization and build cd"
(( normalize_line < first_output_cd_line )) \
  || fail "output normalization happens after a build cd"

verify_source="$(sed -n '/^verify_model_runtime() {/,/^}/p' "$BUILD_SCRIPT")"
prune_source="$(sed -n '/^prune_model_runtime() {/,/^}/p' "$BUILD_SCRIPT")"
assert_contains "$verify_source" 'local root="$1" python="$1/.venv/bin/python"' \
  "verify_model_runtime must use the shared .venv interpreter"
assert_contains "$prune_source" 'site_packages="$(PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$root/.venv/bin/python" -c' \
  "prune_model_runtime must resolve site-packages from the shared .venv"
assert_contains "$prune_source" 'stdlib="$(PYTHONDONTWRITEBYTECODE=1 PYTHONNOUSERSITE=1 "$root/.venv/bin/python" -c' \
  "prune_model_runtime must resolve stdlib from the shared .venv"
assert_contains "$prune_source" 'find "$root" -type d -name '\''__pycache__'\''' \
  "prune_model_runtime bytecode directory scan must cover the shared root"
assert_contains "$prune_source" 'find "$root" -type f' \
  "prune_model_runtime bytecode file scan must cover the shared root"
assert_contains "$prune_source" "-name '*.pyc'" \
  "prune_model_runtime bytecode file scan must cover pyc files"
[[ "$prune_source" != *'site_packages="$("$root/bin/python"'* ]] \
  || fail "prune_model_runtime still uses the virtualenv-root interpreter contract"
assert_file_contains "$BUILD_SCRIPT" 'assert_no_forbidden_backend_files "$OUT/.venv"' \
  "forbidden-backend helper must receive the virtualenv root"
assert_file_contains "$BUILD_SCRIPT" 'prune_model_runtime "$OUT"' \
  "prune_model_runtime must receive the shared runtime root"
assert_file_contains "$BUILD_SCRIPT" 'verify_model_runtime "$OUT"' \
  "verify_model_runtime must receive the shared runtime root"
assert_contains "$prune_source" '"$root/.gitignore"' \
  "prune_model_runtime must remove top-level uv auxiliary files"
assert_contains "$prune_source" '"$root/.lock"' \
  "prune_model_runtime must remove the top-level lock file"
assert_contains "$prune_source" '"$root/.temp"' \
  "prune_model_runtime must remove the top-level temp path"
assert_contains "$prune_source" '"$root/.venv/bin/pip3.12"' \
  "prune_model_runtime must remove the versioned pip launcher explicitly"
assert_contains "$prune_source" 'for path in "$root"/cpython-*-macos-aarch64-none; do' \
  "prune_model_runtime must inspect only matching top-level CPython symlinks"
assert_contains "$prune_source" 'rm -f "$path"' \
  "prune_model_runtime must remove matching CPython symlinks without following them"

for script in "$BUILD_SCRIPT" "$PREPARE_SCRIPT" "$VERIFY_SCRIPT" "$PACKAGE_SMOKE_SCRIPT"; do
  assert_python_env "$script"
  assert_mock_env "$script"
  assert_mock_cwd "$script"
  assert_auxiliary_gate "$script"
done

run_prepare_fixture

echo "build-model-runtime regression passed"
