#include "mlir-trace/TraceRecorder.h"

#include "mlir/IR/BuiltinOps.h"
#include "mlir/IR/MLIRContext.h"
#include "mlir/Pass/Pass.h"
#include "mlir/Pass/PassManager.h"
#include "llvm/ADT/StringRef.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/raw_ostream.h"

#include <sqlite3.h>

#include <memory>
#include <string>

namespace {

struct CanonicalizePass
    : mlir::PassWrapper<CanonicalizePass,
                        mlir::OperationPass<mlir::ModuleOp>> {
  MLIR_DEFINE_EXPLICIT_INTERNAL_INLINE_TYPE_ID(CanonicalizePass)
  llvm::StringRef getArgument() const final { return "canonicalize"; }
  void runOnOperation() final {}
};

struct CsePass
    : mlir::PassWrapper<CsePass, mlir::OperationPass<mlir::ModuleOp>> {
  MLIR_DEFINE_EXPLICIT_INTERNAL_INLINE_TYPE_ID(CsePass)
  llvm::StringRef getArgument() const final { return "cse"; }
  void runOnOperation() final {}
};

int fail(llvm::Error error) {
  llvm::errs() << llvm::toString(std::move(error)) << '\n';
  return 1;
}

int fail(sqlite3 *database, llvm::StringRef action) {
  llvm::errs() << action << ": " << sqlite3_errmsg(database) << '\n';
  return 1;
}

bool scalarEquals(sqlite3 *database, llvm::StringRef sql,
                  llvm::StringRef expected) {
  sqlite3_stmt *statement = nullptr;
  if (sqlite3_prepare_v2(database, sql.data(), static_cast<int>(sql.size()),
                         &statement, nullptr) != SQLITE_OK)
    return false;
  const bool matches = sqlite3_step(statement) == SQLITE_ROW &&
                       expected == reinterpret_cast<const char *>(
                                       sqlite3_column_text(statement, 0));
  sqlite3_finalize(statement);
  return matches;
}

bool scalarEquals(sqlite3 *database, llvm::StringRef sql, int expected) {
  sqlite3_stmt *statement = nullptr;
  if (sqlite3_prepare_v2(database, sql.data(), static_cast<int>(sql.size()),
                         &statement, nullptr) != SQLITE_OK)
    return false;
  const bool matches = sqlite3_step(statement) == SQLITE_ROW &&
                       sqlite3_column_int(statement, 0) == expected;
  sqlite3_finalize(statement);
  return matches;
}

} // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    llvm::errs() << "usage: mlir-trace-recorder-test OUTPUT\n";
    return 2;
  }

  mlir::MLIRContext context;
  mlir::OwningOpRef<mlir::ModuleOp> module(
      mlir::ModuleOp::create(mlir::UnknownLoc::get(&context)));
  mlir::PassManager passManager(&context);
  passManager.addPass(std::make_unique<CanonicalizePass>());
  passManager.addPass(std::make_unique<CsePass>());

  mlir::trace::TraceOptions options;
  options.fidelity = mlir::trace::Fidelity::Timeline;
  mlir::trace::TraceRecorder recorder(argv[1], options);
  if (llvm::Error error = recorder.attach(passManager, context))
    return fail(std::move(error));
  if (mlir::failed(passManager.run(*module))) {
    llvm::errs() << "pass manager failed\n";
    return 1;
  }
  if (llvm::Error error = recorder.finish())
    return fail(std::move(error));

  sqlite3 *database = nullptr;
  if (sqlite3_open_v2(argv[1], &database, SQLITE_OPEN_READONLY, nullptr) !=
      SQLITE_OK)
    return fail(database, "open timeline trace");

  const bool valid =
      scalarEquals(database,
                   "SELECT value FROM meta WHERE key='fidelity'", "timeline") &&
      scalarEquals(database,
                   "SELECT count(*) FROM pass_execution "
                   "WHERE parent_id IS NULL AND name='Pipeline'",
                   1) &&
      scalarEquals(database,
                   "SELECT count(*) FROM pass_execution child "
                   "JOIN pass_execution root ON child.parent_id=root.id "
                   "WHERE root.name='Pipeline' AND child.name IN "
                   "('canonicalize','cse')",
                   2) &&
      scalarEquals(database,
                   "SELECT count(*) FROM pass_execution "
                   "WHERE ir_before IS NOT NULL OR ir_after IS NOT NULL",
                   0) &&
      scalarEquals(database,
                   "SELECT count(*) FROM pass_execution WHERE end_ns < start_ns",
                   0);

  if (!valid)
    fail(database, "validate timeline trace");
  sqlite3_close(database);
  return valid ? 0 : 1;
}
