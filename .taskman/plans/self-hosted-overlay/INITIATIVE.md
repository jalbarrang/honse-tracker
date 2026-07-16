# Self-hosted overlay stack (bypass edge GUI ABI)

## Problem

Our plugins currently draw UI by casting Hachimi-Edge's raw `*mut c_void` egui
pointer back into `egui` types (`edge-sdk::gui::ui_from_ptr`). This couples us to
the host's exact egui source AND rustc version (the v0.1.0 boot-crash), and
limits us to edge's window primitives: decorated, permanently-dropped-on-close
plugin windows, no per-frame egui context, no modal dialogs, no custom chrome.
The surface-window watchdog, the parasitic overlay rendering, and the
close/reopen jank are all downstream of these restrictions.

## Findings (verified against Hachimi-Edge v0.26.4 source)

1. **Present callback = swapchain access.** `hachimi_register_present_callback`
   invokes plugin callbacks INSIDE `IDXGISwapChain::Present`, FIRST — before
   edge's own GUI runs — passing the raw `IDXGISwapChain*`
   (`src/windows/gui_impl/render_hook.rs:52-64`). From it: `GetDevice` →
   own `ID3D11Device`/`DeviceContext`.
2. **Own egui stack is therefore possible.** A plugin can statically link its
   own egui + egui-directx11 (any versions) and render a fully independent
   egui pass each frame. No egui types ever cross the DLL boundary → the
   rustc/egui lockstep constraint disappears for UI. Rendering inside the
   game's own swapchain also works in ExclusiveFullScreen (unlike external
   layered overlay windows — rejected fallback).
3. **Input is subclassable.** Edge itself takes input via a WndProc subclass
   (`SetWindowLongPtrW(GWLP_WNDPROC)`, `src/windows/wnd_hook.rs:198-202`).
   A plugin can subclass on top (install after edge, call through), feeding
   mouse/keyboard/text into its own egui context and swallowing only events
   its UI consumes. GetAsyncKeyState polling (already shipped) stays for
   global hotkeys.
4. **IL2CPP/tracking layer is unaffected.** The edge C API for classes,
   methods, fields, and the interceptor is a version-safe C ABI — keep it.
   Only the GUI usage moves in-house.

## Direction

Build an in-plugin overlay stack (own egui context + DX11 renderer + input
subclass) exposed through honse-services, then migrate tracker/race-hud UI onto
it and delete the host-egui dependency (`ui_from_ptr`, surface watchdog,
lockstep pin for UI purposes).

## Risks / open questions

- DX11 state bleed: must back up/restore device state around our pass (edge
  has `d3d11_backup.rs` for the same reason). egui-directx11 claims state
  restore; verify.
- ResizeBuffers: no plugin callback for it; recreate the RTV when backbuffer
  size changes (poll per frame).
- WndProc chain teardown ordering on unload (unhook LIFO or leak the subclass).
- Two plugins must not each hook Present rendering — the overlay stack must be
  a single shared service (one DLL owns it, or move to a shared runtime DLL).
- Edge menu key / IME interplay: don't fight edge for input when ITS menu is open.
