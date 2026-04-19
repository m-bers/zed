//! HTTP+SSE bridge for Zed's agent panel.
//!
//! Wires the agent panel's live state to a small HTTP server that serves a
//! mobile-first SPA and a Server-Sent-Events stream of every `AcpThreadEvent`
//! across every thread Zed knows about.
//!
//! Upstream Zed needs three additions to use this crate (all behind the
//! `agent_http` feature flag):
//!
//! * `Cargo.toml` (workspace) — add `agent_http = { path = "crates/agent_http" }`.
//! * `crates/zed/Cargo.toml` — `agent_http = { workspace = true, optional = true,
//!   features = ["workspace_discovery"] }` plus `agent_http = ["dep:agent_http"]`
//!   in `[features]`.
//! * `crates/zed/src/zed.rs` — `#[cfg(feature = "agent_http")]` block calling
//!   `agent_http::init(cx)` and `agent_http::setup_workspace_observer(cx)`
//!   inside the existing agent-panel init path.

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
pub use state::{AppState, AppStateHandle, Command, SnapshotEvent, ThreadSummary};
pub use subscriptions::observe_thread;

use std::thread;

use gpui::App;
use tokio::runtime::Builder;

/// Initialise the agent_http subsystem. Idempotent across multiple windows.
pub fn init(cx: &mut App) {
    if cx.try_global::<AppStateHandle>().is_some() {
        return;
    }
    let (state, command_rx) = AppState::new();
    cx.set_global(AppStateHandle::new(state.clone()));
    commands::spawn_worker(cx, command_rx);

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
                    return;
                }
            };
            runtime.block_on(server::run(state));
        })
        .ok();
}
