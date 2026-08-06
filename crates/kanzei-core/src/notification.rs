use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 阶段 A 的技术无关内存 broker：只验证消息幂等和通知补发，不连接任何传输层。
#[derive(Debug, Default)]
pub struct InMemoryBroker {
    messages: HashMap<(String, String), AgentMessage>,
    notifications: Vec<AgentNotification>,
    next_sequence_by_thread: HashMap<String, u64>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSubscription {
    pub thread_id: String,
    pub cursor: u64,
}

impl NotificationSubscription {
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            cursor: 0,
        }
    }
}

impl InMemoryBroker {
    pub fn publish_message(&mut self, message: AgentMessage) -> PublishMessage {
        let message_key = (message.thread_id.clone(), message.idempotency_key.clone());
        if let Some(existing) = self.messages.get(&message_key) {
            return PublishMessage::Duplicate(existing.clone());
        }
        self.messages.insert(message_key, message.clone());
        PublishMessage::Accepted(message)
    }

    pub fn messages_for_thread(&self, thread_id: &str) -> Vec<AgentMessage> {
        self.messages
            .values()
            .filter(|message| message.thread_id == thread_id)
            .cloned()
            .collect()
    }

    pub fn publish_notification(
        &mut self,
        mut notification: AgentNotification,
    ) -> AgentNotification {
        let sequence = self
            .next_sequence_by_thread
            .entry(notification.thread_id.clone())
            .or_default();
        *sequence += 1;
        notification.sequence = *sequence;
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

    pub fn poll_subscription(
        &self,
        subscription: &mut NotificationSubscription,
        limit: usize,
    ) -> Vec<AgentNotification> {
        let events = self.replay_notifications_for_thread(
            &subscription.thread_id,
            subscription.cursor,
            limit,
        );
        if let Some(last) = events.last() {
            subscription.cursor = last.sequence;
        }
        events
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
    fn messages_can_be_read_in_both_directions_without_thread_leak() {
        let mut broker = InMemoryBroker::default();
        let mut reply = message("reply", "msg_b");
        reply.sender_agent_id = "subagent".to_owned();
        reply.receiver_agent_id = "primary".to_owned();
        broker.publish_message(message("request", "msg_a"));
        broker.publish_message(reply);
        let mut other = message("other", "msg_c");
        other.thread_id = "thread_b".to_owned();
        broker.publish_message(other);

        let thread_messages = broker.messages_for_thread("thread_a");
        assert_eq!(thread_messages.len(), 2);
        assert!(thread_messages
            .iter()
            .any(|message| message.sender_agent_id == "primary"));
        assert!(thread_messages
            .iter()
            .any(|message| message.sender_agent_id == "subagent"));
        assert_eq!(broker.messages_for_thread("thread_b").len(), 1);
    }

    #[test]
    fn same_idempotency_key_is_independent_between_threads() {
        let mut broker = InMemoryBroker::default();
        let first = message("same", "msg_a");
        let mut other = message("same", "msg_b");
        other.thread_id = "thread_b".to_owned();
        assert_eq!(
            broker.publish_message(first),
            PublishMessage::Accepted(message("same", "msg_a"))
        );
        assert_eq!(
            broker.publish_message(other.clone()),
            PublishMessage::Accepted(other)
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
        other.sequence = 1;
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
    fn thread_sequence_is_independent_when_notifications_interleave() {
        let mut broker = InMemoryBroker::default();
        let first_a = broker.publish_notification(notification("evt_a1", "running"));
        let mut event_b = notification("evt_b1", "running");
        event_b.thread_id = "thread_b".to_owned();
        let first_b = broker.publish_notification(event_b);
        let second_a = broker.publish_notification(notification("evt_a2", "succeeded"));

        assert_eq!((first_a.sequence, second_a.sequence), (1, 2));
        assert_eq!(first_b.sequence, 1);
        assert_eq!(
            broker
                .replay_notifications_for_thread("thread_a", 1, 10)
                .iter()
                .map(|event| event.sequence)
                .collect::<Vec<_>>(),
            vec![2]
        );
    }

    #[test]
    fn subscription_poll_advances_cursor_without_duplicate_delivery() {
        let mut broker = InMemoryBroker::default();
        broker.publish_notification(notification("evt_1", "running"));
        broker.publish_notification(notification("evt_2", "succeeded"));
        let mut subscription = NotificationSubscription::new("thread_a");

        assert_eq!(broker.poll_subscription(&mut subscription, 1).len(), 1);
        assert_eq!(subscription.cursor, 1);
        assert!(broker.poll_subscription(&mut subscription, 1).len() == 1);
        assert_eq!(subscription.cursor, 2);
        assert!(broker.poll_subscription(&mut subscription, 1).is_empty());

        broker.publish_notification(notification("evt_3", "failed"));
        assert_eq!(broker.poll_subscription(&mut subscription, 1).len(), 1);
        assert_eq!(subscription.cursor, 3);
    }

    #[test]
    fn subscription_ignores_other_thread_without_advancing_cursor() {
        let mut broker = InMemoryBroker::default();
        let mut other = notification("evt_b", "running");
        other.thread_id = "thread_b".to_owned();
        broker.publish_notification(other);
        let mut subscription = NotificationSubscription::new("thread_a");
        assert!(broker.poll_subscription(&mut subscription, 10).is_empty());
        assert_eq!(subscription.cursor, 0);

        broker.publish_notification(notification("evt_a", "running"));
        assert_eq!(broker.poll_subscription(&mut subscription, 10).len(), 1);
        assert_eq!(subscription.cursor, 1);
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
