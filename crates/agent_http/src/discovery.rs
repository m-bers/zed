//! Workspace-aware discovery of `Entity<AcpThread>` instances.
//!
//! Gated behind the `workspace_discovery` feature to keep standalone agent_http
//! CI lightweight — `agent_ui` + `workspace` pull in most of Zed's UI graph.

use acp_thread::AcpThread;
use agent_ui::{AgentPanel, AgentPanelEvent, ConversationView};
use gpui::{App, Context, Entity};
use workspace::Workspace;

use crate::state::AppStateHandle;
use crate::subscriptions::observe_thread;

/// Register a global workspace observer that walks each workspace's
/// `AgentPanel` for retained `AcpThread`s and hands them to
/// [`crate::subscriptions::observe_thread`]. Call once, after [`crate::init`].
pub fn setup_workspace_observer(cx: &mut App) {
    cx.observe_new::<Workspace>(|workspace: &mut Workspace, _window, cx: &mut Context<Workspace>| {
        let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
            return;
        };

        walk_retained_threads(&panel, cx);

        cx.subscribe(&panel, |_workspace_self, panel, _event: &AgentPanelEvent, cx| {
            walk_retained_threads(&panel, cx);
        })
        .detach();
    })
    .detach();
}

fn walk_retained_threads(panel: &Entity<AgentPanel>, cx: &mut App) {
    // Collect `(thread, conversation_view)` pairs before touching the registries
    // so the `panel.read(cx)` borrow is released before each `observe_thread` call
    // (which needs `&mut App`).
    let pairs: Vec<(Entity<AcpThread>, Entity<ConversationView>)> = panel
        .read(cx)
        .retained_threads()
        .values()
        .filter_map(|cv: &Entity<ConversationView>| {
            let tv = cv.read(cx).root_thread_view()?;
            Some((tv.read(cx).thread.clone(), cv.clone()))
        })
        .collect();

    for (thread, cv) in pairs {
        register_conversation_view(&thread, &cv, cx);
        observe_thread(thread, cx);
    }
}

fn register_conversation_view(
    thread: &Entity<AcpThread>,
    conversation_view: &Entity<ConversationView>,
    cx: &mut App,
) {
    let Some(handle) = cx.try_global::<AppStateHandle>().cloned() else {
        return;
    };
    let session_id = thread.read(cx).session_id().clone();
    handle
        .conversation_registry()
        .borrow_mut()
        .register(session_id, conversation_view.downgrade());
}
