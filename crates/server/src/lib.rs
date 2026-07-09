mod api;
mod assets;
mod cache;
mod msgpack;
mod provenance;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use trace_format::TraceReader;

use crate::cache::EngineCache;

#[derive(Clone)]
struct ServerState {
    trace_path: Arc<PathBuf>,
    cache: Arc<EngineCache>,
}

pub fn router(trace_path: impl AsRef<Path>) -> trace_format::Result<Router> {
    let trace_path = trace_path.as_ref().to_path_buf();
    TraceReader::open(&trace_path)?;
    let state = ServerState {
        trace_path: Arc::new(trace_path),
        cache: Arc::new(EngineCache::default()),
    };

    let api = Router::new()
        .route("/graphs/dataflow", get(api::graph))
        .route("/trace/info", get(api::trace_info))
        .route("/passes", get(api::passes))
        .route("/passes/{id}/diff", get(api::diff))
        .route("/passes/{id}/functions", get(api::functions))
        .route("/passes/{id}/ir", get(api::ir_page))
        .route("/passes/{id}/ops", get(api::selectable_ops))
        .route("/ops/{uid}", get(api::op_detail))
        .route("/ops/{uid}/history", get(api::op_history))
        .route("/search", get(api::search))
        .fallback(api::not_found);

    Ok(Router::new()
        .nest("/api", api)
        .fallback(assets::serve)
        .with_state(state))
}

#[cfg(test)]
mod tests {
    use axum::http::header;
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct Payload {
        value: u32,
    }

    #[tokio::test]
    async fn msgpack_response_round_trips_named_fields() {
        let response = crate::msgpack::Msgpack(Payload { value: 7 }).into_response();
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/msgpack"
        );
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let decoded: Payload = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(decoded, Payload { value: 7 });
    }
}
