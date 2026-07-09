//! CM course-parameter resource: per-course distance / surface / set-status
//! thresholds, used to parameterize the [`crate::cm_model`] survival/threshold
//! math for a chosen target course.
//!
//! Loaded once at runtime from `course_params.json`, which the host downloads into
//! the game data dir (the `hosted_data` TRACKER sync); for dev it falls back to a
//! copy next to the plugin DLL (placed by `deploy-windows.ps1`). Generated offline
//! by the `course-data` tool (`cargo run -p course-data`) from master.mdb
//! (`race_course_set` + `race_course_set_status`; fetch master.mdb first with
//! `fetch-master-db`), then published under `data/` via `tracker-data-manifest`.
//!
//! Keeping the data in a sidecar file (not bundled in the DLL) lets it be updated
//! per game version without rebuilding. Mirrors [`crate::eval_data`].

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::compat::Sdk;

use crate::cm_model::CourseParams;

static TABLE: OnceLock<Option<HashMap<i32, CourseParams>>> = OnceLock::new();

/// Path to the resource file: prefer the host-downloaded copy in the game data
/// dir; fall back to next to the plugin DLL / game exe (dev + back-compat).
fn resource_path() -> std::path::PathBuf {
    if let Some(p) = Sdk::try_get().and_then(|sdk| sdk.host_data_path("course_params.json")) {
        if p.is_file() {
            return p;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("course_params.json")))
        .unwrap_or_else(|| std::path::PathBuf::from("course_params.json"))
}

fn load() -> Option<HashMap<i32, CourseParams>> {
    let path = resource_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            hlog_warn!(target: "training-tracker", "course_data: {} not found ({e})", path.display());
            return None;
        }
    };
    // Keys are strings in JSON; parse to a string map then convert ids to i32.
    let raw: HashMap<String, CourseParams> = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(e) => {
            hlog_error!(target: "training-tracker", "course_data: parse failed: {e}");
            return None;
        }
    };
    let map: HashMap<i32, CourseParams> = raw
        .into_iter()
        .filter_map(|(k, v)| k.parse::<i32>().ok().map(|id| (id, v)))
        .collect();
    hlog_info!(target: "training-tracker", "course_data: loaded {} courses", map.len());
    Some(map)
}

/// Lazily-loaded course table; `None` if the resource is missing/invalid.
fn table() -> Option<&'static HashMap<i32, CourseParams>> {
    TABLE.get_or_init(load).as_ref()
}

/// Course parameters for a course id, or `None` when the resource or id is
/// unavailable (so the CM scorer can fall back gracefully).
pub fn course_params(course_id: i32) -> Option<&'static CourseParams> {
    table()?.get(&course_id)
}

/// Racetrack display name from a course id's track prefix (`id / 100`).
fn racetrack_name(course_id: i32) -> String {
    track_name(course_id / 100)
}

/// Racetrack display name from a track id (`course_id / 100`). The JRA tracks are
/// numbered 101..110 in their canonical order, with 111 a local (NAR) dirt
/// track. Falls back to `Track NNN` for anything unexpected.
pub fn track_name(track: i32) -> String {
    let name = match track {
        101 => "Sapporo",
        102 => "Hakodate",
        103 => "Niigata",
        104 => "Fukushima",
        105 => "Nakayama",
        106 => "Tokyo",
        107 => "Chukyo",
        108 => "Kyoto",
        109 => "Hanshin",
        110 => "Kokura",
        111 => "Ooi",
        _ => return format!("Track {track}"),
    };
    name.to_owned()
}

/// Human-readable label for a course id, e.g. `Tokyo · 2000m Turf`. `None` when
/// the course is unknown.
pub fn course_label(course_id: i32) -> Option<String> {
    let c = course_params(course_id)?;
    let surface = match c.surface {
        crate::cm_model::Surface::Turf => "Turf",
        crate::cm_model::Surface::Dirt => "Dirt",
    };
    Some(format!(
        "{} · {}m {}",
        racetrack_name(course_id),
        c.distance as i32,
        surface
    ))
}

/// Courses grouped by track id (`course_id / 100`), each track's list sorted by
/// distance then id. Drives the cascading Track → Course picker. Empty when the
/// resource is unavailable. Returns a `BTreeMap` so tracks iterate in id order.
pub fn courses_by_track() -> std::collections::BTreeMap<i32, Vec<(i32, String)>> {
    let mut map: std::collections::BTreeMap<i32, Vec<(i32, String)>> = std::collections::BTreeMap::new();
    let Some(table) = table() else {
        return map;
    };
    for &id in table.keys() {
        let label = course_label(id).unwrap_or_else(|| format!("#{id}"));
        map.entry(id / 100).or_default().push((id, label));
    }
    for courses in map.values_mut() {
        courses.sort_by(|&(a, _), &(b, _)| {
            let (da, db) = (table[&a].distance as i32, table[&b].distance as i32);
            da.cmp(&db).then(a.cmp(&b))
        });
    }
    map
}

/// All known `(course_id, label)` pairs, sorted by distance then track, for the
/// course picker. Empty when the resource is unavailable.
pub fn all_courses() -> Vec<(i32, String)> {
    let Some(table) = table() else {
        return Vec::new();
    };
    let mut ids: Vec<i32> = table.keys().copied().collect();
    ids.sort_by(|&a, &b| {
        let (da, db) = (table[&a].distance as i32, table[&b].distance as i32);
        da.cmp(&db).then(a.cmp(&b))
    });
    ids.into_iter()
        .map(|id| (id, course_label(id).unwrap_or_else(|| format!("#{id}"))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_model::{StatKind, Surface};

    /// Lock the JSON contract between the `course-data` tool's asset and the
    /// plugin's `CourseParams` (field names + enum string values must agree).
    #[test]
    fn asset_shape_deserializes_into_course_params() {
        let sample = r#"{
            "10104": {
                "distance": 2000.0,
                "surface": "Turf",
                "turn": 1,
                "thresholds": ["Power"],
                "finish_time_min": 1171000.0,
                "finish_time_max": 1230000.0
            },
            "10906": {
                "distance": 1600.0,
                "surface": "Dirt",
                "turn": 2,
                "thresholds": [],
                "finish_time_min": 0.0,
                "finish_time_max": 0.0
            }
        }"#;
        let map: HashMap<String, CourseParams> = serde_json::from_str(sample).expect("asset parses");
        let c = &map["10104"];
        assert_eq!(c.distance, 2000.0);
        assert_eq!(c.surface, Surface::Turf);
        assert_eq!(c.set_status_thresholds, vec![StatKind::Power]);
        assert_eq!(map["10906"].surface, Surface::Dirt);
        assert!(map["10906"].set_status_thresholds.is_empty());
    }

    #[test]
    fn racetrack_names_decode_from_course_id() {
        assert_eq!(racetrack_name(10104), "Sapporo"); // id/100 = 101 → Sapporo
        assert_eq!(racetrack_name(10604), "Tokyo"); // 106 → Tokyo
        assert_eq!(racetrack_name(11101), "Ooi"); // 111 → Ooi (local dirt)
        assert_eq!(racetrack_name(99901), "Track 999"); // unknown → fallback
    }

    #[test]
    fn track_name_decodes_track_id() {
        assert_eq!(track_name(106), "Tokyo");
        assert_eq!(track_name(109), "Hanshin");
        assert_eq!(track_name(999), "Track 999");
    }

    /// The generated resource asset, if present, must parse cleanly.
    #[test]
    fn shipped_asset_parses() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/course_params.json"
        );
        let Ok(bytes) = std::fs::read(path) else {
            return; // asset not generated in this checkout — skip
        };
        let map: HashMap<String, CourseParams> =
            serde_json::from_slice(&bytes).expect("shipped course_params.json parses");
        assert!(!map.is_empty(), "asset should contain courses");
    }
}
