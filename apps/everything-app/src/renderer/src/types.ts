export type ExecutionMode = "Fast" | "Balanced" | "Deep";
export type RunStatus =
  | "Queued"
  | "Started"
  | "Running"
  | "Paused"
  | "AwaitingApproval"
  | "Blocked"
  | "Cancelling"
  | "Cancelled"
  | "Recovering"
  | "Completed"
  | "Failed";

export type RecoveryDisposition = "Recoverable" | "ManualReview" | "NotRecoverable";

export interface ServiceStatusSnapshot {
  status: "idle" | "starting" | "ready" | "stopped" | "error";
  detail: string;
  workspaceRoot: string;
  serviceUrl: string;
  pid: number | null;
  recentLogs: string[];
}

export interface ServiceInfo {
  service: string;
  workspace: string;
  data_dir: string;
  model: string;
}

export interface ModuleDescriptor {
  name: string;
  kind: string;
  responsibility: string;
}

export interface PackageReferenceSummary {
  label: string;
  outbound_references: number;
}

export interface GraphSummaryResponse {
  summary: string;
  package_references: PackageReferenceSummary[];
}

export interface GraphNode {
  id: string;
  label: string;
  kind: string;
  source_path: string;
}

export interface GraphQueryResult {
  matched_nodes: GraphNode[];
}

export interface GraphImpactReport {
  root: GraphNode;
  depth: number;
  affected_nodes: GraphNode[];
}

export interface ModelCapabilityProfile {
  provider: string;
  model_name: string;
  quality_tier: "stub" | "small" | "medium" | "large" | string;
  context_window_tokens: number;
  safe_context_tokens: number;
  coding_suitability: number;
  structured_output_reliability: number;
  tool_calling_reliability: number;
  estimated_tokens_per_second?: number | null;
  memory_estimate_mb?: number | null;
  recommended_task_classes: string[];
}

export interface DiscoveredModel {
  provider: string;
  model_name: string;
  installed: boolean;
  configured: boolean;
  profile: ModelCapabilityProfile;
}

export interface ContextPolicyDecision {
  mode: ExecutionMode;
  model_name: string;
  safe_context_tokens: number;
  prompt_token_budget: number;
  retrieval_depth: number;
  symbol_limit: number;
  excerpt_byte_budget: number;
  verifier_strength: "basic" | "standard" | "strict" | string;
  max_model_calls: number;
  max_tool_invocations: number;
  escalation_threshold: number;
  reasons: string[];
}

export interface PlanningDocument {
  objective: string;
  content: string;
  graph_revision: number;
  selected_files: string[];
  generated_by: string;
  skill_id?: string | null;
  fallback_used: boolean;
  fallback_reason?: string | null;
  model_capability_profile?: ModelCapabilityProfile | null;
  context_policy?: ContextPolicyDecision | null;
  estimated_context_tokens: number;
}

export interface ArtifactDescriptor {
  artifact_id: string;
  run_id: string;
  kind: string;
  content_hash: string;
  media_type: string;
  size_bytes: number;
  object_path: string;
  created_at_epoch_millis: number;
  origin: string;
}

export interface ArtifactContentResponse {
  descriptor: ArtifactDescriptor;
  encoding: string;
  content: string;
}

export interface Checkpoint {
  checkpoint_id: string;
  run_id: string;
  stage_id?: string | null;
  event_sequence: number;
  created_at_epoch_millis: number;
  safe_to_resume: boolean;
  summary: string;
  artifact_ids: string[];
}

export interface PlanResponse {
  run_id: string;
  document: PlanningDocument;
  journal_path: string;
  artifact?: ArtifactDescriptor | null;
}

export interface RunSummary {
  run_id: string;
  objective: string;
  status: RunStatus;
  generated_by: string;
  event_count: number;
  last_stage?: string | null;
  journal_path: string;
  updated_at_epoch_millis?: number;
}

export interface RunEvent {
  sequence: number;
  event_id: string;
  timestamp_epoch_millis: number;
  stage: string;
  stage_id?: string | null;
  correlation_id?: string | null;
  event_kind: string;
  severity: "Trace" | "Debug" | "Info" | "Warning" | "Error";
  summary: string;
  detail: string;
  payload?: unknown;
  provenance: string;
}

export interface RunJournal {
  schema_version: number;
  run_id: string;
  objective: string;
  status: RunStatus;
  generated_by: string;
  mode?: ExecutionMode | null;
  created_at_epoch_millis: number;
  updated_at_epoch_millis: number;
  events: RunEvent[];
  artifact_path?: string | null;
  artifacts: ArtifactDescriptor[];
  failure_class?: string | null;
  recoverable: boolean;
  recovery_disposition?: RecoveryDisposition | null;
}

export interface RuntimeMetricsSnapshot {
  data_dir: string;
  runs_recorded: number;
  benchmarks_recorded: number;
  latest_run_id?: string | null;
  latest_benchmark_id?: string | null;
}

export interface RuntimeDoctorCheck {
  check_id: string;
  label: string;
  status: "healthy" | "degraded" | "failed";
  detail: string;
  remediation?: string | null;
}

export interface RuntimeDoctorReport {
  project: {
    root_path: string;
    file_count: number;
    graph_node_count: number;
    graph_edge_count: number;
  };
  adapters: {
    filesystem: string;
    model: string;
    command: string;
  };
  model_health: {
    status: "Healthy" | "Degraded" | "Unavailable";
    available: boolean;
    adapter: string;
    detail: string;
    primary_available: boolean;
    fallback_available: boolean;
    fallback_active: boolean;
  };
  snapshot_stats: {
    scanned_files: number;
    cache_hits: number;
    cache_misses: number;
    bytes_read: number;
  };
  bootstrap_metrics: {
    snapshot_millis: number;
    graph_millis: number;
    catalog_millis: number;
  };
  overall_status: "healthy" | "degraded" | "failed";
  checks: RuntimeDoctorCheck[];
}

export interface BenchmarkRecord {
  benchmark_id: string;
  benchmark_name: string;
  workspace_path: string;
  iterations: number;
  mean_millis: number;
  min_millis: number;
  max_millis: number;
  p50_millis: number;
  p95_millis: number;
  cache_hits: number;
  cache_misses: number;
  bytes_read: number;
  created_at_epoch_millis: number;
}

export interface ServiceEvent {
  sequence: number;
  timestamp_epoch_millis: number;
  event_kind: string;
  run_id?: string | null;
  stage?: string | null;
  detail: string;
}

export interface CodeEntity {
  id: string;
  revision: number;
  kind: string;
  name: string;
  qualified_name: string;
  language: string;
  file_path: string;
  span: {
    start_line: number;
    end_line: number;
    start_column: number;
    end_column: number;
  };
}

export interface CodeRelation {
  id: string;
  source_id: string;
  target_id: string;
  kind: string;
  confidence: number;
  evidence_kind: "observed" | "inferred";
  evidence_file: string;
  evidence_span: {
    start_line: number;
    end_line: number;
  };
}

export interface PersistentGraphStats {
  schema_version: number;
  graph_revision: number;
  database_path: string;
  active_files: number;
  active_entities: number;
  active_relations: number;
  historical_entities: number;
  historical_relations: number;
  database_bytes: number;
}

export interface CodeGraphIndexReport {
  schema_version: number;
  graph_revision: number;
  workspace: string;
  database_path: string;
  scanned_files: number;
  changed_files: number;
  unchanged_files: number;
  deleted_files: number;
  parsed_entities: number;
  parsed_relations: number;
  parse_errors: number;
  scan_millis: number;
  parse_millis: number;
  commit_millis: number;
  total_millis: number;
}

export interface CodeGraphSearchResult {
  graph_revision: number;
  score: number;
  entity: CodeEntity;
}

export interface CodeGraphImpactReport {
  graph_revision: number;
  root: CodeEntity;
  direction: "inbound" | "outbound" | "both";
  relation_kinds: string[];
  depth: number;
  entities: CodeEntity[];
  relations: CodeRelation[];
}

export interface CodeGraphPath {
  graph_revision: number;
  direction: "inbound" | "outbound" | "both";
  relation_kinds: string[];
  entities: CodeEntity[];
  relations: CodeRelation[];
}

export interface ToolDefinition {
  tool_id: string;
  version: string;
  description: string;
  input_schema: Record<string, unknown>;
  output_schema: Record<string, unknown>;
  required_permissions: string[];
  default_timeout_millis: number;
  max_output_bytes: number;
  supports_cancellation: boolean;
  verifier_hook?: string | null;
  effect: string;
}

export interface ModelInvocationRecord {
  invocation_id: string;
  run_id: string;
  stage_id?: string | null;
  provider: string;
  model: string;
  status: "Queued" | "Running" | "Completed" | "Failed" | "Cancelled";
  started_at_epoch_millis: number;
  finished_at_epoch_millis?: number | null;
  prompt_hash?: string | null;
  response_artifact_id?: string | null;
  fallback_used: boolean;
  primary_error?: string | null;
  capability_profile?: ModelCapabilityProfile | null;
  prompt_estimated_tokens: number;
  output_bytes: number;
  duration_millis: number;
  failure_class?: string | null;
  error_code?: string | null;
}

export interface ToolInvocationRecord {
  invocation_id: string;
  run_id: string;
  stage_id?: string | null;
  tool_name: string;
  tool_version: string;
  required_permissions: string[];
  effect: string;
  status: "Queued" | "Running" | "Completed" | "Failed" | "Cancelled";
  started_at_epoch_millis: number;
  finished_at_epoch_millis?: number | null;
  arguments: unknown;
  output: unknown;
  output_truncated: boolean;
  timeout_millis?: number | null;
  replay_key?: string | null;
  result_summary?: string | null;
  failure_class?: string | null;
  error_code?: string | null;
}

export interface VerificationCheck {
  check_id: string;
  kind: string;
  status: string;
  required: boolean;
  summary: string;
  evidence_ids: string[];
  artifact_ids: string[];
  invocation_ids: string[];
  skip_reason?: string | null;
}

export interface VerificationClaim {
  claim_id: string;
  statement: string;
  status: string;
  evidence_ids: string[];
  artifact_ids: string[];
  confidence: number;
}

export interface VerificationReport {
  report_id: string;
  run_id: string;
  status: string;
  generated_at_epoch_millis: number;
  checks: VerificationCheck[];
  claims: VerificationClaim[];
  confidence: number;
  unresolved_risks: string[];
}

export interface VerificationCommand {
  program: string;
  args: string[];
  label: string;
  timeout_millis?: number | null;
}

export interface PatchExecutionRequest {
  objective: string;
  mode: ExecutionMode;
  relative_path: string;
  expected_content_hash: string;
  replacement_content: string;
  verification_commands: VerificationCommand[];
  approval_granted: boolean;
  allow_repeat_failure: boolean;
}

export interface EditProposalResponse {
  run_id: string;
  status: string;
  summary: string;
  patch: PatchExecutionRequest;
  diff: string;
  artifact: ArtifactDescriptor;
  generated_by: string;
  skill_id?: string | null;
  fallback_used: boolean;
  fallback_reason?: string | null;
  impact_analysis?: CodeGraphChangeImpactReport | null;
  impact_artifact?: ArtifactDescriptor | null;
}

export interface PatchExecutionResponse {
  run_id: string;
  status: string;
  patch_invocation_id: string;
  verification_invocation_ids: string[];
  artifacts: ArtifactDescriptor[];
  rolled_back: boolean;
  summary: string;
  verification_report?: VerificationReport | null;
  verification_artifact_id?: string | null;
}

export interface SkillManifest {
  skill_id: string;
  name: string;
  version: string;
  runtime_api: string;
  description: string;
  permissions: string[];
  input_schema: Record<string, unknown>;
  output_schema: Record<string, unknown>;
  entrypoint: string;
  workflow: string;
}

export interface SkillDescriptor {
  manifest: SkillManifest;
  enabled: boolean;
  compatibility: {
    compatible: boolean;
    runtime_api: string;
    reason?: string | null;
  };
  source: "Builtin" | "Workspace" | "User" | string;
  source_path?: string | null;
  content_hash: string;
  instructions_preview: string;
}

export interface SkillExecutionResponse {
  skill_id: string;
  skill_version: string;
  status: "Completed" | "Failed" | "Blocked" | string;
  run_id?: string | null;
  artifact_ids: string[];
  output: unknown;
  verification_report?: VerificationReport | null;
}

export interface MemoryEntry {
  memory_id: string;
  scope: "Session" | "Workspace" | "Task" | "Artifact" | "Graph" | "Preference" | string;
  title: string;
  content: string;
  source: string;
  workspace_key?: string | null;
  run_id?: string | null;
  artifact_id?: string | null;
  version: number;
  confidence: number;
  evidence_ids: string[];
  tags: string[];
  superseded_by?: string | null;
  editable: boolean;
  forgettable: boolean;
  created_at_epoch_millis: number;
  updated_at_epoch_millis: number;
}

export interface MemorySearchResult {
  entry: MemoryEntry;
  score: number;
}

export type ConnectorProvider = "Gmail" | "Spotify" | "Instagram" | "TikTok" | "GitHub" | "Custom";
export type ConnectorRisk = "ReadOnly" | "ReversibleWrite" | "ExternalPublish" | "AccountMutation";

export interface ConnectorActionDescriptor {
  action_id: string;
  title: string;
  description: string;
  risk: ConnectorRisk;
  required_scopes: string[];
  input_schema: unknown;
  supports_dry_run: boolean;
  idempotent: boolean;
}

export interface ConnectorDescriptor {
  provider: ConnectorProvider;
  display_name: string;
  description: string;
  status: string;
  configured: boolean;
  connected: boolean;
  account_label?: string | null;
  granted_scopes: string[];
  token_expires_at_epoch_millis?: number | null;
  actions: ConnectorActionDescriptor[];
  limitations: string[];
  last_error?: string | null;
  metadata: Record<string, string>;
}

export interface OAuthStartResponse {
  provider: ConnectorProvider;
  authorization_url: string;
  redirect_uri: string;
  state: string;
  expires_at_epoch_millis: number;
}

export interface ConnectorAuditRecord {
  audit_id: string;
  provider: ConnectorProvider;
  action_id: string;
  risk: ConnectorRisk;
  started_at_epoch_millis: number;
  finished_at_epoch_millis: number;
  status: string;
  approval_granted: boolean;
  input_hash: string;
  idempotency_key?: string | null;
  error_code?: string | null;
}

export interface ConnectorActionResponse {
  provider: ConnectorProvider;
  action_id: string;
  status: string;
  risk: ConnectorRisk;
  executed: boolean;
  output: unknown;
  external_reference?: string | null;
  retry_after_millis?: number | null;
  warnings: string[];
}

export interface AutomationDefinition {
  automation_id: string;
  name: string;
  description: string;
  schedule: Record<string, unknown>;
  action: Record<string, unknown>;
  policy: Record<string, unknown>;
  enabled: boolean;
  created_at_epoch_millis: number;
  updated_at_epoch_millis: number;
  next_run_at_epoch_millis?: number | null;
  last_run_at_epoch_millis?: number | null;
  consecutive_failures: number;
  retry_attempt?: number;
  retry_scheduled_for_epoch_millis?: number | null;
  suspended_reason?: string | null;
}

export interface AutomationExecution {
  execution_id: string;
  automation_id: string;
  status: string;
  scheduled_for_epoch_millis: number;
  started_at_epoch_millis: number;
  finished_at_epoch_millis?: number | null;
  run_id?: string | null;
  output: unknown;
  error?: string | null;
  attempt: number;
  next_retry_at_epoch_millis?: number | null;
}


export type ResearchMode = "general" | "technical" | "news" | "academic";
export type ResearchFreshness = "any" | "day" | "week" | "month" | "year";

export interface ResearchSource {
  citation_id: string;
  title: string;
  url: string;
  canonical_url: string;
  domain: string;
  provider: string;
  rank: number;
  score: number;
  snippet: string;
  extracted_text: string;
  published_at?: string | null;
  retrieved_at_epoch_millis: number;
  content_hash: string;
  from_cache: boolean;
  metadata: Record<string, string>;
}

export interface ResearchProviderHealth {
  provider: string;
  available: boolean;
  configured: boolean;
  detail: string;
  latency_millis?: number | null;
}

export interface ResearchStatus {
  enabled: boolean;
  cache_path: string;
  providers: ResearchProviderHealth[];
  cached_queries: number;
  cached_documents: number;
  cache_bytes: number;
  warnings: string[];
}

export interface ResearchReport {
  report_id: string;
  query: string;
  normalized_query: string;
  mode: ResearchMode;
  freshness: ResearchFreshness;
  sources: ResearchSource[];
  provider_count: number;
  cache_hits: number;
  searched_at_epoch_millis: number;
  search_millis: number;
  fetch_millis: number;
  warnings: string[];
}

export type CodeGraphChangeKind = "modify" | "add" | "delete" | "rename" | "dependency" | "configuration" | "public_api";
export type CodeGraphImpactRiskTier = "low" | "medium" | "high" | "critical";

export interface CodeGraphSourceSpan {
  start_byte: number;
  end_byte: number;
  start_line: number;
  start_column: number;
  end_line: number;
  end_column: number;
}

export interface CodeGraphEntity {
  id: string;
  revision: number;
  kind: string;
  name: string;
  qualified_name: string;
  language: string;
  file_path: string;
  span: CodeGraphSourceSpan;
  valid_from_epoch_millis: number;
  valid_until_epoch_millis?: number | null;
}

export interface CodeGraphChangeTarget {
  file_path?: string | null;
  symbol?: string | null;
  start_line?: number | null;
  end_line?: number | null;
  change_kind: CodeGraphChangeKind;
}

export interface CodeGraphEntityMetrics {
  entity_id: string;
  graph_revision: number;
  inbound_count: number;
  outbound_count: number;
  observed_relation_count: number;
  test_relation_count: number;
  centrality: number;
  change_risk: number;
}

export interface CodeGraphImpactPathStep {
  from_entity_id: string;
  to_entity_id: string;
  relation: string;
  confidence: number;
  evidence_kind: string;
  evidence_file: string;
  evidence_span: CodeGraphSourceSpan;
}

export interface CodeGraphImpactedEntity {
  entity: CodeGraphEntity;
  metrics: CodeGraphEntityMetrics;
  distance: number;
  impact_score: number;
  risk_tier: CodeGraphImpactRiskTier;
  reasons: string[];
  path: CodeGraphImpactPathStep[];
}

export interface CodeGraphVerificationTarget {
  file_path: string;
  reason: string;
  confidence: number;
  related_entity_ids: string[];
}

export interface CodeGraphChangeImpactReport {
  schema_version: number;
  graph_revision: number;
  targets: CodeGraphChangeTarget[];
  roots: CodeGraphEntity[];
  risk_tier: CodeGraphImpactRiskTier;
  aggregate_risk_score: number;
  affected_entities: CodeGraphImpactedEntity[];
  affected_files: string[];
  verification_targets: CodeGraphVerificationTarget[];
  public_api_entities: CodeGraphEntity[];
  external_dependencies: CodeGraphEntity[];
  unresolved_targets: string[];
  analysis_millis: number;
}

