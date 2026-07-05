use serde::Serialize;

use crate::diff::{ChangeClass, OpMatcher};
use crate::model::ParsedModule;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub op_name: String,
    pub line_range: (usize, usize),
    pub cluster: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<ChangeClass>,
    /// >0 for a collapsed cluster meta-node: how many ops it hides.
    pub collapsed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphCluster {
    pub path: Vec<usize>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DataflowGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub clusters: Vec<GraphCluster>,
    pub truncated: bool,
}

pub fn extract_dataflow(_module: &ParsedModule, _func: &str, _budget: usize) -> DataflowGraph {
    DataflowGraph {
        nodes: Vec::new(),
        edges: Vec::new(),
        clusters: Vec::new(),
        truncated: false,
    }
}

pub fn extract_dataflow_diff(
    _before: &ParsedModule,
    _after: &ParsedModule,
    _func: &str,
    _budget: usize,
    _matcher: &dyn OpMatcher,
) -> DataflowGraph {
    DataflowGraph {
        nodes: Vec::new(),
        edges: Vec::new(),
        clusters: Vec::new(),
        truncated: false,
    }
}
