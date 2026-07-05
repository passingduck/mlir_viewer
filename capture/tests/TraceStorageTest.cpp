#include "TraceStorage.h"

#include "llvm/Support/Error.h"
#include "llvm/Support/raw_ostream.h"

#include <sqlite3.h>

#include <filesystem>
#include <optional>
#include <string>

using mlir::trace::detail::BlobId;
using mlir::trace::detail::PassId;
using mlir::trace::detail::TraceStorage;

static bool scalarEquals(sqlite3 *database, const char *sql,
                         const char *expected) {
  sqlite3_stmt *statement = nullptr;
  if (sqlite3_prepare_v2(database, sql, -1, &statement, nullptr) != SQLITE_OK)
    return false;
  const bool matches = sqlite3_step(statement) == SQLITE_ROW &&
                       std::string(reinterpret_cast<const char *>(
                           sqlite3_column_text(statement, 0))) == expected;
  sqlite3_finalize(statement);
  return matches;
}

static bool scalarEquals(sqlite3 *database, const char *sql,
                         int64_t expected) {
  sqlite3_stmt *statement = nullptr;
  if (sqlite3_prepare_v2(database, sql, -1, &statement, nullptr) != SQLITE_OK)
    return false;
  const bool matches = sqlite3_step(statement) == SQLITE_ROW &&
                       sqlite3_column_int64(statement, 0) == expected;
  sqlite3_finalize(statement);
  return matches;
}

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

  if (llvm::Error error = storage->writeOpIndex(
          child->value, 1, 4096, 0, 12, "arith.constant"))
    return fail(std::move(error));
  if (llvm::Error error = storage->writeIdentityEvent(
          child->value, "erased", 4096, std::nullopt,
          std::optional<llvm::StringRef>(llvm::StringRef("DCE")), "listener",
          0))
    return fail(std::move(error));

  if (llvm::Error error =
          storage->endPass(child->value, after->value, 20, true))
    return fail(std::move(error));
  if (llvm::Error error =
          storage->endPass(root->value, after->value, 20, true))
    return fail(std::move(error));
  if (llvm::Error error = storage->finish())
    return fail(std::move(error));

  sqlite3 *database = nullptr;
  if (sqlite3_open_v2(argv[1], &database, SQLITE_OPEN_READONLY, nullptr) !=
      SQLITE_OK)
    return 1;
  const bool valid =
      scalarEquals(database,
                   "SELECT value FROM meta WHERE key='format_version'", "2") &&
      scalarEquals(database,
                   "SELECT count(*) FROM op_index WHERE "
                   "op_name='arith.constant'",
                   1) &&
      scalarEquals(database,
                   "SELECT count(*) FROM op_identity WHERE kind='erased' "
                   "AND source='listener'",
                   1);
  sqlite3_close(database);
  if (!valid)
    return 1;

  const std::string path = argv[1];
  return std::filesystem::exists(path + "-wal") ||
                 std::filesystem::exists(path + "-shm")
             ? 1
             : 0;
}
