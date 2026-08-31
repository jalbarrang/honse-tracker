//! Swallowing overlay chords before the game sees them.
//!
//! [`crate::hotkeys`] polls `GetAsyncKeyState`, which observes and consumes
//! nothing — so every overlay chord was also delivered to the game.
//! `Ctrl+Shift+Down` moved the planner cursor *and* the game's; `Ctrl+Shift+D`
//! reached the game as `D`. No choice of key fixes that, because the game reads
//! keys without checking modifiers.
//!
//! The only way to consume a keystroke is to be in the message path, so this
//! subclasses the game window and drops the key messages that match a
//! registered chord.
//!
//! # It swallows, it does not dispatch
//!
//! Hotkeys still *fire* from the poll. This module only removes messages. That
//! split is deliberate: if the subclass fails to install, or the game turns out
//! to read keyboard through Raw Input rather than window messages, the overlay
//! keeps working exactly as it does today — leaking keys, but working. A design
//! where firing depended on the subclass would fail closed into no hotkeys at
//! all.
//!
//! # Chaining, not replacing
//!
//! The previous window procedure is called for everything we do not swallow.
//! Hachimi hooks this window too, so replacing rather than chaining would break
//! its menu.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::Input::{GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEKEYBOARD};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, DefWindowProcW, EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetWindowLongPtrW,
    GWLP_WNDPROC, WM_CHAR, WM_INPUT, WM_KEYDOWN, WM_KEYUP, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// Previous window procedure, or 0 if not installed.
static ORIGINAL: AtomicIsize = AtomicIsize::new(0);
static HOOKED: AtomicBool = AtomicBool::new(false);
/// The swapchain's output window, once the overlay has seen one. `0` = unknown.
static RENDER_WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Key messages logged so far, so a diagnostic run does not fill the log.
static LOGGED: AtomicU32 = AtomicU32::new(0);
static LOGGED_RAW: AtomicU32 = AtomicU32::new(0);
const LOG_LIMIT: u32 = 24;

/// Tell the hook which window the game renders into.
///
/// Called by the overlay once it has a swapchain. Enumerating windows and
/// taking the first visible one is a guess — a process can own a splash, a
/// helper, or an off-screen window — and hooking the wrong one installs
/// successfully while intercepting nothing.
pub fn note_render_window(hwnd: isize) {
    if RENDER_WINDOW.swap(hwnd, Ordering::AcqRel) != hwnd {
        log::info!("honse-services: render window is {hwnd:#x}");
    }
}

/// Subclass the game window if it exists yet.
///
/// Safe to call every frame: it does nothing once installed, and nothing until
/// the window can be found. Returns whether the hook is in place.
pub fn ensure_installed() -> bool {
    if HOOKED.load(Ordering::Acquire) {
        return true;
    }
    // Prefer the swapchain's window; only fall back to enumeration if the
    // overlay has not painted yet.
    let render = RENDER_WINDOW.load(Ordering::Acquire);
    let hwnd = if render == 0 {
        let Some(found) = main_window() else {
            return false; // window not up yet; try again next frame
        };
        found
    } else {
        HWND(render as *mut std::ffi::c_void)
    };
    // SAFETY: `hwnd` is a live top-level window owned by this process.
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wndproc as *const () as isize) };
    if previous == 0 {
        log::error!("honse-services: could not subclass the game window; overlay keys will reach the game");
        // Do not retry forever — a failure here is not transient.
        HOOKED.store(true, Ordering::Release);
        return false;
    }
    ORIGINAL.store(previous, Ordering::Release);
    HOOKED.store(true, Ordering::Release);
    log::info!(
        "honse-services: subclassed window {:#x} ({}); overlay chords will be consumed",
        hwnd.0 as isize,
        if render == 0 { "enumerated" } else { "from swapchain" }
    );
    true
}

/// Our procedure: drop key messages belonging to a registered chord, pass
/// everything else to the previous one untouched.
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Raw Input is a separate delivery path from WM_KEYDOWN, and the game reads
    // that one: swallowing the legacy message alone provably does nothing.
    // SAFETY: `lparam` is the HRAWINPUT handle for WM_INPUT.
    if msg == WM_INPUT && unsafe { swallows_raw_key(lparam) } {
        // The system still needs the packet released, so the default handler
        // runs — but the game's procedure never sees it.
        // SAFETY: the arguments are the ones we were handed.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    if swallows(msg, wparam) {
        return LRESULT(0);
    }
    let original = ORIGINAL.load(Ordering::Acquire);
    if original == 0 {
        // `uninstall` cleared it while this message was in flight. Transmuting
        // 0 into a function pointer and calling it would take the game down;
        // the default handler is the correct fallback.
        // SAFETY: the arguments are the ones we were handed.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    // SAFETY: `original` is the procedure `SetWindowLongPtrW` returned for this
    // window, and the arguments are the ones we were handed.
    unsafe {
        CallWindowProcW(
            Some(std::mem::transmute::<isize, unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT>(
                original,
            )),
            hwnd,
            msg,
            wparam,
            lparam,
        )
    }
}

/// Whether this message is one of ours to eat.
fn swallows(msg: u32, wparam: WPARAM) -> bool {
    match msg {
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
            let vk = (wparam.0 & 0xFFFF) as u16;
            let mods = held_mods();
            let eat = crate::hotkeys::chord_registered(vk, mods);
            // Proof of whether we are in the message path at all. If a key that
            // reaches the game never appears here, the game is not reading
            // keyboard through this window's messages — Raw Input, most likely
            // — and swallowing them can never work.
            if mods != 0 && LOGGED.fetch_add(1, Ordering::Relaxed) < LOG_LIMIT {
                log::info!("honse-services: wndproc key msg={msg:#x} vk={vk:#04x} mods={mods:#04b} eat={eat}");
            }
            eat
        }
        // A chord that reaches the character stage would type into a focused
        // text field. The vk is gone by now, so this keys off the modifiers
        // alone — with Ctrl+Shift down there is no character worth delivering.
        WM_CHAR | WM_SYSCHAR => crate::hotkeys::any_chord_uses(held_mods()),
        _ => false,
    }
}

/// Whether a `WM_INPUT` packet is a keyboard event for one of our chords.
///
/// Only keyboard packets are ever swallowed, and only ones matching a bound
/// chord — the game's mouse and every other key ride the same message type and
/// must pass through untouched.
///
/// # Safety
/// `lparam` must be the `HRAWINPUT` from a `WM_INPUT` message.
unsafe fn swallows_raw_key(lparam: LPARAM) -> bool {
    let mut raw = RAWINPUT::default();
    let mut size = u32::try_from(std::mem::size_of::<RAWINPUT>()).unwrap_or(0);
    // SAFETY: `raw` is a correctly sized buffer for one RAWINPUT record.
    let read = unsafe {
        GetRawInputData(
            HRAWINPUT(lparam.0 as *mut std::ffi::c_void),
            RID_INPUT,
            Some((&raw mut raw).cast()),
            &raw mut size,
            u32::try_from(std::mem::size_of::<RAWINPUTHEADER>()).unwrap_or(0),
        )
    };
    if read == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD.0 {
        return false;
    }
    // SAFETY: dwType says this union member is the keyboard one.
    let vk = unsafe { raw.data.keyboard.VKey };
    let mods = held_mods();
    let eat = crate::hotkeys::chord_registered(vk, mods);
    if mods != 0 && LOGGED_RAW.fetch_add(1, Ordering::Relaxed) < LOG_LIMIT {
        log::info!("honse-services: wndproc raw key vk={vk:#04x} mods={mods:#04b} eat={eat}");
    }
    eat
}

/// Modifiers currently down, in [`crate::hotkeys`]'s bit order.
///
/// `GetKeyState` rather than `GetAsyncKeyState`: inside a window procedure it
/// reports the state as of the message being processed, so a chord released
/// mid-queue cannot make us drop the wrong key.
fn held_mods() -> u8 {
    // SAFETY: GetKeyState is always safe to call with a virtual-key code.
    let down = |vk: u16| unsafe { GetKeyState(i32::from(vk)) < 0 };
    (u8::from(down(VK_CONTROL.0)) * crate::hotkeys::MOD_CTRL)
        | (u8::from(down(VK_SHIFT.0)) * crate::hotkeys::MOD_SHIFT)
        | (u8::from(down(VK_MENU.0)) * crate::hotkeys::MOD_ALT)
}

/// The process's first visible top-level window.
fn main_window() -> Option<HWND> {
    struct Search {
        pid: u32,
        found: Option<HWND>,
    }
    // SAFETY: `lparam` is the `&mut Search` handed to EnumWindows below.
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        // SAFETY: as above.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut pid = 0u32;
        // SAFETY: `hwnd` comes from the enumeration and is live.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&raw mut pid)) };
        // SAFETY: as above.
        if pid == search.pid && unsafe { IsWindowVisible(hwnd) }.as_bool() {
            search.found = Some(hwnd);
            return false.into(); // stop enumerating
        }
        true.into()
    }

    let mut search = Search {
        // SAFETY: GetCurrentProcessId takes no arguments and cannot fail.
        pid: unsafe { GetCurrentProcessId() },
        found: None,
    };
    // SAFETY: `visit` matches the EnumWindows callback contract and `search`
    // outlives the call.
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&raw mut search as isize)) };
    search.found
}

/// Whether the subclass is installed and consuming.
#[must_use]
pub fn is_installed() -> bool {
    ORIGINAL.load(Ordering::Acquire) != 0
}

/// Put the original window procedure back.
///
/// Called on shutdown: a DLL that unloads while still owning a window
/// procedure leaves a pointer into freed memory, and the next key message
/// crashes the game.
pub fn uninstall() {
    let original = ORIGINAL.swap(0, Ordering::AcqRel);
    if original == 0 {
        return;
    }
    if let Some(hwnd) = main_window() {
        // SAFETY: restoring the procedure this module replaced.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, original) };
        log::info!("honse-services: game window procedure restored");
    }
}
