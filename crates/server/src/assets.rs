use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::Response;
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../ui/dist/"]
struct Assets;

const FALLBACK_INDEX: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>MLIR Viewer</title></head>
<body><main><h1>MLIR Viewer</h1><p>Build ui/ before packaging the binary.</p></main></body></html>"#;

pub(crate) async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let requested = if path.is_empty() { "index.html" } else { path };
    let asset = Assets::get(requested).or_else(|| Assets::get("index.html"));
    match asset {
        Some(asset) => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                mime_guess::from_path(requested)
                    .first_or_octet_stream()
                    .as_ref(),
            )
            .body(Body::from(asset.data.into_owned()))
            .expect("valid asset response"),
        None => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(FALLBACK_INDEX))
            .expect("valid fallback response"),
    }
}
