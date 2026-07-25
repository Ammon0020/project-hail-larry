mod filesystem;
mod permission;
mod terminal;

pub(super) use filesystem::{read_text_file, write_text_file};
pub(super) use permission::request_permission;
pub(super) use terminal::{
    cancel_terminals, create_terminal, kill_terminal, release_terminal, terminal_output,
    wait_for_terminal_exit, TerminalRegistry,
};

use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol::{JsonRpcResponse, Responder};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::events::SharedEventBus;
use crate::interfaces::{AppError, PermissionManager, WorkspaceManager};

#[derive(Clone)]
pub(super) struct HandlerDeps {
    pub(super) local_session_id: String,
    pub(super) workspace_id: String,
    pub(super) workspace_path: PathBuf,
    pub(super) workspaces: Arc<dyn WorkspaceManager>,
    pub(super) permissions: Arc<dyn PermissionManager>,
    pub(super) event_bus: SharedEventBus,
    pub(super) terminals: TerminalRegistry,
    pub(super) cancellation: CancellationToken,
    pub(super) callback_slots: Arc<Semaphore>,
}

/// Reserve one bounded callback worker without blocking SDK request dispatch.
fn callback_permit(deps: &HandlerDeps) -> Option<OwnedSemaphorePermit> {
    deps.callback_slots.clone().try_acquire_owned().ok()
}

/// Run a callback until it completes or its owning ACP session closes.
fn spawn_callback<F>(cancellation: CancellationToken, permit: OwnedSemaphorePermit, future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let _permit = permit;
        tokio::select! {
            () = cancellation.cancelled() => {}
            () = future => {}
        }
    });
}

/// Bound an inbound ACP request that maps `Result` to typed success/error replies.
pub(super) fn spawn_result_callback<T, F, Fut>(
    deps: HandlerDeps,
    responder: Responder<T>,
    warn: &'static str,
    work: F,
) where
    T: JsonRpcResponse,
    F: FnOnce(HandlerDeps) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, AppError>> + Send + 'static,
{
    let Some(permit) = callback_permit(&deps) else {
        let _ = responder.respond_with_internal_error(callback_limit_error());
        return;
    };
    spawn_callback(deps.cancellation.clone(), permit, async move {
        match work(deps).await {
            Ok(response) => {
                let _ = responder.respond(response);
            }
            Err(error) => {
                tracing::warn!(error = %error, message = warn);
                let _ = responder.respond_with_internal_error(error);
            }
        }
    });
}

/// Bound an inbound ACP request that always replies with a typed success value.
///
/// Used by `RequestPermission`, which maps failures to `Cancelled` outcomes
/// instead of JSON-RPC internal errors.
pub(super) fn spawn_respond_callback<T, F, Fut>(deps: HandlerDeps, responder: Responder<T>, work: F)
where
    T: JsonRpcResponse,
    F: FnOnce(HandlerDeps) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T> + Send + 'static,
{
    let Some(permit) = callback_permit(&deps) else {
        let _ = responder.respond_with_internal_error(callback_limit_error());
        return;
    };
    spawn_callback(deps.cancellation.clone(), permit, async move {
        let response = work(deps).await;
        let _ = responder.respond(response);
    });
}

fn callback_limit_error() -> AppError {
    AppError::internal("ACP callback capacity exceeded")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::sync::Semaphore;
    use tokio_util::sync::CancellationToken;

    use super::{callback_limit_error, spawn_callback};

    #[test]
    fn callback_capacity_rejection_has_a_stable_error() {
        let slots = Arc::new(Semaphore::new(1));
        let _first = slots.clone().try_acquire_owned().expect("first slot");
        assert!(
            slots.clone().try_acquire_owned().is_err(),
            "a full callback semaphore must reject dispatch without waiting"
        );
        assert!(callback_limit_error()
            .to_string()
            .contains("ACP callback capacity exceeded"));
    }

    #[tokio::test]
    async fn callback_work_is_cancelled_when_the_session_closes() {
        let cancellation = CancellationToken::new();
        let completed = Arc::new(AtomicBool::new(false));
        let permit = Arc::new(Semaphore::new(1))
            .try_acquire_owned()
            .expect("callback slot");
        let work_completed = Arc::clone(&completed);
        spawn_callback(cancellation.clone(), permit, async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            work_completed.store(true, Ordering::Release);
        });

        cancellation.cancel();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !completed.load(Ordering::Acquire),
            "session cancellation must abort in-flight callback work"
        );
    }
}
