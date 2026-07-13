#!/usr/bin/env python3
"""Create a deterministic source archive without generated or machine-local files."""

from __future__ import annotations

import argparse
from pathlib import Path, PurePosixPath
import zipfile

EXCLUDED_DIRS = {
    ".git",
    ".everything",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".venv",
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
    "witness-output",
}
EXCLUDED_NAMES = {
    "cargo-build-raw.log",
    "cargo-test.log",
    "everything-source.zip",
}
EXCLUDED_SUFFIXES = {".pyc", ".pyo", ".sqlite3", ".sqlite3-shm", ".sqlite3-wal"}


def should_include(relative_path: Path, output_name: str) -> bool:
    parts = relative_path.parts
    if any(part in EXCLUDED_DIRS for part in parts[:-1]):
        return False
    if relative_path.name in EXCLUDED_NAMES or relative_path.name == output_name:
        return False
    if any(relative_path.name.endswith(suffix) for suffix in EXCLUDED_SUFFIXES):
        return False
    return True


def create_archive(repo_root: Path, output_path: Path) -> int:
    repo_root = repo_root.resolve()
    output_path = output_path.resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)

    files = sorted(
        path
        for path in repo_root.rglob("*")
        if path.is_file()
        and path.resolve() != output_path
        and should_include(path.relative_to(repo_root), output_path.name)
    )

    with zipfile.ZipFile(
        output_path,
        mode="w",
        compression=zipfile.ZIP_DEFLATED,
        compresslevel=9,
    ) as archive:
        for path in files:
            relative = PurePosixPath(path.relative_to(repo_root).as_posix())
            info = zipfile.ZipInfo(str(relative), date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (path.stat().st_mode & 0xFFFF) << 16
            archive.writestr(info, path.read_bytes())

    return len(files)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="Archive path (default: <repo>/everything-source.zip)",
    )
    args = parser.parse_args()

    repo_root = Path(__file__).resolve().parent.parent
    output = args.output or repo_root / "everything-source.zip"
    count = create_archive(repo_root, output)
    size_mb = output.stat().st_size / (1024 * 1024)
    print(f"Created source archive: {output}")
    print(f"Included files: {count}")
    print(f"Archive size: {size_mb:.2f} MB")


if __name__ == "__main__":
    main()
