#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import PurePosixPath
import zipfile

FORBIDDEN_PARTS = {
    ".git",
    ".everything",
    ".venv-mvp",
    "__pycache__",
    "build",
    "dist",
    "dist-electron",
    "graphify-out",
    "node_modules",
    "out",
    "release",
    "target",
}
FORBIDDEN_SUFFIXES = {".pyc", ".pyo", ".sqlite3", ".sqlite3-shm", ".sqlite3-wal"}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("archive")
    args = parser.parse_args()

    with zipfile.ZipFile(args.archive) as archive:
        names = archive.namelist()

    violations: list[str] = []
    for name in names:
        path = PurePosixPath(name)
        if FORBIDDEN_PARTS.intersection(path.parts):
            violations.append(name)
        elif any(path.name.endswith(suffix) for suffix in FORBIDDEN_SUFFIXES):
            violations.append(name)

    if violations:
        formatted = "\n".join(f"- {name}" for name in violations[:25])
        raise SystemExit(f"Source archive contains forbidden paths:\n{formatted}")

    required = {"Cargo.toml", "README.md", "LICENSE", "rust-toolchain.toml"}
    missing = sorted(required.difference(names))
    if missing:
        raise SystemExit(f"Source archive is missing required files: {', '.join(missing)}")

    print(f"Validated {len(names)} source files")


if __name__ == "__main__":
    main()
