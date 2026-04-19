//! Axum HTTP server: REST snapshots + SSE event stream + embedded SPA.

use std::convert::Infallible;

use axum::Router;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header::AUTHORIZATION};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Json, Response};
use axum::routing::{get, post};
use futures::stream::{Stream, StreamExt};
use serde::Deserialize;
use tokio_stream::wrappers::BroadcastStream;

use crate::settings::RuntimeSettings;
use crate::state::{AppState, Command, PermissionDecision, ThreadSummary};

const INDEX_HTML: &str = include_str!("assets/index.html");

pub async fn run(state: AppState) {
    let settings = RuntimeSettings::from_env();
    state.broker().ensure_started();

    let mut app = Router::new()
        .route("/", get(index))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/:session_id/prompt", post(send_prompt))
        .route("/api/sessions/:session_id/cancel", post(cancel_session))
        .route("/api/sessions/:session_id/approve", post(approve_pending))
        .route("/api/events", get(events))
        .with_state(state);

    if let Some(token) = settings.auth_token.clone() {
        app = app.layer(from_fn_with_state(token, require_bearer));
    }

    let listener = match tokio::net::TcpListener::bind(settings.bind).await {
        Ok(listener) => listener,
        Err(error) => {
            log::error!("agent_http: bind {} failed: {error}", settings.bind);
            return;
        }
    };
    log::info!("agent_http: listening on http://{}", settings.bind);
    if let Err(error) = axum::serve(listener, app).await {
        log::error!("agent_http: serve loop exited: {error}");
    }
}

async fn require_bearer(State(token): State<String>, request: Request, next: Next) -> Response {
    let expected = format!("Bearer {token}");
    let provided = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v: &HeaderValue| v.to_str().ok());
    match provided {
        Some(value) if value == expected => next.run(request).await,
        _ => (StatusCode::UNAUTHORIZED, "unauthorized").into_response(),
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

#[derive(Deserialize)]
struct ApproveBody {
    decision: PermissionDecision,
}

async fn send_prompt(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<PromptBody>,
) -> impl IntoResponse {
    accept_or_error(state.dispatch(Command::SendPrompt {
        session_id,
        content: body.content,
    }))
}

async fn cancel_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    accept_or_error(state.dispatch(Command::CancelSession { session_id }))
}

async fn approve_pending(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(body): Json<ApproveBody>,
) -> impl IntoResponse {
    accept_or_error(state.dispatch(Command::AuthorizePendingTool {
        session_id,
        decision: body.decision,
    }))
}

fn accept_or_error<E: std::fmt::Display>(result: Result<(), E>) -> Response {
    match result {
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
