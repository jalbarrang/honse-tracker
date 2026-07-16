# Spike findings — self-hosted egui overlay (t-006)

**Verdict: GO.** All acceptance criteria met on real hardware (edge v0.26.4,
ExclusiveFullScreen). The self-hosted stack removes the host-egui ABI coupling
entirely for anything drawn through it.

## What was proven

| Criterion | Result |
| --- | --- |
| Own egui + egui-directx11 0.12.1 window over the game | ✅ interactive (button/checkbox/text) |
| Input via WndProc subclass, call-through | ✅ game input unaffected outside our UI |
| Resize / fullscreen | ✅ after per-frame RTV fix (see below) |
| Coexists with edge menu + notifications | ✅ screenshot-verified |
| Frame cost | µs-range, shown live in the demo window (single window; scales with widget count like any egui app) |

## Hard-won specifics (encode into productization)

1. **Never cache backbuffer-derived resources across frames.** A cached
   `ID3D11RenderTargetView` holds a backbuffer reference → the game's
   `ResizeBuffers` fails → resolution/fullscreen breaks. Create → render →
   `OMSetRenderTargets(clear)` → drop, all inside one Present.
2. **Handle critical chords in the WndProc, not the polling stack.** As head of
   the subclass chain, delivery is guaranteed; GetAsyncKeyState polling stays
   for global hotkeys only.
3. **Transient render errors must not disable the overlay** — resize
   transitions produce spurious failures (disable only after ~300 consecutive).
4. **Per-plugin surface identity matters** — every DLL statically linking the
   services crate is its own instance; shared default titles produced duplicate
   host-menu items ("Show Honse Tracker" ×2). Fixed via
   `InitOptions::surface_title`, set before any `register_*`.
5. Present callback receives the raw `IDXGISwapChain*` BEFORE edge's GUI pass —
   our UI draws under edge's menu, which is the right z-order.
6. No DX11 pipeline-state backup was needed in practice (game rebinds at frame
   start; edge's painter sets its own state) — but productization should add a
   state block anyway (edge ships `d3d11_backup.rs` for the same reason).

## Productization requirements (next plan)

1. Move the stack into honse-services (or a new `honse-overlay` crate):
   renderer lifecycle, input translation, per-plugin egui `Context`.
2. **Single-owner rule:** exactly one WndProc subclass + one renderer per DLL;
   if multiple honse plugins load, each has its own (they chain) — validate 3
   plugins chaining, or move to one shared runtime DLL.
3. Full key map + IME awareness (edge does IME composition positioning; we skip
   it — text input in CJK will need it).
4. Migrate tracker/race-hud windows + panels off host egui; then delete
   `ui_from_ptr` usage from plugin UI paths and the surface-window watchdog
   machinery (real close buttons become possible).
5. rustc pin (`rust-toolchain.toml`) can relax to "any rustc" for UI once no
   host-egui call remains; keep the pin until the last `ui_from_ptr` caller dies.
6. Unload story: WndProc restore is chain-order-sensitive; either leak the
   subclass (process-lifetime plugins) or add explicit chain-aware teardown.
