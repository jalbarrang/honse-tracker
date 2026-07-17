# Fix: plugin game-init bootstrap (hotkeys dead + tracking crash)

Base commit: f8af932. Game: Umamusume on Hachimi-Edge v0.26.4. Windows cdylibs.

## Single root cause (verified against edge source + user's hachimi.log)

Our plugins defer IL2CPP-dependent setup to
`edge_sdk::Sdk::register_on_game_initialized(...)`. **Edge never invokes our
callback**, so ALL deferred setup is dead:

- `on_hooking_finished` (edge `src/core/hachimi.rs:364-393`) calls
  `GameSystem::on_game_initialized()` at line 372 **before** the
  `for plugin in ... { plugin.init() }` loop (~line 385). Our
  `register_on_game_initialized` runs inside `plugin.init()` — i.e. AFTER the
  event already fired. Registration function only pushes to a Vec; it does NOT
  call late registrants. So the catch-up call misses us.
- The only OTHER caller is `GameSystem::InitializeGame_MoveNext`
  (edge `src/il2cpp/hook/umamusume/GameSystem.rs:46`), armed by
  `InitializeGameCommon` — which **early-returns when `ui_scale == 1.0`**
  (GameSystem.rs:51). User's config has `ui_scale: 1.0`, so this hook is never
  installed and game-init never re-fires.

Log proof (user session, 58k lines): ZERO occurrences of "Skill shop",
"Command-suspend", "hooked ChangeView", "honse-services", "GameTora catalog
ready" — every one of those is logged from a game-init callback body. The
bodies never ran.

### Why this produces both reported symptoms

1. **Alt+1..6 / Alt+0 / Alt+T hotkeys dead.** honse-services installs its
   present-callback frame source (`frame.rs install_frame_source`) from
   `init.rs on_game_initialized`. Never fired → no present callback → frame
   jobs never run → `hotkeys::poll_hotkeys` never polls. Alt+9 works only
   because honse-debug registers its present callback + WndProc DIRECTLY at
   plugin_init (`own_overlay.rs`), bypassing game-init entirely.
2. **Start-tracking crash.** `start_tracking` → `request_refresh_immediate` →
   `schedule_on_main_thread(refresh_cache_cb)` → `refresh_cache_inner` walks
   Single Mode objects. The read gate `reads_unsafe()` = `in_view_transition()
   || reads_suspended()`. Both inputs are driven by game-init hooks that never
   installed (view hook + command-suspend hooks), so the gate is permanently
   OPEN. Reading `HomeInfo`/`TurnInfo` during a scene teardown races a
   use-after-free → access violation (uncatchable by catch_unwind) → hard crash.

Additional layer for the crash: even with correct timing, honse-services'
`ChangeView` self-hook (`view_hook.rs`) fails — edge already owns that address
and MinHook won't chain (`MH_ERROR_ALREADY_CREATED`). So VIEW_CHANGE needs a
non-hook source.

## Fix

Stop depending on edge's game-init. Bootstrap from the first present tick (the
present callback CAN be registered at plugin_init and DOES fire — the spike
proves it), and get VIEW_CHANGE from per-frame view-id polling instead of the
un-chainable ChangeView hook.

## Files
- crates/honse-services/src/init.rs — register present frame source at init();
  add first-present one-shot + `register_on_game_ready` listener list.
- crates/honse-services/src/frame.rs — present_trampoline drives the one-shot.
- crates/honse-services/src/view_hook.rs — replace ChangeView self-hook with
  per-frame current-view-id poll (dispatch_view_change on diff). Investigate
  the current-view getter (Gallop.SceneManager) via edge C API; reuse
  scene_views. Keep dispatch_view_change signature.
- plugins/honse-tracker/src/entry.rs, plugins/honse-race-hud/src/lib.rs,
  plugins/honse-debug/src/lib.rs — replace
  `edge_sdk::Sdk::register_on_game_initialized` with
  `honse_services::register_on_game_ready`.

## Verification gate (in-game; user runs)
With enable_file_logging true, launch and check hachimi.log for:
"Skill shop visibility hooks installed", "Command-suspend hooks installed",
a view-change dispatch line. Then: Alt+2 toggles Training panel; start tracking
and navigate several screens without crashing.
