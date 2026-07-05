use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// A MessagePack response body for bulk payloads.
pub struct Msgpack<T>(pub T);

impl<T: Serialize> IntoResponse for Msgpack<T> {
    fn into_response(self) -> Response {
        match rmp_serde::to_vec_named(&self.0) {
            Ok(bytes) => ([(header::CONTENT_TYPE, "application/msgpack")], bytes).into_response(),
            Err(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("msgpack encode failed: {error}"),
            )
                .into_response(),
        }
    }
}
