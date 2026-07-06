#include "mlir-trace/TraceRecorder.h"

#include "mlir/Dialect/Arith/IR/Arith.h"
#include "mlir/Dialect/Func/IR/FuncOps.h"
#include "mlir/Dialect/UB/IR/UBOps.h"
#include "mlir/IR/BuiltinOps.h"
#include "mlir/IR/DialectRegistry.h"
#include "mlir/IR/MLIRContext.h"
#include "mlir/Parser/Parser.h"
#include "mlir/Pass/PassManager.h"
#include "mlir/Pass/PassRegistry.h"
#include "mlir/Transforms/Passes.h"
#include "llvm/Support/Error.h"
#include "llvm/Support/SourceMgr.h"
#include "llvm/Support/raw_ostream.h"

namespace {

int fail(llvm::Error error) {
  llvm::errs() << llvm::toString(std::move(error)) << '\n';
  return 1;
}

} // namespace

// mlir-opt-style harness with trace capture: runs an arbitrary textual pass
// pipeline over an input module and records a Full-fidelity trace. Used to
// validate capture coverage and viewer performance on pipelines the hardcoded
// capture-toy example cannot express. The dialect set matches what the local
// minimal MLIR build provides (arith/func/ub + general transforms); extend the
// registry when a fuller toolchain is available.
int main(int argc, char **argv) {
  if (argc != 4) {
    llvm::errs()
        << "usage: mlir-trace-opt INPUT.mlir PIPELINE OUTPUT.mlirtrace\n"
        << "  e.g. mlir-trace-opt in.mlir "
           "'builtin.module(canonicalize,cse)' out.mlirtrace\n";
    return 2;
  }

  mlir::DialectRegistry registry;
  registry.insert<mlir::arith::ArithDialect, mlir::func::FuncDialect,
                  mlir::ub::UBDialect>();
  mlir::registerTransformsPasses();
  mlir::MLIRContext context(registry);
  context.allowUnregisteredDialects();

  auto module = mlir::parseSourceFile<mlir::ModuleOp>(argv[1], &context);
  if (!module) {
    llvm::errs() << "failed to parse " << argv[1] << '\n';
    return 1;
  }

  mlir::PassManager passManager(&context);
  if (mlir::failed(mlir::parsePassPipeline(argv[2], passManager))) {
    llvm::errs() << "failed to parse pipeline: " << argv[2] << '\n';
    return 1;
  }

  mlir::trace::TraceOptions options;
  options.fidelity = mlir::trace::Fidelity::Full;
  mlir::trace::TraceRecorder recorder(argv[3], options);
  if (llvm::Error error = recorder.attach(passManager, context))
    return fail(std::move(error));
  if (mlir::failed(passManager.run(*module))) {
    llvm::errs() << "pass pipeline failed\n";
    return 1;
  }
  if (llvm::Error error = recorder.finish())
    return fail(std::move(error));
  return 0;
}
