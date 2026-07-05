use engine::{
    diff_function, fingerprint_score, parse_module, ChangeClass, GreedyFingerprintMatcher,
    OpFingerprint, OpMatcher,
};

fn all_ops(module: &engine::ParsedModule) -> Vec<usize> {
    (0..module.ops.len()).collect()
}

#[test]
fn fingerprint_score_is_normalized_and_rejects_different_names() {
    let base = OpFingerprint {
        op_name: "arith.addi".into(),
        result_types: vec!["i32".into()],
        operand_count: 2,
        location: Some("file.mlir:1:2".into()),
    };
    assert_eq!(fingerprint_score(&base, &base), Some(100));

    let mut changed = base.clone();
    changed.op_name = "arith.muli".into();
    assert_eq!(fingerprint_score(&base, &changed), None);
}

#[test]
fn identical_functions_match_all_ops_positionally() {
    let text = "%0 = arith.constant 1 : i32\n%1 = arith.addi %0, %0 : i32\n";
    let before = parse_module(text);
    let after = parse_module(text);
    let pairs =
        GreedyFingerprintMatcher.match_ops(&before, &all_ops(&before), &after, &all_ops(&after));

    assert_eq!(pairs.len(), 2);
    assert!(pairs
        .iter()
        .all(|(before_idx, after_idx)| before_idx.is_some() && after_idx.is_some()));
}

#[test]
fn removed_op_is_left_unmatched_on_before_side() {
    let before = parse_module("%0 = arith.constant 1 : i32\n%1 = arith.addi %0, %0 : i32\n");
    let after = parse_module("%0 = arith.constant 1 : i32\n");
    let pairs =
        GreedyFingerprintMatcher.match_ops(&before, &all_ops(&before), &after, &all_ops(&after));

    assert_eq!(
        pairs
            .iter()
            .filter(|(_, after_idx)| after_idx.is_none())
            .count(),
        1
    );
    assert_eq!(
        pairs
            .iter()
            .filter(|(before_idx, _)| before_idx.is_none())
            .count(),
        0
    );
}

#[test]
fn never_matches_across_different_op_names() {
    let before = parse_module("%0 = arith.addf %1, %2 : f32\n");
    let after = parse_module("%0 = arith.mulf %1, %2 : f32\n");
    let pairs =
        GreedyFingerprintMatcher.match_ops(&before, &all_ops(&before), &after, &all_ops(&after));

    assert!(pairs
        .iter()
        .any(|(before_idx, after_idx)| before_idx.is_some() && after_idx.is_none()));
    assert!(pairs
        .iter()
        .any(|(before_idx, after_idx)| before_idx.is_none() && after_idx.is_some()));
}

#[test]
fn classifies_added_removed_modified_unchanged() {
    let before = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.addi %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let after = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.muli %0, %0 : i32\n  %2 = arith.subi %1, %0 : i32\n  return %1 : i32\n}\n",
    );
    let diff = diff_function(&before, &after, "f", &GreedyFingerprintMatcher);
    let classes: Vec<_> = diff.changes.iter().map(|change| change.class).collect();

    assert!(classes.contains(&ChangeClass::Unchanged));
    assert!(classes.contains(&ChangeClass::Removed));
    assert!(classes.contains(&ChangeClass::Added));
}

#[test]
fn modified_op_reports_detail_and_both_line_ranges() {
    let before = parse_module("func.func @f() {\n  %0 = arith.constant 1 : i32\n}\n");
    let after = parse_module("func.func @f() {\n  %0 = arith.constant 1 : i64\n}\n");
    let diff = diff_function(&before, &after, "f", &GreedyFingerprintMatcher);
    let modified = diff
        .changes
        .iter()
        .find(|change| change.class == ChangeClass::Modified)
        .unwrap();

    assert!(modified.before_lines.is_some() && modified.after_lines.is_some());
    assert!(modified.detail.iter().any(|detail| detail.contains("type")));
}

#[test]
fn unknown_function_yields_empty_diff() {
    let module = parse_module("func.func @f() {\n  return\n}\n");
    let diff = diff_function(&module, &module, "nope", &GreedyFingerprintMatcher);
    assert!(diff.changes.is_empty());
}
