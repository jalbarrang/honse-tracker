//! Career presentation tables: how the game's raw numbers become labels,
//! sprites and dates.
//!
//! # Why these live in their own crate
//!
//! Two things need them and they must not disagree: the in-game overlay, and
//! the career viewer that reads the plugin's exported JSON. They were the
//! plugin's own modules first, and the viewer began life with a hand-written
//! copy in another language — which is a table that drifts the first time a
//! condition id is added, silently, with nothing to catch it.
//!
//! Sharing them requires this to be its own crate rather than a module of the
//! plugin: `honse-tracker` pulls in the SDK, the overlay and the Windows
//! graphics stack, and a command-line tool has no business linking D3D11 to
//! look up a rank badge.
//!
//! # What belongs here
//!
//! Pure lookups over data the game already gave us. No IL2CPP, no I/O, no
//! platform. If a helper needs to ask the game or the catalogue something, it
//! belongs in the plugin — see `career_meta::chara_id_from_card_id`, which is
//! the fallback half of a lookup whose better half needs the outfit catalogue.

pub mod career_document;
pub mod career_meta;
pub mod chara_effects;
pub mod paths;
pub mod rank_table;

pub use career_document::{Callback, CareerDocument, FormatError, Source, Unreadable};
pub use career_meta::{
    chara_id_from_card_id, rank_icon_index, rank_label_sprite, stat_icon_path, stat_rank_sprite, turn_date,
};
pub use chara_effects::{is_known, lookup, Polarity};
pub use paths::saved_careers_dir;
pub use rank_table::rank_label;
