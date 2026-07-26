//! Overlay UI preferences for the skill shop panel (sort + filters).

use std::sync::Mutex;

/// How shop rows are ordered in the overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShopSortMode {
    /// Gold (rarity 2) first, then A→Z within tier.
    #[default]
    RarityThenName,
    /// Strict A→Z by display name.
    NameOnly,
}

/// Running style filter (matches `RaceDefine.RunningStyle` / common skill tags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StyleFilter {
    #[default]
    All,
    Nige,
    Senko,
    Sashi,
    Oikomi,
}

/// Distance filter (matches `RaceDefine.CourseDistance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DistanceFilter {
    #[default]
    All,
    Short,
    Mile,
    Middle,
    Long,
}

impl StyleFilter {
    pub const LABELS: &'static [(&'static str, StyleFilter)] = &[
        ("All", StyleFilter::All),
        ("Nige", StyleFilter::Nige),
        ("Senko", StyleFilter::Senko),
        ("Sashi", StyleFilter::Sashi),
        ("Oikomi", StyleFilter::Oikomi),
    ];

    /// Primary tag / enum value used when matching [`SkillShopEntry::tags`].
    pub fn tag_value(self) -> Option<i32> {
        match self {
            Self::All => None,
            Self::Nige => Some(1),
            Self::Senko => Some(2),
            Self::Sashi => Some(3),
            Self::Oikomi => Some(4),
        }
    }
}

impl DistanceFilter {
    pub const LABELS: &'static [(&'static str, DistanceFilter)] = &[
        ("All", DistanceFilter::All),
        ("Short", DistanceFilter::Short),
        ("Mile", DistanceFilter::Mile),
        ("Mid", DistanceFilter::Middle),
        ("Long", DistanceFilter::Long),
    ];

    pub fn tag_value(self) -> Option<i32> {
        match self {
            Self::All => None,
            Self::Short => Some(11),
            Self::Mile => Some(12),
            Self::Middle => Some(13),
            Self::Long => Some(14),
        }
    }
}

/// User-controlled skill shop overlay options.
#[derive(Debug, Clone, Default)]
pub struct SkillShopPrefs {
    pub sort_mode: ShopSortMode,
    pub style_filter: StyleFilter,
    pub distance_filter: DistanceFilter,
    /// Include full-price skills captured from the game shop UI (no hint row).
    pub show_hintless: bool,
}

static PREFS: Mutex<SkillShopPrefs> = Mutex::new(SkillShopPrefs {
    sort_mode: ShopSortMode::RarityThenName,
    style_filter: StyleFilter::All,
    distance_filter: DistanceFilter::All,
    show_hintless: false,
});

pub fn prefs() -> SkillShopPrefs {
    PREFS.lock().ok().map(|g| g.clone()).unwrap_or_default()
}

pub fn set_prefs(update: impl FnOnce(&mut SkillShopPrefs)) {
    if let Ok(mut g) = PREFS.lock() {
        update(&mut g);
    }
}

pub fn cycle_sort_mode() {
    set_prefs(|p| {
        p.sort_mode = match p.sort_mode {
            ShopSortMode::RarityThenName => ShopSortMode::NameOnly,
            ShopSortMode::NameOnly => ShopSortMode::RarityThenName,
        };
    });
}

pub fn sort_mode_label(mode: ShopSortMode) -> &'static str {
    match mode {
        ShopSortMode::RarityThenName => "Rarity+Name",
        ShopSortMode::NameOnly => "Name",
    }
}
