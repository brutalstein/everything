from __future__ import annotations

import json
from pathlib import Path
from typing import Any, TypeVar
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen

from pydantic import BaseModel, TypeAdapter

from .models import (
    ArtifactContentResponse,
    ArtifactDescriptor,
    Checkpoint,
    CodeGraphChangeImpactReport,
    CodeGraphChangeImpactRequest,
    CodeGraphIndexReport,
    CodeGraphPath,
    CodeGraphSearchResult,
    CodeGraphTraversalReport,
    DiscoveredModel,
    EditProposalRequest,
    EditProposalResponse,
    GraphImpactReport,
    GraphQueryResult,
    GraphSummaryResponse,
    ModuleDescriptor,
    ModelCapabilityProfile,
    ModelInvocationRecord,
    PersistentGraphStats,
    PlanResponse,
    RunJournal,
    RunSummary,
    ResearchReport,
    ResearchStatus,
    RuntimeDoctorReport,
    ServiceInfo,
    CancellationResponse,
    PatchExecutionRequest,
    PatchExecutionResponse,
    SkillDescriptor,
    SkillExecutionRequest,
    SkillExecutionResponse,
    MemoryEntry,
    MemorySearchResult,
    MemoryUpsertRequest,
    ToolDefinition,
    ToolInvocationRecord,
    ToolInvocationRequest,
    ToolInvocationResponse,
    AutomationDefinition,
    AutomationExecution,
    AutomationUpsertRequest,
    ConnectorActionRequest,
    ConnectorActionResponse,
    ConnectorAuditRecord,
    ConnectorConfigureRequest,
    ConnectorDescriptor,
    OAuthStartResponse,
    WebFetchResponse,
)

ModelT = TypeVar("ModelT", bound=BaseModel)


class EverythingClientError(RuntimeError):
    """Raised when the Everything daemon cannot satisfy a request."""

    def __init__(
        self,
        message: str,
        *,
        status_code: int | None = None,
        response_body: str | None = None,
    ) -> None:
        super().__init__(message)
        self.status_code = status_code
        self.response_body = response_body


class EverythingClient:
    """Typed synchronous client for the versioned ``everythingd`` HTTP API."""

    def __init__(
        self,
        workspace: str | Path = ".",
        *,
        base_url: str = "http://127.0.0.1:3472",
        timeout: float = 30.0,
    ) -> None:
        self.workspace = Path(workspace).expanduser().resolve()
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout

    def info(self) -> ServiceInfo:
        return self._get_model("/v1/info", ServiceInfo)

    def doctor(self) -> RuntimeDoctorReport:
        return self._get_model("/v1/doctor", RuntimeDoctorReport)

    def models(self) -> list[DiscoveredModel]:
        payload = self._request_json("GET", "/v1/models")
        return TypeAdapter(list[DiscoveredModel]).validate_python(payload)

    def model_capabilities(self) -> ModelCapabilityProfile:
        return self._get_model("/v1/models/capabilities", ModelCapabilityProfile)


    def modules(self) -> list[ModuleDescriptor]:
        payload = self._request_json("GET", "/v1/modules")
        return TypeAdapter(list[ModuleDescriptor]).validate_python(payload)

    def graph(self) -> GraphSummaryResponse:
        return self._get_model("/v1/graph", GraphSummaryResponse)

    def query(self, term: str) -> GraphQueryResult:
        return self._get_model("/v1/query", GraphQueryResult, query={"term": term})

    def impact(self, term: str, depth: int | None = None) -> GraphImpactReport:
        query: dict[str, str | int] = {"term": term}
        if depth is not None:
            query["depth"] = depth
        return self._get_model("/v1/impact", GraphImpactReport, query=query)

    def graph_index(self) -> CodeGraphIndexReport:
        payload = self._request_json("POST", "/v1/graph/index")
        return CodeGraphIndexReport.model_validate(payload)

    def graph_stats(self) -> PersistentGraphStats:
        return self._get_model("/v1/graph/stats", PersistentGraphStats)

    def graph_search(self, term: str, limit: int = 20) -> list[CodeGraphSearchResult]:
        payload = self._request_json(
            "GET", "/v1/graph/search", query={"term": term, "limit": limit}
        )
        return TypeAdapter(list[CodeGraphSearchResult]).validate_python(payload)

    def graph_traverse(
        self,
        term: str,
        *,
        direction: str = "inbound",
        depth: int = 3,
        limit: int = 100,
        relations: list[str] | None = None,
    ) -> CodeGraphTraversalReport:
        normalized = direction.strip().lower()
        if normalized not in {"inbound", "outbound", "both"}:
            raise ValueError("direction must be one of: inbound, outbound, both")
        query: dict[str, str | int] = {
            "term": term,
            "direction": normalized,
            "depth": depth,
            "limit": limit,
        }
        if relations:
            query["relations"] = ",".join(relations)
        return self._get_model(
            "/v1/graph/traverse", CodeGraphTraversalReport, query=query
        )

    def graph_path(
        self,
        source: str,
        target: str,
        *,
        direction: str = "outbound",
        depth: int = 6,
        relations: list[str] | None = None,
    ) -> CodeGraphPath | None:
        normalized = direction.strip().lower()
        if normalized not in {"inbound", "outbound", "both"}:
            raise ValueError("direction must be one of: inbound, outbound, both")
        query: dict[str, str | int] = {
            "from": source,
            "to": target,
            "direction": normalized,
            "depth": depth,
        }
        if relations:
            query["relations"] = ",".join(relations)
        payload = self._request_json("GET", "/v1/graph/path", query=query)
        return None if payload is None else CodeGraphPath.model_validate(payload)

    def graph_change_impact(
        self, request: CodeGraphChangeImpactRequest
    ) -> CodeGraphChangeImpactReport:
        payload = self._request_json(
            "POST",
            "/v1/graph/change-impact",
            body=request.model_dump(mode="json", exclude_none=True),
        )
        return CodeGraphChangeImpactReport.model_validate(payload)

    def research_status(self) -> ResearchStatus:
        return self._get_model("/v1/research/status", ResearchStatus)

    def research_search(
        self,
        query: str,
        *,
        mode: str = "technical",
        freshness: str = "any",
        max_results: int = 12,
        fetch_pages: int = 6,
        allowed_domains: list[str] | None = None,
        blocked_domains: list[str] | None = None,
        force_refresh: bool = False,
    ) -> ResearchReport:
        query = query.strip()
        if not query:
            raise ValueError("query must not be empty")
        payload = self._request_json(
            "POST",
            "/v1/research/search",
            body={
                "query": query,
                "mode": mode.strip().lower(),
                "freshness": freshness.strip().lower(),
                "max_results": max_results,
                "fetch_pages": fetch_pages,
                "allowed_domains": allowed_domains or [],
                "blocked_domains": blocked_domains or [],
                "force_refresh": force_refresh,
            },
        )
        return ResearchReport.model_validate(payload)

    def research_fetch(self, url: str, *, force_refresh: bool = False) -> WebFetchResponse:
        url = url.strip()
        if not url:
            raise ValueError("url must not be empty")
        payload = self._request_json(
            "POST",
            "/v1/research/fetch",
            body={"url": url, "force_refresh": force_refresh},
        )
        return WebFetchResponse.model_validate(payload)

    def purge_research_cache(self) -> int:
        payload = self._request_json("POST", "/v1/research/cache/purge")
        return int(payload.get("removed", 0))

    def plan(self, objective: str, mode: str = "balanced") -> PlanResponse:
        objective = objective.strip()
        if not objective:
            raise ValueError("objective must not be empty")

        normalized_mode = {
            "fast": "Fast",
            "balanced": "Balanced",
            "deep": "Deep",
        }.get(mode.strip().lower())
        if normalized_mode is None:
            raise ValueError("mode must be one of: fast, balanced, deep")

        payload = self._request_json(
            "POST",
            "/v1/plan",
            body={"objective": objective, "mode": normalized_mode},
        )
        return PlanResponse.model_validate(payload)

    def runs(self) -> list[RunSummary]:
        payload = self._request_json("GET", "/v1/runs")
        return TypeAdapter(list[RunSummary]).validate_python(payload)

    def run(self, run_id: str) -> RunJournal:
        if not run_id.strip():
            raise ValueError("run_id must not be empty")
        return self._get_model(f"/v1/runs/{quote(run_id, safe='')}", RunJournal)

    def recoverable_runs(self) -> list[RunJournal]:
        payload = self._request_json("GET", "/v1/runs/recoverable")
        return TypeAdapter(list[RunJournal]).validate_python(payload)

    def journals(self) -> list[RunJournal]:
        """Return full journals, preserving the original SDK method name."""
        return [self.run(item.run_id) for item in self.runs()]

    def artifacts(self, run_id: str) -> list[ArtifactDescriptor]:
        if not run_id.strip():
            raise ValueError("run_id must not be empty")
        payload = self._request_json("GET", f"/v1/runs/{quote(run_id, safe='')}/artifacts")
        return TypeAdapter(list[ArtifactDescriptor]).validate_python(payload)

    def artifact(self, artifact_id: str) -> ArtifactContentResponse:
        if not artifact_id.strip():
            raise ValueError("artifact_id must not be empty")
        return self._get_model(
            f"/v1/artifacts/{quote(artifact_id, safe='')}", ArtifactContentResponse
        )

    def checkpoints(self, run_id: str) -> list[Checkpoint]:
        if not run_id.strip():
            raise ValueError("run_id must not be empty")
        payload = self._request_json(
            "GET", f"/v1/runs/{quote(run_id, safe='')}/checkpoints"
        )
        return TypeAdapter(list[Checkpoint]).validate_python(payload)


    def connectors(self) -> list[ConnectorDescriptor]:
        payload = self._request_json("GET", "/v1/connectors")
        return TypeAdapter(list[ConnectorDescriptor]).validate_python(payload)

    def connector(self, provider: str) -> ConnectorDescriptor:
        provider = _provider_slug(provider)
        return self._get_model(f"/v1/connectors/{provider}", ConnectorDescriptor)

    def configure_connector(
        self, request: ConnectorConfigureRequest
    ) -> ConnectorDescriptor:
        payload = self._request_json(
            "POST", "/v1/connectors", body=request.model_dump(mode="json", exclude_none=True)
        )
        return ConnectorDescriptor.model_validate(payload)

    def disconnect_connector(self, provider: str) -> ConnectorDescriptor:
        provider = _provider_slug(provider)
        payload = self._request_json("DELETE", f"/v1/connectors/{provider}")
        return ConnectorDescriptor.model_validate(payload)

    def start_connector_oauth(
        self, provider: str, *, force_consent: bool = False
    ) -> OAuthStartResponse:
        slug = _provider_slug(provider)
        payload = self._request_json(
            "POST",
            f"/v1/connectors/{slug}/oauth/start",
            body={"provider": _provider_variant(slug), "force_consent": force_consent},
        )
        return OAuthStartResponse.model_validate(payload)

    def execute_connector_action(
        self, request: ConnectorActionRequest
    ) -> ConnectorActionResponse:
        slug = _provider_slug(request.provider)
        payload = self._request_json(
            "POST",
            f"/v1/connectors/{slug}/actions",
            body=request.model_dump(mode="json"),
        )
        return ConnectorActionResponse.model_validate(payload)

    def connector_audits(self, limit: int = 100) -> list[ConnectorAuditRecord]:
        payload = self._request_json(
            "GET", "/v1/connectors/audits", query={"limit": max(1, min(limit, 1000))}
        )
        return TypeAdapter(list[ConnectorAuditRecord]).validate_python(payload)

    def automations(self) -> list[AutomationDefinition]:
        payload = self._request_json("GET", "/v1/automations")
        return TypeAdapter(list[AutomationDefinition]).validate_python(payload)

    def automation(self, automation_id: str) -> AutomationDefinition:
        if not automation_id.strip():
            raise ValueError("automation_id must not be empty")
        return self._get_model(
            f"/v1/automations/{quote(automation_id, safe='')}", AutomationDefinition
        )

    def upsert_automation(
        self, request: AutomationUpsertRequest
    ) -> AutomationDefinition:
        payload = self._request_json(
            "POST", "/v1/automations", body=request.model_dump(mode="json", exclude_none=True)
        )
        return AutomationDefinition.model_validate(payload)

    def delete_automation(self, automation_id: str) -> None:
        if not automation_id.strip():
            raise ValueError("automation_id must not be empty")
        self._request_json("DELETE", f"/v1/automations/{quote(automation_id, safe='')}")

    def run_automation(
        self, automation_id: str, *, approval_granted: bool = False
    ) -> AutomationExecution:
        if not automation_id.strip():
            raise ValueError("automation_id must not be empty")
        payload = self._request_json(
            "POST",
            f"/v1/automations/{quote(automation_id, safe='')}/run",
            body={"approval_granted": approval_granted},
        )
        return AutomationExecution.model_validate(payload)

    def approve_automation_execution(
        self, automation_id: str, execution_id: str
    ) -> AutomationExecution:
        if not automation_id.strip():
            raise ValueError("automation_id must not be empty")
        if not execution_id.strip():
            raise ValueError("execution_id must not be empty")
        payload = self._request_json(
            "POST",
            f"/v1/automations/{quote(automation_id, safe='')}/executions/{quote(execution_id, safe='')}/approve",
        )
        return AutomationExecution.model_validate(payload)

    def automation_executions(
        self, automation_id: str, limit: int = 50
    ) -> list[AutomationExecution]:
        if not automation_id.strip():
            raise ValueError("automation_id must not be empty")
        payload = self._request_json(
            "GET",
            f"/v1/automations/{quote(automation_id, safe='')}/executions",
            query={"limit": max(1, min(limit, 500))},
        )
        return TypeAdapter(list[AutomationExecution]).validate_python(payload)

    def tools(self) -> list[ToolDefinition]:
        payload = self._request_json("GET", "/v1/tools")
        return TypeAdapter(list[ToolDefinition]).validate_python(payload)

    def skills(self) -> list[SkillDescriptor]:
        payload = self._request_json("GET", "/v1/skills")
        return TypeAdapter(list[SkillDescriptor]).validate_python(payload)

    def skill(self, skill_id: str) -> SkillDescriptor:
        if not skill_id.strip():
            raise ValueError("skill_id must not be empty")
        return self._get_model(
            f"/v1/skills/{quote(skill_id, safe='')}", SkillDescriptor
        )

    def refresh_skills(self) -> list[SkillDescriptor]:
        payload = self._request_json("POST", "/v1/skills/refresh")
        return TypeAdapter(list[SkillDescriptor]).validate_python(payload)

    def install_skill(self, source_path: str | Path) -> SkillDescriptor:
        path = Path(source_path).expanduser().resolve()
        payload = self._request_json(
            "POST", "/v1/skills/install", body={"source_path": str(path)}
        )
        return SkillDescriptor.model_validate(payload)

    def uninstall_skill(self, skill_id: str) -> bool:
        if not skill_id.strip():
            raise ValueError("skill_id must not be empty")
        payload = self._request_json(
            "DELETE", f"/v1/skills/{quote(skill_id, safe='')}"
        )
        return bool(payload.get("uninstalled"))

    def set_skill_enabled(self, skill_id: str, enabled: bool) -> SkillDescriptor:
        if not skill_id.strip():
            raise ValueError("skill_id must not be empty")
        action = "enable" if enabled else "disable"
        payload = self._request_json(
            "POST", f"/v1/skills/{quote(skill_id, safe='')}/{action}"
        )
        return SkillDescriptor.model_validate(payload)

    def execute_skill(
        self, skill_id: str, request: SkillExecutionRequest
    ) -> SkillExecutionResponse:
        if not skill_id.strip():
            raise ValueError("skill_id must not be empty")
        payload = self._request_json(
            "POST",
            f"/v1/skills/{quote(skill_id, safe='')}/execute",
            body=request.model_dump(mode="json"),
        )
        return SkillExecutionResponse.model_validate(payload)

    def search_memory(
        self,
        query: str = "",
        *,
        scope: str | None = None,
        workspace_key: str | None = None,
        limit: int = 20,
        include_superseded: bool = False,
    ) -> list[MemorySearchResult]:
        params: dict[str, str | int] = {
            "query": query,
            "limit": max(1, limit),
            "include_superseded": int(include_superseded),
        }
        if scope:
            params["scope"] = scope
        if workspace_key:
            params["workspace_key"] = workspace_key
        payload = self._request_json("GET", "/v1/memory", query=params)
        return TypeAdapter(list[MemorySearchResult]).validate_python(payload)

    def memory(self, memory_id: str) -> MemoryEntry:
        if not memory_id.strip():
            raise ValueError("memory_id must not be empty")
        return self._get_model(
            f"/v1/memory/{quote(memory_id, safe='')}", MemoryEntry
        )

    def remember(self, request: MemoryUpsertRequest) -> MemoryEntry:
        payload = self._request_json(
            "POST", "/v1/memory", body=request.model_dump(mode="json")
        )
        return MemoryEntry.model_validate(payload)

    def forget_memory(self, memory_id: str) -> bool:
        if not memory_id.strip():
            raise ValueError("memory_id must not be empty")
        payload = self._request_json(
            "DELETE", f"/v1/memory/{quote(memory_id, safe='')}"
        )
        return bool(payload.get("forgotten"))

    def supersede_memory(self, old_memory_id: str, new_memory_id: str) -> bool:
        if not old_memory_id.strip() or not new_memory_id.strip():
            raise ValueError("memory ids must not be empty")
        payload = self._request_json(
            "POST",
            f"/v1/memory/{quote(old_memory_id, safe='')}/supersede",
            body={"new_memory_id": new_memory_id},
        )
        return bool(payload.get("superseded"))

    def invoke_tool(
        self,
        invocation_id: str,
        request: ToolInvocationRequest,
    ) -> ToolInvocationResponse:
        if not invocation_id.strip():
            raise ValueError("invocation_id must not be empty")
        payload = self._request_json(
            "POST",
            f"/v1/tools/invocations/{quote(invocation_id, safe='')}",
            body=request.model_dump(mode="json"),
        )
        return ToolInvocationResponse.model_validate(payload)

    def tool_invocation(self, invocation_id: str) -> ToolInvocationRecord:
        if not invocation_id.strip():
            raise ValueError("invocation_id must not be empty")
        return self._get_model(
            f"/v1/tools/invocations/{quote(invocation_id, safe='')}",
            ToolInvocationRecord,
        )

    def model_invocation(self, invocation_id: str) -> ModelInvocationRecord:
        if not invocation_id.strip():
            raise ValueError("invocation_id must not be empty")
        return self._get_model(
            f"/v1/models/invocations/{quote(invocation_id, safe='')}",
            ModelInvocationRecord,
        )

    def model_invocations(self, run_id: str) -> list[ModelInvocationRecord]:
        if not run_id.strip():
            raise ValueError("run_id must not be empty")
        payload = self._request_json(
            "GET", f"/v1/runs/{quote(run_id, safe='')}/model-invocations"
        )
        return TypeAdapter(list[ModelInvocationRecord]).validate_python(payload)

    def tool_invocations(self, run_id: str) -> list[ToolInvocationRecord]:
        if not run_id.strip():
            raise ValueError("run_id must not be empty")
        payload = self._request_json(
            "GET", f"/v1/runs/{quote(run_id, safe='')}/tool-invocations"
        )
        return TypeAdapter(list[ToolInvocationRecord]).validate_python(payload)

    def cancel_tool(self, invocation_id: str) -> CancellationResponse:
        if not invocation_id.strip():
            raise ValueError("invocation_id must not be empty")
        payload = self._request_json(
            "POST",
            f"/v1/tools/invocations/{quote(invocation_id, safe='')}/cancel",
        )
        return CancellationResponse.model_validate(payload)

    def propose_edit(
        self,
        objective: str,
        mode: str = "balanced",
        skill_id: str | None = None,
    ) -> EditProposalResponse:
        if not objective.strip():
            raise ValueError("objective must not be empty")
        normalized_mode = self._normalize_mode(mode)
        request = EditProposalRequest(
            objective=objective.strip(),
            mode=normalized_mode,
            skill_id=skill_id.strip() if skill_id and skill_id.strip() else None,
        )
        payload = self._request_json(
            "POST",
            "/v1/edits/propose",
            body=request.model_dump(mode="json", exclude_none=True),
        )
        return EditProposalResponse.model_validate(payload)

    def execute_patch(self, request: PatchExecutionRequest) -> PatchExecutionResponse:
        if not request.objective.strip():
            raise ValueError("objective must not be empty")
        payload = self._request_json(
            "POST",
            "/v1/executions/patch",
            body=request.model_dump(mode="json"),
        )
        return PatchExecutionResponse.model_validate(payload)

    @staticmethod
    def _normalize_mode(mode: str) -> str:
        normalized = mode.strip().lower()
        modes = {"fast": "Fast", "balanced": "Balanced", "deep": "Deep"}
        try:
            return modes[normalized]
        except KeyError as error:
            raise ValueError("mode must be one of: fast, balanced, deep") from error

    def _get_model(
        self,
        path: str,
        model_type: type[ModelT],
        *,
        query: dict[str, str | int] | None = None,
    ) -> ModelT:
        payload = self._request_json("GET", path, query=query)
        return model_type.model_validate(payload)

    def _request_json(
        self,
        method: str,
        path: str,
        *,
        query: dict[str, str | int] | None = None,
        body: dict[str, Any] | None = None,
    ) -> Any:
        if not path.startswith("/"):
            raise ValueError("API path must start with '/'")

        url = f"{self.base_url}{path}"
        if query:
            url = f"{url}?{urlencode(query)}"

        encoded_body = None
        headers = {"Accept": "application/json"}
        if body is not None:
            encoded_body = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"

        request = Request(url, data=encoded_body, headers=headers, method=method)
        try:
            with urlopen(request, timeout=self.timeout) as response:
                raw = response.read().decode("utf-8")
        except HTTPError as error:
            raw = error.read().decode("utf-8", errors="replace")
            detail = _extract_error_message(raw) or error.reason
            raise EverythingClientError(
                f"Everything API returned HTTP {error.code}: {detail}",
                status_code=error.code,
                response_body=raw,
            ) from error
        except URLError as error:
            raise EverythingClientError(
                f"Could not connect to Everything daemon at {self.base_url}: {error.reason}"
            ) from error
        except TimeoutError as error:
            raise EverythingClientError(
                f"Everything API request timed out after {self.timeout:g} seconds"
            ) from error

        if not raw:
            return None
        try:
            return json.loads(raw)
        except json.JSONDecodeError as error:
            raise EverythingClientError(
                "Everything API returned invalid JSON",
                response_body=raw,
            ) from error


def _provider_slug(provider: str) -> str:
    normalized = provider.strip().lower().replace("_", "-")
    aliases = {"google-mail": "gmail", "meta-instagram": "instagram", "tik-tok": "tiktok"}
    normalized = aliases.get(normalized, normalized)
    if normalized not in {"gmail", "spotify", "instagram", "tiktok", "github"}:
        raise ValueError("provider must be gmail, spotify, instagram, tiktok, or github")
    return normalized


def _provider_variant(provider: str) -> str:
    return {"gmail": "Gmail", "spotify": "Spotify", "instagram": "Instagram", "tiktok": "TikTok", "github": "GitHub"}[provider]


def _extract_error_message(raw: str) -> str | None:
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError:
        return raw.strip() or None
    if isinstance(payload, dict) and isinstance(payload.get("error"), str):
        return payload["error"]
    return raw.strip() or None
