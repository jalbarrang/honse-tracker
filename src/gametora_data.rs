//! GameTora data catalog access.
//!
//! Reads the JSON snapshots the host caches under its data dir (`gametora/`,
//! downloaded by `core::gametora_data`). The directory is resolved at runtime via
//! the host `host_data_path` service (host API v10+, capability `DATA_PATHS`).
//!
//! Snapshots are stored verbatim in GameTora's upstream shape (uma-sim ADR-0002),
//! so the structs below model only the fields the plugin needs and ignore the
//! rest. Skills/support-cards/character-cards are typed; the irregular training
//! event trees and the encoded reward/name dictionaries are exposed as raw JSON.
//!
//! Everything degrades gracefully: a missing host capability, missing directory,
//! or missing/malformed file yields an empty catalog (logged once), never a panic.
//!
//! The catalog is a public API consumed incrementally by tracker features; until
//! every accessor has a call site, unused entries are allowed here.
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::compat::Sdk;
use serde::Deserialize;
use serde_json::Value;

/// GameTora stores skill-id arrays with mixed number/string entries
/// (e.g. `[200162, "201352"]`). Accept both and drop non-numeric values.
fn de_flexible_id_vec<'de, D>(d: D) -> Result<Vec<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum IdOrStr {
        Int(i64),
        Str(String),
    }
    let raw: Vec<IdOrStr> = Vec::deserialize(d)?;
    Ok(raw
        .into_iter()
        .filter_map(|x| match x {
            IdOrStr::Int(i) => Some(i),
            IdOrStr::Str(s) => s.parse::<i64>().ok(),
        })
        .collect())
}

// ── Typed entities ──────────────────────────────────────────────────────────

/// A skill entry (`skills.json`). `condition_groups` / `loc` are kept raw because
/// their shape varies and is only meaningful to a full simulator.
#[derive(Debug, Clone, Deserialize)]
pub struct Skill {
    pub id: i64,
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub jpname: Option<String>,
    #[serde(default)]
    pub rarity: Option<i64>,
    #[serde(default)]
    pub iconid: Option<i64>,
    /// Skill type tags, e.g. `["nac"]`.
    #[serde(default)]
    pub r#type: Vec<String>,
    /// Inline condition/effect groups (JP top-level; Global overrides under `loc`).
    #[serde(default)]
    pub condition_groups: Value,
    /// Server-specific overrides (`loc.en` = Global).
    #[serde(default)]
    pub loc: Value,
    /// Inherited (`9xxxxx`) variant of a unique skill, with its own (usually
    /// reduced) effects — the value the skill runs at when inherited by a Uma
    /// other than its owner.
    #[serde(default)]
    pub gene_version: Option<GeneVersion>,
}

/// The inherited form of a unique skill (a `gene_version` block): a distinct id
/// (`9xxxxx`) and its own effect groups (reduced vs the owner's full unique).
#[derive(Debug, Clone, Deserialize)]
pub struct GeneVersion {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub condition_groups: Value,
}

/// A support card entry (`support-cards.json`).
#[derive(Debug, Clone, Deserialize)]
pub struct SupportCard {
    pub support_id: i64,
    #[serde(default)]
    pub char_id: Option<i64>,
    #[serde(default)]
    pub char_name: Option<String>,
    /// Rarity index (1=R, 2=SR, 3=SSR).
    #[serde(default)]
    pub rarity: Option<i64>,
    /// Card type, e.g. `"group"`, `"guts"`, `"speed"`.
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub title_en: Option<String>,
    #[serde(default)]
    pub title_ja: Option<String>,
    #[serde(default)]
    pub url_name: Option<String>,
    /// Global release date (`YYYY-MM-DD`); `None` ⇒ not on Global yet.
    #[serde(default)]
    pub release_en: Option<String>,
    /// Skill ids granted via this card's training events.
    #[serde(default, deserialize_with = "de_flexible_id_vec")]
    pub event_skills: Vec<i64>,
    /// Hint-skill payload (raw; nested by hint group).
    #[serde(default)]
    pub hints: Value,
    /// Per-level effect table (raw matrix).
    #[serde(default)]
    pub effects: Value,
}

/// A character (trainee) card entry (`character-cards.json`) — an outfit/costume.
#[derive(Debug, Clone, Deserialize)]
pub struct CharacterCard {
    pub card_id: i64,
    #[serde(default)]
    pub char_id: Option<i64>,
    /// Costume id (outfit).
    #[serde(default)]
    pub costume: Option<i64>,
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub name_jp: Option<String>,
    #[serde(default)]
    pub rarity: Option<i64>,
    /// Global outfit title, when localized.
    #[serde(default)]
    pub title_en_gl: Option<String>,
    #[serde(default)]
    pub title_jp: Option<String>,
    /// Global release date (`YYYY-MM-DD`); `None` ⇒ not on Global yet.
    #[serde(default)]
    pub release_en: Option<String>,
    #[serde(default, deserialize_with = "de_flexible_id_vec")]
    pub skills_unique: Vec<i64>,
    #[serde(default, deserialize_with = "de_flexible_id_vec")]
    pub skills_innate: Vec<i64>,
    #[serde(default, deserialize_with = "de_flexible_id_vec")]
    pub skills_event: Vec<i64>,
    #[serde(default, deserialize_with = "de_flexible_id_vec")]
    pub skills_awakening: Vec<i64>,
    /// Global-specific awakening skill ids (when they differ from JP).
    #[serde(default, deserialize_with = "de_flexible_id_vec")]
    pub skills_awakening_en: Vec<i64>,
    /// Evolved-skill mappings (`{ new, old }`); the `new` id is the evolved form.
    #[serde(default)]
    pub skills_evo: Vec<SkillEvo>,
}

/// An evolved-skill mapping on a character card: `old` evolves into `new`.
#[derive(Debug, Clone, Deserialize)]
pub struct SkillEvo {
    #[serde(default)]
    pub new: i64,
    #[serde(default)]
    pub old: i64,
}

/// Which support-card rarity bucket a training-event file covers.
#[derive(Debug, Clone, Copy)]
pub enum EventKind {
    Ssr,
    Sr,
    Shared,
    Friend,
    Group,
}

impl EventKind {
    fn file(self) -> &'static str {
        match self {
            EventKind::Ssr => "training-events-ssr.json",
            EventKind::Sr => "training-events-sr.json",
            EventKind::Shared => "training-events-shared.json",
            EventKind::Friend => "training-events-friend.json",
            EventKind::Group => "training-events-group.json",
        }
    }
}

// ── Catalog (lazy, cached) ──────────────────────────────────────────────────

#[derive(Default)]
struct Catalog {
    skills: HashMap<i64, Skill>,
    support_cards: HashMap<i64, SupportCard>,
    character_cards: HashMap<i64, CharacterCard>,
    /// Pal (`friend`) outing count keyed by **char_id** (last event-group size).
    friend_steps: HashMap<i64, u32>,
    /// Group outing count keyed by **support_id** (last event-group size).
    group_steps: HashMap<i64, u32>,
    /// Chain `(event_id, story_id)` keys per card, for matching fired events.
    /// Stat cards keyed by support_id (ssr+sr); friend by char_id; group by support_id.
    stat_event_keys: HashMap<i64, Vec<(i64, i64)>>,
    friend_event_keys: HashMap<i64, Vec<(i64, i64)>>,
    group_event_keys: HashMap<i64, Vec<(i64, i64)>>,
}

/// GameTora event-string ids are sometimes JSON strings (friend file). Accept both.
fn as_id(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Parse a training-event tree into `key -> [(event_id, story_id)]`. `last_group`
/// selects the outing group (friend/group); otherwise the flat list at index 1
/// (ssr/sr). Each event is `[event_id, choices, event_string_id, ...]`.
fn parse_event_keys(file: &str, last_group: bool) -> HashMap<i64, Vec<(i64, i64)>> {
    let mut map = HashMap::new();
    let Some(value): Option<Value> = load_file(file) else {
        return map;
    };
    let Some(arr) = value.as_array() else {
        return map;
    };
    for entry in arr {
        let Some(e) = entry.as_array() else { continue };
        let Some(key) = e.first().and_then(Value::as_i64) else {
            continue;
        };
        let events = if last_group { e.last() } else { e.get(1) };
        let Some(events) = events.and_then(Value::as_array) else {
            continue;
        };
        let keys: Vec<(i64, i64)> = events
            .iter()
            .filter_map(|ev| ev.as_array())
            .map(|ev| {
                let event_id = ev.first().and_then(as_id).unwrap_or(0);
                let story_id = ev.get(2).and_then(as_id).unwrap_or(0);
                (event_id, story_id)
            })
            .collect();
        map.insert(key, keys);
    }
    map
}

/// Parse a training-event tree into `key -> last-group size`. Entries are
/// `[key, ...groups]`; the **last** group is the outing list (friend: keyed by
/// char_id; group: keyed by support_id). Validated in
/// docs/reverse-engineering/support-card-event-chains.md.
fn parse_chain_steps(file: &str) -> HashMap<i64, u32> {
    let mut map = HashMap::new();
    let Some(value): Option<Value> = load_file(file) else {
        return map;
    };
    let Some(arr) = value.as_array() else {
        return map;
    };
    for entry in arr {
        let Some(e) = entry.as_array() else { continue };
        let Some(key) = e.first().and_then(Value::as_i64) else {
            continue;
        };
        let Some(last) = e.last().and_then(Value::as_array) else {
            continue;
        };
        let n = last.iter().filter(|v| !v.is_null()).count() as u32;
        map.insert(key, n);
    }
    map
}

static CATALOG: OnceLock<Catalog> = OnceLock::new();

/// Absolute path to the host's cached `gametora/` directory, if the host exposes it.
fn data_dir() -> Option<PathBuf> {
    Sdk::try_get().and_then(|sdk| sdk.gametora_data_dir())
}

/// Read + parse a snapshot file relative to the cache dir. `None` on any failure.
fn load_file<T: for<'de> Deserialize<'de>>(file: &str) -> Option<T> {
    let path = data_dir()?.join(file);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            hlog_warn!(target: "training-tracker", "gametora_data: {} unavailable ({e})", path.display());
            return None;
        }
    };
    match serde_json::from_slice::<T>(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            hlog_warn!(target: "training-tracker", "gametora_data: failed to parse {} ({e})", file);
            None
        }
    }
}

fn index_by<T: for<'de> Deserialize<'de>, F: Fn(&T) -> i64>(file: &str, key: F) -> HashMap<i64, T> {
    let items: Vec<T> = load_file(file).unwrap_or_default();
    items.into_iter().map(|it| (key(&it), it)).collect()
}

fn catalog() -> &'static Catalog {
    CATALOG.get_or_init(|| {
        if data_dir().is_none() {
            hlog_warn!(
                target: "training-tracker",
                "gametora_data: host does not expose a data path (API < v10 or capability missing); catalog empty"
            );
            return Catalog::default();
        }
        let catalog = Catalog {
            skills: index_by("skills.json", |s: &Skill| s.id),
            support_cards: index_by("support-cards.json", |c: &SupportCard| c.support_id),
            character_cards: index_by("character-cards.json", |c: &CharacterCard| c.card_id),
            friend_steps: parse_chain_steps(EventKind::Friend.file()),
            group_steps: parse_chain_steps(EventKind::Group.file()),
            stat_event_keys: {
                let mut m = parse_event_keys(EventKind::Ssr.file(), false);
                m.extend(parse_event_keys(EventKind::Sr.file(), false));
                m
            },
            friend_event_keys: parse_event_keys(EventKind::Friend.file(), true),
            group_event_keys: parse_event_keys(EventKind::Group.file(), true),
        };
        hlog_info!(
            target: "training-tracker",
            "gametora_data: loaded {} skills, {} support cards, {} character cards",
            catalog.skills.len(),
            catalog.support_cards.len(),
            catalog.character_cards.len()
        );
        catalog
    })
}

// ── Public accessors ────────────────────────────────────────────────────────

/// Look up a support card by its `support_card_id` (the id uma.moe reports).
#[must_use]
pub fn support_card(id: i64) -> Option<&'static SupportCard> {
    catalog().support_cards.get(&id)
}

/// Look up a skill by id.
#[must_use]
pub fn skill(id: i64) -> Option<&'static Skill> {
    catalog().skills.get(&id)
}

/// A positive-recovery skill the player can plan to run: heal in **basis points**
/// of max HP (the game's native unit; `cm_model` converts it to a stamina relief).
#[derive(Debug, Clone)]
pub struct RecoverySkill {
    pub id: i64,
    pub name: String,
    pub heal_bp: i32,
}

/// Largest positive `type == 9` (recovery) effect value (basis points) in a raw
/// `condition_groups` array, or `0` if none. Drains (negative values) are ignored.
fn max_recovery_bp(groups: &Value) -> i32 {
    let mut best = 0;
    if let Some(arr) = groups.as_array() {
        for g in arr {
            let Some(effects) = g.get("effects").and_then(|e| e.as_array()) else {
                continue;
            };
            for e in effects {
                if e.get("type").and_then(serde_json::Value::as_i64) == Some(9) {
                    if let Some(v) = e.get("value").and_then(serde_json::Value::as_i64) {
                        best = best.max(v as i32);
                    }
                }
            }
        }
    }
    best
}

/// Heal basis points for one skill's **base** (owner) effect, checking the JP
/// top-level and the Global (`loc.en`) condition groups, taking the larger.
fn skill_recovery_bp(s: &Skill) -> i32 {
    let top = max_recovery_bp(&s.condition_groups);
    let en = s
        .loc
        .get("en")
        .and_then(|en| en.get("condition_groups"))
        .map(max_recovery_bp)
        .unwrap_or(0);
    top.max(en)
}

/// Skill ids that are released on Global, derived from cards that carry a
/// `release_en` date. Mirrors uma-sim: a skill is on Global if it is reachable
/// from a Global-released character or support card. Pure (testable) form.
fn collect_released_ids(chars: &[CharacterCard], support: &[SupportCard]) -> HashSet<i64> {
    let mut set = HashSet::new();
    for c in chars {
        if c.release_en.is_none() {
            continue;
        }
        for id in c
            .skills_unique
            .iter()
            .chain(&c.skills_innate)
            .chain(&c.skills_awakening)
            .chain(&c.skills_awakening_en)
            .chain(&c.skills_event)
        {
            set.insert(*id);
        }
        for evo in &c.skills_evo {
            if evo.new != 0 {
                set.insert(evo.new);
            }
        }
    }
    for c in support {
        if c.release_en.is_none() {
            continue;
        }
        for id in &c.event_skills {
            set.insert(*id);
        }
        if let Some(hint_skills) = c.hints.get("hint_skills").and_then(|h| h.as_array()) {
            for v in hint_skills {
                if let Some(id) = v.as_i64() {
                    set.insert(id);
                }
            }
        }
    }
    set
}

/// Lazily-built set of Global-released skill ids.
fn released_skill_ids() -> &'static HashSet<i64> {
    static S: OnceLock<HashSet<i64>> = OnceLock::new();
    S.get_or_init(|| {
        let c = catalog();
        let chars: Vec<CharacterCard> = c.character_cards.values().cloned().collect();
        let support: Vec<SupportCard> = c.support_cards.values().cloned().collect();
        collect_released_ids(&chars, &support)
    })
}

/// Build the recovery entry for a skill, applying the inherited-unique rule:
/// a unique with a `gene_version` is offered as its **inherited** variant (the
/// reduced `gene_version` heal + id, labelled `(inherited)`) — that is the value
/// it runs at when inherited by a Uma other than its owner. The inherited entry
/// is included iff the owner skill is Global-released. Generic (non-unique)
/// skills use their own id/heal and must themselves be released. Returns `None`
/// for non-recovery or unreleased skills.
fn recovery_entry(s: &Skill, released: &HashSet<i64>) -> Option<RecoverySkill> {
    if skill_recovery_bp(s) <= 0 {
        return None;
    }
    // Owner release gates both the base skill and its inheritable variant.
    if !released.contains(&s.id) {
        return None;
    }
    let name = s.name_en.clone().or_else(|| s.jpname.clone())?;
    match &s.gene_version {
        Some(gv) if max_recovery_bp(&gv.condition_groups) > 0 => Some(RecoverySkill {
            id: gv.id,
            name: format!("{name} (inherited)"),
            heal_bp: max_recovery_bp(&gv.condition_groups),
        }),
        _ => Some(RecoverySkill {
            id: s.id,
            name,
            heal_bp: skill_recovery_bp(s),
        }),
    }
}

/// All Global-released positive-recovery skills, sorted by heal (desc) then name.
/// Unique recoveries are listed at their **inherited** (reduced) value. Empty
/// when the catalog is unavailable.
#[must_use]
pub fn recovery_skills() -> Vec<RecoverySkill> {
    let released = released_skill_ids();
    let mut out: Vec<RecoverySkill> = catalog()
        .skills
        .values()
        .filter_map(|s| recovery_entry(s, released))
        .collect();
    out.sort_by(|a, b| b.heal_bp.cmp(&a.heal_bp).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Total heal basis points for a set of (possibly inherited `9xxxxx`) recovery
/// ids. Resolved against [`recovery_skills`] so inherited variants — which are
/// not always top-level skills — still map. Unknown ids contribute 0.
#[must_use]
pub fn recovery_heal_bp_total(ids: &[i64]) -> i32 {
    if ids.is_empty() {
        return 0;
    }
    let by_id: HashMap<i64, i32> = recovery_skills().into_iter().map(|r| (r.id, r.heal_bp)).collect();
    ids.iter().filter_map(|id| by_id.get(id)).sum()
}

/// The recovery skills **built into** a trained outfit (`card_id`): the card's
/// unique / innate / awakening skills that are recoveries. These run at their
/// **full** (owner) value — the trainee owns them, so no inherited reduction.
/// Empty when the catalog or card is unavailable.
#[must_use]
pub fn card_recovery_skills(card_id: i64) -> Vec<RecoverySkill> {
    let Some(card) = character_card(card_id) else {
        return Vec::new();
    };
    // Dedup by skill name (a card can list the same recovery across its
    // unique/innate/awakening sets, or as base+evolved); keep one at its highest
    // value so the same recovery is never counted twice.
    let mut by_name: HashMap<String, RecoverySkill> = HashMap::new();
    for id in card
        .skills_unique
        .iter()
        .chain(&card.skills_innate)
        .chain(&card.skills_awakening)
        .chain(&card.skills_awakening_en)
    {
        let Some(s) = skill(*id) else { continue };
        let heal_bp = skill_recovery_bp(s);
        if heal_bp <= 0 {
            continue;
        }
        let Some(name) = s.name_en.clone().or_else(|| s.jpname.clone()) else {
            continue;
        };
        let entry = by_name.entry(name.clone()).or_insert(RecoverySkill {
            id: *id,
            name,
            heal_bp: 0,
        });
        if heal_bp > entry.heal_bp {
            entry.heal_bp = heal_bp;
            entry.id = *id;
        }
    }
    let mut out: Vec<RecoverySkill> = by_name.into_values().collect();
    out.sort_by(|a, b| b.heal_bp.cmp(&a.heal_bp).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Total built-in recovery heal (basis points) for a trained outfit's own
/// unique / innate / awakening recoveries.
#[must_use]
pub fn card_recovery_bp_total(card_id: i64) -> i32 {
    card_recovery_skills(card_id).iter().map(|r| r.heal_bp).sum()
}

/// Max event-chain / outing steps for a support card (the `Y` in `X/Y`).
///
/// - Stat cards (speed/stamina/power/guts/intelligence): rarity formula R=0/SR=2/SSR=3
///   — robust to GameTora event-tree lag for new/promo cards.
/// - Pal (`friend`): outing count by char_id.
/// - Group: outing count by support_id.
#[must_use]
pub fn max_chain_steps(support_id: i64) -> Option<u32> {
    let c = support_card(support_id)?;
    match c.r#type.as_deref()? {
        "speed" | "stamina" | "power" | "guts" | "intelligence" => Some(match c.rarity {
            Some(3) => 3,
            Some(2) => 2,
            _ => 0,
        }),
        "friend" => catalog().friend_steps.get(&c.char_id?).copied(),
        "group" => catalog().group_steps.get(&support_id).copied(),
        _ => None,
    }
}

/// Display name for a support card (character name), if catalogued.
#[must_use]
pub fn support_card_name(support_id: i64) -> Option<&'static str> {
    support_card(support_id).and_then(|c| c.char_name.as_deref())
}

/// Facility index a support card rainbows on (its specialty): Speed=0, Stamina=1,
/// Power=2, Guts=3, Wit=4. `None` for pal/`friend`/`group` cards (they never
/// rainbow) or when the card/type is uncatalogued. Mirrors the dashboard's
/// `supportTypeFacility(supportCardType(id))`.
#[must_use]
pub fn support_specialty_facility(support_id: i64) -> Option<usize> {
    specialty_facility_of(support_card(support_id)?.r#type.as_deref()?)
}

/// Map a gametora card-`type` string to a facility index (Speed=0 … Wit=4), or
/// `None` for pal/`friend`/`group`/unknown types (no rainbow). Pure.
#[must_use]
pub(crate) fn specialty_facility_of(type_str: &str) -> Option<usize> {
    match type_str {
        "speed" => Some(0),
        "stamina" => Some(1),
        "power" => Some(2),
        "guts" => Some(3),
        "intelligence" => Some(4),
        _ => None, // friend / group / unknown — no rainbow
    }
}

/// `(event_id, story_id)` keys for a card's event chain / outings, for matching
/// against the fired-event history. Empty when not catalogued.
#[must_use]
pub fn chain_event_keys(support_id: i64) -> &'static [(i64, i64)] {
    let empty: &[(i64, i64)] = &[];
    let Some(c) = support_card(support_id) else {
        return empty;
    };
    let keys = match c.r#type.as_deref() {
        Some("speed" | "stamina" | "power" | "guts" | "intelligence") => catalog().stat_event_keys.get(&support_id),
        Some("friend") => c.char_id.and_then(|cid| catalog().friend_event_keys.get(&cid)),
        Some("group") => catalog().group_event_keys.get(&support_id),
        _ => None,
    };
    keys.map_or(empty, Vec::as_slice)
}

/// Look up a character (outfit) card by `card_id`.
#[must_use]
pub fn character_card(card_id: i64) -> Option<&'static CharacterCard> {
    catalog().character_cards.get(&card_id)
}

/// Whether any catalog data was loaded (i.e. the host cache is present).
#[must_use]
pub fn is_available() -> bool {
    let c = catalog();
    !c.skills.is_empty() || !c.support_cards.is_empty() || !c.character_cards.is_empty()
}

/// Raw training-event tree for a rarity bucket (GameTora's nested array form).
/// Returned uncached since these are large and rarely needed.
#[must_use]
pub fn training_events(kind: EventKind) -> Option<Value> {
    load_file(kind.file())
}

/// Raw reward/name dictionary by snapshot filename (e.g. `evrew.json`,
/// `te-names-en.json`, `te-names-ja.json`). Encoded upstream; consumer decodes.
#[must_use]
pub fn raw_dict(file: &str) -> Option<Value> {
    load_file(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).expect("valid test json")
    }

    #[test]
    fn specialty_facility_maps_stat_types_and_excludes_pals() {
        assert_eq!(specialty_facility_of("speed"), Some(0));
        assert_eq!(specialty_facility_of("stamina"), Some(1));
        assert_eq!(specialty_facility_of("power"), Some(2));
        assert_eq!(specialty_facility_of("guts"), Some(3));
        assert_eq!(specialty_facility_of("intelligence"), Some(4));
        assert_eq!(specialty_facility_of("friend"), None);
        assert_eq!(specialty_facility_of("group"), None);
        assert_eq!(specialty_facility_of(""), None);
    }

    #[test]
    fn max_recovery_bp_reads_type9_value() {
        // "Corner Recovery ○" shape: a recovery effect (type 9) worth 150 bp.
        let groups = parse(r#"[{ "condition": "phase==1&corner!=0", "effects": [ { "type": 9, "value": 150 } ] }]"#);
        assert_eq!(max_recovery_bp(&groups), 150);
    }

    #[test]
    fn max_recovery_bp_ignores_non_recovery_and_drains() {
        let groups = parse(
            r#"[{ "effects": [ { "type": 1, "value": 9999 }, { "type": 9, "value": -300 } ] }, { "effects": [ { "type": 9, "value": 550 } ] }]"#,
        );
        // Negative (drain) ignored; non-recovery type ignored; takes the max positive.
        assert_eq!(max_recovery_bp(&groups), 550);
    }

    #[test]
    fn max_recovery_bp_zero_when_absent() {
        let groups = parse(r#"[{ "effects": [ { "type": 2, "value": 100 } ] }]"#);
        assert_eq!(max_recovery_bp(&groups), 0);
        assert_eq!(max_recovery_bp(&serde_json::Value::Null), 0);
    }

    fn char_card(json: &str) -> CharacterCard {
        serde_json::from_str(json).expect("character card json")
    }
    fn sup_card(json: &str) -> SupportCard {
        serde_json::from_str(json).expect("support card json")
    }
    fn mk_skill(json: &str) -> Skill {
        serde_json::from_str(json).expect("skill json")
    }

    #[test]
    fn collect_released_ids_only_counts_released_cards() {
        let chars = vec![
            char_card(
                r#"{ "card_id": 1, "release_en": "2025-06-26", "skills_unique": [10451],
                     "skills_evo": [{ "new": 700, "old": 600 }] }"#,
            ),
            // No release_en ⇒ its skills are NOT on Global.
            char_card(r#"{ "card_id": 2, "skills_unique": [99999] }"#),
        ];
        let support = vec![sup_card(
            r#"{ "support_id": 5, "release_en": "2025-06-26", "event_skills": [200762],
                 "hints": { "hint_skills": [200162, 200232] } }"#,
        )];
        let set = collect_released_ids(&chars, &support);
        assert!(set.contains(&10451)); // released char unique
        assert!(set.contains(&700)); // evolved skill `new` id
        assert!(set.contains(&200762)); // support event skill
        assert!(set.contains(&200162)); // support hint skill
        assert!(!set.contains(&99999)); // unreleased char's skill excluded
    }

    #[test]
    fn recovery_entry_uses_inherited_value_for_uniques() {
        // Unique recovery: base 550 bp, gene_version (inherited) 350 bp.
        let s = mk_skill(
            r#"{ "id": 10451, "name_en": "Clear Heart",
                 "condition_groups": [{ "effects": [{ "type": 9, "value": 550 }] }],
                 "gene_version": { "id": 900451,
                     "condition_groups": [{ "effects": [{ "type": 9, "value": 350 }] }] } }"#,
        );
        let released: HashSet<i64> = [10451].into_iter().collect();
        let entry = recovery_entry(&s, &released).expect("released unique recovery");
        assert_eq!(entry.id, 900451, "uses the inherited gene_version id");
        assert_eq!(entry.heal_bp, 350, "uses the reduced inherited heal");
        assert!(entry.name.contains("(inherited)"));
    }

    #[test]
    fn recovery_entry_generic_and_release_gating() {
        let generic = mk_skill(
            r#"{ "id": 200352, "name_en": "Corner Recovery",
                 "condition_groups": [{ "effects": [{ "type": 9, "value": 150 }] }] }"#,
        );
        let released: HashSet<i64> = [200352].into_iter().collect();
        let entry = recovery_entry(&generic, &released).expect("released generic recovery");
        assert_eq!(entry.id, 200352);
        assert_eq!(entry.heal_bp, 150);

        // Unreleased ⇒ excluded.
        assert!(recovery_entry(&generic, &HashSet::new()).is_none());

        // Non-recovery ⇒ excluded.
        let non = mk_skill(
            r#"{ "id": 1, "name_en": "Speedster",
            "condition_groups": [{ "effects": [{ "type": 1, "value": 500 }] }] }"#,
        );
        assert!(recovery_entry(&non, &[1].into_iter().collect()).is_none());
    }
}
