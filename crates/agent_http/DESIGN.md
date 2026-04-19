# agent_http: design

Web-UI agent manager for Zed, modeled on Google Antigravity's agent manager.
Hooks into Zed's agent panel and keeps one consolidated view across all open
workspaces, all ACP agents (Claude Agent SDK, Gemini CLI, Codex, Native Agent),
and the mobile browser.

## Goals

- **One web UI, all threads.** Task panel + artifacts panel + approvals inbox,
  mirroring Zed's agent panel state in real time.
- **Bidirectional.** Prompts sent from the web are visible in Zed's agent panel
  (and vice versa). Tool approvals can be resolved from either surface.
- **Mobile-first SPA** embedded in the crate; no separate frontend deploy.
- **Headless-capable.** Same binary works when Zed is run via `zed --dev-server`;
  web UI is the only surface required on that machine.
- **Minimal upstream patches.** Everything non-trivial lives in a new crate so
  `m-bers/zed` can rebase on `zed-industries/zed` with ~20 lines of conflict
  surface.

## Non-goals

- Fleet orchestration (what helixml/zed built — we're single-user, many agents).
- External request/response correlation by `request_id` (we don't have a remote
  caller to correlate with — SSE just streams observed events).
- Replacing Zed's agent panel UI.

## Upstream patch surface

Total ~20 lines across 4 files, all feature-gated on `agent_http`.

| File | Change | Lines |
|---|---|---|
| `Cargo.toml` (workspace) | `agent_http` member + path dep | 2 |
| `crates/zed/Cargo.toml` | Optional dep + `agent_http` feature with `workspace_discovery` | 2 |
| `crates/zed/src/zed.rs` | Cfg-gated `agent_http::init(cx)` + `setup_workspace_observer(cx)` inside `agent_ui::init`'s callsite | 5 |
| `crates/agent_ui/src/conversation_view.rs` | Add `pub fn resolve_pending_tool_call` on `ConversationView` so out-of-crate callers can resolve approvals without reaching into the private `Conversation` entity | ~12 |

Everything else lives in `crates/agent_http/`.

## Crate structure

All code in `crates/agent_http/`.

- `src/agent_http.rs` — lib root; public `init` / `setup_workspace_observer`, module decls.
- `src/settings.rs` — env-var-driven `RuntimeSettings` (`AGENT_HTTP_BIND`, `AGENT_HTTP_PORT`, `AGENT_HTTP_TOKEN`).
- `src/state.rs` — `AppState` (Send+Sync, held by tokio side) + `AppStateHandle` (main-thread only, holds `ThreadRegistry` / `ConversationViewRegistry` / subscription reservoir).
- `src/broker.rs` — tokio `broadcast::channel<SnapshotEvent>` fan-out to SSE clients.
- `src/subscriptions.rs` — `observe_thread`: registers a `cx.subscribe` per `Entity<AcpThread>`, translating `AcpThreadEvent` → `SnapshotEvent`.
- `src/discovery.rs` (gated: `workspace_discovery`) — `setup_workspace_observer`: walks every `Workspace`'s `AgentPanel.retained_threads`, calling `observe_thread` and populating the `ConversationView` registry.
- `src/commands.rs` — gpui-side worker consuming a `mpsc::UnboundedReceiver<Command>`, dispatching `SendPrompt` / `CancelSession` / `AuthorizePendingTool`.
- `src/server.rs` — axum HTTP server on a dedicated tokio runtime; REST + SSE.
- `src/assets/index.html` — mobile-first SPA (vanilla JS, <10 KB).
- `src/tests.rs` — unit tests for the non-gpui subsystems (broker, state serialization).

## Subscription pattern (adapted from helix)

One `cx.subscribe(&acp_thread, …)` per `Entity<AcpThread>`, handling:

- `AcpThreadEvent::NewEntry` → emit `SnapshotEvent::EntryAdded`.
- `AcpThreadEvent::EntryUpdated(ix)` → emit `SnapshotEvent::EntryUpdated` with accumulated content.
- `AcpThreadEvent::ToolAuthorizationRequested(id)` → inbox event; SPA shows approval buttons.
- `AcpThreadEvent::ToolAuthorizationReceived(id)` → clears the inbox entry.
- `AcpThreadEvent::Stopped(reason)` → emit `Stopped` event.
- `AcpThreadEvent::TitleUpdated` → emit `TitleChanged`.
- Other variants: ignored for v0.3.

Bugs we inherit fixes for by reading helix's code:
- **Persistent-subscription guard**: double-subscribe leaks + duplicates events. We deduplicate by checking `ThreadRegistry::lookup_by_string` before subscribing.

## HTTP/SSE protocol

REST (JSON):

| Method | Path | Purpose |
|---|---|---|
| GET | `/` | Embedded SPA |
| GET | `/api/sessions` | List all known threads (`ThreadSummary[]`) |
| GET | `/api/events` | SSE stream of `SnapshotEvent` |
| POST | `/api/sessions/:id/prompt` | Send a user message: `{"content": "…"}` |
| POST | `/api/sessions/:id/cancel` | Cancel the current running turn |
| POST | `/api/sessions/:id/approve` | Resolve a pending tool-call approval: `{"decision": "allow_once"\|"allow_always"\|"reject_once"\|"reject_always"}` |

Auth: optional bearer token via `AGENT_HTTP_TOKEN` env var. When set, every
request must include `Authorization: Bearer <token>` or receive 401.

## Runtime configuration

Environment variables read once at server startup:

| Variable | Default | Notes |
|---|---|---|
| `AGENT_HTTP_BIND` | `127.0.0.1` | `0.0.0.0` for LAN/phone access |
| `AGENT_HTTP_PORT` | `9292` | |
| `AGENT_HTTP_TOKEN` | (unset) | Enables bearer-auth middleware |

## Headless mode

Zed's `--dev-server` mode launches a headless SSH-reachable process. The
agent_http server starts from the same init call regardless of UI state, so
when `dev-server` is active the HTTP port is bound and the web UI works without
any GUI being rendered.

## Status

- **v0.1 — observe.** One-way AcpThreadEvent → SSE, REST snapshots. ✅
- **v0.2 — write.** `POST /prompt`. ✅
- **v0.2.1 — config.** Env vars for bind/port/token. ✅
- **v0.3 — full bidi.** `POST /cancel`, `POST /approve` (via new
  `ConversationView::resolve_pending_tool_call` upstream hook). Unit tests. ✅
- **v0.4 (future)** — migrate env vars to `settings::Settings`, markdown
  rendering in SPA, optional WebSocket transport, manual E2E verification.

## Open questions

1. **Streaming throttle.** Helix uses 100ms per-entry throttle. SSE cost is
   lower but the client-side render cost remains — revisit if perceived lag
   appears under heavy streaming.
2. **Multi-workspace coordination.** `setup_workspace_observer` re-walks all
   retained threads on every `AgentPanelEvent`, which is O(threads) per event.
   Fine for tens of threads; revisit above ~100.
3. **Session persistence.** Thread history is Zed's responsibility; we only
   surface live state. No change planned.
