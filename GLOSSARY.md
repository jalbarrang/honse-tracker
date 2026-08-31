# Glossary

Terms that come up while working on this plugin, in plain words. Grouped by
where they come from, not alphabetically, because the grouping is half the
explanation.

## The game

**Career** — one full run with one trainee. Everything this plugin tracks lives
inside a career.

**Turn** — one action in a career. Train, rest, race, and so on.

**Scenario** — the ruleset a career runs under: URA, Aoharu, Grand Live,
Trackblazer, Grandmasters. Dispatch uses the raw `id` from master data, and IDs
3 and 4 are swapped relative to release order. See `docs/scenario-ids.md`.

**Grand Live** — scenario id 3, "Brighter Together: Our Grand Concert". The one
with performance tokens and songs. The game calls the scenario *Grand Concert*
in places, which is also the name of its final concert.

**Performance tokens** — the five things you bank in Grand Live: Dance, Passion,
Vocal, Visual, Composure. Songs and lessons cost them.

**Cap / ceiling** — the highest a single token can go right now. It rises
between concerts: 200, 250, 300, 350, 400. We read it live rather than
hardcoding a turn table, and it is how the plugin knows which concert you are in.

**Concert window** — which concert you are saving for, 1 to 5. Windows 1-4 each
offer songs. Window 5 is the closing Grand Concert: it raises the cap but adds
no songs of its own.

**Carry-over** — a song you planned but did not buy before its concert is still
buyable later, and still owed. This is why totals are counted *through* a
window, not just *at* it.

**Techniques Shop / Lessons** — the Grand Live shop screen where tokens are
spent. View id 1620.

**Square** — one buyable cell in that shop (`TreeSquareInfo` in the game's code).
A square is either a lesson or a song.

**Live id** — the game's id for a song. Names are looked up from it via
`MasterSingleModeLiveSongList`.

**master.mdb** — the game's master database, where scenario, song, and skill
tables live. Useful for checking ids by hand.

**Cut-in** — the animation that interrupts a race when a skill fires. A unique
reserves two: the eye flash (`Eye`) and the animation itself (`Unique` /
`UniqueRare`). `CutInPlayMode` is the game's own setting for them — Long,
LongOnceADay, Short — and has no Off, which is why `race_cutin.rs` exists.

## The host

**Hachimi** — the mod the game loads. **Edge** is the fork with a plugin API.
This project is a plugin for that API, not part of Hachimi itself.

**Present callback** — `hachimi_register_present_callback`. The only hook that
runs every frame regardless of whether Hachimi's own menu is open, which is why
the overlay is built on it.

**IL2CPP** — Unity's way of shipping C# as compiled C++. Reading game state
means resolving classes and methods through the IL2CPP runtime and calling them.

**View id** — an integer naming the screen you are on, from
`SceneManager.GetCurrentViewId()`. 1101 is career training, 1620 is the
Techniques Shop, and so on. The full table is `scene_views.rs`.

**ObscuredInt** — an anti-cheat struct (CodeStage) the game stores some numbers
in. The real value is `hiddenValue XOR cryptoKey`. It has more fields after
those two, so an array of them does **not** step 8 bytes at a time — ask the
runtime for the element size, and ask about the *element* class, not the array
class.

**Stride** — how many bytes to step from one array element to the next. Nothing
to do with songs or races: it is a memory word. Getting it wrong reads across
field boundaries and produces numbers that look real.

## This plugin

**View** — our enum for "which screen is this", one variant per known view id.
Identity only: it says what the screen is, never what to do about it.
(`scene_views.rs`)

**CareerState** — what a screen means for tracking: `CommandSelectActive`,
`CareerMenu`, `AssetTransition`, `RaceActive`, `CutsceneActive`, and so on.
Policy, kept separate from identity so that adding a screen forces a decision.
(`read_gate.rs`)

**Read gate** — the rule that game memory is only read while the player can
actually act (`CommandSelectActive`). Reading at the wrong moment gives numbers
that are mid-change.

**Light refresh** — a narrow exception to that: on shop screens, re-read only
the five stats, energy, and the scenario state, because a purchase moves those
and nothing else. (`memory_reader/snapshot.rs`)

**Snapshot** — the last full read of a turn. Panels draw from it, so they keep
showing the last settled numbers instead of blanking out.

**Face** — how a panel should look right now: `Live` (read this turn),
`Holding` (dimmed, showing the last settled turn), `Away` (a race or cutscene,
draw nothing), `Off` (no career). (`ui/mod.rs`)

**Panel** — one box on screen: training, performance, lessons, planner, debug.
Registered once with a corner and a width.

**Chrome** — the background, border and padding around a panel. Each panel
paints its own, so a panel with nothing to say leaves *nothing* on screen rather
than an empty box.

**Anchor and inset** — a panel's position is a corner plus a distance in from
that corner, never absolute coordinates. A resolution change then moves nothing.

**Layout mode** — the mode for moving panels: drag them, or nudge them with the
arrow keys. Positions are saved per panel.

**Planner** — the song planner. Pick what you are saving for, per concert.

**Ledger** — the planner tracks what is actually *bought*, not just what is
planned, so the shortfall goes down as you buy. Detected from the game where
possible, markable by hand where not. Hand marks are only ever added to what the
game reports, never subtracted.

**Scope** — `Concert(n)` is one concert alone, what the planner edits.
`Through(n)` is that concert and every one before it, what you actually still
owe. (`song_plan.rs`)

## Windows input

**WndProc** — a window's message handler. **Subclassing** it means putting ours
in front and passing on everything we do not want, so overlay keys never reach
the game.

**Raw Input / `WM_INPUT`** — a second, separate way keyboard and mouse events
are delivered. This game reads that one, which is why swallowing only the
classic `WM_KEYDOWN` messages did nothing.

**`GetAsyncKeyState` vs `GetKeyState`** — the first asks "is this key down right
now" and consumes nothing; the second, inside a message handler, reports the
state as of the message being handled. Hotkeys fire from the first; the decision
to swallow uses the second.

**Chord** — a key plus its modifiers. Every overlay chord carries Ctrl+Shift so
a player cannot type it by accident.

**AltGr** — on Windows, AltGr *is* Ctrl+Alt. On a Spanish layout it types
everyday characters, so Ctrl+Alt is not usable as a modifier here.

**Swallow / capture** — dropping an input so the game never sees it. The overlay
only captures the mouse over a panel that has something to click, so the game
stays clickable everywhere else.

**Client pixels vs backbuffer pixels** — mouse messages come in window
coordinates; the overlay draws in backbuffer coordinates. They differ when the
game renders at a scaled resolution, so clicks are converted before use.

## Rendering

**egui** — the UI library the overlay is drawn with. We run our own instance,
separate from Hachimi's, so nothing crosses the plugin boundary.

**egui-directx11** — the renderer that gets egui onto the game's D3D11 surface.
Pinned to an exact version because it must match the same egui we compile.

**Swapchain / backbuffer** — what the game presents each frame, and the image it
presents. We draw into it after the game has finished.

**Gamma vs sRGB target** — egui blends in gamma space, so the overlay needs a
non-sRGB view of the backbuffer or colours come out wrong.

**Tofu** — the empty box a font draws for a character it has no glyph for.
egui's default fonts have no check marks or arrows, so panels use `●`, `○`, `×`
instead. Safe blocks are listed in `overlay/theme.rs`.

## Files

**`hachimi.log`** — the game folder's log. Everything this plugin reports goes
there, and it is the first place to look.

**`songPlan.json`** — planned and hand-marked songs.

**`overlayLayout.json`** — panel positions.

**`hachimi/config.json`** — where the DLL is listed under `load_libraries` so
Edge loads it at startup.
