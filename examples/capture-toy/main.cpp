#include "mlir-trace/TraceRecorder.h"

#include "mlir/Dialect/Arith/IR/Arith.h"
#include "mlir/Dialect/Func/IR/FuncOps.h"
#include "mlir/IR/BuiltinOps.h"
#include "mlir/IR/DialectRegistry.h"
#include "mlir/IR/MLIRContext.h"
#include "mlir/Parser/Parser.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/Transforms/Passes.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/SourceMgr.h"
#include "llvm/Support/raw_ostream.h"

#include <memory>

namespace {

constexpr llvm::StringLiteral source = R"mlir(
module {
  func.func @forward(%arg0: i32) -> i32 {
    %zero = arith.constant 0 : i32
    %result = arith.addi %arg0, %zero : i32
    return %result : i32
  }
}
)mlir";

int fail(llvm::Error error) {
  llvm::errs() << llvm::toString(std::move(error)) << '\n';
  return 1;
}

} // namespace

int main(int argc, char **argv) {
  if (argc != 2) {
    llvm::errs() << "usage: mlir-trace-example OUTPUT.mlirtrace\n";
    return 2;
  }

  mlir::DialectRegistry registry;
  registry.insert<mlir::arith::ArithDialect, mlir::func::FuncDialect>();
  mlir::MLIRContext context(registry);

  auto module = mlir::parseSourceString<mlir::ModuleOp>(source, &context);
  if (!module) {
    llvm::errs() << "failed to parse example module\n";
    return 1;
  }

  mlir::PassManager passManager(&context);
  passManager.addPass(mlir::createCanonicalizerPass());
  passManager.addPass(mlir::createCSEPass());

  mlir::trace::TraceRecorder recorder(argv[1]);
  if (llvm::Error error = recorder.attach(passManager, context))
    return fail(std::move(error));
  if (mlir::failed(passManager.run(*module))) {
    llvm::errs() << "example pass pipeline failed\n";
    return 1;
  }
  if (llvm::Error error = recorder.finish())
    return fail(std::move(error));
  return 0;
}
