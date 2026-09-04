//! Independent Training state, read from `Gallop.WorkIdleSingleModeData`.
//!
//! Independent Training is the game's `IdleSingleMode` feature: you send a
//! trainee off, a real-world timer runs, and you get the run back when it
//! lands. The screen that plays the montage is view 6600
//! (`SceneDefine.ViewId.IdleSingleModePlayCut`).
//!
//! ```text
//! WorkDataManager (singleton)
//!   → get_IdleSingleModeData() → WorkIdleSingleModeData
//!     → get_State() / get_StartTime() / get_EndTime()
//!     → get_CharaId(), which despite the name is a *card* id (optional)
//! ```
//!
//! # Why this and not the view transition
//!
//! `EndTime` is the same value the in-game gauge counts down
//! (`PartsIdleSingleModeRemainTime.Initialize(totalTime, endTime, …)`), and it
//! is known the moment the session starts. So the plugin can say *when* the
//! training lands rather than only noticing *that* it landed — and it works
//! from the home screen, from another mode, from anywhere, instead of only
//! while the player happens to be watching the montage.
//!
//! # Why this read is allowed outside a settled turn
//!
//! Every other career read walks `WorkSingleModeData → HomeInfo/TurnInfo`,
//! which is torn down and rebuilt across screen transitions — that is the
//! use-after-free the whole lifecycle gate in `read_gate` exists to avoid.
//! This walk touches none of it: one singleton, one long-lived work-data
//! object, three plain property getters, no master-data lookup. It still runs
//! on the Unity main thread (see `crate::idle_training`), just without needing
//! a settled career.
//!
//! That "no master-data lookup" is load-bearing, and was learned the hard way:
//! [`read_trainee_greeting`] lives in this file but is deliberately *not* part
//! of [`read_idle_session`], because calling it on the arming path — a screen
//! transition into view 6600 — took the game down. It is safe on a quiet frame,
//! which is when `idle_training::resolve_greeting_if_safe` calls it.

use std::ffi::c_void;
use std::sync::OnceLock;

use crate::compat::Sdk;

use super::il2cpp::{call_i32, call_i64, call_obj};
use super::master_string;

/// `text_data` category 469: what the trainee says when Independent Training
/// lands, one line per character.
///
/// This is the line the mobile client puts in its push notification, and it
/// ships with the game rather than coming off the server — 125 rows covering
/// every released character with room to spare, plain text with no markup or
/// placeholders, and already in the player's language.
///
/// The number is `text_data.category` read straight out of `master.mdb`, which
/// stores the same integer the `MasterString.Category` enum uses.
const TRAINEE_LINE_CATEGORY: i32 = 469;

/// `text_data` category 6: character display names, same index.
const CHARA_NAME_CATEGORY: i32 = 6;

/// The accessor naming the trainee on the session.
///
/// # It is a card id, whatever the name says
///
/// The game calls this `get_CharaId`, and it returns a *card* id — 101601, not
/// 1016. Confirmed against a live session (build 2026-09-03) and against the
/// export in `idle_export`, whose `chara_info.card_id` carries the same shape.
/// `honse_career_meta::chara_id_from_card_id` takes the four leading digits
/// when the character behind the outfit is what is wanted.
///
/// Bound optionally rather than with the other three getters: the countdown and
/// the notification both work without it, so a rename on a game update should
/// cost the trainee's name and nothing else.
const TRAINEE_CARD_ID_GETTER: &str = "get_CharaId";

/// `WorkIdleSingleModeData.PlayingState`.
///
/// Values are the C# declaration order (the dump carries names, not numbers),
/// so an unrecognised one is kept rather than guessed at — every consumer
/// treats it as "not running", which is the fail-closed answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleState {
    /// No Independent Training on record.
    Idle,
    /// Running. `end_time` is when it lands.
    Playing,
    /// The timer is up and the server has settled the result.
    Finished,
    /// The result log has been read; the session is spent.
    LogChecked,
    /// A value the game grew since this was written.
    Unrecognised(i32),
}

impl IdleState {
    #[must_use]
    const fn from_raw(raw: i32) -> Self {
        match raw {
            0 => Self::Idle,
            1 => Self::Playing,
            2 => Self::Finished,
            3 => Self::LogChecked,
            other => Self::Unrecognised(other),
        }
    }

    /// Short label for the diagnostic panel.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Playing => "running",
            Self::Finished => "finished",
            Self::LogChecked => "collected",
            Self::Unrecognised(_) => "unrecognised",
        }
    }
}

/// What the trainee says when their session lands, and who is saying it.
///
/// `name` is optional and `line` is not: the line is the payload, and a
/// character the name table has not caught up with is still worth quoting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraineeGreeting {
    pub name: Option<String>,
    pub line: String,
}

/// The trainee's completion line for `chara_id`, from the game's own text.
///
/// `chara_id` is a *chara* id (1016), not the card id the session reports —
/// `honse_career_meta::chara_id_from_card_id` converts. `None` when the text
/// table has no line for them, which is the answer for a character the client
/// knows but this table has not been updated for.
///
/// # Not safe on the arming path
///
/// Calling this from the Independent Training poll when a session starts
/// crashed the game (2026-09-03); the same call on the home screen a minute
/// later is fine. It is the moment that matters, not the walk — see
/// `idle_training::resolve_greeting_if_safe`, which waits for a quiet one.
///
/// Caller contract: Unity main thread.
#[must_use]
pub fn read_trainee_greeting(chara_id: i32) -> Option<TraineeGreeting> {
    // SAFETY: master-data reads through resolved IL2CPP metadata; a bad pointer
    // is contained here rather than taking the game down.
    match std::panic::catch_unwind(|| {
        master_string::text(TRAINEE_LINE_CATEGORY, chara_id).map(|line| TraineeGreeting {
            name: master_string::text(CHARA_NAME_CATEGORY, chara_id),
            line,
        })
    }) {
        Ok(greeting) => greeting,
        Err(_) => {
            hlog_error!("read_trainee_greeting PANICKED");
            None
        }
    }
}

/// One observation of the Independent Training slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleSession {
    pub state: IdleState,
    /// Unix second the session was started.
    pub start_time: i64,
    /// Unix second the session lands on. This is what the in-game gauge counts
    /// down to.
    pub end_time: i64,
    /// The trained outfit out on the session — a *card* id, see
    /// [`TRAINEE_CARD_ID_GETTER`]. `None` on a build that dropped the accessor;
    /// the timer works either way, so this never gates anything.
    pub card_id: Option<i32>,
}

struct Resolved {
    wdm_klass: *mut c_void,
    m_get_idle_data: *const c_void,
    m_get_state: *const c_void,
    m_get_start_time: *const c_void,
    m_get_end_time: *const c_void,
    /// [`TRAINEE_CARD_ID_GETTER`], when this build still has it.
    m_get_card_id: Option<*const c_void>,
}

// SAFETY: IL2CPP class/method pointers are stable for the process lifetime.
unsafe impl Send for Resolved {}
// SAFETY: IL2CPP class/method pointers are stable for the process lifetime.
unsafe impl Sync for Resolved {}

static RESOLVED: OnceLock<Resolved> = OnceLock::new();

fn ensure_resolved() -> Option<&'static Resolved> {
    if let Some(resolved) = RESOLVED.get() {
        return Some(resolved);
    }
    let resolved = try_resolve().ok()?;
    let _ = RESOLVED.set(resolved);
    RESOLVED.get()
}

fn try_resolve() -> Result<Resolved, &'static str> {
    let sdk = Sdk::get();
    let Some(image) = sdk.get_assembly_image("umamusume.dll") else {
        return Err("umamusume.dll not found");
    };
    let Some(wdm) = sdk.get_class(image, "Gallop", "WorkDataManager") else {
        return Err("WorkDataManager not found");
    };
    let Some(idle) = sdk.get_class(image, "Gallop", "WorkIdleSingleModeData") else {
        return Err("WorkIdleSingleModeData not found");
    };
    let method = |klass, name: &str| -> Result<*const c_void, &'static str> {
        sdk.get_method(klass, name, 0)
            .map(|m| m.cast::<c_void>())
            .ok_or("WorkIdleSingleModeData accessor not found")
    };

    let card_id_getter = sdk
        .get_method(idle, TRAINEE_CARD_ID_GETTER, 0)
        .map(|m| m.cast::<c_void>());

    let resolved = Resolved {
        wdm_klass: wdm.cast(),
        m_get_idle_data: method(wdm, "get_IdleSingleModeData")?,
        m_get_state: method(idle, "get_State")?,
        m_get_start_time: method(idle, "get_StartTime")?,
        m_get_end_time: method(idle, "get_EndTime")?,
        m_get_card_id: card_id_getter,
    };
    if card_id_getter.is_none() {
        hlog_warn!("Independent Training reader: no {TRAINEE_CARD_ID_GETTER}; trainee will be unnamed");
    }
    hlog_info!("Independent Training reader resolved");
    Ok(resolved)
}

/// Read the current Independent Training slot.
///
/// `None` means the accessors are not resolvable yet (game runtime not up) or
/// the game holds no session object — never "no session in progress", which is
/// [`IdleState::Idle`].
///
/// Caller contract: Unity main thread.
#[must_use]
pub fn read_idle_session() -> Option<IdleSession> {
    // SAFETY: all reads go through resolved IL2CPP metadata onto live objects;
    // a bad pointer is contained here rather than taking the game down.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { inner() })) {
        Ok(session) => session,
        Err(_) => {
            hlog_error!("read_idle_session PANICKED");
            None
        }
    }
}

unsafe fn inner() -> Option<IdleSession> {
    let resolved = ensure_resolved()?;
    let singleton = Sdk::get().get_singleton(resolved.wdm_klass)?.cast::<c_void>();
    // SAFETY: 0-arg getter on the live WorkDataManager singleton.
    let idle = unsafe { call_obj(singleton, resolved.m_get_idle_data) };
    if idle.is_null() {
        return None;
    }
    // `0` is discarded as "nobody out", which is also what an empty slot reads
    // as. Nothing acts on this value; it is displayed and logged.
    let card_id = resolved.m_get_card_id.and_then(|method| {
        // SAFETY: 0-arg Int32 getter resolved on this class, on a live instance.
        let value = unsafe { call_i32(idle, method) };
        (value != 0).then_some(value)
    });

    // SAFETY: three 0-arg property getters on the live WorkIdleSingleModeData.
    unsafe {
        Some(IdleSession {
            state: IdleState::from_raw(call_i32(idle, resolved.m_get_state)),
            start_time: call_i64(idle, resolved.m_get_start_time),
            end_time: call_i64(idle, resolved.m_get_end_time),
            card_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::IdleState;

    #[test]
    fn playing_state_maps_declaration_order() {
        assert_eq!(IdleState::from_raw(0), IdleState::Idle);
        assert_eq!(IdleState::from_raw(1), IdleState::Playing);
        assert_eq!(IdleState::from_raw(2), IdleState::Finished);
        assert_eq!(IdleState::from_raw(3), IdleState::LogChecked);
    }

    /// A value the game grows later must not silently read as a known state —
    /// least of all as `Playing`, which is the one that arms a notification.
    #[test]
    fn unknown_state_is_kept_not_guessed() {
        assert_eq!(IdleState::from_raw(9), IdleState::Unrecognised(9));
        assert_ne!(IdleState::from_raw(9), IdleState::Playing);
    }
}
