//! Curated Trackblazer Coin Shop item catalog.
//!
//! Game master data only exposes raw `effect_type` / `effect_value_*` ints whose
//! meaning isn't decoded yet, so the human-readable **Effect** string and the
//! editorial **Worth** tier are sourced from the community guide
//! (<https://uma.guide/guides/trackblazer#coin-shop-and-items>), keyed by
//! `item_id` (the shop icon `scenario_free_item_icon_0XXXXX` filename = `item_id`).
//!
//! `item_id`s observed in-game must match these; the on-change shop log prints
//! them for verification. Unlisted items fall back to the raw master value.

/// Editorial buy-priority tier for a shop item (drives a future decision picker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Worth {
    /// Staple purchase whenever affordable.
    MustBuy,
    /// Useful only in specific situations.
    Situational,
    /// Low priority; only with leftover coins.
    Optional,
    /// Generally a trap / waste of coins.
    Skip,
}

impl Worth {
    /// Short label for the UI badge.
    pub fn label(self) -> &'static str {
        match self {
            Worth::MustBuy => "Must Buy",
            Worth::Situational => "Situational",
            Worth::Optional => "Optional",
            Worth::Skip => "Skip",
        }
    }
}

/// Curated effect description + worth tier for a known shop item.
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub effect: &'static str,
    pub worth: Worth,
}

/// Look up the curated catalog entry for an `item_id`, if known.
pub fn lookup(item_id: i32) -> Option<CatalogEntry> {
    use Worth::*;
    let e = |effect, worth| Some(CatalogEntry { effect, worth });
    match item_id {
        // ── Stat items ──
        1201 => e("+15 Specific Stat", MustBuy),
        1101 => e("+7 Specific Stat", MustBuy),
        1001 => e("+3 Specific Stat", Optional),
        // ── Mood items ──
        2302 => e("+2 Mood", MustBuy),
        2301 => e("+1 Mood", MustBuy),
        // ── Energy items ──
        2003 => e("+65 Energy", MustBuy),
        2002 => e("+40 Energy", MustBuy),
        2001 => e("+20 Energy", MustBuy),
        2101 => e("+100 Energy, -1 Mood", MustBuy),
        2202 => e("+8 Max Energy", Skip),
        2201 => e("+5 Energy, +4 Max Energy", Situational),
        // ── Bond items ──
        3101 => e("+5 Bond (all cards)", MustBuy),
        3001 => e("+5 Akikawa Bond", Skip),
        // ── Race items ──
        11002 => e("+35% Race Bonus (1T)", MustBuy),
        11001 => e("+20% Race Bonus (1T)", MustBuy),
        11003 => e("+50% Fan Bonus (1T)", Situational),
        // ── Training-bonus items ──
        8003 => e("+60% Training Bonus (2T)", MustBuy),
        8002 => e("+40% Training Bonus (3T)", Situational),
        8001 => e("+20% Training Bonus (4T)", Skip),
        7001 => e("Shuffle Support Cards", MustBuy),
        9001 => e("+50% Specific Training, +20% Energy cost (1T)", MustBuy),
        10001 => e("No Training fails (1T)", MustBuy),
        // ── Other items ──
        4201 => e("Heal all Conditions", Optional),
        4103 => e("Heal Skin Outbreak", Optional),
        4004 => e("Grants Fast Learner", Situational),
        4001 => e("Grants Charming", Situational),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_items_resolve_with_expected_tier() {
        let train = lookup(8003).expect("8003 in catalog");
        assert_eq!(train.worth, Worth::MustBuy);
        assert_eq!(train.effect, "+60% Training Bonus (2T)");
        assert_eq!(lookup(2202).expect("2202 in catalog").worth, Worth::Skip);
        assert_eq!(lookup(1001).expect("1001 in catalog").worth, Worth::Optional);
    }

    #[test]
    fn unknown_item_is_none() {
        assert!(lookup(999_999).is_none());
    }

    #[test]
    fn worth_labels_are_distinct() {
        let labels = [
            Worth::MustBuy.label(),
            Worth::Situational.label(),
            Worth::Optional.label(),
            Worth::Skip.label(),
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
    }
}
