# egui 0.34 → 0.33.3 / egui_taffy 0.12 → 0.10 downgrade notes

Verified against the cargo registry checkouts:

- `~/.cargo/registry/src/.../egui-0.33.3`
- `~/.cargo/registry/src/.../epaint-0.33.3`
- `~/.cargo/registry/src/.../ecolor-0.33.3`
- `~/.cargo/registry/src/.../egui_taffy-0.10.0`

Cross-checked against the 0.34.3 counterparts where an API looked new.

## Dependency pin

| crate | fork | honse-tracker |
|---|---|---|
| `egui` | `0.34` (default-features false) | workspace `=0.33.3` (same pin as hachimi-edge / edge-sdk) |
| `egui_taffy` | `=0.12.0` | workspace `=0.10.0` |

## Source substitutions (honse-ui)

`honse-ui` (519 LOC across `components.rs` / `paint.rs` / `theme.rs` / `lib.rs`) compiled against egui 0.33.3 **with no source edits**. Every API used was confirmed present in the 0.33.3 registry sources:

| API used in fork (0.34) | 0.33.3 status | Evidence |
|---|---|---|
| `egui::CornerRadius` / `CornerRadius::same(u8)` / `CornerRadius::ZERO` | present (epaint 0.33.3 `corner_radius.rs`) | `pub const fn same(radius: u8)`; re-exported from `egui` |
| `egui::Rounding` (deprecated alias) | present as `type Rounding = CornerRadius` | `egui-0.33.3/src/lib.rs` |
| `egui::Margin::symmetric(i8, i8)` / `Margin::same(i8)` / `Margin::ZERO` | present (epaint 0.33.3 `margin.rs`) | `pub const fn symmetric(x: i8, y: i8)`; same signature in 0.34.3 |
| `egui::Frame::new()` / `.fill` / `.stroke` / `.corner_radius` / `.inner_margin` | present | `egui-0.33.3/src/containers/frame.rs` `pub const fn new()` |
| `Button::corner_radius` | present | `egui-0.33.3/src/widgets/button.rs` (`.rounding` deprecated alias also present) |
| `StrokeKind::Inside` | present | re-exported from epaint via `egui` lib.rs |
| `Color32::from_white_alpha` / `from_black_alpha` / `linear_multiply` | present | `ecolor-0.33.3/src/color32.rs` |
| `Sense::hover`, `Align2`, `FontId`, `TextStyle`, `TextWrapMode`, `ComboBox`, `Slider::trailing_fill` | present | used unchanged; crate builds clean |

**No old→new code substitutions were required.** The fork's 0.34 surface for these symbols is source-compatible with 0.33.3.

## egui_taffy 0.12 → 0.10

`honse-ui` only `pub use egui_taffy;` — it does not call taffy APIs in-crate. `egui_taffy` 0.10.0 exports the same public entry points used by consumers (`tui`, `TuiBuilder`, `TuiBuilderLogic`, `tid`, …). No source changes.

## Telemetry vendoring notes

- Package renamed to `honse-telemetry`; `[lib] name = "hachimi_telemetry"` kept so `hachimi_telemetry::` / `hachimi_telemetry::pb` imports stay valid for plans 3/4.
- Config path is already injected (`init(cfg_path: Option<PathBuf>)`) — no fork-host `Hachimi::instance()` / `crate::core` coupling to remove.
- `proto/` + `build.rs` (prost 0.13 + protox, no system `protoc`) copied verbatim.
