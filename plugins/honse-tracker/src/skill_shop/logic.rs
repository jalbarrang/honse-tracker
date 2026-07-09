//! Pure shop logic: variant selection, discounts, sorting, and filtering.
//! No IL2CPP access — fully unit-testable.

use crate::skill_shop_prefs::{DistanceFilter, ShopSortMode, SkillShopPrefs, StyleFilter};

use super::SkillShopEntry;

pub fn discount_pct(hint_level: i32, has_kiremono: bool) -> i32 {
    let base = match hint_level {
        0 => 0,
        1 => 10,
        2 => 20,
        3 => 30,
        4 => 35,
        _ => 40,
    };
    base + if has_kiremono { 10 } else { 0 }
}

/// Apply a discount percentage to a base cost, returning the discounted cost.
/// Uses integer division: `base_cost * (100 - discount) / 100`.
pub fn discounted_cost(base_cost: i32, hint_level: i32, has_kiremono: bool) -> i32 {
    let pct = discount_pct(hint_level, has_kiremono);
    base_cost * (100 - pct) / 100
}

pub fn rarity_label(rarity: i32) -> &'static str {
    match rarity {
        1 => "\u{26aa}",  // ⚪
        2 => "\u{1f31f}", // 🌟
        _ => "?",
    }
}

/// A skill candidate from MasterSkillData group expansion (group_rate > 0).
/// This is the pure-data subset of what `read_skill_shop` collects per-group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillCandidate {
    pub skill_id: i32,
    /// 1 = ⚪ white (base), 2 = 🌟 gold (upgrade). Lower must be learned first.
    pub rarity: i32,
    pub group_rate: i32,
}

/// Pick the next skill to buy within a group, enforcing the ⚪→🌟 prerequisite.
///
/// A gold (rarity 2) upgrade requires its white (rarity 1) base to be learned
/// first, so candidates are ordered by `(rarity, group_rate)` ascending and the
/// first **unlearned** one is the next purchase. This means a hinted gold whose
/// white base is not yet owned correctly resolves to the white base. If all are
/// learned, returns the top variant marked learned.
///
/// Returns `(skill_id, is_learned)` or `None` if candidates is empty.
pub fn pick_best_variant(candidates: &[SkillCandidate], learned_ids: &[i32]) -> Option<(i32, bool)> {
    if candidates.is_empty() {
        return None;
    }

    let mut sorted: Vec<&SkillCandidate> = candidates.iter().collect();
    sorted.sort_by_key(|c| (c.rarity, c.group_rate));

    // Pick the lowest (rarity, group_rate) not yet learned: the prerequisite
    // white before the gold upgrade.
    if let Some(pick) = sorted.iter().find(|c| !learned_ids.contains(&c.skill_id)) {
        return Some((pick.skill_id, false));
    }

    // All learned → show the top one
    sorted.last().map(|c| (c.skill_id, true))
}

/// Sort shop entries according to [`ShopSortMode`].
pub fn sort_shop_entries(entries: &mut [SkillShopEntry], mode: ShopSortMode) {
    match mode {
        ShopSortMode::RarityThenName => {
            entries.sort_by(|a, b| b.rarity.cmp(&a.rarity).then(a.name.cmp(&b.name)));
        }
        ShopSortMode::NameOnly => {
            entries.sort_by(|a, b| a.name.cmp(&b.name));
        }
    }
}

/// Whether an entry passes the overlay style/distance filters.
pub fn entry_matches_filters(entry: &SkillShopEntry, style: StyleFilter, distance: DistanceFilter) -> bool {
    let style_tag = style.tag_value();
    let dist_tag = distance.tag_value();
    if style_tag.is_none() && dist_tag.is_none() {
        return true;
    }
    if entry.tags.is_empty() {
        return true;
    }
    let style_ok = style_tag.is_none_or(|t| entry.tags.contains(&t));
    let dist_ok = dist_tag.is_none_or(|t| entry.tags.contains(&t));
    style_ok && dist_ok
}

/// Apply overlay filters and sort (for UI rendering). Learned skills are kept
/// (rendered struck-through for triage); only the style/distance filters prune.
pub fn prepare_entries_for_display(mut entries: Vec<SkillShopEntry>, prefs: &SkillShopPrefs) -> Vec<SkillShopEntry> {
    entries.retain(|e| entry_matches_filters(e, prefs.style_filter, prefs.distance_filter));
    sort_shop_entries(&mut entries, prefs.sort_mode);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- discount_pct ----

    #[test]
    fn discount_pct_levels() {
        assert_eq!(discount_pct(0, false), 0);
        assert_eq!(discount_pct(1, false), 10);
        assert_eq!(discount_pct(2, false), 20);
        assert_eq!(discount_pct(3, false), 30);
        assert_eq!(discount_pct(4, false), 35);
        assert_eq!(discount_pct(5, false), 40);
        assert_eq!(discount_pct(99, false), 40); // clamps at 40
    }

    #[test]
    fn discount_pct_kiremono_adds_10() {
        assert_eq!(discount_pct(0, true), 10);
        assert_eq!(discount_pct(3, true), 40);
        assert_eq!(discount_pct(5, true), 50);
    }

    // ---- discounted_cost ----

    #[test]
    fn discounted_cost_basic() {
        assert_eq!(discounted_cost(100, 0, false), 100);
        assert_eq!(discounted_cost(100, 1, false), 90);
        assert_eq!(discounted_cost(100, 3, true), 60); // 30+10=40% off
        assert_eq!(discounted_cost(170, 2, false), 136); // 170 * 80 / 100
    }

    #[test]
    fn discounted_cost_truncates() {
        // Integer division truncation: 150 * 65 / 100 = 97 (not 97.5)
        assert_eq!(discounted_cost(150, 4, false), 97); // 35% off
    }

    // ---- rarity_label ----

    #[test]
    fn rarity_labels() {
        assert_eq!(rarity_label(1), "\u{26aa}");
        assert_eq!(rarity_label(2), "\u{1f31f}");
        assert_eq!(rarity_label(0), "?");
        assert_eq!(rarity_label(3), "?");
    }

    // ---- pick_best_variant ----

    #[test]
    fn pick_empty_candidates() {
        assert_eq!(pick_best_variant(&[], &[]), None);
    }

    fn cand(skill_id: i32, rarity: i32, group_rate: i32) -> SkillCandidate {
        SkillCandidate {
            skill_id,
            rarity,
            group_rate,
        }
    }

    #[test]
    fn pick_single_unlearned() {
        let cs = [cand(100, 1, 1)];
        assert_eq!(pick_best_variant(&cs, &[]), Some((100, false)));
    }

    #[test]
    fn pick_lowest_group_rate_first() {
        let cs = [cand(200, 1, 2), cand(100, 1, 1), cand(300, 1, 3)];
        // Should pick skill_id=100 (lowest group_rate)
        assert_eq!(pick_best_variant(&cs, &[]), Some((100, false)));
    }

    #[test]
    fn pick_white_before_gold_even_if_gold_hinted() {
        // Group with white (rarity 1) + gold (rarity 2). Gold has lower id but
        // must not be offered until the white base is learned.
        let gold = cand(200601, 2, 1);
        let white = cand(200602, 1, 1);
        let cs = [gold, white];
        // Nothing learned → must pick the white base first.
        assert_eq!(pick_best_variant(&cs, &[]), Some((200602, false)));
        // White learned → gold is now the next purchase.
        assert_eq!(pick_best_variant(&cs, &[200602]), Some((200601, false)));
    }

    #[test]
    fn pick_skips_learned() {
        let cs = [cand(100, 1, 1), cand(200, 2, 2)];
        // 100 is learned, should pick 200
        assert_eq!(pick_best_variant(&cs, &[100]), Some((200, false)));
    }

    #[test]
    fn pick_all_learned_returns_highest() {
        let cs = [cand(100, 1, 1), cand(200, 2, 2)];
        assert_eq!(pick_best_variant(&cs, &[100, 200]), Some((200, true)));
    }

    // ---- sort_shop_entries ----

    fn entry(name: &str, rarity: i32) -> SkillShopEntry {
        SkillShopEntry {
            skill_id: 0,
            group_id: 0,
            rarity,
            hint_level: 0,
            name: name.to_string(),
            base_cost: 0,
            is_learned: false,
            has_hint: true,
            tags: Vec::new(),
            filter_switch: 0,
        }
    }

    #[test]
    fn sort_gold_first_then_alpha() {
        let mut entries = vec![
            entry("Zetsu", 1),
            entry("Alpha", 2),
            entry("Beta", 1),
            entry("Gamma", 2),
        ];
        sort_shop_entries(&mut entries, ShopSortMode::RarityThenName);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Gamma", "Beta", "Zetsu"]);
    }

    #[test]
    fn sort_name_only() {
        let mut entries = vec![entry("Zetsu", 2), entry("Alpha", 1), entry("Beta", 2)];
        sort_shop_entries(&mut entries, ShopSortMode::NameOnly);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "Beta", "Zetsu"]);
    }

    #[test]
    fn filter_style_and_distance() {
        let e = SkillShopEntry {
            skill_id: 1,
            group_id: 1,
            rarity: 1,
            hint_level: 0,
            name: "Test".into(),
            base_cost: 100,
            is_learned: false,
            has_hint: true,
            tags: vec![2, 12],
            filter_switch: 0,
        };
        assert!(entry_matches_filters(&e, StyleFilter::All, DistanceFilter::All));
        assert!(entry_matches_filters(&e, StyleFilter::Senko, DistanceFilter::All));
        assert!(!entry_matches_filters(&e, StyleFilter::Nige, DistanceFilter::All));
        assert!(entry_matches_filters(&e, StyleFilter::Senko, DistanceFilter::Mile));
        assert!(!entry_matches_filters(&e, StyleFilter::Senko, DistanceFilter::Short));
    }

    #[test]
    fn sort_stable_same_rarity() {
        let mut entries = vec![entry("C", 1), entry("A", 1), entry("B", 1)];
        sort_shop_entries(&mut entries, ShopSortMode::RarityThenName);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["A", "B", "C"]);
    }
}
