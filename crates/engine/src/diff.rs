use serde::Serialize;

use crate::model::{OpFingerprint, OpIdx, ParsedModule};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeClass {
    Added,
    Removed,
    Modified,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpChange {
    pub class: ChangeClass,
    pub before: Option<OpIdx>,
    pub after: Option<OpIdx>,
    pub before_lines: Option<(usize, usize)>,
    pub after_lines: Option<(usize, usize)>,
    pub detail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
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

pub fn diff_function(
    _before: &ParsedModule,
    _after: &ParsedModule,
    func: &str,
    _matcher: &dyn OpMatcher,
) -> FunctionDiff {
    FunctionDiff {
        func: func.to_string(),
        changes: Vec::new(),
    }
}
