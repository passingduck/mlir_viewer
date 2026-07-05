#include "TraceStorage.h"

#include "llvm/Support/Error.h"

#include <sqlite3.h>
#define XXH_INLINE_ALL
#include <xxhash.h>
#include <zstd.h>

#include <array>
#include <filesystem>
#include <string>
#include <utility>
#include <vector>

namespace mlir::trace::detail {
namespace {

constexpr llvm::StringLiteral schemaSql = R"sql(
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE ir_blob (
    id          INTEGER PRIMARY KEY,
    hash        BLOB NOT NULL UNIQUE,
    size_bytes  INTEGER NOT NULL,
    compression TEXT NOT NULL,
    data        BLOB NOT NULL
);

CREATE TABLE pass_execution (
    id         INTEGER PRIMARY KEY,
    parent_id  INTEGER REFERENCES pass_execution(id),
    seq        INTEGER NOT NULL,
    name       TEXT NOT NULL,
    ir_before  INTEGER REFERENCES ir_blob(id),
    ir_after   INTEGER REFERENCES ir_blob(id),
    start_ns   INTEGER NOT NULL,
    end_ns     INTEGER NOT NULL,
    ir_changed INTEGER NOT NULL
);

CREATE INDEX idx_pass_parent ON pass_execution(parent_id, seq);

CREATE TABLE op_index (
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    side       INTEGER NOT NULL,
    ptr_token  INTEGER NOT NULL,
    byte_start INTEGER NOT NULL,
    byte_end   INTEGER NOT NULL,
    op_name    TEXT NOT NULL
);
CREATE INDEX idx_op_index_pass ON op_index(pass_id, side);

CREATE TABLE op_identity (
    id         INTEGER PRIMARY KEY,
    pass_id    INTEGER NOT NULL REFERENCES pass_execution(id),
    kind       TEXT NOT NULL,
    ptr_token  INTEGER NOT NULL,
    new_token  INTEGER,
    pattern    TEXT,
    source     TEXT NOT NULL,
    seq        INTEGER NOT NULL
);
CREATE INDEX idx_op_identity_pass ON op_identity(pass_id, seq);
)sql";

llvm::Error makeSqliteError(sqlite3 *database, llvm::StringRef action) {
  return llvm::createStringError(llvm::inconvertibleErrorCode(), "%s: %s",
                                 action.str().c_str(),
                                 sqlite3_errmsg(database));
}

llvm::Error execute(sqlite3 *database, llvm::StringRef sql,
                    llvm::StringRef action) {
  char *message = nullptr;
  const int result = sqlite3_exec(database, sql.str().c_str(), nullptr, nullptr,
                                  &message);
  if (result == SQLITE_OK)
    return llvm::Error::success();

  std::string detail = message ? message : sqlite3_errmsg(database);
  sqlite3_free(message);
  return llvm::createStringError(llvm::inconvertibleErrorCode(), "%s: %s",
                                 action.str().c_str(), detail.c_str());
}

class Statement {
public:
  Statement() = default;
  ~Statement() {
    if (statement)
      sqlite3_finalize(statement);
  }

  Statement(const Statement &) = delete;
  Statement &operator=(const Statement &) = delete;

  static llvm::Expected<std::unique_ptr<Statement>>
  prepare(sqlite3 *database, llvm::StringRef sql) {
    auto result = std::unique_ptr<Statement>(new Statement());
    if (sqlite3_prepare_v2(database, sql.data(), static_cast<int>(sql.size()),
                           &result->statement, nullptr) != SQLITE_OK)
      return makeSqliteError(database, "prepare statement");
    return std::move(result);
  }

  sqlite3_stmt *get() const { return statement; }

private:
  sqlite3_stmt *statement = nullptr;
};

llvm::Error bindText(sqlite3 *database, sqlite3_stmt *statement, int index,
                     llvm::StringRef value) {
  if (sqlite3_bind_text(statement, index, value.data(),
                        static_cast<int>(value.size()), SQLITE_TRANSIENT) !=
      SQLITE_OK)
    return makeSqliteError(database, "bind text");
  return llvm::Error::success();
}

llvm::Error bindOptionalInt(sqlite3 *database, sqlite3_stmt *statement,
                            int index, std::optional<int64_t> value) {
  const int result = value ? sqlite3_bind_int64(statement, index, *value)
                           : sqlite3_bind_null(statement, index);
  if (result != SQLITE_OK)
    return makeSqliteError(database, "bind integer");
  return llvm::Error::success();
}

std::array<unsigned char, 8> encodeBigEndian(uint64_t value) {
  std::array<unsigned char, 8> bytes{};
  for (unsigned index = 0; index < bytes.size(); ++index)
    bytes[bytes.size() - 1 - index] =
        static_cast<unsigned char>((value >> (index * 8)) & 0xff);
  return bytes;
}

} // namespace

llvm::Expected<std::unique_ptr<TraceStorage>>
TraceStorage::create(llvm::StringRef pathRef) {
  const std::string path = pathRef.str();
  std::error_code filesystemError;
  std::filesystem::remove(path, filesystemError);
  if (filesystemError)
    return llvm::createStringError(filesystemError, "remove existing trace");
  std::filesystem::remove(path + "-wal", filesystemError);
  filesystemError.clear();
  std::filesystem::remove(path + "-shm", filesystemError);

  sqlite3 *database = nullptr;
  const int openResult = sqlite3_open_v2(
      path.c_str(), &database, SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
      nullptr);
  if (openResult != SQLITE_OK) {
    std::string message = database ? sqlite3_errmsg(database)
                                   : "sqlite returned no database handle";
    if (database)
      sqlite3_close(database);
    return llvm::createStringError(llvm::inconvertibleErrorCode(),
                                   "open trace: %s", message.c_str());
  }

  auto storage =
      std::unique_ptr<TraceStorage>(new TraceStorage(database, path));
  if (llvm::Error error = execute(database, "PRAGMA journal_mode=WAL",
                                  "enable WAL journal mode"))
    return std::move(error);
  if (llvm::Error error = execute(database, schemaSql, "create v2 schema"))
    return std::move(error);
  if (llvm::Error error = storage->setMeta("format_version", "2"))
    return std::move(error);
  return std::move(storage);
}

TraceStorage::~TraceStorage() {
  if (database)
    sqlite3_close(database);
}

llvm::Error TraceStorage::setMeta(llvm::StringRef key,
                                  llvm::StringRef value) {
  auto statementOr = Statement::prepare(
      database,
      "INSERT INTO meta(key, value) VALUES (?1, ?2) "
      "ON CONFLICT(key) DO UPDATE SET value = excluded.value");
  if (!statementOr)
    return statementOr.takeError();
  auto statement = std::move(*statementOr);

  if (llvm::Error error = bindText(database, statement->get(), 1, key))
    return error;
  if (llvm::Error error = bindText(database, statement->get(), 2, value))
    return error;
  if (sqlite3_step(statement->get()) != SQLITE_DONE)
    return makeSqliteError(database, "write metadata");
  return llvm::Error::success();
}

llvm::Expected<BlobId> TraceStorage::writeBlob(llvm::StringRef text) {
  const auto hash = encodeBigEndian(XXH3_64bits(text.data(), text.size()));

  auto selectOr =
      Statement::prepare(database, "SELECT id FROM ir_blob WHERE hash = ?1");
  if (!selectOr)
    return selectOr.takeError();
  auto select = std::move(*selectOr);
  if (sqlite3_bind_blob(select->get(), 1, hash.data(), hash.size(),
                        SQLITE_TRANSIENT) != SQLITE_OK)
    return makeSqliteError(database, "bind blob hash");

  int result = sqlite3_step(select->get());
  if (result == SQLITE_ROW)
    return BlobId{sqlite3_column_int64(select->get(), 0)};
  if (result != SQLITE_DONE)
    return makeSqliteError(database, "query blob hash");

  std::vector<unsigned char> compressed(ZSTD_compressBound(text.size()));
  const size_t compressedSize =
      ZSTD_compress(compressed.data(), compressed.size(), text.data(),
                    text.size(), 3);
  if (ZSTD_isError(compressedSize))
    return llvm::createStringError(llvm::inconvertibleErrorCode(),
                                   "compress IR blob: %s",
                                   ZSTD_getErrorName(compressedSize));
  compressed.resize(compressedSize);

  auto insertOr = Statement::prepare(
      database,
      "INSERT INTO ir_blob(hash, size_bytes, compression, data) "
      "VALUES (?1, ?2, 'zstd', ?3)");
  if (!insertOr)
    return insertOr.takeError();
  auto insert = std::move(*insertOr);

  if (sqlite3_bind_blob(insert->get(), 1, hash.data(), hash.size(),
                        SQLITE_TRANSIENT) != SQLITE_OK ||
      sqlite3_bind_int64(insert->get(), 2,
                         static_cast<int64_t>(text.size())) != SQLITE_OK ||
      sqlite3_bind_blob(insert->get(), 3, compressed.data(),
                        static_cast<int>(compressed.size()),
                        SQLITE_TRANSIENT) != SQLITE_OK)
    return makeSqliteError(database, "bind compressed blob");
  if (sqlite3_step(insert->get()) != SQLITE_DONE)
    return makeSqliteError(database, "insert compressed blob");
  return BlobId{sqlite3_last_insert_rowid(database)};
}

llvm::Expected<PassId>
TraceStorage::beginPass(std::optional<int64_t> parent, int64_t seq,
                        llvm::StringRef name, std::optional<int64_t> before,
                        int64_t startNs, bool changed) {
  auto statementOr = Statement::prepare(
      database,
      "INSERT INTO pass_execution "
      "(parent_id, seq, name, ir_before, ir_after, start_ns, end_ns, "
      "ir_changed) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?5, ?6)");
  if (!statementOr)
    return statementOr.takeError();
  auto statement = std::move(*statementOr);

  if (llvm::Error error =
          bindOptionalInt(database, statement->get(), 1, parent))
    return std::move(error);
  if (sqlite3_bind_int64(statement->get(), 2, seq) != SQLITE_OK)
    return makeSqliteError(database, "bind pass sequence");
  if (llvm::Error error = bindText(database, statement->get(), 3, name))
    return std::move(error);
  if (llvm::Error error =
          bindOptionalInt(database, statement->get(), 4, before))
    return std::move(error);
  if (sqlite3_bind_int64(statement->get(), 5, startNs) != SQLITE_OK ||
      sqlite3_bind_int(statement->get(), 6, changed ? 1 : 0) != SQLITE_OK)
    return makeSqliteError(database, "bind pass timing");
  if (sqlite3_step(statement->get()) != SQLITE_DONE)
    return makeSqliteError(database, "insert pass execution");
  return PassId{sqlite3_last_insert_rowid(database)};
}

llvm::Error TraceStorage::endPass(int64_t id, std::optional<int64_t> after,
                                  int64_t endNs, bool changed) {
  auto statementOr = Statement::prepare(
      database,
      "UPDATE pass_execution SET ir_after=?1, end_ns=?2, ir_changed=?3 "
      "WHERE id=?4");
  if (!statementOr)
    return statementOr.takeError();
  auto statement = std::move(*statementOr);

  if (llvm::Error error =
          bindOptionalInt(database, statement->get(), 1, after))
    return error;
  if (sqlite3_bind_int64(statement->get(), 2, endNs) != SQLITE_OK ||
      sqlite3_bind_int(statement->get(), 3, changed ? 1 : 0) != SQLITE_OK ||
      sqlite3_bind_int64(statement->get(), 4, id) != SQLITE_OK)
    return makeSqliteError(database, "bind completed pass");
  if (sqlite3_step(statement->get()) != SQLITE_DONE)
    return makeSqliteError(database, "complete pass execution");
  if (sqlite3_changes(database) != 1)
    return llvm::createStringError(llvm::inconvertibleErrorCode(),
                                   "complete pass execution: unknown id %lld",
                                   static_cast<long long>(id));
  return llvm::Error::success();
}

llvm::Error TraceStorage::writeOpIndex(int64_t passId, int side,
                                       int64_t ptrToken, int64_t byteStart,
                                       int64_t byteEnd,
                                       llvm::StringRef opName) {
  auto statementOr = Statement::prepare(
      database,
      "INSERT INTO op_index "
      "(pass_id, side, ptr_token, byte_start, byte_end, op_name) "
      "VALUES (?1, ?2, ?3, ?4, ?5, ?6)");
  if (!statementOr)
    return statementOr.takeError();
  auto statement = std::move(*statementOr);

  if (sqlite3_bind_int64(statement->get(), 1, passId) != SQLITE_OK ||
      sqlite3_bind_int(statement->get(), 2, side) != SQLITE_OK ||
      sqlite3_bind_int64(statement->get(), 3, ptrToken) != SQLITE_OK ||
      sqlite3_bind_int64(statement->get(), 4, byteStart) != SQLITE_OK ||
      sqlite3_bind_int64(statement->get(), 5, byteEnd) != SQLITE_OK)
    return makeSqliteError(database, "bind op_index");
  if (llvm::Error error =
          bindText(database, statement->get(), 6, opName))
    return error;
  if (sqlite3_step(statement->get()) != SQLITE_DONE)
    return makeSqliteError(database, "insert op_index");
  return llvm::Error::success();
}

llvm::Error TraceStorage::writeIdentityEvent(
    int64_t passId, llvm::StringRef kind, int64_t ptrToken,
    std::optional<int64_t> newToken, std::optional<llvm::StringRef> pattern,
    llvm::StringRef source, int64_t seq) {
  auto statementOr = Statement::prepare(
      database,
      "INSERT INTO op_identity "
      "(pass_id, kind, ptr_token, new_token, pattern, source, seq) "
      "VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)");
  if (!statementOr)
    return statementOr.takeError();
  auto statement = std::move(*statementOr);

  if (sqlite3_bind_int64(statement->get(), 1, passId) != SQLITE_OK)
    return makeSqliteError(database, "bind identity pass");
  if (llvm::Error error = bindText(database, statement->get(), 2, kind))
    return error;
  if (sqlite3_bind_int64(statement->get(), 3, ptrToken) != SQLITE_OK)
    return makeSqliteError(database, "bind identity token");
  if (llvm::Error error =
          bindOptionalInt(database, statement->get(), 4, newToken))
    return error;
  if (pattern) {
    if (llvm::Error error =
            bindText(database, statement->get(), 5, *pattern))
      return error;
  } else if (sqlite3_bind_null(statement->get(), 5) != SQLITE_OK) {
    return makeSqliteError(database, "bind identity pattern");
  }
  if (llvm::Error error = bindText(database, statement->get(), 6, source))
    return error;
  if (sqlite3_bind_int64(statement->get(), 7, seq) != SQLITE_OK)
    return makeSqliteError(database, "bind identity seq");
  if (sqlite3_step(statement->get()) != SQLITE_DONE)
    return makeSqliteError(database, "insert op_identity");
  return llvm::Error::success();
}

llvm::Error TraceStorage::finish() {
  if (finished)
    return llvm::Error::success();
  if (llvm::Error error = execute(database, "PRAGMA journal_mode=DELETE",
                                  "checkpoint trace"))
    return error;
  if (sqlite3_close(database) != SQLITE_OK)
    return makeSqliteError(database, "close trace");
  database = nullptr;

  std::error_code filesystemError;
  std::filesystem::remove(path + "-wal", filesystemError);
  if (filesystemError)
    return llvm::createStringError(filesystemError,
                                   "remove finished WAL sidecar");
  std::filesystem::remove(path + "-shm", filesystemError);
  if (filesystemError)
    return llvm::createStringError(filesystemError,
                                   "remove finished SHM sidecar");
  finished = true;
  return llvm::Error::success();
}

} // namespace mlir::trace::detail
