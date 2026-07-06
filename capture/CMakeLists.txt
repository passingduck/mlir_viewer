cmake_minimum_required(VERSION 3.20)
project(MLIRTrace VERSION 0.1.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")

find_package(MLIR CONFIG REQUIRED)
find_package(SQLite3 REQUIRED)

if(TARGET zstd::libzstd_shared)
  set(MLIR_TRACE_ZSTD_TARGET zstd::libzstd_shared)
elseif(TARGET zstd::libzstd_static)
  set(MLIR_TRACE_ZSTD_TARGET zstd::libzstd_static)
else()
  message(FATAL_ERROR "LLVM's zstd dependency target is unavailable")
endif()

message(STATUS "Using MLIRConfig.cmake in: ${MLIR_DIR}")
message(STATUS "Using LLVMConfig.cmake in: ${LLVM_DIR}")

include(FetchContent)
find_path(XXHASH_INCLUDE_DIR NAMES xxhash.h)

if(NOT XXHASH_INCLUDE_DIR)
  FetchContent_Declare(xxhash
    GIT_REPOSITORY https://github.com/Cyan4973/xxHash.git
    GIT_TAG v0.8.3
    GIT_SHALLOW TRUE)
  FetchContent_GetProperties(xxhash)
  if(NOT xxhash_POPULATED)
    FetchContent_Populate(xxhash)
  endif()
  set(XXHASH_INCLUDE_DIR "${xxhash_SOURCE_DIR}")
endif()

add_library(MLIRTrace STATIC
  lib/TraceRecorder.cpp
  lib/TraceStorage.cpp)
target_compile_features(MLIRTrace PUBLIC cxx_std_17)

target_include_directories(MLIRTrace
  PUBLIC
    "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>"
    "$<INSTALL_INTERFACE:include>"
  PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/lib"
    "${XXHASH_INCLUDE_DIR}"
    ${LLVM_INCLUDE_DIRS}
    ${MLIR_INCLUDE_DIRS})

target_compile_definitions(MLIRTrace PRIVATE ${LLVM_DEFINITIONS})
if(NOT LLVM_ENABLE_RTTI)
  target_compile_options(MLIRTrace PRIVATE -fno-rtti)
endif()
target_link_libraries(MLIRTrace
  PUBLIC MLIRIR MLIRPass LLVMSupport
  PRIVATE SQLite::SQLite3 ${MLIR_TRACE_ZSTD_TARGET})

add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../examples/capture-toy"
                 "${CMAKE_CURRENT_BINARY_DIR}/capture-toy")
add_subdirectory("${CMAKE_CURRENT_SOURCE_DIR}/../examples/mlir-trace-opt"
                 "${CMAKE_CURRENT_BINARY_DIR}/mlir-trace-opt-build")

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

install(TARGETS MLIRTrace
  EXPORT MLIRTraceTargets
  ARCHIVE DESTINATION "${CMAKE_INSTALL_LIBDIR}")
install(DIRECTORY include/ DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")

configure_package_config_file(
  cmake/MLIRTraceConfig.cmake.in
  "${CMAKE_CURRENT_BINARY_DIR}/MLIRTraceConfig.cmake"
  INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/MLIRTrace")
write_basic_package_version_file(
  "${CMAKE_CURRENT_BINARY_DIR}/MLIRTraceConfigVersion.cmake"
  VERSION "${PROJECT_VERSION}"
  COMPATIBILITY SameMajorVersion)

install(EXPORT MLIRTraceTargets
  FILE MLIRTraceTargets.cmake
  NAMESPACE MLIRTrace::
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/MLIRTrace")
install(FILES
  "${CMAKE_CURRENT_BINARY_DIR}/MLIRTraceConfig.cmake"
  "${CMAKE_CURRENT_BINARY_DIR}/MLIRTraceConfigVersion.cmake"
  DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/MLIRTrace")

include(CTest)
if(BUILD_TESTING)
  set(MLIR_VIEWER_BIN
      "${CMAKE_CURRENT_SOURCE_DIR}/../target/debug/mlir-viewer"
      CACHE FILEPATH "Path to the Rust mlir-viewer binary")

  add_executable(mlir-trace-storage-test tests/TraceStorageTest.cpp)
  target_include_directories(mlir-trace-storage-test PRIVATE
    lib
    ${LLVM_INCLUDE_DIRS}
    ${MLIR_INCLUDE_DIRS})
  if(NOT LLVM_ENABLE_RTTI)
    target_compile_options(mlir-trace-storage-test PRIVATE -fno-rtti)
  endif()
  target_link_libraries(mlir-trace-storage-test PRIVATE MLIRTrace)

  add_test(NAME storage_contract
    COMMAND mlir-trace-storage-test
            "${CMAKE_CURRENT_BINARY_DIR}/storage-contract.mlirtrace")

  add_executable(mlir-trace-recorder-test tests/TraceRecorderTest.cpp)
  target_include_directories(mlir-trace-recorder-test PRIVATE
    ${LLVM_INCLUDE_DIRS}
    ${MLIR_INCLUDE_DIRS})
  if(NOT LLVM_ENABLE_RTTI)
    target_compile_options(mlir-trace-recorder-test PRIVATE -fno-rtti)
  endif()
  target_link_libraries(mlir-trace-recorder-test PRIVATE
    MLIRTrace MLIRArithDialect MLIRFuncDialect MLIRParser MLIRTransforms
    SQLite::SQLite3)

  add_test(NAME recorder_timeline
    COMMAND mlir-trace-recorder-test
            timeline
            "${CMAKE_CURRENT_BINARY_DIR}/recorder-timeline.mlirtrace")
  add_test(NAME recorder_text
    COMMAND mlir-trace-recorder-test
            text
            "${CMAKE_CURRENT_BINARY_DIR}/recorder-text.mlirtrace")
  add_test(NAME recorder_failed_pass
    COMMAND mlir-trace-recorder-test
            failed
            "${CMAKE_CURRENT_BINARY_DIR}/recorder-failed.mlirtrace")
  add_test(NAME recorder_nested_pipeline
    COMMAND mlir-trace-recorder-test
            nested
            "${CMAKE_CURRENT_BINARY_DIR}/recorder-nested.mlirtrace")
  add_test(NAME recorder_full
    COMMAND mlir-trace-recorder-test
            full
            "${CMAKE_CURRENT_BINARY_DIR}/recorder-full.mlirtrace")

  add_test(NAME cpp_trace_rust_reader
    COMMAND "${CMAKE_COMMAND}"
      -DEXAMPLE_BIN=${CMAKE_CURRENT_BINARY_DIR}/mlir-trace-example
      -DMLIR_VIEWER_BIN=${MLIR_VIEWER_BIN}
      -DTRACE_PATH=${CMAKE_CURRENT_BINARY_DIR}/cpp-demo.mlirtrace
      -P ${CMAKE_CURRENT_SOURCE_DIR}/tests/CheckRustReader.cmake)

  add_test(NAME install_consumer
    COMMAND "${CMAKE_COMMAND}"
      -DPROJECT_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}
      -DTEST_BINARY_DIR=${CMAKE_CURRENT_BINARY_DIR}
      -DCONSUMER_SOURCE_DIR=${CMAKE_CURRENT_SOURCE_DIR}/tests/install-consumer
      -DPACKAGE_CMAKE_DIR=lib/cmake/MLIRTrace
      -DMLIR_DIR=${MLIR_DIR}
      -P ${CMAKE_CURRENT_SOURCE_DIR}/tests/CheckInstallConsumer.cmake)
endif()
