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

fn collapse_to_budget(mut graph: DataflowGraph, budget: usize) -> DataflowGraph {
    if graph.nodes.len() <= budget || budget == 0 {
        return maybe_truncate(graph, budget);
    }

    // The shallowest path is the function body itself, not a collapsible
    // nested region. Only deeper paths become cluster candidates.
    let root_depth = graph
        .nodes
        .iter()
        .filter(|node| node.collapsed_count == 0)
        .map(|node| node.cluster.len())
        .min()
        .unwrap_or(0);
    let mut cluster_paths: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| node.collapsed_count == 0 && node.cluster.len() > root_depth)
        .map(|node| node.cluster.clone())
        .collect();
    cluster_paths.sort();
    cluster_paths.dedup();
    cluster_paths.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));

    for path in cluster_paths {
        if graph.nodes.len() <= budget {
            break;
        }
        graph = collapse_cluster(graph, &path);
    }
    maybe_truncate(graph, budget)
}

fn collapse_cluster(graph: DataflowGraph, path: &[usize]) -> DataflowGraph {
    let in_cluster = |node: &GraphNode| {
        node.collapsed_count == 0 && !node.cluster.is_empty() && node.cluster.starts_with(path)
    };
    let hidden: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| in_cluster(node))
        .map(|node| node.id.clone())
        .collect();
    if hidden.is_empty() {
        return graph;
    }

    let meta_id = format!(
        "cluster{}",
        path.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join("_")
    );
    let hidden_set: HashSet<_> = hidden.iter().cloned().collect();
    let mut nodes: Vec<_> = graph
        .nodes
        .into_iter()
        .filter(|node| !hidden_set.contains(&node.id))
        .collect();
    nodes.push(GraphNode {
        id: meta_id.clone(),
        label: format!("{} ops", hidden.len()),
        op_name: "(cluster)".to_string(),
        line_range: (0, 0),
        cluster: path.to_vec(),
        change: None,
        collapsed_count: hidden.len(),
    });

    let remap = |id: &str| {
        if hidden_set.contains(id) {
            meta_id.clone()
        } else {
            id.to_string()
        }
    };
    let mut seen = HashSet::new();
    let mut edges = Vec::new();
    for edge in graph.edges {
        let from = remap(&edge.from);
        let to = remap(&edge.to);
        if from != to && seen.insert((from.clone(), to.clone(), edge.removed)) {
            edges.push(GraphEdge {
                from,
                to,
                removed: edge.removed,
            });
        }
    }

    let mut clusters = graph.clusters;
    clusters.push(GraphCluster {
        path: path.to_vec(),
        label: format!("region {path:?}"),
    });
    DataflowGraph {
        nodes,
        edges,
        clusters,
        truncated: graph.truncated,
    }
}

fn maybe_truncate(mut graph: DataflowGraph, budget: usize) -> DataflowGraph {
    if budget > 0 && graph.nodes.len() > budget {
        graph.truncated = true;
        // Preserve collapsed meta-nodes so truncation does not erase the only
        // indication that a nested region was summarized. Real nodes retain
        // their original op order within the remaining slots.
        let mut meta_nodes = Vec::new();
        let mut real_nodes = Vec::new();
        for node in graph.nodes {
            if node.collapsed_count > 0 {
                meta_nodes.push(node);
            } else {
                real_nodes.push(node);
            }
        }
        meta_nodes.extend(real_nodes);
        meta_nodes.truncate(budget);
        graph.nodes = meta_nodes;
        let kept: HashSet<_> = graph.nodes.iter().map(|node| node.id.clone()).collect();
        graph
            .edges
            .retain(|edge| kept.contains(&edge.from) && kept.contains(&edge.to));
    }
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
