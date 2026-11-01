//! Durable identity for a response delivered by a tool rather than a provider draft.
use super::{stable_message_hash, Message, SessionFact, SessionFactEnvelope, TypedWriterState};

impl TypedWriterState {
    pub fn tool_response_committed(
        &mut self,
        source_step: u32,
        call_id: &str,
        message: Message,
    ) -> bool {
        let resolved = self
            .invariant
            .turns
            .get(&self.turn_id)
            .and_then(|turn| turn.calls.get(call_id))
            .is_some_and(|call| call.resolved && call.step == self.draft.step);
        if self.terminal
            || self.source_step != Some(source_step)
            || !resolved
            || !self.open_calls.is_empty()
        {
            self.errors.push(format!(
                "tool response requires a resolved call in the active step: {call_id}"
            ));
            return false;
        }
        self.append(vec![SessionFactEnvelope::new(
            &self.turn_id,
            Some(self.draft.step),
            SessionFact::AssistantMessageCommitted {
                message_id: format!("{}:tool-response:{call_id}", self.turn_id),
                content_hash: stable_message_hash(&message),
                message,
            },
        )])
    }
}
