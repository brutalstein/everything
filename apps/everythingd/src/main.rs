use anyhow::Result;
use axum::extract::{Path as RoutePath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use everything_domain::{
    AutomationDefinition, AutomationExecution, AutomationRunNowRequest, AutomationUpsertRequest,
    ConnectorActionRequest, ConnectorActionResponse, ConnectorAuditRecord,
    ConnectorConfigureRequest, ConnectorDescriptor, ConnectorProvider, EditProposalRequest,
    EditProposalResponse, ExecutionMode, InvocationId, MemoryEntry, MemoryQuery, MemoryScope,
    MemorySearchResult, MemoryUpsertRequest, ModelInvocationRecord, OAuthCallbackRequest,
    OAuthStartRequest, OAuthStartResponse, PatchExecutionRequest, PatchExecutionResponse,
    PlanResponse, RunJournal, ServiceEvent, SkillDescriptor, SkillExecutionRequest,
    SkillExecutionResponse, SkillInstallRequest, ToolDefinition, ToolInvocationRecord,
    ToolInvocationRequest, ToolInvocationResponse, WebFetchRequest, WebSearchRequest,
};
use everything_runtime::{
    ModularRuntime, build_runtime_with_oauth_port, build_task_request_for_workspace,
};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, broadcast};
use tokio_stream::wrappers::BroadcastStream;

#[derive(clap::Parser)]
#[command(name = "everythingd")]
#[command(version)]
#[command(about = "Typed control-plane service for the Everything runtime.")]
struct Cli {
    #[arg(long, default_value = ".")]
    workspace: PathBuf,

    #[arg(long, default_value = "127.0.0.1:3472")]
    listen: SocketAddr,

    #[arg(long, default_value = "127.0.0.1:43821")]
    oauth_listen: SocketAddr,
}

#[derive(Clone)]
struct AppState {
    runtime: Arc<ModularRuntime>,
    workspace: PathBuf,
    events: broadcast::Sender<ServiceEvent>,
    sequence: Arc<AtomicU64>,
}

#[derive(Debug, Serialize)]
struct ServiceInfo {
    service: &'static str,
    workspace: PathBuf,
    data_dir: PathBuf,
    model: String,
}

#[derive(Debug, Deserialize)]
struct QueryParams {
    term: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ImpactParams {
    term: String,
    depth: Option<usize>,
    limit: Option<usize>,
    direction: Option<String>,
    relations: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodePathParams {
    from: String,
    to: String,
    depth: Option<usize>,
    direction: Option<String>,
    relations: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlanRequestBody {
    objective: String,
    mode: ExecutionMode,
}

#[derive(Debug, Deserialize)]
struct BenchmarkRequestBody {
    iterations: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MemoryQueryParams {
    query: Option<String>,
    scope: Option<String>,
    workspace_key: Option<String>,
    limit: Option<usize>,
    include_superseded: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct ConnectorAuditParams {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct OAuthCallbackQuery {
    state: String,
    code: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AutomationHistoryParams {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MemorySupersedeRequest {
    new_memory_id: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = <Cli as clap::Parser>::parse();
    let listener = TcpListener::bind(cli.listen).await?;
    let actual_listen = listener.local_addr()?;
    let oauth_listener = if cli.oauth_listen != cli.listen {
        Some(TcpListener::bind(cli.oauth_listen).await?)
    } else {
        None
    };
    let actual_oauth = match oauth_listener.as_ref() {
        Some(listener) => listener.local_addr()?,
        None => actual_listen,
    };
    let runtime = Arc::new(build_runtime_with_oauth_port(
        &cli.workspace,
        actual_oauth.port(),
    )?);
    let (event_tx, _) = broadcast::channel(256);
    let state = AppState {
        runtime: runtime.clone(),
        workspace: cli.workspace.canonicalize()?,
        events: event_tx,
        sequence: Arc::new(AtomicU64::new(0)),
    };

    let app = Router::new()
        .route("/v1/info", get(info))
        .route("/v1/doctor", get(doctor))
        .route("/v1/models/capabilities", get(model_capabilities))
        .route("/v1/models", get(models))
        .route("/v1/modules", get(modules))
        .route("/v1/graph", get(graph))
        .route("/v1/query", get(query))
        .route("/v1/impact", get(impact))
        .route("/v1/plan", post(plan))
        .route("/v1/runs", get(runs))
        .route("/v1/runs/recoverable", get(recoverable_runs))
        .route("/v1/runs/{run_id}", get(run_by_id))
        .route("/v1/runs/{run_id}/artifacts", get(run_artifacts))
        .route("/v1/runs/{run_id}/checkpoints", get(run_checkpoints))
        .route(
            "/v1/runs/{run_id}/tool-invocations",
            get(run_tool_invocations),
        )
        .route(
            "/v1/runs/{run_id}/model-invocations",
            get(run_model_invocations),
        )
        .route("/v1/tools", get(tools))
        .route(
            "/v1/models/invocations/{invocation_id}",
            get(model_invocation_by_id),
        )
        .route(
            "/v1/tools/invocations/{invocation_id}",
            get(tool_invocation_by_id).post(invoke_tool),
        )
        .route(
            "/v1/tools/invocations/{invocation_id}/cancel",
            post(cancel_tool_invocation),
        )
        .route("/v1/edits/propose", post(propose_edit))
        .route("/v1/executions/patch", post(execute_patch))
        .route("/v1/skills", get(skills))
        .route("/v1/skills/refresh", post(refresh_skills))
        .route("/v1/skills/install", post(install_skill))
        .route(
            "/v1/skills/{skill_id}",
            get(skill_by_id).delete(uninstall_skill),
        )
        .route("/v1/skills/{skill_id}/enable", post(enable_skill))
        .route("/v1/skills/{skill_id}/disable", post(disable_skill))
        .route("/v1/skills/{skill_id}/execute", post(execute_skill))
        .route("/v1/memory", get(search_memory).post(upsert_memory))
        .route(
            "/v1/memory/{memory_id}",
            get(memory_by_id).delete(forget_memory),
        )
        .route("/v1/memory/{memory_id}/supersede", post(supersede_memory))
        .route("/v1/artifacts/{artifact_id}", get(artifact_by_id))
        .route("/v1/metrics", get(metrics))
        .route("/v1/benchmarks/bootstrap", post(benchmark_bootstrap))
        .route("/v1/code-graph/index", post(code_graph_index))
        .route("/v1/code-graph/stats", get(code_graph_stats))
        .route("/v1/code-graph/search", get(code_graph_search))
        .route("/v1/code-graph/impact", get(code_graph_impact))
        .route(
            "/v1/code-graph/change-impact",
            post(code_graph_change_impact),
        )
        .route("/v1/code-graph/path", get(code_graph_path))
        .route("/v1/research/status", get(research_status))
        .route("/v1/research/search", post(research_search))
        .route("/v1/research/fetch", post(research_fetch))
        .route("/v1/research/cache/purge", post(research_cache_purge))
        .route("/v1/graph/index", post(code_graph_index))
        .route("/v1/graph/stats", get(code_graph_stats))
        .route("/v1/graph/search", get(code_graph_search))
        .route("/v1/graph/traverse", get(code_graph_impact))
        .route("/v1/graph/change-impact", post(code_graph_change_impact))
        .route("/v1/graph/path", get(code_graph_path))
        .route("/v1/connectors", get(connectors).post(configure_connector))
        .route("/v1/connectors/audits", get(connector_audits))
        .route(
            "/v1/connectors/{provider}",
            get(connector_by_provider).delete(disconnect_connector),
        )
        .route(
            "/v1/connectors/{provider}/oauth/start",
            post(start_connector_oauth),
        )
        .route(
            "/v1/connectors/{provider}/actions",
            post(execute_connector_action),
        )
        .route(
            "/v1/connectors/oauth/callback/{provider}",
            get(connector_oauth_callback),
        )
        .route("/v1/automations", get(automations).post(upsert_automation))
        .route(
            "/v1/automations/{automation_id}",
            get(automation_by_id).delete(delete_automation),
        )
        .route(
            "/v1/automations/{automation_id}/run",
            post(run_automation_now),
        )
        .route(
            "/v1/automations/{automation_id}/executions",
            get(automation_executions),
        )
        .route(
            "/v1/automations/{automation_id}/executions/{execution_id}/approve",
            post(approve_automation_execution),
        )
        .route("/v1/events", get(events))
        .with_state(state.clone());

    emit_service_event(
        &state,
        "service.started",
        None,
        None,
        format!(
            "listen={} oauth_listen={} workspace={}",
            actual_listen,
            actual_oauth,
            state.workspace.display()
        ),
    );
    spawn_code_graph_warmup(state.clone());
    spawn_automation_scheduler(state.clone());
    if let Some(oauth_listener) = oauth_listener {
        let oauth_app = Router::new()
            .route(
                "/v1/connectors/oauth/callback/{provider}",
                get(connector_oauth_callback),
            )
            .with_state(state.clone());
        tokio::spawn(async move {
            if let Err(error) = axum::serve(oauth_listener, oauth_app).await {
                eprintln!("Everything OAuth callback server stopped: {error}");
            }
        });
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(state))
        .await?;
    Ok(())
}

async fn shutdown_signal(state: AppState) {
    let _ = tokio::signal::ctrl_c().await;
    emit_service_event(&state, "service.stopped", None, None, "shutdown".to_owned());
}

async fn info(State(state): State<AppState>) -> Json<ServiceInfo> {
    Json(ServiceInfo {
        service: "everythingd",
        workspace: state.workspace.clone(),
        data_dir: state.runtime.settings().data_dir.clone(),
        model: state.runtime.components().model.name().to_owned(),
    })
}

async fn model_capabilities(
    State(state): State<AppState>,
) -> Json<everything_domain::ModelCapabilityProfile> {
    Json(state.runtime.model_capability_profile())
}

async fn models(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<everything_domain::DiscoveredModel>>> {
    let runtime = state.runtime.clone();
    let models = tokio::task::spawn_blocking(move || runtime.discover_models())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(models))
}

async fn doctor(
    State(state): State<AppState>,
) -> ApiResult<Json<everything_domain::RuntimeDoctorReport>> {
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let report = tokio::task::spawn_blocking(move || runtime.doctor(&workspace))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(report))
}

async fn modules(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<everything_domain::ModuleDescriptor>>> {
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let report = tokio::task::spawn_blocking(move || runtime.bootstrap(&workspace))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(report.modules))
}

async fn graph(
    State(state): State<AppState>,
) -> ApiResult<Json<everything_domain::GraphSummaryResponse>> {
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let summary = tokio::task::spawn_blocking(move || runtime.graph_summary(&workspace))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(summary))
}

async fn query(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> ApiResult<Json<everything_graph::GraphQueryResult>> {
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let term = params.term;
    let limit = params.limit.unwrap_or(20);
    let result =
        tokio::task::spawn_blocking(move || runtime.compatibility_query(&workspace, &term, limit))
            .await
            .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn impact(
    State(state): State<AppState>,
    Query(params): Query<ImpactParams>,
) -> ApiResult<Json<everything_graph::GraphImpactReport>> {
    let depth = params
        .depth
        .unwrap_or(state.runtime.settings().graph.max_impact_depth);
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let term = params.term;
    let limit = params.limit.unwrap_or(100);
    let result = tokio::task::spawn_blocking(move || {
        runtime.compatibility_impact(&workspace, &term, depth, limit)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn plan(
    State(state): State<AppState>,
    Json(payload): Json<PlanRequestBody>,
) -> ApiResult<Json<PlanResponse>> {
    if payload.objective.trim().is_empty() {
        return Err(ApiError::bad_request("objective must not be empty"));
    }

    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let event_state = state.clone();
    let objective = payload.objective;
    let mode = payload.mode;

    let response = tokio::task::spawn_blocking(move || -> Result<PlanResponse> {
        let request = build_task_request_for_workspace(&workspace, objective, mode)?;
        let (document, journal, journal_path) =
            runtime.plan_and_record_with_events(&request, |run_id, event| {
                emit_service_event(
                    &event_state,
                    "run.event",
                    Some(run_id),
                    Some(&event.stage),
                    event.detail.clone(),
                );
            })?;

        Ok(PlanResponse {
            run_id: journal.run_id.clone(),
            document,
            journal_path,
            artifact: journal.artifacts.first().cloned(),
        })
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;

    Ok(Json(response))
}

async fn runs(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<everything_domain::RunSummary>>> {
    let runtime = state.runtime.clone();
    let runs = tokio::task::spawn_blocking(move || runtime.list_runs())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(runs))
}

async fn recoverable_runs(State(state): State<AppState>) -> ApiResult<Json<Vec<RunJournal>>> {
    let runtime = state.runtime.clone();
    let runs = tokio::task::spawn_blocking(move || runtime.recoverable_runs())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(runs))
}

async fn run_by_id(
    State(state): State<AppState>,
    RoutePath(run_id): RoutePath<String>,
) -> ApiResult<Json<RunJournal>> {
    let runtime = state.runtime.clone();
    let requested_run_id = run_id.clone();
    tokio::task::spawn_blocking(move || runtime.get_run(&requested_run_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("run '{run_id}' not found")))
}

async fn run_artifacts(
    State(state): State<AppState>,
    RoutePath(run_id): RoutePath<String>,
) -> ApiResult<Json<Vec<everything_domain::ArtifactDescriptor>>> {
    let runtime = state.runtime.clone();
    let target_run_id = run_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<_> {
        let exists = runtime.get_run(&target_run_id)?.is_some();
        let artifacts = if exists {
            runtime.list_artifacts(&target_run_id)?
        } else {
            Vec::new()
        };
        Ok((exists, artifacts))
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    if !result.0 {
        return Err(ApiError::not_found(format!("run '{run_id}' not found")));
    }
    Ok(Json(result.1))
}

async fn run_checkpoints(
    State(state): State<AppState>,
    RoutePath(run_id): RoutePath<String>,
) -> ApiResult<Json<Vec<everything_domain::Checkpoint>>> {
    let runtime = state.runtime.clone();
    let target_run_id = run_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<_> {
        let exists = runtime.get_run(&target_run_id)?.is_some();
        let checkpoints = if exists {
            runtime.list_checkpoints(&target_run_id)?
        } else {
            Vec::new()
        };
        Ok((exists, checkpoints))
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    if !result.0 {
        return Err(ApiError::not_found(format!("run '{run_id}' not found")));
    }
    Ok(Json(result.1))
}

async fn tools(State(state): State<AppState>) -> Json<Vec<ToolDefinition>> {
    Json(state.runtime.tool_definitions())
}

async fn invoke_tool(
    State(state): State<AppState>,
    RoutePath(invocation_id): RoutePath<String>,
    Json(payload): Json<ToolInvocationRequest>,
) -> ApiResult<Json<ToolInvocationResponse>> {
    if invocation_id.trim().is_empty() {
        return Err(ApiError::bad_request("invocation_id must not be empty"));
    }
    if payload.tool_id.trim().is_empty() {
        return Err(ApiError::bad_request("tool_id must not be empty"));
    }
    let runtime = state.runtime.clone();
    let target_run_id = payload.run_id.to_string();
    let response = tokio::task::spawn_blocking(move || {
        runtime.invoke_tool(InvocationId::new(invocation_id), payload)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?
    .map_err(|error| {
        if error.to_string().contains("not found") {
            ApiError::not_found(error.to_string())
        } else {
            ApiError::bad_request(error.to_string())
        }
    })?;
    emit_service_event(
        &state,
        "tool.invocation",
        Some(&target_run_id),
        Some("tool"),
        format!(
            "tool={} status={:?}",
            response.invocation.tool_name, response.invocation.status
        ),
    );
    Ok(Json(response))
}

async fn tool_invocation_by_id(
    State(state): State<AppState>,
    RoutePath(invocation_id): RoutePath<String>,
) -> ApiResult<Json<ToolInvocationRecord>> {
    let runtime = state.runtime.clone();
    let requested = invocation_id.clone();
    tokio::task::spawn_blocking(move || runtime.get_tool_invocation(&requested))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("tool invocation '{invocation_id}' not found")))
}

async fn model_invocation_by_id(
    State(state): State<AppState>,
    RoutePath(invocation_id): RoutePath<String>,
) -> ApiResult<Json<ModelInvocationRecord>> {
    let runtime = state.runtime.clone();
    let requested = invocation_id.clone();
    tokio::task::spawn_blocking(move || runtime.get_model_invocation(&requested))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("model invocation '{invocation_id}' not found")))
}

async fn run_model_invocations(
    State(state): State<AppState>,
    RoutePath(run_id): RoutePath<String>,
) -> ApiResult<Json<Vec<ModelInvocationRecord>>> {
    let runtime = state.runtime.clone();
    let requested = run_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<_> {
        let exists = runtime.get_run(&requested)?.is_some();
        let invocations = if exists {
            runtime.list_model_invocations(&requested)?
        } else {
            Vec::new()
        };
        Ok((exists, invocations))
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    if !result.0 {
        return Err(ApiError::not_found(format!("run '{run_id}' not found")));
    }
    Ok(Json(result.1))
}

async fn run_tool_invocations(
    State(state): State<AppState>,
    RoutePath(run_id): RoutePath<String>,
) -> ApiResult<Json<Vec<ToolInvocationRecord>>> {
    let runtime = state.runtime.clone();
    let requested = run_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<_> {
        let exists = runtime.get_run(&requested)?.is_some();
        let invocations = if exists {
            runtime.list_tool_invocations(&requested)?
        } else {
            Vec::new()
        };
        Ok((exists, invocations))
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    if !result.0 {
        return Err(ApiError::not_found(format!("run '{run_id}' not found")));
    }
    Ok(Json(result.1))
}

#[derive(Debug, Serialize)]
struct CancellationResponse {
    invocation_id: String,
    cancelled: bool,
}

async fn cancel_tool_invocation(
    State(state): State<AppState>,
    RoutePath(invocation_id): RoutePath<String>,
) -> Json<CancellationResponse> {
    let cancelled = state.runtime.cancel_tool_invocation(&invocation_id);
    Json(CancellationResponse {
        invocation_id,
        cancelled,
    })
}

async fn propose_edit(
    State(state): State<AppState>,
    Json(payload): Json<EditProposalRequest>,
) -> ApiResult<Json<EditProposalResponse>> {
    if payload.objective.trim().is_empty() {
        return Err(ApiError::bad_request("objective must not be empty"));
    }
    let runtime = state.runtime.clone();
    let response = tokio::task::spawn_blocking(move || runtime.propose_edit(&payload))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    emit_service_event(
        &state,
        "edit.proposal.ready",
        Some(response.run_id.as_str()),
        Some("edit-proposal"),
        format!(
            "path={} status={}",
            response.patch.relative_path.display(),
            response.status
        ),
    );
    Ok(Json(response))
}

async fn execute_patch(
    State(state): State<AppState>,
    Json(payload): Json<PatchExecutionRequest>,
) -> ApiResult<Json<PatchExecutionResponse>> {
    if payload.objective.trim().is_empty() {
        return Err(ApiError::bad_request("objective must not be empty"));
    }
    let runtime = state.runtime.clone();
    let response = tokio::task::spawn_blocking(move || runtime.execute_patch(&payload))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    emit_service_event(
        &state,
        "execution.patch.completed",
        Some(response.run_id.as_str()),
        Some("execution"),
        format!(
            "status={} rolled_back={}",
            response.status, response.rolled_back
        ),
    );
    Ok(Json(response))
}

async fn skills(State(state): State<AppState>) -> ApiResult<Json<Vec<SkillDescriptor>>> {
    let runtime = state.runtime.clone();
    let skills = tokio::task::spawn_blocking(move || runtime.list_skills())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(skills))
}

async fn refresh_skills(State(state): State<AppState>) -> ApiResult<Json<Vec<SkillDescriptor>>> {
    let runtime = state.runtime.clone();
    let descriptors = tokio::task::spawn_blocking(move || runtime.refresh_skills())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(descriptors))
}

async fn install_skill(
    State(state): State<AppState>,
    Json(payload): Json<SkillInstallRequest>,
) -> ApiResult<Json<SkillDescriptor>> {
    if payload.source_path.as_os_str().is_empty() {
        return Err(ApiError::bad_request("source_path must not be empty"));
    }
    let runtime = state.runtime.clone();
    let descriptor =
        tokio::task::spawn_blocking(move || runtime.install_skill(&payload.source_path))
            .await
            .map_err(|error| ApiError::internal(error.to_string()))?
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    emit_service_event(
        &state,
        "skill.installed",
        None,
        Some("skills"),
        format!(
            "skill={} version={}",
            descriptor.manifest.skill_id, descriptor.manifest.version
        ),
    );
    Ok(Json(descriptor))
}

async fn uninstall_skill(
    State(state): State<AppState>,
    RoutePath(skill_id): RoutePath<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let runtime = state.runtime.clone();
    let requested = skill_id.clone();
    let removed = tokio::task::spawn_blocking(move || runtime.uninstall_skill(&requested))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !removed {
        return Err(ApiError::not_found(format!("skill '{skill_id}' not found")));
    }
    emit_service_event(
        &state,
        "skill.uninstalled",
        None,
        Some("skills"),
        format!("skill={skill_id}"),
    );
    Ok(Json(serde_json::json!({"uninstalled": true})))
}

async fn skill_by_id(
    State(state): State<AppState>,
    RoutePath(skill_id): RoutePath<String>,
) -> ApiResult<Json<SkillDescriptor>> {
    let runtime = state.runtime.clone();
    tokio::task::spawn_blocking(move || runtime.get_skill(&skill_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??
        .map(Json)
        .ok_or_else(|| ApiError::not_found("skill not found"))
}

async fn enable_skill(
    State(state): State<AppState>,
    RoutePath(skill_id): RoutePath<String>,
) -> ApiResult<Json<SkillDescriptor>> {
    set_skill_enabled(state, skill_id, true).await
}

async fn disable_skill(
    State(state): State<AppState>,
    RoutePath(skill_id): RoutePath<String>,
) -> ApiResult<Json<SkillDescriptor>> {
    set_skill_enabled(state, skill_id, false).await
}

async fn set_skill_enabled(
    state: AppState,
    skill_id: String,
    enabled: bool,
) -> ApiResult<Json<SkillDescriptor>> {
    let runtime = state.runtime.clone();
    let descriptor =
        tokio::task::spawn_blocking(move || runtime.set_skill_enabled(&skill_id, enabled))
            .await
            .map_err(|error| ApiError::internal(error.to_string()))?
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(descriptor))
}

async fn execute_skill(
    State(state): State<AppState>,
    RoutePath(skill_id): RoutePath<String>,
    Json(payload): Json<SkillExecutionRequest>,
) -> ApiResult<Json<SkillExecutionResponse>> {
    let runtime = state.runtime.clone();
    let response = tokio::task::spawn_blocking(move || runtime.execute_skill(&skill_id, payload))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(response))
}

async fn search_memory(
    State(state): State<AppState>,
    Query(params): Query<MemoryQueryParams>,
) -> ApiResult<Json<Vec<MemorySearchResult>>> {
    let scope = params
        .scope
        .as_deref()
        .map(parse_memory_scope)
        .transpose()?;
    let query = MemoryQuery {
        query: params.query.unwrap_or_default(),
        scope,
        workspace_key: params.workspace_key,
        limit: params.limit.unwrap_or(20),
        include_superseded: params.include_superseded.unwrap_or(false),
    };
    let runtime = state.runtime.clone();
    let results = tokio::task::spawn_blocking(move || runtime.search_memory(&query))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(results))
}

async fn upsert_memory(
    State(state): State<AppState>,
    Json(payload): Json<MemoryUpsertRequest>,
) -> ApiResult<Json<MemoryEntry>> {
    let runtime = state.runtime.clone();
    let entry = tokio::task::spawn_blocking(move || runtime.remember(payload))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(entry))
}

async fn memory_by_id(
    State(state): State<AppState>,
    RoutePath(memory_id): RoutePath<String>,
) -> ApiResult<Json<MemoryEntry>> {
    let runtime = state.runtime.clone();
    tokio::task::spawn_blocking(move || runtime.get_memory(&memory_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??
        .map(Json)
        .ok_or_else(|| ApiError::not_found("memory not found"))
}

async fn forget_memory(
    State(state): State<AppState>,
    RoutePath(memory_id): RoutePath<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let runtime = state.runtime.clone();
    let forgotten = tokio::task::spawn_blocking(move || runtime.forget_memory(&memory_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(serde_json::json!({"forgotten": forgotten})))
}

async fn supersede_memory(
    State(state): State<AppState>,
    RoutePath(memory_id): RoutePath<String>,
    Json(payload): Json<MemorySupersedeRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    if payload.new_memory_id.trim().is_empty() {
        return Err(ApiError::bad_request("new_memory_id must not be empty"));
    }
    let runtime = state.runtime.clone();
    let new_memory_id = payload.new_memory_id;
    tokio::task::spawn_blocking(move || runtime.supersede_memory(&memory_id, &new_memory_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(serde_json::json!({"superseded": true})))
}

async fn artifact_by_id(
    State(state): State<AppState>,
    RoutePath(artifact_id): RoutePath<String>,
) -> ApiResult<Json<everything_domain::ArtifactContentResponse>> {
    let runtime = state.runtime.clone();
    let requested_artifact_id = artifact_id.clone();
    tokio::task::spawn_blocking(move || runtime.get_artifact(&requested_artifact_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("artifact '{artifact_id}' not found")))
}

async fn metrics(
    State(state): State<AppState>,
) -> ApiResult<Json<everything_domain::RuntimeMetricsSnapshot>> {
    let runtime = state.runtime.clone();
    let metrics = tokio::task::spawn_blocking(move || runtime.metrics())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(metrics))
}

async fn benchmark_bootstrap(
    State(state): State<AppState>,
    Json(payload): Json<BenchmarkRequestBody>,
) -> ApiResult<Json<everything_domain::BenchmarkRecord>> {
    let iterations = payload.iterations.unwrap_or(3).max(1);
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let record =
        tokio::task::spawn_blocking(move || runtime.benchmark_bootstrap(&workspace, iterations))
            .await
            .map_err(|error| ApiError::internal(error.to_string()))??;
    emit_service_event(
        &state,
        "benchmark.completed",
        None,
        Some("bootstrap"),
        format!(
            "iterations={} mean_ms={:.2} p95_ms={}",
            record.iterations, record.mean_millis, record.p95_millis
        ),
    );
    Ok(Json(record))
}

async fn code_graph_index(
    State(state): State<AppState>,
) -> ApiResult<Json<everything_graph::CodeGraphIndexReport>> {
    emit_service_event(
        &state,
        "code_graph.index.started",
        None,
        Some("code_graph"),
        format!("workspace={}", state.workspace.display()),
    );
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();
    let report = tokio::task::spawn_blocking(move || runtime.refresh_code_graph(&workspace))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    emit_service_event(
        &state,
        "code_graph.index.completed",
        None,
        Some("code_graph"),
        format!(
            "scanned={} changed={} total_ms={}",
            report.scanned_files, report.changed_files, report.total_millis
        ),
    );
    Ok(Json(report))
}

async fn code_graph_stats(
    State(state): State<AppState>,
) -> ApiResult<Json<everything_graph::PersistentGraphStats>> {
    let runtime = state.runtime.clone();
    let stats = tokio::task::spawn_blocking(move || runtime.persistent_graph_stats())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(stats))
}

async fn code_graph_search(
    State(state): State<AppState>,
    Query(params): Query<QueryParams>,
) -> ApiResult<Json<Vec<everything_graph::CodeGraphSearchResult>>> {
    let runtime = state.runtime.clone();
    let term = params.term;
    let limit = params.limit.unwrap_or(20);
    let matches = tokio::task::spawn_blocking(move || runtime.code_search(&term, limit))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(matches))
}

async fn code_graph_impact(
    State(state): State<AppState>,
    Query(params): Query<ImpactParams>,
) -> ApiResult<Json<everything_graph::CodeGraphImpactReport>> {
    let runtime = state.runtime.clone();
    let term = params.term;
    let depth = params
        .depth
        .unwrap_or(state.runtime.settings().graph.max_impact_depth);
    let limit = params.limit.unwrap_or(100);
    let direction = parse_graph_direction(
        params.direction.as_deref(),
        everything_graph::GraphDirection::Inbound,
    )?;
    let relation_kinds = parse_relation_kinds(params.relations.as_deref())?;
    let impact = tokio::task::spawn_blocking(move || {
        runtime.code_traverse(&term, direction, &relation_kinds, depth, limit)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(impact))
}

async fn code_graph_change_impact(
    State(state): State<AppState>,
    Json(request): Json<everything_graph::CodeGraphChangeImpactRequest>,
) -> ApiResult<Json<everything_graph::CodeGraphChangeImpactReport>> {
    if request.targets.is_empty() {
        return Err(ApiError::bad_request(
            "at least one change target is required",
        ));
    }
    if request.targets.len() > 64 {
        return Err(ApiError::bad_request(
            "at most 64 change targets are allowed",
        ));
    }
    let runtime = state.runtime.clone();
    let impact = tokio::task::spawn_blocking(move || runtime.analyze_code_change(&request))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(impact))
}

async fn code_graph_path(
    State(state): State<AppState>,
    Query(params): Query<CodePathParams>,
) -> ApiResult<Json<Option<everything_graph::CodeGraphPath>>> {
    let runtime = state.runtime.clone();
    let from = params.from;
    let to = params.to;
    let depth = params
        .depth
        .unwrap_or(state.runtime.settings().graph.max_impact_depth);
    let direction = parse_graph_direction(
        params.direction.as_deref(),
        everything_graph::GraphDirection::Outbound,
    )?;
    let relation_kinds = parse_relation_kinds(params.relations.as_deref())?;
    let path = tokio::task::spawn_blocking(move || {
        runtime.code_path_with_options(&from, &to, depth, direction, &relation_kinds)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(path))
}

async fn research_status(
    State(state): State<AppState>,
) -> ApiResult<Json<everything_domain::ResearchStatus>> {
    let runtime = state.runtime.clone();
    let status = tokio::task::spawn_blocking(move || runtime.research_status())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(status))
}

async fn research_search(
    State(state): State<AppState>,
    Json(request): Json<WebSearchRequest>,
) -> ApiResult<Json<everything_domain::ResearchReport>> {
    let runtime = state.runtime.clone();
    let report = tokio::task::spawn_blocking(move || runtime.web_search(&request))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    emit_service_event(
        &state,
        "research.completed",
        None,
        Some("research"),
        format!(
            "query={} sources={} providers={} cache_hits={} search_ms={} fetch_ms={}",
            report.normalized_query,
            report.sources.len(),
            report.provider_count,
            report.cache_hits,
            report.search_millis,
            report.fetch_millis
        ),
    );
    Ok(Json(report))
}

async fn research_fetch(
    State(state): State<AppState>,
    Json(request): Json<WebFetchRequest>,
) -> ApiResult<Json<everything_domain::WebFetchResponse>> {
    let runtime = state.runtime.clone();
    let response = tokio::task::spawn_blocking(move || runtime.web_fetch(&request))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok(Json(response))
}

async fn research_cache_purge(State(state): State<AppState>) -> ApiResult<Json<serde_json::Value>> {
    let runtime = state.runtime.clone();
    let removed = tokio::task::spawn_blocking(move || runtime.purge_research_cache())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(serde_json::json!({"removed": removed})))
}

async fn connectors(State(state): State<AppState>) -> ApiResult<Json<Vec<ConnectorDescriptor>>> {
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.list_connectors())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn connector_by_provider(
    State(state): State<AppState>,
    RoutePath(provider): RoutePath<String>,
) -> ApiResult<Json<ConnectorDescriptor>> {
    let provider = parse_connector_provider(&provider)?;
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.get_connector(provider))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn configure_connector(
    State(state): State<AppState>,
    Json(request): Json<ConnectorConfigureRequest>,
) -> ApiResult<Json<ConnectorDescriptor>> {
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.configure_connector(request))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    emit_service_event(
        &state,
        "connector.configured",
        None,
        Some("connectors"),
        format!("provider={}", result.provider.as_str()),
    );
    Ok(Json(result))
}

async fn disconnect_connector(
    State(state): State<AppState>,
    RoutePath(provider): RoutePath<String>,
) -> ApiResult<Json<ConnectorDescriptor>> {
    let provider = parse_connector_provider(&provider)?;
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.disconnect_connector(provider))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    emit_service_event(
        &state,
        "connector.disconnected",
        None,
        Some("connectors"),
        format!("provider={}", provider.as_str()),
    );
    Ok(Json(result))
}

async fn start_connector_oauth(
    State(state): State<AppState>,
    RoutePath(provider): RoutePath<String>,
    Json(mut request): Json<OAuthStartRequest>,
) -> ApiResult<Json<OAuthStartResponse>> {
    request.provider = parse_connector_provider(&provider)?;
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.start_connector_oauth(request))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn connector_oauth_callback(
    State(state): State<AppState>,
    RoutePath(provider): RoutePath<String>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Response {
    let provider = match parse_connector_provider(&provider) {
        Ok(provider) => provider,
        Err(error) => return error.into_response(),
    };
    let runtime = state.runtime.clone();
    let request = OAuthCallbackRequest {
        provider,
        state: query.state,
        code: query.code,
        error: query.error,
        error_description: query.error_description,
    };
    match tokio::task::spawn_blocking(move || runtime.complete_connector_oauth(request)).await {
        Ok(Ok(descriptor)) => {
            emit_service_event(
                &state,
                "connector.connected",
                None,
                Some("connectors"),
                format!("provider={}", descriptor.provider.as_str()),
            );
            Html("<!doctype html><meta charset=utf-8><title>Everything connected</title><style>body{font-family:system-ui;background:#111;color:#eee;display:grid;place-items:center;min-height:100vh}main{max-width:36rem;padding:2rem}h1{color:#9af}</style><main><h1>Account connected</h1><p>You can close this window and return to Everything.</p></main>").into_response()
        }
        Ok(Err(error)) => (
            StatusCode::BAD_REQUEST,
            Html(format!("<!doctype html><meta charset=utf-8><title>Connection failed</title><main><h1>Connection failed</h1><pre>{}</pre></main>", html_escape(&error.to_string()))),
        ).into_response(),
        Err(error) => ApiError::internal(error.to_string()).into_response(),
    }
}

async fn execute_connector_action(
    State(state): State<AppState>,
    RoutePath(provider): RoutePath<String>,
    Json(mut request): Json<ConnectorActionRequest>,
) -> ApiResult<Json<ConnectorActionResponse>> {
    request.provider = parse_connector_provider(&provider)?;
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.execute_connector_action(request))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    emit_service_event(
        &state,
        "connector.action",
        None,
        Some("connectors"),
        format!(
            "provider={} action={} status={}",
            result.provider.as_str(),
            result.action_id,
            result.status
        ),
    );
    Ok(Json(result))
}

async fn connector_audits(
    State(state): State<AppState>,
    Query(params): Query<ConnectorAuditParams>,
) -> ApiResult<Json<Vec<ConnectorAuditRecord>>> {
    let runtime = state.runtime.clone();
    let limit = params.limit.unwrap_or(100).clamp(1, 1_000);
    let result = tokio::task::spawn_blocking(move || runtime.connector_audits(limit))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn automations(State(state): State<AppState>) -> ApiResult<Json<Vec<AutomationDefinition>>> {
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.list_automations())
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn upsert_automation(
    State(state): State<AppState>,
    Json(request): Json<AutomationUpsertRequest>,
) -> ApiResult<Json<AutomationDefinition>> {
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.upsert_automation(request))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    emit_service_event(
        &state,
        "automation.saved",
        None,
        Some("automation"),
        format!(
            "automation_id={} enabled={}",
            result.automation_id, result.enabled
        ),
    );
    Ok(Json(result))
}

async fn automation_by_id(
    State(state): State<AppState>,
    RoutePath(automation_id): RoutePath<String>,
) -> ApiResult<Json<AutomationDefinition>> {
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || runtime.get_automation(&automation_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??
        .ok_or_else(|| ApiError::not_found("automation was not found"))?;
    Ok(Json(result))
}

async fn delete_automation(
    State(state): State<AppState>,
    RoutePath(automation_id): RoutePath<String>,
) -> ApiResult<StatusCode> {
    let runtime = state.runtime.clone();
    let deleted = tokio::task::spawn_blocking(move || runtime.delete_automation(&automation_id))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))??;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("automation was not found"))
    }
}

async fn run_automation_now(
    State(state): State<AppState>,
    RoutePath(automation_id): RoutePath<String>,
    Json(request): Json<AutomationRunNowRequest>,
) -> ApiResult<Json<AutomationExecution>> {
    let runtime = state.runtime.clone();
    let result =
        tokio::task::spawn_blocking(move || runtime.run_automation_now(&automation_id, request))
            .await
            .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn approve_automation_execution(
    State(state): State<AppState>,
    RoutePath((automation_id, execution_id)): RoutePath<(String, String)>,
) -> ApiResult<Json<AutomationExecution>> {
    let runtime = state.runtime.clone();
    let result = tokio::task::spawn_blocking(move || {
        runtime.approve_automation_execution(&automation_id, &execution_id)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    emit_service_event(
        &state,
        "automation.approved",
        result.run_id.as_deref(),
        Some("automation"),
        format!(
            "automation_id={} execution_id={} status={:?}",
            result.automation_id, result.execution_id, result.status
        ),
    );
    Ok(Json(result))
}

async fn automation_executions(
    State(state): State<AppState>,
    RoutePath(automation_id): RoutePath<String>,
    Query(params): Query<AutomationHistoryParams>,
) -> ApiResult<Json<Vec<AutomationExecution>>> {
    let runtime = state.runtime.clone();
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let result = tokio::task::spawn_blocking(move || {
        runtime.list_automation_executions(&automation_id, limit)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))??;
    Ok(Json(result))
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.events.subscribe()).filter_map(|message| async move {
        match message {
            Ok(event) => {
                let payload = serde_json::to_string(&event).ok()?;
                Some(Ok(Event::default()
                    .event(event.event_kind.clone())
                    .data(payload)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn parse_graph_direction(
    value: Option<&str>,
    default: everything_graph::GraphDirection,
) -> ApiResult<everything_graph::GraphDirection> {
    match value {
        None => Ok(default),
        Some(value) => everything_graph::GraphDirection::parse(value)
            .ok_or_else(|| ApiError::bad_request("direction must be inbound, outbound, or both")),
    }
}

fn parse_memory_scope(value: &str) -> ApiResult<MemoryScope> {
    match value.trim().to_ascii_lowercase().as_str() {
        "session" => Ok(MemoryScope::Session),
        "workspace" => Ok(MemoryScope::Workspace),
        "task" => Ok(MemoryScope::Task),
        "artifact" => Ok(MemoryScope::Artifact),
        "graph" => Ok(MemoryScope::Graph),
        "preference" => Ok(MemoryScope::Preference),
        _ => Err(ApiError::bad_request(
            "scope must be session, workspace, task, artifact, graph, or preference",
        )),
    }
}

fn parse_relation_kinds(value: Option<&str>) -> ApiResult<Vec<everything_graph::CodeRelationKind>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            everything_graph::CodeRelationKind::parse(value)
                .ok_or_else(|| ApiError::bad_request(format!("unknown relation kind '{value}'")))
        })
        .collect()
}

fn parse_connector_provider(value: &str) -> ApiResult<ConnectorProvider> {
    ConnectorProvider::parse(value).ok_or_else(|| {
        ApiError::bad_request("provider must be gmail, spotify, instagram, tiktok, or github")
    })
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

type ApiResult<T> = std::result::Result<T, ApiError>;

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({
            "error": self.message,
        }));
        (self.status, body).into_response()
    }
}

fn emit_service_event(
    state: &AppState,
    event_kind: &str,
    run_id: Option<&str>,
    stage: Option<&str>,
    detail: String,
) {
    let sequence = state.sequence.fetch_add(1, Ordering::Relaxed) + 1;
    let timestamp_epoch_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let event = ServiceEvent {
        sequence,
        timestamp_epoch_millis,
        event_kind: event_kind.to_owned(),
        run_id: run_id.map(str::to_owned),
        stage: stage.map(str::to_owned),
        detail,
    };
    let _ = state.events.send(event);
}

fn spawn_automation_scheduler(state: AppState) {
    if !state.runtime.settings().autonomy.scheduler_enabled {
        return;
    }
    let poll_millis = state
        .runtime
        .settings()
        .autonomy
        .scheduler_poll_millis
        .clamp(250, 60_000);
    let concurrency = state
        .runtime
        .settings()
        .autonomy
        .max_concurrency
        .clamp(1, 32);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(poll_millis));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            while semaphore.available_permits() > 0 {
                let runtime = state.runtime.clone();
                let claimed =
                    match tokio::task::spawn_blocking(move || runtime.claim_due_automation()).await
                    {
                        Ok(Ok(value)) => value,
                        Ok(Err(error)) => {
                            emit_service_event(
                                &state,
                                "automation.scheduler.error",
                                None,
                                Some("automation"),
                                error.to_string(),
                            );
                            break;
                        }
                        Err(error) => {
                            emit_service_event(
                                &state,
                                "automation.scheduler.error",
                                None,
                                Some("automation"),
                                error.to_string(),
                            );
                            break;
                        }
                    };
                let Some(lease) = claimed else { break };
                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => return,
                };
                let runtime = state.runtime.clone();
                let event_state = state.clone();
                tokio::spawn(async move {
                    let automation_id = lease.automation.automation_id.clone();
                    emit_service_event(
                        &event_state,
                        "automation.started",
                        None,
                        Some("automation"),
                        format!("automation_id={automation_id}"),
                    );
                    let result = tokio::task::spawn_blocking(move || {
                        runtime.execute_claimed_automation(lease)
                    })
                    .await;
                    match result {
                        Ok(Ok(execution)) => emit_service_event(
                            &event_state,
                            "automation.finished",
                            execution.run_id.as_deref(),
                            Some("automation"),
                            format!(
                                "automation_id={} status={:?}",
                                execution.automation_id, execution.status
                            ),
                        ),
                        Ok(Err(error)) => emit_service_event(
                            &event_state,
                            "automation.failed",
                            None,
                            Some("automation"),
                            format!("automation_id={automation_id} error={error}"),
                        ),
                        Err(error) => emit_service_event(
                            &event_state,
                            "automation.failed",
                            None,
                            Some("automation"),
                            format!("automation_id={automation_id} error={error}"),
                        ),
                    }
                    drop(permit);
                });
            }
        }
    });
}

fn spawn_code_graph_warmup(state: AppState) {
    let runtime = state.runtime.clone();
    let workspace = state.workspace.clone();

    tokio::spawn(async move {
        emit_service_event(
            &state,
            "code_graph.warmup.started",
            None,
            Some("code_graph"),
            format!("workspace={}", workspace.display()),
        );

        let result =
            tokio::task::spawn_blocking(move || runtime.refresh_code_graph(&workspace)).await;

        match result {
            Ok(Ok(report)) => emit_service_event(
                &state,
                "code_graph.warmup.completed",
                None,
                Some("code_graph"),
                format!(
                    "scanned={} changed={} total_ms={}",
                    report.scanned_files, report.changed_files, report.total_millis
                ),
            ),
            Ok(Err(error)) => emit_service_event(
                &state,
                "code_graph.warmup.failed",
                None,
                Some("code_graph"),
                error.to_string(),
            ),
            Err(error) => emit_service_event(
                &state,
                "code_graph.warmup.failed",
                None,
                Some("code_graph"),
                error.to_string(),
            ),
        }
    });
}
