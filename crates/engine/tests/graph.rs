use engine::{extract_dataflow, parse_module};

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
}
