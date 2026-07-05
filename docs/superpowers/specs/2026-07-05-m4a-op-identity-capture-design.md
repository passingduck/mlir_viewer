# M4a — Op identity capture & schema design

**Date:** 2026-07-05
**Status:** Implemented and verified (2026-07-05)
**Parent spec:** `2026-07-02-mlir-viewer-design.md` (§4 fidelity, §5 trace format, §9 identity — the #1 technical risk, §13 M4)
**Base branch:** `main` (M3 graph+diff complete and merged)
**Target toolchain:** LLVM/MLIR **21.1.0-rc1** (installed at `/Users/sungjin/work/mlir-release`)

## 1. Goal

Retire the project's #1 technical risk (parent spec §9): prove that real op-lifecycle
events can be captured from MLIR 21.1 and persisted, with confidence provenance, into
the trace format. M4a is the **capture + data foundation**; it ships no user-visible
feature (like M0 was for text snapshots).

M4 is decomposed into two independently shippable sub-milestones:

- **M4a (this spec):** time-boxed C++ identity research spike, `Fidelity::Full`
  capture, trace schema v2 (identity events + op byte-index), and a synthetic
  identity fixture.
- **M4b (separate spec → plan):** Rust provenance engine stitching events + diff into
  per-op history chains, `GET /api/ops/{uid}/history`, and the Inspector panel +
  History tab. Built fixture-driven against M4a's synthetic trace, so it never blocks
  on C++ hook uncertainty.

## 2. Decisions made

| Question | Decision |
|---|---|
| M4 shape | **Split M4a (spike + schema + fixture) / M4b (engine + API + UI).** Each gets its own spec → plan → SDD cycle, so the large low-risk downstream never blocks on the risky C++ spike. |
| Identity model | **Pointer-keyed events; the Rust engine (M4b) stitches durable uids.** C++ emits raw lifecycle events keyed by an intra-pass `Operation*` token plus per-op byte ranges; all cross-pass reasoning and confidence interpretation live in one Rust place. C++ never assigns durable uids. |
| Reader compatibility | **Version-tolerant reader (≥ 1, identity tables optional).** Existing M3 (v1) traces still open; identity simply reads as empty. |
| Spike acceptance bar | **Capture insert/erase/replace/modify reliably; pattern attribution best-effort.** §9 explicitly allows graceful degradation, so M4a is not blocked on perfect attribution. |

## 3. Constraint that shapes everything

MLIR exposes op lifecycle unevenly (parent spec §9): `RewriterBase::Listener` fires only
for work done through a listening rewriter (greedy pattern driver, dialect conversion),
and the `Actions` framework gives pattern attribution — but there is **no universal
context-wide op lifecycle hook**. Therefore:

- Changes made outside a listening rewriter will have **no** identity events; those ops
  are stitched later by M4b's fingerprint matcher (the M3 diff engine) at match
  confidence. This is expected graceful degradation, not a defect.
- The trace stores identity as **events with a `source`** (parent spec §9.3), so capture
  improvements never require schema redesign — richer hooks just add higher-confidence
  events.

## 4. C++ capture (`capture/`)

Builds against real MLIR 21.1 (`find_package(MLIR CONFIG REQUIRED)`, already wired).

### 4.1 Fidelity ladder
Extend the enum `Timeline → Text → **Full**`. `Full` = Text snapshots **+** identity
events **+** op byte-index. Lower fidelities are unchanged (graceful degradation is a
feature, parent spec §4).

### 4.2 Hooks (installed on `attach` at `Fidelity::Full`)
- A `RewriterBase::Listener` capturing `notifyOperationInserted`,
  `notifyOperationErased`, `notifyOperationReplaced`, `notifyOperationModified`,
  installed where a pass-level rewriter is reachable.
- An `Actions` handler via `MLIRContext::registerActionHandler` to attribute the
  wrapping pattern/action name to concurrent events when present.

### 4.3 Op byte-index
At each snapshot, walk the printed IR and record every op's **byte range**
(`byte_start`, `byte_end`) into the side's `ir_blob` text, paired with the op's
intra-pass `Operation*` token. Identity events reference ops by that same token, so no
cross-pass reasoning happens in C++.

### 4.4 Spike deliverable
A short findings doc, `docs/superpowers/specs/M4a-identity-spike-findings.md`, recording:
which hooks actually fire on a real toy pipeline (canonicalize / CSE / DCE — passes that
use the pattern driver), at what fidelity, the observed event/pattern coverage, and
where capture falls back. Proven end-to-end through `examples/capture-toy`.

## 5. Trace schema v2 (`crates/trace-format`)

Bump `FORMAT_VERSION "1" → "2"`. The reader **accepts version ≥ 1**; the two new tables
are optional, so all existing M3 (v1) traces still open (identity reads as empty). Two
additive tables:

```sql
CREATE TABLE op_index (           -- op ↔ text location, per pass side
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    side       INTEGER NOT NULL,       -- 0 = before, 1 = after
    ptr_token  INTEGER NOT NULL,       -- intra-pass Operation* identity (opaque)
    byte_start INTEGER NOT NULL,
    byte_end   INTEGER NOT NULL,
    op_name    TEXT NOT NULL
);
CREATE INDEX idx_op_index_pass ON op_index(pass_id, side);

CREATE TABLE op_identity (        -- lifecycle event log
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    kind       TEXT NOT NULL,          -- inserted | erased | replaced | modified
    ptr_token  INTEGER NOT NULL,       -- subject op (pre-state token)
    new_token  INTEGER,                -- replacement op token, for 'replaced'
    pattern    TEXT,                    -- action/pattern name if attributed
    source     TEXT NOT NULL,          -- listener | action (provenance of THIS event)
    seq        INTEGER NOT NULL         -- ordering within the pass
);
CREATE INDEX idx_op_identity_pass ON op_identity(pass_id, seq);
```

- `ptr_token` is meaningful only **within a single pass's** event stream + that pass's
  `op_index` (pointers are reused after free). It links events ↔ op_index intra-pass.
- **Cross-pass** stitching (M4b) works off **blob content** — snapshots dedup by hash, so
  `ir_after`ₙ and `ir_before`ₙ₊₁ are the same blob — plus the byte ranges. The `source`
  column is what M4b maps to solid-vs-dashed link confidence. C++ assigns no durable uids.

The Rust writer gains methods to insert `op_index` / `op_identity` rows; the reader gains
tolerant version handling and (empty-safe) accessors for both tables.

## 6. Synthetic identity fixture (`crates/trace-format`, Rust)

So M4b is buildable and testable **without** the MLIR toolchain, add a `gen-fixture
--full` demo trace whose event stream is hand-authored but realistic:

- a canonicalize-style pass that **replaces** an op (listener source, with pattern name),
- a DCE-style pass that **erases** an op,
- an in-place **modify** of an op's attribute,
- matching `op_index` rows for both sides of each pass.

This is the decoupling seam: M4b's engine and UI tests run against this fixture, mirroring
how M3's engine was fixture-driven. The fixture is pure Rust — no C++/MLIR dependency.

## 7. HTTP API

None in M4a. `GET /api/ops/{uid}/history` and any identity-aware endpoints are M4b, once
durable uids exist.

## 8. Error handling & edge cases

- **Non-`Full` fidelity** → no identity tables written; reader returns empty identity;
  everything downstream degrades to M3 behavior.
- **v1 trace opened by a v2 reader** → accepted; identity accessors return empty.
- **Pass with no listening rewriter** → snapshots + op_index still recorded; zero identity
  events for that pass (expected; M4b falls back to fingerprint matching).
- **`replaced` with a null/erased replacement** → `new_token` stored NULL; M4b treats as
  erase-then-nothing.
- **Op that spans a snapshot re-print oddly** (byte ranges non-contiguous) → op_index
  records the printed range of the top-level op; nested ops indexed independently.

## 9. Testing

- **C++:** recorder integration test on a real toy pipeline (gated on MLIR available in
  CI) asserting the expected event kinds + pattern attribution land in the SQLite trace;
  a `Fidelity::Text` run asserts **no** identity tables are populated.
- **Rust:** schema v2 round-trip (write → read `op_index`/`op_identity`); **v1
  back-compat** (an M3 trace opens, identity empty); `gen-fixture --full` produces a
  well-formed identity stream with the three scripted scenarios; writer/reader symmetry.

## 10. Out of scope (deferred)

- Provenance engine, uid resolution, confidence-to-link mapping (M4b).
- `GET /api/ops/{uid}/history` and Inspector/History UI (M4b).
- Structural `operation` / `op_attr` tables (parent spec §5) — M4b/M5 stitch identity
  onto M3's tolerant-parser op tables rather than requiring full structural rows now.
- Dialect-conversion and non-rewriter pass coverage beyond what the spike observes;
  captured improvements slot in as higher-confidence events without schema change.
