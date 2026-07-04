#include "mlir-trace/TraceRecorder.h"

#include "TraceStorage.h"

#include "mlir/IR/MLIRContext.h"
#include "mlir/IR/Operation.h"
#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassInstrumentation.h"
#include "mlir/Pass/PassManager.h"
#include "llvm/Support/Threading.h"
#include "llvm/Support/raw_ostream.h"

#include <chrono>
#include <map>
#include <mutex>
#include <optional>
#include <string>
#include <tuple>
#include <utility>
#include <vector>

namespace mlir::trace {
namespace {

using Clock = std::chrono::steady_clock;

struct ExecutionKey {
  uint64_t threadId;
  Pass *pass;

  friend bool operator<(const ExecutionKey &lhs, const ExecutionKey &rhs) {
    return std::tie(lhs.threadId, lhs.pass) <
           std::tie(rhs.threadId, rhs.pass);
  }
};

struct ActivePass {
  int64_t id;
  int64_t parent;
  std::optional<int64_t> before;
};

} // namespace

class TraceRecorder::Impl {
public:
  Impl(llvm::StringRef path, TraceOptions options)
      : path(path.str()), options(options) {}

  class Instrumentation final : public PassInstrumentation {
  public:
    explicit Instrumentation(Impl &owner) : owner(owner) {}

    void runBeforePipeline(
        std::optional<OperationName> name,
        const PipelineParentInfo &parentInfo) final {
      (void)name;
      owner.beforePipeline(parentInfo);
    }

    void runAfterPipeline(std::optional<OperationName> name,
                          const PipelineParentInfo &parentInfo) final {
      (void)name;
      (void)parentInfo;
      owner.afterPipeline();
    }

    void runBeforePass(Pass *pass, Operation *operation) final {
      owner.beforePass(pass, operation);
    }

    void runAfterPass(Pass *pass, Operation *operation) final {
      owner.afterPass(pass, operation, false);
    }

    void runAfterPassFailed(Pass *pass, Operation *operation) final {
      owner.afterPass(pass, operation, true);
    }

  private:
    Impl &owner;
  };

  llvm::Error attach(PassManager &passManager, MLIRContext &context) {
    (void)context;
    std::lock_guard<std::mutex> lock(mutex);
    if (attached)
      return llvm::createStringError(llvm::inconvertibleErrorCode(),
                                     "trace recorder is already attached");

    auto storageOr = detail::TraceStorage::create(path);
    if (!storageOr)
      return storageOr.takeError();
    storage = std::move(*storageOr);

    if (llvm::Error error = storage->setMeta("producer", "libMLIRTrace 0.1"))
      return error;
    if (llvm::Error error = storage->setMeta(
            "fidelity", options.fidelity == Fidelity::Timeline ? "timeline"
                                                                 : "text"))
      return error;

    epoch = Clock::now();
    attached = true;
    passManager.addInstrumentation(std::make_unique<Instrumentation>(*this));
    return llvm::Error::success();
  }

  llvm::Error finish() {
    std::lock_guard<std::mutex> lock(mutex);
    if (finished)
      return llvm::Error::success();
    if (!attached)
      return llvm::createStringError(llvm::inconvertibleErrorCode(),
                                     "trace recorder is not attached");

    if (storage) {
      if (llvm::Error error = storage->finish())
        rememberError(std::move(error));
    }
    finished = true;
    if (firstError) {
      std::string message = std::move(*firstError);
      firstError.reset();
      return llvm::createStringError(llvm::inconvertibleErrorCode(), "%s",
                                     message.c_str());
    }
    return llvm::Error::success();
  }

private:
  int64_t nowNs() const {
    return std::chrono::duration_cast<std::chrono::nanoseconds>(Clock::now() -
                                                                epoch)
        .count();
  }

  void rememberError(llvm::Error error) {
    if (!error)
      return;
    if (!firstError)
      firstError = llvm::toString(std::move(error));
    else
      llvm::consumeError(std::move(error));
  }

  void beforePipeline(const PassInstrumentation::PipelineParentInfo &info) {
    std::lock_guard<std::mutex> lock(mutex);
    if (finished || firstError)
      return;

    std::optional<int64_t> parent;
    auto active = activePasses.find({info.parentThreadID, info.parentPass});
    if (active != activePasses.end())
      parent = active->second.id;
    pipelineParents[llvm::get_threadid()].push_back(parent);
  }

  void afterPipeline() {
    std::lock_guard<std::mutex> lock(mutex);
    auto parents = pipelineParents.find(llvm::get_threadid());
    if (parents == pipelineParents.end() || parents->second.empty())
      return;
    parents->second.pop_back();
    if (parents->second.empty())
      pipelineParents.erase(parents);
  }

  llvm::Expected<std::optional<int64_t>> snapshot(Operation *operation) {
    if (options.fidelity == Fidelity::Timeline)
      return std::optional<int64_t>();

    std::string text;
    llvm::raw_string_ostream stream(text);
    operation->print(stream);
    stream.flush();
    auto blobOr = storage->writeBlob(text);
    if (!blobOr)
      return blobOr.takeError();
    return std::optional<int64_t>(blobOr->value);
  }

  void beforePass(Pass *pass, Operation *operation) {
    std::lock_guard<std::mutex> lock(mutex);
    if (finished || firstError || !storage)
      return;

    const int64_t startNs = nowNs();
    auto beforeOr = snapshot(operation);
    if (!beforeOr) {
      rememberError(beforeOr.takeError());
      return;
    }
    std::optional<int64_t> before = *beforeOr;
    if (!rootPass) {
      auto rootOr =
          storage->beginPass(std::nullopt, 0, "Pipeline", before, startNs,
                             false);
      if (!rootOr) {
        rememberError(rootOr.takeError());
        return;
      }
      rootPass = rootOr->value;
      rootBefore = before;
    }

    int64_t parent = *rootPass;
    auto pipeline = pipelineParents.find(llvm::get_threadid());
    if (pipeline != pipelineParents.end() && !pipeline->second.empty() &&
        pipeline->second.back())
      parent = *pipeline->second.back();

    llvm::StringRef name = pass->getArgument();
    if (name.empty())
      name = pass->getName();
    const int64_t sequence = nextSequence[parent]++;
    auto passOr =
        storage->beginPass(parent, sequence, name, before, startNs, false);
    if (!passOr) {
      rememberError(passOr.takeError());
      return;
    }

    ExecutionKey key{llvm::get_threadid(), pass};
    if (!activePasses
             .emplace(key, ActivePass{passOr->value, parent, before})
             .second)
      rememberError(llvm::createStringError(
          llvm::inconvertibleErrorCode(), "pass instrumentation re-entered"));
  }

  void afterPass(Pass *pass, Operation *operation, bool failed) {
    std::lock_guard<std::mutex> lock(mutex);
    if (finished || firstError || !storage)
      return;

    ExecutionKey key{llvm::get_threadid(), pass};
    auto active = activePasses.find(key);
    if (active == activePasses.end()) {
      rememberError(llvm::createStringError(
          llvm::inconvertibleErrorCode(), "pass completed without start"));
      return;
    }

    const int64_t endNs = nowNs();
    std::optional<int64_t> after;
    if (!failed) {
      auto afterOr = snapshot(operation);
      if (!afterOr) {
        rememberError(afterOr.takeError());
        return;
      }
      after = *afterOr;
    }
    const bool changed =
        failed || (active->second.before && after &&
                   active->second.before.value() != after.value());

    if (llvm::Error error =
            storage->endPass(active->second.id, after, endNs, changed))
      rememberError(std::move(error));
    if (!firstError && rootPass && active->second.parent == *rootPass) {
      const bool rootChanged =
          failed || (rootBefore && after && rootBefore.value() != after.value());
      if (llvm::Error error = storage->endPass(*rootPass, after, endNs,
                                               rootChanged))
        rememberError(std::move(error));
    }
    if (failed && !firstError) {
      if (llvm::Error error =
              storage->setMeta("capture_status", "pass_failed"))
        rememberError(std::move(error));
    }
    activePasses.erase(active);
  }

  std::string path;
  TraceOptions options;
  Clock::time_point epoch;
  std::mutex mutex;
  std::unique_ptr<detail::TraceStorage> storage;
  std::optional<std::string> firstError;
  std::optional<int64_t> rootPass;
  std::optional<int64_t> rootBefore;
  std::map<ExecutionKey, ActivePass> activePasses;
  std::map<uint64_t, std::vector<std::optional<int64_t>>> pipelineParents;
  std::map<int64_t, int64_t> nextSequence;
  bool attached = false;
  bool finished = false;
};

TraceRecorder::TraceRecorder(llvm::StringRef path, TraceOptions options)
    : impl(std::make_unique<Impl>(path, options)) {}

TraceRecorder::~TraceRecorder() {
  if (impl)
    llvm::consumeError(impl->finish());
}

llvm::Error TraceRecorder::attach(PassManager &passManager,
                                  MLIRContext &context) {
  return impl->attach(passManager, context);
}

llvm::Error TraceRecorder::finish() { return impl->finish(); }

} // namespace mlir::trace
