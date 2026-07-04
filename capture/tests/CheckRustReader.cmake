if(NOT EXISTS "${EXAMPLE_BIN}")
  message(FATAL_ERROR "capture example not found: ${EXAMPLE_BIN}")
endif()
if(NOT EXISTS "${MLIR_VIEWER_BIN}")
  message(FATAL_ERROR "mlir-viewer binary not found: ${MLIR_VIEWER_BIN}")
endif()

execute_process(
  COMMAND "${EXAMPLE_BIN}" "${TRACE_PATH}"
  RESULT_VARIABLE example_result
  OUTPUT_VARIABLE example_stdout
  ERROR_VARIABLE example_stderr)
if(NOT example_result EQUAL 0)
  message(FATAL_ERROR
    "capture example failed (${example_result})\n${example_stdout}${example_stderr}")
endif()

execute_process(
  COMMAND "${MLIR_VIEWER_BIN}" trace dump "${TRACE_PATH}"
  RESULT_VARIABLE reader_result
  OUTPUT_VARIABLE reader_stdout
  ERROR_VARIABLE reader_stderr)
if(NOT reader_result EQUAL 0)
  message(FATAL_ERROR
    "Rust reader failed (${reader_result})\n${reader_stdout}${reader_stderr}")
endif()

foreach(expected "Pipeline" "canonicalize" "cse" "(no change)")
  string(FIND "${reader_stdout}" "${expected}" position)
  if(position EQUAL -1)
    message(FATAL_ERROR
      "Rust reader output lacks '${expected}':\n${reader_stdout}")
  endif()
endforeach()
