//! Conversation export and one-shot transfer context for session rebinds.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::events::SharedEventBus;
use crate::interfaces::{AppError, EventStore, EventType};

/// Enough events for a useful transfer while bounding one SQLite query.
const EXPORT_EVENT_LIMIT: i32 = 10_000;

/// Context queued for the first prompt after a successful rebind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationTransfer {
    /// Rendered markdown transcript.
    pub markdown: String,
    /// Display name of the prior harness, used only in the transfer heading.
    pub from_agent_name: String,
}

/// Thread-safe, one-shot transfer queue keyed by stable local session ID.
#[derive(Default)]
pub struct TransferQueue {
    entries: Mutex<HashMap<String, ConversationTransfer>>,
}

impl TransferQueue {
    /// Queue a transcript for the session's next first prompt.
    pub fn insert(
        &self,
        session_id: String,
        transfer: ConversationTransfer,
    ) -> Result<(), AppError> {
        self.entries
            .lock()
            .map_err(|_| AppError::internal("conversation transfer lock poisoned"))?
            .insert(session_id, transfer);
        Ok(())
    }

    /// Consume a transfer only for prompt zero; stale transfers are discarded.
    pub fn take_for_first_prompt(
        &self,
        session_id: &str,
        prompt_count: usize,
    ) -> Result<Option<ConversationTransfer>, AppError> {
        let transfer = self
            .entries
            .lock()
            .map_err(|_| AppError::internal("conversation transfer lock poisoned"))?
            .remove(session_id);
        Ok((prompt_count == 0).then_some(transfer).flatten())
    }

    /// Drop pending state when a session is closed.
    pub fn clear(&self, session_id: &str) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(session_id);
        }
    }
}

/// Export a session's visible conversation as markdown from the durable event bus.
///
/// Stream chunks are coalesced into assistant turns and internal thoughts are
/// omitted. `max_bytes <= 0` leaves the output unbounded; otherwise the result
/// is UTF-8-safe and includes a truncation marker when required.
pub async fn export_conversation(
    event_bus: &SharedEventBus,
    session_id: &str,
    max_bytes: i64,
) -> Result<String, AppError> {
    let events = event_bus.query(session_id, 0, EXPORT_EVENT_LIMIT).await?;
    let mut output = String::new();
    let mut assistant = String::new();

    let flush_assistant = |output: &mut String, assistant: &mut String| {
        let text = assistant.trim();
        if !text.is_empty() {
            append_block(output, &format!("**Assistant:** {text}"));
        }
        assistant.clear();
    };

    for event in events {
        match event.event_type {
            EventType::PromptSubmitted => {
                flush_assistant(&mut output, &mut assistant);
                append_block(&mut output, &format!("**User:** {}", event.content.trim()));
            }
            EventType::StreamUpdate if !event.thought && !event.content.is_empty() => {
                if !assistant.is_empty() {
                    assistant.push(' ');
                }
                assistant.push_str(event.content.trim());
            }
            EventType::ToolStarted => {
                flush_assistant(&mut output, &mut assistant);
                let name = if event.tool.is_empty() {
                    event.tool_kind
                } else {
                    event.tool
                };
                append_block(
                    &mut output,
                    &format!("[Tool: {}]", non_empty(&name, "tool")),
                );
            }
            _ => {}
        }
    }
    flush_assistant(&mut output, &mut assistant);
    Ok(truncate_export(output, max_bytes))
}

fn append_block(output: &mut String, block: &str) {
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str(block);
}

fn non_empty<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() {
        fallback
    } else {
        value
    }
}

/// UTF-8-safe rendering truncation used after the transcript is complete.
#[must_use]
pub fn truncate_export(output: String, max_bytes: i64) -> String {
    let Ok(max_bytes) = usize::try_from(max_bytes) else {
        return output;
    };
    if max_bytes == 0 || output.len() <= max_bytes {
        return output;
    }
    let note = format!(
        "\n\n[... conversation truncated, {} bytes total ...]",
        output.len()
    );
    if note.len() >= max_bytes {
        return truncate_utf8(&note, max_bytes).to_string();
    }
    let prefix = truncate_utf8(&output, max_bytes - note.len());
    format!("{prefix}{note}")
}

fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    let mut end = max_bytes.min(input.len());
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    &input[..end]
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{truncate_export, TransferQueue};
    use crate::events::{EventBus, Store};
    use crate::interfaces::{Event, EventType};

    #[test]
    fn truncation_is_utf8_safe_and_marked() {
        let rendered = truncate_export(format!("**User:** {}", "café ".repeat(50)), 100);

        assert!(std::str::from_utf8(rendered.as_bytes()).is_ok());
        assert!(rendered.len() <= 100);
        assert!(rendered.contains("conversation truncated"));
    }

    #[test]
    fn transfer_is_one_shot_and_first_prompt_only() {
        let queue = TransferQueue::default();
        queue
            .insert(
                "session".to_string(),
                super::ConversationTransfer {
                    markdown: "history".to_string(),
                    from_agent_name: "old".to_string(),
                },
            )
            .expect("queue transfer");

        assert!(queue
            .take_for_first_prompt("session", 1)
            .expect("take stale transfer")
            .is_none());
        assert!(queue
            .take_for_first_prompt("session", 0)
            .expect("take consumed transfer")
            .is_none());
    }

    #[tokio::test]
    async fn export_renders_visible_history_and_skips_thoughts() {
        let directory = tempfile::tempdir().expect("temporary event database");
        let bus = std::sync::Arc::new(EventBus::new(
            Store::open(directory.path().join("events.db")).expect("open event store"),
        ));
        let now = Utc::now();
        let mut user = Event::new(0, EventType::PromptSubmitted, "session", now);
        user.role = "user".to_string();
        user.content = "Can you help?".to_string();
        bus.append_and_publish(user)
            .await
            .expect("append user event");
        let mut thought = Event::new(0, EventType::StreamUpdate, "session", now);
        thought.thought = true;
        thought.content = "hidden".to_string();
        bus.append_and_publish(thought)
            .await
            .expect("append thought event");
        let mut answer = Event::new(0, EventType::StreamUpdate, "session", now);
        answer.content = "Yes.".to_string();
        bus.append_and_publish(answer)
            .await
            .expect("append response event");

        let exported = super::export_conversation(&bus, "session", 0)
            .await
            .expect("export conversation");
        assert_eq!(exported, "**User:** Can you help?\n\n**Assistant:** Yes.");
    }
}
