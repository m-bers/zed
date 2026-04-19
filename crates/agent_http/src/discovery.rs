//! Workspace-aware discovery of `Entity<AcpThread>` instances.
//!
//! Gated behind the `workspace_discovery` feature to keep standalone agent_http
//! CI lightweight — `agent_ui` + `workspace` pull in most of Zed's UI graph.

use acp_thread::AcpThread;
use agent_ui::{AgentPanel, AgentPanelEvent, ConversationView};
use gpui::{App, Context, Entity};
use workspace::Workspace;

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
    let threads: Vec<Entity<AcpThread>> = panel
        .read(cx)
        .retained_threads()
        .values()
        .filter_map(|cv: &Entity<ConversationView>| {
            let tv = cv.read(cx).root_thread_view()?;
            Some(tv.read(cx).thread.clone())
        })
        .collect();
    for thread in threads {
        observe_thread(thread, cx);
    }
}
