use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::diff::{diff_function, ChangeClass, OpMatcher};
use crate::model::{OpIdx, ParsedModule, ParsedOp};
use crate::SnapshotSide;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip)]
    pub op_idx: Option<OpIdx>,
    #[serde(skip)]
    pub provenance_side: Option<SnapshotSide>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct GraphCluster {
    pub path: Vec<usize>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
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

fn node_of(
    op: &ParsedOp,
    change: Option<ChangeClass>,
    provenance_side: Option<SnapshotSide>,
) -> GraphNode {
    GraphNode {
        id: node_id(op.idx),
        label: label(op),
        op_name: op.name.clone(),
        line_range: (op.line_start, op.line_end),
        cluster: op.region_path.clone(),
        change,
        collapsed_count: 0,
        uid: None,
        op_idx: Some(op.idx),
        provenance_side,
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
        uid: None,
        op_idx: None,
        provenance_side: None,
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
        .map(|&op_idx| node_of(&module.ops[op_idx], None, None))
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
    before: &ParsedModule,
    after: &ParsedModule,
    func: &str,
    budget: usize,
    matcher: &dyn OpMatcher,
) -> DataflowGraph {
    let Some(after_scope) = after.scope(func) else {
        return extract_dataflow(before, func, budget);
    };
    let diff = diff_function(before, after, func, matcher);
    let mut after_classes = HashMap::new();
    let mut removed_before = Vec::new();
    let mut before_to_after = HashMap::new();

    for change in &diff.changes {
        match (change.before, change.after, change.class) {
            (before_idx, Some(after_idx), class) => {
                after_classes.insert(after_idx, class);
                if let Some(before_idx) = before_idx {
                    before_to_after.insert(before_idx, after_idx);
                }
            }
            (Some(before_idx), None, ChangeClass::Removed) => removed_before.push(before_idx),
            _ => {}
        }
    }

    let mut nodes: Vec<_> = after_scope
        .ops
        .iter()
        .map(|&op_idx| {
            node_of(
                &after.ops[op_idx],
                Some(
                    after_classes
                        .get(&op_idx)
                        .copied()
                        .unwrap_or(ChangeClass::Unchanged),
                ),
                Some(SnapshotSide::After),
            )
        })
        .collect();
    let mut edges = dataflow_edges(after, &after_scope.ops);
    let ghost_id = |before_idx: OpIdx| format!("ghost{before_idx}");

    for &before_idx in &removed_before {
        let op = &before.ops[before_idx];
        nodes.push(GraphNode {
            id: ghost_id(before_idx),
            label: label(op),
            op_name: op.name.clone(),
            line_range: (op.line_start, op.line_end),
            cluster: op.region_path.clone(),
            change: Some(ChangeClass::Removed),
            collapsed_count: 0,
            uid: None,
            op_idx: Some(op.idx),
            provenance_side: Some(SnapshotSide::Before),
        });
    }

    let before_ops = before_scope_ops(before, func);
    let mut before_definitions = HashMap::new();
    for &op_idx in &before_ops {
        for result in &before.ops[op_idx].results {
            before_definitions.insert(result.as_str(), op_idx);
        }
    }
    let endpoint = |before_idx: OpIdx| {
        before_to_after
            .get(&before_idx)
            .map(|&after_idx| node_id(after_idx))
            .unwrap_or_else(|| ghost_id(before_idx))
    };

    for &before_idx in &removed_before {
        for operand in &before.ops[before_idx].operands {
            if let Some(&definition_idx) = before_definitions.get(operand.as_str()) {
                if definition_idx != before_idx {
                    edges.push(GraphEdge {
                        from: endpoint(definition_idx),
                        to: ghost_id(before_idx),
                        removed: true,
                    });
                }
            }
        }

        for &user_idx in &before_ops {
            if user_idx == before_idx {
                continue;
            }
            let uses_removed_result = before.ops[user_idx].operands.iter().any(|operand| {
                before.ops[before_idx]
                    .results
                    .iter()
                    .any(|result| result == operand)
            });
            if uses_removed_result {
                edges.push(GraphEdge {
                    from: ghost_id(before_idx),
                    to: endpoint(user_idx),
                    removed: true,
                });
            }
        }
    }

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

fn before_scope_ops(module: &ParsedModule, func: &str) -> Vec<OpIdx> {
    module
        .scope(func)
        .map(|scope| scope.ops.clone())
        .unwrap_or_default()
}
