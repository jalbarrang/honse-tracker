//! Dioxus UI root: chrome (topbar/statusbar), page routing, theme handling,
//! and the coroutine that folds ingest events into the view model.

mod components;
mod history;
mod overview;
mod settings;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dioxus::prelude::*;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::state::{DashboardVm, Delivery, StateService, ThemeMode};
use crate::storage::{now_ms, Storage};
use crate::{AppEvent, APP_VERSION};

/// Stylesheet ported from the approved prototype, embedded so plain
/// `cargo run`/release binaries need no asset pipeline.
pub const APP_CSS: &str = include_str!("../../assets/app.css");

/// Immutable wiring shared with every component via context.
#[derive(Clone)]
pub struct AppCtx {
    pub storage: Storage,
    /// Taken exactly once by the event coroutine.
    pub events: Arc<Mutex<Option<UnboundedReceiver<AppEvent>>>>,
    pub listen_addr: SocketAddr,
    pub data_root: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Overview,
    History,
    Settings,
}

/// Root component (the `fn() -> Element` handed to the launcher).
pub fn root() -> Element {
    let ctx = use_context::<AppCtx>();
    let vm = use_signal(|| Option::<DashboardVm>::None);
    let tab = use_signal(|| Tab::Overview);
    let theme_mode = use_signal(ThemeMode::default);
    let system_dark = use_signal(|| false);

    use_event_pump(&ctx, vm, theme_mode);
    use_system_theme_watch(system_dark);
    use_theme_apply(theme_mode, system_dark);

    let delivery = vm.read().as_ref().map_or(Delivery::Offline, |v| v.delivery);
    let (dot_class, link_label) = match delivery {
        Delivery::Connected => ("dot", "Receiving game data"),
        Delivery::Stale => ("dot warn", "Waiting for next turn"),
        Delivery::Offline => ("dot idle", "No data received"),
    };

    rsx! {
        style { {APP_CSS} }
        div { class: "app",
            div { class: "topbar",
                div { class: "wordmark",
                    b { "Honse Tracker" }
                    span { "sidecar {APP_VERSION}" }
                }
                div { class: "tabs", role: "tablist",
                    TabButton { tab, this: Tab::Overview, label: "Overview" }
                    TabButton { tab, this: Tab::History, label: "Career History" }
                    TabButton { tab, this: Tab::Settings, label: "Settings" }
                }
                div { class: "topbar-right",
                    span { class: "{dot_class}" }
                    span { "{link_label}" }
                }
            }
            div { class: "body",
                match tab() {
                    Tab::Overview => rsx! { overview::OverviewPage { vm, tab } },
                    Tab::History => rsx! { history::HistoryPage { vm } },
                    Tab::Settings => rsx! { settings::SettingsPage { vm, theme_mode } },
                }
            }
            Statusbar { vm, delivery }
        }
    }
}

#[component]
fn TabButton(tab: Signal<Tab>, this: Tab, label: &'static str) -> Element {
    let selected = tab() == this;
    rsx! {
        button {
            role: "tab",
            aria_selected: "{selected}",
            onclick: move |_| tab.set(this),
            "{label}"
        }
    }
}

#[component]
fn Statusbar(vm: Signal<Option<DashboardVm>>, delivery: Delivery) -> Element {
    let ctx = use_context::<AppCtx>();
    let addr = ctx.listen_addr;
    let path = ctx.data_root.display().to_string();
    let db = vm
        .read()
        .as_ref()
        .map_or_else(|| "—".to_string(), |v| crate::state::size_label(v.totals.db_size_bytes));
    let link = match delivery {
        Delivery::Connected => format!("● listening {addr}"),
        Delivery::Stale => format!("● listening {addr} · awaiting settle"),
        Delivery::Offline => format!("○ listening {addr} · idle"),
    };
    rsx! {
        div { class: "statusbar",
            span { "{link}" }
            span { class: "sep", "|" }
            span { "protobuf / http" }
            span { class: "sep", "|" }
            span { "db {db}" }
            span { class: "sep", "|" }
            span { class: "path", "{path}" }
        }
    }
}

/// Load persisted theme, then fold ingest events (and a slow freshness tick)
/// into the dashboard view model. All storage work runs on blocking threads.
fn use_event_pump(ctx: &AppCtx, mut vm: Signal<Option<DashboardVm>>, mut theme_mode: Signal<ThemeMode>) {
    let ctx = ctx.clone();
    use_future(move || {
        let ctx = ctx.clone();
        async move {
            let storage = ctx.storage.clone();
            if let Ok(Ok(saved)) =
                tokio::task::spawn_blocking(move || storage.get_setting(ThemeMode::SETTING_KEY)).await
            {
                theme_mode.set(ThemeMode::parse(saved.as_deref()));
            }

            let mut service = StateService::new(ctx.storage.clone());
            let mut rx = ctx.events.lock().expect("events mutex").take();

            loop {
                let (svc, dash) = tokio::task::spawn_blocking(move || {
                    let mut svc = service;
                    let dash = svc.snapshot(now_ms());
                    (svc, dash)
                })
                .await
                .expect("state snapshot task");
                service = svc;
                vm.set(Some(dash));

                match rx.as_mut() {
                    Some(events) => {
                        tokio::select! {
                            ev = events.recv() => match ev {
                                Some(ev) => service.handle_event(&ev),
                                None => rx = None, // ingest gone; keep ticking
                            },
                            () = tokio::time::sleep(std::time::Duration::from_secs(10)) => {}
                        }
                    }
                    None => tokio::time::sleep(std::time::Duration::from_secs(10)).await,
                }
            }
        }
    });
}

/// Mirror the OS light/dark preference into a signal, live.
fn use_system_theme_watch(mut system_dark: Signal<bool>) {
    use_future(move || async move {
        let mut eval = document::eval(
            r"const mq = window.matchMedia('(prefers-color-scheme: dark)');
              dioxus.send(mq.matches);
              mq.addEventListener('change', (e) => dioxus.send(e.matches));",
        );
        while let Ok(prefers_dark) = eval.recv::<bool>().await {
            system_dark.set(prefers_dark);
        }
    });
}

/// Apply the effective theme to `<html data-theme>` whenever it changes.
fn use_theme_apply(theme_mode: Signal<ThemeMode>, system_dark: Signal<bool>) {
    use_effect(move || {
        let resolved = theme_mode().resolve(system_dark());
        document::eval(&format!(
            "document.documentElement.setAttribute('data-theme', '{resolved}');"
        ));
    });
}
