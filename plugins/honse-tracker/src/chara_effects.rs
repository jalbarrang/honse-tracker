//! Career condition (状態 / chara-effect) id → display name + polarity.
//!
//! The live `WorkSingleModeCharaData.CharaEffectIdArray` exposes the active
//! condition ids; this table maps them to the English names shown in-game.
//! Generated from the game master DB (`single_mode_chara_effect` joined with
//! `text_data` category 142). `effect_type` 1 = positive, 2 = negative.

/// Whether a condition helps (orange in-game) or hurts (blue in-game).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    Positive,
    Negative,
}

use Polarity::{Negative, Positive};

/// (id, English name, polarity), ascending by id.
const EFFECTS: &[(i32, &str, Polarity)] = &[
    (1, "Hot Topic", Positive),
    (2, "Slacker", Negative),
    (3, "Skin Outbreak", Negative),
    (4, "Slow Metabolism", Negative),
    (5, "Migraine", Negative),
    (6, "Practice Poor", Negative),
    (7, "Fast Learner", Positive),
    (8, "Charming \u{25cb}", Positive),
    (9, "Night Owl", Negative),
    (10, "Practice Perfect \u{25cb}", Positive),
    (11, "Practice Perfect \u{25ce}", Positive),
    (12, "Under the Weather", Negative),
    (13, "Shining Brightly", Positive),
    (14, "Fan Promise (Hokkaido)", Positive),
    (15, "Fan Promise (Hokuto)", Positive),
    (16, "Fan Promise (Nakayama)", Positive),
    (17, "Fan Promise (Kansai)", Positive),
    (18, "Fan Promise (Kokura)", Positive),
    (19, "Not Ready", Negative),
    (20, "Legs of Glass", Negative),
    (100, "Pure Passion: Team Sirius", Positive),
];

/// Look up a condition by its chara-effect id. Returns `(name, polarity)`.
/// Unknown ids fall back to a generic label and `Positive` so new/unmapped
/// effects still render (and get logged once for follow-up).
#[allow(dead_code)]
pub fn lookup(id: i32) -> (String, Polarity) {
    match EFFECTS.iter().find(|&&(eid, _, _)| eid == id) {
        Some(&(_, name, pol)) => (name.to_string(), pol),
        None => (format!("Effect #{id}"), Polarity::Positive),
    }
}

/// `true` if the id is present in the known table.
pub fn is_known(id: i32) -> bool {
    EFFECTS.iter().any(|&(eid, _, _)| eid == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_effects_resolve() {
        assert_eq!(lookup(1), ("Hot Topic".to_string(), Polarity::Positive));
        assert_eq!(lookup(2), ("Slacker".to_string(), Polarity::Negative));
        assert_eq!(lookup(7), ("Fast Learner".to_string(), Polarity::Positive));
        assert_eq!(lookup(9), ("Night Owl".to_string(), Polarity::Negative));
    }

    #[test]
    fn unknown_effect_falls_back() {
        let (name, pol) = lookup(9999);
        assert_eq!(name, "Effect #9999");
        assert_eq!(pol, Polarity::Positive);
        assert!(!is_known(9999));
    }

    #[test]
    fn table_ids_are_unique_and_ascending() {
        for w in EFFECTS.windows(2) {
            assert!(w[0].0 < w[1].0, "ids must ascend: {} {}", w[0].0, w[1].0);
        }
    }
}
