set(prefix "${TEST_BINARY_DIR}/install-prefix")
set(consumer_build "${TEST_BINARY_DIR}/install-consumer-build")

file(REMOVE_RECURSE "${prefix}" "${consumer_build}")

execute_process(
  COMMAND "${CMAKE_COMMAND}" --install "${PROJECT_BINARY_DIR}"
          --prefix "${prefix}"
  RESULT_VARIABLE install_result
  OUTPUT_VARIABLE install_stdout
  ERROR_VARIABLE install_stderr)
if(NOT install_result EQUAL 0)
  message(FATAL_ERROR
    "install failed (${install_result})\n${install_stdout}${install_stderr}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}"
          -S "${CONSUMER_SOURCE_DIR}"
          -B "${consumer_build}"
          -G Ninja
          "-DMLIRTrace_DIR=${prefix}/${PACKAGE_CMAKE_DIR}"
          "-DMLIR_DIR=${MLIR_DIR}"
  RESULT_VARIABLE configure_result
  OUTPUT_VARIABLE configure_stdout
  ERROR_VARIABLE configure_stderr)
if(NOT configure_result EQUAL 0)
  message(FATAL_ERROR
    "consumer configure failed (${configure_result})\n"
    "${configure_stdout}${configure_stderr}")
endif()

execute_process(
  COMMAND "${CMAKE_COMMAND}" --build "${consumer_build}"
  RESULT_VARIABLE build_result
  OUTPUT_VARIABLE build_stdout
  ERROR_VARIABLE build_stderr)
if(NOT build_result EQUAL 0)
  message(FATAL_ERROR
    "consumer build failed (${build_result})\n${build_stdout}${build_stderr}")
endif()
