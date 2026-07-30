//! Settings page: appearance (System/Light/Dark, persisted), capture endpoint
//! info, storage diagnostics (size, integrity, compact, export), delivery
//! notes, and sidecar version information.

use dioxus::prelude::*;

use super::AppCtx;
use crate::state::{size_label, DashboardVm, ThemeMode};
use crate::{APP_VERSION, INGEST_PROTOCOL};

#[component]
pub fn SettingsPage(vm: Signal<Option<DashboardVm>>, theme_mode: Signal<ThemeMode>) -> Element {
    let ctx = use_context::<AppCtx>();
    let op_status = use_signal(String::new);

    let guard = vm.read();
    let (totals, duplicates) = guard
        .as_ref()
        .map_or((crate::storage::StorageTotals::default(), 0), |v| {
            (v.totals, v.duplicates_discarded)
        });
    let endpoint = format!("{}", ctx.listen_addr);
    let data_dir = ctx.data_root.display().to_string();
    let db_name = crate::platform::DB_FILE;

    rsx! {
        section { class: "page on",
            div { class: "card page-card",
                div { class: "page-head",
                    div { class: "band", "Settings" }
                    h2 { "Sidecar options" }
                    p {
                        "The sidecar owns ingest, storage and presentation. The game plugin only pushes one snapshot per settled turn and never blocks on this process."
                    }
                }

                div { class: "group",
                    div { class: "band", "Appearance" }
                    div { class: "setting",
                        div {
                            strong { "Theme" }
                            p {
                                "System follows the operating-system light/dark preference and updates live when it changes. Light and Dark override it; the override is remembered on this machine."
                            }
                        }
                        div { class: "ctl",
                            div { class: "seg", role: "radiogroup", aria_label: "Theme",
                                ThemeButton { theme_mode, mode: ThemeMode::System, label: "System" }
                                ThemeButton { theme_mode, mode: ThemeMode::Light, label: "Light" }
                                ThemeButton { theme_mode, mode: ThemeMode::Dark, label: "Dark" }
                            }
                        }
                    }
                }

                div { class: "group",
                    div { class: "band", "Capture" }
                    div { class: "setting",
                        div {
                            strong { "Ingest endpoint" }
                            p { "Loopback only. The plugin posts protobuf envelopes to this address with a per-install bearer token." }
                        }
                        div { class: "ctl",
                            input { r#type: "text", readonly: true, value: "{endpoint}" }
                        }
                    }
                    div { class: "setting",
                        div {
                            strong { "Discard duplicate captures" }
                            p { "Transport retries are deduplicated by capture id. Always on — required for immutable turn history." }
                        }
                        div { class: "ctl",
                            span { class: "chip now", "ALWAYS ON" }
                        }
                    }
                }

                div { class: "group",
                    div { class: "band", "Storage" }
                    div { class: "setting",
                        div {
                            strong { "Data directory" }
                            p { "{data_dir}" }
                        }
                        div { class: "ctl",
                            button {
                                class: "btn ghost",
                                onclick: {
                                    let dir = ctx.data_root.clone();
                                    move |_| {
                                        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
                                    }
                                },
                                "Open folder"
                            }
                        }
                    }
                    div { class: "setting",
                        div {
                            strong { "Database" }
                            p {
                                "{db_name} · {size_label(totals.db_size_bytes)} · WAL mode · {totals.captures} captures in {totals.careers} runs · retention is currently unlimited."
                            }
                        }
                        div { class: "ctl",
                            button {
                                class: "btn ghost",
                                onclick: storage_op(&ctx, op_status, StorageOp::Check),
                                "Check"
                            }
                            button {
                                class: "btn ghost",
                                onclick: storage_op(&ctx, op_status, StorageOp::Compact),
                                "Compact"
                            }
                            button {
                                class: "btn ghost",
                                onclick: storage_op(&ctx, op_status, StorageOp::Export),
                                "Export"
                            }
                        }
                    }
                    if !op_status().is_empty() {
                        div { class: "setting",
                            div {
                                strong { "Last maintenance result" }
                                p { "{op_status}" }
                            }
                        }
                    }
                }

                div { class: "group",
                    div { class: "band", "Delivery" }
                    div { class: "setting",
                        div {
                            strong { "Duplicates discarded this session" }
                            p { "Replays acknowledged without a second row." }
                        }
                        div { class: "ctl",
                            b { class: "num", "{duplicates}" }
                        }
                    }
                }

                div { class: "group",
                    div { class: "band", "About" }
                    div { class: "setting",
                        div {
                            strong { "Sidecar" }
                            p { "honse-dashboard {APP_VERSION} · ingest protocol v{INGEST_PROTOCOL} · GPL-3.0-or-later" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ThemeButton(theme_mode: Signal<ThemeMode>, mode: ThemeMode, label: &'static str) -> Element {
    let ctx = use_context::<AppCtx>();
    let checked = theme_mode() == mode;
    rsx! {
        button {
            r#type: "button",
            role: "radio",
            aria_checked: "{checked}",
            onclick: move |_| {
                theme_mode.set(mode);
                let storage = ctx.storage.clone();
                spawn(async move {
                    let _ = tokio::task::spawn_blocking(move || {
                        storage.set_setting(ThemeMode::SETTING_KEY, mode.as_str())
                    })
                    .await;
                });
            },
            "{label}"
        }
    }
}

#[derive(Clone, Copy)]
enum StorageOp {
    Check,
    Compact,
    Export,
}

/// Build an onclick handler running one maintenance operation off the UI loop.
fn storage_op(ctx: &AppCtx, status: Signal<String>, op: StorageOp) -> impl FnMut(MouseEvent) + 'static {
    let ctx = ctx.clone();
    move |_| {
        let storage = ctx.storage.clone();
        let data_root = ctx.data_root.clone();
        let mut status = status;
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || match op {
                StorageOp::Check => storage.integrity_check().map(|r| format!("Integrity: {r}")),
                StorageOp::Compact => storage.compact().map(|()| "Database compacted.".to_string()),
                StorageOp::Export => {
                    let dest = data_root.join(format!("honse-backup-{}.db", crate::storage::now_ms()));
                    storage
                        .backup_to(&dest)
                        .map(|()| format!("Backup written to {}", dest.display()))
                }
            })
            .await;
            status.set(match result {
                Ok(Ok(msg)) => msg,
                Ok(Err(err)) => format!("Failed: {err}"),
                Err(err) => format!("Failed: {err}"),
            });
        });
    }
}
