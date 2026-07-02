# MLIR Viewer — Software Design Specification

**Date:** 2026-07-02
**Status:** Draft for review
**Scope:** Full-system architecture; implementation decomposed into milestones M0–M7, each with its own implementation plan.

---

## 1. Vision

A production-quality, open-source **visual debugging environment for MLIR-based compilers**. It answers the questions compiler engineers actually ask while debugging passes: *what changed, which pass changed it, which pattern did it, where did this op come from, why was it removed.*

**Non-goals (v1):** IR editing, running/driving the compiler from the UI, live streaming during compilation, support for non-MLIR IRs (StableHLO-as-protobuf, ONNX, etc.). These are future work with identified seams (§11, §15) — not abstractions built today.

**Primary workflow served:** `torch.export (.pt2)` → MLIR (custom dialects/ops/attrs/passes) → optimization pipeline → codegen, compiled on a **remote Linux machine**, inspected from a **local browser**.

## 2. Decisions locked during brainstorming

| Decision | Choice | Why |
|---|---|---|
| IR capture | C++ instrumentation library linked into the user's compiler | Only place full introspection of custom dialects/attrs is possible; unlocks op history + pattern attribution, which text dumps cannot provide |
| Debugging model | Post-mortem trace files | Simple, robust, shareable (attach to bug reports); "replay" = navigating recorded snapshots |
| Form factor | Local web server (`mlir-viewer serve trace.mlirtrace`), TensorBoard/Perfetto model | Works over SSH port-forwarding for remote-Linux workflows; no desktop packaging; Tauri wrapper possible later with near-zero rework |
| Overall shape | Structured capture + thick Rust backend + React frontend (Approach A) | Perfetto-proven architecture for huge post-mortem traces |

**The load-bearing insight:** the viewer never parses or interprets MLIR semantically. The instrumentation library serializes IR *structurally* (ops, operands, results, attributes, types, regions — generically, via MLIR's introspection APIs) inside the compiler process, where every custom dialect is registered. Unknown dialects work **by construction**, and the viewer is fully decoupled from LLVM versions.

## 3. System architecture

Three components, two contracts. Each component is independently replaceable and testable; the contracts are the only coupling points.

```
┌─────────────────────────┐   Contract 1: Trace format   ┌──────────────────────┐
│ your compiler            │   (SQLite file, versioned    │ mlir-viewer backend  │
│ + libmlir-trace (C++)    │──▶ schema, §5)          ────▶│ (Rust, axum)         │
│   PassInstrumentation    │                              │ diff / provenance /  │
│   Action tracing         │        trace.mlirtrace       │ index / graph engines│
└─────────────────────────┘                              └──────────┬───────────┘
                                                                     │ Contract 2: HTTP API
                                                                     │ (JSON control +
                                                                     │  MessagePack bulk, §7)
                                                          ┌──────────▼───────────┐
                                                          │ frontend (React/TS)  │
                                                          │ rendering+interaction│
                                                          │ only — no analysis   │
                                                          └──────────────────────┘
```

**Dependency rule:** frontend → API → backend → trace format ← instrumentation. Nothing depends on the frontend; the instrumentation library and the viewer share **only** the trace schema (no shared code — C++ writes it, Rust reads it, conformance enforced by golden-trace tests).

### Repository layout

```
mlir-viewer/
  crates/
    trace-format/     # schema, Rust reader/writer, fixtures (M0)
    engine/           # diff, provenance, search, graph extraction (M3+)
    server/           # axum HTTP server, embeds built UI via rust-embed (M2)
    cli/              # `mlir-viewer` binary: serve, trace dump, dev tools (M0)
  capture/            # C++ libmlir-trace: CMake package, PassInstrumentation (M1)
  ui/                 # React + TypeScript app (M2)
  examples/           # executable toy pipelines producing real traces (per milestone)
  docs/
```

Ships as a **single static binary** (UI assets embedded) — `cargo install` / prebuilt release binaries; ideal for copying to remote machines.

## 4. Instrumentation library (`libmlir-trace`, C++)

**Integration API** (one call in the user's compiler setup):

```cpp
mlir::trace::TraceRecorder recorder("out.mlirtrace", options);
recorder.attach(passManager, mlirContext);  // installs PassInstrumentation + action handler
```

**Captures:**
- Pass tree (nested pass managers), per-pass wall time, before/after IR.
- IR snapshots in two forms: (a) **text** (as printed, with a byte-offset index per operation for editor click-mapping), (b) **structured** rows: op name, dialect, attributes (as generic printed forms + structural breakdown), operand/result types, location, region/block tree.
- **Pattern applications** via the MLIR Actions framework (`MLIRContext::registerActionHandler`; `ApplyPatternAction` carries pattern name + root op).
- Op identity events where obtainable (§9).

**Fidelity levels** (runtime option — graceful degradation is a feature, not a fallback): `timeline` (timings only) → `text` (+ text snapshots) → `full` (+ structure, patterns, identity). Snapshot dedup by content hash (xxhash); pass that changed nothing stores no new blob.

**Performance budget:** `full` fidelity ≤ 2× compile time, `text` ≤ 1.3×. zstd-compressed blobs, written incrementally (SQLite WAL) so a crashed compile still yields a usable partial trace.

## 5. Contract 1 — trace format

SQLite container, `format_version` in a `meta` table; readers reject newer major versions. Core tables (v1, M0 scope — structural tables added in M3 as additive migration):

```sql
meta(key TEXT PRIMARY KEY, value TEXT)
ir_blob(id INTEGER PRIMARY KEY, hash BLOB UNIQUE, size_bytes INTEGER,
        compression TEXT, data BLOB)
pass_execution(id INTEGER PRIMARY KEY, parent_id INTEGER REFERENCES pass_execution,
        seq INTEGER, name TEXT, ir_before INTEGER REFERENCES ir_blob,
        ir_after INTEGER REFERENCES ir_blob, start_ns INTEGER, end_ns INTEGER,
        ir_changed INTEGER)
```

M3 additions: `operation`, `op_attr`, `op_index` (byte offsets into text), `pattern_application`, `op_identity`. SQLite chosen over FlatBuffers/Protobuf/MessagePack files because the dominant access pattern is **random access + indexed queries into multi-GB traces without loading them** — exactly SQLite's strength (Perfetto precedent). ADR-3.

## 6. Backend engines (Rust)

- **Diff engine:** structural tree diff between two snapshots. Ops matched by identity uid when present; otherwise hierarchical greedy matching scored on (op name, location, result types, operand shape, position). Classifies added / removed / modified; per-op attribute- and type-level diffs. Computed **per-function, lazily, cached** — never whole-module eagerly.
- **Provenance engine:** stitches op identity events + diff matches into per-op history chains across the whole pipeline (created-by, modified-by, replaced-by, erased-by, with pattern attribution when captured).
- **Search index:** ops by name/dialect/attribute/symbol, built lazily per snapshot.
- **Graph extraction:** CFG / dataflow / call graph extracted from structural rows on demand, clustered by region/function before it ever reaches the client.

## 7. Contract 2 — HTTP API (sketch)

```
GET /api/trace/info                     — meta, fidelity level, stats
GET /api/passes                         — pass tree with timings + change flags
GET /api/passes/{id}/ir?side=before|after&range=…   — paged text + op index
GET /api/passes/{id}/diff?func=…        — structural diff for one function
GET /api/ops/{uid}/history              — provenance chain
GET /api/ops/{uid}                      — full inspector payload (lazy attr detail)
GET /api/search?q=…&scope=…
GET /api/graphs/{kind}?pass=…&focus=…&budget=2000   — clustered, budgeted graph
```

JSON for control-plane, MessagePack for bulk payloads (IR pages, graphs). All list/tree/blob responses are **paged or budgeted** — the API never returns an unbounded payload. ADR-6.

## 8. Frontend (React + TypeScript)

**Stack (ADRs 7–10):** React (ecosystem: dockview, cmdk, mature tooling) · **CodeMirror 6** editor (superior huge-document performance, first-class custom language mode for MLIR; Monaco rejected — heavier, harder custom tokenizers) · **Zustand** state (minimal ceremony; Redux Toolkit rejected as ceremony without benefit here) · **dockview** docking layout · graph rendering via **ELK layout in a Web Worker + custom canvas-2D renderer** with level-of-detail (React Flow rejected for scale: DOM nodes collapse past ~1–2k elements; Perfetto precedent for canvas) · Vite · Vitest + Playwright.

**Layout** (dockable, persisted, preset-able):

- **Timeline panel** (left): the pass pipeline tree — the primary navigation axis. Rows show name, duration bar, changed/no-op badge. Click = select pass; `[`/`]` step backward/forward (this *is* pass replay).
- **Editor center**: before/after split of the selected pass, synchronized scrolling, structural diff decorations (added/removed/modified gutter + inline attribute changes). Virtualized via CodeMirror + server-side paging; regions server-collapsed beyond depth N with click-to-expand.
- **Inspector panel** (right): tree view of the selected op — operands, results, types, attributes (lazy-loaded; dense tensors summarized: shape, dtype, min/max/mean, first-k elements), location, region tree. A **History tab** shows the op's provenance chain; clicking a hop navigates the timeline.
- **Bottom dock**: pass statistics, pattern applications table, search results.
- **Command palette** (`cmd-k`, cmdk) + **search everywhere**: ops, passes, symbols, attributes. Keyboard-first: every navigation has a binding; no modal dialogs in core flows.

**UX principles:** density without clutter, progressive disclosure (summaries → detail on demand), context preservation (selection survives pass navigation via op identity — selecting an op then stepping passes follows *that op*).

## 9. Op identity & provenance — the #1 technical risk

True cross-pass op identity requires observing creation/replacement/erasure. MLIR exposes this unevenly: rewrite drivers accept `RewriterBase::Listener`, the Actions framework wraps pattern application, but there is no universal context-wide op lifecycle hook. **Strategy:**

1. **M4 opens with a time-boxed research spike** against the target LLVM version to determine the strongest available hook combination (action handler + driver listeners).
2. Where events are unavailable, fall back to **fingerprint matching** (the diff engine's matcher) — degrading gracefully from "exact identity" to "high-confidence match", surfaced honestly in the UI (solid vs. dashed history links).
3. The trace schema stores identity as *events with confidence*, so capture improvements never require schema redesign.

## 10. Scalability strategy (LLM-scale IR)

Assume 10⁴–10⁵ ops, deeply nested regions, hundreds of passes. Techniques, in priority order:

1. **Never ship the whole module to the browser** — server-side paging, per-function lazy diff, budgeted graph responses (§7). This single rule does more than any rendering trick.
2. **Virtualized text rendering** (CodeMirror viewport model) + server-computed region folding.
3. **Graphs: cluster-first, render-second.** Hierarchical clustering by function/region; semantic zoom (module → function → block → op); hard node budget (~2k) with search/focus-driven expansion; ELK layered layout in a worker; canvas LOD (zoomed out = boxes + heat coloring, zoomed in = full labels).
4. **Snapshot dedup + zstd** keep traces proportional to *change*, not pipeline length.

Rejected: rendering full op graphs naïvely (any renderer dies at 10⁵ DOM/SVG nodes); client-side whole-trace loading (browser memory).

## 11. Extensibility seams (defined now, exploited later)

- **Unknown dialects/attrs:** handled by construction (§2) — no plugin needed for correctness.
- **Cosmetic plugin surface (M7, v0):** a declarative JSON/TS registration — dialect → colors, icons, attribute renderers, custom inspector panels. Runs in the frontend only; the core never imports plugins. Custom graph extractors are a *backend* trait (`GraphExtractor`) — new graph kinds without touching existing engines.
- **Other IRs later:** everything downstream of the trace format is IR-agnostic-ish already; a future non-MLIR producer only needs to write the trace schema. No abstraction built for this today beyond keeping MLIR-isms out of table names where free.

## 12. Testing strategy

- **Trace format:** round-trip tests (Rust writer↔reader); **golden conformance traces** committed as fixtures — the C++ writer must produce files the Rust reader validates bit-semantically (this test *is* Contract 1).
- **Instrumentation:** examples under `examples/` run real `mlir-opt`-style pipelines (upstream dialects for CI; toy custom dialect for the extensibility claim) and assert trace contents.
- **Engines:** unit tests on synthetic snapshots; diff engine property tests (diff(A,A)=∅; apply-classification consistency).
- **API:** integration tests over fixture traces.
- **UI:** Vitest component tests; Playwright end-to-end on a fixture trace (open → navigate passes → inspect op → view diff).
- **Performance gates:** benchmark trace (generated, 50k ops × 100 passes) with budget assertions on API latency and capture overhead.

## 13. Milestones (each = independently shippable, own implementation plan)

| M | Deliverable | Proves |
|---|---|---|
| **M0** | Trace format crate: schema v1, Rust writer/reader, synthetic fixture generator, `mlir-viewer trace dump` CLI | Contract 1 exists and is testable without a compiler |
| **M1** | `libmlir-trace` C++ capture (timeline + text fidelity), CMake package, example pipeline, conformance vs. M0 reader | Real traces from a real MLIR pipeline |
| **M2** | `mlir-viewer serve`: walking-skeleton UI — timeline panel + virtualized IR text viewer with MLIR highlighting | End-to-end architecture works over SSH |
| **M3** | Structural capture (schema v1.1) + diff engine + before/after diff UI | The core debugging loop: *what changed* |
| **M4** | Identity research spike → op history, provenance, pattern attribution UI | *Why/where did it change* |
| **M5** | Inspector panel, search everywhere, command palette, layout persistence | Daily-driver ergonomics |
| **M6** | Graph views (CFG first, then dataflow/call) with clustering + canvas LOD | Visual navigation at scale |
| **M7** | Stats dashboards, cosmetic plugin API v0, docs site, release packaging | Open-source launch readiness |

## 14. ADR index

1. **Capture via instrumentation, not dump parsing** — fidelity requires living in the compiler process.
2. **Post-mortem traces, not live driving** — simplicity, shareability; streaming is a future additive layer (a trace is a persisted event stream).
3. **SQLite trace container** — random access into GB-scale traces; incremental/crash-safe writes; vs. FlatBuffers (zero-copy but no indexing/incremental write story), Protobuf/MessagePack (stream formats, must load fully).
4. **Rust backend** — memory safety + performance for the analysis engines; vs. C++ (no memory-safety story for a long-lived server, and tempts MLIR linkage — see ADR-5), Python (perf ceiling), TypeScript/Node (perf + no rayon-class parallelism).
5. **Viewer never links MLIR/LLVM** — decouples from LLVM versioning and user dialect builds; all semantics captured at trace time.
6. **JSON control-plane + MessagePack bulk; every response bounded** — scalability is an API property before it is a rendering property.
7. **React over Svelte/Vue** — the specific libraries this app needs (dockview, cmdk, CodeMirror integrations) are React-first; largest contributor pool for an OSS project.
8. **CodeMirror 6 over Monaco** — huge-document performance, lighter bundle, purpose-built custom-language support.
9. **Zustand over Redux Toolkit** — server-derived state dominates; minimal client state ceremony.
10. **ELK-in-worker + canvas LOD over React Flow/Cytoscape/D3-force** — deterministic layered layouts suit IR; DOM/SVG renderers don't survive the node budget; force layouts are wrong for dataflow semantics.
11. **Single static binary with embedded UI** — remote-machine deployment is `scp` + run.

## 15. Risks & future roadmap

**Risks:** (1) op identity hook coverage (§9 — mitigated by confidence model); (2) capture overhead on huge models (mitigated by fidelity levels + dedup); (3) LLVM API drift affecting `libmlir-trace` (mitigated by thin capture surface + CI against pinned LLVM releases); (4) three-language repo raises contributor bar (mitigated by strict contracts — most contributions touch one language).

**Future:** live streaming mode; Tauri desktop wrapper; StableHLO/ONNX producers; trace diffing across compiler versions; VS Code extension embedding the UI.
