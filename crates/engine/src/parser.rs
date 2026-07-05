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
    let mut out: Vec<String> = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c != '%' {
            continue;
        }
        // Consume the identifier body, advancing by whole chars so slice
        // boundaries always land on char boundaries (never panic on UTF-8).
        let mut end = start + c.len_utf8();
        while let Some(&(idx, ch)) = chars.peek() {
            if ch.is_alphanumeric() || matches!(ch, '_' | '.' | '$' | '-' | '#') {
                end = idx + ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let name = s[start..end].to_string();
        if !out.contains(&name) {
            out.push(name);
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
    // Find `loc(` as a standalone token, not as a substring of an op name
    // like `memref.alloc(` or `memref.dealloc(`. The char before `loc` must
    // not be part of a preceding identifier.
    let mut search = 0;
    let p = loop {
        let rel = s[search..].find("loc(")?;
        let at = search + rel;
        let boundary = s[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-' | '#')));
        if boundary {
            break at;
        }
        search = at + 4;
    };
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

/// Net `{` minus `}` in a statement, ignoring braces inside strings.
fn brace_delta(s: &str) -> i32 {
    let mut delta = 0;
    let mut in_string = false;
    let mut escaped = false;

    for byte in s.bytes() {
        if in_string && byte == b'\\' && !escaped {
            escaped = true;
            continue;
        }
        if byte == b'"' && !escaped {
            in_string = !in_string;
        } else if !in_string {
            match byte {
                b'{' => delta += 1,
                b'}' => delta -= 1,
                _ => {}
            }
        }
        escaped = false;
    }

    delta
}

fn symbol(s: &str) -> Option<String> {
    let at = s.find('@')?;
    let name: String = s[at + 1..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-'))
        .collect();
    (!name.is_empty()).then_some(name)
}

pub fn parse_module(text: &str) -> ParsedModule {
    use crate::model::FunctionScope;

    let mut ops: Vec<ParsedOp> = Vec::new();
    let mut functions: Vec<FunctionScope> = Vec::new();
    let mut region_path: Vec<usize> = Vec::new();
    let mut sibling_counter = vec![0usize];
    let mut active_scope: Option<(usize, usize)> = None;

    for st in assemble_statements(text) {
        let trimmed = st.text.trim();
        let delta = brace_delta(trimmed);
        let opens_region = delta > 0;

        if trimmed == "}" || (delta < 0 && trimmed.starts_with('}')) {
            for _ in 0..(-delta) {
                region_path.pop();
                sibling_counter.pop();
                if let Some((function_idx, body_depth)) = active_scope {
                    if region_path.len() < body_depth {
                        functions[function_idx].line_end = st.line_end;
                        active_scope = None;
                    }
                }
            }
            continue;
        }

        if trimmed == "{" || trimmed.starts_with('^') {
            if opens_region {
                for _ in 0..delta {
                    let child = sibling_counter
                        .last_mut()
                        .map(|counter| {
                            let child = *counter;
                            *counter += 1;
                            child
                        })
                        .unwrap_or(0);
                    region_path.push(child);
                    sibling_counter.push(0);
                }
            }
            continue;
        }

        let depth = region_path.len();
        let (results, rest) = split_results(trimmed);
        let name = op_name(rest);
        let opaque = name.is_empty()
            || !name
                .chars()
                .next()
                .map(|first| first.is_alphabetic() || first == '_')
                .unwrap_or(false);
        let name = if opaque {
            trimmed
                .split_whitespace()
                .next()
                .unwrap_or("<opaque>")
                .to_string()
        } else {
            name
        };
        let operands: Vec<String> = ssa_names(rest)
            .into_iter()
            .filter(|n| !results.contains(n))
            .collect();
        let (results, operands, result_types, attr_summary) = if opaque {
            (Vec::new(), Vec::new(), Vec::new(), String::new())
        } else {
            (
                results,
                operands,
                result_types(trimmed),
                attr_summary(trimmed),
            )
        };
        let idx: OpIdx = ops.len();
        ops.push(ParsedOp {
            idx,
            name,
            results,
            operands,
            result_types,
            attr_summary,
            location: location(trimmed),
            region_path: region_path.clone(),
            depth,
            line_start: st.line_start,
            line_end: st.line_end,
            opaque,
        });

        // Balanced single-line function bodies need their inner operations
        // parsed separately because statement assembly keeps the line intact.
        let balanced_function = !opens_region && symbol(trimmed).is_some();
        if balanced_function {
            if let (Some(open), Some(close)) = (trimmed.find('{'), trimmed.rfind('}')) {
                if open < close {
                    let inner = parse_module(&trimmed[open + 1..close]);
                    let mut inner_ops = Vec::new();
                    for mut op in inner.ops {
                        op.idx = ops.len();
                        op.line_start = st.line_start;
                        op.line_end = st.line_end;
                        inner_ops.push(op.idx);
                        ops.push(op);
                    }
                    functions.push(FunctionScope {
                        name: symbol(trimmed).expect("checked above"),
                        ops: inner_ops,
                        line_start: st.line_start,
                        line_end: st.line_end,
                    });
                }
            }
        }

        if opens_region {
            if let Some(function_name) = symbol(trimmed).filter(|_| active_scope.is_none()) {
                let function_idx = functions.len();
                functions.push(FunctionScope {
                    name: function_name,
                    ops: Vec::new(),
                    line_start: st.line_start,
                    line_end: st.line_end,
                });
                active_scope = Some((function_idx, region_path.len() + 1));
            } else if let Some((function_idx, _)) = active_scope {
                functions[function_idx].ops.push(idx);
                functions[function_idx].line_end = st.line_end;
            }
        } else if let Some((function_idx, _)) = active_scope {
            functions[function_idx].ops.push(idx);
            functions[function_idx].line_end = st.line_end;
        }

        if opens_region {
            for _ in 0..delta {
                let child = sibling_counter
                    .last_mut()
                    .map(|counter| {
                        let child = *counter;
                        *counter += 1;
                        child
                    })
                    .unwrap_or(0);
                region_path.push(child);
                sibling_counter.push(0);
            }
        }
    }

    if functions.is_empty() && !ops.is_empty() {
        functions.push(FunctionScope {
            name: "(module)".to_string(),
            ops: (0..ops.len()).collect(),
            line_start: ops.first().map(|op| op.line_start).unwrap_or(1),
            line_end: ops.last().map(|op| op.line_end).unwrap_or(1),
        });
    }

    ParsedModule { ops, functions }
}
