#!/usr/bin/env python3
"""Fail release automation when package versions drift across project surfaces."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def match(path: str, pattern: str) -> str:
    text = (ROOT / path).read_text(encoding="utf-8")
    found = re.search(pattern, text, re.MULTILINE | re.DOTALL)
    if not found:
        raise SystemExit(f"could not read version from {path}")
    return found.group(1)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--expected")
    args = parser.parse_args()
    versions = {
        "Cargo.toml": match("Cargo.toml", r"\[workspace\.package\].*?^version = \"([^\"]+)\""),
        "python/everything_control/pyproject.toml": match(
            "python/everything_control/pyproject.toml", r"^version = \"([^\"]+)\""
        ),
        "apps/everything-app/package.json": json.loads(
            (ROOT / "apps/everything-app/package.json").read_text(encoding="utf-8")
        )["version"],
        "install.sh": match("install.sh", r'^VERSION="([^"]+)"'),
        "install.ps1": match("install.ps1", r'^\$Version = "([^"]+)"'),
    }
    unique = set(versions.values())
    if len(unique) != 1:
        raise SystemExit("version mismatch: " + ", ".join(f"{path}={version}" for path, version in versions.items()))
    version = unique.pop()
    if args.expected and version != args.expected:
        raise SystemExit(f"tag version {args.expected} does not match project version {version}")
    major_minor = ".".join(version.split(".")[:2])
    research_agent = match(
        "crates/everything-domain/src/config.rs",
        r'EverythingResearch/([0-9]+\.[0-9]+) \(',
    )
    connector_agent = match(
        "crates/everything-connectors/src/http.rs",
        r'Everything/([0-9]+\.[0-9]+) \(',
    )
    if research_agent != major_minor or connector_agent != major_minor:
        raise SystemExit(
            f"runtime user-agent version mismatch: project={major_minor}, "
            f"research={research_agent}, connector={connector_agent}"
        )
    print(f"All project surfaces use version {version}")


if __name__ == "__main__":
    main()
