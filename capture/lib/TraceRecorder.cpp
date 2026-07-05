#include "mlir-trace/TraceRecorder.h"

#include "TraceStorage.h"

#include "mlir/IR/MLIRContext.h"
#include "mlir/IR/Operation.h"
#include "mlir/IR/PatternMatch.h"
#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassInstrumentation.h"
#include "mlir/Pass/PassManager.h"
#include "llvm/Support/Threading.h"
#include "llvm/Support/raw_ostream.h"

#include <chrono>
#include <cstdint>
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
      : path(path.str()), options(options), listener(*this) {}

  class RewriteListener final : public RewriterBase::Listener {
  public:
    explicit RewriteListener(Impl &owner) : owner(owner) {}

    void notifyOperationInserted(Operation *operation,
                                 OpBuilder::InsertPoint previous) final {
      (void)previous;
      owner.recordIdentity("inserted", operation, nullptr);
    }

    void notifyOperationErased(Operation *operation) final {
      owner.recordIdentity("erased", operation, nullptr);
    }

    void notifyOperationReplaced(Operation *operation,
                                 Operation *replacement) final {
      owner.recordIdentity("replaced", operation, replacement);
    }

    void notifyOperationReplaced(Operation *operation,
                                 ValueRange replacements) final {
      Operation *replacement = nullptr;
      if (!replacements.empty())
        replacement = replacements.front().getDefiningOp();
      owner.recordIdentity("replaced", operation, replacement);
    }

    void notifyOperationModified(Operation *operation) final {
      owner.recordIdentity("modified", operation, nullptr);
    }

    void notifyPatternBegin(const Pattern &pattern, Operation *operation) final {
      (void)operation;
      owner.beginPattern(pattern.getDebugName());
    }

    void notifyPatternEnd(const Pattern &pattern, LogicalResult status) final {
      (void)pattern;
      (void)status;
      owner.endPattern();
    }

  private:
    Impl &owner;
  };

  RewriterBase::Listener *rewriteListener() { return &listener; }

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
    const char *fidelityName = "text";
    if (options.fidelity == Fidelity::Timeline)
      fidelityName = "timeline";
    else if (options.fidelity == Fidelity::Full)
      fidelityName = "full";
    if (llvm::Error error = storage->setMeta("fidelity", fidelityName))
      return error;

    epoch = Clock::now();
    attached = true;
    if (options.fidelity == Fidelity::Full && !context.hasActionHandler()) {
      context.registerActionHandler(
          [this](llvm::function_ref<void()> actionFn,
                 const tracing::Action &action) {
            std::string description;
            llvm::raw_string_ostream stream(description);
            action.print(stream);
            stream.flush();
            beginAction(description);
            actionFn();
            endAction();
          });
      actionContext = &context;
    }
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

    if (actionContext) {
      actionContext->registerActionHandler(nullptr);
      actionContext = nullptr;
    }

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
  static int64_t token(Operation *operation) {
    return static_cast<int64_t>(reinterpret_cast<intptr_t>(operation));
  }

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

  void writeOpIndex(int64_t passId, int side, Operation *operation) {
    if (options.fidelity != Fidelity::Full)
      return;

    int64_t ordinal = 0;
    operation->walk<WalkOrder::PreOrder>([&](Operation *nested) {
      if (firstError)
        return WalkResult::interrupt();
      if (llvm::Error error = storage->writeOpIndex(
              passId, side, token(nested), ordinal++, -1,
              nested->getName().getStringRef())) {
        rememberError(std::move(error));
        return WalkResult::interrupt();
      }
      return WalkResult::advance();
    });
  }

  void beginPattern(llvm::StringRef pattern) {
    std::lock_guard<std::mutex> lock(mutex);
    currentPatterns[llvm::get_threadid()].push_back(pattern.str());
  }

  void endPattern() {
    std::lock_guard<std::mutex> lock(mutex);
    auto patterns = currentPatterns.find(llvm::get_threadid());
    if (patterns == currentPatterns.end() || patterns->second.empty())
      return;
    patterns->second.pop_back();
    if (patterns->second.empty())
      currentPatterns.erase(patterns);
  }

  void beginAction(llvm::StringRef action) {
    std::lock_guard<std::mutex> lock(mutex);
    currentActions[llvm::get_threadid()].push_back(action.str());
  }

  void endAction() {
    std::lock_guard<std::mutex> lock(mutex);
    auto actions = currentActions.find(llvm::get_threadid());
    if (actions == currentActions.end() || actions->second.empty())
      return;
    actions->second.pop_back();
    if (actions->second.empty())
      currentActions.erase(actions);
  }

  void recordIdentity(llvm::StringRef kind, Operation *operation,
                      Operation *replacement) {
    std::lock_guard<std::mutex> lock(mutex);
    if (options.fidelity != Fidelity::Full || finished || firstError ||
        !storage)
      return;

    const uint64_t threadId = llvm::get_threadid();
    auto passes = activePassIds.find(threadId);
    if (passes == activePassIds.end() || passes->second.empty())
      return;
    const int64_t passId = passes->second.back();

    std::optional<llvm::StringRef> pattern;
    auto currentPattern = currentPatterns.find(threadId);
    if (currentPattern != currentPatterns.end() &&
        !currentPattern->second.empty())
      pattern = currentPattern->second.back();
    auto currentAction = currentActions.find(threadId);
    if (!pattern && currentAction != currentActions.end() &&
        !currentAction->second.empty())
      pattern = currentAction->second.back();
    const llvm::StringRef source =
        currentAction != currentActions.end() && !currentAction->second.empty()
            ? llvm::StringRef("action")
            : llvm::StringRef("listener");

    const std::optional<int64_t> replacementToken =
        replacement ? std::optional<int64_t>(token(replacement))
                    : std::nullopt;
    if (llvm::Error error = storage->writeIdentityEvent(
            passId, kind, token(operation), replacementToken, pattern, source,
            nextIdentitySequence[passId]++))
      rememberError(std::move(error));
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
    if (!firstError) {
      activePassIds[llvm::get_threadid()].push_back(passOr->value);
      writeOpIndex(passOr->value, /*side=*/0, operation);
    }
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
      writeOpIndex(active->second.id, /*side=*/1, operation);
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
    auto passIds = activePassIds.find(llvm::get_threadid());
    if (passIds != activePassIds.end() && !passIds->second.empty()) {
      passIds->second.pop_back();
      if (passIds->second.empty())
        activePassIds.erase(passIds);
    }
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
  std::map<uint64_t, std::vector<int64_t>> activePassIds;
  std::map<uint64_t, std::vector<std::optional<int64_t>>> pipelineParents;
  std::map<int64_t, int64_t> nextSequence;
  std::map<int64_t, int64_t> nextIdentitySequence;
  std::map<uint64_t, std::vector<std::string>> currentPatterns;
  std::map<uint64_t, std::vector<std::string>> currentActions;
  RewriteListener listener;
  MLIRContext *actionContext = nullptr;
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

RewriterBase::Listener *TraceRecorder::rewriteListener() {
  return impl->rewriteListener();
}

llvm::Error TraceRecorder::finish() { return impl->finish(); }

} // namespace mlir::trace
