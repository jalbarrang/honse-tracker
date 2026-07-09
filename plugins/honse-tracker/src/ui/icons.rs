//! Embedded race-condition icon assets (weather / season / time-of-day) and a
//! small egui image-toggle helper that mirrors uma-sim's look: an unselected
//! icon renders **grayscale**, and a selected (or hovered) icon renders in full
//! **color**.
//!
//! The PNG files are vendored under `assets/icons/` and embedded with `include_bytes!`
//! so the DLL is self-contained (no runtime file dependency). Each icon is
//! decoded once into two cached [`egui::TextureHandle`]s — color + grayscale —
//! keyed by its basename (see [`crate::race_context`]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::compat::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

/// `(basename, png-bytes)` for every embedded icon. The basenames match
/// `*::icon_name()` in [`crate::race_context`].
const ASSETS: &[(&str, &[u8])] = &[
    (
        "utx_ico_weather_00",
        include_bytes!("../../assets/icons/utx_ico_weather_00.png"),
    ),
    (
        "utx_ico_weather_01",
        include_bytes!("../../assets/icons/utx_ico_weather_01.png"),
    ),
    (
        "utx_ico_weather_02",
        include_bytes!("../../assets/icons/utx_ico_weather_02.png"),
    ),
    (
        "utx_ico_weather_03",
        include_bytes!("../../assets/icons/utx_ico_weather_03.png"),
    ),
    (
        "utx_txt_season_00",
        include_bytes!("../../assets/icons/utx_txt_season_00.png"),
    ),
    (
        "utx_txt_season_01",
        include_bytes!("../../assets/icons/utx_txt_season_01.png"),
    ),
    (
        "utx_txt_season_02",
        include_bytes!("../../assets/icons/utx_txt_season_02.png"),
    ),
    (
        "utx_txt_season_03",
        include_bytes!("../../assets/icons/utx_txt_season_03.png"),
    ),
    (
        "utx_ico_timezone_00",
        include_bytes!("../../assets/icons/utx_ico_timezone_00.png"),
    ),
    (
        "utx_ico_timezone_01",
        include_bytes!("../../assets/icons/utx_ico_timezone_01.png"),
    ),
    (
        "utx_ico_timezone_02",
        include_bytes!("../../assets/icons/utx_ico_timezone_02.png"),
    ),
];

/// A decoded icon: `[color, grayscale]` texture handles.
type IconPair = [TextureHandle; 2];

fn cache() -> &'static Mutex<HashMap<&'static str, IconPair>> {
    static C: OnceLock<Mutex<HashMap<&'static str, IconPair>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Embedded PNG bytes for a basename, if it is one of our assets.
fn asset_bytes(name: &str) -> Option<&'static [u8]> {
    ASSETS.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
}

/// Decode an embedded PNG into an RGBA8 [`ColorImage`]. Returns `None` on any
/// decode error (the caller then renders a text fallback).
fn decode_rgba(bytes: &[u8]) -> Option<ColorImage> {
    let decoder = png::Decoder::new(bytes);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = &buf[..info.buffer_size()];

    // Normalize whatever color type the PNG used into tightly-packed RGBA8.
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::Grayscale => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => src.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        // Palette/indexed is uncommon for these assets; bail to text fallback.
        png::ColorType::Indexed => return None,
    };
    if rgba.len() != w * h * 4 {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

/// Desaturate a color image to grayscale (Rec. 601 luma), preserving alpha.
fn to_grayscale(img: &ColorImage) -> ColorImage {
    let pixels = img
        .pixels
        .iter()
        .map(|p| {
            let l = (0.299 * p.r() as f32 + 0.587 * p.g() as f32 + 0.114 * p.b() as f32).round() as u8;
            Color32::from_rgba_premultiplied(l, l, l, p.a())
        })
        .collect();
    ColorImage {
        size: img.size,
        pixels,
        source_size: img.source_size,
    }
}

/// Get (loading on first use) the `[color, grayscale]` textures for an icon.
fn icon_pair(ctx: &egui::Context, name: &'static str) -> Option<IconPair> {
    if let Some(pair) = cache().lock().ok().and_then(|c| c.get(name).cloned()) {
        return Some(pair);
    }
    let color_img = decode_rgba(asset_bytes(name)?)?;
    let gray_img = to_grayscale(&color_img);
    let color = ctx.load_texture(format!("tt_icon_{name}"), color_img, TextureOptions::LINEAR);
    let gray = ctx.load_texture(format!("tt_icon_{name}_g"), gray_img, TextureOptions::LINEAR);
    let pair = [color, gray];
    if let Ok(mut c) = cache().lock() {
        c.insert(name, pair.clone());
    }
    Some(pair)
}

/// Draw a clickable race-condition icon toggle of side `size_px`. Renders color
/// when `selected` or hovered, grayscale otherwise, with a subtle accent ring on
/// the selected one. Falls back to a small `selectable_label` (using `fallback`
/// text) if the icon cannot be decoded. Returns the click response.
pub fn icon_toggle(
    ui: &mut egui::Ui,
    name: &'static str,
    fallback: &str,
    selected: bool,
    size_px: f32,
) -> egui::Response {
    let Some([color, gray]) = icon_pair(ui.ctx(), name) else {
        return ui.selectable_label(selected, fallback);
    };

    let size = egui::vec2(size_px, size_px);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let lit = selected || resp.hovered();
    let tex = if lit { &color } else { &gray };
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
    if selected {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            4.0,
            egui::Stroke::new(1.5_f32, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Outside,
        );
    }
    resp.on_hover_text(fallback)
}
