#!/usr/bin/env python3
"""Static MVP release smoke test that does not require downloading toolchains."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--require-built-ui", action="store_true")
    args = parser.parse_args()

    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]
    members = workspace["members"]
    for member in members:
        require((ROOT / member / "Cargo.toml").is_file(), f"missing workspace member: {member}")

    lock = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
    for package in ("everything-memory", "everything-skills", "everything-verifier", "everything-research"):
        require(f'name = "{package}"' in lock, f"Cargo.lock missing {package}")

    config = tomllib.loads((ROOT / "everything.toml").read_text(encoding="utf-8"))
    require(config["model"]["backend"] == "Ollama", "MVP must default to local Ollama")
    require(bool(config["model"]["model_name"]), "model name is empty")

    package = json.loads((ROOT / "apps/everything-app/package.json").read_text(encoding="utf-8"))
    require(package["main"] == "out/main/index.js", "Electron main entry is inconsistent")
    require((ROOT / "apps/everything-app/package-lock.json").is_file(), "package lock missing")

    if args.require_built_ui:
        for relative in ("out/main/index.js", "out/preload/index.js", "out/renderer/index.html"):
            require((ROOT / "apps/everything-app" / relative).is_file(), f"missing UI build: {relative}")

    for script in (
        ROOT / "install.sh",
        ROOT / "install.ps1",
        ROOT / "setup.sh",
        ROOT / "setup.ps1",
        ROOT / "bootstrap.sh",
        ROOT / "bootstrap.ps1",
        ROOT / "scripts/smoke_ollama.py",
        ROOT / "scripts/smoke_installers.py",
        ROOT / "scripts/research_sidecar.sh",
        ROOT / "scripts/research_sidecar.ps1",
    ):
        require(script.is_file(), f"missing installer: {script.name}")
    require(os.access(ROOT / "install.sh", os.X_OK), "install.sh is not executable")
    require(os.access(ROOT / "setup.sh", os.X_OK), "setup.sh is not executable")
    require(os.access(ROOT / "bootstrap.sh", os.X_OK), "bootstrap.sh is not executable")
    require(os.access(ROOT / "scripts/smoke_installers.py", os.X_OK), "installer smoke is not executable")
    unix_installer = (ROOT / "install.sh").read_text(encoding="utf-8")
    windows_installer = (ROOT / "install.ps1").read_text(encoding="utf-8")
    require(
        "node node_modules/electron/install.js" in unix_installer,
        "Unix install does not materialize the Electron platform runtime",
    )
    require(
        "node node_modules/electron/install.js" in windows_installer,
        "Windows install does not materialize the Electron platform runtime",
    )
    require("npm prune --omit=dev" in unix_installer, "Unix install keeps development dependencies")
    require("npm prune --omit=dev" in windows_installer, "Windows install keeps development dependencies")
    require(
        "node_modules/electron/dist/electron" in unix_installer,
        "Unix launcher does not use the packaged Electron runtime",
    )
    require(
        "node_modules\\electron\\dist\\electron.exe" in windows_installer,
        "Windows launcher does not use the packaged Electron runtime",
    )

    example = ROOT / "examples/skills/review-helper/SKILL.md"
    require(example.is_file(), "example skill package missing")
    contents = example.read_text(encoding="utf-8")
    require(contents.startswith("---\n"), "example skill front matter missing")
    require(re.search(r"(?m)^id:\s*review-helper\s*$", contents) is not None, "example skill id missing")

    required_routes = (
        "/v1/skills",
        "/v1/connectors",
        "/v1/automations",
        "/v1/skills/install",
        "/v1/memory",
        "/v1/runs",
        "/v1/models",
        "/v1/edits/propose",
        "/v1/executions/patch",
        "/v1/code-graph/change-impact",
        "/v1/research/status",
        "/v1/research/search",
        "/v1/research/fetch",
        "/v1/research/cache/purge",
    )
    edit_runtime = ROOT / "crates/everything-runtime/src/edit.rs"
    require(edit_runtime.is_file(), "natural-language edit proposal runtime missing")
    edit_source = edit_runtime.read_text(encoding="utf-8")
    require("preview_patch" in edit_source, "edit proposal does not produce a safe preview")
    require("AwaitingApproval" in edit_source, "edit proposal is not approval-gated")
    require("skill_policy" in edit_source, "edit proposal does not carry skill policy")
    require("candidate_change_impact" in edit_source, "edit proposal does not use change impact before file selection")
    require("smallest safe blast radius" in edit_source, "edit model policy does not minimize blast radius")
    require(
        "edit_skill_instruction_byte_budget" in edit_source,
        "edit skill policy is not mode-budgeted",
    )
    cli = (ROOT / "apps/everything-cli/src/main.rs").read_text(encoding="utf-8")
    require("skill_id" in cli and "ProposeEdit" in cli, "CLI edit skill selection missing")
    renderer = (ROOT / "apps/everything-app/src/renderer/src/App.tsx").read_text(encoding="utf-8")
    require("editSkillId" in renderer, "desktop edit skill selection missing")
    automation_ui = (ROOT / "apps/everything-app/src/renderer/src/AutomationsScreen.tsx").read_text(encoding="utf-8")
    require("WeeklyFixedOffset" in automation_ui, "desktop weekly automation scheduling missing")
    require("DailyLocal" in automation_ui, "desktop DST-aware local scheduling missing")
    require("Everything skill" in automation_ui, "desktop skill automation selection missing")
    require("Everything öz-sağlık" in automation_ui, "desktop native doctor routine preset missing")
    require("Onayla ve çalıştır" in automation_ui, "desktop durable automation approval missing")
    connections_ui = (ROOT / "apps/everything-app/src/renderer/src/ConnectionsScreen.tsx").read_text(encoding="utf-8")
    require("official API" in connections_ui or "resmi API" in connections_ui, "desktop official connector messaging missing")
    require("selectedMissingScopes" in connections_ui, "desktop connector scope preflight missing")
    providers = (ROOT / "crates/everything-connectors/src/providers.rs").read_text(encoding="utf-8")
    spotify_defaults = providers.split("ConnectorProvider::Spotify => ProviderSpec", 1)[1].split("ConnectorProvider::TikTok => ProviderSpec", 1)[0]
    tiktok_defaults = providers.split("ConnectorProvider::TikTok => ProviderSpec", 1)[1].split("ConnectorProvider::Instagram => ProviderSpec", 1)[0]
    instagram_defaults = providers.split("ConnectorProvider::Instagram => ProviderSpec", 1)[1].split("ConnectorProvider::Custom => ProviderSpec", 1)[0]
    require('"user-modify-playback-state"' not in spotify_defaults.split("allowed_hosts", 1)[0], "Spotify defaults request write scope")
    require('"video.publish"' not in tiktok_defaults.split("allowed_hosts", 1)[0], "TikTok defaults request publish scope")
    require('"instagram_business_content_publish"' not in instagram_defaults.split("allowed_hosts", 1)[0], "Instagram defaults request publish scope")
    main_process = (ROOT / "apps/everything-app/src/main/index.ts").read_text(encoding="utf-8")
    require("notifyAutomationEvent" in main_process, "desktop automation notifications missing")

    daemon = (ROOT / "apps/everythingd/src/main.rs").read_text(encoding="utf-8")
    for route in required_routes:
        require(route in daemon, f"daemon route missing: {route}")
    require("/executions/{execution_id}/approve" in daemon, "pending automation approval route missing")

    automation_domain = (ROOT / "crates/everything-domain/src/automation.rs").read_text(encoding="utf-8")
    require("Doctor {}" in automation_domain, "native automation doctor action missing")
    require("DailyLocal" in automation_domain and "WeeklyLocal" in automation_domain, "DST-aware schedule contracts missing")
    execution_runtime = (ROOT / "crates/everything-runtime/src/execution.rs").read_text(encoding="utf-8")
    require("patch-preflight-impact" in execution_runtime, "all patch transactions must persist preflight impact evidence")
    require("patch-postflight-impact" in execution_runtime, "successful patches must persist postflight impact evidence")
    require("changed_line_range" in execution_runtime, "postflight impact must use the actual changed line range")
    graph_store = (ROOT / "crates/everything-graph/src/store.rs").read_text(encoding="utf-8")
    require("BinaryHeap::<ImpactQueueEntry>" in graph_store, "change impact traversal is not priority-queue based")
    require("Converging impact from" in graph_store, "multi-root change convergence is not scored")
    require("dangling edges" in graph_store, "incremental graph cleanup does not document dangling-edge prevention")
    research_runtime = (ROOT / "crates/everything-research/src/lib.rs").read_text(encoding="utf-8")
    for token in ("SearXNG", "Wikipedia", "OpenAlex", "force_refresh"):
        require(token in research_runtime, f"native research runtime missing: {token}")
    for token in ("MAX_FETCH_WORKERS", "MAX_CACHED_SEARCHES", "MAX_CACHED_DOCUMENTS", "source_quality_score", "source_quality"):
        require(token in research_runtime, f"research quality/performance control missing: {token}")
    runtime_source = (ROOT / "crates/everything-runtime/src/lib.rs").read_text(encoding="utf-8")
    require("should_auto_research" in runtime_source and "[offline]" in runtime_source, "automatic research/offline policy missing")
    skills_source = (ROOT / "crates/everything-skills/src/lib.rs").read_text(encoding="utf-8")
    require('"web-research"' in skills_source and '"github-operator"' in skills_source, "research/GitHub builtin skills missing")
    require("NetworkExternal" in skills_source and "GitRead" in skills_source, "builtin integration skill permissions missing")
    require('ConnectorProvider::GitHub' in providers, "GitHub provider missing")
    for action in ("rest_read", "rest_write", "graphql_read", "graphql_mutation", "create_release", "security_alerts"):
        require(f'"{action}"' in providers, f"GitHub connector action missing: {action}")

    automation_runtime = (ROOT / "crates/everything-runtime/src/automation.rs").read_text(encoding="utf-8")
    require("DeadLetter" in automation_runtime, "automation dead-letter handling missing")
    require("renew_automation_lease" in automation_runtime, "automation lease renewal missing")
    smoke_ollama = (ROOT / "scripts/smoke_ollama.py").read_text(encoding="utf-8")
    require("wait_for_service" in smoke_ollama, "live model smoke does not wait for daemon readiness")
    require("REQUIRED_DOCTOR_CHECKS" in smoke_ollama, "live model smoke does not validate runtime doctor")

    print("Everything MVP static smoke test passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AssertionError as error:
        print(f"MVP smoke test failed: {error}", file=sys.stderr)
        raise SystemExit(1)
