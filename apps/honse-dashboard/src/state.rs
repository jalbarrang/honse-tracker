//! Reactive application state: a deterministic service that projects the
//! persisted data into view models the UI renders directly.
//!
//! The service is pure with respect to time — callers pass `now_ms` — so every
//! transition is testable without a WebView. The Dioxus layer only forwards
//! [`AppEvent`]s and re-renders the returned [`DashboardVm`].

use anyhow::Result;

use crate::pb;
use crate::storage::{CareerSummary, HistoryQuery, Storage, StorageTotals, TurnView};
use crate::AppEvent;

/// Facility/stat order shared by the proto and the game UI.
pub const FACILITIES: [&str; 5] = ["Speed", "Stamina", "Power", "Guts", "Wisdom"];
/// Short stat headers (game uses "Wit" for wisdom).
pub const STAT_SHORT: [&str; 5] = ["Spd", "Sta", "Pow", "Gut", "Wit"];

/// Delivery freshness thresholds.
pub const CONNECTED_WITHIN_MS: u64 = 120_000;
pub const STALE_WITHIN_MS: u64 = 900_000;

/// Link/delivery state shown in the top bar and status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// A capture was committed recently — the game is feeding us.
    Connected,
    /// Data exists but the newest capture is old; figures are stale.
    Stale,
    /// No capture recently (or ever). There is no process detection — this
    /// only means no data has been received within the stale window.
    Offline,
}

/// Pure delivery classification from the newest capture timestamp.
#[must_use]
pub fn delivery_state(now_ms: u64, last_capture_ms: Option<u64>) -> Delivery {
    match last_capture_ms {
        Some(t) if now_ms.saturating_sub(t) <= CONNECTED_WITHIN_MS => Delivery::Connected,
        Some(t) if now_ms.saturating_sub(t) <= STALE_WITHIN_MS => Delivery::Stale,
        _ => Delivery::Offline,
    }
}

/// Persisted theme preference (Settings → Appearance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub const SETTING_KEY: &'static str = "theme.mode";

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Parse a persisted value; anything unrecognized falls back to System.
    #[must_use]
    pub fn parse(value: Option<&str>) -> Self {
        match value {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => Self::System,
        }
    }

    /// Resolve the effective theme given the OS preference.
    #[must_use]
    pub fn resolve(self, system_prefers_dark: bool) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => {
                if system_prefers_dark {
                    "dark"
                } else {
                    "light"
                }
            }
        }
    }
}

/// One main stat with headroom and recent delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatLine {
    pub name: &'static str,
    pub value: i32,
    pub cap: i32,
    /// Change versus the previous stored turn (0 when unknown).
    pub delta: i32,
}

/// A support partner previewed at a facility this turn.
#[derive(Debug, Clone, PartialEq)]
pub struct PartnerVm {
    pub name: String,
    pub initials: String,
    pub bond: i32,
    /// Near-rainbow pressure 0..1 when reported.
    pub bond_pressure: Option<f32>,
    /// Bond at or beyond the friendship-training threshold.
    pub hot: bool,
}

/// One ranked training option.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainingOptionVm {
    /// 0..5 facility slot (proto order).
    pub facility: usize,
    pub facility_name: &'static str,
    pub level: i32,
    /// Per-stat gains `[Spd, Sta, Pow, Gut, Wit]`.
    pub gains: [i32; 5],
    pub total_gain: i32,
    /// Failure percent, `-1` = unknown.
    pub fail_pct: i32,
    pub partners: Vec<PartnerVm>,
    /// Local heuristic 0..100 (see [`build_overview`]); options are ranked by it.
    pub value: i32,
}

/// An acquired skill row (Skills panel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillVm {
    pub name: String,
    pub level: i32,
}

/// One recent-turns row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentTurnVm {
    pub turn: i32,
    pub captured_at_ms: u64,
    /// e.g. "+41 total stats" or "Snapshot stored".
    pub summary: String,
    pub is_latest: bool,
}

/// A reserved race (program id resolves to a concrete race via master data,
/// which the sidecar does not ship; ids are shown as-is).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservedRaceVm {
    pub year: i32,
    pub program_id: i32,
}

/// Everything the Overview page renders for an active career.
#[derive(Debug, Clone, PartialEq)]
pub struct OverviewVm {
    pub career_id: i64,
    pub card_id: i32,
    pub scenario_id: i32,
    pub turn: i32,
    pub captured_at_ms: u64,
    pub capture_id: String,
    pub stats: [StatLine; 5],
    pub total_stats: i32,
    pub headroom: i32,
    pub skill_points: i32,
    pub energy: (i32, i32),
    pub motivation: i32,
    pub motivation_label: &'static str,
    pub fans: i32,
    pub races: (i32, i32),
    pub rating: Option<i32>,
    pub star: i32,
    pub aptitudes: Option<pb::Aptitudes>,
    pub options: Vec<TrainingOptionVm>,
    pub skills: Vec<SkillVm>,
    pub reserved_races: Vec<ReservedRaceVm>,
    pub recent_turns: Vec<RecentTurnVm>,
    pub turns_stored: i64,
}

/// Top-level page data state.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    Loading,
    /// Storage is healthy but holds no careers yet.
    Empty,
    Ready(Box<OverviewVm>),
    Error(String),
}

/// The full dashboard view model.
#[derive(Debug, Clone, PartialEq)]
pub struct DashboardVm {
    pub delivery: Delivery,
    pub phase: Phase,
    pub history: Vec<CareerSummary>,
    pub totals: StorageTotals,
    pub duplicates_discarded: u64,
}

/// Stateful projection service. Owns only counters and the freshest capture
/// timestamp; all durable data comes from [`Storage`] on demand.
pub struct StateService {
    storage: Storage,
    last_capture_ms: Option<u64>,
    duplicates_discarded: u64,
}

impl StateService {
    #[must_use]
    pub fn new(storage: Storage) -> Self {
        Self {
            storage,
            last_capture_ms: None,
            duplicates_discarded: 0,
        }
    }

    /// Fold one ingest event into the service counters.
    pub fn handle_event(&mut self, event: &AppEvent) {
        match event {
            AppEvent::TurnCommitted { captured_at_ms, .. } => {
                self.last_capture_ms = Some((*captured_at_ms).max(self.last_capture_ms.unwrap_or(0)));
            }
            AppEvent::DuplicateDiscarded { .. } => {
                self.duplicates_discarded += 1;
            }
        }
    }

    /// Project the current dashboard view model.
    pub fn snapshot(&mut self, now_ms: u64) -> DashboardVm {
        match self.load(now_ms) {
            Ok(vm) => vm,
            Err(err) => DashboardVm {
                delivery: delivery_state(now_ms, self.last_capture_ms),
                phase: Phase::Error(err.to_string()),
                history: Vec::new(),
                totals: StorageTotals::default(),
                duplicates_discarded: self.duplicates_discarded,
            },
        }
    }

    fn load(&mut self, now_ms: u64) -> Result<DashboardVm> {
        let history = self.storage.career_history(HistoryQuery::default())?;
        let totals = self.storage.totals()?;
        let current = self.storage.current_career()?;

        let phase = match current {
            None => Phase::Empty,
            Some(view) => {
                // Seed freshness from disk so restarts don't report Offline
                // while the newest capture is actually recent.
                self.last_capture_ms = Some(view.latest.captured_at_ms.max(self.last_capture_ms.unwrap_or(0)));
                let turns = self.storage.turns_for_career(view.summary.career_id)?;
                Phase::Ready(Box::new(build_overview(&view.summary, &turns)))
            }
        };

        Ok(DashboardVm {
            delivery: delivery_state(now_ms, self.last_capture_ms),
            phase,
            history,
            totals,
            duplicates_discarded: self.duplicates_discarded,
        })
    }
}

/// Motivation enum (1-5) to the game's mood words.
#[must_use]
pub fn motivation_label(motivation: i32) -> &'static str {
    match motivation {
        5 => "Great",
        4 => "Good",
        3 => "Normal",
        2 => "Bad",
        1 => "Awful",
        _ => "—",
    }
}

/// Uppercase initials for a partner tile (up to two characters).
#[must_use]
pub fn initials(name: &str) -> String {
    let mut out: String = name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect();
    if out.len() < 2 {
        out = name.chars().take(2).collect();
    }
    out.to_uppercase()
}

fn get5(values: &[i32], idx: usize) -> i32 {
    values.get(idx).copied().unwrap_or(0)
}

/// Build the Overview view model from the latest revisions of a career's
/// turns. `turns` must be ascending by turn number (latest revision each), as
/// returned by [`Storage::turns_for_career`].
#[must_use]
pub fn build_overview(summary: &CareerSummary, turns: &[TurnView]) -> OverviewVm {
    let latest = turns.last().expect("build_overview requires at least one turn");
    let prev_snapshot = turns
        .len()
        .checked_sub(2)
        .and_then(|i| turns.get(i))
        .and_then(|t| t.payload.snapshot.as_ref());
    let snap = latest.payload.snapshot.clone().unwrap_or_default();
    let extras = latest.payload.extras.clone().unwrap_or_default();

    let values = [snap.speed, snap.stamina, snap.power, snap.guts, snap.wiz];
    let prev_values = prev_snapshot.map(|p| [p.speed, p.stamina, p.power, p.guts, p.wiz]);
    let stats: [StatLine; 5] = std::array::from_fn(|i| StatLine {
        name: FACILITIES[i],
        value: values[i],
        cap: get5(&snap.stat_caps, i).max(values[i]),
        delta: prev_values.map_or(0, |p| values[i] - p[i]),
    });
    let total_stats = if snap.total_stats > 0 {
        snap.total_stats
    } else {
        values.iter().sum()
    };
    let headroom: i32 = stats.iter().map(|s| (s.cap - s.value).max(0)).sum();

    let options = build_options(&snap, &extras);

    let mut skills: Vec<SkillVm> = extras
        .skills
        .iter()
        .map(|s| SkillVm {
            name: s.name.clone(),
            level: s.level,
        })
        .collect();
    skills.sort_by(|a, b| b.level.cmp(&a.level).then_with(|| a.name.cmp(&b.name)));

    let recent_turns = build_recent_turns(turns);

    OverviewVm {
        career_id: summary.career_id,
        card_id: summary.card_id,
        scenario_id: summary.scenario_id,
        turn: latest.turn,
        captured_at_ms: latest.captured_at_ms,
        capture_id: latest.capture_id.clone(),
        stats,
        total_stats,
        headroom,
        skill_points: extras.skill_points.unwrap_or(snap.skill_point),
        energy: (snap.hp, snap.max_hp.max(snap.hp)),
        motivation: snap.motivation,
        motivation_label: motivation_label(snap.motivation),
        fans: snap.fan_count,
        races: (snap.win_count, snap.total_races),
        rating: snap.evaluation_value,
        star: snap.star,
        aptitudes: snap.aptitudes,
        options,
        skills,
        reserved_races: extras
            .reserved_races
            .iter()
            .map(|r| ReservedRaceVm {
                year: r.year,
                program_id: r.program_id,
            })
            .collect(),
        recent_turns,
        turns_stored: summary.turns_stored,
    }
}

/// Rank the five facilities by a local value heuristic over this snapshot's
/// gains, support bonds, and failure risk (the same inputs the prototype
/// footnote documents). Deterministic: ties break by facility order.
fn build_options(snap: &pb::CareerSnapshot, extras: &pb::CareerExtras) -> Vec<TrainingOptionVm> {
    let mut raw: Vec<(f32, TrainingOptionVm)> = (0..5)
        .map(|facility| {
            let gains: [i32; 5] = std::array::from_fn(|stat| {
                snap.per_stat_gains
                    .get(facility)
                    .map_or(0, |row| get5(&row.values, stat))
            });
            let total_gain = if snap.stat_gains.len() > facility {
                snap.stat_gains[facility]
            } else {
                gains.iter().sum()
            };
            let fail_pct = snap.failure_rates.get(facility).copied().unwrap_or(-1);

            let partners: Vec<PartnerVm> = extras
                .evaluations
                .iter()
                .filter(|e| e.training_facility == Some(facility as i32))
                .map(|e| PartnerVm {
                    name: e.name.clone(),
                    initials: initials(&e.name),
                    bond: e.value,
                    bond_pressure: e.bond_pressure,
                    hot: e.value >= 80,
                })
                .collect();

            let fail = fail_pct.max(0) as f32 / 100.0;
            let bond_score: f32 = partners
                .iter()
                .map(|p| 2.0 + p.bond_pressure.unwrap_or(0.0) * 6.0 + if p.hot { 4.0 } else { 0.0 })
                .sum();
            let score = total_gain as f32 * (1.0 - fail) + bond_score;

            (
                score,
                TrainingOptionVm {
                    facility,
                    facility_name: FACILITIES[facility],
                    level: get5(&snap.training_levels, facility),
                    gains,
                    total_gain,
                    fail_pct,
                    partners,
                    value: 0,
                },
            )
        })
        .collect();

    let max_score = raw.iter().map(|(s, _)| *s).fold(0.0f32, f32::max);
    for (score, opt) in &mut raw {
        opt.value = if max_score > 0.0 {
            ((*score / max_score) * 100.0).round() as i32
        } else {
            0
        };
    }
    raw.sort_by(|a, b| b.1.value.cmp(&a.1.value).then_with(|| a.1.facility.cmp(&b.1.facility)));
    raw.into_iter().map(|(_, opt)| opt).collect()
}

fn build_recent_turns(turns: &[TurnView]) -> Vec<RecentTurnVm> {
    let mut out = Vec::new();
    for (i, view) in turns.iter().enumerate().rev().take(6) {
        let summary = match i
            .checked_sub(1)
            .and_then(|p| turns.get(p))
            .and_then(|p| p.payload.snapshot.as_ref())
            .zip(view.payload.snapshot.as_ref())
        {
            Some((prev, cur)) => {
                let gained = (cur.speed - prev.speed)
                    + (cur.stamina - prev.stamina)
                    + (cur.power - prev.power)
                    + (cur.guts - prev.guts)
                    + (cur.wiz - prev.wiz);
                if gained != 0 {
                    format!("{gained:+} total stats")
                } else {
                    "Snapshot stored".to_string()
                }
            }
            None => "Snapshot stored".to_string(),
        };
        out.push(RecentTurnVm {
            turn: view.turn,
            captured_at_ms: view.captured_at_ms,
            summary,
            is_latest: i == turns.len() - 1,
        });
    }
    out
}

/// Compact "time ago" for capture timestamps.
#[must_use]
pub fn ago_label(now_ms: u64, then_ms: u64) -> String {
    let secs = now_ms.saturating_sub(then_ms) / 1000;
    match secs {
        0..=59 => format!("{secs} s"),
        60..=3599 => format!("{} m", secs / 60),
        3600..=86_399 => format!("{} h", secs / 3600),
        _ => format!("{} d", secs / 86_400),
    }
}

/// Human-readable byte size for the status bar and settings.
#[must_use]
pub fn size_label(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes >= 1_048_576.0 {
        format!("{:.1} MB", bytes / 1_048_576.0)
    } else if bytes >= 1024.0 {
        format!("{:.0} KB", bytes / 1024.0)
    } else {
        format!("{bytes:.0} B")
    }
}
