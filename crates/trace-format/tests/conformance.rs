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
