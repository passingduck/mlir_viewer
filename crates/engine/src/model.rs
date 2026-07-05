use serde::Serialize;

/// Index of an op within `ParsedModule::ops`.
pub type OpIdx = usize;

/// One operation recovered from printed IR. Fields are best-effort: a line the
/// parser cannot understand still yields a `ParsedOp` with `opaque = true`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedOp {
    pub idx: OpIdx,
    /// Dialect-qualified op name, e.g. `arith.constant`. For opaque lines this
    /// is the first whitespace-delimited token.
    pub name: String,
    /// SSA result names including the leading `%`, e.g. `["%0"]`.
    pub results: Vec<String>,
    /// SSA operand names referenced by this op, in textual order, deduplicated.
    pub operands: Vec<String>,
    /// Result types as printed, e.g. `["tensor<4x4xf32>"]`.
    pub result_types: Vec<String>,
    /// Raw text of the op's attribute dictionary `{...}` if present, else "".
    pub attr_summary: String,
    /// `loc(...)` payload if the IR was printed with locations, else None.
    pub location: Option<String>,
    /// Region nesting path: the index-in-parent of each enclosing region op.
    pub region_path: Vec<usize>,
    /// Region nesting depth (== region_path.len()).
    pub depth: usize,
    /// 1-based inclusive line range of this op's statement in the snapshot.
    pub line_start: usize,
    pub line_end: usize,
    pub opaque: bool,
}

/// A function-like scope: the unit of diff and graph extraction.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionScope {
    /// Symbol name without `@`, e.g. `forward`. `(module)` when no function-like
    /// op is found in the snapshot.
    pub name: String,
    pub ops: Vec<OpIdx>,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedModule {
    pub ops: Vec<ParsedOp>,
    pub functions: Vec<FunctionScope>,
}

impl ParsedModule {
    pub fn scope(&self, func: &str) -> Option<&FunctionScope> {
        self.functions.iter().find(|f| f.name == func)
    }
}

/// The signal the fingerprint matcher scores on. Kept small and cheap to compare.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpFingerprint {
    pub op_name: String,
    pub result_types: Vec<String>,
    pub operand_count: usize,
    pub location: Option<String>,
}

impl OpFingerprint {
    pub fn of(op: &ParsedOp) -> OpFingerprint {
        OpFingerprint {
            op_name: op.name.clone(),
            result_types: op.result_types.clone(),
            operand_count: op.operands.len(),
            location: op.location.clone(),
        }
    }
}
