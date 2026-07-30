//! Overview page: the settled-turn decision card plus the side rail, ported
//! from the approved prototype. Handles populated, no-career (empty),
//! settling/stale, offline, loading, and error states.

use dioxus::prelude::*;

use super::components::{fail_class, grade_class, grade_letter};
use super::Tab;
use crate::state::{ago_label, size_label, DashboardVm, Delivery, OverviewVm, Phase, STAT_SHORT};
use crate::storage::now_ms;

#[component]
pub fn OverviewPage(vm: Signal<Option<DashboardVm>>, tab: Signal<Tab>) -> Element {
    let selected = use_signal(|| 0usize);

    let guard = vm.read();
    let Some(dash) = guard.as_ref() else {
        return rsx! {
            div { class: "card decision",
                div { class: "banner idle", b { "Loading…" } " reading the local database." }
            }
        };
    };

    match &dash.phase {
        Phase::Loading => rsx! {
            div { class: "card decision",
                div { class: "banner idle", b { "Loading…" } " reading the local database." }
            }
        },
        Phase::Error(msg) => rsx! {
            div { class: "card decision",
                div { class: "banner error",
                    b { "Storage error." }
                    " {msg}"
                }
            }
        },
        Phase::Empty => rsx! {
            div { class: "card decision",
                div { class: "mast",
                    div {
                        span { class: "eyebrow", "No career in progress" }
                        h1 { "Waiting for a career " em { "— nothing to act on" } }
                        p { class: "sub",
                            "The sidecar is listening. Snapshots appear automatically the first time a career turn settles."
                        }
                    }
                }
                div { class: "empty-note",
                    "No settled turns are stored yet. Start a career in the game with the tracker plugin enabled."
                }
            }
        },
        Phase::Ready(ov) => {
            let ov = ov.as_ref().clone();
            let dash = dash.clone();
            rsx! {
                DecisionSplit { dash, ov, selected, tab }
            }
        }
    }
}

#[component]
fn DecisionSplit(dash: DashboardVm, ov: OverviewVm, selected: Signal<usize>, tab: Signal<Tab>) -> Element {
    let stale = dash.delivery != Delivery::Connected;
    let decision_class = if stale { "decision card stale" } else { "decision card" };
    let now = now_ms();
    let settled_ago = ago_label(now, ov.captured_at_ms);
    let capture_short: String = ov.capture_id.chars().take(12).collect();
    let option_count = ov.options.len();

    rsx! {
        div { class: "split",
            div {
                class: "{decision_class}",
                tabindex: "0",
                onkeydown: move |evt| {
                    let key = evt.key();
                    match key {
                        Key::ArrowDown => {
                            evt.prevent_default();
                            let next = (selected() + 1).min(option_count.saturating_sub(1));
                            selected.set(next);
                        }
                        Key::ArrowUp => {
                            evt.prevent_default();
                            selected.set(selected().saturating_sub(1));
                        }
                        Key::Character(ch) => {
                            if let Ok(n) = ch.parse::<usize>() {
                                if (1..=option_count.min(9)).contains(&n) {
                                    selected.set(n - 1);
                                }
                            }
                        }
                        _ => {}
                    }
                },

                div { class: "mast",
                    div {
                        span { class: "eyebrow", "Career run {ov.career_id} · card {ov.card_id}" }
                        h1 {
                            "Turn {ov.turn} "
                            em {
                                match dash.delivery {
                                    Delivery::Connected => "— awaiting your command",
                                    Delivery::Stale => "— last settled snapshot",
                                    Delivery::Offline => "— no data received",
                                }
                            }
                        }
                        p { class: "sub",
                            "Snapshot settled {settled_ago} ago · capture {capture_short} · {ov.turns_stored} turns stored this run"
                        }
                    }
                    Vitals { ov: ov.clone() }
                }

                match dash.delivery {
                    Delivery::Connected => rsx! {},
                    Delivery::Stale => rsx! {
                        div { class: "banner",
                            b { "Turn {ov.turn} may no longer be actionable." }
                            " These figures are the last settled snapshot and will be replaced the moment the next turn resolves."
                        }
                    },
                    Delivery::Offline => rsx! {
                        div { class: "banner idle",
                            b { "The sidecar is still listening." }
                            " Snapshots resume automatically the next time a career turn settles — nothing needs restarting."
                        }
                    },
                }

                div { class: "band",
                    "Current Stats"
                    span { class: "aside", "run {ov.career_id} · turn {ov.turn}" }
                }
                StatBand { ov: ov.clone() }
                div { class: "stat-foot num",
                    span { "Total " b { "{ov.total_stats}" } }
                    span { "Headroom to caps " b { "{ov.headroom}" } }
                    span { "Δ = since previous turn" }
                }

                div { class: "band rail-gap",
                    "Training Options"
                    span { class: "aside", "ranked by value" }
                }
                div { class: "table-head",
                    div {}
                    div { "Facility" }
                    for name in STAT_SHORT {
                        div { class: "c", "{name}" }
                    }
                    div { class: "r", "Total" }
                    div { class: "r", "Fail" }
                    div { "Support" }
                    div { class: "r", "Value" }
                }
                div { class: "optlist",
                    for (i, opt) in ov.options.iter().cloned().enumerate() {
                        OptionRow { opt, index: i, selected }
                    }
                }

                div { class: "decision-foot",
                    p {
                        "Value is a local heuristic over this snapshot's gains, support bonds, failure risk and remaining stat headroom. One immutable snapshot is stored per settled turn and retry duplicates are discarded by capture id — the game is read, never written."
                    }
                    p { style: "white-space:nowrap",
                        kbd { "1" }
                        "–"
                        kbd { "5" }
                        " pick facility"
                        br {}
                        kbd { "↑" }
                        kbd { "↓" }
                        " move"
                    }
                }
            }

            Rail { dash, ov, tab }
        }
    }
}

#[component]
fn Vitals(ov: OverviewVm) -> Element {
    let (hp, max_hp) = ov.energy;
    let pct = if max_hp > 0 {
        (hp * 100 / max_hp).clamp(0, 100)
    } else {
        0
    };
    rsx! {
        div { class: "vitals",
            div { class: "vh", "Energy" }
            div { class: "vh", "Mood" }
            div { class: "vc",
                span { class: "n num", "{hp} " small { "/ {max_hp}" } }
                div { class: "ebar", i { style: "width:{pct}%" } }
            }
            div { class: "vc",
                span { class: "mood", "{ov.motivation_label}" }
                div { class: "ticks",
                    for i in 1..=5 {
                        span { class: if i <= ov.motivation { "on" } else { "" } }
                    }
                }
            }
        }
    }
}

#[component]
fn StatBand(ov: OverviewVm) -> Element {
    rsx! {
        div { class: "statband",
            for stat in ov.stats.iter() {
                div { class: "h", "{stat.name}" }
            }
            div { class: "h sp", "Skill Pts" }
            for stat in ov.stats.iter().cloned() {
                div { class: "c",
                    div { class: "v num", "{stat.value}" }
                    div { class: "d num",
                        if stat.delta != 0 {
                            span { class: "a", "▲" }
                            "{stat.delta:+}"
                        } else {
                            "\u{a0}"
                        }
                    }
                    if stat.cap > 0 {
                        {
                            let base = ((stat.value - stat.delta).max(0) * 100) as f32 / stat.cap as f32;
                            let delta = (stat.delta.max(0) * 100) as f32 / stat.cap as f32;
                            rsx! {
                                div { class: "track",
                                    i { style: "width:{base:.1}%" }
                                    u { style: "width:{delta:.1}%" }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "c sp",
                div { class: "v num", "{ov.skill_points}" }
                div { class: "d num", "\u{a0}" }
            }
        }
    }
}

#[component]
fn OptionRow(opt: crate::state::TrainingOptionVm, index: usize, selected: Signal<usize>) -> Element {
    let is_sel = selected() == index;
    let lead = opt
        .gains
        .iter()
        .enumerate()
        .max_by_key(|(_, g)| **g)
        .map_or(usize::MAX, |(i, _)| i);
    let fail_label = if opt.fail_pct < 0 {
        "—".to_string()
    } else {
        format!("{}%", opt.fail_pct)
    };
    let fail_cls = fail_class(opt.fail_pct);
    let risk_note = match fail_cls {
        "ok" => "Low. Failure is unlikely at current energy.",
        "mid" => "Moderate. A failed attempt costs progress and mood.",
        _ => "High. Expected value is poor once the failure penalty is priced in.",
    };

    rsx! {
        div {
            class: if is_sel { "opt sel" } else { "opt" },
            role: "button",
            tabindex: "0",
            aria_label: "Rank {index + 1}: {opt.facility_name} training",
            onclick: move |_| selected.set(index),
            onkeydown: move |evt| {
                if matches!(evt.key(), Key::Enter) {
                    selected.set(index);
                }
            },
            if index == 0 {
                span { class: "best", "Best pick" }
            }
            div { class: "opt-main",
                div { class: "rank num", "{index + 1}" }
                div { class: "facility",
                    strong { "{opt.facility_name}" }
                    span { "facility Lv {opt.level}" }
                }
                for (gi, gain) in opt.gains.iter().enumerate() {
                    if *gain == 0 {
                        div { class: "g zero", "·" }
                    } else {
                        div {
                            class: if gi == lead { "g num lead" } else { "g num" },
                            "{gain:+}"
                        }
                    }
                }
                div { class: "total num", "+{opt.total_gain}" }
                div { class: "fail num {fail_cls}", "{fail_label}" }
                div { class: "support",
                    if opt.partners.is_empty() {
                        span { class: "none", "none" }
                    } else {
                        for p in opt.partners.iter() {
                            span {
                                class: if p.hot { "tile hot" } else { "tile" },
                                title: "{p.name}",
                                "{p.initials}"
                            }
                        }
                    }
                }
                div { class: "value-cell",
                    div { class: "n num", "{opt.value}" }
                    div { class: "vbar", i { style: "width:{opt.value}%" } }
                }
            }
            div { class: "detail",
                div {
                    h4 { "Gains" }
                    p {
                        "Ranked {index + 1} of 5 by the local value heuristic for this snapshot."
                    }
                    div { class: "kv",
                        span { "Total stat gain" }
                        b { class: "num", "+{opt.total_gain}" }
                    }
                    div { class: "kv",
                        span { "Supports on facility" }
                        b { class: "num", "{opt.partners.len()}" }
                    }
                }
                div {
                    h4 { "Support at this facility" }
                    if opt.partners.is_empty() {
                        p { style: "color:var(--brown-soft)",
                            "No support cards are present at this facility this turn."
                        }
                    } else {
                        for p in opt.partners.iter() {
                            div { class: "partner",
                                div { class: "who",
                                    span { class: if p.hot { "tile hot" } else { "tile" }, "{p.initials}" }
                                    "{p.name}"
                                    if p.hot {
                                        span { class: "flag", "rainbow" }
                                    }
                                }
                                div { class: "n num", "{p.bond}" }
                                div { class: "bond",
                                    i {
                                        class: if p.hot { "hot" } else { "" },
                                        style: "width:{p.bond.clamp(0, 100)}%",
                                    }
                                }
                            }
                        }
                    }
                }
                div {
                    h4 { "Failure risk" }
                    div { class: "risk-figure num {fail_cls}", "{fail_label}" }
                    p { style: "margin-top:6px;font-size:11px", "{risk_note}" }
                }
            }
        }
    }
}

#[component]
fn Rail(dash: DashboardVm, ov: OverviewVm, tab: Signal<Tab>) -> Element {
    let now = now_ms();
    let record = format!("{} – {}", ov.races.0, (ov.races.1 - ov.races.0).max(0));
    let rating = ov.rating.map_or_else(|| "—".to_string(), |r| r.to_string());

    rsx! {
        div { class: "rail",
            section { class: "panel card",
                div { class: "band",
                    "Career in Progress"
                    span { class: "aside", "run {ov.career_id}" }
                }
                div { class: "pad",
                    p { class: "career-name", "Card #{ov.card_id}" }
                    p { class: "career-sub",
                        "Scenario #{ov.scenario_id} · turn {ov.turn} · {ov.star}★"
                    }
                    div { class: "facts",
                        div {
                            span { class: "cap", "Rating" }
                            b { class: "num", "{rating}" }
                        }
                        div {
                            span { class: "cap", "Fans" }
                            b { class: "num", "{ov.fans}" }
                        }
                        div {
                            span { class: "cap", "Record" }
                            b { class: "num", "{record}" }
                        }
                        div {
                            span { class: "cap", "Total stats" }
                            b { class: "num", "{ov.total_stats}" }
                        }
                    }
                    if let Some(apt) = ov.aptitudes {
                        div { class: "apt",
                            span { class: "lab", "Apt" }
                            div { class: "cells",
                                span {
                                    "Turf "
                                    b { class: "{grade_class(apt.ground_turf)}", "{grade_letter(apt.ground_turf)}" }
                                }
                                span {
                                    "Mile "
                                    b { class: "{grade_class(apt.dist_mile)}", "{grade_letter(apt.dist_mile)}" }
                                }
                                span {
                                    "Med "
                                    b { class: "{grade_class(apt.dist_middle)}", "{grade_letter(apt.dist_middle)}" }
                                }
                                span {
                                    "Long "
                                    b { class: "{grade_class(apt.dist_long)}", "{grade_letter(apt.dist_long)}" }
                                }
                            }
                        }
                    }
                }
            }

            section { class: "panel card",
                div { class: "band", "Reserved Races" }
                div { class: "pad",
                    if ov.reserved_races.is_empty() {
                        p { style: "margin:0;color:var(--brown-soft);font-size:11px",
                            "No races are reserved right now."
                        }
                    } else {
                        for race in ov.reserved_races.iter() {
                            div { class: "nextrace",
                                div {
                                    strong { "Program #{race.program_id}" }
                                    p { "reserved in the in-game agenda" }
                                }
                                span { class: "in chip now num", "year {race.year}" }
                            }
                        }
                    }
                }
            }

            section { class: "panel card",
                div { class: "band",
                    "Skills Acquired"
                    span { class: "aside", "{ov.skill_points} pts banked" }
                }
                if ov.skills.is_empty() {
                    p { style: "margin:12px 0 4px;color:var(--brown-soft);font-size:11px",
                        "No skills acquired yet."
                    }
                } else {
                    for skill in ov.skills.iter().take(6) {
                        div { class: "hint",
                            span { class: "lv", "Lv {skill.level}" }
                            span { "{skill.name}" }
                        }
                    }
                }
                p { class: "hint-foot",
                    b { class: "num", "{ov.skill_points}" }
                    " skill points banked · {ov.skills.len()} skills learned"
                }
            }

            section { class: "panel card",
                div { class: "band",
                    "Recent Turns"
                    button { class: "link", onclick: move |_| tab.set(Tab::History), "Full history" }
                }
                div { class: "turns",
                    for t in ov.recent_turns.iter() {
                        div { class: if t.is_latest { "turn now" } else { "turn" },
                            div { class: "no num", "{t.turn}" }
                            div {
                                strong {
                                    if t.is_latest { "Awaiting command" } else { "Settled turn" }
                                }
                                p { class: if t.summary.starts_with('+') { "up" } else { "" },
                                    "{t.summary}"
                                }
                            }
                            div { class: "when num", "{ago_label(now, t.captured_at_ms)}" }
                        }
                    }
                }
            }

            section { class: "panel card",
                div { class: "band", "Delivery & Storage" }
                div { class: "pad",
                    div { class: "dl-row",
                        span { "Game link" }
                        b {
                            class: match dash.delivery {
                                Delivery::Connected => "num ok",
                                Delivery::Stale => "num warn",
                                Delivery::Offline => "num warn",
                            },
                            match dash.delivery {
                                Delivery::Connected => "Connected",
                                Delivery::Stale => "Stale",
                                Delivery::Offline => "Offline",
                            }
                        }
                    }
                    div { class: "dl-row",
                        span { "Last capture" }
                        b { class: "num", "{ago_label(now, ov.captured_at_ms)} ago" }
                    }
                    div { class: "dl-row",
                        span { "Turns stored, this run" }
                        b { class: "num", "{ov.turns_stored}" }
                    }
                    div { class: "dl-row",
                        span { "Turns stored, all runs" }
                        b { class: "num", "{dash.totals.captures}" }
                    }
                    div { class: "dl-row",
                        span { "Duplicates discarded" }
                        b { class: "num", "{dash.duplicates_discarded}" }
                    }
                    div { class: "dl-row",
                        span { "Database" }
                        b { class: "num ok", "Healthy · {size_label(dash.totals.db_size_bytes)}" }
                    }
                }
            }
        }
    }
}
