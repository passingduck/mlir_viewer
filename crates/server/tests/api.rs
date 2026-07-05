use axum::body::Body;
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
    assert_eq!(info["format_version"], "1");
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
