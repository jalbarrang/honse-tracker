//! Skill-evaluation resource: per-skill grade value + aptitude role + unique flag.
//!
//! Loaded once at runtime from `skill_grades.json`, which the host downloads into
//! the game data dir (the `hosted_data` TRACKER sync); for dev it falls back to a
//! copy next to the plugin DLL (placed by `deploy-windows.ps1`). Generated offline
//! by the `skill-grades` tool (`cargo run -p skill-grades`) from the game's
//! master.mdb `grade_value` + UmaTools `affinity_role` (fetch master.mdb first with
//! `cargo run -p fetch-master-db`), then published under `data/` via
//! `cargo run -p tracker-data-manifest`.
//!
//! Keeping the data in a sidecar file (not bundled in the DLL) lets it be updated
//! per game version without rebuilding.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::compat::Sdk;
use serde::Deserialize;

/// One skill's evaluation inputs.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillGrade {
    /// Base grade value (evaluation points before aptitude scaling).
    pub g: i32,
    /// Aptitude role key (lowercased; `"a/b"` for compound). `None` = no scaling.
    #[serde(default)]
    pub r: Option<String>,
    /// `Some(1)` when this is a trainee unique (scored via the level bonus instead).
    #[serde(default)]
    pub u: Option<u8>,
}

impl SkillGrade {
    pub fn is_unique(&self) -> bool {
        self.u == Some(1)
    }
}

static TABLE: OnceLock<Option<HashMap<i32, SkillGrade>>> = OnceLock::new();

/// Path to the resource file: prefer the host-downloaded copy in the game data
/// dir; fall back to next to the plugin DLL / game exe (dev + back-compat).
fn resource_path() -> std::path::PathBuf {
    if let Some(p) = Sdk::try_get().and_then(|sdk| sdk.host_data_path("skill_grades.json")) {
        if p.is_file() {
            return p;
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|dir| dir.join("skill_grades.json")))
        .unwrap_or_else(|| std::path::PathBuf::from("skill_grades.json"))
}

fn load() -> Option<HashMap<i32, SkillGrade>> {
    let path = resource_path();
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            hlog_warn!(target: "training-tracker", "eval_data: {} not found ({e})", path.display());
            return None;
        }
    };
    // Keys are strings in JSON; parse to a string map then convert ids to i32.
    let raw: HashMap<String, SkillGrade> = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(e) => {
            hlog_error!(target: "training-tracker", "eval_data: parse failed: {e}");
            return None;
        }
    };
    let map: HashMap<i32, SkillGrade> = raw
        .into_iter()
        .filter_map(|(k, v)| k.parse::<i32>().ok().map(|id| (id, v)))
        .collect();
    hlog_info!(target: "training-tracker", "eval_data: loaded {} skill grades", map.len());
    Some(map)
}

/// Lazily-loaded skill-grade table; `None` if the resource is missing/invalid.
pub fn table() -> Option<&'static HashMap<i32, SkillGrade>> {
    TABLE.get_or_init(load).as_ref()
}
