//! HTTP+SSE bridge for Zed's agent panel.
//!
//! Wires the agent panel's live state to a small HTTP server that serves a
//! mobile-first SPA and a Server-Sent-Events stream of every `AcpThreadEvent`
//! across every thread Zed knows about.
//!
//! Upstream Zed needs three additions to use this crate:
//!
//! * `Cargo.toml` (workspace) — add `agent_http = { path = "crates/agent_http" }`.
//! * `crates/zed/Cargo.toml` —
//!   `agent_http = { workspace = true, features = ["workspace_discovery"] }`.
//! * `crates/zed/src/zed.rs` — `agent_http::init(cx);
//!   agent_http::setup_workspace_observer(cx);` inside the existing agent-panel
//!   init path.
//!
//! No Cargo feature flag is used — the HTTP server starts only when the
//! `AGENT_HTTP_BIND` environment variable is set, so unconfigured Zed runs see
//! zero overhead beyond a pair of small `cx.set_global` and `cx.observe_new`
//! registrations.

mod broker;
mod commands;
#[cfg(feature = "workspace_discovery")]
mod discovery;
mod server;
mod settings;
mod state;
mod subscriptions;

#[cfg(test)]
mod tests;

#[cfg(feature = "workspace_discovery")]
pub use discovery::setup_workspace_observer;
pub use server::run as run_server;
pub use state::{AppState, AppStateHandle, Command, SnapshotEvent, ThreadSummary};
pub use subscriptions::observe_thread;

use std::thread;

use gpui::App;
use tokio::runtime::Builder;

/// Initialise the agent_http subsystem. Idempotent across multiple windows.
///
/// The HTTP server only starts if the `AGENT_HTTP_BIND` environment variable is
/// set. Without it, this function still installs the global `AppStateHandle`
/// (so `observe_thread` and `setup_workspace_observer` can be called safely),
/// but no port is bound and no tokio runtime is spawned.
pub fn init(cx: &mut App) {
    if cx.try_global::<AppStateHandle>().is_some() {
        return;
    }
    let (state, command_rx) = AppState::new();
    cx.set_global(AppStateHandle::new(state.clone()));
    commands::spawn_worker(cx, command_rx);

    if std::env::var_os("AGENT_HTTP_BIND").is_none() {
        log::info!(
            "agent_http: AGENT_HTTP_BIND unset, HTTP server not starting (set AGENT_HTTP_BIND=0.0.0.0 or 127.0.0.1 to enable)"
        );
        return;
    }

    log::info!("agent_http: AGENT_HTTP_BIND set, starting HTTP server thread");
    eprintln!("[agent_http] starting HTTP server thread");

    thread::Builder::new()
        .name("agent-http".into())
        .spawn(move || {
            let runtime = match Builder::new_multi_thread()
                .enable_all()
                .thread_name("agent-http-worker")
                .build()
            {
                Ok(rt) => rt,
                Err(error) => {
                    log::error!("agent_http: failed to build tokio runtime: {error}");
                    eprintln!("[agent_http] failed to build tokio runtime: {error}");
                    return;
                }
            };
            runtime.block_on(server::run(state));
        })
        .ok();
}
