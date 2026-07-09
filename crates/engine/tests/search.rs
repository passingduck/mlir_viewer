use engine::{parse_module, search_module};

const IR: &str = r#"module {
  func.func @forward(%arg0: i32) -> i32 {
    %c = arith.constant {value = 42 : i32} 42 : i32
    %r = arith.addi %arg0, %c : i32
    return %r : i32
  }
}"#;

#[test]
fn matches_are_case_insensitive_and_scoped_to_functions() {
    let module = parse_module(IR);
    let hits = search_module(&module, "ADDI", 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].func, "forward");
    assert_eq!(hits[0].name, "arith.addi");
    assert!(hits[0].line_start >= 1);
}

#[test]
fn matches_attributes_and_respects_budget() {
    let module = parse_module(IR);
    assert_eq!(search_module(&module, "value = 42", 10).len(), 1);
    assert_eq!(search_module(&module, "arith", 1).len(), 1); // 2 candidates, budget 1
    assert!(search_module(&module, "", 10).is_empty()); // blank query matches nothing
    assert!(search_module(&module, "zzz", 10).is_empty());
}
