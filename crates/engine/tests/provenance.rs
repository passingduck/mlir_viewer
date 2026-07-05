use engine::{OpAnchor, OpUid, SnapshotSide};

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
