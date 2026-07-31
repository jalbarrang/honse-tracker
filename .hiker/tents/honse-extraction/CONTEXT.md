# honse-extraction — intent context

## Invariants

### Compat surface partition

Every method of the fork's in-core `training_tracker/compat.rs` `Sdk` surface lands in **exactly one** provider crate:

1. `edge-sdk` — 1:1 wrappers over edge `get_api`
2. `honse-services` — higher-level host services (overlays, hotkeys, pages, scene views)
3. plugin-local — one-off helpers that stay in a plugin crate

This prevents the plan-drift failure mode where a method is claimed by both crates or by neither. The annotated method list in `crates/edge-sdk/src/sdk.rs` (t-003) is the source of truth for assignments; plan 3 materializes it as `facts.json` for hiker `unique_provider` / `assigned`.

### Crash-safety career lifecycle

IL2CPP reads are permitted in exactly one event-driven state: `CommandSelectActive` (integer value 2 in the Hiker sort). Command submission, Apply responses, view transitions, races, concerts, career loading, and idle/outside-career states all fail closed.

The real decision point is `src/read_gate.rs::reads_permitted(CareerState)`. `src/read_gate.rs::transition` is the pure reducer; runtime hook events update one atomic lifecycle value in `src/career_poll.rs`. Only post-original `SetupCommandSelectStart*` and `OnCompletePlayInCommandView` events may enter the permitted state. Apply hooks report fresh data but cannot permit reads, and view ID 1101 is never itself a settle proof. The capture scheduler checks the same state both when claiming a schedule slot and immediately before any IL2CPP read.

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
