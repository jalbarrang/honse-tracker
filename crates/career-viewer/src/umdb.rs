//! Names for the ids the export only carries as numbers.
//!
//! The plugin dumps the server's response verbatim, which means ids and nothing
//! else — the game resolves those from master data the export has no reason to
//! duplicate. hakuraku already publishes that master data as one JSON file, so
//! the viewer reads it rather than shipping a table of its own that would go
//! stale on the next game update.
//!
//! # Optional by design
//!
//! Everything here returns `Option`, and a missing file simply means every
//! lookup misses. The pages then show the raw ids, which is exactly what they
//! showed before this module existed — a viewer that refuses to start because
//! it cannot find someone else's checkout would be worse than one that renders
//! numbers.
//!
//! # What it cannot answer
//!
//! Race `program_id`. Those are small ordinals (1, 3, 73…) from the game's
//! `single_mode_program` table, not the six-digit `race_instance` ids this file
//! carries, and there is no join between them here. Races keep their raw
//! program id until something publishes that table.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

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

fn index(rows: Vec<Named>) -> HashMap<i64, String> {
    rows.into_iter().filter_map(|row| Some((row.id?, row.name?))).collect()
}

impl Umdb {
    /// Read hakuraku's `umdb.json`. `None` when it is absent or unreadable —
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
