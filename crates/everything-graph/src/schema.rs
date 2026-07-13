use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

pub const CODE_GRAPH_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeLanguage {
    Project,
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Toml,
    Json,
}

impl CodeLanguage {
    pub fn from_path(path: &Path) -> Option<Self> {
        let file_name = path.file_name()?.to_str()?;
        if matches!(file_name, "Cargo.toml" | "pyproject.toml") {
            return Some(Self::Toml);
        }
        if file_name == "package.json" {
            return Some(Self::Json);
        }
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::Toml => "toml",
            Self::Json => "json",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "rust" => Some(Self::Rust),
            "python" => Some(Self::Python),
            "javascript" => Some(Self::JavaScript),
            "typescript" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "toml" => Some(Self::Toml),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

impl Display for CodeLanguage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeEntityKind {
    Project,
    Package,
    File,
    Module,
    Namespace,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    TypeAlias,
    Implementation,
    Function,
    Method,
    Import,
    Constant,
    Variable,
    Test,
    Route,
    Event,
    EnvironmentVariable,
    ConfigurationKey,
    DatabaseObject,
    External,
}

impl CodeEntityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Package => "package",
            Self::File => "file",
            Self::Module => "module",
            Self::Namespace => "namespace",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Trait => "trait",
            Self::Interface => "interface",
            Self::TypeAlias => "type_alias",
            Self::Implementation => "implementation",
            Self::Function => "function",
            Self::Method => "method",
            Self::Import => "import",
            Self::Constant => "constant",
            Self::Variable => "variable",
            Self::Test => "test",
            Self::Route => "route",
            Self::Event => "event",
            Self::EnvironmentVariable => "environment_variable",
            Self::ConfigurationKey => "configuration_key",
            Self::DatabaseObject => "database_object",
            Self::External => "external",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "project" => Self::Project,
            "package" => Self::Package,
            "file" => Self::File,
            "module" => Self::Module,
            "namespace" => Self::Namespace,
            "class" => Self::Class,
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            "trait" => Self::Trait,
            "interface" => Self::Interface,
            "type_alias" => Self::TypeAlias,
            "implementation" => Self::Implementation,
            "function" => Self::Function,
            "method" => Self::Method,
            "import" => Self::Import,
            "constant" => Self::Constant,
            "variable" => Self::Variable,
            "test" => Self::Test,
            "route" => Self::Route,
            "event" => Self::Event,
            "environment_variable" => Self::EnvironmentVariable,
            "configuration_key" => Self::ConfigurationKey,
            "database_object" => Self::DatabaseObject,
            "external" => Self::External,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeRelationKind {
    Contains,
    Defines,
    Imports,
    Calls,
    References,
    Implements,
    Extends,
    DependsOn,
    Reads,
    Writes,
    Tests,
    RoutesTo,
    Emits,
    Handles,
    UsesEnvironment,
    Configures,
    Queries,
    Mutates,
}

impl CodeRelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Defines => "defines",
            Self::Imports => "imports",
            Self::Calls => "calls",
            Self::References => "references",
            Self::Implements => "implements",
            Self::Extends => "extends",
            Self::DependsOn => "depends_on",
            Self::Reads => "reads",
            Self::Writes => "writes",
            Self::Tests => "tests",
            Self::RoutesTo => "routes_to",
            Self::Emits => "emits",
            Self::Handles => "handles",
            Self::UsesEnvironment => "uses_environment",
            Self::Configures => "configures",
            Self::Queries => "queries",
            Self::Mutates => "mutates",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "contains" => Self::Contains,
            "defines" => Self::Defines,
            "imports" => Self::Imports,
            "calls" => Self::Calls,
            "references" => Self::References,
            "implements" => Self::Implements,
            "extends" => Self::Extends,
            "depends_on" => Self::DependsOn,
            "reads" => Self::Reads,
            "writes" => Self::Writes,
            "tests" => Self::Tests,
            "routes_to" => Self::RoutesTo,
            "emits" => Self::Emits,
            "handles" => Self::Handles,
            "uses_environment" => Self::UsesEnvironment,
            "configures" => Self::Configures,
            "queries" => Self::Queries,
            "mutates" => Self::Mutates,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphDirection {
    Inbound,
    Outbound,
    #[default]
    Both,
}

impl GraphDirection {
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "inbound" => Self::Inbound,
            "outbound" => Self::Outbound,
            "both" => Self::Both,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationEvidenceKind {
    Observed,
    #[default]
    Inferred,
}

impl RelationEvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Inferred => "inferred",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "observed" => Self::Observed,
            "inferred" => Self::Inferred,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeEntity {
    pub id: String,
    pub revision: u64,
    pub kind: CodeEntityKind,
    pub name: String,
    pub qualified_name: String,
    pub language: CodeLanguage,
    pub file_path: PathBuf,
    pub span: SourceSpan,
    pub valid_from_epoch_millis: u128,
    pub valid_until_epoch_millis: Option<u128>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeRelation {
    pub id: String,
    pub revision: u64,
    pub source_id: String,
    pub target_id: String,
    pub kind: CodeRelationKind,
    pub confidence: f32,
    #[serde(default)]
    pub evidence_kind: RelationEvidenceKind,
    pub evidence_file: PathBuf,
    pub evidence_span: SourceSpan,
    pub extractor: String,
    pub valid_from_epoch_millis: u128,
    pub valid_until_epoch_millis: Option<u128>,
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    #[default]
    Modify,
    Add,
    Delete,
    Rename,
    Dependency,
    Configuration,
    PublicApi,
}

impl ChangeKind {
    pub fn risk_multiplier(self) -> f64 {
        match self {
            Self::Modify => 1.0,
            Self::Add => 0.75,
            Self::Delete => 1.45,
            Self::Rename => 1.35,
            Self::Dependency => 1.3,
            Self::Configuration => 1.25,
            Self::PublicApi => 1.5,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpactRiskTier {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphChangeTarget {
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
    #[serde(default)]
    pub change_kind: ChangeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphChangeImpactRequest {
    pub targets: Vec<CodeGraphChangeTarget>,
    #[serde(default = "default_change_impact_depth")]
    pub max_depth: usize,
    #[serde(default = "default_change_impact_limit")]
    pub max_entities: usize,
    #[serde(default)]
    pub include_inferred: bool,
}

fn default_change_impact_depth() -> usize {
    4
}
fn default_change_impact_limit() -> usize {
    256
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphEntityMetrics {
    pub entity_id: String,
    pub graph_revision: u64,
    pub inbound_count: usize,
    pub outbound_count: usize,
    pub observed_relation_count: usize,
    pub test_relation_count: usize,
    pub centrality: f64,
    pub change_risk: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphImpactPathStep {
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub relation: CodeRelationKind,
    pub confidence: f32,
    pub evidence_kind: RelationEvidenceKind,
    pub evidence_file: PathBuf,
    pub evidence_span: SourceSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphImpactedEntity {
    pub entity: CodeEntity,
    pub metrics: CodeGraphEntityMetrics,
    pub distance: usize,
    pub impact_score: f64,
    pub risk_tier: ImpactRiskTier,
    pub reasons: Vec<String>,
    pub path: Vec<CodeGraphImpactPathStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphVerificationTarget {
    pub file_path: PathBuf,
    pub reason: String,
    pub confidence: f64,
    #[serde(default)]
    pub related_entity_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphChangeImpactReport {
    pub schema_version: u32,
    pub graph_revision: u64,
    pub targets: Vec<CodeGraphChangeTarget>,
    pub roots: Vec<CodeEntity>,
    pub risk_tier: ImpactRiskTier,
    pub aggregate_risk_score: f64,
    pub affected_entities: Vec<CodeGraphImpactedEntity>,
    pub affected_files: Vec<PathBuf>,
    pub verification_targets: Vec<CodeGraphVerificationTarget>,
    pub public_api_entities: Vec<CodeEntity>,
    pub external_dependencies: Vec<CodeEntity>,
    pub unresolved_targets: Vec<String>,
    pub analysis_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphIndexReport {
    pub schema_version: u32,
    #[serde(default)]
    pub graph_revision: u64,
    pub workspace: PathBuf,
    pub database_path: PathBuf,
    pub scanned_files: usize,
    pub changed_files: usize,
    pub unchanged_files: usize,
    pub deleted_files: usize,
    pub parsed_entities: usize,
    pub parsed_relations: usize,
    pub parse_errors: usize,
    pub scan_millis: u128,
    pub parse_millis: u128,
    pub commit_millis: u128,
    pub total_millis: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistentGraphStats {
    pub schema_version: u32,
    #[serde(default)]
    pub graph_revision: u64,
    pub database_path: PathBuf,
    pub active_files: usize,
    pub active_entities: usize,
    pub active_relations: usize,
    pub historical_entities: usize,
    pub historical_relations: usize,
    pub database_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphPackageReference {
    pub label: String,
    pub outbound_references: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphSearchResult {
    pub graph_revision: u64,
    pub score: f64,
    pub entity: CodeEntity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphImpactReport {
    pub graph_revision: u64,
    pub root: CodeEntity,
    pub direction: GraphDirection,
    pub relation_kinds: Vec<CodeRelationKind>,
    pub depth: usize,
    pub entities: Vec<CodeEntity>,
    pub relations: Vec<CodeRelation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGraphPath {
    pub graph_revision: u64,
    pub direction: GraphDirection,
    pub relation_kinds: Vec<CodeRelationKind>,
    pub entities: Vec<CodeEntity>,
    pub relations: Vec<CodeRelation>,
}
