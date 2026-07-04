# M1 — MLIR Capture Library Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `libMLIRTrace`, a C++ MLIR pass instrumentation library that records timeline or text-fidelity traces directly into the M0 SQLite format and prove its output is readable by the Rust reader.

**Architecture:** A public `TraceRecorder` owns shared, thread-safe recording state and installs a private `PassInstrumentation` into the caller's `PassManager`. A private `TraceStorage` layer owns the SQLite/zstd/XXH3 contract and is independently exercised before MLIR callbacks are added. The real-pipeline integration test generates a trace with upstream MLIR dialects and validates it through the existing `mlir-viewer trace dump` binary.

**Tech Stack:** C++17, MLIR/LLVM 21 APIs, CMake 3.20+, SQLite3, zstd, xxHash 0.8.3, CTest, Rust M0 reader/CLI.

---

## Locked M1 decisions

- M1 implements only `timeline` and `text` fidelity. Structured operation capture and pattern/action tracing remain M3/M4 work.
- Trace files use M0 format version `1`, zstd level 3, and XXH3-64 of uncompressed UTF-8 text stored as eight big-endian bytes.
- Instrumentation callback failures are retained in shared state because MLIR callback methods cannot return errors; `TraceRecorder::finish()` returns the first error.
- Callback mutation is serialized with one mutex. This prioritizes correctness for nested and parallel pass managers; performance optimization follows measurement.
- A synthetic `Pipeline` root owns top-level pass executions. Nested pipeline passes are parented to the active pass supplied by `PipelineParentInfo`.
- Failed passes retain their before snapshot, null after snapshot, and end timestamp. The v1 schema has no pass-status column, so failure metadata is stored as `capture_status=pass_failed`.
- xxHash is fetched at the pinned `v0.8.3` tag when a system `xxhash.h`/library is unavailable. This keeps the source tree small while preserving an offline system-dependency path.

## Task 1: CMake project and exact trace storage contract

**Files:**
- Create: `capture/CMakeLists.txt`
- Create: `capture/cmake/FindZSTD.cmake`
- Create: `capture/include/mlir-trace/TraceRecorder.h`
- Create: `capture/lib/TraceStorage.h`
- Create: `capture/lib/TraceStorage.cpp`
- Create: `capture/tests/TraceStorageTest.cpp`
- Modify: `.gitignore`

- [ ] **Step 1: Add the build-system smoke test first**

Create `capture/tests/TraceStorageTest.cpp` with a test executable that exits nonzero unless it can create a trace, set metadata, deduplicate two equal blobs, record a root and child pass, finish the file, and observe that no `-wal` or `-shm` sidecars remain. Its `main` accepts exactly one output path so later tests can reuse the generated file.

The test uses this storage interface:

```cpp
auto storageOr = mlir::trace::detail::TraceStorage::create(argv[1]);
if (!storageOr) return fail(storageOr.takeError());
auto storage = std::move(*storageOr);
if (llvm::Error error = storage->setMeta("producer", "cpp-storage-test"))
  return fail(std::move(error));
auto before = storage->writeBlob("module { func.func @before() }");
auto duplicate = storage->writeBlob("module { func.func @before() }");
auto after = storage->writeBlob("module { func.func @after() }");
if (!before || !duplicate || !after || *before != *duplicate || *before == *after)
  return 1;
auto root = storage->beginPass(std::nullopt, 0, "Pipeline", before->value,
                               0, false);
if (!root) return fail(root.takeError());
auto child = storage->beginPass(root->value, 0, "canonicalize", before->value,
                                10, false);
if (!child) return fail(child.takeError());
if (llvm::Error error = storage->endPass(child->value, after->value, 20, true))
  return fail(std::move(error));
if (llvm::Error error = storage->endPass(root->value, after->value, 20, true))
  return fail(std::move(error));
if (llvm::Error error = storage->finish()) return fail(std::move(error));
return std::filesystem::exists(std::string(argv[1]) + "-wal") ||
       std::filesystem::exists(std::string(argv[1]) + "-shm");
```

- [ ] **Step 2: Add CMake configuration and verify the test cannot build**

`capture/CMakeLists.txt` must:

1. require CMake 3.20 and C++17,
2. locate LLVM/MLIR from `MLIR_DIR`,
3. locate SQLite3 and zstd,
4. locate a system xxHash or fetch tag `v0.8.3`,
5. define shared target `MLIRTrace`,
6. define `mlir-trace-storage-test` under `BUILD_TESTING`, and
7. register `storage_contract` with an output under the CMake binary tree.

Run:

```bash
cmake -S capture -B build/capture -G Ninja -DMLIR_DIR="$MLIR_DIR" -DBUILD_TESTING=ON
cmake --build build/capture --target mlir-trace-storage-test
```

Expected: FAIL because `TraceStorage` is not implemented.

- [ ] **Step 3: Implement `TraceStorage`**

Define these exact private types in `capture/lib/TraceStorage.h`:

```cpp
namespace mlir::trace::detail {
struct BlobId { int64_t value; friend bool operator==(BlobId, BlobId) = default; };
struct PassId { int64_t value; friend bool operator==(PassId, PassId) = default; };

class TraceStorage {
public:
  static llvm::Expected<std::unique_ptr<TraceStorage>> create(llvm::StringRef path);
  ~TraceStorage();
  llvm::Error setMeta(llvm::StringRef key, llvm::StringRef value);
  llvm::Expected<BlobId> writeBlob(llvm::StringRef text);
  llvm::Expected<PassId> beginPass(std::optional<int64_t> parent, int64_t seq,
                                   llvm::StringRef name,
                                   std::optional<int64_t> before,
                                   int64_t startNs, bool changed);
  llvm::Error endPass(int64_t id, std::optional<int64_t> after,
                      int64_t endNs, bool changed);
  llvm::Error finish();
private:
  explicit TraceStorage(sqlite3 *db) : db(db) {}
  sqlite3 *db = nullptr;
  bool finished = false;
};
} // namespace mlir::trace::detail
```

`TraceStorage.cpp` must apply the M0 DDL byte-for-byte in meaning, set `format_version=1`, enable WAL while recording, use prepared statements with bound values, compress with `ZSTD_compress(..., 3)`, and encode `XXH3_64bits` in network byte order. Every SQLite/zstd failure becomes an `llvm::StringError`. `finish()` executes `PRAGMA journal_mode=DELETE` and closes the database; the destructor closes an unfinished handle without throwing.

- [ ] **Step 4: Run the storage contract test**

Run:

```bash
cmake --build build/capture --target mlir-trace-storage-test
ctest --test-dir build/capture -R storage_contract --output-on-failure
```

Expected: PASS, one trace file, no WAL/SHM sidecars.

- [ ] **Step 5: Commit**

```bash
git add .gitignore capture
git commit -m "feat(capture): add v1 C++ trace storage"
```

## Task 2: Public recorder API and timeline instrumentation

**Files:**
- Modify: `capture/include/mlir-trace/TraceRecorder.h`
- Create: `capture/lib/TraceRecorder.cpp`
- Create: `capture/tests/TraceRecorderTest.cpp`
- Modify: `capture/CMakeLists.txt`

- [ ] **Step 1: Write the failing timeline test**

The test creates an `MLIRContext`, parses a builtin module, attaches the recorder, runs canonicalizer and CSE passes, calls `finish()`, and verifies via SQLite that:

- `meta.fidelity` is `timeline`,
- the synthetic `Pipeline` root exists,
- `canonicalize` and `cse` are children,
- all blob references are null, and
- every `end_ns >= start_ns`.

Use the public API exactly as follows:

```cpp
mlir::trace::TraceOptions options;
options.fidelity = mlir::trace::Fidelity::Timeline;
mlir::trace::TraceRecorder recorder(outputPath, options);
if (llvm::Error error = recorder.attach(passManager, context))
  return fail(std::move(error));
if (mlir::failed(passManager.run(*module))) return 1;
if (llvm::Error error = recorder.finish()) return fail(std::move(error));
```

Run the new test and expect a link failure because `TraceRecorder.cpp` does not exist yet.

- [ ] **Step 2: Implement the public header**

`capture/include/mlir-trace/TraceRecorder.h` exposes only:

```cpp
namespace mlir {
class MLIRContext;
class PassManager;
namespace trace {
enum class Fidelity { Timeline, Text };
struct TraceOptions { Fidelity fidelity = Fidelity::Text; };
class TraceRecorder {
public:
  TraceRecorder(llvm::StringRef path, TraceOptions options = {});
  ~TraceRecorder();
  TraceRecorder(const TraceRecorder &) = delete;
  TraceRecorder &operator=(const TraceRecorder &) = delete;
  llvm::Error attach(PassManager &passManager, MLIRContext &context);
  llvm::Error finish();
private:
  class Impl;
  std::unique_ptr<Impl> impl;
};
} // namespace trace
} // namespace mlir
```

- [ ] **Step 3: Implement timeline callbacks**

`TraceRecorder::Impl` stores the output path, options, epoch, mutex, storage, first error, root pass id, active executions keyed by `(thread id, Pass*)`, a pipeline-parent stack per thread, and a next-sequence counter per parent.

The instrumentation must implement:

- `runBeforePipeline`: resolve `PipelineParentInfo{parentThreadID,parentPass}` to an active pass id and push it for the current thread.
- `runAfterPipeline`: pop the current thread's pipeline-parent stack.
- `runBeforePass`: lazily create `Pipeline`, choose the active nested parent or root, allocate a per-parent sequence, and insert the pass row.
- `runAfterPass`: set end time with no blob ids and `ir_changed=false`.
- `runAfterPassFailed`: set end time, set `capture_status=pass_failed`, and preserve the first error.

Use `pass->getArgument()` when nonempty and `pass->getName()` otherwise. Use `llvm::get_threadid()` and nanoseconds from `std::chrono::steady_clock` relative to attach time.

- [ ] **Step 4: Run timeline tests**

Run:

```bash
cmake --build build/capture
ctest --test-dir build/capture -R 'storage_contract|recorder_timeline' --output-on-failure
```

Expected: both tests PASS.

- [ ] **Step 5: Commit**

```bash
git add capture
git commit -m "feat(capture): record MLIR pass timelines"
```

## Task 3: Text snapshots, deduplication, and failed-pass behavior

**Files:**
- Modify: `capture/lib/TraceRecorder.cpp`
- Modify: `capture/tests/TraceRecorderTest.cpp`

- [ ] **Step 1: Add failing text-fidelity assertions**

Run a module through a pass that changes the IR followed by a no-op pass. Verify:

- `meta.fidelity=text`,
- root and pass `ir_before`/`ir_after` are populated,
- the changed pass has distinct blob ids and `ir_changed=1`,
- the no-op pass shares its before/after blob id and has `ir_changed=0`,
- decompressed content contains `module` and `func.func`, and
- a deliberately failing pass leaves `ir_after IS NULL` while the database remains readable.

Run `recorder_text` and expect FAIL because timeline mode never writes blobs.

- [ ] **Step 2: Implement snapshot capture**

Add this helper and call it only in text fidelity:

```cpp
static std::string printOperation(Operation *operation) {
  std::string text;
  llvm::raw_string_ostream stream(text);
  operation->print(stream);
  stream.flush();
  return text;
}
```

In `runBeforePass`, print and write the before blob before inserting the pass row. In `runAfterPass`, print and write the after blob, compare blob ids, and update the row. The first top-level before blob initializes the root; each completed top-level pass refreshes the root after blob and end timestamp. `runAfterPassFailed` does not print potentially invalid IR.

All storage calls occur while holding the recorder mutex. Once an error occurs, callbacks stop writing but continue allowing the compiler pipeline to run; `finish()` returns the stored error after closing the database.

- [ ] **Step 3: Run all C++ tests**

Run:

```bash
cmake --build build/capture
ctest --test-dir build/capture --output-on-failure
```

Expected: storage, timeline, text, dedup, and failed-pass cases PASS.

- [ ] **Step 4: Commit**

```bash
git add capture
git commit -m "feat(capture): record deduplicated MLIR text snapshots"
```

## Task 4: Real example pipeline and Rust-reader conformance

**Files:**
- Create: `examples/capture-toy/CMakeLists.txt`
- Create: `examples/capture-toy/main.cpp`
- Create: `capture/tests/CheckRustReader.cmake`
- Modify: `capture/CMakeLists.txt`
- Modify: `crates/trace-format/tests/conformance.rs`

- [ ] **Step 1: Add the failing cross-language CTest**

Add cache variable `MLIR_VIEWER_BIN` and register a test that:

1. runs `mlir-trace-example <binary-dir>/cpp-demo.mlirtrace`,
2. runs `${MLIR_VIEWER_BIN} trace dump` on that file, and
3. fails unless output contains `Pipeline`, `canonicalize`, `cse`, and `(no change)`.

Configure with the existing Rust binary:

```bash
cargo build -p cli
cmake -S capture -B build/capture -G Ninja \
  -DMLIR_DIR="$MLIR_DIR" \
  -DMLIR_VIEWER_BIN="$PWD/target/debug/mlir-viewer" \
  -DBUILD_TESTING=ON
```

Run the test and expect FAIL because the example does not exist.

- [ ] **Step 2: Implement the toy MLIR pipeline**

`main.cpp` registers Builtin, Func, and Arith dialects; parses this module; adds canonicalizer then CSE; attaches a text recorder; runs and finishes:

```mlir
module {
  func.func @forward(%arg0: i32) -> i32 {
    %zero = arith.constant 0 : i32
    %result = arith.addi %arg0, %zero : i32
    return %result : i32
  }
}
```

The executable accepts exactly one trace output path and prints MLIR diagnostics on parse/pass failure.

- [ ] **Step 3: Add a durable Rust-side C++ conformance hook**

Extend `conformance.rs` with an ignored-by-default test named `cpp_generated_trace_is_v1_compatible`. It reads path `MLIR_TRACE_CPP_FIXTURE`, opens it with `TraceReader`, verifies the two pass names, and decompresses every present blob. This makes CI invocation explicit:

```bash
MLIR_TRACE_CPP_FIXTURE=build/capture/cpp-demo.mlirtrace \
  cargo test -p trace-format --test conformance cpp_generated_trace_is_v1_compatible -- --ignored
```

- [ ] **Step 4: Run both language directions**

Run:

```bash
ctest --test-dir build/capture --output-on-failure
MLIR_TRACE_CPP_FIXTURE=build/capture/cpp-demo.mlirtrace \
  cargo test -p trace-format --test conformance cpp_generated_trace_is_v1_compatible -- --ignored
cargo test --workspace
```

Expected: all CTest and Cargo tests PASS.

- [ ] **Step 5: Commit**

```bash
git add capture examples crates/trace-format/tests/conformance.rs
git commit -m "test(capture): prove C++ traces satisfy Contract 1"
```

## Task 5: Installable CMake package and integration documentation

**Files:**
- Create: `capture/cmake/MLIRTraceConfig.cmake.in`
- Create: `capture/README.md`
- Modify: `capture/CMakeLists.txt`

- [ ] **Step 1: Add a failing install-consumer test**

Add `capture/tests/install-consumer/CMakeLists.txt` and `main.cpp`. The consumer uses only:

```cmake
find_package(MLIRTrace CONFIG REQUIRED)
add_executable(consumer main.cpp)
target_link_libraries(consumer PRIVATE MLIRTrace::MLIRTrace)
```

Register a CTest script that installs to a temporary prefix, configures the consumer with `MLIRTrace_DIR=<prefix>/lib/cmake/MLIRTrace`, and builds it. Run it and expect FAIL before package export rules exist.

- [ ] **Step 2: Export the package**

Use `GNUInstallDirs` and `CMakePackageConfigHelpers` to install:

- `libMLIRTrace`,
- `include/mlir-trace/TraceRecorder.h`,
- `MLIRTraceTargets.cmake` under namespace `MLIRTrace::`, and
- version/config files under `${CMAKE_INSTALL_LIBDIR}/cmake/MLIRTrace`.

The config calls `find_dependency(MLIR CONFIG REQUIRED)` because the public target links MLIR pass APIs. SQLite, zstd, and xxHash remain private to the shared library.

- [ ] **Step 3: Document integration and fidelity semantics**

`capture/README.md` must include exact configure, build, install, `find_package`, recorder usage, SSH/post-mortem workflow, timeline/text semantics, failure propagation through `finish()`, and the requirement that `TraceRecorder` outlive `PassManager::run()`.

- [ ] **Step 4: Run final M1 verification**

Run:

```bash
cmake --build build/capture
ctest --test-dir build/capture --output-on-failure
cmake --install build/capture --prefix build/capture-install
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected: every command exits 0 and `git status --short` is clean after commit.

- [ ] **Step 5: Commit**

```bash
git add capture
git commit -m "docs(capture): package and document libMLIRTrace"
```

## Self-review

- **Spec coverage:** timeline and text fidelity, CMake package, real MLIR example, crash-tolerant incremental SQLite writes, deduplication, and Rust-reader conformance are each assigned to a task.
- **Deferred transparently:** structured operation rows, action/pattern tracing, identity, byte-offset indexes, and formal performance gates belong to M3/M4 or require the M2 server. No M1 API pre-builds those abstractions.
- **Type consistency:** storage ids are `int64_t`; public API uses `llvm::Error`; recorder and tests use the same `Fidelity` and `TraceOptions` names.
- **Environment contract:** configuration requires a built MLIR CMake package. Source headers alone are insufficient; local setup may build MLIR into a temporary directory without modifying the user's LLVM installation.
- **Placeholder scan:** no TBD/TODO steps or undefined public interfaces remain.
