# M4b Op History and Provenance Implementation Plan

**Status:** Complete — implemented and verified on 2026-07-05.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Resolve M4a identity events and fingerprint fallbacks into deterministic pipeline-wide op histories, expose them through HTTP, and add selectable Text/Graph operations with a dedicated History view.

**Architecture:** The engine builds a provenance graph from normalized per-function pass stages, uses union-find to support N→1 merges, and assigns one deterministic function-scoped UID per connected component. The server adapts trace schema v1/v2 into engine inputs and caches resolved functions; the React client consumes selectable-op and history endpoints without loading a whole trace.

**Tech Stack:** Rust 2021 (`serde`, `base64`, Axum, MessagePack), React 19, TypeScript 6, Zustand, CodeMirror 6, canvas graph renderer, Vitest, Playwright.

---

## File structure

- Create `crates/engine/src/provenance.rs`: provenance input/output types, UID codec, resolver, merge-aware component construction.
- Create `crates/engine/tests/provenance.rs`: pure resolver contract tests.
- Modify `crates/engine/src/diff.rs`: expose deterministic fingerprint scores used by provenance confidence.
- Modify `crates/engine/src/graph.rs`: retain op index/side metadata and optional UID on graph nodes.
- Modify `crates/engine/src/lib.rs`: re-export provenance interfaces.
- Create `crates/server/src/provenance.rs`: pass-tree flattening, op-index normalization, trace-to-engine adapter.
- Modify `crates/server/src/cache.rs`: bounded resolved-function cache.
- Modify `crates/server/src/api.rs`: selectable-op/history handlers and graph UID decoration.
- Modify `crates/server/src/lib.rs`: register M4b routes.
- Modify `crates/server/tests/api.rs`: Full/v1 API, errors, graph UID, cache behavior.
- Modify `ui/src/api.ts`: M4b wire types and fetchers.
- Modify `ui/src/store.ts`: selected UID, selectable ops, history loading/navigation.
- Modify `ui/src/components/IrViewer.tsx`: operation-line selection.
- Modify `ui/src/components/GraphView.tsx`: graph-node UID selection.
- Create `ui/src/components/HistoryView.tsx`: merge-aware history timeline.
- Create `ui/src/components/HistoryView.test.tsx`: exact/inferred/merge rendering and navigation.
- Modify `ui/src/components/Toolbar.tsx`, `ui/src/App.tsx`, and `ui/src/styles.css`: History mode.
- Modify UI unit tests and `ui/e2e/graph-diff.spec.ts`: selection and end-to-end history flow.

## Global constraints

- Keep trace files read-only. No schema v3, migration, or sidecar.
- v1 and Text traces must resolve through fingerprints with empty identity tables.
- UIDs use `op1.{unpadded-base64url-function}.{pass_id}.{b|a}.{function_ordinal}` and clients treat them as opaque.
- `ParsedOp.idx` remains module-global. `function_ordinal` is the position in `FunctionScope::ops`.
- Explicit relations override fingerprints. Fingerprints never connect different op names.
- One old token with multiple replacement successors chooses the lowest-sequence valid event. Multiple old tokens may merge into one new token/component.
- Every task follows RED → GREEN → refactor and commits independently.

### Task 1: Expose scored fingerprint matching

**Files:**
- Modify: `crates/engine/src/diff.rs`
- Modify: `crates/engine/src/lib.rs`
- Test: `crates/engine/tests/diff.rs`

- [ ] **Step 1: Write failing score tests**

Append tests that construct `OpFingerprint` values directly:

```rust
use engine::{fingerprint_score, OpFingerprint};

#[test]
fn fingerprint_score_is_normalized_and_rejects_different_names() {
    let base = OpFingerprint {
        op_name: "arith.addi".into(),
        result_types: vec!["i32".into()],
        operand_count: 2,
        location: Some("file.mlir:1:2".into()),
    };
    assert_eq!(fingerprint_score(&base, &base), Some(100));
    let mut changed = base.clone();
    changed.op_name = "arith.muli".into();
    assert_eq!(fingerprint_score(&base, &changed), None);
}
```

- [ ] **Step 2: Run the focused test and observe RED**

Run: `cargo test -p engine --test diff fingerprint_score_is_normalized -- --exact`

Expected: compile failure because `fingerprint_score` is not exported.

- [ ] **Step 3: Replace the private score with the public normalized API**

```rust
pub fn fingerprint_score(before: &OpFingerprint, after: &OpFingerprint) -> Option<u16> {
    if before.op_name != after.op_name {
        return None;
    }
    let mut score = 50;
    if before.result_types == after.result_types { score += 25; }
    if before.operand_count == after.operand_count { score += 15; }
    if before.location.is_some() && before.location == after.location { score += 10; }
    Some(score)
}
```

Use `fingerprint_score(...).unwrap_or(0) as i32` in `GreedyFingerprintMatcher`, preserving its threshold and tie order. Re-export the function from `engine::lib`.

- [ ] **Step 4: Verify matcher behavior is unchanged**

Run: `cargo test -p engine --test diff`

Expected: all existing and new diff tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/engine/src/diff.rs crates/engine/src/lib.rs crates/engine/tests/diff.rs
git commit -m "refactor(engine): expose fingerprint confidence score"
```

### Task 2: Define provenance types and versioned UID codec

**Files:**
- Create: `crates/engine/src/provenance.rs`
- Modify: `crates/engine/src/lib.rs`
- Modify: `crates/engine/Cargo.toml`
- Modify: `Cargo.toml`
- Test: `crates/engine/tests/provenance.rs`

- [ ] **Step 1: Add RED tests for UID round-trip and punctuation**

```rust
use engine::{OpAnchor, OpUid, SnapshotSide};

#[test]
fn uid_round_trips_function_punctuation() {
    let anchor = OpAnchor {
        function: "dialect/f-with.dash".into(),
        pass_id: 42,
        side: SnapshotSide::After,
        function_ordinal: 7,
    };
    let uid = OpUid::from_anchor(&anchor);
    assert_eq!(uid.parse_anchor().unwrap(), anchor);
    assert!(!uid.as_str().contains('/'));
}

#[test]
fn uid_rejects_unknown_version_and_bad_base64() {
    assert!(OpUid::parse("op2.Zg.1.a.0").is_err());
    assert!(OpUid::parse("op1.!.1.a.0").is_err());
}
```

- [ ] **Step 2: Run and observe RED**

Run: `cargo test -p engine --test provenance uid_ -- --nocapture`

Expected: unresolved provenance imports.

- [ ] **Step 3: Add base64 dependency and core public types**

Add `base64 = "0.22"` to workspace dependencies and `base64.workspace = true` to engine. Define and serde-enable:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotSide { Before, After }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpAnchor {
    pub function: String,
    pub pass_id: i64,
    pub side: SnapshotSide,
    pub function_ordinal: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct OpUid(String);
```

Implement `from_anchor`, `parse`, `parse_anchor`, and `as_str` with `URL_SAFE_NO_PAD`. Parse exactly five dot-separated fields, require prefix `op1`, require side `b|a`, and reject decoded non-UTF-8.

- [ ] **Step 4: Add the complete normalized and response model**

Define `OccurrenceKey`, `SnapshotOps`, `TimelineStage`, `NormalizedIdentityEvent`, `NormalizedIdentityKind`, `EvidenceSource`, `HistoryEvidence`, `LinkConfidence`, `HistoryChange`, `OpOccurrence`, `HistoryStep`, `OpHistory`, `SelectableOp`, and `ResolvedFunction`. `ResolvedFunction` owns:

```rust
pub struct ResolvedFunction {
    pub function: String,
    pub selectable: HashMap<OccurrenceKey, SelectableOp>,
    pub histories: HashMap<OpUid, OpHistory>,
}
```

Use serde snake_case tags for all wire enums. Keep `TimelineStage` and normalization types non-serialized.

- [ ] **Step 5: Verify and commit**

Run: `cargo test -p engine --test provenance uid_`

```bash
git add Cargo.toml Cargo.lock crates/engine/Cargo.toml crates/engine/src/lib.rs crates/engine/src/provenance.rs crates/engine/tests/provenance.rs
git commit -m "feat(engine): add provenance types and deterministic op UID"
```

### Task 3: Resolve exact, inferred, and merged provenance components

**Files:**
- Modify: `crates/engine/src/provenance.rs`
- Test: `crates/engine/tests/provenance.rs`

- [ ] **Step 1: Add test builders and RED lifecycle tests**

Create `snapshot(side, ops, tokens)` and `stage(pass_id, before, after, events)`
helpers, then add these exact cases:

| Test | Input | Required assertions |
|---|---|---|
| `exact_replace_modify_erase_and_insert_form_expected_steps` | addi token 1 replaced by shli token 2; shli token 2 modified; muli token 3 erased; constant token 4 inserted | changes are `Replaced`, `Modified`, `Erased`, `Inserted`; every link is `Exact`; event sequence and pattern are retained |
| `missing_events_use_inferred_score_and_never_cross_names` | addi→addi with no events followed by addi→muli | first link is `Inferred { score: 100 }`; second pair produces separate components |
| `two_predecessors_replaced_by_one_token_share_uid_and_merge_steps` | addi tokens 10 and 11 both replaced by token 20 | all three occurrences have one UID; two ordered steps share the same after occurrence |
| `duplicate_old_successors_choose_lowest_event_sequence` | token 10→20 at seq 2 and token 10→21 at seq 3 | token 20 joins token 10; token 21 remains separate |

Each assertion must compare concrete `HistoryChange`, `EvidenceSource`, UID equality, and merge step order—not only counts.

- [ ] **Step 2: Run and observe RED**

Run: `cargo test -p engine --test provenance -- --nocapture`

Expected: `resolve_function` is absent.

- [ ] **Step 3: Implement relation collection**

Add:

```rust
pub fn resolve_function(function: &str, stages: &[TimelineStage]) -> ResolvedFunction
```

Allocate one node per `(stage_index, side, ParsedOp.idx)` occurrence. Collect explicit edges first, sorting events by `seq`. Reserve explicit old/new occurrences before calling `GreedyFingerprintMatcher` on remaining before/after ops. Score fallback edges with `fingerprint_score`.

For adjacent stages, add `SharedSnapshot` exact edges when the adapter marked equal blob IDs; otherwise fingerprint-match remaining prior-after/current-before occurrences.

- [ ] **Step 4: Implement union-find and stable component anchors**

Union every accepted edge. N→1 edges naturally merge components. For each root, sort occurrences by `(stage_index, side before-first, function_ordinal)` and use the first occurrence to construct `OpUid`. Build the occurrence→UID map before histories so selecting any predecessor returns the shared component UID.

- [ ] **Step 5: Build ordered history steps**

Emit one `HistoryStep` per accepted predecessor edge. Grouping is represented by repeated `pass_id` and identical `after` occurrence. Put the anchor branch first, then other predecessors by function ordinal. Emit insert/erase terminal steps for unmatched explicitly annotated occurrences. Retain all matching events in sequence in `evidence`.

- [ ] **Step 6: Verify engine contracts**

Run: `cargo test -p engine --test provenance && cargo test -p engine`

Expected: lifecycle, fallback, merge, deterministic conflict, and existing suites pass.

- [ ] **Step 7: Commit**

```bash
git add crates/engine/src/provenance.rs crates/engine/tests/provenance.rs
git commit -m "feat(engine): resolve merge-aware operation histories"
```

### Task 4: Normalize trace passes and op indexes in the server

**Files:**
- Create: `crates/server/src/provenance.rs`
- Modify: `crates/server/src/lib.rs`
- Test: `crates/server/tests/api.rs`

- [ ] **Step 1: Add RED adapter tests through the Full fixture**

Expose only a test helper inside the server module and assert the synthetic Full trace produces three leaf stages, mapped tokens on both sides, and exact replacement/erase/modify events for function `f`. Add a separate hand-built v1 trace assertion with empty tokens/events.

- [ ] **Step 2: Run and observe RED**

Run: `cargo test -p server provenance::tests -- --nocapture`

Expected: module/helper unresolved.

- [ ] **Step 3: Implement stable leaf flattening**

```rust
fn leaf_passes(nodes: &[PassNode], output: &mut Vec<PassNode>) {
    for node in nodes {
        if node.children.is_empty() { output.push(node.clone()); }
        else { leaf_passes(&node.children, output); }
    }
    output.sort_by_key(|pass| (pass.start_ns, pass.id.0));
}
```

Preserve tree order as the tie-breaker by recording traversal ordinal before sorting; do not use pass ID as the primary tie-breaker.

- [ ] **Step 4: Implement byte-span and ordinal mapping**

For real spans, precompute line-start byte offsets and map a row to the narrowest parsed statement whose byte interval contains `[byte_start, byte_end)`. For `byte_end == -1`, map the non-negative `byte_start` ordinal to `ParsedModule::ops[ordinal].idx`. Reject negative starts, invalid spans, and duplicate token mappings locally.

Filter the final `SnapshotOps.ops` and token map to `FunctionScope::ops`, while retaining module-global `ParsedOp.idx` values and computing function ordinals from scope order.

- [ ] **Step 5: Normalize identity rows**

Convert trace-format sides/kinds/sources into engine types. A token that failed index mapping remains absent; do not fail the whole timeline. Preserve `seq`, `pattern`, old token, and optional new token.

- [ ] **Step 6: Verify and commit**

Run: `cargo test -p server provenance::tests`

```bash
git add crates/server/src/provenance.rs crates/server/src/lib.rs crates/server/tests/api.rs
git commit -m "feat(server): normalize trace identity timelines"
```

### Task 5: Add bounded timeline and resolved-function caches

**Files:**
- Modify: `crates/server/src/cache.rs`
- Test: `crates/server/src/cache.rs`

- [ ] **Step 1: Write a RED eviction test**

Construct `EngineCache::with_capacities(2, 2)`, insert timelines `a`, `b`,
access `a`, insert `c`, and assert timeline `b` was evicted. Then insert resolved functions
`a` and `b` containing one history each, access `a`, insert one-history function
`c`, and assert `b` was evicted while `a` and `c` remain. The cache key is the
function name because one router serves one immutable trace.

- [ ] **Step 2: Run and observe RED**

Run: `cargo test -p server cache::tests::history_cache_evicts_oldest -- --exact`

- [ ] **Step 3: Implement deterministic LRU behavior**

Add a private `TimelineCache` holding `Arc<Vec<TimelineStage>>` with an
oldest-first capacity of 128 functions. Add a private `HistoryCache` with
`HashMap<String, Arc<ResolvedFunction>>`,
`VecDeque<String>`, `chain_count`, and `chain_capacity`. Cache hits move the key
to the back. On insertion, add `value.histories.len()` to `chain_count` and
evict whole least-recently-used functions until the retained total is at most
2,048 histories. If one function alone exceeds the bound, retain that function
alone so its selectable UID map remains usable. Never hold the mutex while
building a timeline or resolving a function.

- [ ] **Step 4: Verify and commit**

Run: `cargo test -p server cache::tests`

```bash
git add crates/server/src/cache.rs
git commit -m "feat(server): cache bounded resolved provenance"
```

### Task 6: Add selectable-op and history endpoints

**Files:**
- Modify: `crates/server/src/api.rs`
- Modify: `crates/server/src/lib.rs`
- Modify: `crates/server/src/provenance.rs`
- Test: `crates/server/tests/api.rs`

- [ ] **Step 1: Add RED endpoint tests**

Generate `write_full_demo_trace`, discover canonicalize pass ID, and request:

```text
/api/passes/{id}/ops?side=before&func=f
/api/ops/{percent_encoded_uid}/history
```

Decode MessagePack into `Vec<SelectableOp>` and `OpHistory`. Assert the
`arith.addi` UID resolves to a history containing `Replaced` followed by
`Modified`, both with exact evidence. Resolve the `arith.muli` UID separately
and assert it ends with `Erased`. Add malformed `op2.Zg.1.a.0` → 400 and
valid-but-absent `op1.Zg.999.a.0` → 404. Add a v1 fixture request whose history
contains `Inferred` evidence.

- [ ] **Step 2: Run and observe RED**

Run: `cargo test -p server --test api op_history -- --nocapture`

Expected: routes return 404.

- [ ] **Step 3: Implement one shared resolved-function loader**

```rust
fn resolved_function(
    state: &ServerState,
    reader: &TraceReader,
    function: &str,
) -> Result<Arc<engine::ResolvedFunction>, ApiError>
```

Check the resolved cache first. On a miss, reuse or build the normalized timeline
cache entry, call `engine::resolve_function`, insert the resolved result, and
return it. If no stage contains the function, return 404.

- [ ] **Step 4: Implement handlers and route registration**

Register:

```rust
.route("/passes/{id}/ops", get(api::selectable_ops))
.route("/ops/{uid}/history", get(api::op_history))
```

Validate side before trace work. `selectable_ops` filters the resolved occurrence map to pass/side and sorts by function ordinal. `op_history` parses `OpUid`, resolves its embedded function, then looks up the exact UID; distinguish syntax 400 from missing 404.

- [ ] **Step 5: Verify and commit**

Run: `cargo test -p server --test api`

```bash
git add crates/server/src/api.rs crates/server/src/lib.rs crates/server/src/provenance.rs crates/server/tests/api.rs
git commit -m "feat(server): expose selectable operations and history API"
```

### Task 7: Decorate graph operation nodes with UIDs

**Files:**
- Modify: `crates/engine/src/graph.rs`
- Modify: `crates/server/src/api.rs`
- Test: `crates/engine/tests/graph.rs`
- Test: `crates/server/tests/api.rs`

- [ ] **Step 1: Add RED graph metadata tests**

Assert ordinary graph nodes retain `op_idx`, removed ghost nodes retain `SnapshotSide::Before`, normal diff nodes use `SnapshotSide::After`, and collapsed cluster nodes have neither op metadata nor UID. Server integration must assert at least one non-cluster node has a UID matching `/ops/{uid}/history`.

- [ ] **Step 2: Add graph node fields**

```rust
pub struct GraphNode {
    // existing wire fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip)]
    pub op_idx: Option<OpIdx>,
    #[serde(skip)]
    pub provenance_side: Option<SnapshotSide>,
}
```

Set metadata in `node_of`, ghost construction, and cluster construction. Keep `uid = None` inside the pure engine.

- [ ] **Step 3: Decorate in the server**

After graph extraction, load the resolved function and map each node's `(pass, provenance_side, op_idx)` occurrence to a UID. For a non-diff graph, use the actual chosen blob side (`after` preferred, otherwise `before`). Leave clusters undecorated.

- [ ] **Step 4: Verify and commit**

Run: `cargo test -p engine --test graph && cargo test -p server --test api graph_endpoint`

```bash
git add crates/engine/src/graph.rs crates/engine/tests/graph.rs crates/server/src/api.rs crates/server/tests/api.rs
git commit -m "feat: attach provenance UIDs to graph nodes"
```

### Task 8: Add frontend API types and provenance state

**Files:**
- Modify: `ui/src/api.ts`
- Modify: `ui/src/api.test.ts`
- Modify: `ui/src/store.ts`
- Modify: `ui/src/store.test.ts`

- [ ] **Step 1: Add RED API/store tests**

Mock `api.selectableOps` and `api.opHistory`. Assert pass selection loads both sides' selectable maps, `selectOp(uid)` loads history and switches to `history`, selected UID survives another pass selection, and `viewHistoryStep(id)` selects the pass then switches to `text`.

- [ ] **Step 2: Define exact TypeScript wire types**

Add `SelectableOp`, `SnapshotSide`, `HistoryChange`, `EvidenceSource`, discriminated `LinkConfidence`, `HistoryEvidence`, `OpOccurrence`, `HistoryStep`, and `OpHistory`. Add `uid?: string` to `GraphNode`.

Add fetchers:

```ts
selectableOps: (passId: number, side: IrSide, func: string) =>
  getMsgpack<SelectableOp[]>(`/api/passes/${passId}/ops?side=${side}&func=${encodeURIComponent(func)}`),
opHistory: (uid: string) =>
  getMsgpack<OpHistory>(`/api/ops/${encodeURIComponent(uid)}/history`),
```

- [ ] **Step 3: Extend store state and transitions**

Set `ViewMode = 'text' | 'graph' | 'history'`. Add `selectableBefore`, `selectableAfter`, `selectedOpUid`, `history`, `selectOp`, and `viewHistoryStep`. Load selectable operations after resolving `selectedFunc`; missing sides yield empty arrays. Do not clear `selectedOpUid` during `selectPass`.

- [ ] **Step 4: Verify and commit**

Run: `cd ui && npm test -- --run src/api.test.ts src/store.test.ts && npm run typecheck`

```bash
git add ui/src/api.ts ui/src/api.test.ts ui/src/store.ts ui/src/store.test.ts
git commit -m "feat(ui): add provenance API and selection state"
```

### Task 9: Select operations from Text and Graph

**Files:**
- Modify: `ui/src/components/IrViewer.tsx`
- Modify: `ui/src/components/IrViewer.test.tsx`
- Modify: `ui/src/components/GraphView.tsx`
- Modify: `ui/src/graph/render.ts`
- Modify: `ui/src/graph/render.test.ts`
- Modify: `ui/src/App.tsx`

- [ ] **Step 1: Write RED interaction tests**

For `IrViewer`, click coordinates on a line covered by a `SelectableOp` and assert its UID callback fires; click whitespace and assert no callback. For GraphView/render, select a UID-bearing op node and assert the callback; a cluster node must not call it.

- [ ] **Step 2: Add CodeMirror line hit testing**

Pass `beforeOps`, `afterOps`, and `onSelectOp`. Install `EditorView.domEventHandlers({ mousedown })`; use `posAtCoords`, `doc.lineAt`, and choose the narrowest selectable range containing the line. Call `onSelectOp(op.uid)` and return `true` only for a match. Add a `selectable-op` line decoration and pointer cursor.

- [ ] **Step 3: Add canvas UID selection**

On pointer-up, resolve the hit node from `graph.nodes`; retain visual `selectedId`, and call `onSelectOp(node.uid)` only when UID exists. Keep drag and hover behavior unchanged.

- [ ] **Step 4: Wire App to the store**

Pass side-specific selectable arrays and `selectOp` to `IrViewer`; pass `selectOp` to `GraphView`.

- [ ] **Step 5: Verify and commit**

Run: `cd ui && npm test -- --run src/components/IrViewer.test.tsx src/graph/render.test.ts && npm run typecheck`

```bash
git add ui/src/components/IrViewer.tsx ui/src/components/IrViewer.test.tsx ui/src/components/GraphView.tsx ui/src/graph/render.ts ui/src/graph/render.test.ts ui/src/App.tsx
git commit -m "feat(ui): select operations from text and graph"
```

### Task 10: Build the dedicated merge-aware History view

**Files:**
- Create: `ui/src/components/HistoryView.tsx`
- Create: `ui/src/components/HistoryView.test.tsx`
- Modify: `ui/src/components/Toolbar.tsx`
- Modify: `ui/src/components/Toolbar.test.tsx`
- Modify: `ui/src/App.tsx`
- Modify: `ui/src/styles.css`

- [ ] **Step 1: Add RED component tests**

Render a history with one exact replacement, one inferred modification, and two merge predecessors sharing an after occurrence. Assert solid/dashed classes, confidence `75%`, action/listener/fingerprint badges, repeated merge rows, and `View IR` callback pass ID. Assert Toolbar History is disabled without `selectedOpUid` and keyboard `h` switches only when selected.

- [ ] **Step 2: Implement `HistoryView`**

Render header UID/first→last name and ordered `<ol className="history-timeline">`. A step receives `exact` or `inferred` class; inferred renders its score. Group adjacent steps with identical `(pass_id, after.op_idx)` under a merge label without reordering them. Render all evidence entries in sequence and a `View IR` button.

- [ ] **Step 3: Add History mode controls**

Add the third segmented button, `aria-pressed`, disabled state, title, and `h` shortcut. Disable Diff and function controls while in History, but preserve their state for return to Text/Graph.

- [ ] **Step 4: Add App branch and styling**

Render `HistoryView` when `viewMode === 'history'`. Add responsive timeline, solid/dashed connectors, merge indentation, evidence chips, empty/loading states, and focus-visible outlines. Reuse existing color tokens; do not add a UI dependency.

- [ ] **Step 5: Run React best-practices review**

Apply the `vercel:react-best-practices` skill because multiple TSX components changed. Resolve accessibility, unstable callback, effect dependency, and unnecessary rerender findings before proceeding.

- [ ] **Step 6: Verify and commit**

Run: `cd ui && npm test -- --run && npm run typecheck && npm run build`

```bash
git add ui/src/components/HistoryView.tsx ui/src/components/HistoryView.test.tsx ui/src/components/Toolbar.tsx ui/src/components/Toolbar.test.tsx ui/src/App.tsx ui/src/styles.css
git commit -m "feat(ui): add operation History view"
```

### Task 11: End-to-end Full trace flow and final verification

**Files:**
- Modify: `ui/playwright.config.ts`
- Modify: `ui/e2e/graph-diff.spec.ts`
- Modify: `docs/superpowers/specs/2026-07-05-m4b-op-history-provenance-design.md`

- [ ] **Step 1: Switch Playwright fixture generation to Full**

Change the web server command to:

```text
cargo run --manifest-path ../Cargo.toml -q -p cli -- dev gen-fixture --full ../target/e2e-demo.mlirtrace
```

- [ ] **Step 2: Add the RED browser scenario**

After the existing Text/Graph assertions, select a UID-bearing graph node, assert History opens, assert at least one exact evidence badge and a pattern name, click `View IR`, and assert the timeline pass selection and Text view. Collect `page.on('console')` errors and assert none.

- [ ] **Step 3: Run Playwright and debug any integration failure**

Run: `cd ui && npx playwright test --reporter=line`

Expected: all existing graph/diff behavior plus the new history scenario passes.
If it fails, invoke `superpowers:systematic-debugging`, identify the failing
boundary, and add a focused regression test before changing implementation.

- [ ] **Step 4: Run full verification**

```bash
export PATH=/Users/sungjin/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cd ui
npm test -- --run
npm run typecheck
npm run build
npx playwright test --reporter=line
```

Expected: Rust, TypeScript, Vitest, Vite build, and Playwright all pass without warnings or console errors.

- [ ] **Step 5: Update status and commit**

Set the M4b spec status to `Implemented and verified (2026-07-05)` and add a short verification note listing the commands above.

```bash
git add ui/playwright.config.ts ui/e2e/graph-diff.spec.ts docs/superpowers/specs/2026-07-05-m4b-op-history-provenance-design.md
git commit -m "test: verify M4b operation history flow"
```

- [ ] **Step 6: Finish the branch**

Use `superpowers:requesting-code-review`, `superpowers:verification-before-completion`, and `superpowers:finishing-a-development-branch`. Because this environment forbids subagents, perform the review checklist inline and record any residual concern before merging and pushing.

## Plan self-review

- **Spec coverage:** Tasks 1–3 implement exact/inferred/merge resolution and deterministic UIDs; Tasks 4–7 implement trace adaptation, bounded caching, APIs, and graph decoration; Tasks 8–10 implement both selection paths and dedicated History; Task 11 covers Full trace E2E and all verification layers.
- **Type consistency:** `SnapshotSide`, `OpUid`, `SelectableOp`, `OpHistory`, `HistoryStep`, `LinkConfidence`, and evidence enums originate in `engine::provenance` and map one-to-one to TypeScript snake_case wire types.
- **Scope:** Full Inspector, search, docking, sidecar persistence, schema changes, and C++ hook improvements remain outside M4b.
