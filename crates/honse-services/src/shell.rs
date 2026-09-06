//! Handing something to the rest of the desktop: the clipboard, and the
//! browser.
//!
//! Copy first, open second. The game is usually fullscreen, so the browser
//! launch is the step most likely to go wrong, and a link already on the
//! clipboard survives it. Neither call is fatal; both report what happened.
//!
//! Raw Win32 because `windows` is already a dependency here and neither API
//! needs registering, unlike the toast in [`crate::toast`].

#![cfg(windows)]

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

/// `CF_UNICODETEXT`. Not exported by the `windows` crate as a typed constant
/// in a form `SetClipboardData` takes, so it is spelled out.
const CF_UNICODETEXT: u32 = 13;

/// Open `url` in whatever the user's default browser is.
///
/// Returns whether the shell accepted it. `ShellExecuteW` reports success as an
/// `HINSTANCE` above 32 — a historical quirk, not a handle.
pub fn open_url(url: &str) -> bool {
    let operation = HSTRING::from("open");
    let target = HSTRING::from(url);
    // SAFETY: both strings outlive the call; a null hwnd/dir is documented as
    // "no owner window" and "current directory".
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    result.0 as usize > 32
}

/// Put `text` on the clipboard as Unicode text.
///
/// Returns whether it landed. The clipboard is a shared, lockable resource:
/// another process can hold it, in which case this fails and says so rather
/// than retrying — the caller has a browser to fall back on.
pub fn copy_text(text: &str) -> bool {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0);
    let bytes = std::mem::size_of_val(utf16.as_slice());

    // SAFETY: a null hwnd associates the clipboard with the current task, which
    // is what a DLL with no window of its own wants.
    if unsafe { OpenClipboard(Some(HWND::default())) }.is_err() {
        return false;
    }
    let placed = place(&utf16, bytes);
    // SAFETY: the clipboard is open on this thread.
    let _ = unsafe { CloseClipboard() };
    placed
}

/// Allocate, fill and hand over the global block, with the clipboard already
/// open. Split out so every failure path still closes the clipboard.
fn place(utf16: &[u16], bytes: usize) -> bool {
    // SAFETY: the clipboard is open on this thread.
    if unsafe { EmptyClipboard() }.is_err() {
        return false;
    }
    // SAFETY: GMEM_MOVEABLE with a non-zero size; the handle is freed below on
    // every path that does not hand it to the clipboard.
    let Ok(handle) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes) }) else {
        return false;
    };

    // SAFETY: handle came from GlobalAlloc and is not yet locked.
    let ptr = unsafe { GlobalLock(handle) };
    if ptr.is_null() {
        // SAFETY: handle is a live unlocked allocation we still own.
        let _ = unsafe { GlobalFree(Some(handle)) };
        return false;
    }
    // SAFETY: the block was allocated at exactly this size.
    unsafe { std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr.cast::<u16>(), utf16.len()) };
    // SAFETY: the block is locked by the call above.
    let _ = unsafe { GlobalUnlock(handle) };

    // SAFETY: ownership of the block passes to the clipboard on success; on
    // failure it is still ours to free.
    match unsafe { SetClipboardData(CF_UNICODETEXT, Some(HANDLE(handle.0))) } {
        Ok(_) => true,
        Err(_) => {
            // SAFETY: the clipboard refused the block, so we still own it.
            let _ = unsafe { GlobalFree(Some(handle)) };
            false
        }
    }
}
