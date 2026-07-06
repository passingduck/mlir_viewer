use trace_format::TraceReader;

/// Contract 1 anchor: v1 golden files must remain readable forever within major version 1.
/// If this test breaks, you changed the format — bump FORMAT_VERSION and write a migration
/// story instead of editing this file.
#[test]
fn golden_v1_trace_remains_readable() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../testdata/golden/demo-v1.mlirtrace"
    );
    let r = TraceReader::open(std::path::Path::new(path)).unwrap();
    let roots = r.passes().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].children.len(), 5);
    let text = r.blob_text(roots[0].ir_before.unwrap()).unwrap();
    assert!(text.contains("linalg.matmul"));
}

/// Cross-language Contract 1 check. CMake generates this fixture from a real
/// MLIR pass pipeline; CI supplies its path explicitly after building capture/.
#[test]
#[ignore = "requires MLIR_TRACE_CPP_FIXTURE from the C++ capture build"]
fn cpp_generated_trace_is_v1_compatible() {
    let path = std::env::var("MLIR_TRACE_CPP_FIXTURE")
        .expect("MLIR_TRACE_CPP_FIXTURE must point to a C++-generated trace");
    // Cargo runs tests from the crate directory, so resolve a relative path
    // against the workspace root the developer actually invoked cargo from.
    let mut resolved = std::path::PathBuf::from(&path);
    if resolved.is_relative() {
        let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        resolved = workspace_root.join(resolved);
    }
    let reader = TraceReader::open(&resolved).unwrap();
    let roots = reader.passes().unwrap();
    assert_eq!(roots.len(), 1);

    fn visit(reader: &TraceReader, node: &trace_format::PassNode, names: &mut Vec<String>) {
        names.push(node.name.clone());
        for blob in [node.ir_before, node.ir_after].into_iter().flatten() {
            assert!(!reader.blob_text(blob).unwrap().is_empty());
        }
        for child in &node.children {
            visit(reader, child, names);
        }
    }

    let mut names = Vec::new();
    visit(&reader, &roots[0], &mut names);
    assert!(names.iter().any(|name| name == "canonicalize"));
    assert!(names.iter().any(|name| name == "cse"));
}
