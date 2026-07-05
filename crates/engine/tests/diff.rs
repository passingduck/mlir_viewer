use engine::{parse_module, GreedyFingerprintMatcher, OpMatcher};

fn all_ops(module: &engine::ParsedModule) -> Vec<usize> {
    (0..module.ops.len()).collect()
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
