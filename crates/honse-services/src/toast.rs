//! Real Windows notifications, for saying something to a player who is not
//! looking at the game.
//!
//! # Why not the host's notification
//!
//! Edge's `gui_show_notification` paints into Edge's own egui overlay, which
//! lives inside the game window. It is the right surface for "your config
//! failed to load" and the wrong one for "the thing you walked away from is
//! finished", because by definition nobody is looking at that window.
//!
//! # Why a tray balloon and not a WinRT toast
//!
//! The modern `ToastNotificationManager` API needs an AppUserModelID backed by
//! a real Start Menu shortcut. A DLL injected into someone else's process has
//! no business installing shortcuts, and without one the toast is silently
//! dropped. `Shell_NotifyIcon` with `NIF_INFO` needs no registration at all,
//! and Windows 10/11 render it as a genuine toast that lands in the Action
//! Center.
//!
//! The cost is a tray icon: the balloon belongs to one, so there has to be one.
//! It is added as soon as something is *pending* rather than when that thing
//! fires (see [`prepare`]) — partly so it only exists when there is a reason
//! for it, and partly because the shell establishes a new icon asynchronously
//! and refuses a balloon aimed at one that has not landed yet. It borrows the
//! game's own icon, so what appears in the tray and on the toast is the game.
//! It is removed on shutdown.
//!
//! # What can still swallow it
//!
//! Focus Assist / Do Not Disturb suppresses toasts, and Windows turns it on by
//! default while a fullscreen app is in the foreground. That rule only applies
//! while the game *is* the foreground window — which is the case where the
//! player is already looking at the screen.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_TIP, NIIF_NONE, NIIF_USER, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW, NOTIFY_ICON_INFOTIP_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClassLongPtrW, LoadIconW, GCLP_HICON, HICON, IDI_APPLICATION};

/// Identifies our icon within the owning window. Any value; it only has to
/// stay the same between [`show`] and [`remove`].
const ICON_ID: u32 = 0x484f_4e53; // "HONS"

/// Whether the tray icon is currently registered.
static REGISTERED: AtomicBool = AtomicBool::new(false);
/// The window the icon was registered against, so [`remove`] addresses the same
/// one even if the render window changed underneath us.
static OWNER: AtomicIsize = AtomicIsize::new(0);

/// How long to keep re-offering a refused balloon, and the gaps between tries.
///
/// The shell establishes a newly added icon asynchronously. A balloon aimed at
/// one that has not landed yet is refused outright — which is exactly what
/// happened when the icon was added and the balloon requested 0.12&nbsp;ms
/// later. [`prepare`] is the real fix; this is what covers a session armed with
/// less warning than the shell needs.
const RETRY_GAPS_MS: &[u64] = &[120, 300, 700, 1500];

/// Register the tray icon now, ahead of the notification that will need it.
///
/// Call this as soon as something is *pending* rather than when it fires. The
/// icon then has minutes to be established instead of microseconds, and it also
/// means the icon shows up only once there is a reason for it.
pub fn prepare() {
    if REGISTERED.load(Ordering::Acquire) {
        return;
    }
    std::thread::spawn(|| {
        if let Some(hwnd) = crate::input_block::game_window() {
            ensure_icon(hwnd);
        }
    });
}

/// Show a Windows notification.
///
/// Returns immediately: `Shell_NotifyIconW` is a synchronous call into
/// explorer, which can stall for seconds when explorer is busy, and callers
/// reach this from the render thread. A notification arriving a few hundred
/// milliseconds late is nobody's problem; a dropped frame is.
///
/// `title` is truncated to 63 UTF-16 units and `body` to 255 — the balloon's
/// own limits, not ours.
pub fn show(title: &str, body: &str) {
    let (title, body) = (title.to_owned(), body.to_owned());
    std::thread::spawn(move || show_blocking(&title, &body));
}

/// Remove the tray icon. Idempotent, and safe to call having never shown one.
///
/// Called from shutdown: an icon whose owning process is gone leaves a ghost in
/// the tray until the user waves the mouse over it.
pub fn remove() {
    if !REGISTERED.swap(false, Ordering::AcqRel) {
        return;
    }
    let data = icon_data(HWND(OWNER.swap(0, Ordering::AcqRel) as *mut std::ffi::c_void));
    // SAFETY: `data` is fully initialised and carries its own size; NIM_DELETE
    // reads only the hWnd/uID pair that identifies the icon.
    let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &raw const data) };
}

fn show_blocking(title: &str, body: &str) {
    let Some(hwnd) = crate::input_block::game_window() else {
        log::warn!("honse-services: no game window yet; notification dropped");
        return;
    };
    if !ensure_icon(hwnd) {
        return;
    }

    // Our own icon in the balloon, so the toast is recognisably from the game
    // rather than a generic blue (i).
    if balloon(hwnd, title, body, NIIF_USER) {
        return;
    }
    for gap in RETRY_GAPS_MS {
        std::thread::sleep(std::time::Duration::from_millis(*gap));
        if balloon(hwnd, title, body, NIIF_USER) {
            return;
        }
    }
    // Still refused. The other thing the shell rejects a balloon over is the
    // icon it was told to draw, so drop that requirement and take the generic
    // one — a plain notification beats a pretty absent one. Logged distinctly
    // because which of the two it was is the whole diagnosis.
    if balloon(hwnd, title, body, NIIF_NONE) {
        log::warn!("honse-services: notification shown only without its own icon");
    } else {
        log::warn!("honse-services: Shell_NotifyIcon(NIM_MODIFY) refused the notification");
    }
}

/// One attempt at the balloon. `true` if the shell took it.
fn balloon(hwnd: HWND, title: &str, body: &str, info_flags: NOTIFY_ICON_INFOTIP_FLAGS) -> bool {
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = info_flags;
    if info_flags == NIIF_USER {
        data.hBalloonIcon = window_icon(hwnd);
    }
    write_wide(&mut data.szInfoTitle, title);
    write_wide(&mut data.szInfo, body);

    // SAFETY: `data` is fully initialised, carries its own size, and names an
    // icon this process registered against a live window.
    let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &raw const data) }.as_bool();
    if ok {
        log::info!("honse-services: notification shown ({title})");
    }
    ok
}

/// Register the tray icon if it is not already there. The balloon belongs to an
/// icon, so there is no way to show one without this.
fn ensure_icon(hwnd: HWND) -> bool {
    if REGISTERED.load(Ordering::Acquire) {
        return true;
    }
    let mut data = icon_data(hwnd);
    data.uFlags = NIF_ICON | NIF_TIP;
    data.hIcon = window_icon(hwnd);
    write_wide(&mut data.szTip, "Honse Tracker");

    // SAFETY: `data` is fully initialised and carries its own size. No
    // NIF_MESSAGE, so the shell never posts anything back to `hwnd` and the
    // game's window procedure is untouched.
    if !unsafe { Shell_NotifyIconW(NIM_ADD, &raw const data) }.as_bool() {
        log::warn!("honse-services: could not add the tray icon; notifications unavailable");
        return false;
    }
    OWNER.store(hwnd.0 as isize, Ordering::Release);
    REGISTERED.store(true, Ordering::Release);
    log::info!("honse-services: tray icon registered for notifications");
    true
}

/// A zeroed `NOTIFYICONDATAW` carrying its own size and identity. Callers set
/// `uFlags` and whichever fields those flags name.
fn icon_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: u32::try_from(std::mem::size_of::<NOTIFYICONDATAW>()).unwrap_or(0),
        hWnd: hwnd,
        uID: ICON_ID,
        ..Default::default()
    }
}

/// The game's own icon, falling back to the generic application icon.
///
/// Read from the window *class* rather than asked for with `WM_GETICON`:
/// sending a message to a window owned by another thread blocks until that
/// thread pumps it, and this runs while the game is mid-frame.
fn window_icon(hwnd: HWND) -> HICON {
    // SAFETY: `hwnd` is a live window owned by this process.
    let handle = unsafe { GetClassLongPtrW(hwnd, GCLP_HICON) };
    if handle != 0 {
        return HICON(handle as *mut std::ffi::c_void);
    }
    // SAFETY: IDI_APPLICATION is a built-in resource and always loads.
    unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default()
}

/// Copy `text` into a fixed wide buffer, NUL-terminated, truncating to fit.
///
/// Truncation is on UTF-16 units, so a surrogate pair straddling the boundary
/// would leave half a character. Both call sites pass ASCII-ish text well under
/// the limit; this is here so an over-long string is a clipped notification
/// rather than a buffer overrun.
fn write_wide(buffer: &mut [u16], text: &str) {
    let limit = buffer.len().saturating_sub(1);
    let mut written = 0;
    for unit in text.encode_utf16().take(limit) {
        buffer[written] = unit;
        written += 1;
    }
    buffer[written] = 0;
}

#[cfg(test)]
mod tests {
    use super::write_wide;

    fn read_back(buffer: &[u16]) -> String {
        let end = buffer.iter().position(|&u| u == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    #[test]
    fn text_round_trips_nul_terminated() {
        let mut buffer = [0xFFFFu16; 16];
        write_wide(&mut buffer, "done");
        assert_eq!(read_back(&buffer), "done");
        assert_eq!(buffer[4], 0);
    }

    /// The balloon's buffers are fixed size. Over-long text must clip, and must
    /// still leave room for the terminator the shell reads to.
    #[test]
    fn over_long_text_clips_and_still_terminates() {
        let mut buffer = [0xFFFFu16; 8];
        write_wide(&mut buffer, "far too long to fit");
        assert_eq!(read_back(&buffer), "far too");
        assert_eq!(buffer[7], 0);
    }

    #[test]
    fn empty_text_is_just_a_terminator() {
        let mut buffer = [0xFFFFu16; 4];
        write_wide(&mut buffer, "");
        assert_eq!(buffer[0], 0);
    }
}
