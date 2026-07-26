//! On-demand, disk-backed PNG textures for the Career panel.
//!
//! Unlike [`super::icons`] (small race-condition sprites embedded in the DLL),
//! the Career panel reuses the game UI sprite set the dashboard already extracted
//! (~16 MB: trainee portraits, rank sprites, stat/skill icons). Those are far too
//! large to embed, so the deploy script stages them under the host data dir
//! (`<game-data>/hachimi/icons/...`) and we load each PNG on demand, decode it
//! once into an [`egui::TextureHandle`], and cache it by relative path.
//!
//! Every lookup is cheap after the first: hits return the cached handle, and
//! *misses are cached too* (as `None`) so a missing asset doesn't re-hit the
//! filesystem every frame. Callers render a text/colored fallback on `None`.
//! (A missing asset stays missing until the overlay reloads — fine, since assets
//! are staged at deploy time.)

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::compat::egui::{self, ColorImage, TextureHandle, TextureOptions};
use crate::compat::Sdk;

/// Relative path (under `icons/`) → decoded texture, or `None` when the file is
/// absent / undecodable (negative cache).
fn cache() -> &'static Mutex<HashMap<String, Option<TextureHandle>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<TextureHandle>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Drop negative-cache entries (paths that resolved to `None` because the file
/// was missing at first lookup). Called after the Career icon set finishes
/// downloading so freshly-fetched icons load on the next frame without a restart.
/// Successfully-decoded handles are kept.
pub fn clear_missing() {
    if let Ok(mut c) = cache().lock() {
        c.retain(|_, v| v.is_some());
    }
}

/// Get (loading + caching on first use) the texture for an icon under the staged
/// `icons/` dir, e.g. `"status_00.png"` or `"statusrank/ui_statusrank_08.png"`.
/// Returns `None` when telemetry/data paths are unavailable, the file is missing,
/// or the PNG cannot be decoded — the caller then draws a fallback.
pub fn texture(ctx: &egui::Context, rel: &str) -> Option<TextureHandle> {
    if let Some(slot) = cache().lock().ok().and_then(|c| c.get(rel).cloned()) {
        return slot;
    }
    let loaded = load(ctx, rel);
    if let Ok(mut c) = cache().lock() {
        c.insert(rel.to_string(), loaded.clone());
    }
    loaded
}

/// Dev-harness override for the icon root directory. When set (by the desktop
/// preview), `icons/{rel}` is resolved under this dir instead of via the host
/// `host_data_path` (which requires a live SDK).
#[cfg(feature = "dev-harness")]
static HARNESS_ICON_ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Point the texture loader at an on-disk `icons/` root for the preview harness.
#[cfg(feature = "dev-harness")]
pub(crate) fn set_harness_icon_root(root: std::path::PathBuf) {
    let _ = HARNESS_ICON_ROOT.set(root);
}

/// Resolve the absolute path for an icon, preferring the harness override when
/// present and otherwise asking the host for its staged data path.
fn resolve_icon_path(rel: &str) -> Option<std::path::PathBuf> {
    #[cfg(feature = "dev-harness")]
    if let Some(root) = HARNESS_ICON_ROOT.get() {
        return Some(root.join(rel));
    }
    Sdk::try_get()?.host_data_path(&format!("icons/{rel}"))
}

fn load(ctx: &egui::Context, rel: &str) -> Option<TextureHandle> {
    let path = resolve_icon_path(rel)?;
    let bytes = std::fs::read(&path).ok()?;
    let img = decode_png_rgba(&bytes)?;
    Some(ctx.load_texture(format!("tt_career_{rel}"), img, TextureOptions::LINEAR))
}

/// Decode a PNG (any color type, incl. palette/16-bit) into tightly-packed
/// RGBA8. `EXPAND` turns indexed→RGB, sub-8-bit grayscale→8-bit, and `tRNS`→
/// alpha; `STRIP_16` collapses 16-bit channels to 8-bit. Returns `None` on error.
fn decode_png_rgba(bytes: &[u8]) -> Option<ColorImage> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let src = &buf[..info.buffer_size()];

    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => src.to_vec(),
        png::ColorType::Rgb => src.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => src.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        // EXPAND should have removed Indexed; bail to fallback if it somehow remains.
        png::ColorType::Indexed => return None,
    };
    if rgba.len() != w * h * 4 {
        return None;
    }
    Some(ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

/// Draw `rel` as a `size`×`size` square image at the cursor, tinted by `tint`
/// (use [`egui::Color32::WHITE`] for no tint). Returns `false` (drawing nothing)
/// when the texture is unavailable, so callers can render a fallback instead.
pub fn image_square(ui: &mut egui::Ui, rel: &str, size: f32, tint: egui::Color32) -> bool {
    let Some(tex) = texture(ui.ctx(), rel) else {
        return false;
    };
    ui.add(
        egui::Image::new(&tex)
            .fit_to_exact_size(egui::vec2(size, size))
            .tint(tint),
    );
    true
}
