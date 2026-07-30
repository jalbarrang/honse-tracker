//! Storage integration tests: idempotency, career grouping, revisions,
//! persistence across reopen, settings, and maintenance operations.

mod common;

use common::make_turn;
use honse_dashboard::storage::{self, HistoryQuery, InsertOutcome};

#[test]
fn insert_then_duplicate_keeps_one_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");

    let turn = make_turn("cap-1", 1001, 5, 10, 1_000);
    let first = db.insert_settled_turn(&turn).expect("insert");
    let InsertOutcome::Committed {
        career_id,
        turn: turn_no,
    } = first
    else {
        panic!("expected committed, got {first:?}");
    };
    assert_eq!(turn_no, 10);

    let second = db.insert_settled_turn(&turn).expect("insert duplicate");
    assert_eq!(
        second,
        InsertOutcome::Duplicate {
            capture_id: "cap-1".to_string()
        }
    );

    let totals = db.totals().expect("totals");
    assert_eq!(totals.captures, 1, "duplicate must not add a row");
    assert_eq!(totals.careers, 1);
    let turns = db.turns_for_career(career_id).expect("turns");
    assert_eq!(turns.len(), 1);
}

#[test]
fn same_identity_and_forward_turns_stay_one_career() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");

    let a = db
        .insert_settled_turn(&make_turn("cap-1", 1001, 5, 10, 1_000))
        .expect("a");
    let b = db
        .insert_settled_turn(&make_turn("cap-2", 1001, 5, 11, 2_000))
        .expect("b");
    let (InsertOutcome::Committed { career_id: ca, .. }, InsertOutcome::Committed { career_id: cb, .. }) = (a, b)
    else {
        panic!("both must commit");
    };
    assert_eq!(ca, cb, "forward turn with same identity continues the run");

    let history = db.career_history(HistoryQuery::default()).expect("history");
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].turns_stored, 2);
    assert_eq!(history[0].latest_turn, 11);
    assert_eq!(history[0].started_at_ms, 1_000);
    assert_eq!(history[0].ended_at_ms, 2_000);
}

#[test]
fn turn_rewind_starts_a_new_career() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");

    db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 30, 1_000))
        .expect("a");
    let rewound = db
        .insert_settled_turn(&make_turn("cap-2", 1001, 5, 2, 2_000))
        .expect("b");
    let InsertOutcome::Committed { career_id, .. } = rewound else {
        panic!("must commit");
    };

    let history = db.career_history(HistoryQuery::default()).expect("history");
    assert_eq!(history.len(), 2, "rewind starts a new run");
    assert_eq!(history[0].career_id, career_id, "newest run first");
    assert_eq!(history[0].latest_turn, 2);
    assert_eq!(history[1].latest_turn, 30);
}

#[test]
fn identity_change_starts_a_new_career() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");

    db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 10, 1_000))
        .expect("a");
    db.insert_settled_turn(&make_turn("cap-2", 2002, 5, 11, 2_000))
        .expect("b: card change");
    db.insert_settled_turn(&make_turn("cap-3", 2002, 9, 12, 3_000))
        .expect("c: scenario change");

    let history = db.career_history(HistoryQuery::default()).expect("history");
    assert_eq!(history.len(), 3);
}

#[test]
fn same_turn_revisions_default_to_latest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");

    let mut rev1 = make_turn("cap-1", 1001, 5, 10, 1_000);
    rev1.snapshot.as_mut().expect("snap").speed = 100;
    let mut rev2 = make_turn("cap-2", 1001, 5, 10, 2_000);
    rev2.snapshot.as_mut().expect("snap").speed = 200;

    let a = db.insert_settled_turn(&rev1).expect("rev1");
    db.insert_settled_turn(&rev2).expect("rev2");
    let InsertOutcome::Committed { career_id, .. } = a else {
        panic!("must commit")
    };

    let turns = db.turns_for_career(career_id).expect("turns");
    assert_eq!(turns.len(), 1, "one visible row per turn");
    assert_eq!(turns[0].capture_id, "cap-2", "latest revision wins");
    assert_eq!(turns[0].payload.snapshot.as_ref().expect("snap").speed, 200);

    let history = db.career_history(HistoryQuery::default()).expect("history");
    assert_eq!(history[0].captures_stored, 2, "both revisions stay stored");
    assert_eq!(history[0].turns_stored, 1);
}

#[test]
fn data_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("t.db");
    {
        let db = storage::open(&path).expect("open");
        db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 10, 1_000))
            .expect("insert");
        db.set_setting("theme.mode", "dark").expect("setting");
    }
    // First handle (and its worker) intentionally dropped: fresh process view.
    let db = storage::open(&path).expect("reopen");
    let current = db
        .current_career()
        .expect("current")
        .expect("career exists after reopen");
    assert_eq!(current.latest.capture_id, "cap-1");
    assert_eq!(current.summary.card_id, 1001);
    assert_eq!(db.get_setting("theme.mode").expect("get"), Some("dark".to_string()));

    let dup = db
        .insert_settled_turn(&make_turn("cap-1", 1001, 5, 10, 1_000))
        .expect("dup");
    assert!(
        matches!(dup, InsertOutcome::Duplicate { .. }),
        "idempotency survives reopen"
    );
}

#[test]
fn rejects_capture_without_snapshot_or_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");

    let mut no_snap = make_turn("cap-1", 1001, 5, 10, 1_000);
    no_snap.snapshot = None;
    assert!(db.insert_settled_turn(&no_snap).is_err());

    let no_id = make_turn("", 1001, 5, 10, 1_000);
    assert!(db.insert_settled_turn(&no_id).is_err());

    assert_eq!(db.totals().expect("totals").captures, 0);
}

#[test]
fn maintenance_operations_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");
    db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 10, 1_000))
        .expect("insert");

    assert_eq!(db.integrity_check().expect("check"), "ok");
    db.compact().expect("compact");

    let backup = dir.path().join("backup.db");
    db.backup_to(&backup).expect("backup");
    let restored = storage::open(&backup).expect("open backup");
    assert_eq!(restored.totals().expect("totals").captures, 1);

    assert!(db.totals().expect("totals").db_size_bytes > 0);
}
