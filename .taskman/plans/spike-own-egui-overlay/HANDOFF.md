# Spike: prove an in-plugin egui overlay over edge's present callback

## Goal

A minimal, throwaway-quality proof inside `plugins/honse-debug` (dev-only
plugin) that renders an interactive, self-hosted egui window over the game
WITHOUT touching host egui.

## Context (read first)

- Initiative overview: `.taskman/plans/self-hosted-overlay/INITIATIVE.md` —
  contains the verified edge v0.26.4 source facts (present callback passes the
  raw `IDXGISwapChain*` before edge's GUI; WndProc subclassing is chainable).
- Current host-egui path to AVOID: `crates/edge-sdk/src/gui.rs::ui_from_ptr`.
- Present callback registration: `edge_sdk::Sdk::register_present_callback`
  (see `crates/honse-services/src/frame.rs` for an existing user).
- ABI lockstep background: `rust-toolchain.toml` (why host egui is a trap).

## Acceptance

1. honse-debug renders its own egui window (label + button + text field) over
   the game via its own egui context + own renderer crate, driven from the
   edge present callback swapchain pointer.
2. Mouse + keyboard interact with that window via a WndProc subclass that
   calls through to the previous proc; game input unaffected when the window
   is not hovered/focused.
3. Window size changes (resize / fullscreen toggle) do not crash or stretch —
   RTV recreated on backbuffer size change.
4. Edge's own menu still opens and renders correctly on top/below ours; no
   DX11 state corruption artifacts in the game render.
5. Written notes in the plan on: egui version used (independent of host),
   frame cost, and any coexistence issues found — these feed the follow-up
   productization plan.

## Non-goals

- No integration with tracker/race-hud UI yet.
- No teardown-safe unload polish (dev plugin; note issues, don't solve).
- No upstreaming to honse-services yet.
