#!/usr/bin/env bash
set -euo pipefail
ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
ARCHIVE="$ROOT/everything-source.zip"
VERIFY_VENV=$(mktemp -d "${TMPDIR:-/tmp}/everything-verify-python.XXXXXX")
cleanup() { rm -rf "$VERIFY_VENV"; }
trap cleanup EXIT

step() { printf '
[Everything Verify] %s
' "$*"; }

cd "$ROOT"
step "Rust format"
cargo fmt --all -- --check
step "Rust Clippy"
cargo clippy --locked --workspace --all-targets -- -D warnings
step "Rust tests"
cargo test --locked --workspace --all-targets
step "Rust release build"
cargo build --locked --workspace --release

cd "$ROOT/apps/everything-app"
step "Electron dependencies"
npm ci
step "Electron TypeScript"
npm run typecheck
step "Electron production build"
npm run build
step "Electron production dependency audit"
npm audit --omit=dev --audit-level=high

step "Python isolated verification environment"
python3 -m venv "$VERIFY_VENV"
"$VERIFY_VENV/bin/python" -m pip install --upgrade pip
"$VERIFY_VENV/bin/python" -m pip install -e "$ROOT/python/everything_control[dev]" tree-sitter tree-sitter-rust
cd "$ROOT/python/everything_control"
"$VERIFY_VENV/bin/python" -m pytest
"$VERIFY_VENV/bin/python" -m build

cd "$ROOT"
step "Version and static contracts"
"$VERIFY_VENV/bin/python" scripts/check_versions.py
"$VERIFY_VENV/bin/python" scripts/static_rust_check.py
"$VERIFY_VENV/bin/python" scripts/smoke_installers.py
"$VERIFY_VENV/bin/python" scripts/smoke_mvp.py --require-built-ui
step "Deterministic source archive"
"$VERIFY_VENV/bin/python" scripts/package_source.py --output "$ARCHIVE"
"$VERIFY_VENV/bin/python" scripts/validate_source_archive.py "$ARCHIVE"
printf '
[Everything Verify] ALL CHECKS PASSED
Archive: %s
' "$ARCHIVE"
