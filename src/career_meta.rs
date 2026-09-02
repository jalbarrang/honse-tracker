//! The one career presentation helper that needs more than the card id.
//!
//! Everything else — rank badges, stat sprites, the career calendar, condition
//! names — lives in [`honse_career_meta`], shared with the career viewer so the
//! overlay and the viewer cannot disagree. This is the part that stays: the
//! outfit catalogue is a plugin thing, and the shared crate is deliberately
//! free of anything that has to ask the game a question.

use crate::gametora_data;

/// Trainee portrait sprite path for a trained outfit `card_id`, e.g.
/// `chara/chr_icon_1014.png`.
///
/// Asks the outfit catalogue first and falls back to the card id's own leading
/// digits, which is all a reader without a catalogue has. `None` when neither
/// answers.
#[must_use]
#[allow(dead_code)]
pub fn trainee_portrait_path(card_id: i32) -> Option<String> {
    let chara_id = gametora_data::character_card(i64::from(card_id))
        .and_then(|c| c.char_id)
        .filter(|&c| c > 0)
        .or_else(|| honse_career_meta::chara_id_from_card_id(card_id))?;
    Some(format!("chara/chr_icon_{chara_id}.png"))
}
