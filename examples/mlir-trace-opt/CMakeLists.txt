add_executable(mlir-trace-opt main.cpp)

target_include_directories(mlir-trace-opt PRIVATE
  ${LLVM_INCLUDE_DIRS}
  ${MLIR_INCLUDE_DIRS})

if(NOT LLVM_ENABLE_RTTI)
  target_compile_options(mlir-trace-opt PRIVATE -fno-rtti)
endif()

target_link_libraries(mlir-trace-opt PRIVATE
  MLIRTrace
  MLIRArithDialect
  MLIRFuncDialect
  MLIRUBDialect
  MLIRParser
  MLIRPass
  MLIRTransforms)

set_target_properties(mlir-trace-opt PROPERTIES
  RUNTIME_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}")
