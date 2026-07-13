from __future__ import annotations

import json
from email.message import Message
from io import BytesIO
from pathlib import Path
from unittest.mock import patch
from urllib.error import HTTPError, URLError

import pytest

from everything_control import EverythingClient, EverythingClientError


class FakeResponse:
    def __init__(self, payload: object) -> None:
        self._body = json.dumps(payload).encode("utf-8")

    def __enter__(self) -> "FakeResponse":
        return self

    def __exit__(self, *args: object) -> None:
        return None

    def read(self) -> bytes:
        return self._body


def test_plan_posts_versioned_payload_and_returns_run_id(tmp_path: Path) -> None:
    response = {
        "run_id": "run-123",
        "document": {
            "objective": "repair imports",
            "content": "plan",
            "generated_by": "loopback",
        },
        "journal_path": str(tmp_path / "run-123.json"),
    }

    with patch("everything_control.client.urlopen", return_value=FakeResponse(response)) as opened:
        client = EverythingClient(tmp_path, base_url="http://localhost:9999")
        result = client.plan("repair imports", "deep")

    request = opened.call_args.args[0]
    assert request.full_url == "http://localhost:9999/v1/plan"
    assert request.method == "POST"
    assert json.loads(request.data) == {"objective": "repair imports", "mode": "Deep"}
    assert result.run_id == "run-123"


def test_query_url_encodes_term(tmp_path: Path) -> None:
    response = {"matched_nodes": []}
    with patch("everything_control.client.urlopen", return_value=FakeResponse(response)) as opened:
        EverythingClient(tmp_path).query("runtime graph")

    request = opened.call_args.args[0]
    assert request.full_url.endswith("/v1/query?term=runtime+graph")


def test_journals_fetches_full_run_details(tmp_path: Path) -> None:
    summary = {
        "run_id": "run-1",
        "objective": "objective",
        "status": "Completed",
        "generated_by": "loopback",
        "event_count": 1,
        "last_stage": "completed",
        "journal_path": str(tmp_path / "run-1.json"),
    }
    journal = {
        "run_id": "run-1",
        "objective": "objective",
        "status": "Completed",
        "generated_by": "loopback",
        "events": [{"stage": "completed", "detail": "ok"}],
        "artifact_path": None,
    }
    with patch(
        "everything_control.client.urlopen",
        side_effect=[FakeResponse([summary]), FakeResponse(journal)],
    ):
        result = EverythingClient(tmp_path).journals()

    assert [item.run_id for item in result] == ["run-1"]
    assert result[0].events[0].detail == "ok"


def test_http_error_preserves_status_and_api_message(tmp_path: Path) -> None:
    error = HTTPError(
        url="http://localhost/v1/plan",
        code=400,
        msg="Bad Request",
        hdrs=Message(),
        fp=BytesIO(b'{"error":"objective must not be empty"}'),
    )
    with patch("everything_control.client.urlopen", side_effect=error):
        with pytest.raises(EverythingClientError) as raised:
            EverythingClient(tmp_path).plan("valid", "fast")

    assert raised.value.status_code == 400
    assert "objective must not be empty" in str(raised.value)


def test_connection_error_has_actionable_daemon_address(tmp_path: Path) -> None:
    with patch(
        "everything_control.client.urlopen",
        side_effect=URLError("connection refused"),
    ):
        with pytest.raises(EverythingClientError, match="127.0.0.1:3472"):
            EverythingClient(tmp_path).doctor()



def test_doctor_parses_component_health_and_remediation(tmp_path: Path) -> None:
    payload = {
        "project": {
            "root_path": str(tmp_path),
            "file_count": 4,
            "graph_node_count": 12,
            "graph_edge_count": 8,
        },
        "adapters": {
            "filesystem": "local",
            "model": "ollama",
            "command": "local",
        },
        "model_health": {
            "status": "Degraded",
            "available": True,
            "adapter": "ollama",
            "detail": "fallback active",
            "primary_available": False,
            "fallback_available": True,
            "fallback_active": True,
        },
        "snapshot_stats": {
            "scanned_files": 4,
            "cache_hits": 3,
            "cache_misses": 1,
            "bytes_read": 1024,
        },
        "bootstrap_metrics": {
            "snapshot_millis": 10,
            "graph_millis": 20,
            "catalog_millis": 5,
        },
        "overall_status": "degraded",
        "checks": [
            {
                "check_id": "model",
                "label": "Local model",
                "status": "degraded",
                "detail": "fallback active",
                "remediation": "Start Ollama.",
            },
            {
                "check_id": "state-store",
                "label": "Durable runtime state",
                "status": "healthy",
                "detail": "SQLite/WAL readable",
            },
        ],
    }
    with patch("everything_control.client.urlopen", return_value=FakeResponse(payload)):
        report = EverythingClient(tmp_path).doctor()

    assert report.overall_status == "degraded"
    assert report.model_health.fallback_active is True
    assert report.checks[0].check_id == "model"
    assert report.checks[0].remediation == "Start Ollama."
    assert report.checks[1].status == "healthy"

def test_plan_rejects_unknown_mode_without_network(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="fast, balanced, deep"):
        EverythingClient(tmp_path).plan("objective", "turbo")


def test_artifact_methods_validate_typed_payloads_and_escape_ids(tmp_path: Path) -> None:
    descriptor = {
        "artifact_id": "artifact/run 1",
        "run_id": "run 1",
        "kind": "Plan",
        "content_hash": "abc123",
        "media_type": "text/markdown",
        "size_bytes": 6,
        "object_path": str(tmp_path / "abc123"),
        "created_at_epoch_millis": 1,
        "origin": "planner",
    }
    content = {"descriptor": descriptor, "encoding": "utf-8", "content": "# Plan"}

    with patch(
        "everything_control.client.urlopen",
        side_effect=[FakeResponse([descriptor]), FakeResponse(content)],
    ) as opened:
        client = EverythingClient(tmp_path)
        artifacts = client.artifacts("run 1")
        artifact = client.artifact("artifact/run 1")

    assert artifacts[0].kind == "Plan"
    assert artifact.content == "# Plan"
    assert opened.call_args_list[0].args[0].full_url.endswith(
        "/v1/runs/run%201/artifacts"
    )
    assert opened.call_args_list[1].args[0].full_url.endswith(
        "/v1/artifacts/artifact%2Frun%201"
    )


def test_recoverable_runs_preserve_recovery_disposition(tmp_path: Path) -> None:
    journal = {
        "run_id": "run-interrupted",
        "objective": "continue work",
        "status": "Blocked",
        "generated_by": "loopback",
        "events": [],
        "recoverable": True,
        "recovery_disposition": "ManualReview",
    }
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse([journal])
    ) as opened:
        result = EverythingClient(tmp_path).recoverable_runs()

    assert result[0].recovery_disposition == "ManualReview"
    assert opened.call_args.args[0].full_url.endswith("/v1/runs/recoverable")


def test_checkpoints_are_typed_and_run_id_is_escaped(tmp_path: Path) -> None:
    checkpoint = {
        "checkpoint_id": "checkpoint-1",
        "run_id": "run 1",
        "stage_id": "run 1:bootstrap",
        "event_sequence": 3,
        "created_at_epoch_millis": 10,
        "safe_to_resume": True,
        "summary": "bootstrap complete",
        "artifact_ids": [],
    }
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse([checkpoint])
    ) as opened:
        result = EverythingClient(tmp_path).checkpoints("run 1")

    assert result[0].safe_to_resume is True
    assert opened.call_args.args[0].full_url.endswith("/v1/runs/run%201/checkpoints")


def test_graph_traverse_sends_direction_and_relation_filters(tmp_path: Path) -> None:
    entity = {
        "id": "entity-1",
        "revision": 1,
        "kind": "function",
        "name": "run",
        "qualified_name": "runtime::run",
        "language": "rust",
        "file_path": "src/lib.rs",
        "span": {"start_line": 1, "end_line": 2},
        "valid_from_epoch_millis": 1,
    }
    response = {
        "graph_revision": 4,
        "root": entity,
        "direction": "outbound",
        "relation_kinds": ["calls"],
        "depth": 2,
        "entities": [entity],
        "relations": [],
    }
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse(response)
    ) as opened:
        result = EverythingClient(tmp_path).graph_traverse(
            "run", direction="outbound", depth=2, relations=["calls"]
        )

    assert result.graph_revision == 4
    assert result.direction == "outbound"
    assert "direction=outbound" in opened.call_args.args[0].full_url
    assert "relations=calls" in opened.call_args.args[0].full_url


def test_graph_path_rejects_invalid_direction_without_network(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="inbound, outbound, both"):
        EverythingClient(tmp_path).graph_path("a", "b", direction="sideways")


def test_tools_and_invocations_are_typed(tmp_path: Path) -> None:
    definition = {
        "tool_id": "workspace.read_file",
        "version": "1.0.0",
        "description": "read",
        "input_schema": {"type": "object"},
        "output_schema": {"type": "object"},
        "required_permissions": ["workspace_read"],
        "default_timeout_millis": 1000,
        "max_output_bytes": 4096,
        "supports_cancellation": False,
        "effect": "read_only",
    }
    invocation = {
        "invocation_id": "invocation/read 1",
        "run_id": "run 1",
        "stage_id": None,
        "tool_name": "workspace.read_file",
        "tool_version": "1.0.0",
        "required_permissions": ["workspace_read"],
        "effect": "read_only",
        "status": "Completed",
        "started_at_epoch_millis": 1,
        "finished_at_epoch_millis": 2,
        "arguments": {"path": "README.md"},
        "output": {"content": "hello"},
        "output_truncated": False,
        "timeout_millis": 1000,
        "replay_key": "abc",
        "result_summary": "done",
        "failure_class": None,
        "error_code": None,
    }
    with patch(
        "everything_control.client.urlopen",
        side_effect=[FakeResponse([definition]), FakeResponse([invocation])],
    ) as opened:
        client = EverythingClient(tmp_path)
        tools = client.tools()
        invocations = client.tool_invocations("run 1")

    assert tools[0].tool_id == "workspace.read_file"
    assert invocations[0].status == "Completed"
    assert opened.call_args_list[1].args[0].full_url.endswith(
        "/v1/runs/run%201/tool-invocations"
    )


def test_invoke_tool_posts_typed_request_and_escapes_id(tmp_path: Path) -> None:
    from everything_control import ToolInvocationRequest

    invocation = {
        "invocation_id": "invocation 1",
        "run_id": "run-1",
        "tool_name": "workspace.read_file",
        "status": "Completed",
        "started_at_epoch_millis": 1,
        "finished_at_epoch_millis": 2,
        "arguments": {"path": "README.md"},
        "output": {"content": "hello"},
    }
    response = {"invocation": invocation, "output": {"content": "hello"}}
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse(response)
    ) as opened:
        result = EverythingClient(tmp_path).invoke_tool(
            "invocation 1",
            ToolInvocationRequest(
                run_id="run-1",
                tool_id="workspace.read_file",
                input={"path": "README.md"},
            ),
        )

    request = opened.call_args.args[0]
    assert request.full_url.endswith("/v1/tools/invocations/invocation%201")
    assert request.method == "POST"
    assert json.loads(request.data)["approval_granted"] is False
    assert result.output == {"content": "hello"}


def test_propose_edit_posts_typed_approval_contract(tmp_path: Path) -> None:
    descriptor = {
        "artifact_id": "artifact-proposal",
        "run_id": "run-edit",
        "kind": "Patch",
        "content_hash": "abc123",
        "media_type": "application/json",
        "size_bytes": 64,
        "object_path": str(tmp_path / "abc123"),
        "created_at_epoch_millis": 1,
        "origin": "model-edit-proposal",
    }
    patch_request = {
        "objective": "update greeting",
        "mode": "Balanced",
        "relative_path": "hello.txt",
        "expected_content_hash": "abc",
        "replacement_content": "hello\n",
        "verification_commands": [],
        "approval_granted": False,
        "allow_repeat_failure": False,
    }
    response = {
        "run_id": "run-edit",
        "status": "awaiting_approval",
        "summary": "Update the greeting",
        "patch": patch_request,
        "diff": "--- hello.txt\n+++ hello.txt",
        "artifact": descriptor,
        "generated_by": "qwen2.5-coder:9b",
        "skill_id": "scoped-edit",
        "fallback_used": False,
        "fallback_reason": None,
    }
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse(response)
    ) as opened:
        result = EverythingClient(tmp_path).propose_edit(
            " update greeting ", "balanced", "scoped-edit"
        )

    request = opened.call_args.args[0]
    assert request.full_url.endswith("/v1/edits/propose")
    assert request.method == "POST"
    assert json.loads(request.data) == {
        "objective": "update greeting",
        "mode": "Balanced",
        "skill_id": "scoped-edit",
    }
    assert result.patch.approval_granted is False
    assert result.generated_by == "qwen2.5-coder:9b"
    assert result.skill_id == "scoped-edit"


def test_execute_patch_posts_verification_contract(tmp_path: Path) -> None:
    from everything_control import PatchExecutionRequest, VerificationCommand

    response = {
        "run_id": "run-patch",
        "status": "completed",
        "patch_invocation_id": "inv-patch",
        "verification_invocation_ids": ["inv-test"],
        "artifacts": [],
        "rolled_back": False,
        "summary": "patch applied and verification completed",
    }
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse(response)
    ) as opened:
        result = EverythingClient(tmp_path).execute_patch(
            PatchExecutionRequest(
                objective="update greeting",
                relative_path=Path("hello.txt"),
                expected_content_hash="abc",
                replacement_content="hello\n",
                verification_commands=[
                    VerificationCommand(
                        program="python",
                        args=["-m", "pytest", "-q"],
                        label="tests",
                    )
                ],
                approval_granted=True,
            )
        )

    request = opened.call_args.args[0]
    assert request.full_url.endswith("/v1/executions/patch")
    assert json.loads(request.data)["verification_commands"][0]["label"] == "tests"
    assert result.status == "completed"


def test_model_capabilities_are_typed(tmp_path: Path) -> None:
    payload = {
        "provider": "ollama",
        "model_name": "qwen2.5-coder:7b",
        "quality_tier": "small",
        "context_window_tokens": 32768,
        "safe_context_tokens": 12000,
        "coding_suitability": 0.85,
        "structured_output_reliability": 0.72,
        "tool_calling_reliability": 0.68,
        "estimated_tokens_per_second": None,
        "memory_estimate_mb": 8000,
        "recommended_task_classes": ["debugging"],
    }
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse(payload)
    ) as opened:
        profile = EverythingClient(tmp_path).model_capabilities()

    assert profile.quality_tier == "small"
    assert profile.safe_context_tokens == 12000
    assert opened.call_args.args[0].full_url.endswith("/v1/models/capabilities")


def test_model_discovery_is_typed(tmp_path: Path) -> None:
    payload = [{
        "provider": "ollama",
        "model_name": "qwen2.5-coder:7b",
        "installed": True,
        "configured": True,
        "profile": {
            "provider": "ollama",
            "model_name": "qwen2.5-coder:7b",
            "quality_tier": "small",
            "context_window_tokens": 32768,
            "safe_context_tokens": 12000,
            "coding_suitability": 0.85,
            "structured_output_reliability": 0.72,
            "tool_calling_reliability": 0.68,
            "estimated_tokens_per_second": None,
            "memory_estimate_mb": 8000,
            "recommended_task_classes": ["debugging"],
        },
    }]
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse(payload)
    ) as opened:
        models = EverythingClient(tmp_path).models()

    assert models[0].installed is True
    assert models[0].profile.quality_tier == "small"
    assert opened.call_args.args[0].full_url.endswith("/v1/models")


def test_model_invocations_are_typed_and_ids_are_escaped(tmp_path: Path) -> None:
    record = {
        "invocation_id": "model/run 1",
        "run_id": "run 1",
        "stage_id": "run 1:planning",
        "provider": "ollama",
        "model": "qwen2.5-coder:7b",
        "status": "Completed",
        "started_at_epoch_millis": 1,
        "finished_at_epoch_millis": 2,
        "prompt_hash": "abc",
        "response_artifact_id": "artifact-1",
        "fallback_used": False,
        "primary_error": None,
        "prompt_estimated_tokens": 1200,
        "output_bytes": 512,
        "duration_millis": 100,
        "failure_class": None,
        "error_code": None,
    }
    with patch(
        "everything_control.client.urlopen",
        side_effect=[FakeResponse([record]), FakeResponse(record)],
    ) as opened:
        client = EverythingClient(tmp_path)
        records = client.model_invocations("run 1")
        record_by_id = client.model_invocation("model/run 1")

    assert records[0].prompt_estimated_tokens == 1200
    assert record_by_id.duration_millis == 100
    assert opened.call_args_list[0].args[0].full_url.endswith(
        "/v1/runs/run%201/model-invocations"
    )
    assert opened.call_args_list[1].args[0].full_url.endswith(
        "/v1/models/invocations/model%2Frun%201"
    )


def _skill_payload(tmp_path: Path, *, enabled: bool = True) -> dict[str, object]:
    return {
        "manifest": {
            "skill_id": "review-helper",
            "name": "Review Helper",
            "version": "1.0.0",
            "runtime_api": "v1",
            "description": "Review code",
            "permissions": ["WorkspaceRead"],
            "input_schema": {"type": "object"},
            "output_schema": {"type": "object"},
            "entrypoint": "prompt:SKILL.md",
            "workflow": "Prompt",
        },
        "enabled": enabled,
        "compatibility": {"compatible": True, "runtime_api": "v1"},
        "source": "Workspace",
        "source_path": str(tmp_path / ".everything/skills/review-helper"),
        "content_hash": "abc",
        "instructions_preview": "Review the code.",
    }


def test_skill_install_toggle_execute_and_uninstall(tmp_path: Path) -> None:
    from everything_control import SkillExecutionRequest

    skill = _skill_payload(tmp_path)
    execution = {
        "skill_id": "review-helper",
        "skill_version": "1.0.0",
        "status": "Completed",
        "run_id": "run-skill",
        "artifact_ids": [],
        "output": {"document": {"content": "review"}},
    }
    with patch(
        "everything_control.client.urlopen",
        side_effect=[
            FakeResponse(skill),
            FakeResponse({**skill, "enabled": False}),
            FakeResponse(execution),
            FakeResponse({"uninstalled": True}),
        ],
    ) as opened:
        client = EverythingClient(tmp_path)
        installed = client.install_skill(tmp_path / "source-skill")
        disabled = client.set_skill_enabled("review-helper", False)
        result = client.execute_skill(
            "review-helper",
            SkillExecutionRequest(input={"objective": "review runtime"}),
        )
        removed = client.uninstall_skill("review-helper")

    assert installed.source == "Workspace"
    assert disabled.enabled is False
    assert result.run_id == "run-skill"
    assert removed is True
    assert opened.call_args_list[0].args[0].method == "POST"
    assert opened.call_args_list[0].args[0].full_url.endswith("/v1/skills/install")
    assert opened.call_args_list[3].args[0].method == "DELETE"


def test_memory_lifecycle_is_typed(tmp_path: Path) -> None:
    from everything_control import MemoryUpsertRequest

    entry = {
        "memory_id": "memory-1",
        "scope": "Workspace",
        "title": "Build preference",
        "content": "Run focused tests first.",
        "source": "operator",
        "workspace_key": str(tmp_path),
        "version": 1,
        "confidence": 0.9,
        "evidence_ids": [],
        "tags": ["tests"],
        "editable": True,
        "forgettable": True,
        "created_at_epoch_millis": 1,
        "updated_at_epoch_millis": 1,
    }
    with patch(
        "everything_control.client.urlopen",
        side_effect=[
            FakeResponse(entry),
            FakeResponse([{"entry": entry, "score": 1.0}]),
            FakeResponse({"forgotten": True}),
        ],
    ) as opened:
        client = EverythingClient(tmp_path)
        saved = client.remember(
            MemoryUpsertRequest(
                scope="Workspace",
                title="Build preference",
                content="Run focused tests first.",
                source="operator",
                workspace_key=str(tmp_path),
                confidence=0.9,
                tags=["tests"],
            )
        )
        results = client.search_memory("focused tests", scope="workspace")
        forgotten = client.forget_memory("memory-1")

    assert saved.memory_id == "memory-1"
    assert results[0].entry.title == "Build preference"
    assert forgotten is True
    assert opened.call_args_list[2].args[0].method == "DELETE"


def test_connector_oauth_and_action_are_typed(tmp_path: Path) -> None:
    from everything_control import ConnectorActionRequest

    oauth = {
        "provider": "Spotify",
        "authorization_url": "https://accounts.spotify.com/authorize?state=x",
        "redirect_uri": "http://127.0.0.1:43821/v1/connectors/oauth/callback/spotify",
        "state": "x",
        "expires_at_epoch_millis": 123,
    }
    action = {
        "provider": "Spotify",
        "action_id": "playback",
        "status": "completed",
        "risk": "ReadOnly",
        "executed": True,
        "output": {"data": {"is_playing": True}},
        "warnings": [],
    }
    with patch(
        "everything_control.client.urlopen",
        side_effect=[FakeResponse(oauth), FakeResponse(action)],
    ) as opened:
        client = EverythingClient(tmp_path)
        start = client.start_connector_oauth("spotify")
        result = client.execute_connector_action(
            ConnectorActionRequest(
                provider="Spotify", action_id="playback", input={}
            )
        )

    assert start.provider == "Spotify"
    assert result.executed is True
    assert opened.call_args_list[0].args[0].full_url.endswith(
        "/v1/connectors/spotify/oauth/start"
    )
    assert opened.call_args_list[1].args[0].full_url.endswith(
        "/v1/connectors/spotify/actions"
    )



def test_automation_pending_execution_can_be_approved(tmp_path: Path) -> None:
    execution = {
        "execution_id": "aexec-approved",
        "automation_id": "auto/1",
        "status": "Completed",
        "scheduled_for_epoch_millis": 2,
        "started_at_epoch_millis": 3,
        "finished_at_epoch_millis": 4,
        "output": {"status": "completed"},
        "attempt": 1,
    }
    with patch(
        "everything_control.client.urlopen", return_value=FakeResponse(execution)
    ) as opened:
        result = EverythingClient(tmp_path).approve_automation_execution(
            "auto/1", "pending execution"
        )

    assert result.status == "Completed"
    request = opened.call_args.args[0]
    assert request.method == "POST"
    assert request.full_url.endswith(
        "/v1/automations/auto%2F1/executions/pending%20execution/approve"
    )

def test_automation_lifecycle_is_typed(tmp_path: Path) -> None:
    from everything_control import AutomationUpsertRequest

    definition = {
        "automation_id": "auto-1",
        "name": "Morning inbox",
        "description": "Read unread mail",
        "schedule": {"DailyFixedOffset": {"hour": 9, "minute": 0, "utc_offset_minutes": 180}},
        "action": {"Connector": {"request": {"provider": "Gmail", "action_id": "unread_summary"}}},
        "policy": {"autonomy_level": "Assist", "allow_external_read": True},
        "enabled": True,
        "created_at_epoch_millis": 1,
        "updated_at_epoch_millis": 1,
        "next_run_at_epoch_millis": 2,
        "consecutive_failures": 0,
    }
    execution = {
        "execution_id": "aexec-1",
        "automation_id": "auto-1",
        "status": "Completed",
        "scheduled_for_epoch_millis": 2,
        "started_at_epoch_millis": 2,
        "finished_at_epoch_millis": 3,
        "output": {},
        "attempt": 1,
    }
    with patch(
        "everything_control.client.urlopen",
        side_effect=[FakeResponse(definition), FakeResponse(execution), FakeResponse(None)],
    ) as opened:
        client = EverythingClient(tmp_path)
        saved = client.upsert_automation(
            AutomationUpsertRequest(
                name="Morning inbox",
                schedule={"DailyFixedOffset": {"hour": 9, "minute": 0, "utc_offset_minutes": 180}},
                action={"Connector": {"request": {"provider": "Gmail", "action_id": "unread_summary"}}},
            )
        )
        run = client.run_automation("auto-1")
        client.delete_automation("auto-1")

    assert saved.name == "Morning inbox"
    assert run.status == "Completed"
    assert opened.call_args_list[2].args[0].method == "DELETE"


def test_research_search_is_typed_and_bounded(tmp_path: Path) -> None:
    payload = {
        "report_id": "research-1",
        "query": "latest rust release",
        "normalized_query": "latest rust release",
        "mode": "technical",
        "freshness": "month",
        "sources": [{
            "citation_id": "W1",
            "title": "Rust release",
            "url": "https://example.com/rust",
            "canonical_url": "https://example.com/rust",
            "domain": "example.com",
            "provider": "SearXNG",
            "rank": 1,
            "score": 3.5,
            "snippet": "release notes",
            "extracted_text": "release notes",
            "retrieved_at_epoch_millis": 1,
            "content_hash": "abc",
            "from_cache": False,
            "metadata": {},
        }],
        "provider_count": 1,
        "cache_hits": 0,
        "searched_at_epoch_millis": 1,
        "search_millis": 10,
        "fetch_millis": 20,
        "warnings": [],
    }
    with patch("everything_control.client.urlopen", return_value=FakeResponse(payload)) as opened:
        report = EverythingClient(tmp_path).research_search(
            "latest rust release", freshness="month", allowed_domains=["rust-lang.org"]
        )

    assert report.sources[0].citation_id == "W1"
    request = opened.call_args.args[0]
    assert request.full_url.endswith("/v1/research/search")
    assert json.loads(request.data)["allowed_domains"] == ["rust-lang.org"]


def test_graph_change_impact_posts_typed_request(tmp_path: Path) -> None:
    from everything_control import CodeGraphChangeImpactRequest, CodeGraphChangeTarget

    response = {
        "schema_version": 2,
        "graph_revision": 4,
        "targets": [{"file_path": "src/lib.rs", "change_kind": "modify"}],
        "roots": [],
        "risk_tier": "medium",
        "aggregate_risk_score": 42.0,
        "affected_entities": [],
        "affected_files": ["src/lib.rs"],
        "verification_targets": [],
        "public_api_entities": [],
        "external_dependencies": [],
        "unresolved_targets": [],
        "analysis_millis": 3,
    }
    with patch("everything_control.client.urlopen", return_value=FakeResponse(response)) as opened:
        report = EverythingClient(tmp_path).graph_change_impact(
            CodeGraphChangeImpactRequest(targets=[CodeGraphChangeTarget(file_path="src/lib.rs")])
        )

    assert report.aggregate_risk_score == 42.0
    assert json.loads(opened.call_args.args[0].data)["targets"][0]["file_path"] == "src/lib.rs"


def test_github_is_accepted_as_connector_provider(tmp_path: Path) -> None:
    payload = {
        "provider": "GitHub",
        "display_name": "GitHub",
        "description": "repositories",
        "status": "Connected",
        "configured": True,
        "connected": True,
        "granted_scopes": ["github:token"],
        "actions": [],
        "limitations": [],
        "metadata": {},
    }
    with patch("everything_control.client.urlopen", return_value=FakeResponse(payload)):
        connector = EverythingClient(tmp_path).connector("github")
    assert connector.provider == "GitHub"
