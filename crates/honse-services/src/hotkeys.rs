//! Hotkey registry with GetAsyncKeyState polling (global, focus-independent).
//!
//! Fork reference: `hachimi-redux/.../plugin/hotkeys.rs` fires from a global
//! WndProc hook. Edge has no WndProc plugin hook, so polling host input state
//! would only see keys while a host surface has focus. Replacement: poll
//! `GetAsyncKeyState` for registered VK chords inside the present-callback job
//! list (t-001), edge-triggered on down-transitions, gated on the game window
//! being foreground.
//!
//! Non-Windows builds compile the registry but never fire (test-friendly).

use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::next_handle;

/// Modifier bit: Ctrl held.
pub const MOD_CTRL: u8 = 1 << 0;
/// Modifier bit: Shift held.
pub const MOD_SHIFT: u8 = 1 << 1;
/// Modifier bit: Alt held.
pub const MOD_ALT: u8 = 1 << 2;

/// The leader every overlay binding is built on: **Ctrl+Shift**.
///
/// The game has text fields and its own shortcuts, so an overlay chord must
/// never be something a player could type. Two constraints pick this:
///
/// - a bare key, or Shift alone, types characters — excluded by
///   [`register_hotkey`]'s modifier requirement;
/// - **Ctrl+Alt is AltGr on Windows.** On non-US layouts AltGr is how you type
///   everyday characters — on a Spanish keyboard `AltGr+2` is `@` — so a
///   Ctrl+Alt chord fires while the player types perfectly ordinary text.
///
/// Ctrl+Shift produces no characters on any layout, which leaves it free.
pub const MOD_OVERLAY: u8 = MOD_CTRL | MOD_SHIFT;

/// A key combination: modifier bits + primary virtual-key code.
/// `vk == 0` means "unbound" and never matches.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Chord {
    pub mods: u8,
    pub vk: u16,
}

impl Chord {
    #[must_use]
    pub fn new(mods: u8, vk: u16) -> Self {
        Self { mods, vk }
    }

    #[must_use]
    pub fn is_bound(self) -> bool {
        self.vk != 0
    }

    #[must_use]
    pub fn matches(self, other: Chord) -> bool {
        self.is_bound() && self.vk == other.vk && self.mods == other.mods
    }
}

/// Edge-trigger: fire once on the down-transition, not while held.
#[must_use]
pub fn should_fire(was_down: bool, is_down: bool) -> bool {
    !was_down && is_down
}

/// Compat `GuiMenuCallback` shape: `extern "C" fn(userdata)`.
pub type HotkeyCallback = extern "C" fn(userdata: *mut c_void);

/// Frames a repeating chord must be held before it starts repeating, and the
/// gap between repeats after that. Counted in present ticks rather than
/// milliseconds so the poll needs no clock — at 60fps this is roughly a
/// 0.4s delay then 15 repeats a second, which matches typical key repeat.
const REPEAT_DELAY_FRAMES: u32 = 24;
const REPEAT_EVERY_FRAMES: u32 = 4;

struct Registration {
    handle: u64,
    id: String,
    #[allow(dead_code)]
    label: String,
    chord: Chord,
    callback: HotkeyCallback,
    userdata: usize,
    was_down: bool,
    /// Whether holding the chord fires repeatedly. Off by default: a toggle
    /// that repeats is a toggle that flickers.
    repeat: bool,
    held_frames: u32,
}

impl Registration {
    /// Whether this frame should fire, given the chord's current state.
    /// Advances the hold counter as a side effect.
    fn tick(&mut self, is_down: bool) -> bool {
        if !is_down {
            self.was_down = false;
            self.held_frames = 0;
            return false;
        }
        let first = !self.was_down;
        self.was_down = true;
        if first {
            self.held_frames = 0;
            return true;
        }
        if !self.repeat {
            return false;
        }
        self.held_frames += 1;
        self.held_frames >= REPEAT_DELAY_FRAMES && (self.held_frames - REPEAT_DELAY_FRAMES).is_multiple_of(REPEAT_EVERY_FRAMES)
    }
}

static HOTKEYS: Lazy<Mutex<Vec<Registration>>> = Lazy::new(|| Mutex::new(Vec::new()));
static POLL_INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a modifier set is safe to bind against a game with text fields.
///
/// Requires Ctrl or Alt. A bare key, or Shift alone, is a character the player
/// could be typing — `Shift+A` is just `A`. This is a floor, not a
/// recommendation: prefer [`MOD_OVERLAY`], which also dodges AltGr.
#[must_use]
pub const fn mods_are_typable(mods: u8) -> bool {
    mods & (MOD_CTRL | MOD_ALT) == 0
}

/// Register a hotkey matching compat `Sdk::register_hotkey`.
///
/// Re-registering the same `id` replaces the old entry (fork semantics).
/// Returns a non-zero handle, or 0 if `id` is empty or the chord is one the
/// player could type — see [`mods_are_typable`]. Refusing is deliberate: a
/// binding that steals keystrokes from an in-game text field is worse than no
/// binding, and silently registering it would surface as the game "eating"
/// characters with no obvious cause.
pub fn register_hotkey(
    id: &str,
    label: &str,
    default_mods: u8,
    default_vk: u16,
    callback: HotkeyCallback,
    userdata: *mut c_void,
) -> u64 {
    if id.is_empty() {
        return 0;
    }
    if default_vk != 0 && mods_are_typable(default_mods) {
        log::error!(
            "honse-services: refusing hotkey '{id}' — mods {default_mods:#04b} need Ctrl or Alt, \
             or the player cannot type"
        );
        return 0;
    }
    install_poll_job();
    let mut hotkeys = HOTKEYS.lock();
    hotkeys.retain(|h| h.id != id);
    let handle = next_handle();
    hotkeys.push(Registration {
        handle,
        id: id.to_owned(),
        label: label.to_owned(),
        chord: Chord::new(default_mods, default_vk),
        callback,
        userdata: userdata as usize,
        was_down: false,
        repeat: false,
        held_frames: 0,
    });
    handle
}

/// Make a registered chord fire repeatedly while held.
///
/// For nudging things — a cursor, a panel position — where one press per step
/// would mean fifty presses to cross the screen. Never for toggles.
pub fn set_repeat(handle: u64, repeat: bool) -> bool {
    let mut hotkeys = HOTKEYS.lock();
    match hotkeys.iter_mut().find(|h| h.handle == handle) {
        Some(h) => {
            h.repeat = repeat;
            true
        }
        None => false,
    }
}

/// Remove a hotkey by handle.
pub fn unregister(handle: u64) -> bool {
    let mut hotkeys = HOTKEYS.lock();
    let before = hotkeys.len();
    hotkeys.retain(|h| h.handle != handle);
    hotkeys.len() != before
}

fn install_poll_job() {
    if POLL_INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    crate::frame::register_frame_job(Box::new(|| {
        poll_hotkeys(platform_key_down, platform_is_foreground);
    }));
}

/// Pure chord matcher given a key-state reader (`vk -> currently down`).
#[must_use]
pub fn chord_is_down(chord: Chord, key_down: &dyn Fn(u16) -> bool) -> bool {
    if !chord.is_bound() {
        return false;
    }
    if !key_down(chord.vk) {
        return false;
    }
    let ctrl = key_down(vk_control());
    let shift = key_down(vk_shift());
    let alt = key_down(vk_menu());
    let mods = (u8::from(ctrl) * MOD_CTRL) | (u8::from(shift) * MOD_SHIFT) | (u8::from(alt) * MOD_ALT);
    mods == chord.mods
}

fn vk_control() -> u16 {
    0x11 // VK_CONTROL
}
fn vk_shift() -> u16 {
    0x10 // VK_SHIFT
}
fn vk_menu() -> u16 {
    0x12 // VK_MENU (Alt)
}

/// Poll all registrations: edge-trigger + foreground gate.
///
/// `key_down` / `is_foreground` are injected so unit tests run without Win32.
pub fn poll_hotkeys(key_down: impl Fn(u16) -> bool, is_foreground: impl Fn() -> bool) {
    if !is_foreground() {
        // Reset edge state so a chord held while unfocused doesn't fire on refocus.
        for h in HOTKEYS.lock().iter_mut() {
            h.was_down = false;
            h.held_frames = 0;
        }
        return;
    }

    let mut to_fire: Vec<(HotkeyCallback, usize)> = Vec::new();
    {
        let mut hotkeys = HOTKEYS.lock();
        for h in hotkeys.iter_mut() {
            let is_down = chord_is_down(h.chord, &key_down);
            if h.tick(is_down) {
                to_fire.push((h.callback, h.userdata));
            }
        }
    }
    for (callback, userdata) in to_fire {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            callback(userdata as *mut c_void);
        }));
    }
}

#[cfg(windows)]
fn platform_key_down(vk: u16) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    // High bit set ⇒ key currently down.
    // SAFETY: GetAsyncKeyState is always safe to call with a VK code.
    unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
}

#[cfg(not(windows))]
fn platform_key_down(_vk: u16) -> bool {
    false
}

#[cfg(windows)]
fn platform_is_foreground() -> bool {
    use windows::Win32::{
        Foundation::HWND,
        System::Threading::GetCurrentProcessId,
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };
    // SAFETY: Win32 foreground/pid queries; null HWND is handled.
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        pid == GetCurrentProcessId()
    }
}

#[cfg(not(windows))]
fn platform_is_foreground() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crate::TEST_LOCK;

    static HITS: AtomicU32 = AtomicU32::new(0);

    extern "C" fn count(_u: *mut c_void) {
        HITS.fetch_add(1, Ordering::Relaxed);
    }

    fn clear() {
        HOTKEYS.lock().clear();
    }

    #[test]
    fn should_fire_truth_table() {
        assert!(!should_fire(false, false));
        assert!(should_fire(false, true));
        assert!(!should_fire(true, true));
        assert!(!should_fire(true, false));
    }

    #[test]
    fn chord_match_with_synthetic_key_state() {
        let chord = Chord::new(MOD_CTRL, 0x70); // Ctrl+F1
        let down = |vk: u16| matches!(vk, 0x70 | 0x11);
        assert!(chord_is_down(chord, &down));
        let no_ctrl = |vk: u16| vk == 0x70;
        assert!(!chord_is_down(chord, &no_ctrl));
        let wrong_key = |vk: u16| matches!(vk, 0x71 | 0x11);
        assert!(!chord_is_down(chord, &wrong_key));
        assert!(!chord_is_down(Chord::new(0, 0), &down));
    }

    #[test]
    fn register_fire_once_across_held_frames_then_unregister() {
        let _guard = TEST_LOCK.lock();
        clear();
        HITS.store(0, Ordering::Relaxed);

        let h = register_hotkey("test.f1", "F1", 0, 0x70, count, std::ptr::null_mut());
        assert_ne!(h, 0);

        let held = |vk: u16| vk == 0x70;
        // Frame 1: down-transition → fire.
        poll_hotkeys(held, || true);
        assert_eq!(HITS.load(Ordering::Relaxed), 1);
        // Frame 2: still held → no fire.
        poll_hotkeys(held, || true);
        assert_eq!(HITS.load(Ordering::Relaxed), 1);

        assert!(unregister(h));
        // Frame 3: still held but unregistered → no fire.
        poll_hotkeys(held, || true);
        assert_eq!(HITS.load(Ordering::Relaxed), 1);
        clear();
    }

    #[test]
    fn foreground_gate_suppresses_fire() {
        let _guard = TEST_LOCK.lock();
        clear();
        HITS.store(0, Ordering::Relaxed);
        let _h = register_hotkey("test.fg", "F1", 0, 0x70, count, std::ptr::null_mut());
        poll_hotkeys(|vk| vk == 0x70, || false);
        assert_eq!(HITS.load(Ordering::Relaxed), 0);
        clear();
    }
}
