# Real-pipeline validation findings (post-M4b)

**Date:** 2026-07-06
**Tool:** `examples/mlir-trace-opt` (added for this validation): an
mlir-opt-style harness that runs an arbitrary textual pass pipeline at
`Fidelity::Full`. Dialect set limited to arith/func/ub because the local MLIR
21.1 build is minimal (no SCF/LLVM dialects, no conversion passes).
**Workload:** synthetic module with ~11,400 arith ops across 20 functions,
pipeline `canonicalize,cse,canonicalize,symbol-dce,sccp,canonicalize`
(6 executable passes), debug-build server.

## Findings

1. **Stock passes emit zero lifecycle events.** The full 6-pass run produced
   0 `op_identity` rows. This quantifies the gap M4a documented: the
   recorder's listener is cooperative, and upstream passes (canonicalize,
   CSE, SCCP) construct their own drivers, so nothing reaches it. On real
   pipelines today, *every* history link is fingerprint-inferred or
   shared-snapshot exact; `listener`/`action` evidence only appears for
   passes written to accept `TraceRecorder::rewriteListener()`.
   *Implication:* the highest-ROI capture improvement is a wrapper
   canonicalize/greedy-driver pass that injects the recorder listener via
   `GreedyRewriteConfig` — no schema change needed (events are additive).

2. **Provenance performance is comfortable at 10⁴ ops.** First `GET
   /passes/{id}/ops` for a 573-op function across 6 passes (whole-pipeline
   resolve, the accepted M4b first-hit cost): **~280 ms debug**. Subsequent
   ops/history requests: ~1 ms. `op_index` was 16,828 rows; snapshot dedup
   kept the trace at 4 blobs / 1.1 MB. No perf work needed for M5 at this
   scale; re-measure at 10⁵ ops with a release build before declaring the
   §10 budget met.

3. **Empty history for unmatched-removed ops.** An op present only in one
   snapshot with no exact or inferred link (e.g. folded away wholesale)
   resolves to a history with `steps: []`. The UI renders an empty timeline
   with no explanation. M5 should either synthesize a terminal
   "disappeared in pass X (no evidence)" step or show an explicit empty
   state in the History view.

4. **Pipeline-anchor pitfall.** `mlir-trace-opt` with
   `builtin.module(canonicalize)` on a module-anchored PassManager nests a
   second module pass manager: the passes silently never run and the trace
   records only an `OpToOpPassAdaptor` with no leaves. Use unanchored
   pipelines (`canonicalize,cse`). A guard (warn when a Full-fidelity trace
   records zero executable leaves) would catch this class of user error.

5. **Dialect-conversion coverage remains unmeasured** — blocked on a fuller
   MLIR build (the local one has no conversion passes). Revisit when a full
   LLVM/MLIR install is available.
