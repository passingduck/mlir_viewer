use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use trace_format::{BlobId, PassId, PassNode, TraceError, TraceReader};

use crate::ServerState;

const DEFAULT_PAGE_BYTES: usize = 256 * 1024;
const MAX_PAGE_BYTES: usize = 256 * 1024;

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub(crate) struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl From<TraceError> for ApiError {
    fn from(error: TraceError) -> Self {
        match error {
            TraceError::VersionMismatch { .. } => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message: error.to_string(),
            },
            TraceError::Corrupt(message) if message.starts_with("missing pass ") => {
                Self::not_found(message)
            }
            TraceError::Corrupt(message) if message.starts_with("missing blob ") => {
                Self::not_found(message)
            }
            TraceError::Corrupt(message) => Self {
                status: StatusCode::UNPROCESSABLE_ENTITY,
                message,
            },
            TraceError::Sqlite(_) | TraceError::Io(_) => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Serialize)]
pub(crate) struct TraceInfo {
    format_version: String,
    pass_count: usize,
    meta: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub(crate) struct PassDto {
    id: i64,
    name: String,
    ir_before: Option<i64>,
    ir_after: Option<i64>,
    start_ns: i64,
    end_ns: i64,
    ir_changed: bool,
    children: Vec<PassDto>,
}

impl From<PassNode> for PassDto {
    fn from(node: PassNode) -> Self {
        Self {
            id: node.id.0,
            name: node.name,
            ir_before: node.ir_before.map(|id| id.0),
            ir_after: node.ir_after.map(|id| id.0),
            start_ns: node.start_ns,
            end_ns: node.end_ns,
            ir_changed: node.ir_changed,
            children: node.children.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct IrQuery {
    side: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Serialize)]
pub(crate) struct IrPage {
    pass_id: i64,
    side: String,
    text: String,
    offset: usize,
    next_offset: Option<usize>,
    total_bytes: usize,
}

#[derive(Serialize)]
pub(crate) struct FunctionDto {
    name: String,
    op_count: usize,
    has_before: bool,
    has_after: bool,
}

fn open(state: &ServerState) -> Result<TraceReader, ApiError> {
    TraceReader::open(&state.trace_path).map_err(Into::into)
}

fn parsed_side(
    state: &ServerState,
    reader: &TraceReader,
    blob: Option<BlobId>,
) -> Result<Option<std::sync::Arc<engine::ParsedModule>>, ApiError> {
    match blob {
        None => Ok(None),
        Some(blob) => {
            let text = reader.blob_text(blob)?;
            Ok(Some(state.cache.parsed(blob, &text)))
        }
    }
}

fn count_passes(nodes: &[PassNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + count_passes(&node.children))
        .sum()
}

pub(crate) async fn trace_info(
    State(state): State<ServerState>,
) -> Result<Json<TraceInfo>, ApiError> {
    let reader = open(&state)?;
    let meta = reader.meta()?;
    let format_version = meta
        .get("format_version")
        .cloned()
        .ok_or_else(|| ApiError::from(TraceError::Corrupt("missing format_version".into())))?;
    let roots = reader.passes()?;
    Ok(Json(TraceInfo {
        format_version,
        pass_count: count_passes(&roots),
        meta,
    }))
}

pub(crate) async fn passes(
    State(state): State<ServerState>,
) -> Result<Json<Vec<PassDto>>, ApiError> {
    let roots = open(&state)?.passes()?;
    Ok(Json(roots.into_iter().map(Into::into).collect()))
}

pub(crate) async fn ir_page(
    State(state): State<ServerState>,
    Path(id): Path<i64>,
    Query(query): Query<IrQuery>,
) -> Result<Json<IrPage>, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE_BYTES);
    if limit == 0 || limit > MAX_PAGE_BYTES {
        return Err(ApiError::bad_request(format!(
            "limit must be between 1 and {MAX_PAGE_BYTES}"
        )));
    }
    if query.side != "before" && query.side != "after" {
        return Err(ApiError::bad_request("side must be 'before' or 'after'"));
    }

    let reader = open(&state)?;
    let pass = reader.pass(PassId(id))?;
    let blob: BlobId = match query.side.as_str() {
        "before" => pass.ir_before,
        "after" => pass.ir_after,
        _ => unreachable!(),
    }
    .ok_or_else(|| ApiError::not_found(format!("pass {id} has no {} snapshot", query.side)))?;
    let text = reader.blob_text(blob)?;
    let total_bytes = text.len();
    let mut start = query.offset.unwrap_or(0);
    if start > total_bytes {
        return Err(ApiError::bad_request("offset exceeds snapshot size"));
    }
    while start < total_bytes && !text.is_char_boundary(start) {
        start += 1;
    }
    let mut end = start.saturating_add(limit).min(total_bytes);
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let next_offset = (end < total_bytes).then_some(end);

    Ok(Json(IrPage {
        pass_id: id,
        side: query.side,
        text: text[start..end].to_owned(),
        offset: start,
        next_offset,
        total_bytes,
    }))
}

pub(crate) async fn functions(
    State(state): State<ServerState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<FunctionDto>>, ApiError> {
    let reader = open(&state)?;
    let pass = reader.pass(PassId(id))?;
    let before = parsed_side(&state, &reader, pass.ir_before)?;
    let after = parsed_side(&state, &reader, pass.ir_after)?;
    let mut functions: BTreeMap<String, (usize, bool, bool)> = BTreeMap::new();

    if let Some(module) = &before {
        for function in &module.functions {
            let entry = functions
                .entry(function.name.clone())
                .or_insert((0, false, false));
            entry.0 = entry.0.max(function.ops.len());
            entry.1 = true;
        }
    }
    if let Some(module) = &after {
        for function in &module.functions {
            let entry = functions
                .entry(function.name.clone())
                .or_insert((0, false, false));
            entry.0 = entry.0.max(function.ops.len());
            entry.2 = true;
        }
    }

    Ok(Json(
        functions
            .into_iter()
            .map(|(name, (op_count, has_before, has_after))| FunctionDto {
                name,
                op_count,
                has_before,
                has_after,
            })
            .collect(),
    ))
}

pub(crate) async fn not_found() -> ApiError {
    ApiError::not_found("API route not found")
}
