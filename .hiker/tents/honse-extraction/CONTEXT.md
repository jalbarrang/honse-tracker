# honse-extraction — intent context

## Invariants

### Compat surface partition

Every method of the fork's in-core `training_tracker/compat.rs` `Sdk` surface lands in **exactly one** provider crate:

1. `edge-sdk` — 1:1 wrappers over edge `get_api`
2. `honse-services` — higher-level host services (overlays, hotkeys, pages, scene views)
3. plugin-local — one-off helpers that stay in a plugin crate

This prevents the plan-drift failure mode where a method is claimed by both crates or by neither. The annotated method list in `crates/edge-sdk/src/sdk.rs` (t-003) is the source of truth for assignments; plan 3 materializes it as `facts.json` for hiker `unique_provider` / `assigned`.

### Crash-safety read gate

IL2CPP reads are permitted only when **both** independent gates are open:

1. **View-transition cooldown** — `SceneManager.ChangeView` sets a cooldown so reads do not race a tearing-down view hierarchy.
2. **Command-submit suspension** — `SingleModeMainViewController` command submission increments a suspend depth so reads do not race command-driven mutations.

Collapsing these into one gate is the guarded-against shortcut: a use-after-free crash results if either mechanism is dropped. The real decision point is `src/read_gate.rs::reads_permitted`, which requires both `view_cooldown_active == 0` and `command_suspend_depth == 0` before `permitted == 1`. Every consumer routes through it: the event-driven capture scheduler in `src/career_poll.rs` checks it both when pumping a held capture request (`take_schedule_slot`) and again inside the main-thread capture callback, and the settle diagnostics check it before any turn read. The law holds regardless of what *drives* captures (the interval poll was replaced by passive settle edges; the gate semantics did not change).

### Layering / lockstep

- `edge-sdk` must never depend on `honse-services` (`sdk_depends_on_services`).
- No git-sourced egui (`git_sourced_egui`) — registry pin matching `hachimi-edge` `Cargo.lock` only.
- No imports of the fork's `hachimi_plugin_abi` / `hachimi_plugin_sdk` (`fork_abi_import`).

## Code anchors

- Compat partition list: `crates/edge-sdk/src/sdk.rs` module doc (t-003).
- Read gate: `src/read_gate.rs` (law) consumed by `src/career_poll.rs` (event-driven capture scheduler + settle diagnostics).
- Fork references (read-only): `apps/hachimi/src/il2cpp/hook/umamusume/SceneManager.rs`, `apps/hachimi/src/il2cpp/hook/umamusume/SingleModeMainViewController.rs`.

## Expressiveness boundary

Totality ("every compat method has an assignment") is **not** expressible in hiker laws. Plan 3's audit compares `facts.json` row count to the compat method count instead.
