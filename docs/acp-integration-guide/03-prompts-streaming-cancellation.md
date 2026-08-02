# 03 — Prompts, Streaming & Cancellation

This document covers the core interaction loop: sending a prompt, observing the
streaming `session/update` notifications, handling the prompt response, and
cancelling in-flight prompts.

## The two-channel model

A prompt turn involves **two independent channels**:

1. **The prompt request/response** — `cx.send_request(PromptRequest)`. The
   response (`PromptResponse { stop_reason }`) arrives when the agent finishes
   the turn. You `await` this.
2. **Streaming notifications** — `SessionNotification` carrying `SessionUpdate`.
   These arrive **asynchronously** via the notification handler you registered
   globally (see [01-getting-started.md](01-getting-started.md) §"Notification
   handler"). They are **not** awaited inside the prompt turn; they're handled
   by the dispatch loop and routed to your handler independently.

**The prompt is "done" when the request response arrives**, not when a specific
notification arrives. Notifications may continue briefly after the response
(race) — your handler must tolerate this.

## Constructing `PromptRequest`

A prompt is a `Vec<ContentBlock>`. The first block is the user's text; optional
subsequent blocks are embedded resources (file attachments, etc.).

```rust
use agent_client_protocol::schema::v1::{
    ContentBlock, EmbeddedResource, EmbeddedResourceResource, PromptRequest,
    SessionId, TextContent, TextResourceContents,
};

fn build_prompt_blocks(user_text: &str, attachments: &[Attachment]) -> Vec<ContentBlock> {
    let mut blocks = vec![ContentBlock::Text(TextContent::new(user_text))];

    // Attachments → EmbeddedResource blocks (requires embedded_context cap).
    for att in attachments {
        blocks.push(ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(
                TextResourceContents::new(att.text.clone(), att.uri.clone())
                    .mime_type(att.mime_type.clone()), // optional, chained builder
            ),
        )));
    }
    blocks
}

let blocks = build_prompt_blocks(&user_content, &attachments);
let prompt = cx
    .send_request(PromptRequest::new(agent_session_id.clone(), blocks))
    .block_task();
```

### System prompt / context injection

ACP has no separate "system prompt" field. To inject context (open files,
instructions, profile), **prepend it to the user text** as a single
`ContentBlock::Text`. The production code combines a context prefix with the
user text using a separator:

```rust
// From PreparedPrompt::with_user_text (context.rs pattern):
fn with_user_text(&self, user_text: &str) -> String {
    if self.prefix.is_empty() {
        user_text.to_string()
    } else {
        format!("{}\n\n---\n\n{}", self.prefix, user_text)
    }
}
// Then: ContentBlock::Text(TextContent::new(prepared.with_user_text(&user_content)))
```

Only embed resources if the agent advertised `prompt_capabilities.embedded_context`
in `initialize`. Otherwise the agent may ignore or error on resource blocks.

## The prompt turn: `tokio::select!` over request + commands

The prompt request can run for a long time (minutes). While it runs, you must
still accept **control commands** (cancel, close, switch model). Use
`tokio::select!` to race the prompt response against a command channel.

```rust
use agent_client_protocol::schema::v1::{PromptResponse, StopReason};
use tokio::pin;
use tokio::sync::mpsc;

enum ActorCommand {
    Prompt { /* ... */ result: oneshot::Sender<Result<(), MyError>> },
    Cancel,
    Close(oneshot::Sender<()>),
    // ... ListProviders, SetProvider, SwitchModel, etc.
}

async fn await_prompt(
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    user_content: String,
    attachments: Vec<Attachment>,
    result: oneshot::Sender<Result<(), MyError>>,
    commands: &mut mpsc::Receiver<ActorCommand>,
) -> Result<(), agent_client_protocol::Error> {
    let blocks = build_prompt_blocks(&user_content, &attachments);
    let prompt = cx
        .send_request(PromptRequest::new(agent_session_id.clone(), blocks))
        .block_task();
    pin!(prompt);
    let mut result_slot = Some(result);

    loop {
        tokio::select! {
            // Branch 1: the agent finished the turn.
            reply = &mut prompt => {
                if let Some(result) = result_slot.take() {
                    match reply {
                        Ok(response) => {
                            // response: PromptResponse { stop_reason, .. }
                            let stop = stop_reason_name(response.stop_reason);
                            // ... emit a final "stream ended" event with stop_reason ...
                            let _ = result.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = result.send(Err(MyError::internal(format!("ACP prompt: {error}"))));
                        }
                    }
                }
                return Ok(()); // continue the outer actor loop
            }
            // Branch 2: a control command arrived during the prompt.
            command = commands.recv() => {
                match command {
                    Some(ActorCommand::Cancel) => {
                        // Send session/cancel; the prompt request will then
                        // resolve (usually with StopReason::Cancelled).
                        send_cancel(&cx, &agent_session_id)?;
                    }
                    Some(ActorCommand::Close(ack)) => {
                        // Dropping `prompt` (SentRequest) auto-sends $/cancel_request.
                        return Ok(()); // outer loop tears down
                    }
                    Some(other) => {
                        // Other commands (SwitchModel, etc.) can be handled here
                        // or rejected because a prompt is active.
                    }
                    None => return Ok(()), // command channel closed → exit
                }
            }
        }
    }
}

fn stop_reason_name(reason: StopReason) -> &'static str {
    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurnRequests => "max_turn_requests",
        StopReason::Refusal => "refusal",
        StopReason::Cancelled => "cancelled",
        _ => "unknown", // non-exhaustive enum — always have a fallback
    }
}
```

**Key points:**
- `pin!(prompt)` makes the `SentRequest` pollable inside `select!`.
- Dropping `prompt` (by returning or letting it go out of scope) auto-sends
  `$/cancel_request` to the agent. This is the cancellation guarantee.
- `result_slot` is `Option<oneshot::Sender>` so you can `take()` it once and
  avoid "use after send".

## Streaming: `SessionNotification` / `SessionUpdate`

The notification handler (registered once, globally) receives every
`session/update` and translates it to your app's event model. It runs on the
dispatch loop task, so keep it fast (persist + publish; don't do heavy work).

```rust
use agent_client_protocol::schema::v1::{SessionNotification, SessionUpdate};

// Registered once in Client.builder().on_receive_notification(...)
async fn handle_session_update(
    deps: &HandlerDeps,
    notification: SessionNotification,
) -> Result<(), MyError> {
    let Some(payload) = session_update_to_payload(&notification.update) else {
        return Ok(()); // unsupported update kind — already logged
    };
    // Persist in order, then publish to live listeners.
    deps.event_bus.append_and_publish(payload).await?;
    Ok(())
}
```

### `SessionUpdate` variants

`SessionUpdate` is a typed enum. Match each variant you care about; use a
non-exhaustive fallback for unknowns (protocol upgrades must not lose data
silently — log the **kind** only, never the untrusted payload body, which may
contain prompts/credentials).

```rust
use agent_client_protocol::schema::v1::{
    ContentBlock, ContentChunk, Plan, PlanEntryStatus, SessionUpdate,
    ToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolKind,
};

fn session_update_to_payload(update: &SessionUpdate) -> Option<EventPayload> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => text_chunk(&chunk.content, /*thought=*/ false),
        SessionUpdate::AgentThoughtChunk(chunk) => text_chunk(&chunk.content, /*thought=*/ true),
        SessionUpdate::ToolCall(tool_call) => Some(tool_started(tool_call)),
        SessionUpdate::ToolCallUpdate(update) => Some(tool_completed(update)),
        SessionUpdate::Plan(plan) => Some(plan_updated(plan)),
        SessionUpdate::UserMessageChunk(_) => None, // duplicates local PromptSubmitted
        // Unsupported but observable — log kind only, redact body.
        SessionUpdate::AvailableCommandsUpdate(_) => unmapped("available_commands_update"),
        SessionUpdate::CurrentModeUpdate(_) => unmapped("current_mode_update"),
        SessionUpdate::ConfigOptionUpdate(_) => unmapped("config_option_update"),
        SessionUpdate::SessionInfoUpdate(_) => unmapped("session_info_update"),
        SessionUpdate::UsageUpdate(_) => unmapped("usage_update"),
        _ => unmapped("unrecognized"), // non-exhaustive fallback
    }
}

fn text_chunk(content: &ContentBlock, thought: bool) -> Option<EventPayload> {
    match content {
        ContentBlock::Text(text) => Some(EventPayload::StreamUpdate {
            content: text.text.clone(),
            streaming: true,
            thought,
            // stop_reason is empty during streaming; set on the final event
            // when the PromptResponse arrives.
            stop_reason: String::new(),
        }),
        // Non-text chunks (Image/Audio/ResourceLink/Resource) — log kind, drop body.
        ContentBlock::Image(_) | ContentBlock::Audio(_)
        | ContentBlock::ResourceLink(_) | ContentBlock::Resource(_) => unmapped("non_text_chunk"),
        _ => unmapped("unrecognized_chunk"),
    }
}
```

### `ToolCall` (tool started) and `ToolCallUpdate` (tool progress/result)

```rust
fn tool_started(tool_call: &ToolCall) -> EventPayload {
    EventPayload::ToolStarted {
        tool: tool_call.title.clone(),
        tool_kind: tool_kind_name(tool_call.kind).to_string(),
        tool_call_id: tool_call.tool_call_id.to_string(),
        target: tool_call.locations.first()
            .map_or(String::new, |loc| loc.path.to_string_lossy().into_owned()),
        // raw_input is Option<serde_json::Value> — as_ref() then to_string.
        command: tool_call.raw_input.as_ref()
            .map_or(String::new, ToString::to_string),
        summary: tool_status_name(tool_call.status).to_string(),
    }
}

fn tool_completed(update: &ToolCallUpdate) -> EventPayload {
    let f = &update.fields;
    EventPayload::ToolCompleted {
        tool: f.title.clone().unwrap_or_default(),
        tool_kind: f.kind.map_or(String::new, |k| tool_kind_name(k).to_string()),
        tool_call_id: update.tool_call_id.to_string(),
        target: f.locations.as_deref()
            .map_or(String::new, |locs| locs.first()
                .map_or(String::new, |l| l.path.to_string_lossy().into_owned())),
        summary: f.status.map_or(String::new, |s| tool_status_name(s).to_string()),
        content: tool_content_summary(f.content.as_deref().unwrap_or(&[])),
        exit_code: None,
    }
}

/// Render tool result blocks (text/diff/terminal) into compact content.
fn tool_content_summary(blocks: &[ToolCallContent]) -> String {
    blocks.iter().filter_map(|block| match block {
        ToolCallContent::Content(c) => match &c.content {
            ContentBlock::Text(t) => Some(t.text.clone()),
            _ => None,
        },
        ToolCallContent::Diff(d) => Some(format!(
            "--- {}\n{}\n+++\n{}",
            d.path.display(),
            d.old_text.as_deref().unwrap_or_default(),
            d.new_text,
        )),
        ToolCallContent::Terminal(t) => Some(format!("[terminal {}]", t.terminal_id)),
        _ => None,
    }).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}
```

### Enum name tables

```rust
fn tool_kind_name(kind: ToolKind) -> &'static str {
    match kind {
        ToolKind::Read => "read",       ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",   ToolKind::Move => "move",
        ToolKind::Search => "search",   ToolKind::Execute => "execute",
        ToolKind::Think => "think",     ToolKind::Fetch => "fetch",
        ToolKind::SwitchMode => "switch_mode",
        ToolKind::Other | _ => "other", // non-exhaustive
    }
}

fn tool_status_name(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Pending => "pending",
        ToolCallStatus::InProgress => "in_progress",
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        _ => "unknown",
    }
}

fn plan_status_name(status: &PlanEntryStatus) -> &'static str {
    match status {
        PlanEntryStatus::Pending => "pending",
        PlanEntryStatus::InProgress => "in_progress",
        PlanEntryStatus::Completed => "completed",
        _ => "unknown",
    }
}
```

## `StopReason` — the prompt response

`PromptResponse` carries `stop_reason: StopReason`:

| Variant | Meaning |
|---------|---------|
| `EndTurn` | Agent finished naturally. |
| `MaxTokens` | Hit token limit. |
| `MaxTurnRequests` | Hit tool-call round limit. |
| `Refusal` | Agent refused the request. |
| `Cancelled` | Cancelled (by you or the agent). |
| `_` | Unknown — **always include a fallback** (non-exhaustive). |

Emit a final "stream ended" event with the stop reason when the response
arrives (the `select!` branch 1 above).

## Cancellation

### Sending `session/cancel`

```rust
use agent_client_protocol::schema::v1::CancelNotification;

fn send_cancel(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &SessionId,
) -> Result<(), agent_client_protocol::Error> {
    cx.send_notification(CancelNotification::new(agent_session_id.clone()))
        .map_err(|_| agent_client_protocol::Error::internal_error())
}
```

`session/cancel` is a **notification** (no response). After sending it, the
prompt request will resolve — usually with `StopReason::Cancelled`.

### Implicit cancellation via `SentRequest` drop

Dropping a `SentRequest` (letting it go out of scope, or returning from the
`select!` loop) **auto-sends `$/cancel_request`**. This is the teardown
guarantee: if your `main_fn` returns early, in-flight requests are cancelled
automatically. You don't need to explicitly cancel unless you want to keep the
connection alive.

### Sticky cancel (race-safe)

If a cancel command arrives **just before** you send the prompt request, a naive
implementation would start the prompt anyway. Use a sticky `AtomicBool` to
record the cancel intent and check it before starting the prompt:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

let prompt_cancel: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

// On ActorCommand::Cancel (outside a prompt): set the flag.
prompt_cancel.store(true, Ordering::Release);

// Before starting a prompt: consume the flag.
fn take_sticky_cancel(flag: &AtomicBool) -> bool {
    flag.swap(false, Ordering::AcqRel)
}

// In await_prompt, before send_request:
if take_sticky_cancel(&prompt_cancel) {
    let _ = result.send(Err(MyError::cancelled("cancelled before prompt")));
    return Ok(());
}
```

## Error code labeling (safe logging)

`agent_client_protocol::Error` carries a JSON-RPC `ErrorCode`. When logging
prompt failures, derive a **safe, prompt-data-free label** from the code —
never log the error's text directly, which may contain agent-controlled content:

```rust
use agent_client_protocol::schema::v1::ErrorCode;

fn error_code_label(error: &agent_client_protocol::Error) -> &'static str {
    match error.code {
        ErrorCode::ParseError => "parse_error",
        ErrorCode::InvalidRequest => "invalid_request",
        ErrorCode::MethodNotFound => "method_not_found",
        ErrorCode::InvalidParams => "invalid_params",
        ErrorCode::InternalError => "internal_error",
        ErrorCode::RequestCancelled => "request_cancelled",
        ErrorCode::AuthRequired => "auth_required",
        ErrorCode::ResourceNotFound => "resource_not_found",
        ErrorCode::Other(_) => "other",
        _ => "unknown",
    }
}
```
