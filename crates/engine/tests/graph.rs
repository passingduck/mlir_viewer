use engine::{
    extract_dataflow, extract_dataflow_diff, parse_module, ChangeClass, GreedyFingerprintMatcher,
    SnapshotSide,
};

#[test]
fn builds_def_use_edges_within_function() {
    let module = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.addi %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let graph = extract_dataflow(&module, "f", 2000);

    assert_eq!(graph.nodes.len(), 3);
    let has_edge = |from_op: &str, to_op: &str| {
        graph.edges.iter().any(|edge| {
            let from = &graph
                .nodes
                .iter()
                .find(|node| node.id == edge.from)
                .unwrap()
                .op_name;
            let to = &graph
                .nodes
                .iter()
                .find(|node| node.id == edge.to)
                .unwrap()
                .op_name;
            from == from_op && to == to_op
        })
    };
    assert!(has_edge("arith.constant", "arith.addi"));
    assert!(has_edge("arith.addi", "return"));
    assert!(!graph.truncated);
}

#[test]
fn node_labels_carry_op_and_result_type() {
    let module = parse_module("func.func @f() {\n  %0 = arith.constant 1 : i32\n}\n");
    let graph = extract_dataflow(&module, "f", 2000);
    let node = &graph.nodes[0];

    assert_eq!(node.op_name, "arith.constant");
    assert!(node.label.contains("arith.constant"));
    assert_eq!(node.line_range, (2, 2));
    assert!(node.op_idx.is_some());
    assert_eq!(node.provenance_side, None);
    assert_eq!(node.uid, None);
}

#[test]
fn collapses_clusters_deterministically_under_budget() {
    let text = "\
func.func @f() {
  %0 = arith.constant 0 : i32
  scf.for %i = %0 to %0 step %0 {
    %1 = arith.addi %0, %0 : i32
    %2 = arith.muli %1, %0 : i32
  }
  return
}
";
    let module = parse_module(text);
    let full = extract_dataflow(&module, "f", 2000);
    let budgeted = extract_dataflow(&module, "f", 3);

    assert!(budgeted.nodes.len() <= 3);
    assert!(
        budgeted.nodes.iter().any(|node| node.collapsed_count > 0),
        "expected a meta-node"
    );
    assert_eq!(budgeted, extract_dataflow(&module, "f", 3));
    assert!(full.nodes.len() > budgeted.nodes.len());
}

#[test]
fn truncates_when_budget_below_cluster_count() {
    let text = "func.func @f() {\n  %0 = arith.constant 0 : i32\n  %1 = arith.addi %0, %0 : i32\n  %2 = arith.muli %1, %0 : i32\n}\n";
    let module = parse_module(text);
    let graph = extract_dataflow(&module, "f", 1);

    assert!(graph.truncated);
    assert!(graph.nodes.len() <= 1);
}

#[test]
fn diff_graph_tags_added_removed_and_marks_ghost() {
    let before = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.addi %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let after = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let graph = extract_dataflow_diff(&before, &after, "f", 2000, &GreedyFingerprintMatcher);
    let classes: Vec<_> = graph.nodes.iter().filter_map(|node| node.change).collect();

    assert!(classes.contains(&ChangeClass::Added));
    assert!(classes.contains(&ChangeClass::Removed));
    assert!(graph.nodes.iter().any(|node| node.id.starts_with("ghost")));
    assert!(graph.nodes.iter().any(|node| {
        node.id.starts_with("ghost")
            && node.op_idx.is_some()
            && node.provenance_side == Some(SnapshotSide::Before)
    }));
    assert!(graph.nodes.iter().any(|node| {
        node.id.starts_with("op")
            && node.op_idx.is_some()
            && node.provenance_side == Some(SnapshotSide::After)
    }));
    assert!(graph.edges.iter().any(|edge| edge.removed));
}
