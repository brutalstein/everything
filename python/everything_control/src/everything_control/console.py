from __future__ import annotations

import argparse
import json
from pathlib import Path

from .client import EverythingClient, EverythingClientError
from .models import (
    AutomationUpsertRequest,
    ConnectorActionRequest,
    ConnectorConfigureRequest,
    CodeGraphChangeImpactRequest,
    MemoryUpsertRequest,
    PatchExecutionRequest,
    SkillExecutionRequest,
    ToolInvocationRequest,
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="everything-control")
    parser.add_argument("--workspace", default=".")
    parser.add_argument("--base-url", default="http://127.0.0.1:3472")
    parser.add_argument("--timeout", type=float, default=30.0)
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("info")
    subparsers.add_parser("doctor")
    subparsers.add_parser("models")
    subparsers.add_parser("model-capabilities")
    subparsers.add_parser("modules")
    subparsers.add_parser("graph")
    subparsers.add_parser("graph-index")
    subparsers.add_parser("graph-stats")
    graph_search = subparsers.add_parser("graph-search")
    graph_search.add_argument("term")
    graph_search.add_argument("--limit", type=int, default=20)
    graph_traverse = subparsers.add_parser("graph-traverse")
    graph_traverse.add_argument("term")
    graph_traverse.add_argument("--direction", choices=("inbound", "outbound", "both"), default="inbound")
    graph_traverse.add_argument("--depth", type=int, default=3)
    graph_traverse.add_argument("--limit", type=int, default=100)
    graph_traverse.add_argument("--relations", default="")
    graph_path = subparsers.add_parser("graph-path")
    graph_path.add_argument("source")
    graph_path.add_argument("target")
    graph_path.add_argument("--direction", choices=("inbound", "outbound", "both"), default="outbound")
    graph_path.add_argument("--depth", type=int, default=6)
    graph_path.add_argument("--relations", default="")

    graph_change_impact = subparsers.add_parser("graph-change-impact")
    graph_change_impact.add_argument("request", type=Path)

    subparsers.add_parser("research-status")
    research_search = subparsers.add_parser("research-search")
    research_search.add_argument("query")
    research_search.add_argument("--mode", choices=("general", "technical", "news", "academic"), default="technical")
    research_search.add_argument("--freshness", choices=("any", "day", "week", "month", "year"), default="any")
    research_search.add_argument("--max-results", type=int, default=12)
    research_search.add_argument("--fetch-pages", type=int, default=6)
    research_search.add_argument("--allow-domain", action="append", default=[])
    research_search.add_argument("--block-domain", action="append", default=[])
    research_search.add_argument("--force-refresh", action="store_true")
    research_fetch = subparsers.add_parser("research-fetch")
    research_fetch.add_argument("url")
    research_fetch.add_argument("--force-refresh", action="store_true")
    subparsers.add_parser("research-cache-purge")

    query = subparsers.add_parser("query")
    query.add_argument("term")

    impact = subparsers.add_parser("impact")
    impact.add_argument("term")
    impact.add_argument("--depth", type=int, default=None)

    plan = subparsers.add_parser("plan")
    plan.add_argument("--mode", choices=("fast", "balanced", "deep"), default="balanced")
    plan.add_argument("objective")

    subparsers.add_parser("runs")
    subparsers.add_parser("recoverable-runs")
    subparsers.add_parser("journals", help="Fetch complete journals for all runs")
    run = subparsers.add_parser("run")
    run.add_argument("run_id")
    artifacts = subparsers.add_parser("artifacts")
    artifacts.add_argument("run_id")
    checkpoints = subparsers.add_parser("checkpoints")
    checkpoints.add_argument("run_id")
    artifact = subparsers.add_parser("artifact")
    artifact.add_argument("artifact_id")
    subparsers.add_parser("tools")
    model_invocations = subparsers.add_parser("model-invocations")
    model_invocations.add_argument("run_id")
    model_invocation = subparsers.add_parser("model-invocation")
    model_invocation.add_argument("invocation_id")
    tool_invocations = subparsers.add_parser("tool-invocations")
    tool_invocations.add_argument("run_id")
    tool_invocation = subparsers.add_parser("tool-invocation")
    tool_invocation.add_argument("invocation_id")
    invoke_tool = subparsers.add_parser("invoke-tool")
    invoke_tool.add_argument("invocation_id")
    invoke_tool.add_argument("request", type=Path)
    cancel_tool = subparsers.add_parser("cancel-tool")
    cancel_tool.add_argument("invocation_id")
    propose_edit = subparsers.add_parser("propose-edit")
    propose_edit.add_argument("objective")
    propose_edit.add_argument(
        "--mode", choices=("fast", "balanced", "deep"), default="balanced"
    )
    propose_edit.add_argument("--skill")
    execute_patch = subparsers.add_parser("execute-patch")
    execute_patch.add_argument("request", type=Path)

    subparsers.add_parser("skills")
    skill = subparsers.add_parser("skill")
    skill.add_argument("skill_id")
    subparsers.add_parser("skills-refresh")
    skill_install = subparsers.add_parser("skill-install")
    skill_install.add_argument("path", type=Path)
    skill_uninstall = subparsers.add_parser("skill-uninstall")
    skill_uninstall.add_argument("skill_id")
    skill_enable = subparsers.add_parser("skill-enable")
    skill_enable.add_argument("skill_id")
    skill_disable = subparsers.add_parser("skill-disable")
    skill_disable.add_argument("skill_id")
    skill_run = subparsers.add_parser("skill-run")
    skill_run.add_argument("skill_id")
    skill_run.add_argument("input", type=Path)
    skill_run.add_argument("--approve", action="store_true")

    memory_search = subparsers.add_parser("memory-search")
    memory_search.add_argument("query", nargs="?", default="")
    memory_search.add_argument(
        "--scope",
        choices=("session", "workspace", "task", "artifact", "graph", "preference"),
    )
    memory_search.add_argument("--workspace-key")
    memory_search.add_argument("--limit", type=int, default=20)
    memory_search.add_argument("--include-superseded", action="store_true")
    memory_get = subparsers.add_parser("memory-get")
    memory_get.add_argument("memory_id")
    memory_upsert = subparsers.add_parser("memory-upsert")
    memory_upsert.add_argument("request", type=Path)
    memory_forget = subparsers.add_parser("memory-forget")
    memory_forget.add_argument("memory_id")
    memory_supersede = subparsers.add_parser("memory-supersede")
    memory_supersede.add_argument("old_memory_id")
    memory_supersede.add_argument("new_memory_id")

    subparsers.add_parser("connectors")
    connector = subparsers.add_parser("connector")
    connector.add_argument("provider", choices=("gmail", "spotify", "instagram", "tiktok", "github"))
    connector_configure = subparsers.add_parser("connector-configure")
    connector_configure.add_argument("request", type=Path)
    connector_disconnect = subparsers.add_parser("connector-disconnect")
    connector_disconnect.add_argument("provider", choices=("gmail", "spotify", "instagram", "tiktok", "github"))
    connector_oauth = subparsers.add_parser("connector-oauth")
    connector_oauth.add_argument("provider", choices=("gmail", "spotify", "instagram", "tiktok", "github"))
    connector_oauth.add_argument("--force-consent", action="store_true")
    connector_action = subparsers.add_parser("connector-action")
    connector_action.add_argument("request", type=Path)
    connector_audits = subparsers.add_parser("connector-audits")
    connector_audits.add_argument("--limit", type=int, default=100)

    subparsers.add_parser("automations")
    automation = subparsers.add_parser("automation")
    automation.add_argument("automation_id")
    automation_upsert = subparsers.add_parser("automation-upsert")
    automation_upsert.add_argument("request", type=Path)
    automation_delete = subparsers.add_parser("automation-delete")
    automation_delete.add_argument("automation_id")
    automation_run = subparsers.add_parser("automation-run")
    automation_run.add_argument("automation_id")
    automation_run.add_argument("--approve", action="store_true")
    automation_history = subparsers.add_parser("automation-history")
    automation_history.add_argument("automation_id")
    automation_history.add_argument("--limit", type=int, default=50)
    automation_approve = subparsers.add_parser("automation-approve")
    automation_approve.add_argument("automation_id")
    automation_approve.add_argument("execution_id")
    return parser


def main() -> None:
    args = build_parser().parse_args()
    client = EverythingClient(
        Path(args.workspace),
        base_url=args.base_url,
        timeout=args.timeout,
    )

    try:
        if args.command == "info":
            print_json(client.info().model_dump(mode="json"))
        elif args.command == "doctor":
            print_json(client.doctor().model_dump(mode="json"))
        elif args.command == "models":
            print_json([item.model_dump(mode="json") for item in client.models()])
        elif args.command == "model-capabilities":
            print_json(client.model_capabilities().model_dump(mode="json"))
        elif args.command == "modules":
            print_json([item.model_dump(mode="json") for item in client.modules()])
        elif args.command == "graph":
            print_json(client.graph().model_dump(mode="json"))
        elif args.command == "graph-index":
            print_json(client.graph_index().model_dump(mode="json"))
        elif args.command == "graph-stats":
            print_json(client.graph_stats().model_dump(mode="json"))
        elif args.command == "graph-search":
            print_json([item.model_dump(mode="json") for item in client.graph_search(args.term, args.limit)])
        elif args.command == "graph-traverse":
            relations = [value for value in args.relations.split(",") if value]
            print_json(
                client.graph_traverse(
                    args.term,
                    direction=args.direction,
                    depth=args.depth,
                    limit=args.limit,
                    relations=relations,
                ).model_dump(mode="json")
            )
        elif args.command == "graph-path":
            relations = [value for value in args.relations.split(",") if value]
            result = client.graph_path(
                args.source,
                args.target,
                direction=args.direction,
                depth=args.depth,
                relations=relations,
            )
            print_json(None if result is None else result.model_dump(mode="json"))
        elif args.command == "graph-change-impact":
            request = CodeGraphChangeImpactRequest.model_validate_json(args.request.read_text())
            print_json(client.graph_change_impact(request).model_dump(mode="json"))
        elif args.command == "research-status":
            print_json(client.research_status().model_dump(mode="json"))
        elif args.command == "research-search":
            print_json(client.research_search(
                args.query,
                mode=args.mode,
                freshness=args.freshness,
                max_results=args.max_results,
                fetch_pages=args.fetch_pages,
                allowed_domains=args.allow_domain,
                blocked_domains=args.block_domain,
                force_refresh=args.force_refresh,
            ).model_dump(mode="json"))
        elif args.command == "research-fetch":
            print_json(client.research_fetch(args.url, force_refresh=args.force_refresh).model_dump(mode="json"))
        elif args.command == "research-cache-purge":
            print_json({"removed": client.purge_research_cache()})
        elif args.command == "query":
            print_json(client.query(args.term).model_dump(mode="json"))
        elif args.command == "impact":
            print_json(client.impact(args.term, args.depth).model_dump(mode="json"))
        elif args.command == "plan":
            print_json(client.plan(args.objective, args.mode).model_dump(mode="json"))
        elif args.command == "runs":
            print_json([item.model_dump(mode="json") for item in client.runs()])
        elif args.command == "recoverable-runs":
            print_json(
                [item.model_dump(mode="json") for item in client.recoverable_runs()]
            )
        elif args.command == "journals":
            print_json([item.model_dump(mode="json") for item in client.journals()])
        elif args.command == "run":
            print_json(client.run(args.run_id).model_dump(mode="json"))
        elif args.command == "artifacts":
            print_json([item.model_dump(mode="json") for item in client.artifacts(args.run_id)])
        elif args.command == "checkpoints":
            print_json(
                [item.model_dump(mode="json") for item in client.checkpoints(args.run_id)]
            )
        elif args.command == "artifact":
            print_json(client.artifact(args.artifact_id).model_dump(mode="json"))
        elif args.command == "tools":
            print_json([item.model_dump(mode="json") for item in client.tools()])
        elif args.command == "model-invocations":
            print_json(
                [item.model_dump(mode="json") for item in client.model_invocations(args.run_id)]
            )
        elif args.command == "model-invocation":
            print_json(client.model_invocation(args.invocation_id).model_dump(mode="json"))
        elif args.command == "tool-invocations":
            print_json(
                [item.model_dump(mode="json") for item in client.tool_invocations(args.run_id)]
            )
        elif args.command == "tool-invocation":
            print_json(client.tool_invocation(args.invocation_id).model_dump(mode="json"))
        elif args.command == "invoke-tool":
            request = ToolInvocationRequest.model_validate_json(args.request.read_text())
            print_json(
                client.invoke_tool(args.invocation_id, request).model_dump(mode="json")
            )
        elif args.command == "cancel-tool":
            print_json(client.cancel_tool(args.invocation_id).model_dump(mode="json"))
        elif args.command == "propose-edit":
            print_json(
                client.propose_edit(args.objective, args.mode, args.skill).model_dump(mode="json")
            )
        elif args.command == "execute-patch":
            request = PatchExecutionRequest.model_validate_json(args.request.read_text())
            print_json(client.execute_patch(request).model_dump(mode="json"))
        elif args.command == "skills":
            print_json([item.model_dump(mode="json") for item in client.skills()])
        elif args.command == "skill":
            print_json(client.skill(args.skill_id).model_dump(mode="json"))
        elif args.command == "skills-refresh":
            print_json(
                [item.model_dump(mode="json") for item in client.refresh_skills()]
            )
        elif args.command == "skill-install":
            print_json(client.install_skill(args.path).model_dump(mode="json"))
        elif args.command == "skill-uninstall":
            print_json({"uninstalled": client.uninstall_skill(args.skill_id)})
        elif args.command == "skill-enable":
            print_json(
                client.set_skill_enabled(args.skill_id, True).model_dump(mode="json")
            )
        elif args.command == "skill-disable":
            print_json(
                client.set_skill_enabled(args.skill_id, False).model_dump(mode="json")
            )
        elif args.command == "skill-run":
            request = SkillExecutionRequest.model_validate_json(args.input.read_text())
            request.approval_granted = args.approve or request.approval_granted
            print_json(
                client.execute_skill(args.skill_id, request).model_dump(mode="json")
            )
        elif args.command == "memory-search":
            print_json(
                [
                    item.model_dump(mode="json")
                    for item in client.search_memory(
                        args.query,
                        scope=args.scope,
                        workspace_key=args.workspace_key,
                        limit=args.limit,
                        include_superseded=args.include_superseded,
                    )
                ]
            )
        elif args.command == "memory-get":
            print_json(client.memory(args.memory_id).model_dump(mode="json"))
        elif args.command == "memory-upsert":
            request = MemoryUpsertRequest.model_validate_json(args.request.read_text())
            print_json(client.remember(request).model_dump(mode="json"))
        elif args.command == "memory-forget":
            print_json({"forgotten": client.forget_memory(args.memory_id)})
        elif args.command == "memory-supersede":
            print_json(
                {
                    "superseded": client.supersede_memory(
                        args.old_memory_id, args.new_memory_id
                    )
                }
            )
        elif args.command == "connectors":
            print_json([item.model_dump(mode="json") for item in client.connectors()])
        elif args.command == "connector":
            print_json(client.connector(args.provider).model_dump(mode="json"))
        elif args.command == "connector-configure":
            request = ConnectorConfigureRequest.model_validate_json(args.request.read_text())
            print_json(client.configure_connector(request).model_dump(mode="json"))
        elif args.command == "connector-disconnect":
            print_json(client.disconnect_connector(args.provider).model_dump(mode="json"))
        elif args.command == "connector-oauth":
            print_json(
                client.start_connector_oauth(
                    args.provider, force_consent=args.force_consent
                ).model_dump(mode="json")
            )
        elif args.command == "connector-action":
            request = ConnectorActionRequest.model_validate_json(args.request.read_text())
            print_json(client.execute_connector_action(request).model_dump(mode="json"))
        elif args.command == "connector-audits":
            print_json(
                [item.model_dump(mode="json") for item in client.connector_audits(args.limit)]
            )
        elif args.command == "automations":
            print_json([item.model_dump(mode="json") for item in client.automations()])
        elif args.command == "automation":
            print_json(client.automation(args.automation_id).model_dump(mode="json"))
        elif args.command == "automation-upsert":
            request = AutomationUpsertRequest.model_validate_json(args.request.read_text())
            print_json(client.upsert_automation(request).model_dump(mode="json"))
        elif args.command == "automation-delete":
            client.delete_automation(args.automation_id)
            print_json({"deleted": True, "automation_id": args.automation_id})
        elif args.command == "automation-run":
            print_json(
                client.run_automation(
                    args.automation_id, approval_granted=args.approve
                ).model_dump(mode="json")
            )
        elif args.command == "automation-history":
            print_json(
                [
                    item.model_dump(mode="json")
                    for item in client.automation_executions(
                        args.automation_id, args.limit
                    )
                ]
            )
        elif args.command == "automation-approve":
            print_json(
                client.approve_automation_execution(
                    args.automation_id, args.execution_id
                ).model_dump(mode="json")
            )
    except EverythingClientError as error:
        raise SystemExit(str(error)) from error


def print_json(payload: object) -> None:
    print(json.dumps(payload, indent=2))
