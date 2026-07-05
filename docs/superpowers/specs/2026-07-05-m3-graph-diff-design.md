# M3 — Graph view & structural diff design

**Date:** 2026-07-05
**Status:** Approved (brainstorming session)
**Parent spec:** `2026-07-02-mlir-viewer-design.md` (§6 engines, §7 API, §8 frontend, §10 scalability)
**Base branch:** `feat/m2-walking-skeleton` → new branch `feat/m3-graph-diff`

## 1. Goal

Add two toggles to the viewer:

- **View toggle `[Text | Graph]`** — the current IR text panes, or an SSA def-use graph of the selected pass's IR.
- **Diff toggle `[Diff]`** — in Text mode, a git-style split diff; in Graph mode, node colors expressing added / modified / removed ops.

Both are backed by a new server-side engine crate: MLIR text parsing, structural diff, and graph extraction. This is the M3 milestone of the parent spec, minus provenance/search (deferred).

## 2. Decisions made

| Question | Decision |
|---|---|
| Graph semantics | **SSA def-use graph**: node = op, edge = value def→use. (CFG, structure tree rejected: weaker fit for pass-diff visualization.) |
| Where to parse | **Server (Rust)**, new `crates/engine`. Client-side TS parsing rejected: breaks at the 256 KiB paging boundary and violates the "never ship the whole module to the browser" rule (parent spec §10.1). |
| Rendering | **Full spec form**: ELK layered layout in a Web Worker + custom canvas-2D renderer with LOD. (React Flow rejected in parent spec ADR; SVG intermediate step rejected by user.) |
| Graph diff layout | **One unified graph**: after-graph as the base, removed ops included as ghosts. (Side-by-side graphs rejected: space cost, hard node correspondence.) |
| Text diff layout | **Split view**: keep the existing before/after panes, add line highlights + synced scrolling. (Unified view rejected.) |
| Diff engine level | **Full structural diff engine** per parent spec §6 — per-function, lazy, cached, hierarchical greedy matching. (Line-diff-only and fingerprint-only options rejected by user.) |

## 3. Constraint that shapes everything

Trace format v1 stores **text snapshots only** (`ir_blob`); there are no structural op rows and no identity uids yet (those arrive in M4). Therefore:

- The engine must include a **tolerant MLIR text parser** that recovers structure from printed IR.
- The diff matcher runs on **fingerprints only** for now, but its interface is *uid-first, fingerprint-fallback* so M4 identity events slot in without redesign (parent spec §9.2–9.3).

## 4. Engine crate (`crates/engine`)

### 4.1 Tolerant MLIR text parser

Purpose-built structure extractor, not a full MLIR grammar. Per op it captures: op name, result SSA names, operand SSA names, type signature, attribute summary (raw text span), location (if printed), region nesting depth/path, and **source line range** in the snapshot text.

- Handles generic *and* pretty-printed forms structurally; unknown dialects are fine by construction.
- **Error recovery is mandatory:** a line that fails to parse becomes an *opaque op* (name = first token, no operands) and parsing continues. The parser must never abort a snapshot.
- Output: a flat op table with region-tree indices, grouped by top-level function-like ops (`func.func`, `gpu.func`, anything with a symbol name and a single region — detected heuristically; see §8 edge cases).

### 4.2 Structural diff

Parent spec §6 verbatim, scoped to fingerprint matching:

- **Granularity:** per-function, computed lazily on first request, cached (keyed by `(blob_before, blob_after, func)` — blob ids, not pass ids, so dedup'd snapshots share cache entries).
- **Matching:** hierarchical greedy within matched parent regions, scored on (op name, location, result types, operand shape, relative position). Exact-fingerprint matches first, then best-score above a threshold.
- **Classification:** `added` / `removed` / `modified` (+ per-op detail for modified: which attributes/types/operands changed), `unchanged`.
- **Matcher interface:** `trait OpMatcher` with the greedy fingerprint impl today; M4 swaps in a uid-based impl that falls back to fingerprints.
- Diff results carry **line ranges on both sides** so the UI can project op-level changes onto text lines.

### 4.3 Graph extraction

- Def-use graph per function from the parsed op table: node per op, edge per (result → operand use).
- **Clustered by region** (parent spec §10.3): nodes carry a cluster path; the server collapses clusters to stay under the node budget (default 2000), emitting collapsed-cluster meta-nodes with contained-op counts.
- **Diff mode:** extract both sides, run the structural diff, emit the after-graph plus removed ops as ghost nodes (attached where their operands/users were matched), each node tagged `added|removed|modified|unchanged`. Removed edges tagged for dashed rendering.

## 5. HTTP API (extends `crates/server`)

```
GET /api/passes/{id}/functions
      → [{ name, op_count, has_before, has_after }]        (budgeted list)

GET /api/passes/{id}/diff?func=<name>
      → structural diff for one function: op matches with change class,
        per-op detail, and before/after line ranges          (MessagePack)

GET /api/graphs/dataflow?pass={id}&func=<name>&diff=0|1&budget=N
      → { nodes, edges, clusters, truncated }                (MessagePack)
        node: { id, label, op_name, line_range, cluster, change? }
```

All responses budgeted/paged per ADR-6. `diff=1` on a pass missing one side is a 409 with a machine-readable reason (UI disables the toggle instead of ever seeing this in practice).

## 6. UI

### 6.1 Toolbar & state

- Toolbar in the main pane header: segmented `[Text | Graph]` control + `[Diff]` toggle button.
- Store additions: `viewMode: 'text' | 'graph'`, `diffEnabled: boolean`, `selectedFunc: string | null`. All survive pass navigation; `selectedFunc` resets only if the new pass lacks that function.
- Keyboard: `t` text, `g` graph, `d` diff toggle.

### 6.2 Text mode

- **Diff OFF:** current behavior, untouched.
- **Diff ON:** split view. Fetch `/diff` for functions intersecting the visible page; project op change classes to CodeMirror line decorations — removed = red (before pane), added = green (after pane), modified = yellow (both), gutter markers. Scroll sync aligns panes by matched-op line anchors. Visually a git split diff, but driven by the structural engine, so SSA renames and reorders don't produce noise walls.

### 6.3 Graph mode

- Function selector dropdown (populated from `/functions`; defaults to first function; single-scope snapshots skip the dropdown).
- **Layout:** ELK (`elkjs`) layered layout runs in a **Web Worker**; UI shows a layout spinner and stays interactive.
- **Renderer:** custom canvas-2D. Zoom/pan (wheel + drag). **LOD:** zoomed out = colored boxes with cluster heat-coloring; zoomed in = op name + result type labels. Hover highlights the node's def-use neighborhood. Click selects (selection stored for future inspector integration; no inspector in M3).
- Collapsed cluster meta-nodes render with an op count badge; click expands (re-request with focus param is out of scope — expansion re-runs layout on client-held children only if under budget, else shows "N ops hidden").

### 6.4 Graph + diff

- Unified graph from `diff=1`: added = green, modified = yellow, removed = semi-transparent red ghosts, unchanged = default. Removed edges dashed. Legend chip row above the canvas.

## 7. Error handling & edge cases

- Pass missing before or after snapshot → Diff toggle disabled with tooltip explaining why; Graph mode renders the existing side.
- `ir_changed = false` → diff renders with a "no changes" badge (cheap: blob ids equal ⇒ skip engine entirely).
- Snapshot with no recognizable function-like ops → the whole module is one scope named `(module)`.
- Parser failure on a snapshot (catastrophic, not per-line) → Graph mode shows an error banner with the parse error; Text mode is unaffected.
- Node budget exceeded even after cluster collapse → server truncates deterministically and sets `truncated: true`; UI shows a warning chip.
- Text paging (256 KiB) is unaffected: diff/graph are computed server-side over full blobs; only text display remains paged.

## 8. Testing

- **engine:** parser unit tests against real fixture traces (generic + pretty forms, unknown dialect, malformed lines → opaque ops); diff golden tests for add/remove/modify/reorder/rename scenarios; graph extraction tests incl. budget collapse determinism.
- **server:** endpoint integration tests — unknown pass, unknown func, missing side (409), budget behavior, MessagePack round-trip.
- **ui:** vitest — toggle state transitions, decoration mapping from diff payloads, store persistence across pass selection; Playwright smoke on a real trace — switch text↔graph, toggle diff in both modes, function switch.

## 9. Out of scope (deferred)

- Provenance engine & op history (M4+, needs identity events).
- Search index (parent spec M-later).
- Graph kinds other than dataflow (CFG, call graph) — the extractor sits behind the `GraphExtractor` seam so they add without touching this work.
- Inspector panel integration (node click only records selection).
- Focus/expand server round-trip for collapsed clusters.
