# M2 — Server and Viewer Walking Skeleton Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `mlir-viewer serve` with a bounded HTTP API and an embedded React UI that navigates the pass timeline and displays before/after MLIR text.

**Architecture:** A new Axum server crate opens the M0 trace through `TraceReader`, serializes control-plane JSON, and caps every IR response. A Vite/React frontend owns interaction and rendering only: it fetches the pass tree, selects a pass, and renders text in read-only CodeMirror editors. Production UI assets are embedded into the Rust binary after `npm run build`; Vite proxies API requests during development.

**Tech Stack:** Rust 2021, axum, tokio, serde, rust-embed; React, TypeScript, Vite, Zustand, CodeMirror 6, Vitest, Playwright.

---

## Locked M2 decisions

- Bind to `127.0.0.1:3000` by default. Remote access uses SSH port forwarding; public binding requires an explicit `--listen` value.
- JSON is sufficient for the M2 control plane and text pages. MessagePack begins when M3 introduces bulk structural payloads.
- IR endpoints accept byte `offset` and `limit`, cap `limit` at 256 KiB, and return UTF-8 boundary-safe pages with `next_offset`.
- The server opens a read-only `TraceReader` per request. This avoids sharing a rusqlite connection across async tasks; connection pooling is deferred until measurement requires it.
- M2 UI is a fixed responsive grid, not dockview. Docking and persistence remain M5 work.
- M2 ships a syntax-highlighted text viewer, not structural diff decorations. Diff belongs to M3.
- Generated `ui/dist` is ignored except for `.gitkeep`. Release builds must run `npm run build` before `cargo build` so rust-embed sees assets.

## Task 1: Reader lookup APIs

**Files:**
- Modify: `crates/trace-format/src/reader.rs`
- Modify: `crates/trace-format/src/lib.rs`

- [ ] Write failing tests for `TraceReader::pass(PassId)` and `TraceReader::blob_size(BlobId)`. Cover an existing pass, missing pass, and uncompressed size.
- [ ] Run `cargo test -p trace-format` and confirm compile failure for missing methods.
- [ ] Add `PassRecordView` with id, parent id, seq, name, blob ids, timestamps, and changed flag. Implement direct indexed queries without assembling the full forest.
- [ ] Return `TraceError::Corrupt("missing pass …")` and `TraceError::Corrupt("missing blob …")` for absent ids.
- [ ] Run workspace tests and commit as `feat(trace-format): add indexed pass lookup APIs`.

## Task 2: Bounded Axum trace API

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/server/Cargo.toml`
- Create: `crates/server/src/lib.rs`
- Create: `crates/server/src/api.rs`
- Create: `crates/server/src/assets.rs`
- Create: `crates/server/tests/api.rs`
- Create: `ui/dist/.gitkeep`
- Modify: `.gitignore`

- [ ] Add failing router integration tests using the M0 fixture. Required requests and assertions:

```text
GET /api/trace/info                         200; format_version=1; pass_count=6
GET /api/passes                             200; one Pipeline root; five children
GET /api/passes/{id}/ir?side=before         200; bounded text page
GET /api/passes/{id}/ir?side=invalid        400
GET /api/passes/999999/ir?side=before       404
GET /api/passes/{id}/ir?side=before&limit=0 400
```

- [ ] Run `cargo test -p server` and confirm failure because the crate/router is absent.
- [ ] Define `ServerState { trace_path: Arc<PathBuf> }` and `pub fn router(trace_path) -> Result<Router>`.
- [ ] Implement JSON DTOs independent of trace-format internals. Pass JSON fields are `id`, `name`, `start_ns`, `end_ns`, `ir_changed`, `ir_before`, `ir_after`, and recursive `children`.
- [ ] Implement `GET /api/trace/info`, `GET /api/passes`, and `GET /api/passes/{id}/ir`. Validate side, offset, and limit before reading a blob; cap limit at 262144 bytes and adjust start/end to UTF-8 character boundaries.
- [ ] Map invalid input to 400, missing pass/blob to 404, unsupported/corrupt trace to 422, and unexpected I/O to 500 with `{ "error": "…" }`.
- [ ] Add a rust-embed fallback for `ui/dist`; unknown non-API routes return `index.html`, while unknown API routes return JSON 404.
- [ ] Run server and workspace tests; commit as `feat(server): expose bounded trace HTTP API`.

## Task 3: `mlir-viewer serve`

**Files:**
- Modify: `crates/cli/Cargo.toml`
- Modify: `crates/cli/src/main.rs`
- Create: `crates/cli/tests/serve.rs`

- [ ] Add a failing CLI parser/unit test for:

```text
mlir-viewer serve TRACE --listen 127.0.0.1:0
```

The command must reject an unreadable trace before binding and print the actual bound URL to stderr.
- [ ] Refactor `main` into async Tokio execution while preserving `trace dump` and `dev gen-fixture` output.
- [ ] Add `Serve { file: PathBuf, listen: SocketAddr }`; bind a `TcpListener`, print `mlir-viewer listening on http://…`, and call `axum::serve` with the server router.
- [ ] Add a server shutdown integration test that starts on port 0, requests `/api/trace/info`, then aborts the task.
- [ ] Run workspace tests; commit as `feat(cli): serve trace files over HTTP`.

## Task 4: React shell and typed API client

**Files:**
- Create: `ui/package.json`
- Create: `ui/package-lock.json`
- Create: `ui/tsconfig.json`
- Create: `ui/vite.config.ts`
- Create: `ui/index.html`
- Create: `ui/src/main.tsx`
- Create: `ui/src/App.tsx`
- Create: `ui/src/api.ts`
- Create: `ui/src/store.ts`
- Create: `ui/src/styles.css`
- Create: `ui/src/test/setup.ts`
- Create: `ui/src/App.test.tsx`

- [ ] Scaffold Vite React/TypeScript dependencies and scripts: `dev`, `build`, `test`, and `typecheck`. Add Zustand, CodeMirror 6 packages, Vitest, Testing Library, jsdom, and Playwright.
- [ ] Write a failing component test with mocked fetch: loading state → pass tree render → first changed pass selected → before/after requests issued.
- [ ] Define exact TypeScript DTOs matching Task 2 and a fetch wrapper that throws an `ApiError` containing status and server message.
- [ ] Define Zustand state for trace info, pass roots, selected pass id, side pages, loading state, and error state. Keep server-derived payloads normalized by pass id.
- [ ] Build a semantic app shell with top status bar, left timeline region, center editor region, and visible loading/error/empty states.
- [ ] Run `npm run typecheck`, `npm test -- --run`, and `npm run build`; commit as `feat(ui): add viewer shell and typed trace client`.

## Task 5: Timeline navigation and CodeMirror IR viewer

**Files:**
- Create: `ui/src/components/Timeline.tsx`
- Create: `ui/src/components/Timeline.test.tsx`
- Create: `ui/src/components/IrViewer.tsx`
- Create: `ui/src/components/IrViewer.test.tsx`
- Create: `ui/src/mlirLanguage.ts`
- Modify: `ui/src/App.tsx`
- Modify: `ui/src/store.ts`
- Modify: `ui/src/styles.css`

- [ ] Write failing tests for recursive nesting, duration labels, changed/no-op badges, click selection, and `[`/`]` previous/next navigation over depth-first pass order.
- [ ] Implement an accessible tree with buttons, `aria-current`, keyboard focus, and stable pass ids. Duration is `(end_ns-start_ns)/1_000_000` with two decimals.
- [ ] Write failing tests that `IrViewer` creates two read-only CodeMirror instances, displays before/after headings, and shows a clear placeholder when a side has no snapshot.
- [ ] Implement a small MLIR StreamLanguage tokenizer for comments, SSA values, symbols, types, strings, numbers, and operation names. Do not build a semantic parser.
- [ ] Fetch an initial 256 KiB page for each side on selection. Show truncation state when `next_offset` is present; incremental paging UI is deferred but the API contract is ready.
- [ ] Apply a dense dark theme with responsive collapse below 900px and no animations that block interaction.
- [ ] Run typecheck, Vitest, and production build; commit as `feat(ui): navigate passes and inspect MLIR snapshots`.

## Task 6: Embedded single-binary end-to-end verification

**Files:**
- Create: `ui/playwright.config.ts`
- Create: `ui/e2e/viewer.spec.ts`
- Create: `scripts/build-ui.sh`
- Modify: `capture/README.md` only if the top-level workflow needs a corrected cross-link

- [ ] Add a Playwright test that generates the demo fixture, starts `mlir-viewer serve` on a free port, opens the page, selects `cse`, observes the no-change badge, selects `my-custom-fusion`, and observes `mycompiler.fused_matmul` in the after editor.
- [ ] Build UI, then force a server/CLI rebuild so rust-embed captures `ui/dist`.
- [ ] Run the browser test against the compiled binary, not Vite dev server.
- [ ] Verify `cargo fmt --check`, Clippy with `-D warnings`, Cargo workspace tests, UI typecheck/tests/build, Playwright, and `git diff --check`.
- [ ] Commit as `test: verify embedded viewer end to end`.

## Self-review

- **Spec coverage:** M2 proves CLI→HTTP API→embedded React→timeline→IR text end to end. Every response is bounded, and the browser never loads the full trace by default.
- **Scope control:** structural diff, inspector, provenance, search, docking, graphs, and MessagePack remain in their assigned later milestones.
- **Interfaces:** Rust and TypeScript DTO names match; pass ids and blob ids remain signed 64-bit on Rust and JSON numbers for M2 fixture-scale traces.
- **Build order:** UI production assets must precede the final Cargo build; tests state this explicitly.
- **Placeholder scan:** no TBD/TODO implementation steps remain.
