//! Low-level pure egui painters.

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, Ui};

/// Paint a vertical two-stop gradient inside `rect` (rounded `corner`), top→bottom.
pub fn vgrad(ui: &Ui, rect: Rect, top: Color32, bottom: Color32, corner: u8) {
    const BANDS: usize = 12;
    let painter = ui.painter();
    for i in 0..BANDS {
        let t0 = i as f32 / BANDS as f32;
        let t1 = (i + 1) as f32 / BANDS as f32;
        let y0 = rect.top() + rect.height() * t0;
        let y1 = rect.top() + rect.height() * t1;
        let c = lerp_color(top, bottom, (t0 + t1) * 0.5);
        let band = Rect::from_min_max(Pos2::new(rect.left(), y0), Pos2::new(rect.right(), y1));
        let r = if i == 0 {
            CornerRadius {
                nw: corner,
                ne: corner,
                sw: 0,
                se: 0,
            }
        } else if i == BANDS - 1 {
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: corner,
                se: corner,
            }
        } else {
            CornerRadius::ZERO
        };
        painter.rect_filled(band, r, c);
    }
}

#[must_use]
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

#[must_use]
pub fn brighten_toward_white(c: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8| (f32::from(x) + (255.0 - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(lerp(c.r()), lerp(c.g()), lerp(c.b()))
}

/// Paint the diagonal highlight-stripe overlay used by green section strips.
pub fn strip_stripes(ui: &Ui, rect: Rect) {
    let painter = ui.painter();
    let clip = painter.with_clip_rect(rect);
    let step = 14.0;
    let mut x = rect.left() - rect.height();
    while x < rect.right() + rect.height() {
        let alpha = (((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * 36.0) as u8;
        let top = Pos2::new(x + rect.height() * 0.5, rect.top());
        let bot = Pos2::new(x - rect.height() * 0.5, rect.bottom());
        clip.line_segment([top, bot], Stroke::new(3.0, Color32::from_white_alpha(alpha)));
        x += step;
    }
    painter.line_segment(
        [
            Pos2::new(rect.left() + 4.0, rect.top() + 1.0),
            Pos2::new(rect.right() - 4.0, rect.top() + 1.0),
        ],
        Stroke::new(1.0, Color32::from_white_alpha(40)),
    );
}
