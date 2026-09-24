//! Export the server's Independent Training result as JSON, for analysis later.
//!
//! An idle run's whole outcome arrives in one response — every stat, every
//! skill gained, the race history, the succession factors, the support-card
//! contributions — and then the game shows you a summary screen and throws the
//! rest away. This writes the response to disk before that happens.
//!
//! # Where the hook goes
//!
//! `SingleModeChangeViewManager` fires two callbacks, each taking the
//! deserialised response:
//!
//! ```text
//! <EndIdleSingleMode>b__0(IdleSingleModeEndResponse res)
//! <ResultIdleSingleMode>b__0(IdleSingleModeResultResponse res)
//! ```
//!
//! Both carry the same `data { progress_log_info, end_info }` payload and both
//! return void, so neither has the return-value ABI trap that
//! `command_hooks` documents. They are hooked *after* the original runs, so a
//! failure here can never cost the player their result.
//!
//! # What goes on disk
//!
//! A [`CareerDocument`]: the walked response, verbatim, under an envelope that
//! says when and what wrote it. The shape is the shared crate's, not this
//! module's — the career viewer reads the same type — and so is the file name.
//!
//! # Waiting for the trained character
//!
//! The response says what the run did; it does not say which trained character
//! the run became. That record is created afterwards, and the game announces it
//! to `WorkTrainedCharaData.AddTrainedCharaArray(Gallop.TrainedChara[])` — the
//! server's own `TrainedChara[]`, which carries `trained_chara_id` and the full
//! record. `IdleSingleModeEndResponse` has no such field (`Global` build
//! 2026-09-13: `IdleSingleModeEndResponse.CommonResponse` holds only
//! `progress_log_info` and `end_info`), and `Gallop.TrainedChara` has no
//! `single_mode_chara_id`, so there is no id shared by the two sides to join on.
//!
//! So the walk still happens on the callback thread, where the response is
//! live, but the write waits: the document is held until that hook reports a
//! new veteran, the veteran is matched to the run, and only then is the file
//! written with the veteran in the envelope. A run whose veteran never appears
//! is dropped, which is the deliberate trade — see `docs/idle-career-format.md`.
//!
//! If `WorkTrainedCharaData` cannot be hooked (a game update moved it), there is
//! nothing to wait for, so runs are written immediately as they were before,
//! just without the veteran. Losing every export to a renamed method would be
//! the worse failure.
//!
//! # Finding them without depending on a compiler counter
//!
//! Both live on generated closure classes — `<>c__DisplayClass70_0` and
//! `<>c__DisplayClass71_0` today. Those numbers are assigned by the C#
//! compiler in source order and shift whenever anyone edits the file above
//! them, so binding to them by name would break on a game update in a way that
//! looks like the feature silently switching itself off. Instead the nested
//! types are enumerated and matched on the *method* name, which carries the
//! original method it closed over and is stable.
//!
//! # Cost
//!
//! The walk has to happen on the callback's own thread, because that is where
//! the response is live. Serialising and writing do not, so they are handed to
//! a background thread — the main thread pays for the walk and nothing else.

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use serde_json::Value;

use honse_career_meta::{Callback, CareerDocument, Source};

use crate::compat::{Il2CppObject, MethodInfo, Sdk};
use crate::veterans_export::Veteran;

/// Whether responses are written. Set from config at init.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Where they are written. Resolved once at init.
static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

static ORIG_END: AtomicUsize = AtomicUsize::new(0);
static ORIG_RESULT: AtomicUsize = AtomicUsize::new(0);
/// `WorkTrainedCharaData.AddTrainedCharaArray`'s trampoline.
static ORIG_ADD_TRAINED: AtomicUsize = AtomicUsize::new(0);
/// `WorkTrainedCharaData.Update`'s trampoline.
static ORIG_UPDATE_TRAINED: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);
/// Whether the veteran hooks took, and so whether a finished run may be held
/// back for its trained character. Set once by [`install`].
static VET_HOOKED: AtomicBool = AtomicBool::new(false);

/// The run whose file is waiting for its trained character. At most one: the
/// next callback replaces it, and a second run finishing before the first's
/// veteran appeared is a case the game does not produce.
static PENDING: Mutex<Option<CareerDocument>> = Mutex::new(None);

/// `(this, argument, MethodInfo*)` — the shape of the response callbacks and of
/// the `WorkTrainedCharaData` ones. They differ in what the middle argument
/// points at, not in how it is passed.
type ResponseCallbackFn = extern "C" fn(*mut Il2CppObject, *mut Il2CppObject, *const MethodInfo);

/// Turn exporting on or off. Returns what actually took: asked to turn on with
/// no hooks installed, the answer is no.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled && INSTALLED.load(Ordering::Acquire), Ordering::Release);
    if !is_enabled() {
        // Turning it off drops whatever was waiting; it is not going to be
        // written later by an export that is off.
        PENDING.lock().take();
    }
    hlog_info!(
        target: "training-tracker",
        "Idle career export: {}",
        if is_enabled() { "on" } else { "off" }
    );
}

#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// The directory results are written to, for the menu and the log.
#[must_use]
pub fn output_dir() -> PathBuf {
    DIR.get().cloned().unwrap_or_else(default_dir)
}

/// The user's profile directory, if Windows will say.
fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(PathBuf::from)
}

/// The default export directory — the same rule the career viewer uses to find
/// them, so the two cannot disagree. Falls back to the working directory when
/// there is no profile to hang it off: the export is a convenience, not worth
/// refusing to load over.
fn default_dir() -> PathBuf {
    home().map_or_else(
        || PathBuf::from("idle-careers"),
        |home| honse_career_meta::idle_careers_dir(&home),
    )
}

/// Resolve the output directory from an optional config override.
///
/// A relative override resolves under the user profile rather than the game
/// folder: someone typing `Documents\Runs` means their own Documents, not one
/// inside Program Files that Windows would then refuse to write to.
pub fn configure(enabled: bool, override_dir: Option<&str>) {
    let dir = match override_dir.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => resolve_override(raw, home().as_deref()),
        None => default_dir(),
    };
    let _ = DIR.set(dir);
    set_enabled(enabled);
}

/// Expand `%USERPROFILE%` and anchor a relative path under the profile. Pure,
/// so the rule can be tested without an environment.
fn resolve_override(raw: &str, home: Option<&std::path::Path>) -> PathBuf {
    let expanded = match home.and_then(std::path::Path::to_str) {
        Some(home) => raw.replace("%USERPROFILE%", home),
        None => raw.to_string(),
    };
    let path = PathBuf::from(expanded);
    match (path.is_absolute(), home) {
        (false, Some(home)) => home.join(path),
        _ => path,
    }
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

extern "C" fn on_end(this: *mut Il2CppObject, response: *mut Il2CppObject, method: *const MethodInfo) {
    call_original(&ORIG_END, this, response, method);
    capture(Callback::End, response);
}

extern "C" fn on_result(this: *mut Il2CppObject, response: *mut Il2CppObject, method: *const MethodInfo) {
    call_original(&ORIG_RESULT, this, response, method);
    capture(Callback::Result, response);
}

/// `WorkTrainedCharaData.AddTrainedCharaArray(TrainedChara[])` — the server's
/// new trained characters, handed to the roster the moment the game has them.
///
/// Runs on the Unity main thread, which is where the roster may be read; the
/// original has already added the entries, so the dictionary is consistent.
extern "C" fn on_add_trained(this: *mut Il2CppObject, array: *mut Il2CppObject, method: *const MethodInfo) {
    call_original(&ORIG_ADD_TRAINED, this, array, method);
    // SAFETY: `array` is the `Gallop.TrainedChara[]` the game just passed us, on
    // the thread that owns it, and the original has already returned.
    let ids = unsafe { crate::memory_reader::server_trained_chara_ids(array.cast()) };
    resolve(ids);
}

/// `WorkTrainedCharaData.Update(TrainedChara)` — the single-record spelling of
/// the same event, for a build that updates a veteran instead of adding it.
extern "C" fn on_update_trained(this: *mut Il2CppObject, chara: *mut Il2CppObject, method: *const MethodInfo) {
    call_original(&ORIG_UPDATE_TRAINED, this, chara, method);
    // SAFETY: as in `on_add_trained`.
    let ids = unsafe { crate::memory_reader::server_trained_chara_id(chara.cast()) }
        .into_iter()
        .collect();
    resolve(ids);
}

/// The original runs first, always. Whatever this module does afterwards, the
/// player's result has already been handed to the game.
fn call_original(slot: &AtomicUsize, this: *mut Il2CppObject, response: *mut Il2CppObject, method: *const MethodInfo) {
    let addr = slot.load(Ordering::Acquire);
    if addr == 0 {
        return;
    }
    // SAFETY: `addr` is the trampoline MinHook returned for a method with this
    // exact signature; the arguments are the ones we were called with.
    let original: ResponseCallbackFn = unsafe { std::mem::transmute::<usize, ResponseCallbackFn>(addr) };
    original(this, response, method);
}

/// Walk the response here — it is only live on this thread — and either hold
/// the document for its veteran or hand it to a background thread to write.
fn capture(callback: Callback, response: *mut Il2CppObject) {
    if !is_enabled() || response.is_null() {
        return;
    }
    // SAFETY: `response` is the live argument the game just passed us, on the
    // thread that owns it, and the original has already returned.
    let Some(walked) = (unsafe { crate::il2cpp_json::object_to_json(response.cast()) }) else {
        hlog_warn!(target: "training-tracker", "Idle export: could not read the {} response", callback.as_str());
        return;
    };
    for skipped in &walked.unreadable {
        hlog_warn!(target: "training-tracker", "Idle export: {} unreadable at {}", skipped.reason, skipped.at);
    }
    let document = CareerDocument::capture(
        Source::new(callback, env!("CARGO_PKG_VERSION")),
        chrono::Local::now().fixed_offset(),
        walked.value,
        walked.unreadable,
    );

    // With the veteran hooks in, hold the run until the game creates the trained
    // character it produced, so the file can carry it. Without them there is
    // nothing to wait for, and writing now beats losing the run.
    if VET_HOOKED.load(Ordering::Acquire) {
        let replaced = PENDING.lock().replace(document).is_some();
        hlog_info!(
            target: "training-tracker",
            "Idle export: {} run held for its veteran{}",
            callback.as_str(),
            if replaced { " (replaced an older pending run)" } else { "" }
        );
        if callback == Callback::Result {
            // The log is opened after the run was finalised, so the veteran
            // already exists and no creation hook will fire for it.
            resolve_from_roster();
        }
        return;
    }

    let dir = output_dir();
    std::thread::spawn(move || write(&dir, &document));
}

/// Match the held run against the veterans the game just created, and write the
/// file if one fits.
///
/// Unity main thread (the hooks fire there). A roster update with no run waiting
/// on it is the common case — a login sync, a lock or memo change, a second
/// callback after the run was already written — and costs one lock.
fn resolve(ids: Vec<i32>) {
    if ids.is_empty() {
        return;
    }
    let Some(document) = PENDING.lock().take() else {
        // Proof the hook is alive even when nothing is waiting on it, which is
        // what tells a wrong method name from a quiet roster.
        hlog_info!(target: "training-tracker", "Idle export: {} new veteran(s) with no run waiting", ids.len());
        return;
    };
    let veterans = crate::memory_reader::read_veterans_by_id(&ids);
    finish_with_vet(document, &veterans, &format!("new veteran ids {ids:?}"));
}

/// Match the held run against the whole roster, for a callback that arrives
/// after the veteran already exists.
///
/// The response callback may not be the main thread, so the read is scheduled.
fn resolve_from_roster() {
    if !Sdk::get().schedule_on_main_thread(resolve_from_roster_cb) {
        hlog_warn!(target: "training-tracker", "Idle export: could not reach the main thread to match the veteran");
    }
}

extern "C" fn resolve_from_roster_cb() {
    let Some(document) = PENDING.lock().take() else {
        return;
    };
    let veterans = crate::memory_reader::read_veterans();
    finish_with_vet(document, &veterans, "roster");
}

/// Attach the matching veteran to `document` and write it, or drop the run.
///
/// Dropping is deliberate: a file that claims a run and cannot say which trained
/// character it became is the shape this feature exists to avoid. The trade is
/// recorded in `docs/idle-career-format.md`.
fn finish_with_vet(document: CareerDocument, veterans: &[Veteran], source: &str) {
    let run = RunFacts::from_response(document.response());
    let Some(vet) = veterans
        .iter()
        .filter_map(|vet| {
            let score = run.match_score(vet);
            (score > 0).then_some((score, vet))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, vet)| vet)
    else {
        hlog_warn!(
            target: "training-tracker",
            "Idle export: no veteran matched the {} run held for it; dropping it",
            document.source().callback.as_str()
        );
        return;
    };
    let Ok(vet) = serde_json::to_value(vet) else {
        hlog_error!(target: "training-tracker", "Idle export: cannot serialise the veteran");
        return;
    };
    hlog_info!(
        target: "training-tracker",
        "Idle export: matched trained_chara_id {} from {source}",
        vet.get("trained_chara_id").and_then(Value::as_i64).unwrap_or(0)
    );
    let document = document.with_vet(vet);
    let dir = output_dir();
    std::thread::spawn(move || write(&dir, &document));
}

// ---------------------------------------------------------------------------
// Matching a run to its veteran
// ---------------------------------------------------------------------------

/// What a captured run and a trained character both carry, in one place so the
/// two sides are compared field for field.
struct RunFacts {
    card_id: i64,
    /// `(turn, program_id, weather, ground_condition, running_style, result_rank)`,
    /// in the order the run raced them.
    races: Vec<[i64; 6]>,
    /// Speed, stamina, power, wit, guts — the order `chara_info` and `Veteran`
    /// both use.
    stats: [i64; 5],
    fans: i64,
    skills: Vec<i64>,
    supports: Vec<i64>,
}

impl RunFacts {
    /// Read what the payload offers. A field the game has moved reads as zero or
    /// empty and costs the match its weight, never the run: the payload is the
    /// game's and this module does not get to require its shape.
    fn from_response(response: &Value) -> Self {
        let int = |value: Option<&Value>| value.and_then(Value::as_i64).unwrap_or(0);
        let chara = response.pointer("/data/end_info/chara_info");
        let field = |name: &str| chara.and_then(|c| c.get(name));
        let ids = |name: &str, key: &str| -> Vec<i64> {
            let mut ids: Vec<i64> = chara
                .and_then(|c| c.get(name))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|entry| entry.get(key).and_then(Value::as_i64))
                        .collect()
                })
                .unwrap_or_default();
            ids.sort_unstable();
            ids
        };
        let races = response
            .pointer("/data/progress_log_info/race_history_array")
            .and_then(Value::as_array)
            .map(|entries| entries.iter().filter_map(race_fingerprint).collect())
            .unwrap_or_default();
        Self {
            card_id: int(field("card_id")),
            races,
            stats: [
                int(field("speed")),
                int(field("stamina")),
                int(field("power")),
                int(field("wiz")),
                int(field("guts")),
            ],
            fans: int(field("fans")),
            skills: ids("skill_array", "skill_id"),
            supports: ids("support_card_array", "support_card_id"),
        }
    }

    /// How well one veteran fits this run. `0` means "not this run".
    ///
    /// A recorded race schedule has to agree in full and in order — a missing or
    /// extra race is a different run, not a partial match. Everything else is
    /// weight: the schedule is what tells two runs of the same card apart, so it
    /// alone is decisive and the rest only orders candidates that already agree
    /// on it. A run with no races on either side falls back to card and stats,
    /// which is all there is.
    fn match_score(&self, vet: &Veteran) -> i32 {
        let races = vet_races(vet);
        if !self.races.is_empty() || !races.is_empty() {
            if self.races != races {
                return 0;
            }
        } else if self.card_id == 0 || self.card_id != i64::from(vet.card_id) {
            // No schedule on either side, so the card is the only thing that can
            // say "not this run" at all.
            return 0;
        }
        let mut score = if self.races.is_empty() { 0 } else { 1000 };
        if self.card_id != 0 && self.card_id == i64::from(vet.card_id) {
            score += 10;
        }
        if self.stats == vet_stats(vet) {
            score += 5;
        }
        if self.fans != 0 && self.fans == i64::from(vet.fans) {
            score += 2;
        }
        if !self.skills.is_empty() && self.skills == vet_skills(vet) {
            score += 3;
        }
        if !self.supports.is_empty() && self.supports == vet_supports(vet) {
            score += 1;
        }
        score
    }
}

/// One race from `progress_log_info.race_history_array`, or `None` when the
/// payload no longer has that shape.
fn race_fingerprint(entry: &Value) -> Option<[i64; 6]> {
    let race = entry.get("race_history")?;
    let mut out = [0i64; 6];
    for (slot, key) in out.iter_mut().zip([
        "turn",
        "program_id",
        "weather",
        "ground_condition",
        "running_style",
        "result_rank",
    ]) {
        *slot = race.get(key)?.as_i64()?;
    }
    Some(out)
}

fn vet_races(vet: &Veteran) -> Vec<[i64; 6]> {
    vet.race_result_list
        .iter()
        .map(|r| {
            [
                i64::from(r.turn),
                i64::from(r.program_id),
                i64::from(r.weather),
                i64::from(r.ground_condition),
                i64::from(r.running_style),
                i64::from(r.result_rank),
            ]
        })
        .collect()
}

fn vet_stats(vet: &Veteran) -> [i64; 5] {
    [vet.speed, vet.stamina, vet.power, vet.wiz, vet.guts].map(i64::from)
}

fn vet_skills(vet: &Veteran) -> Vec<i64> {
    let mut ids: Vec<i64> = vet.skill_array.iter().map(|s| i64::from(s.skill_id)).collect();
    ids.sort_unstable();
    ids
}

fn vet_supports(vet: &Veteran) -> Vec<i64> {
    let mut ids: Vec<i64> = vet
        .support_card_list
        .iter()
        .map(|s| i64::from(s.support_card_id))
        .collect();
    ids.sort_unstable();
    ids
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn write(dir: &std::path::Path, document: &CareerDocument) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        hlog_error!(target: "training-tracker", "Idle export: cannot create {}: {e}", dir.display());
        return;
    }
    let path = dir.join(document.file_name());
    let json = match document.to_json() {
        Ok(json) => json,
        Err(e) => {
            hlog_error!(target: "training-tracker", "Idle export: cannot serialise {}: {e}", path.display());
            return;
        }
    };
    match std::fs::write(&path, json) {
        Ok(()) => hlog_info!(target: "training-tracker", "Idle export: saved {}", path.display()),
        Err(e) => hlog_error!(target: "training-tracker", "Idle export: cannot write {}: {e}", path.display()),
    }
}

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

/// Hook both response callbacks, and the roster callbacks a finished run waits
/// on. Idempotent; returns whether the response hooks took.
pub fn install() -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    if !crate::il2cpp_json::is_available() {
        hlog_warn!(target: "training-tracker", "Idle export: IL2CPP reflection exports unavailable");
        return false;
    }
    let sdk = Sdk::get();
    let Some(image) = sdk.get_assembly_image("umamusume.dll") else {
        hlog_warn!(target: "training-tracker", "Idle export: umamusume.dll not found");
        return false;
    };
    let Some(manager) = sdk.get_class(image, "Gallop", "SingleModeChangeViewManager") else {
        hlog_warn!(target: "training-tracker", "Idle export: SingleModeChangeViewManager not found");
        return false;
    };

    let targets: [(&str, &AtomicUsize, *mut c_void); 2] = [
        ("<EndIdleSingleMode>b__0", &ORIG_END, on_end as *mut c_void),
        ("<ResultIdleSingleMode>b__0", &ORIG_RESULT, on_result as *mut c_void),
    ];

    let mut hooked = 0;
    for (name, slot, hook_fn) in targets {
        let Some(addr) = closure_method_addr(manager, name) else {
            hlog_warn!(target: "training-tracker", "Idle export: {name} not found on any nested type");
            continue;
        };
        match sdk.hook(addr, hook_fn) {
            Some(trampoline) => {
                slot.store(trampoline as usize, Ordering::Release);
                hooked += 1;
                hlog_info!(target: "training-tracker", "Idle export: hooked {name}");
            }
            None => hlog_warn!(target: "training-tracker", "Idle export: hook failed for {name}"),
        }
    }

    if hooked == 0 {
        hlog_warn!(target: "training-tracker", "Idle export: no hooks installed");
        return false;
    }

    // The veteran hooks are what let a finished run wait for the trained
    // character it produced. They are separate from the response hooks on
    // purpose: if a game update moves this class, exports keep working (written
    // as soon as they finish, just without a veteran) instead of silently
    // stopping. `AddTrainedCharaArray` is the incremental add the finish flow
    // uses; `Update` is the single-record spelling of the same event.
    let vet_hooked = sdk.get_class(image, "Gallop", "WorkTrainedCharaData").is_some_and(|trained| {
        let mut n = 0;
        let targets: [(&str, &AtomicUsize, *mut c_void); 2] = [
            ("AddTrainedCharaArray", &ORIG_ADD_TRAINED, on_add_trained as *mut c_void),
            ("Update", &ORIG_UPDATE_TRAINED, on_update_trained as *mut c_void),
        ];
        for (name, slot, hook_fn) in targets {
            let Some(addr) = sdk.get_method_addr(trained, name, 1) else {
                hlog_warn!(target: "training-tracker", "Idle export: WorkTrainedCharaData.{name} not found");
                continue;
            };
            match sdk.hook(addr, hook_fn) {
                Some(trampoline) => {
                    slot.store(trampoline as usize, Ordering::Release);
                    n += 1;
                    hlog_info!(target: "training-tracker", "Idle export: hooked WorkTrainedCharaData.{name}");
                }
                None => hlog_warn!(target: "training-tracker", "Idle export: hook failed for WorkTrainedCharaData.{name}"),
            }
        }
        n > 0
    });
    VET_HOOKED.store(vet_hooked, Ordering::Release);
    if !vet_hooked {
        hlog_warn!(
            target: "training-tracker",
            "Idle export: no veteran hooks; runs will be written as soon as they finish, without one"
        );
    }

    INSTALLED.store(true, Ordering::Release);
    true
}

/// Remove every hook. Idempotent.
pub fn uninstall() {
    if !INSTALLED.swap(false, Ordering::AcqRel) {
        return;
    }
    ENABLED.store(false, Ordering::Release);
    VET_HOOKED.store(false, Ordering::Release);
    // A run held for a veteran is not going to get one now, and this is not the
    // moment to write it either.
    PENDING.lock().take();
    let sdk = Sdk::get();
    for (hook_fn, slot) in [
        (on_end as *mut c_void, &ORIG_END),
        (on_result as *mut c_void, &ORIG_RESULT),
        (on_add_trained as *mut c_void, &ORIG_ADD_TRAINED),
        (on_update_trained as *mut c_void, &ORIG_UPDATE_TRAINED),
    ] {
        if slot.swap(0, Ordering::AcqRel) != 0 {
            sdk.unhook(hook_fn);
        }
    }
    hlog_info!(target: "training-tracker", "Idle export: hooks removed");
}

/// Find a generated closure method by name across `parent`'s nested types.
///
/// The closure classes are numbered by the compiler and renumber on any edit to
/// the source file, so the number is not something to bind to. The method name
/// inside them carries the method it closed over and does not move.
fn closure_method_addr(parent: *mut c_void, method: &str) -> Option<*mut c_void> {
    let sdk = Sdk::get();
    // SAFETY: `il2cpp_class_get_nested_types` is an IL2CPP C API export with
    // exactly this signature — `(klass, iter) -> klass`, null at the end.
    let get_nested: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void =
        unsafe { std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_nested_types")?) };

    let mut iter: *mut c_void = std::ptr::null_mut();
    loop {
        // SAFETY: `parent` is a live class and `iter` follows the iteration contract.
        let nested = unsafe { get_nested(parent, &raw mut iter) };
        if nested.is_null() {
            return None;
        }
        if let Some(addr) = sdk.get_method_addr(nested, method, 1) {
            return Some(addr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resolve_override, RunFacts};
    use crate::veterans_export::{RaceResult, Skill, SupportCard, Veteran};
    use serde_json::{json, Value};
    use std::path::{Path, PathBuf};

    /// A relative override means "under my profile", never "under the game
    /// folder" — which on a Steam install is somewhere Windows refuses writes.
    #[test]
    fn a_relative_override_anchors_under_the_profile() {
        let home = Path::new(r"C:\Users\juan");
        assert_eq!(
            resolve_override(r"Documents\Runs", Some(home)),
            PathBuf::from(r"C:\Users\juan\Documents\Runs")
        );
        assert_eq!(
            resolve_override(r"D:\runs", Some(home)),
            PathBuf::from(r"D:\runs"),
            "absolute stays put"
        );
        assert_eq!(
            resolve_override(r"%USERPROFILE%\x", Some(home)),
            PathBuf::from(r"C:\Users\juan\x"),
            "the placeholder expands"
        );
        assert_eq!(
            resolve_override("runs", None),
            PathBuf::from("runs"),
            "no profile: taken as given"
        );
    }

    /// A run's payload with the fields the matcher reads. The schedule is what
    /// tells two runs of one card apart, so it is the parameter that matters.
    fn run_response(card_id: i64, schedule: &[(i64, i64)]) -> Value {
        let races: Vec<Value> = schedule
            .iter()
            .map(|(turn, program_id)| {
                json!({
                    "race_history": {
                        "turn": turn,
                        "program_id": program_id,
                        "weather": 2,
                        "ground_condition": 2,
                        "running_style": 4,
                        "result_rank": 1,
                    }
                })
            })
            .collect();
        json!({
            "data": {
                "end_info": {
                    "chara_info": {
                        "card_id": card_id,
                        "speed": 1017, "stamina": 637, "power": 961, "wiz": 764, "guts": 527,
                        "fans": 621191,
                        "skill_array": [ { "skill_id": 110071, "level": 4 } ],
                        "support_card_array": [ { "support_card_id": 30034 } ],
                    }
                },
                "progress_log_info": { "race_history_array": races }
            }
        })
    }

    /// The trained character a run produced, as the hook hands it over.
    fn vet(card_id: i32, schedule: &[(i32, i32)]) -> Veteran {
        Veteran {
            card_id,
            speed: 1017,
            stamina: 637,
            power: 961,
            wiz: 764,
            guts: 527,
            fans: 621191,
            skill_array: vec![Skill {
                skill_id: 110071,
                level: 4,
            }],
            support_card_list: vec![SupportCard {
                support_card_id: 30034,
                ..SupportCard::default()
            }],
            race_result_list: schedule
                .iter()
                .map(|(turn, program_id)| RaceResult {
                    turn: *turn,
                    program_id: *program_id,
                    weather: 2,
                    ground_condition: 2,
                    running_style: 4,
                    result_rank: 1,
                    ..RaceResult::default()
                })
                .collect(),
            ..Veteran::default()
        }
    }

    /// Two veterans that agree on everything but the races: the schedule picks
    /// the right one, and the other is refused outright.
    #[test]
    fn the_race_schedule_decides_between_two_runs_of_one_card() {
        let run = RunFacts::from_response(&run_response(100702, &[(12, 1068), (24, 1070)]));
        assert!(run.match_score(&vet(100702, &[(12, 1068), (24, 1070)])) > 0);
        assert_eq!(
            run.match_score(&vet(100702, &[(12, 1068), (24, 1071)])),
            0,
            "a different race is a different run"
        );
    }

    /// A schedule has to agree in full: a run that raced one more time is not a
    /// partial match, and a veteran with no races at all is not the run.
    #[test]
    fn a_schedule_that_does_not_agree_in_full_is_a_different_run() {
        let run = RunFacts::from_response(&run_response(100702, &[(12, 1068), (24, 1070)]));
        assert_eq!(run.match_score(&vet(100702, &[(12, 1068)])), 0, "shorter");
        assert_eq!(run.match_score(&vet(100702, &[])), 0, "no races");
    }

    /// With no races on either side there is no schedule to compare, so the card
    /// and the final stats are all there is — and the card is what says no.
    #[test]
    fn without_races_the_card_and_stats_stand_in() {
        let run = RunFacts::from_response(&run_response(100702, &[]));
        assert!(run.match_score(&vet(100702, &[])) > 0);
        assert_eq!(run.match_score(&vet(100701, &[])), 0, "another card's run");
    }
}
