pub mod diff;
pub mod graph;
pub mod model;
pub mod parser;
pub mod provenance;

pub use diff::{
    diff_function, fingerprint_score, ChangeClass, FunctionDiff, GreedyFingerprintMatcher,
    OpChange, OpMatcher,
};
pub use graph::{
    extract_dataflow, extract_dataflow_diff, DataflowGraph, GraphCluster, GraphEdge, GraphNode,
};
pub use model::{FunctionScope, OpFingerprint, OpIdx, ParsedModule, ParsedOp};
pub use parser::parse_module;
pub use provenance::{
    resolve_function, EvidenceSource, HistoryChange, HistoryEvidence, HistoryStep, LinkConfidence,
    NormalizedIdentityEvent, NormalizedIdentityKind, OccurrenceKey, OpAnchor, OpHistory,
    OpOccurrence, OpUid, ResolvedFunction, SelectableOp, SnapshotOps, SnapshotSide, TimelineStage,
    UidError,
};
