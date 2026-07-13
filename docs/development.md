# Development Guide

## Toolchains

Rust is selected by `rust-toolchain.toml`; do not commit absolute `rustc`, `rustdoc`, target, or linker paths. Node.js 22+ and Python 3.11+ are the supported development baselines.

## Verification commands

Run `scripts/verify.sh` on Unix-like systems or `scripts/verify.ps1` on Windows. The scripts execute:

1. Rust formatting, Clippy, and workspace tests.
2. Electron clean install, typecheck, production build, and high-severity audit.
3. Python editable install with development extras, pytest, and package build.
4. Source archive creation plus validation that generated/machine-local paths are absent.

## Running without a model

Set the runtime model backend to loopback in `everything.toml` while developing deterministic runtime and control-plane behavior. Ollama health or generation failures must be visible rather than silently represented as primary-model success.

## Runtime state cleanup

Delete `<workspace>/.everything/` only when intentionally resetting local journals, metrics, caches, and graph databases. Never commit that directory.

## Packaging

`scripts/package_source.py` is the canonical source-packaging implementation. Shell and PowerShell wrappers call it so Windows and Unix exclusions cannot drift.
