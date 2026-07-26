//! Pure presentation helpers for the Career overlay panel, ported from the
//! honse-tracker dashboard so the egui panel labels/sprites match it exactly:
//!
//! - [`turn_date`] — career turn → `(year, date)` label (`career-calendar.ts`).
//! - [`stat_rank_sprite`] — stat value → `statusrank` sprite path (`stat-rank.ts`
//!   `rankIconIndex`, itself uma-sim's `rankForStat`).
//! - [`rank_label_sprite`] — overall-rank label → badge sprite path
//!   (`stat-rank.ts` `rankLabelIconIndex`).
//! - [`trainee_portrait_path`] / [`stat_icon_path`] — icon paths under `icons/`.
//!
//! Paths are relative to the staged `icons/` dir (see [`crate::ui::textures`]).

use crate::gametora_data;

// Hidden-for-now Career date/skills section helpers; kept until re-enabled.
#[allow(dead_code)]
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
#[allow(dead_code)]
const YEARS: [&str; 3] = ["Junior Year", "Classic Year", "Senior Year"];
#[allow(dead_code)]
const FINALE_STAGES: [&str; 3] = ["URA Qualifiers", "URA Semifinals", "URA Finals"];
const LETTER_ORDER: [&str; 8] = ["G", "F", "E", "D", "C", "B", "A", "S"];

/// Trackblazer (Make a New Track), per `get_ScenarioId`.
#[allow(dead_code)]
const SCENARIO_TRACKBLAZER: i32 = 4;

/// In-game `(year, date)` label for a 1-based career turn. Mirrors
/// `career-calendar.ts::turnDate`. Junior turns 1–12 are pre-debut at two turns
/// per half-month (Apr–Jun); 13–24 normal (Jul–Dec); 25–48 Classic; 49–72
/// Senior; 73+ the scenario finale.
#[must_use]
#[allow(dead_code)]
pub fn turn_date(turn: i32, scenario_id: i32) -> (&'static str, String) {
    if turn >= 73 {
        if scenario_id == SCENARIO_TRACKBLAZER {
            return ("TS Climax", "Races Underway".to_string());
        }
        // ceil((turn-72)/2) == (turn-71)/2 in integer math; clamp to 1..=3.
        let idx = ((turn - 71) / 2).clamp(1, 3) - 1;
        let stage = FINALE_STAGES.get(idx as usize).copied().unwrap_or("URA Finals");
        return ("Finale Season", stage.to_string());
    }
    let t = turn.max(1);
    let year = YEARS[((t - 1) / 24).min(2) as usize];
    let in_year = ((t - 1) % 24) + 1; // 1..=24

    if t <= 12 {
        let half_idx = (in_year + 1) / 2; // ceil(in_year/2): 1..=6 → Early Apr .. Late Jun
        let month = MONTHS[3 + ((half_idx - 1) / 2) as usize]; // Apr, May, Jun
        let half = if half_idx % 2 == 1 { "Early" } else { "Late" };
        return (year, format!("Pre-Debut · {half} {month}"));
    }
    if t <= 24 {
        let rest = in_year - 12; // 1..=12 → Early Jul .. Late Dec
        let month = MONTHS[6 + ((rest - 1) / 2) as usize];
        let half = if rest % 2 == 1 { "Early" } else { "Late" };
        return (year, format!("{half} {month}"));
    }
    let month = MONTHS[((in_year + 1) / 2 - 1) as usize]; // ceil(in_year/2) - 1
    let half = if in_year % 2 == 1 { "Early" } else { "Late" };
    (year, format!("{half} {month}"))
}

/// Stat value → `statusrank` sprite index (uma-sim `rankForStat`). Clamped 0..=97.
fn rank_icon_index(x: i32) -> i32 {
    if x > 1200 {
        (18 + ((x - 1200) / 100) * 10 + (x / 10) % 10).min(97)
    } else if x >= 1150 {
        17 // SS+
    } else if x >= 1100 {
        16 // SS
    } else if x >= 400 {
        8 + (x - 400) / 100 // C(8) up by 100
    } else {
        (x.max(0) / 50).min(7) // G(0) up by 50
    }
}

/// Per-stat rank sprite path for a stat value, e.g. `statusrank/ui_statusrank_08.png`.
#[must_use]
pub fn stat_rank_sprite(value: i32) -> String {
    format!(
        "statusrank/ui_statusrank_{:02}.png",
        rank_icon_index(value).clamp(0, 97)
    )
}

/// Overall-rank badge label (`rank_table::rank_label`) → sprite index, or `None`
/// for labels with no sprite (the L-ladder beyond the U range). Mirrors
/// `stat-rank.ts::rankLabelIconIndex`.
fn rank_label_index(label: &str) -> Option<i32> {
    // SS / SS+
    if label == "SS" {
        return Some(16);
    }
    if label == "SS+" {
        return Some(17);
    }
    // U-ladder: U<letter><digit?>  (e.g. UG, UA3)
    if let Some(rest) = label.strip_prefix('U') {
        let mut chars = rest.chars();
        let letter = chars.next()?;
        let li = LETTER_ORDER.iter().position(|&l| l.starts_with(letter))? as i32;
        let digit = match chars.next() {
            None => 0,
            Some(d @ '1'..='9') => d as i32 - '0' as i32,
            Some(_) => return None,
        };
        if chars.next().is_some() {
            return None;
        }
        return Some((18 + li * 10 + digit).min(97));
    }
    // Single letter with optional '+'.
    let (letter, plus) = label.strip_suffix('+').map_or((label, 0), |l| (l, 1));
    LETTER_ORDER
        .iter()
        .position(|&l| l == letter)
        .map(|li| li as i32 * 2 + plus)
}

/// Overall-rank badge sprite path for a rank label, or `None` when no sprite
/// exists (caller falls back to text).
#[must_use]
pub fn rank_label_sprite(label: &str) -> Option<String> {
    rank_label_index(label).map(|idx| format!("statusrank/ui_statusrank_{idx:02}.png"))
}

/// Trainee portrait sprite path for a trained outfit `card_id`, e.g.
/// `chara/chr_icon_1014.png`. Resolves the character id via the outfit catalog,
/// falling back to the card id's first four digits. `None` when unresolvable.
#[must_use]
#[allow(dead_code)]
pub fn trainee_portrait_path(card_id: i32) -> Option<String> {
    let char_id = gametora_data::character_card(card_id as i64)
        .and_then(|c| c.char_id)
        .filter(|&c| c > 0)
        .or_else(|| {
            // Fallback: the chara id is the card id's leading 4 digits.
            card_id.to_string().get(0..4).and_then(|p| p.parse::<i64>().ok())
        })
        .filter(|&c| c > 0)?;
    Some(format!("chara/chr_icon_{char_id}.png"))
}

/// Stat-type glyph sprite path for a facility index (0 Speed … 4 Wit), e.g.
/// `status_02.png`. Index is assumed in range (callers iterate 0..5).
#[must_use]
pub fn stat_icon_path(facility: usize) -> String {
    format!("status_0{facility}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_date_matches_dashboard_samples() {
        // Senior, turn 58 → "Late May" (the reference screenshot).
        assert_eq!(turn_date(58, 0), ("Senior Year", "Late May".to_string()));
        // Junior pre-debut (turn 1) → Early Apr.
        assert_eq!(turn_date(1, 0), ("Junior Year", "Pre-Debut · Early Apr".to_string()));
        // Junior post-debut window starts Early Jul at turn 13.
        assert_eq!(turn_date(13, 0), ("Junior Year", "Early Jul".to_string()));
        // Classic year boundary (turn 25 → Early Jan).
        assert_eq!(turn_date(25, 0), ("Classic Year", "Early Jan".to_string()));
        // Finale stages + Trackblazer override.
        assert_eq!(turn_date(73, 0), ("Finale Season", "URA Qualifiers".to_string()));
        assert_eq!(turn_date(77, 0), ("Finale Season", "URA Finals".to_string()));
        assert_eq!(turn_date(75, 4), ("TS Climax", "Races Underway".to_string()));
    }

    #[test]
    fn stat_rank_sprite_zero_pads_and_brackets() {
        assert_eq!(rank_icon_index(0), 0);
        assert_eq!(rank_icon_index(399), 7); // just under C
        assert_eq!(rank_icon_index(400), 8); // C
        assert_eq!(rank_icon_index(1100), 16); // SS
        assert_eq!(rank_icon_index(1150), 17); // SS+
        assert!(rank_icon_index(2200) <= 97);
        assert_eq!(stat_rank_sprite(400), "statusrank/ui_statusrank_08.png");
    }

    #[test]
    fn rank_label_sprite_parses_letters_plus_and_uladder() {
        assert_eq!(rank_label_index("G"), Some(0));
        assert_eq!(rank_label_index("G+"), Some(1));
        assert_eq!(rank_label_index("B"), Some(10));
        assert_eq!(rank_label_index("B+"), Some(11));
        assert_eq!(rank_label_index("S"), Some(14));
        assert_eq!(rank_label_index("SS"), Some(16));
        assert_eq!(rank_label_index("SS+"), Some(17));
        assert_eq!(rank_label_index("UG"), Some(18));
        assert_eq!(rank_label_index("UA3"), Some(18 + 6 * 10 + 3));
        assert_eq!(rank_label_index("LS15"), None); // L-ladder: no sprite
        assert_eq!(
            rank_label_sprite("B"),
            Some("statusrank/ui_statusrank_10.png".to_string())
        );
    }

    #[test]
    fn stat_icon_path_indices() {
        assert_eq!(stat_icon_path(0), "status_00.png");
        assert_eq!(stat_icon_path(4), "status_04.png");
    }
}
