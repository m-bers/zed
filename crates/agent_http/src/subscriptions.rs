//! Translate `AcpThreadEvent` into broker `SnapshotEvent`s.
//!
//! `App::subscribe` returns a `Subscription` whose `Drop` deregisters the
//! callback. We park the subscriptions in `AppStateHandle::subscriptions`
//! (Arc<Mutex<Vec<_>>>) so they live as long as the app does. Subscriptions
//! still self-cancel when the observed `Entity<AcpThread>` is dropped.

use std::sync::Arc;

use acp_thread::{AcpThread, AcpThreadEvent, AgentThreadEntry};
use gpui::{App, Entity};
use parking_lot::Mutex;

use crate::state::{AppStateHandle, SnapshotEvent};

pub fn observe_thread(thread: Entity<AcpThread>, cx: &mut App) {
    let Some(handle) = cx.try_global::<AppStateHandle>().cloned() else {
        log::warn!("agent_http::observe_thread called before init()");
        return;
    };
    let state = handle.state().clone();
    let subscriptions: Arc<Mutex<Vec<gpui::Subscription>>> = handle.subscriptions();

    let session_id = thread.read(cx).session_id().clone();
    let title = thread.read(cx).title().map(|t| t.to_string());
    state.record_thread(session_id, title);

    let sub = cx.subscribe(&thread, move |thread, event, cx| {
        let session_id_str = thread.read(cx).session_id().to_string();
        match event {
            AcpThreadEvent::NewEntry => {
                let thread_ref = thread.read(cx);
                let entries = thread_ref.entries();
                let idx = entries.len().saturating_sub(1);
                if let Some(entry) = entries.get(idx) {
                    let (role, kind, content) = describe_entry(entry, cx);
                    state.publish(SnapshotEvent::EntryAdded {
                        session_id: session_id_str,
                        entry_index: idx,
                        role,
                        kind,
                        content,
                    });
                }
            }
            AcpThreadEvent::EntryUpdated(idx) => {
                let thread_ref = thread.read(cx);
                if let Some(entry) = thread_ref.entries().get(*idx) {
                    let (role, kind, content) = describe_entry(entry, cx);
                    state.publish(SnapshotEvent::EntryUpdated {
                        session_id: session_id_str,
                        entry_index: *idx,
                        role,
                        kind,
                        content,
                    });
                }
            }
            AcpThreadEvent::ToolAuthorizationRequested(tool_id) => {
                state.publish(SnapshotEvent::ToolAuthorizationRequested {
                    session_id: session_id_str,
                    tool_call_id: tool_id.to_string(),
                });
            }
            AcpThreadEvent::ToolAuthorizationReceived(tool_id) => {
                state.publish(SnapshotEvent::ToolAuthorizationReceived {
                    session_id: session_id_str,
                    tool_call_id: tool_id.to_string(),
                });
            }
            AcpThreadEvent::Stopped(reason) => {
                state.publish(SnapshotEvent::Stopped {
                    session_id: session_id_str,
                    reason: format!("{reason:?}"),
                });
            }
            AcpThreadEvent::TitleUpdated => {
                if let Some(t) = thread.read(cx).title() {
                    let id = thread.read(cx).session_id().clone();
                    state.update_title(&id, t.to_string());
                }
            }
            _ => {}
        }
    });

    subscriptions.lock().push(sub);
}

fn describe_entry(entry: &AgentThreadEntry, cx: &App) -> (&'static str, &'static str, String) {
    match entry {
        AgentThreadEntry::UserMessage(_) => ("user", "text", entry.to_markdown(cx)),
        AgentThreadEntry::AssistantMessage(_) => ("assistant", "text", entry.to_markdown(cx)),
        AgentThreadEntry::ToolCall(_) => ("assistant", "tool_call", entry.to_markdown(cx)),
        AgentThreadEntry::CompletedPlan(_) => ("assistant", "plan", entry.to_markdown(cx)),
    }
}
