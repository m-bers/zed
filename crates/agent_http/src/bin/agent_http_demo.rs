//! Smoke-test binary: spins up the agent_http HTTP+SSE server with no Zed
//! integration, seeds a synthetic session and a few entries, then waits for
//! connections so you can curl the endpoints and confirm the wire format.
//!
//! Build with: `cargo build -p agent_http --features demo --bin agent_http_demo`

use std::time::Duration;

use agent_client_protocol as acp;
use agent_http::{AppState, SnapshotEvent, run_server};

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let (state, mut command_rx) = AppState::new();

    let demo_session_id = acp::SessionId::new("demo-session-0001".to_string());
    state.record_thread(demo_session_id.clone(), Some("Demo thread".into()));

    // Simulated streaming output so /api/events has something to broadcast.
    {
        let state = state.clone();
        let demo_id = demo_session_id.clone();
        tokio::spawn(async move {
            let mut tick: u32 = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(3)).await;
                tick += 1;
                state.publish(SnapshotEvent::EntryUpdated {
                    session_id: demo_id.to_string(),
                    entry_index: 0,
                    role: "assistant",
                    kind: "text",
                    content: format!("Demo tick {tick}"),
                });
            }
        });
    }

    // Log incoming commands so `POST /prompt`/`/cancel`/`/approve` can be
    // verified end to end even though no real AcpThread is wired up.
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            log::info!("demo received command: {command:?}");
        }
    });

    log::info!("starting agent_http_demo — press ctrl-c to stop");
    run_server(state).await;
}
