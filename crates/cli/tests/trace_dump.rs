use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn gen_fixture_then_dump_shows_pass_tree() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");

    Command::cargo_bin("mlir-viewer")
        .unwrap()
        .args(["dev", "gen-fixture"])
        .arg(&trace)
        .assert()
        .success();

    Command::cargo_bin("mlir-viewer")
        .unwrap()
        .args(["trace", "dump"])
        .arg(&trace)
        .assert()
        .success()
        .stdout(predicate::str::contains("Pipeline"))
        .stdout(predicate::str::contains("canonicalize"))
        // no-op pass is visibly marked
        .stdout(predicate::str::contains("cse").and(predicate::str::contains("(no change)")))
        .stdout(predicate::str::contains("1.00ms"));
}

#[test]
fn dump_rejects_non_trace_file() {
    let dir = tempfile::tempdir().unwrap();
    let bogus = dir.path().join("bogus.mlirtrace");
    std::fs::write(&bogus, "not a database").unwrap();

    Command::cargo_bin("mlir-viewer")
        .unwrap()
        .args(["trace", "dump"])
        .arg(&bogus)
        .assert()
        .failure();
}
