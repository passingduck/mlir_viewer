#ifndef MLIR_TRACE_TRACE_STORAGE_H
#define MLIR_TRACE_TRACE_STORAGE_H

#include "llvm/ADT/StringRef.h"
#include "llvm/Support/Error.h"

#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <utility>

struct sqlite3;

namespace mlir::trace::detail {

struct BlobId {
  int64_t value;
  friend bool operator==(BlobId lhs, BlobId rhs) {
    return lhs.value == rhs.value;
  }
  friend bool operator!=(BlobId lhs, BlobId rhs) { return !(lhs == rhs); }
};

struct PassId {
  int64_t value;
  friend bool operator==(PassId lhs, PassId rhs) {
    return lhs.value == rhs.value;
  }
};

class TraceStorage {
public:
  static llvm::Expected<std::unique_ptr<TraceStorage>>
  create(llvm::StringRef path);

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
  TraceStorage(sqlite3 *database, std::string path)
      : database(database), path(std::move(path)) {}

  sqlite3 *database = nullptr;
  std::string path;
  bool finished = false;
};

} // namespace mlir::trace::detail

#endif // MLIR_TRACE_TRACE_STORAGE_H
