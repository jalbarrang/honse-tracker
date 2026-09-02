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

use serde_json::Value;

use crate::compat::{Il2CppObject, MethodInfo, Sdk};

/// Whether responses are written. Set from config at init.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Where they are written. Resolved once at init.
static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

static ORIG_END: AtomicUsize = AtomicUsize::new(0);
static ORIG_RESULT: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// `(this, response, MethodInfo*)` — the shape of both callbacks.
type ResponseCallbackFn = extern "C" fn(*mut Il2CppObject, *mut Il2CppObject, *const MethodInfo);

/// Turn exporting on or off. Returns what actually took: asked to turn on with
/// no hooks installed, the answer is no.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled && INSTALLED.load(Ordering::Acquire), Ordering::Release);
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
        || PathBuf::from("SavedIdleCareers"),
        |home| honse_career_meta::saved_careers_dir(&home),
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
    capture("end", response);
}

extern "C" fn on_result(this: *mut Il2CppObject, response: *mut Il2CppObject, method: *const MethodInfo) {
    call_original(&ORIG_RESULT, this, response, method);
    capture("result", response);
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

/// Walk the response here (it is only live on this thread), then hand the JSON
/// to a background thread to serialise and write.
fn capture(source: &'static str, response: *mut Il2CppObject) {
    if !is_enabled() || response.is_null() {
        return;
    }
    // SAFETY: `response` is the live argument the game just passed us, on the
    // thread that owns it, and the original has already returned.
    let Some(value) = (unsafe { crate::il2cpp_json::object_to_json(response.cast()) }) else {
        hlog_warn!(target: "training-tracker", "Idle export: could not read the {source} response");
        return;
    };
    let dir = output_dir();
    std::thread::spawn(move || write(&dir, source, value));
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Field names that identify the account rather than the run. Removed before
/// anything reaches disk, at every depth.
///
/// A folder of exports is for analysis and may well be shared; an account id
/// in each one is not something the player would knowingly hand over, and it
/// contributes nothing to the analysis. Both spellings are listed because the
/// walker normalises the game's `_ownerViewerId` backing fields to camelCase
/// while the response types themselves use snake_case.
const IDENTIFYING_FIELDS: &[&str] = &["viewer_id", "owner_viewer_id", "viewerId", "ownerViewerId"];

/// Strip [`IDENTIFYING_FIELDS`] from every object in the tree.
fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !IDENTIFYING_FIELDS.contains(&key.as_str()));
            map.values_mut().for_each(scrub);
        }
        Value::Array(items) => items.iter_mut().for_each(scrub),
        _ => {}
    }
}

fn write(dir: &std::path::Path, source: &str, mut value: Value) {
    scrub(&mut value);

    // Stamp what produced the file. A folder of these is worth nothing in six
    // months if you cannot tell which plugin version or which callback wrote
    // one, and the game's own payload has nowhere to say so.
    if let Value::Object(map) = &mut value {
        map.insert("honse_source".to_string(), Value::String(source.to_string()));
        map.insert(
            "honse_tracker_version".to_string(),
            Value::String(env!("CARGO_PKG_VERSION").to_string()),
        );
    }

    if let Err(e) = std::fs::create_dir_all(dir) {
        hlog_error!(target: "training-tracker", "Idle export: cannot create {}: {e}", dir.display());
        return;
    }
    let path = dir.join(file_name(source, &value));
    let json = match serde_json::to_string_pretty(&value) {
        Ok(json) => json,
        Err(e) => {
            hlog_error!(target: "training-tracker", "Idle export: cannot serialise the {source} response: {e}");
            return;
        }
    };
    match std::fs::write(&path, json) {
        Ok(()) => hlog_info!(target: "training-tracker", "Idle export: saved {}", path.display()),
        Err(e) => hlog_error!(target: "training-tracker", "Idle export: cannot write {}: {e}", path.display()),
    }
}

/// `20260902_014233-card101302-end.json`.
///
/// Timestamp first so the folder sorts chronologically, then the card so a run
/// is identifiable without opening it. The card id is looked up rather than
/// required: a payload that has moved on still gets written, just with a
/// duller name.
fn file_name(source: &str, value: &Value) -> String {
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    match card_id(value) {
        Some(card) => format!("{stamp}-card{card}-{source}.json"),
        None => format!("{stamp}-{source}.json"),
    }
}

/// `data.end_info.chara_info.card_id`, if the response still has that shape.
fn card_id(value: &Value) -> Option<i64> {
    value
        .get("data")?
        .get("end_info")?
        .get("chara_info")?
        .get("card_id")?
        .as_i64()
}

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

/// Hook both response callbacks. Idempotent; returns whether either took.
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
    INSTALLED.store(true, Ordering::Release);
    true
}

/// Remove both hooks. Idempotent.
pub fn uninstall() {
    if !INSTALLED.swap(false, Ordering::AcqRel) {
        return;
    }
    ENABLED.store(false, Ordering::Release);
    let sdk = Sdk::get();
    for (hook_fn, slot) in [
        (on_end as *mut c_void, &ORIG_END),
        (on_result as *mut c_void, &ORIG_RESULT),
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
    use super::{card_id, file_name, resolve_override, scrub};
    use serde_json::json;
    use std::path::{Path, PathBuf};

    /// The one thing an export must never carry: the account it came from.
    /// Checked at depth, because the id appears six times in a real response
    /// and only one of them is near the top.
    #[test]
    fn the_account_id_is_stripped_at_every_depth() {
        let mut value = json!({
            "owner_viewer_id": 413,
            "data": {
                "end_info": { "chara_info": { "owner_viewer_id": 413, "card_id": 100702 } },
                "list": [ { "viewer_id": 413 }, { "ownerViewerId": 413, "keep": 1 } ]
            }
        });
        scrub(&mut value);
        let text = value.to_string();
        assert!(!text.contains("413"), "{text}");
        assert!(!text.contains("viewer"), "{text}");
        assert_eq!(
            value["data"]["end_info"]["chara_info"]["card_id"], 100702,
            "everything else survives"
        );
        assert_eq!(value["data"]["list"][1]["keep"], 1);
    }

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

    #[test]
    fn the_card_id_names_the_file() {
        let response = json!({ "data": { "end_info": { "chara_info": { "card_id": 101_302 } } } });
        assert_eq!(card_id(&response), Some(101_302));
        assert!(file_name("end", &response).ends_with("-card101302-end.json"));
    }

    /// A payload that has changed shape must still be written. Losing the run
    /// because the filename could not be prettified would be the wrong trade.
    #[test]
    fn a_response_without_a_card_id_still_gets_a_name() {
        for response in [
            json!({}),
            json!({ "data": {} }),
            json!({ "data": { "end_info": null } }),
        ] {
            assert_eq!(card_id(&response), None);
            let name = file_name("result", &response);
            assert!(name.ends_with("-result.json"), "{name}");
            assert!(!name.contains("card"), "{name}");
        }
    }

    /// Timestamp first, so a folder of these sorts chronologically.
    #[test]
    fn names_sort_chronologically() {
        let name = file_name("end", &json!({}));
        let stamp = name.split('-').next().expect("a stamp");
        assert_eq!(stamp.len(), 15, "YYYYMMDD_HHMMSS: {name}");
        assert!(stamp.chars().all(|c| c.is_ascii_digit() || c == '_'), "{name}");
    }
}
