//! Export every trained character ("veteran") the account owns, as JSON.
//!
//! # Why in-process
//!
//! umadump (`D:\work\dreki\umadump`) reads the same data from *outside* the
//! game: pattern-scan for `MetadataRegistration`, parse `global-metadata.dat`,
//! then walk hardcoded ctypes offsets. It works, but every one of those steps
//! is a thing that breaks on a game update, and it means running a second
//! program with the game open.
//!
//! This plugin is already inside the process with the IL2CPP metadata APIs in
//! hand, so the whole chain resolves *by name* — no offsets, no metadata file,
//! no second program. What is ported from umadump is the part worth keeping:
//! the field selection and the output schema.
//!
//! # It carries account identifiers
//!
//! `viewer_id` and `owner_viewer_id` are in umadump's schema and so are in
//! ours. That makes `veterans.json` a file about the account, not just about
//! the characters — unlike an idle-career export, it is not something to hand
//! to someone else without reading it first.
//!
//! # The schema is the contract
//!
//! The types here serialise to exactly umadump's `trained_chara_data.json`, so
//! anything already reading that file reads this one. The keys are the game
//! server's own names for these fields, which is why they do not match the C#
//! field names the values are read from.
//!
//! A handful of keys are always `0`. They exist in the server's response but
//! not in the client's memory, and umadump emits them as zeros; dropping them
//! would silently change the shape of the file for every existing consumer.
//! They are marked where they are declared.
//!
//! # Where the work happens
//!
//! The walk has to run on the Unity main thread — it is reading live managed
//! objects. Serialising and writing do not, so they go to a background thread,
//! the same split [`crate::idle_export`] uses.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::compat::Sdk;

/// One trained character, in the server's own field names.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Veteran {
    pub viewer_id: i64,
    pub trained_chara_id: i32,
    pub owner_viewer_id: i64,
    pub owner_trained_chara_id: i32,
    /// Server-only; see the module header.
    pub single_mode_chara_id: i32,
    /// Server-only.
    pub chara_seed: i32,
    pub card_id: i32,
    /// Server-only.
    pub succession_trained_chara_id_1: i32,
    /// Server-only.
    pub succession_trained_chara_id_2: i32,
    pub use_type: i32,
    pub speed: i32,
    pub stamina: i32,
    pub power: i32,
    pub wiz: i32,
    pub guts: i32,
    pub fans: i32,
    pub rank_score: i32,
    pub rank: i32,
    pub scenario_id: i32,
    /// Server-only.
    pub route_id: i32,
    /// Server-only.
    pub arrive_route_race_id: i32,
    pub proper_ground_turf: i32,
    pub proper_ground_dirt: i32,
    pub proper_running_style_nige: i32,
    pub proper_running_style_senko: i32,
    pub proper_running_style_sashi: i32,
    pub proper_running_style_oikomi: i32,
    pub proper_distance_short: i32,
    pub proper_distance_mile: i32,
    pub proper_distance_middle: i32,
    pub proper_distance_long: i32,
    pub succession_num: i32,
    pub rarity: i32,
    /// `0`/`1`, not a bool — that is how the server sends it.
    pub is_saved: i32,
    /// `0`/`1`, as above.
    pub is_locked: i32,
    pub talent_level: i32,
    /// Server-only.
    pub race_cloth_id: i32,
    pub chara_grade: i32,
    pub running_style: i32,
    pub nickname_id: i32,
    pub wins: i32,
    /// `2026-03-14 23:42:18`, the game's own formatting.
    pub register_time: String,
    /// The same value as `register_time`; both keys are in the server response.
    pub create_time: String,
    pub skill_array: Vec<Skill>,
    pub support_card_list: Vec<SupportCard>,
    pub race_result_list: Vec<RaceResult>,
    pub win_saddle_id_array: Vec<i32>,
    pub nickname_id_array: Vec<i32>,
    pub factor_info_array: Vec<Factor>,
    pub factor_extend_array: Vec<FactorExtend>,
    pub succession_chara_array: Vec<SuccessionChara>,
    /// The favourite marker's icon, `0` when the veteran is not marked.
    pub icon_type: i32,
    /// The note attached to the favourite marker, `""` when there is none.
    pub memo: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Skill {
    pub skill_id: i32,
    pub level: i32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SupportCard {
    pub position: i32,
    pub support_card_id: i32,
    pub exp: i32,
    pub limit_break_count: i32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RaceResult {
    pub turn: i32,
    pub program_id: i32,
    pub weather: i32,
    pub ground_condition: i32,
    pub running_style: i32,
    /// Server-only.
    pub popularity: i32,
    pub result_rank: i32,
    /// Server-only.
    pub result_time: i32,
    /// Server-only.
    pub prize_money: i32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Factor {
    pub factor_id: i32,
    pub level: i32,
}

/// One step of a factor's upgrade history — which factor it became, and when.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FactorExtend {
    /// Which slot the factor sits in: [`SELF_POSITION`] for the veteran's own,
    /// otherwise the inherited character's `position_id`.
    pub position_id: i32,
    pub base_factor_id: i32,
    pub factor_id: i32,
    pub register_time: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SuccessionChara {
    pub position_id: i32,
    pub card_id: i32,
    pub rank: i32,
    pub rarity: i32,
    pub talent_level: i32,
    pub factor_info_array: Vec<Factor>,
    pub factor_extend_array: Vec<FactorExtend>,
    pub win_saddle_id_array: Vec<i32>,
    pub owner_viewer_id: i64,
}

/// `SuccessionCharaPosition.SELF` — the veteran's own factor slot, as opposed
/// to an inherited one.
pub const SELF_POSITION: i32 = 1;

/// What the game writes for a timestamp it does not have.
const NO_TIME: &str = "0000-00-00 00:00:00";

/// A Unix second as the game formats it, in JST.
///
/// JST because that is the clock the server's own `register_time` strings are
/// on, and a history entry that disagreed with the field next to it by nine
/// hours would be worse than useless. Pure, so the rule is testable.
#[must_use]
pub fn jst_timestamp(unix_secs: i64) -> String {
    use chrono::{DateTime, FixedOffset};
    /// UTC+9, with no daylight saving to worry about.
    const JST_OFFSET_SECS: i32 = 9 * 60 * 60;

    if unix_secs == 0 {
        return NO_TIME.to_owned();
    }
    let (Some(utc), Some(jst)) = (
        DateTime::from_timestamp(unix_secs, 0),
        FixedOffset::east_opt(JST_OFFSET_SECS),
    ) else {
        return NO_TIME.to_owned();
    };
    utc.with_timezone(&jst).format("%Y-%m-%d %H:%M:%S").to_string()
}

// ---------------------------------------------------------------------------
// Running an export
// ---------------------------------------------------------------------------

/// Set while an export is in flight, so a second click during the walk is
/// ignored rather than queueing another pass over a few hundred veterans.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// The user's profile directory, if Windows will say.
fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// Where the file goes. One name, rewritten each time: the export is a
/// snapshot of the account as it is now, and a folder of near-identical
/// snapshots would be a folder nobody reads.
#[must_use]
pub fn output_path() -> PathBuf {
    home().map_or_else(
        || PathBuf::from("veterans.json"),
        |home| honse_career_meta::veterans_file(&home),
    )
}

/// Kick off an export. Safe to call from the menu (any thread): the read is
/// scheduled onto the Unity main thread, where the objects are live.
pub fn request() {
    schedule(export_cb);
}

/// Upload only on an explicit click; a missing key never schedules a read or request.
pub fn request_upload() {
    if upload_key().is_none() {
        Sdk::get().show_notification("Veteran upload: set uma-moe.api-key in your config first");
        return;
    }
    schedule(upload_cb);
}

fn upload_key() -> Option<String> {
    crate::config::read(|file| {
        let key = file.uma_moe.api_key.trim();
        (!key.is_empty()).then(|| key.to_owned())
    })
}

fn schedule(callback: extern "C" fn()) {
    if RUNNING.swap(true, Ordering::AcqRel) {
        hlog_info!(target: "training-tracker", "Veteran export already running");
        return;
    }
    if !Sdk::get().schedule_on_main_thread(callback) {
        // Nothing was queued, so nothing will ever clear the flag.
        RUNNING.store(false, Ordering::Release);
        hlog_warn!(target: "training-tracker", "Veteran export: could not reach the main thread");
    }
}

/// Unity main thread: read everything, then hand the writing off.
extern "C" fn export_cb() {
    let veterans = crate::memory_reader::read_veterans();
    std::thread::spawn(move || {
        finish(veterans);
        RUNNING.store(false, Ordering::Release);
    });
}

extern "C" fn upload_cb() {
    let key = upload_key();
    let veterans = crate::memory_reader::read_veterans();
    std::thread::spawn(move || {
        let result = key
            .ok_or_else(|| "set uma-moe.api-key in your config first".to_owned())
            .and_then(|key| upload(&veterans, &key));
        match result {
            Ok(()) => {
                Sdk::get().show_notification(&format!("Uploaded {} veterans to uma.moe", veterans.len()));
            }
            Err(message) => {
                hlog_warn!(target: "training-tracker", "Veteran upload: {message}");
                Sdk::get().show_notification(&format!("Veteran upload failed: {message}"));
            }
        }
        RUNNING.store(false, Ordering::Release);
    });
}

/// `viewer_id` identifies the roster's account, not a borrowed ancestor's owner.
fn upload_account_id(veterans: &[Veteran]) -> Result<i64, String> {
    let first = veterans.first().ok_or("no veterans found")?;
    let id = first.viewer_id;
    if id <= 0 || veterans.iter().any(|v| v.viewer_id != id) {
        return Err("cannot determine a consistent account ID; upload skipped".to_owned());
    }
    Ok(id)
}

fn upload(veterans: &[Veteran], key: &str) -> Result<(), String> {
    let account_id = upload_account_id(veterans)?;
    let body = serde_json::to_vec(veterans).map_err(|_| "cannot serialise veterans")?;
    send_upload("https://uma.moe/ingest/veteran", account_id, key, &body)
}

fn send_upload(url: &str, account_id: i64, key: &str, body: &[u8]) -> Result<(), String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(60))
        .redirects(0)
        .build();
    // X-API-Key: https://uma.moe/api/docs/openapi.yaml, ApiKeyAuth (checked 2026-09-06).
    let response = agent
        .post(url)
        .query("account_id", &account_id.to_string())
        .set("X-API-Key", key)
        .set("Accept", "application/json")
        .set("Content-Type", "application/json")
        .send_bytes(body);
    // Do not log response bodies or transport details: they may echo credentials.
    match response {
        Ok(response) if (200..300).contains(&response.status()) => Ok(()),
        Ok(response) => Err(format!("unexpected HTTP {}", response.status())),
        Err(ureq::Error::Status(code, _)) => Err(format!("uma.moe returned HTTP {code}")),
        Err(ureq::Error::Transport(_)) => Err("network request failed or timed out".to_owned()),
    }
}

/// Serialise and write, saying what happened either way. Off the game's thread.
fn finish(veterans: Vec<Veteran>) {
    let sdk = Sdk::get();
    if veterans.is_empty() {
        hlog_warn!(target: "training-tracker", "Veteran export: nothing to write (is the account loaded?)");
        sdk.show_notification("Veteran export: no veterans found");
        return;
    }
    let path = output_path();
    if let Err(e) = write_json(&path, &veterans) {
        hlog_error!(target: "training-tracker", "Veteran export: {e}");
        sdk.show_notification(&format!("Veteran export failed: {e}"));
        return;
    }
    hlog_info!(
        target: "training-tracker",
        "Veteran export: wrote {} veterans to {}",
        veterans.len(),
        path.display()
    );
    sdk.show_notification(&format!("Exported {} veterans", veterans.len()));
}

/// Write the array, via a temp file so a failure never leaves a half-file
/// where a consumer expects valid JSON.
fn write_json(path: &std::path::Path, veterans: &[Veteran]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(veterans).map_err(|e| format!("cannot serialise: {e}"))?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, &json).map_err(|e| format!("cannot write {}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{jst_timestamp, RaceResult, Skill, SuccessionChara, SupportCard, Veteran, NO_TIME};

    #[test]
    fn upload_posts_json_with_api_key_and_handles_http_failures() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        for status in [200, 302, 401, 500] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/ingest/veteran", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                let headers = String::from_utf8(request).unwrap().to_ascii_lowercase();
                assert!(headers.starts_with("post /ingest/veteran?account_id=123456789 http/1.1\r\n"));
                assert!(headers.contains("x-api-key: test-secret\r\n"));
                assert!(headers.contains("content-type: application/json\r\n"));
                assert!(!headers.contains("authorization:"));
                let mut body = [0; 2];
                stream.read_exact(&mut body).unwrap();
                assert_eq!(&body, b"[]");
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            });
            let result = super::send_upload(&url, 123456789, "test-secret", b"[]");
            server.join().unwrap();
            assert_eq!(result.is_ok(), status == 200);
            if let Err(error) = result {
                assert!(error.contains(&status.to_string()));
                assert!(!error.contains("test-secret"));
            }
        }
    }

    #[test]
    fn upload_requires_a_nonempty_roster_with_one_valid_account() {
        assert!(super::upload_account_id(&[]).is_err());
        assert!(super::upload_account_id(&[Veteran::default()]).is_err());
        let veteran = Veteran {
            viewer_id: 280420104509,
            ..Veteran::default()
        };
        assert_eq!(
            super::upload_account_id(&[veteran.clone(), veteran.clone()]).unwrap(),
            280420104509
        );
        let other = Veteran {
            viewer_id: 123456789,
            ..Veteran::default()
        };
        assert!(super::upload_account_id(&[veteran, other]).is_err());
        assert!(super::upload_account_id(&[Veteran {
            viewer_id: -1,
            ..Veteran::default()
        }])
        .is_err());
    }

    /// The key order of one entry of umadump's `trained_chara_data.json`.
    ///
    /// Copied from the real file rather than from its source, so this fails if
    /// a field here is renamed, reordered or dropped — which is the whole
    /// promise of the export: anything reading umadump's output reads ours.
    const UMADUMP_KEYS: &[&str] = &[
        "viewer_id",
        "trained_chara_id",
        "owner_viewer_id",
        "owner_trained_chara_id",
        "single_mode_chara_id",
        "chara_seed",
        "card_id",
        "succession_trained_chara_id_1",
        "succession_trained_chara_id_2",
        "use_type",
        "speed",
        "stamina",
        "power",
        "wiz",
        "guts",
        "fans",
        "rank_score",
        "rank",
        "scenario_id",
        "route_id",
        "arrive_route_race_id",
        "proper_ground_turf",
        "proper_ground_dirt",
        "proper_running_style_nige",
        "proper_running_style_senko",
        "proper_running_style_sashi",
        "proper_running_style_oikomi",
        "proper_distance_short",
        "proper_distance_mile",
        "proper_distance_middle",
        "proper_distance_long",
        "succession_num",
        "rarity",
        "is_saved",
        "is_locked",
        "talent_level",
        "race_cloth_id",
        "chara_grade",
        "running_style",
        "nickname_id",
        "wins",
        "register_time",
        "create_time",
        "skill_array",
        "support_card_list",
        "race_result_list",
        "win_saddle_id_array",
        "nickname_id_array",
        "factor_info_array",
        "factor_extend_array",
        "succession_chara_array",
        "icon_type",
        "memo",
    ];

    /// The keys `value` serialises to, sorted.
    ///
    /// Sorted, not in declaration order: `serde_json::Value` stores objects in
    /// a `BTreeMap`, so the order is gone by the time a test can look. The
    /// written file does keep declaration order — serde emits struct fields in
    /// the order they are declared — but what a consumer actually depends on
    /// is the set of keys, and that is what this compares.
    fn keys_of(value: &impl serde::Serialize) -> Vec<String> {
        let json = serde_json::to_value(value).expect("serialises");
        json.as_object().expect("an object").keys().cloned().collect()
    }

    /// `expected` sorted the same way [`keys_of`] returns its keys.
    fn sorted(expected: &[&str]) -> Vec<String> {
        let mut keys: Vec<String> = expected.iter().map(|k| (*k).to_owned()).collect();
        keys.sort_unstable();
        keys
    }

    #[test]
    fn a_veteran_serialises_to_umadumps_schema() {
        assert_eq!(keys_of(&Veteran::default()), sorted(UMADUMP_KEYS));
    }

    #[test]
    fn the_nested_records_match_too() {
        assert_eq!(keys_of(&Skill::default()), sorted(&["skill_id", "level"]));
        assert_eq!(
            keys_of(&SupportCard::default()),
            sorted(&["position", "support_card_id", "exp", "limit_break_count"])
        );
        assert_eq!(
            keys_of(&RaceResult::default()),
            sorted(&[
                "turn",
                "program_id",
                "weather",
                "ground_condition",
                "running_style",
                "popularity",
                "result_rank",
                "result_time",
                "prize_money",
            ])
        );
        assert_eq!(
            keys_of(&SuccessionChara::default()),
            sorted(&[
                "position_id",
                "card_id",
                "rank",
                "rarity",
                "talent_level",
                "factor_info_array",
                "factor_extend_array",
                "win_saddle_id_array",
                "owner_viewer_id",
            ])
        );
    }

    #[test]
    fn a_timestamp_formats_the_way_the_game_writes_them() {
        // 2026-03-14 14:42:18 UTC is 23:42:18 the same day in JST.
        assert_eq!(jst_timestamp(1_773_499_338), "2026-03-14 23:42:18");
    }

    #[test]
    fn no_timestamp_means_the_placeholder_not_the_epoch() {
        assert_eq!(jst_timestamp(0), NO_TIME);
    }
}
