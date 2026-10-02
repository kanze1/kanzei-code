use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentNotification {
    pub event_id: String,
    pub thread_id: String,
    pub agent_id: String,
    pub kind: String,
    pub status: String,
    pub summary: String,
    pub requires_action: bool,
    #[serde(default)]
    pub sequence: u64,
    #[serde(default)]
    pub created_at: i64,
}
