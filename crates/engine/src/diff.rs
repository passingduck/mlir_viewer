use serde::Serialize;

use crate::model::{OpIdx, ParsedModule};

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

impl OpMatcher for GreedyFingerprintMatcher {
    fn match_ops(
        &self,
        _before: &ParsedModule,
        _before_ops: &[OpIdx],
        _after: &ParsedModule,
        _after_ops: &[OpIdx],
    ) -> Vec<(Option<OpIdx>, Option<OpIdx>)> {
        Vec::new()
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
