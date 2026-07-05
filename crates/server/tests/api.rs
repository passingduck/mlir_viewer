use axum::body::Body;
use engine::{
    ChangeClass, DataflowGraph, FunctionDiff, HistoryChange, LinkConfidence, OpHistory,
    SelectableOp,
};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

async fn response_json(app: axum::Router, uri: &str) -> (axum::http::StatusCode, Value) {
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = serde_json::from_slice(&bytes).unwrap();
    (status, json)
}

#[allow(dead_code)]
async fn response_msgpack<T: serde::de::DeserializeOwned>(
    app: axum::Router,
    uri: &str,
) -> (axum::http::StatusCode, Option<T>) {
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = rmp_serde::from_slice(&bytes).ok();
    (status, value)
}

#[tokio::test]
async fn fixture_api_is_bounded_and_validated() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();

    let (status, info) = response_json(app.clone(), "/api/trace/info").await;
    assert_eq!(status, 200);
    assert_eq!(info["format_version"], trace_format::schema::FORMAT_VERSION);
    assert_eq!(info["pass_count"], 6);

    let (status, passes) = response_json(app.clone(), "/api/passes").await;
    assert_eq!(status, 200);
    assert_eq!(passes.as_array().unwrap().len(), 1);
    assert_eq!(passes[0]["name"], "Pipeline");
    assert_eq!(passes[0]["children"].as_array().unwrap().len(), 5);
    let pass_id = passes[0]["children"][0]["id"].as_i64().unwrap();

    let (status, page) = response_json(
        app.clone(),
        &format!("/api/passes/{pass_id}/ir?side=before&limit=32"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(page["text"].as_str().unwrap().len() <= 32);
    assert_eq!(page["offset"], 0);
    assert!(page["total_bytes"].as_u64().unwrap() > 32);
    assert!(page["next_offset"].is_number());

    let (status, _) = response_json(
        app.clone(),
        &format!("/api/passes/{pass_id}/ir?side=invalid"),
    )
    .await;
    assert_eq!(status, 400);

    let (status, _) = response_json(app.clone(), "/api/passes/999999/ir?side=before").await;
    assert_eq!(status, 404);

    let (status, _) = response_json(
        app,
        &format!("/api/passes/{pass_id}/ir?side=before&limit=0"),
    )
    .await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn functions_endpoint_lists_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();

    let (status, passes) = response_json(app.clone(), "/api/passes").await;
    assert_eq!(status, 200);
    let pass_id = passes[0]["children"][0]["id"].as_i64().unwrap();
    let (status, functions) = response_json(app, &format!("/api/passes/{pass_id}/functions")).await;

    assert_eq!(status, 200);
    let functions = functions.as_array().unwrap();
    let forward = functions
        .iter()
        .find(|function| function["name"] == "forward")
        .unwrap();
    assert!(forward["op_count"].as_u64().unwrap() >= 1);
    assert_eq!(forward["has_before"], true);
    assert_eq!(forward["has_after"], true);
}

#[tokio::test]
async fn diff_endpoint_returns_structural_changes() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let canonicalize = passes[0]["children"][0]["id"].as_i64().unwrap();

    let (status, diff) = response_msgpack::<FunctionDiff>(
        app,
        &format!("/api/passes/{canonicalize}/diff?func=forward"),
    )
    .await;

    assert_eq!(status, 200);
    assert!(diff.unwrap().changes.iter().any(|change| matches!(
        change.class,
        ChangeClass::Removed | ChangeClass::Added | ChangeClass::Modified
    )));
}

#[tokio::test]
async fn diff_endpoint_no_op_pass_is_all_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let cse = passes[0]["children"][1]["id"].as_i64().unwrap();

    let (status, diff) =
        response_msgpack::<FunctionDiff>(app, &format!("/api/passes/{cse}/diff?func=forward"))
            .await;

    assert_eq!(status, 200);
    assert!(diff
        .unwrap()
        .changes
        .iter()
        .all(|change| change.class == ChangeClass::Unchanged));
}

#[tokio::test]
async fn graph_endpoint_returns_nodes_and_respects_budget() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("demo.mlirtrace");
    trace_format::fixture::write_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let canonicalize = passes[0]["children"][0]["id"].as_i64().unwrap();

    let (status, graph) = response_msgpack::<DataflowGraph>(
        app.clone(),
        &format!("/api/graphs/dataflow?pass={canonicalize}&func=forward&diff=0&budget=2000"),
    )
    .await;
    assert_eq!(status, 200);
    let graph = graph.unwrap();
    assert!(!graph.nodes.is_empty());
    assert!(graph.nodes.iter().any(|node| node.uid.is_some()));

    let (status, diff_graph) = response_msgpack::<DataflowGraph>(
        app.clone(),
        &format!("/api/graphs/dataflow?pass={canonicalize}&func=forward&diff=1&budget=2000"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(diff_graph
        .unwrap()
        .nodes
        .iter()
        .any(|node| node.change.is_some()));

    let (status, small) = response_msgpack::<DataflowGraph>(
        app,
        &format!("/api/graphs/dataflow?pass={canonicalize}&func=forward&diff=0&budget=1"),
    )
    .await;
    assert_eq!(status, 200);
    assert!(small.unwrap().nodes.len() <= 1);
}

#[tokio::test]
async fn op_history_endpoints_resolve_full_fixture_and_validate_uids() {
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("full.mlirtrace");
    trace_format::fixture::write_full_demo_trace(&trace).unwrap();
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let canonicalize = passes[0]["children"][0]["id"].as_i64().unwrap();

    let (status, ops) = response_msgpack::<Vec<SelectableOp>>(
        app.clone(),
        &format!("/api/passes/{canonicalize}/ops?side=before&func=f"),
    )
    .await;
    assert_eq!(status, 200);
    let ops = ops.unwrap();
    let addi = ops
        .iter()
        .find(|operation| operation.name == "arith.addi")
        .unwrap();
    let muli = ops
        .iter()
        .find(|operation| operation.name == "arith.muli")
        .unwrap();

    let (status, addi_history) = response_msgpack::<OpHistory>(
        app.clone(),
        &format!("/api/ops/{}/history", addi.uid.as_str()),
    )
    .await;
    assert_eq!(status, 200);
    let addi_history = addi_history.unwrap();
    assert!(addi_history
        .steps
        .iter()
        .any(|step| step.change == HistoryChange::Replaced));
    assert!(addi_history
        .steps
        .iter()
        .any(|step| step.change == HistoryChange::Modified));
    assert!(addi_history
        .steps
        .iter()
        .all(|step| step.confidence == LinkConfidence::Exact));

    let (status, muli_history) = response_msgpack::<OpHistory>(
        app.clone(),
        &format!("/api/ops/{}/history", muli.uid.as_str()),
    )
    .await;
    assert_eq!(status, 200);
    assert!(muli_history
        .unwrap()
        .steps
        .iter()
        .any(|step| step.change == HistoryChange::Erased));

    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/ops/op2.Zg.1.a.0/history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/ops/op1.Zg.999.a.0/history")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn op_history_falls_back_to_fingerprints_without_identity_rows() {
    let trace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../testdata/golden/demo-v1.mlirtrace");
    let app = server::router(&trace).unwrap();
    let (_, passes) = response_json(app.clone(), "/api/passes").await;
    let canonicalize = passes[0]["children"][0]["id"].as_i64().unwrap();
    let (_, ops) = response_msgpack::<Vec<SelectableOp>>(
        app.clone(),
        &format!("/api/passes/{canonicalize}/ops?side=before&func=forward"),
    )
    .await;
    let uid = ops.unwrap()[0].uid.clone();
    let (status, history) =
        response_msgpack::<OpHistory>(app, &format!("/api/ops/{}/history", uid.as_str())).await;
    assert_eq!(status, 200);
    assert!(history
        .unwrap()
        .steps
        .iter()
        .any(|step| matches!(step.confidence, LinkConfidence::Inferred { .. })));
}
