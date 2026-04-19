//! Translate `AcpThreadEvent` into broker `SnapshotEvent`s.
//!
//! `App::subscribe` returns a `Subscription` whose `Drop` deregisters the
//! callback. We park the subscriptions in `AppStateHandle::subscriptions`
//! (`Rc<RefCell<Vec<_>>>`, single-threaded since gpui is) so they live as long
//! as the app does. Subscriptions still self-cancel when the observed
//! `Entity<AcpThread>` is dropped.

use acp_thread::{AcpThread, AcpThreadEvent, AgentThreadEntry};
use gpui::{App, Entity};

use crate::state::{AppStateHandle, SnapshotEvent};

pub fn observe_thread(thread: Entity<AcpThread>, cx: &mut App) {
    let Some(handle) = cx.try_global::<AppStateHandle>().cloned() else {
        log::warn!("agent_http::observe_thread called before init()");
        return;
    };
    let state = handle.state().clone();
    let subscriptions = handle.subscriptions();

    let (session_id, title) = thread.read_with(cx, |thread, _| {
        (
            thread.session_id().clone(),
            thread.title().map(|t| t.to_string()),
        )
    });
    state.record_thread(session_id, title);

    let sub = cx.subscribe(&thread, move |thread, event, cx| {
        let session_id_str = thread.read_with(cx, |t, _| t.session_id().to_string());
        match event {
            AcpThreadEvent::NewEntry => {
                let payload = thread.read_with(cx, |t, cx| {
                    let entries = t.entries();
                    let idx = entries.len().saturating_sub(1);
                    entries.get(idx).map(|entry| {
                        let (role, kind) = describe_entry_kind(entry);
                        let content = entry.to_markdown(cx);
                        (idx, role, kind, content)
                    })
                });
                if let Some((idx, role, kind, content)) = payload {
                    state.publish(SnapshotEvent::EntryAdded {
                        session_id: session_id_str,
                        entry_index: idx,
                        role,
                        kind,
                        content,
                    });
                }
            }
            AcpThreadEvent::EntryUpdated(entry_idx) => {
                let idx = *entry_idx;
                let payload = thread.read_with(cx, |t, cx| {
                    t.entries().get(idx).map(|entry| {
                        let (role, kind) = describe_entry_kind(entry);
                        let content = entry.to_markdown(cx);
                        (role, kind, content)
                    })
                });
                if let Some((role, kind, content)) = payload {
                    state.publish(SnapshotEvent::EntryUpdated {
                        session_id: session_id_str,
                        entry_index: idx,
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
                let updated = thread.read_with(cx, |t, _| {
                    t.title().map(|s| (t.session_id().clone(), s.to_string()))
                });
                if let Some((id, title)) = updated {
                    state.update_title(&id, title);
                }
            }
            _ => {}
        }
    });

    subscriptions.borrow_mut().push(sub);
}

fn describe_entry_kind(entry: &AgentThreadEntry) -> (&'static str, &'static str) {
    match entry {
        AgentThreadEntry::UserMessage(_) => ("user", "text"),
        AgentThreadEntry::AssistantMessage(_) => ("assistant", "text"),
        AgentThreadEntry::ToolCall(_) => ("assistant", "tool_call"),
        AgentThreadEntry::CompletedPlan(_) => ("assistant", "plan"),
    }
}
