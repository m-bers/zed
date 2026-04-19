use std::sync::Arc;

use agent_client_protocol as acp;
use collections::HashMap;
use gpui::{Global, Subscription};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;

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

#[derive(Clone, Default)]
pub struct AppState {
    inner: Arc<RwLock<Inner>>,
    broker: Broker,
}

#[derive(Default)]
struct Inner {
    threads: HashMap<acp::SessionId, ThreadSummary>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Inner::default())),
            broker: Broker::default(),
        }
    }

    pub fn broker(&self) -> &Broker {
        &self.broker
    }

    pub fn record_thread(&self, session_id: acp::SessionId, title: Option<String>) {
        let summary = ThreadSummary {
            session_id: session_id.to_string(),
            title: title.clone(),
        };
        self.inner.write().threads.insert(session_id, summary.clone());
        self.broker.publish(SnapshotEvent::ThreadDiscovered {
            session_id: summary.session_id,
            title: summary.title,
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
}

/// Global handle so any `App` can reach the shared state without each window
/// re-initialising. Owns the cross-thread `AppState` plus the subscription
/// reservoir (subscriptions live as long as this handle does).
#[derive(Clone)]
pub struct AppStateHandle {
    state: AppState,
    subscriptions: Arc<Mutex<Vec<Subscription>>>,
}

impl AppStateHandle {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            subscriptions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn subscriptions(&self) -> Arc<Mutex<Vec<Subscription>>> {
        self.subscriptions.clone()
    }
}

impl Global for AppStateHandle {}
