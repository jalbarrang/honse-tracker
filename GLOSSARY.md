# Glossary

Terms that come up while working on this plugin, in plain words. Grouped by
where they come from, not alphabetically, because the grouping is half the
explanation.

## The game

**Career** — one full run with one trainee. Everything this plugin tracks lives
inside a career.

**Turn** — one action in a career. Train, rest, race, and so on.

**Honse** — what this project calls the game, and where the repo name comes
from. The game itself is Umamusume Pretty Derby; its code namespace is `Gallop`.

**Trainee** — the uma you are training this career. One per career.

**Facility** — one of the five things you can train: Speed, Stamina, Power,
Guts, Wits. A turn's preview gives each one a failure rate and per-stat gains.
The game calls them training *commands*, which is why the code says `CommandId`,
and it spells the fifth one `Wiz` — grep for that, not for Wits.

**Support card / deck** — the six cards you take into a career. They show up on
facilities, add stat gains, and carry their own event chains. Read from
`EquipSupportCardArray`.

**Bond / friendship** — how far along you are with one support. At 80 it turns
*rainbow*: the card trains at full strength. "Near-rainbow pressure" is our own
number for how close a card is to that threshold, used to rank facilities.

**Motivation / mood** — Great, Good, Normal, Bad, Terrible. An enum 5 down to 1
in the game's own numbering, so bigger is better.

**Energy** — the bar you spend to train. The game stores it as `Hp` / `MaxHp`,
which is not health and has nothing to do with racing.

**Aptitude** — the letter grades for distance, running style, and surface
(`ProperGrade` in the game's code). Feeds our evaluation estimate.

**Evaluation / Rating** — the overall career score, and the badge ladder it maps
to (G through LS24). The game only computes it at career end, so we reproduce it
ourselves in `evaluation.rs` and map it with `rank_table.rs`.

**Condition** — the ongoing good and bad states a trainee picks up
(状態 in the game's data, "chara effect" in its code). `chara_effects.rs` names them
and says which are good.

**Agenda / reserved race** — races you have booked ahead on the schedule.
Stored per deck, read as `(year, program_id)` pairs.

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

**Trackblazer** — scenario id 4, "Make a New Track". The one with RaceCoins and
a rotating coin shop.

**master.mdb** — the game's master database, where scenario, song, and skill
tables live. Useful for checking ids by hand.

**Independent Training** — send a trainee off and a real-world timer runs
(about 45 minutes); you collect the finished career when it lands. The game
calls it `IdleSingleMode` internally, which is why grepping the class dump for
"Independent" finds nothing. Its montage screen is view 6600
(`IdleSingleModePlayCut`), and the deadline itself is
`WorkIdleSingleModeData.EndTime` — the same value the on-screen gauge counts
down. `idle_training.rs` watches that clock so the notification lands whether or
not you are looking at the game.

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

**Gallop** — the game's own C# namespace. Nearly every class we resolve is
`Gallop.Something`.

**Singleton** — the game keeps one live instance of its manager classes and
hands it out on request. Every read starts by asking for one.

**Work data** — the game's live session state, rooted at `WorkDataManager`.
`WorkSingleModeData` is the current career, `WorkSingleModeCharaData` the
trainee. If you are reading something about *right now*, it hangs off here.

**Master data** — the static tables, reached at runtime through
`MasterDataManager` (the same content as master.mdb). Looking things up here does
real work and crashes if called off the main thread — which is why so much of
this plugin prefers a plain field read.

**Main thread** — Unity's. IL2CPP calls that touch master data or game objects
belong on it; the present callback does not run there, so work is handed over
with `schedule_on_main_thread`.

**MinHook / trampoline** — the library Edge patches functions with, and the copy
of the original it leaves you to call. One address can only be hooked once:
Edge already hooks `SceneManager.ChangeView`, which is why our view signal is a
poll instead. (`view_hook.rs`)

**Class dump** — `il2cpp_classes.txt`, produced by the plugin's own menu item.
Every class, field and method name in the build. It carries names but not enum
*values*, so those are declaration order until proven otherwise.

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

**Settled turn** — a turn that has finished changing: the command view is
rebuilt and actionable. The only moment a full career read is allowed, and the
unit telemetry publishes.

**Capture** — one settled-turn read, published once. Requests are held and
coalesced until the lifecycle permits one.

**Epoch** — a career's namespace for capture ids, restarted on a new career, a
deck change, or a turn rewind. Without it, an untouched turn 1 of the same
trainee in two careers would produce the same id twice.

**Deadline** — Independent Training's landing time, remembered so the
notification can fire off the wall clock rather than off anything on screen.
Armed by the slow poll, settled by the per-frame tick. (`idle_training.rs`)

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

**Career viewer** — `crates/career-viewer`, a local site that browses the
Independent Training exports with the game's own art. Outside the workspace's
`default-members`, so a deploy build never compiles it.

**Shared tables** — `crates/honse-career-meta`: the rank ladder, stat-rank
sprites, career calendar and condition names. Its own crate rather than a module
because the viewer needs them too and the plugin drags in the SDK, the overlay
and D3D11 — nothing a viewer should link to look up a badge.

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

**Tray balloon** — `Shell_NotifyIcon` with `NIF_INFO`, which Windows 10/11 draw
as a real notification. It needs a tray icon to hang off, which is why one
appears the first time the plugin notifies. The modern toast API would need a
Start Menu shortcut we have no business installing.

**Focus Assist / Do Not Disturb** — Windows suppressing notifications, on by
default while a fullscreen app is in front. The taskbar flash still gets
through.

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

**`honse-tracker.json`** — everything this plugin remembers, in one document:
settings, panel positions, song plan. One owner (`config.rs`) writes it, because
`PluginConfig` round-trips a whole file and two owners sharing one path would
erase each other. Replaces `honseTrackerConfig.json`, `overlayLayout.json` and
`songPlan.json`, which the first launch folds in and then leaves alone.

**`telemetry.json`** — telemetry endpoint and token. Absent means telemetry is
off, which is the default.

**`hachimi/config.json`** — where the DLL is listed under `load_libraries` so
Edge loads it at startup.

**`il2cpp_classes.txt`** — the class dump, written next to the game exe. The
reference for every class and method name in this repo.
