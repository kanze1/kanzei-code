//! Message authorship is independent of a provider's user/assistant transport role.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOrigin {
    Human,
    Engine,
    Summary,
    Notification,
    Agent,
    Tool,
    #[default]
    Unspecified,
}

impl MessageOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Engine => "engine",
            Self::Summary => "summary",
            Self::Notification => "notification",
            Self::Agent => "agent",
            Self::Tool => "tool",
            Self::Unspecified => "unspecified",
        }
    }

    pub fn is_unspecified(&self) -> bool {
        *self == Self::Unspecified
    }

    /// Admission sources are server-authored metadata. Missing historical metadata stays unknown.
    pub fn from_admission(source: Option<&str>) -> Self {
        match source {
            Some("human") => Self::Human,
            Some("engine") => Self::Engine,
            Some("agent") => Self::Agent,
            Some("tool") => Self::Tool,
            Some("summary") => Self::Summary,
            Some("unspecified") | None => Self::Unspecified,
            Some(_) => Self::Notification,
        }
    }
}
