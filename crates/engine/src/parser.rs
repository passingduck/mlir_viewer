use crate::model::{OpIdx, ParsedModule, ParsedOp};

/// A logical statement: one or more physical lines forming a single op or a
/// structural token (`{`, `}`, block label).
struct Statement {
    text: String,
    line_start: usize,
    line_end: usize,
}

/// Structural keywords whose bare (dot-less) first token still begins a new op.
const KEYWORDS: &[&str] = &["module", "return", "func", "cf", "scf", "llvm", "loc"];

fn starts_new_statement(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if matches!(first, '%' | '}' | '^' | '"') {
        return true;
    }
    let head: String = trimmed
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    if head.contains('.') {
        return true; // dialect.op
    }
    KEYWORDS.contains(&head.as_str())
}

fn assemble_statements(text: &str) -> Vec<Statement> {
    let mut out: Vec<Statement> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if out.is_empty() || starts_new_statement(trimmed) {
            out.push(Statement {
                text: trimmed.to_string(),
                line_start: line_no,
                line_end: line_no,
            });
        } else {
            let last = out.last_mut().expect("non-empty");
            last.text.push(' ');
            last.text.push_str(trimmed);
            last.line_end = line_no;
        }
    }
    out
}

/// Extract `%`-prefixed SSA names appearing in `s`, in order, deduplicated.
fn ssa_names(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                let c = bytes[i] as char;
                if c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-' | '#') {
                    i += 1;
                } else {
                    break;
                }
            }
            let name = s[start..i].to_string();
            if !out.contains(&name) {
                out.push(name);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Split a statement into (results, rest) at the top-level ` = `.
fn split_results(s: &str) -> (Vec<String>, &str) {
    // Results appear only before the first `=` and only as `%a, %b = ...`.
    if let Some(eq) = s.find('=') {
        let lhs = s[..eq].trim();
        if lhs.starts_with('%') && !lhs.contains('(') {
            let results = lhs.split(',').map(|r| r.trim().to_string()).collect();
            return (results, s[eq + 1..].trim_start());
        }
    }
    (Vec::new(), s)
}

/// First token after results is the op name (strip a leading quote for generic form).
fn op_name(rest: &str) -> String {
    let tok: String = rest
        .trim_start_matches('"')
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
        .collect();
    if tok.is_empty() {
        rest.split_whitespace().next().unwrap_or("").to_string()
    } else {
        tok
    }
}

/// The op's attribute dict: the first balanced `{...}` that closes within the
/// statement (a body region `{` would not close on the same statement here).
fn attr_summary(s: &str) -> String {
    let bytes = s.as_bytes();
    if let Some(open) = s.find('{') {
        let mut depth = 0i32;
        for (k, &b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return s[open..=k].to_string();
                    }
                }
                _ => {}
            }
        }
    }
    String::new()
}

/// Result types: everything after the final top-level `->`, else after the last `:`.
fn result_types(s: &str) -> Vec<String> {
    let tail = if let Some(p) = s.rfind("->") {
        &s[p + 2..]
    } else if let Some(p) = s.rfind(':') {
        &s[p + 1..]
    } else {
        return Vec::new();
    };
    tail.trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn location(s: &str) -> Option<String> {
    let p = s.find("loc(")?;
    let rest = &s[p + 4..];
    let mut depth = 1i32;
    for (k, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(rest[..k].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

pub fn parse_module(text: &str) -> ParsedModule {
    let mut ops: Vec<ParsedOp> = Vec::new();
    for st in assemble_statements(text) {
        let trimmed = st.text.trim();
        // Pure structural tokens carry no op.
        if trimmed == "}" || trimmed == "{" || trimmed.starts_with('^') {
            continue;
        }
        let (results, rest) = split_results(trimmed);
        let name = op_name(rest);
        // Operands = SSA names in the statement, minus the results.
        let operands: Vec<String> = ssa_names(rest)
            .into_iter()
            .filter(|n| !results.contains(n))
            .collect();
        let idx: OpIdx = ops.len();
        ops.push(ParsedOp {
            idx,
            name,
            results,
            operands,
            result_types: result_types(trimmed),
            attr_summary: attr_summary(trimmed),
            location: location(trimmed),
            region_path: Vec::new(),
            depth: 0,
            line_start: st.line_start,
            line_end: st.line_end,
            opaque: false,
        });
    }
    ParsedModule {
        ops,
        functions: Vec::new(),
    }
}
