//! SQLite persistence with versioned migrations.

use crate::events::{Event, EventKind, NewEvent, Source};
use crate::model::{FactoryDef, RunState, TaskState, WorkerResult};
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{de::DeserializeOwned, Serialize};
use std::path::Path;
use std::sync::LazyLock;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

static MIGRATIONS: LazyLock<Migrations<'static>> =
    LazyLock::new(|| Migrations::new(vec![M::up(include_str!("../migrations/0001_init.sql"))]));

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn enc<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_string(v)?)
}
fn dec<T: DeserializeOwned>(s: &str) -> Result<T> {
    Ok(serde_json::from_str(s)?)
}
/// Unit-variant enums are stored as their bare snake_case name.
fn enc_enum<T: Serialize>(v: &T) -> Result<String> {
    Ok(serde_json::to_value(v)?
        .as_str()
        .unwrap_or_default()
        .to_string())
}
fn dec_enum<T: DeserializeOwned>(s: &str) -> Result<T> {
    Ok(serde_json::from_value(serde_json::Value::String(
        s.to_string(),
    ))?)
}

fn insert_event(conn: &Connection, e: &NewEvent) -> Result<Event> {
    let ts = now_ms();
    conn.execute(
        "INSERT INTO events (run_id, ts_ms, node_id, source, kind) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![e.run_id, ts, e.node_id, enc_enum(&e.source)?, enc(&e.kind)?],
    )?;
    Ok(Event {
        seq: conn.last_insert_rowid(),
        ts_ms: ts,
        event: e.clone(),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    pub id: String,
    pub factory_version_id: String,
    pub state: RunState,
    pub request: String,
    pub created_ms: i64,
    pub updated_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskRow {
    pub run_id: String,
    pub node_id: String,
    pub state: TaskState,
    pub attempt: u32,
    pub rework_count: u32,
    pub result: Option<WorkerResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalStatus {
    Pending,
    Granted,
    Rejected,
}

impl ApprovalStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Granted => "granted",
            Self::Rejected => "rejected",
        }
    }
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Store { conn })
    }

    // ---- factories ----

    pub fn create_factory(&mut self, def: &FactoryDef) -> Result<(String, String)> {
        let tx = self.conn.transaction()?;
        let fid = uuid::Uuid::new_v4().to_string();
        let vid = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO factories (id, name, created_ms) VALUES (?1, ?2, ?3)",
            params![fid, def.name, now],
        )?;
        tx.execute(
            "INSERT INTO factory_versions (id, factory_id, version, definition, created_ms) VALUES (?1, ?2, 1, ?3, ?4)",
            params![vid, fid, enc(def)?, now],
        )?;
        tx.commit()?;
        Ok((fid, vid))
    }

    /// Saves a new immutable version; runs keep pointing at the version they started with.
    pub fn save_factory_version(
        &mut self,
        factory_id: &str,
        def: &FactoryDef,
    ) -> Result<(String, u32)> {
        let tx = self.conn.transaction()?;
        let next: u32 = tx.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM factory_versions WHERE factory_id = ?1",
            [factory_id],
            |r| r.get(0),
        )?;
        let vid = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO factory_versions (id, factory_id, version, definition, created_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![vid, factory_id, next, enc(def)?, now_ms()],
        )?;
        tx.execute(
            "UPDATE factories SET name = ?2 WHERE id = ?1",
            params![factory_id, def.name],
        )?;
        tx.commit()?;
        Ok((vid, next))
    }

    pub fn get_factory_version(&self, version_id: &str) -> Result<FactoryDef> {
        let s: Option<String> = self
            .conn
            .query_row(
                "SELECT definition FROM factory_versions WHERE id = ?1",
                [version_id],
                |r| r.get(0),
            )
            .optional()?;
        dec(&s.ok_or_else(|| StoreError::NotFound(format!("factory version {version_id}")))?)
    }

    pub fn list_factories(&self) -> Result<Vec<(String, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT id, name FROM factories ORDER BY created_ms")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ---- runs ----

    pub fn create_run(&mut self, factory_version_id: &str, request: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        // The run row and its RunCreated event must land together.
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO runs (id, factory_version_id, state, request, created_ms, updated_ms) VALUES (?1, ?2, 'created', ?3, ?4, ?4)",
            params![id, factory_version_id, request, now],
        )?;
        insert_event(
            &tx,
            &NewEvent {
                run_id: id.clone(),
                node_id: None,
                source: Source::Application,
                kind: EventKind::RunCreated {
                    request: request.to_string(),
                },
            },
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn set_run_state(&mut self, run_id: &str, state: RunState) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE runs SET state = ?2, updated_ms = ?3 WHERE id = ?1",
            params![run_id, enc_enum(&state)?, now_ms()],
        )?;
        if n == 0 {
            return Err(StoreError::NotFound(format!("run {run_id}")));
        }
        Ok(())
    }

    pub fn get_run(&self, run_id: &str) -> Result<RunRow> {
        self.conn
            .query_row(
                "SELECT id, factory_version_id, state, request, created_ms, updated_ms FROM runs WHERE id = ?1",
                [run_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )
            .optional()?
            .map(|(id, fv, st, request, c, u)| {
                Ok(RunRow { id, factory_version_id: fv, state: dec_enum(&st)?, request, created_ms: c, updated_ms: u })
            })
            .unwrap_or_else(|| Err(StoreError::NotFound(format!("run {run_id}"))))
    }

    /// Most recent first.
    pub fn list_runs(&self, limit: u32) -> Result<Vec<RunRow>> {
        let ids: Vec<String> = {
            let mut st = self
                .conn
                .prepare("SELECT id FROM runs ORDER BY created_ms DESC, rowid DESC LIMIT ?1")?;
            let rows = st.query_map([limit], |r| r.get(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        ids.iter().map(|i| self.get_run(i)).collect()
    }

    /// Runs that were mid-flight when the app last stopped.
    pub fn interrupted_runs(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT id FROM runs WHERE state IN ('running', 'paused')")?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    // ---- tasks ----

    pub fn upsert_task(&mut self, t: &TaskRow) -> Result<()> {
        let result = t.result.as_ref().map(enc).transpose()?;
        self.conn.execute(
            "INSERT INTO tasks (run_id, node_id, state, attempt, rework_count, result, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (run_id, node_id) DO UPDATE SET
               state = excluded.state, attempt = excluded.attempt, rework_count = excluded.rework_count,
               result = excluded.result, updated_ms = excluded.updated_ms",
            params![t.run_id, t.node_id, enc_enum(&t.state)?, t.attempt, t.rework_count, result, now_ms()],
        )?;
        Ok(())
    }

    pub fn tasks_for_run(&self, run_id: &str) -> Result<Vec<TaskRow>> {
        let mut st = self.conn.prepare(
            "SELECT node_id, state, attempt, rework_count, result FROM tasks WHERE run_id = ?1 ORDER BY rowid",
        )?;
        let raw: Vec<(String, String, u32, u32, Option<String>)> = st
            .query_map([run_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        raw.into_iter()
            .map(|(node_id, state, attempt, rework_count, result)| {
                Ok(TaskRow {
                    run_id: run_id.to_string(),
                    node_id,
                    state: dec_enum(&state)?,
                    attempt,
                    rework_count,
                    result: result.as_deref().map(dec).transpose()?,
                })
            })
            .collect()
    }

    // ---- events ----

    pub fn append_event(&mut self, e: &NewEvent) -> Result<Event> {
        insert_event(&self.conn, e)
    }

    /// Events of a run with `seq > after_seq`, oldest first.
    pub fn events_for_run(&self, run_id: &str, after_seq: i64) -> Result<Vec<Event>> {
        let mut st = self.conn.prepare(
            "SELECT seq, ts_ms, node_id, source, kind FROM events WHERE run_id = ?1 AND seq > ?2 ORDER BY seq",
        )?;
        let raw: Vec<(i64, i64, Option<String>, String, String)> = st
            .query_map(params![run_id, after_seq], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        raw.into_iter()
            .map(|(seq, ts_ms, node_id, source, kind)| {
                Ok(Event {
                    seq,
                    ts_ms,
                    event: NewEvent {
                        run_id: run_id.to_string(),
                        node_id,
                        source: dec_enum(&source)?,
                        kind: dec(&kind)?,
                    },
                })
            })
            .collect()
    }

    // ---- approvals / sessions / artifacts ----

    pub fn create_approval(
        &mut self,
        run_id: &str,
        node_id: Option<&str>,
        action: &str,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO approvals (id, run_id, node_id, action, status, created_ms) VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
            params![id, run_id, node_id, action, now_ms()],
        )?;
        Ok(id)
    }

    /// Only a pending approval can be resolved; returns false if it was already resolved.
    pub fn resolve_approval(&mut self, id: &str, status: ApprovalStatus) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE approvals SET status = ?2, resolved_ms = ?3 WHERE id = ?1 AND status = 'pending'",
            params![id, status.as_str(), now_ms()],
        )?;
        Ok(n == 1)
    }

    pub fn pending_approvals(&self, run_id: &str) -> Result<Vec<(String, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT id, action FROM approvals WHERE run_id = ?1 AND status = 'pending' ORDER BY created_ms")?;
        let rows = st.query_map([run_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn record_session(
        &mut self,
        run_id: &str,
        node_id: &str,
        provider: &str,
        session_id: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO sessions (run_id, node_id, provider, session_id, created_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![run_id, node_id, provider, session_id, now_ms()],
        )?;
        Ok(())
    }

    /// Latest session for a node, for resuming after a restart.
    pub fn latest_session(&self, run_id: &str, node_id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT session_id FROM sessions WHERE run_id = ?1 AND node_id = ?2 ORDER BY created_ms DESC, rowid DESC LIMIT 1",
                params![run_id, node_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn add_artifact(
        &mut self,
        run_id: &str,
        node_id: Option<&str>,
        kind: &str,
        path: &str,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO artifacts (id, run_id, node_id, kind, path, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, run_id, node_id, kind, path, now_ms()],
        )?;
        Ok(id)
    }

    pub fn artifacts_for_run(&self, run_id: &str) -> Result<Vec<(String, String)>> {
        let mut st = self.conn.prepare(
            "SELECT kind, path FROM artifacts WHERE run_id = ?1 ORDER BY created_ms, rowid",
        )?;
        let rows = st.query_map([run_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn def() -> FactoryDef {
        FactoryDef {
            name: "demo".into(),
            nodes: vec![Node {
                id: "a".into(),
                name: "A".into(),
                kind: NodeKind::Worker,
                worker: WorkerDef {
                    provider: "fake".into(),
                    model: None,
                    effort: Some(Effort::High),
                    role: "r".into(),
                    instructions: "do it".into(),
                    permissions: ToolPermissions::default(),
                    working_dir: None,
                    timeout_secs: 30,
                    max_retries: 1,
                    provider_options: Default::default(),
                },
            }],
            edges: vec![],
            max_concurrency: 2,
        }
    }

    fn setup() -> (Store, String) {
        let mut s = Store::open_in_memory().unwrap();
        let (_, vid) = s.create_factory(&def()).unwrap();
        let run = s.create_run(&vid, "build a thing").unwrap();
        (s, run)
    }

    #[test]
    fn migrations_are_valid() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn factory_versions_are_immutable_and_roundtrip() {
        let mut s = Store::open_in_memory().unwrap();
        let (fid, v1) = s.create_factory(&def()).unwrap();
        let mut d2 = def();
        d2.max_concurrency = 5;
        let (v2, n) = s.save_factory_version(&fid, &d2).unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.get_factory_version(&v1).unwrap(), def());
        assert_eq!(s.get_factory_version(&v2).unwrap().max_concurrency, 5);
        assert_eq!(s.list_factories().unwrap().len(), 1);
    }

    #[test]
    fn run_lifecycle_and_created_event() {
        let (mut s, run) = setup();
        assert_eq!(s.get_run(&run).unwrap().state, RunState::Created);
        s.set_run_state(&run, RunState::Running).unwrap();
        assert_eq!(s.interrupted_runs().unwrap(), vec![run.clone()]);
        s.set_run_state(&run, RunState::Completed).unwrap();
        assert!(s.interrupted_runs().unwrap().is_empty());
        assert!(matches!(
            s.set_run_state("nope", RunState::Running),
            Err(StoreError::NotFound(_))
        ));
        let ev = s.events_for_run(&run, 0).unwrap();
        assert_eq!(ev.len(), 1);
        assert_eq!(ev[0].event.source, Source::Application);
        assert!(matches!(ev[0].event.kind, EventKind::RunCreated { .. }));
    }

    #[test]
    fn events_are_ordered_and_resumable() {
        let (mut s, run) = setup();
        let mk = |kind| NewEvent {
            run_id: run.clone(),
            node_id: Some("a".into()),
            source: Source::Provider,
            kind,
        };
        let e1 = s
            .append_event(&mk(EventKind::WorkerStarted {
                session_id: Some("s1".into()),
            }))
            .unwrap();
        let e2 = s
            .append_event(&mk(EventKind::WorkerOutput { text: "hi".into() }))
            .unwrap();
        assert!(e2.seq > e1.seq);
        let tail = s.events_for_run(&run, e1.seq).unwrap();
        assert_eq!(tail, vec![e2]);
    }

    #[test]
    fn tasks_upsert_and_persist_results() {
        let (mut s, run) = setup();
        let mut t = TaskRow {
            run_id: run.clone(),
            node_id: "a".into(),
            state: TaskState::Running,
            attempt: 1,
            rework_count: 0,
            result: None,
        };
        s.upsert_task(&t).unwrap();
        t.state = TaskState::Completed;
        t.result = Some(WorkerResult {
            status: ResultStatus::Success,
            summary: "ok".into(),
            outputs: Default::default(),
            files_changed: vec!["x.rs".into()],
            checks: vec![],
            blockers: vec![],
        });
        s.upsert_task(&t).unwrap();
        assert_eq!(s.tasks_for_run(&run).unwrap(), vec![t]);
    }

    #[test]
    fn approvals_resolve_once() {
        let (mut s, run) = setup();
        let id = s
            .create_approval(&run, Some("a"), "run rm -rf build")
            .unwrap();
        assert_eq!(s.pending_approvals(&run).unwrap().len(), 1);
        assert!(s.resolve_approval(&id, ApprovalStatus::Granted).unwrap());
        assert!(!s.resolve_approval(&id, ApprovalStatus::Rejected).unwrap());
        assert!(s.pending_approvals(&run).unwrap().is_empty());
    }

    #[test]
    fn sessions_and_artifacts() {
        let (mut s, run) = setup();
        s.record_session(&run, "a", "claude", "s1").unwrap();
        s.record_session(&run, "a", "claude", "s2").unwrap();
        assert_eq!(s.latest_session(&run, "a").unwrap().as_deref(), Some("s2"));
        assert_eq!(s.latest_session(&run, "b").unwrap(), None);
        s.add_artifact(&run, Some("a"), "diff", "/tmp/x.diff")
            .unwrap();
        assert_eq!(
            s.artifacts_for_run(&run).unwrap(),
            vec![("diff".to_string(), "/tmp/x.diff".to_string())]
        );
    }

    #[test]
    fn state_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("boiler-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("b.db");
        let run = {
            let mut s = Store::open(&path).unwrap();
            let (_, v) = s.create_factory(&def()).unwrap();
            let r = s.create_run(&v, "x").unwrap();
            s.set_run_state(&r, RunState::Running).unwrap();
            r
        };
        let s = Store::open(&path).unwrap();
        assert_eq!(s.interrupted_runs().unwrap(), vec![run]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
