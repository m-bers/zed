//! Unit tests for the subsystems that don't require a running gpui App.

use crate::broker::Broker;
use crate::state::{PermissionDecision, SnapshotEvent};

#[test]
fn permission_decision_deserializes_from_snake_case() {
    let cases = [
        ("\"allow_once\"", PermissionDecision::AllowOnce),
        ("\"allow_always\"", PermissionDecision::AllowAlways),
        ("\"reject_once\"", PermissionDecision::RejectOnce),
        ("\"reject_always\"", PermissionDecision::RejectAlways),
    ];
    for (input, expected) in cases {
        let parsed: PermissionDecision = serde_json::from_str(input).expect(input);
        assert_eq!(
            std::mem::discriminant(&parsed),
            std::mem::discriminant(&expected),
            "input {input}"
        );
    }
}

#[test]
fn snapshot_event_serializes_with_type_tag() {
    let event = SnapshotEvent::EntryAdded {
        session_id: "sess-1".into(),
        entry_index: 3,
        role: "assistant",
        kind: "text",
        content: "hello".into(),
    };
    let json = serde_json::to_value(&event).expect("serialize");
    assert_eq!(json["type"], "entry_added");
    assert_eq!(json["session_id"], "sess-1");
    assert_eq!(json["entry_index"], 3);
}

#[tokio::test]
async fn broker_delivers_published_events_to_subscribers() {
    let broker = Broker::default();
    broker.ensure_started();
    let mut rx = broker.subscribe().expect("subscribe after ensure_started");
    broker.publish(SnapshotEvent::ThreadDiscovered {
        session_id: "s1".into(),
        title: Some("hi".into()),
    });
    let received = tokio::time::timeout(std::time::Duration::from_millis(250), rx.recv())
        .await
        .expect("deadline")
        .expect("channel alive");
    match received {
        SnapshotEvent::ThreadDiscovered { session_id, .. } => assert_eq!(session_id, "s1"),
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn broker_publish_without_subscribers_is_a_noop() {
    let broker = Broker::default();
    broker.ensure_started();
    // No explicit subscriber: the channel's initial receiver was dropped by
    // `ensure_started`, so this publish has no listeners.
    broker.publish(SnapshotEvent::Stopped {
        session_id: "s1".into(),
        reason: "done".into(),
    });
}
