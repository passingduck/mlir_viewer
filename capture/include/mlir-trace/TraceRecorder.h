#ifndef MLIR_TRACE_TRACE_RECORDER_H
#define MLIR_TRACE_TRACE_RECORDER_H

#include "llvm/ADT/StringRef.h"
#include "llvm/Support/Error.h"
#include "mlir/IR/PatternMatch.h"

#include <memory>

namespace mlir {
class MLIRContext;
class PassManager;

namespace trace {

enum class Fidelity { Timeline, Text, Full };

struct TraceOptions {
  Fidelity fidelity = Fidelity::Text;
};

class TraceRecorder {
public:
  TraceRecorder(llvm::StringRef path, TraceOptions options = {});
  ~TraceRecorder();

  TraceRecorder(const TraceRecorder &) = delete;
  TraceRecorder &operator=(const TraceRecorder &) = delete;

  llvm::Error attach(PassManager &passManager, MLIRContext &context);
  /// Listener for cooperative pattern drivers such as GreedyRewriteConfig.
  /// The pointer remains valid for the lifetime of this recorder.
  RewriterBase::Listener *rewriteListener();
  llvm::Error finish();

private:
  class Impl;
  std::unique_ptr<Impl> impl;
};

} // namespace trace
} // namespace mlir

#endif // MLIR_TRACE_TRACE_RECORDER_H
