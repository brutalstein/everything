use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use everything_runtime::{ModularRuntime, build_runtime, build_task_request_for_workspace};
use serde::Serialize;
use std::path::{Path, PathBuf};

use everything_domain::{
    AutomationRunNowRequest, AutomationUpsertRequest, ConnectorActionRequest,
    ConnectorConfigureRequest, ConnectorProvider, EditProposalRequest, ExecutionMode, InvocationId,
    MemoryQuery, MemoryScope, MemoryUpsertRequest, ModuleDescriptor, OAuthStartRequest,
    PatchExecutionRequest, ResearchFreshness, ResearchMode, SkillExecutionRequest,
    ToolInvocationRequest, WebFetchRequest, WebSearchRequest,
};

#[derive(Parser)]
#[command(name = "everything")]
#[command(version)]
#[command(about = "Native-first runtime for local coding agents.")]
struct Cli {
    #[arg(long, default_value = ".")]
    workspace: PathBuf,

    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Doctor,
    Models,
    ModelCapabilities,
    Modules,
    Graph,
    CodeGraph {
        #[command(subcommand)]
        command: CodeGraphCommand,
    },
    Query {
        term: String,
    },
    Impact {
        term: String,
        #[arg(long)]
        depth: Option<usize>,
    },
    Plan {
        #[arg(value_enum, long, default_value_t = ModeArg::Balanced)]
        mode: ModeArg,
        objective: Vec<String>,
    },
    ProposeEdit {
        #[arg(value_enum, long, default_value_t = ModeArg::Balanced)]
        mode: ModeArg,
        #[arg(long)]
        skill: Option<String>,
        objective: Vec<String>,
    },
    Runs,
    Run {
        run_id: String,
    },
    RecoverableRuns,
    Artifacts {
        run_id: String,
    },
    Checkpoints {
        run_id: String,
    },
    Artifact {
        artifact_id: String,
    },
    Tools,
    ModelInvocations {
        run_id: String,
    },
    ModelInvocation {
        invocation_id: String,
    },
    ToolInvocations {
        run_id: String,
    },
    InvokeTool {
        invocation_id: String,
        request: PathBuf,
    },
    CancelTool {
        invocation_id: String,
    },
    ExecutePatch {
        request: PathBuf,
    },
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommand,
    },
    Connectors {
        #[command(subcommand)]
        command: ConnectorsCommand,
    },
    Automations {
        #[command(subcommand)]
        command: AutomationsCommand,
    },
    Research {
        #[command(subcommand)]
        command: ResearchCommand,
    },
    Metrics,
    Bench {
        #[arg(long, default_value_t = 3)]
        iterations: usize,
    },
}

#[derive(Subcommand)]
enum CodeGraphCommand {
    Index,
    Stats,
    Search {
        term: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    Impact {
        term: String,
        #[arg(long)]
        depth: Option<usize>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long, value_enum, default_value_t = DirectionArg::Inbound)]
        direction: DirectionArg,
        #[arg(long, value_delimiter = ',')]
        relations: Vec<String>,
    },
    Path {
        from: String,
        to: String,
        #[arg(long)]
        depth: Option<usize>,
        #[arg(long, value_enum, default_value_t = DirectionArg::Outbound)]
        direction: DirectionArg,
        #[arg(long, value_delimiter = ',')]
        relations: Vec<String>,
    },
    ChangeImpact {
        request: PathBuf,
    },
}

#[derive(Subcommand)]
enum ResearchCommand {
    Status,
    Search {
        query: Vec<String>,
        #[arg(long, value_enum, default_value_t = ResearchModeArg::Technical)]
        mode: ResearchModeArg,
        #[arg(long, value_enum, default_value_t = ResearchFreshnessArg::Any)]
        freshness: ResearchFreshnessArg,
        #[arg(long, default_value_t = 12)]
        max_results: usize,
        #[arg(long, default_value_t = 6)]
        fetch_pages: usize,
        #[arg(long)]
        refresh: bool,
    },
    Fetch {
        url: String,
        #[arg(long)]
        refresh: bool,
    },
    Purge,
}

#[derive(Subcommand)]
enum SkillsCommand {
    List,
    Show {
        skill_id: String,
    },
    Refresh,
    Enable {
        skill_id: String,
    },
    Disable {
        skill_id: String,
    },
    Install {
        path: PathBuf,
    },
    Uninstall {
        skill_id: String,
    },
    Run {
        skill_id: String,
        input: PathBuf,
        #[arg(long)]
        approve: bool,
    },
}

#[derive(Subcommand)]
enum ConnectorsCommand {
    List,
    Show {
        provider: ConnectorProviderArg,
    },
    Configure {
        request: PathBuf,
    },
    Disconnect {
        provider: ConnectorProviderArg,
    },
    OAuthStart {
        provider: ConnectorProviderArg,
        #[arg(long)]
        force_consent: bool,
    },
    Action {
        request: PathBuf,
    },
    Audits {
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}

#[derive(Subcommand)]
enum AutomationsCommand {
    List,
    Show {
        automation_id: String,
    },
    Upsert {
        request: PathBuf,
    },
    Delete {
        automation_id: String,
    },
    Run {
        automation_id: String,
        #[arg(long)]
        approve: bool,
    },
    History {
        automation_id: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Approve {
        automation_id: String,
        execution_id: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ConnectorProviderArg {
    Gmail,
    Spotify,
    Instagram,
    TikTok,
    GitHub,
}

impl From<ConnectorProviderArg> for ConnectorProvider {
    fn from(value: ConnectorProviderArg) -> Self {
        match value {
            ConnectorProviderArg::Gmail => Self::Gmail,
            ConnectorProviderArg::Spotify => Self::Spotify,
            ConnectorProviderArg::Instagram => Self::Instagram,
            ConnectorProviderArg::TikTok => Self::TikTok,
            ConnectorProviderArg::GitHub => Self::GitHub,
        }
    }
}

#[derive(Subcommand)]
enum MemoryCommand {
    Search {
        #[arg(default_value = "")]
        query: String,
        #[arg(long, value_enum)]
        scope: Option<MemoryScopeArg>,
        #[arg(long)]
        workspace_key: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        include_superseded: bool,
    },
    Get {
        memory_id: String,
    },
    Upsert {
        request: PathBuf,
    },
    Forget {
        memory_id: String,
    },
    Supersede {
        old_memory_id: String,
        new_memory_id: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum MemoryScopeArg {
    Session,
    Workspace,
    Task,
    Artifact,
    Graph,
    Preference,
}

impl From<MemoryScopeArg> for MemoryScope {
    fn from(value: MemoryScopeArg) -> Self {
        match value {
            MemoryScopeArg::Session => Self::Session,
            MemoryScopeArg::Workspace => Self::Workspace,
            MemoryScopeArg::Task => Self::Task,
            MemoryScopeArg::Artifact => Self::Artifact,
            MemoryScopeArg::Graph => Self::Graph,
            MemoryScopeArg::Preference => Self::Preference,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ResearchModeArg {
    General,
    Technical,
    News,
    Academic,
}

impl From<ResearchModeArg> for ResearchMode {
    fn from(value: ResearchModeArg) -> Self {
        match value {
            ResearchModeArg::General => Self::General,
            ResearchModeArg::Technical => Self::Technical,
            ResearchModeArg::News => Self::News,
            ResearchModeArg::Academic => Self::Academic,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ResearchFreshnessArg {
    Any,
    Day,
    Week,
    Month,
    Year,
}

impl From<ResearchFreshnessArg> for ResearchFreshness {
    fn from(value: ResearchFreshnessArg) -> Self {
        match value {
            ResearchFreshnessArg::Any => Self::Any,
            ResearchFreshnessArg::Day => Self::Day,
            ResearchFreshnessArg::Week => Self::Week,
            ResearchFreshnessArg::Month => Self::Month,
            ResearchFreshnessArg::Year => Self::Year,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DirectionArg {
    Inbound,
    Outbound,
    Both,
}

impl From<DirectionArg> for everything_graph::GraphDirection {
    fn from(value: DirectionArg) -> Self {
        match value {
            DirectionArg::Inbound => Self::Inbound,
            DirectionArg::Outbound => Self::Outbound,
            DirectionArg::Both => Self::Both,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ModeArg {
    Fast,
    Balanced,
    Deep,
}

impl From<ModeArg> for ExecutionMode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Fast => ExecutionMode::Fast,
            ModeArg::Balanced => ExecutionMode::Balanced,
            ModeArg::Deep => ExecutionMode::Deep,
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let runtime = build_runtime(&cli.workspace)?;

    match cli.command {
        Command::Doctor => run_doctor(&runtime, &cli.workspace, cli.json)?,
        Command::Models => run_models(&runtime, cli.json)?,
        Command::ModelCapabilities => run_model_capabilities(&runtime, cli.json)?,
        Command::Modules => run_modules(&runtime, &cli.workspace, cli.json)?,
        Command::Graph => run_graph(&runtime, &cli.workspace, cli.json)?,
        Command::CodeGraph { command } => {
            run_code_graph(&runtime, &cli.workspace, command, cli.json)?
        }
        Command::Query { term } => run_query(&runtime, &cli.workspace, &term, cli.json)?,
        Command::Impact { term, depth } => {
            run_impact(&runtime, &cli.workspace, &term, depth, cli.json)?
        }
        Command::Plan { mode, objective } => {
            run_plan(&runtime, &cli.workspace, mode, objective, cli.json)?
        }
        Command::ProposeEdit {
            mode,
            skill,
            objective,
        } => run_propose_edit(&runtime, mode, skill, objective, cli.json)?,
        Command::Runs => run_runs(&runtime, cli.json)?,
        Command::Run { run_id } => run_by_id(&runtime, &run_id, cli.json)?,
        Command::RecoverableRuns => run_recoverable_runs(&runtime, cli.json)?,
        Command::Artifacts { run_id } => run_artifacts(&runtime, &run_id, cli.json)?,
        Command::Checkpoints { run_id } => run_checkpoints(&runtime, &run_id, cli.json)?,
        Command::Artifact { artifact_id } => run_artifact(&runtime, &artifact_id, cli.json)?,
        Command::Tools => run_tools(&runtime, cli.json)?,
        Command::ModelInvocations { run_id } => run_model_invocations(&runtime, &run_id, cli.json)?,
        Command::ModelInvocation { invocation_id } => {
            run_model_invocation(&runtime, &invocation_id, cli.json)?
        }
        Command::ToolInvocations { run_id } => run_tool_invocations(&runtime, &run_id, cli.json)?,
        Command::InvokeTool {
            invocation_id,
            request,
        } => run_invoke_tool(&runtime, &invocation_id, &request, cli.json)?,
        Command::CancelTool { invocation_id } => {
            run_cancel_tool(&runtime, &invocation_id, cli.json)?
        }
        Command::ExecutePatch { request } => run_execute_patch(&runtime, &request, cli.json)?,
        Command::Skills { command } => run_skills(&runtime, command, cli.json)?,
        Command::Memory { command } => run_memory(&runtime, command, cli.json)?,
        Command::Connectors { command } => run_connectors(&runtime, command, cli.json)?,
        Command::Automations { command } => run_automations(&runtime, command, cli.json)?,
        Command::Research { command } => run_research(&runtime, command, cli.json)?,
        Command::Metrics => run_metrics(&runtime, cli.json)?,
        Command::Bench { iterations } => run_bench(&runtime, &cli.workspace, iterations, cli.json)?,
    }

    Ok(())
}

fn run_code_graph(
    runtime: &ModularRuntime,
    workspace: &Path,
    command: CodeGraphCommand,
    json: bool,
) -> Result<()> {
    match command {
        CodeGraphCommand::Index => {
            let payload = runtime.refresh_code_graph(workspace)?;
            if json {
                return print_json(&payload);
            }

            println!("Code graph index refreshed");
            println!("- Workspace: {}", payload.workspace.display());
            println!("- Graph revision: {}", payload.graph_revision);
            println!("- Database: {}", payload.database_path.display());
            println!("- Scanned: {}", payload.scanned_files);
            println!("- Changed: {}", payload.changed_files);
            println!("- Unchanged: {}", payload.unchanged_files);
            println!("- Deleted: {}", payload.deleted_files);
            println!("- Entities: {}", payload.parsed_entities);
            println!("- Relations: {}", payload.parsed_relations);
            println!("- Parse errors: {}", payload.parse_errors);
            println!(
                "- Timings: scan={}ms parse={}ms commit={}ms total={}ms",
                payload.scan_millis,
                payload.parse_millis,
                payload.commit_millis,
                payload.total_millis
            );
        }
        CodeGraphCommand::Stats => {
            let payload = runtime.persistent_graph_stats()?;
            if json {
                return print_json(&payload);
            }

            println!("Persistent code graph");
            println!("- Database: {}", payload.database_path.display());
            println!("- Graph revision: {}", payload.graph_revision);
            println!("- Active files: {}", payload.active_files);
            println!("- Active entities: {}", payload.active_entities);
            println!("- Active relations: {}", payload.active_relations);
            println!("- Historical entities: {}", payload.historical_entities);
            println!("- Historical relations: {}", payload.historical_relations);
            println!("- Size: {} bytes", payload.database_bytes);
        }
        CodeGraphCommand::Search { term, limit } => {
            let payload = runtime.code_search(&term, limit)?;
            if json {
                return print_json(&payload);
            }

            println!("Matches: {}", payload.len());
            for item in payload {
                println!(
                    "- {:?} {} [{}] score={:.3}",
                    item.entity.kind,
                    item.entity.qualified_name,
                    item.entity.file_path.display(),
                    item.score
                );
            }
        }
        CodeGraphCommand::Impact {
            term,
            depth,
            limit,
            direction,
            relations,
        } => {
            let relation_kinds = parse_relation_kinds(&relations)?;
            let payload = runtime.code_traverse(
                &term,
                direction.into(),
                &relation_kinds,
                depth.unwrap_or(runtime.settings().graph.max_impact_depth),
                limit,
            )?;
            if json {
                return print_json(&payload);
            }

            println!(
                "Root: {} [{:?}]",
                payload.root.qualified_name, payload.root.kind
            );
            println!("- Graph revision: {}", payload.graph_revision);
            println!("- Direction: {:?}", payload.direction);
            println!("- Edge kinds: {:?}", payload.relation_kinds);
            println!("- Depth: {}", payload.depth);
            println!("- Entities: {}", payload.entities.len());
            println!("- Relations: {}", payload.relations.len());
            for entity in payload.entities.iter().take(25) {
                println!(
                    "- {:?} {} [{}]",
                    entity.kind,
                    entity.qualified_name,
                    entity.file_path.display()
                );
            }
        }
        CodeGraphCommand::Path {
            from,
            to,
            depth,
            direction,
            relations,
        } => {
            let relation_kinds = parse_relation_kinds(&relations)?;
            let payload = runtime.code_path_with_options(
                &from,
                &to,
                depth.unwrap_or(runtime.settings().graph.max_impact_depth),
                direction.into(),
                &relation_kinds,
            )?;
            if json {
                return print_json(&payload);
            }

            match payload {
                Some(path) => {
                    println!("Graph revision: {}", path.graph_revision);
                    println!("Direction: {:?}", path.direction);
                    println!("Edge kinds: {:?}", path.relation_kinds);
                    println!("Entities: {}", path.entities.len());
                    println!("Relations: {}", path.relations.len());
                    for entity in path.entities {
                        println!(
                            "- {:?} {} [{}]",
                            entity.kind,
                            entity.qualified_name,
                            entity.file_path.display()
                        );
                    }
                }
                None => println!("No path found"),
            }
        }
        CodeGraphCommand::ChangeImpact { request } => {
            let payload: everything_graph::CodeGraphChangeImpactRequest =
                serde_json::from_str(&std::fs::read_to_string(&request)?)?;
            let report = runtime.analyze_code_change(&payload)?;
            if json {
                return print_json(&report);
            }
            println!(
                "Change impact: {:?} risk ({:.1}/100)",
                report.risk_tier, report.aggregate_risk_score
            );
            println!("- Graph revision: {}", report.graph_revision);
            println!("- Roots: {}", report.roots.len());
            println!("- Impacted entities: {}", report.affected_entities.len());
            println!("- Impacted files: {}", report.affected_files.len());
            println!(
                "- Verification targets: {}",
                report.verification_targets.len()
            );
            for entity in report.affected_entities.iter().take(30) {
                println!(
                    "- {:.2} {} [{}]",
                    entity.impact_score,
                    entity.entity.qualified_name,
                    entity.entity.file_path.display()
                );
            }
        }
    }

    Ok(())
}

fn run_research(runtime: &ModularRuntime, command: ResearchCommand, json: bool) -> Result<()> {
    match command {
        ResearchCommand::Status => {
            let status = runtime.research_status()?;
            if json {
                return print_json(&status);
            }
            println!(
                "Web research: {}",
                if status.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            );
            println!("- Cache: {}", status.cache_path);
            println!(
                "- Cached queries/documents: {}/{}",
                status.cached_queries, status.cached_documents
            );
            for provider in status.providers {
                println!(
                    "- {}: {} ({})",
                    provider.provider,
                    if provider.available {
                        "available"
                    } else {
                        "unavailable"
                    },
                    provider.detail
                );
            }
        }
        ResearchCommand::Search {
            query,
            mode,
            freshness,
            max_results,
            fetch_pages,
            refresh,
        } => {
            anyhow::ensure!(!query.is_empty(), "research search requires a query");
            let report = runtime.web_search(&WebSearchRequest {
                query: query.join(" "),
                mode: mode.into(),
                freshness: freshness.into(),
                max_results,
                fetch_pages,
                allowed_domains: Vec::new(),
                blocked_domains: Vec::new(),
                force_refresh: refresh,
            })?;
            if json {
                return print_json(&report);
            }
            println!(
                "Research {}: {} source(s) from {} provider(s)",
                report.report_id,
                report.sources.len(),
                report.provider_count
            );
            for source in report.sources {
                println!(
                    "- [{}] {} — {}",
                    source.citation_id, source.title, source.canonical_url
                );
            }
            for warning in report.warnings {
                println!("warning: {warning}");
            }
        }
        ResearchCommand::Fetch { url, refresh } => {
            let response = runtime.web_fetch(&WebFetchRequest {
                url,
                force_refresh: refresh,
            })?;
            if json {
                return print_json(&response);
            }
            println!("{} {}", response.status_code, response.source.title);
            println!("{}", response.source.canonical_url);
            println!(
                "{} bytes{}",
                response.bytes_received,
                if response.truncated {
                    " (truncated)"
                } else {
                    ""
                }
            );
        }
        ResearchCommand::Purge => {
            let removed = runtime.purge_research_cache()?;
            if json {
                return print_json(&serde_json::json!({"removed": removed}));
            }
            println!("Removed {removed} expired research cache row(s)");
        }
    }
    Ok(())
}

fn run_doctor(runtime: &ModularRuntime, workspace: &Path, json: bool) -> Result<()> {
    let report = runtime.doctor(workspace)?;
    if json {
        return print_json(&report);
    }

    println!("Runtime Doctor");
    println!("- Overall: {:?}", report.overall_status);
    println!("- Root: {}", report.project.root_path.display());
    println!("- Files: {}", report.project.file_count);
    println!("- Graph nodes: {}", report.project.graph_node_count);
    println!("- Graph edges: {}", report.project.graph_edge_count);
    println!("- Filesystem: {}", report.adapters.filesystem);
    println!("- Model: {}", report.adapters.model);
    println!("- Command: {}", report.adapters.command);
    println!("- Model health: {:?}", report.model_health.status);
    println!("- Model available: {}", report.model_health.available);
    println!(
        "- Primary/fallback: {} / {} (fallback active: {})",
        report.model_health.primary_available,
        report.model_health.fallback_available,
        report.model_health.fallback_active
    );
    println!("- Active adapter: {}", report.model_health.adapter);
    println!("- Model detail: {}", report.model_health.detail);
    println!(
        "- Snapshot cache: {} hits / {} misses / {} files / {} bytes",
        report.snapshot_stats.cache_hits,
        report.snapshot_stats.cache_misses,
        report.snapshot_stats.scanned_files,
        report.snapshot_stats.bytes_read
    );
    println!(
        "- Bootstrap timings: snapshot={}ms graph={}ms catalog={}ms",
        report.bootstrap_metrics.snapshot_millis,
        report.bootstrap_metrics.graph_millis,
        report.bootstrap_metrics.catalog_millis
    );
    for check in report.checks {
        println!("- [{:?}] {}: {}", check.status, check.label, check.detail);
        if let Some(remediation) = check.remediation {
            println!("  remediation: {remediation}");
        }
    }
    Ok(())
}

fn run_modules(runtime: &ModularRuntime, workspace: &Path, json: bool) -> Result<()> {
    let report = runtime.bootstrap(workspace)?;
    if json {
        return print_json(&report.modules);
    }

    print_modules(&report.modules);
    Ok(())
}

fn run_graph(runtime: &ModularRuntime, workspace: &Path, json: bool) -> Result<()> {
    let payload = runtime.graph_summary(workspace)?;

    if json {
        return print_json(&payload);
    }

    println!("{}", payload.summary);
    if payload.package_references.is_empty() {
        return Ok(());
    }

    println!();
    println!("Package references");
    for item in payload.package_references {
        println!(
            "- {}: {} outbound references",
            item.label, item.outbound_references
        );
    }

    Ok(())
}

fn run_query(runtime: &ModularRuntime, workspace: &Path, term: &str, json: bool) -> Result<()> {
    let query = runtime.compatibility_query(workspace, term, 20)?;
    if json {
        return print_json(&query);
    }

    println!("Matches: {}", query.matched_nodes.len());
    for node in query.matched_nodes.iter().take(20) {
        println!(
            "- {:?} {} [{}]",
            node.kind,
            node.label,
            node.source_path.display()
        );
    }
    Ok(())
}

fn run_impact(
    runtime: &ModularRuntime,
    workspace: &Path,
    term: &str,
    depth: Option<usize>,
    json: bool,
) -> Result<()> {
    let max_depth = depth.unwrap_or(runtime.settings().graph.max_impact_depth);
    let impact = runtime.compatibility_impact(workspace, term, max_depth, 100)?;
    if json {
        return print_json(&impact);
    }

    println!("Root: {} [{:?}]", impact.root.label, impact.root.kind);
    println!("Depth: {}", impact.depth);
    println!("Affected: {}", impact.affected_nodes.len());
    for node in impact.affected_nodes.iter().take(25) {
        println!(
            "- {:?} {} [{}]",
            node.kind,
            node.label,
            node.source_path.display()
        );
    }
    Ok(())
}

fn run_plan(
    runtime: &ModularRuntime,
    workspace: &Path,
    mode: ModeArg,
    objective: Vec<String>,
    json: bool,
) -> Result<()> {
    if objective.is_empty() {
        anyhow::bail!("plan requires a non-empty objective");
    }

    let request = build_task_request_for_workspace(workspace, objective.join(" "), mode.into())?;
    let payload = runtime.plan_response(&request)?;

    if json {
        return print_json(&payload);
    }

    println!("{}", payload.document.content);
    println!();
    println!("Run ID: {}", payload.run_id);
    println!("Run journal: {}", payload.journal_path.display());
    Ok(())
}

fn run_propose_edit(
    runtime: &ModularRuntime,
    mode: ModeArg,
    skill: Option<String>,
    objective: Vec<String>,
    json: bool,
) -> Result<()> {
    if objective.is_empty() {
        anyhow::bail!("propose-edit requires a non-empty objective");
    }
    let payload = runtime.propose_edit(&EditProposalRequest {
        objective: objective.join(" "),
        mode: mode.into(),
        skill_id: skill,
    })?;
    if json {
        return print_json(&payload);
    }
    println!("Run: {}", payload.run_id);
    println!("Status: {}", payload.status);
    println!("Model: {}", payload.generated_by);
    if let Some(skill_id) = &payload.skill_id {
        println!("Skill: {skill_id}");
    }
    println!("Target: {}", payload.patch.relative_path.display());
    println!();
    println!("{}", payload.summary);
    println!();
    println!("{}", payload.diff);
    println!();
    println!(
        "The proposal was not applied. Review it, set approval_granted=true, and use execute-patch."
    );
    Ok(())
}

fn run_runs(runtime: &ModularRuntime, json: bool) -> Result<()> {
    let payload = runtime.list_runs()?;
    if json {
        return print_json(&payload);
    }

    println!("Runs");
    for run in payload {
        println!(
            "- {} [{}] events={} last_stage={} {}",
            run.run_id,
            format_status(&run.status),
            run.event_count,
            run.last_stage.as_deref().unwrap_or("n/a"),
            run.objective
        );
    }
    Ok(())
}

fn run_by_id(runtime: &ModularRuntime, run_id: &str, json: bool) -> Result<()> {
    let payload = runtime
        .get_run(run_id)?
        .ok_or_else(|| anyhow::anyhow!("run '{run_id}' not found"))?;
    if json {
        return print_json(&payload);
    }

    println!("Run {}", payload.run_id);
    println!("- Status: {}", format_status(&payload.status));
    println!("- Objective: {}", payload.objective);
    println!("- Generator: {}", payload.generated_by);
    println!("- Recoverable: {}", payload.recoverable);
    println!(
        "- Recovery disposition: {}",
        payload
            .recovery_disposition
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "n/a".to_owned())
    );
    println!("- Events: {}", payload.events.len());
    println!("- Artifacts: {}", payload.artifacts.len());
    for event in payload.events {
        println!(
            "  #{} [{:?}] {}: {}",
            event.sequence, event.severity, event.stage, event.detail
        );
    }
    Ok(())
}

fn run_recoverable_runs(runtime: &ModularRuntime, json: bool) -> Result<()> {
    let payload = runtime.recoverable_runs()?;
    if json {
        return print_json(&payload);
    }

    println!("Recoverable runs");
    for run in payload {
        println!(
            "- {} [{}] disposition={} {}",
            run.run_id,
            format_status(&run.status),
            run.recovery_disposition
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|| "n/a".to_owned()),
            run.objective
        );
    }
    Ok(())
}

fn run_artifacts(runtime: &ModularRuntime, run_id: &str, json: bool) -> Result<()> {
    if runtime.get_run(run_id)?.is_none() {
        anyhow::bail!("run '{run_id}' not found");
    }
    let payload = runtime.list_artifacts(run_id)?;
    if json {
        return print_json(&payload);
    }

    println!("Artifacts for {run_id}");
    for artifact in payload {
        println!(
            "- {} [{:?}] {} bytes {}",
            artifact.artifact_id, artifact.kind, artifact.size_bytes, artifact.media_type
        );
    }
    Ok(())
}

fn run_artifact(runtime: &ModularRuntime, artifact_id: &str, json: bool) -> Result<()> {
    let payload = runtime
        .get_artifact(artifact_id)?
        .ok_or_else(|| anyhow::anyhow!("artifact '{artifact_id}' not found"))?;
    if json {
        return print_json(&payload);
    }

    println!("{}", payload.content);
    Ok(())
}

fn run_checkpoints(runtime: &ModularRuntime, run_id: &str, json: bool) -> Result<()> {
    if runtime.get_run(run_id)?.is_none() {
        anyhow::bail!("run '{run_id}' not found");
    }
    let payload = runtime.list_checkpoints(run_id)?;
    if json {
        return print_json(&payload);
    }

    println!("Checkpoints for {run_id}");
    for checkpoint in payload {
        println!(
            "- {} sequence={} safe={} {}",
            checkpoint.checkpoint_id,
            checkpoint.event_sequence,
            checkpoint.safe_to_resume,
            checkpoint.summary
        );
    }
    Ok(())
}

fn run_models(runtime: &ModularRuntime, json: bool) -> Result<()> {
    let models = runtime.discover_models()?;
    if json {
        return print_json(&models);
    }
    println!("Discovered models");
    for model in models {
        println!(
            "- {}/{} installed={} configured={} tier={:?} safe_context={}",
            model.provider,
            model.model_name,
            model.installed,
            model.configured,
            model.profile.quality_tier,
            model.profile.safe_context_tokens
        );
    }
    Ok(())
}

fn run_model_capabilities(runtime: &ModularRuntime, json: bool) -> Result<()> {
    let profile = runtime.model_capability_profile();
    if json {
        return print_json(&profile);
    }
    println!("Model capabilities");
    println!("- Provider: {}", profile.provider);
    println!("- Model: {}", profile.model_name);
    println!("- Tier: {:?}", profile.quality_tier);
    println!("- Context window: {} tokens", profile.context_window_tokens);
    println!("- Safe context: {} tokens", profile.safe_context_tokens);
    println!("- Coding suitability: {:.2}", profile.coding_suitability);
    println!(
        "- Structured output reliability: {:.2}",
        profile.structured_output_reliability
    );
    println!(
        "- Tool calling reliability: {:.2}",
        profile.tool_calling_reliability
    );
    Ok(())
}

fn run_tools(runtime: &ModularRuntime, json: bool) -> Result<()> {
    let payload = runtime.tool_definitions();
    if json {
        return print_json(&payload);
    }
    println!("Tools");
    for tool in payload {
        println!(
            "- {}@{} effect={:?} permissions={:?}",
            tool.tool_id, tool.version, tool.effect, tool.required_permissions
        );
        println!("  {}", tool.description);
    }
    Ok(())
}

fn run_model_invocations(runtime: &ModularRuntime, run_id: &str, json: bool) -> Result<()> {
    if runtime.get_run(run_id)?.is_none() {
        anyhow::bail!("run '{run_id}' not found");
    }
    let payload = runtime.list_model_invocations(run_id)?;
    if json {
        return print_json(&payload);
    }
    println!("Model invocations for {run_id}");
    for invocation in payload {
        println!(
            "- {} {}/{} [{:?}] duration={}ms prompt={} output={} fallback={}",
            invocation.invocation_id,
            invocation.provider,
            invocation.model,
            invocation.status,
            invocation.duration_millis,
            invocation.prompt_estimated_tokens,
            invocation.output_bytes,
            invocation.fallback_used
        );
    }
    Ok(())
}

fn run_model_invocation(runtime: &ModularRuntime, invocation_id: &str, json: bool) -> Result<()> {
    let payload = runtime
        .get_model_invocation(invocation_id)?
        .ok_or_else(|| anyhow::anyhow!("model invocation '{invocation_id}' not found"))?;
    if json {
        return print_json(&payload);
    }
    println!("Model invocation {}", payload.invocation_id);
    println!("- Provider: {}", payload.provider);
    println!("- Model: {}", payload.model);
    println!("- Status: {:?}", payload.status);
    println!("- Duration: {} ms", payload.duration_millis);
    println!(
        "- Prompt estimate: {} tokens",
        payload.prompt_estimated_tokens
    );
    println!("- Output: {} bytes", payload.output_bytes);
    println!("- Fallback: {}", payload.fallback_used);
    if let Some(error) = payload.primary_error {
        println!("- Primary error: {error}");
    }
    Ok(())
}

fn run_tool_invocations(runtime: &ModularRuntime, run_id: &str, json: bool) -> Result<()> {
    if runtime.get_run(run_id)?.is_none() {
        anyhow::bail!("run '{run_id}' not found");
    }
    let payload = runtime.list_tool_invocations(run_id)?;
    if json {
        return print_json(&payload);
    }
    println!("Tool invocations for {run_id}");
    for invocation in payload {
        println!(
            "- {} {}@{} [{:?}] {}",
            invocation.invocation_id,
            invocation.tool_name,
            invocation.tool_version,
            invocation.status,
            invocation.result_summary.as_deref().unwrap_or("n/a")
        );
    }
    Ok(())
}

fn run_invoke_tool(
    runtime: &ModularRuntime,
    invocation_id: &str,
    request_path: &Path,
    json: bool,
) -> Result<()> {
    let request: ToolInvocationRequest = serde_json::from_slice(&std::fs::read(request_path)?)?;
    let payload = runtime.invoke_tool(InvocationId::new(invocation_id), request)?;
    if json {
        return print_json(&payload);
    }
    println!(
        "{} [{:?}] {}",
        payload.invocation.tool_name,
        payload.invocation.status,
        payload
            .invocation
            .result_summary
            .as_deref()
            .unwrap_or("no summary")
    );
    print_json(&payload.output)
}

fn run_cancel_tool(runtime: &ModularRuntime, invocation_id: &str, json: bool) -> Result<()> {
    let cancelled = runtime.cancel_tool_invocation(invocation_id);
    let payload = serde_json::json!({
        "invocation_id": invocation_id,
        "cancelled": cancelled,
    });
    if json {
        return print_json(&payload);
    }
    println!("Cancellation requested: {cancelled}");
    Ok(())
}

fn run_execute_patch(runtime: &ModularRuntime, request_path: &Path, json: bool) -> Result<()> {
    let request: PatchExecutionRequest = serde_json::from_slice(&std::fs::read(request_path)?)?;
    let payload = runtime.execute_patch(&request)?;
    if json {
        return print_json(&payload);
    }
    println!("Run: {}", payload.run_id);
    println!("Status: {}", payload.status);
    println!("Rolled back: {}", payload.rolled_back);
    println!("{}", payload.summary);
    Ok(())
}

fn run_skills(runtime: &ModularRuntime, command: SkillsCommand, json: bool) -> Result<()> {
    match command {
        SkillsCommand::List => {
            let payload = runtime.list_skills()?;
            if json {
                return print_json(&payload);
            }
            println!("Skills");
            for skill in payload {
                println!(
                    "- {}@{} enabled={} compatible={} source={:?}",
                    skill.manifest.skill_id,
                    skill.manifest.version,
                    skill.enabled,
                    skill.compatibility.compatible,
                    skill.source
                );
                println!("  {}", skill.manifest.description);
            }
        }
        SkillsCommand::Show { skill_id } => {
            let payload = runtime
                .get_skill(&skill_id)?
                .ok_or_else(|| anyhow::anyhow!("skill '{skill_id}' not found"))?;
            if json {
                return print_json(&payload);
            }
            println!("{} ({})", payload.manifest.name, payload.manifest.skill_id);
            println!("- Version: {}", payload.manifest.version);
            println!("- Runtime API: {}", payload.manifest.runtime_api);
            println!("- Workflow: {:?}", payload.manifest.workflow);
            println!("- Source: {:?}", payload.source);
            println!("- Enabled: {}", payload.enabled);
            println!("- Compatible: {}", payload.compatibility.compatible);
            println!("- Permissions: {:?}", payload.manifest.permissions);
            if let Some(path) = payload.source_path {
                println!("- Source path: {}", path.display());
            }
            println!("- Hash: {}", payload.content_hash);
            println!();
            println!("{}", payload.instructions_preview);
        }
        SkillsCommand::Refresh => {
            let payload = runtime.refresh_skills()?;
            if json {
                return print_json(&payload);
            }
            println!("Refreshed {} skills", payload.len());
        }
        SkillsCommand::Enable { skill_id } => {
            let payload = runtime.set_skill_enabled(&skill_id, true)?;
            if json {
                return print_json(&payload);
            }
            println!("Enabled {}", payload.manifest.skill_id);
        }
        SkillsCommand::Disable { skill_id } => {
            let payload = runtime.set_skill_enabled(&skill_id, false)?;
            if json {
                return print_json(&payload);
            }
            println!("Disabled {}", payload.manifest.skill_id);
        }
        SkillsCommand::Install { path } => {
            let payload = runtime.install_skill(&path)?;
            if json {
                return print_json(&payload);
            }
            println!(
                "Installed {}@{} from {}",
                payload.manifest.skill_id,
                payload.manifest.version,
                path.display()
            );
        }
        SkillsCommand::Uninstall { skill_id } => {
            let removed = runtime.uninstall_skill(&skill_id)?;
            let payload = serde_json::json!({"skill_id": skill_id, "uninstalled": removed});
            if json {
                return print_json(&payload);
            }
            if removed {
                println!("Uninstalled {skill_id}");
            } else {
                println!("Skill {skill_id} was not installed");
            }
        }
        SkillsCommand::Run {
            skill_id,
            input,
            approve,
        } => {
            let input_value = serde_json::from_slice(&std::fs::read(&input)?)?;
            let payload = runtime.execute_skill(
                &skill_id,
                SkillExecutionRequest {
                    input: input_value,
                    approval_granted: approve,
                },
            )?;
            if json {
                return print_json(&payload);
            }
            println!("Skill: {}@{}", payload.skill_id, payload.skill_version);
            println!("Status: {:?}", payload.status);
            if let Some(run_id) = payload.run_id {
                println!("Run: {run_id}");
            }
            println!("Artifacts: {}", payload.artifact_ids.len());
            print_json(&payload.output)?;
        }
    }
    Ok(())
}

fn run_memory(runtime: &ModularRuntime, command: MemoryCommand, json: bool) -> Result<()> {
    match command {
        MemoryCommand::Search {
            query,
            scope,
            workspace_key,
            limit,
            include_superseded,
        } => {
            let payload = runtime.search_memory(&MemoryQuery {
                query,
                scope: scope.map(Into::into),
                workspace_key,
                limit,
                include_superseded,
            })?;
            if json {
                return print_json(&payload);
            }
            println!("Memory results: {}", payload.len());
            for result in payload {
                println!(
                    "- {} [{:?}] score={:.3} confidence={:.2}",
                    result.entry.title, result.entry.scope, result.score, result.entry.confidence
                );
                println!(
                    "  id={} source={}",
                    result.entry.memory_id, result.entry.source
                );
                println!("  {}", result.entry.content.replace('\n', " "));
            }
        }
        MemoryCommand::Get { memory_id } => {
            let payload = runtime
                .get_memory(&memory_id)?
                .ok_or_else(|| anyhow::anyhow!("memory '{memory_id}' not found"))?;
            if json {
                return print_json(&payload);
            }
            println!("{} ({})", payload.title, payload.memory_id);
            println!("- Scope: {:?}", payload.scope);
            println!("- Source: {}", payload.source);
            println!("- Confidence: {:.2}", payload.confidence);
            println!("- Version: {}", payload.version);
            println!();
            println!("{}", payload.content);
        }
        MemoryCommand::Upsert { request } => {
            let request: MemoryUpsertRequest = serde_json::from_slice(&std::fs::read(&request)?)?;
            let payload = runtime.remember(request)?;
            if json {
                return print_json(&payload);
            }
            println!("Saved memory {}", payload.memory_id);
        }
        MemoryCommand::Forget { memory_id } => {
            let forgotten = runtime.forget_memory(&memory_id)?;
            let payload = serde_json::json!({"memory_id": memory_id, "forgotten": forgotten});
            if json {
                return print_json(&payload);
            }
            println!("Forgotten: {forgotten}");
        }
        MemoryCommand::Supersede {
            old_memory_id,
            new_memory_id,
        } => {
            runtime.supersede_memory(&old_memory_id, &new_memory_id)?;
            let payload = serde_json::json!({
                "old_memory_id": old_memory_id,
                "new_memory_id": new_memory_id,
                "superseded": true
            });
            if json {
                return print_json(&payload);
            }
            println!("Superseded {old_memory_id} with {new_memory_id}");
        }
    }
    Ok(())
}

fn run_connectors(runtime: &ModularRuntime, command: ConnectorsCommand, json: bool) -> Result<()> {
    match command {
        ConnectorsCommand::List => {
            let payload = runtime.list_connectors()?;
            if json {
                return print_json(&payload);
            }
            println!("Connectors");
            for connector in payload {
                println!(
                    "- {} [{:?}] configured={} connected={} account={}",
                    connector.display_name,
                    connector.status,
                    connector.configured,
                    connector.connected,
                    connector.account_label.as_deref().unwrap_or("n/a")
                );
                for limitation in connector.limitations {
                    println!("  limit: {limitation}");
                }
            }
        }
        ConnectorsCommand::Show { provider } => {
            let payload = runtime.get_connector(provider.into())?;
            if json {
                return print_json(&payload);
            }
            println!("{} [{:?}]", payload.display_name, payload.status);
            println!("- Configured: {}", payload.configured);
            println!("- Connected: {}", payload.connected);
            println!("- Actions: {}", payload.actions.len());
            if let Some(label) = payload.account_label {
                println!("- Account: {label}");
            }
        }
        ConnectorsCommand::Configure { request } => {
            let request: ConnectorConfigureRequest =
                serde_json::from_slice(&std::fs::read(request)?)?;
            let payload = runtime.configure_connector(request)?;
            if json {
                return print_json(&payload);
            }
            println!("Configured {}", payload.display_name);
        }
        ConnectorsCommand::Disconnect { provider } => {
            let payload = runtime.disconnect_connector(provider.into())?;
            if json {
                return print_json(&payload);
            }
            println!("Disconnected {}", payload.display_name);
        }
        ConnectorsCommand::OAuthStart {
            provider,
            force_consent,
        } => {
            let payload = runtime.start_connector_oauth(OAuthStartRequest {
                provider: provider.into(),
                force_consent,
            })?;
            if json {
                return print_json(&payload);
            }
            println!("Open this URL in your system browser:");
            println!("{}", payload.authorization_url);
            println!("Callback: {}", payload.redirect_uri);
        }
        ConnectorsCommand::Action { request } => {
            let request: ConnectorActionRequest = serde_json::from_slice(&std::fs::read(request)?)?;
            let payload = runtime.execute_connector_action(request)?;
            if json {
                return print_json(&payload);
            }
            println!(
                "{}:{} status={} executed={}",
                payload.provider.as_str(),
                payload.action_id,
                payload.status,
                payload.executed
            );
            print_json(&payload.output)?;
        }
        ConnectorsCommand::Audits { limit } => {
            let payload = runtime.connector_audits(limit)?;
            if json {
                return print_json(&payload);
            }
            println!("Connector audits: {}", payload.len());
            for audit in payload {
                println!(
                    "- {} {}:{} {} approval={}",
                    audit.audit_id,
                    audit.provider.as_str(),
                    audit.action_id,
                    audit.status,
                    audit.approval_granted
                );
            }
        }
    }
    Ok(())
}

fn run_automations(
    runtime: &ModularRuntime,
    command: AutomationsCommand,
    json: bool,
) -> Result<()> {
    match command {
        AutomationsCommand::List => {
            let payload = runtime.list_automations()?;
            if json {
                return print_json(&payload);
            }
            println!("Automations");
            for automation in payload {
                println!(
                    "- {} id={} enabled={} next={} failures={}",
                    automation.name,
                    automation.automation_id,
                    automation.enabled,
                    automation
                        .next_run_at_epoch_millis
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "n/a".to_owned()),
                    automation.consecutive_failures,
                );
            }
        }
        AutomationsCommand::Show { automation_id } => {
            let payload = runtime
                .get_automation(&automation_id)?
                .ok_or_else(|| anyhow::anyhow!("automation '{automation_id}' not found"))?;
            if json {
                return print_json(&payload);
            }
            println!("{} ({})", payload.name, payload.automation_id);
            println!("- Enabled: {}", payload.enabled);
            println!("- Autonomy: {:?}", payload.policy.autonomy_level);
            println!(
                "- Next run: {}",
                payload
                    .next_run_at_epoch_millis
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "n/a".to_owned())
            );
        }
        AutomationsCommand::Upsert { request } => {
            let request: AutomationUpsertRequest =
                serde_json::from_slice(&std::fs::read(request)?)?;
            let payload = runtime.upsert_automation(request)?;
            if json {
                return print_json(&payload);
            }
            println!(
                "Saved automation {} ({})",
                payload.name, payload.automation_id
            );
        }
        AutomationsCommand::Delete { automation_id } => {
            let deleted = runtime.delete_automation(&automation_id)?;
            let payload = serde_json::json!({"automation_id": automation_id, "deleted": deleted});
            if json {
                return print_json(&payload);
            }
            println!("Deleted: {deleted}");
        }
        AutomationsCommand::Run {
            automation_id,
            approve,
        } => {
            let payload = runtime.run_automation_now(
                &automation_id,
                AutomationRunNowRequest {
                    approval_granted: approve,
                },
            )?;
            if json {
                return print_json(&payload);
            }
            println!("Execution {} [{:?}]", payload.execution_id, payload.status);
            if let Some(error) = payload.error {
                println!("Error: {error}");
            }
            print_json(&payload.output)?;
        }
        AutomationsCommand::History {
            automation_id,
            limit,
        } => {
            let payload = runtime.list_automation_executions(&automation_id, limit)?;
            if json {
                return print_json(&payload);
            }
            println!("Automation executions: {}", payload.len());
            for execution in payload {
                println!(
                    "- {} [{:?}] attempt={} error={}",
                    execution.execution_id,
                    execution.status,
                    execution.attempt,
                    execution.error.as_deref().unwrap_or("n/a")
                );
            }
        }
        AutomationsCommand::Approve {
            automation_id,
            execution_id,
        } => {
            let payload = runtime.approve_automation_execution(&automation_id, &execution_id)?;
            if json {
                return print_json(&payload);
            }
            println!(
                "Approved execution {} as new execution {} [{:?}]",
                execution_id, payload.execution_id, payload.status
            );
        }
    }
    Ok(())
}

fn run_metrics(runtime: &ModularRuntime, json: bool) -> Result<()> {
    let payload = runtime.metrics()?;
    if json {
        return print_json(&payload);
    }

    println!("Metrics");
    println!("- Data dir: {}", payload.data_dir.display());
    println!("- Runs recorded: {}", payload.runs_recorded);
    println!("- Benchmarks recorded: {}", payload.benchmarks_recorded);
    println!(
        "- Latest run: {}",
        payload.latest_run_id.as_deref().unwrap_or("n/a")
    );
    println!(
        "- Latest benchmark: {}",
        payload.latest_benchmark_id.as_deref().unwrap_or("n/a")
    );
    Ok(())
}

fn run_bench(
    runtime: &ModularRuntime,
    workspace: &Path,
    iterations: usize,
    json: bool,
) -> Result<()> {
    let payload = runtime.benchmark_bootstrap(workspace, iterations)?;
    if json {
        return print_json(&payload);
    }

    println!("Benchmark: {}", payload.benchmark_name);
    println!("- Iterations: {}", payload.iterations);
    println!("- Mean: {:.2}ms", payload.mean_millis);
    println!(
        "- Min/Max: {}ms / {}ms",
        payload.min_millis, payload.max_millis
    );
    println!(
        "- P50/P95: {}ms / {}ms",
        payload.p50_millis, payload.p95_millis
    );
    println!(
        "- Cache totals: {} hits / {} misses / {} bytes",
        payload.cache_hits, payload.cache_misses, payload.bytes_read
    );
    Ok(())
}

fn parse_relation_kinds(values: &[String]) -> Result<Vec<everything_graph::CodeRelationKind>> {
    values
        .iter()
        .map(|value| {
            everything_graph::CodeRelationKind::parse(value)
                .ok_or_else(|| anyhow::anyhow!("unknown relation kind '{value}'"))
        })
        .collect()
}

fn print_modules(modules: &[ModuleDescriptor]) {
    println!("Modules");
    for module in modules {
        println!(
            "- {} [{:?}] {}",
            module.name, module.kind, module.responsibility
        );
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn format_status(status: &everything_domain::RunStatus) -> &'static str {
    match status {
        everything_domain::RunStatus::Queued => "queued",
        everything_domain::RunStatus::Started => "started",
        everything_domain::RunStatus::Running => "running",
        everything_domain::RunStatus::Paused => "paused",
        everything_domain::RunStatus::AwaitingApproval => "awaiting-approval",
        everything_domain::RunStatus::Blocked => "blocked",
        everything_domain::RunStatus::Cancelling => "cancelling",
        everything_domain::RunStatus::Cancelled => "cancelled",
        everything_domain::RunStatus::Recovering => "recovering",
        everything_domain::RunStatus::Completed => "completed",
        everything_domain::RunStatus::Failed => "failed",
    }
}
