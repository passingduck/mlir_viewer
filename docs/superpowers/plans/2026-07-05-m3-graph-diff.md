# M3 — Graph View & Structural Diff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `[Text | Graph]` view toggle and a `[Diff]` toggle to the viewer, backed by a new server-side `crates/engine` that parses printed MLIR, computes per-function structural diffs, and extracts clustered SSA def-use graphs.

**Architecture:** A new pure-Rust `engine` crate holds three seams — a tolerant text parser (`parser`), a structural diff (`diff`) behind an `OpMatcher` trait, and a graph extractor (`graph`). The `server` crate gains three budgeted MessagePack endpoints (`/functions`, `/diff`, `/graphs/dataflow`) that call the engine over full blobs and cache results by blob id. The React UI gains a toolbar, structural-diff CodeMirror decorations in Text mode, and a Graph mode that lays out with ELK in a Web Worker and renders on a custom canvas-2D with level-of-detail. Nothing new ships the whole module to the browser: diff and graph are computed server-side, text display stays paged.

**Tech Stack:** Rust 2021 (new `engine` crate; `server` gains `rmp-serde`); React 19 + TypeScript, Zustand, CodeMirror 6, `elkjs` (Web Worker), `@msgpack/msgpack`, Vitest + Playwright.

## Global Constraints

- **Base branch:** create `feat/m3-graph-diff` off `feat/m2-walking-skeleton` before Task 1.
- **Trace format is v1, text-only.** No structural op rows, no identity uids. The engine recovers all structure from printed IR text. Do not modify the trace schema or `FORMAT_VERSION`.
- **Matcher is uid-first, fingerprint-fallback by interface, fingerprint-only by implementation.** Ship `GreedyFingerprintMatcher`; keep the `OpMatcher` trait as the seam so an M4 uid matcher drops in without touching diff/graph/server code.
- **Never ship the whole module to the browser** (parent spec §10.1). Diff and graph are server-computed over full blobs; only text display is paged at 256 KiB.
- **Every new API response is budgeted or bounded** (ADR-6). Graph responses honor a node `budget` (default 2000) and set `truncated`. JSON stays the control plane; **MessagePack** carries diff and graph bulk payloads.
- **The parser must never abort a snapshot.** A line that fails to parse becomes an *opaque op* (name = first token, no operands) and parsing continues.
- **Graph is SSA def-use only** in M3 (node = op, edge = value def→use), behind a `GraphExtractor` seam. No CFG/call graph, no provenance, no search, no inspector (node click only records selection).
- Rust: keep `cargo fmt` clean and `cargo clippy` warning-free, matching existing crates. UI: keep `npm run typecheck` clean.

---

## File structure

**New crate `crates/engine`** (pure library, no I/O, no tokio):
- `crates/engine/Cargo.toml` — deps: `serde` (derive). Dev-deps: none beyond std.
- `crates/engine/src/lib.rs` — module wiring + re-exports.
- `crates/engine/src/model.rs` — `ParsedOp`, `FunctionScope`, `ParsedModule`, `OpFingerprint`, id aliases.
- `crates/engine/src/parser.rs` — `parse_module(text) -> ParsedModule` (tolerant, infallible).
- `crates/engine/src/diff.rs` — `ChangeClass`, `OpChange`, `FunctionDiff`, `OpMatcher` trait, `GreedyFingerprintMatcher`, `diff_function(...)`.
- `crates/engine/src/graph.rs` — `DataflowGraph`, `GraphNode`, `GraphEdge`, `GraphCluster`, `extract_dataflow(...)`, `extract_dataflow_diff(...)`.

**Modified `crates/server`**:
- `crates/server/Cargo.toml` — add `engine` + `rmp-serde` + `tokio` (for cache mutex — actually `std::sync::Mutex` suffices; add `engine`, `rmp-serde` only).
- `crates/server/src/msgpack.rs` (new) — `Msgpack<T>` response wrapper.
- `crates/server/src/cache.rs` (new) — parse cache (by blob id) + diff cache (by blob-pair+func).
- `crates/server/src/api.rs` — new handlers: `functions`, `diff`, `graph`.
- `crates/server/src/lib.rs` — register routes; put caches in `ServerState`.

**Modified `ui/`**:
- `ui/package.json` — add `elkjs`, `@msgpack/msgpack`.
- `ui/src/api.ts` — new types + `functions`/`diff`/`graph` fetchers (msgpack decode).
- `ui/src/store.ts` — `viewMode`, `diffEnabled`, `selectedFunc`, function list, diff & graph payloads, actions.
- `ui/src/components/Toolbar.tsx` (new) — segmented control, diff toggle, function dropdown, keyboard.
- `ui/src/diffDecorations.ts` (new) — pure map: `FunctionDiff` → CodeMirror decoration ranges.
- `ui/src/components/IrViewer.tsx` — accept diff decorations + scroll-sync anchors.
- `ui/src/graph/layout.worker.ts` (new) — ELK layered layout in a worker.
- `ui/src/graph/render.ts` (new) — pure canvas draw + hit-testing + LOD helpers.
- `ui/src/components/GraphView.tsx` (new) — canvas host, zoom/pan, worker orchestration, legend.
- `ui/src/App.tsx` — mount Toolbar; switch Text/Graph panes.

---

## Task 1: Engine crate scaffold + data model

**Files:**
- Create: `crates/engine/Cargo.toml`
- Create: `crates/engine/src/lib.rs`
- Create: `crates/engine/src/model.rs`
- Modify: `Cargo.toml` (workspace members)

**Interfaces:**
- Produces: the shared model every later engine task uses.
  - `type OpIdx = usize;`
  - `struct ParsedOp { idx: OpIdx, name: String, results: Vec<String>, operands: Vec<String>, result_types: Vec<String>, attr_summary: String, location: Option<String>, region_path: Vec<usize>, depth: usize, line_start: usize, line_end: usize, opaque: bool }`
  - `struct FunctionScope { name: String, ops: Vec<OpIdx>, line_start: usize, line_end: usize }`
  - `struct ParsedModule { ops: Vec<ParsedOp>, functions: Vec<FunctionScope> }` with `fn scope(&self, func: &str) -> Option<&FunctionScope>`.
  - `struct OpFingerprint { op_name: String, result_types: Vec<String>, operand_count: usize, location: Option<String> }` with `fn of(op: &ParsedOp) -> OpFingerprint`.

- [ ] **Step 1: Add the crate to the workspace**

Modify `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/trace-format", "crates/engine", "crates/server", "crates/cli"]
```

- [ ] **Step 2: Create `crates/engine/Cargo.toml`**

```toml
[package]
name = "engine"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }

[dev-dependencies]
trace-format = { path = "../trace-format" }
```

- [ ] **Step 3: Write `crates/engine/src/model.rs`**

```rust
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
```

- [ ] **Step 4: Write `crates/engine/src/lib.rs`**

```rust
pub mod diff;
pub mod graph;
pub mod model;
pub mod parser;

pub use diff::{diff_function, ChangeClass, FunctionDiff, GreedyFingerprintMatcher, OpChange, OpMatcher};
pub use graph::{
    extract_dataflow, extract_dataflow_diff, DataflowGraph, GraphCluster, GraphEdge, GraphNode,
};
pub use model::{FunctionScope, OpFingerprint, OpIdx, ParsedModule, ParsedOp};
pub use parser::parse_module;
```

This will not compile until later tasks add `parser.rs`, `diff.rs`, `graph.rs`. Create empty stubs so the crate builds now:

- [ ] **Step 5: Create stub modules so the crate compiles**

`crates/engine/src/parser.rs`:

```rust
use crate::model::ParsedModule;

pub fn parse_module(_text: &str) -> ParsedModule {
    ParsedModule { ops: Vec::new(), functions: Vec::new() }
}
```

`crates/engine/src/diff.rs`:

```rust
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
    FunctionDiff { func: func.to_string(), changes: Vec::new() }
}
```

`crates/engine/src/graph.rs`:

```rust
use serde::Serialize;

use crate::diff::{ChangeClass, OpMatcher};
use crate::model::ParsedModule;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub op_name: String,
    pub line_range: (usize, usize),
    pub cluster: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change: Option<ChangeClass>,
    /// >0 for a collapsed cluster meta-node: how many ops it hides.
    pub collapsed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphCluster {
    pub path: Vec<usize>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DataflowGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub clusters: Vec<GraphCluster>,
    pub truncated: bool,
}

pub fn extract_dataflow(_module: &ParsedModule, _func: &str, _budget: usize) -> DataflowGraph {
    DataflowGraph { nodes: Vec::new(), edges: Vec::new(), clusters: Vec::new(), truncated: false }
}

pub fn extract_dataflow_diff(
    _before: &ParsedModule,
    _after: &ParsedModule,
    _func: &str,
    _budget: usize,
    _matcher: &dyn OpMatcher,
) -> DataflowGraph {
    DataflowGraph { nodes: Vec::new(), edges: Vec::new(), clusters: Vec::new(), truncated: false }
}
```

- [ ] **Step 6: Verify the crate compiles**

Run: `cargo build -p engine`
Expected: builds clean (warnings for unused args are acceptable at this stage; suppress with `_` prefixes already applied).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/engine
git commit -m "feat(engine): scaffold engine crate with parsed-IR data model"
```

---

## Task 2: Parser — statement assembly & single-op fields

**Files:**
- Modify: `crates/engine/src/parser.rs`
- Create: `crates/engine/tests/parser.rs`

**Interfaces:**
- Consumes: `ParsedModule`, `ParsedOp` (Task 1).
- Produces: `parse_module` populates `ops` with correct `name`, `results`, `operands`, `result_types`, `attr_summary`, `line_start`, `line_end` for well-formed pretty-form ops. Region/scope come in Task 3.

The parser groups **physical lines into logical statements** because the fixtures wrap ops across lines. Rule: a trimmed physical line *starts a new statement* iff it begins with `%`, `}`, `^`, `"`, or a token that is a structural keyword (`module`, `return`, `func`, `cf`, `llvm`, …) or a dialect-qualified op name `word.word`. Otherwise it continues the previous statement (e.g. a wrapped `outs(...) -> ...`).

- [ ] **Step 1: Write failing tests** in `crates/engine/tests/parser.rs`

```rust
use engine::parse_module;

#[test]
fn parses_result_name_op_and_operands() {
    let m = parse_module("  %0 = arith.addf %1, %2 : tensor<4xf32>\n");
    assert_eq!(m.ops.len(), 1);
    let op = &m.ops[0];
    assert_eq!(op.name, "arith.addf");
    assert_eq!(op.results, vec!["%0"]);
    assert_eq!(op.operands, vec!["%1", "%2"]);
    assert_eq!(op.result_types, vec!["tensor<4xf32>"]);
    assert_eq!(op.line_start, 1);
    assert_eq!(op.line_end, 1);
    assert!(!op.opaque);
}

#[test]
fn joins_wrapped_continuation_lines_into_one_op() {
    let text = "\
%0 = linalg.matmul ins(%arg0, %arg1 : tensor<4x8xf32>, tensor<8x4xf32>)
    outs(%c : tensor<4x4xf32>) -> tensor<4x4xf32>
";
    let m = parse_module(text);
    assert_eq!(m.ops.len(), 1, "wrapped op must be a single statement");
    let op = &m.ops[0];
    assert_eq!(op.name, "linalg.matmul");
    assert_eq!(op.results, vec!["%0"]);
    assert!(op.operands.contains(&"%arg0".to_string()));
    assert!(op.operands.contains(&"%c".to_string()));
    assert_eq!(op.line_start, 1);
    assert_eq!(op.line_end, 2);
}

#[test]
fn captures_attribute_dict_summary() {
    let m = parse_module(
        "%0 = mycompiler.fused_matmul %arg0, %arg1 {tile_size = 4 : i64} : (tensor<4x8xf32>, tensor<8x4xf32>) -> tensor<4x4xf32>\n",
    );
    assert_eq!(m.ops[0].attr_summary, "{tile_size = 4 : i64}");
    assert!(m.ops[0].operands.contains(&"%arg0".to_string()));
}

#[test]
fn op_without_results_still_parses() {
    let m = parse_module("return %0 : tensor<4x4xf32>\n");
    assert_eq!(m.ops[0].name, "return");
    assert!(m.ops[0].results.is_empty());
    assert_eq!(m.ops[0].operands, vec!["%0"]);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p engine --test parser`
Expected: FAIL (stub returns empty module).

- [ ] **Step 3: Implement statement assembly + field extraction** in `crates/engine/src/parser.rs`

```rust
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
    let Some(first) = trimmed.chars().next() else { return false };
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
            out.push(Statement { text: trimmed.to_string(), line_start: line_no, line_end: line_no });
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
    if tok.is_empty() { rest.split_whitespace().next().unwrap_or("").to_string() } else { tok }
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
        let operands: Vec<String> =
            ssa_names(rest).into_iter().filter(|n| !results.contains(n)).collect();
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
    ParsedModule { ops, functions: Vec::new() }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine --test parser`
Expected: PASS (all four).

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/parser.rs crates/engine/tests/parser.rs
git commit -m "feat(engine): assemble wrapped statements and extract op fields"
```

---

## Task 3: Parser — region nesting & function scope detection

**Files:**
- Modify: `crates/engine/src/parser.rs`
- Modify: `crates/engine/tests/parser.rs`

**Interfaces:**
- Consumes: statement assembly (Task 2).
- Produces: each `ParsedOp` gets `region_path`/`depth`; `ParsedModule::functions` is populated. A function-like op = a statement carrying an `@symbol` that opens a region (`{` unbalanced at end of statement). No function-like ops ⇒ one scope named `(module)` holding every op.

- [ ] **Step 1: Add failing tests** to `crates/engine/tests/parser.rs`

```rust
#[test]
fn assigns_function_scope_and_nesting() {
    let text = "\
module {
  func.func @forward(%arg0: tensor<4x4xf32>) -> tensor<4x4xf32> {
    %0 = arith.negf %arg0 : tensor<4x4xf32>
    return %0 : tensor<4x4xf32>
  }
}
";
    let m = parse_module(text);
    assert_eq!(m.functions.len(), 1);
    let f = &m.functions[0];
    assert_eq!(f.name, "forward");
    // The two body ops (negf, return) belong to the scope; nested deeper than module.
    let names: Vec<_> = f.ops.iter().map(|&i| m.ops[i].name.as_str()).collect();
    assert_eq!(names, vec!["arith.negf", "return"]);
    assert!(m.ops[f.ops[0]].depth >= 2);
}

#[test]
fn module_only_snapshot_yields_module_scope() {
    let m = parse_module("%0 = arith.constant 1 : i32\n%1 = arith.addi %0, %0 : i32\n");
    assert_eq!(m.functions.len(), 1);
    assert_eq!(m.functions[0].name, "(module)");
    assert_eq!(m.functions[0].ops.len(), 2);
}

#[test]
fn two_functions_are_separate_scopes() {
    let text = "\
llvm.func @a() { llvm.return }
llvm.func @b() { llvm.return }
";
    let m = parse_module(text);
    let names: Vec<_> = m.functions.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b"]);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p engine --test parser`
Expected: FAIL (functions empty, depth 0).

- [ ] **Step 3: Implement region tracking + scope grouping**

Replace `parse_module` in `crates/engine/src/parser.rs` with a version that tracks brace depth per statement and records scopes. Add these helpers above `parse_module`:

```rust
/// Net `{` minus `}` in a statement, ignoring braces inside an attribute dict
/// or string. Positive ⇒ opens region(s); negative ⇒ closes.
fn brace_delta(s: &str) -> i32 {
    let bytes = s.as_bytes();
    let mut delta = 0i32;
    let mut in_str = false;
    for &b in bytes {
        match b {
            b'"' => in_str = !in_str,
            b'{' if !in_str => delta += 1,
            b'}' if !in_str => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// The `@symbol` of a function-like op, without the `@`.
fn symbol(s: &str) -> Option<String> {
    let p = s.find('@')?;
    let name: String = s[p + 1..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '$' | '-'))
        .collect();
    (!name.is_empty()).then_some(name)
}
```

Now rewrite the `parse_module` body. Keep the per-op field extraction from Task 2, but wrap it in region/scope bookkeeping:

```rust
pub fn parse_module(text: &str) -> ParsedModule {
    use crate::model::FunctionScope;

    let mut ops: Vec<ParsedOp> = Vec::new();
    let mut functions: Vec<FunctionScope> = Vec::new();

    // Region stack: index-in-parent counters for region_path.
    let mut region_path: Vec<usize> = Vec::new();
    let mut sibling_counter: Vec<usize> = vec![0]; // per depth: next child index
    // Active function scope: (functions index, depth at which it was opened).
    let mut active_scope: Option<(usize, usize)> = None;

    for st in assemble_statements(text) {
        let trimmed = st.text.trim();
        let delta = brace_delta(trimmed);
        let opens_region = delta > 0;

        // A closing-only statement pops regions and may close the active scope.
        if trimmed == "}" || (delta < 0 && trimmed.starts_with('}')) {
            for _ in 0..(-delta) {
                region_path.pop();
                sibling_counter.pop();
                if let Some((fi, d)) = active_scope {
                    if region_path.len() < d {
                        functions[fi].line_end = st.line_end;
                        active_scope = None;
                    }
                }
            }
            continue;
        }
        if trimmed == "{" || trimmed.starts_with('^') {
            if opens_region {
                let child = sibling_counter.last_mut().map(|c| { let v = *c; *c += 1; v }).unwrap_or(0);
                region_path.push(child);
                sibling_counter.push(0);
            }
            continue;
        }

        // A real op. Record it at the current depth first.
        let depth = region_path.len();
        let (results, rest) = split_results(trimmed);
        let name = op_name(rest);
        let operands: Vec<String> =
            ssa_names(rest).into_iter().filter(|n| !results.contains(n)).collect();
        let idx: OpIdx = ops.len();
        ops.push(ParsedOp {
            idx,
            name,
            results,
            operands,
            result_types: result_types(trimmed),
            attr_summary: attr_summary(trimmed),
            location: location(trimmed),
            region_path: region_path.clone(),
            depth,
            line_start: st.line_start,
            line_end: st.line_end,
            opaque: false,
        });

        // Function-like? It has a symbol and opens a region for its body.
        let is_func_like = opens_region && symbol(trimmed).is_some();
        if is_func_like && active_scope.is_none() {
            let fi = functions.len();
            functions.push(FunctionScope {
                name: symbol(trimmed).unwrap(),
                ops: Vec::new(),
                line_start: st.line_start,
                line_end: st.line_end,
            });
            // Body ops live one region deeper than the func statement.
            active_scope = Some((fi, region_path.len() + 1));
        } else if let Some((fi, _)) = active_scope {
            functions[fi].ops.push(idx);
            functions[fi].line_end = functions[fi].line_end.max(st.line_end);
        }

        // If this op opened region(s), push them now (after recording the op).
        if opens_region {
            for _ in 0..delta {
                let child =
                    sibling_counter.last_mut().map(|c| { let v = *c; *c += 1; v }).unwrap_or(0);
                region_path.push(child);
                sibling_counter.push(0);
            }
        }
    }

    // No function-like op anywhere ⇒ the whole module is one scope.
    if functions.is_empty() && !ops.is_empty() {
        functions.push(FunctionScope {
            name: "(module)".to_string(),
            ops: (0..ops.len()).collect(),
            line_start: ops.first().map(|o| o.line_start).unwrap_or(1),
            line_end: ops.last().map(|o| o.line_end).unwrap_or(1),
        });
    }

    ParsedModule { ops, functions }
}
```

> Note: single-line function-like ops such as `llvm.func @a() { llvm.return }` have `delta == 0` (the `{` and `}` balance on one statement). Handle them: when `symbol(trimmed).is_some()` and the statement contains a balanced `{...}` body, create a scope and attach the inner ops. For the M3 fixtures this only occurs in the two-function test; extract body ops by re-parsing the substring between the outermost `{` and `}`. Implement this branch:

```rust
        // Single-line function bodies: `@sym(...) { <ops> }` balanced on one line.
        if !opens_region && symbol(trimmed).is_some() {
            if let (Some(o), Some(c)) = (trimmed.find('{'), trimmed.rfind('}')) {
                if o < c {
                    let inner = parse_module(&trimmed[o + 1..c]);
                    let base = ops.len();
                    let mut inner_ops = Vec::new();
                    for mut op in inner.ops {
                        op.idx = base + inner_ops.len();
                        op.line_start = st.line_start;
                        op.line_end = st.line_end;
                        inner_ops.push(op.idx);
                        ops.push(op);
                    }
                    functions.push(FunctionScope {
                        name: symbol(trimmed).unwrap(),
                        ops: inner_ops,
                        line_start: st.line_start,
                        line_end: st.line_end,
                    });
                }
            }
        }
```

Place this branch right after pushing the op and before the `is_func_like` multi-line handling; guard the multi-line branch with `&& opens_region` so the two do not both fire.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine --test parser`
Expected: PASS (all scope tests plus Task 2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/parser.rs crates/engine/tests/parser.rs
git commit -m "feat(engine): recover region nesting and function scopes"
```

---

## Task 4: Parser — error recovery & fixture golden coverage

**Files:**
- Modify: `crates/engine/src/parser.rs`
- Modify: `crates/engine/tests/parser.rs`

**Interfaces:**
- Consumes: parser (Tasks 2–3), `trace_format::fixture::write_demo_trace` (dev-dep).
- Produces: `opaque = true` ops for unparseable lines; a guarantee that every fixture snapshot parses into ≥1 function with all lines accounted for.

- [ ] **Step 1: Add failing tests**

```rust
use engine::parse_module;
use trace_format::fixture::write_demo_trace;
use trace_format::TraceReader;

#[test]
fn malformed_line_becomes_opaque_op_and_parsing_continues() {
    // `@@@` is not valid IR; the parser must not abort and must keep the next op.
    let m = parse_module("@@@ garbage !!!\n%0 = arith.constant 1 : i32\n");
    assert!(m.ops.iter().any(|o| o.opaque), "expected an opaque op");
    assert!(m.ops.iter().any(|o| o.name == "arith.constant"));
}

#[test]
fn every_demo_snapshot_parses_into_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("demo.mlirtrace");
    write_demo_trace(&path).unwrap();
    let reader = TraceReader::open(&path).unwrap();

    // Walk every distinct blob referenced by the pipeline root.
    let roots = reader.passes().unwrap();
    let root = &roots[0];
    for pass in &root.children {
        for blob in [pass.ir_before, pass.ir_after].into_iter().flatten() {
            let text = reader.blob_text(blob).unwrap();
            let m = parse_module(&text);
            assert!(!m.functions.is_empty(), "snapshot produced no scope:\n{text}");
            // The forward function is found in every stage.
            assert!(
                m.functions.iter().any(|f| f.name == "forward"),
                "missing @forward scope:\n{text}"
            );
        }
    }
}
```

Add `tempfile = "3"` to `crates/engine/Cargo.toml` `[dev-dependencies]`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p engine --test parser`
Expected: `malformed_line_...` FAILS (no op is flagged opaque yet).

- [ ] **Step 3: Add opaque-op recovery**

In `parse_module`, when a statement is neither structural nor a recognizable op (no op name token, or the first token contains illegal characters), still emit a `ParsedOp` with `opaque = true`, `name` = first whitespace token, empty operands/results. Concretely, after computing `name`, detect an unusable name and mark opaque:

```rust
        let opaque = name.is_empty()
            || !name.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false);
        let name = if opaque {
            trimmed.split_whitespace().next().unwrap_or("<opaque>").to_string()
        } else {
            name
        };
```

Then include `opaque` in the pushed `ParsedOp` (replace the hardcoded `opaque: false`), and when `opaque`, force `results`/`operands`/`result_types`/`attr_summary` empty:

```rust
        let (results, operands, result_types, attr_summary) = if opaque {
            (Vec::new(), Vec::new(), Vec::new(), String::new())
        } else {
            (results, operands, result_types(trimmed), attr_summary(trimmed))
        };
```

(Adjust the `ParsedOp { ... }` literal to consume these bindings.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine --test parser`
Expected: PASS (all parser tests, including the demo golden).

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/parser.rs crates/engine/tests/parser.rs crates/engine/Cargo.toml
git commit -m "feat(engine): opaque-op error recovery and fixture golden coverage"
```

---

## Task 5: Diff — fingerprint matcher

**Files:**
- Modify: `crates/engine/src/diff.rs`
- Create: `crates/engine/tests/diff.rs`

**Interfaces:**
- Consumes: `ParsedModule`, `OpFingerprint::of` (Task 1).
- Produces: `GreedyFingerprintMatcher::match_ops` returns a stable pairing over two op-index lists: exact-fingerprint matches first (in before-order), then best-score matches above threshold, leaving the rest unmatched. Later tasks rely on this ordering.

Scoring (0..=100): op name equal = +50 (required — never match across different op names); result-type list equal = +25; operand-count equal = +15; location equal = +10. Threshold to accept a non-exact match: **50** (name must match). Greedy: for each unmatched before-op in order, pick the highest-scoring unmatched after-op; ties break by lowest after-index.

- [ ] **Step 1: Write failing tests** in `crates/engine/tests/diff.rs`

```rust
use engine::{parse_module, GreedyFingerprintMatcher, OpMatcher};

fn all_ops(m: &engine::ParsedModule) -> Vec<usize> {
    (0..m.ops.len()).collect()
}

#[test]
fn identical_functions_match_all_ops_positionally() {
    let text = "%0 = arith.constant 1 : i32\n%1 = arith.addi %0, %0 : i32\n";
    let a = parse_module(text);
    let b = parse_module(text);
    let pairs =
        GreedyFingerprintMatcher.match_ops(&a, &all_ops(&a), &b, &all_ops(&b));
    assert_eq!(pairs.len(), 2);
    assert!(pairs.iter().all(|(x, y)| x.is_some() && y.is_some()));
}

#[test]
fn removed_op_is_left_unmatched_on_before_side() {
    let before = parse_module("%0 = arith.constant 1 : i32\n%1 = arith.addi %0, %0 : i32\n");
    let after = parse_module("%0 = arith.constant 1 : i32\n");
    let pairs =
        GreedyFingerprintMatcher.match_ops(&before, &all_ops(&before), &after, &all_ops(&after));
    // The addi has no counterpart: exactly one pair with after == None.
    assert_eq!(pairs.iter().filter(|(_, y)| y.is_none()).count(), 1);
    assert_eq!(pairs.iter().filter(|(x, _)| x.is_none()).count(), 0);
}

#[test]
fn never_matches_across_different_op_names() {
    let before = parse_module("%0 = arith.addf %1, %2 : f32\n");
    let after = parse_module("%0 = arith.mulf %1, %2 : f32\n");
    let pairs =
        GreedyFingerprintMatcher.match_ops(&before, &all_ops(&before), &after, &all_ops(&after));
    assert!(pairs.iter().any(|(x, y)| x.is_some() && y.is_none()));
    assert!(pairs.iter().any(|(x, y)| x.is_none() && y.is_some()));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p engine --test diff`
Expected: FAIL (stub returns empty).

- [ ] **Step 3: Implement the matcher** in `crates/engine/src/diff.rs`

Replace the `GreedyFingerprintMatcher` impl:

```rust
use crate::model::OpFingerprint;

fn score(a: &OpFingerprint, b: &OpFingerprint) -> i32 {
    if a.op_name != b.op_name {
        return 0; // op name mismatch is disqualifying
    }
    let mut s = 50;
    if a.result_types == b.result_types {
        s += 25;
    }
    if a.operand_count == b.operand_count {
        s += 15;
    }
    if a.location.is_some() && a.location == b.location {
        s += 10;
    }
    s
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
        let bfp: Vec<OpFingerprint> =
            before_ops.iter().map(|&i| OpFingerprint::of(&before.ops[i])).collect();
        let afp: Vec<OpFingerprint> =
            after_ops.iter().map(|&i| OpFingerprint::of(&after.ops[i])).collect();

        let mut after_taken = vec![false; after_ops.len()];
        let mut pairs: Vec<(Option<OpIdx>, Option<OpIdx>)> = Vec::new();

        // Pass 1: exact fingerprint matches, in before order.
        for (bi, fp) in bfp.iter().enumerate() {
            if let Some(aj) = (0..after_ops.len())
                .find(|&j| !after_taken[j] && &afp[j] == fp)
            {
                after_taken[aj] = true;
                pairs.push((Some(before_ops[bi]), Some(after_ops[aj])));
            } else {
                pairs.push((Some(before_ops[bi]), None)); // provisional; may upgrade in pass 2
            }
        }

        // Pass 2: best-score above threshold for still-unmatched before-ops.
        for pair in pairs.iter_mut() {
            let (Some(bi_idx), None) = *pair else { continue };
            let bi = before_ops.iter().position(|&x| x == bi_idx).unwrap();
            let mut best: Option<(usize, i32)> = None;
            for j in 0..after_ops.len() {
                if after_taken[j] {
                    continue;
                }
                let sc = score(&bfp[bi], &afp[j]);
                if sc >= MATCH_THRESHOLD && best.map(|(_, bs)| sc > bs).unwrap_or(true) {
                    best = Some((j, sc));
                }
            }
            if let Some((j, _)) = best {
                after_taken[j] = true;
                pair.1 = Some(after_ops[j]);
            }
        }

        // Remaining after-ops are additions, appended in after order.
        for (j, taken) in after_taken.iter().enumerate() {
            if !taken {
                pairs.push((None, Some(after_ops[j])));
            }
        }
        pairs
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine --test diff`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/diff.rs crates/engine/tests/diff.rs
git commit -m "feat(engine): greedy fingerprint OpMatcher"
```

---

## Task 6: Diff — classification & line ranges

**Files:**
- Modify: `crates/engine/src/diff.rs`
- Modify: `crates/engine/tests/diff.rs`

**Interfaces:**
- Consumes: `OpMatcher` (Task 5).
- Produces: `diff_function(before, after, func, matcher) -> FunctionDiff` with per-op `ChangeClass`, `detail` for modified ops, and `before_lines`/`after_lines` for text projection. `changes` are ordered by after-op position, then removed ops interleaved at their matched neighborhood (removed ops sorted by before position, appended after their nearest preceding matched op; if none, at front).

- [ ] **Step 1: Add failing tests**

```rust
use engine::{diff_function, ChangeClass, GreedyFingerprintMatcher, parse_module};

#[test]
fn classifies_added_removed_modified_unchanged() {
    let before = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.addi %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let after = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.muli %0, %0 : i32\n  %2 = arith.subi %1, %0 : i32\n  return %1 : i32\n}\n",
    );
    let d = diff_function(&before, &after, "f", &GreedyFingerprintMatcher);
    let classes: Vec<_> = d.changes.iter().map(|c| c.class).collect();
    assert!(classes.contains(&ChangeClass::Unchanged)); // constant, return
    assert!(classes.contains(&ChangeClass::Removed)); // addi
    assert!(classes.contains(&ChangeClass::Added)); // muli, subi
}

#[test]
fn modified_op_reports_detail_and_both_line_ranges() {
    // Same op name + operand count, changed result type ⇒ modified.
    let before = parse_module("func.func @f() {\n  %0 = arith.constant 1 : i32\n}\n");
    let after = parse_module("func.func @f() {\n  %0 = arith.constant 1 : i64\n}\n");
    let d = diff_function(&before, &after, "f", &GreedyFingerprintMatcher);
    let m = d.changes.iter().find(|c| c.class == ChangeClass::Modified).unwrap();
    assert!(m.before_lines.is_some() && m.after_lines.is_some());
    assert!(m.detail.iter().any(|s| s.contains("type")));
}

#[test]
fn unknown_function_yields_empty_diff() {
    let m = parse_module("func.func @f() {\n  return\n}\n");
    let d = diff_function(&m, &m, "nope", &GreedyFingerprintMatcher);
    assert!(d.changes.is_empty());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p engine --test diff`
Expected: FAIL (stub returns empty for all).

- [ ] **Step 3: Implement `diff_function`**

Replace the `diff_function` stub in `crates/engine/src/diff.rs`:

```rust
use crate::model::ParsedOp;

fn detail(before: &ParsedOp, after: &ParsedOp) -> Vec<String> {
    let mut out = Vec::new();
    if before.result_types != after.result_types {
        out.push(format!(
            "result type {:?} → {:?}",
            before.result_types, after.result_types
        ));
    }
    if before.operands.len() != after.operands.len() {
        out.push(format!(
            "operand count {} → {}",
            before.operands.len(),
            after.operands.len()
        ));
    }
    if before.attr_summary != after.attr_summary {
        out.push(format!(
            "attributes {:?} → {:?}",
            before.attr_summary, after.attr_summary
        ));
    }
    out
}

pub fn diff_function(
    before: &ParsedModule,
    after: &ParsedModule,
    func: &str,
    matcher: &dyn OpMatcher,
) -> FunctionDiff {
    let empty = FunctionDiff { func: func.to_string(), changes: Vec::new() };
    let (Some(bs), Some(af)) = (before.scope(func), after.scope(func)) else {
        return empty;
    };
    let pairs = matcher.match_ops(before, &bs.ops, after, &af.ops);

    // Index pairs by after-op so we can emit in after order; collect removals.
    let mut by_after: std::collections::HashMap<OpIdx, OpChange> = std::collections::HashMap::new();
    let mut removed: Vec<OpChange> = Vec::new();
    for (b, a) in pairs {
        match (b, a) {
            (Some(bi), Some(ai)) => {
                let bo = &before.ops[bi];
                let ao = &after.ops[ai];
                let det = detail(bo, ao);
                let class = if det.is_empty() { ChangeClass::Unchanged } else { ChangeClass::Modified };
                by_after.insert(
                    ai,
                    OpChange {
                        class,
                        before: Some(bi),
                        after: Some(ai),
                        before_lines: Some((bo.line_start, bo.line_end)),
                        after_lines: Some((ao.line_start, ao.line_end)),
                        detail: det,
                    },
                );
            }
            (None, Some(ai)) => {
                let ao = &after.ops[ai];
                by_after.insert(
                    ai,
                    OpChange {
                        class: ChangeClass::Added,
                        before: None,
                        after: Some(ai),
                        before_lines: None,
                        after_lines: Some((ao.line_start, ao.line_end)),
                        detail: Vec::new(),
                    },
                );
            }
            (Some(bi), None) => {
                let bo = &before.ops[bi];
                removed.push(OpChange {
                    class: ChangeClass::Removed,
                    before: Some(bi),
                    after: None,
                    before_lines: Some((bo.line_start, bo.line_end)),
                    after_lines: None,
                    detail: Vec::new(),
                });
            }
            (None, None) => {}
        }
    }

    // Emit in after order; splice removals after their nearest preceding
    // matched before-op (by before index), else at the front.
    let mut changes: Vec<OpChange> = Vec::new();
    // Precompute, for each removed op, the after-slot it should follow.
    removed.sort_by_key(|c| c.before.unwrap());
    let mut removed_iter = removed.into_iter().peekable();

    let mut after_order: Vec<OpIdx> = af.ops.clone();
    after_order.sort();
    for ai in after_order {
        if let Some(change) = by_after.remove(&ai) {
            // Flush removals whose before index precedes this op's before index.
            if let Some(this_before) = change.before {
                while removed_iter.peek().map(|r| r.before.unwrap() < this_before).unwrap_or(false) {
                    changes.push(removed_iter.next().unwrap());
                }
            }
            changes.push(change);
        }
    }
    // Trailing removals.
    for r in removed_iter {
        changes.push(r);
    }
    FunctionDiff { func: func.to_string(), changes }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine --test diff`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/diff.rs crates/engine/tests/diff.rs
git commit -m "feat(engine): structural diff classification with line ranges"
```

---

## Task 7: Graph — dataflow extraction

**Files:**
- Modify: `crates/engine/src/graph.rs`
- Create: `crates/engine/tests/graph.rs`

**Interfaces:**
- Consumes: `ParsedModule`, `FunctionScope` (Task 1).
- Produces: `extract_dataflow(module, func, budget) -> DataflowGraph`. Node id = `op{idx}`. One edge per (result def op → operand use op) where both ops are in the scope. `cluster` = the op's `region_path`. Under budget here (collapse in Task 8).

- [ ] **Step 1: Write failing tests** in `crates/engine/tests/graph.rs`

```rust
use engine::{extract_dataflow, parse_module};

#[test]
fn builds_def_use_edges_within_function() {
    let m = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.addi %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let g = extract_dataflow(&m, "f", 2000);
    // Three ops: constant, addi, return.
    assert_eq!(g.nodes.len(), 3);
    // constant → addi (uses %0), addi → return (uses %1).
    let has = |from_op: &str, to_op: &str| {
        g.edges.iter().any(|e| {
            let f = &g.nodes.iter().find(|n| n.id == e.from).unwrap().op_name;
            let t = &g.nodes.iter().find(|n| n.id == e.to).unwrap().op_name;
            f == from_op && t == to_op
        })
    };
    assert!(has("arith.constant", "arith.addi"));
    assert!(has("arith.addi", "return"));
    assert!(!g.truncated);
}

#[test]
fn node_labels_carry_op_and_result_type() {
    let m = parse_module("func.func @f() {\n  %0 = arith.constant 1 : i32\n}\n");
    let g = extract_dataflow(&m, "f", 2000);
    let n = &g.nodes[0];
    assert_eq!(n.op_name, "arith.constant");
    assert!(n.label.contains("arith.constant"));
    assert_eq!(n.line_range, (2, 2));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p engine --test graph`
Expected: FAIL (stub empty).

- [ ] **Step 3: Implement `extract_dataflow`**

Replace the stub in `crates/engine/src/graph.rs`. Add a private helper that both public functions share:

```rust
use std::collections::HashMap;

use crate::model::{OpIdx, ParsedOp};

fn node_id(idx: OpIdx) -> String {
    format!("op{idx}")
}

fn label(op: &ParsedOp) -> String {
    if let Some(ty) = op.result_types.first() {
        format!("{} : {}", op.name, ty)
    } else {
        op.name.clone()
    }
}

fn node_of(op: &ParsedOp, change: Option<ChangeClass>) -> GraphNode {
    GraphNode {
        id: node_id(op.idx),
        label: label(op),
        op_name: op.name.clone(),
        line_range: (op.line_start, op.line_end),
        cluster: op.region_path.clone(),
        change,
        collapsed_count: 0,
    }
}

/// Build def→use edges among `ops` (a set of op indices in `module`).
fn dataflow_edges(module: &ParsedModule, ops: &[OpIdx]) -> Vec<GraphEdge> {
    // Map each SSA result name to its defining op index (last writer wins).
    let mut def: HashMap<&str, OpIdx> = HashMap::new();
    for &i in ops {
        for r in &module.ops[i].results {
            def.insert(r.as_str(), i);
        }
    }
    let in_scope: std::collections::HashSet<OpIdx> = ops.iter().copied().collect();
    let mut edges = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &use_op in ops {
        for operand in &module.ops[use_op].operands {
            if let Some(&def_op) = def.get(operand.as_str()) {
                if def_op != use_op && in_scope.contains(&def_op) {
                    let key = (def_op, use_op);
                    if seen.insert(key) {
                        edges.push(GraphEdge {
                            from: node_id(def_op),
                            to: node_id(use_op),
                            removed: false,
                        });
                    }
                }
            }
        }
    }
    edges
}

pub fn extract_dataflow(module: &ParsedModule, func: &str, budget: usize) -> DataflowGraph {
    let Some(scope) = module.scope(func) else {
        return DataflowGraph { nodes: Vec::new(), edges: Vec::new(), clusters: Vec::new(), truncated: false };
    };
    let nodes: Vec<GraphNode> = scope.ops.iter().map(|&i| node_of(&module.ops[i], None)).collect();
    let edges = dataflow_edges(module, &scope.ops);
    let graph = DataflowGraph { nodes, edges, clusters: Vec::new(), truncated: false };
    collapse_to_budget(graph, budget)
}
```

Add a pass-through `collapse_to_budget` for now (Task 8 fills it in):

```rust
fn collapse_to_budget(graph: DataflowGraph, _budget: usize) -> DataflowGraph {
    graph
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine --test graph`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/graph.rs crates/engine/tests/graph.rs
git commit -m "feat(engine): SSA def-use dataflow graph extraction"
```

---

## Task 8: Graph — cluster collapse & budget

**Files:**
- Modify: `crates/engine/src/graph.rs`
- Modify: `crates/engine/tests/graph.rs`

**Interfaces:**
- Consumes: `extract_dataflow` (Task 7).
- Produces: `collapse_to_budget` collapses deepest region clusters into meta-nodes (id `cluster{path}`, `collapsed_count` = ops hidden) until `nodes.len() <= budget`, deterministically (collapse clusters in descending depth, then ascending path order). Sets `truncated = true` if still over budget after all clusters collapse; then keeps the first `budget` nodes by index order. Edges are rewritten to meta-node endpoints; self-edges dropped. `clusters` lists surviving collapsed clusters.

- [ ] **Step 1: Add failing tests**

```rust
#[test]
fn collapses_clusters_deterministically_under_budget() {
    // A function with two nested regions; budget forces collapse.
    let text = "\
func.func @f() {
  %0 = arith.constant 0 : i32
  scf.for %i = %0 to %0 step %0 {
    %1 = arith.addi %0, %0 : i32
    %2 = arith.muli %1, %0 : i32
  }
  return
}
";
    let m = parse_module(text);
    let full = extract_dataflow(&m, "f", 2000);
    let budgeted = extract_dataflow(&m, "f", 3);
    assert!(budgeted.nodes.len() <= 3);
    assert!(budgeted.nodes.iter().any(|n| n.collapsed_count > 0), "expected a meta-node");
    // Determinism: same input, same output.
    let again = extract_dataflow(&m, "f", 3);
    assert_eq!(budgeted, again);
    assert!(full.nodes.len() > budgeted.nodes.len());
}

#[test]
fn truncates_when_budget_below_cluster_count() {
    let text = "func.func @f() {\n  %0 = arith.constant 0 : i32\n  %1 = arith.addi %0, %0 : i32\n  %2 = arith.muli %1, %0 : i32\n}\n";
    let m = parse_module(text);
    // All ops at same depth (no sub-regions to collapse) ⇒ truncation path.
    let g = extract_dataflow(&m, "f", 1);
    assert!(g.truncated);
    assert!(g.nodes.len() <= 1);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p engine --test graph`
Expected: FAIL (pass-through returns all nodes).

- [ ] **Step 3: Implement `collapse_to_budget`**

Replace the pass-through:

```rust
fn collapse_to_budget(mut graph: DataflowGraph, budget: usize) -> DataflowGraph {
    if graph.nodes.len() <= budget || budget == 0 {
        return maybe_truncate(graph, budget);
    }

    // Candidate clusters = distinct non-empty region paths among real nodes,
    // ordered deepest-first then by path, for deterministic collapse.
    let mut cluster_paths: Vec<Vec<usize>> = graph
        .nodes
        .iter()
        .filter(|n| n.collapsed_count == 0 && !n.cluster.is_empty())
        .map(|n| n.cluster.clone())
        .collect();
    cluster_paths.sort();
    cluster_paths.dedup();
    cluster_paths.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));

    for path in cluster_paths {
        if graph.nodes.len() <= budget {
            break;
        }
        graph = collapse_cluster(graph, &path);
    }
    maybe_truncate(graph, budget)
}

fn collapse_cluster(graph: DataflowGraph, path: &[usize]) -> DataflowGraph {
    let in_cluster = |n: &GraphNode| n.collapsed_count == 0 && n.cluster.starts_with(path) && n.cluster.len() >= path.len() && !n.cluster.is_empty() && n.cluster.starts_with(path);
    let hidden: Vec<String> = graph
        .nodes
        .iter()
        .filter(|n| in_cluster(n))
        .map(|n| n.id.clone())
        .collect();
    if hidden.is_empty() {
        return graph;
    }
    let meta_id = format!("cluster{}", path.iter().map(|p| p.to_string()).collect::<Vec<_>>().join("_"));
    let hidden_set: std::collections::HashSet<String> = hidden.iter().cloned().collect();

    let mut nodes: Vec<GraphNode> = graph.nodes.into_iter().filter(|n| !hidden_set.contains(&n.id)).collect();
    nodes.push(GraphNode {
        id: meta_id.clone(),
        label: format!("{} ops", hidden.len()),
        op_name: "(cluster)".to_string(),
        line_range: (0, 0),
        cluster: path.to_vec(),
        change: None,
        collapsed_count: hidden.len(),
    });

    let remap = |id: &str| -> String {
        if hidden_set.contains(id) { meta_id.clone() } else { id.to_string() }
    };
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for e in graph.edges {
        let from = remap(&e.from);
        let to = remap(&e.to);
        if from == to {
            continue; // dropped self-edge inside collapsed cluster
        }
        if seen.insert((from.clone(), to.clone(), e.removed)) {
            edges.push(GraphEdge { from, to, removed: e.removed });
        }
    }

    let mut clusters = graph.clusters;
    clusters.push(GraphCluster { path: path.to_vec(), label: format!("region {:?}", path) });

    DataflowGraph { nodes, edges, clusters, truncated: graph.truncated }
}

fn maybe_truncate(mut graph: DataflowGraph, budget: usize) -> DataflowGraph {
    if budget > 0 && graph.nodes.len() > budget {
        graph.truncated = true;
        graph.nodes.truncate(budget);
        let kept: std::collections::HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
        graph.edges.retain(|e| kept.contains(&e.from) && kept.contains(&e.to));
    }
    graph
}
```

> Simplify the `in_cluster` closure (the repetition above is a typo guard) to:
> ```rust
> let in_cluster = |n: &GraphNode| n.collapsed_count == 0 && !n.cluster.is_empty() && n.cluster.starts_with(path);
> ```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine --test graph`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/graph.rs crates/engine/tests/graph.rs
git commit -m "feat(engine): deterministic cluster collapse and node budget"
```

---

## Task 9: Graph — unified diff graph with ghosts

**Files:**
- Modify: `crates/engine/src/graph.rs`
- Modify: `crates/engine/tests/graph.rs`

**Interfaces:**
- Consumes: `diff_function` matching via `OpMatcher` (Task 5), `extract_dataflow` internals (Tasks 7–8).
- Produces: `extract_dataflow_diff(before, after, func, budget, matcher)`. Base = after-graph; nodes tagged `Unchanged`/`Added`/`Modified` from the diff. Removed before-ops added as ghost nodes (`change = Removed`, id `ghost{before_idx}`), attached to matched neighbors' after-node ids; removed edges tagged `removed = true` (dashed). Then budget-collapsed.

- [ ] **Step 1: Add failing test**

```rust
use engine::{extract_dataflow_diff, GreedyFingerprintMatcher};

#[test]
fn diff_graph_tags_added_removed_and_marks_ghost() {
    let before = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.addi %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let after = parse_module(
        "func.func @f() {\n  %0 = arith.constant 1 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n",
    );
    let g = extract_dataflow_diff(&before, &after, "f", 2000, &GreedyFingerprintMatcher);
    let classes: Vec<_> = g.nodes.iter().filter_map(|n| n.change).collect();
    assert!(classes.contains(&engine::ChangeClass::Added)); // muli
    assert!(classes.contains(&engine::ChangeClass::Removed)); // addi ghost
    assert!(g.nodes.iter().any(|n| n.id.starts_with("ghost")));
    // At least one removed (dashed) edge from the ghost.
    assert!(g.edges.iter().any(|e| e.removed));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p engine --test graph`
Expected: FAIL (stub empty).

- [ ] **Step 3: Implement `extract_dataflow_diff`**

Replace the stub. Reuse `diff_function` to obtain matches, then build the after-graph, retag, and splice ghosts:

```rust
use crate::diff::diff_function;

pub fn extract_dataflow_diff(
    before: &ParsedModule,
    after: &ParsedModule,
    func: &str,
    budget: usize,
    matcher: &dyn OpMatcher,
) -> DataflowGraph {
    let Some(after_scope) = after.scope(func) else {
        // No after side: fall back to the before graph as-is.
        return extract_dataflow(before, func, budget);
    };
    let diff = diff_function(before, after, func, matcher);

    // change class per after-op index and per removed before-op index.
    let mut after_class: HashMap<OpIdx, ChangeClass> = HashMap::new();
    let mut removed_before: Vec<OpIdx> = Vec::new();
    // Map a before-op to the after-op it matched (for ghost edge attachment).
    let mut before_to_after: HashMap<OpIdx, OpIdx> = HashMap::new();
    for c in &diff.changes {
        match (c.before, c.after, c.class) {
            (_, Some(ai), class) => {
                after_class.insert(ai, class);
                if let Some(bi) = c.before {
                    before_to_after.insert(bi, ai);
                }
            }
            (Some(bi), None, ChangeClass::Removed) => removed_before.push(bi),
            _ => {}
        }
    }

    // Base: after nodes tagged by class.
    let mut nodes: Vec<GraphNode> = after_scope
        .ops
        .iter()
        .map(|&i| {
            let class = after_class.get(&i).copied().unwrap_or(ChangeClass::Unchanged);
            node_of(&after.ops[i], Some(class))
        })
        .collect();
    let mut edges = dataflow_edges(after, &after_scope.ops);

    // Ghost nodes for removed before-ops.
    let ghost_id = |bi: OpIdx| format!("ghost{bi}");
    // Map before result name → the after node id its user should attach to.
    // For each removed before-op, connect to after-nodes of its matched
    // operand-defs and matched users, as dashed edges.
    for &bi in &removed_before {
        let bop = &before.ops[bi];
        nodes.push(GraphNode {
            id: ghost_id(bi),
            label: label(bop),
            op_name: bop.name.clone(),
            line_range: (bop.line_start, bop.line_end),
            cluster: bop.region_path.clone(),
            change: Some(ChangeClass::Removed),
            collapsed_count: 0,
        });
    }

    // Ghost edges: for each removed op, link to matched neighbor after-nodes.
    // Build before def map to find who defines the ghost's operands.
    let mut before_def: HashMap<&str, OpIdx> = HashMap::new();
    for &i in &before_scope_ops(before, func) {
        for r in &before.ops[i].results {
            before_def.insert(r.as_str(), i);
        }
    }
    let endpoint = |bi: OpIdx| -> String {
        before_to_after.get(&bi).map(|&ai| node_id(ai)).unwrap_or_else(|| ghost_id(bi))
    };
    for &bi in &removed_before {
        // operand-def → ghost
        for operand in &before.ops[bi].operands {
            if let Some(&def_bi) = before_def.get(operand.as_str()) {
                if def_bi != bi {
                    edges.push(GraphEdge { from: endpoint(def_bi), to: ghost_id(bi), removed: true });
                }
            }
        }
        // ghost → users
        for &user in &before_scope_ops(before, func) {
            if user == bi { continue; }
            let uses_ghost = before.ops[user]
                .operands
                .iter()
                .any(|o| before.ops[bi].results.iter().any(|r| r == o));
            if uses_ghost {
                edges.push(GraphEdge { from: ghost_id(bi), to: endpoint(user), removed: true });
            }
        }
    }

    let graph = DataflowGraph { nodes, edges, clusters: Vec::new(), truncated: false };
    collapse_to_budget(graph, budget)
}

fn before_scope_ops(module: &ParsedModule, func: &str) -> Vec<OpIdx> {
    module.scope(func).map(|s| s.ops.clone()).unwrap_or_default()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p engine`
Expected: PASS (all engine tests).

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/graph.rs crates/engine/tests/graph.rs
git commit -m "feat(engine): unified diff graph with removed-op ghosts"
```

---

## Task 10: Server — engine dep, MessagePack response, caches

**Files:**
- Modify: `crates/server/Cargo.toml`
- Modify: `Cargo.toml` (add `rmp-serde` to workspace deps)
- Create: `crates/server/src/msgpack.rs`
- Create: `crates/server/src/cache.rs`
- Modify: `crates/server/src/lib.rs`

**Interfaces:**
- Produces:
  - `struct Msgpack<T>(pub T)` implementing `IntoResponse` (Content-Type `application/msgpack`), used by Tasks 12–13.
  - `struct EngineCache` in `cache.rs` with `fn parsed(&self, blob: BlobId, text: &str) -> Arc<ParsedModule>` (parse cache keyed by blob id) and `fn diff(&self, before: BlobId, after: BlobId, func: &str, compute: impl FnOnce() -> FunctionDiff) -> Arc<FunctionDiff>`.
  - `ServerState` gains `cache: Arc<EngineCache>`.

- [ ] **Step 1: Add dependencies**

Workspace `Cargo.toml` `[workspace.dependencies]`:

```toml
rmp-serde = "1"
```

`crates/server/Cargo.toml` `[dependencies]`:

```toml
engine = { path = "../engine" }
rmp-serde = { workspace = true }
```

- [ ] **Step 2: Write a failing msgpack round-trip test**

Add to `crates/server/tests/api.rs` a helper + test (msgpack decode using `rmp-serde` as a dev-dep — add `rmp-serde = { workspace = true }` under `crates/server/Cargo.toml` `[dev-dependencies]`):

```rust
async fn response_msgpack<T: serde::de::DeserializeOwned>(
    app: axum::Router,
    uri: &str,
) -> (axum::http::StatusCode, Option<T>) {
    let response = app
        .oneshot(axum::http::Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = rmp_serde::from_slice::<T>(&bytes).ok();
    (status, value)
}
```

This helper is exercised by Tasks 12–13; add a compile-only reference now via `#[allow(dead_code)]` so this task builds and commits independently.

- [ ] **Step 3: Implement `crates/server/src/msgpack.rs`**

```rust
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A MessagePack response body for bulk payloads (ADR-6).
pub struct Msgpack<T>(pub T);

impl<T: Serialize> IntoResponse for Msgpack<T> {
    fn into_response(self) -> Response {
        match rmp_serde::to_vec_named(&self.0) {
            Ok(bytes) => (
                [(header::CONTENT_TYPE, "application/msgpack")],
                bytes,
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("msgpack encode failed: {e}"),
            )
                .into_response(),
        }
    }
}
```

> `to_vec_named` keeps struct field names as map keys so the JS `@msgpack/msgpack` decoder yields objects with the same field names the UI types expect.

- [ ] **Step 4: Implement `crates/server/src/cache.rs`**

```rust
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use engine::{FunctionDiff, ParsedModule};
use trace_format::BlobId;

/// Process-lifetime memo of parsed modules and per-function diffs.
#[derive(Default)]
pub struct EngineCache {
    parsed: Mutex<HashMap<i64, Arc<ParsedModule>>>,
    diffs: Mutex<HashMap<(i64, i64, String), Arc<FunctionDiff>>>,
}

impl EngineCache {
    pub fn parsed(&self, blob: BlobId, text: &str) -> Arc<ParsedModule> {
        if let Some(hit) = self.parsed.lock().unwrap().get(&blob.0).cloned() {
            return hit;
        }
        let module = Arc::new(engine::parse_module(text));
        self.parsed.lock().unwrap().insert(blob.0, module.clone());
        module
    }

    pub fn diff<F: FnOnce() -> FunctionDiff>(
        &self,
        before: BlobId,
        after: BlobId,
        func: &str,
        compute: F,
    ) -> Arc<FunctionDiff> {
        let key = (before.0, after.0, func.to_string());
        if let Some(hit) = self.diffs.lock().unwrap().get(&key).cloned() {
            return hit;
        }
        let value = Arc::new(compute());
        self.diffs.lock().unwrap().insert(key, value.clone());
        value
    }
}
```

- [ ] **Step 5: Wire cache + modules into `lib.rs`**

Add `mod msgpack;` and `mod cache;` at the top of `crates/server/src/lib.rs`, and extend `ServerState`:

```rust
use crate::cache::EngineCache;

#[derive(Clone)]
struct ServerState {
    trace_path: Arc<PathBuf>,
    cache: Arc<EngineCache>,
}
```

In `router`, construct it:

```rust
    let state = ServerState {
        trace_path: Arc::new(trace_path),
        cache: Arc::new(EngineCache::default()),
    };
```

- [ ] **Step 6: Verify build + existing tests**

Run: `cargo test -p server`
Expected: PASS (existing endpoints unchanged; new modules compile).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/server
git commit -m "feat(server): engine dep, MessagePack response, parse/diff caches"
```

---

## Task 11: Server — `/functions` endpoint

**Files:**
- Modify: `crates/server/src/api.rs`
- Modify: `crates/server/src/lib.rs`
- Modify: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `EngineCache::parsed`, `TraceReader` (Task 10).
- Produces: `GET /api/passes/{id}/functions` → JSON `[{ name, op_count, has_before, has_after }]`. Union of function names across before+after snapshots (sorted, deduped). Control-plane list ⇒ JSON, not msgpack.

- [ ] **Step 1: Write failing test**

```rust
#[tokio::test]
async fn functions_endpoint_lists_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();

    let (status, passes) = response_json(app.clone(), "/api/passes").await;
    assert_eq!(status, 200);
    let pass_id = passes[0]["children"][0]["id"].as_i64().unwrap(); // canonicalize

    let (status, funcs) =
        response_json(app.clone(), &format!("/api/passes/{pass_id}/functions")).await;
    assert_eq!(status, 200);
    let arr = funcs.as_array().unwrap();
    assert!(arr.iter().any(|f| f["name"] == "forward"));
    let fwd = arr.iter().find(|f| f["name"] == "forward").unwrap();
    assert!(fwd["op_count"].as_u64().unwrap() >= 1);
    assert_eq!(fwd["has_before"], true);
    assert_eq!(fwd["has_after"], true);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p server functions_endpoint_lists_scopes`
Expected: FAIL (404 route not found).

- [ ] **Step 3: Implement handler** in `crates/server/src/api.rs`

Add near the other handlers:

```rust
use std::collections::BTreeMap as _BTreeMap; // (already imported as BTreeMap)
use crate::msgpack::Msgpack;

#[derive(Serialize)]
pub(crate) struct FunctionDto {
    name: String,
    op_count: usize,
    has_before: bool,
    has_after: bool,
}

fn parsed_side(
    state: &ServerState,
    reader: &TraceReader,
    blob: Option<BlobId>,
) -> Result<Option<std::sync::Arc<engine::ParsedModule>>, ApiError> {
    match blob {
        None => Ok(None),
        Some(b) => {
            let text = reader.blob_text(b)?;
            Ok(Some(state.cache.parsed(b, &text)))
        }
    }
}

pub(crate) async fn functions(
    State(state): State<ServerState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<FunctionDto>>, ApiError> {
    let reader = open(&state)?;
    let pass = reader.pass(PassId(id))?;
    let before = parsed_side(&state, &reader, pass.ir_before)?;
    let after = parsed_side(&state, &reader, pass.ir_after)?;

    let mut map: std::collections::BTreeMap<String, (usize, bool, bool)> = Default::default();
    if let Some(m) = &before {
        for f in &m.functions {
            let e = map.entry(f.name.clone()).or_insert((0, false, false));
            e.0 = e.0.max(f.ops.len());
            e.1 = true;
        }
    }
    if let Some(m) = &after {
        for f in &m.functions {
            let e = map.entry(f.name.clone()).or_insert((0, false, false));
            e.0 = e.0.max(f.ops.len());
            e.2 = true;
        }
    }
    let out = map
        .into_iter()
        .map(|(name, (op_count, has_before, has_after))| FunctionDto {
            name,
            op_count,
            has_before,
            has_after,
        })
        .collect();
    Ok(Json(out))
}
```

Add `engine` import usage at top if needed (`use engine;` is implicit via path). Register the route in `crates/server/src/lib.rs`:

```rust
        .route("/passes/{id}/functions", get(api::functions))
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p server functions_endpoint_lists_scopes`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/server
git commit -m "feat(server): GET /passes/{id}/functions"
```

---

## Task 12: Server — `/diff` endpoint

**Files:**
- Modify: `crates/server/src/api.rs`
- Modify: `crates/server/src/lib.rs`
- Modify: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `EngineCache` (parsed + diff), `Msgpack`, `diff_function` (engine).
- Produces: `GET /api/passes/{id}/diff?func=<name>` → `Msgpack<FunctionDiff>`. Missing before or after snapshot → **409** with JSON `{ error }`. `ir_changed == false` (before blob == after blob) → a `FunctionDiff` whose changes are all `Unchanged` (cheap path: skip the engine, mark no-changes). Unknown func → 200 with empty `changes`.

- [ ] **Step 1: Write failing tests**

```rust
use engine::{ChangeClass, FunctionDiff};

#[tokio::test]
async fn diff_endpoint_returns_structural_changes() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let canon = passes[0]["children"][0]["id"].as_i64().unwrap(); // canonicalize: real change

    let (status, diff) =
        response_msgpack::<FunctionDiff>(app.clone(), &format!("/api/passes/{canon}/diff?func=forward")).await;
    assert_eq!(status, 200);
    let diff = diff.unwrap();
    assert!(diff.changes.iter().any(|c| c.class == ChangeClass::Removed
        || c.class == ChangeClass::Added
        || c.class == ChangeClass::Modified));
}

#[tokio::test]
async fn diff_endpoint_no_op_pass_is_all_unchanged() {
    // All fixture children have both sides, so the 409 branch has no fixture to
    // hit; assert the no-changes fast path instead: cse is a no-op (before==after).
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let cse = passes[0]["children"][1]["id"].as_i64().unwrap(); // cse: no-op

    let (status, diff) =
        response_msgpack::<FunctionDiff>(app.clone(), &format!("/api/passes/{cse}/diff?func=forward")).await;
    assert_eq!(status, 200);
    assert!(diff.unwrap().changes.iter().all(|c| c.class == ChangeClass::Unchanged));
}
```

> The fixture has no pass missing a side, so the 409 branch is covered by a unit assertion in Step 3's code review and by the UI disabling the toggle. If a missing-side fixture is later added, extend this test. For now assert the no-op path.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p server diff_`
Expected: FAIL (route missing).

- [ ] **Step 3: Implement handler**

```rust
#[derive(Deserialize)]
pub(crate) struct DiffQuery {
    func: String,
}

pub(crate) async fn diff(
    State(state): State<ServerState>,
    Path(id): Path<i64>,
    Query(query): Query<DiffQuery>,
) -> Result<Msgpack<engine::FunctionDiff>, ApiError> {
    let reader = open(&state)?;
    let pass = reader.pass(PassId(id))?;
    let (Some(before_id), Some(after_id)) = (pass.ir_before, pass.ir_after) else {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!("pass {id} is missing a before or after snapshot"),
        });
    };

    // Cheap no-changes path: identical blob ids ⇒ every op unchanged.
    if before_id == after_id {
        let text = reader.blob_text(after_id)?;
        let module = state.cache.parsed(after_id, &text);
        let changes = module
            .scope(&query.func)
            .map(|s| {
                s.ops
                    .iter()
                    .map(|&i| engine::OpChange {
                        class: engine::ChangeClass::Unchanged,
                        before: Some(i),
                        after: Some(i),
                        before_lines: Some((module.ops[i].line_start, module.ops[i].line_end)),
                        after_lines: Some((module.ops[i].line_start, module.ops[i].line_end)),
                        detail: Vec::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Ok(Msgpack(engine::FunctionDiff { func: query.func, changes }));
    }

    let before_text = reader.blob_text(before_id)?;
    let after_text = reader.blob_text(after_id)?;
    let before = state.cache.parsed(before_id, &before_text);
    let after = state.cache.parsed(after_id, &after_text);
    let func = query.func.clone();
    let diff = state.cache.diff(before_id, after_id, &func, || {
        engine::diff_function(&before, &after, &func, &engine::GreedyFingerprintMatcher)
    });
    Ok(Msgpack((*diff).clone()))
}
```

> `ApiError` needs field-literal construction here; its fields are private to `api.rs`, which is fine since this handler lives in `api.rs`. Register the route in `lib.rs`:

```rust
        .route("/passes/{id}/diff", get(api::diff))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p server diff_`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/server
git commit -m "feat(server): GET /passes/{id}/diff with 409 and no-op fast path"
```

---

## Task 13: Server — `/graphs/dataflow` endpoint

**Files:**
- Modify: `crates/server/src/api.rs`
- Modify: `crates/server/src/lib.rs`
- Modify: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `EngineCache::parsed`, `Msgpack`, `extract_dataflow` / `extract_dataflow_diff`.
- Produces: `GET /api/graphs/dataflow?pass={id}&func=<name>&diff=0|1&budget=N` → `Msgpack<DataflowGraph>`. `budget` clamps to `[1, 5000]`, default 2000. `diff=1` with a missing side → 409.

- [ ] **Step 1: Write failing test**

```rust
use engine::DataflowGraph;

#[tokio::test]
async fn graph_endpoint_returns_nodes_and_respects_budget() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let canon = passes[0]["children"][0]["id"].as_i64().unwrap();

    let (status, g) = response_msgpack::<DataflowGraph>(
        app.clone(),
        &format!("/api/graphs/dataflow?pass={canon}&func=forward&diff=0&budget=2000"),
    )
    .await;
    assert_eq!(status, 200);
    let g = g.unwrap();
    assert!(!g.nodes.is_empty());

    // Diff mode tags nodes.
    let (status, gd) = response_msgpack::<DataflowGraph>(
        app.clone(),
        &format!("/api/graphs/dataflow?pass={canon}&func=forward&diff=1&budget=2000"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(gd.unwrap().nodes.iter().any(|n| n.change.is_some()));

    // Tiny budget truncates or collapses.
    let (status, small) = response_msgpack::<DataflowGraph>(
        app,
        &format!("/api/graphs/dataflow?pass={canon}&func=forward&diff=0&budget=1"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(small.unwrap().nodes.len() <= 1);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p server graph_endpoint`
Expected: FAIL.

- [ ] **Step 3: Implement handler**

```rust
#[derive(Deserialize)]
pub(crate) struct GraphQuery {
    pass: i64,
    func: String,
    #[serde(default)]
    diff: u8,
    budget: Option<usize>,
}

const DEFAULT_BUDGET: usize = 2000;
const MAX_BUDGET: usize = 5000;

pub(crate) async fn graph(
    State(state): State<ServerState>,
    Query(query): Query<GraphQuery>,
) -> Result<Msgpack<engine::DataflowGraph>, ApiError> {
    let budget = query.budget.unwrap_or(DEFAULT_BUDGET).clamp(1, MAX_BUDGET);
    let reader = open(&state)?;
    let pass = reader.pass(PassId(query.pass))?;

    if query.diff == 1 {
        let (Some(before_id), Some(after_id)) = (pass.ir_before, pass.ir_after) else {
            return Err(ApiError {
                status: StatusCode::CONFLICT,
                message: format!("pass {} is missing a before or after snapshot", query.pass),
            });
        };
        let before_text = reader.blob_text(before_id)?;
        let after_text = reader.blob_text(after_id)?;
        let before = state.cache.parsed(before_id, &before_text);
        let after = state.cache.parsed(after_id, &after_text);
        let g = engine::extract_dataflow_diff(
            &before,
            &after,
            &query.func,
            budget,
            &engine::GreedyFingerprintMatcher,
        );
        return Ok(Msgpack(g));
    }

    // Non-diff: render whichever side exists (prefer after).
    let blob = pass
        .ir_after
        .or(pass.ir_before)
        .ok_or_else(|| ApiError::not_found(format!("pass {} has no snapshot", query.pass)))?;
    let text = reader.blob_text(blob)?;
    let module = state.cache.parsed(blob, &text);
    let g = engine::extract_dataflow(&module, &query.func, budget);
    Ok(Msgpack(g))
}
```

Register in `lib.rs` (a new top-level API route group; add alongside the others):

```rust
        .route("/graphs/dataflow", get(api::graph))
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p server`
Expected: PASS (all server tests).

- [ ] **Step 5: Commit**

```bash
git add crates/server
git commit -m "feat(server): GET /graphs/dataflow with diff and budget"
```

---

## Task 14: UI — dependencies, API types & fetchers

**Files:**
- Modify: `ui/package.json`
- Modify: `ui/src/api.ts`
- Create: `ui/src/api.test.ts`

**Interfaces:**
- Produces (added to `ui/src/api.ts`):
  - Types: `FunctionInfo`, `ChangeClass`, `OpChange`, `FunctionDiff`, `GraphNode`, `GraphEdge`, `GraphCluster`, `DataflowGraph` — field names matching the Rust `Serialize` output.
  - `api.functions(passId)`, `api.diff(passId, func)`, `api.graph(passId, func, diff, budget)` — the latter two decode MessagePack via `@msgpack/msgpack`.

- [ ] **Step 1: Add dependencies**

Run:

```bash
cd ui && npm install elkjs@^0.9.3 @msgpack/msgpack@^3.0.0
```

- [ ] **Step 2: Write failing tests** in `ui/src/api.test.ts`

```ts
import { describe, expect, it, vi, afterEach } from 'vitest'
import { encode } from '@msgpack/msgpack'
import { api, type DataflowGraph, type FunctionDiff } from './api'

afterEach(() => vi.restoreAllMocks())

function mockFetch(body: Uint8Array | object, ok = true, contentType = 'application/msgpack') {
  const isBinary = body instanceof Uint8Array
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({
      ok,
      status: ok ? 200 : 500,
      headers: { get: () => contentType },
      arrayBuffer: async () => (isBinary ? (body as Uint8Array).buffer : new ArrayBuffer(0)),
      json: async () => body,
    })) as unknown as typeof fetch,
  )
}

describe('api msgpack decoding', () => {
  it('decodes a diff payload', async () => {
    const payload: FunctionDiff = { func: 'forward', changes: [] }
    mockFetch(encode(payload))
    const diff = await api.diff(3, 'forward')
    expect(diff.func).toBe('forward')
  })

  it('decodes a graph payload', async () => {
    const payload: DataflowGraph = { nodes: [], edges: [], clusters: [], truncated: false }
    mockFetch(encode(payload))
    const g = await api.graph(3, 'forward', false, 2000)
    expect(g.truncated).toBe(false)
  })
})
```

- [ ] **Step 3: Run to verify failure**

Run: `cd ui && npx vitest run src/api.test.ts`
Expected: FAIL (`api.diff` / `api.graph` undefined).

- [ ] **Step 4: Extend `ui/src/api.ts`**

Append types and fetchers:

```ts
import { decode } from '@msgpack/msgpack'

export interface FunctionInfo {
  name: string
  op_count: number
  has_before: boolean
  has_after: boolean
}

export type ChangeClass = 'added' | 'removed' | 'modified' | 'unchanged'

export interface OpChange {
  class: ChangeClass
  before: number | null
  after: number | null
  before_lines: [number, number] | null
  after_lines: [number, number] | null
  detail: string[]
}

export interface FunctionDiff {
  func: string
  changes: OpChange[]
}

export interface GraphNode {
  id: string
  label: string
  op_name: string
  line_range: [number, number]
  cluster: number[]
  change?: ChangeClass
  collapsed_count: number
}

export interface GraphEdge {
  from: string
  to: string
  removed: boolean
}

export interface GraphCluster {
  path: number[]
  label: string
}

export interface DataflowGraph {
  nodes: GraphNode[]
  edges: GraphEdge[]
  clusters: GraphCluster[]
  truncated: boolean
}

async function getMsgpack<T>(path: string): Promise<T> {
  const response = await fetch(path)
  if (!response.ok) {
    let message = `Request failed (${response.status})`
    try {
      const body = (await response.json()) as { error?: string }
      message = body.error ?? message
    } catch {
      // non-JSON error body; keep the status message
    }
    throw new ApiError(response.status, message)
  }
  const buffer = await response.arrayBuffer()
  return decode(new Uint8Array(buffer)) as T
}
```

Extend the exported `api` object:

```ts
export const api = {
  traceInfo: () => getJson<TraceInfo>('/api/trace/info'),
  passes: () => getJson<PassNode[]>('/api/passes'),
  irPage: (passId: number, side: IrSide) =>
    getJson<IrPage>(`/api/passes/${passId}/ir?side=${side}&limit=262144`),
  functions: (passId: number) => getJson<FunctionInfo[]>(`/api/passes/${passId}/functions`),
  diff: (passId: number, func: string) =>
    getMsgpack<FunctionDiff>(`/api/passes/${passId}/diff?func=${encodeURIComponent(func)}`),
  graph: (passId: number, func: string, diff: boolean, budget: number) =>
    getMsgpack<DataflowGraph>(
      `/api/graphs/dataflow?pass=${passId}&func=${encodeURIComponent(func)}&diff=${diff ? 1 : 0}&budget=${budget}`,
    ),
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd ui && npx vitest run src/api.test.ts && npm run typecheck`
Expected: PASS + clean typecheck.

- [ ] **Step 6: Commit**

```bash
git add ui/package.json ui/package-lock.json ui/src/api.ts ui/src/api.test.ts
git commit -m "feat(ui): msgpack diff/graph API clients and types"
```

---

## Task 15: UI — store additions (toggles, function list, payloads)

**Files:**
- Modify: `ui/src/store.ts`
- Create: `ui/src/store.test.ts`

**Interfaces:**
- Consumes: `api.functions`/`diff`/`graph` (Task 14).
- Produces store fields/actions used by Toolbar/IrViewer/GraphView:
  - `viewMode: 'text' | 'graph'`, `diffEnabled: boolean`, `selectedFunc: string | null`
  - `functions: FunctionInfo[]`, `diff: FunctionDiff | null`, `graph: DataflowGraph | null`
  - `setViewMode(m)`, `toggleDiff()`, `selectFunc(name)`
  - persistence: `viewMode`/`diffEnabled` survive `selectPass`; `selectedFunc` survives if the new pass still has it, else resets to the first function.
  - `selectPass` also loads `functions` and refreshes `diff`/`graph` per current mode.

- [ ] **Step 1: Write failing tests** in `ui/src/store.test.ts`

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { useViewerStore } from './store'
import { api } from './api'

vi.mock('./api', async (orig) => {
  const actual = await orig<typeof import('./api')>()
  return {
    ...actual,
    api: {
      traceInfo: vi.fn(async () => ({ format_version: '1', pass_count: 1, meta: {} })),
      passes: vi.fn(async () => [
        { id: 1, name: 'p', ir_before: 10, ir_after: 11, start_ns: 0, end_ns: 1, ir_changed: true, children: [] },
      ]),
      irPage: vi.fn(async () => ({ pass_id: 1, side: 'before', text: 'x', offset: 0, next_offset: null, total_bytes: 1 })),
      functions: vi.fn(async () => [
        { name: 'forward', op_count: 3, has_before: true, has_after: true },
      ]),
      diff: vi.fn(async () => ({ func: 'forward', changes: [] })),
      graph: vi.fn(async () => ({ nodes: [], edges: [], clusters: [], truncated: false })),
    },
  }
})

beforeEach(() => useViewerStore.getState().reset())

describe('store toggles', () => {
  it('defaults to text mode, diff off', () => {
    const s = useViewerStore.getState()
    expect(s.viewMode).toBe('text')
    expect(s.diffEnabled).toBe(false)
  })

  it('viewMode and diffEnabled survive pass selection', async () => {
    await useViewerStore.getState().load()
    useViewerStore.getState().setViewMode('graph')
    useViewerStore.getState().toggleDiff()
    await useViewerStore.getState().selectPass(1)
    expect(useViewerStore.getState().viewMode).toBe('graph')
    expect(useViewerStore.getState().diffEnabled).toBe(true)
  })

  it('loads functions and defaults selectedFunc to first', async () => {
    await useViewerStore.getState().load()
    expect(useViewerStore.getState().functions.map((f) => f.name)).toEqual(['forward'])
    expect(useViewerStore.getState().selectedFunc).toBe('forward')
  })

  it('fetches diff when diff enabled in text mode', async () => {
    await useViewerStore.getState().load()
    useViewerStore.getState().toggleDiff()
    await useViewerStore.getState().selectPass(1)
    expect(api.diff).toHaveBeenCalled()
    expect(useViewerStore.getState().diff).not.toBeNull()
  })
})
```

- [ ] **Step 2: Run to verify failure**

Run: `cd ui && npx vitest run src/store.test.ts`
Expected: FAIL (fields/actions absent).

- [ ] **Step 3: Extend the store**

Update `ui/src/store.ts`. Add imports and state:

```ts
import {
  api,
  type DataflowGraph,
  type FunctionDiff,
  type FunctionInfo,
  type IrPage,
  type IrSide,
  type PassNode,
  type TraceInfo,
} from './api'

type ViewMode = 'text' | 'graph'
const GRAPH_BUDGET = 2000
```

Extend `ViewerState` with:

```ts
  viewMode: ViewMode
  diffEnabled: boolean
  selectedFunc: string | null
  functions: FunctionInfo[]
  diff: FunctionDiff | null
  graph: DataflowGraph | null
  setViewMode: (mode: ViewMode) => void
  toggleDiff: () => void
  selectFunc: (name: string) => void
```

Extend `initialState`:

```ts
  viewMode: 'text' as ViewMode,
  diffEnabled: false,
  selectedFunc: null as string | null,
  functions: [] as FunctionInfo[],
  diff: null as FunctionDiff | null,
  graph: null as DataflowGraph | null,
```

Add a `refreshView` helper inside the store body and call it from `selectPass`, `setViewMode`, `toggleDiff`, `selectFunc`:

```ts
  setViewMode: (mode) => {
    set({ viewMode: mode })
    void get().refreshView()
  },
  toggleDiff: () => {
    set({ diffEnabled: !get().diffEnabled })
    void get().refreshView()
  },
  selectFunc: (name) => {
    set({ selectedFunc: name })
    void get().refreshView()
  },
  refreshView: async () => {
    const { selectedPassId, selectedFunc, viewMode, diffEnabled } = get()
    if (selectedPassId === null || selectedFunc === null) return
    try {
      if (viewMode === 'graph') {
        const graph = await api.graph(selectedPassId, selectedFunc, diffEnabled, GRAPH_BUDGET)
        if (get().selectedPassId === selectedPassId) set({ graph })
      } else if (diffEnabled) {
        const diff = await api.diff(selectedPassId, selectedFunc)
        if (get().selectedPassId === selectedPassId) set({ diff })
      }
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) })
    }
  },
```

Add `refreshView: () => Promise<void>` to the interface. In `selectPass`, after setting `before`/`after`, load functions and preserve/reset `selectedFunc`:

```ts
  selectPass: async (id) => {
    const pass = get().passesById[id]
    if (!pass) return
    set({ selectedPassId: id, before: null, after: null, diff: null, graph: null, error: null })
    try {
      const [before, after, functions] = await Promise.all([
        loadSide(pass, 'before'),
        loadSide(pass, 'after'),
        api.functions(id),
      ])
      if (get().selectedPassId !== id) return
      const prev = get().selectedFunc
      const selectedFunc =
        prev && functions.some((f) => f.name === prev) ? prev : (functions[0]?.name ?? null)
      set({ before, after, functions, selectedFunc })
      await get().refreshView()
    } catch (error) {
      set({ error: error instanceof Error ? error.message : String(error) })
    }
  },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd ui && npx vitest run src/store.test.ts && npm run typecheck`
Expected: PASS + clean typecheck.

- [ ] **Step 5: Commit**

```bash
git add ui/src/store.ts ui/src/store.test.ts
git commit -m "feat(ui): store toggles, function list, diff/graph payloads"
```

---

## Task 16: UI — Toolbar component

**Files:**
- Create: `ui/src/components/Toolbar.tsx`
- Create: `ui/src/components/Toolbar.test.tsx`
- Modify: `ui/src/App.tsx`
- Modify: `ui/src/styles.css`

**Interfaces:**
- Consumes: store (`viewMode`, `diffEnabled`, `functions`, `selectedFunc`, `setViewMode`, `toggleDiff`, `selectFunc`), and a `diffAvailable: boolean` prop computed by App (pass has both sides).
- Produces: `<Toolbar diffAvailable={boolean} />` rendering a segmented `[Text | Graph]` control, a `[Diff]` toggle button (disabled + tooltip when `!diffAvailable`), and (Graph mode, >1 function) a function `<select>`. Keyboard: `t`/`g`/`d` when focus is not in an input.

- [ ] **Step 1: Write failing tests** in `ui/src/components/Toolbar.test.tsx`

```tsx
import { describe, expect, it, beforeEach } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { Toolbar } from './Toolbar'
import { useViewerStore } from '../store'

beforeEach(() => {
  useViewerStore.setState({
    ...useViewerStore.getState(),
    viewMode: 'text',
    diffEnabled: false,
    functions: [{ name: 'forward', op_count: 3, has_before: true, has_after: true }],
    selectedFunc: 'forward',
    selectedPassId: 1,
  })
})

describe('Toolbar', () => {
  it('switches view mode on click', () => {
    render(<Toolbar diffAvailable />)
    fireEvent.click(screen.getByRole('button', { name: 'Graph' }))
    expect(useViewerStore.getState().viewMode).toBe('graph')
  })

  it('disables diff when unavailable', () => {
    render(<Toolbar diffAvailable={false} />)
    expect(screen.getByRole('button', { name: /Diff/ })).toBeDisabled()
  })

  it('keyboard g switches to graph, t back to text', () => {
    render(<Toolbar diffAvailable />)
    fireEvent.keyDown(window, { key: 'g' })
    expect(useViewerStore.getState().viewMode).toBe('graph')
    fireEvent.keyDown(window, { key: 't' })
    expect(useViewerStore.getState().viewMode).toBe('text')
  })
})
```

- [ ] **Step 2: Run to verify failure**

Run: `cd ui && npx vitest run src/components/Toolbar.test.tsx`
Expected: FAIL (no Toolbar).

- [ ] **Step 3: Implement `ui/src/components/Toolbar.tsx`**

```tsx
import { useEffect } from 'react'
import { useViewerStore } from '../store'

interface ToolbarProps {
  diffAvailable: boolean
}

export function Toolbar({ diffAvailable }: ToolbarProps) {
  const { viewMode, diffEnabled, functions, selectedFunc, setViewMode, toggleDiff, selectFunc } =
    useViewerStore()

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null
      if (target && (target.tagName === 'INPUT' || target.tagName === 'SELECT')) return
      if (e.key === 't') setViewMode('text')
      else if (e.key === 'g') setViewMode('graph')
      else if (e.key === 'd' && diffAvailable) toggleDiff()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [setViewMode, toggleDiff, diffAvailable])

  return (
    <div className="toolbar" role="toolbar" aria-label="View controls">
      <div className="segmented" role="group" aria-label="View mode">
        <button
          type="button"
          aria-pressed={viewMode === 'text'}
          className={viewMode === 'text' ? 'active' : ''}
          onClick={() => setViewMode('text')}
        >
          Text
        </button>
        <button
          type="button"
          aria-pressed={viewMode === 'graph'}
          className={viewMode === 'graph' ? 'active' : ''}
          onClick={() => setViewMode('graph')}
        >
          Graph
        </button>
      </div>
      <button
        type="button"
        className={diffEnabled ? 'diff-toggle active' : 'diff-toggle'}
        aria-pressed={diffEnabled}
        disabled={!diffAvailable}
        title={diffAvailable ? 'Toggle structural diff (d)' : 'This pass is missing a before or after snapshot'}
        onClick={() => toggleDiff()}
      >
        Diff
      </button>
      {viewMode === 'graph' && functions.length > 1 && (
        <select
          aria-label="Function"
          value={selectedFunc ?? ''}
          onChange={(e) => selectFunc(e.target.value)}
        >
          {functions.map((f) => (
            <option key={f.name} value={f.name}>
              {f.name} ({f.op_count})
            </option>
          ))}
        </select>
      )}
    </div>
  )
}
```

- [ ] **Step 4: Mount in `App.tsx`**

Compute `diffAvailable` from the selected pass and render `<Toolbar>` in the main pane header. In `App`, derive the selected pass:

```tsx
import { Toolbar } from './components/Toolbar'
// ...
const selectedPass = selectedPassId !== null ? useViewerStore.getState().passesById[selectedPassId] : null
const diffAvailable = !!(selectedPass && selectedPass.ir_before !== null && selectedPass.ir_after !== null)
```

Inside the `status === 'ready'` main block, add above `<IrViewer>`:

```tsx
<Toolbar diffAvailable={diffAvailable} />
```

- [ ] **Step 5: Add minimal styles** to `ui/src/styles.css`

```css
.toolbar { display: flex; gap: 12px; align-items: center; padding: 6px 10px; border-bottom: 1px solid #1b2130; }
.segmented button { background: #0e1117; color: #9aa4b2; border: 1px solid #1b2130; padding: 3px 12px; }
.segmented button.active { background: #1b2740; color: #cdd6e3; }
.diff-toggle { background: #0e1117; color: #9aa4b2; border: 1px solid #1b2130; padding: 3px 12px; }
.diff-toggle.active { background: #24402a; color: #cfe9d4; }
.diff-toggle:disabled { opacity: 0.4; cursor: not-allowed; }
```

- [ ] **Step 6: Run tests + typecheck**

Run: `cd ui && npx vitest run src/components/Toolbar.test.tsx && npm run typecheck`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add ui/src/components/Toolbar.tsx ui/src/components/Toolbar.test.tsx ui/src/App.tsx ui/src/styles.css
git commit -m "feat(ui): view/diff toolbar with keyboard shortcuts"
```

---

## Task 17: UI — text-mode structural diff decorations

**Files:**
- Create: `ui/src/diffDecorations.ts`
- Create: `ui/src/diffDecorations.test.ts`
- Modify: `ui/src/components/IrViewer.tsx`

**Interfaces:**
- Consumes: `FunctionDiff`, `IrPage` (Task 14), CodeMirror.
- Produces:
  - `lineClasses(diff, side, pageOffsetLine) -> Map<number, 'added'|'removed'|'modified'>` — maps a 1-based *document line within the page* to a class. `side='before'` uses `before_lines` for `removed`/`modified`; `side='after'` uses `after_lines` for `added`/`modified`. `pageOffsetLine` is the 1-based line number of the page's first line in the full snapshot (page offset → line).
  - `IrViewer` gains optional `diff: FunctionDiff | null` prop and applies line-background decorations per pane.

- [ ] **Step 1: Write failing tests** in `ui/src/diffDecorations.test.ts`

```ts
import { describe, expect, it } from 'vitest'
import { lineClasses } from './diffDecorations'
import type { FunctionDiff } from './api'

const diff: FunctionDiff = {
  func: 'f',
  changes: [
    { class: 'removed', before: 1, after: null, before_lines: [3, 3], after_lines: null, detail: [] },
    { class: 'added', before: null, after: 2, before_lines: null, after_lines: [4, 4], detail: [] },
    { class: 'modified', before: 5, after: 5, before_lines: [6, 6], after_lines: [6, 6], detail: ['type'] },
  ],
}

describe('lineClasses', () => {
  it('maps removed and modified to the before pane', () => {
    const m = lineClasses(diff, 'before', 1)
    expect(m.get(3)).toBe('removed')
    expect(m.get(6)).toBe('modified')
    expect(m.has(4)).toBe(false)
  })

  it('maps added and modified to the after pane', () => {
    const m = lineClasses(diff, 'after', 1)
    expect(m.get(4)).toBe('added')
    expect(m.get(6)).toBe('modified')
    expect(m.has(3)).toBe(false)
  })

  it('shifts by page offset line', () => {
    const m = lineClasses(diff, 'before', 3) // page starts at snapshot line 3
    expect(m.get(1)).toBe('removed') // snapshot line 3 → page line 1
  })
})
```

- [ ] **Step 2: Run to verify failure**

Run: `cd ui && npx vitest run src/diffDecorations.test.ts`
Expected: FAIL (no module).

- [ ] **Step 3: Implement `ui/src/diffDecorations.ts`**

```ts
import type { FunctionDiff, IrSide } from './api'

export type LineClass = 'added' | 'removed' | 'modified'

/**
 * Map snapshot op-change line ranges to page-local 1-based line numbers.
 * `pageOffsetLine` is the snapshot line number of the page's first line.
 */
export function lineClasses(
  diff: FunctionDiff,
  side: IrSide,
  pageOffsetLine: number,
): Map<number, LineClass> {
  const out = new Map<number, LineClass>()
  const add = (range: [number, number] | null, cls: LineClass) => {
    if (!range) return
    for (let line = range[0]; line <= range[1]; line++) {
      const local = line - pageOffsetLine + 1
      if (local >= 1) out.set(local, cls)
    }
  }
  for (const c of diff.changes) {
    if (side === 'before') {
      if (c.class === 'removed') add(c.before_lines, 'removed')
      else if (c.class === 'modified') add(c.before_lines, 'modified')
    } else {
      if (c.class === 'added') add(c.after_lines, 'added')
      else if (c.class === 'modified') add(c.after_lines, 'modified')
    }
  }
  return out
}
```

- [ ] **Step 4: Apply decorations in `IrViewer.tsx`**

Add a `diff` prop and build CodeMirror line decorations. The page begins at snapshot line 1 for M3 (Text-diff only renders the first page; multi-page diff projection is a later refinement — the store fetches full-blob diffs but text display is paged, so pass `pageOffsetLine = 1`). Extend `IrViewerProps`:

```tsx
import { Decoration, type DecorationSet, EditorView, ViewPlugin, lineNumbers } from '@codemirror/view'
import { RangeSetBuilder } from '@codemirror/state'
import { lineClasses, type LineClass } from '../diffDecorations'
import type { FunctionDiff, IrPage, IrSide } from '../api'

interface IrViewerProps {
  before: IrPage | null
  after: IrPage | null
  diff: FunctionDiff | null
}
```

Add a decoration builder and plugin factory:

```tsx
const lineDeco: Record<LineClass, Decoration> = {
  added: Decoration.line({ attributes: { class: 'diff-added' } }),
  removed: Decoration.line({ attributes: { class: 'diff-removed' } }),
  modified: Decoration.line({ attributes: { class: 'diff-modified' } }),
}

function diffExtension(diff: FunctionDiff | null, side: IrSide) {
  const classes = diff ? lineClasses(diff, side, 1) : new Map<number, LineClass>()
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet
      constructor(view: EditorView) {
        this.decorations = this.build(view)
      }
      update() {}
      build(view: EditorView): DecorationSet {
        const builder = new RangeSetBuilder<Decoration>()
        for (let i = 1; i <= view.state.doc.lines; i++) {
          const cls = classes.get(i)
          if (cls) builder.add(view.state.doc.line(i).from, view.state.doc.line(i).from, lineDeco[cls])
        }
        return builder.finish()
      }
    },
    { decorations: (v) => v.decorations },
  )
}
```

Pass `diffExtension(diff, side)` into the `EditorState.create` `extensions` array in `EditorPane`, and add `diff` + `side` to the `useEffect` dependency list so panes rebuild when the diff arrives. Thread the `diff` prop through `IrViewer` → `EditorPane`.

- [ ] **Step 5: Add diff line styles** to `ui/src/styles.css`

```css
.cm-line.diff-added { background: rgba(46, 160, 67, 0.18); }
.cm-line.diff-removed { background: rgba(248, 81, 73, 0.18); }
.cm-line.diff-modified { background: rgba(210, 168, 60, 0.20); }
```

- [ ] **Step 6: Update `App.tsx`** to pass `diff`

```tsx
<IrViewer before={before} after={after} diff={diffEnabled && viewMode === 'text' ? diff : null} />
```

Pull `diff`, `diffEnabled`, `viewMode` from the store destructuring.

- [ ] **Step 7: Run tests + typecheck**

Run: `cd ui && npx vitest run src/diffDecorations.test.ts && npm run typecheck`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add ui/src/diffDecorations.ts ui/src/diffDecorations.test.ts ui/src/components/IrViewer.tsx ui/src/styles.css ui/src/App.tsx
git commit -m "feat(ui): structural-diff line decorations in text mode"
```

---

## Task 18: UI — graph rendering (ELK worker + canvas LOD)

**Files:**
- Create: `ui/src/graph/layout.worker.ts`
- Create: `ui/src/graph/render.ts`
- Create: `ui/src/graph/render.test.ts`
- Create: `ui/src/components/GraphView.tsx`
- Modify: `ui/src/App.tsx`
- Modify: `ui/src/styles.css`

**Interfaces:**
- Consumes: `DataflowGraph` (Task 14), `elkjs`.
- Produces:
  - `layout.worker.ts` — receives `{ graph: DataflowGraph }`, runs ELK layered layout, posts `{ positions: Record<string, {x,y,width,height}>, edges: {from,to,removed,sections}[] }`.
  - `render.ts` — pure helpers: `nodeColor(change?)`, `hitTest(layout, worldX, worldY)`, `drawGraph(ctx, layout, graph, view)` where `view = { scale, offsetX, offsetY, hoverId }`. LOD: `scale < 0.5` → filled boxes only; else labels.
  - `GraphView.tsx` — canvas host: dispatches layout to the worker, renders on the returned layout, wheel-zoom + drag-pan, hover highlight, click selection, legend row, truncated/collapsed chips.

- [ ] **Step 1: Write failing tests for pure helpers** in `ui/src/graph/render.test.ts`

```ts
import { describe, expect, it } from 'vitest'
import { nodeColor, hitTest, type Layout } from './render'

describe('render helpers', () => {
  it('colors nodes by change class', () => {
    expect(nodeColor('added')).toBe('#2ea043')
    expect(nodeColor('removed')).toBe('#f85149')
    expect(nodeColor('modified')).toBe('#d2a83c')
    expect(nodeColor(undefined)).toBe('#3a4658')
  })

  it('hit-tests a node by world coordinates', () => {
    const layout: Layout = {
      positions: { op0: { x: 10, y: 10, width: 100, height: 40 } },
      edges: [],
    }
    expect(hitTest(layout, 50, 30)).toBe('op0')
    expect(hitTest(layout, 500, 500)).toBeNull()
  })
})
```

- [ ] **Step 2: Run to verify failure**

Run: `cd ui && npx vitest run src/graph/render.test.ts`
Expected: FAIL (no module).

- [ ] **Step 3: Implement `ui/src/graph/render.ts`**

```ts
import type { ChangeClass, DataflowGraph } from '../api'

export interface NodeBox {
  x: number
  y: number
  width: number
  height: number
}

export interface LaidOutEdge {
  from: string
  to: string
  removed: boolean
  sections: { startPoint: { x: number; y: number }; endPoint: { x: number; y: number } }[]
}

export interface Layout {
  positions: Record<string, NodeBox>
  edges: LaidOutEdge[]
}

export interface ViewState {
  scale: number
  offsetX: number
  offsetY: number
  hoverId: string | null
  selectedId: string | null
}

export function nodeColor(change?: ChangeClass): string {
  switch (change) {
    case 'added':
      return '#2ea043'
    case 'removed':
      return '#f85149'
    case 'modified':
      return '#d2a83c'
    default:
      return '#3a4658'
  }
}

/** Return the id of the node whose box contains (worldX, worldY), else null. */
export function hitTest(layout: Layout, worldX: number, worldY: number): string | null {
  for (const [id, b] of Object.entries(layout.positions)) {
    if (worldX >= b.x && worldX <= b.x + b.width && worldY >= b.y && worldY <= b.y + b.height) {
      return id
    }
  }
  return null
}

/** Draw the whole graph. Screen = world * scale + offset. LOD by scale. */
export function drawGraph(
  ctx: CanvasRenderingContext2D,
  layout: Layout,
  graph: DataflowGraph,
  view: ViewState,
): void {
  const { scale, offsetX, offsetY } = view
  ctx.save()
  ctx.setTransform(scale, 0, 0, scale, offsetX, offsetY)
  ctx.clearRect(-offsetX / scale, -offsetY / scale, ctx.canvas.width / scale, ctx.canvas.height / scale)

  // Edges first.
  for (const e of layout.edges) {
    ctx.strokeStyle = e.removed ? 'rgba(248,81,73,0.7)' : '#4a5568'
    ctx.lineWidth = 1 / scale
    if (e.removed) ctx.setLineDash([4 / scale, 3 / scale])
    else ctx.setLineDash([])
    for (const s of e.sections) {
      ctx.beginPath()
      ctx.moveTo(s.startPoint.x, s.startPoint.y)
      ctx.lineTo(s.endPoint.x, s.endPoint.y)
      ctx.stroke()
    }
  }
  ctx.setLineDash([])

  const byId = new Map(graph.nodes.map((n) => [n.id, n]))
  const showLabels = scale >= 0.5
  for (const [id, b] of Object.entries(layout.positions)) {
    const node = byId.get(id)
    const color = nodeColor(node?.change)
    ctx.globalAlpha = node?.change === 'removed' ? 0.5 : 1
    ctx.fillStyle = color
    ctx.fillRect(b.x, b.y, b.width, b.height)
    if (view.hoverId === id || view.selectedId === id) {
      ctx.strokeStyle = '#cdd6e3'
      ctx.lineWidth = 2 / scale
      ctx.strokeRect(b.x, b.y, b.width, b.height)
    }
    if (showLabels && node) {
      ctx.globalAlpha = 1
      ctx.fillStyle = '#0b0d12'
      ctx.font = `${12}px ui-monospace, monospace`
      const text = node.collapsed_count > 0 ? `${node.collapsed_count} ops` : node.label
      ctx.fillText(text.slice(0, 28), b.x + 6, b.y + b.height / 2 + 4)
    }
  }
  ctx.globalAlpha = 1
  ctx.restore()
}
```

- [ ] **Step 4: Implement `ui/src/graph/layout.worker.ts`**

```ts
import ELK from 'elkjs/lib/elk.bundled.js'
import type { DataflowGraph } from '../api'

const elk = new ELK()

self.onmessage = async (e: MessageEvent<{ graph: DataflowGraph }>) => {
  const { graph } = e.data
  const elkGraph = {
    id: 'root',
    layoutOptions: {
      'elk.algorithm': 'layered',
      'elk.direction': 'DOWN',
      'elk.spacing.nodeNode': '24',
      'elk.layered.spacing.nodeNodeBetweenLayers': '40',
    },
    children: graph.nodes.map((n) => ({ id: n.id, width: 160, height: 34 })),
    edges: graph.edges.map((edge, i) => ({
      id: `e${i}`,
      sources: [edge.from],
      targets: [edge.to],
    })),
  }
  const res = await elk.layout(elkGraph)
  const positions: Record<string, { x: number; y: number; width: number; height: number }> = {}
  for (const c of res.children ?? []) {
    positions[c.id] = { x: c.x ?? 0, y: c.y ?? 0, width: c.width ?? 160, height: c.height ?? 34 }
  }
  const edges = (res.edges ?? []).map((le, i) => ({
    from: graph.edges[i].from,
    to: graph.edges[i].to,
    removed: graph.edges[i].removed,
    sections: (le.sections ?? []).map((s) => ({ startPoint: s.startPoint, endPoint: s.endPoint })),
  }))
  ;(self as unknown as Worker).postMessage({ positions, edges })
}
```

- [ ] **Step 5: Implement `ui/src/components/GraphView.tsx`**

```tsx
import { useEffect, useRef, useState } from 'react'
import type { DataflowGraph } from '../api'
import { drawGraph, hitTest, type Layout, type ViewState } from '../graph/render'
import { useViewerStore } from '../store'

interface GraphViewProps {
  graph: DataflowGraph | null
  diffEnabled: boolean
}

export function GraphView({ graph, diffEnabled }: GraphViewProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const [layout, setLayout] = useState<Layout | null>(null)
  const [laying, setLaying] = useState(false)
  const viewRef = useRef<ViewState>({ scale: 1, offsetX: 20, offsetY: 20, hoverId: null, selectedId: null })

  // Run ELK layout in the worker whenever the graph changes.
  useEffect(() => {
    if (!graph) {
      setLayout(null)
      return
    }
    setLaying(true)
    const worker = new Worker(new URL('../graph/layout.worker.ts', import.meta.url), { type: 'module' })
    worker.onmessage = (e: MessageEvent<Layout>) => {
      setLayout(e.data)
      setLaying(false)
      worker.terminate()
    }
    worker.postMessage({ graph })
    return () => worker.terminate()
  }, [graph])

  // Redraw on layout/view changes.
  const redraw = () => {
    const canvas = canvasRef.current
    if (!canvas || !layout || !graph) return
    const ctx = canvas.getContext('2d')
    if (ctx) drawGraph(ctx, layout, graph, viewRef.current)
  }
  useEffect(redraw, [layout, graph])

  const toWorld = (clientX: number, clientY: number) => {
    const canvas = canvasRef.current!
    const rect = canvas.getBoundingClientRect()
    const v = viewRef.current
    return {
      x: (clientX - rect.left - v.offsetX) / v.scale,
      y: (clientY - rect.top - v.offsetY) / v.scale,
    }
  }

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault()
    const v = viewRef.current
    const factor = e.deltaY < 0 ? 1.1 : 0.9
    v.scale = Math.min(4, Math.max(0.15, v.scale * factor))
    redraw()
  }

  const dragging = useRef<{ x: number; y: number } | null>(null)
  const onPointerDown = (e: React.PointerEvent) => {
    dragging.current = { x: e.clientX, y: e.clientY }
  }
  const onPointerMove = (e: React.PointerEvent) => {
    const v = viewRef.current
    if (dragging.current) {
      v.offsetX += e.clientX - dragging.current.x
      v.offsetY += e.clientY - dragging.current.y
      dragging.current = { x: e.clientX, y: e.clientY }
      redraw()
      return
    }
    if (layout) {
      const w = toWorld(e.clientX, e.clientY)
      const hit = hitTest(layout, w.x, w.y)
      if (hit !== v.hoverId) {
        v.hoverId = hit
        redraw()
      }
    }
  }
  const onPointerUp = (e: React.PointerEvent) => {
    const wasDrag = dragging.current && (Math.abs(e.clientX - dragging.current.x) > 3)
    dragging.current = null
    if (!wasDrag && layout) {
      const w = toWorld(e.clientX, e.clientY)
      const hit = hitTest(layout, w.x, w.y)
      viewRef.current.selectedId = hit
      useViewerStore.setState({}) // selection recorded locally; inspector is out of scope
      redraw()
    }
  }

  return (
    <section className="graph-view" aria-label="Dataflow graph">
      <div className="graph-legend">
        {diffEnabled && (
          <>
            <span className="chip added">added</span>
            <span className="chip removed">removed</span>
            <span className="chip modified">modified</span>
          </>
        )}
        {graph?.truncated && <span className="chip warn">Graph truncated to node budget</span>}
        {graph?.clusters.length ? <span className="chip">{graph.clusters.length} clusters collapsed</span> : null}
      </div>
      {laying && <div className="status">Laying out graph…</div>}
      <canvas
        ref={canvasRef}
        width={1200}
        height={800}
        onWheel={onWheel}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
      />
    </section>
  )
}
```

- [ ] **Step 6: Mount in `App.tsx`** and add styles

In `App.tsx`, render `GraphView` when `viewMode === 'graph'`, else `IrViewer`:

```tsx
{viewMode === 'graph' ? (
  <GraphView graph={graph} diffEnabled={diffEnabled} />
) : (
  <IrViewer before={before} after={after} diff={diffEnabled ? diff : null} />
)}
```

Add to `ui/src/styles.css`:

```css
.graph-view { display: flex; flex-direction: column; height: 100%; }
.graph-view canvas { flex: 1; background: #0b0d12; touch-action: none; cursor: grab; }
.graph-legend { display: flex; gap: 8px; padding: 4px 10px; }
.chip { font-size: 12px; padding: 2px 8px; border-radius: 10px; background: #1b2130; color: #9aa4b2; }
.chip.added { background: #24402a; color: #cfe9d4; }
.chip.removed { background: #40242a; color: #e9cfd4; }
.chip.modified { background: #40392a; color: #e9e2cf; }
.chip.warn { background: #4a2a1b; color: #e9d4c0; }
```

- [ ] **Step 7: Run tests + typecheck**

Run: `cd ui && npx vitest run src/graph/render.test.ts && npm run typecheck`
Expected: PASS. (The worker and canvas draw are exercised by Playwright in Task 19; jsdom lacks canvas 2D, so component-mount tests are not attempted here.)

- [ ] **Step 8: Commit**

```bash
git add ui/src/graph ui/src/components/GraphView.tsx ui/src/App.tsx ui/src/styles.css
git commit -m "feat(ui): ELK-in-worker canvas graph renderer with LOD and diff colors"
```

---

## Task 19: Playwright smoke test on a real trace

**Files:**
- Create: `ui/e2e/graph-diff.spec.ts` (or extend the existing M2 e2e file if present)
- Modify: `ui/playwright.config.ts` if needed (reuse M2 config)

**Interfaces:**
- Consumes: the running server (`mlir-viewer serve`) over the demo fixture, the full UI.
- Produces: a smoke test that switches Text↔Graph, toggles Diff in both modes, and switches function.

- [ ] **Step 1: Confirm how the M2 e2e harness starts the server**

Run: `ls ui/e2e 2>/dev/null; sed -n '1,60p' ui/playwright.config.ts 2>/dev/null`
Reuse its `webServer`/fixture setup. If M2 has no e2e, add a `webServer` that builds the UI, generates the demo trace via the CLI, and runs `mlir-viewer serve` on a test port. (Match the CLI subcommand used in M2 docs; the fixture writer is `trace_format::fixture::write_demo_trace`, exposed through the CLI's demo/fixture command.)

- [ ] **Step 2: Write the smoke spec** in `ui/e2e/graph-diff.spec.ts`

```ts
import { expect, test } from '@playwright/test'

test('text and graph views with diff toggles', async ({ page }) => {
  await page.goto('/')
  // Select a pass that changes IR (canonicalize).
  await page.getByText('canonicalize').click()

  // Text mode: toggle diff, expect decorated lines.
  await page.getByRole('button', { name: /Diff/ }).click()
  await expect(page.locator('.cm-line.diff-removed, .cm-line.diff-added').first()).toBeVisible()

  // Switch to graph mode.
  await page.getByRole('button', { name: 'Graph' }).click()
  await expect(page.locator('canvas')).toBeVisible()
  // Layout spinner resolves.
  await expect(page.getByText('Laying out graph…')).toBeHidden({ timeout: 10000 })

  // Diff is already on: legend chips show.
  await expect(page.locator('.graph-legend .chip.added')).toBeVisible()

  // Switch back to text.
  await page.getByRole('button', { name: 'Text' }).click()
  await expect(page.locator('.editor-grid')).toBeVisible()
})
```

- [ ] **Step 3: Run the smoke test**

Run: `cd ui && npx playwright test e2e/graph-diff.spec.ts`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add ui/e2e/graph-diff.spec.ts ui/playwright.config.ts
git commit -m "test(ui): Playwright smoke for graph/diff toggles on demo trace"
```

---

## Task 20: Full-suite verification & branch finish

**Files:** none (verification only).

- [ ] **Step 1: Rust suite + lints**

Run:
```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --check
```
Expected: all pass, no clippy warnings, formatting clean.

- [ ] **Step 2: UI suite + build**

Run:
```bash
cd ui && npm run typecheck && npx vitest run && npm run build
```
Expected: clean typecheck, all vitest pass, production build succeeds (rust-embed will pick up `ui/dist`).

- [ ] **Step 3: End-to-end manual check with the real binary**

Run:
```bash
cargo run -p cli -- serve <demo.mlirtrace>   # match the M2 CLI serve invocation
```
Open the browser, verify: Text/Graph toggle, diff decorations on `canonicalize`, graph layout renders, function dropdown appears on the LLVM stage (two functions), Diff disabled when a pass lacks a side (none in the demo — confirm tooltip wiring instead).

- [ ] **Step 4: Finish the branch**

Use the `superpowers:finishing-a-development-branch` skill to merge `feat/m3-graph-diff` into `feat/m2-walking-skeleton` (or open a PR), per repo convention.

---

## Self-review notes (author checklist, resolved)

- **Spec §4.1 parser** → Tasks 2–4 (statements, regions/scopes, opaque recovery, fixture golden). ✅
- **Spec §4.2 diff** → Tasks 5–6 (`OpMatcher` seam + greedy fingerprint impl; classification with detail + line ranges; per-function; cached in Task 10/12). ✅
- **Spec §4.3 graph** → Tasks 7–9 (def-use, cluster/budget determinism, unified diff graph with ghosts + dashed removed edges). ✅
- **Spec §5 API** → `/functions` (Task 11), `/diff` (Task 12, 409 + no-op path), `/graphs/dataflow` (Task 13, diff + budget). MessagePack for diff/graph (Task 10). ✅
- **Spec §6 UI** → Toolbar + keyboard + state persistence (Tasks 15–16); text split diff decorations (Task 17); graph mode ELK-worker + canvas LOD + hover/click + collapsed/truncated chips + legend (Task 18); function dropdown (Task 16). ✅
- **Spec §7 edge cases** → missing side ⇒ Diff disabled (Task 16) + 409 server (Tasks 12–13); `ir_changed=false` fast path (Task 12); module-only scope (Task 3); opaque recovery (Task 4); budget truncation chip (Task 18). ✅
- **Spec §8 testing** → engine unit/golden (Tasks 2–9), server integration incl. msgpack round-trip (Tasks 11–13), UI vitest (Tasks 14–18) + Playwright (Task 19). ✅
- **Spec §9 out-of-scope** honored: no provenance, no search, dataflow-only behind seam, node-click records selection only, no cluster focus round-trip. ✅
- **Uid-first/fingerprint-fallback**: `OpMatcher` trait is the seam; only `GreedyFingerprintMatcher` ships. ✅
- **Type consistency**: `FunctionDiff`/`OpChange`/`ChangeClass`/`DataflowGraph`/`GraphNode`/`GraphEdge`/`GraphCluster` names identical across Rust (`Serialize`) and TS; `collapsed_count`, `line_range`, `before_lines`/`after_lines` field names match msgpack `to_vec_named` output. ✅
