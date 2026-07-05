use serde::{Deserialize, Serialize};

use crate::model::{OpFingerprint, OpIdx, ParsedModule, ParsedOp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeClass {
    Added,
    Removed,
    Modified,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct OpChange {
    pub class: ChangeClass,
    pub before: Option<OpIdx>,
    pub after: Option<OpIdx>,
    pub before_lines: Option<(usize, usize)>,
    pub after_lines: Option<(usize, usize)>,
    pub detail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct FunctionDiff {
    pub func: String,
    pub changes: Vec<OpChange>,
}

pub trait OpMatcher {
    /// Returns pairings `(before_idx, after_idx)`; `None` on a side means the op
    /// is unmatched (added when after-only, removed when before-only).
    fn match_ops(
        &self,
        before: &ParsedModule,
        before_ops: &[OpIdx],
        after: &ParsedModule,
        after_ops: &[OpIdx],
    ) -> Vec<(Option<OpIdx>, Option<OpIdx>)>;
}

pub struct GreedyFingerprintMatcher;

fn score(before: &OpFingerprint, after: &OpFingerprint) -> i32 {
    if before.op_name != after.op_name {
        return 0;
    }

    let mut score = 50;
    if before.result_types == after.result_types {
        score += 25;
    }
    if before.operand_count == after.operand_count {
        score += 15;
    }
    if before.location.is_some() && before.location == after.location {
        score += 10;
    }
    score
}

const MATCH_THRESHOLD: i32 = 50;

impl OpMatcher for GreedyFingerprintMatcher {
    fn match_ops(
        &self,
        before: &ParsedModule,
        before_ops: &[OpIdx],
        after: &ParsedModule,
        after_ops: &[OpIdx],
    ) -> Vec<(Option<OpIdx>, Option<OpIdx>)> {
        let before_fingerprints: Vec<_> = before_ops
            .iter()
            .map(|&idx| OpFingerprint::of(&before.ops[idx]))
            .collect();
        let after_fingerprints: Vec<_> = after_ops
            .iter()
            .map(|&idx| OpFingerprint::of(&after.ops[idx]))
            .collect();
        let mut after_taken = vec![false; after_ops.len()];
        let mut pairs = Vec::with_capacity(before_ops.len() + after_ops.len());

        for (before_position, fingerprint) in before_fingerprints.iter().enumerate() {
            if let Some(after_position) = (0..after_ops.len()).find(|&position| {
                !after_taken[position] && after_fingerprints[position] == *fingerprint
            }) {
                after_taken[after_position] = true;
                pairs.push((
                    Some(before_ops[before_position]),
                    Some(after_ops[after_position]),
                ));
            } else {
                pairs.push((Some(before_ops[before_position]), None));
            }
        }

        for pair in &mut pairs {
            let (Some(before_idx), None) = *pair else {
                continue;
            };
            let before_position = before_ops
                .iter()
                .position(|&idx| idx == before_idx)
                .expect("pair came from before_ops");
            let mut best: Option<(usize, i32)> = None;

            for after_position in 0..after_ops.len() {
                if after_taken[after_position] {
                    continue;
                }
                let candidate_score = score(
                    &before_fingerprints[before_position],
                    &after_fingerprints[after_position],
                );
                if candidate_score >= MATCH_THRESHOLD
                    && best
                        .map(|(_, best_score)| candidate_score > best_score)
                        .unwrap_or(true)
                {
                    best = Some((after_position, candidate_score));
                }
            }

            if let Some((after_position, _)) = best {
                after_taken[after_position] = true;
                pair.1 = Some(after_ops[after_position]);
            }
        }

        for (after_position, taken) in after_taken.into_iter().enumerate() {
            if !taken {
                pairs.push((None, Some(after_ops[after_position])));
            }
        }

        pairs
    }
}

fn detail(before: &ParsedOp, after: &ParsedOp) -> Vec<String> {
    let mut details = Vec::new();
    if before.result_types != after.result_types {
        details.push(format!(
            "result type {:?} → {:?}",
            before.result_types, after.result_types
        ));
    }
    if before.operands.len() != after.operands.len() {
        details.push(format!(
            "operand count {} → {}",
            before.operands.len(),
            after.operands.len()
        ));
    }
    if before.attr_summary != after.attr_summary {
        details.push(format!(
            "attributes {:?} → {:?}",
            before.attr_summary, after.attr_summary
        ));
    }
    details
}

pub fn diff_function(
    before: &ParsedModule,
    after: &ParsedModule,
    func: &str,
    matcher: &dyn OpMatcher,
) -> FunctionDiff {
    let empty = FunctionDiff {
        func: func.to_string(),
        changes: Vec::new(),
    };
    let (Some(before_scope), Some(after_scope)) = (before.scope(func), after.scope(func)) else {
        return empty;
    };
    let pairs = matcher.match_ops(before, &before_scope.ops, after, &after_scope.ops);
    let mut by_after = std::collections::HashMap::new();
    let mut removed = Vec::new();

    for (before_idx, after_idx) in pairs {
        match (before_idx, after_idx) {
            (Some(before_idx), Some(after_idx)) => {
                let before_op = &before.ops[before_idx];
                let after_op = &after.ops[after_idx];
                let details = detail(before_op, after_op);
                let class = if details.is_empty() {
                    ChangeClass::Unchanged
                } else {
                    ChangeClass::Modified
                };
                by_after.insert(
                    after_idx,
                    OpChange {
                        class,
                        before: Some(before_idx),
                        after: Some(after_idx),
                        before_lines: Some((before_op.line_start, before_op.line_end)),
                        after_lines: Some((after_op.line_start, after_op.line_end)),
                        detail: details,
                    },
                );
            }
            (None, Some(after_idx)) => {
                let after_op = &after.ops[after_idx];
                by_after.insert(
                    after_idx,
                    OpChange {
                        class: ChangeClass::Added,
                        before: None,
                        after: Some(after_idx),
                        before_lines: None,
                        after_lines: Some((after_op.line_start, after_op.line_end)),
                        detail: Vec::new(),
                    },
                );
            }
            (Some(before_idx), None) => {
                let before_op = &before.ops[before_idx];
                removed.push(OpChange {
                    class: ChangeClass::Removed,
                    before: Some(before_idx),
                    after: None,
                    before_lines: Some((before_op.line_start, before_op.line_end)),
                    after_lines: None,
                    detail: Vec::new(),
                });
            }
            (None, None) => {}
        }
    }

    removed.sort_by_key(|change| change.before.expect("removed op has before index"));
    let mut removed = removed.into_iter().peekable();
    let mut changes = Vec::new();
    let mut after_order = after_scope.ops.clone();
    after_order.sort_unstable();

    for after_idx in after_order {
        if let Some(change) = by_after.remove(&after_idx) {
            if let Some(this_before) = change.before {
                while removed
                    .peek()
                    .map(|change| change.before.expect("removed op has before index") < this_before)
                    .unwrap_or(false)
                {
                    changes.push(removed.next().expect("peeked above"));
                }
            }
            changes.push(change);
        }
    }
    changes.extend(removed);

    FunctionDiff {
        func: func.to_string(),
        changes,
    }
}
