use engine::parse_module;

#[test]
fn parses_result_name_op_and_operands() {
    let m = parse_module("  %0 = arith.addf %1, %2 : tensor<4xf32>\n");
    assert_eq!(m.ops.len(), 1);
    let op = &m.ops[0];
    assert_eq!(op.name, "arith.addf");
    assert_eq!(op.results, vec!["%0"]);
    assert_eq!(op.operands, vec!["%1", "%2"]);
    assert_eq!(op.result_types, vec!["tensor<4xf32>"]);
    assert_eq!(op.line_start, 1);
    assert_eq!(op.line_end, 1);
    assert!(!op.opaque);
}

#[test]
fn joins_wrapped_continuation_lines_into_one_op() {
    let text = "\
%0 = linalg.matmul ins(%arg0, %arg1 : tensor<4x8xf32>, tensor<8x4xf32>)
    outs(%c : tensor<4x4xf32>) -> tensor<4x4xf32>
";
    let m = parse_module(text);
    assert_eq!(m.ops.len(), 1, "wrapped op must be a single statement");
    let op = &m.ops[0];
    assert_eq!(op.name, "linalg.matmul");
    assert_eq!(op.results, vec!["%0"]);
    assert!(op.operands.contains(&"%arg0".to_string()));
    assert!(op.operands.contains(&"%c".to_string()));
    assert_eq!(op.line_start, 1);
    assert_eq!(op.line_end, 2);
}

#[test]
fn captures_attribute_dict_summary() {
    let m = parse_module(
        "%0 = mycompiler.fused_matmul %arg0, %arg1 {tile_size = 4 : i64} : (tensor<4x8xf32>, tensor<8x4xf32>) -> tensor<4x4xf32>\n",
    );
    assert_eq!(m.ops[0].attr_summary, "{tile_size = 4 : i64}");
    assert!(m.ops[0].operands.contains(&"%arg0".to_string()));
}

#[test]
fn op_without_results_still_parses() {
    let m = parse_module("return %0 : tensor<4x4xf32>\n");
    assert_eq!(m.ops[0].name, "return");
    assert!(m.ops[0].results.is_empty());
    assert_eq!(m.ops[0].operands, vec!["%0"]);
}

#[test]
fn assigns_function_scope_and_nesting() {
    let text = "\
module {
  func.func @forward(%arg0: tensor<4x4xf32>) -> tensor<4x4xf32> {
    %0 = arith.negf %arg0 : tensor<4x4xf32>
    return %0 : tensor<4x4xf32>
  }
}
";
    let m = parse_module(text);
    assert_eq!(m.functions.len(), 1);
    let f = &m.functions[0];
    assert_eq!(f.name, "forward");
    let names: Vec<_> = f.ops.iter().map(|&i| m.ops[i].name.as_str()).collect();
    assert_eq!(names, vec!["arith.negf", "return"]);
    assert!(m.ops[f.ops[0]].depth >= 2);
}

#[test]
fn module_only_snapshot_yields_module_scope() {
    let m = parse_module("%0 = arith.constant 1 : i32\n%1 = arith.addi %0, %0 : i32\n");
    assert_eq!(m.functions.len(), 1);
    assert_eq!(m.functions[0].name, "(module)");
    assert_eq!(m.functions[0].ops.len(), 2);
}

#[test]
fn two_functions_are_separate_scopes() {
    let text = "\
llvm.func @a() { llvm.return }
llvm.func @b() { llvm.return }
";
    let m = parse_module(text);
    let names: Vec<_> = m.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b"]);
}
