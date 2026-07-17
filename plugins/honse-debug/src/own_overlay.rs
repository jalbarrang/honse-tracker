//! SPIKE: self-hosted egui overlay rendered from edge's present callback.
//!
//! Proves the `self-hosted-overlay` initiative direction: this module owns its
//! egui `Context` and an `egui-directx11` renderer, statically linked into THIS
//! plugin — **no egui types ever cross the plugin↔host boundary**, so the
//! egui/rustc ABI-lockstep constraint does not apply to anything drawn here.
//!
//! How it plugs in (verified against Hachimi-Edge v0.26.4 source):
//! - `hachimi_register_present_callback` invokes us inside
//!   `IDXGISwapChain::Present`, FIRST (before edge's own GUI pass), passing the
//!   raw `IDXGISwapChain*` (`src/windows/gui_impl/render_hook.rs:52-64`).
//! - Input arrives via a WndProc subclass installed on top of edge's own
//!   subclass (`src/windows/wnd_hook.rs:198-202`), calling through to the
//!   previous proc and swallowing only events our UI consumes.
//!
//! Spike-scope shortcuts (documented, deliberate):
//! - No DX11 pipeline-state backup/restore around `Renderer::render` (the game
//!   re-binds at frame start; edge's painter sets up its own full state).
//!   Watch for artifacts during validation — productization should add a
//!   state block like edge's `d3d11_backup.rs`.
//! - Minimal key map (text editing + navigation keys), no IME.
//! - WndProc restore on unload is best-effort (chain-order caveat in notes).

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::Instant;

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

/// Toggled by the Alt+9 hotkey (see [`install`]). Starts hidden — silent boot.
static VISIBLE: AtomicBool = AtomicBool::new(false);
/// egui wants the pointer (hover or drag on our UI) — swallow mouse buttons/wheel.
static WANTS_POINTER: AtomicBool = AtomicBool::new(false);
/// egui wants keyboard (text field focused) — swallow keys/chars.
static WANTS_KEYBOARD: AtomicBool = AtomicBool::new(false);
/// Previous WndProc in the subclass chain (0 = not installed).
static ORIG_WNDPROC: AtomicIsize = AtomicIsize::new(0);
/// Our own proc address, to detect whether we are still head of the chain.
static OUR_WNDPROC: AtomicIsize = AtomicIsize::new(0);

/// Events translated on the window thread, drained on the render thread.
static PENDING_EVENTS: Mutex<Vec<egui::Event>> = Mutex::new(Vec::new());

static STATE: Mutex<Option<OverlayState>> = Mutex::new(None);
static INIT_FAILED: AtomicBool = AtomicBool::new(false);
/// Consecutive render failures — transient errors (resize transitions, device
/// churn) must not kill the spike; only sustained failure disables it.
static RENDER_FAILS: AtomicU32 = AtomicU32::new(0);
const MAX_CONSECUTIVE_RENDER_FAILS: u32 = 300;
/// Bounded key-event diagnostics (t-005 debugging: menu key / Alt+9 delivery).
static KEY_LOG_BUDGET: AtomicU32 = AtomicU32::new(50);

struct OverlayState {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    renderer: egui_directx11::Renderer,
    egui_ctx: egui::Context,
    hwnd: HWND,
    /// Last seen backbuffer size (log-only; the RTV itself is created per frame
    /// and dropped before Present returns — a cached RTV holds a backbuffer
    /// reference and makes `ResizeBuffers` fail, breaking resolution changes).
    last_size: (u32, u32),
    started: Instant,
    // Demo widgets
    text: String,
    clicks: u32,
    checked: bool,
    frame_cost_us: f32,
}

// SAFETY: COM interface pointers are thread-safe to move; all access is behind the Mutex.
unsafe impl Send for OverlayState {}

/// Register the present callback + toggle hotkey. Call once from plugin init.
pub fn install() {
    let Some(sdk) = edge_sdk::Sdk::try_get() else {
        hlog_warn!(target: "debug-viewer", "own_overlay: Sdk not ready; spike disabled");
        return;
    };
    if !sdk.register_present_callback(present_callback, std::ptr::null_mut()) {
        hlog_warn!(target: "debug-viewer", "own_overlay: present callback registration failed");
        return;
    }
    // Alt+9 is handled ONLY in subclass_wndproc. It was also registered as a
    // polling hotkey while the polling stack was dead (pre game-ready-bootstrap);
    // once polling came back both paths fired on one press — double-toggle, so
    // the window "never opened". One binding, one owner: the WndProc.
    hlog_info!(target: "debug-viewer", "own_overlay: spike installed (Alt+9 toggles via WndProc)");
}

extern "C" fn toggle_hotkey(_userdata: *mut c_void) {
    let now = !VISIBLE.load(Ordering::Relaxed);
    VISIBLE.store(now, Ordering::Relaxed);
    hlog_info!(target: "debug-viewer", "own_overlay: visible={now}");
}

/// Present callback: the host passes the raw `IDXGISwapChain*` before its own GUI runs.
unsafe extern "C" fn present_callback(swapchain: *mut c_void, _userdata: *mut c_void) {
    if swapchain.is_null() || INIT_FAILED.load(Ordering::Relaxed) {
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
                hlog_info!(target: "debug-viewer", "own_overlay: renderer + input initialized");
                *guard = Some(state);
            }
            Err(e) => {
                hlog_warn!(target: "debug-viewer", "own_overlay: init failed: {e}; spike disabled");
                INIT_FAILED.store(true, Ordering::Relaxed);
                return;
            }
        }
    }
    let state = guard.as_mut().expect("just initialized");

    if !VISIBLE.load(Ordering::Relaxed) {
        WANTS_POINTER.store(false, Ordering::Relaxed);
        WANTS_KEYBOARD.store(false, Ordering::Relaxed);
        PENDING_EVENTS.lock().clear();
        return;
    }

    let frame_start = Instant::now();
    match render_frame(state, swapchain) {
        Ok(()) => {
            RENDER_FAILS.store(0, Ordering::Relaxed);
            state.frame_cost_us = frame_start.elapsed().as_secs_f32() * 1_000_000.0;
        }
        Err(e) => {
            // Transient during resolution changes (backbuffer churn) — skip the
            // frame; only sustained failure disables the spike.
            let fails = RENDER_FAILS.fetch_add(1, Ordering::Relaxed) + 1;
            if fails == 1 {
                hlog_warn!(target: "debug-viewer", "own_overlay: render failed (transient?): {e}");
            }
            if fails >= MAX_CONSECUTIVE_RENDER_FAILS {
                hlog_warn!(target: "debug-viewer", "own_overlay: {fails} consecutive render failures; spike disabled");
                INIT_FAILED.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn init_state(swapchain: &IDXGISwapChain) -> Result<OverlayState, String> {
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

    let egui_ctx = egui::Context::default();
    Ok(OverlayState {
        device,
        context,
        renderer,
        egui_ctx,
        hwnd,
        last_size: (0, 0),
        started: Instant::now(),
        text: String::new(),
        clicks: 0,
        checked: false,
        frame_cost_us: 0.0,
    })
}

fn render_frame(state: &mut OverlayState, swapchain: &IDXGISwapChain) -> Result<(), String> {
    // Backbuffer size — t-003: recreate the RTV whenever it changes (resize,
    // fullscreen toggle). GetBuffer per frame is cheap (refcount bump).
    // SAFETY: valid swapchain; buffer 0 always exists.
    let backbuffer: ID3D11Texture2D = unsafe { swapchain.GetBuffer(0) }.map_err(|e| format!("GetBuffer: {e}"))?;
    let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: live texture; out-param call.
    unsafe { backbuffer.GetDesc(&mut tex_desc) };
    let size = (tex_desc.Width, tex_desc.Height);
    if size.0 == 0 || size.1 == 0 {
        return Ok(()); // minimized
    }

    if state.last_size != (0, 0) && state.last_size != size {
        hlog_info!(target: "debug-viewer", "own_overlay: backbuffer resized to {}x{}", size.0, size.1);
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

    let text = &mut state.text;
    let clicks = &mut state.clicks;
    let checked = &mut state.checked;
    let frame_cost = state.frame_cost_us;
    let full_output = state.egui_ctx.run(raw_input, |ctx| {
        egui::Window::new("Self-hosted overlay (spike)")
            .default_pos(egui::pos2(60.0, 120.0))
            .resizable(true)
            .collapsible(true)
            .show(ctx, |ui| {
                ui.label("This window is rendered by honse-debug's OWN egui —");
                ui.label("no host egui, no ABI lockstep, real close semantics.");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Click me").clicked() {
                        *clicks += 1;
                    }
                    ui.label(format!("clicks: {clicks}"));
                });
                ui.checkbox(checked, "A checkbox");
                ui.horizontal(|ui| {
                    ui.label("Text input:");
                    ui.text_edit_singleline(text);
                });
                ui.separator();
                ui.small(format!("frame cost: {frame_cost:.0} µs — Alt+9 hides me"));
            });
    });

    // Publish input-consumption wishes for the WndProc.
    WANTS_POINTER.store(state.egui_ctx.wants_pointer_input(), Ordering::Relaxed);
    WANTS_KEYBOARD.store(state.egui_ctx.wants_keyboard_input(), Ordering::Relaxed);

    let (renderer_output, _platform, _viewports) = egui_directx11::split_output(full_output);
    let result = state
        .renderer
        .render(&state.context, &rtv, &state.egui_ctx, renderer_output)
        .map_err(|e| format!("render: {e}"));

    // Unbind our RTV from the output-merger stage so the pipeline holds no
    // internal backbuffer reference either (would also block ResizeBuffers).
    // SAFETY: clearing render-target slot 0 on a live device context.
    unsafe { state.context.OMSetRenderTargets(Some(&[None]), None) };
    result
}

// ───────────────────────────── input (t-004) ─────────────────────────────

fn install_wndproc(hwnd: HWND) -> Result<(), String> {
    let ours = subclass_wndproc as *const () as isize;
    OUR_WNDPROC.store(ours, Ordering::SeqCst);
    // SAFETY: same-process window; SetWindowLongPtrW swaps the proc atomically.
    let orig = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, ours) };
    if orig == 0 {
        return Err("SetWindowLongPtrW(GWLP_WNDPROC) failed".into());
    }
    ORIG_WNDPROC.store(orig, Ordering::SeqCst);
    hlog_info!(target: "debug-viewer", "own_overlay: WndProc subclassed (chained to {orig:#x})");
    Ok(())
}

/// Best-effort unhook for DLL unload. Only restores if we are still the head of
/// the chain (someone subclassing after us would be broken by a blind restore).
pub fn uninstall_wndproc(hwnd_hint: Option<HWND>) {
    let orig = ORIG_WNDPROC.swap(0, Ordering::SeqCst);
    if orig == 0 {
        return;
    }
    let hwnd = hwnd_hint.or_else(|| STATE.lock().as_ref().map(|s| s.hwnd));
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

    // Alt+9 toggle handled here directly: we are the head of the WndProc chain,
    // so delivery is guaranteed regardless of the hotkey-polling stack.
    if msg == WM_SYSKEYDOWN && wparam.0 as u16 == 0x39 {
        toggle_hotkey(std::ptr::null_mut());
        return LRESULT(0);
    }

    // Bounded diagnostics for t-005: what key traffic do we see / forward?
    if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN) && KEY_LOG_BUDGET.load(Ordering::Relaxed) > 0 {
        KEY_LOG_BUDGET.fetch_sub(1, Ordering::Relaxed);
        hlog_info!(
            target: "debug-viewer",
            "own_overlay: keydown vk={:#04x} visible={} wants_kb={}",
            wparam.0 as u16,
            VISIBLE.load(Ordering::Relaxed),
            WANTS_KEYBOARD.load(Ordering::Relaxed)
        );
    }

    if !VISIBLE.load(Ordering::Relaxed) {
        return call_orig();
    }

    let consumed = translate_message(msg, wparam, lparam);
    if consumed {
        LRESULT(0)
    } else {
        call_orig()
    }
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

/// Minimal VK → egui key map: text editing + navigation (spike scope).
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
        _ => return None,
    })
}
