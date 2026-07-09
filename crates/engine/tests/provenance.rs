use std::collections::HashMap;

use engine::{
    parse_module, resolve_function, EvidenceSource, HistoryChange, LinkConfidence,
    NormalizedIdentityEvent, NormalizedIdentityKind, OccurrenceKey, OpAnchor, OpHistory, OpUid,
    ResolvedFunction, SnapshotOps, SnapshotSide, TimelineStage,
};

#[test]
fn uid_round_trips_function_punctuation() {
    let anchor = OpAnchor {
        function: "dialect/f-with.dash".into(),
        pass_id: 42,
        side: SnapshotSide::After,
        function_ordinal: 7,
    };
    let uid = OpUid::from_anchor(&anchor);

    assert_eq!(uid.parse_anchor().unwrap(), anchor);
    assert!(!uid.as_str().contains('/'));
}

#[test]
fn uid_rejects_unknown_version_and_bad_base64() {
    assert!(OpUid::parse("op2.Zg.1.a.0").is_err());
    assert!(OpUid::parse("op1.!.1.a.0").is_err());
}

fn snapshot(side: SnapshotSide, blob_id: i64, text: &str, tokens: &[(&str, i64)]) -> SnapshotOps {
    let module = parse_module(text);
    let scope = module.scope("f").unwrap();
    let function_ordinals = scope
        .ops
        .iter()
        .copied()
        .enumerate()
        .map(|(ordinal, op_idx)| (op_idx, ordinal))
        .collect();
    let tokens = tokens
        .iter()
        .map(|(name, token)| {
            let op_idx = scope
                .ops
                .iter()
                .copied()
                .find(|&idx| module.ops[idx].name == *name)
                .unwrap();
            (*token, op_idx)
        })
        .collect();
    SnapshotOps {
        side,
        blob_id: Some(blob_id),
        module,
        function_ordinals,
        tokens,
    }
}

fn event(
    kind: NormalizedIdentityKind,
    ptr_token: i64,
    new_token: Option<i64>,
    pattern: Option<&str>,
    seq: i64,
) -> NormalizedIdentityEvent {
    NormalizedIdentityEvent {
        kind,
        ptr_token,
        new_token,
        pattern: pattern.map(str::to_string),
        source: EvidenceSource::Action,
        seq,
    }
}

fn op_idx(snapshot: &SnapshotOps, name: &str) -> usize {
    snapshot
        .module
        .scope("f")
        .unwrap()
        .ops
        .iter()
        .copied()
        .find(|&idx| snapshot.module.ops[idx].name == name)
        .unwrap()
}

fn history_at(
    resolved: &ResolvedFunction,
    stage_index: usize,
    side: SnapshotSide,
    op_idx: usize,
) -> &OpHistory {
    let uid = &resolved
        .selectable
        .get(&OccurrenceKey {
            stage_index,
            side,
            op_idx,
        })
        .unwrap()
        .uid;
    resolved.histories.get(uid).unwrap()
}

#[test]
fn exact_replace_modify_erase_and_insert_form_expected_steps() {
    let initial = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.addi %arg0, %arg0 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n";
    let replaced = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n";
    let modified = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 {fast} : i32\n  %1 = arith.muli %0, %0 : i32\n  return %1 : i32\n}\n";
    let erased = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 {fast} : i32\n  return %0 : i32\n}\n";
    let inserted = "func.func @f(%arg0: i32) -> i32 {\n  %c1 = arith.constant 1 : i32\n  %0 = arith.shli %arg0, %arg0 {fast} : i32\n  return %0 : i32\n}\n";

    let stages = vec![
        TimelineStage {
            pass_id: 1,
            pass_name: "canonicalize".into(),
            before: Some(snapshot(
                SnapshotSide::Before,
                10,
                initial,
                &[("arith.addi", 1), ("arith.muli", 3)],
            )),
            after: Some(snapshot(
                SnapshotSide::After,
                11,
                replaced,
                &[("arith.shli", 2), ("arith.muli", 3)],
            )),
            events: vec![event(
                NormalizedIdentityKind::Replaced,
                1,
                Some(2),
                Some("AddIToShift"),
                0,
            )],
        },
        TimelineStage {
            pass_id: 2,
            pass_name: "set-attr".into(),
            before: Some(snapshot(
                SnapshotSide::Before,
                11,
                replaced,
                &[("arith.shli", 2), ("arith.muli", 3)],
            )),
            after: Some(snapshot(
                SnapshotSide::After,
                12,
                modified,
                &[("arith.shli", 2), ("arith.muli", 3)],
            )),
            events: vec![event(
                NormalizedIdentityKind::Modified,
                2,
                None,
                Some("SetFastAttr"),
                0,
            )],
        },
        TimelineStage {
            pass_id: 3,
            pass_name: "dce".into(),
            before: Some(snapshot(
                SnapshotSide::Before,
                12,
                modified,
                &[("arith.shli", 2), ("arith.muli", 3)],
            )),
            after: Some(snapshot(
                SnapshotSide::After,
                13,
                erased,
                &[("arith.shli", 2)],
            )),
            events: vec![event(NormalizedIdentityKind::Erased, 3, None, None, 0)],
        },
        TimelineStage {
            pass_id: 4,
            pass_name: "materialize".into(),
            before: Some(snapshot(
                SnapshotSide::Before,
                13,
                erased,
                &[("arith.shli", 2)],
            )),
            after: Some(snapshot(
                SnapshotSide::After,
                14,
                inserted,
                &[("arith.constant", 4), ("arith.shli", 2)],
            )),
            events: vec![event(
                NormalizedIdentityKind::Inserted,
                4,
                None,
                Some("MaterializeOne"),
                0,
            )],
        },
    ];

    let addi_idx = op_idx(stages[0].before.as_ref().unwrap(), "arith.addi");
    let addi_history = history_at(
        &resolve_function("f", &stages),
        0,
        SnapshotSide::Before,
        addi_idx,
    )
    .clone();
    assert!(addi_history.steps.iter().any(|step| {
        step.change == HistoryChange::Replaced
            && step.confidence == LinkConfidence::Exact
            && step.evidence[0].pattern.as_deref() == Some("AddIToShift")
    }));
    assert!(addi_history
        .steps
        .iter()
        .any(|step| step.change == HistoryChange::Modified));

    let resolved = resolve_function("f", &stages);
    let muli_idx = op_idx(stages[2].before.as_ref().unwrap(), "arith.muli");
    assert!(history_at(&resolved, 2, SnapshotSide::Before, muli_idx)
        .steps
        .iter()
        .any(|step| step.change == HistoryChange::Erased));
    let constant_idx = op_idx(stages[3].after.as_ref().unwrap(), "arith.constant");
    assert!(history_at(&resolved, 3, SnapshotSide::After, constant_idx)
        .steps
        .iter()
        .any(|step| step.change == HistoryChange::Inserted));
}

#[test]
fn missing_events_use_inferred_score_and_never_cross_names() {
    let before = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.addi %arg0, %arg0 : i32\n  return %0 : i32\n}\n";
    let after = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.addi %arg0, %arg0 {fast} : i32\n  return %0 : i32\n}\n";
    let different = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.muli %arg0, %arg0 : i32\n  return %0 : i32\n}\n";
    let stages = vec![
        TimelineStage {
            pass_id: 1,
            pass_name: "unknown".into(),
            before: Some(snapshot(SnapshotSide::Before, 1, before, &[])),
            after: Some(snapshot(SnapshotSide::After, 2, after, &[])),
            events: vec![],
        },
        TimelineStage {
            pass_id: 2,
            pass_name: "rename".into(),
            before: Some(snapshot(SnapshotSide::Before, 2, after, &[])),
            after: Some(snapshot(SnapshotSide::After, 3, different, &[])),
            events: vec![],
        },
    ];
    let resolved = resolve_function("f", &stages);
    let addi_idx = op_idx(stages[0].before.as_ref().unwrap(), "arith.addi");
    let history = history_at(&resolved, 0, SnapshotSide::Before, addi_idx);
    assert!(history
        .steps
        .iter()
        .any(|step| { step.confidence == LinkConfidence::Inferred { score: 90 } }));
    let muli_idx = op_idx(stages[1].after.as_ref().unwrap(), "arith.muli");
    let addi_uid = &resolved
        .selectable
        .get(&OccurrenceKey {
            stage_index: 0,
            side: SnapshotSide::Before,
            op_idx: addi_idx,
        })
        .unwrap()
        .uid;
    let muli_uid = &resolved
        .selectable
        .get(&OccurrenceKey {
            stage_index: 1,
            side: SnapshotSide::After,
            op_idx: muli_idx,
        })
        .unwrap()
        .uid;
    assert_ne!(addi_uid, muli_uid);
}

#[test]
fn unlinked_removed_op_gets_terminal_disappeared_step() {
    // Stage 1: before has an op that vanishes; after contains only an op with
    // a different name so the fingerprint matcher cannot link them and no
    // identity events exist.
    let stages = vec![TimelineStage {
        pass_id: 1,
        pass_name: "canonicalize".into(),
        before: Some(snapshot(
            SnapshotSide::Before,
            1,
            "func.func @f() {\n  \"x.vanish\"() : () -> ()\n}\n",
            &[],
        )),
        after: Some(snapshot(
            SnapshotSide::After,
            2,
            "func.func @f() {\n  \"y.other\"() : () -> ()\n}\n",
            &[],
        )),
        events: Vec::new(),
    }];
    let resolved = resolve_function("f", &stages);
    let history = resolved
        .histories
        .values()
        .find(|h| h.first_name == "x.vanish")
        .expect("vanished op has a history");
    assert_eq!(history.steps.len(), 1);
    assert_eq!(history.steps[0].change, HistoryChange::Disappeared);
    assert!(history.steps[0].before.is_some());
    assert!(history.steps[0].after.is_none());
    assert_eq!(history.steps[0].evidence, vec![]);

    // The unlinked op that only exists in the after snapshot must read as an
    // insertion, not an inverted disappearance.
    let appeared = resolved
        .histories
        .values()
        .find(|h| h.first_name == "y.other")
        .expect("appeared op has a history");
    assert_eq!(appeared.steps.len(), 1);
    assert_eq!(appeared.steps[0].change, HistoryChange::Inserted);
    assert!(appeared.steps[0].before.is_none());
    let after_occurrence = appeared.steps[0].after.as_ref().expect("after occurrence");
    assert_eq!(after_occurrence.side, SnapshotSide::After);
    assert_eq!(appeared.steps[0].evidence, vec![]);
}

fn merge_stages(conflict: bool) -> Vec<TimelineStage> {
    let before = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.addi %arg0, %arg0 : i32\n  %1 = arith.addi %0, %arg0 : i32\n  return %1 : i32\n}\n";
    let after = if conflict {
        "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 : i32\n  %1 = arith.subi %arg0, %arg0 : i32\n  return %0 : i32\n}\n"
    } else {
        "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.shli %arg0, %arg0 : i32\n  return %0 : i32\n}\n"
    };
    let mut before_snapshot = snapshot(SnapshotSide::Before, 1, before, &[]);
    let addis: Vec<_> = before_snapshot
        .module
        .scope("f")
        .unwrap()
        .ops
        .iter()
        .copied()
        .filter(|&idx| before_snapshot.module.ops[idx].name == "arith.addi")
        .collect();
    before_snapshot.tokens = HashMap::from([(10, addis[0]), (11, addis[1])]);
    let mut after_snapshot = snapshot(SnapshotSide::After, 2, after, &[]);
    let shli = op_idx(&after_snapshot, "arith.shli");
    after_snapshot.tokens.insert(20, shli);
    if conflict {
        let subi = op_idx(&after_snapshot, "arith.subi");
        after_snapshot.tokens.insert(21, subi);
    }
    let events = if conflict {
        vec![
            event(NormalizedIdentityKind::Replaced, 10, Some(20), None, 2),
            event(NormalizedIdentityKind::Replaced, 10, Some(21), None, 3),
        ]
    } else {
        vec![
            event(NormalizedIdentityKind::Replaced, 10, Some(20), None, 0),
            event(NormalizedIdentityKind::Replaced, 11, Some(20), None, 1),
        ]
    };
    vec![TimelineStage {
        pass_id: 7,
        pass_name: "cse".into(),
        before: Some(before_snapshot),
        after: Some(after_snapshot),
        events,
    }]
}

#[test]
fn two_predecessors_replaced_by_one_token_share_uid_and_merge_steps() {
    let stages = merge_stages(false);
    let resolved = resolve_function("f", &stages);
    let before = stages[0].before.as_ref().unwrap();
    let addis: Vec<_> = before
        .module
        .scope("f")
        .unwrap()
        .ops
        .iter()
        .copied()
        .filter(|&idx| before.module.ops[idx].name == "arith.addi")
        .collect();
    let after_idx = op_idx(stages[0].after.as_ref().unwrap(), "arith.shli");
    let uids: Vec<_> = addis
        .iter()
        .map(|&idx| {
            &resolved
                .selectable
                .get(&OccurrenceKey {
                    stage_index: 0,
                    side: SnapshotSide::Before,
                    op_idx: idx,
                })
                .unwrap()
                .uid
        })
        .collect();
    let after_uid = &resolved
        .selectable
        .get(&OccurrenceKey {
            stage_index: 0,
            side: SnapshotSide::After,
            op_idx: after_idx,
        })
        .unwrap()
        .uid;
    assert_eq!(uids[0], uids[1]);
    assert_eq!(uids[0], after_uid);
    let history = resolved.histories.get(after_uid).unwrap();
    assert_eq!(
        history
            .steps
            .iter()
            .filter(|step| step.change == HistoryChange::Replaced)
            .count(),
        2
    );
}

#[test]
fn duplicate_old_successors_choose_lowest_event_sequence() {
    let stages = merge_stages(true);
    let resolved = resolve_function("f", &stages);
    let before_idx = stages[0].before.as_ref().unwrap().tokens[&10];
    let after = stages[0].after.as_ref().unwrap();
    let old_uid = &resolved
        .selectable
        .get(&OccurrenceKey {
            stage_index: 0,
            side: SnapshotSide::Before,
            op_idx: before_idx,
        })
        .unwrap()
        .uid;
    let chosen_uid = &resolved
        .selectable
        .get(&OccurrenceKey {
            stage_index: 0,
            side: SnapshotSide::After,
            op_idx: after.tokens[&20],
        })
        .unwrap()
        .uid;
    let ignored_uid = &resolved
        .selectable
        .get(&OccurrenceKey {
            stage_index: 0,
            side: SnapshotSide::After,
            op_idx: after.tokens[&21],
        })
        .unwrap()
        .uid;
    assert_eq!(old_uid, chosen_uid);
    assert_ne!(old_uid, ignored_uid);
}

#[test]
fn same_pointer_token_without_event_does_not_link_different_op_names() {
    let before = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.addi %arg0, %arg0 : i32\n  return %0 : i32\n}\n";
    let after = "func.func @f(%arg0: i32) -> i32 {\n  %0 = arith.muli %arg0, %arg0 : i32\n  return %0 : i32\n}\n";
    let stages = vec![TimelineStage {
        pass_id: 1,
        pass_name: "pointer-reuse".into(),
        before: Some(snapshot(
            SnapshotSide::Before,
            1,
            before,
            &[("arith.addi", 99)],
        )),
        after: Some(snapshot(
            SnapshotSide::After,
            2,
            after,
            &[("arith.muli", 99)],
        )),
        events: Vec::new(),
    }];
    let resolved = resolve_function("f", &stages);
    let before_idx = op_idx(stages[0].before.as_ref().unwrap(), "arith.addi");
    let after_idx = op_idx(stages[0].after.as_ref().unwrap(), "arith.muli");
    let before_uid = &resolved.selectable[&OccurrenceKey {
        stage_index: 0,
        side: SnapshotSide::Before,
        op_idx: before_idx,
    }]
        .uid;
    let after_uid = &resolved.selectable[&OccurrenceKey {
        stage_index: 0,
        side: SnapshotSide::After,
        op_idx: after_idx,
    }]
        .uid;

    assert_ne!(before_uid, after_uid);
}
