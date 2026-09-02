# Honse Tracker

A HUD for the Honse game, as a plugin for [Hachimi-Edge](https://github.com/Hachimi-Hachimi/Hachimi).

It shows you what the game already knows but does not put on screen: what each
training facility is really offering this turn, and — in Grand Live — where your
performance tokens are going and what you still owe for the songs you want.

It reads the game and draws. It does not play it for you, and it does not
recommend: the trade stays yours.

## Install

1. Download `honse_tracker.dll` from the
   [latest release](https://github.com/jalbarrang/honse-tracker/releases/latest).
2. Put it in the `hachimi` folder inside your game folder — the same folder as
   `config.json`, not the game root.
3. Add it to `load_libraries` in `hachimi/config.json`:

   ```json
   { "load_libraries": ["hachimi\\honse_tracker.dll"] }
   ```

4. Restart the game. Edge loads plugins at startup only, so a running game will
   not pick it up.

You should see a "Training Tracker loaded!" notification, and the panels appear
once you are in a career.

The release is built for **Hachimi-Edge v0.26.4**. The overlay renders on its
own statically-linked egui, so it does not have to match the Edge binary's
compiler — but the plugin ABI does have to exist, so older Edge builds will not
load it.

## The panels

| Panel | Corner | What it shows |
| --- | --- | --- |
| **Training** | top-right | The five facilities: stat gains, failure rate, and energy cost. The biggest total gain is highlighted with its cost beside it. |
| **Performance** | top-left | Grand Live only. Your five tokens against their current ceiling, then the songs you still owe for and what they cost. |
| **Lessons** | bottom-right | Grand Live only, on the Techniques Shop. Every square on offer in the game's own order, and what the ones you cannot take are short by. |
| **Planner** | top-left | Opened with a key. Pick which songs you are saving for, per concert. |
| **Independent Training** | top-right | How long is left on the real-world timer, and the clock time it lands at. The only panel that shows outside a career. |
| **Debug** | bottom-left | Off by default. Which screen you are on and why panels are or are not painting. |

Panels dim when the numbers are not fresh, and disappear entirely during races
and cutscenes — there is no decision to support there. A dimmed panel is showing
the last settled turn, which is also the turn it next applies to.

## Shortcuts

Every shortcut is **Ctrl+Shift** plus a key. The game reads keys without
checking modifiers, so the plugin takes them out of the message stream before
the game sees them — a bound chord does not reach it.

| Shortcut | Does |
| --- | --- |
| `Ctrl+Shift+O` | Show/hide the whole overlay |
| `Ctrl+Shift+P` | Open/close the song planner |
| `Ctrl+Shift+D` | Show/hide the screen debug readout |
| `Ctrl+Shift+I` | Show/hide the Independent Training timer |
| `Ctrl+Shift+M` | Enter/leave layout mode |

### In the planner

| Shortcut | Does |
| --- | --- |
| `Ctrl+Shift+↑` / `↓` | Previous / next song |
| `Ctrl+Shift+←` / `→` | Previous / next concert |
| `Ctrl+Shift+Space` | Plan or skip the selected song |
| `Ctrl+Shift+B` | Mark the selected song bought, or un-mark it |
| `Ctrl+Shift+R` | Reset this concert to the default plan |

The mouse works too: **left-click** plans a song, **right-click** marks it
bought. Clicks only go to the planner while it is open — everywhere else they
reach the game as normal.

### In layout mode

| Shortcut | Does |
| --- | --- |
| `Ctrl+Shift+N` | Select the next panel |
| `Ctrl+Shift+A` | Send it to the next corner |
| `Ctrl+Shift+↑↓←→` | Nudge it, four pixels at a time. Hold to repeat |
| `Ctrl+Shift+R` | Put it back where it started |

You can also just drag a panel with the mouse. Positions are saved as a corner
plus a distance from it, so changing resolution does not move anything.

## The Hachimi menu

Five items, for things you set once rather than press:

- **Toggle debug overlay** — same as `Ctrl+Shift+D`.
- **Toggle Independent Training timer** — same as `Ctrl+Shift+I`.
- **Toggle Independent Training export** — see below.
- **Toggle race cut-in skip** — see below.
- **Dump IL2CPP classes** — writes `il2cpp_classes.txt` next to the game
  executable. Only useful if you are working on the plugin.

## Settings

Everything the plugin remembers lives in one file, `hachimi/honse-tracker.json`:

```json
{
  "settings": {
    "skip_race_skill_cutins": false,
    "save_idle_careers": true,
    "idle_career_dir": ""
  },
  "hosted_data": {},
  "layout": {},
  "song_plan": {}
}
```

`layout` is where you dragged each panel and `song_plan` is what you are saving
for; both are written for you and there is no reason to edit them by hand. The
`settings` block is the part worth opening the file for.

**`skip_race_skill_cutins`** — when a unique skill fires in a race, the game
stops to play a cinematic. Turn this on and it does not play: the skill banner
still appears and the race carries on. Off by default, and the hook is not even
installed until you turn it on, because it is the only thing here that changes
what the game does rather than reporting on it. The Hachimi menu toggles it
without a restart and remembers your choice.

The game has its own, gentler version of this in its options — **Cut-in Play
Mode**, with a **Short** setting. Worth trying first; this is for when you want
none at all.

**`save_idle_careers`** — when an Independent Training finishes, the server
sends the whole run in one response: every stat, every skill learned, the race
history, the succession factors, what each support card contributed. The game
shows you a summary and drops the rest. This writes the response to disk first,
as pretty-printed JSON, so you can analyse it later.

On by default — it only reads, and the data is gone once you have clicked
through the summary. Turn it off in the Hachimi menu if you do not want the
files.

**`idle_career_dir`** — where those files go. Empty means
`%USERPROFILE%\Documents\SavedIdleCareers`. A relative path resolves under your
user profile, not under the game folder, so you cannot accidentally aim it
somewhere Windows will refuse to write.

Files are named `20260902_014233-card101302-end.json`: timestamp first so the
folder sorts chronologically, then the trainee's card id, then which of the
game's two result callbacks produced it (`end` when the run is finalised,
`result` when you view it — same payload, kept apart so you can tell). Each file
also carries `honse_source` and `honse_tracker_version`.

## Files it writes

- `hachimi/honse-tracker.json` — everything above, plus your panel positions and
  song plan. Deleting it is safe; you get the defaults back.
- `Documents\SavedIdleCareers\*.json` — one file per finished Independent
  Training, if `save_idle_careers` is on. See `idle_career_dir` above.

Upgrading from an older build, the first launch folds `honseTrackerConfig.json`,
`overlayLayout.json` and `songPlan.json` into the single file and says so in the
log. The three originals are left where they are rather than deleted, so
dropping back to an older DLL still finds its settings.
