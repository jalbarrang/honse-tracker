# honse-tracker

Plugins for [Hachimi-Edge](https://github.com/kairusds/Hachimi-Edge) that add training analytics, a race HUD, and a debug viewer for the Honse game.

| Plugin | DLL | Role |
| --- | --- | --- |
| **honse-tracker** | `honse_tracker.dll` | Training tracker overlay and analytics (career panels, recommendations, skill shop helpers). |
| **honse-race-hud** | `honse_race_hud.dll` | Live per-runner heads-up display during races. |
| **honse-debug** | `honse_debug.dll` | Development-only view-transition / debug feed (off unless you load it). |

## Compatibility (read this first)

Each plugin release targets **one specific Hachimi-Edge release** — currently **v0.26.4**.

The plugins draw their UI by casting Hachimi-Edge's raw egui pointer back into `egui` types. That is only sound when both sides are built from the same egui source (`=0.33.3`) **with the same rustc** — Rust struct layout is not stable across compiler versions. A mismatch crashes the game at launch (before the title screen), even though the plugin configs still get generated. `rust-toolchain.toml` pins the exact rustc used by the targeted Edge release (1.96.0 for v0.26.4); `scripts/check-rustc-lockstep.ps1` verifies a build against a `hachimi.dll`.

If the game crashes on boot with these DLLs loaded, first check that your Hachimi-Edge version matches the one named in the plugin release notes, and remove the honse DLLs from `load_libraries` to confirm the game boots without them.

## Installation

1. Install Hachimi-Edge from [`https://github.com/kairusds/Hachimi-Edge/releases/latest`](https://github.com/kairusds/Hachimi-Edge/releases/latest).
2. Download the three plugin DLLs from this repo's [`releases/latest`](https://github.com/jalbarrang/honse-tracker/releases/latest).
3. Place `honse_tracker.dll`, `honse_race_hud.dll`, and (optionally) `honse_debug.dll` in the Honse game folder root — the same directory as the game executable.
4. Open `hachimi/config.json` in that folder and add the DLLs to `load_libraries`:

```json
{
  "load_libraries": [
    "honse_tracker.dll",
    "honse_race_hud.dll",
    "honse_debug.dll"
  ]
}
```

5. Launch the Honse game once. Edge auto-creates plugin configs under the `hachimi/` data directory (same folder as `config.json`):
   - `hachimi/honseTrackerConfig.json` — tracker settings + optional hosted-data URL overrides
   - `hachimi/raceHudConfig.json` — which race-HUD metrics are shown
   - (honse-debug has no persisted config)

## Configuration

### honse-tracker (`honseTrackerConfig.json`)

Flattened tracker fields plus optional `hosted_data` URL overrides. Defaults match the structs in `plugins/honse-tracker/src/config.rs`, `recommend.rs`, `planner.rs`, and `build_profile.rs`:

```json
{
  "stat_targets": [0, 0, 0, 0, 0],
  "recommend": {
    "risk_threshold_pct": 25,
    "all_risky_pct": 30,
    "mood_drop_penalty": 30,
    "failure_stat_loss": 5
  },
  "planner": {
    "lookahead_depth": 2,
    "lookahead_aggressiveness": 0.6,
    "energy_floor_pct": 40,
    "specialty_rainbow_gating": false
  },
  "overlay_zoom": 1.0,
  "build_profile": {
    "name": "Default",
    "objective": "Rank",
    "per_stat_target": [0, 0, 0, 0, 0],
    "stat_weights": [1.0, 1.0, 1.0, 1.0, 1.0],
    "strategy": "LateSurger",
    "target_course_id": 0,
    "ground_condition": "Firm",
    "weather": "Sunny",
    "season": "Spring",
    "time_of_day": "Noon",
    "rush_buffer": 0,
    "recovery_skill_ids": [],
    "notes": ""
  },
  "hosted_data": {
    "gametora_data_url": null,
    "tracker_data_url": null,
    "icons_data_url": null
  }
}
```

- `stat_targets` — legacy per-stat targets; migrated into `build_profile.per_stat_target` when no profile is present.
- `recommend` — smart-recommendation tuning (failure % thresholds and modeled penalties).
- `planner` — lookahead depth/aggressiveness, energy floor, specialty rainbow gating.
- `overlay_zoom` — uniform overlay scale (`0.4`–`2.5`, default `1.0`).
- `build_profile` — active objective, targets, weights, course/strategy context.
- `hosted_data` — optional URL overrides; omit or leave null to use the defaults in Data below.

Enum string values (`objective`, `strategy`, ground/weather/season/time) follow the serde names of the Rust enums; if a field is missing, defaults apply.

### Hotkeys

Default bindings (tracker panels start hidden — toggle them with these or the checkboxes in the Training Tracker menu section):

| Chord | Action |
| --- | --- |
| `Alt+1` … `Alt+6` | Toggle Energy / Training / Bonds / Scenario / Shop / Rank panel |
| `Alt+0` | Toggle all tracker panels |
| `Alt+T` | Start/stop tracking |
| `Alt+7` | Toggle Race HUD (timer + per-uma widgets) |

Rebind the tracker actions in `honseTrackerConfig.json` under `"hotkeys"` (`mods`: Ctrl=1, Shift=2, Alt=4; `vk`: Windows virtual-key code, 0 = unbound; restart the game to apply):

```json
{
  "hotkeys": {
    "training-tracker.toggle_training": { "mods": 4, "vk": 50 },
    "training-tracker.toggle_tracking": { "mods": 0, "vk": 0 }
  }
}
```

Hotkeys fire only while the game window is foreground; they work with the menu closed.

### honse-race-hud (`raceHudConfig.json`)

```json
{
  "shown_metrics": 31
}
```

`shown_metrics` is a bitmask of HP / Velocity / Acceleration / States / Recoveries (default `31` = all five shown). Toggle them from the in-game race-HUD controls; the plugin persists the mask.

## Data

On game-initialized, honse-tracker syncs three hosted snapshots from the [hachimi-redux](https://github.com/jalbarrang/hachimi-redux) data repo into Edge's data directory:

| Set | Default URL | Cache location (under Edge data dir) |
| --- | --- | --- |
| GameTora catalog | `https://raw.githubusercontent.com/jalbarrang/hachimi-redux/main/data/gametora` | `gametora/` (+ `.gametora_cache.json`) |
| Tracker resources | `https://raw.githubusercontent.com/jalbarrang/hachimi-redux/main/data` | data-dir root (+ `.tracker_cache.json`) |
| Career icons | `https://raw.githubusercontent.com/jalbarrang/hachimi-redux/main/data/icons` | `icons/` (+ `.icons_cache.json`) |

These URLs are load-bearing for every deployed plugin. Renaming the data repo, the `main` branch, or the `data/` path breaks downloads until configs override them via `hosted_data`.

## Development

### Build

```bash
cargo build --release
```

Artifacts land at `target/release/honse_tracker.dll`, `honse_race_hud.dll`, and `honse_debug.dll` (on Windows).

### Deploy

On a Windows machine with the Honse game installed:

```powershell
$env:HACHIMI_GAME_DIR = "C:\path\to\game"   # optional override
.\scripts\deploy-windows.ps1 -Build
.\scripts\deploy-windows.ps1 -ConfigHint    # prints load_libraries JSON snippet
```

The script copies only the three plugin DLLs into the game folder root. It never launches or kills the game. If a DLL is locked, close the Honse game and retry.

### EGUI LOCKSTEP RULE

Plugins must build with the egui version pinned in this workspace (`egui = "=0.33.3"` today), which must equal the version resolved in Hachimi-Edge's `Cargo.lock` for the targeted Edge release, plus a compatible rustc. The overlay path casts `*mut c_void` to `&mut egui::Ui`; a mismatched egui (or incompatible rustc) makes that cast unsound and crashes. When Edge ships a new build: read egui from *its* `Cargo.lock`, re-pin this workspace to match, rebuild, and cut a new release.

### Hiker intent

Architectural laws live under `.hiker/tents/`; committed facts under `.hiker/facts/`.

```bash
hiker check .hiker/tents
hiker verify .hiker/tents/honse-extraction/honse-extraction.tent --facts .hiker/facts/honse-extraction.facts.json
hiker gen .hiker/tents/honse-extraction/honse-extraction.tent --target rust --module honse_tracker
cargo test
```

Always run `hiker gen` before `cargo test` — the intent test includes the gitignored file under `.hiker-cache/`.

### Honest losses vs the fork

- **No hot-swap** — Edge loads plugins at startup only; restart the Honse game to reload a rebuilt DLL.
- **No `menu_preview` harness** — future work; iterate against a live game session for now.
- **Surface window jank** — the overlay surface is closable and reappears; known jank carried from the port.
