//! Direct memory reader for career state via IL2CPP singleton chain.
//!
//! Reads character stats, turn info, and career state by walking:
//! ```text
//! WorkDataManager (singleton)
//!   → get_SingleMode() → WorkSingleModeData
//!     → get_Character() → WorkSingleModeCharaData
//!       → get_Speed/Stamina/Power/Guts/Wiz/Hp/MaxHp/FanCount/...()
//! ```
//!
//! All property getters return decrypted values (bypassing ObscuredInt).
//!
//! Organized by concern, re-exported flatly so `memory_reader::*` call sites
//! keep working:
//! - `chain` — IL2CPP resolution, tracking lifecycle, `get_chara_ptr`
//! - `il2cpp` — low-level call/read primitives + `read_list_field`
//! - `snapshot` — `CareerSnapshot` (stats, turns, training levels)
//! - `skills` — acquired skills
//! - `evaluations` — support-card friendship
//! - `presentation` — motivation label/color mapping

use std::sync::atomic::AtomicBool;

mod chain;
mod command_info;
mod eval_master;
mod evaluations;
mod il2cpp;
#[allow(dead_code)]
mod presentation;
mod reserve;
mod scenario;
mod skills;
mod snapshot;
mod story_events;
mod support_deck;

pub use chain::{get_chara_ptr, start_tracking, stop_tracking};
pub use eval_master::probe as probe_eval_master;
pub use evaluations::{read_evaluations, EvaluationInfo};
pub use il2cpp::read_list_field;
pub use presentation::mood_label;
// Only referenced by a unit test now (the Training tab that used it was removed).
#[allow(unused_imports)]
pub use presentation::motivation_color;
pub use reserve::{read_reserved_races, ReservedRace};
pub use scenario::{ScenarioState, TrackblazerOwnedItem, TrackblazerShop, TrackblazerShopItem, Worth};
pub use skills::{read_acquired_skill_list, read_acquired_skills, AcquiredSkillInfo};
pub use snapshot::{read_snapshot, CareerSnapshot};
pub use story_events::{read_fired_events, FiredEvent};
pub use support_deck::read_equipped_support_ids;

/// Whether the memory reader is actively tracking.
pub static TRACKING: AtomicBool = AtomicBool::new(false);
