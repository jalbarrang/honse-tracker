//! Display mapping for career state values (pure, no IL2CPP).

/// Map motivation enum value to display string.
pub fn mood_label(m: i32) -> &'static str {
    match m {
        5 => "\u{2b06}\u{2b06} Great",    // ⬆⬆
        4 => "\u{2b06} Good",             // ⬆
        3 => "\u{27a1} Normal",           // ➡
        2 => "\u{2b07} Bad",              // ⬇
        1 => "\u{2b07}\u{2b07} Terrible", // ⬇⬇
        _ => "???",
    }
}

/// Map motivation to color (r, g, b).
pub fn motivation_color(m: i32) -> (u8, u8, u8) {
    match m {
        5 => (255, 200, 50),  // Gold
        4 => (100, 220, 100), // Green
        3 => (200, 200, 200), // Gray
        2 => (100, 150, 255), // Blue
        1 => (255, 70, 70),   // Red
        _ => (200, 200, 200),
    }
}
