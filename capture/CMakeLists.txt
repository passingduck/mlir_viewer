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
find_library(XXHASH_LIBRARY NAMES xxhash)

if(XXHASH_INCLUDE_DIR AND XXHASH_LIBRARY)
  add_library(MLIRTraceXXHash UNKNOWN IMPORTED)
  set_target_properties(MLIRTraceXXHash PROPERTIES
    IMPORTED_LOCATION "${XXHASH_LIBRARY}"
    INTERFACE_INCLUDE_DIRECTORIES "${XXHASH_INCLUDE_DIR}")
else()
  FetchContent_Declare(xxhash
    GIT_REPOSITORY https://github.com/Cyan4973/xxHash.git
    GIT_TAG v0.8.3
    GIT_SHALLOW TRUE)
  FetchContent_GetProperties(xxhash)
  if(NOT xxhash_POPULATED)
    FetchContent_Populate(xxhash)
  endif()
  add_library(MLIRTraceXXHash STATIC "${xxhash_SOURCE_DIR}/xxhash.c")
  target_include_directories(MLIRTraceXXHash PUBLIC "${xxhash_SOURCE_DIR}")
  set_target_properties(MLIRTraceXXHash PROPERTIES POSITION_INDEPENDENT_CODE ON)
endif()

add_library(MLIRTrace SHARED
  lib/TraceStorage.cpp)

target_include_directories(MLIRTrace
  PUBLIC
    "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>"
    "$<INSTALL_INTERFACE:include>"
  PRIVATE
    "${CMAKE_CURRENT_SOURCE_DIR}/lib"
    ${LLVM_INCLUDE_DIRS}
    ${MLIR_INCLUDE_DIRS})

target_compile_definitions(MLIRTrace PRIVATE ${LLVM_DEFINITIONS})
if(NOT LLVM_ENABLE_RTTI)
  target_compile_options(MLIRTrace PRIVATE -fno-rtti)
endif()
target_link_libraries(MLIRTrace
  PUBLIC MLIRIR MLIRPass LLVMSupport
  PRIVATE SQLite::SQLite3 ${MLIR_TRACE_ZSTD_TARGET} MLIRTraceXXHash)

include(CTest)
if(BUILD_TESTING)
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
endif()
