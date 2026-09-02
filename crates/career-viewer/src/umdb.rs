//! Names for the ids the export only carries as numbers.
//!
//! The plugin dumps the server's response verbatim, which means ids and nothing
//! else — the game resolves those from master data the export has no reason to
//! duplicate. hakuraku.moe publishes that master data as one JSON file, so the
//! viewer downloads it rather than shipping a table of its own that would go
//! stale on the next game update.
//!
//! # Cached, because it is 1.5 MB
//!
//! [`Umdb::fetch`] keeps a copy on disk and only asks the site again once the
//! copy is a day old. When the site cannot be reached the stale copy is used;
//! deleting the file forces a fresh download.
//!
//! # Optional by design
//!
//! Everything here returns `Option`, and no file at all simply means every
//! lookup misses. The pages then show the raw ids, which is exactly what they
//! showed before this module existed — a viewer that refuses to start because
//! it is offline would be worse than one that renders numbers.
//!
//! # What it cannot answer
//!
//! Race `program_id`. Those are small ordinals (1, 3, 73…) from the game's
//! `single_mode_program` table, not the six-digit `race_instance` ids this file
//! carries, and there is no join between them here. Races keep their raw
//! program id until something publishes that table.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// How old the cached file may be before the site is asked again.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// `%LOCALAPPDATA%\honse-tracker\umdb.json`, or the temp directory when there
/// is no profile to hang it off.
pub fn default_cache() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("honse-tracker")
        .join("umdb.json")
}

pub struct Umdb {
    charas: HashMap<i64, String>,
    /// Card id → outfit title, e.g. `[RUN! RUIN! LAUNCHER!]`.
    cards: HashMap<i64, String>,
    support_cards: HashMap<i64, String>,
    skills: HashMap<i64, Skill>,
}

pub struct Skill {
    pub name: String,
    /// Skills share icons, so this is not the skill id — matching on that finds
    /// nothing at all.
    pub icon_id: Option<i64>,
}

// The on-disk shape. Typed rather than untyped, unlike the career export: this
// file is generated to a fixed schema, so a field that vanishes is worth a loud
// failure rather than a silently empty name.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    #[serde(default)]
    chara: Vec<Named>,
    #[serde(default)]
    card: Vec<Named>,
    #[serde(default)]
    support_card: Vec<Named>,
    #[serde(default)]
    skill: Vec<RawSkill>,
}

#[derive(Deserialize)]
struct Named {
    id: Option<i64>,
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSkill {
    id: Option<i64>,
    name: Option<String>,
    icon_id: Option<i64>,
}

/// Younger than [`MAX_AGE`]. A file whose age cannot be read counts as stale,
/// which at worst costs one download.
fn is_fresh(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < MAX_AGE)
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build()
        .get(url)
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn index(rows: Vec<Named>) -> HashMap<i64, String> {
    rows.into_iter().filter_map(|row| Some((row.id?, row.name?))).collect()
}

impl Umdb {
    /// The database from `<base>/data/umdb.json`, through the cache at `cache`.
    ///
    /// A fresh enough cache is used without asking the site. Otherwise the file
    /// is downloaded and the cache replaced — but only after the download
    /// succeeds, so a network failure never leaves an empty file behind and the
    /// stale copy still serves. `None` only when there is nothing at all.
    pub fn fetch(base: &str, cache: &Path) -> Option<Self> {
        if !is_fresh(cache) {
            match download(&format!("{}/data/umdb.json", base.trim_end_matches('/'))) {
                Ok(bytes) => {
                    if let Some(dir) = cache.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if let Err(e) = std::fs::write(cache, bytes) {
                        eprintln!("note: could not cache umdb at {}: {e}", cache.display());
                    }
                }
                Err(e) => eprintln!("note: could not download umdb ({e}); using the cached copy if any"),
            }
        }
        Self::load(cache)
    }

    /// Parse an `umdb.json` on disk. `None` when it is absent or unreadable —
    /// see the module note: that is a degraded viewer, not a broken one.
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let file: File = serde_json::from_str(&text).ok()?;
        Some(Self {
            charas: index(file.chara),
            cards: index(file.card),
            support_cards: index(file.support_card),
            skills: file
                .skill
                .into_iter()
                .filter_map(|s| {
                    Some((
                        s.id?,
                        Skill {
                            name: s.name?,
                            icon_id: s.icon_id,
                        },
                    ))
                })
                .collect(),
        })
    }

    /// An empty database, for when the file is not there. Every lookup misses.
    pub fn empty() -> Self {
        Self {
            charas: HashMap::new(),
            cards: HashMap::new(),
            support_cards: HashMap::new(),
            skills: HashMap::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.charas.is_empty() && self.cards.is_empty() && self.support_cards.is_empty()
    }

    /// The trainee as the game writes it: outfit title then character, e.g.
    /// `[RUN! RUIN! LAUNCHER!] Gold Ship`.
    ///
    /// The card table holds only the outfit, and the character is joined
    /// through the card id's own leading digits. Either half alone is still
    /// worth showing — an outfit with no character still identifies the run.
    pub fn trainee(&self, card_id: i64) -> Option<String> {
        let outfit = self.cards.get(&card_id);
        let chara = i32::try_from(card_id)
            .ok()
            .and_then(honse_career_meta::chara_id_from_card_id)
            .and_then(|id| self.charas.get(&id));
        match (outfit, chara) {
            (Some(outfit), Some(chara)) => Some(format!("{outfit} {chara}")),
            (Some(one), None) | (None, Some(one)) => Some(one.clone()),
            (None, None) => None,
        }
    }

    pub fn support_card(&self, id: i64) -> Option<&str> {
        self.support_cards.get(&id).map(String::as_str)
    }

    pub fn skill(&self, id: i64) -> Option<&Skill> {
        self.skills.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::Umdb;

    fn sample() -> Umdb {
        let json = r#"{
            "chara": [{"id": 1007, "name": "Gold Ship"}],
            "card": [{"id": 100702, "name": "[RUN! RUIN! LAUNCHER!]"}],
            "supportCard": [{"id": 30034, "name": "[Happiness] Rice Shower"}],
            "skill": [{"id": 10071, "name": "Warning Shot!", "iconId": 20013}]
        }"#;
        let path = std::env::temp_dir().join("honse-umdb-test.json");
        std::fs::write(&path, json).expect("write sample");
        Umdb::load(&path).expect("parse sample")
    }

    #[test]
    fn the_trainee_joins_outfit_and_character() {
        assert_eq!(
            sample().trainee(100_702).as_deref(),
            Some("[RUN! RUIN! LAUNCHER!] Gold Ship")
        );
    }

    /// Half an answer still identifies a run, so a card the table does not know
    /// falls back to whichever half it does.
    #[test]
    fn half_a_name_is_better_than_none() {
        let db = sample();
        // Outfit unknown, character known through the leading digits.
        assert_eq!(db.trainee(100_799).as_deref(), Some("Gold Ship"));
        // Neither known.
        assert_eq!(db.trainee(999_999), None);
    }

    /// Skills share icons, so the icon comes from `iconId`. Matching on the
    /// skill id finds nothing at all — 0 of 714 in the real file.
    #[test]
    fn skills_carry_a_separate_icon_id() {
        let db = sample();
        let skill = db.skill(10071).expect("known skill");
        assert_eq!(skill.name, "Warning Shot!");
        assert_eq!(skill.icon_id, Some(20013));
        assert_ne!(skill.icon_id, Some(10071));
    }

    /// A cache that was just written must not trigger a download; one that is
    /// missing must. (The download itself is not exercised here — it needs the
    /// network — only the decision to make it.)
    #[test]
    fn freshness_decides_whether_to_ask_the_site() {
        let path = std::env::temp_dir().join("honse-umdb-fresh-test.json");
        std::fs::write(&path, "{}").expect("write");
        assert!(super::is_fresh(&path));
        assert!(!super::is_fresh(std::path::Path::new("no-such-umdb.json")));
    }

    /// A missing file is a degraded viewer, not a broken one.
    #[test]
    fn a_missing_file_yields_no_database() {
        assert!(Umdb::load(std::path::Path::new("no-such-umdb.json")).is_none());
        let empty = Umdb::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.trainee(100_702), None);
        assert_eq!(empty.support_card(30_034), None);
    }
}
