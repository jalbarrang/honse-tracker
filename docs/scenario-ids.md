# Single Mode scenario IDs

Scenario dispatch uses the raw value returned by
`Gallop.WorkSingleModeCharaData.get_ScenarioId()`. That value is the primary key
`single_mode_scenario.id`; it is **not** the scenario's release-order `sort_id`,
`scenario_image_id`, `turn_set_id`, or an element position in
`WorkSingleModeData._scenarioIdList`.

## Current Global build mapping

| Raw `id` | `sort_id` | IL2CPP `SingleModeDefine.ScenarioId` | Scenario |
|---:|---:|---|---|
| 1 | 1 | `URA` | The Beginning: URA Finale |
| 2 | 2 | `TeamRace` | Unity Cup: Shine On, Team Spirit! (Aoharu Hai) |
| 3 | 4 | `Live` | Brighter Together: Our Grand Concert (Grand Live) |
| 4 | 3 | `Free` | Trackblazer: Start of the Climax (Make a New Track) |
| 5 | not active in `single_mode_scenario` yet | `Venus` | Grandmasters: Legacies Immortal |

The important trap is IDs 3 and 4: Trackblazer was released third, but its raw
ID is 4. Grand Live was released fourth, but its raw ID is 3. Release order must
not be used for dispatch.

This build exposes no `ScenarioId` enum member beyond `Venus`. It contains empty
preloaded master tables for later mechanics such as Arc, but those do not prove
an active scenario ID and are intentionally not mapped.

## Evidence

### Installed master data

Source at investigation time:

```text
%LOCALAPPDATA%Low/Cygames/Umamusume/master/master.mdb
```

Reproduce the active mapping with:

```sql
SELECT s.id, s.sort_id, t.text
FROM single_mode_scenario AS s
LEFT JOIN text_data AS t
  ON t.category = 119 AND t."index" = s.id
ORDER BY s.id;
```

Result:

```text
1 | 1 | The Beginning: URA Finale
2 | 2 | Unity Cup: Shine On, Team Spirit!
3 | 4 | Brighter Together / Our Grand Concert
4 | 3 | Trackblazer: Start of the Climax
```

`text_data` category 119 also has index 5, `Grandmasters / Legacies Immortal`.

### IL2CPP metadata

`il2cpp_classes.txt` declares `SingleModeDefine.ScenarioId` in this order:

```text
URA, TeamRace, Live, Free, Venus
```

It also shows `WorkSingleModeCharaData._scenarioId` plus
`get_ScenarioId()`, and separate work objects/accessors for `ScenarioLive` and
`WorkScenarioFree`. The tracker resolves the character getter directly in
`src/memory_reader/chain.rs`; it does not read `_scenarioIdList`.

### Runtime

The lifecycle-state-machine deployment logged a known Grand Live career as:

```text
Command info: scenario_id=3 ...
Grand Live performance: Da=... Pa=... Vo=... Vi=... Co=...
```

This simultaneously verifies the extracted raw ID and the active Live work
object in the installed build.

## Failure analysis

The original bug was fallback/probe dispatch: `get_WorkScenarioFree()` can be
non-null during Grand Live, so probing Trackblazer first could claim a Grand Live
career. Commit `ae58b27` removed that probe order and dispatched on the raw ID.

The current extraction is the correct field/getter, scenario state is rebuilt
for every `CareerSnapshot`, and there is no cached `ScenarioState` that can leak
between careers. Dispatch is now explicitly fail-closed:

- raw ID 3 can call only the Grand Live reader;
- raw ID 4 can call only the Trackblazer reader;
- every other ID is unsupported and calls neither reader;
- a selected reader returning `None` does not fall back to another reader.
