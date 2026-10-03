use anyhow::{Context, Result};
use kanzei_llm::Message;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentJob {
    pub id: String,
    pub owner: String,
    pub project_dir: String,
    pub process_id: Option<String>,
    pub name: String,
    pub role: String,
    pub model: String,
    pub model_tier: String,
    pub prompt: String,
    pub schema: Option<serde_json::Value>,
    pub state: String,
    pub outcome: String,
    pub latest: String,
    pub result: String,
    pub worktree: Option<PathBuf>,
    pub base: Option<String>,
    pub head: Option<String>,
    pub files: Vec<String>,
    pub depends_on: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub attempt: u32,
    pub revision: u64,
    pub reported: u64,
    pub messages: Vec<AgentMessage>,
    pub trace: Vec<serde_json::Value>,
    pub trace_seq: u64,
    #[serde(default)]
    pub notify_on_completion: bool,
    #[serde(default)]
    pub replaces: Option<String>,
    #[serde(default)]
    pub notified: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentMessage {
    pub id: String,
    pub from: String,
    pub text: String,
    pub state: String,
    pub at: u64,
}

impl AgentJob {
    pub fn active(&self) -> bool {
        matches!(
            self.state.as_str(),
            "queued" | "waiting" | "waiting_user" | "running" | "stopping"
        ) || self.state == "done" && self.messages.iter().any(|m| m.state == "queued")
    }
}

#[derive(Clone)]
pub struct TeamStore {
    path: PathBuf,
    owner: String,
}

impl TeamStore {
    pub(super) fn state_path(&self) -> &Path {
        &self.path
    }
    pub(super) fn owner(&self) -> &str {
        &self.owner
    }
    pub fn open(root: &Path, owner: &str) -> Result<Self> {
        let path = kanzei_core::project_state_path(root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self {
            path,
            owner: owner.into(),
        };
        store.db()?.execute_batch("CREATE TABLE IF NOT EXISTS agent_team_jobs (owner TEXT NOT NULL, id TEXT NOT NULL, job TEXT NOT NULL, history TEXT NOT NULL DEFAULT '[]', PRIMARY KEY(owner,id));")?;
        Ok(store)
    }
    fn db(&self) -> Result<Connection> {
        let db = Connection::open(&self.path)?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        Ok(db)
    }
    pub fn list(&self) -> Result<Vec<AgentJob>> {
        let db = self.db()?;
        let mut q = db.prepare("SELECT job FROM agent_team_jobs WHERE owner=? ORDER BY rowid")?;
        let rows = q.query_map([&self.owner], |r| r.get::<_, String>(0))?;
        rows.map(|s| Ok(serde_json::from_str(&s?)?)).collect()
    }
    pub fn get(&self, id: &str) -> Result<AgentJob> {
        let raw: Option<String> = self
            .db()?
            .query_row(
                "SELECT job FROM agent_team_jobs WHERE owner=? AND id=?",
                params![self.owner, id],
                |r| r.get(0),
            )
            .optional()?;
        serde_json::from_str(&raw.context("子任务不存在或不属于当前对话")?).map_err(Into::into)
    }
    pub fn insert(&self, job: &AgentJob, history: &[Message]) -> Result<()> {
        self.db()?.execute(
            "INSERT INTO agent_team_jobs(owner,id,job,history) VALUES(?,?,?,?)",
            params![
                self.owner,
                job.id,
                serde_json::to_string(job)?,
                serde_json::to_string(history)?
            ],
        )?;
        Ok(())
    }
    pub fn update(&self, id: &str, f: impl FnOnce(&mut AgentJob)) -> Result<AgentJob> {
        let mut db = self.db()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let raw: String = tx.query_row(
            "SELECT job FROM agent_team_jobs WHERE owner=? AND id=?",
            params![self.owner, id],
            |r| r.get(0),
        )?;
        let mut job: AgentJob = serde_json::from_str(&raw)?;
        f(&mut job);
        tx.execute(
            "UPDATE agent_team_jobs SET job=? WHERE owner=? AND id=?",
            params![serde_json::to_string(&job)?, self.owner, id],
        )?;
        tx.commit()?;
        Ok(job)
    }
    pub fn history(&self, id: &str) -> Result<Vec<Message>> {
        let raw: String = self.db()?.query_row(
            "SELECT history FROM agent_team_jobs WHERE owner=? AND id=?",
            params![self.owner, id],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&raw)?)
    }
    pub fn checkpoint(&self, id: &str, messages: &[Message]) -> Result<()> {
        self.db()?.execute(
            "UPDATE agent_team_jobs SET history=? WHERE owner=? AND id=?",
            params![serde_json::to_string(messages)?, self.owner, id],
        )?;
        Ok(())
    }
}
