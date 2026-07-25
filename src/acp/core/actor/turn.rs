//! Commands and prompt/control turn ownership for one ACP actor.
//!
//! This module is the only place that drives a live SDK connection after the
//! actor runtime completes its startup handshake. Client lifecycle code can
//! enqueue intent-level commands, but it cannot access the connection.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    CancelNotification, ContentBlock, EmbeddedResource, EmbeddedResourceResource, PromptRequest,
    SessionId, TextContent, TextResourceContents,
};
use agent_client_protocol::{Agent, ConnectionTo};
use tokio::sync::{mpsc, oneshot};

use super::super::events::append_payload;
use crate::acp::context::PreparedPrompt;
use crate::acp::providers::{
    rpc_disable_provider, rpc_list_providers, rpc_set_model_config, rpc_set_profile_config,
    rpc_set_provider,
};
use crate::events::SharedEventBus;
use crate::interfaces::{AppError, Attachment, EventPayload, ProviderInfo};

/// Commands sent to the connection-owning actor task.
pub(in crate::acp::core) enum ActorCommand {
    Prompt {
        /// User text is persisted verbatim; middleware context is transport-only.
        user_content: String,
        prepared: PreparedPrompt,
        attachments: Vec<Attachment>,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    ListProviders {
        result: oneshot::Sender<Result<Vec<ProviderInfo>, AppError>>,
    },
    SetProvider {
        id: String,
        api_type: String,
        base_url: String,
        headers: HashMap<String, String>,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    DisableProvider {
        id: String,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    SwitchModel {
        config_id: String,
        model_id: String,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    /// Live profile switch via `session/set_config_option` (mode category).
    /// Sent only when `SessionEntry::profile_config_id` is `Some`.
    SetProfile {
        config_id: String,
        profile_id: String,
        result: oneshot::Sender<Result<(), AppError>>,
    },
    Cancel,
    Close(oneshot::Sender<()>),
}

/// Outcome returned by the connection-owning actor loop.
pub(in crate::acp::core) enum ActorExit {
    Closed(oneshot::Sender<()>),
}

pub(super) async fn actor_loop(
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    commands: &mut mpsc::Receiver<ActorCommand>,
    event_bus: SharedEventBus,
    local_session_id: String,
    prompt_cancel: Arc<AtomicBool>,
) -> Result<ActorExit, agent_client_protocol::Error> {
    while let Some(command) = commands.recv().await {
        match command {
            ActorCommand::Prompt {
                user_content,
                prepared,
                attachments,
                result,
            } => match await_prompt(PromptTurn {
                cx: cx.clone(),
                agent_session_id: agent_session_id.clone(),
                user_content,
                prepared,
                attachments,
                result,
                commands,
                event_bus: &event_bus,
                local_session_id: &local_session_id,
                prompt_cancel: &prompt_cancel,
            })
            .await?
            {
                PromptExit::Continue => {}
                PromptExit::Closed(result) => return Ok(ActorExit::Closed(result)),
            },
            other => {
                if let Some(closed) = handle_non_prompt_command(&cx, &agent_session_id, other).await
                {
                    return Ok(ActorExit::Closed(closed));
                }
            }
        }
    }
    Err(agent_client_protocol::Error::internal_error())
}

/// Handle provider / model / cancel / close commands outside a prompt turn.
///
/// Returns `Some(close_ack)` when the session should tear down.
async fn handle_non_prompt_command(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &SessionId,
    command: ActorCommand,
) -> Option<oneshot::Sender<()>> {
    match command {
        ActorCommand::ListProviders { result } => {
            let _ = result.send(rpc_list_providers(cx).await);
            None
        }
        ActorCommand::SetProvider {
            id,
            api_type,
            base_url,
            headers,
            result,
        } => {
            let _ = result.send(rpc_set_provider(cx, id, api_type, base_url, headers).await);
            None
        }
        ActorCommand::DisableProvider { id, result } => {
            let _ = result.send(rpc_disable_provider(cx, id).await);
            None
        }
        ActorCommand::SwitchModel {
            config_id,
            model_id,
            result,
        } => {
            let _ = result
                .send(rpc_set_model_config(cx, agent_session_id, &config_id, &model_id).await);
            None
        }
        ActorCommand::SetProfile {
            config_id,
            profile_id,
            result,
        } => {
            let _ = result
                .send(rpc_set_profile_config(cx, agent_session_id, &config_id, &profile_id).await);
            None
        }
        ActorCommand::Cancel => {
            if let Err(error) = send_cancel(cx, agent_session_id) {
                tracing::error!(error = %error, "ACP cancel notification failed");
            }
            None
        }
        ActorCommand::Close(result) => Some(result),
        ActorCommand::Prompt { result, .. } => {
            // Nested prompts are rejected at begin_prompt; this is a defensive path.
            let _ = result.send(Err(AppError::validation(
                "ACP session already has an active prompt",
            )));
            None
        }
    }
}

enum PromptExit {
    Continue,
    Closed(oneshot::Sender<()>),
}

/// Stable wire spelling for ACP stop reasons, including an SDK-forward fallback.
fn stop_reason_name(reason: agent_client_protocol::schema::v1::StopReason) -> &'static str {
    use agent_client_protocol::schema::v1::StopReason;

    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::MaxTurnRequests => "max_turn_requests",
        StopReason::Refusal => "refusal",
        StopReason::Cancelled => "cancelled",
        _ => "unknown",
    }
}

struct PromptTurn<'a> {
    cx: ConnectionTo<Agent>,
    agent_session_id: SessionId,
    user_content: String,
    prepared: PreparedPrompt,
    attachments: Vec<Attachment>,
    result: oneshot::Sender<Result<(), AppError>>,
    commands: &'a mut mpsc::Receiver<ActorCommand>,
    event_bus: &'a SharedEventBus,
    local_session_id: &'a str,
    prompt_cancel: &'a AtomicBool,
}

/// Await one prompt while continuing to receive session control commands.
async fn await_prompt(turn: PromptTurn<'_>) -> Result<PromptExit, agent_client_protocol::Error> {
    let PromptTurn {
        cx,
        agent_session_id,
        user_content,
        prepared,
        attachments,
        result,
        commands,
        event_bus,
        local_session_id,
        prompt_cancel,
    } = turn;

    // Cancel may have won the race onto an idle actor before this Prompt was
    // dequeued. Honor the sticky bit before touching the agent.
    if take_sticky_cancel(prompt_cancel) {
        let cancel = send_cancel(&cx, &agent_session_id);
        let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
        cancel?;
        return Ok(PromptExit::Continue);
    }

    // Persist lifecycle events only after the actor owns this turn so Cancel
    // cannot race onto an idle loop and become a no-op.
    if let Err(error) = append_payload(
        event_bus,
        local_session_id,
        EventPayload::PromptSubmitted {
            role: "user".to_string(),
            content: user_content.clone(),
            attachments,
        },
    )
    .await
    {
        tracing::error!(
            session_id = local_session_id,
            error = %error,
            "failed to persist ACP prompt-submitted event"
        );
        let _ = result.send(Err(error));
        return Ok(PromptExit::Continue);
    }
    if let Err(error) = append_payload(
        event_bus,
        local_session_id,
        // The typed contract contains no response-start text field. The
        // wire adapter therefore emits the stable role-only shape.
        EventPayload::ResponseStarted {
            role: "agent".to_string(),
        },
    )
    .await
    {
        tracing::error!(
            session_id = local_session_id,
            error = %error,
            "failed to persist ACP response-started event"
        );
        let _ = result.send(Err(error));
        return Ok(PromptExit::Continue);
    }

    // Drain control commands that arrived while persisting lifecycle events
    // so Cancel/Close cannot sit behind a prompt that has not started yet.
    // Provider/model RPCs are serviced here too (concurrent with the upcoming
    // prompt) so they are not starved behind a long turn.
    while let Ok(command) = commands.try_recv() {
        match command {
            ActorCommand::Cancel => {
                let cancel = send_cancel(&cx, &agent_session_id);
                let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
                cancel?;
                return Ok(PromptExit::Continue);
            }
            ActorCommand::Close(close) => {
                let _ = result.send(Err(AppError::internal("ACP session closed during prompt")));
                return Ok(PromptExit::Closed(close));
            }
            ActorCommand::Prompt { result: nested, .. } => {
                let _ = nested.send(Err(AppError::validation(
                    "ACP session already has an active prompt",
                )));
            }
            other => {
                if let Some(closed) = handle_non_prompt_command(&cx, &agent_session_id, other).await
                {
                    let _ =
                        result.send(Err(AppError::internal("ACP session closed during prompt")));
                    return Ok(PromptExit::Closed(closed));
                }
            }
        }
    }
    if take_sticky_cancel(prompt_cancel) {
        let cancel = send_cancel(&cx, &agent_session_id);
        let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
        cancel?;
        return Ok(PromptExit::Continue);
    }

    let mut blocks = vec![ContentBlock::Text(TextContent::new(
        prepared.with_user_text(&user_content),
    ))];
    blocks.extend(prepared.resources.into_iter().map(|resource| {
        ContentBlock::Resource(EmbeddedResource::new(
            EmbeddedResourceResource::TextResourceContents(
                TextResourceContents::new(resource.text, resource.uri)
                    .mime_type(resource.mime_type),
            ),
        ))
    }));
    let prompt = cx
        .send_request(PromptRequest::new(agent_session_id.clone(), blocks))
        .block_task();
    tokio::pin!(prompt);
    let mut result = Some(result);

    loop {
        tokio::select! {
            reply = &mut prompt => {
                if let Some(result) = result.take() {
                    match reply {
                        Ok(response) => {
                            let final_event = EventPayload::StreamUpdate {
                                role: "agent".to_string(),
                                content: String::new(),
                                streaming: false,
                                thought: false,
                                stop_reason: stop_reason_name(response.stop_reason).to_string(),
                            };
                            if let Err(error) =
                                append_payload(event_bus, local_session_id, final_event).await
                            {
                                tracing::error!(
                                    session_id = local_session_id,
                                    error = %error,
                                    "failed to persist ACP prompt-complete event"
                                );
                                // Do not kill the actor on a durable-append
                                // failure; the response already streamed via
                                // notifications and the actor is still usable.
                            }
                            let _ = result.send(Ok(()));
                        }
                        Err(error) => {
                            // Do not copy SDK error text into events/logs:
                            // agents control it and it can contain prompt data.
                            tracing::warn!(
                                session_id = local_session_id,
                                "ACP prompt request failed"
                            );
                            if let Err(append_error) = append_payload(
                                event_bus,
                                local_session_id,
                                EventPayload::AgentExited {
                                    content: "ACP prompt request failed".to_string(),
                                },
                            )
                            .await
                            {
                                tracing::error!(
                                    session_id = local_session_id,
                                    error = %append_error,
                                    "failed to persist ACP prompt-failure event"
                                );
                                // Do not kill the actor on a durable-append
                                // failure; the prompt error is still surfaced
                                // to the caller via the result oneshot below.
                            }
                            let _ = result.send(Err(AppError::internal(format!(
                                "ACP prompt: {error}"
                            ))));
                        }
                    }
                }
                return Ok(PromptExit::Continue);
            }
            command = commands.recv() => {
                match command {
                    Some(ActorCommand::Cancel) => {
                        let cancel = send_cancel(&cx, &agent_session_id);
                        if let Some(result) = result.take() {
                            let _ = result.send(Err(AppError::internal("ACP prompt cancelled")));
                        }
                        cancel?;
                        return Ok(PromptExit::Continue);
                    }
                    Some(ActorCommand::Close(close)) => {
                        if let Some(result) = result.take() {
                            let _ = result.send(Err(AppError::internal("ACP session closed during prompt")));
                        }
                        return Ok(PromptExit::Closed(close));
                    }
                    Some(ActorCommand::Prompt { result, .. }) => {
                        let _ = result.send(Err(AppError::validation(
                            "ACP session already has an active prompt",
                        )));
                    }
                    Some(other) => {
                        if let Some(closed) =
                            handle_non_prompt_command(&cx, &agent_session_id, other)
                                .await
                        {
                            if let Some(result) = result.take() {
                                let _ = result.send(Err(AppError::internal(
                                    "ACP session closed during prompt",
                                )));
                            }
                            return Ok(PromptExit::Closed(closed));
                        }
                    }
                    None => return Err(agent_client_protocol::Error::internal_error()),
                }
            }
        }
    }
}

/// Notify the agent that the local session cancelled an in-flight turn.
fn send_cancel(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &SessionId,
) -> Result<(), agent_client_protocol::Error> {
    cx.send_notification(CancelNotification::new(agent_session_id.clone()))
        .map_err(|_| agent_client_protocol::Error::internal_error())
}

/// Consume a cancellation that arrived before the actor dequeued the prompt.
fn take_sticky_cancel(prompt_cancel: &AtomicBool) -> bool {
    prompt_cancel.swap(false, Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use agent_client_protocol::schema::v1::StopReason;

    use super::{stop_reason_name, take_sticky_cancel};
    use crate::acp::core::lifecycle::tests::{mock_client, wait_until_running};
    use crate::interfaces::ACPClient;

    #[test]
    fn stop_reasons_keep_the_wire_spelling() {
        assert_eq!(stop_reason_name(StopReason::EndTurn), "end_turn");
        assert_eq!(stop_reason_name(StopReason::MaxTokens), "max_tokens");
        assert_eq!(stop_reason_name(StopReason::Cancelled), "cancelled");
    }

    #[test]
    fn sticky_cancel_is_consumed_before_a_prompt_starts() {
        let cancelled = AtomicBool::new(true);
        assert!(take_sticky_cancel(&cancelled));
        assert!(!cancelled.load(Ordering::Acquire));
    }

    /// A prompt RPC must not monopolize the actor's control receiver.
    #[tokio::test]
    async fn cancel_preempts_prompt_and_rejects_second_prompt() {
        let (client, _permissions, _workspace) = mock_client().await;
        let session = client.list_sessions().pop().expect("one mock session");
        let session_id = session.id.clone();
        let prompt_client = Arc::clone(&client);
        let prompt_session_id = session_id.clone();
        let prompt = tokio::spawn(async move {
            prompt_client
                .send_prompt(&prompt_session_id, "please stream a response slowly", &[])
                .await
        });
        wait_until_running(&client, &session_id).await;

        let second = client
            .send_prompt(&session_id, "concurrent prompt", &[])
            .await
            .expect_err("a second prompt must not be queued as another active turn");
        assert!(
            second.to_string().contains("active prompt"),
            "unexpected concurrent-prompt error: {second}"
        );

        tokio::time::timeout(Duration::from_secs(2), client.cancel_session(&session_id))
            .await
            .expect("cancel must be serviced while prompt is in flight")
            .expect("cancel session");
        let prompt_result = tokio::time::timeout(Duration::from_secs(2), prompt)
            .await
            .expect("cancelled prompt task must finish")
            .expect("cancelled prompt task must not panic");
        assert!(
            prompt_result.is_err(),
            "cancelled prompt unexpectedly succeeded"
        );

        client
            .close_session(&session_id)
            .await
            .expect("close after cancellation");
    }

    /// Close must interrupt a prompt, reap the actor, and clean local permission state.
    #[tokio::test]
    async fn close_preempts_prompt_and_clears_local_permission_state() {
        let (client, permissions, _workspace) = mock_client().await;
        let session = client.list_sessions().pop().expect("one mock session");
        let session_id = session.id.clone();
        let prompt_client = Arc::clone(&client);
        let prompt_session_id = session_id.clone();
        let prompt = tokio::spawn(async move {
            prompt_client
                .send_prompt(&prompt_session_id, "please stream a response slowly", &[])
                .await
        });
        wait_until_running(&client, &session_id).await;

        tokio::time::timeout(Duration::from_secs(2), client.close_session(&session_id))
            .await
            .expect("close must not wait for a prompt RPC")
            .expect("close session");
        let prompt_result = tokio::time::timeout(Duration::from_secs(2), prompt)
            .await
            .expect("closed prompt task must finish")
            .expect("closed prompt task must not panic");
        assert!(
            prompt_result.is_err(),
            "closed prompt unexpectedly succeeded"
        );
        assert!(
            client.get_session_info(&session_id).is_err(),
            "closed session must not remain callable"
        );

        let cleared = permissions
            .cleared_sessions
            .lock()
            .expect("recording permissions lock");
        assert!(
            cleared.iter().any(|id| id == &session_id),
            "close did not clear permissions using local session ID; cleared: {cleared:?}"
        );
    }

    #[tokio::test]
    async fn cancellation_grace_period_force_closes_the_session() {
        let (client, permissions, _workspace) = mock_client().await;
        let session = client.list_sessions().pop().expect("one mock session");
        let session_id = session.id.clone();
        let prompt_client = Arc::clone(&client);
        let prompt_session_id = session_id.clone();
        let prompt = tokio::spawn(async move {
            prompt_client
                .send_prompt(&prompt_session_id, "please stream a response slowly", &[])
                .await
        });
        wait_until_running(&client, &session_id).await;

        client
            .cancel_session(&session_id)
            .await
            .expect("cancel session");
        prompt
            .await
            .expect("cancelled prompt task must not panic")
            .expect_err("cancelled prompt must fail locally");
        assert_eq!(
            client
                .get_session_info(&session_id)
                .expect("session remains while grace period is active")
                .status,
            "interrupted"
        );

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.get_session_info(&session_id).is_err() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cancel grace period must force-close the session");

        let cleared = permissions
            .cleared_sessions
            .lock()
            .expect("recording permissions lock");
        assert!(
            cleared.iter().any(|id| id == &session_id),
            "grace close did not clear permissions using local session ID; cleared: {cleared:?}"
        );
    }
}
