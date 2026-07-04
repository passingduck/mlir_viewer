#include "TraceStorage.h"

#include "llvm/Support/Error.h"
#include "llvm/Support/raw_ostream.h"

#include <filesystem>
#include <optional>
#include <string>

using mlir::trace::detail::BlobId;
using mlir::trace::detail::PassId;
using mlir::trace::detail::TraceStorage;

static int fail(llvm::Error error) {
  llvm::errs() << llvm::toString(std::move(error)) << '\n';
  return 1;
}

template <typename T>
static std::optional<T> unwrap(llvm::Expected<T> value) {
  if (!value) {
    llvm::errs() << llvm::toString(value.takeError()) << '\n';
    return std::nullopt;
  }
  return std::move(*value);
}

int main(int argc, char **argv) {
  if (argc != 2) {
    llvm::errs() << "usage: mlir-trace-storage-test OUTPUT\n";
    return 2;
  }

  auto storageOr = TraceStorage::create(argv[1]);
  if (!storageOr)
    return fail(storageOr.takeError());
  std::unique_ptr<TraceStorage> storage = std::move(*storageOr);

  if (llvm::Error error = storage->setMeta("producer", "cpp-storage-test"))
    return fail(std::move(error));

  auto before = unwrap(storage->writeBlob("module { func.func @before() }"));
  auto duplicate = unwrap(storage->writeBlob("module { func.func @before() }"));
  auto after = unwrap(storage->writeBlob("module { func.func @after() }"));
  if (!before || !duplicate || !after || *before != *duplicate ||
      *before == *after)
    return 1;

  auto root = unwrap(storage->beginPass(std::nullopt, 0, "Pipeline",
                                        before->value, 0, false));
  if (!root)
    return 1;
  auto child = unwrap(storage->beginPass(root->value, 0, "canonicalize",
                                         before->value, 10, false));
  if (!child)
    return 1;

  if (llvm::Error error =
          storage->endPass(child->value, after->value, 20, true))
    return fail(std::move(error));
  if (llvm::Error error =
          storage->endPass(root->value, after->value, 20, true))
    return fail(std::move(error));
  if (llvm::Error error = storage->finish())
    return fail(std::move(error));

  const std::string path = argv[1];
  return std::filesystem::exists(path + "-wal") ||
                 std::filesystem::exists(path + "-shm")
             ? 1
             : 0;
}
