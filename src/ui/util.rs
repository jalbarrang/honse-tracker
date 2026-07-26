//! Formatting and color helpers for overlay rendering.

use crate::compat::egui;

use crate::memory_reader;

/// Proximity of a stat to its cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CapLevel {
    Normal,
    Near,
    AtCap,
}

/// Classify a stat value against its cap. Unknown cap (`<= 0`) is always `Normal`.
/// `Near` triggers at ≥ 90% of cap; `AtCap` at ≥ cap.
pub(super) fn cap_level(value: i32, cap: i32) -> CapLevel {
    if cap <= 0 {
        return CapLevel::Normal;
    }
    if value >= cap {
        CapLevel::AtCap
    } else if value * 100 >= cap * 90 {
        CapLevel::Near
    } else {
        CapLevel::Normal
    }
}

/// Color for a single stat value, keyed off the real in-game rank thresholds,
/// sampled from the game's official rank sprite. Letter ranks G→SS+ span values
/// 1..=1200 (primary palette); above 1200 stats keep ranking on the U ladder
/// UG..US9 (1201..=2000). Since a single number can't be two-tone like the
/// evaluation badge, U-ladder stats recycle the primary palette by their base
/// letter (UG→G, UF→F, …, US→S). Stats never reach the `L*` ranks — those only
/// exist on the overall evaluation badge. Letter `+`/`-` subranks share their
/// base color.
pub(super) fn stat_rank_color(value: i32) -> egui::Color32 {
    // U-ladder stats (>1200) reuse the primary letter palette via the
    // equivalent base letter, keeping a single source of truth.
    let letter = match value {
        v if v >= 1901 => "S",  // US
        v if v >= 1801 => "A",  // UA
        v if v >= 1701 => "B",  // UB
        v if v >= 1601 => "C",  // UC
        v if v >= 1501 => "D",  // UD
        v if v >= 1401 => "E",  // UE
        v if v >= 1301 => "F",  // UF
        v if v >= 1201 => "G",  // UG
        v if v >= 1100 => "SS", // SS / SS+
        v if v >= 1000 => "S",  // S / S+
        v if v >= 800 => "A",   // A / A+
        v if v >= 600 => "B",   // B / B+
        v if v >= 400 => "C",   // C / C+
        v if v >= 300 => "D",   // D / D+
        v if v >= 200 => "E",   // E / E+
        v if v >= 100 => "F",   // F / F+
        _ => "G",               // G / G+
    };
    rank_letter_color(letter)
}

/// Primary-palette color for a base letter rank
/// (`G`,`F`,`E`,`D`,`C`,`B`,`A`,`S`,`SS`), sampled from the game's rank sprite
/// and tuned for legibility on the dark overlay. Used for the stat ladder's
/// letter tier and as the "base letter" color in the two-tone evaluation badge.
/// Unknown letters fall back to gray.
pub fn rank_letter_color(letter: &str) -> egui::Color32 {
    let (r, g, b) = match letter {
        "SS" => (255, 220, 120), // light gold
        "S" => (250, 195, 70),   // gold
        "A" => (250, 140, 55),   // orange
        "B" => (245, 105, 150),  // rose
        "C" => (110, 205, 80),   // green
        "D" => (60, 150, 225),   // azure blue
        "E" => (210, 110, 215),  // magenta / orchid
        "F" => (165, 135, 225),  // violet
        _ => (150, 150, 150),    // G / unknown - gray
    };
    egui::Color32::from_rgb(r, g, b)
}

/// Flat color for a `U*`/`L*` rank family prefix (first char of the label).
/// Returns `None` for non-prefixed ranks. The in-game `U`/`L` icons use a
/// special gradient; for now we use a single flat color per prefix. Only the
/// `U`/`L` glyph itself is tinted — the trailing base letter keeps its primary
/// color (see `rank_badge_segments`).
pub fn rank_family_color(family: &str) -> Option<egui::Color32> {
    let (r, g, b) = match family.as_bytes().first() {
        Some(b'U') => (90, 245, 244),  // cyan
        Some(b'L') => (191, 241, 213), // mint green
        _ => return None,
    };
    Some(egui::Color32::from_rgb(r, g, b))
}

/// Decompose an evaluation rank label into colored segments for two-tone
/// rendering.
///
/// - `G`..`SS+` (no prefix): one segment, the whole label in its primary
///   base-letter color.
/// - `U*`/`L*` (e.g. `UG3`, `LF12`): two segments — the prefix char (`U`/`L`)
///   in its flat family color, then the remainder in the primary base-letter
///   color.
pub fn rank_badge_segments(label: &str) -> Vec<(String, egui::Color32)> {
    let bytes = label.as_bytes();
    if matches!(bytes.first(), Some(b'U') | Some(b'L')) && label.len() >= 2 {
        let family = &label[..2];
        if let Some(prefix_color) = rank_family_color(family) {
            let base_letter = &label[1..2];
            return vec![
                (label[..1].to_string(), prefix_color),
                (label[1..].to_string(), rank_letter_color(base_letter)),
            ];
        }
    }
    // Non-prefixed: strip a trailing `+`/`-` to find the base letter.
    let base_letter = label.trim_end_matches(['+', '-']);
    vec![(label.to_string(), rank_letter_color(base_letter))]
}

/// Color for a training failure rate %: green (safe) → yellow → orange → red.
pub(super) fn failure_rate_color(pct: i32) -> (u8, u8, u8) {
    if pct >= 60 {
        (255, 80, 80) // red - dangerous
    } else if pct >= 40 {
        (255, 140, 50) // orange
    } else if pct >= 20 {
        (255, 200, 50) // yellow - caution
    } else {
        (120, 220, 120) // green - safe
    }
}

/// Color for bond/friendship value: blue → green → orange → gold (max).
pub fn bond_color(value: i32) -> (u8, u8, u8) {
    if value >= 100 {
        (255, 200, 50) // Gold - maxed
    } else if value >= 80 {
        (255, 160, 40) // Orange - high
    } else if value >= 40 {
        (100, 220, 100) // Green - medium
    } else {
        (100, 150, 255) // Blue - low
    }
}

/// Colour for an editorial buy-priority tier.
pub(super) fn worth_color(w: memory_reader::Worth) -> egui::Color32 {
    match w {
        memory_reader::Worth::MustBuy => egui::Color32::from_rgb(110, 200, 110),
        memory_reader::Worth::Situational => egui::Color32::from_rgb(230, 200, 90),
        memory_reader::Worth::Optional => egui::Color32::from_rgb(120, 170, 220),
        memory_reader::Worth::Skip => egui::Color32::from_rgb(150, 150, 150),
    }
}

/// Format a number with comma separators.
pub(super) fn format_number(n: i32) -> String {
    if n < 0 {
        return format!("-{}", format_number(-n));
    }
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_reader;

    #[test]
    fn cap_level_thresholds() {
        assert_eq!(cap_level(1200, 0), CapLevel::Normal);
        assert_eq!(cap_level(0, 0), CapLevel::Normal);
        assert_eq!(cap_level(1000, 1200), CapLevel::Normal);
        assert_eq!(cap_level(1079, 1200), CapLevel::Normal);
        assert_eq!(cap_level(1080, 1200), CapLevel::Near);
        assert_eq!(cap_level(1199, 1200), CapLevel::Near);
        assert_eq!(cap_level(1200, 1200), CapLevel::AtCap);
        assert_eq!(cap_level(1300, 1200), CapLevel::AtCap);
    }

    #[test]
    fn stat_rank_color_thresholds() {
        assert_eq!(stat_rank_color(0), egui::Color32::from_rgb(150, 150, 150)); // G
        assert_eq!(stat_rank_color(99), egui::Color32::from_rgb(150, 150, 150)); // G+
        assert_eq!(stat_rank_color(100), egui::Color32::from_rgb(165, 135, 225)); // F
        assert_eq!(stat_rank_color(199), egui::Color32::from_rgb(165, 135, 225)); // F+
        assert_eq!(stat_rank_color(200), egui::Color32::from_rgb(210, 110, 215)); // E
        assert_eq!(stat_rank_color(300), egui::Color32::from_rgb(60, 150, 225)); // D
        assert_eq!(stat_rank_color(400), egui::Color32::from_rgb(110, 205, 80)); // C
        assert_eq!(stat_rank_color(599), egui::Color32::from_rgb(110, 205, 80)); // C+
        assert_eq!(stat_rank_color(600), egui::Color32::from_rgb(245, 105, 150)); // B
        assert_eq!(stat_rank_color(800), egui::Color32::from_rgb(250, 140, 55)); // A
        assert_eq!(stat_rank_color(1000), egui::Color32::from_rgb(250, 195, 70)); // S
        assert_eq!(stat_rank_color(1100), egui::Color32::from_rgb(255, 220, 120)); // SS
        assert_eq!(stat_rank_color(1200), egui::Color32::from_rgb(255, 220, 120)); // SS+
                                                                                   // U-ladder stats recycle the primary palette by base letter.
        assert_eq!(stat_rank_color(1201), egui::Color32::from_rgb(150, 150, 150)); // UG -> G
        assert_eq!(stat_rank_color(1401), egui::Color32::from_rgb(210, 110, 215)); // UE -> E
        assert_eq!(stat_rank_color(1901), egui::Color32::from_rgb(250, 195, 70)); // US -> S
        assert_eq!(stat_rank_color(2000), egui::Color32::from_rgb(250, 195, 70));
        // US9 -> S
    }

    #[test]
    fn rank_badge_segments_split() {
        // Non-prefixed: single segment in the base-letter color.
        let g = rank_badge_segments("G");
        assert_eq!(g.len(), 1);
        assert_eq!(g[0], ("G".to_string(), egui::Color32::from_rgb(150, 150, 150)));

        let cp = rank_badge_segments("C+");
        assert_eq!(cp.len(), 1);
        assert_eq!(cp[0].1, egui::Color32::from_rgb(110, 205, 80)); // C base color

        let ssp = rank_badge_segments("SS+");
        assert_eq!(ssp.len(), 1);
        assert_eq!(ssp[0].1, egui::Color32::from_rgb(255, 220, 120)); // SS base color

        // U-rank: prefix in flat family color, remainder in primary base color.
        let ug3 = rank_badge_segments("UG3");
        assert_eq!(ug3.len(), 2);
        assert_eq!(ug3[0], ("U".to_string(), egui::Color32::from_rgb(90, 245, 244)));
        assert_eq!(ug3[1], ("G3".to_string(), egui::Color32::from_rgb(150, 150, 150)));

        // L-rank with multi-digit sub-level.
        let lf12 = rank_badge_segments("LF12");
        assert_eq!(lf12.len(), 2);
        assert_eq!(lf12[0], ("L".to_string(), egui::Color32::from_rgb(191, 241, 213)));
        assert_eq!(lf12[1], ("F12".to_string(), egui::Color32::from_rgb(165, 135, 225)));
    }

    #[test]
    fn failure_rate_color_thresholds() {
        assert_eq!(failure_rate_color(0), (120, 220, 120));
        assert_eq!(failure_rate_color(19), (120, 220, 120));
        assert_eq!(failure_rate_color(20), (255, 200, 50));
        assert_eq!(failure_rate_color(39), (255, 200, 50));
        assert_eq!(failure_rate_color(40), (255, 140, 50));
        assert_eq!(failure_rate_color(59), (255, 140, 50));
        assert_eq!(failure_rate_color(60), (255, 80, 80));
        assert_eq!(failure_rate_color(100), (255, 80, 80));
    }

    #[test]
    fn bond_color_thresholds() {
        assert_eq!(bond_color(100), (255, 200, 50));
        assert_eq!(bond_color(150), (255, 200, 50));
        assert_eq!(bond_color(80), (255, 160, 40));
        assert_eq!(bond_color(99), (255, 160, 40));
        assert_eq!(bond_color(40), (100, 220, 100));
        assert_eq!(bond_color(79), (100, 220, 100));
        assert_eq!(bond_color(0), (100, 150, 255));
        assert_eq!(bond_color(39), (100, 150, 255));
    }

    #[test]
    fn format_number_basic() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn format_number_negative() {
        assert_eq!(format_number(-1000), "-1,000");
        assert_eq!(format_number(-42), "-42");
    }

    #[test]
    fn mood_labels() {
        assert!(memory_reader::mood_label(5).contains("Great"));
        assert!(memory_reader::mood_label(3).contains("Normal"));
        assert!(memory_reader::mood_label(1).contains("Terrible"));
        assert_eq!(memory_reader::mood_label(0), "???");
    }

    #[test]
    fn motivation_colors_distinct() {
        let colors: Vec<_> = (1..=5).map(memory_reader::motivation_color).collect();
        for i in 0..colors.len() {
            for j in (i + 1)..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }
}
