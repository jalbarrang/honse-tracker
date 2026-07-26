//! Self-computed career evaluation (評価点) estimate.
//!
//! The game does not expose the overall evaluation live (it is computed at career
//! result in native code), so we reproduce it — validated to ~0.3% against a real
//! runner. Mirrors the UmaTools model:
//!
//! ```text
//! total = Σ stat_score(stat) + Σ skill_score + unique_bonus
//! skill_score = round(gradeValue × aptitude_multiplier)   // per non-unique skill
//! unique_bonus = uniqueSkillLevel × (170 if star ≥ 3 else 120)
//! ```
//!
//! `gradeValue` + role + unique flag come from [`crate::eval_data`]; the stat curve
//! is the reconstructed "umakonga" per-point formula. Residual error is dominated by
//! the stat curve (the one piece we cannot read from the game).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::eval_data::{self, SkillGrade};
use crate::memory_reader::AcquiredSkillInfo;

/// Race aptitude grades (`RaceDefine.ProperGrade`: Null=0, G=1 … S=8).
#[derive(Debug, Clone, Default)]
pub struct Aptitudes {
    pub dist_short: i32,
    pub dist_mile: i32,
    pub dist_middle: i32,
    pub dist_long: i32,
    pub style_nige: i32,
    pub style_senko: i32,
    pub style_sashi: i32,
    pub style_oikomi: i32,
    pub ground_turf: i32,
    pub ground_dirt: i32,
}

impl Aptitudes {
    /// Grade for a UmaTools role key, or `None` if the key is unknown.
    fn role_grade(&self, key: &str) -> Option<i32> {
        Some(match key {
            "turf" => self.ground_turf,
            "dirt" => self.ground_dirt,
            "sprint" => self.dist_short,
            "mile" => self.dist_mile,
            "medium" => self.dist_middle,
            "long" => self.dist_long,
            "front" => self.style_nige,
            "pace" => self.style_senko,
            "late" => self.style_sashi,
            "end" => self.style_oikomi,
            _ => return None,
        })
    }
}

/// Category a role belongs to (for compound-role grouping).
fn role_category(key: &str) -> u8 {
    match key {
        "turf" | "dirt" => 0,                       // surface
        "sprint" | "mile" | "medium" | "long" => 1, // distance
        "front" | "pace" | "late" | "end" => 2,     // style
        _ => 3,
    }
}

/// Aptitude-grade → score multiplier (UmaTools BUCKET_MULTIPLIER).
fn bucket_multiplier(grade: i32) -> f32 {
    match grade {
        g if g >= 7 => 1.1, // S, A      → good
        5..=6 => 0.9,       // B, C      → average
        2..=4 => 0.8,       // D, E, F   → bad
        1 => 0.7,           // G         → terrible
        _ => 1.0,           // Null/none → base
    }
}

/// Multiplier for a skill's role string (`"front"` or compound `"sprint/front"`).
/// Compound roles take the best multiplier per category, multiplied across categories.
fn role_multiplier(apt: &Aptitudes, role: &str) -> f32 {
    // Best multiplier per category (index 0..3).
    let mut cat_best: [Option<f32>; 4] = [None; 4];
    for part in role.split('/') {
        let Some(grade) = apt.role_grade(part) else {
            continue;
        };
        let cat = role_category(part) as usize;
        let m = bucket_multiplier(grade);
        cat_best[cat] = Some(cat_best[cat].map_or(m, |prev| prev.max(m)));
    }
    let mut factor = 1.0;
    let mut any = false;
    for best in cat_best.into_iter().flatten() {
        factor *= best;
        any = true;
    }
    if any {
        factor
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// Stat scoring curve ("umakonga" per-point formula)
// ---------------------------------------------------------------------------

const MAX_STAT: usize = 2500;

/// Per-point cumulative score table, indexed by stat value 0..=2500.
fn stat_table() -> &'static [i32; MAX_STAT + 1] {
    static TABLE: OnceLock<[i32; MAX_STAT + 1]> = OnceLock::new();
    TABLE.get_or_init(build_stat_table)
}

// The umakonga generator indexes `sc[c]` while `c` also drives block selection;
// an index loop is the clearest faithful port.
#[allow(clippy::needless_range_loop)]
fn build_stat_table() -> [i32; MAX_STAT + 1] {
    // Rates per block; raw accumulates then sc = round(raw / 10).
    const R1: [i32; 25] = [
        5, 8, 10, 13, 16, 18, 21, 24, 26, 28, 29, 30, 31, 33, 34, 35, 39, 41, 42, 43, 52, 55, 66, 68, 68,
    ];
    const R2: [i32; 81] = [
        79, 80, 81, 83, 84, 85, 86, 88, 89, 90, 92, 93, 94, 96, 97, 98, 100, 101, 102, 103, 105, 106, 107, 109, 110,
        111, 113, 114, 115, 117, 118, 119, 121, 122, 123, 124, 126, 127, 128, 130, 131, 132, 134, 135, 136, 138, 139,
        140, 141, 143, 144, 145, 147, 148, 149, 151, 152, 153, 155, 156, 157, 159, 160, 161, 162, 164, 165, 166, 168,
        169, 170, 172, 173, 174, 176, 177, 178, 179, 181, 182, 182,
    ];
    let round10 = |raw: i64| -> i32 { ((raw as f64 / 10.0) + 0.5).floor() as i32 };

    let mut sc = [0i32; MAX_STAT + 1];
    let mut raw: i64 = 0;
    let mut idx = 0usize;
    for c in 1..=1200 {
        if c <= 49 {
            idx = 0;
        } else if c <= 99 {
            idx = 1;
        } else if c % 50 == 0 {
            idx += 1;
        }
        raw += R1[idx] as i64;
        sc[c] = round10(raw);
    }
    raw = 38413;
    idx = 0;
    for c in 1201..=2000 {
        if c <= 1209 {
            idx = 0;
        } else if c <= 1219 {
            idx = 1;
        } else if c % 10 == 0 {
            idx += 1;
        }
        raw += R2[idx] as i64;
        sc[c] = round10(raw);
    }
    raw = 142796;
    idx = 0;
    let mut rate: i64 = 183;
    for c in 2001..=MAX_STAT {
        if idx >= 25 {
            rate += 1;
            idx = 0;
        }
        raw += rate;
        idx += 1;
        sc[c] = round10(raw);
    }
    sc
}

/// Evaluation points contributed by a single stat (clamped 0..=2500).
pub fn stat_score(stat: i32) -> i32 {
    let s = stat.clamp(0, MAX_STAT as i32) as usize;
    stat_table()[s]
}

// ---------------------------------------------------------------------------
// Total evaluation
// ---------------------------------------------------------------------------

/// Breakdown of an evaluation computation.
pub(crate) struct Breakdown {
    pub stat: i32,
    pub skills: i32,
    pub unique: i32,
    pub total: i32,
}

/// Pure evaluation core: total = Σ stat_score + Σ skill_score + unique_bonus.
/// Takes the skill-grade table explicitly so it is testable without the resource file.
pub(crate) fn compute_with(
    table: &HashMap<i32, SkillGrade>,
    stats: [i32; 5],
    apt: &Aptitudes,
    star: i32,
    skills: &[AcquiredSkillInfo],
) -> Breakdown {
    let stat: i32 = stats.iter().map(|&s| stat_score(s)).sum();

    let mut skill_sum = 0i32;
    let mut unique = 0i32;
    for sk in skills {
        let Some(g) = table.get(&sk.master_id) else {
            continue;
        };
        if g.is_unique() {
            let mult = if star >= 3 { 170 } else { 120 };
            unique = sk.level * mult;
            continue;
        }
        let factor = g.r.as_deref().map_or(1.0, |role| role_multiplier(apt, role));
        skill_sum += (g.g as f32 * factor).round() as i32;
    }

    Breakdown {
        stat,
        skills: skill_sum,
        unique,
        total: stat + skill_sum + unique,
    }
}

/// Compute the estimated overall evaluation value, or `None` when the skill-grade
/// resource is unavailable (so the UI can fall back to "—").
pub fn compute(stats: [i32; 5], apt: &Aptitudes, star: i32, skills: &[AcquiredSkillInfo]) -> Option<i32> {
    let table = eval_data::table()?;
    let b = compute_with(table, stats, apt, star, skills);

    // One-time breakdown on the first *settled* frame (skills loaded) so the total
    // can be checked against the game's 評価点 and the unique bonus is confirmed.
    // Frame 1 often has raw stats only (skill list not yet populated); skip it.
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if (b.skills > 0 || b.unique > 0) && !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        hlog_info!(
            target: "training-tracker",
            "evaluation: stat={} skills={} unique_bonus={} (star={}) total={}",
            b.stat,
            b.skills,
            b.unique,
            star,
            b.total
        );
    }

    Some(b.total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_multipliers() {
        assert_eq!(bucket_multiplier(8), 1.1); // S
        assert_eq!(bucket_multiplier(7), 1.1); // A
        assert_eq!(bucket_multiplier(6), 0.9); // B
        assert_eq!(bucket_multiplier(4), 0.8); // D
        assert_eq!(bucket_multiplier(1), 0.7); // G
        assert_eq!(bucket_multiplier(0), 1.0); // Null
    }

    #[test]
    fn stat_curve_anchors() {
        // Matches the ported UmaTools per-point generator.
        assert_eq!(stat_score(0), 0);
        assert_eq!(stat_score(50), 25);
        assert_eq!(stat_score(2500), stat_score(3000)); // clamped
    }

    #[test]
    fn stat_curve_matches_validated_runner() {
        // Veteran runner used to validate the formula (game total 18535 = SS).
        // Per-stat scores from the ported curve must reproduce the 12,897 stat total.
        assert_eq!(stat_score(1195), 3807);
        assert_eq!(stat_score(826), 1910);
        assert_eq!(stat_score(988), 2582);
        assert_eq!(stat_score(564), 1035);
        assert_eq!(stat_score(1159), 3563);
        let total: i32 = [1195, 826, 988, 564, 1159].iter().map(|&s| stat_score(s)).sum();
        assert_eq!(total, 12_897);
    }

    // ---- end-to-end regression against real careers (veterans/*.json) ----

    fn grade(letter: &str) -> i32 {
        match letter {
            "S" => 8,
            "A" => 7,
            "B" => 6,
            "C" => 5,
            "D" => 4,
            "E" => 3,
            "F" => 2,
            "G" => 1,
            _ => 0,
        }
    }

    #[derive(serde::Deserialize)]
    struct VetApt {
        surface: Vec<String>,
        distance: Vec<String>,
        style: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct Veteran {
        speed: i32,
        stamina: i32,
        power: i32,
        guts: i32,
        wisdom: i32,
        star: i32,
        #[serde(rename = "uniqueLevel")]
        unique_level: i32,
        aptitudes: VetApt,
        skills: Vec<String>,
        #[serde(rename = "evaluationScore")]
        evaluation_score: i32,
    }

    /// The computed evaluation must match each real career's ground-truth score exactly.
    /// Guards the stat curve, aptitude buckets, unique bonus, and the skill-grade resource.
    #[test]
    fn validated_runners_match_exactly() {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let resource =
            std::fs::read(format!("{manifest}/assets/skill_grades.json")).expect("skill_grades.json resource present");
        let raw: HashMap<String, SkillGrade> = serde_json::from_slice(&resource).expect("resource parses");
        let table: HashMap<i32, SkillGrade> = raw
            .into_iter()
            .filter_map(|(k, v)| k.parse::<i32>().ok().map(|id| (id, v)))
            .collect();

        let vets_dir = format!("{manifest}/veterans");
        let entries = std::fs::read_dir(&vets_dir).expect("veterans/ dir present");
        let mut checked = 0;
        for entry in entries {
            let path = entry.expect("dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("read veteran fixture");
            let v: Veteran = serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));

            let apt = Aptitudes {
                ground_turf: grade(&v.aptitudes.surface[0]),
                ground_dirt: grade(&v.aptitudes.surface[1]),
                dist_short: grade(&v.aptitudes.distance[0]),
                dist_mile: grade(&v.aptitudes.distance[1]),
                dist_middle: grade(&v.aptitudes.distance[2]),
                dist_long: grade(&v.aptitudes.distance[3]),
                style_nige: grade(&v.aptitudes.style[0]),
                style_senko: grade(&v.aptitudes.style[1]),
                style_sashi: grade(&v.aptitudes.style[2]),
                style_oikomi: grade(&v.aptitudes.style[3]),
            };
            // Build acquired skills; the unique carries its level (others irrelevant).
            let skills: Vec<AcquiredSkillInfo> = v
                .skills
                .iter()
                .filter_map(|s| s.parse::<i32>().ok())
                .map(|id| {
                    let level = if table.get(&id).is_some_and(|g| g.is_unique()) {
                        v.unique_level
                    } else {
                        0
                    };
                    AcquiredSkillInfo {
                        master_id: id,
                        level,
                        name: String::new(),
                    }
                })
                .collect();

            let b = compute_with(
                &table,
                [v.speed, v.stamina, v.power, v.guts, v.wisdom],
                &apt,
                v.star,
                &skills,
            );
            assert_eq!(
                b.total,
                v.evaluation_score,
                "{} expected {} got {} (stat={} skills={} unique={})",
                path.display(),
                v.evaluation_score,
                b.total,
                b.stat,
                b.skills,
                b.unique
            );
            checked += 1;
        }
        assert!(checked >= 2, "expected >=2 veteran fixtures, checked {checked}");
    }

    #[test]
    fn compound_role_all_a_is_product() {
        // All aptitudes A (grade 7) → each category 1.1; two categories → 1.21.
        let apt = Aptitudes {
            dist_short: 7,
            style_nige: 7,
            ..Default::default()
        };
        assert!((role_multiplier(&apt, "sprint/front") - 1.21).abs() < 1e-6);
        assert!((role_multiplier(&apt, "front") - 1.1).abs() < 1e-6);
        assert!((role_multiplier(&apt, "unknownrole") - 1.0).abs() < 1e-6);
    }
}
