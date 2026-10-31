use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub url: String,
    pub account: String,
    pub model: Option<String>,
    pub persona: String,
    pub friends_enabled: bool,
    pub groups: Vec<Group>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            url: "ws://127.0.0.1:3001".into(),
            account: String::new(),
            model: None,
            persona: "自然、友善地交流；依据事实说话，尊重每个人的称呼和偏好。".into(),
            friends_enabled: true,
            groups: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Group {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub interval_seconds: u64,
    pub proactive_minutes: u64,
    pub quiet_start: u32,
    pub quiet_end: u32,
    pub persona: String,
}

impl Default for Group {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            interval_seconds: 120,
            proactive_minutes: 0,
            quiet_start: 0,
            quiet_end: 8,
            persona: String::new(),
        }
    }
}

impl Group {
    pub fn quiet(&self, hour: u32) -> bool {
        match self.quiet_start.cmp(&self.quiet_end) {
            std::cmp::Ordering::Equal => false,
            std::cmp::Ordering::Less => (self.quiet_start..self.quiet_end).contains(&hour),
            std::cmp::Ordering::Greater => hour >= self.quiet_start || hour < self.quiet_end,
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Stored {
    pub settings: Settings,
    pub token: String,
}

impl Stored {
    pub fn validate(&self) -> Result<(), String> {
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let request = self
            .settings
            .url
            .as_str()
            .into_client_request()
            .map_err(|e| e.to_string())?;
        let scheme = request.uri().scheme_str();
        if !matches!(scheme, Some("ws" | "wss")) {
            return Err("OneBot 地址必须使用 ws:// 或 wss://".into());
        }
        if request
            .uri()
            .authority()
            .is_some_and(|a| a.as_str().contains('@'))
            || request.uri().query().is_some()
        {
            return Err("连接令牌请填写到专用字段，地址不能包含认证参数".into());
        }
        if self.settings.enabled && self.token.trim().is_empty() {
            return Err("启用 QQ 连接需要 OneBot 令牌".into());
        }
        if !self.settings.account.is_empty() {
            kanzei_harness::QqConversation::new(
                &self.settings.account,
                kanzei_harness::QqPeerKind::Friend,
                "1",
            )
            .map_err(|e| e.to_string())?;
        }
        let mut ids = std::collections::BTreeSet::new();
        for group in &self.settings.groups {
            kanzei_harness::QqConversation::new("1", kanzei_harness::QqPeerKind::Group, &group.id)
                .map_err(|e| e.to_string())?;
            if !ids.insert(&group.id) {
                return Err("群号重复".into());
            }
            if group.interval_seconds == 0 || group.quiet_start > 23 || group.quiet_end > 23 {
                return Err("群发言间隔必须大于零，安静时段小时范围为 0–23".into());
            }
            if i64::try_from(group.interval_seconds)
                .ok()
                .and_then(|v| v.checked_mul(1000))
                .is_none()
                || i64::try_from(group.proactive_minutes)
                    .ok()
                    .and_then(|v| v.checked_mul(60_000))
                    .is_none()
            {
                return Err("群聊间隔超出可表示的时间范围".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quiet_hours_cover_midnight_and_disabled_window() {
        let mut group = Group {
            quiet_start: 23,
            quiet_end: 7,
            ..Group::default()
        };
        assert!(group.quiet(23));
        assert!(group.quiet(0));
        assert!(!group.quiet(7));
        group.quiet_end = 23;
        assert!(!group.quiet(23));
    }
    #[test]
    fn enabled_connection_requires_dedicated_credentials_and_unique_peers() {
        let mut stored = Stored::default();
        stored.settings.enabled = true;
        assert!(stored.validate().is_err());
        stored.token = "test".into();
        assert!(stored.validate().is_ok());
        stored.settings.url += "?access_token=test";
        assert!(stored.validate().is_err());
        stored.settings.url = "ws://127.0.0.1:3001".into();
        stored.settings.groups = vec![
            Group {
                id: "123".into(),
                ..Group::default()
            };
            2
        ];
        assert!(stored.validate().is_err());
    }
}
