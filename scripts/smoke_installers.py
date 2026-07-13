#!/usr/bin/env python3
"""Static and side-effect-free installer contract checks for all supported platforms."""
from __future__ import annotations

import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def run(*command: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=ROOT,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=30,
    )


def check_bash() -> None:
    scripts = [
        "bootstrap.sh",
        "setup.sh",
        "install.sh",
        "package-source.sh",
        "scripts/verify.sh",
        "scripts/research_sidecar.sh",
    ]
    for relative in scripts:
        path = ROOT / relative
        require(path.is_file(), f"missing shell script: {relative}")
        result = run("bash", "-n", str(path))
        require(result.returncode == 0, f"bash syntax failed for {relative}: {result.stdout}")
        if os.name != "nt":
            require(os.access(path, os.X_OK), f"shell script is not executable: {relative}")

    for relative in ("setup.sh", "install.sh"):
        result = run(str(ROOT / relative), "--help")
        require(result.returncode == 0, f"{relative} --help failed: {result.stdout}")
        require("Usage:" in result.stdout, f"{relative} help is incomplete")


def check_powershell_structure() -> None:
    scripts = ["bootstrap.ps1", "setup.ps1", "install.ps1", "package-source.ps1", "scripts/verify.ps1", "scripts/research_sidecar.ps1"]
    for relative in scripts:
        source = read(relative)
        require(source.count("@'\n") == source.count("\n'@"), f"unbalanced literal here-string in {relative}")
        require(source.count('@"\n') == source.count('\n"@'), f"unbalanced expandable here-string in {relative}")
        require(source.count("{") == source.count("}"), f"unbalanced braces in {relative}")
        require("Invoke-Expression" not in source, f"unsafe Invoke-Expression in {relative}")


def check_transaction_contract() -> None:
    unix = read("install.sh")
    windows = read("install.ps1")
    setup_unix = read("setup.sh")
    setup_windows = read("setup.ps1")

    shared_unix = [
        "INSTALL_BACKUP", "INSTALL_SWITCHED", "INSTALL_COMPLETE", "cleanup_all",
        "cargo clippy --locked", "cargo test --locked", "smoke_ollama.py",
        "/v1/info", "systemctl --user", "LaunchAgents", "node_modules/electron/install.js",
        "npm audit --omit=dev --audit-level=high", "EVERYTHING_SERVICE_PORT",
        "service-ports.env", "install-manifest.json", "runtime-doctor.json",
        "runtime_doctor_status", "schema_version: 3", "LOCK_OWNED",
        "find_free_loopback_port", "prepare_service_ports", "run_runtime_doctor",
        "const required = [", "tool-sandbox", "data-directory", "research", "run_as_root",
        "research_sidecar.sh", "local_research_sidecar", 'RUST_TOOLCHAIN="1.97.0"',
        "cargo test --locked --workspace --all-targets", "Node.js 22+ is required",
        "npm 10+ is required", "-m build",
    ]
    shared_windows = [
        "InstallBackup", "InstallSwitched", "InstallComplete", "Register-ScheduledTask",
        "cargo clippy --locked", "cargo test --locked", "smoke_ollama.py",
        "/v1/info", "node_modules/electron/install.js", "npm audit --omit=dev --audit-level=high",
        "EVERYTHING_SERVICE_PORT", "service-ports.env", "install-manifest.json",
        "runtime-doctor.json", "runtime_doctor_status", "schema_version = 3",
        "Save-ServicePorts", "Prepare-ServicePorts", "Get-FreeLoopbackPort",
        "Invoke-RuntimeDoctor", "$required = @('model'", "tool-sandbox", "data-directory", "research",
        "research_sidecar.ps1", "local_research_sidecar",
        "Microsoft.VisualStudio.Workload.VCTools", '$RustToolchain = "1.97.0"',
        "Find-PythonExecutable", "Test-OllamaReady", "npm install --global npm@10",
    ]
    for token in shared_unix:
        require(token in unix, f"Unix installer contract missing: {token}")
    for token in shared_windows:
        require(token in windows, f"Windows installer contract missing: {token}")

    require("EVERYTHING_MODEL:-auto" in setup_unix, "Unix setup does not honor EVERYTHING_MODEL")
    require("$env:EVERYTHING_MODEL" in setup_windows, "Windows setup does not honor EVERYTHING_MODEL")
    require("Start-Transcript" in setup_windows, "Windows setup must write a persistent installation log")
    require("15" in setup_windows and "Get-FreeDiskGiB" in setup_windows, "Windows setup disk preflight is missing")
    for source, label in ((setup_unix, "Unix"), (setup_windows, "Windows")):
        for model in ("qwen2.5-coder:3b", "qwen2.5-coder:7b", "qwen2.5-coder:14b"):
            require(model in source, f"{label} smart model selection missing {model}")

    sidecar_unix = read("scripts/research_sidecar.sh")
    sidecar_windows = read("scripts/research_sidecar.ps1")
    compose = read("deploy/searxng/compose.yml")
    for token in ("127.0.0.1", "COMPOSE_PROJECT_NAME", "EVERYTHING_SEARXNG_SETTINGS", "native anahtarsız"):
        require(token in sidecar_unix, f"Unix research sidecar contract missing: {token}")
    for token in ("127.0.0.1", "COMPOSE_PROJECT_NAME", "EVERYTHING_SEARXNG_SETTINGS", "RandomNumberGenerator"):
        require(token in sidecar_windows, f"Windows research sidecar contract missing: {token}")
    for token in ("read_only: true", "cap_drop:", "no-new-privileges:true", "127.0.0.1"):
        require(token in compose, f"SearXNG compose hardening missing: {token}")

    verify_unix = read("scripts/verify.sh")
    verify_windows = read("scripts/verify.ps1")
    require("python3 -m venv" in verify_unix, "Unix verification must isolate Python packaging")
    require("everything-verify-python" in verify_unix, "Unix verification temp environment is missing")
    require("-m venv $VerifyVenv" in verify_windows, "Windows verification must isolate Python packaging")
    require("everything-verify-python" in verify_windows, "Windows verification temp environment is missing")

    bootstrap_unix = read("bootstrap.sh")
    bootstrap_windows = read("bootstrap.ps1")
    require("sha256" in bootstrap_unix.lower(), "Unix bootstrap does not verify SHA-256")
    require("Get-FileHash" in bootstrap_windows, "Windows bootstrap does not verify SHA-256")
    require("https://" in bootstrap_unix and "https://" in bootstrap_windows, "bootstrap transport must use HTTPS")


def check_no_shell_download_execution() -> None:
    for relative in ("bootstrap.sh", "setup.sh", "install.sh"):
        source = read(relative)
        require(re.search(r"curl[^\n]*\|\s*(?:ba)?sh", source) is None, f"download-to-shell pipeline in {relative}")
    for relative in ("bootstrap.ps1", "setup.ps1", "install.ps1"):
        source = read(relative)
        require("iex (" not in source.lower(), f"download-to-eval pattern in {relative}")


def main() -> int:
    if shutil.which("bash"):
        check_bash()
    elif os.name != "nt":
        raise AssertionError("bash is required for Unix installer syntax checks")
    else:
        print("bash bulunamadı; Windows üzerinde Unix script çalışma testi atlandı")
    check_powershell_structure()
    check_transaction_contract()
    check_no_shell_download_execution()
    print("Everything installer smoke checks passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (AssertionError, subprocess.TimeoutExpired) as error:
        print(f"Installer smoke failed: {error}", file=sys.stderr)
        raise SystemExit(1)
