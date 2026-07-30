//! Durable SQLite storage behind a dedicated worker thread.
//!
//! `rusqlite::Connection` is not `Sync`, and both the axum ingest runtime and
//! the Dioxus event loop must never block on database work directly. A single
//! worker thread owns the connection; [`Storage`] is a cheap `Clone` handle
//! that sends closures to the worker and waits for the reply. Async callers
//! wrap calls in `spawn_blocking`.
//!
//! Career grouping is transactional: a capture continues the active run when
//! card/scenario match and the turn does not rewind; an identity change or a
//! turn rewind starts a new run. Multiple captures for one turn are revisions
//! (readers default to the latest by `captured_at_ms`, then rowid). A replayed
//! `capture_id` is acknowledged as a duplicate without writing a second row.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use prost::Message;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::pb;

/// Current schema version, stored in `PRAGMA user_version`.
const SCHEMA_VERSION: i32 = 1;

/// Result of committing one settled turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertOutcome {
    /// A new row was committed.
    Committed { career_id: i64, turn: i32 },
    /// The `capture_id` already existed; nothing was written.
    Duplicate { capture_id: String },
}

/// One career run (a grouping of captures for a single card+scenario attempt).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CareerSummary {
    pub career_id: i64,
    pub card_id: i32,
    pub scenario_id: i32,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    /// Distinct turns stored for this run.
    pub turns_stored: i64,
    /// Total capture rows (revisions included).
    pub captures_stored: i64,
    /// Highest turn number seen in the run.
    pub latest_turn: i32,
    /// Total payload bytes stored for the run.
    pub payload_bytes: i64,
}

/// The active (most recent) career plus its latest settled capture.
#[derive(Debug, Clone, PartialEq)]
pub struct CareerView {
    pub summary: CareerSummary,
    pub latest: TurnView,
}

/// One stored capture, decoded.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnView {
    pub capture_id: String,
    pub career_id: i64,
    pub turn: i32,
    pub captured_at_ms: u64,
    pub payload: pb::SettledTurn,
}

/// Query shape for [`Storage::career_history`].
#[derive(Debug, Clone, Default)]
pub struct HistoryQuery {
    /// Maximum runs to return (newest first). `None` = all.
    pub limit: Option<u32>,
}

/// Aggregate counters for the status bar / settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StorageTotals {
    pub careers: i64,
    pub captures: i64,
    pub db_size_bytes: i64,
}

type Command = Box<dyn FnOnce(&mut Connection) + Send>;

/// Cloneable handle to the storage worker thread.
#[derive(Clone)]
pub struct Storage {
    tx: mpsc::Sender<Command>,
    path: PathBuf,
}

impl std::fmt::Debug for Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Storage")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// Open (creating if needed) the database at `path` and start the worker.
pub fn open(path: &Path) -> Result<Storage> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create data dir {}", parent.display()))?;
    }
    let mut conn = Connection::open(path).with_context(|| format!("open sqlite db {}", path.display()))?;
    configure(&conn)?;
    migrate(&mut conn)?;

    let (tx, rx) = mpsc::channel::<Command>();
    std::thread::Builder::new()
        .name("honse-storage".to_string())
        .spawn(move || {
            while let Ok(cmd) = rx.recv() {
                cmd(&mut conn);
            }
        })
        .context("spawn storage worker thread")?;

    Ok(Storage {
        tx,
        path: path.to_path_buf(),
    })
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(anyhow!(
            "database schema {version} is newer than supported {SCHEMA_VERSION}"
        ));
    }
    if version < 1 {
        let tx = conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS career_runs (
                 id            INTEGER PRIMARY KEY AUTOINCREMENT,
                 card_id       INTEGER NOT NULL,
                 scenario_id   INTEGER NOT NULL,
                 started_at_ms INTEGER NOT NULL,
                 ended_at_ms   INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS turn_captures (
                 capture_id     TEXT PRIMARY KEY,
                 career_id      INTEGER NOT NULL REFERENCES career_runs(id),
                 turn           INTEGER NOT NULL,
                 captured_at_ms INTEGER NOT NULL,
                 payload        BLOB NOT NULL,
                 fingerprint    TEXT NOT NULL,
                 inserted_at_ms INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_captures_career_turn
                 ON turn_captures(career_id, turn, captured_at_ms);
             CREATE TABLE IF NOT EXISTS app_settings (
                 key   TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             PRAGMA user_version = 1;",
        )?;
        tx.commit()?;
    }
    Ok(())
}

impl Storage {
    /// Path of the underlying database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `f` on the worker thread and wait for its result.
    fn call<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let (reply_tx, reply_rx) = mpsc::channel::<Result<T>>();
        self.tx
            .send(Box::new(move |conn| {
                let _ = reply_tx.send(f(conn));
            }))
            .map_err(|_| anyhow!("storage worker is gone"))?;
        reply_rx
            .recv()
            .map_err(|_| anyhow!("storage worker dropped the reply"))?
    }

    /// Commit one settled turn inside a single transaction, applying the
    /// career-grouping and idempotency rules. Returns after the commit is
    /// durable; the caller acknowledges the HTTP request only on `Ok`.
    pub fn insert_settled_turn(&self, turn: &pb::SettledTurn) -> Result<InsertOutcome> {
        let turn = turn.clone();
        self.call(move |conn| insert_settled_turn_impl(conn, &turn))
    }

    /// The most recent career run with its latest capture, if any exist.
    pub fn current_career(&self) -> Result<Option<CareerView>> {
        self.call(|conn| {
            let Some(summary) = query_career_summaries(conn, Some(1))?.into_iter().next() else {
                return Ok(None);
            };
            let latest = conn
                .query_row(
                    "SELECT capture_id, career_id, turn, captured_at_ms, payload
                       FROM turn_captures WHERE career_id = ?1
                      ORDER BY turn DESC, captured_at_ms DESC, rowid DESC LIMIT 1",
                    params![summary.career_id],
                    row_to_turn_view,
                )
                .optional()?;
            match latest {
                Some(latest) => Ok(Some(CareerView {
                    summary,
                    latest: latest?,
                })),
                None => Ok(None),
            }
        })
    }

    /// Career runs, newest first.
    pub fn career_history(&self, query: HistoryQuery) -> Result<Vec<CareerSummary>> {
        self.call(move |conn| query_career_summaries(conn, query.limit))
    }

    /// Latest revision per turn for one career, ascending by turn number.
    pub fn turns_for_career(&self, career_id: i64) -> Result<Vec<TurnView>> {
        self.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT capture_id, career_id, turn, captured_at_ms, payload
                   FROM turn_captures tc
                  WHERE career_id = ?1
                    AND rowid = (SELECT rowid FROM turn_captures i
                                  WHERE i.career_id = tc.career_id AND i.turn = tc.turn
                                  ORDER BY i.captured_at_ms DESC, i.rowid DESC LIMIT 1)
                  ORDER BY turn ASC",
            )?;
            let rows = stmt.query_map(params![career_id], row_to_turn_view)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row??);
            }
            Ok(out)
        })
    }

    /// Read one persisted setting.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let key = key.to_string();
        self.call(move |conn| {
            Ok(conn
                .query_row("SELECT value FROM app_settings WHERE key = ?1", params![key], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?)
        })
    }

    /// Write one persisted setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let key = key.to_string();
        let value = value.to_string();
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    /// Aggregate counters for the UI (careers, captures, database size).
    pub fn totals(&self) -> Result<StorageTotals> {
        self.call(|conn| {
            let careers: i64 = conn.query_row("SELECT COUNT(*) FROM career_runs", [], |r| r.get(0))?;
            let captures: i64 = conn.query_row("SELECT COUNT(*) FROM turn_captures", [], |r| r.get(0))?;
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
            let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
            Ok(StorageTotals {
                careers,
                captures,
                db_size_bytes: page_count * page_size,
            })
        })
    }

    /// Run `PRAGMA quick_check`, returning `Ok(diagnostic)` where `"ok"` means
    /// the database passed.
    pub fn integrity_check(&self) -> Result<String> {
        self.call(|conn| {
            let mut stmt = conn.prepare("PRAGMA quick_check")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut lines = Vec::new();
            for row in rows {
                lines.push(row?);
            }
            Ok(lines.join("; "))
        })
    }

    /// Reclaim free pages in place.
    pub fn compact(&self) -> Result<()> {
        self.call(|conn| {
            conn.execute("VACUUM", [])?;
            Ok(())
        })
    }

    /// Write a consistent backup copy of the database to `dest` (`VACUUM INTO`).
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        let dest = dest.to_path_buf();
        self.call(move |conn| {
            let dest_str = dest
                .to_str()
                .ok_or_else(|| anyhow!("backup path is not valid UTF-8"))?
                .to_string();
            conn.execute("VACUUM INTO ?1", params![dest_str])?;
            Ok(())
        })
    }
}

type TurnRowResult = std::result::Result<Result<TurnView>, rusqlite::Error>;

fn row_to_turn_view(row: &rusqlite::Row<'_>) -> TurnRowResult {
    let capture_id: String = row.get(0)?;
    let career_id: i64 = row.get(1)?;
    let turn: i32 = row.get(2)?;
    let captured_at_ms: i64 = row.get(3)?;
    let payload: Vec<u8> = row.get(4)?;
    Ok(pb::SettledTurn::decode(payload.as_slice())
        .map_err(|e| anyhow!("stored payload for {capture_id} failed to decode: {e}"))
        .map(|payload| TurnView {
            capture_id,
            career_id,
            turn,
            captured_at_ms: captured_at_ms as u64,
            payload,
        }))
}

fn query_career_summaries(conn: &Connection, limit: Option<u32>) -> Result<Vec<CareerSummary>> {
    let sql = format!(
        "SELECT r.id, r.card_id, r.scenario_id, r.started_at_ms, r.ended_at_ms,
                COUNT(DISTINCT tc.turn), COUNT(tc.rowid),
                COALESCE(MAX(tc.turn), 0), COALESCE(SUM(LENGTH(tc.payload)), 0)
           FROM career_runs r LEFT JOIN turn_captures tc ON tc.career_id = r.id
          GROUP BY r.id ORDER BY r.id DESC{}",
        limit.map_or(String::new(), |n| format!(" LIMIT {n}"))
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(CareerSummary {
            career_id: row.get(0)?,
            card_id: row.get(1)?,
            scenario_id: row.get(2)?,
            started_at_ms: row.get::<_, i64>(3)? as u64,
            ended_at_ms: row.get::<_, i64>(4)? as u64,
            turns_stored: row.get(5)?,
            captures_stored: row.get(6)?,
            latest_turn: row.get(7)?,
            payload_bytes: row.get(8)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn insert_settled_turn_impl(conn: &mut Connection, turn: &pb::SettledTurn) -> Result<InsertOutcome> {
    if turn.capture_id.is_empty() {
        return Err(anyhow!("capture_id is empty"));
    }
    let snapshot = turn
        .snapshot
        .as_ref()
        .ok_or_else(|| anyhow!("settled turn has no snapshot"))?;
    let (card_id, scenario_id, turn_no) = (snapshot.card_id, snapshot.scenario_id, snapshot.current_turn);

    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

    let exists: Option<String> = tx
        .query_row(
            "SELECT capture_id FROM turn_captures WHERE capture_id = ?1",
            params![turn.capture_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(capture_id) = exists {
        // No write happened; the open transaction simply ends.
        return Ok(InsertOutcome::Duplicate { capture_id });
    }

    // Latest run + the highest turn it has stored, to apply the grouping rules.
    let active: Option<(i64, i32, i32, Option<i32>)> = tx
        .query_row(
            "SELECT r.id, r.card_id, r.scenario_id,
                    (SELECT MAX(turn) FROM turn_captures WHERE career_id = r.id)
               FROM career_runs r ORDER BY r.id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let career_id = match active {
        Some((id, run_card, run_scenario, max_turn))
            if run_card == card_id && run_scenario == scenario_id && max_turn.is_none_or(|m| turn_no >= m) =>
        {
            tx.execute(
                "UPDATE career_runs SET ended_at_ms = MAX(ended_at_ms, ?1) WHERE id = ?2",
                params![turn.captured_at_ms as i64, id],
            )?;
            id
        }
        _ => {
            tx.execute(
                "INSERT INTO career_runs(card_id, scenario_id, started_at_ms, ended_at_ms)
                 VALUES (?1, ?2, ?3, ?3)",
                params![card_id, scenario_id, turn.captured_at_ms as i64],
            )?;
            tx.last_insert_rowid()
        }
    };

    let payload = turn.encode_to_vec();
    let fingerprint = blake3::hash(&payload).to_hex().to_string();
    tx.execute(
        "INSERT INTO turn_captures(capture_id, career_id, turn, captured_at_ms, payload, fingerprint, inserted_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            turn.capture_id,
            career_id,
            turn_no,
            turn.captured_at_ms as i64,
            payload,
            fingerprint,
            now_ms() as i64
        ],
    )?;

    tx.commit()?;
    Ok(InsertOutcome::Committed {
        career_id,
        turn: turn_no,
    })
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
