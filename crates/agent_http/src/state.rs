use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use acp_thread::AcpThread;
use agent_client_protocol as acp;
use collections::HashMap;
use gpui::{Global, Subscription, WeakEntity};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::broker::Broker;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SnapshotEvent {
    ThreadDiscovered {
        session_id: String,
        title: Option<String>,
    },
    EntryAdded {
        session_id: String,
        entry_index: usize,
        role: &'static str,
        kind: &'static str,
        content: String,
    },
    EntryUpdated {
        session_id: String,
        entry_index: usize,
        role: &'static str,
        kind: &'static str,
        content: String,
    },
    ToolAuthorizationRequested {
        session_id: String,
        tool_call_id: String,
    },
    ToolAuthorizationReceived {
        session_id: String,
        tool_call_id: String,
    },
    Stopped {
        session_id: String,
        reason: String,
    },
    TitleChanged {
        session_id: String,
        title: String,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct ThreadSummary {
    pub session_id: String,
    pub title: Option<String>,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

impl PermissionDecision {
    pub fn as_kind(self) -> acp::PermissionOptionKind {
        match self {
            Self::AllowOnce => acp::PermissionOptionKind::AllowOnce,
            Self::AllowAlways => acp::PermissionOptionKind::AllowAlways,
            Self::RejectOnce => acp::PermissionOptionKind::RejectOnce,
            Self::RejectAlways => acp::PermissionOptionKind::RejectAlways,
        }
    }
}

/// Commands posted from the tokio-side HTTP handlers back to the gpui main
/// thread. A dedicated worker task in agent_http::init drains the receiver and
/// dispatches each command against the `ThreadRegistry` held by
/// `AppStateHandle`.
#[derive(Debug)]
pub enum Command {
    SendPrompt {
        session_id: String,
        content: String,
    },
    CancelSession {
        session_id: String,
    },
    AuthorizePendingTool {
        session_id: String,
        decision: PermissionDecision,
    },
}

/// Shared state safe to hand to the tokio runtime hosting the HTTP server.
///
/// Holds only `Send + Sync` data. Per-thread gpui handles (the entity registry
/// and the subscription reservoir) live in `AppStateHandle`, which is
/// gpui-main-thread only.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<Inner>>,
    broker: Broker,
    command_tx: mpsc::UnboundedSender<Command>,
}

#[derive(Default)]
struct Inner {
    threads: HashMap<acp::SessionId, ThreadSummary>,
}

impl AppState {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<Command>) {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        (
            Self {
                inner: Arc::new(RwLock::new(Inner::default())),
                broker: Broker::default(),
                command_tx,
            },
            command_rx,
        )
    }

    pub fn broker(&self) -> &Broker {
        &self.broker
    }

    pub fn record_thread(&self, session_id: acp::SessionId, title: Option<String>) {
        let session_id_str = session_id.to_string();
        self.inner.write().threads.insert(
            session_id,
            ThreadSummary {
                session_id: session_id_str.clone(),
                title: title.clone(),
            },
        );
        self.broker.publish(SnapshotEvent::ThreadDiscovered {
            session_id: session_id_str,
            title,
        });
    }

    pub fn update_title(&self, session_id: &acp::SessionId, title: String) {
        if let Some(handle) = self.inner.write().threads.get_mut(session_id) {
            handle.title = Some(title.clone());
        }
        self.broker.publish(SnapshotEvent::TitleChanged {
            session_id: session_id.to_string(),
            title,
        });
    }

    pub fn list_threads(&self) -> Vec<ThreadSummary> {
        self.inner.read().threads.values().cloned().collect()
    }

    pub fn publish(&self, event: SnapshotEvent) {
        self.broker.publish(event);
    }

    pub fn dispatch(&self, command: Command) -> Result<(), mpsc::error::SendError<Command>> {
        self.command_tx.send(command)
    }
}

/// Main-thread-only registry mapping session IDs to weak `AcpThread` handles.
#[derive(Default)]
pub struct ThreadRegistry {
    by_session: HashMap<acp::SessionId, WeakEntity<AcpThread>>,
}

impl ThreadRegistry {
    pub fn register(&mut self, session_id: acp::SessionId, handle: WeakEntity<AcpThread>) {
        self.by_session.insert(session_id, handle);
    }

    pub fn lookup_by_string(&self, session_id_str: &str) -> Option<WeakEntity<AcpThread>> {
        self.by_session
            .iter()
            .find(|(id, _)| id.to_string() == session_id_str)
            .map(|(_, h)| h.clone())
    }
}

/// Main-thread-only registry mapping session IDs to weak `ConversationView`
/// handles. Only populated when the `workspace_discovery` feature is active.
#[cfg(feature = "workspace_discovery")]
#[derive(Default)]
pub struct ConversationViewRegistry {
    by_session: HashMap<acp::SessionId, WeakEntity<agent_ui::ConversationView>>,
}

#[cfg(feature = "workspace_discovery")]
impl ConversationViewRegistry {
    pub fn register(
        &mut self,
        session_id: acp::SessionId,
        handle: WeakEntity<agent_ui::ConversationView>,
    ) {
        self.by_session.insert(session_id, handle);
    }

    pub fn lookup_by_string(
        &self,
        session_id_str: &str,
    ) -> Option<WeakEntity<agent_ui::ConversationView>> {
        self.by_session
            .iter()
            .find(|(id, _)| id.to_string() == session_id_str)
            .map(|(_, h)| h.clone())
    }
}

/// Global handle so any `App` can reach the shared state without each window
/// re-initialising. Owns the cross-thread `AppState`, the thread-entity
/// registry, and the gpui subscription reservoir.
#[derive(Clone)]
pub struct AppStateHandle {
    state: AppState,
    registry: Rc<RefCell<ThreadRegistry>>,
    #[cfg(feature = "workspace_discovery")]
    conversation_registry: Rc<RefCell<ConversationViewRegistry>>,
    subscriptions: Rc<RefCell<Vec<Subscription>>>,
}

impl AppStateHandle {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            registry: Rc::new(RefCell::new(ThreadRegistry::default())),
            #[cfg(feature = "workspace_discovery")]
            conversation_registry: Rc::new(RefCell::new(ConversationViewRegistry::default())),
            subscriptions: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn registry(&self) -> Rc<RefCell<ThreadRegistry>> {
        self.registry.clone()
    }

    #[cfg(feature = "workspace_discovery")]
    pub fn conversation_registry(&self) -> Rc<RefCell<ConversationViewRegistry>> {
        self.conversation_registry.clone()
    }

    pub fn subscriptions(&self) -> Rc<RefCell<Vec<Subscription>>> {
        self.subscriptions.clone()
    }
}

impl Global for AppStateHandle {}
