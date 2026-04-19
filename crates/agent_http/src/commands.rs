//! Worker task that drains the `Command` channel on the gpui main thread,
//! looking up the target `Entity<AcpThread>` and dispatching via `AcpThread::send_raw`.
//!
//! Runs under `cx.spawn` so it shares the gpui foreground scheduler. The only
//! cross-thread step is the mpsc receive — everything after that touches gpui
//! types.

use anyhow::Result;
use gpui::App;
use tokio::sync::mpsc;

use crate::state::{AppStateHandle, Command};

pub fn spawn_worker(cx: &mut App, mut command_rx: mpsc::UnboundedReceiver<Command>) {
    cx.spawn(async move |cx| {
        while let Some(command) = command_rx.recv().await {
            if let Err(error) = cx.update(|cx| dispatch(command, cx)) {
                log::error!("agent_http: command dispatch failed: {error}");
            }
        }
    })
    .detach();
}

fn dispatch(command: Command, cx: &mut App) -> Result<()> {
    let handle = cx
        .try_global::<AppStateHandle>()
        .ok_or_else(|| anyhow::anyhow!("agent_http state not initialised"))?
        .clone();

    match command {
        Command::SendPrompt {
            session_id,
            content,
        } => send_prompt(&handle, &session_id, content, cx),
    }
}

fn send_prompt(
    handle: &AppStateHandle,
    session_id: &str,
    content: String,
    cx: &mut App,
) -> Result<()> {
    let Some(weak) = handle.registry().borrow().lookup_by_string(session_id) else {
        anyhow::bail!("unknown session_id {session_id}");
    };
    let thread = weak
        .upgrade()
        .ok_or_else(|| anyhow::anyhow!("session {session_id} no longer exists"))?;

    thread.update(cx, |thread, cx| {
        let future = thread.send(vec![content.into()], cx);
        cx.spawn(async move |_, _| {
            if let Err(error) = future.await {
                log::error!("agent_http: send failed: {error}");
            }
        })
        .detach();
    });
    Ok(())
}
