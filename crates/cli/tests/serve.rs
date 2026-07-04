use assert_cmd::Command as AssertCommand;
use predicates::prelude::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn serve_rejects_invalid_trace_before_binding() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("invalid.mlirtrace");
    std::fs::write(&trace, "not sqlite").unwrap();

    AssertCommand::cargo_bin("mlir-viewer")
        .unwrap()
        .arg("serve")
        .arg(&trace)
        .args(["--listen", "127.0.0.1:0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("sqlite error"));
}

#[test]
fn serve_binds_ephemeral_port_and_answers_api_requests() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();

    let mut command = Command::new(assert_cmd::cargo::cargo_bin("mlir-viewer"));
    command
        .arg("serve")
        .arg(&trace)
        .args(["--listen", "127.0.0.1:0"]);
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stderr = child.stderr.take().unwrap();
    let mut stderr = BufReader::new(stderr);
    let mut line = String::new();
    stderr.read_line(&mut line).unwrap();
    let address = line
        .trim()
        .strip_prefix("mlir-viewer listening on http://")
        .expect("server should print its bound URL");

    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /api/trace/info HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    child.kill().unwrap();
    child.wait().unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("\"format_version\":\"1\""), "{response}");
}
