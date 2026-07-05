mod api;
mod assets;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::routing::get;
use axum::Router;
use trace_format::TraceReader;

#[derive(Clone)]
struct ServerState {
    trace_path: Arc<PathBuf>,
}

pub fn router(trace_path: impl AsRef<Path>) -> trace_format::Result<Router> {
    let trace_path = trace_path.as_ref().to_path_buf();
    TraceReader::open(&trace_path)?;
    let state = ServerState {
        trace_path: Arc::new(trace_path),
    };

    let api = Router::new()
        .route("/trace/info", get(api::trace_info))
        .route("/passes", get(api::passes))
        .route("/passes/{id}/ir", get(api::ir_page))
        .fallback(api::not_found);

    Ok(Router::new()
        .nest("/api", api)
        .fallback(assets::serve)
        .with_state(state))
}
