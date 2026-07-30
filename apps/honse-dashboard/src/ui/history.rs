//! Career History page: run list with filter, plus a run detail view showing
//! the latest revision per turn.

use dioxus::prelude::*;

use super::AppCtx;
use crate::state::{ago_label, size_label, DashboardVm};
use crate::storage::{now_ms, CareerSummary, TurnView};

#[component]
pub fn HistoryPage(vm: Signal<Option<DashboardVm>>) -> Element {
    let filter = use_signal(String::new);
    let selected = use_signal(|| Option::<i64>::None);
    let turns = use_signal(Vec::<TurnView>::new);

    let guard = vm.read();
    let (history, totals) = guard
        .as_ref()
        .map_or((Vec::new(), crate::storage::StorageTotals::default()), |v| {
            (v.history.clone(), v.totals)
        });

    let needle = filter().to_lowercase();
    let filtered: Vec<CareerSummary> = history
        .iter()
        .filter(|r| {
            needle.is_empty()
                || r.card_id.to_string().contains(&needle)
                || r.scenario_id.to_string().contains(&needle)
                || r.career_id.to_string().contains(&needle)
        })
        .cloned()
        .collect();
    let run_count = history.len();

    rsx! {
        section { class: "page on",
            div { class: "card page-card",
                div { class: "page-head",
                    div { class: "band",
                        "Career History"
                        span { class: "aside", "{run_count} runs stored" }
                    }
                    h2 { "Every run, turn by turn" }
                    p {
                        "Every settled turn is kept in SQLite and grouped into career runs. Open a run to replay its turn-by-turn snapshots; multiple captures of one turn keep only the latest revision visible."
                    }
                }

                if let Some(career_id) = selected() {
                    RunDetail { career_id, selected, turns }
                } else {
                    div { class: "toolbar",
                        input {
                            r#type: "text",
                            placeholder: "Filter by run, card or scenario id",
                            value: "{filter}",
                            oninput: {
                                let mut filter = filter;
                                move |evt: FormEvent| filter.set(evt.value())
                            },
                        }
                    }
                    if filtered.is_empty() {
                        div { class: "empty-note", "No stored runs match." }
                    } else {
                        table { class: "runs",
                            thead {
                                tr {
                                    th { "Run" }
                                    th { "Card" }
                                    th { "Scenario" }
                                    th { class: "r", "Turns" }
                                    th { class: "r", "Latest turn" }
                                    th { class: "r", "Captures" }
                                    th { class: "r", "Stored" }
                                    th { class: "r", "Last activity" }
                                }
                            }
                            tbody {
                                for run in filtered.iter().cloned() {
                                    RunRow { run, selected, turns }
                                }
                            }
                        }
                    }
                    p { class: "runs-note",
                        "{totals.captures} captures across {run_count} runs · {size_label(totals.db_size_bytes)} on disk · retention is currently unlimited."
                    }
                }
            }
        }
    }
}

#[component]
fn RunRow(run: CareerSummary, selected: Signal<Option<i64>>, turns: Signal<Vec<TurnView>>) -> Element {
    let ctx = use_context::<AppCtx>();
    let now = now_ms();
    let open = move |_| {
        let storage = ctx.storage.clone();
        let career_id = run.career_id;
        let mut selected = selected;
        let mut turns = turns;
        spawn(async move {
            let loaded = tokio::task::spawn_blocking(move || storage.turns_for_career(career_id))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
            turns.set(loaded);
            selected.set(Some(career_id));
        });
    };
    rsx! {
        tr { onclick: open,
            td { class: "num", "{run.career_id}" }
            td { "Card #{run.card_id}" }
            td {
                span { class: "chip ex", "Scenario #{run.scenario_id}" }
            }
            td { class: "r num", "{run.turns_stored}" }
            td { class: "r num", "{run.latest_turn}" }
            td { class: "r num", "{run.captures_stored}" }
            td { class: "r num", "{size_label(run.payload_bytes)}" }
            td { class: "r num", "{ago_label(now, run.ended_at_ms)} ago" }
        }
    }
}

#[component]
fn RunDetail(career_id: i64, selected: Signal<Option<i64>>, turns: Signal<Vec<TurnView>>) -> Element {
    let now = now_ms();
    let rows = turns();
    rsx! {
        div { class: "toolbar",
            button { class: "btn ghost", onclick: move |_| selected.set(None), "← All runs" }
        }
        div { class: "band",
            "Run {career_id}"
            span { class: "aside", "{rows.len()} turns · latest revision each" }
        }
        if rows.is_empty() {
            div { class: "empty-note", "No turns stored for this run." }
        } else {
            table { class: "runs", style: "margin-top:10px",
                thead {
                    tr {
                        th { "Turn" }
                        th { "Capture" }
                        th { class: "r", "Speed" }
                        th { class: "r", "Stamina" }
                        th { class: "r", "Power" }
                        th { class: "r", "Guts" }
                        th { class: "r", "Wit" }
                        th { class: "r", "Skill pts" }
                        th { class: "r", "Captured" }
                    }
                }
                tbody {
                    for view in rows.iter() {
                        {
                            let snap = view.payload.snapshot.clone().unwrap_or_default();
                            let pts = view
                                .payload
                                .extras
                                .as_ref()
                                .and_then(|e| e.skill_points)
                                .unwrap_or(snap.skill_point);
                            let capture_short: String = view.capture_id.chars().take(12).collect();
                            rsx! {
                                tr {
                                    td { class: "num", "{view.turn}" }
                                    td { class: "num", "{capture_short}" }
                                    td { class: "r num", "{snap.speed}" }
                                    td { class: "r num", "{snap.stamina}" }
                                    td { class: "r num", "{snap.power}" }
                                    td { class: "r num", "{snap.guts}" }
                                    td { class: "r num", "{snap.wiz}" }
                                    td { class: "r num", "{pts}" }
                                    td { class: "r num", "{ago_label(now, view.captured_at_ms)} ago" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
