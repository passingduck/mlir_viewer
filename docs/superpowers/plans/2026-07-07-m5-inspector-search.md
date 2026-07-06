# M5 Inspector, Search & Palette Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Op Inspector panel (structure + history tabs), server-side op search with a cmd-k command palette, dockview workspace with persisted layout, and a synthesized `Disappeared` history step for evidence-less removals.

**Architecture:** Engine gains a pure `search` module and one new `HistoryChange` variant; the server exposes `GET /api/search` and `GET /api/ops/{uid}` reusing the existing timeline/resolved caches; the UI replaces the `History` toolbar mode with a right-hand Inspector panel, adds a cmdk palette, and finally wraps the three panes in dockview with localStorage persistence.

**Tech Stack:** Rust (axum, serde, rmp-serde via existing `Msgpack`), React 19 + TypeScript + Zustand, `cmdk` ^1, `dockview` ^4, Vitest + Playwright.

## Global Constraints

- Bulk payloads are MessagePack (`Msgpack<T>`); control-plane stays JSON — match the existing split.
- Every list response is budgeted: search default 200, max 500 results.
- No trace schema changes; v1/Text-only traces must keep working through every task.
- Rust: `export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"` first (cargo not on PATH).
- After each task all suites pass: `cargo test --workspace`, `cd ui && npm run typecheck && npx vitest run`, and (UI tasks) `npx playwright test`.
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Engine — synthesized `Disappeared` history step

**Files:**
- Modify: `crates/engine/src/provenance.rs` (`HistoryChange` enum ~line 171; `resolve_function` ~line 546)
- Test: `crates/engine/tests/provenance.rs`

**Interfaces:**
- Consumes: existing `resolve_function(function, stages) -> ResolvedFunction`.
- Produces: `HistoryChange::Disappeared` (serialized `"disappeared"`); every `OpHistory` in `ResolvedFunction::histories` now has ≥1 step.

- [ ] **Step 1: Write the failing test** in `crates/engine/tests/provenance.rs` (follow the existing test helpers in that file for building `TimelineStage`s — reuse its snapshot-builder helper):

```rust
#[test]
fn unlinked_removed_op_gets_terminal_disappeared_step() {
    // Stage 1: before has an op that vanishes; after contains only an op with
    // a different name so the fingerprint matcher cannot link them and no
    // identity events exist.
    let stages = vec![stage(
        1,
        "canonicalize",
        Some(snapshot(SnapshotSide::Before, "func.func @f() {\n  \"x.vanish\"() : () -> ()\n}\n")),
        Some(snapshot(SnapshotSide::After, "func.func @f() {\n  \"y.other\"() : () -> ()\n}\n")),
        vec![],
    )];
    let resolved = resolve_function("f", &stages);
    let history = resolved
        .histories
        .values()
        .find(|h| h.first_name == "x.vanish")
        .expect("vanished op has a history");
    assert_eq!(history.steps.len(), 1);
    assert_eq!(history.steps[0].change, HistoryChange::Disappeared);
    assert!(history.steps[0].before.is_some());
    assert!(history.steps[0].after.is_none());
    assert_eq!(history.steps[0].evidence, vec![]);
}
```

Adapt `stage(...)`/`snapshot(...)` to whatever helper names the test file already defines; if it builds stages inline, build them inline the same way.

- [ ] **Step 2: Run it, expect FAIL** (`histories` entry has `steps.len() == 0` or variant doesn't exist):

```sh
cargo test -p engine --test provenance unlinked_removed -- --nocapture
```

- [ ] **Step 3: Implement.** In `provenance.rs`: add the variant

```rust
pub enum HistoryChange {
    Inserted,
    Erased,
    Replaced,
    Modified,
    Unchanged,
    /// Present in one snapshot, then gone with neither an exact event nor an
    /// inferred match — synthesized so no history is ever an empty timeline.
    Disappeared,
}
```

At the end of the per-component history construction inside `resolve_function` (where `OpHistory { steps, .. }` is assembled), when `steps.is_empty()`, synthesize a terminal step from the component's sole occurrence (the anchor):

```rust
if steps.is_empty() {
    steps.push(HistoryStep {
        pass_id: anchor_pass_id,
        pass_name: anchor_pass_name.clone(),
        change: HistoryChange::Disappeared,
        before: Some(anchor_occurrence.clone()),
        after: None,
        evidence: Vec::new(),
        confidence: LinkConfidence::Exact, // its presence in the snapshot is not inferred
    });
}
```

Use whatever locals the surrounding code already has for the anchor's stage/occurrence (`Node` carries `pass_id` and `operation`).

- [ ] **Step 4: Run engine + server suites, expect PASS** (server tests consume the enum):

```sh
cargo test -p engine && cargo test -p server
```

- [ ] **Step 5: Update the UI type** — in `ui/src/api.ts` find `HistoryChange`/`change` union and add `'disappeared'`; in `ui/src/components/HistoryView.tsx` the `∅` right side already renders. Add a vitest case to `ui/src/components/HistoryView.test.tsx`:

```tsx
it('renders a disappeared terminal step', () => {
  render(
    <HistoryView
      history={{
        uid: 'op1.Zg.1.b.0',
        first_name: 'x.vanish',
        last_name: 'x.vanish',
        steps: [
          {
            pass_id: 1,
            pass_name: 'canonicalize',
            change: 'disappeared',
            before: { side: 'before', op_idx: 0, name: 'x.vanish', line_start: 2, line_end: 2, attr_summary: '', location: null },
            after: null,
            evidence: [],
            confidence: { kind: 'exact' },
          },
        ],
      }}
      onViewIr={() => {}}
    />,
  )
  expect(screen.getByText('disappeared')).toBeInTheDocument()
})
```

- [ ] **Step 6: Run UI tests, expect PASS:** `cd ui && npx vitest run`

- [ ] **Step 7: Commit**

```sh
git add crates/engine ui/src
git commit -m "feat(engine): synthesize Disappeared step for unlinked removals"
```

---

### Task 2: Engine — `search` module

**Files:**
- Create: `crates/engine/src/search.rs`
- Modify: `crates/engine/src/lib.rs` (add `pub mod search;` + re-export)
- Test: `crates/engine/tests/search.rs`

**Interfaces:**
- Consumes: `ParsedModule` / `ParsedOp` from `model.rs`.
- Produces: `pub fn search_module(module: &ParsedModule, query: &str, budget: usize) -> Vec<SearchMatch>` and `pub struct SearchMatch { pub func: String, pub op_idx: OpIdx, pub name: String, pub line_start: usize, pub line_end: usize, pub excerpt: String }` (Serialize).

- [ ] **Step 1: Write the failing test** `crates/engine/tests/search.rs`:

```rust
use engine::{parse_module, search_module};

const IR: &str = r#"module {
  func.func @forward(%arg0: i32) -> i32 {
    %c = arith.constant {value = 42 : i32} 42 : i32
    %r = arith.addi %arg0, %c : i32
    return %r : i32
  }
}"#;

#[test]
fn matches_are_case_insensitive_and_scoped_to_functions() {
    let module = parse_module(IR);
    let hits = search_module(&module, "ADDI", 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].func, "forward");
    assert_eq!(hits[0].name, "arith.addi");
    assert!(hits[0].line_start >= 1);
}

#[test]
fn matches_attributes_and_respects_budget() {
    let module = parse_module(IR);
    assert_eq!(search_module(&module, "value = 42", 10).len(), 1);
    assert_eq!(search_module(&module, "arith", 1).len(), 1); // 2 candidates, budget 1
    assert!(search_module(&module, "", 10).is_empty()); // blank query matches nothing
    assert!(search_module(&module, "zzz", 10).is_empty());
}
```

- [ ] **Step 2: Run, expect FAIL** (module missing): `cargo test -p engine --test search`

- [ ] **Step 3: Implement** `crates/engine/src/search.rs`. A linear scan over parsed ops is the per-snapshot "index" — modules are already cached parsed, and 10⁴ ops scan in well under a millisecond, so no separate index structure (YAGNI):

```rust
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
```

In `crates/engine/src/lib.rs` add `pub mod search;` and `pub use search::{search_module, SearchMatch};`.

- [ ] **Step 4: Run, expect PASS:** `cargo test -p engine --test search`

- [ ] **Step 5: Commit**

```sh
git add crates/engine
git commit -m "feat(engine): add case-insensitive op search over parsed modules"
```

---

### Task 3: Server — `GET /api/search`

**Files:**
- Modify: `crates/server/src/api.rs`, `crates/server/src/lib.rs` (route), `crates/server/src/provenance.rs` (make `collect_leaves` `pub(crate)`)
- Test: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `engine::search_module`, `collect_leaves`, `state.cache.parsed`.
- Produces: `GET /api/search?q=…&pass=…&side=before|after&scope=pass|pipeline&budget=…` → `Msgpack<Vec<SearchResultDto>>` where

```rust
#[derive(Serialize)]
pub(crate) struct SearchResultDto {
    pass_id: i64,
    side: String,        // "before" | "after"
    func: String,
    op_idx: usize,
    name: String,
    line_start: usize,
    line_end: usize,
    excerpt: String,
}
```

No `uid` field: the client resolves a uid on click via the existing `/passes/{id}/ops` (already cached per function), keeping pipeline-wide search free of provenance resolution cost.

- [ ] **Step 1: Write failing tests** in `crates/server/tests/api.rs`, following that file's existing fixture/server bootstrap pattern (it builds a demo trace and issues requests; mirror the ops/history tests):

```rust
#[tokio::test]
async fn search_scope_pass_finds_ops_on_requested_side() {
    let (server, _dir) = full_fixture_server().await; // reuse the file's helper
    let body = msgpack_get::<Vec<serde_json::Value>>(
        &server,
        "/api/search?q=arith&pass=2&side=before&scope=pass",
    )
    .await;
    assert!(!body.is_empty());
    assert!(body.iter().all(|r| r["side"] == "before" && r["pass_id"] == 2));
}

#[tokio::test]
async fn search_scope_pipeline_dedupes_shared_blobs_and_respects_budget() {
    let (server, _dir) = full_fixture_server().await;
    let all = msgpack_get::<Vec<serde_json::Value>>(
        &server,
        "/api/search?q=arith&pass=2&scope=pipeline",
    )
    .await;
    let one = msgpack_get::<Vec<serde_json::Value>>(
        &server,
        "/api/search?q=arith&pass=2&scope=pipeline&budget=1",
    )
    .await;
    assert!(!all.is_empty());
    assert_eq!(one.len(), 1);
}

#[tokio::test]
async fn search_rejects_bad_params() {
    let (server, _dir) = full_fixture_server().await;
    assert_eq!(status_of(&server, "/api/search?q=x&pass=2&side=sideways").await, 400);
    assert_eq!(status_of(&server, "/api/search?q=x&pass=999").await, 404);
    assert_eq!(status_of(&server, "/api/search?q=x&pass=2&budget=9999").await, 400);
}
```

Use the response-decoding helpers the test file already has (or add small `msgpack_get`/`status_of` helpers next to them).

- [ ] **Step 2: Run, expect FAIL (404 route):** `cargo test -p server --test api search`

- [ ] **Step 3: Implement.** In `provenance.rs` change `fn collect_leaves` to `pub(crate) fn collect_leaves`. In `api.rs`:

```rust
#[derive(Deserialize)]
pub(crate) struct SearchQuery {
    q: String,
    pass: i64,
    side: Option<String>,
    scope: Option<String>,
    budget: Option<usize>,
}

const DEFAULT_SEARCH_BUDGET: usize = 200;
const MAX_SEARCH_BUDGET: usize = 500;

pub(crate) async fn search(
    State(state): State<ServerState>,
    Query(query): Query<SearchQuery>,
) -> Result<Msgpack<Vec<SearchResultDto>>, ApiError> {
    let budget = query.budget.unwrap_or(DEFAULT_SEARCH_BUDGET);
    if budget == 0 || budget > MAX_SEARCH_BUDGET {
        return Err(ApiError::bad_request(format!(
            "budget must be between 1 and {MAX_SEARCH_BUDGET}"
        )));
    }
    let scope = query.scope.as_deref().unwrap_or("pass");
    if scope != "pass" && scope != "pipeline" {
        return Err(ApiError::bad_request("scope must be 'pass' or 'pipeline'"));
    }
    let reader = open(&state)?;
    let pass = reader.pass(PassId(query.pass))?;

    // (pass_id, side-name, blob) list to search, deduped by blob.
    let mut targets: Vec<(i64, &'static str, BlobId)> = Vec::new();
    if scope == "pass" {
        let side = query.side.as_deref().unwrap_or("after");
        let blob = match side {
            "before" => pass.ir_before,
            "after" => pass.ir_after,
            _ => return Err(ApiError::bad_request("side must be 'before' or 'after'")),
        };
        let blob = blob.ok_or_else(|| {
            ApiError::not_found(format!("pass {} has no {side} snapshot", query.pass))
        })?;
        let side_name: &'static str = if side == "before" { "before" } else { "after" };
        targets.push((query.pass, side_name, blob));
    } else {
        let roots = reader.passes()?;
        let mut leaves = Vec::new();
        crate::provenance::collect_leaves(&roots, &mut 0, &mut leaves);
        leaves.sort_by_key(|(order, leaf)| (leaf.start_ns, *order));
        let mut seen = std::collections::HashSet::new();
        for (_, leaf) in leaves {
            let (side_name, blob) = match (leaf.ir_after, leaf.ir_before) {
                (Some(blob), _) => ("after", blob),
                (None, Some(blob)) => ("before", blob),
                (None, None) => continue,
            };
            if seen.insert(blob) {
                targets.push((leaf.id.0, side_name, blob));
            }
        }
    }

    let mut results = Vec::new();
    for (pass_id, side_name, blob) in targets {
        let text = reader.blob_text(blob)?;
        let module = state.cache.parsed(blob, &text);
        for hit in engine::search_module(&module, &query.q, budget - results.len()) {
            results.push(SearchResultDto {
                pass_id,
                side: side_name.to_string(),
                func: hit.func,
                op_idx: hit.op_idx,
                name: hit.name,
                line_start: hit.line_start,
                line_end: hit.line_end,
                excerpt: hit.excerpt,
            });
        }
        if results.len() >= budget {
            break;
        }
    }
    Ok(Msgpack(results))
}
```

Route in `lib.rs`: `.route("/search", get(api::search))`.

- [ ] **Step 4: Run, expect PASS:** `cargo test -p server`

- [ ] **Step 5: Commit**

```sh
git add crates/server
git commit -m "feat(server): add budgeted op search endpoint"
```

---

### Task 4: Server — `GET /api/ops/{uid}` inspector detail

**Files:**
- Modify: `crates/server/src/api.rs`, `crates/server/src/lib.rs` (route)
- Test: `crates/server/tests/api.rs`

**Interfaces:**
- Consumes: `engine::OpUid::parse`, `parse_anchor()` (`OpAnchor { function, pass_id, side, function_ordinal }`), `crate::provenance::resolved_function`, `state.cache.timeline`.
- Produces: `GET /api/ops/{uid}?pass=…&side=before|after` → `Msgpack<OpDetailDto>`:

```rust
#[derive(Serialize)]
pub(crate) struct OpDetailDto {
    uid: String,
    func: String,
    pass_id: i64,
    side: String,
    op_idx: usize,
    name: String,
    results: Vec<String>,
    operands: Vec<String>,
    result_types: Vec<String>,
    attr_summary: String,   // truncated to 2048 chars with '…'
    location: Option<String>,
    region_path: Vec<usize>,
    line_start: usize,
    line_end: usize,
    opaque: bool,
}
```

`pass`/`side` select which occurrence to describe (the pass the user is viewing); omitted → the anchor occurrence. If the op has no occurrence at the requested pass/side → 404.

- [ ] **Step 1: Write failing tests** in `crates/server/tests/api.rs`:

```rust
#[tokio::test]
async fn op_detail_returns_anchor_occurrence_by_default() {
    let (server, _dir) = full_fixture_server().await;
    let ops = msgpack_get::<Vec<serde_json::Value>>(
        &server, "/api/passes/2/ops?side=before&func=f").await;
    let uid = ops[0]["uid"].as_str().unwrap().to_string();
    let detail = msgpack_get::<serde_json::Value>(
        &server, &format!("/api/ops/{uid}")).await;
    assert_eq!(detail["uid"], uid.as_str());
    assert_eq!(detail["func"], "f");
    assert!(detail["name"].as_str().unwrap().len() > 0);
    assert!(detail["region_path"].is_array());
}

#[tokio::test]
async fn op_detail_respects_pass_and_side_and_404s_when_absent() {
    let (server, _dir) = full_fixture_server().await;
    let ops = msgpack_get::<Vec<serde_json::Value>>(
        &server, "/api/passes/2/ops?side=before&func=f").await;
    let uid = ops[0]["uid"].as_str().unwrap().to_string();
    // valid explicit occurrence
    let detail = msgpack_get::<serde_json::Value>(
        &server, &format!("/api/ops/{uid}?pass=2&side=before")).await;
    assert_eq!(detail["pass_id"], 2);
    // malformed uid is 400, unknown-but-valid uid is 404
    assert_eq!(status_of(&server, "/api/ops/not-a-uid").await, 400);
    assert_eq!(status_of(&server, "/api/ops/op1.Zg.2.b.999").await, 404);
}
```

- [ ] **Step 2: Run, expect FAIL:** `cargo test -p server --test api op_detail`

- [ ] **Step 3: Implement** in `api.rs`:

```rust
#[derive(Deserialize)]
pub(crate) struct OpDetailQuery {
    pass: Option<i64>,
    side: Option<String>,
}

fn truncate_attrs(mut text: String) -> String {
    const MAX: usize = 2048;
    if text.len() > MAX {
        let mut cut = MAX;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        text.push('…');
    }
    text
}

pub(crate) async fn op_detail(
    State(state): State<ServerState>,
    Path(uid): Path<String>,
    Query(query): Query<OpDetailQuery>,
) -> Result<Msgpack<OpDetailDto>, ApiError> {
    let uid = engine::OpUid::parse(&uid)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let anchor = uid
        .parse_anchor()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let side = match query.side.as_deref() {
        None => anchor.side,
        Some("before") => engine::SnapshotSide::Before,
        Some("after") => engine::SnapshotSide::After,
        Some(_) => return Err(ApiError::bad_request("side must be 'before' or 'after'")),
    };
    let pass_id = query.pass.unwrap_or(anchor.pass_id);

    let reader = open(&state)?;
    let resolved = crate::provenance::resolved_function(&state, &reader, &anchor.function)?
        .ok_or_else(|| ApiError::not_found(format!("function {:?} not found", anchor.function)))?;
    let timeline = state
        .cache
        .timeline(&anchor.function)
        .ok_or_else(|| ApiError::not_found(format!("function {:?} not found", anchor.function)))?;
    let stage_index = timeline
        .iter()
        .position(|stage| stage.pass_id == pass_id)
        .ok_or_else(|| ApiError::not_found(format!("pass {pass_id} is not an executable leaf")))?;
    // Find this uid's occurrence at the requested stage/side.
    let (key, _) = resolved
        .selectable
        .iter()
        .find(|(key, op)| {
            key.stage_index == stage_index && key.side == side && op.uid == uid
        })
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "operation UID {} has no occurrence at pass {pass_id}",
                uid.as_str()
            ))
        })?;
    let stage = &timeline[stage_index];
    let snapshot = match side {
        engine::SnapshotSide::Before => stage.before.as_ref(),
        engine::SnapshotSide::After => stage.after.as_ref(),
    }
    .ok_or_else(|| ApiError::not_found(format!("pass {pass_id} has no such snapshot")))?;
    let op = &snapshot.module.ops[key.op_idx];
    Ok(Msgpack(OpDetailDto {
        uid: uid.as_str().to_string(),
        func: anchor.function,
        pass_id,
        side: if side == engine::SnapshotSide::Before { "before" } else { "after" }.to_string(),
        op_idx: op.idx,
        name: op.name.clone(),
        results: op.results.clone(),
        operands: op.operands.clone(),
        result_types: op.result_types.clone(),
        attr_summary: truncate_attrs(op.attr_summary.clone()),
        location: op.location.clone(),
        region_path: op.region_path.clone(),
        line_start: op.line_start,
        line_end: op.line_end,
        opaque: op.opaque,
    }))
}
```

Route in `lib.rs` **before** the history route: `.route("/ops/{uid}", get(api::op_detail))` (axum treats `/ops/{uid}` and `/ops/{uid}/history` as distinct — order doesn't matter, but keep them adjacent).

- [ ] **Step 4: Run, expect PASS:** `cargo test -p server`

- [ ] **Step 5: Commit**

```sh
git add crates/server
git commit -m "feat(server): add op inspector detail endpoint"
```

---

### Task 5: UI — api client for search + op detail

**Files:**
- Modify: `ui/src/api.ts`
- Test: `ui/src/api.test.ts`

**Interfaces:**
- Consumes: Task 3/4 endpoints.
- Produces:

```ts
export interface SearchResult {
  pass_id: number; side: IrSide; func: string; op_idx: number
  name: string; line_start: number; line_end: number; excerpt: string
}
export interface OpDetail {
  uid: string; func: string; pass_id: number; side: IrSide; op_idx: number
  name: string; results: string[]; operands: string[]; result_types: string[]
  attr_summary: string; location: string | null; region_path: number[]
  line_start: number; line_end: number; opaque: boolean
}
// on the exported `api` object:
searchOps(q: string, passId: number, scope: 'pass' | 'pipeline', side?: IrSide): Promise<SearchResult[]>
opDetail(uid: string, passId?: number, side?: IrSide): Promise<OpDetail>
```

- [ ] **Step 1: Write failing tests** in `ui/src/api.test.ts` following its existing fetch-mock pattern (msgpack-encoded response bodies):

```ts
it('searchOps encodes query params and decodes results', async () => {
  mockMsgpackResponse([{ pass_id: 2, side: 'after', func: 'f', op_idx: 1, name: 'arith.addi', line_start: 3, line_end: 3, excerpt: 'arith.addi' }])
  const results = await api.searchOps('addi', 2, 'pipeline')
  expect(fetchMock).toHaveBeenCalledWith(
    '/api/search?q=addi&pass=2&scope=pipeline', expect.anything())
  expect(results[0].name).toBe('arith.addi')
})

it('opDetail hits /api/ops/{uid} with optional pass/side', async () => {
  mockMsgpackResponse({ uid: 'op1.Zg.2.b.0', func: 'f', pass_id: 2, side: 'before', op_idx: 0, name: 'x', results: [], operands: [], result_types: [], attr_summary: '', location: null, region_path: [], line_start: 1, line_end: 1, opaque: false })
  await api.opDetail('op1.Zg.2.b.0', 2, 'before')
  expect(fetchMock).toHaveBeenCalledWith(
    '/api/ops/op1.Zg.2.b.0?pass=2&side=before', expect.anything())
})
```

Match the mock helper names already used in `api.test.ts`.

- [ ] **Step 2: Run, expect FAIL:** `npx vitest run src/api.test.ts`

- [ ] **Step 3: Implement** in `api.ts` next to the existing msgpack fetchers (reuse its `fetchMsgpack`-style helper):

```ts
searchOps: (q: string, passId: number, scope: 'pass' | 'pipeline', side?: IrSide) => {
  const params = new URLSearchParams({ q, pass: String(passId), scope })
  if (side) params.set('side', side)
  return fetchMsgpack<SearchResult[]>(`/api/search?${params}`)
},
opDetail: (uid: string, passId?: number, side?: IrSide) => {
  const params = new URLSearchParams()
  if (passId !== undefined) params.set('pass', String(passId))
  if (side) params.set('side', side)
  const suffix = params.size > 0 ? `?${params}` : ''
  return fetchMsgpack<OpDetail>(`/api/ops/${encodeURIComponent(uid)}${suffix}`)
},
```

Note: `URLSearchParams` serializes in insertion order — insert `q`, `pass`, `scope`, then `side` to match the test's expected URL.

- [ ] **Step 4: Run, expect PASS:** `npx vitest run src/api.test.ts && npm run typecheck`

- [ ] **Step 5: Commit**

```sh
git add ui/src/api.ts ui/src/api.test.ts
git commit -m "feat(ui): add search and op-detail api clients"
```

---

### Task 6: UI — Inspector panel (Structure + History tabs), retire History mode

**Files:**
- Create: `ui/src/components/InspectorPanel.tsx`
- Modify: `ui/src/store.ts`, `ui/src/App.tsx`, `ui/src/components/Toolbar.tsx`, `ui/src/styles.css`
- Test: Create `ui/src/components/InspectorPanel.test.tsx`; modify `ui/src/components/Toolbar.test.tsx`, `ui/src/store.test.ts`, `ui/e2e/graph-diff.spec.ts`

**Interfaces:**
- Consumes: `api.opDetail`, existing `api.opHistory`, `HistoryView`.
- Produces store shape (later tasks rely on these exact names):

```ts
type ViewMode = 'text' | 'graph'                    // 'history' removed
inspectorOpen: boolean
inspectorTab: 'structure' | 'history'
opDetail: OpDetail | null
selectOp: (uid: string) => Promise<void>            // now opens the inspector
openInspector: (tab: 'structure' | 'history') => void
closeInspector: () => void
```

`selectOp(uid)` sets `{ selectedOpUid: uid, inspectorOpen: true }`, fetches history **and** `api.opDetail(uid, selectedPassId ?? undefined)` in parallel, and stores both (guarding on `selectedOpUid === uid` like the current code). `viewHistoryStep(passId)` keeps working but now just calls `selectPass` and leaves the inspector open. On `selectPass`, if an op is selected, re-fetch `opDetail(uid, newPassId)` and swallow a 404 by keeping the previous detail with a `detailStale: boolean` flag set.

- [ ] **Step 1: Write failing component test** `ui/src/components/InspectorPanel.test.tsx`:

```tsx
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, expect, it } from 'vitest'
import { useViewerStore } from '../store'
import { InspectorPanel } from './InspectorPanel'

const detail = {
  uid: 'op1.Zg.2.b.0', func: 'f', pass_id: 2, side: 'before' as const, op_idx: 0,
  name: 'arith.addi', results: ['%r'], operands: ['%a', '%b'], result_types: ['i32'],
  attr_summary: '{fast}', location: 'unknown', region_path: [0, 0],
  line_start: 3, line_end: 3, opaque: false,
}

beforeEach(() => {
  useViewerStore.setState({
    ...useViewerStore.getState(),
    inspectorOpen: true, inspectorTab: 'structure',
    selectedOpUid: detail.uid, opDetail: detail, history: null, detailStale: false,
  })
})
afterEach(cleanup)

it('renders structure fields and switches to history tab', () => {
  render(<InspectorPanel />)
  expect(screen.getByText('arith.addi')).toBeInTheDocument()
  expect(screen.getByText('%a')).toBeInTheDocument()
  expect(screen.getByText('i32')).toBeInTheDocument()
  expect(screen.getByText('{fast}')).toBeInTheDocument()
  fireEvent.click(screen.getByRole('tab', { name: 'History' }))
  expect(useViewerStore.getState().inspectorTab).toBe('history')
})

it('close button clears inspectorOpen', () => {
  render(<InspectorPanel />)
  fireEvent.click(screen.getByRole('button', { name: 'Close inspector' }))
  expect(useViewerStore.getState().inspectorOpen).toBe(false)
})
```

- [ ] **Step 2: Run, expect FAIL:** `npx vitest run src/components/InspectorPanel.test.tsx`

- [ ] **Step 3: Implement.** Store changes in `store.ts`: remove `'history'` from `ViewMode`; add `inspectorOpen: false`, `inspectorTab: 'structure' as const`, `opDetail: null`, `detailStale: false` to `initialState`; implement `openInspector`/`closeInspector`; rewrite `selectOp`:

```ts
selectOp: async (uid) => {
  set({ selectedOpUid: uid, history: null, opDetail: null, detailStale: false, inspectorOpen: true, error: null })
  const passId = get().selectedPassId
  try {
    const [history, detail] = await Promise.all([
      api.opHistory(uid),
      api.opDetail(uid, passId ?? undefined),
    ])
    if (get().selectedOpUid === uid) set({ history, opDetail: detail })
  } catch (error) {
    if (get().selectedOpUid === uid) {
      set({ error: error instanceof Error ? error.message : String(error) })
    }
  }
},
```

`InspectorPanel.tsx`:

```tsx
import { useViewerStore } from '../store'
import { HistoryView } from './HistoryView'

export function InspectorPanel() {
  const { inspectorTab, opDetail, detailStale, history, openInspector, closeInspector, viewHistoryStep } = useViewerStore()
  return (
    <aside className="inspector" aria-label="Operation inspector">
      <header className="inspector-header">
        <div role="tablist" aria-label="Inspector tabs">
          <button role="tab" aria-selected={inspectorTab === 'structure'} onClick={() => openInspector('structure')}>Structure</button>
          <button role="tab" aria-selected={inspectorTab === 'history'} onClick={() => openInspector('history')}>History</button>
        </div>
        <button type="button" aria-label="Close inspector" onClick={closeInspector}>×</button>
      </header>
      {inspectorTab === 'history' ? (
        <HistoryView history={history} onViewIr={viewHistoryStep} />
      ) : !opDetail ? (
        <div className="status">Loading operation…</div>
      ) : (
        <dl className="op-structure">
          {detailStale && <div className="status">Not present in this pass — showing last known occurrence.</div>}
          <dt>Operation</dt><dd><code>{opDetail.name}</code></dd>
          {opDetail.results.length > 0 && (<><dt>Results</dt><dd>{opDetail.results.map((r) => <code key={r}>{r}</code>)}</dd></>)}
          {opDetail.operands.length > 0 && (<><dt>Operands</dt><dd>{opDetail.operands.map((o) => <code key={o}>{o}</code>)}</dd></>)}
          {opDetail.result_types.length > 0 && (<><dt>Types</dt><dd>{opDetail.result_types.map((t) => <code key={t}>{t}</code>)}</dd></>)}
          {opDetail.attr_summary && (<><dt>Attributes</dt><dd><code className="attrs">{opDetail.attr_summary}</code></dd></>)}
          {opDetail.location && (<><dt>Location</dt><dd><code>{opDetail.location}</code></dd></>)}
          <dt>Region path</dt><dd><code>{opDetail.region_path.join(' / ') || '(top level)'}</code></dd>
          <dt>Lines</dt><dd>{opDetail.line_start}–{opDetail.line_end} ({opDetail.side})</dd>
        </dl>
      )}
    </aside>
  )
}
```

`App.tsx`: drop the `viewMode === 'history'` branch; render `{inspectorOpen && <InspectorPanel />}` beside the viewer pane (flex row). `Toolbar.tsx`: delete the History button and the `'h'` key's `setViewMode('history')` (make `'h'` call `openInspector('history')` when an op is selected); drop `viewMode === 'history'` from the Diff-disable condition. Update `Toolbar.test.tsx`/`store.test.ts` accordingly (any test referencing view mode `'history'` moves to `inspectorOpen`/`inspectorTab`). Add minimal `.inspector` styles to `styles.css` (fixed 320px column, scrollable).

- [ ] **Step 4: Update the e2e spec.** In `ui/e2e/graph-diff.spec.ts` the flow after pressing Enter on a graph node now asserts the inspector: replace the heading assertion block with

```ts
await expect(page.getByRole('tab', { name: 'History' })).toBeVisible()
await page.getByRole('tab', { name: 'History' }).click()
await expect(page.getByText('AddIToShift')).toBeVisible()
await expect(page.getByText('listener').first()).toBeVisible()
await page.getByRole('button', { name: 'View IR' }).first().click()
await expect(page.locator('.editor-grid')).toBeVisible()
```

- [ ] **Step 5: Run everything, expect PASS:**

```sh
npx vitest run && npm run typecheck && npm run build && npx playwright test
```

- [ ] **Step 6: Commit**

```sh
git add ui
git commit -m "feat(ui): inspector panel with structure and history tabs"
```

---

### Task 7: UI — command palette (cmdk) + pass stepping keys

**Files:**
- Create: `ui/src/components/CommandPalette.tsx`, `ui/src/useGlobalKeys.ts`
- Modify: `ui/src/App.tsx`, `ui/src/store.ts` (add `stepPass`, `paletteOpen`), `ui/package.json` (`npm install cmdk`)
- Test: Create `ui/src/components/CommandPalette.test.tsx`, extend `ui/src/store.test.ts`

**Interfaces:**
- Consumes: `api.searchOps`, store `selectPass`/`selectFunc`/`selectOp`/`toggleDiff`/`setViewMode`, `api.selectableOps` for uid resolution on search-result click.
- Produces store additions:

```ts
paletteOpen: boolean
setPaletteOpen: (open: boolean) => void
stepPass: (direction: 1 | -1) => Promise<void>   // executable leaves only, clamped at ends
jumpToSearchResult: (r: SearchResult) => Promise<void>
```

`jumpToSearchResult`: `await selectPass(r.pass_id)`, `selectFunc(r.func)` if different, then resolve the uid via `api.selectableOps(r.pass_id, r.side, r.func)` matching `op.op_idx === r.op_idx`, and `selectOp(uid)` when found (silently skip when not — v1 traces without provenance still navigate to the pass).

- [ ] **Step 1: `npm install cmdk`** (adds ~1 dependency; verify `npm run build` still passes).

- [ ] **Step 2: Write failing store test** in `ui/src/store.test.ts` (mock `api` the way that file already does):

```ts
it('stepPass walks executable leaves in order and clamps at the ends', async () => {
  // seed passesById/roots with the demo tree the file already uses:
  // Pipeline(1) -> [canonicalize(2), dce(3), set-attr(4)]
  useViewerStore.setState({ ...state, selectedPassId: 2 })
  await useViewerStore.getState().stepPass(1)
  expect(useViewerStore.getState().selectedPassId).toBe(3)
  await useViewerStore.getState().stepPass(-1)
  await useViewerStore.getState().stepPass(-1) // clamped
  expect(useViewerStore.getState().selectedPassId).toBe(2)
})
```

- [ ] **Step 3: Implement `stepPass`** in `store.ts`:

```ts
stepPass: async (direction) => {
  const { roots, selectedPassId } = get()
  const leaves: PassNode[] = []
  const walk = (nodes: PassNode[]) => {
    for (const node of nodes) {
      if (node.children.length === 0) leaves.push(node)
      else walk(node.children)
    }
  }
  walk(roots)
  const index = leaves.findIndex((leaf) => leaf.id === selectedPassId)
  const next = leaves[index === -1 ? 0 : Math.min(Math.max(index + direction, 0), leaves.length - 1)]
  if (next && next.id !== selectedPassId) await get().selectPass(next.id)
},
```

- [ ] **Step 4: Write failing palette test** `ui/src/components/CommandPalette.test.tsx`:

```tsx
it('lists passes and runs actions', async () => {
  render(<CommandPalette />)
  // palette is controlled by store
  useViewerStore.setState({ ...useViewerStore.getState(), paletteOpen: true })
  fireEvent.change(screen.getByPlaceholderText('Search passes, functions, ops…'), { target: { value: 'canonic' } })
  expect(await screen.findByText('canonicalize')).toBeInTheDocument()
  fireEvent.click(screen.getByText('canonicalize'))
  expect(useViewerStore.getState().selectedPassId).toBe(2)
  expect(useViewerStore.getState().paletteOpen).toBe(false)
})
```

(Seed the store with the demo pass tree in `beforeEach`, mock `api.searchOps` to return `[]`.)

- [ ] **Step 5: Implement `CommandPalette.tsx`** with `cmdk`'s `Command.Dialog`:

```tsx
import { Command } from 'cmdk'
import { useEffect, useState } from 'react'
import { api, type SearchResult } from '../api'
import { useViewerStore } from '../store'

export function CommandPalette() {
  const { paletteOpen, setPaletteOpen, roots, functions, selectedPassId, selectPass, selectFunc, toggleDiff, setViewMode, jumpToSearchResult } = useViewerStore()
  const [query, setQuery] = useState('')
  const [ops, setOps] = useState<SearchResult[]>([])

  useEffect(() => {
    if (!paletteOpen || query.trim().length < 2 || selectedPassId === null) {
      setOps([])
      return
    }
    const handle = setTimeout(() => {
      api.searchOps(query, selectedPassId, 'pipeline').then(setOps).catch(() => setOps([]))
    }, 150)
    return () => clearTimeout(handle)
  }, [paletteOpen, query, selectedPassId])

  const leaves: { id: number; name: string }[] = []
  const walk = (nodes: typeof roots) => {
    for (const node of nodes) {
      if (node.children.length === 0) leaves.push({ id: node.id, name: node.name })
      else walk(node.children)
    }
  }
  walk(roots)
  const close = () => setPaletteOpen(false)

  return (
    <Command.Dialog open={paletteOpen} onOpenChange={setPaletteOpen} label="Command palette">
      <Command.Input value={query} onValueChange={setQuery} placeholder="Search passes, functions, ops…" />
      <Command.List>
        <Command.Empty>No results.</Command.Empty>
        <Command.Group heading="Passes">
          {leaves.map((leaf) => (
            <Command.Item key={leaf.id} onSelect={() => { void selectPass(leaf.id); close() }}>{leaf.name}</Command.Item>
          ))}
        </Command.Group>
        <Command.Group heading="Functions">
          {functions.map((func) => (
            <Command.Item key={func.name} onSelect={() => { selectFunc(func.name); close() }}>{func.name}</Command.Item>
          ))}
        </Command.Group>
        <Command.Group heading="Actions">
          <Command.Item onSelect={() => { setViewMode('text'); close() }}>View: Text</Command.Item>
          <Command.Item onSelect={() => { setViewMode('graph'); close() }}>View: Graph</Command.Item>
          <Command.Item onSelect={() => { toggleDiff(); close() }}>Toggle diff</Command.Item>
        </Command.Group>
        {ops.length > 0 && (
          <Command.Group heading="Operations">
            {ops.map((result) => (
              <Command.Item
                key={`${result.pass_id}-${result.side}-${result.op_idx}`}
                onSelect={() => { void jumpToSearchResult(result); close() }}
              >
                {result.excerpt} — {result.func}, pass {result.pass_id}:{result.line_start}
              </Command.Item>
            ))}
          </Command.Group>
        )}
      </Command.List>
    </Command.Dialog>
  )
}
```

Implement `jumpToSearchResult` in the store as specified in Interfaces. Global keys in `ui/src/useGlobalKeys.ts` (called once from `App`):

```ts
import { useEffect } from 'react'
import { useViewerStore } from './store'

export function useGlobalKeys() {
  const { setPaletteOpen, stepPass, closeInspector } = useViewerStore()
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null
      const typing = target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT')
      if ((event.metaKey || event.ctrlKey) && event.key === 'k') {
        event.preventDefault()
        setPaletteOpen(true)
      } else if (typing) {
        return
      } else if (event.key === '[') void stepPass(-1)
      else if (event.key === ']') void stepPass(1)
      else if (event.key === '/') { event.preventDefault(); setPaletteOpen(true) }
      else if (event.key === 'Escape') closeInspector()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [setPaletteOpen, stepPass, closeInspector])
}
```

Render `<CommandPalette />` and call `useGlobalKeys()` in `App`. Add basic palette styles to `styles.css` (`[cmdk-dialog]` centered overlay).

- [ ] **Step 6: Run everything, expect PASS:** `npx vitest run && npm run typecheck && npm run build && npx playwright test`

- [ ] **Step 7: Commit**

```sh
git add ui
git commit -m "feat(ui): cmdk command palette, op search, and pass stepping keys"
```

---

### Task 8: UI — dockview workspace with persisted layout

**Files:**
- Create: `ui/src/Workspace.tsx`
- Modify: `ui/src/App.tsx`, `ui/src/styles.css`, `ui/package.json` (`npm install dockview`)
- Test: Create `ui/src/Workspace.test.tsx`

**Interfaces:**
- Consumes: `Timeline`, `Toolbar` + viewer pane, `InspectorPanel` as dockview panels; store `inspectorOpen`.
- Produces: `Workspace` component replacing `App`'s `<main>` grid; layout JSON persisted under localStorage key `mlir-viewer-layout-v1`; `resetLayout()` exported from `Workspace` module and wired as a palette action ("Reset layout").

Panels: `timeline` (left, ~280px), `viewer` (center: Toolbar + Text/Graph pane), `inspector` (right, ~340px, added/removed as `inspectorOpen` changes). Layout is saved on dockview's `onDidLayoutChange` (debounced 250 ms) and restored via `fromJSON` on mount; a corrupt/mismatched saved layout falls back to the default (wrap `fromJSON` in try/catch, clear the key).

- [ ] **Step 1: `npm install dockview`** and import `dockview/dist/styles/dockview.css` in `main.tsx`.

- [ ] **Step 2: Write failing test** `ui/src/Workspace.test.tsx`:

```tsx
it('persists layout to localStorage and resets it', async () => {
  render(<Workspace />)
  await screen.findByLabelText('Pass timeline')      // timeline panel mounted
  // saving happens debounced after layout init
  await waitFor(() => expect(localStorage.getItem('mlir-viewer-layout-v1')).toBeTruthy())
  resetLayout()
  expect(localStorage.getItem('mlir-viewer-layout-v1')).toBeNull()
})
```

(Seed the store to `status: 'ready'` with the demo pass tree in `beforeEach`; mock api calls to resolve empty like `store.test.ts` does. If dockview needs a sized container in jsdom, wrap the render in a fixed-size div and stub `ResizeObserver` in `src/test/setup.ts` when missing.)

- [ ] **Step 3: Implement `Workspace.tsx`:**

```tsx
import { DockviewReact, type DockviewReadyEvent, type DockviewApi, type IDockviewPanelProps } from 'dockview'
import { useEffect, useRef } from 'react'
import { Timeline } from './components/Timeline'
import { Toolbar } from './components/Toolbar'
import { IrViewer } from './components/IrViewer'
import { GraphView } from './components/GraphView'
import { InspectorPanel } from './components/InspectorPanel'
import { useViewerStore } from './store'

const LAYOUT_KEY = 'mlir-viewer-layout-v1'
let dockApi: DockviewApi | null = null

export function resetLayout() {
  localStorage.removeItem(LAYOUT_KEY)
  if (dockApi) buildDefaultLayout(dockApi)
}

function buildDefaultLayout(api: DockviewApi) {
  api.clear()
  api.addPanel({ id: 'timeline', component: 'timeline', title: 'Timeline' })
  api.addPanel({ id: 'viewer', component: 'viewer', title: 'IR', position: { referencePanel: 'timeline', direction: 'right' } })
  api.getPanel('timeline')?.api.setSize({ width: 280 })
}

function TimelinePanel(_: IDockviewPanelProps) {
  const { roots, selectedPassId, selectPass } = useViewerStore()
  return (
    <nav aria-label="Pass timeline" className="panel-scroll">
      <Timeline roots={roots} selectedPassId={selectedPassId} onSelect={(id) => void selectPass(id)} />
    </nav>
  )
}

function ViewerPanel(_: IDockviewPanelProps) {
  const { passesById, selectedPassId, before, after, diff, graph, diffEnabled, viewMode, selectableBefore, selectableAfter, selectOp } = useViewerStore()
  const selectedPass = selectedPassId === null ? null : passesById[selectedPassId]
  const diffAvailable = Boolean(selectedPass && selectedPass.ir_before !== null && selectedPass.ir_after !== null)
  return (
    <div className="viewer-pane">
      <Toolbar diffAvailable={diffAvailable} />
      {viewMode === 'graph' ? (
        <GraphView graph={graph} diffEnabled={diffEnabled} onSelectOp={selectOp} />
      ) : (
        <IrViewer before={before} after={after} diff={diffEnabled ? diff : null} beforeOps={selectableBefore} afterOps={selectableAfter} onSelectOp={selectOp} />
      )}
    </div>
  )
}

function InspectorDockPanel(_: IDockviewPanelProps) {
  return <InspectorPanel />
}

const components = { timeline: TimelinePanel, viewer: ViewerPanel, inspector: InspectorDockPanel }

export function Workspace() {
  const inspectorOpen = useViewerStore((state) => state.inspectorOpen)
  const saveTimer = useRef<number | undefined>(undefined)

  const onReady = (event: DockviewReadyEvent) => {
    dockApi = event.api
    const saved = localStorage.getItem(LAYOUT_KEY)
    let restored = false
    if (saved) {
      try {
        event.api.fromJSON(JSON.parse(saved))
        restored = true
      } catch {
        localStorage.removeItem(LAYOUT_KEY)
      }
    }
    if (!restored) buildDefaultLayout(event.api)
    event.api.onDidLayoutChange(() => {
      window.clearTimeout(saveTimer.current)
      saveTimer.current = window.setTimeout(() => {
        localStorage.setItem(LAYOUT_KEY, JSON.stringify(event.api.toJSON()))
      }, 250)
    })
  }

  useEffect(() => {
    if (!dockApi) return
    const existing = dockApi.getPanel('inspector')
    if (inspectorOpen && !existing) {
      dockApi.addPanel({ id: 'inspector', component: 'inspector', title: 'Inspector', position: { referencePanel: 'viewer', direction: 'right' } })
      dockApi.getPanel('inspector')?.api.setSize({ width: 340 })
    } else if (!inspectorOpen && existing) {
      dockApi.removePanel(existing)
    }
  }, [inspectorOpen])

  return <DockviewReact className="dockview-theme-dark workspace" components={components} onReady={onReady} />
}
```

`App.tsx` replaces its `<main>` markup with `<Workspace />` (keep the header, status states, palette, and `useGlobalKeys`). When the user closes the inspector panel via dockview's own close button, sync the store: subscribe in `onReady` to `event.api.onDidRemovePanel((panel) => { if (panel.id === 'inspector') useViewerStore.getState().closeInspector() })`. Add the "Reset layout" `Command.Item` in `CommandPalette.tsx` calling `resetLayout()`. Note: `InspectorPanel`'s own `×` button already drives `inspectorOpen`, which the effect above turns into panel removal.

- [ ] **Step 4: Run everything; fix e2e selectors if the dockview wrapper changed roles** (the `nav[aria-label="Pass timeline"]` and toolbar/inspector roles are preserved by design):

```sh
npx vitest run && npm run typecheck && npm run build && npx playwright test
```

- [ ] **Step 5: Commit**

```sh
git add ui
git commit -m "feat(ui): dockview workspace with persisted layout"
```

---

### Task 9: E2E — M5 journey + spec/status updates

**Files:**
- Create: `ui/e2e/inspector-search.spec.ts`
- Modify: `docs/superpowers/specs/2026-07-06-m5-inspector-search-design.md` (status → Implemented; note the two deviations: linear-scan search instead of a stored index, bottom search dock folded into the palette)

**Interfaces:** none new — this task locks the whole milestone behind one browser journey.

- [ ] **Step 1: Write the e2e spec** `ui/e2e/inspector-search.spec.ts`:

```ts
import { expect, test } from '@playwright/test'

test('inspector, palette search, and layout persistence', async ({ page }) => {
  const consoleErrors: string[] = []
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text())
  })
  await page.goto('/')
  await page.getByText('canonicalize').click()

  // Select an op from the text view -> inspector opens on structure
  await page.locator('.cm-line.selectable-op').first().click()
  await expect(page.getByRole('tab', { name: 'Structure' })).toBeVisible()
  await expect(page.locator('.op-structure')).toBeVisible()

  // History tab shows the provenance chain
  await page.getByRole('tab', { name: 'History' }).click()
  await expect(page.getByText(/replaced|unchanged|modified|disappeared/).first()).toBeVisible()

  // Palette: search an op pipeline-wide and jump to it
  await page.keyboard.press('ControlOrMeta+k')
  await page.getByPlaceholder('Search passes, functions, ops…').fill('shli')
  await page.getByText(/arith\.shli/).first().click()
  await expect(page.locator('.op-structure')).toContainText('arith.shli')

  // Pass stepping
  await page.keyboard.press('Escape')
  await page.keyboard.press(']')
  await expect(page.getByRole('button', { name: /dce/ })).toHaveAttribute('aria-pressed', 'true')

  // Layout persistence across reload
  const saved = await page.evaluate(() => localStorage.getItem('mlir-viewer-layout-v1'))
  expect(saved).toBeTruthy()
  await page.reload()
  await expect(page.getByText('canonicalize')).toBeVisible()

  expect(consoleErrors).toEqual([])
})
```

Adjust the text-view op-line selector to whatever class `IrViewer` gives selectable lines (check `IrViewer.tsx`; if lines aren't clickable outside diff mode, click the graph node path instead, mirroring `graph-diff.spec.ts`). Adjust the timeline `aria-pressed` assertion to the Timeline component's actual selected-state attribute.

- [ ] **Step 2: Run the full gate 3× to guard against flakes:**

```sh
npm run build && for i in 1 2 3; do npx playwright test || break; done
```

- [ ] **Step 3: Update the spec status + deviations, run the complete verification:**

```sh
cargo test --workspace && cd ui && npx vitest run && npm run typecheck
```

- [ ] **Step 4: Commit**

```sh
git add ui docs
git commit -m "test(e2e): cover M5 inspector, palette search, and layout persistence"
```

---

## Self-Review Notes

- **Spec coverage:** Inspector endpoint+panel (T4/T6), search endpoint (T2/T3), palette (T7), docking+persistence (T8), `Disappeared` step (T1), keyboard `[`/`]`/`cmd-k`/`/`/`Escape` (T7), empty-history UX (T1 makes it unreachable; HistoryView keeps its empty-state text as a safety net), e2e journey (T9). Deviations recorded in T9: search is a budgeted linear scan over the cached parse (the "index" is the parse cache), and the bottom search-results dock is folded into the palette — both to be noted in the spec on completion.
- **Types:** `SearchResult`/`SearchResultDto` field lists match; `OpDetail`/`OpDetailDto` match; store names (`inspectorOpen`, `inspectorTab`, `opDetail`, `detailStale`, `paletteOpen`, `stepPass`, `jumpToSearchResult`) are used consistently across T6–T9.
- **Ordering:** every task leaves all suites green; T6 updates the existing e2e in the same commit that removes the History mode.
