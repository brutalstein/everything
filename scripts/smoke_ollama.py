#!/usr/bin/env python3
"""Run a live Everything/Ollama MVP smoke test against an already running daemon."""
from __future__ import annotations

import argparse
import json
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import urljoin
from urllib.request import Request, urlopen


REQUIRED_DOCTOR_CHECKS = {
    "model",
    "graph",
    "state-store",
    "memory",
    "skills",
    "connectors",
    "scheduler",
    "tool-sandbox",
    "data-directory",
}


def request(base_url: str, method: str, path: str, body: object | None = None, timeout: float = 300.0):
    payload = None if body is None else json.dumps(body).encode("utf-8")
    headers = {"content-type": "application/json"} if payload is not None else {}
    req = Request(urljoin(base_url.rstrip("/") + "/", path.lstrip("/")), data=payload, headers=headers, method=method)
    with urlopen(req, timeout=timeout) as response:
        raw = response.read()
        return json.loads(raw) if raw else None


def wait_for_service(base_url: str, timeout: float) -> dict[str, object]:
    deadline = time.monotonic() + max(1.0, timeout)
    delay = 0.1
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            response = request(base_url, "GET", "/v1/info", timeout=min(5.0, timeout))
            if isinstance(response, dict) and response.get("service"):
                return response
            last_error = RuntimeError("service info response was incomplete")
        except (HTTPError, URLError, TimeoutError, ConnectionError, json.JSONDecodeError) as error:
            last_error = error
        time.sleep(delay)
        delay = min(delay * 1.7, 2.0)
    raise RuntimeError(f"daemon did not become ready within {timeout:.1f}s: {last_error}")


def validate_doctor(doctor: object) -> dict[str, object]:
    if not isinstance(doctor, dict):
        raise RuntimeError("doctor response was not an object")
    checks = doctor.get("checks")
    if not isinstance(checks, list):
        raise RuntimeError("doctor response did not contain component checks")
    check_ids = {
        item.get("check_id")
        for item in checks
        if isinstance(item, dict) and isinstance(item.get("check_id"), str)
    }
    missing = sorted(REQUIRED_DOCTOR_CHECKS - check_ids)
    if missing:
        raise RuntimeError(f"doctor response is missing checks: {', '.join(missing)}")
    failed = [
        item
        for item in checks
        if isinstance(item, dict) and str(item.get("status", "")).lower() == "failed"
    ]
    if failed:
        summary = "; ".join(
            f"{item.get('check_id')}: {item.get('detail', 'failed')}" for item in failed
        )
        raise RuntimeError(f"doctor reported failed components: {summary}")
    return doctor


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://127.0.0.1:3472")
    parser.add_argument("--model", default="qwen2.5-coder:7b")
    parser.add_argument(
        "--objective",
        default="Summarize this repository architecture and identify the three highest-risk engineering gaps.",
    )
    parser.add_argument("--mode", choices=("Fast", "Balanced", "Deep"), default="Fast")
    parser.add_argument("--timeout", type=float, default=600.0)
    parser.add_argument("--startup-timeout", type=float, default=45.0)
    parser.add_argument(
        "--edit-objective",
        default=None,
        help="Optionally request a proposal-only single-file edit; nothing is applied.",
    )
    parser.add_argument(
        "--edit-skill",
        default="scoped-edit",
        help="Enabled coding or prompt skill used as proposal policy.",
    )
    args = parser.parse_args()

    info = wait_for_service(args.base_url, args.startup_timeout)
    print(f"service: {info.get('service', 'unknown')}")

    doctor = validate_doctor(request(args.base_url, "GET", "/v1/doctor", timeout=30))
    print(f"model health: {doctor.get('model_health', {}).get('status', 'unknown')}")

    models = request(args.base_url, "GET", "/v1/models", timeout=30)
    configured = [item for item in models if item.get("configured")]
    if not configured:
        raise RuntimeError("no configured model was reported")
    configured_names = [
        item.get("model_name") for item in configured if item.get("model_name")
    ]
    print(f"configured model: {configured_names[0]}")
    if args.model not in configured_names:
        raise RuntimeError(
            f"expected configured model {args.model!r}, got {configured_names!r}"
        )

    index = request(args.base_url, "POST", "/v1/graph/index", {}, timeout=args.timeout)
    print(
        "graph index: "
        f"revision={index.get('graph_revision')} scanned={index.get('scanned_files')} "
        f"changed={index.get('changed_files')} unchanged={index.get('unchanged_files')}"
    )

    started = time.monotonic()
    plan = request(
        args.base_url,
        "POST",
        "/v1/plan",
        {"objective": args.objective, "mode": args.mode},
        timeout=args.timeout,
    )
    duration = time.monotonic() - started
    run_id = plan.get("run_id")
    if not run_id:
        raise RuntimeError("plan response did not contain run_id")
    document = plan.get("document", {}).get("content", "")
    if not document.strip():
        raise RuntimeError("plan response was empty")
    print(f"plan run: {run_id} ({duration:.2f}s, {len(document.encode('utf-8'))} bytes)")

    run = request(args.base_url, "GET", f"/v1/runs/{run_id}", timeout=30)
    if run.get("status") != "Completed":
        raise RuntimeError(f"run did not complete: {run.get('status')}")
    model_calls = request(args.base_url, "GET", f"/v1/runs/{run_id}/model-invocations", timeout=30)
    if not model_calls:
        raise RuntimeError("run contains no model invocation evidence")
    call = model_calls[-1]
    print(
        "model evidence: "
        f"provider={call.get('provider')} model={call.get('model')} "
        f"fallback={call.get('fallback_used')} duration_ms={call.get('duration_millis')}"
    )

    if args.edit_objective:
        proposal = request(
            args.base_url,
            "POST",
            "/v1/edits/propose",
            {
                "objective": args.edit_objective,
                "mode": args.mode,
                "skill_id": args.edit_skill,
            },
            timeout=args.timeout,
        )
        if proposal.get("status") != "awaiting_approval":
            raise RuntimeError(
                f"edit proposal was not approval-gated: {proposal.get('status')}"
            )
        patch = proposal.get("patch", {})
        if patch.get("approval_granted") is not False:
            raise RuntimeError("edit proposal unexpectedly granted mutation approval")
        if not proposal.get("diff") or not patch.get("expected_content_hash"):
            raise RuntimeError("edit proposal is missing diff or base hash")
        if proposal.get("skill_id") != args.edit_skill:
            raise RuntimeError(
                f"edit proposal did not preserve skill policy: {proposal.get('skill_id')!r}"
            )
        print(
            "edit proposal: "
            f"run={proposal.get('run_id')} path={patch.get('relative_path')} "
            f"model={proposal.get('generated_by')} skill={proposal.get('skill_id')}"
        )

    print("Everything live Ollama smoke test passed")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (HTTPError, URLError, RuntimeError, TimeoutError) as error:
        print(f"Everything live Ollama smoke test failed: {error}", file=sys.stderr)
        raise SystemExit(1)
