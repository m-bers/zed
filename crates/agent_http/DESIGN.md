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
  `m-bers/zed` can rebase on `zed-industries/zed` with ~10 lines of conflict
  surface.

## Non-goals

- Fleet orchestration (what helixml/zed built — we're single-user, many agents).
- External request/response correlation by `request_id` (we don't have a remote
  caller to correlate with — SSE just streams observed events).
- Replacing Zed's agent panel UI.

## Upstream patch surface

Target: ≤ ~10 lines across 3 files, all feature-gated on `agent_http`.

| File | Change |
|---|---|
| `Cargo.toml` (workspace) | Add `agent_http = { path = "crates/agent_http" }` to `[workspace.dependencies]` |
| `crates/zed/Cargo.toml` | Optional dep + `agent_http` feature |
| `crates/zed/src/zed.rs` | Feature-gated calls to `agent_http::init(cx)` and `agent_http::setup(workspace, cx)` inside `initialize_agent_panel` |

Whether any accessors need to be added to `AgentPanel` is TBD — see open
questions #1.

## Crate structure

All code in `crates/agent_http/`.

- `src/agent_http.rs` — lib root, public `init` / `setup` entry points, re-exports
- `src/settings.rs` — `AgentHttpSettings` (enabled/bind/port/token/cors)
- `src/state.rs` — `AppState` global: thread registry, snapshots, broker
- `src/broker.rs` — tokio `broadcast::channel<SnapshotEvent>` fan-out to SSE clients
- `src/subscriptions.rs` — `ensure_thread_subscription` ported from helix's
  `thread_service.rs`, translating `AcpThreadEvent` to `SnapshotEvent`
- `src/server.rs` — Axum HTTP server on `gpui_tokio` runtime; routes below
- `src/routes/` — REST + SSE handlers
- `src/assets.rs` — embedded SPA bundle (HTML/JS/CSS via `include_str!`)

## Subscription pattern (adapted from helix)

One `cx.subscribe(&acp_thread, …)` per `Entity<AcpThread>`, handling:

- `AcpThreadEvent::NewEntry` → emit `SnapshotEvent::EntryAdded { session_id,
  entry_index, role, kind, content }`. Entry kinds: `UserMessage`,
  `AssistantMessage`, `ToolCall` (with `tool_name`, `status`), `ToolCallUpdate`.
- `AcpThreadEvent::EntryUpdated(ix)` → emit `SnapshotEvent::EntryUpdated { … }`
  with accumulated content (throttled per-entry, TBD — see open q #2).
- `AcpThreadEvent::ToolAuthorizationRequested(id)` → add to inbox, emit event.
- `AcpThreadEvent::ToolAuthorizationReceived(id)` → clear inbox, emit.
- `AcpThreadEvent::Stopped(reason)` → flush any throttled content for the
  session (critical: helix learned this bug the hard way — missing flush =
  truncated final tokens + stuck "streaming" spinner).
- `AcpThreadEvent::TitleUpdated`, `TokenUsageUpdated`, `SubagentSpawned(id)` →
  route to corresponding UI sections.

Bugs we inherit fixes for, by reading helix's code:
- **Persistent-subscription guard**: double-subscribe leaks + duplicates events.
- **Stopped must flush**: otherwise last ~100ms of streamed content is lost.
- **Per-entry accumulation semantics**: Zed sends cumulative content per entry
  id (overwrite), not deltas. Client-side accumulator tracks last message id +
  byte offset.

Thread discovery mechanism — TBD (open q #1).

## HTTP/SSE protocol

REST (JSON):
- `GET /api/sessions` — list all active sessions with summary (title, status, counts)
- `GET /api/sessions/:id` — full snapshot of one session (entries, plan, token usage)
- `POST /api/sessions/:id/prompt` — send a prompt (bidirectional write path)
- `POST /api/sessions/:id/approvals/:tool_id` — resolve a tool authorization
- `POST /api/sessions` — spawn a new session in a given workspace + agent

SSE:
- `GET /api/events` — server-sent events stream of all `SnapshotEvent`s across all sessions
- `GET /api/sessions/:id/events` — SSE scoped to one session

All responses gzipped; SSE uses keep-alive pings every 15s.

Auth: if `auth_token` is set, require `Authorization: Bearer <token>` header.
Default bind is `127.0.0.1:9292`; users who want LAN access set `bind` + `token`.

## Web UI layout

Single-page, mobile-first. Three main views:

- **Tasks** — list of active sessions (status, workspace, agent, last activity)
- **Session detail** — message feed, tool call timeline, diffs, terminal output
- **Inbox** — cross-session pending approvals

Uses plain HTML + vanilla JS (no bundler): ~1 KB HTML, ~5 KB JS. Served as
embedded static assets from the crate.

## Headless mode

Zed's `--dev-server` mode launches a headless SSH-reachable process. The
agent_http server starts from the same init call regardless of UI state, so
when `dev-server` is active the HTTP port is bound and the web UI works without
any GUI being rendered. We don't add a new runtime mode — we just work inside
the one Zed already has.

## Open questions

1. **Thread discovery.** `AcpThread` instances are created inside
   `agent_ui::agent_panel` / `conversation_view` when the user or an external
   caller starts a session. There's no public "new thread" event on the panel
   or on `AgentConnectionStore`. Options:
   - Add a small `EventEmitter<ThreadSpawned>` on `AgentPanel` (~5 lines
     upstream)
   - Observe `Project` + `AgentServerStore` and heuristically walk for new
     threads (fragile)
   - Poll `history_store.sessions()` periodically (ugly but simple)
   First option is cleanest; counts toward the patch budget.

2. **Streaming throttle.** Helix uses 100 ms per-entry throttle to limit
   message_added events. For SSE, the network-side cost is lower but the
   client-side render cost remains — pick a throttle (50–100 ms range) and
   make it configurable.

3. **Multi-window coordination.** If Zed has multiple windows open, `init(cx)`
   may be called once per window if we're not careful. We bind the server at
   App-global scope (`cx.set_global(AppState)`) and make `init` idempotent so
   only the first caller starts the listener.

4. **Persistence.** Do we persist the event log to disk for history when Zed
   restarts, or accept that history is Zed's responsibility (via `history_store`)
   and we only show live state? Leaning toward the latter for v1.

5. **Auth model for bidirectional writes.** Token is enough for LAN use, but
   anyone on LAN with the token can send prompts. Acceptable? Or per-session
   ACLs? v1: single-token.

## Milestones

- **v0.1 — observe.** One-way: all `AcpThreadEvent`s → SSE. REST for
  snapshots. No write operations. Inbox for approvals but read-only display.
- **v0.2 — write.** `POST /prompt` and `POST /approvals`.
- **v0.3 — spawn.** `POST /sessions` to create new threads from web.
- **v0.4 — mobile polish.** SPA refinements, offline queueing, notifications.
