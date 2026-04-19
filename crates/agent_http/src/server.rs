//! Axum HTTP server: REST snapshots + SSE event stream + embedded SPA.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::{get, post};
use futures::stream::{Stream, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;

use crate::state::{AppState, Command, ThreadSummary};

const DEFAULT_PORT: u16 = 9292;
const INDEX_HTML: &str = include_str!("assets/index.html");

pub async fn run(state: AppState) {
    state.broker().ensure_started();

    let bind = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_PORT);
    let app = Router::new()
        .route("/", get(index))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/:session_id/prompt", post(send_prompt))
        .route("/api/events", get(events))
        .with_state(state);

    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("agent_http: bind {bind} failed: {error}");
            return;
        }
    };
    log::info!("agent_http: listening on http://{bind}");
    if let Err(error) = axum::serve(listener, app).await {
        log::error!("agent_http: serve loop exited: {error}");
    }
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn list_sessions(State(state): State<AppState>) -> Json<Vec<ThreadSummary>> {
    Json(state.list_threads())
}

#[derive(Deserialize)]
struct PromptBody {
    content: String,
}

async fn send_prompt(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<PromptBody>,
) -> impl IntoResponse {
    match state.dispatch(Command::SendPrompt {
        session_id,
        content: body.content,
    }) {
        Ok(()) => (StatusCode::ACCEPTED, "accepted").into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("dispatch failed: {error}"),
        )
            .into_response(),
    }
}

async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.broker().ensure_started().subscribe();
    let stream = BroadcastStream::new(receiver).filter_map(|item| async move {
        let event = item.ok()?;
        let json = serde_json::to_string(&event).ok()?;
        Some(Ok(Event::default().event("snapshot").data(json)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
