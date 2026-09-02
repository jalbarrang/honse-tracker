//! Game art, hotlinked from hakuraku.moe.
//!
//! The site publishes its `public/` folder at the root, so the same paths a
//! local checkout has — `assets/character_thumbs/chara_stand_1007_100702.webp`
//! and so on — exist as URLs. Nothing is copied, served or checked here: each
//! method only builds the address and the browser fetches it.
//!
//! # Misses are the browser's problem
//!
//! The asset set is incomplete, and a server cannot cheaply ask a remote host
//! whether a file exists before rendering every page. So every lookup returns a
//! URL and the templates handle the miss with an `onerror` attribute: the big
//! images turn into the blank placeholder, the small badges remove themselves.
//! The `alt=""` on each one keeps browsers from drawing a broken-image glyph
//! in the meantime.
//!
//! Only the two character lookups can still fail here, when the card id is too
//! short to carry a character id.

/// The host the art comes from, without a trailing slash.
pub struct Assets {
    base: String,
}

impl Assets {
    pub const DEFAULT_BASE: &'static str = "https://hakuraku.moe";

    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The big standing portrait for a trained outfit, e.g. card 100702 →
    /// `chara_stand_1007_100702.webp`.
    pub fn portrait(&self, card_id: i64) -> Option<String> {
        let chara = honse_career_meta::chara_id_from_card_id(i32::try_from(card_id).ok()?)?;
        Some(self.url("character_thumbs", &format!("chara_stand_{chara}_{card_id}")))
    }

    /// The small round character icon, for the list.
    pub fn chara_icon(&self, card_id: i64) -> Option<String> {
        let chara = honse_career_meta::chara_id_from_card_id(i32::try_from(card_id).ok()?)?;
        Some(self.url("umamusume_icons", &format!("chr_icon_{chara}")))
    }

    pub fn support_card(&self, support_card_id: i64) -> String {
        self.url("umamusume_cards", &format!("tex_support_card_{support_card_id}"))
    }

    /// Skill icon, keyed by the skill's `icon_id` from umdb — skills share
    /// icons, and matching on the skill id resolves nothing at all (0 of 714
    /// against the real asset set).
    pub fn skill_icon(&self, icon_id: i64) -> String {
        self.url("skill_icons", &format!("utx_ico_skill_{icon_id}"))
    }

    /// The stat rank badge for a stat value, via the same table the overlay
    /// uses — so a stat that shows a B badge in game shows a B badge here.
    pub fn stat_rank(&self, value: i64) -> String {
        // The index is what the overlay and this viewer share; the file it
        // names differs (PNG under `statusrank/` there, webp here).
        let index = honse_career_meta::rank_icon_index(i32::try_from(value).unwrap_or(0));
        self.url("textures/uma_ranks", &format!("utx_ico_statusrank_{index:02}"))
    }

    fn url(&self, dir: &str, stem: &str) -> String {
        format!("{}/assets/{dir}/{stem}.webp", self.base)
    }
}

#[cfg(test)]
mod tests {
    use super::Assets;

    /// The paths must match what hakuraku.moe actually serves, which is its
    /// `public/` folder at the root. These five were checked against the site.
    #[test]
    fn urls_follow_the_site_layout() {
        let assets = Assets::new("https://hakuraku.moe");
        assert_eq!(
            assets.portrait(100_702).as_deref(),
            Some("https://hakuraku.moe/assets/character_thumbs/chara_stand_1007_100702.webp")
        );
        assert_eq!(
            assets.chara_icon(100_702).as_deref(),
            Some("https://hakuraku.moe/assets/umamusume_icons/chr_icon_1007.webp")
        );
        assert_eq!(
            assets.support_card(30_034),
            "https://hakuraku.moe/assets/umamusume_cards/tex_support_card_30034.webp"
        );
        assert_eq!(
            assets.skill_icon(10_011),
            "https://hakuraku.moe/assets/skill_icons/utx_ico_skill_10011.webp"
        );
        assert_eq!(
            assets.stat_rank(1005),
            "https://hakuraku.moe/assets/textures/uma_ranks/utx_ico_statusrank_14.webp"
        );
    }

    /// A pasted base with a trailing slash must not produce `//assets`.
    #[test]
    fn a_trailing_slash_is_tolerated() {
        assert_eq!(
            Assets::new("http://localhost:8080/").skill_icon(1),
            "http://localhost:8080/assets/skill_icons/utx_ico_skill_1.webp"
        );
    }

    /// A card id too short to carry a character id must not be turned into a
    /// URL for a nonsense file.
    #[test]
    fn an_unusable_card_id_yields_no_url() {
        let assets = Assets::new(Assets::DEFAULT_BASE);
        assert!(assets.portrait(12).is_none());
        assert!(assets.chara_icon(-1).is_none());
    }
}
