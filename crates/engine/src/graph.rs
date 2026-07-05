use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::diff::{ChangeClass, OpMatcher};
use crate::model::{OpIdx, ParsedModule, ParsedOp};

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

fn node_id(idx: OpIdx) -> String {
    format!("op{idx}")
}

fn label(op: &ParsedOp) -> String {
    op.result_types
        .first()
        .map(|result_type| format!("{} : {result_type}", op.name))
        .unwrap_or_else(|| op.name.clone())
}

fn node_of(op: &ParsedOp, change: Option<ChangeClass>) -> GraphNode {
    GraphNode {
        id: node_id(op.idx),
        label: label(op),
        op_name: op.name.clone(),
        line_range: (op.line_start, op.line_end),
        cluster: op.region_path.clone(),
        change,
        collapsed_count: 0,
    }
}

fn dataflow_edges(module: &ParsedModule, ops: &[OpIdx]) -> Vec<GraphEdge> {
    let mut definitions: HashMap<&str, OpIdx> = HashMap::new();
    for &op_idx in ops {
        for result in &module.ops[op_idx].results {
            definitions.insert(result, op_idx);
        }
    }

    let in_scope: HashSet<_> = ops.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut edges = Vec::new();
    for &use_idx in ops {
        for operand in &module.ops[use_idx].operands {
            let Some(&definition_idx) = definitions.get(operand.as_str()) else {
                continue;
            };
            if definition_idx != use_idx
                && in_scope.contains(&definition_idx)
                && seen.insert((definition_idx, use_idx))
            {
                edges.push(GraphEdge {
                    from: node_id(definition_idx),
                    to: node_id(use_idx),
                    removed: false,
                });
            }
        }
    }
    edges
}

fn collapse_to_budget(graph: DataflowGraph, _budget: usize) -> DataflowGraph {
    graph
}

pub fn extract_dataflow(module: &ParsedModule, func: &str, budget: usize) -> DataflowGraph {
    let Some(scope) = module.scope(func) else {
        return DataflowGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            clusters: Vec::new(),
            truncated: false,
        };
    };
    let nodes = scope
        .ops
        .iter()
        .map(|&op_idx| node_of(&module.ops[op_idx], None))
        .collect();
    let edges = dataflow_edges(module, &scope.ops);

    collapse_to_budget(
        DataflowGraph {
            nodes,
            edges,
            clusters: Vec::new(),
            truncated: false,
        },
        budget,
    )
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
