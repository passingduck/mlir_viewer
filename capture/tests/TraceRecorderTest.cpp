#include "mlir-trace/TraceRecorder.h"

#include "mlir/Dialect/Func/IR/FuncOps.h"
#include "mlir/IR/Builders.h"
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
  void runOnOperation() final {
    getOperation()->setAttr("test.changed",
                            mlir::UnitAttr::get(&getContext()));
  }
};

struct CsePass
    : mlir::PassWrapper<CsePass, mlir::OperationPass<mlir::ModuleOp>> {
  MLIR_DEFINE_EXPLICIT_INTERNAL_INLINE_TYPE_ID(CsePass)
  llvm::StringRef getArgument() const final { return "cse"; }
  void runOnOperation() final {}
};

struct FailingPass
    : mlir::PassWrapper<FailingPass, mlir::OperationPass<mlir::ModuleOp>> {
  MLIR_DEFINE_EXPLICIT_INTERNAL_INLINE_TYPE_ID(FailingPass)
  llvm::StringRef getArgument() const final { return "failing-pass"; }
  void runOnOperation() final { signalPassFailure(); }
};

struct NestedCsePass
    : mlir::PassWrapper<NestedCsePass,
                        mlir::OperationPass<mlir::func::FuncOp>> {
  MLIR_DEFINE_EXPLICIT_INTERNAL_INLINE_TYPE_ID(NestedCsePass)
  llvm::StringRef getArgument() const final { return "nested-cse"; }
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

mlir::OwningOpRef<mlir::ModuleOp> createModule(mlir::MLIRContext &context) {
  context.getOrLoadDialect<mlir::func::FuncDialect>();
  mlir::OpBuilder builder(&context);
  auto module = mlir::ModuleOp::create(builder.getUnknownLoc());
  auto function = mlir::func::FuncOp::create(
      builder.getUnknownLoc(), "forward", builder.getFunctionType({}, {}));
  function.setPrivate();
  module.getBody()->push_back(function);
  return mlir::OwningOpRef<mlir::ModuleOp>(module);
}

int runRecorder(llvm::StringRef mode, llvm::StringRef path) {
  mlir::MLIRContext context;
  auto module = createModule(context);
  mlir::PassManager passManager(&context);
  if (mode == "failed") {
    passManager.addPass(std::make_unique<FailingPass>());
  } else if (mode == "nested") {
    passManager.addNestedPass<mlir::func::FuncOp>(
        std::make_unique<NestedCsePass>());
  } else {
    passManager.addPass(std::make_unique<CanonicalizePass>());
    passManager.addPass(std::make_unique<CsePass>());
  }

  mlir::trace::TraceOptions options;
  options.fidelity = mode == "timeline" || mode == "nested"
                         ? mlir::trace::Fidelity::Timeline
                         : mlir::trace::Fidelity::Text;
  mlir::trace::TraceRecorder recorder(path, options);
  if (llvm::Error error = recorder.attach(passManager, context))
    return fail(std::move(error));

  const bool passFailed = mlir::failed(passManager.run(*module));
  if (passFailed != (mode == "failed")) {
    llvm::errs() << "unexpected pass manager result\n";
    return 1;
  }
  if (llvm::Error error = recorder.finish())
    return fail(std::move(error));
  return 0;
}

} // namespace

int main(int argc, char **argv) {
  if (argc != 3) {
    llvm::errs() << "usage: mlir-trace-recorder-test MODE OUTPUT\n";
    return 2;
  }

  const llvm::StringRef mode = argv[1];
  if (mode != "timeline" && mode != "text" && mode != "failed" &&
      mode != "nested") {
    llvm::errs() << "unknown mode: " << mode << '\n';
    return 2;
  }
  if (int result = runRecorder(mode, argv[2]))
    return result;

  sqlite3 *database = nullptr;
  if (sqlite3_open_v2(argv[2], &database, SQLITE_OPEN_READONLY, nullptr) !=
      SQLITE_OK)
    return fail(database, "open recorder trace");

  bool valid = scalarEquals(
      database, "SELECT value FROM meta WHERE key='fidelity'",
      mode == "timeline" || mode == "nested" ? "timeline" : "text");
  if (mode == "timeline") {
    valid = valid &&
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
                         0);
  } else if (mode == "nested") {
    valid = valid &&
            scalarEquals(database,
                         "SELECT count(*) FROM pass_execution child "
                         "JOIN pass_execution parent ON child.parent_id=parent.id "
                         "JOIN pass_execution root ON parent.parent_id=root.id "
                         "WHERE child.name='nested-cse' "
                         "AND root.name='Pipeline'",
                         1);
  } else if (mode == "text") {
    valid = valid &&
            scalarEquals(database,
                         "SELECT count(*) FROM pass_execution "
                         "WHERE ir_before IS NULL OR ir_after IS NULL",
                         0) &&
            scalarEquals(database,
                         "SELECT ir_changed FROM pass_execution "
                         "WHERE name='canonicalize'",
                         1) &&
            scalarEquals(database,
                         "SELECT count(*) FROM pass_execution "
                         "WHERE name='cse' AND ir_changed=0 "
                         "AND ir_before=ir_after",
                         1) &&
            scalarEquals(database, "SELECT count(*) FROM ir_blob", 2) &&
            scalarEquals(database,
                         "SELECT count(*) FROM ir_blob WHERE size_bytes > 0",
                         2);
  } else {
    valid = valid &&
            scalarEquals(database,
                         "SELECT value FROM meta WHERE key='capture_status'",
                         "pass_failed") &&
            scalarEquals(database,
                         "SELECT count(*) FROM pass_execution "
                         "WHERE name='failing-pass' AND ir_before IS NOT NULL "
                         "AND ir_after IS NULL",
                         1);
  }
  valid = valid &&
          scalarEquals(database,
                       "SELECT count(*) FROM pass_execution "
                       "WHERE end_ns < start_ns",
                       0);

  if (!valid)
    fail(database, "validate recorder trace");
  sqlite3_close(database);
  return valid ? 0 : 1;
}
