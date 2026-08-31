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
//! - `chain` — lazy IL2CPP resolution, `get_chara_ptr`
//! - `il2cpp` — low-level call/read primitives + `read_list_field`
//! - `snapshot` — `CareerSnapshot` (stats, turns, training levels)
//! - `skills` — acquired skills
//! - `evaluations` — support-card friendship
//! - `presentation` — motivation label/color mapping

mod chain;
mod command_info;
mod eval_master;
mod evaluations;
mod il2cpp;
#[allow(dead_code)]
mod presentation;
mod reserve;
mod scenario;
mod skill_points;
mod skills;
mod snapshot;
mod story_events;
mod support_deck;

pub use chain::get_chara_ptr;
pub(crate) use chain::{diag_read_current_turn, ensure_resolved};
pub use eval_master::probe as probe_eval_master;
pub use evaluations::{read_evaluations, EvaluationInfo};
#[allow(unused_imports)]
pub use presentation::mood_label;
// Only referenced by a unit test now (the Training tab that used it was removed).
#[allow(unused_imports)]
pub use presentation::motivation_color;
pub use reserve::{read_reserved_races, ReservedRace};
#[allow(unused_imports)]
pub use scenario::{
    GrandLivePerformance, GrandLiveSquare, PerformanceTokens, ScenarioState, TrackblazerOwnedItem, TrackblazerShop,
    TrackblazerShopItem, Worth,
};
pub(crate) use skill_points::read_skill_points;
pub use skills::{read_acquired_skill_list, read_acquired_skills, AcquiredSkillInfo};
pub use snapshot::{read_light_refresh, read_snapshot, CareerSnapshot, LightRefresh};
pub use story_events::{read_fired_events, FiredEvent};
pub use support_deck::read_equipped_support_ids;
