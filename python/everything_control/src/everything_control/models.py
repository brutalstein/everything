from __future__ import annotations

from pathlib import Path
from typing import Literal

from pydantic import BaseModel, ConfigDict, Field


class ModelBase(BaseModel):
    model_config = ConfigDict(extra="ignore")


class ServiceInfo(ModelBase):
    service: str
    workspace: Path
    data_dir: Path
    model: str


class ProjectDescriptor(ModelBase):
    root_path: Path
    file_count: int
    graph_node_count: int
    graph_edge_count: int


class AdapterSummary(ModelBase):
    filesystem: str
    model: str
    command: str


class ModelHealth(ModelBase):
    status: Literal["Healthy", "Degraded", "Unavailable"] | str = "Unavailable"
    available: bool
    adapter: str
    detail: str
    primary_available: bool = False
    fallback_available: bool = False
    fallback_active: bool = False


class WorkspaceSnapshotStats(ModelBase):
    scanned_files: int
    cache_hits: int
    cache_misses: int
    bytes_read: int


class BootstrapMetrics(ModelBase):
    snapshot_millis: int
    graph_millis: int
    catalog_millis: int


class RuntimeDoctorCheck(ModelBase):
    check_id: str
    label: str
    status: Literal["healthy", "degraded", "failed"] | str
    detail: str
    remediation: str | None = None


class RuntimeDoctorReport(ModelBase):
    project: ProjectDescriptor
    adapters: AdapterSummary
    model_health: ModelHealth
    snapshot_stats: WorkspaceSnapshotStats
    bootstrap_metrics: BootstrapMetrics
    overall_status: Literal["healthy", "degraded", "failed"] | str = "healthy"
    checks: list[RuntimeDoctorCheck] = Field(default_factory=list)


class ModuleDescriptor(ModelBase):
    name: str
    kind: Literal["Core", "Adapter", "EntryPoint", "Tests"]
    responsibility: str


class GraphNode(ModelBase):
    id: str
    label: str
    kind: str
    source_path: Path


class GraphQueryResult(ModelBase):
    matched_nodes: list[GraphNode]


class GraphImpactReport(ModelBase):
    root: GraphNode
    depth: int
    affected_nodes: list[GraphNode]


class PackageReferenceSummary(ModelBase):
    label: str
    outbound_references: int


class GraphSummaryResponse(ModelBase):
    summary: str
    package_references: list[PackageReferenceSummary]


class SourceSpan(ModelBase):
    start_byte: int = 0
    end_byte: int = 0
    start_line: int = 0
    start_column: int = 0
    end_line: int = 0
    end_column: int = 0


class CodeEntity(ModelBase):
    id: str
    revision: int
    kind: str
    name: str
    qualified_name: str
    language: str
    file_path: Path
    span: SourceSpan
    valid_from_epoch_millis: int
    valid_until_epoch_millis: int | None = None


class CodeRelation(ModelBase):
    id: str
    revision: int
    source_id: str
    target_id: str
    kind: str
    confidence: float
    evidence_kind: Literal["observed", "inferred"] | str = "inferred"
    evidence_file: Path
    evidence_span: SourceSpan
    extractor: str
    valid_from_epoch_millis: int
    valid_until_epoch_millis: int | None = None


class PersistentGraphStats(ModelBase):
    schema_version: int
    graph_revision: int = 0
    database_path: Path
    active_files: int
    active_entities: int
    active_relations: int
    historical_entities: int
    historical_relations: int
    database_bytes: int


class CodeGraphIndexReport(ModelBase):
    schema_version: int
    graph_revision: int = 0
    workspace: Path
    database_path: Path
    scanned_files: int
    changed_files: int
    unchanged_files: int
    deleted_files: int
    parsed_entities: int
    parsed_relations: int
    parse_errors: int
    scan_millis: int
    parse_millis: int
    commit_millis: int
    total_millis: int


class CodeGraphSearchResult(ModelBase):
    graph_revision: int = 0
    score: float
    entity: CodeEntity


class CodeGraphTraversalReport(ModelBase):
    graph_revision: int = 0
    root: CodeEntity
    direction: Literal["inbound", "outbound", "both"] | str
    relation_kinds: list[str] = Field(default_factory=list)
    depth: int
    entities: list[CodeEntity]
    relations: list[CodeRelation]


class CodeGraphPath(ModelBase):
    graph_revision: int = 0
    direction: Literal["inbound", "outbound", "both"] | str
    relation_kinds: list[str] = Field(default_factory=list)
    entities: list[CodeEntity]
    relations: list[CodeRelation]


class CodeGraphChangeTarget(ModelBase):
    file_path: Path | None = None
    symbol: str | None = None
    start_line: int | None = None
    end_line: int | None = None
    change_kind: Literal["modify", "add", "delete", "rename", "dependency", "configuration", "public_api"] | str = "modify"


class CodeGraphChangeImpactRequest(ModelBase):
    targets: list[CodeGraphChangeTarget]
    max_depth: int = 4
    max_entities: int = 256
    include_inferred: bool = False


class CodeGraphEntityMetrics(ModelBase):
    entity_id: str
    graph_revision: int
    inbound_count: int
    outbound_count: int
    observed_relation_count: int
    test_relation_count: int
    centrality: float
    change_risk: float


class CodeGraphImpactPathStep(ModelBase):
    from_entity_id: str
    to_entity_id: str
    relation: str
    confidence: float
    evidence_kind: str
    evidence_file: Path
    evidence_span: SourceSpan


class CodeGraphImpactedEntity(ModelBase):
    entity: CodeEntity
    metrics: CodeGraphEntityMetrics
    distance: int
    impact_score: float
    risk_tier: str
    reasons: list[str] = Field(default_factory=list)
    path: list[CodeGraphImpactPathStep] = Field(default_factory=list)


class CodeGraphVerificationTarget(ModelBase):
    file_path: Path
    reason: str
    confidence: float
    related_entity_ids: list[str] = Field(default_factory=list)


class CodeGraphChangeImpactReport(ModelBase):
    schema_version: int
    graph_revision: int
    targets: list[CodeGraphChangeTarget]
    roots: list[CodeEntity]
    risk_tier: str
    aggregate_risk_score: float
    affected_entities: list[CodeGraphImpactedEntity]
    affected_files: list[Path]
    verification_targets: list[CodeGraphVerificationTarget]
    public_api_entities: list[CodeEntity]
    external_dependencies: list[CodeEntity]
    unresolved_targets: list[str]
    analysis_millis: int


class ResearchSource(ModelBase):
    citation_id: str
    title: str
    url: str
    canonical_url: str
    domain: str
    provider: str
    rank: int
    score: float
    snippet: str = ""
    extracted_text: str = ""
    published_at: str | None = None
    retrieved_at_epoch_millis: int
    content_hash: str
    from_cache: bool = False
    metadata: dict[str, str] = Field(default_factory=dict)


class ResearchReport(ModelBase):
    report_id: str
    query: str
    normalized_query: str
    mode: str
    freshness: str
    sources: list[ResearchSource]
    provider_count: int
    cache_hits: int
    searched_at_epoch_millis: int
    search_millis: int
    fetch_millis: int
    warnings: list[str] = Field(default_factory=list)


class ResearchProviderHealth(ModelBase):
    provider: str
    available: bool
    configured: bool
    detail: str
    latency_millis: int | None = None


class ResearchStatus(ModelBase):
    enabled: bool
    cache_path: str
    providers: list[ResearchProviderHealth]
    cached_queries: int
    cached_documents: int
    cache_bytes: int
    warnings: list[str] = Field(default_factory=list)


class WebFetchResponse(ModelBase):
    source: ResearchSource
    status_code: int
    media_type: str
    bytes_received: int
    truncated: bool
    warnings: list[str] = Field(default_factory=list)


class ModelCapabilityProfile(ModelBase):
    provider: str
    model_name: str
    quality_tier: Literal["stub", "small", "medium", "large"] | str
    context_window_tokens: int
    safe_context_tokens: int
    coding_suitability: float
    structured_output_reliability: float
    tool_calling_reliability: float
    estimated_tokens_per_second: float | None = None
    memory_estimate_mb: int | None = None
    recommended_task_classes: list[str] = Field(default_factory=list)


class DiscoveredModel(ModelBase):
    provider: str
    model_name: str
    installed: bool
    configured: bool
    profile: ModelCapabilityProfile


class ContextPolicyDecision(ModelBase):
    mode: str
    model_name: str
    safe_context_tokens: int
    prompt_token_budget: int
    retrieval_depth: int
    symbol_limit: int
    excerpt_byte_budget: int
    verifier_strength: Literal["basic", "standard", "strict"] | str
    max_model_calls: int
    max_tool_invocations: int
    escalation_threshold: float
    reasons: list[str] = Field(default_factory=list)


class PlanningDocument(ModelBase):
    objective: str
    content: str
    graph_revision: int = 0
    selected_files: list[Path] = Field(default_factory=list)
    generated_by: str
    skill_id: str | None = None
    fallback_used: bool = False
    fallback_reason: str | None = None
    model_capability_profile: ModelCapabilityProfile | None = None
    context_policy: ContextPolicyDecision | None = None
    estimated_context_tokens: int = 0


class ArtifactDescriptor(ModelBase):
    artifact_id: str
    run_id: str
    kind: str
    content_hash: str
    media_type: str
    size_bytes: int
    object_path: Path
    created_at_epoch_millis: int
    origin: str


class ArtifactContentResponse(ModelBase):
    descriptor: ArtifactDescriptor
    encoding: str
    content: str


class Checkpoint(ModelBase):
    checkpoint_id: str
    run_id: str
    stage_id: str | None = None
    event_sequence: int
    created_at_epoch_millis: int
    safe_to_resume: bool
    summary: str
    artifact_ids: list[str] = Field(default_factory=list)


class PlanResponse(ModelBase):
    run_id: str
    document: PlanningDocument
    journal_path: Path
    artifact: ArtifactDescriptor | None = None


class RunEvent(ModelBase):
    sequence: int = 0
    event_id: str = ""
    timestamp_epoch_millis: int = 0
    stage: str
    stage_id: str | None = None
    correlation_id: str | None = None
    event_kind: str = ""
    severity: str = "Info"
    summary: str = ""
    detail: str
    payload: object | None = None
    provenance: str = ""


class RunJournal(ModelBase):
    schema_version: int = 1
    run_id: str
    objective: str
    status: Literal["Queued", "Started", "Running", "Paused", "AwaitingApproval", "Blocked", "Cancelling", "Cancelled", "Recovering", "Completed", "Failed"] | str
    generated_by: str
    mode: str | None = None
    created_at_epoch_millis: int = 0
    updated_at_epoch_millis: int = 0
    events: list[RunEvent]
    artifact_path: Path | None = None
    artifacts: list[ArtifactDescriptor] = Field(default_factory=list)
    failure_class: str | None = None
    recoverable: bool = False
    recovery_disposition: Literal["Recoverable", "ManualReview", "NotRecoverable"] | str | None = None


class RunSummary(ModelBase):
    run_id: str
    objective: str
    status: Literal["Queued", "Started", "Running", "Paused", "AwaitingApproval", "Blocked", "Cancelling", "Cancelled", "Recovering", "Completed", "Failed"] | str
    generated_by: str
    event_count: int
    last_stage: str | None = None
    journal_path: Path

class ToolDefinition(ModelBase):
    tool_id: str
    version: str
    description: str
    input_schema: dict[str, object] = Field(default_factory=dict)
    output_schema: dict[str, object] = Field(default_factory=dict)
    required_permissions: list[str] = Field(default_factory=list)
    default_timeout_millis: int
    max_output_bytes: int
    supports_cancellation: bool
    verifier_hook: str | None = None
    effect: str


class ModelInvocationRecord(ModelBase):
    invocation_id: str
    run_id: str
    stage_id: str | None = None
    provider: str
    model: str
    status: str
    started_at_epoch_millis: int
    finished_at_epoch_millis: int | None = None
    prompt_hash: str | None = None
    response_artifact_id: str | None = None
    fallback_used: bool = False
    primary_error: str | None = None
    capability_profile: ModelCapabilityProfile | None = None
    prompt_estimated_tokens: int = 0
    output_bytes: int = 0
    duration_millis: int = 0
    failure_class: str | None = None
    error_code: str | None = None


class ToolInvocationRecord(ModelBase):
    invocation_id: str
    run_id: str
    stage_id: str | None = None
    tool_name: str
    tool_version: str = ""
    required_permissions: list[str] = Field(default_factory=list)
    effect: str = "read_only"
    status: str
    started_at_epoch_millis: int
    finished_at_epoch_millis: int | None = None
    arguments: object | None = None
    output: object | None = None
    output_truncated: bool = False
    timeout_millis: int | None = None
    replay_key: str | None = None
    result_summary: str | None = None
    failure_class: str | None = None
    error_code: str | None = None


class ToolInvocationRequest(ModelBase):
    run_id: str
    tool_id: str
    input: object = Field(default_factory=dict)
    approval_granted: bool = False
    timeout_millis: int | None = None


class ToolInvocationResponse(ModelBase):
    invocation: ToolInvocationRecord
    output: object | None = None


class VerificationCommand(ModelBase):
    program: str
    args: list[str] = Field(default_factory=list)
    label: str
    timeout_millis: int | None = None


class PatchExecutionRequest(ModelBase):
    objective: str
    mode: Literal["Fast", "Balanced", "Deep"] | str = "Balanced"
    relative_path: Path
    expected_content_hash: str
    replacement_content: str
    verification_commands: list[VerificationCommand] = Field(default_factory=list)
    approval_granted: bool = False
    allow_repeat_failure: bool = False


class EditProposalRequest(ModelBase):
    objective: str
    mode: Literal["Fast", "Balanced", "Deep"] | str = "Balanced"
    skill_id: str | None = None


class EditProposalResponse(ModelBase):
    run_id: str
    status: str
    summary: str
    patch: PatchExecutionRequest
    diff: str
    artifact: ArtifactDescriptor
    generated_by: str
    skill_id: str | None = None
    fallback_used: bool = False
    fallback_reason: str | None = None


class PatchExecutionResponse(ModelBase):
    run_id: str
    status: str
    patch_invocation_id: str
    verification_invocation_ids: list[str] = Field(default_factory=list)
    artifacts: list[ArtifactDescriptor] = Field(default_factory=list)
    rolled_back: bool
    summary: str
    verification_report: "VerificationReport | None" = None
    verification_artifact_id: str | None = None


class CancellationResponse(ModelBase):
    invocation_id: str
    cancelled: bool

class VerificationCheck(ModelBase):
    check_id: str
    kind: str
    status: str
    required: bool
    summary: str
    evidence_ids: list[str] = Field(default_factory=list)
    artifact_ids: list[str] = Field(default_factory=list)
    invocation_ids: list[str] = Field(default_factory=list)
    skip_reason: str | None = None


class VerificationClaim(ModelBase):
    claim_id: str
    statement: str
    status: str
    evidence_ids: list[str] = Field(default_factory=list)
    artifact_ids: list[str] = Field(default_factory=list)
    confidence: float


class VerificationReport(ModelBase):
    report_id: str
    run_id: str
    status: str
    generated_at_epoch_millis: int
    checks: list[VerificationCheck] = Field(default_factory=list)
    claims: list[VerificationClaim] = Field(default_factory=list)
    confidence: float
    unresolved_risks: list[str] = Field(default_factory=list)


class SkillManifest(ModelBase):
    skill_id: str
    name: str
    version: str
    runtime_api: str
    description: str
    permissions: list[str] = Field(default_factory=list)
    input_schema: dict[str, object] = Field(default_factory=dict)
    output_schema: dict[str, object] = Field(default_factory=dict)
    entrypoint: str
    workflow: str


class SkillCompatibility(ModelBase):
    compatible: bool
    runtime_api: str
    reason: str | None = None


class SkillDescriptor(ModelBase):
    manifest: SkillManifest
    enabled: bool
    compatibility: SkillCompatibility
    source: Literal["Builtin", "Workspace", "User"] | str = "Builtin"
    source_path: Path | None = None
    content_hash: str = ""
    instructions_preview: str = ""


class SkillExecutionRequest(ModelBase):
    input: object = Field(default_factory=dict)
    approval_granted: bool = False


class SkillExecutionResponse(ModelBase):
    skill_id: str
    skill_version: str
    status: str
    run_id: str | None = None
    artifact_ids: list[str] = Field(default_factory=list)
    output: object = Field(default_factory=dict)
    verification_report: VerificationReport | None = None


class MemoryEntry(ModelBase):
    memory_id: str
    scope: Literal["Session", "Workspace", "Task", "Artifact", "Graph", "Preference"] | str
    title: str
    content: str
    source: str
    workspace_key: str | None = None
    run_id: str | None = None
    artifact_id: str | None = None
    valid_from_epoch_millis: int | None = None
    valid_until_epoch_millis: int | None = None
    version: int
    confidence: float
    evidence_ids: list[str] = Field(default_factory=list)
    tags: list[str] = Field(default_factory=list)
    superseded_by: str | None = None
    editable: bool
    forgettable: bool
    created_at_epoch_millis: int
    updated_at_epoch_millis: int


class MemoryUpsertRequest(ModelBase):
    memory_id: str | None = None
    scope: Literal["Session", "Workspace", "Task", "Artifact", "Graph", "Preference"] | str
    title: str
    content: str
    source: str
    workspace_key: str | None = None
    run_id: str | None = None
    artifact_id: str | None = None
    valid_from_epoch_millis: int | None = None
    valid_until_epoch_millis: int | None = None
    version: int = 1
    confidence: float = 0.8
    evidence_ids: list[str] = Field(default_factory=list)
    tags: list[str] = Field(default_factory=list)
    editable: bool = True
    forgettable: bool = True


class MemorySearchResult(ModelBase):
    entry: MemoryEntry
    score: float


class BooleanResult(ModelBase):
    forgotten: bool | None = None
    uninstalled: bool | None = None
    superseded: bool | None = None

class ConnectorActionDescriptor(ModelBase):
    action_id: str
    title: str
    description: str
    risk: str
    required_scopes: list[str] = Field(default_factory=list)
    input_schema: object = Field(default_factory=dict)
    supports_dry_run: bool = False
    idempotent: bool = False


class ConnectorDescriptor(ModelBase):
    provider: Literal["Gmail", "Spotify", "Instagram", "TikTok", "GitHub", "Custom"] | str
    display_name: str
    description: str
    status: str
    configured: bool
    connected: bool
    account_label: str | None = None
    granted_scopes: list[str] = Field(default_factory=list)
    token_expires_at_epoch_millis: int | None = None
    actions: list[ConnectorActionDescriptor] = Field(default_factory=list)
    limitations: list[str] = Field(default_factory=list)
    last_error: str | None = None
    metadata: dict[str, str] = Field(default_factory=dict)


class ConnectorConfigureRequest(ModelBase):
    provider: Literal["Gmail", "Spotify", "Instagram", "TikTok", "GitHub", "Custom"] | str
    client_id: str
    client_secret: str | None = None
    access_token: str | None = None
    redirect_uri: str | None = None
    scopes: list[str] = Field(default_factory=list)
    metadata: dict[str, str] = Field(default_factory=dict)


class OAuthStartResponse(ModelBase):
    provider: str
    authorization_url: str
    redirect_uri: str
    state: str
    expires_at_epoch_millis: int


class ConnectorActionRequest(ModelBase):
    provider: str
    action_id: str
    input: object = Field(default_factory=dict)
    approval_granted: bool = False
    dry_run: bool = False
    idempotency_key: str | None = None


class ConnectorActionResponse(ModelBase):
    provider: str
    action_id: str
    status: str
    risk: str
    executed: bool
    output: object = Field(default_factory=dict)
    external_reference: str | None = None
    retry_after_millis: int | None = None
    warnings: list[str] = Field(default_factory=list)


class ConnectorAuditRecord(ModelBase):
    audit_id: str
    provider: str
    action_id: str
    risk: str
    started_at_epoch_millis: int
    finished_at_epoch_millis: int
    status: str
    approval_granted: bool
    input_hash: str
    idempotency_key: str | None = None
    error_code: str | None = None


class AutomationUpsertRequest(ModelBase):
    automation_id: str | None = None
    name: str
    description: str = ""
    schedule: dict[str, object]
    action: dict[str, object]
    policy: dict[str, object] = Field(default_factory=dict)
    enabled: bool = True


class AutomationDefinition(ModelBase):
    automation_id: str
    name: str
    description: str
    schedule: dict[str, object]
    action: dict[str, object]
    policy: dict[str, object]
    enabled: bool
    created_at_epoch_millis: int
    updated_at_epoch_millis: int
    next_run_at_epoch_millis: int | None = None
    last_run_at_epoch_millis: int | None = None
    consecutive_failures: int = 0
    retry_attempt: int = 0
    retry_scheduled_for_epoch_millis: int | None = None
    suspended_reason: str | None = None


class AutomationExecution(ModelBase):
    execution_id: str
    automation_id: str
    status: str
    scheduled_for_epoch_millis: int
    started_at_epoch_millis: int
    finished_at_epoch_millis: int | None = None
    run_id: str | None = None
    output: object = Field(default_factory=dict)
    error: str | None = None
    attempt: int = 1
    next_retry_at_epoch_millis: int | None = None
