mod builder;
mod extractor;
mod model;
mod persistent;
mod query;
mod schema;
mod store;

pub use builder::WorkspaceGraphBuilder;
pub use model::{GraphEdge, GraphEdgeKind, GraphNode, GraphNodeKind, WorkspaceGraph};
pub use persistent::PersistentCodeGraph;
pub use query::{GraphImpactReport, GraphQueryResult};
pub use schema::{
    CODE_GRAPH_SCHEMA_VERSION, ChangeKind, CodeEntity, CodeEntityKind, CodeGraphChangeImpactReport,
    CodeGraphChangeImpactRequest, CodeGraphChangeTarget, CodeGraphEntityMetrics,
    CodeGraphImpactPathStep, CodeGraphImpactReport, CodeGraphImpactedEntity, CodeGraphIndexReport,
    CodeGraphPackageReference, CodeGraphPath, CodeGraphSearchResult, CodeGraphVerificationTarget,
    CodeLanguage, CodeRelation, CodeRelationKind, GraphDirection, ImpactRiskTier,
    PersistentGraphStats, RelationEvidenceKind, SourceSpan,
};
