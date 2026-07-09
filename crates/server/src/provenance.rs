use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use engine::{
    EvidenceSource, NormalizedIdentityEvent, NormalizedIdentityKind, ParsedModule, SnapshotOps,
    SnapshotSide, TimelineStage,
};
use trace_format::{
    BlobId, IdentityKind, IdentitySource, OpIndexRow, PassId, PassNode, Side, TraceReader,
};

use crate::ServerState;

pub(crate) fn collect_leaves<'a>(
    nodes: &'a [PassNode],
    order: &mut usize,
    output: &mut Vec<(usize, &'a PassNode)>,
) {
    for node in nodes {
        let traversal_order = *order;
        *order += 1;
        if node.children.is_empty() {
            output.push((traversal_order, node));
        } else {
            collect_leaves(&node.children, order, output);
        }
    }
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' && index + 1 < text.len() {
            starts.push(index + 1);
        }
    }
    starts
}

fn row_op_idx(
    row: &OpIndexRow,
    module: &ParsedModule,
    scope_ops: &HashSet<usize>,
    starts: &[usize],
    text_len: usize,
) -> Option<usize> {
    if row.byte_start < 0 {
        return None;
    }
    if row.byte_end == -1 {
        let op_idx = usize::try_from(row.byte_start).ok()?;
        return module
            .ops
            .get(op_idx)
            .and_then(|operation| scope_ops.contains(&operation.idx).then_some(operation.idx));
    }
    if row.byte_end < row.byte_start {
        return None;
    }
    let byte_start = usize::try_from(row.byte_start).ok()?;
    let byte_end = usize::try_from(row.byte_end).ok()?;
    if byte_end > text_len {
        return None;
    }
    module
        .ops
        .iter()
        .filter(|operation| scope_ops.contains(&operation.idx))
        .filter_map(|operation| {
            let start = *starts.get(operation.line_start.checked_sub(1)?)?;
            let end = starts.get(operation.line_end).copied().unwrap_or(text_len);
            (start <= byte_start && byte_end <= end).then_some((end - start, operation.idx))
        })
        .min_by_key(|(width, op_idx)| (*width, *op_idx))
        .map(|(_, op_idx)| op_idx)
}

fn build_snapshot<F>(
    reader: &TraceReader,
    pass_id: PassId,
    blob_id: Option<BlobId>,
    side: SnapshotSide,
    function: &str,
    parse: &mut F,
) -> trace_format::Result<Option<SnapshotOps>>
where
    F: FnMut(BlobId, &str) -> Arc<ParsedModule>,
{
    let Some(blob_id) = blob_id else {
        return Ok(None);
    };
    let text = reader.blob_text(blob_id)?;
    let module = parse(blob_id, &text);
    let Some(scope) = module.scope(function) else {
        return Ok(None);
    };
    let function_ordinals = scope
        .ops
        .iter()
        .copied()
        .enumerate()
        .map(|(ordinal, op_idx)| (op_idx, ordinal))
        .collect();
    let scope_ops: HashSet<_> = scope.ops.iter().copied().collect();
    let starts = line_starts(&text);
    let trace_side = match side {
        SnapshotSide::Before => Side::Before,
        SnapshotSide::After => Side::After,
    };
    let mut tokens = HashMap::new();
    let mut duplicate_tokens = HashSet::new();
    for row in reader
        .op_index(pass_id)?
        .into_iter()
        .filter(|row| row.side == trace_side)
    {
        if duplicate_tokens.contains(&row.ptr_token) {
            continue;
        }
        let Some(op_idx) = row_op_idx(&row, &module, &scope_ops, &starts, text.len()) else {
            continue;
        };
        if tokens.insert(row.ptr_token, op_idx).is_some() {
            tokens.remove(&row.ptr_token);
            duplicate_tokens.insert(row.ptr_token);
        }
    }
    Ok(Some(SnapshotOps {
        side,
        blob_id: Some(blob_id.0),
        module: (*module).clone(),
        function_ordinals,
        tokens,
    }))
}

fn normalize_event(event: trace_format::IdentityEvent) -> NormalizedIdentityEvent {
    let kind = match event.kind {
        IdentityKind::Inserted => NormalizedIdentityKind::Inserted,
        IdentityKind::Erased => NormalizedIdentityKind::Erased,
        IdentityKind::Replaced => NormalizedIdentityKind::Replaced,
        IdentityKind::Modified => NormalizedIdentityKind::Modified,
    };
    let source = match event.source {
        IdentitySource::Listener => EvidenceSource::Listener,
        IdentitySource::Action => EvidenceSource::Action,
    };
    NormalizedIdentityEvent {
        kind,
        ptr_token: event.ptr_token,
        new_token: event.new_token,
        pattern: event.pattern,
        source,
        seq: event.seq,
    }
}

pub(crate) fn build_timeline<F>(
    reader: &TraceReader,
    function: &str,
    mut parse: F,
) -> trace_format::Result<Vec<TimelineStage>>
where
    F: FnMut(BlobId, &str) -> Arc<ParsedModule>,
{
    let roots = reader.passes()?;
    let mut leaves = Vec::new();
    collect_leaves(&roots, &mut 0, &mut leaves);
    leaves.sort_by_key(|(order, pass)| (pass.start_ns, *order));

    let mut timeline = Vec::new();
    for (_, pass) in leaves {
        let before = build_snapshot(
            reader,
            pass.id,
            pass.ir_before,
            SnapshotSide::Before,
            function,
            &mut parse,
        )?;
        let after = build_snapshot(
            reader,
            pass.id,
            pass.ir_after,
            SnapshotSide::After,
            function,
            &mut parse,
        )?;
        if before.is_none() && after.is_none() {
            continue;
        }
        let events = reader
            .identity_events(pass.id)?
            .into_iter()
            .map(normalize_event)
            .collect();
        timeline.push(TimelineStage {
            pass_id: pass.id.0,
            pass_name: pass.name.clone(),
            before,
            after,
            events,
        });
    }
    Ok(timeline)
}

pub(crate) fn resolved_function(
    state: &ServerState,
    reader: &TraceReader,
    function: &str,
) -> trace_format::Result<Option<Arc<engine::ResolvedFunction>>> {
    if let Some(resolved) = state.cache.resolved(function) {
        return Ok(Some(resolved));
    }
    let timeline = if let Some(timeline) = state.cache.timeline(function) {
        timeline
    } else {
        let timeline = Arc::new(build_timeline(reader, function, |blob, text| {
            state.cache.parsed(blob, text)
        })?);
        if timeline.is_empty() {
            return Ok(None);
        }
        state.cache.put_timeline(function, timeline.clone());
        timeline
    };
    let resolved = Arc::new(engine::resolve_function(function, &timeline));
    state.cache.put_resolved(function, resolved.clone());
    Ok(Some(resolved))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use engine::NormalizedIdentityKind;
    use trace_format::TraceReader;

    #[test]
    fn full_fixture_normalizes_leaf_stages_tokens_and_events() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("full.mlirtrace");
        trace_format::fixture::write_full_demo_trace(&path).unwrap();
        let reader = TraceReader::open(&path).unwrap();

        let timeline =
            super::build_timeline(&reader, "f", |_, text| Arc::new(engine::parse_module(text)))
                .unwrap();

        assert_eq!(timeline.len(), 3);
        assert_eq!(timeline[0].pass_name, "canonicalize");
        assert!(timeline[0]
            .before
            .as_ref()
            .unwrap()
            .tokens
            .contains_key(&0x1001));
        assert!(timeline[0]
            .after
            .as_ref()
            .unwrap()
            .tokens
            .contains_key(&0x1081));
        assert_eq!(timeline[0].events[0].kind, NormalizedIdentityKind::Replaced);
        assert_eq!(timeline[1].events[0].kind, NormalizedIdentityKind::Erased);
        assert_eq!(timeline[2].events[0].kind, NormalizedIdentityKind::Modified);
    }

    #[test]
    fn text_fixture_normalizes_without_identity_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("text.mlirtrace");
        trace_format::fixture::write_demo_trace(&path).unwrap();
        let reader = TraceReader::open(&path).unwrap();

        let timeline = super::build_timeline(&reader, "forward", |_, text| {
            Arc::new(engine::parse_module(text))
        })
        .unwrap();

        assert_eq!(timeline.len(), 5);
        assert!(timeline.iter().all(|stage| stage.events.is_empty()));
        assert!(timeline.iter().all(|stage| stage
            .before
            .as_ref()
            .is_none_or(|snapshot| snapshot.tokens.is_empty())));
    }
}
