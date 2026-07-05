# M4b — Op history and provenance design

**Date:** 2026-07-05
**Status:** Approved
**Parent spec:** `2026-07-02-mlir-viewer-design.md` (§8–§10, §13 M4)
**Foundation:** `2026-07-05-m4a-op-identity-capture-design.md` and schema v2

## 1. Goal

Turn M4a's raw pointer-keyed lifecycle events and per-snapshot op indexes into
stable, trace-local operation histories. A user can select an operation from
either Text or Graph view, open a dedicated History view, see its complete
pipeline-wide chain, distinguish exact events from inferred matches, and jump
to the IR at any step.

M4b completes the user-visible half of M4. The full structured Inspector remains
M5 work; M4b adds only the operation summary required by History.

## 2. Decisions

| Question | Decision |
|---|---|
| Computation | Lazy per requested function/op, cached; no eager whole-trace index |
| UID | Deterministic, trace-local, and function-scoped, anchored by the earliest pass/side/op ordinal in the resolved component |
| Persistence | Read-only; do not mutate the trace or create a sidecar |
| Exact evidence | M4a `op_identity` plus matching `op_index` rows |
| Missing evidence | Existing greedy fingerprint matcher, surfaced as inferred confidence |
| History scope | Entire executable leaf-pass pipeline, not only adjacent snapshots |
| Selection | Both Text op lines and Graph nodes |
| UI placement | Dedicated `History` mode beside Text and Graph |
| Confidence UI | Exact links are solid; inferred links are dashed with a score |

## 3. Architecture

### 3.1 Pure provenance engine

Add `engine::provenance`, independent of SQLite, HTTP, and React. It consumes a
normalized function timeline:

```rust
pub struct TimelineStage {
    pub pass_id: i64,
    pub pass_name: String,
    pub before: Option<SnapshotOps>,
    pub after: Option<SnapshotOps>,
    pub events: Vec<NormalizedIdentityEvent>,
}

pub struct SnapshotOps {
    pub side: SnapshotSide,
    pub ops: Vec<ParsedOp>,
    pub tokens: HashMap<i64, OpIdx>,
}
```

The engine returns `OpHistory` and a mapping from every occurrence in the
resolved component to one `OpUid`. The server owns trace reading and
normalization; the engine owns matching, chain construction, confidence, and
deterministic ordering.

### 3.2 Timeline normalization

The server flattens the pass tree to executable leaves in stable execution
order (`start_ns`, then tree/sequence order). Synthetic `Pipeline` and adaptor
parents are not separate history transitions when they contain child passes.
An ordinary pass with no children remains a stage.

For each stage and side, the adapter parses its blob through the existing parser
cache and restricts work to the requested function. It maps `op_index` rows to
`ParsedOp` indexes as follows:

1. A non-negative byte span maps to the parsed statement containing that span.
2. The C++ fallback encoding (`byte_end = -1`) maps `byte_start` as the pre-order
   ordinal.
3. Invalid, duplicate, or out-of-range rows are ignored. The affected relation
   falls back to fingerprint inference instead of failing the request.

Pointer tokens never cross a pass boundary. Cross-pass continuity is established
from identical boundary content where possible and otherwise by fingerprint.

### 3.3 Link resolution

Within a pass, evidence is applied in this order:

1. `replaced(old, new)` links the before token to the after token.
2. A token present on both sides links the same operation; `modified` annotates
   that link.
3. `erased` terminates a chain. A replacement with `new_token = NULL` has the
   same effect.
4. `inserted` starts a chain at the after occurrence.
5. Remaining operations are passed to `GreedyFingerprintMatcher` as a one-to-one
   fallback.

Adjacent stages are bridged using identical parsed occurrences when the prior
after blob and next before blob are the same. Otherwise the same matcher is used
for the requested function. Fingerprint inference never links different op
names unless an explicit `replaced` event authorizes the transition.

Multiple listener/action events for one transition are retained in sequence;
they are not collapsed to a single pattern string.

Resolved components can merge but M4b does not infer fan-out. Two `replaced`
events sharing one `new_token` (e.g. CSE folding duplicates) join their chains
into a single resolved component with one UID. Schema v2 cannot relate one old
operation to multiple inserted replacements, and the fingerprint matcher is
one-to-one, so such inserted branches remain separate components instead of
receiving a fabricated relationship. If malformed input gives one old token
multiple `replaced` successors, the lowest-sequence valid event wins
deterministically.

### 3.4 UID and confidence

```rust
pub struct OpUid(String); // "op1.{func_b64url}.{pass_id}.{b|a}.{func_ordinal}"

pub enum LinkConfidence {
    Exact,
    Inferred { score: u16 },
}

pub enum EvidenceSource {
    Listener,
    Action,
    Fingerprint,
    SharedSnapshot,
}
```

`func_ordinal` is the anchor operation's ordinal within the function scope;
`ParsedOp.idx` remains its existing module-global parser index. `func_b64url` is
the unpadded URL-safe base64 encoding of the UTF-8 function symbol, so symbols
containing punctuation cannot make UID parsing ambiguous. The `op1` prefix
versions the wire encoding. A UID resolves to its function without a separate
query parameter and identical ordinals in different functions never collide.
Clients treat the UID as opaque.

After resolving a component, its earliest occurrence in normalized execution
order is the UID anchor. Selecting any later occurrence — including one on a
branch that later merges into the component — therefore produces the same UID,
including after a server restart. Determinism holds per viewer version: a
future change to the fingerprint matcher may re-anchor chains whose links are
inferred, which is acceptable because UIDs are trace-local and never persisted.

`Exact` covers lifecycle events, same-token continuity within a pass, and the
same operation in a shared immutable snapshot boundary. Shared boundaries
record `SharedSnapshot` as their evidence source. Matcher links are always
`Inferred` and carry the normalized matcher score used for ordering.

## 4. API contract

### 4.1 Selectable operations

```http
GET /api/passes/{pass_id}/ops?side=before|after&func={symbol}
```

Returns MessagePack:

```rust
pub struct SelectableOp {
    pub uid: String,
    pub op_idx: usize,
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
}
```

This endpoint is the shared selection map for CodeMirror and canvas graph nodes.
An unknown pass/snapshot/function is 404; an invalid side is 400.

Because every `uid` is anchored at its component's earliest occurrence, the
first request for a function resolves provenance across the entire pipeline for
that function, not just the requested pass. Later requests for any pass of the
same function are served from the caches in §5. This first-hit latency is the
accepted cost of the lazy, no-eager-index decision in §2.

### 4.2 Operation history

```http
GET /api/ops/{uid}/history
```

Returns MessagePack:

```rust
pub struct OpHistory {
    pub uid: String,
    pub first_name: String,
    pub last_name: String,
    pub steps: Vec<HistoryStep>,
}

pub struct HistoryStep {
    pub pass_id: i64,
    pub pass_name: String,
    pub change: HistoryChange,
    pub before: Option<OpOccurrence>,
    pub after: Option<OpOccurrence>,
    pub evidence: Vec<HistoryEvidence>,
    pub confidence: LinkConfidence,
}

pub struct OpOccurrence {
    pub side: SnapshotSide,
    pub op_idx: usize,
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub attr_summary: String,
    pub location: Option<String>,
}
```

`HistoryChange` is `Inserted | Erased | Replaced | Modified | Unchanged`.
`HistoryEvidence` carries event sequence, optional pattern, and source. Malformed
UID syntax is 400; a syntactically valid but absent anchor is 404.

`steps` is ordered by execution but is not strictly linear: a pass where
several predecessors merge yields one `HistoryStep` per predecessor, each with
its own `before` and all sharing the same `after` occurrence and `pass_id`.
The anchor branch's step comes first; the remaining merge steps follow in op
ordinal order. The UI renders these as converging edges into one node. Since
M4b does not infer fan-out (§3.3), an `after` occurrence is never duplicated
across different successors.

Version 1 and Text-only traces remain supported: both endpoints resolve through
parsed snapshots and fingerprint links when identity tables or rows are absent.

## 5. Server integration and caching

Extend `EngineCache` with two bounded caches:

- normalized per-function timelines keyed by trace/pass-tree identity + function;
- resolved histories and occurrence-to-UID maps keyed by function + anchor.

History entries are limited to 2,048 chains and evicted oldest-first. The server
still opens `TraceReader` read-only per request. Cache values contain owned engine
types and never retain SQLite connections.

Graph extraction stays pure. The server combines graph `op_idx` values with the
selectable-op map before serializing the response, adding a nullable `uid` to
graph nodes. Ghost nodes use the before side; ordinary diff nodes use the after
side when present. Non-op cluster nodes have no UID.

## 6. Frontend behavior

The toolbar becomes `Text | Graph | History`.

- Clicking a parsed operation line in Text selects its UID and opens History.
- Clicking an operation node in Graph does the same.
- History is disabled until an operation has been selected.
- The selected UID survives pass navigation and mode changes.
- History renders a vertical pipeline timeline. Exact links are solid;
  inferred links are dashed and show their score.
- Each step shows change kind, before/after op names, pattern, and evidence badge
  (`action`, `listener`, `fingerprint`, or `shared snapshot`).
- `View IR` selects the step's pass and returns to Text mode, preserving the UID.

This dedicated mode intentionally hides the IR while reading history. It avoids
shrinking the existing side-by-side editor and canvas. The structured operands,
results, types, attributes, and region-tree Inspector remains M5.

## 7. Error handling and edge cases

- Empty or unchanged passes produce an `Unchanged` transition when the selected
  op survives.
- An erase ends forward traversal; an insert has no predecessor.
- A function appearing or disappearing mid-pipeline starts or ends its chains.
- Missing snapshots are skipped without joining across unrelated op names.
- Bad identity enum/source encodings remain trace corruption errors from
  `TraceReader`; only unmappable index rows degrade locally.
- Equal fingerprint candidates resolve by score, execution order, then op
  ordinal, making results reproducible.
- A UID from another trace normally resolves to no anchor and returns 404. UIDs
  are explicitly trace-local, not globally portable identifiers.
- Selecting any predecessor of a merged component returns the whole component's
  history under the shared UID; the merge pass renders one step per predecessor
  (§4.2).

## 8. Testing

1. **Engine:** exact replace/modify/insert/erase, shared boundaries, inferred
   gaps, mixed exact/inferred chains, deterministic UID, no cross-name inference,
   invalid ordinal fallback, stable tie-breaking, and N→1 merge (two `replaced`
   events sharing one `new_token` yield one component, one UID, and per-
   predecessor merge steps).
2. **Server:** Full synthetic fixture selectable ops/history, v1 fallback,
   graph UID decoration, malformed UID 400, absent UID 404, and cache eviction.
3. **UI:** Text line selection, Graph node selection, disabled/enabled History,
   solid/dashed rendering, evidence badges, and `View IR` navigation.
4. **End to end:** generate `--full`, serve it, select an op, inspect its full
   history, jump to a pass, and confirm there are no browser console errors.

## 9. Out of scope

- The full right-side Inspector (M5).
- Search, command palette, docking, and layout persistence (M5).
- Writing durable UIDs back into trace schema v2 or a sidecar.
- Improving C++ listener coverage or replacing ordinal op indexes.
- Cross-trace identity, distributed trace merging, or provenance export.
