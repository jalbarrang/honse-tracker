# Productize the self-hosted overlay: break the surface-window chains

Base commit: 4b3805c. Prereq reading: .taskman/plans/spike-own-egui-overlay/FINDINGS.md
(GO verdict + 6 encoded gotchas), plugins/honse-debug/src/own_overlay.rs (the proven spike).

## Problem being solved

Tracker/race-hud panels render parasitically from inside each plugin's decorated
"surface" window (honse-services/src/surface.rs) because edge's host-egui API has
no per-frame egui entry point. Consequences the user explicitly hates:
- Showing ANY panel drags the whole config window on screen ("bound to this window").
- Closing the window kills every panel.
- All UI is host-egui (`ui_from_ptr`) -> rustc/egui ABI lockstep applies.

In hachimi-redux the user broke this by patching the host. We do not own edge;
instead we own a full in-plugin egui stack proven by the spike: own egui context +
egui-directx11 renderer driven from edge's present callback (raw IDXGISwapChain*),
input via chained WndProc subclass.

## Direction

1. Extract the spike stack into a new crate `honse-overlay` (or honse-services
   module — decide by dependency shape; edge-sdk must stay below it):
   renderer lifecycle w/ per-frame RTV (NEVER cache backbuffer refs — FINDINGS #1),
   transient-error tolerance (#3), input translation + WndProc chords (#2),
   exactly ONE stack instance per DLL (#4/productization-2).
2. Panel/window registry on OUR context: chromeless egui Areas for panels;
   decorated egui::Window with real user-respected close for config windows;
   reopen via hachimi menu items + hotkeys; persist egui memory (positions) to the
   plugin config dir.
3. Migrate honse-tracker: 6 panels + Tracker config UI off surface/host-egui onto
   honse-overlay. Panels become fully independent of any window. Remove tracker's
   surface usage.
4. Migrate honse-race-hud overlays; then retire surface.rs machinery (watchdog,
   reshow, parasitic draw) from honse-services when no consumer remains.
5. Once no `ui_from_ptr` caller remains, the egui/rustc lockstep constraint no
   longer applies to UI: update README compatibility section + rust-toolchain.toml
   rationale (keep the pin only if some host-egui call survives).

## Verification gates
- Per task: build + clippy + tests green.
- In-game (user): panels toggle via Alt+1..6 with NO surface window appearing;
  config window opens from hachimi menu, closes with [X] and STAYS closed;
  race HUD renders during a race independent of any window; edge menu coexists;
  fullscreen/resolution changes survive (per-frame RTV).
