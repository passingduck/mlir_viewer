use std::collections::HashMap;
use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::{
    fingerprint_score, GreedyFingerprintMatcher, OpFingerprint, OpIdx, OpMatcher, ParsedModule,
    ParsedOp,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotSide {
    Before,
    After,
}

impl SnapshotSide {
    fn code(self) -> &'static str {
        match self {
            Self::Before => "b",
            Self::After => "a",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpAnchor {
    pub function: String,
    pub pass_id: i64,
    pub side: SnapshotSide,
    pub function_ordinal: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub struct OpUid(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UidError(String);

impl fmt::Display for UidError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UidError {}

impl OpUid {
    pub fn from_anchor(anchor: &OpAnchor) -> Self {
        let function = URL_SAFE_NO_PAD.encode(anchor.function.as_bytes());
        Self(format!(
            "op1.{function}.{}.{}.{}",
            anchor.pass_id,
            anchor.side.code(),
            anchor.function_ordinal
        ))
    }

    pub fn parse(value: &str) -> Result<Self, UidError> {
        let uid = Self(value.to_string());
        uid.parse_anchor()?;
        Ok(uid)
    }

    pub fn parse_anchor(&self) -> Result<OpAnchor, UidError> {
        let fields: Vec<_> = self.0.split('.').collect();
        if fields.len() != 5 || fields[0] != "op1" {
            return Err(UidError("invalid op UID version or field count".into()));
        }
        let function_bytes = URL_SAFE_NO_PAD
            .decode(fields[1])
            .map_err(|_| UidError("invalid op UID function encoding".into()))?;
        let function = String::from_utf8(function_bytes)
            .map_err(|_| UidError("op UID function is not UTF-8".into()))?;
        let pass_id = fields[2]
            .parse()
            .map_err(|_| UidError("invalid op UID pass id".into()))?;
        let side = match fields[3] {
            "b" => SnapshotSide::Before,
            "a" => SnapshotSide::After,
            _ => return Err(UidError("invalid op UID side".into())),
        };
        let function_ordinal = fields[4]
            .parse()
            .map_err(|_| UidError("invalid op UID ordinal".into()))?;
        Ok(OpAnchor {
            function,
            pass_id,
            side,
            function_ordinal,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OccurrenceKey {
    pub stage_index: usize,
    pub side: SnapshotSide,
    pub op_idx: OpIdx,
}

#[derive(Debug, Clone)]
pub struct SnapshotOps {
    pub side: SnapshotSide,
    pub blob_id: Option<i64>,
    pub module: ParsedModule,
    pub function_ordinals: HashMap<OpIdx, usize>,
    pub tokens: HashMap<i64, OpIdx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormalizedIdentityKind {
    Inserted,
    Erased,
    Replaced,
    Modified,
}

#[derive(Debug, Clone)]
pub struct NormalizedIdentityEvent {
    pub kind: NormalizedIdentityKind,
    pub ptr_token: i64,
    pub new_token: Option<i64>,
    pub pattern: Option<String>,
    pub source: EvidenceSource,
    pub seq: i64,
}

#[derive(Debug, Clone)]
pub struct TimelineStage {
    pub pass_id: i64,
    pub pass_name: String,
    pub before: Option<SnapshotOps>,
    pub after: Option<SnapshotOps>,
    pub events: Vec<NormalizedIdentityEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    Listener,
    Action,
    Fingerprint,
    SharedSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HistoryEvidence {
    pub seq: i64,
    pub pattern: Option<String>,
    pub source: EvidenceSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LinkConfidence {
    Exact,
    Inferred { score: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryChange {
    Inserted,
    Erased,
    Replaced,
    Modified,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct OpOccurrence {
    pub side: SnapshotSide,
    pub op_idx: OpIdx,
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
    pub attr_summary: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct HistoryStep {
    pub pass_id: i64,
    pub pass_name: String,
    pub change: HistoryChange,
    pub before: Option<OpOccurrence>,
    pub after: Option<OpOccurrence>,
    pub evidence: Vec<HistoryEvidence>,
    pub confidence: LinkConfidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct OpHistory {
    pub uid: OpUid,
    pub first_name: String,
    pub last_name: String,
    pub steps: Vec<HistoryStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct SelectableOp {
    pub uid: OpUid,
    pub op_idx: OpIdx,
    pub name: String,
    pub line_start: usize,
    pub line_end: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedFunction {
    pub function: String,
    pub selectable: HashMap<OccurrenceKey, SelectableOp>,
    pub histories: HashMap<OpUid, OpHistory>,
}

#[derive(Debug, Clone)]
struct Node {
    key: OccurrenceKey,
    function_ordinal: usize,
    pass_id: i64,
    operation: ParsedOp,
}

#[derive(Debug, Clone)]
struct Relation {
    stage_index: usize,
    from: Option<usize>,
    to: Option<usize>,
    change: HistoryChange,
    evidence: Vec<HistoryEvidence>,
    confidence: LinkConfidence,
    record_step: bool,
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
        }
    }

    fn find(&mut self, value: usize) -> usize {
        if self.parent[value] != value {
            self.parent[value] = self.find(self.parent[value]);
        }
        self.parent[value]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left = self.find(left);
        let right = self.find(right);
        if left != right {
            self.parent[right] = left;
        }
    }
}

fn occurrence(operation: &ParsedOp, side: SnapshotSide) -> OpOccurrence {
    OpOccurrence {
        side,
        op_idx: operation.idx,
        name: operation.name.clone(),
        line_start: operation.line_start,
        line_end: operation.line_end,
        attr_summary: operation.attr_summary.clone(),
        location: operation.location.clone(),
    }
}

fn changed(before: &ParsedOp, after: &ParsedOp) -> HistoryChange {
    if before.name != after.name {
        HistoryChange::Replaced
    } else if before.result_types != after.result_types
        || before.operands.len() != after.operands.len()
        || before.attr_summary != after.attr_summary
    {
        HistoryChange::Modified
    } else {
        HistoryChange::Unchanged
    }
}

fn event_evidence(event: &NormalizedIdentityEvent) -> HistoryEvidence {
    HistoryEvidence {
        seq: event.seq,
        pattern: event.pattern.clone(),
        source: event.source,
    }
}

fn push_relation(relations: &mut Vec<Relation>, relation: Relation) {
    if relation.record_step {
        if let Some(existing) = relations.iter_mut().find(|existing| {
            existing.stage_index == relation.stage_index
                && existing.from == relation.from
                && existing.to == relation.to
                && existing.change == relation.change
                && existing.record_step
        }) {
            existing.evidence.extend(relation.evidence);
            existing.evidence.sort_by_key(|evidence| evidence.seq);
            return;
        }
    }
    relations.push(relation);
}

fn snapshot_nodes(
    function: &str,
    stage_index: usize,
    pass_id: i64,
    snapshot: &SnapshotOps,
    nodes: &mut Vec<Node>,
    node_ids: &mut HashMap<OccurrenceKey, usize>,
) {
    let Some(scope) = snapshot.module.scope(function) else {
        return;
    };
    for (fallback_ordinal, &op_idx) in scope.ops.iter().enumerate() {
        let key = OccurrenceKey {
            stage_index,
            side: snapshot.side,
            op_idx,
        };
        let id = nodes.len();
        nodes.push(Node {
            key,
            function_ordinal: snapshot
                .function_ordinals
                .get(&op_idx)
                .copied()
                .unwrap_or(fallback_ordinal),
            pass_id,
            operation: snapshot.module.ops[op_idx].clone(),
        });
        node_ids.insert(key, id);
    }
}

fn node_for_token(
    snapshot: Option<&SnapshotOps>,
    token: i64,
    stage_index: usize,
    node_ids: &HashMap<OccurrenceKey, usize>,
) -> Option<usize> {
    let snapshot = snapshot?;
    let op_idx = *snapshot.tokens.get(&token)?;
    node_ids
        .get(&OccurrenceKey {
            stage_index,
            side: snapshot.side,
            op_idx,
        })
        .copied()
}

struct FallbackContext<'a> {
    function: &'a str,
    nodes: &'a [Node],
    node_ids: &'a HashMap<OccurrenceKey, usize>,
    relations: &'a mut Vec<Relation>,
}

fn fallback_relations(
    context: &mut FallbackContext<'_>,
    stage_index: usize,
    before: &SnapshotOps,
    after: &SnapshotOps,
    used_before: &mut std::collections::HashSet<usize>,
    used_after: &mut std::collections::HashSet<usize>,
) {
    let Some(before_scope) = before.module.scope(context.function) else {
        return;
    };
    let Some(after_scope) = after.module.scope(context.function) else {
        return;
    };
    let before_ops: Vec<_> = before_scope
        .ops
        .iter()
        .copied()
        .filter(|&op_idx| {
            context
                .node_ids
                .get(&OccurrenceKey {
                    stage_index,
                    side: before.side,
                    op_idx,
                })
                .is_some_and(|id| !used_before.contains(id))
        })
        .collect();
    let after_ops: Vec<_> = after_scope
        .ops
        .iter()
        .copied()
        .filter(|&op_idx| {
            context
                .node_ids
                .get(&OccurrenceKey {
                    stage_index,
                    side: after.side,
                    op_idx,
                })
                .is_some_and(|id| !used_after.contains(id))
        })
        .collect();
    for (before_idx, after_idx) in
        GreedyFingerprintMatcher.match_ops(&before.module, &before_ops, &after.module, &after_ops)
    {
        let (Some(before_idx), Some(after_idx)) = (before_idx, after_idx) else {
            continue;
        };
        let from = context.node_ids[&OccurrenceKey {
            stage_index,
            side: before.side,
            op_idx: before_idx,
        }];
        let to = context.node_ids[&OccurrenceKey {
            stage_index,
            side: after.side,
            op_idx: after_idx,
        }];
        let score = fingerprint_score(
            &OpFingerprint::of(&before.module.ops[before_idx]),
            &OpFingerprint::of(&after.module.ops[after_idx]),
        )
        .expect("matcher never pairs different operation names");
        used_before.insert(from);
        used_after.insert(to);
        push_relation(
            context.relations,
            Relation {
                stage_index,
                from: Some(from),
                to: Some(to),
                change: changed(&context.nodes[from].operation, &context.nodes[to].operation),
                evidence: vec![HistoryEvidence {
                    seq: 0,
                    pattern: None,
                    source: EvidenceSource::Fingerprint,
                }],
                confidence: LinkConfidence::Inferred { score },
                record_step: true,
            },
        );
    }
}

fn bridge_stages(
    function: &str,
    left_index: usize,
    right_index: usize,
    stages: &[TimelineStage],
    node_ids: &HashMap<OccurrenceKey, usize>,
    relations: &mut Vec<Relation>,
) {
    let (Some(left), Some(right)) = (
        stages[left_index].after.as_ref(),
        stages[right_index].before.as_ref(),
    ) else {
        return;
    };
    let (Some(left_scope), Some(right_scope)) =
        (left.module.scope(function), right.module.scope(function))
    else {
        return;
    };
    let shared = left.blob_id.is_some() && left.blob_id == right.blob_id;
    let pairs = if shared {
        left_scope
            .ops
            .iter()
            .copied()
            .filter(|idx| right_scope.ops.contains(idx))
            .map(|idx| (Some(idx), Some(idx)))
            .collect()
    } else {
        GreedyFingerprintMatcher.match_ops(
            &left.module,
            &left_scope.ops,
            &right.module,
            &right_scope.ops,
        )
    };
    for (left_op, right_op) in pairs {
        let (Some(left_op), Some(right_op)) = (left_op, right_op) else {
            continue;
        };
        let Some(&from) = node_ids.get(&OccurrenceKey {
            stage_index: left_index,
            side: left.side,
            op_idx: left_op,
        }) else {
            continue;
        };
        let Some(&to) = node_ids.get(&OccurrenceKey {
            stage_index: right_index,
            side: right.side,
            op_idx: right_op,
        }) else {
            continue;
        };
        relations.push(Relation {
            stage_index: right_index,
            from: Some(from),
            to: Some(to),
            change: HistoryChange::Unchanged,
            evidence: vec![HistoryEvidence {
                seq: 0,
                pattern: None,
                source: if shared {
                    EvidenceSource::SharedSnapshot
                } else {
                    EvidenceSource::Fingerprint
                },
            }],
            confidence: if shared {
                LinkConfidence::Exact
            } else {
                let score = fingerprint_score(
                    &OpFingerprint::of(&left.module.ops[left_op]),
                    &OpFingerprint::of(&right.module.ops[right_op]),
                )
                .expect("matcher never pairs different operation names");
                LinkConfidence::Inferred { score }
            },
            record_step: false,
        });
    }
}

pub fn resolve_function(function: &str, stages: &[TimelineStage]) -> ResolvedFunction {
    let mut nodes = Vec::new();
    let mut node_ids = HashMap::new();
    for (stage_index, stage) in stages.iter().enumerate() {
        if let Some(before) = &stage.before {
            snapshot_nodes(
                function,
                stage_index,
                stage.pass_id,
                before,
                &mut nodes,
                &mut node_ids,
            );
        }
        if let Some(after) = &stage.after {
            snapshot_nodes(
                function,
                stage_index,
                stage.pass_id,
                after,
                &mut nodes,
                &mut node_ids,
            );
        }
    }

    let mut relations = Vec::new();
    for (stage_index, stage) in stages.iter().enumerate() {
        let mut events = stage.events.clone();
        events.sort_by_key(|event| event.seq);
        let mut used_before = std::collections::HashSet::new();
        let mut used_after = std::collections::HashSet::new();
        let mut successor_by_old = std::collections::HashSet::new();
        let mut inserted_after = std::collections::HashSet::new();

        for event in &events {
            match event.kind {
                NormalizedIdentityKind::Replaced => {
                    let Some(from) = node_for_token(
                        stage.before.as_ref(),
                        event.ptr_token,
                        stage_index,
                        &node_ids,
                    ) else {
                        continue;
                    };
                    if successor_by_old.contains(&from) {
                        continue;
                    }
                    let to = event.new_token.and_then(|token| {
                        node_for_token(stage.after.as_ref(), token, stage_index, &node_ids)
                    });
                    successor_by_old.insert(from);
                    used_before.insert(from);
                    if let Some(to) = to {
                        used_after.insert(to);
                    }
                    push_relation(
                        &mut relations,
                        Relation {
                            stage_index,
                            from: Some(from),
                            to,
                            change: if to.is_some() {
                                HistoryChange::Replaced
                            } else {
                                HistoryChange::Erased
                            },
                            evidence: vec![event_evidence(event)],
                            confidence: LinkConfidence::Exact,
                            record_step: true,
                        },
                    );
                }
                NormalizedIdentityKind::Modified => {
                    let from = node_for_token(
                        stage.before.as_ref(),
                        event.ptr_token,
                        stage_index,
                        &node_ids,
                    );
                    let to = node_for_token(
                        stage.after.as_ref(),
                        event.ptr_token,
                        stage_index,
                        &node_ids,
                    );
                    let (Some(from), Some(to)) = (from, to) else {
                        continue;
                    };
                    used_before.insert(from);
                    used_after.insert(to);
                    push_relation(
                        &mut relations,
                        Relation {
                            stage_index,
                            from: Some(from),
                            to: Some(to),
                            change: HistoryChange::Modified,
                            evidence: vec![event_evidence(event)],
                            confidence: LinkConfidence::Exact,
                            record_step: true,
                        },
                    );
                }
                NormalizedIdentityKind::Erased => {
                    let Some(from) = node_for_token(
                        stage.before.as_ref(),
                        event.ptr_token,
                        stage_index,
                        &node_ids,
                    ) else {
                        continue;
                    };
                    used_before.insert(from);
                    push_relation(
                        &mut relations,
                        Relation {
                            stage_index,
                            from: Some(from),
                            to: None,
                            change: HistoryChange::Erased,
                            evidence: vec![event_evidence(event)],
                            confidence: LinkConfidence::Exact,
                            record_step: true,
                        },
                    );
                }
                NormalizedIdentityKind::Inserted => {
                    let Some(to) = node_for_token(
                        stage.after.as_ref(),
                        event.ptr_token,
                        stage_index,
                        &node_ids,
                    ) else {
                        continue;
                    };
                    inserted_after.insert(to);
                    used_after.insert(to);
                    push_relation(
                        &mut relations,
                        Relation {
                            stage_index,
                            from: None,
                            to: Some(to),
                            change: HistoryChange::Inserted,
                            evidence: vec![event_evidence(event)],
                            confidence: LinkConfidence::Exact,
                            record_step: true,
                        },
                    );
                }
            }
        }

        if let (Some(before), Some(after)) = (&stage.before, &stage.after) {
            for (&token, &before_idx) in &before.tokens {
                let Some(&after_idx) = after.tokens.get(&token) else {
                    continue;
                };
                let from = node_ids[&OccurrenceKey {
                    stage_index,
                    side: before.side,
                    op_idx: before_idx,
                }];
                let to = node_ids[&OccurrenceKey {
                    stage_index,
                    side: after.side,
                    op_idx: after_idx,
                }];
                if used_before.contains(&from) || inserted_after.contains(&to) {
                    continue;
                }
                used_before.insert(from);
                used_after.insert(to);
                push_relation(
                    &mut relations,
                    Relation {
                        stage_index,
                        from: Some(from),
                        to: Some(to),
                        change: changed(&nodes[from].operation, &nodes[to].operation),
                        evidence: Vec::new(),
                        confidence: LinkConfidence::Exact,
                        record_step: true,
                    },
                );
            }
            fallback_relations(
                &mut FallbackContext {
                    function,
                    nodes: &nodes,
                    node_ids: &node_ids,
                    relations: &mut relations,
                },
                stage_index,
                before,
                after,
                &mut used_before,
                &mut used_after,
            );
        }
    }

    for right_index in 1..stages.len() {
        bridge_stages(
            function,
            right_index - 1,
            right_index,
            stages,
            &node_ids,
            &mut relations,
        );
    }

    let mut union_find = UnionFind::new(nodes.len());
    for relation in &relations {
        if let (Some(from), Some(to)) = (relation.from, relation.to) {
            union_find.union(from, to);
        }
    }
    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for id in 0..nodes.len() {
        components.entry(union_find.find(id)).or_default().push(id);
    }

    let mut resolved = ResolvedFunction {
        function: function.to_string(),
        ..ResolvedFunction::default()
    };
    for node_ids_in_component in components.values_mut() {
        node_ids_in_component.sort_by_key(|&id| {
            let node = &nodes[id];
            (node.key.stage_index, node.key.side, node.function_ordinal)
        });
        let anchor = &nodes[node_ids_in_component[0]];
        let uid = OpUid::from_anchor(&OpAnchor {
            function: function.to_string(),
            pass_id: anchor.pass_id,
            side: anchor.key.side,
            function_ordinal: anchor.function_ordinal,
        });
        for &id in node_ids_in_component.iter() {
            let node = &nodes[id];
            resolved.selectable.insert(
                node.key,
                SelectableOp {
                    uid: uid.clone(),
                    op_idx: node.operation.idx,
                    name: node.operation.name.clone(),
                    line_start: node.operation.line_start,
                    line_end: node.operation.line_end,
                },
            );
        }
        let component_set: std::collections::HashSet<_> =
            node_ids_in_component.iter().copied().collect();
        let mut component_relations: Vec<_> = relations
            .iter()
            .filter(|relation| {
                relation.record_step
                    && relation
                        .from
                        .or(relation.to)
                        .is_some_and(|id| component_set.contains(&id))
            })
            .collect();
        component_relations.sort_by_key(|relation| {
            let ordinal = relation
                .from
                .or(relation.to)
                .map(|id| nodes[id].function_ordinal)
                .unwrap_or(usize::MAX);
            (relation.stage_index, ordinal)
        });
        let steps = component_relations
            .into_iter()
            .map(|relation| {
                let stage = &stages[relation.stage_index];
                HistoryStep {
                    pass_id: stage.pass_id,
                    pass_name: stage.pass_name.clone(),
                    change: relation.change,
                    before: relation
                        .from
                        .map(|id| occurrence(&nodes[id].operation, SnapshotSide::Before)),
                    after: relation
                        .to
                        .map(|id| occurrence(&nodes[id].operation, SnapshotSide::After)),
                    evidence: relation.evidence.clone(),
                    confidence: relation.confidence,
                }
            })
            .collect();
        let first_name = nodes[node_ids_in_component[0]].operation.name.clone();
        let last_name = nodes[*node_ids_in_component.last().unwrap()]
            .operation
            .name
            .clone();
        resolved.histories.insert(
            uid.clone(),
            OpHistory {
                uid,
                first_name,
                last_name,
                steps,
            },
        );
    }
    resolved
}
