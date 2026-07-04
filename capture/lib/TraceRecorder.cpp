#include "mlir-trace/TraceRecorder.h"

#include "TraceStorage.h"

#include "mlir/IR/MLIRContext.h"
#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassInstrumentation.h"
#include "mlir/Pass/PassManager.h"
#include "llvm/Support/Threading.h"

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
      (void)operation;
      owner.beforePass(pass);
    }

    void runAfterPass(Pass *pass, Operation *operation) final {
      (void)operation;
      owner.afterPass(pass, false);
    }

    void runAfterPassFailed(Pass *pass, Operation *operation) final {
      (void)operation;
      owner.afterPass(pass, true);
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

  void beforePass(Pass *pass) {
    std::lock_guard<std::mutex> lock(mutex);
    if (finished || firstError || !storage)
      return;

    const int64_t startNs = nowNs();
    if (!rootPass) {
      auto rootOr =
          storage->beginPass(std::nullopt, 0, "Pipeline", std::nullopt,
                             startNs, false);
      if (!rootOr) {
        rememberError(rootOr.takeError());
        return;
      }
      rootPass = rootOr->value;
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
    auto passOr = storage->beginPass(parent, sequence, name, std::nullopt,
                                     startNs, false);
    if (!passOr) {
      rememberError(passOr.takeError());
      return;
    }

    ExecutionKey key{llvm::get_threadid(), pass};
    if (!activePasses.emplace(key, ActivePass{passOr->value, parent}).second)
      rememberError(llvm::createStringError(
          llvm::inconvertibleErrorCode(), "pass instrumentation re-entered"));
  }

  void afterPass(Pass *pass, bool failed) {
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
    if (llvm::Error error = storage->endPass(active->second.id, std::nullopt,
                                             endNs, failed))
      rememberError(std::move(error));
    if (!firstError && rootPass && active->second.parent == *rootPass) {
      if (llvm::Error error =
              storage->endPass(*rootPass, std::nullopt, endNs, failed))
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
