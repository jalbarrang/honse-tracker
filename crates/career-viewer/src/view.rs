//! The pages, as maud markup.
//!
//! Templates are Rust rather than files: the compiler checks the structure, and
//! a field renamed in `career.rs` fails the build instead of rendering an empty
//! cell nobody notices.

use maud::{html, Markup, DOCTYPE};

use crate::assets::Assets;
use crate::career::{Career, Entry, STAT_LABELS};

const STYLE: &str = include_str!("style.css");

// The art is hotlinked and the set is incomplete, so a miss is discovered by
// the browser, not the server. A big image that fails becomes the blank
// placeholder; a small badge that fails just goes away.
const MISS: &str = "this.classList.add('blank')";
const GONE: &str = "this.remove()";

fn page(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                style { (maud::PreEscaped(STYLE)) }
            }
            body { main { (body) } }
        }
    }
}

pub fn index(entries: &[Entry], assets: &Assets, dir: &std::path::Path) -> Markup {
    page(
        "Saved careers",
        html! {
            header.top {
                h1 { "Saved careers" }
                p.sub { (dir.display().to_string()) }
            }
            @if entries.is_empty() {
                p.empty {
                    "Nothing here yet. Finish an Independent Training with "
                    code { "save_idle_careers" }
                    " on and it will appear."
                }
            } @else {
                ul.careers {
                    @for entry in entries {
                        li {
                            a.row href={ "/career/" (entry.file) } {
                                .thumb {
                                    @match assets.chara_icon(entry.card_id) {
                                        Some(url) => img src=(url) alt="" loading="lazy" onerror=(MISS);,
                                        None => span.blank {},
                                    }
                                }
                                .who {
                                    @match &entry.trainee {
                                        Some(name) => span.card { (name) },
                                        None => span.card { "Card " (entry.card_id) },
                                    }
                                    span.when { (entry.when) }
                                }
                                .grade { "Grade " (entry.chara_grade) }
                                .stats {
                                    @for (i, value) in entry.stats.iter().enumerate() {
                                        span.stat { span.k { (STAT_LABELS[i]) } span.v { (value) } }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub fn career(c: &Career, assets: &Assets) -> Markup {
    page(
        &format!("{} · {}", c.trainee.as_deref().unwrap_or("Career"), c.when),
        html! {
            p.back { a href="/" { "← All careers" } }
            header.hero {
                .portrait {
                    @match assets.portrait(c.card_id) {
                        Some(url) => img src=(url) alt="" onerror=(MISS);,
                        None => span.blank {},
                    }
                }
                .facts {
                    @match &c.trainee {
                        Some(name) => h1 { (name) },
                        None => h1 { "Card " (c.card_id) },
                    }
                    p.sub {
                        (c.when) " · card " (c.card_id) " · grade " (c.chara_grade)
                        " · " (c.callback) " · plugin " (c.plugin_version)
                    }
                    .statrow {
                        @for (i, value) in c.stats.iter().enumerate() {
                            .statbox {
                                span.k { (STAT_LABELS[i]) }
                                span.v { (value) }
                                img.rank src=(assets.stat_rank(*value)) alt="" onerror=(GONE);
                            }
                        }
                    }
                    p.sp { "Skill points " span.v { (c.skill_points) } }
                }
            }

            (section_races(c))
            (section_supports(c, assets))
            (section_conditions(c))
            (section_factors(c))
            (section_skills(c, assets))

            p.raw { a href={ "/raw/" (c.file) } { "Raw JSON" } }
        },
    )
}

fn section_races(c: &Career) -> Markup {
    html! {
        @if !c.races.is_empty() {
            section {
                h2 { "Races " span.count { (c.races.len()) } }
                .scroll {
                    table {
                        thead { tr {
                            th { "Turn" } th { "When" } th.num { "Place" } th.num { "Program" }
                            th { "Ground" } th { "Weather" } th { "Style" } th.num { "Fans" }
                        } }
                        tbody {
                            @for r in &c.races {
                                tr {
                                    td.num { (r.turn) }
                                    td.dim { (r.year) " · " (r.date) }
                                    td.num.place[r.rank == 1] { (r.rank) }
                                    td.num.dim { (r.program_id) }
                                    td { (r.ground) }
                                    td { (r.weather) }
                                    td { (r.style) }
                                    td.num { (r.fans) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn section_supports(c: &Career, assets: &Assets) -> Markup {
    html! {
        @if !c.supports.is_empty() {
            section {
                h2 { "Support cards " span.count { (c.supports.len()) } }
                ul.cards {
                    @for s in &c.supports {
                        li.card {
                            img src=(assets.support_card(s.card_id)) alt="" loading="lazy" onerror=(MISS);
                            .gains {
                                @match &s.name {
                                    Some(name) => span.id.name { (name) },
                                    None => span.id { "#" (s.card_id) },
                                }
                                @for (i, value) in s.gains.iter().enumerate() {
                                    @if *value != 0 {
                                        // `{:+}` writes the sign either way, so a loss
                                        // reads "-12" rather than "+-12".
                                        span.gain.loss[*value < 0] {
                                            span.k { (STAT_LABELS[i]) }
                                            span.v { (format!("{value:+}")) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn section_conditions(c: &Career) -> Markup {
    html! {
        @if !c.conditions.is_empty() {
            section {
                h2 { "Conditions" }
                ul.chips {
                    @for cond in &c.conditions {
                        li.chip.good[cond.good].bad[!cond.good].off[!cond.active] {
                            (cond.name) " " span.id { "#" (cond.id) }
                        }
                    }
                }
            }
        }
    }
}

fn section_factors(c: &Career) -> Markup {
    html! {
        @if !c.factors.is_empty() {
            section {
                h2 { "Succession factors" }
                @for year in &c.factors {
                    .year {
                        h3 { "Year " (year.year) }
                        ul.chips {
                            @for f in &year.factors {
                                li.chip { "#" (f.id) @if f.level > 0 { " Lv" (f.level) } }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn section_skills(c: &Career, assets: &Assets) -> Markup {
    html! {
        @if !c.skills.is_empty() {
            section {
                h2 { "Skills " span.count { (c.skills.len()) } }
                ul.chips {
                    @for skill in &c.skills {
                        li.chip {
                            @if let Some(icon) = skill.icon_id {
                                img.skill src=(assets.skill_icon(icon)) alt="" loading="lazy" onerror=(GONE);
                            }
                            @match &skill.name {
                                Some(name) => span { (name) },
                                None => span { "#" (skill.id) },
                            }
                            @if skill.level > 1 { span.id { "Lv" (skill.level) } }
                        }
                    }
                }
            }
        }
    }
}

/// A message page for the handful of ways a request can be wrong.
pub fn message(title: &str, detail: &str) -> Markup {
    page(
        title,
        html! {
            p.back { a href="/" { "← All careers" } }
            h1 { (title) }
            p.empty { (detail) }
        },
    )
}
