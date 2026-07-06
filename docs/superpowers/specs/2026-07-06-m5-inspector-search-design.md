# M5 — Inspector, search, command palette, layout design

**Date:** 2026-07-06
**Status:** Approved (2026-07-06)
**Parent spec:** `2026-07-02-mlir-viewer-design.md` (§8 frontend, §13 M5)
**Foundation:** M4b provenance (`OpUid`, selectable ops, history) and
`2026-07-06-real-pipeline-validation-findings.md`

## 1. Goal

Daily-driver ergonomics. A user can open any op and see its full structure
(operands, results, types, attributes, regions), find anything by name from
the keyboard, arrange the workspace, and keep that arrangement across
sessions.

## 2. Decisions (proposed)

| Question | Decision |
|---|---|
| Inspector data source | Tolerant parser output only — no new schema tables; dense attrs summarized server-side |
| Inspector endpoint | `GET /api/ops/{uid}` per parent spec §7, MessagePack |
| Inspector placement | Right-hand dock panel; History becomes a tab inside it (replaces the toolbar History mode) |
| Search backend | Server-side, lazy per-snapshot index in `engine::search`; case-insensitive substring over op name, symbol, attr text |
| Search endpoint | `GET /api/search?q=…&pass=…&side=…&scope=pass\|pipeline&budget=200` |
| Command palette | cmdk; client-side fuzzy over passes/functions/view actions, server search for ops |
| Docking | dockview: timeline left, editor/graph center, inspector right, search results bottom |
| Layout persistence | localStorage (versioned key); "reset layout" action; no server persistence |
| Empty history | Synthesize a terminal `Disappeared` step when a chain has occurrences but no links (findings #3) |
| Keyboard | `[`/`]` pass step, `cmd-k` palette, `/` search, `Escape` closes panels |

## 3. Scope notes

- The Inspector reuses M4b's occurrence data (`attr_summary`, location) and
  extends the parser to expose operands/results/types and the region tree per
  op. Dense tensor literals are summarized (shape, dtype, first-k elements) —
  never shipped whole.
- Search results carry `uid` where resolvable so a result click selects the
  op and can open History; scope=pipeline searches executable leaves only and
  is budgeted, never exhaustive.
- Moving History into the Inspector keeps one selection model: select op →
  inspector shows structure + history tabs. The `Text | Graph` toolbar
  remains; `History` toolbar mode is removed.
- Layout presets ("debug diff", "graph review") are M7 polish, not M5.

## 4. Out of scope

- Stats dashboards, pattern-application tables (M7 per parent spec §8 bottom
  dock contents can start empty).
- CFG/call graphs, canvas LOD (M6).
- Capture-side improvements (listener-injecting canonicalize wrapper — see
  findings #1; schedule as capture work parallel to M5/M6).
- Cross-trace search, saved searches, regex/query language.

## 5. Testing sketch

Engine: search index build/query, budget, unicode symbols. Server: endpoint
contracts, uid resolution in results, 400/404. UI: inspector tabs, palette
navigation, layout save/restore/reset, empty-history state. E2E: select op →
inspect structure → search → jump → history → view IR, console-error-free.
