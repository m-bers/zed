use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::state::SnapshotEvent;

const CHANNEL_CAPACITY: usize = 1024;

/// Cross-thread fan-out for `SnapshotEvent`. The gpui main thread publishes;
/// the tokio thread (axum handlers) subscribes to drive SSE clients.
#[derive(Clone)]
pub struct Broker {
    sender: Arc<RwLock<Option<broadcast::Sender<SnapshotEvent>>>>,
}

impl Default for Broker {
    fn default() -> Self {
        Self {
            sender: Arc::new(RwLock::new(None)),
        }
    }
}

impl Broker {
    pub fn ensure_started(&self) -> broadcast::Sender<SnapshotEvent> {
        if let Some(existing) = self.sender.read().clone() {
            return existing;
        }
        let mut guard = self.sender.write();
        if let Some(existing) = guard.clone() {
            return existing;
        }
        let (sender, _initial_receiver_kept_alive_via_field) = broadcast::channel(CHANNEL_CAPACITY);
        *guard = Some(sender.clone());
        sender
    }

    pub fn publish(&self, event: SnapshotEvent) {
        let Some(sender) = self.sender.read().clone() else {
            return;
        };
        // SendError just means no SSE clients are subscribed right now —
        // that's the steady state when nobody has the page open.
        if let Err(_no_subscribers) = sender.send(event) {}
    }

    pub fn subscribe(&self) -> Option<broadcast::Receiver<SnapshotEvent>> {
        self.sender.read().as_ref().map(|s| s.subscribe())
    }
}
