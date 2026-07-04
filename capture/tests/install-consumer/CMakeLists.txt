cmake_minimum_required(VERSION 3.20)
project(MLIRTraceInstallConsumer LANGUAGES C CXX)

find_package(MLIRTrace CONFIG REQUIRED)

add_executable(mlir-trace-install-consumer main.cpp)
target_link_libraries(mlir-trace-install-consumer PRIVATE
  MLIRTrace::MLIRTrace)
