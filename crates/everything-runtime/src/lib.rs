mod artifact;
mod automation;
mod bootstrap;
mod catalog;
mod config;
mod connectors;
mod edit;
mod execution;
mod journal;
mod memory;
mod metrics;
mod planner;
mod recovery;
mod retrieval;
mod skills;

use anyhow::Result;
use artifact::ArtifactStore;
use bootstrap::{BootstrapReport, WorkspaceBootstrapper};
use everything_adapters::{
    CommandExecutor, FileSystemAdapter, LocalModelAdapter, ModelAdapter, OllamaModelAdapter,
    ResilientModelAdapter,
};
use everything_domain::{
    AdapterSummary, ArtifactContentResponse, ArtifactDescriptor, ArtifactId, ArtifactKind,
    BenchmarkRecord, Checkpoint, CheckpointId, DoctorCheckStatus, ExecutionMode,
    GraphSummaryResponse, MemoryQuery, ModelBackend, ModelFallbackPolicy, PackageReferenceSummary,
    PermissionScope, PlanResponse, PlanningDocument, PolicyDecision, ResearchFreshness,
    ResearchMode, ResearchReport, ResearchStatus, RetrievalContextPack, RunEvent, RunId,
    RunJournal, RunStatus, RunSummary, RuntimeDoctorCheck, RuntimeDoctorReport,
    RuntimeMetricsSnapshot, RuntimeSettings, StageId, TaskRequest, ToolTrustMode, WebFetchRequest,
    WebFetchResponse, WebSearchRequest,
};
use everything_graph::{
    CodeEntityKind, CodeGraphChangeImpactReport, CodeGraphChangeImpactRequest,
    CodeGraphImpactReport, CodeGraphIndexReport, CodeGraphPath, CodeGraphSearchResult,
    CodeRelationKind, GraphDirection, PersistentCodeGraph, PersistentGraphStats,
};
use everything_state::StateStore;
use journal::RunJournalBuilder;
use metrics::{build_bootstrap_benchmark, metrics_snapshot, persist_benchmark, summarize_journals};
use planner::ArchitecturePlanner;
use recovery::reconcile_interrupted_runs;
use retrieval::RetrievalService;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

pub use bootstrap::BootstrapReport as RuntimeBootstrapReport;
pub use config::{build_task_request, ensure_data_dir, load_settings};

#[derive(Clone)]
pub struct RuntimeComponents {
    pub file_system: Arc<dyn FileSystemAdapter>,
    pub model: Arc<dyn ModelAdapter>,
    pub command: Arc<dyn CommandExecutor>,
}

pub struct ModularRuntime {
    components: RuntimeComponents,
    bootstrapper: WorkspaceBootstrapper,
    planner: ArchitecturePlanner,
    retrieval: RetrievalService,
    persistent_graph: PersistentCodeGraph,
    settings: RuntimeSettings,
    state_store: StateStore,
    artifact_store: ArtifactStore,
    tool_runtime: everything_tools::ToolRuntime,
    memory_store: everything_memory::MemoryStore,
    skill_registry: everything_skills::SkillRegistry,
    verifier: everything_verifier::DeterministicVerifier,
    connector_runtime: everything_connectors::ConnectorRuntime,
    research_runtime: everything_research::ResearchRuntime,
}

impl ModularRuntime {
    pub fn new(
        components: RuntimeComponents,
        persistent_graph: PersistentCodeGraph,
        settings: RuntimeSettings,
    ) -> Self {
        Self::try_new(components, persistent_graph, settings)
            .expect("initialize Everything runtime state")
    }

    pub fn try_new(
        components: RuntimeComponents,
        persistent_graph: PersistentCodeGraph,
        settings: RuntimeSettings,
    ) -> Result<Self> {
        let bootstrapper = WorkspaceBootstrapper::new(components.file_system.clone());
        let planner = ArchitecturePlanner::new(components.model.clone());
        let retrieval = RetrievalService::new(persistent_graph.clone());
        let state_store = StateStore::new(settings.data_dir.join("state/runtime.sqlite3"))?;
        state_store.import_legacy_json_dir(&settings.data_dir.join("runs"))?;
        state_store.reconcile_interrupted_tool_invocations()?;
        state_store.reconcile_interrupted_model_invocations()?;
        reconcile_interrupted_runs(&state_store)?;
        let artifact_store = ArtifactStore::new(settings.data_dir.join("artifacts"))?;
        let memory_store =
            everything_memory::MemoryStore::new(settings.data_dir.join("state/memory.sqlite3"))?;
        let skill_registry = everything_skills::SkillRegistry::new_for_workspace(
            settings.data_dir.join("state/skills.sqlite3"),
            settings.data_dir.join("skills"),
        )?;
        let connector_runtime = everything_connectors::ConnectorRuntime::new(
            settings.data_dir.join("state/connectors.sqlite3"),
            settings.autonomy.oauth_callback_port,
            settings.connectors.http_timeout_millis,
            settings.connectors.max_response_bytes,
            settings.connectors.allow_custom_connectors,
        )?;
        let research_runtime = everything_research::ResearchRuntime::new(
            settings.data_dir.join("state/research.sqlite3"),
            settings.research.clone(),
        )?;
        let trusted_workspace = settings.tools.trust_mode == ToolTrustMode::TrustedWorkspace;
        let mut tool_policy = everything_tools::ToolPolicy::default();
        if trusted_workspace {
            tool_policy.set(PermissionScope::WorkspaceWrite, PolicyDecision::Allow);
            tool_policy.set(PermissionScope::ProcessExecute, PolicyDecision::Allow);
            tool_policy.set(PermissionScope::GitWrite, PolicyDecision::Allow);
        }
        let tool_runtime = everything_tools::ToolRuntime::new_with_security(
            &settings.workspace_path,
            tool_policy,
            settings.tools.allowed_programs.clone(),
            settings.tools.default_timeout_millis,
            settings.tools.max_output_bytes,
            trusted_workspace,
            settings.tools.os_sandbox_enabled,
        )?;
        Ok(Self {
            components,
            bootstrapper,
            planner,
            retrieval,
            persistent_graph,
            settings,
            state_store,
            artifact_store,
            tool_runtime,
            memory_store,
            skill_registry,
            verifier: everything_verifier::DeterministicVerifier,
            connector_runtime,
            research_runtime,
        })
    }

    pub fn bootstrap(&self, root: &Path) -> Result<BootstrapReport> {
        let mut report = self.bootstrapper.bootstrap(root)?;
        let graph_started = Instant::now();
        self.persistent_graph.index_workspace(root)?;
        let graph_stats = self.persistent_graph.stats()?;
        report.project.graph_node_count = graph_stats.active_entities;
        report.project.graph_edge_count = graph_stats.active_relations;
        report.metrics.graph_millis = graph_started.elapsed().as_millis();
        Ok(report)
    }

    fn build_context_with_memory(
        &self,
        request: &TaskRequest,
        model_profile: &everything_domain::ModelCapabilityProfile,
    ) -> Result<RetrievalContextPack> {
        let mut context_pack = self.retrieval.build_context(request, model_profile)?;
        let workspace_key = request
            .workspace_path
            .canonicalize()
            .unwrap_or_else(|_| request.workspace_path.clone())
            .display()
            .to_string();
        let memories = self.memory_store.search(&MemoryQuery {
            query: request.objective.clone(),
            scope: None,
            workspace_key: Some(workspace_key),
            limit: match request.mode {
                ExecutionMode::Fast => 4,
                ExecutionMode::Balanced => 8,
                ExecutionMode::Deep => 16,
            },
            include_superseded: false,
        })?;
        self.retrieval
            .enrich_with_memory(&mut context_pack, &memories);
        if should_auto_research(request, &self.settings) {
            let web_request = automatic_research_request(request);
            match self.research_runtime.search(&web_request) {
                Ok(report) => self
                    .retrieval
                    .enrich_with_research(&mut context_pack, &report),
                Err(error) => context_pack.policy.reasons.push(format!(
                    "automatic web research was requested but unavailable: {error}"
                )),
            }
        }
        Ok(context_pack)
    }

    pub fn doctor(&self, root: &Path) -> Result<RuntimeDoctorReport> {
        let report = self.bootstrap(root)?;
        let model_health = self
            .components
            .model
            .health_check()
            .unwrap_or_else(|error| everything_domain::ModelHealth {
                status: everything_domain::ModelHealthStatus::Unavailable,
                available: false,
                adapter: self.components.model.name().to_owned(),
                detail: error.to_string(),
                primary_available: false,
                fallback_available: false,
                fallback_active: false,
            });
        let mut checks = Vec::new();
        checks.push(RuntimeDoctorCheck {
            check_id: "model".to_owned(),
            label: "Local model".to_owned(),
            status: match model_health.status {
                everything_domain::ModelHealthStatus::Healthy => DoctorCheckStatus::Healthy,
                everything_domain::ModelHealthStatus::Degraded => DoctorCheckStatus::Degraded,
                everything_domain::ModelHealthStatus::Unavailable => DoctorCheckStatus::Failed,
            },
            detail: model_health.detail.clone(),
            remediation: (!model_health.available).then(|| {
                format!(
                    "Start Ollama and install the configured model '{}'.",
                    self.settings.model.model_name
                )
            }),
        });
        checks.push(RuntimeDoctorCheck {
            check_id: "graph".to_owned(),
            label: "Persistent code graph".to_owned(),
            status: if report.project.file_count == 0 || report.project.graph_node_count > 0 {
                DoctorCheckStatus::Healthy
            } else {
                DoctorCheckStatus::Degraded
            },
            detail: format!(
                "{} files, {} entities, {} relations",
                report.project.file_count,
                report.project.graph_node_count,
                report.project.graph_edge_count
            ),
            remediation: (report.project.file_count > 0 && report.project.graph_node_count == 0)
                .then(|| "Re-index the workspace and inspect parser diagnostics.".to_owned()),
        });
        checks.push(doctor_check(
            "state-store",
            "Durable runtime state",
            self.state_store.load_journals().map(|journals| {
                format!(
                    "SQLite/WAL readable; {} run journals",
                    journals.entries.len()
                )
            }),
            "Inspect the runtime SQLite database and filesystem permissions.",
        ));
        let workspace_key = root
            .canonicalize()
            .unwrap_or_else(|_| root.to_path_buf())
            .display()
            .to_string();
        checks.push(doctor_check(
            "memory",
            "Structured memory",
            self.memory_store
                .search(&MemoryQuery {
                    query: String::new(),
                    scope: None,
                    workspace_key: Some(workspace_key),
                    limit: 1,
                    include_superseded: false,
                })
                .map(|_| "Memory SQLite/FTS index is readable".to_owned()),
            "Inspect the memory database or reset only the corrupted memory store.",
        ));
        checks.push(doctor_check(
            "skills",
            "Skill registry",
            self.skill_registry
                .list()
                .map(|skills| format!("{} compatible or inspectable skill packages", skills.len())),
            "Refresh the registry and remove malformed skill packages.",
        ));
        checks.push(match self.connector_runtime.list() {
            Ok(connectors) if self.connector_runtime.vault_available() => RuntimeDoctorCheck {
                check_id: "connectors".to_owned(),
                label: "Connectors and secret vault".to_owned(),
                status: DoctorCheckStatus::Healthy,
                detail: format!(
                    "{} official connectors; secret vault backend '{}' available",
                    connectors.len(),
                    self.connector_runtime.vault_backend()
                ),
                remediation: None,
            },
            Ok(connectors) => RuntimeDoctorCheck {
                check_id: "connectors".to_owned(),
                label: "Connectors and secret vault".to_owned(),
                status: DoctorCheckStatus::Degraded,
                detail: format!(
                    "{} connectors are discoverable, but secret vault backend '{}' is unavailable",
                    connectors.len(),
                    self.connector_runtime.vault_backend()
                ),
                remediation: Some(
                    "Install or unlock the platform credential vault before OAuth connection."
                        .to_owned(),
                ),
            },
            Err(error) => failed_doctor_check(
                "connectors",
                "Connectors and secret vault",
                error,
                "Inspect connector state and OS credential-vault availability.",
            ),
        });
        checks.push(match self.research_runtime.status() {
            Ok(status) if status.enabled && status.providers.iter().any(|provider| provider.available) => RuntimeDoctorCheck {
                check_id: "research".to_owned(),
                label: "Web research runtime".to_owned(),
                status: DoctorCheckStatus::Healthy,
                detail: format!(
                    "{} provider(s) available; {} cached queries and {} cached documents",
                    status.providers.iter().filter(|provider| provider.available).count(),
                    status.cached_queries,
                    status.cached_documents
                ),
                remediation: None,
            },
            Ok(status) if !status.enabled => RuntimeDoctorCheck {
                check_id: "research".to_owned(),
                label: "Web research runtime".to_owned(),
                status: DoctorCheckStatus::Degraded,
                detail: "Web research is disabled by runtime policy".to_owned(),
                remediation: Some("Enable [research].enabled in everything.toml when current external evidence is required.".to_owned()),
            },
            Ok(status) => RuntimeDoctorCheck {
                check_id: "research".to_owned(),
                label: "Web research runtime".to_owned(),
                status: DoctorCheckStatus::Degraded,
                detail: status.warnings.join("; "),
                remediation: Some("Start the local SearXNG sidecar or install curl for the keyless HTTPS fallback providers.".to_owned()),
            },
            Err(error) => failed_doctor_check(
                "research",
                "Web research runtime",
                error,
                "Inspect research cache permissions and provider configuration.",
            ),
        });
        checks.push(doctor_check(
            "scheduler",
            "Autonomous scheduler",
            self.state_store.list_automations().map(|automations| {
                format!("Scheduler state readable; {} routines", automations.len())
            }),
            "Inspect automation state and the background service logs.",
        ));
        let sandbox = self.tool_runtime.sandbox_description();
        let sandbox_healthy =
            sandbox.starts_with("bubblewrap") || sandbox.starts_with("sandbox-exec");
        checks.push(RuntimeDoctorCheck {
            check_id: "tool-sandbox".to_owned(),
            label: "Workspace tool sandbox".to_owned(),
            status: if sandbox_healthy {
                DoctorCheckStatus::Healthy
            } else {
                DoctorCheckStatus::Degraded
            },
            detail: format!("{}; trust mode {:?}", sandbox, self.settings.tools.trust_mode),
            remediation: (!sandbox_healthy).then(|| {
                "Install the supported OS sandbox helper; broad executable access remains disabled until then."
                    .to_owned()
            }),
        });
        checks.push(data_directory_check(&self.settings.data_dir));

        let overall_status = if checks
            .iter()
            .any(|check| check.status == DoctorCheckStatus::Failed)
        {
            DoctorCheckStatus::Failed
        } else if checks
            .iter()
            .any(|check| check.status == DoctorCheckStatus::Degraded)
        {
            DoctorCheckStatus::Degraded
        } else {
            DoctorCheckStatus::Healthy
        };

        Ok(RuntimeDoctorReport {
            project: report.project,
            adapters: AdapterSummary {
                filesystem: self.components.file_system.name().to_owned(),
                model: self.components.model.name().to_owned(),
                command: self.components.command.name().to_owned(),
            },
            model_health,
            snapshot_stats: report.snapshot.stats.clone(),
            bootstrap_metrics: report.metrics,
            overall_status,
            checks,
        })
    }

    pub fn plan(&self, request: &TaskRequest) -> Result<PlanningDocument> {
        let report = self.bootstrap(&request.workspace_path)?;
        let model_profile = self.components.model.capability_profile();
        let context_pack = self.build_context_with_memory(request, &model_profile)?;
        self.planner.create_plan(request, &report, &context_pack)
    }

    pub fn plan_and_record(
        &self,
        request: &TaskRequest,
    ) -> Result<(PlanningDocument, RunJournal, std::path::PathBuf)> {
        self.plan_and_record_with_events(request, |_, _| {})
    }

    pub fn plan_and_record_with_events<F>(
        &self,
        request: &TaskRequest,
        mut on_event: F,
    ) -> Result<(PlanningDocument, RunJournal, std::path::PathBuf)>
    where
        F: FnMut(&str, &RunEvent),
    {
        let mut journal = RunJournalBuilder::new_for_task(
            &request.objective,
            self.components.model.name(),
            request.mode,
        );
        emit_event(
            &mut journal,
            &mut on_event,
            "run",
            format!("status=started objective={}", request.objective),
        );
        emit_event(
            &mut journal,
            &mut on_event,
            "bootstrap",
            format!("workspace={}", request.workspace_path.display()),
        );

        self.state_store
            .save_journal(&journal.snapshot(RunStatus::Started, None))?;

        let report = match self.bootstrap(&request.workspace_path) {
            Ok(report) => report,
            Err(error) => {
                emit_event(
                    &mut journal,
                    &mut on_event,
                    "run",
                    format!("status=failed error={error}"),
                );
                journal.set_failure_class(everything_domain::FailureClass::Internal);
                self.state_store
                    .save_journal(&journal.snapshot(RunStatus::Failed, None))?;
                return Err(error);
            }
        };
        emit_event(
            &mut journal,
            &mut on_event,
            "graph",
            format!(
                "nodes={}, edges={}, snapshot_ms={}, graph_ms={}",
                report.project.graph_node_count,
                report.project.graph_edge_count,
                report.metrics.snapshot_millis,
                report.metrics.graph_millis
            ),
        );
        emit_event(
            &mut journal,
            &mut on_event,
            "cache",
            format!(
                "hits={}, misses={}, bytes_read={}",
                report.snapshot.stats.cache_hits,
                report.snapshot.stats.cache_misses,
                report.snapshot.stats.bytes_read
            ),
        );

        let model_profile = self.components.model.capability_profile();
        let context_pack = match self.build_context_with_memory(request, &model_profile) {
            Ok(context_pack) => context_pack,
            Err(error) => {
                emit_event(
                    &mut journal,
                    &mut on_event,
                    "run",
                    format!("status=failed stage=retrieval error={error}"),
                );
                journal.set_failure_class(everything_domain::FailureClass::Internal);
                self.state_store
                    .save_journal(&journal.snapshot(RunStatus::Failed, None))?;
                return Err(error);
            }
        };
        let context_bytes = serde_json::to_vec_pretty(&context_pack)?;
        let context_artifact = self.artifact_store.persist(
            journal.run_id(),
            ArtifactKind::ContextPack,
            "application/json; charset=utf-8",
            "graph-retrieval",
            &context_bytes,
        )?;
        self.state_store.save_artifact(&context_artifact)?;
        journal.add_artifact(context_artifact.clone());
        emit_event(
            &mut journal,
            &mut on_event,
            "retrieval",
            format!(
                "graph_revision={} selected_symbols={} selected_files={} excerpt_bytes={} estimated_tokens={} prompt_budget={} verifier={:?} model_tier={:?}",
                context_pack.graph_revision,
                context_pack.selections.len(),
                context_pack.related_files.len(),
                context_pack.total_excerpt_bytes,
                context_pack.total_estimated_tokens,
                context_pack.policy.prompt_token_budget,
                context_pack.policy.verifier_strength,
                context_pack.model_profile.quality_tier
            ),
        );
        emit_event(
            &mut journal,
            &mut on_event,
            "checkpoint",
            "safe_to_resume=true boundary=context_ready".to_owned(),
        );

        self.state_store
            .save_journal(&journal.snapshot(RunStatus::Started, None))?;
        let checkpoint = Checkpoint {
            checkpoint_id: CheckpointId::new(format!(
                "{}-checkpoint-bootstrap",
                journal.run_id()
            )),
            run_id: RunId::new(journal.run_id()),
            stage_id: Some(StageId::new(format!("{}:bootstrap", journal.run_id()))),
            event_sequence: journal.last_event_sequence().unwrap_or_default(),
            created_at_epoch_millis: now_epoch_millis(),
            safe_to_resume: true,
            summary: "Workspace graph retrieval completed; planning can be retried from this context boundary."
                .to_owned(),
            artifact_ids: vec![ArtifactId::new(context_artifact.artifact_id.clone())],
        };
        self.state_store.save_checkpoint(&checkpoint)?;

        let planning_outcome = match self.planner.create_plan_for_run(
            request,
            &report,
            &context_pack,
            journal.run_id(),
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                emit_event(
                    &mut journal,
                    &mut on_event,
                    "run",
                    format!("status=failed error={error}"),
                );
                journal.set_failure_class(everything_domain::FailureClass::Model);
                self.state_store
                    .save_journal(&journal.snapshot(RunStatus::Failed, None))?;
                return Err(error);
            }
        };
        let document = planning_outcome.document;
        let mut model_invocation = planning_outcome.invocation;
        let plan_artifact = self.artifact_store.persist(
            journal.run_id(),
            ArtifactKind::Plan,
            "text/markdown; charset=utf-8",
            "architecture-planner",
            document.content.as_bytes(),
        )?;
        self.state_store.save_artifact(&plan_artifact)?;
        model_invocation.response_artifact_id =
            Some(ArtifactId::new(plan_artifact.artifact_id.clone()));
        self.state_store.save_model_invocation(&model_invocation)?;
        journal.add_artifact(plan_artifact.clone());
        journal.set_generated_by(&document.generated_by);
        emit_event(
            &mut journal,
            &mut on_event,
            "model",
            if document.fallback_used {
                format!(
                    "fallback=true generated_by={} reason={} duration_ms={} prompt_tokens={} output_bytes={}",
                    document.generated_by,
                    document.fallback_reason.as_deref().unwrap_or("unknown"),
                    model_invocation.duration_millis,
                    model_invocation.prompt_estimated_tokens,
                    model_invocation.output_bytes
                )
            } else {
                format!(
                    "fallback=false generated_by={} duration_ms={} prompt_tokens={} output_bytes={}",
                    document.generated_by,
                    model_invocation.duration_millis,
                    model_invocation.prompt_estimated_tokens,
                    model_invocation.output_bytes
                )
            },
        );
        emit_event(
            &mut journal,
            &mut on_event,
            "plan",
            format!("generated_by={}", document.generated_by),
        );
        emit_event(
            &mut journal,
            &mut on_event,
            "run",
            "status=completed".to_owned(),
        );

        let journal = journal.build(
            RunStatus::Completed,
            Some(plan_artifact.object_path.clone()),
        );
        let path = self.state_store.save_journal(&journal)?;
        Ok((document, journal, path))
    }

    pub fn graph_summary(&self, root: &Path) -> Result<GraphSummaryResponse> {
        self.persistent_graph.index_workspace(root)?;
        let stats = self.persistent_graph.stats()?;
        let packages = self.persistent_graph.package_references()?;
        Ok(GraphSummaryResponse {
            summary: format!(
                "Persistent code graph: {} active files, {} active entities, {} active relations (schema v{}).",
                stats.active_files,
                stats.active_entities,
                stats.active_relations,
                stats.schema_version
            ),
            package_references: packages
                .into_iter()
                .map(|package| PackageReferenceSummary {
                    label: package.label,
                    outbound_references: package.outbound_references,
                })
                .collect(),
        })
    }

    pub fn compatibility_query(
        &self,
        root: &Path,
        term: &str,
        limit: usize,
    ) -> Result<everything_graph::GraphQueryResult> {
        self.persistent_graph.index_workspace(root)?;
        Ok(everything_graph::GraphQueryResult {
            matched_nodes: self
                .persistent_graph
                .search(term, limit)?
                .into_iter()
                .map(|result| everything_graph::GraphNode {
                    id: result.entity.id,
                    label: result.entity.qualified_name,
                    kind: compatibility_node_kind(result.entity.kind),
                    source_path: result.entity.file_path,
                })
                .collect(),
        })
    }

    pub fn compatibility_impact(
        &self,
        root: &Path,
        term: &str,
        depth: usize,
        limit: usize,
    ) -> Result<everything_graph::GraphImpactReport> {
        self.persistent_graph.index_workspace(root)?;
        let report = self.persistent_graph.impact(term, depth, limit)?;
        Ok(everything_graph::GraphImpactReport {
            root: everything_graph::GraphNode {
                id: report.root.id,
                label: report.root.qualified_name,
                kind: compatibility_node_kind(report.root.kind),
                source_path: report.root.file_path,
            },
            depth: report.depth,
            affected_nodes: report
                .entities
                .into_iter()
                .skip(1)
                .map(|entity| everything_graph::GraphNode {
                    id: entity.id,
                    label: entity.qualified_name,
                    kind: compatibility_node_kind(entity.kind),
                    source_path: entity.file_path,
                })
                .collect(),
        })
    }

    pub fn plan_response(&self, request: &TaskRequest) -> Result<PlanResponse> {
        let (document, journal, journal_path) = self.plan_and_record(request)?;
        Ok(PlanResponse {
            run_id: journal.run_id.clone(),
            document,
            journal_path,
            artifact: journal
                .artifacts
                .iter()
                .find(|artifact| artifact.kind == ArtifactKind::Plan)
                .cloned(),
        })
    }

    pub fn list_runs(&self) -> Result<Vec<RunSummary>> {
        Ok(summarize_journals(
            self.state_store.load_journals()?.entries,
        ))
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<RunJournal>> {
        self.state_store.get_journal(run_id)
    }

    pub fn recoverable_runs(&self) -> Result<Vec<RunJournal>> {
        self.state_store.recoverable_journals()
    }

    pub fn list_artifacts(&self, run_id: &str) -> Result<Vec<ArtifactDescriptor>> {
        self.state_store.list_artifacts(run_id)
    }

    pub fn list_checkpoints(&self, run_id: &str) -> Result<Vec<Checkpoint>> {
        self.state_store.list_checkpoints(run_id)
    }

    pub fn get_artifact(&self, artifact_id: &str) -> Result<Option<ArtifactContentResponse>> {
        let Some(descriptor) = self.state_store.get_artifact(artifact_id)? else {
            return Ok(None);
        };
        let bytes = self.artifact_store.read(&descriptor)?;
        let content = String::from_utf8(bytes)
            .map_err(|error| anyhow::anyhow!("artifact is not valid UTF-8: {error}"))?;
        Ok(Some(ArtifactContentResponse {
            descriptor,
            encoding: "utf-8".to_owned(),
            content,
        }))
    }

    pub fn metrics(&self) -> Result<RuntimeMetricsSnapshot> {
        metrics_snapshot(&self.settings.data_dir, &self.state_store)
    }

    pub fn benchmark_bootstrap(&self, root: &Path, iterations: usize) -> Result<BenchmarkRecord> {
        let total_iterations = iterations.max(1);
        let mut reports = Vec::with_capacity(total_iterations);
        for _ in 0..total_iterations {
            reports.push(self.bootstrap(root)?);
        }

        let record = build_bootstrap_benchmark(root, total_iterations, &reports);
        persist_benchmark(&self.settings.data_dir, &record)?;
        Ok(record)
    }

    pub fn refresh_code_graph(&self, root: &Path) -> Result<CodeGraphIndexReport> {
        self.persistent_graph.index_workspace(root)
    }

    pub fn persistent_graph_stats(&self) -> Result<PersistentGraphStats> {
        self.persistent_graph.stats()
    }

    pub fn code_search(&self, term: &str, limit: usize) -> Result<Vec<CodeGraphSearchResult>> {
        self.persistent_graph.search(term, limit)
    }

    pub fn web_search(&self, request: &WebSearchRequest) -> Result<ResearchReport> {
        self.research_runtime.search(request)
    }

    pub fn web_fetch(&self, request: &WebFetchRequest) -> Result<WebFetchResponse> {
        self.research_runtime.fetch(request)
    }

    pub fn research_status(&self) -> Result<ResearchStatus> {
        self.research_runtime.status()
    }

    pub fn purge_research_cache(&self) -> Result<usize> {
        self.research_runtime.purge_expired()
    }

    pub fn analyze_code_change(
        &self,
        request: &CodeGraphChangeImpactRequest,
    ) -> Result<CodeGraphChangeImpactReport> {
        self.persistent_graph.analyze_change(request)
    }

    pub fn code_impact(
        &self,
        term: &str,
        depth: usize,
        limit: usize,
    ) -> Result<CodeGraphImpactReport> {
        self.persistent_graph.impact(term, depth, limit)
    }

    pub fn code_traverse(
        &self,
        term: &str,
        direction: GraphDirection,
        relation_kinds: &[CodeRelationKind],
        depth: usize,
        limit: usize,
    ) -> Result<CodeGraphImpactReport> {
        self.persistent_graph
            .traverse(term, direction, relation_kinds, depth, limit)
    }

    pub fn code_path(&self, from: &str, to: &str, depth: usize) -> Result<Option<CodeGraphPath>> {
        self.persistent_graph.path(from, to, depth)
    }

    pub fn code_path_with_options(
        &self,
        from: &str,
        to: &str,
        depth: usize,
        direction: GraphDirection,
        relation_kinds: &[CodeRelationKind],
    ) -> Result<Option<CodeGraphPath>> {
        self.persistent_graph
            .path_with_options(from, to, depth, direction, relation_kinds)
    }

    pub fn model_capability_profile(&self) -> everything_domain::ModelCapabilityProfile {
        self.components.model.capability_profile()
    }

    pub fn discover_models(&self) -> Result<Vec<everything_domain::DiscoveredModel>> {
        self.components.model.discover_models()
    }

    pub fn get_model_invocation(
        &self,
        invocation_id: &str,
    ) -> Result<Option<everything_domain::ModelInvocationRecord>> {
        self.state_store.get_model_invocation(invocation_id)
    }

    pub fn list_model_invocations(
        &self,
        run_id: &str,
    ) -> Result<Vec<everything_domain::ModelInvocationRecord>> {
        self.state_store.list_model_invocations(run_id)
    }

    pub fn settings(&self) -> &RuntimeSettings {
        &self.settings
    }

    pub fn components(&self) -> &RuntimeComponents {
        &self.components
    }
}

fn doctor_check(
    check_id: &str,
    label: &str,
    result: Result<String>,
    remediation: &str,
) -> RuntimeDoctorCheck {
    match result {
        Ok(detail) => RuntimeDoctorCheck {
            check_id: check_id.to_owned(),
            label: label.to_owned(),
            status: DoctorCheckStatus::Healthy,
            detail,
            remediation: None,
        },
        Err(error) => failed_doctor_check(check_id, label, error, remediation),
    }
}

fn failed_doctor_check(
    check_id: &str,
    label: &str,
    error: impl std::fmt::Display,
    remediation: &str,
) -> RuntimeDoctorCheck {
    RuntimeDoctorCheck {
        check_id: check_id.to_owned(),
        label: label.to_owned(),
        status: DoctorCheckStatus::Failed,
        detail: error.to_string(),
        remediation: Some(remediation.to_owned()),
    }
}

fn data_directory_check(data_dir: &Path) -> RuntimeDoctorCheck {
    let probe = data_dir.join(format!(".doctor-write-probe-{}", std::process::id()));
    let result = (|| -> Result<String> {
        std::fs::create_dir_all(data_dir)?;
        std::fs::write(&probe, b"everything-doctor")?;
        std::fs::remove_file(&probe)?;
        Ok(format!(
            "Runtime data directory is writable: {}",
            data_dir.display()
        ))
    })();
    let _ = std::fs::remove_file(&probe);
    doctor_check(
        "data-directory",
        "Runtime data directory",
        result,
        "Repair ownership and permissions for the Everything data directory.",
    )
}

pub fn build_runtime(workspace: &Path) -> Result<ModularRuntime> {
    let settings = load_settings(workspace)?;
    build_runtime_from_settings(settings)
}

pub fn build_runtime_with_oauth_port(
    workspace: &Path,
    oauth_callback_port: u16,
) -> Result<ModularRuntime> {
    let mut settings = load_settings(workspace)?;
    settings.autonomy.oauth_callback_port = oauth_callback_port;
    build_runtime_from_settings(settings)
}

fn build_runtime_from_settings(settings: RuntimeSettings) -> Result<ModularRuntime> {
    ensure_data_dir(&settings.data_dir)?;

    let primary_model: Box<dyn ModelAdapter> = match settings.model.backend {
        ModelBackend::Loopback => {
            Box::new(LocalModelAdapter::new(settings.model.model_name.clone()))
        }
        ModelBackend::Ollama => Box::new(
            OllamaModelAdapter::new(
                settings.model.model_name.clone(),
                settings.model.binary.clone(),
                settings.model.keep_alive.clone(),
                settings.model.hide_thinking,
            )
            .with_limits(
                settings.model.timeout_millis,
                settings.model.max_output_bytes,
            )
            .with_context_override(
                settings.model.context_window_tokens,
                settings.model.safe_context_tokens,
            ),
        ),
    };

    let model: Arc<dyn ModelAdapter> = match settings.model.fallback_policy {
        ModelFallbackPolicy::Disabled => Arc::from(primary_model),
        ModelFallbackPolicy::Reported => {
            let fallback: Box<dyn ModelAdapter> =
                Box::new(LocalModelAdapter::new("loopback-fallback"));
            Arc::new(ResilientModelAdapter::new(primary_model, fallback))
        }
    };

    let components = RuntimeComponents {
        file_system: Arc::new(everything_adapters::LocalFileSystemAdapter::new(
            settings.data_dir.join("cache"),
        )),
        model,
        command: Arc::new(everything_adapters::LocalCommandExecutor),
    };

    let persistent_graph =
        PersistentCodeGraph::new(settings.data_dir.join("graph/codegraph.sqlite3"));
    ModularRuntime::try_new(components, persistent_graph, settings)
}

pub fn build_task_request_for_workspace(
    workspace: &Path,
    objective: String,
    mode: ExecutionMode,
) -> Result<TaskRequest> {
    let settings = load_settings(workspace)?;
    Ok(build_task_request(&settings, objective, mode))
}

fn compatibility_node_kind(kind: CodeEntityKind) -> everything_graph::GraphNodeKind {
    match kind {
        CodeEntityKind::Project => everything_graph::GraphNodeKind::Package,
        CodeEntityKind::Package => everything_graph::GraphNodeKind::Package,
        CodeEntityKind::File => everything_graph::GraphNodeKind::File,
        CodeEntityKind::Module | CodeEntityKind::Namespace => {
            everything_graph::GraphNodeKind::Module
        }
        CodeEntityKind::Function | CodeEntityKind::Method => {
            everything_graph::GraphNodeKind::Function
        }
        CodeEntityKind::Test | CodeEntityKind::Route | CodeEntityKind::Event => {
            everything_graph::GraphNodeKind::Function
        }
        CodeEntityKind::Import
        | CodeEntityKind::Constant
        | CodeEntityKind::Variable
        | CodeEntityKind::EnvironmentVariable
        | CodeEntityKind::ConfigurationKey
        | CodeEntityKind::DatabaseObject
        | CodeEntityKind::External => everything_graph::GraphNodeKind::Type,
        CodeEntityKind::Class
        | CodeEntityKind::Struct
        | CodeEntityKind::Enum
        | CodeEntityKind::Trait
        | CodeEntityKind::Interface
        | CodeEntityKind::TypeAlias
        | CodeEntityKind::Implementation => everything_graph::GraphNodeKind::Type,
    }
}

fn emit_event<F>(journal: &mut RunJournalBuilder, on_event: &mut F, stage: &str, detail: String)
where
    F: FnMut(&str, &RunEvent),
{
    let event = journal.push(stage.to_owned(), detail);
    on_event(journal.run_id(), &event);
}

fn should_auto_research(request: &TaskRequest, settings: &RuntimeSettings) -> bool {
    if !settings.research.enabled || !settings.research.auto_research {
        return false;
    }
    let objective = request.objective.to_lowercase();
    let offline_markers = [
        "[offline]",
        "offline",
        "local only",
        "yalnızca yerel",
        "sadece yerel",
        "internete çıkma",
        "web kullanma",
        "no web",
    ];
    if offline_markers
        .iter()
        .any(|marker| objective.contains(marker))
    {
        return false;
    }
    let external_markers = [
        "[web]",
        "search the web",
        "araştır",
        "kaynak bul",
        "latest",
        "current",
        "today",
        "recent",
        "news",
        "web",
        "internet",
        "compare",
        "recommend",
        "documentation",
        "official docs",
        "docs",
        "api",
        "sdk",
        "release",
        "version",
        "standard",
        "specification",
        "rfc",
        "cve",
        "security advisory",
        "github",
        "crate",
        "npm",
        "pypi",
        "package",
        "dependency",
        "deprecated",
        "breaking change",
        "compatibility",
        "güncel",
        "bugün",
        "internette",
        "webde",
        "dokümantasyon",
        "sürüm",
        "yayın",
        "standart",
        "güvenlik açığı",
        "karşılaştır",
        "öneri",
    ];
    external_markers
        .iter()
        .any(|marker| objective.contains(marker))
        || matches!(request.mode, ExecutionMode::Deep)
}

fn automatic_research_request(request: &TaskRequest) -> WebSearchRequest {
    let objective = request.objective.trim();
    let lowered = objective.to_lowercase();
    let freshness = if [
        "today", "latest", "current", "recent", "news", "bugün", "güncel",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
    {
        ResearchFreshness::Month
    } else {
        ResearchFreshness::Year
    };
    let (max_results, fetch_pages) = match request.mode {
        ExecutionMode::Fast => (5, 1),
        ExecutionMode::Balanced => (8, 3),
        ExecutionMode::Deep => (14, 6),
    };
    WebSearchRequest {
        query: format!("{objective} official documentation primary source"),
        mode: ResearchMode::Technical,
        freshness,
        max_results,
        fetch_pages,
        allowed_domains: Vec::new(),
        blocked_domains: Vec::new(),
        force_refresh: false,
    }
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::{ModularRuntime, RuntimeComponents};
    use anyhow::{Result, anyhow};
    use everything_adapters::{
        CommandExecutor, CommandOutput, CommandRequest, FileSystemAdapter, LocalModelAdapter,
    };
    use everything_domain::{
        ExecutionMode, RunStatus, RuntimeSettings, TaskRequest, WorkspaceSnapshot,
    };
    use everything_graph::PersistentCodeGraph;
    use std::path::Path;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct FailingFileSystem;

    impl FileSystemAdapter for FailingFileSystem {
        fn name(&self) -> &str {
            "failing-filesystem"
        }

        fn snapshot(&self, _root: &Path) -> Result<WorkspaceSnapshot> {
            Err(anyhow!("snapshot failed"))
        }
    }

    struct NoopCommand;

    impl CommandExecutor for NoopCommand {
        fn name(&self) -> &str {
            "noop-command"
        }

        fn execute(&self, _request: CommandRequest) -> Result<CommandOutput> {
            unreachable!("command execution is not part of planning")
        }
    }

    #[test]
    fn failed_bootstrap_leaves_a_resumable_journal() {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("everything-runtime-{suffix}"));
        let data_dir = root.join("state");
        let mut settings = RuntimeSettings::for_workspace(root.clone());
        settings.data_dir = data_dir.clone();

        let runtime = ModularRuntime::new(
            RuntimeComponents {
                file_system: Arc::new(FailingFileSystem),
                model: Arc::new(LocalModelAdapter::new("test-model")),
                command: Arc::new(NoopCommand),
            },
            PersistentCodeGraph::new(data_dir.join("graph/codegraph.sqlite3")),
            settings,
        );
        let request = TaskRequest {
            objective: "test durable failure".to_owned(),
            mode: ExecutionMode::Fast,
            workspace_path: root.clone(),
        };

        let error = runtime
            .plan_and_record(&request)
            .expect_err("bootstrap must fail");
        assert!(error.to_string().contains("snapshot failed"));

        let journals = runtime.list_runs().expect("load failed journal");
        assert_eq!(journals.len(), 1);
        assert_eq!(journals[0].status, RunStatus::Failed);
        assert_eq!(journals[0].last_stage.as_deref(), Some("run"));

        std::fs::remove_dir_all(root).expect("remove test state");
    }
}
