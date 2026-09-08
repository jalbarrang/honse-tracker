//! Grand Live / Our Grand Concert performance points reader.
//!
//! Path (all ObscuredInt backing fields — safe direct reads):
//! ```text
//! WorkSingleModeData.get_ScenarioLive() -> WorkSingleModeScenarioLive
//!   ._performance (PerformanceData value-type field, inline struct)
//!     fields: Dance, Passion, Vocal, Visual, Mental (each ObscuredInt)
//! ```
//!
//! PerformanceData is a value type (struct) stored inline in the parent object.
//! We read the ObscuredInt fields directly from the parent using field offsets.

use std::ffi::c_void;
use std::sync::Mutex;

use crate::compat::Sdk;

use super::super::il2cpp::{
    call_bool, call_bool_with_i32, call_i32, call_obj, call_obj_with_i32, read_i32_field, read_il2cpp_string,
    read_obj_array, read_obscured_int_array, read_obscured_int_field, resolve_obj_method,
};

/// One value per performance token.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerformanceTokens {
    pub dance: i32,
    pub passion: i32,
    pub vocal: i32,
    pub visual: i32,
    /// Called "Composure" in EN, "Mental" internally.
    pub mental: i32,
}

impl PerformanceTokens {
    /// Token values in the game's display order, for arithmetic against the
    /// song catalogue's `TokenVector` (same order).
    #[must_use]
    pub const fn to_vector(self) -> [i32; 5] {
        [self.dance, self.passion, self.vocal, self.visual, self.mental]
    }

    /// Token values in the game's display order, paired with their EN labels.
    #[must_use]
    pub const fn labelled(&self) -> [(&'static str, i32); 5] {
        [
            ("Dance", self.dance),
            ("Passion", self.passion),
            ("Vocal", self.vocal),
            ("Visual", self.visual),
            ("Composure", self.mental),
        ]
    }
}

/// One square currently offered on the Lessons tree: a technique or a song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrandLiveSquare {
    pub square_id: i32,
    /// Raw TreeSquareInfo.SquareNum, not interpreted as lesson progression.
    /// Source: `il2cpp_classes.txt`, Steam Global 2026-09-08.
    /// The backing field is ObscuredInt; absence remains distinguishable from zero.
    pub square_num: Option<i32>,
    /// The game's own list position (`GetSortId`). Squares are returned in this
    /// order so the panel reads top-to-bottom against the screen behind it.
    pub sort_id: i32,
    /// Localized name, or `None` if the master row or its text did not resolve.
    pub name: Option<String>,
    /// Songs and techniques share the tree; songs also raise the concert.
    pub is_music: bool,
    /// Token cost, assembled from the master row's `PerfType`/`PerfValue` pairs.
    pub cost: PerformanceTokens,
    /// The game's own answer to "can this be taken right now"
    /// (`CanGetTreeSquare`), not a cost comparison of ours.
    pub affordable: bool,
}

/// A song already learned this run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedSong {
    /// `LiveId` — the music id, as stored in `TotalMusicIdArray`.
    pub live_id: i32,
    /// Localized name from `MasterSingleModeLiveSongList.GetWithLiveId`, or
    /// `None` if the master row did not resolve.
    pub name: Option<String>,
}

/// What the game currently permits us to say about the next song unlock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NextSongStatus {
    /// A song is among the three current lesson offers.
    Available,
    /// Purchases observed since the previous song or concert.
    Techniques { bought: u8, required: u8 },
    /// The plugin attached partway through an unlock cycle. The next song offer
    /// or concert boundary gives the observer an honest anchor again.
    #[default]
    Unknown,
    /// Every purchasable song in the catalogue is owned.
    Complete,
}

/// Live performance points for the Grand Live scenario.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrandLivePerformance {
    /// Points banked so far.
    pub tokens: PerformanceTokens,
    /// Per-token ceiling. **Not a constant** — it rises as the run progresses,
    /// so it is read live rather than hardcoded. All-zero means
    /// `GetPerformanceMax` did not resolve and the ceiling is unknown; callers
    /// must not print `0` as a denominator.
    pub caps: PerformanceTokens,
    /// Squares on offer this turn. Empty when the tree is unavailable or the
    /// master data did not resolve.
    pub squares: Vec<GrandLiveSquare>,
    /// Songs already learned, from `TotalMusicIdArray`. This is what turns the
    /// planner from a wish list into a ledger.
    pub owned: Vec<OwnedSong>,
    /// Observed progress toward the game's next song offer.
    pub next_song: NextSongStatus,
}

impl PerformanceTokens {
    /// Per-token shortfall of `self` against `cost`: how much more of each is
    /// needed. Zero where the tokens already cover it.
    #[must_use]
    pub fn shortfall(&self, cost: &Self) -> Self {
        Self {
            dance: (cost.dance - self.dance).max(0),
            passion: (cost.passion - self.passion).max(0),
            vocal: (cost.vocal - self.vocal).max(0),
            visual: (cost.visual - self.visual).max(0),
            mental: (cost.mental - self.mental).max(0),
        }
    }

    /// Whether every token is zero — an empty cost, or nothing missing.
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.dance == 0 && self.passion == 0 && self.vocal == 0 && self.visual == 0 && self.mental == 0
    }

    /// Add `value` to the token named by a master row's `PerfType`
    /// (the same 1..=5 enum the getters use). Unknown types are ignored.
    fn add(&mut self, perf_type: i32, value: i32) {
        match perf_type {
            x if x == PerformanceKind::Dance as i32 => self.dance += value,
            x if x == PerformanceKind::Passion as i32 => self.passion += value,
            x if x == PerformanceKind::Vocal as i32 => self.vocal += value,
            x if x == PerformanceKind::Visual as i32 => self.visual += value,
            x if x == PerformanceKind::Mental as i32 => self.mental += value,
            _ => {}
        }
    }
}

/// `SingleModeScenarioLive.Performance` includes `None = 0`; token slots start at 1.
#[derive(Debug, Clone, Copy)]
#[repr(i32)]
enum PerformanceKind {
    Dance = 1,
    Passion = 2,
    Vocal = 3,
    Visual = 4,
    Mental = 5,
}

fn tokens_from_getter(mut get: impl FnMut(i32) -> i32) -> PerformanceTokens {
    PerformanceTokens {
        dance: get(PerformanceKind::Dance as i32),
        passion: get(PerformanceKind::Passion as i32),
        vocal: get(PerformanceKind::Vocal as i32),
        visual: get(PerformanceKind::Visual as i32),
        mental: get(PerformanceKind::Mental as i32),
    }
}

/// Read Grand Live performance points from `WorkSingleModeData`.
/// Returns `None` if the Grand Live scenario is not active.
///
/// # Safety
/// `wsmd` must be a valid non-null `WorkSingleModeData` IL2CPP object pointer.
pub(in crate::memory_reader) unsafe fn read_performance(wsmd: *mut c_void) -> Option<GrandLivePerformance> {
    unsafe {
        let m_get_live = resolve_obj_method(wsmd, "get_ScenarioLive", 0)?;
        let live = call_obj(wsmd, m_get_live);
        if live.is_null() {
            return None;
        }

        // PerformanceData is a value type. Use GetPerformance(enum) instead,
        // which takes a Performance enum (int-backed) and returns the decrypted i32.
        // None=0, Dance=1, Passion=2, Vocal=3, Visual=4, Mental=5.
        let m_get = resolve_obj_method(live, "GetPerformance", 1)?;

        let fp: extern "C" fn(*mut c_void, i32, *const c_void) -> i32 =
            std::mem::transmute(super::super::il2cpp::method_ptr(m_get));

        let tokens = tokens_from_getter(|kind| fp(live, kind, m_get));

        // The ceiling rises over the run, so read it rather than assume it.
        // Its absence is not fatal: the tokens are still true without it.
        let caps = resolve_obj_method(live, "GetPerformanceMax", 1).map_or_else(PerformanceTokens::default, |m_max| {
            let fp_max: extern "C" fn(*mut c_void, i32, *const c_void) -> i32 =
                std::mem::transmute(super::super::il2cpp::method_ptr(m_max));
            tokens_from_getter(|kind| fp_max(live, kind, m_max))
        });

        let squares = read_squares(live);
        let owned = read_owned(live);

        let mut result = GrandLivePerformance {
            tokens,
            caps,
            squares,
            owned,
            next_song: NextSongStatus::Unknown,
        };
        result.next_song = observe_next_song(&result);
        let next_music_num = resolve_obj_method(live, "get_NextMusicNum", 0).map(|mi| call_i32(live, mi));
        log_on_change(&result, next_music_num);
        Some(result)
    }
}

/// Read the squares currently offered on the Lessons tree.
///
/// Affordability comes from the game's own `CanGetTreeSquare(squareId)` rather
/// than from comparing our cost numbers against our token numbers. The game
/// knows about prerequisites, per-turn limits and anything else we have not
/// modelled; a cost comparison would silently disagree with it.
///
/// # Safety
/// `live` must be a valid non-null `WorkSingleModeScenarioLive`.
unsafe fn read_squares(live: *mut c_void) -> Vec<GrandLiveSquare> {
    unsafe {
        let Some(m_tree) = resolve_obj_method(live, "get_TreeSquareInfoArray", 0) else {
            return Vec::new();
        };
        let array = call_obj(live, m_tree);
        let Some((base, len)) = read_obj_array(array) else {
            return Vec::new();
        };
        // Absent `CanGetTreeSquare`, report the squares without claiming any is
        // affordable — better an under-claim than a wrong one.
        let m_can_get = resolve_obj_method(live, "CanGetTreeSquare", 1);

        let mut out = Vec::with_capacity(len);
        let mut f_square_id: *mut c_void = std::ptr::null_mut();
        let mut f_square_num: Option<*mut c_void> = None;
        let mut m_is_music: Option<*const c_void> = None;
        let mut m_sort_id: Option<*const c_void> = None;

        for i in 0..len {
            let info = *base.add(i);
            if info.is_null() {
                continue;
            }
            if f_square_id.is_null() {
                // SAFETY: IL2CPP object header — klass pointer at offset 0.
                let klass = *(info as *const *mut c_void);
                let Some(field) = Sdk::get().get_field_from_name(klass.cast(), "<SquareId>k__BackingField") else {
                    hlog_warn!("grand_live: TreeSquareInfo.SquareId field not found");
                    return Vec::new();
                };
                f_square_id = field.cast();
                f_square_num = Sdk::get()
                    .get_field_from_name(klass.cast(), "<SquareNum>k__BackingField")
                    .map(|field| field.cast());
                m_is_music = resolve_obj_method(info, "get_IsMusic", 0);
                m_sort_id = resolve_obj_method(info, "GetSortId", 0);
            }
            let square_id = read_obscured_int_field(info, f_square_id);
            if square_id <= 0 {
                continue;
            }
            let (name, cost) = master_square(square_id);
            out.push(GrandLiveSquare {
                square_id,
                square_num: f_square_num.map(|field| read_obscured_int_field(info, field)),
                sort_id: m_sort_id.map_or(i32::MAX, |mi| call_i32(info, mi)),
                name,
                is_music: m_is_music.is_some_and(|mi| call_bool(info, mi)),
                cost,
                affordable: m_can_get.is_some_and(|mi| call_bool_with_i32(live, mi, square_id)),
            });
        }
        // The game's order, so the panel and the screen behind it agree row for
        // row. Reordering by affordability broke that correspondence.
        out.sort_by_key(|s| (s.sort_id, s.square_id));
        out
    }
}

/// Read the songs already learned this run.
///
/// `TotalMusicIdArray` holds `LiveId`s, which
/// `MasterSingleModeLiveSongList.GetWithLiveId` turns into names directly — so
/// a song can be identified without ever having been seen on the tree, which
/// matters because buying one removes it from the offer.
///
/// # Safety
/// `live` must be a valid non-null `WorkSingleModeScenarioLive`.
unsafe fn read_owned(live: *mut c_void) -> Vec<OwnedSong> {
    unsafe {
        let Some(m_total) = resolve_obj_method(live, "get_TotalMusicIdArray", 0) else {
            return Vec::new();
        };
        let array = call_obj(live, m_total);
        if array.is_null() {
            return Vec::new();
        }
        read_obscured_int_array(array)
            .into_iter()
            .filter(|&id| id > 0)
            .map(|live_id| OwnedSong {
                live_id,
                name: music_name(live_id),
            })
            .collect()
    }
}

/// Resolve a `LiveId` to its song name via `MasterSingleModeLiveSongList`.
fn music_name(live_id: i32) -> Option<String> {
    let mdm = super::super::master_string::master_data_manager()?;
    // SAFETY: `mdm` is the live MasterDataManager singleton; each call is a
    // resolved getter on a pointer checked non-null before use.
    unsafe {
        let m_table = resolve_obj_method(mdm, "get_masterSingleModeLiveSongList", 0)?;
        let table = call_obj(mdm, m_table);
        if table.is_null() {
            return None;
        }
        let m_get = resolve_obj_method(table, "GetWithLiveId", 1)?;
        let row = call_obj_with_i32(table, m_get, live_id);
        if row.is_null() {
            return None;
        }
        let m_name = resolve_obj_method(row, "get_MusicName", 0)?;
        let name = call_obj(row, m_name);
        if name.is_null() {
            return None;
        }
        read_il2cpp_string(name).filter(|s| !s.is_empty())
    }
}

/// Look up a square's name and token cost in `MasterSingleModeLiveSquare`.
///
/// The cost is stored as five `(PerfType, PerfValue)` pairs rather than one
/// field per token, so it is assembled rather than read.
fn master_square(square_id: i32) -> (Option<String>, PerformanceTokens) {
    let mut cost = PerformanceTokens::default();
    let Some(mdm) = super::super::master_string::master_data_manager() else {
        return (None, cost);
    };
    // SAFETY: `mdm` is the live MasterDataManager singleton; every call below
    // is a resolved getter on a non-null object, checked before use.
    unsafe {
        let Some(m_table) = resolve_obj_method(mdm, "get_masterSingleModeLiveSquare", 0) else {
            return (None, cost);
        };
        let table = call_obj(mdm, m_table);
        if table.is_null() {
            return (None, cost);
        }
        let Some(m_get) = resolve_obj_method(table, "Get", 1) else {
            return (None, cost);
        };
        let row = call_obj_with_i32(table, m_get, square_id);
        if row.is_null() {
            return (None, cost);
        }
        for slot in 1..=5 {
            let perf_type = read_i32_field(row, &format!("PerfType{slot}"));
            let value = read_i32_field(row, &format!("PerfValue{slot}"));
            if value > 0 {
                cost.add(perf_type, value);
            }
        }
        let name = resolve_obj_method(row, "get_SquareTitleText", 0)
            .map(|mi| call_obj(row, mi))
            .filter(|s| !s.is_null())
            .and_then(|s| read_il2cpp_string(s))
            .filter(|s| !s.is_empty());
        (name, cost)
    }
}

/// GameTora's observed technique requirements (checked 2026-09-08):
/// <https://gametora.com/umamusume/our-grand-concert#lesson-patterns>.
/// The initial prefix is followed by a repeating cycle.
const FIRST_WINDOW_INITIAL: &[u8] = &[1, 2, 3];
const FIRST_WINDOW_LOOP: &[u8] = &[4, 4, 2, 2];
const MIDDLE_WINDOW_INITIAL: &[u8] = &[2, 2, 2];
const MIDDLE_WINDOW_LOOP: &[u8] = &[4, 5, 2, 2];
const FINAL_WINDOW_INITIAL: &[u8] = &[2, 2, 2];
const FINAL_WINDOW_LOOP: &[u8] = &[4, 3, 2, 2];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum OfferKind {
    Technique,
    Song,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Progress {
    Known {
        target: usize,
        techniques: u8,
        carried_song: bool,
    },
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default)]
struct NextSongObserver {
    window: Option<u8>,
    offers: Vec<(i32, bool)>,
    owned_ids: Vec<i32>,
    progress: Progress,
}

fn requirement(window: u8, target: usize) -> Option<u8> {
    let (initial, cycle): (&[u8], &[u8]) = match window {
        1 => (FIRST_WINDOW_INITIAL, FIRST_WINDOW_LOOP),
        2..=4 => (MIDDLE_WINDOW_INITIAL, MIDDLE_WINDOW_LOOP),
        5 => (FINAL_WINDOW_INITIAL, FINAL_WINDOW_LOOP),
        _ => return None,
    };
    if let Some(&value) = initial.get(target) {
        Some(value)
    } else {
        Some(cycle[(target - initial.len()) % cycle.len()])
    }
}

fn offer_kind(squares: &[GrandLiveSquare]) -> OfferKind {
    if squares.is_empty() {
        OfferKind::Other
    } else if squares.iter().all(|square| square.is_music) {
        OfferKind::Song
    } else if squares.iter().all(|square| !square.is_music) {
        OfferKind::Technique
    } else {
        OfferKind::Other
    }
}

fn catalogue_owned_count(performance: &GrandLivePerformance) -> usize {
    performance
        .owned
        .iter()
        .filter_map(|song| song.name.as_deref())
        .filter(|name| crate::song_catalog::song_by_name(name).is_some())
        .count()
}

static NEXT_SONG_OBSERVER: Mutex<NextSongObserver> = Mutex::new(NextSongObserver {
    window: None,
    offers: Vec::new(),
    owned_ids: Vec::new(),
    progress: Progress::Unknown,
});

pub(crate) fn reset_next_song_observer() {
    if let Ok(mut observer) = NEXT_SONG_OBSERVER.lock() {
        *observer = NextSongObserver::default();
    }
}

fn observe_next_song(performance: &GrandLivePerformance) -> NextSongStatus {
    let Ok(mut observer) = NEXT_SONG_OBSERVER.lock() else {
        return NextSongStatus::Unknown;
    };
    update_next_song(&mut observer, performance)
}

fn update_next_song(observer: &mut NextSongObserver, performance: &GrandLivePerformance) -> NextSongStatus {
    let Some(window) = crate::song_catalog::window_for_cap(performance.caps.dance) else {
        return NextSongStatus::Unknown;
    };
    let current_kind = offer_kind(&performance.squares);
    let current_offers: Vec<_> = performance
        .squares
        .iter()
        .map(|square| (square.square_id, square.is_music))
        .collect();
    let mut current_owned: Vec<_> = performance.owned.iter().map(|song| song.live_id).collect();
    current_owned.sort_unstable();
    let all_catalogue_songs_owned = catalogue_owned_count(performance) == crate::song_catalog::SONGS.len();

    let previous_kind = if observer.offers.is_empty() {
        OfferKind::Other
    } else if observer.offers.iter().all(|(_, music)| *music) {
        OfferKind::Song
    } else if observer.offers.iter().all(|(_, music)| !*music) {
        OfferKind::Technique
    } else {
        OfferKind::Other
    };
    let window_changed = observer.window.is_some_and(|previous| previous != window);
    let first_observation = observer.window.is_none();

    if window_changed {
        // A cap increase is the game's concert boundary. A song offer surviving
        // it is a carried offer; buying that song contributes one lesson to the
        // new window's first requirement rather than completing that target.
        observer.progress = Progress::Known {
            target: 0,
            techniques: 0,
            carried_song: current_kind == OfferKind::Song,
        };
    } else if first_observation {
        // Before concert 1, no purchasable songs owned proves which target this
        // is. At any other attach point, claiming zero could under-count work
        // already done before the plugin loaded.
        let bought = catalogue_owned_count(performance);
        observer.progress = if window == 1 && current_kind == OfferKind::Song {
            Progress::Known {
                target: bought,
                techniques: requirement(window, bought).unwrap_or(0),
                carried_song: false,
            }
        } else if window == 1 && bought == 0 {
            Progress::Known {
                target: 0,
                techniques: 0,
                carried_song: false,
            }
        } else {
            Progress::Unknown
        };
    } else if observer.offers != current_offers {
        let song_was_bought = previous_kind == OfferKind::Song && observer.owned_ids != current_owned;
        observer.progress = match observer.progress {
            Progress::Known {
                mut target,
                mut techniques,
                mut carried_song,
            } => {
                if previous_kind == OfferKind::Technique {
                    techniques = techniques.saturating_add(1);
                } else if song_was_bought {
                    if carried_song {
                        techniques = techniques.saturating_add(1);
                        carried_song = false;
                    } else {
                        target = target.saturating_add(1);
                        techniques = 0;
                    }
                }
                Progress::Known {
                    target,
                    techniques,
                    carried_song,
                }
            }
            // An observed song offer is an anchor in concert 1 because every
            // purchasable owned song necessarily belongs to this first segment.
            Progress::Unknown if window == 1 && current_kind == OfferKind::Song => {
                let target = catalogue_owned_count(performance);
                Progress::Known {
                    target,
                    techniques: requirement(window, target).unwrap_or(0),
                    carried_song: false,
                }
            }
            Progress::Unknown => Progress::Unknown,
        };
    }

    observer.window = Some(window);
    observer.offers = current_offers;
    observer.owned_ids = current_owned;

    if all_catalogue_songs_owned {
        return NextSongStatus::Complete;
    }
    if current_kind == OfferKind::Song {
        return NextSongStatus::Available;
    }
    match observer.progress {
        Progress::Known { target, techniques, .. } => {
            requirement(window, target).map_or(NextSongStatus::Unknown, |required| NextSongStatus::Techniques {
                bought: techniques.min(required),
                required,
            })
        }
        Progress::Unknown => NextSongStatus::Unknown,
    }
}

/// Snapshot diagnostics, including offer-only changes. These are observations,
/// not purchase events: multiple purchases between reads may be collapsed.
fn log_on_change(p: &GrandLivePerformance, next_music_num: Option<i32>) {
    static LAST: Mutex<Option<(GrandLivePerformance, Option<i32>)>> = Mutex::new(None);
    if let Ok(mut guard) = LAST.lock() {
        if !update_diagnostic_snapshot(&mut guard, p, next_music_num) {
            return;
        }
    }
    hlog_info!(
        "Grand Live lesson diagnostic: caps={:?} owned_count={} next_music_num={:?} next_song={:?} offers={}",
        p.caps.to_vector(),
        p.owned.len(),
        next_music_num,
        p.next_song,
        p.squares.len()
    );
    for square in &p.squares {
        hlog_info!(
            "Grand Live lesson offer: square_id={} square_num={:?} sort_id={} is_music={} affordable={} cost={:?} name={:?}",
            square.square_id,
            square.square_num,
            square.sort_id,
            square.is_music,
            square.affordable,
            square.cost.to_vector(),
            square.name
        );
    }
    let t = p.tokens;
    hlog_info!(
        "Grand Live performance: Da={} Pa={} Vo={} Vi={} Co={} (cap {}) squares={} affordable={}",
        t.dance,
        t.passion,
        t.vocal,
        t.visual,
        t.mental,
        p.caps.dance,
        p.squares.len(),
        p.squares.iter().filter(|s| s.affordable).count()
    );
    // Named individually: matching these to the catalogue is by name, so an
    // unmatched song shows up here as the thing to compare against.
    for song in &p.owned {
        hlog_info!(
            "Grand Live owned song: live_id={} name={:?}",
            song.live_id,
            song.name.as_deref().unwrap_or("<unresolved>")
        );
    }
}

fn update_diagnostic_snapshot(
    last: &mut Option<(GrandLivePerformance, Option<i32>)>,
    performance: &GrandLivePerformance,
    next_music_num: Option<i32>,
) -> bool {
    if last
        .as_ref()
        .is_some_and(|(p, n)| p == performance && *n == next_music_num)
    {
        return false;
    }
    *last = Some((performance.clone(), next_music_num));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lesson_state(cap: i32, offer_id: i32, music: bool, owned: &[&str]) -> GrandLivePerformance {
        GrandLivePerformance {
            caps: PerformanceTokens {
                dance: cap,
                passion: cap,
                vocal: cap,
                visual: cap,
                mental: cap,
            },
            squares: vec![GrandLiveSquare {
                square_id: offer_id,
                square_num: Some(1),
                sort_id: 1,
                name: None,
                is_music: music,
                cost: PerformanceTokens::default(),
                affordable: true,
            }],
            owned: owned
                .iter()
                .enumerate()
                .map(|(index, name)| OwnedSong {
                    live_id: i32::try_from(index + 1).unwrap_or_default(),
                    name: Some((*name).to_string()),
                })
                .collect(),
            ..GrandLivePerformance::default()
        }
    }

    #[test]
    fn requirements_follow_the_documented_initial_and_looping_patterns() {
        assert_eq!(
            (0..8).map(|i| requirement(1, i)).collect::<Vec<_>>(),
            [Some(1), Some(2), Some(3), Some(4), Some(4), Some(2), Some(2), Some(4),]
        );
        assert_eq!(
            (0..8).map(|i| requirement(2, i)).collect::<Vec<_>>(),
            [Some(2), Some(2), Some(2), Some(4), Some(5), Some(2), Some(2), Some(4),]
        );
        assert_eq!(
            (0..8).map(|i| requirement(5, i)).collect::<Vec<_>>(),
            [Some(2), Some(2), Some(2), Some(4), Some(3), Some(2), Some(2), Some(4),]
        );
        assert_eq!(requirement(6, 0), None);
    }

    #[test]
    fn observer_counts_techniques_between_song_offers() {
        let mut observer = NextSongObserver::default();
        let techniques = lesson_state(200, 10, false, &[]);
        assert_eq!(
            update_next_song(&mut observer, &techniques),
            NextSongStatus::Techniques { bought: 0, required: 1 }
        );

        let songs = lesson_state(200, 20, true, &[]);
        assert_eq!(update_next_song(&mut observer, &songs), NextSongStatus::Available);

        let techniques = lesson_state(200, 30, false, &["Believe in Miracles!"]);
        assert_eq!(
            update_next_song(&mut observer, &techniques),
            NextSongStatus::Techniques { bought: 0, required: 2 }
        );
        let techniques = lesson_state(200, 31, false, &["Believe in Miracles!"]);
        assert_eq!(
            update_next_song(&mut observer, &techniques),
            NextSongStatus::Techniques { bought: 1, required: 2 }
        );
        let songs = lesson_state(200, 40, true, &["Believe in Miracles!"]);
        assert_eq!(update_next_song(&mut observer, &songs), NextSongStatus::Available);
    }

    #[test]
    fn concert_resets_progress_and_a_carried_song_counts_as_one_lesson() {
        let mut observer = NextSongObserver::default();
        let before_concert = lesson_state(200, 10, false, &[]);
        update_next_song(&mut observer, &before_concert);

        let carried_offer = lesson_state(250, 20, true, &[]);
        assert_eq!(
            update_next_song(&mut observer, &carried_offer),
            NextSongStatus::Available
        );
        let after_purchase = lesson_state(250, 30, false, &["Believe in Miracles!"]);
        assert_eq!(
            update_next_song(&mut observer, &after_purchase),
            NextSongStatus::Techniques { bought: 1, required: 2 }
        );
    }

    #[test]
    fn diagnostics_dedupe_but_keep_offer_and_song_changes() {
        let mut last = None;
        let mut p = GrandLivePerformance::default();
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        assert!(!update_diagnostic_snapshot(&mut last, &p, None));
        p.squares.push(GrandLiveSquare {
            square_id: 100,
            square_num: None,
            sort_id: 0,
            name: None,
            is_music: false,
            cost: PerformanceTokens::default(),
            affordable: false,
        });
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        p.squares[0].square_num = Some(0);
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        p.squares[0].square_num = Some(1);
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        p.squares[0].square_id = 101;
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        p.squares[0].is_music = true;
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        p.owned.push(OwnedSong { live_id: 1, name: None });
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        p.owned[0].live_id = 2; // Same count, different song.
        assert!(update_diagnostic_snapshot(&mut last, &p, None));
        assert!(update_diagnostic_snapshot(&mut last, &p, Some(0)));
        assert!(!update_diagnostic_snapshot(&mut last, &p, Some(0)));
        p.caps.dance = 250;
        assert!(update_diagnostic_snapshot(&mut last, &p, Some(0)));
        p.squares.clear();
        assert!(update_diagnostic_snapshot(&mut last, &p, Some(0)));
    }

    #[test]
    fn performance_tokens_use_enum_slots_one_through_five() {
        let mut requested = Vec::new();
        let performance = tokens_from_getter(|kind| {
            requested.push(kind);
            kind * 10
        });

        assert_eq!(requested, vec![1, 2, 3, 4, 5]);
        assert_eq!(performance.dance, 10);
        assert_eq!(performance.passion, 20);
        assert_eq!(performance.vocal, 30);
        assert_eq!(performance.visual, 40);
        assert_eq!(performance.mental, 50);
    }

    #[test]
    fn labels_follow_the_games_display_order() {
        let t = PerformanceTokens {
            dance: 1,
            passion: 2,
            vocal: 3,
            visual: 4,
            mental: 5,
        };
        assert_eq!(
            t.labelled(),
            [
                ("Dance", 1),
                ("Passion", 2),
                ("Vocal", 3),
                ("Visual", 4),
                ("Composure", 5),
            ]
        );
    }
}
