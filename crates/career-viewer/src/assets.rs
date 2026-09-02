//! Game art, borrowed from hakuraku's asset folder.
//!
//! # Every lookup is checked
//!
//! The asset sets are incomplete — 139 character icons, 514 support cards, 88
//! skill icons — and the game has far more of each. So nothing here returns a
//! URL it has not confirmed exists on disk. A miss becomes `None` and the page
//! draws a placeholder, rather than a broken-image glyph in the middle of a
//! trainee's portrait.
//!
//! The cost is a `stat` per lookup, on a page that renders a handful of them.

use std::path::{Path, PathBuf};

/// Where the art lives, and the URL prefix it is served under.
pub struct Assets {
    root: PathBuf,
}

impl Assets {
    pub const MOUNT: &'static str = "/assets";

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The big standing portrait for a trained outfit, e.g. card 100702 →
    /// `chara_stand_1007_100702.webp`.
    pub fn portrait(&self, card_id: i64) -> Option<String> {
        let chara = honse_career_meta::chara_id_from_card_id(i32::try_from(card_id).ok()?)?;
        self.url("character_thumbs", &format!("chara_stand_{chara}_{card_id}"))
    }

    /// The small round character icon, for the list.
    pub fn chara_icon(&self, card_id: i64) -> Option<String> {
        let chara = honse_career_meta::chara_id_from_card_id(i32::try_from(card_id).ok()?)?;
        self.url("umamusume_icons", &format!("chr_icon_{chara}"))
    }

    pub fn support_card(&self, support_card_id: i64) -> Option<String> {
        self.url("umamusume_cards", &format!("tex_support_card_{support_card_id}"))
    }

    /// Skill icon, keyed by the skill's `icon_id` from umdb — skills share
    /// icons, and matching on the skill id resolves nothing at all (0 of 714
    /// against the real asset set).
    pub fn skill_icon(&self, icon_id: i64) -> Option<String> {
        self.url("skill_icons", &format!("utx_ico_skill_{icon_id}"))
    }

    /// The stat rank badge for a stat value, via the same table the overlay
    /// uses — so a stat that shows a B badge in game shows a B badge here.
    pub fn stat_rank(&self, value: i64) -> Option<String> {
        // The index is what the overlay and this viewer share; the file it
        // names differs (PNG under `statusrank/` there, webp here).
        let index = honse_career_meta::rank_icon_index(i32::try_from(value).unwrap_or(0));
        self.url("textures/uma_ranks", &format!("utx_ico_statusrank_{index:02}"))
    }

    /// `Some(url)` only when the file is really there.
    fn url(&self, dir: &str, stem: &str) -> Option<String> {
        let name = format!("{stem}.webp");
        self.root
            .join(dir.replace('/', std::path::MAIN_SEPARATOR_STR))
            .join(&name)
            .is_file()
            .then(|| format!("{}/{dir}/{name}", Self::MOUNT))
    }
}

#[cfg(test)]
mod tests {
    use super::Assets;

    /// A root with nothing in it must produce no URLs at all — the guarantee
    /// the templates rely on to never emit a broken image.
    #[test]
    fn a_missing_file_yields_no_url() {
        let assets = Assets::new(std::env::temp_dir().join("honse-assets-that-do-not-exist"));
        assert!(assets.portrait(100_702).is_none());
        assert!(assets.support_card(30_034).is_none());
        assert!(assets.skill_icon(10_011).is_none());
        assert!(assets.stat_rank(1005).is_none());
    }

    /// A card id too short to carry a character id must not be turned into a
    /// lookup for a nonsense file.
    #[test]
    fn an_unusable_card_id_yields_no_url() {
        let assets = Assets::new(std::env::temp_dir());
        assert!(assets.portrait(12).is_none());
        assert!(assets.chara_icon(-1).is_none());
    }
}
