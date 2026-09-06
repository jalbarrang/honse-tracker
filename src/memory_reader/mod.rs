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
//! - `idle_training` — Independent Training slot (its own singleton walk)
//! - `il2cpp` — low-level call/read primitives + `read_list_field`
//! - `snapshot` — `CareerSnapshot` (stats, turns, training levels)
//! - `skills` — acquired skills
//! - `evaluations` — support-card friendship
//! - `presentation` — motivation label/color mapping

mod chain;
mod command_info;
mod eval_master;
mod evaluations;
mod idle_training;
mod il2cpp;
mod master_string;
#[allow(dead_code)]
mod presentation;
mod reserve;
mod scenario;
mod skill_points;
mod skill_tips;
mod skills;
mod snapshot;
mod story_events;
mod support_deck;
mod veterans;

pub(crate) use chain::{diag_read_current_turn, ensure_resolved};
pub use chain::{get_chara_ptr, get_skills_chara_ptr};
pub use eval_master::probe as probe_eval_master;
pub use evaluations::{read_evaluations, EvaluationInfo};
pub use idle_training::{read_idle_session, read_trainee_greeting, IdleSession, IdleState, TraineeGreeting};
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
pub(crate) use skill_points::{read_skill_points, read_skill_points_of};
pub use skill_tips::{read_skill_tips, SkillTip};
pub use skills::{read_acquired_skill_list, read_acquired_skills, read_acquired_skills_of, AcquiredSkillInfo};
pub use snapshot::{
    read_light_refresh, read_planner_basics, read_snapshot, CareerSnapshot, LightRefresh, PlannerBasics,
};
pub use story_events::{read_fired_events, FiredEvent};
pub use support_deck::read_equipped_support_ids;
pub use veterans::read_veterans;
