add_executable(mlir-trace-example main.cpp)

target_include_directories(mlir-trace-example PRIVATE
  ${LLVM_INCLUDE_DIRS}
  ${MLIR_INCLUDE_DIRS})

if(NOT LLVM_ENABLE_RTTI)
  target_compile_options(mlir-trace-example PRIVATE -fno-rtti)
endif()

target_link_libraries(mlir-trace-example PRIVATE
  MLIRTrace
  MLIRArithDialect
  MLIRFuncDialect
  MLIRParser
  MLIRTransforms)

set_target_properties(mlir-trace-example PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}")
