# Idle career file format

One file per finished Independent Training, written by the plugin when
`save_idle_careers` is on and read by the career viewer. `format_version` is 1.

`idle-career-format.example.json` next to this file is a complete, trimmed
example. The shared crate's tests parse it and re-serialise it, so it is by
construction a file the current code writes.

## Shape

```json
{
  "format": "honse-tracker/idle-career",
  "format_version": 1,
  "captured_at": "2026-09-02T01:24:56-04:00",
  "source": {
    "plugin_version": "0.4.0",
    "callback": "end",
    "response_type": "IdleSingleModeEndResponse"
  },
  "unreadable": [],
  "response": {
    "data": {
      "end_info": { "...": "..." },
      "progress_log_info": { "...": "..." }
    }
  }
}
```

Two halves, with a hard line between them:

- **Everything outside `response` is ours.** It says when and what wrote the
  file. New metadata goes here, never inside the payload.
- **Everything under `response` is the game's.** It is the deserialised
  `IdleSingleModeEndResponse` (or `...ResultResponse`) as the client held it,
  walked field by field through IL2CPP reflection: the game's own key names,
  its types, its nulls. Nothing is resolved to a name, no enum is spelled out,
  no field is renamed. Readability is the viewer's job.

## Envelope keys

| key | type | meaning |
| --- | --- | --- |
| `format` | string | Always `honse-tracker/idle-career`. Check this before anything else. |
| `format_version` | integer | Bumped only when a reader written for the old number would misread the new file. Adding an envelope key is not that. |
| `captured_at` | string | RFC 3339 with the plugin's local offset, taken when the callback fired. |
| `source.plugin_version` | string | The `honse-tracker` build that wrote the file. |
| `source.callback` | `"end"` or `"result"` | Which of the game's two callbacks produced it. `end` fires when the run is finalised, `result` when its log is opened later. Same payload shape either way. |
| `source.response_type` | string | The game's own type for the payload, to look up in the client. |
| `unreadable` | array | Branches the walk gave up on. See below. |
| `response` | object | The payload. |

## Departures from "verbatim"

A reader has to know these two.

**Account ids are stripped.** `viewer_id` and `owner_viewer_id` (and their
camelCase spellings) are removed at every depth before the file is written.
A folder of these is for analysis and may well be shared.

**Unreadable branches are `null`.** The reflection walk refuses cycles,
nesting past 24 levels, arrays past 4096 elements and strings past a
megabyte. Where it gives up, the payload holds `null` and `unreadable` holds
one entry saying where and why:

```json
"unreadable": [
  { "at": "/data/end_info/chara_info/evaluation_info_array/3/group_outing_info_array",
    "reason": "cycle" }
]
```

`at` is an RFC 6901 JSON pointer relative to `response`. A parser therefore
sees a valid null where it expected a value, never a marker string, and the
explanation lives in one place at the top.

## What to know about the payload

- Keys are sorted. That is what the writer does and it is deliberate: two
  exports diff cleanly.
- `chara_info.start_time` is the one string field, in the server's clock.
- Stat gains under `progress_log_info` (`event_gain_info`,
  `succession_gain_info`, `support_card_gain_info_array[].gain_info`) come as
  `{ "sign": 0, "value": 22 }` pairs. Every sample seen so far has `sign: 0`;
  `1` is read by the viewer as a loss, which has not been observed. Whether
  this pair is the wire shape or a client-side struct is unverified.
- Primitive arrays such as `chara_info.route_race_id_array` are plain
  `[267, 268, ...]`. Files from plugin 0.3 and 0.4 have `{"m_value": 267}`
  elements there instead; that was the walker reading `System.Int32` as a
  struct, not the game.

## Files from before the format

Plugin 0.3 and 0.4 wrote the bare payload with `honse_source` and
`honse_tracker_version` inside it, and the capture time only in the file name.
The viewer still opens those: it lifts them into this envelope on read, taking
`captured_at` from the name. A pre-format file that has been renamed cannot be
lifted and is skipped with a note.

## File names

`20260902_014233-card101302-end.json`: the capture stamp in local time so a
folder sorts chronologically, the trainee's card id so a run is identifiable
without opening it, then the callback. The card id is looked up rather than
required, so a payload that has changed shape is still written under
`20260902_014233-end.json`.
