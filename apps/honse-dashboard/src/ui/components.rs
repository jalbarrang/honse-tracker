//! Small shared UI helpers.

/// Failure-percent severity class, mirroring the prototype's thresholds.
#[must_use]
pub fn fail_class(pct: i32) -> &'static str {
    if pct < 5 {
        "ok"
    } else if pct < 15 {
        "mid"
    } else {
        "high"
    }
}

/// Game aptitude rank (8..1) to its letter.
#[must_use]
pub fn grade_letter(rank: i32) -> &'static str {
    match rank {
        8 => "S",
        7 => "A",
        6 => "B",
        5 => "C",
        4 => "D",
        3 => "E",
        2 => "F",
        1 => "G",
        _ => "—",
    }
}

/// CSS class for a grade letter.
#[must_use]
pub fn grade_class(rank: i32) -> String {
    format!("gr gr-{}", grade_letter(rank))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_severity_thresholds() {
        assert_eq!(fail_class(0), "ok");
        assert_eq!(fail_class(4), "ok");
        assert_eq!(fail_class(5), "mid");
        assert_eq!(fail_class(14), "mid");
        assert_eq!(fail_class(15), "high");
        assert_eq!(fail_class(-1), "ok");
    }

    #[test]
    fn grades_map_game_ranks() {
        assert_eq!(grade_letter(8), "S");
        assert_eq!(grade_letter(7), "A");
        assert_eq!(grade_letter(1), "G");
        assert_eq!(grade_letter(0), "—");
    }
}
