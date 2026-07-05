use std::collections::BTreeMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use trace_format::{BlobId, PassId, PassNode, TraceError, TraceReader};

use crate::msgpack::Msgpack;
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

#[derive(Deserialize)]
pub(crate) struct DiffQuery {
    func: String,
}

#[derive(Deserialize)]
pub(crate) struct OpsQuery {
    side: String,
    func: String,
}

#[derive(Deserialize)]
pub(crate) struct GraphQuery {
    pass: i64,
    func: String,
    #[serde(default)]
    diff: u8,
    budget: Option<usize>,
}

const DEFAULT_GRAPH_BUDGET: usize = 2000;
const MAX_GRAPH_BUDGET: usize = 5000;

fn decorate_graph_uids(
    state: &ServerState,
    reader: &TraceReader,
    pass_id: i64,
    function: &str,
    default_side: engine::SnapshotSide,
    graph: &mut engine::DataflowGraph,
) -> Result<(), ApiError> {
    let Some(resolved) = crate::provenance::resolved_function(state, reader, function)? else {
        return Ok(());
    };
    let Some(timeline) = state.cache.timeline(function) else {
        return Ok(());
    };
    let Some(stage_index) = timeline.iter().position(|stage| stage.pass_id == pass_id) else {
        return Ok(());
    };
    for node in &mut graph.nodes {
        let (Some(op_idx), side) = (node.op_idx, node.provenance_side.unwrap_or(default_side))
        else {
            continue;
        };
        node.uid = resolved
            .selectable
            .get(&engine::OccurrenceKey {
                stage_index,
                side,
                op_idx,
            })
            .map(|operation| operation.uid.as_str().to_string());
    }
    Ok(())
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

pub(crate) async fn selectable_ops(
    State(state): State<ServerState>,
    Path(id): Path<i64>,
    Query(query): Query<OpsQuery>,
) -> Result<Msgpack<Vec<engine::SelectableOp>>, ApiError> {
    let side = match query.side.as_str() {
        "before" => engine::SnapshotSide::Before,
        "after" => engine::SnapshotSide::After,
        _ => return Err(ApiError::bad_request("side must be 'before' or 'after'")),
    };
    let reader = open(&state)?;
    reader.pass(PassId(id))?;
    let resolved = crate::provenance::resolved_function(&state, &reader, &query.func)?
        .ok_or_else(|| ApiError::not_found(format!("function {:?} not found", query.func)))?;
    let timeline = state
        .cache
        .timeline(&query.func)
        .ok_or_else(|| ApiError::not_found(format!("function {:?} not found", query.func)))?;
    let stage_index = timeline
        .iter()
        .position(|stage| stage.pass_id == id)
        .ok_or_else(|| ApiError::not_found(format!("pass {id} is not an executable leaf")))?;
    let snapshot_exists = match side {
        engine::SnapshotSide::Before => timeline[stage_index].before.is_some(),
        engine::SnapshotSide::After => timeline[stage_index].after.is_some(),
    };
    if !snapshot_exists {
        return Err(ApiError::not_found(format!(
            "pass {id} has no {} snapshot for function {:?}",
            query.side, query.func
        )));
    }
    let mut operations: Vec<_> = resolved
        .selectable
        .iter()
        .filter(|(key, _)| key.stage_index == stage_index && key.side == side)
        .map(|(_, operation)| operation.clone())
        .collect();
    operations.sort_by_key(|operation| (operation.line_start, operation.op_idx));
    Ok(Msgpack(operations))
}

pub(crate) async fn op_history(
    State(state): State<ServerState>,
    Path(uid): Path<String>,
) -> Result<Msgpack<engine::OpHistory>, ApiError> {
    let uid =
        engine::OpUid::parse(&uid).map_err(|error| ApiError::bad_request(error.to_string()))?;
    let anchor = uid
        .parse_anchor()
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let reader = open(&state)?;
    let resolved = crate::provenance::resolved_function(&state, &reader, &anchor.function)?
        .ok_or_else(|| ApiError::not_found(format!("function {:?} not found", anchor.function)))?;
    let history =
        resolved.histories.get(&uid).cloned().ok_or_else(|| {
            ApiError::not_found(format!("operation UID {} not found", uid.as_str()))
        })?;
    Ok(Msgpack(history))
}

pub(crate) async fn diff(
    State(state): State<ServerState>,
    Path(id): Path<i64>,
    Query(query): Query<DiffQuery>,
) -> Result<Msgpack<engine::FunctionDiff>, ApiError> {
    let reader = open(&state)?;
    let pass = reader.pass(PassId(id))?;
    let (Some(before_id), Some(after_id)) = (pass.ir_before, pass.ir_after) else {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: format!("pass {id} is missing a before or after snapshot"),
        });
    };

    if before_id == after_id {
        let text = reader.blob_text(after_id)?;
        let module = state.cache.parsed(after_id, &text);
        let changes = module
            .scope(&query.func)
            .map(|scope| {
                scope
                    .ops
                    .iter()
                    .map(|&op_idx| {
                        let op = &module.ops[op_idx];
                        engine::OpChange {
                            class: engine::ChangeClass::Unchanged,
                            before: Some(op_idx),
                            after: Some(op_idx),
                            before_lines: Some((op.line_start, op.line_end)),
                            after_lines: Some((op.line_start, op.line_end)),
                            detail: Vec::new(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        return Ok(Msgpack(engine::FunctionDiff {
            func: query.func,
            changes,
        }));
    }

    let before_text = reader.blob_text(before_id)?;
    let after_text = reader.blob_text(after_id)?;
    let before = state.cache.parsed(before_id, &before_text);
    let after = state.cache.parsed(after_id, &after_text);
    let func = query.func;
    let diff = state.cache.diff(before_id, after_id, &func, || {
        engine::diff_function(&before, &after, &func, &engine::GreedyFingerprintMatcher)
    });
    Ok(Msgpack((*diff).clone()))
}

pub(crate) async fn graph(
    State(state): State<ServerState>,
    Query(query): Query<GraphQuery>,
) -> Result<Msgpack<engine::DataflowGraph>, ApiError> {
    let budget = query
        .budget
        .unwrap_or(DEFAULT_GRAPH_BUDGET)
        .clamp(1, MAX_GRAPH_BUDGET);
    let reader = open(&state)?;
    let pass = reader.pass(PassId(query.pass))?;

    if query.diff == 1 {
        let (Some(before_id), Some(after_id)) = (pass.ir_before, pass.ir_after) else {
            return Err(ApiError {
                status: StatusCode::CONFLICT,
                message: format!("pass {} is missing a before or after snapshot", query.pass),
            });
        };
        let before_text = reader.blob_text(before_id)?;
        let after_text = reader.blob_text(after_id)?;
        let before = state.cache.parsed(before_id, &before_text);
        let after = state.cache.parsed(after_id, &after_text);
        let mut graph = engine::extract_dataflow_diff(
            &before,
            &after,
            &query.func,
            budget,
            &engine::GreedyFingerprintMatcher,
        );
        decorate_graph_uids(
            &state,
            &reader,
            query.pass,
            &query.func,
            engine::SnapshotSide::After,
            &mut graph,
        )?;
        return Ok(Msgpack(graph));
    }

    let (blob, side) = if let Some(blob) = pass.ir_after {
        (blob, engine::SnapshotSide::After)
    } else if let Some(blob) = pass.ir_before {
        (blob, engine::SnapshotSide::Before)
    } else {
        return Err(ApiError::not_found(format!(
            "pass {} has no snapshot",
            query.pass
        )));
    };
    let text = reader.blob_text(blob)?;
    let module = state.cache.parsed(blob, &text);
    let mut graph = engine::extract_dataflow(&module, &query.func, budget);
    decorate_graph_uids(&state, &reader, query.pass, &query.func, side, &mut graph)?;
    Ok(Msgpack(graph))
}

pub(crate) async fn not_found() -> ApiError {
    ApiError::not_found("API route not found")
}
