use serde::Serialize;

use crate::model::{OpIdx, ParsedModule};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchMatch {
    pub func: String,
    pub op_idx: OpIdx,
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
    /// Short human-readable context: op name plus attr text, truncated.
    pub excerpt: String,
}

const EXCERPT_MAX: usize = 120;

fn excerpt(name: &str, attrs: &str) -> String {
    let mut text = if attrs.is_empty() {
        name.to_string()
    } else {
        format!("{name} {attrs}")
    };
    if text.len() > EXCERPT_MAX {
        let mut cut = EXCERPT_MAX;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push('…');
    }
    text
}

/// Case-insensitive substring search over every op's name, SSA names, result
/// types, attribute text, and location, within each function scope. Results
/// come back in (function, op) order and never exceed `budget`.
pub fn search_module(module: &ParsedModule, query: &str, budget: usize) -> Vec<SearchMatch> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() || budget == 0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for function in &module.functions {
        for &op_idx in &function.ops {
            let op = &module.ops[op_idx];
            let haystack = format!(
                "{} {} {} {} {} {}",
                op.name,
                op.results.join(" "),
                op.operands.join(" "),
                op.result_types.join(" "),
                op.attr_summary,
                op.location.as_deref().unwrap_or(""),
            )
            .to_lowercase();
            if haystack.contains(&needle) {
                out.push(SearchMatch {
                    func: function.name.clone(),
                    op_idx,
                    name: op.name.clone(),
                    line_start: op.line_start,
                    line_end: op.line_end,
                    excerpt: excerpt(&op.name, &op.attr_summary),
                });
                if out.len() == budget {
                    return out;
                }
            }
        }
    }
    out
}
