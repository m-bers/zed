# agent_http

Web-UI agent manager for Zed, modeled on Google Antigravity's agent manager.
Embeds a mobile-first single-page app and serves it alongside a live HTTP+SSE
view of every `AcpThread` across every open workspace — plus a `POST /prompt`
endpoint so you can send messages from a browser (phone included).

## Architecture in one page

```
┌──────────────────┐   Entity<AcpThread> events         ┌───────────────────┐
│ Zed main thread  │ ─────────────────────────────────→ │ broker (tokio)    │
│                  │      cx.subscribe(&thread, …)      │ broadcast channel │
│ workspace_observer                                    │                   │
│  → observe_thread                                     └────────┬──────────┘
│                                                                │
│                                                                │ SSE
│                                                                ▼
│                                                       ┌───────────────────┐
│                                          HTTP POST    │ axum HTTP server  │
│ commands::worker  ◄─── mpsc Command ───────────────── │  + embedded SPA   │
│   AcpThread::send                                     └───────────────────┘
└──────────────────┘
```

`AppState` (Send+Sync, clone to the tokio thread) holds the broker and a
summary of discovered threads. `AppStateHandle` (main-thread only) adds the
`WeakEntity<AcpThread>` registry and the subscription reservoir.

## Enabling the integration

All integration goes through a single Zed feature flag — the crate is inert
until you opt in:

```toml
# Cargo.toml (workspace)
agent_http = { path = "crates/agent_http" }
```

```toml
# crates/zed/Cargo.toml
[features]
agent_http = ["dep:agent_http"]

[dependencies]
agent_http = { workspace = true, optional = true, features = ["workspace_discovery"] }
```

```rust
// crates/zed/src/zed.rs, inside initialize_agent_panel's init block
#[cfg(feature = "agent_http")]
{
    agent_http::init(cx);
    agent_http::setup_workspace_observer(cx);
}
```

Total upstream surface: ~9 lines.

## Build

```sh
# standalone (fast, no UI deps)
cargo check -p agent_http

# full integration (~5 min on warm cache)
cargo check -p zed --features agent_http
```

Both are verified in CI via `.github/workflows/agent-http-check.yml` and
`.github/workflows/agent-http-integration.yml`.

## Runtime configuration

Environment variables read once at server start:

| Variable | Default | Notes |
|---|---|---|
| `AGENT_HTTP_BIND` | `127.0.0.1` | Set to `0.0.0.0` for LAN/phone access |
| `AGENT_HTTP_PORT` | `9292` | |
| `AGENT_HTTP_TOKEN` | (unset) | When set, every request must carry `Authorization: Bearer <token>` |

## HTTP API

| Method | Path | Purpose |
|---|---|---|
| GET | `/` | Embedded SPA |
| GET | `/api/sessions` | List all known threads (`ThreadSummary[]`) |
| GET | `/api/events` | SSE stream of `SnapshotEvent` |
| POST | `/api/sessions/:id/prompt` | Send a user message: `{"content": "…"}` |

### SnapshotEvent variants

`thread_discovered` · `title_changed` · `entry_added` · `entry_updated` ·
`tool_authorization_requested` · `tool_authorization_received` · `stopped`

## Status

- v0.1 — observe-only ✅
- v0.2 — bidirectional writes (prompt) ✅
- v0.2.1 — env-var config (bind/port/token) ✅
- v0.3 — tool approval flow, Zed settings integration, tests

See `DESIGN.md` for the full architecture and roadmap.
