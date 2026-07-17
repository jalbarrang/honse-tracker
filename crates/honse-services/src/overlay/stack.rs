//! The self-hosted render/input stack: own egui `Context` + `egui-directx11`
//! renderer driven from edge's present callback, input via a WndProc subclass
//! chained on top of edge's.
//!
//! Productized from the proven honse-debug spike (`own_overlay.rs`, GO verdict).
//! Invariants (spike FINDINGS, do not regress):
//! - The backbuffer RTV is created, used, unbound, and dropped inside ONE
//!   Present. A cached RTV holds a backbuffer reference, `ResizeBuffers` fails,
//!   and resolution/fullscreen changes break.
//! - Transient render errors skip the frame; only [`MAX_CONSECUTIVE_RENDER_FAILS`]
//!   consecutive failures disable the stack.
//! - WndProc chords fire from the head of the subclass chain (guaranteed
//!   delivery); the polling hotkey stack owns everything else. One binding, one
//!   owner — never bind the same chord in both.
//! - Full DX11 pipeline-state backup/restore brackets the egui render pass.

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use windows::core::Interface;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D, D3D11_TEXTURE2D_DESC,
};
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain, DXGI_SWAP_CHAIN_DESC};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{
    CallWindowProcW, GetWindowLongPtrW, SetWindowLongPtrW, GWLP_WNDPROC, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WNDPROC,
};

use crate::hotkeys::Chord;

use super::d3d11_state::StateBackup;

/// egui wants the pointer (hover or drag on our UI) — swallow mouse buttons/wheel.
static WANTS_POINTER: AtomicBool = AtomicBool::new(false);
/// egui wants keyboard (text field focused) — swallow keys/chars.
static WANTS_KEYBOARD: AtomicBool = AtomicBool::new(false);
/// Something visible this frame — the WndProc translates input only while true.
static UI_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Previous WndProc in the subclass chain (0 = not installed).
static ORIG_WNDPROC: AtomicIsize = AtomicIsize::new(0);
/// Our own proc address, to detect whether we are still head of the chain.
static OUR_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// Events translated on the window thread, drained on the render thread.
static PENDING_EVENTS: Mutex<Vec<egui::Event>> = Mutex::new(Vec::new());

static STATE: Mutex<Option<StackState>> = Mutex::new(None);
static INIT_FAILED: AtomicBool = AtomicBool::new(false);
/// Consecutive render failures — transient errors (resize transitions, device
/// churn) must not kill the overlay; only sustained failure disables it.
static RENDER_FAILS: AtomicU32 = AtomicU32::new(0);
const MAX_CONSECUTIVE_RENDER_FAILS: u32 = 300;

/// WndProc-owned chords (critical bindings with guaranteed delivery).
type ChordFn = Arc<Mutex<dyn FnMut() + Send>>;
static CHORDS: Lazy<Mutex<Vec<(Chord, ChordFn)>>> = Lazy::new(|| Mutex::new(Vec::new()));

struct StackState {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    renderer: egui_directx11::Renderer,
    egui_ctx: egui::Context,
    hwnd: HWND,
    /// Last seen backbuffer size (log-only; the RTV itself is per-frame).
    last_size: (u32, u32),
    started: Instant,
}

// SAFETY: COM interface pointers are thread-safe to move; all access is behind the Mutex.
unsafe impl Send for StackState {}

/// Register a chord handled in the WndProc subclass (head of the chain, so
/// delivery is guaranteed even when the game or egui consumes keyboard input).
///
/// One binding, one owner: a chord registered here must NOT also be registered
/// with the polling hotkey stack, or one press fires twice (the Alt+9
/// double-toggle bug). Reserve this for bindings that must work while our UI
/// has keyboard focus; everything else belongs in `register_hotkey`.
pub fn register_wndproc_chord(mods: u8, vk: u16, callback: impl FnMut() + Send + 'static) {
    CHORDS
        .lock()
        .push((Chord::new(mods, vk), Arc::new(Mutex::new(callback))));
}

/// Per-present entry point, called from the frame trampoline with the raw
/// `IDXGISwapChain*` edge hands us (BEFORE edge's own GUI pass).
pub(crate) fn on_present(swapchain: *mut c_void) {
    if swapchain.is_null() || INIT_FAILED.load(Ordering::Relaxed) || !super::has_registrations() {
        return;
    }
    // SAFETY: the host passes a live IDXGISwapChain* for the duration of this call.
    let Some(swapchain) = (unsafe { IDXGISwapChain::from_raw_borrowed(&swapchain) }) else {
        return;
    };

    let mut guard = STATE.lock();
    if guard.is_none() {
        match init_state(swapchain) {
            Ok(state) => {
                log::info!("honse-services: overlay renderer + input initialized");
                *guard = Some(state);
            }
            Err(e) => {
                log::warn!("honse-services: overlay init failed: {e}; overlay disabled");
                INIT_FAILED.store(true, Ordering::Relaxed);
                return;
            }
        }
    }
    let state = guard.as_mut().expect("just initialized");

    if !super::any_visible() {
        UI_ACTIVE.store(false, Ordering::Relaxed);
        WANTS_POINTER.store(false, Ordering::Relaxed);
        WANTS_KEYBOARD.store(false, Ordering::Relaxed);
        PENDING_EVENTS.lock().clear();
        return;
    }
    UI_ACTIVE.store(true, Ordering::Relaxed);

    match render_frame(state, swapchain) {
        Ok(()) => {
            RENDER_FAILS.store(0, Ordering::Relaxed);
        }
        Err(e) => {
            // Transient during resolution changes (backbuffer churn) — skip the
            // frame; only sustained failure disables the overlay.
            let fails = RENDER_FAILS.fetch_add(1, Ordering::Relaxed) + 1;
            if fails == 1 {
                log::warn!("honse-services: overlay render failed (transient?): {e}");
            }
            if fails >= MAX_CONSECUTIVE_RENDER_FAILS {
                log::warn!("honse-services: {fails} consecutive overlay render failures; overlay disabled");
                INIT_FAILED.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn init_state(swapchain: &IDXGISwapChain) -> Result<StackState, String> {
    // SAFETY: valid swapchain from the host present hook.
    let device: ID3D11Device = unsafe { swapchain.GetDevice() }.map_err(|e| format!("GetDevice: {e}"))?;
    // SAFETY: device is live.
    let context: ID3D11DeviceContext =
        unsafe { device.GetImmediateContext() }.map_err(|e| format!("GetImmediateContext: {e}"))?;
    let renderer = egui_directx11::Renderer::new(&device).map_err(|e| format!("Renderer::new: {e}"))?;

    // SAFETY: valid swapchain.
    let desc: DXGI_SWAP_CHAIN_DESC = unsafe { swapchain.GetDesc() }.map_err(|e| format!("GetDesc: {e}"))?;
    let hwnd = desc.OutputWindow;
    if hwnd.0.is_null() {
        return Err("swapchain has no output window".into());
    }
    install_wndproc(hwnd)?;

    Ok(StackState {
        device,
        context,
        renderer,
        egui_ctx: egui::Context::default(),
        hwnd,
        last_size: (0, 0),
        started: Instant::now(),
    })
}

fn render_frame(state: &mut StackState, swapchain: &IDXGISwapChain) -> Result<(), String> {
    // SAFETY: valid swapchain; buffer 0 always exists. GetBuffer per frame is a
    // cheap refcount bump; the reference is dropped before Present returns.
    let backbuffer: ID3D11Texture2D = unsafe { swapchain.GetBuffer(0) }.map_err(|e| format!("GetBuffer: {e}"))?;
    let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: live texture; out-param call.
    unsafe { backbuffer.GetDesc(&mut tex_desc) };
    let size = (tex_desc.Width, tex_desc.Height);
    if size.0 == 0 || size.1 == 0 {
        return Ok(()); // minimized
    }
    if state.last_size != (0, 0) && state.last_size != size {
        log::info!("honse-services: overlay backbuffer resized to {}x{}", size.0, size.1);
    }
    state.last_size = size;

    // Per-frame RTV: created, used, unbound, and dropped inside this Present.
    // Holding it across frames keeps a backbuffer reference alive, which makes
    // the game's `ResizeBuffers` fail on resolution / fullscreen changes.
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    // SAFETY: backbuffer is a valid render-target-capable texture.
    unsafe { state.device.CreateRenderTargetView(&backbuffer, None, Some(&mut rtv)) }
        .map_err(|e| format!("CreateRenderTargetView: {e}"))?;
    let rtv = rtv.ok_or("CreateRenderTargetView returned none")?;

    // Build raw input: screen rect + drained window events.
    let events = std::mem::take(&mut *PENDING_EVENTS.lock());
    let raw_input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(size.0 as f32, size.1 as f32),
        )),
        time: Some(state.started.elapsed().as_secs_f64()),
        modifiers: current_modifiers(),
        events,
        focused: true,
        ..Default::default()
    };

    let full_output = state.egui_ctx.run(raw_input, |ctx| {
        super::draw_all(ctx);
    });

    // Publish input-consumption wishes for the WndProc.
    WANTS_POINTER.store(state.egui_ctx.wants_pointer_input(), Ordering::Relaxed);
    WANTS_KEYBOARD.store(state.egui_ctx.wants_keyboard_input(), Ordering::Relaxed);

    // Layout harvesting + debounced persistence (outside the run closure).
    super::after_frame(&state.egui_ctx);

    let (renderer_output, _platform, _viewports) = egui_directx11::split_output(full_output);

    // Back up the game's pipeline state, render, restore. The backup lives only
    // within this Present (no cached backbuffer-derived references).
    let backup = StateBackup::capture(&state.context);
    let result = state
        .renderer
        .render(&state.context, &rtv, &state.egui_ctx, renderer_output)
        .map_err(|e| format!("render: {e}"));
    // Unbind our RTV from the output-merger stage first so the pipeline holds no
    // reference of ours (would also block ResizeBuffers), then restore.
    // SAFETY: clearing render-target slot 0 on a live device context.
    unsafe { state.context.OMSetRenderTargets(Some(&[None]), None) };
    backup.restore(&state.context);
    result
}

// ───────────────────────────── input ─────────────────────────────

fn install_wndproc(hwnd: HWND) -> Result<(), String> {
    let ours = subclass_wndproc as *const () as isize;
    OUR_WNDPROC.store(ours, Ordering::SeqCst);
    // SAFETY: same-process window; SetWindowLongPtrW swaps the proc atomically.
    let orig = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours) };
    if orig == 0 {
        return Err("SetWindowLongPtrW(GWLP_WNDPROC) failed".into());
    }
    ORIG_WNDPROC.store(orig, Ordering::SeqCst);
    log::info!("honse-services: overlay WndProc subclassed (chained to {orig:#x})");
    Ok(())
}

/// Best-effort unhook for DLL unload. Only restores if we are still the head of
/// the chain (someone subclassing after us would be broken by a blind restore).
pub fn uninstall_wndproc() {
    let orig = ORIG_WNDPROC.swap(0, Ordering::SeqCst);
    if orig == 0 {
        return;
    }
    let hwnd = STATE.lock().as_ref().map(|s| s.hwnd);
    if let Some(hwnd) = hwnd {
        // SAFETY: same-process window; only restore when we are still installed.
        unsafe {
            if GetWindowLongPtrW(hwnd, GWLP_WNDPROC) == OUR_WNDPROC.load(Ordering::SeqCst) {
                SetWindowLongPtrW(hwnd, GWLP_WNDPROC, orig);
            }
        }
    }
}

extern "system" fn subclass_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let orig = ORIG_WNDPROC.load(Ordering::SeqCst);
    let call_orig = move || -> LRESULT {
        // SAFETY: orig is the previous WNDPROC captured at subclass time.
        unsafe {
            let proc: WNDPROC = std::mem::transmute::<isize, WNDPROC>(orig);
            CallWindowProcW(proc, hwnd, msg, wparam, lparam)
        }
    };

    // WndProc-owned chords: head of the chain → guaranteed delivery.
    if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN) && fire_matching_chords(wparam.0 as u16) {
        return LRESULT(0);
    }

    if !UI_ACTIVE.load(Ordering::Relaxed) {
        return call_orig();
    }

    let consumed = translate_message(msg, wparam, lparam);
    if consumed {
        LRESULT(0)
    } else {
        call_orig()
    }
}

/// Fire every registered chord matching `vk` + current modifiers. Returns true
/// when at least one fired (the message is then swallowed).
fn fire_matching_chords(vk: u16) -> bool {
    let mods = current_modifiers();
    let mut mods_bits = 0u8;
    if mods.ctrl {
        mods_bits |= crate::hotkeys::MOD_CTRL;
    }
    if mods.shift {
        mods_bits |= crate::hotkeys::MOD_SHIFT;
    }
    if mods.alt {
        mods_bits |= crate::hotkeys::MOD_ALT;
    }
    let pressed = Chord::new(mods_bits, vk);
    let matching: Vec<ChordFn> = CHORDS
        .lock()
        .iter()
        .filter(|(chord, _)| chord.matches(pressed))
        .map(|(_, cb)| cb.clone())
        .collect();
    let fired = !matching.is_empty();
    for cb in matching {
        (cb.lock())();
    }
    fired
}

/// Translate a Win32 message into egui events. Returns `true` when the message
/// should be swallowed (not forwarded to the game / previous proc).
fn translate_message(msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    let wants_pointer = WANTS_POINTER.load(Ordering::Relaxed);
    let wants_keyboard = WANTS_KEYBOARD.load(Ordering::Relaxed);
    let modifiers = current_modifiers();
    let pos = || {
        let x = (lparam.0 & 0xFFFF) as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
        egui::pos2(x, y)
    };

    match msg {
        WM_MOUSEMOVE => {
            push_event(egui::Event::PointerMoved(pos()));
            false // moves always pass through — the game keeps its hover state
        }
        WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN | WM_MBUTTONUP => {
            let (button, pressed) = match msg {
                WM_LBUTTONDOWN => (egui::PointerButton::Primary, true),
                WM_LBUTTONUP => (egui::PointerButton::Primary, false),
                WM_RBUTTONDOWN => (egui::PointerButton::Secondary, true),
                WM_RBUTTONUP => (egui::PointerButton::Secondary, false),
                WM_MBUTTONDOWN => (egui::PointerButton::Middle, true),
                _ => (egui::PointerButton::Middle, false),
            };
            push_event(egui::Event::PointerButton {
                pos: pos(),
                button,
                pressed,
                modifiers,
            });
            wants_pointer
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            push_event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, delta),
                modifiers,
            });
            wants_pointer
        }
        WM_CHAR => {
            let Some(ch) = char::from_u32(wparam.0 as u32) else {
                return wants_keyboard;
            };
            if !ch.is_control() {
                push_event(egui::Event::Text(ch.to_string()));
            }
            wants_keyboard
        }
        WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
            let pressed = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN);
            if let Some(key) = vk_to_egui_key(wparam.0 as u16) {
                push_event(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers,
                });
            }
            wants_keyboard
        }
        _ => false,
    }
}

fn push_event(event: egui::Event) {
    let mut events = PENDING_EVENTS.lock();
    // Bound the queue in case rendering stalls; drop oldest.
    if events.len() > 256 {
        events.remove(0);
    }
    events.push(event);
}

fn current_modifiers() -> egui::Modifiers {
    // SAFETY: GetKeyState is always safe to call.
    let down = |vk| unsafe { GetKeyState(vk) } < 0;
    let ctrl = down(VK_CONTROL.0 as i32);
    egui::Modifiers {
        alt: down(VK_MENU.0 as i32),
        ctrl,
        shift: down(VK_SHIFT.0 as i32),
        mac_cmd: false,
        command: ctrl,
    }
}

/// VK → egui key map: text editing + navigation + alphanumerics.
fn vk_to_egui_key(vk: u16) -> Option<egui::Key> {
    use egui::Key;
    Some(match vk {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Enter,
        0x1B => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::ArrowLeft,
        0x26 => Key::ArrowUp,
        0x27 => Key::ArrowRight,
        0x28 => Key::ArrowDown,
        0x2E => Key::Delete,
        0x30..=0x39 => Key::from_name(&char::from(b'0' + (vk - 0x30) as u8).to_string())?,
        0x41..=0x5A => Key::from_name(&char::from(b'A' + (vk - 0x41) as u8).to_string())?,
        0x70..=0x87 => Key::from_name(&format!("F{}", vk - 0x6F))?,
        _ => return None,
    })
}
