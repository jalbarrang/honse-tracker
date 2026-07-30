//! State-service tests: deterministic view-model transitions driven by
//! committed-ingest events and explicit clocks — no WebView involved.

mod common;

use common::make_turn;
use honse_dashboard::state::{
    delivery_state, size_label, Delivery, Phase, StateService, ThemeMode, CONNECTED_WITHIN_MS, STALE_WITHIN_MS,
};
use honse_dashboard::{storage, AppEvent};

#[test]
fn empty_database_projects_empty_offline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");
    let mut service = StateService::new(db);

    let vm = service.snapshot(1_000_000);
    assert_eq!(vm.phase, Phase::Empty);
    assert_eq!(vm.delivery, Delivery::Offline);
    assert_eq!(vm.totals.captures, 0);
    assert!(vm.history.is_empty());
}

#[test]
fn committed_event_transitions_to_ready_connected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");
    let mut service = StateService::new(db.clone());
    let now = 10_000_000u64;

    db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 31, now - 2_000))
        .expect("insert");
    service.handle_event(&AppEvent::TurnCommitted {
        career_id: 1,
        turn: 31,
        capture_id: "cap-1".to_string(),
        captured_at_ms: now - 2_000,
    });

    let vm = service.snapshot(now);
    assert_eq!(vm.delivery, Delivery::Connected);
    let Phase::Ready(ov) = vm.phase else {
        panic!("expected ready, got {:?}", vm.phase);
    };
    assert_eq!(ov.turn, 31);
    assert_eq!(ov.card_id, 1001);
    assert_eq!(ov.stats[0].value, 531, "speed = 500 + turn");
    assert_eq!(ov.skill_points, 420);
    assert_eq!(ov.motivation_label, "Great");
    assert_eq!(ov.options.len(), 5);
    // Speed has the top gains and a rainbow-ready partner: it must rank first.
    assert_eq!(ov.options[0].facility_name, "Speed");
    assert_eq!(ov.options[0].value, 100);
    assert!(ov.options[0].partners[0].hot);
    // Ranking is monotonically decreasing in value.
    assert!(ov.options.windows(2).all(|w| w[0].value >= w[1].value));
    assert_eq!(vm.history.len(), 1);
}

#[test]
fn freshness_decays_from_connected_to_stale_to_offline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");
    let mut service = StateService::new(db.clone());
    let base = 100_000_000u64;

    db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 31, base))
        .expect("insert");

    assert_eq!(service.snapshot(base + 1_000).delivery, Delivery::Connected);
    assert_eq!(
        service.snapshot(base + CONNECTED_WITHIN_MS + 1).delivery,
        Delivery::Stale
    );
    assert_eq!(service.snapshot(base + STALE_WITHIN_MS + 1).delivery, Delivery::Offline);
    // The career itself remains visible even while offline.
    assert!(matches!(
        service.snapshot(base + STALE_WITHIN_MS + 1).phase,
        Phase::Ready(_)
    ));
}

#[test]
fn freshness_is_seeded_from_disk_on_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");
    let base = 200_000_000u64;
    db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 31, base))
        .expect("insert");

    // Fresh service (no events seen) simulating a restart.
    let mut service = StateService::new(db);
    assert_eq!(service.snapshot(base + 5_000).delivery, Delivery::Connected);
}

#[test]
fn duplicate_events_only_bump_the_counter() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");
    let mut service = StateService::new(db);

    service.handle_event(&AppEvent::DuplicateDiscarded {
        capture_id: "cap-1".to_string(),
    });
    service.handle_event(&AppEvent::DuplicateDiscarded {
        capture_id: "cap-1".to_string(),
    });
    let vm = service.snapshot(1_000);
    assert_eq!(vm.duplicates_discarded, 2);
    assert_eq!(vm.phase, Phase::Empty, "duplicates alone never fabricate data");
}

#[test]
fn deltas_come_from_previous_turn_revision() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");
    let mut service = StateService::new(db.clone());

    db.insert_settled_turn(&make_turn("cap-1", 1001, 5, 30, 1_000))
        .expect("t30");
    db.insert_settled_turn(&make_turn("cap-2", 1001, 5, 31, 2_000))
        .expect("t31");

    let vm = service.snapshot(3_000);
    let Phase::Ready(ov) = vm.phase else { panic!("ready") };
    // make_turn stats are 500+turn etc: every stat moved by exactly +1.
    assert!(ov.stats.iter().all(|s| s.delta == 1), "deltas vs previous turn");
    assert_eq!(ov.recent_turns.len(), 2);
    assert!(ov.recent_turns[0].is_latest);
    assert_eq!(ov.recent_turns[0].summary, "+5 total stats");
}

#[test]
fn delivery_state_boundaries_are_exact() {
    assert_eq!(delivery_state(1_000, None), Delivery::Offline);
    assert_eq!(delivery_state(CONNECTED_WITHIN_MS, Some(0)), Delivery::Connected);
    assert_eq!(delivery_state(CONNECTED_WITHIN_MS + 1, Some(0)), Delivery::Stale);
    assert_eq!(delivery_state(STALE_WITHIN_MS, Some(0)), Delivery::Stale);
    assert_eq!(delivery_state(STALE_WITHIN_MS + 1, Some(0)), Delivery::Offline);
    // Clock skew (capture in the future) still reads as connected.
    assert_eq!(delivery_state(0, Some(10_000)), Delivery::Connected);
}

#[test]
fn theme_mode_persists_and_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open");

    // Default: nothing persisted → System.
    assert_eq!(
        ThemeMode::parse(db.get_setting(ThemeMode::SETTING_KEY).expect("get").as_deref()),
        ThemeMode::System
    );

    db.set_setting(ThemeMode::SETTING_KEY, ThemeMode::Dark.as_str())
        .expect("set");
    let restored = ThemeMode::parse(db.get_setting(ThemeMode::SETTING_KEY).expect("get").as_deref());
    assert_eq!(restored, ThemeMode::Dark);

    // Resolution rules: overrides win, System follows the OS.
    assert_eq!(ThemeMode::System.resolve(true), "dark");
    assert_eq!(ThemeMode::System.resolve(false), "light");
    assert_eq!(ThemeMode::Light.resolve(true), "light");
    assert_eq!(ThemeMode::Dark.resolve(false), "dark");
    // Unknown persisted garbage degrades to System.
    assert_eq!(ThemeMode::parse(Some("plaid")), ThemeMode::System);
}

#[test]
fn size_labels_are_human() {
    assert_eq!(size_label(512), "512 B");
    assert_eq!(size_label(4 * 1024), "4 KB");
    assert_eq!(size_label(4_404_019), "4.2 MB");
}
