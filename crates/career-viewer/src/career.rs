//! Reading the exported career files: what is on disk, and what one contains.
//!
//! The export is a reflection dump of a server response, so it is read as
//! untyped JSON rather than modelled with structs. Naming every field would
//! mean this crate breaking whenever the game adds one — and the whole point of
//! dumping the response was to keep the parts nobody has decoded yet.
//!
//! What is modelled here is only what a page actually renders, each field
//! looked up defensively so a payload that has moved on renders a dash instead
//! of failing.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// One file in the careers directory, as the index page needs it.
pub struct Entry {
    /// The file name, which is also the URL segment.
    pub file: String,
    /// `2026-09-01 22:07`, parsed out of the file name.
    pub when: String,
    pub card_id: i64,
    pub trainee: Option<String>,
    pub chara_grade: i64,
    /// Speed, Stamina, Power, Guts, Wit — the panel's order.
    pub stats: [i64; 5],
}

/// One career, as the detail page needs it.
pub struct Career {
    pub file: String,
    pub when: String,
    pub source: String,
    pub plugin_version: String,
    pub card_id: i64,
    /// From umdb when it is loaded; `None` leaves the page showing the id.
    pub trainee: Option<String>,
    pub chara_grade: i64,
    pub stats: [i64; 5],
    pub skill_points: i64,
    pub races: Vec<Race>,
    pub supports: Vec<Support>,
    pub factors: Vec<FactorYear>,
    pub conditions: Vec<Condition>,
    pub skills: Vec<Skill>,
}

pub struct Race {
    pub turn: i64,
    pub year: &'static str,
    pub date: String,
    pub rank: i64,
    pub program_id: i64,
    pub ground: &'static str,
    pub weather: &'static str,
    pub style: &'static str,
    pub fans: i64,
}

pub struct Support {
    pub card_id: i64,
    pub name: Option<String>,
    /// Delta per main stat, in panel order. Negative for a loss.
    pub gains: [i64; 5],
}

pub struct Skill {
    pub id: i64,
    pub level: i64,
    pub name: Option<String>,
    pub icon_id: Option<i64>,
}

pub struct FactorYear {
    pub year: i64,
    pub factors: Vec<Factor>,
}

pub struct Factor {
    pub id: i64,
    pub level: i64,
}

pub struct Condition {
    pub id: i32,
    pub name: String,
    pub good: bool,
    pub active: bool,
}

/// The five main stats, in the order the overlay uses them.
pub const STAT_KEYS: [&str; 5] = ["speed", "stamina", "power", "guts", "wiz"];
pub const STAT_LABELS: [&str; 5] = ["Speed", "Stamina", "Power", "Guts", "Wit"];

// ---------------------------------------------------------------------------
// Directory
// ---------------------------------------------------------------------------

/// Every career file, newest first.
///
/// The names are timestamp-prefixed, so lexical order is chronological and no
/// file has to be opened to sort the list. A file that will not parse is
/// skipped rather than failing the page — one bad export must not hide the
/// rest.
pub fn list(dir: &Path, umdb: &crate::umdb::Umdb) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new(); // no directory yet: nothing exported, not an error
    };
    let mut names: Vec<String> = read
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    names.sort_unstable();
    names.reverse();

    names
        .into_iter()
        .filter_map(|file| {
            let value = read_json(&dir.join(&file))?;
            let chara = chara_info(&value);
            let card_id = int(chara, "card_id");
            Some(Entry {
                when: pretty_stamp(&file),
                card_id,
                trainee: umdb.trainee(card_id),
                chara_grade: int(chara, "chara_grade"),
                stats: stats_of(chara),
                file,
            })
        })
        .collect()
}

/// Resolve a URL segment to a file inside `dir`, or `None` if it tries to leave.
///
/// Two gates rather than one: the name may not contain a separator or `..` in
/// the first place, and the resolved path must still be inside `dir`. The
/// second alone would be enough on a well-behaved filesystem; the first is what
/// makes the intent obvious.
pub fn resolve(dir: &Path, file: &str) -> Option<PathBuf> {
    if !file.ends_with(".json")
        || file.contains("..")
        || file.contains('/')
        || file.contains('\\')
        || file.contains(':')
    {
        return None;
    }
    let path = dir.join(file);
    let (real_dir, real_path) = (dir.canonicalize().ok()?, path.canonicalize().ok()?);
    real_path.starts_with(&real_dir).then_some(path)
}

pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

// ---------------------------------------------------------------------------
// One career
// ---------------------------------------------------------------------------

pub fn parse(file: &str, value: &Value, umdb: &crate::umdb::Umdb) -> Career {
    let chara = chara_info(value);
    let log = value.pointer("/data/progress_log_info");

    Career {
        file: file.to_string(),
        when: pretty_stamp(file),
        source: string(value.get("honse_source")),
        plugin_version: string(value.get("honse_tracker_version")),
        card_id: int(chara, "card_id"),
        trainee: umdb.trainee(int(chara, "card_id")),
        chara_grade: int(chara, "chara_grade"),
        stats: stats_of(chara),
        skill_points: int(log, "total_skill_point"),
        races: races(log),
        supports: supports(log, umdb),
        factors: factors(log),
        conditions: conditions(log),
        skills: skills(chara, umdb),
    }
}

fn races(log: Option<&Value>) -> Vec<Race> {
    array(log, "race_history_array")
        .iter()
        .map(|entry| {
            let h = entry.get("race_history");
            let turn = int(h, "turn");
            // The export carries no scenario id, and every finale label this
            // could pick is wrong for an idle run anyway; 0 means "not
            // Trackblazer", which is the only branch that matters here.
            let (year, date) = honse_career_meta::turn_date(i32::try_from(turn).unwrap_or(0), 0);
            Race {
                turn,
                year,
                date,
                rank: int(h, "result_rank"),
                program_id: int(h, "program_id"),
                ground: ground(int(h, "ground_condition")),
                weather: weather(int(h, "weather")),
                style: running_style(int(h, "running_style")),
                fans: int(entry.get("race_reward_info"), "gained_fans"),
            }
        })
        .collect()
}

fn supports(log: Option<&Value>, umdb: &crate::umdb::Umdb) -> Vec<Support> {
    array(log, "support_card_gain_info_array")
        .iter()
        .map(|entry| {
            let card_id = int(Some(entry), "support_card_id");
            Support {
                card_id,
                name: umdb.support_card(card_id).map(str::to_owned),
                gains: signed_stats(entry.get("gain_info")),
            }
        })
        .collect()
}

/// The trainee's skills at the end of the run.
///
/// Read from `chara_info.skill_array`, which is where the response actually
/// lists them — `progress_log_info.gain_skill_id_array` was empty in the only
/// real export while `skill_array` held the learned skill and its level.
fn skills(chara: Option<&Value>, umdb: &crate::umdb::Umdb) -> Vec<Skill> {
    array(chara, "skill_array")
        .iter()
        .map(|entry| {
            let id = int(Some(entry), "skill_id");
            let found = umdb.skill(id);
            Skill {
                id,
                level: int(Some(entry), "level"),
                name: found.map(|s| s.name.clone()),
                icon_id: found.and_then(|s| s.icon_id),
            }
        })
        .collect()
}

fn factors(log: Option<&Value>) -> Vec<FactorYear> {
    array(log, "succession_factor_gain_array")
        .iter()
        .map(|entry| FactorYear {
            year: int(Some(entry), "year"),
            factors: array(Some(entry), "gain_factor_info_array")
                .iter()
                .map(|f| Factor {
                    id: int(Some(f), "factor_id"),
                    level: int(Some(f), "level"),
                })
                .collect(),
        })
        .collect()
}

fn conditions(log: Option<&Value>) -> Vec<Condition> {
    array(log, "chara_effect_log_array")
        .iter()
        .map(|entry| {
            let id = i32::try_from(int(Some(entry), "chara_effect_id")).unwrap_or(0);
            let (name, polarity) = honse_career_meta::lookup(id);
            Condition {
                id,
                name,
                good: polarity == honse_career_meta::Polarity::Positive,
                active: entry.get("is_active").and_then(Value::as_bool).unwrap_or(false),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Field access
// ---------------------------------------------------------------------------

fn chara_info(value: &Value) -> Option<&Value> {
    value.pointer("/data/end_info/chara_info")
}

fn stats_of(chara: Option<&Value>) -> [i64; 5] {
    STAT_KEYS.map(|key| int(chara, key))
}

/// The five stats out of a `{sign, value}` gain block.
///
/// `sign` is the game's own negation flag. Every sample seen so far is `0`;
/// `1` is read as a loss, which is the only reading that makes the field mean
/// anything, but it has not been observed and is flagged in the README.
fn signed_stats(gain: Option<&Value>) -> [i64; 5] {
    STAT_KEYS.map(|key| {
        let Some(field) = gain.and_then(|g| g.get(key)) else {
            return 0;
        };
        // Either a bare number or the wrapper, depending on the field.
        let magnitude = field.as_i64().unwrap_or_else(|| int(Some(field), "value"));
        if int(Some(field), "sign") == 1 {
            -magnitude
        } else {
            magnitude
        }
    })
}

fn int(parent: Option<&Value>, key: &str) -> i64 {
    parent.and_then(|v| v.get(key)).and_then(Value::as_i64).unwrap_or(0)
}

fn string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("—").to_string()
}

fn array<'a>(parent: Option<&'a Value>, key: &str) -> &'a [Value] {
    parent
        .and_then(|v| v.get(key))
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

// ---------------------------------------------------------------------------
// Labels
// ---------------------------------------------------------------------------

/// `20260901_220752-card100702-end.json` → `2026-09-01 22:07`.
///
/// Falls back to the raw name: a file someone renamed is still worth listing.
fn pretty_stamp(file: &str) -> String {
    let stamp = file.split('-').next().unwrap_or(file);
    let (date, time) = stamp.split_once('_').unwrap_or((stamp, ""));
    if date.len() != 8 || time.len() < 4 || !date.chars().all(|c| c.is_ascii_digit()) {
        return file.to_string();
    }
    format!(
        "{}-{}-{} {}:{}",
        &date[0..4],
        &date[4..6],
        &date[6..8],
        &time[0..2],
        &time[2..4]
    )
}

fn ground(v: i64) -> &'static str {
    match v {
        1 => "Firm",
        2 => "Good",
        3 => "Soft",
        4 => "Heavy",
        _ => "—",
    }
}

fn weather(v: i64) -> &'static str {
    match v {
        1 => "Sunny",
        2 => "Cloudy",
        3 => "Rainy",
        4 => "Snowy",
        _ => "—",
    }
}

fn running_style(v: i64) -> &'static str {
    match v {
        1 => "Front",
        2 => "Pace",
        3 => "Late",
        4 => "End",
        _ => "—",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_stamp_comes_out_of_the_file_name() {
        assert_eq!(pretty_stamp("20260901_220752-card100702-end.json"), "2026-09-01 22:07");
    }

    /// A renamed file still belongs in the list, under whatever it is called.
    #[test]
    fn an_unparseable_name_falls_back_to_itself() {
        assert_eq!(pretty_stamp("my-run.json"), "my-run.json");
        assert_eq!(pretty_stamp("notadate_1200-end.json"), "notadate_1200-end.json");
    }

    /// The one gate that matters: a URL segment must not be able to name a file
    /// outside the careers directory.
    #[test]
    fn traversal_is_refused() {
        let dir = std::env::temp_dir();
        for bad in [
            "../secret.json",
            "..\\secret.json",
            "sub/other.json",
            "C:\\windows\\x.json",
            "notjson.txt",
        ] {
            assert!(resolve(&dir, bad).is_none(), "{bad} should be refused");
        }
    }

    #[test]
    fn stat_gains_unwrap_the_sign_value_pair() {
        let gain = json!({
            "speed": { "sign": 0, "value": 83 },
            "stamina": { "sign": 1, "value": 12 },
            "power": 40
        });
        let stats = signed_stats(Some(&gain));
        assert_eq!(stats[0], 83, "sign 0 is a gain");
        assert_eq!(stats[1], -12, "sign 1 reads as a loss");
        assert_eq!(stats[2], 40, "a bare number is taken as-is");
        assert_eq!(stats[3], 0, "an absent stat is zero, not a failure");
    }

    /// Skills live on the trainee, not in the progress log. Pinned because the
    /// first version read the log's `gain_skill_id_array`, which is empty in a
    /// real export, and rendered nothing without anyone noticing.
    #[test]
    fn skills_come_from_the_trainee() {
        let value = json!({ "data": {
            "end_info": { "chara_info": { "skill_array": [ { "skill_id": 110071, "level": 4 } ] } },
            "progress_log_info": { "gain_skill_id_array": [] }
        } });
        let career = parse("x.json", &value, &crate::umdb::Umdb::empty());
        assert_eq!(career.skills.len(), 1);
        assert_eq!(career.skills[0].id, 110_071);
        assert_eq!(career.skills[0].level, 4);
    }

    /// A payload that has moved on must render, not panic.
    #[test]
    fn an_empty_document_parses_to_an_empty_career() {
        let career = parse("x.json", &json!({}), &crate::umdb::Umdb::empty());
        assert_eq!(career.card_id, 0);
        assert!(career.races.is_empty());
        assert!(career.supports.is_empty());
        assert_eq!(career.source, "—");
    }
}
