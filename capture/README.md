# libMLIRTrace

`libMLIRTrace` records an MLIR pass pipeline into a post-mortem
`.mlirtrace` file. It links into the compiler process, so custom dialects and
custom printers are available when snapshots are captured. The viewer itself
does not link LLVM or MLIR.

## Prerequisites

- CMake 3.20 or newer and Ninja
- a built MLIR tree that exports `MLIRConfig.cmake`
- SQLite3 development headers and library
- zstd development headers and library
- a C++17 compiler compatible with the MLIR build

xxHash 0.8.3 is fetched at configure time when `xxhash.h` is not installed.
For offline builds, install the xxHash development headers or pass
`-DXXHASH_INCLUDE_DIR=/path/to/include`.

## Build and test

Build the Rust CLI first because the cross-language test uses it to read a C++
trace:

```bash
cargo build -p cli
cmake -S capture -B build/capture -G Ninja \
  -DMLIR_DIR=/path/to/mlir/lib/cmake/mlir \
  -DMLIR_VIEWER_BIN="$PWD/target/debug/mlir-viewer" \
  -DBUILD_TESTING=ON
cmake --build build/capture
ctest --test-dir build/capture --output-on-failure
```

The tests cover SQLite format compatibility, blob deduplication, timeline and
text capture, failed passes, nested pass managers, a real canonicalizer/CSE
pipeline, and an installed-package consumer.

## Install and consume

```bash
cmake --install build/capture --prefix /path/to/mlir-trace-install
```

Consumer `CMakeLists.txt`:

```cmake
find_package(MLIRTrace CONFIG REQUIRED)
target_link_libraries(your_compiler PRIVATE MLIRTrace::MLIRTrace)
```

Configure the consumer with both packages:

```bash
cmake -S . -B build -G Ninja \
  -DMLIR_DIR=/path/to/mlir/lib/cmake/mlir \
  -DMLIRTrace_DIR=/path/to/mlir-trace-install/lib/cmake/MLIRTrace
```

## Integrate with a pass manager

```cpp
#include "mlir-trace/TraceRecorder.h"

mlir::PassManager passManager(&context);
// Add the compiler's passes first.

mlir::trace::TraceOptions options;
options.fidelity = mlir::trace::Fidelity::Text;
mlir::trace::TraceRecorder recorder("compile.mlirtrace", options);

if (llvm::Error error = recorder.attach(passManager, context))
  return std::move(error);
if (mlir::failed(passManager.run(module)))
  return llvm::createStringError(llvm::inconvertibleErrorCode(),
                                 "pass pipeline failed");
if (llvm::Error error = recorder.finish())
  return std::move(error);
```

`TraceRecorder` must outlive `PassManager::run()`. MLIR instrumentation
callbacks cannot return errors, so callback failures are retained and surfaced
by `finish()`. Always call `finish()` and check its result; the destructor only
performs best-effort cleanup.

## Fidelity

- `Fidelity::Timeline` records pass names, nesting, sequence, and monotonic
  timing. Blob references are null and `ir_changed` is not inferred.
- `Fidelity::Text` additionally prints the operation before and after each
  pass. Snapshots are zstd-compressed and content-addressed with XXH3-64, so a
  no-op pass reuses one blob and records `ir_changed=0`.

If a pass fails, its before snapshot remains readable, its after snapshot is
null, and `meta.capture_status` is `pass_failed`. A finished trace switches out
of WAL mode and is a single copyable file. During compilation, SQLite WAL keeps
the partial trace recoverable after a crash.

## Remote workflow

Generate the trace on the remote compiler machine, then inspect it there over
an SSH tunnel or copy the single finished file locally:

```bash
mlir-viewer trace dump compile.mlirtrace
```

The C++ library and Rust reader share only the versioned SQLite contract. A
breaking schema change requires a new major `format_version`; version 1 changes
remain additive.
