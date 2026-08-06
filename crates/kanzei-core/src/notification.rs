use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 阶段 A 的技术无关内存 broker：只验证消息幂等和通知补发，不连接任何传输层。
#[derive(Debug, Default)]
pub struct InMemoryBroker {
    messages: HashMap<String, AgentMessage>,
    notifications: Vec<AgentNotification>,
    next_sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMessage {
    pub message_id: String,
    pub idempotency_key: String,
    pub thread_id: String,
    pub sender_agent_id: String,
    pub receiver_agent_id: String,
    pub message_kind: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishMessage {
    Accepted(AgentMessage),
    Duplicate(AgentMessage),
}

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
}

impl InMemoryBroker {
    pub fn publish_message(&mut self, message: AgentMessage) -> PublishMessage {
        if let Some(existing) = self.messages.get(&message.idempotency_key) {
            return PublishMessage::Duplicate(existing.clone());
        }
        self.messages
            .insert(message.idempotency_key.clone(), message.clone());
        PublishMessage::Accepted(message)
    }

    pub fn publish_notification(
        &mut self,
        mut notification: AgentNotification,
    ) -> AgentNotification {
        self.next_sequence += 1;
        notification.sequence = self.next_sequence;
        self.notifications.push(notification.clone());
        notification
    }

    pub fn replay_notifications(&self, cursor: u64, limit: usize) -> Vec<AgentNotification> {
        self.notifications
            .iter()
            .filter(|notification| notification.sequence > cursor)
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn replay_notifications_for_thread(
        &self,
        thread_id: &str,
        cursor: u64,
        limit: usize,
    ) -> Vec<AgentNotification> {
        self.notifications
            .iter()
            .filter(|notification| {
                notification.thread_id == thread_id && notification.sequence > cursor
            })
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn notification_count(&self) -> usize {
        self.notifications.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(key: &str, id: &str) -> AgentMessage {
        AgentMessage {
            message_id: id.to_owned(),
            idempotency_key: key.to_owned(),
            thread_id: "thread_a".to_owned(),
            sender_agent_id: "primary".to_owned(),
            receiver_agent_id: "subagent".to_owned(),
            message_kind: "task_requested".to_owned(),
            payload: serde_json::json!({"prompt": "inspect"}),
        }
    }

    fn notification(id: &str, status: &str) -> AgentNotification {
        AgentNotification {
            event_id: id.to_owned(),
            thread_id: "thread_a".to_owned(),
            agent_id: "subagent".to_owned(),
            kind: "agent_status_changed".to_owned(),
            status: status.to_owned(),
            summary: status.to_owned(),
            requires_action: false,
            sequence: 0,
        }
    }

    #[test]
    fn duplicate_idempotency_key_does_not_create_second_message() {
        let mut broker = InMemoryBroker::default();
        assert!(matches!(
            broker.publish_message(message("same", "msg_1")),
            PublishMessage::Accepted(_)
        ));
        let duplicate = broker.publish_message(message("same", "msg_2"));
        assert_eq!(
            duplicate,
            PublishMessage::Duplicate(message("same", "msg_1"))
        );
    }

    #[test]
    fn notifications_get_monotonic_sequence_and_cursor_replay() {
        let mut broker = InMemoryBroker::default();
        let first = broker.publish_notification(notification("evt_1", "running"));
        let second = broker.publish_notification(notification("evt_2", "succeeded"));
        assert_eq!((first.sequence, second.sequence), (1, 2));
        assert_eq!(broker.replay_notifications(1, 10), vec![second]);
    }

    #[test]
    fn replay_limit_does_not_advance_cursor_or_drop_events() {
        let mut broker = InMemoryBroker::default();
        broker.publish_notification(notification("evt_1", "running"));
        broker.publish_notification(notification("evt_2", "failed"));
        broker.publish_notification(notification("evt_3", "cancelled"));
        assert_eq!(broker.replay_notifications(0, 1).len(), 1);
        assert_eq!(broker.replay_notifications(0, 10).len(), 3);
        assert_eq!(broker.notification_count(), 3);
    }

    #[test]
    fn thread_replay_does_not_leak_notifications_between_threads() {
        let mut broker = InMemoryBroker::default();
        broker.publish_notification(notification("evt_a", "running"));
        let mut other = notification("evt_b", "failed");
        other.thread_id = "thread_b".to_owned();
        broker.publish_notification(other.clone());

        let mut expected_a = notification("evt_a", "running");
        expected_a.sequence = 1;
        other.sequence = 2;
        assert_eq!(
            broker.replay_notifications_for_thread("thread_a", 0, 10),
            vec![expected_a]
        );
        assert_eq!(
            broker.replay_notifications_for_thread("thread_b", 0, 10),
            vec![other]
        );
        assert!(broker
            .replay_notifications_for_thread("thread_a", 2, 10)
            .is_empty());
    }

    #[test]
    fn cursor_after_latest_sequence_returns_empty() {
        let mut broker = InMemoryBroker::default();
        broker.publish_notification(notification("evt_1", "running"));
        assert!(broker.replay_notifications(1, 10).is_empty());
    }

    #[test]
    fn terminal_notification_is_replayed_like_any_other_event() {
        let mut broker = InMemoryBroker::default();
        broker.publish_notification(notification("evt_1", "running"));
        let terminal = broker.publish_notification(notification("evt_2", "failed"));
        assert_eq!(broker.replay_notifications(1, 10), vec![terminal]);
    }
}
