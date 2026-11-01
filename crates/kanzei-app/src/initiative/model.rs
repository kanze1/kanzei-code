//! Intentions are proposals for a future conversation, never cached outgoing messages.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Channel {
    Desktop,
    QqFriend,
    QqGroup,
    Bilibili,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Identity {
    pub session_id: String,
    pub process_id: String,
    pub root: std::path::PathBuf,
    pub channel: Channel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub review_model: Option<String>,
    pub idle_minutes: u32,
    pub review_minutes: u32,
    pub daily_messages: u32,
    pub unanswered_messages: u32,
    pub quiet_start: u32,
    pub quiet_end: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            review_model: None,
            idle_minutes: 15,
            review_minutes: 60,
            daily_messages: 3,
            unanswered_messages: 2,
            quiet_start: 0,
            quiet_end: 8,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.review_model
                .as_ref()
                .is_none_or(|model| !model.trim().is_empty()),
            "评估模型不能为空字符串"
        );
        anyhow::ensure!(self.review_minutes > 0, "主动评估间隔必须大于零");
        anyhow::ensure!(self.daily_messages > 0, "每日主动消息上限必须大于零");
        anyhow::ensure!(self.unanswered_messages > 0, "未回复消息上限必须大于零");
        anyhow::ensure!(
            self.quiet_start < 24 && self.quiet_end < 24,
            "安静时段小时无效"
        );
        Ok(())
    }
    pub fn quiet(&self, hour: u32) -> bool {
        match self.quiet_start.cmp(&self.quiet_end) {
            std::cmp::Ordering::Less => (self.quiet_start..self.quiet_end).contains(&hour),
            std::cmp::Ordering::Greater => hour >= self.quiet_start || hour < self.quiet_end,
            std::cmp::Ordering::Equal => false,
        }
    }
}

/// Both inbox admissions and QQ ingress matter: a newly received group message
/// can invalidate an opening before it has been promoted to a model turn.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Activity {
    pub human_input: Option<String>,
    pub message_sequence: i64,
}
impl Activity {
    pub fn is_empty(&self) -> bool {
        self.human_input.is_none() && self.message_sequence == 0
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Context {
    pub activity: Activity,
    pub async_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TargetState {
    Ready,
    Stopped,
    Faulted,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Target {
    pub identity: Identity,
    pub title: String,
    pub settings: Settings,
    pub revision: u64,
    pub state: TargetState,
    pub activity: Activity,
    pub last_activity: Option<i64>,
    pub last_review: Option<i64>,
    pub next_review: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TopicKind {
    FollowUp,
    Curiosity,
    Share,
    Discovery,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Plan {
    pub kind: TopicKind,
    pub topic: String,
    /// A model's reason/evidence note, not a confirmed memory or read receipt.
    pub context: String,
    /// Optional Unix milliseconds. Unscheduled curiosity/share ideas use null.
    pub not_before: Option<i64>,
    pub expires: Option<i64>,
}
impl Plan {
    pub fn validate(&self, now: i64) -> anyhow::Result<()> {
        anyhow::ensure!(!self.topic.trim().is_empty(), "话题不能为空");
        anyhow::ensure!(self.topic.chars().count() <= 200, "话题超过 200 字符");
        anyhow::ensure!(
            !self.context.trim().is_empty(),
            "话题需要保留依据或好奇的原因"
        );
        anyhow::ensure!(
            self.context.chars().count() <= 2000,
            "话题依据超过 2000 字符"
        );
        if let Some(expires) = self.expires {
            anyhow::ensure!(expires > now, "话题已过期");
            anyhow::ensure!(
                expires > self.not_before.unwrap_or(now),
                "话题截止时间必须晚于开始时间"
            );
        }
        Ok(())
    }
    pub fn available(&self, now: i64) -> bool {
        self.not_before.is_none_or(|at| at <= now) && self.expires.is_none_or(|at| at > now)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TopicState {
    Active,
    Discussed,
    Closed,
    Uncertain,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Topic {
    pub id: String,
    pub session_id: String,
    pub revision: u64,
    pub plan: Plan,
    pub state: TopicState,
    pub source_run: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "selection", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TopicSelection {
    Existing { id: String, revision: u64 },
    New { plan: Plan },
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Decision {
    Speak {
        topic: TopicSelection,
        reason: String,
    },
    Defer {
        after_minutes: u32,
        reason: String,
    },
    Silent {
        reason: String,
    },
}
impl Decision {
    pub fn reason(&self) -> &str {
        match self {
            Self::Speak { reason, .. } | Self::Defer { reason, .. } | Self::Silent { reason } => {
                reason
            }
        }
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.reason().trim().is_empty(), "主动评估需要给出原因");
        anyhow::ensure!(
            self.reason().chars().count() <= 2000,
            "评估原因超过 2000 字符"
        );
        if let Self::Defer { after_minutes, .. } = self {
            anyhow::ensure!(*after_minutes > 0, "延后时间必须大于零");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewState {
    Reviewing,
    Selected,
    Running,
    Sending,
    Silent,
    Deferred,
    Sent,
    Unknown,
    Cancelled,
    Failed,
    Interrupted,
}
impl ReviewState {
    pub fn active(self) -> bool {
        matches!(
            self,
            Self::Reviewing | Self::Selected | Self::Running | Self::Sending
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Review {
    pub id: String,
    pub session_id: String,
    pub target_revision: u64,
    pub context: Context,
    pub day: String,
    pub created_at: i64,
    pub state: ReviewState,
    pub decision: Option<Decision>,
    pub topic_id: Option<String>,
    pub topic_revision: Option<u64>,
    pub run_id: Option<String>,
    /// References the channel/core receipt; this ledger does not own delivery facts.
    pub receipt: Option<String>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Eligibility {
    Ready,
    Disabled,
    Stopped,
    Faulted,
    NoConversation,
    Busy,
    NotDue,
    Quiet,
    DailyLimit,
    UnansweredLimit,
}

pub(crate) fn after_minutes(at: i64, minutes: u32) -> anyhow::Result<i64> {
    at.checked_add(i64::from(minutes) * 60_000)
        .ok_or_else(|| anyhow::anyhow!("主动评估时间超出范围"))
}
