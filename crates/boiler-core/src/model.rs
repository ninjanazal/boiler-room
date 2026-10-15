//! Domain types shared by the engine, store and adapters.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type NodeId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Pending,
    Ready,
    Running,
    Blocked,
    Failed,
    NeedsReview,
    Completed,
}

impl TaskState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Failed | Self::Completed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// Runs an AI worker through a provider adapter.
    Worker,
    /// A worker whose result decides pass/fail; failure follows rework edges.
    Gate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolPermissions {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerDef {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub role: String,
    pub instructions: String,
    pub permissions: ToolPermissions,
    pub working_dir: Option<String>,
    pub timeout_secs: u64,
    /// Attempts allowed after the first one for process-level failures.
    pub max_retries: u32,
    #[serde(default)]
    pub provider_options: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    pub worker: WorkerDef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// `to` depends on `from` completing.
    Dependency,
    /// Followed when a gate `from` fails: `to` is re-run, then the path to the
    /// gate re-executes. Bounded by `max_traversals`.
    Rework,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    /// Required (>0) for rework edges; the engine's hard loop cap.
    #[serde(default)]
    pub max_traversals: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactoryDef {
    pub name: String,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub max_concurrency: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Success,
    Failure,
    NeedsInput,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

/// The structured result every worker must return.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkerResult {
    pub status: ResultStatus,
    pub summary: String,
    #[serde(default)]
    pub outputs: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub checks: Vec<CheckResult>,
    #[serde(default)]
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Created,
    Running,
    Paused,
    Cancelled,
    Completed,
    Failed,
}
